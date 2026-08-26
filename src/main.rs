mod context;
mod mcp;
mod output;

use std::{
    fs,
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser, Subcommand};
use context::{ScreenshotTarget, capture_screenshot, read_context};
use mcp::McpClient;
use output::{PreparedOutput, file_names_text, prepare_tool_output};
use serde_json::{Map, Value, json};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:29979/mcp";

/// Use every tool exposed by Paper Desktop from a terminal or coding agent.
#[derive(Debug, Parser)]
#[command(name = "paper", version, about)]
struct Cli {
    /// Paper Desktop's Streamable HTTP MCP endpoint.
    #[arg(
        long,
        global = true,
        env = "PAPER_MCP_URL",
        default_value = DEFAULT_ENDPOINT
    )]
    url: String,

    /// Per-request timeout in seconds.
    #[arg(long, global = true, default_value_t = 120)]
    timeout: u64,

    /// Print JSON on one line instead of formatting it.
    #[arg(long, global = true)]
    compact: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect and print Paper's server information and capabilities.
    Status {
        /// Print only connection and protocol details.
        #[arg(long)]
        short: bool,
    },

    /// List every tool currently exposed by Paper.
    Tools {
        /// Print only tool names, one per line.
        #[arg(long)]
        names: bool,
    },

    /// Print the live JSON schema for one Paper tool.
    Schema {
        /// Tool name, as shown by `paper tools`.
        name: String,
    },

    /// Call any Paper tool.
    Call {
        /// Tool name, as shown by `paper tools`.
        name: String,

        /// JSON object, @path to a JSON file, or - to read JSON from stdin.
        #[arg(value_name = "ARGUMENTS")]
        arguments: Option<String>,

        /// Print only text content returned by the tool.
        #[arg(long, conflicts_with = "output")]
        text: bool,

        /// Decode exactly one returned image to this file.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,

        /// Overwrite an existing output file.
        #[arg(long, requires = "output")]
        force: bool,
    },

    /// Capture a Paper node directly to an image file.
    #[command(group(
        ArgGroup::new("screenshot_target")
            .required(true)
            .multiple(false)
            .args(["node_id", "selected", "active_artboard", "artboard"])
    ))]
    Screenshot {
        /// ID of the node to capture.
        node_id: Option<String>,

        /// Capture the single currently selected node.
        #[arg(long)]
        selected: bool,

        /// Capture the selected node's artboard, or the page's only artboard.
        #[arg(long)]
        active_artboard: bool,

        /// Capture an artboard whose full name matches exactly.
        #[arg(long, value_name = "EXACT_NAME")]
        artboard: Option<String>,

        /// Destination image path.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,

        /// Paper render scale, commonly 1 or 2.
        #[arg(long)]
        scale: Option<f64>,

        /// Paper file ID when more than one file is open.
        #[arg(long)]
        file_id: Option<String>,

        /// Overwrite an existing output file.
        #[arg(long)]
        force: bool,
    },

    /// Print the current Paper file, page, artboards, and selection.
    Context {
        /// Omit raw MCP results and raw node IDs.
        #[arg(long)]
        short: bool,

        /// Paper file ID when more than one file is open.
        #[arg(long)]
        file_id: Option<String>,
    },

    /// List open and recently accessed Paper files.
    Files {
        /// Maximum number of files to return.
        #[arg(
            long,
            default_value_t = 50,
            value_parser = clap::value_parser!(u16).range(1..=200)
        )]
        limit: u16,

        /// Print only file names, one per line.
        #[arg(long)]
        names: bool,
    },

    /// Open a Paper file by ID or URL.
    Open {
        /// Paper file ID, route, or URL.
        file_id_or_url: String,

        /// Page to switch to after opening the file.
        #[arg(long)]
        page_id: Option<String>,
    },

    /// Send any MCP JSON-RPC request after initialization.
    Request {
        /// MCP method, such as prompts/list or resources/list.
        method: String,

        /// JSON value, @path to a JSON file, or - to read JSON from stdin.
        #[arg(value_name = "PARAMS")]
        params: Option<String>,
    },

    /// Send an MCP JSON-RPC notification after initialization.
    Notify {
        /// MCP notification method.
        method: String,

        /// JSON value, @path to a JSON file, or - to read JSON from stdin.
        #[arg(value_name = "PARAMS")]
        params: Option<String>,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let timeout = Duration::from_secs(cli.timeout);
    let short_status = matches!(&cli.command, Command::Status { short: true });
    let connection = McpClient::connect(&cli.url, timeout);
    let (mut client, initialize_result) = match connection {
        Ok(connection) => connection,
        Err(error) if short_status => {
            print_json(&status_output(&cli.url, None, true), cli.compact)?;
            return Err(error).context("failed to connect to Paper Desktop");
        }
        Err(error) => return Err(error).context("failed to connect to Paper Desktop"),
    };

    let output = match cli.command {
        Command::Status { short } => Some(status_output(&cli.url, Some(&initialize_result), short)),
        Command::Tools { names } => {
            let tools = client.list_tools()?;
            if names {
                for tool in tools {
                    let name = tool
                        .get("name")
                        .and_then(Value::as_str)
                        .context("Paper returned a tool without a name")?;
                    println!("{name}");
                }
                None
            } else {
                Some(json!({ "tools": tools }))
            }
        }
        Command::Schema { name } => {
            let schema = client
                .list_tools()?
                .into_iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name.as_str()))
                .with_context(|| format!("Paper does not expose a tool named `{name}`"))?;
            Some(schema)
        }
        Command::Call {
            name,
            arguments,
            text,
            output,
            force,
        } => {
            let arguments = read_json(arguments.as_deref(), json!({}))?;
            if !arguments.is_object() {
                bail!("tool arguments must be a JSON object");
            }

            let result = client.call_tool(&name, arguments)?;
            emit_prepared_output(prepare_tool_output(result, text, output.as_deref(), force)?)
        }
        Command::Screenshot {
            node_id,
            selected,
            active_artboard,
            artboard,
            output,
            scale,
            file_id,
            force,
        } => {
            let target = match (node_id, selected, active_artboard, artboard) {
                (Some(node_id), false, false, None) => ScreenshotTarget::NodeId(node_id),
                (None, true, false, None) => ScreenshotTarget::Selected,
                (None, false, true, None) => ScreenshotTarget::ActiveArtboard,
                (None, false, false, Some(name)) => ScreenshotTarget::Artboard(name),
                _ => unreachable!("Clap enforces exactly one screenshot target"),
            };
            let result = capture_screenshot(&mut client, target, file_id.as_deref(), scale)?;
            emit_prepared_output(prepare_tool_output(result, false, Some(&output), force)?)
        }
        Command::Context { short, file_id } => {
            Some(read_context(&mut client, file_id.as_deref())?.output(short))
        }
        Command::Files { limit, names } => {
            let result = client.call_tool("list_files", json!({ "limit": limit }))?;
            if names {
                print!("{}", file_names_text(&result)?);
                None
            } else {
                Some(result)
            }
        }
        Command::Open {
            file_id_or_url,
            page_id,
        } => {
            let mut arguments = Map::new();
            arguments.insert("fileId".into(), Value::String(file_id_or_url));
            if let Some(page_id) = page_id {
                arguments.insert("pageId".into(), Value::String(page_id));
            }
            Some(client.call_tool("open_file", Value::Object(arguments))?)
        }
        Command::Request { method, params } => {
            let params = params
                .as_deref()
                .map(|value| read_json(Some(value), Value::Null))
                .transpose()?;
            Some(client.request(&method, params)?)
        }
        Command::Notify { method, params } => {
            let params = params
                .as_deref()
                .map(|value| read_json(Some(value), Value::Null))
                .transpose()?;
            client.notify(&method, params)?;
            Some(json!({ "accepted": true }))
        }
    };

    if let Some(output) = output {
        print_json(&output, cli.compact)?;
    }

    client.close();
    Ok(())
}

