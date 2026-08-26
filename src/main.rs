mod mcp;

use std::{
    fs,
    io::{self, IsTerminal, Read},
    path::Path,
    process::ExitCode,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use mcp::McpClient;
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
    Status,

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
        #[arg(long)]
        text: bool,
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
    let (mut client, initialize_result) =
        McpClient::connect(&cli.url, timeout).context("failed to connect to Paper Desktop")?;

    let output = match cli.command {
        Command::Status => {
            let mut status = Map::new();
            status.insert("endpoint".into(), Value::String(cli.url));
            status.insert("connected".into(), Value::Bool(true));
            if let Some(object) = initialize_result.as_object() {
                status.extend(object.clone());
            } else {
                status.insert("initializeResult".into(), initialize_result);
            }
            Some(Value::Object(status))
        }
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
        } => {
            let arguments = read_json(arguments.as_deref(), json!({}))?;
            if !arguments.is_object() {
                bail!("tool arguments must be a JSON object");
            }

            let result = client.call_tool(&name, arguments)?;
            if text {
                print_text_content(&result)?;
                None
            } else {
                Some(result)
            }
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

fn print_text_content(result: &Value) -> Result<()> {
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .context("tool result has no MCP content array; omit --text to see the full JSON")?;

    let mut found = false;
    for item in content {
        if item.get("type").and_then(Value::as_str) == Some("text") {
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .context("Paper returned text content without a text value")?;
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
            found = true;
        }
    }

    if !found {
        bail!("tool returned no text content; omit --text to see the full JSON");
    }
    Ok(())
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
}
