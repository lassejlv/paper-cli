use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::mcp::McpClient;

pub trait PaperTools {
    fn list_tools(&mut self) -> Result<Vec<Value>>;
    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value>;
}

impl PaperTools for McpClient {
    fn list_tools(&mut self) -> Result<Vec<Value>> {
        McpClient::list_tools(self)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        McpClient::call_tool(self, name, arguments)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScreenshotTarget {
    NodeId(String),
    Selected,
    ActiveArtboard,
    Artboard(String),
}

pub fn capture_screenshot(
    tools: &mut impl PaperTools,
    target: ScreenshotTarget,
    file_id: Option<&str>,
    scale: Option<f64>,
) -> Result<Value> {
    let node_id = match target {
        ScreenshotTarget::NodeId(node_id) => node_id,
        target => {
            let catalog = ToolCatalog::load(tools)?;
            catalog.require("get_screenshot")?;
            resolve_screenshot_target(tools, &catalog, target, file_id)?
        }
    };

    let mut arguments = context_arguments(file_id);
    arguments.insert("nodeId".into(), Value::String(node_id));
    if let Some(scale) = scale {
        arguments.insert("scale".into(), json!(scale));
    }
    tools.call_tool("get_screenshot", Value::Object(arguments))
}

pub fn read_context(tools: &mut impl PaperTools, file_id: Option<&str>) -> Result<PaperContext> {
    let catalog = ToolCatalog::load(tools)?;
    catalog.require("get_basic_info")?;
    catalog.require("get_selection")?;

    let basic_raw = tools.call_tool("get_basic_info", Value::Object(context_arguments(file_id)))?;
    let selection_raw =
        tools.call_tool("get_selection", Value::Object(context_arguments(file_id)))?;
    let basic = BasicInfo::parse(&basic_raw)?;
    let selection = Selection::parse(&selection_raw)?;

    Ok(PaperContext {
        basic_raw,
        selection_raw,
        basic,
        selection,
    })
}

pub struct PaperContext {
    basic_raw: Value,
    selection_raw: Value,
    basic: BasicInfo,
    selection: Selection,
}

impl PaperContext {
    pub fn output(self, short: bool) -> Value {
        if !short {
            return json!({
                "basicInfo": self.basic_raw,
                "selection": self.selection_raw
            });
        }

        let artboards = self
            .basic
            .artboards
            .into_iter()
            .map(|artboard| {
                json!({
                    "name": artboard.name,
                    "width": artboard.width,
                    "height": artboard.height
                })
            })
            .collect::<Vec<_>>();
        let selected_nodes = self
            .selection
            .nodes
            .into_iter()
            .map(|node| Value::String(node.name))
            .collect::<Vec<_>>();
        let selected_node_count = selected_nodes.len();

        json!({
            "fileName": self.basic.file_name,
            "pageName": self.basic.page_name,
            "artboards": artboards,
            "selectedNodes": selected_nodes,
            "selectedNodeCount": selected_node_count
        })
    }
}

fn resolve_screenshot_target(
    tools: &mut impl PaperTools,
    catalog: &ToolCatalog,
    target: ScreenshotTarget,
    file_id: Option<&str>,
) -> Result<String> {
    match target {
        ScreenshotTarget::NodeId(node_id) => Ok(node_id),
        ScreenshotTarget::Selected => {
            catalog.require("get_selection")?;
            let selection = call_selection(tools, file_id)?;
            match selection.nodes.as_slice() {
                [] => bail!(
                    "no nodes are selected; select one node in Paper or use --active-artboard, --artboard <exact-name>, or a node ID"
                ),
                [node] => Ok(node.id.clone()),
                nodes => bail!(
                    "multiple nodes are selected ({}); select one node or use another screenshot target mode",
                    quoted_names(nodes.iter().map(|node| node.name.as_str()))
                ),
            }
        }
        ScreenshotTarget::ActiveArtboard => {
            catalog.require("get_selection")?;
            let selection = call_selection(tools, file_id)?;
            if let [node] = selection.nodes.as_slice()
                && let Some(artboard_id) = node.artboard_id.as_ref()
            {
                return Ok(artboard_id.clone());
            }

            catalog.require("get_basic_info")?;
            let basic = call_basic_info(tools, file_id)?;
            match basic.artboards.as_slice() {
                [] => bail!("the active page contains no artboards"),
                [artboard] => Ok(artboard.id.clone()),
                artboards => bail!(
                    "the active page contains multiple artboards ({}); use --artboard \"<exact-name>\"",
                    quoted_names(artboards.iter().map(|artboard| artboard.name.as_str()))
                ),
            }
        }
        ScreenshotTarget::Artboard(name) => {
            catalog.require("get_basic_info")?;
            let basic = call_basic_info(tools, file_id)?;
            let matches = basic
                .artboards
                .iter()
                .filter(|artboard| artboard.name == name)
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [artboard] => Ok(artboard.id.clone()),
                [] => {
                    let available = quoted_names(
                        basic
                            .artboards
                            .iter()
                            .map(|artboard| artboard.name.as_str()),
                    );
                    if available.is_empty() {
                        bail!(
                            "no artboard named {name:?} exists on the active page; no artboards are available"
                        );
                    }
                    bail!(
                        "no artboard named {name:?} exists on the active page; available artboards: {available}"
                    )
                }
                _ => bail!(
                    "multiple artboards are named {name:?}; rename the duplicates or use a raw node ID"
                ),
            }
        }
    }
}

fn call_selection(tools: &mut impl PaperTools, file_id: Option<&str>) -> Result<Selection> {
    let result = tools.call_tool("get_selection", Value::Object(context_arguments(file_id)))?;
    Selection::parse(&result)
}

fn call_basic_info(tools: &mut impl PaperTools, file_id: Option<&str>) -> Result<BasicInfo> {
    let result = tools.call_tool("get_basic_info", Value::Object(context_arguments(file_id)))?;
    BasicInfo::parse(&result)
}

fn context_arguments(file_id: Option<&str>) -> Map<String, Value> {
    let mut arguments = Map::new();
    if let Some(file_id) = file_id {
        arguments.insert("fileId".into(), Value::String(file_id.to_owned()));
    }
    arguments
}

struct ToolCatalog {
    names: HashSet<String>,
}

impl ToolCatalog {
    fn load(tools: &mut impl PaperTools) -> Result<Self> {
        let names = tools
            .list_tools()
            .context("failed to read Paper's live tool catalog")?
            .into_iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        Ok(Self { names })
    }