fn read_json(source: Option<&str>, default: Value) -> Result<Value> {
    let Some(source) = source else {
        if !io::stdin().is_terminal() {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .context("failed to read JSON from stdin")?;
            if !input.trim().is_empty() {
                return parse_json(&input, "stdin");
            }
        }
        return Ok(default);
    };

    if source == "-" {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .context("failed to read JSON from stdin")?;
        return parse_json(&input, "stdin");
    }

    if let Some(path) = source.strip_prefix('@') {
        if path.is_empty() {
            bail!("expected a file path after @");
        }
        let input = fs::read_to_string(path)
            .with_context(|| format!("failed to read JSON from {}", Path::new(path).display()))?;
        return parse_json(&input, path);
    }

    parse_json(source, "command line")
}

fn parse_json(input: &str, source: &str) -> Result<Value> {
    serde_json::from_str(input).with_context(|| format!("invalid JSON from {source}"))
}

fn print_json(value: &Value, compact: bool) -> Result<()> {
    let rendered = if compact {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    }
    .context("failed to serialize Paper response")?;
    println!("{rendered}");
    Ok(())
}

fn emit_prepared_output(output: PreparedOutput) -> Option<Value> {
    match output {
        PreparedOutput::Json(value) => Some(value),
        PreparedOutput::Text(text) => {
            print!("{text}");
            None
        }
    }
}

