use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

#[derive(Debug)]
pub enum PreparedOutput {
    Json(Value),
    Text(String),
}

pub fn prepare_tool_output(
    result: Value,
    text: bool,
    output: Option<&Path>,
    force: bool,
) -> Result<PreparedOutput> {
    if text && output.is_some() {
        bail!("--text and --output cannot be used together");
    }

    if let Some(output) = output {
        let written = write_image(&result, output, force)?;
        return Ok(PreparedOutput::Json(json!({
            "path": written.path,
            "mimeType": written.mime_type,
            "bytes": written.bytes
        })));
    }

    if text {
        return Ok(PreparedOutput::Text(extract_text(&result)?));
    }

    Ok(PreparedOutput::Json(result))
}

pub fn file_names_text(result: &Value) -> Result<String> {
    if let Some(files) = result
        .get("structuredContent")
        .and_then(|content| content.get("files"))
        .and_then(Value::as_array)
    {
        return render_file_names(files);
    }

    let content = content_array(result)?;
    for item in content {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if let Some(files) = payload.get("files").and_then(Value::as_array) {
            return render_file_names(files);
        }
    }

    bail!("list_files response does not contain a files array")
}

struct WrittenImage {
    path: String,
    mime_type: String,
    bytes: usize,
}

struct EncodedImage<'a> {
    data: &'a str,
    mime_type: &'a str,
}

fn write_image(result: &Value, requested_path: &Path, force: bool) -> Result<WrittenImage> {
    let image = extract_single_image(result)?;
    let mime_type = normalize_mime_type(image.mime_type);
    let output_path = resolve_output_path(requested_path, &mime_type)?;
    let bytes = STANDARD
        .decode(image.data)
        .context("Paper returned invalid base64 image data")?;

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output directory {}",
                parent.to_string_lossy()
            )
        })?;
    }

    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    let mut file = match options.open(&output_path) {
        Ok(file) => file,
        Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
            bail!(
                "output file {} already exists; pass --force to overwrite it",
                output_path.display()
            )
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to open output file {}", output_path.display()));
        }
    };
    file.write_all(&bytes)
        .with_context(|| format!("failed to write image to {}", output_path.display()))?;

    Ok(WrittenImage {
        path: output_path.to_string_lossy().into_owned(),
        mime_type,
        bytes: bytes.len(),
    })
}

fn extract_single_image(result: &Value) -> Result<EncodedImage<'_>> {
    let content = content_array(result)?;
    let image_items = content
        .iter()
        .filter(|item| {
            item.get("type").and_then(Value::as_str) == Some("image")
                || embedded_resource_mime(item).is_some_and(is_image_mime)
        })
        .collect::<Vec<_>>();

    match image_items.len() {
        0 => bail!(
            "tool response contains no writable image; omit --output to inspect structured JSON"
        ),
        1 => {}
        count => bail!(
            "tool response contains {count} images; --output requires exactly one writable image"
        ),
    }

    let item = image_items[0];
    if item.get("type").and_then(Value::as_str) == Some("image") {
        let data = item
            .get("data")
            .and_then(Value::as_str)
            .context("image content is missing base64 data")?;
        let mime_type = item
            .get("mimeType")
            .and_then(Value::as_str)
            .context("image content is missing a MIME type")?;
        if !is_image_mime(mime_type) {
            bail!("image content has incompatible MIME type `{mime_type}`");
        }
        return Ok(EncodedImage { data, mime_type });
    }

    let resource = item
        .get("resource")
        .and_then(Value::as_object)
        .context("embedded image resource is malformed")?;
    let data = resource
        .get("blob")
        .and_then(Value::as_str)
        .context("embedded image resource is missing base64 data")?;
    let mime_type = resource
        .get("mimeType")
        .and_then(Value::as_str)
        .context("embedded image resource is missing a MIME type")?;
    Ok(EncodedImage { data, mime_type })
}

fn extract_text(result: &Value) -> Result<String> {
    let content = content_array(result)?;
    let non_text_types = content
        .iter()
        .filter_map(|item| {
            let content_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            (content_type != "text").then_some(content_type)
        })
        .collect::<Vec<_>>();

    if !non_text_types.is_empty() {
        let kinds = non_text_types.join(", ");
        bail!(
            "tool returned non-text MCP content ({kinds}); omit --text for structured JSON or use --output <path> to write a single image"
        );
    }

    let mut output = String::new();
    for item in content {
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .context("Paper returned text content without a text value")?;
        output.push_str(text);
        if !text.ends_with('\n') {
            output.push('\n');
        }
    }

    if output.is_empty() {
        bail!("tool returned no text content; omit --text to see the full JSON");
    }
    Ok(output)
}

fn content_array(result: &Value) -> Result<&Vec<Value>> {
    result
        .get("content")
        .and_then(Value::as_array)
        .context("tool result has no MCP content array; omit output options to see the full JSON")
}

fn embedded_resource_mime(item: &Value) -> Option<&str> {
    if item.get("type").and_then(Value::as_str) != Some("resource") {
        return None;
    }
    item.get("resource")?.get("mimeType")?.as_str()
}

fn is_image_mime(mime_type: &str) -> bool {
    normalize_mime_type(mime_type).starts_with("image/")
}

fn normalize_mime_type(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase()
}