    fn require(&self, name: &str) -> Result<()> {
        if self.names.contains(name) {
            return Ok(());
        }
        bail!(
            "Paper does not expose the required `{name}` tool; run `paper tools --names` and update Paper Desktop if needed"
        )
    }
}

struct Selection {
    nodes: Vec<SelectedNode>,
}

impl Selection {
    fn parse(result: &Value) -> Result<Self> {
        let payload = tool_payload(result, "get_selection")?;
        let nodes = payload
            .get("selectedNodes")
            .and_then(Value::as_array)
            .context("get_selection response is missing selectedNodes")?
            .iter()
            .map(SelectedNode::parse)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { nodes })
    }
}

struct SelectedNode {
    id: String,
    name: String,
    artboard_id: Option<String>,
}

impl SelectedNode {
    fn parse(value: &Value) -> Result<Self> {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .context("get_selection returned a selected node without an ID")?
            .to_owned();
        let name = human_name(value, "Unnamed node");
        let artboard_id = value
            .get("artboardId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(Self {
            id,
            name,
            artboard_id,
        })
    }
}

struct BasicInfo {
    file_name: String,
    page_name: String,
    artboards: Vec<Artboard>,
}

impl BasicInfo {
    fn parse(result: &Value) -> Result<Self> {
        let payload = tool_payload(result, "get_basic_info")?;
        let file_name = payload
            .get("fileName")
            .and_then(Value::as_str)
            .context("get_basic_info response is missing fileName")?
            .to_owned();
        let page_name = payload
            .get("pageName")
            .and_then(Value::as_str)
            .context("get_basic_info response is missing pageName")?
            .to_owned();
        let artboards = payload
            .get("artboards")
            .and_then(Value::as_array)
            .context("get_basic_info response is missing artboards")?
            .iter()
            .map(Artboard::parse)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            file_name,
            page_name,
            artboards,
        })
    }
}