fn status_output(endpoint: &str, initialize_result: Option<&Value>, short: bool) -> Value {
    if short {
        let server_info = initialize_result.and_then(|result| result.get("serverInfo"));
        return json!({
            "connected": initialize_result.is_some(),
            "endpoint": endpoint,
            "server": {
                "name": server_info.and_then(|server| server.get("name")).cloned(),
                "version": server_info.and_then(|server| server.get("version")).cloned()
            },
            "protocolVersion": initialize_result
                .and_then(|result| result.get("protocolVersion"))
                .cloned()
        });
    }

    let mut status = Map::new();
    status.insert("endpoint".into(), Value::String(endpoint.to_owned()));
    status.insert("connected".into(), Value::Bool(true));
    if let Some(object) = initialize_result.and_then(Value::as_object) {
        status.extend(object.clone());
    } else if let Some(initialize_result) = initialize_result {
        status.insert("initializeResult".into(), initialize_result.clone());
    }
    Value::Object(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inline_json() {
        let value = read_json(Some(r#"{"nodeId":"1-2"}"#), Value::Null).unwrap();
        assert_eq!(value["nodeId"], "1-2");
    }

    #[test]
    fn rejects_non_object_tool_arguments() {
        let value = read_json(Some("[]"), json!({})).unwrap();
        assert!(!value.is_object());
    }

    #[test]
    fn status_short_contains_only_connection_details() {
        let initialize = json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": {
                "name": "paper-desktop",
                "version": "0.5.5"
            },
            "capabilities": { "tools": {} },
            "instructions": "long instructions"
        });

        assert_eq!(
            status_output("http://127.0.0.1:29979/mcp", Some(&initialize), true),
            json!({
                "connected": true,
                "endpoint": "http://127.0.0.1:29979/mcp",
                "server": {
                    "name": "paper-desktop",
                    "version": "0.5.5"
                },
                "protocolVersion": "2025-06-18"
            })
        );
    }

    #[test]
    fn full_status_preserves_initialize_response() {
        let initialize = json!({
            "protocolVersion": "2025-06-18",
            "serverInfo": {
                "name": "paper-desktop",
                "version": "0.5.5"
            },
            "capabilities": { "tools": {} },
            "instructions": "Paper instructions"
        });
        let status = status_output("http://127.0.0.1:29979/mcp", Some(&initialize), false);

        assert_eq!(status["connected"], true);
        assert_eq!(status["endpoint"], "http://127.0.0.1:29979/mcp");
        for key in [
            "protocolVersion",
            "serverInfo",
            "capabilities",
            "instructions",
        ] {
            assert_eq!(status[key], initialize[key]);
        }
    }

    #[test]
    fn existing_generic_commands_remain_compatible() {
        assert!(matches!(
            Cli::try_parse_from(["paper", "call", "get_basic_info"])
                .unwrap()
                .command,
            Command::Call {
                name,
                arguments: None,
                text: false,
                output: None,
                force: false
            } if name == "get_basic_info"
        ));
        assert!(matches!(
            Cli::try_parse_from(["paper", "schema", "get_node_info"])
                .unwrap()
                .command,
            Command::Schema { name } if name == "get_node_info"
        ));
        assert!(matches!(
            Cli::try_parse_from(["paper", "request", "tools/list"])
                .unwrap()
                .command,
            Command::Request { method, params: None } if method == "tools/list"
        ));
        assert!(matches!(
            Cli::try_parse_from(["paper", "notify", "notifications/cancelled"])
                .unwrap()
                .command,
            Command::Notify { method, params: None } if method == "notifications/cancelled"
        ));
    }

    #[test]
    fn text_and_output_are_mutually_exclusive() {
        let error = Cli::try_parse_from([
            "paper",
            "call",
            "get_screenshot",
            "{}",
            "--text",
            "--output",
            "shot.png",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn file_and_screenshot_shortcuts_parse() {
        assert!(matches!(
            Cli::try_parse_from(["paper", "files", "--names"])
                .unwrap()
                .command,
            Command::Files {
                limit: 50,
                names: true
            }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "screenshot",
                "1-2",
                "--output",
                "shot.png",
                "--scale",
                "1",
                "--file-id",
                "file-1",
                "--force"
            ])
            .unwrap()
            .command,
            Command::Screenshot {
                node_id: Some(node_id),
                selected: false,
                active_artboard: false,
                artboard: None,
                output,
                scale: Some(1.0),
                file_id: Some(file_id),
                force: true
            } if node_id == "1-2"
                && output.as_path() == Path::new("shot.png")
                && file_id == "file-1"
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "open",
                "https://app.paper.design/file/file-1",
                "--page-id",
                "1-0"
            ])
            .unwrap()
            .command,
            Command::Open {
                file_id_or_url,
                page_id: Some(page_id)
            } if file_id_or_url == "https://app.paper.design/file/file-1" && page_id == "1-0"
        ));
    }

    #[test]
    fn screenshot_target_modes_parse() {
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "screenshot",
                "--selected",
                "--output",
                "selected.png"
            ])
            .unwrap()
            .command,
            Command::Screenshot {
                node_id: None,
                selected: true,
                active_artboard: false,
                artboard: None,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "screenshot",
                "--active-artboard",
                "--output",
                "artboard.jpg"
            ])
            .unwrap()
            .command,
            Command::Screenshot {
                node_id: None,
                selected: false,
                active_artboard: true,
                artboard: None,
                ..
            }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "screenshot",
                "--artboard",
                "Dashboard — Desktop",
                "--output",
                "artboard.jpg"
            ])
            .unwrap()
            .command,
            Command::Screenshot {
                node_id: None,
                selected: false,
                active_artboard: false,
                artboard: Some(name),
                ..
            } if name == "Dashboard — Desktop"
        ));
    }

    #[test]
    fn screenshot_target_modes_conflict_before_execution() {
        let conflict = Cli::try_parse_from([
            "paper",
            "screenshot",
            "node-1",
            "--selected",
            "--output",
            "shot.jpg",
        ])
        .unwrap_err();
        assert_eq!(conflict.kind(), clap::error::ErrorKind::ArgumentConflict);

        let missing =
            Cli::try_parse_from(["paper", "screenshot", "--output", "shot.jpg"]).unwrap_err();
        assert_eq!(
            missing.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn short_context_flag_parses() {
        assert!(matches!(
            Cli::try_parse_from([
                "paper",
                "context",
                "--short",
                "--file-id",
                "file-1"
            ])
            .unwrap()
            .command,
            Command::Context {
                short: true,
                file_id: Some(file_id)
            } if file_id == "file-1"
        ));
    }

    #[test]
    fn short_status_flag_parses() {
        assert!(matches!(
            Cli::try_parse_from(["paper", "status", "--short"])
                .unwrap()
                .command,
            Command::Status { short: true }
        ));
    }
}
