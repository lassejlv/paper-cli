use std::{collections::HashSet, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue},
};
use serde_json::{Value, json};

const CLIENT_PROTOCOL_VERSION: &str = "2025-06-18";
const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";

pub struct McpClient {
    http: Client,
    endpoint: String,
    session_id: Option<String>,
    protocol_version: String,
    next_id: u64,
    closed: bool,
}

impl McpClient {
    pub fn connect(endpoint: &str, timeout: Duration) -> Result<(Self, Value)> {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to construct HTTP client")?;

        let mut client = Self {
            http,
            endpoint: endpoint.to_owned(),
            session_id: None,
            protocol_version: CLIENT_PROTOCOL_VERSION.to_owned(),
            next_id: 1,
            closed: false,
        };

        let initialize_result = client.request_uninitialized(
            "initialize",
            Some(json!({
                "protocolVersion": CLIENT_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "paper-cli",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        )?;

        let protocol_version = initialize_result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .context("Paper's initialize response has no protocolVersion")?;
        client.protocol_version = protocol_version.to_owned();
        client.notify("notifications/initialized", None)?;

        Ok((client, initialize_result))
    }

    pub fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        self.send_request(method, params, true)
    }

    pub fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let mut payload = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(params) = params {
            payload["params"] = params;
        }

        let response = self.post(&payload, true)?;
        consume_notification_response(response)
            .with_context(|| format!("MCP notification `{method}` failed"))
    }

    pub fn list_tools(&mut self) -> Result<Vec<Value>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen_cursors = HashSet::new();

        loop {
            let params = cursor.as_ref().map(|cursor| json!({ "cursor": cursor }));
            let result = self.request("tools/list", params)?;
            let page = result
                .get("tools")
                .and_then(Value::as_array)
                .context("Paper's tools/list response has no tools array")?;
            tools.extend(page.iter().cloned());

            let Some(next_cursor) = result.get("nextCursor").and_then(Value::as_str) else {
                break;
            };
            if !seen_cursors.insert(next_cursor.to_owned()) {
                bail!("Paper repeated tools/list cursor `{next_cursor}`");
            }
            cursor = Some(next_cursor.to_owned());
        }

        Ok(tools)
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        if name != "get_guide" {
            let guide = self.request(
                "tools/call",
                Some(json!({
                    "name": "get_guide",
                    "arguments": { "topic": "paper-mcp-instructions" }
                })),
            )?;
            ensure_tool_succeeded("get_guide", &guide)?;
        }

        let result = self.request(
            "tools/call",
            Some(json!({
                "name": name,
                "arguments": arguments
            })),
        )?;
        ensure_tool_succeeded(name, &result)?;
        Ok(result)
    }

    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;

        let Some(session_id) = self.session_id.as_deref() else {
            return;
        };
        let Ok(session_header) = HeaderValue::from_str(session_id) else {
            return;
        };
        let Ok(protocol_header) = HeaderValue::from_str(&self.protocol_version) else {
            return;
        };

        let _ = self
            .http
            .delete(&self.endpoint)
            .header(HeaderName::from_static(SESSION_HEADER), session_header)
            .header(HeaderName::from_static(PROTOCOL_HEADER), protocol_header)
            .send();
    }

    fn request_uninitialized(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        self.send_request(method, params, false)
    }

    fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
        initialized: bool,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let mut payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(params) = params {
            payload["params"] = params;
        }

        let response = self.post(&payload, initialized)?;
        parse_request_response(response, id)
            .with_context(|| format!("MCP request `{method}` failed"))
    }

    fn post(&mut self, payload: &Value, initialized: bool) -> Result<Response> {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .json(payload);

        if initialized {
            let session_id = self
                .session_id
                .as_ref()
                .context("Paper did not establish an MCP session")?;
            request = request
                .header(HeaderName::from_static(SESSION_HEADER), session_id)
                .header(
                    HeaderName::from_static(PROTOCOL_HEADER),
                    &self.protocol_version,
                );
        }

        let response = request
            .send()
            .with_context(|| format!("could not reach {}", self.endpoint))?;

        if self.session_id.is_none()
            && let Some(session_id) = response
                .headers()
                .get(HeaderName::from_static(SESSION_HEADER))
        {
            self.session_id = Some(
                session_id
                    .to_str()
                    .context("Paper returned a non-UTF-8 MCP session ID")?
                    .to_owned(),
            );
        }

        Ok(response)
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        self.close();
    }
}