struct Artboard {
    id: String,
    name: String,
    width: Value,
    height: Value,
}

impl Artboard {
    fn parse(value: &Value) -> Result<Self> {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .context("get_basic_info returned an artboard without an ID")?
            .to_owned();
        Ok(Self {
            id,
            name: human_name(value, "Unnamed artboard"),
            width: value.get("width").cloned().unwrap_or(Value::Null),
            height: value.get("height").cloned().unwrap_or(Value::Null),
        })
    }
}

fn human_name(value: &Value, fallback: &str) -> String {
    for key in ["name", "component"] {
        if let Some(name) = value.get(key).and_then(Value::as_str)
            && !name.trim().is_empty()
        {
            return name.to_owned();
        }
    }
    fallback.to_owned()
}

fn tool_payload(result: &Value, tool_name: &str) -> Result<Value> {
    if let Some(payload) = result.get("structuredContent") {
        return Ok(payload.clone());
    }

    let content = result
        .get("content")
        .and_then(Value::as_array)
        .with_context(|| format!("{tool_name} returned no MCP content"))?;
    for item in content {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(payload) = serde_json::from_str(text) {
            return Ok(payload);
        }
    }
    bail!("{tool_name} returned no structured JSON content")
}

fn quoted_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct MockTools {
        available: Vec<&'static str>,
        responses: VecDeque<(&'static str, Value)>,
        calls: Vec<(String, Value)>,
        list_calls: usize,
    }

    impl MockTools {
        fn new(available: Vec<&'static str>, responses: Vec<(&'static str, Value)>) -> Self {
            Self {
                available,
                responses: responses.into(),
                calls: Vec::new(),
                list_calls: 0,
            }
        }
    }

    impl PaperTools for MockTools {
        fn list_tools(&mut self) -> Result<Vec<Value>> {
            self.list_calls += 1;
            Ok(self
                .available
                .iter()
                .map(|name| json!({ "name": name }))
                .collect())
        }

        fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
            self.calls.push((name.to_owned(), arguments));
            let Some((expected_name, response)) = self.responses.pop_front() else {
                bail!("unexpected call to {name}");
            };
            if expected_name != name {
                bail!("expected call to {expected_name}, got {name}");
            }
            Ok(response)
        }
    }

    fn tool_result(payload: Value) -> Value {
        json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&payload).unwrap()
            }]
        })
    }

    fn selection(nodes: Value) -> Value {
        let count = nodes.as_array().map_or(0, Vec::len);
        tool_result(json!({
            "selectedNodes": nodes,
            "count": count
        }))
    }

    fn basic_info(artboards: Value) -> Value {
        let artboard_count = artboards.as_array().map_or(0, Vec::len);
        tool_result(json!({
            "fileName": "Example file",
            "pageName": "Page 1",
            "artboards": artboards,
            "artboardCount": artboard_count
        }))
    }

    fn image_result() -> Value {
        json!({
            "content": [{
                "type": "image",
                "mimeType": "image/jpeg",
                "data": "/9j/2Q=="
            }]
        })
    }

    fn all_tools() -> Vec<&'static str> {
        vec!["get_selection", "get_basic_info", "get_screenshot"]
    }

    #[test]
    fn positional_node_id_preserves_existing_screenshot_behavior() {
        let mut tools = MockTools::new(vec![], vec![("get_screenshot", image_result())]);

        capture_screenshot(
            &mut tools,
            ScreenshotTarget::NodeId("node-secret".into()),
            Some("file-1"),
            Some(2.0),
        )
        .unwrap();

        assert_eq!(tools.list_calls, 0);
        assert_eq!(
            tools.calls,
            vec![(
                "get_screenshot".into(),
                json!({"nodeId": "node-secret", "fileId": "file-1", "scale": 2.0})
            )]
        );
    }

    #[test]
    fn selected_resolves_exactly_one_node() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                (
                    "get_selection",
                    selection(json!([{
                        "id": "node-secret",
                        "name": "Search field",
                        "artboardId": "artboard-secret"
                    }])),
                ),
                ("get_screenshot", image_result()),
            ],
        );

        capture_screenshot(&mut tools, ScreenshotTarget::Selected, None, None).unwrap();

        assert_eq!(tools.calls[1].1["nodeId"], "node-secret");
    }

    #[test]
    fn selected_rejects_empty_selection() {
        let mut tools = MockTools::new(all_tools(), vec![("get_selection", selection(json!([])))]);
        let error =
            capture_screenshot(&mut tools, ScreenshotTarget::Selected, None, None).unwrap_err();

        assert!(error.to_string().contains("no nodes are selected"));
        assert_eq!(tools.calls.len(), 1);
    }

    #[test]
    fn selected_rejects_multiple_nodes_without_exposing_ids() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![(
                "get_selection",
                selection(json!([
                    {"id": "secret-1", "name": "Header"},
                    {"id": "secret-2", "name": "Footer"}
                ])),
            )],
        );
        let error =
            capture_screenshot(&mut tools, ScreenshotTarget::Selected, None, None).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("\"Header\", \"Footer\""));
        assert!(!message.contains("secret-1"));
        assert!(!message.contains("secret-2"));
    }

    #[test]
    fn active_artboard_resolves_from_selected_child() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                (
                    "get_selection",
                    selection(json!([{
                        "id": "child-secret",
                        "name": "Title",
                        "artboardId": "artboard-secret"
                    }])),
                ),
                ("get_screenshot", image_result()),
            ],
        );

        capture_screenshot(&mut tools, ScreenshotTarget::ActiveArtboard, None, None).unwrap();

        assert_eq!(tools.calls[1].1["nodeId"], "artboard-secret");
        assert!(!tools.calls.iter().any(|(name, _)| name == "get_basic_info"));
    }

    #[test]
    fn active_artboard_falls_back_to_only_artboard() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                ("get_selection", selection(json!([]))),
                (
                    "get_basic_info",
                    basic_info(json!([{
                        "id": "artboard-secret",
                        "name": "Dashboard",
                        "width": 1440,
                        "height": 900
                    }])),
                ),
                ("get_screenshot", image_result()),
            ],
        );

        capture_screenshot(&mut tools, ScreenshotTarget::ActiveArtboard, None, None).unwrap();

        assert_eq!(tools.calls[2].1["nodeId"], "artboard-secret");
    }

    #[test]
    fn active_artboard_rejects_ambiguous_artboards() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                ("get_selection", selection(json!([]))),
                (
                    "get_basic_info",
                    basic_info(json!([
                        {"id": "secret-1", "name": "Desktop", "width": 1440, "height": 900},
                        {"id": "secret-2", "name": "Mobile", "width": 390, "height": 844}
                    ])),
                ),
            ],
        );
        let error = capture_screenshot(&mut tools, ScreenshotTarget::ActiveArtboard, None, None)
            .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("\"Desktop\", \"Mobile\""));
        assert!(message.contains("--artboard"));
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn active_artboard_rejects_page_without_artboards() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                ("get_selection", selection(json!([]))),
                ("get_basic_info", basic_info(json!([]))),
            ],
        );
        let error = capture_screenshot(&mut tools, ScreenshotTarget::ActiveArtboard, None, None)
            .unwrap_err();

        assert!(error.to_string().contains("contains no artboards"));
    }

    #[test]
    fn named_artboard_resolves_one_exact_match() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                (
                    "get_basic_info",
                    basic_info(json!([
                        {"id": "desktop-secret", "name": "Dashboard — Desktop", "width": 1440, "height": 900},
                        {"id": "mobile-secret", "name": "Dashboard — Mobile", "width": 390, "height": 844}
                    ])),
                ),
                ("get_screenshot", image_result()),
            ],
        );

        capture_screenshot(
            &mut tools,
            ScreenshotTarget::Artboard("Dashboard — Desktop".into()),
            None,
            None,
        )
        .unwrap();

        assert_eq!(tools.calls[1].1["nodeId"], "desktop-secret");
    }

    #[test]
    fn named_artboard_lists_available_names_when_no_match_exists() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![(
                "get_basic_info",
                basic_info(json!([
                    {"id": "secret-1", "name": "Desktop", "width": 1440, "height": 900},
                    {"id": "secret-2", "name": "Mobile", "width": 390, "height": 844}
                ])),
            )],
        );
        let error = capture_screenshot(
            &mut tools,
            ScreenshotTarget::Artboard("Tablet".into()),
            None,
            None,
        )
        .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("available artboards: \"Desktop\", \"Mobile\""));
        assert!(!message.contains("secret-1"));
    }

    #[test]
    fn named_artboard_rejects_duplicate_exact_names() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![(
                "get_basic_info",
                basic_info(json!([
                    {"id": "secret-1", "name": "Dashboard", "width": 1440, "height": 900},
                    {"id": "secret-2", "name": "Dashboard", "width": 390, "height": 844}
                ])),
            )],
        );
        let error = capture_screenshot(
            &mut tools,
            ScreenshotTarget::Artboard("Dashboard".into()),
            None,
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("multiple artboards are named"));
        assert!(!error.to_string().contains("secret-1"));
    }

    #[test]
    fn file_id_is_propagated_to_every_required_tool_call() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                ("get_selection", selection(json!([]))),
                (
                    "get_basic_info",
                    basic_info(json!([{
                        "id": "artboard-secret",
                        "name": "Dashboard",
                        "width": 1440,
                        "height": 900
                    }])),
                ),
                ("get_screenshot", image_result()),
            ],
        );

        capture_screenshot(
            &mut tools,
            ScreenshotTarget::ActiveArtboard,
            Some("file-1"),
            Some(1.0),
        )
        .unwrap();

        assert_eq!(tools.calls.len(), 3);
        for (_, arguments) in &tools.calls {
            assert_eq!(arguments["fileId"], "file-1");
        }
        assert_eq!(tools.calls[2].1["scale"], 1.0);
    }

    #[test]
    fn missing_required_tool_is_actionable() {
        let mut tools = MockTools::new(vec!["get_screenshot"], vec![]);
        let error =
            capture_screenshot(&mut tools, ScreenshotTarget::Selected, None, None).unwrap_err();

        assert!(error.to_string().contains("required `get_selection` tool"));
        assert!(error.to_string().contains("paper tools --names"));
    }

    #[test]
    fn short_context_contains_names_and_dimensions_without_ids() {
        let mut tools = MockTools::new(
            all_tools(),
            vec![
                (
                    "get_basic_info",
                    basic_info(json!([{
                        "id": "artboard-secret",
                        "name": "Dashboard — Desktop",
                        "width": 1440,
                        "height": 900
                    }])),
                ),
                (
                    "get_selection",
                    selection(json!([{
                        "id": "node-secret",
                        "name": "Search field",
                        "artboardId": "artboard-secret"
                    }])),
                ),
            ],
        );

        let output = read_context(&mut tools, Some("file-1"))
            .unwrap()
            .output(true);
        let rendered = serde_json::to_string(&output).unwrap();

        assert_eq!(output["fileName"], "Example file");
        assert_eq!(output["pageName"], "Page 1");
        assert_eq!(output["artboards"][0]["name"], "Dashboard — Desktop");
        assert_eq!(output["artboards"][0]["width"], 1440);
        assert_eq!(output["selectedNodes"][0], "Search field");
        assert_eq!(output["selectedNodeCount"], 1);
        assert!(!rendered.contains("secret"));
        for (_, arguments) in &tools.calls {
            assert_eq!(arguments["fileId"], "file-1");
        }
    }
}