fn resolve_output_path(requested_path: &Path, mime_type: &str) -> Result<PathBuf> {
    let (preferred, compatible): (&str, &[&str]) = match mime_type {
        "image/jpeg" | "image/jpg" => ("jpg", &["jpg", "jpeg"]),
        "image/png" => ("png", &["png"]),
        "image/webp" => ("webp", &["webp"]),
        "image/gif" => ("gif", &["gif"]),
        "image/svg+xml" => ("svg", &["svg"]),
        "image/avif" => ("avif", &["avif"]),
        "image/tiff" => ("tiff", &["tif", "tiff"]),
        "image/bmp" => ("bmp", &["bmp"]),
        "image/x-icon" | "image/vnd.microsoft.icon" => ("ico", &["ico"]),
        _ => bail!("unsupported image MIME type `{mime_type}`"),
    };

    let Some(extension) = requested_path.extension() else {
        return Ok(requested_path.with_extension(preferred));
    };
    let extension = extension
        .to_str()
        .context("output file extension is not valid UTF-8")?
        .to_ascii_lowercase();
    if !compatible.contains(&extension.as_str()) {
        bail!(
            "output extension .{extension} is incompatible with returned MIME type `{mime_type}`"
        );
    }

    Ok(requested_path.to_owned())
}

fn render_file_names(files: &[Value]) -> Result<String> {
    let mut output = String::new();
    for file in files {
        let name = file
            .get("name")
            .and_then(Value::as_str)
            .context("list_files returned a file without a name")?;
        output.push_str(name);
        output.push('\n');
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    const JPEG_DATA: &str = "/9j/2Q==";
    const PNG_DATA: &str = "iVBORw0KGgo=";

    fn image_result(mime_type: &str, data: &str) -> Value {
        json!({
            "content": [{
                "type": "image",
                "mimeType": mime_type,
                "data": data
            }]
        })
    }

    #[test]
    fn writes_jpeg_image_to_nested_directory() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("captures/shot.jpg");
        let prepared = prepare_tool_output(
            image_result("image/jpeg", JPEG_DATA),
            false,
            Some(&path),
            false,
        )
        .unwrap();

        assert_eq!(fs::read(&path).unwrap(), [0xff, 0xd8, 0xff, 0xd9]);
        let PreparedOutput::Json(metadata) = prepared else {
            panic!("expected JSON metadata");
        };
        assert_eq!(metadata["path"], path.to_string_lossy().as_ref());
        assert_eq!(metadata["mimeType"], "image/jpeg");
        assert_eq!(metadata["bytes"], 4);
    }

    #[test]
    fn writes_png_image_and_infers_extension() {
        let directory = tempdir().unwrap();
        let requested = directory.path().join("shot");
        let actual = directory.path().join("shot.png");
        prepare_tool_output(
            image_result("image/png", PNG_DATA),
            false,
            Some(&requested),
            false,
        )
        .unwrap();

        assert_eq!(
            fs::read(actual).unwrap(),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shot.jpg");
        fs::write(&path, b"existing").unwrap();

        let error = prepare_tool_output(
            image_result("image/jpeg", JPEG_DATA),
            false,
            Some(&path),
            false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert_eq!(fs::read(path).unwrap(), b"existing");
    }

    #[test]
    fn overwrites_with_force() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("shot.jpg");
        fs::write(&path, b"existing").unwrap();

        prepare_tool_output(
            image_result("image/jpeg", JPEG_DATA),
            false,
            Some(&path),
            true,
        )
        .unwrap();

        assert_eq!(fs::read(path).unwrap(), [0xff, 0xd8, 0xff, 0xd9]);
    }

    #[test]
    fn rejects_text_for_image_content() {
        let error = prepare_tool_output(image_result("image/png", PNG_DATA), true, None, false)
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("non-text MCP content (image)"));
        assert!(message.contains("--output <path>"));
    }

    #[test]
    fn rejects_text_for_embedded_resource_content() {
        let error = prepare_tool_output(
            json!({
                "content": [{
                    "type": "resource",
                    "resource": {
                        "uri": "paper://image",
                        "mimeType": "image/png",
                        "blob": PNG_DATA
                    }
                }]
            }),
            true,
            None,
            false,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("non-text MCP content (resource)")
        );
    }

    #[test]
    fn rejects_output_when_no_image_exists() {
        let directory = tempdir().unwrap();
        let error = prepare_tool_output(
            json!({"content": [{"type": "text", "text": "hello"}]}),
            false,
            Some(&directory.path().join("shot.png")),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no writable image"));
    }

    #[test]
    fn rejects_ambiguous_multiple_images() {
        let directory = tempdir().unwrap();
        let error = prepare_tool_output(
            json!({
                "content": [
                    {"type": "image", "mimeType": "image/png", "data": PNG_DATA},
                    {"type": "image", "mimeType": "image/jpeg", "data": JPEG_DATA}
                ]
            }),
            false,
            Some(&directory.path().join("shot.png")),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("2 images"));
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn preserves_json_without_output_options() {
        let result = image_result("image/png", PNG_DATA);
        let PreparedOutput::Json(actual) =
            prepare_tool_output(result.clone(), false, None, false).unwrap()
        else {
            panic!("expected JSON output");
        };
        assert_eq!(actual, result);
    }

    #[test]
    fn rejects_incompatible_extension() {
        let directory = tempdir().unwrap();
        let error = prepare_tool_output(
            image_result("image/png", PNG_DATA),
            false,
            Some(&directory.path().join("shot.jpg")),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("incompatible"));
    }

    #[test]
    fn file_names_are_rendered_one_per_line() {
        let result = json!({
            "content": [{
                "type": "text",
                "text": "{\"files\":[{\"id\":\"1\",\"name\":\"First file\"},{\"id\":\"2\",\"name\":\"Second file\"}]}"
            }]
        });
        assert_eq!(
            file_names_text(&result).unwrap(),
            "First file\nSecond file\n"
        );
    }
}