fn consume_notification_response(response: Response) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response
        .text()
        .context("failed to read Paper's HTTP error response")?;
    bail!("HTTP {status}: {body}");
}

fn parse_request_response(response: Response, expected_id: u64) -> Result<Value> {
    let status = response.status();
    let is_event_stream = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    let body = response
        .text()
        .context("failed to read Paper's HTTP response")?;

    if !status.is_success() {
        bail!("HTTP {status}: {body}");
    }
    if status == StatusCode::ACCEPTED {
        bail!("Paper accepted a request but did not return its result");
    }

    let messages = if is_event_stream {
        parse_sse_messages(&body)?
    } else {
        parse_json_messages(&body)?
    };

    let response = messages
        .into_iter()
        .find(|message| message.get("id").and_then(Value::as_u64) == Some(expected_id))
        .with_context(|| {
            format!("Paper returned no JSON-RPC response for request {expected_id}")
        })?;

    if let Some(error) = response.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown JSON-RPC error");
        let data = error.get("data");
        let mut rendered = match code {
            Some(code) => format!("JSON-RPC {code}: {message}"),
            None => format!("JSON-RPC error: {message}"),
        };
        if let Some(data) = data {
            rendered.push_str(": ");
            rendered.push_str(&serde_json::to_string(data)?);
        }
        bail!(rendered);
    }

    response
        .get("result")
        .cloned()
        .context("Paper's JSON-RPC response has neither result nor error")
}

fn parse_json_messages(body: &str) -> Result<Vec<Value>> {
    let value: Value = serde_json::from_str(body).context("Paper returned malformed JSON")?;
    match value {
        Value::Array(messages) => Ok(messages),
        message => Ok(vec![message]),
    }
}

fn parse_sse_messages(body: &str) -> Result<Vec<Value>> {
    let mut messages = Vec::new();
    let mut data_lines = Vec::new();

    for line in body.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n");
                let value = serde_json::from_str(&data)
                    .with_context(|| format!("Paper returned malformed SSE data: {data}"))?;
                messages.push(value);
                data_lines.clear();
            }
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data));
        }
    }

    if messages.is_empty() {
        bail!("Paper returned an empty event stream");
    }
    Ok(messages)
}

fn ensure_tool_succeeded(name: &str, result: &Value) -> Result<()> {
    if result.get("isError").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }

    let detail = result
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|detail| !detail.is_empty())
        .unwrap_or_else(|| result.to_string());
    Err(anyhow!("Paper tool `{name}` failed: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_and_multiline_sse_data() {
        let body = concat!(
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\n",
            "data: \"id\":2,\"result\":{\"ok\":true}}\n\n"
        );

        let messages = parse_sse_messages(body).unwrap();
        assert_eq!(messages[0]["id"], 2);
        assert_eq!(messages[0]["result"]["ok"], true);
    }

    #[test]
    fn ignores_sse_comments_and_non_data_fields() {
        let body = concat!(
            ": keep-alive\n",
            "event: message\n",
            "id: ignored\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":null}\n\n"
        );

        let messages = parse_sse_messages(body).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], 3);
    }

    #[test]
    fn rejects_empty_sse_stream() {
        let error = parse_sse_messages(": keep-alive\n\n").unwrap_err();
        assert!(error.to_string().contains("empty event stream"));
    }
}
