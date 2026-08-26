# paper-cli

`paper-cli` gives terminal-based coding agents full access to the MCP server
inside [Paper Desktop](https://paper.design/). It is useful when an agent can run
shell commands but cannot connect to MCP servers itself.

The CLI does not hard-code a subset of Paper commands. It discovers the live
`tools/list` catalog and can invoke any tool Paper exposes, including tools added
by future Paper releases. Generic `request` and `notify` commands also expose
other MCP methods.

## Requirements

- Paper Desktop is running.
- A Paper file is open.
- A current stable Rust toolchain (the project uses Rust 2024).

Paper Desktop serves MCP at `http://127.0.0.1:29979/mcp` by default.

## Install

```sh
cargo install --path .
```

This installs the binary as `paper`.

## Agent workflow

Discover capabilities instead of guessing tool names or arguments:

```sh
paper status
paper tools
paper schema get_node_info
```

Call any tool with an inline JSON object:

```sh
paper call get_basic_info
paper call get_selection
paper call get_node_info '{"nodeId":"1-2"}'
```

Arguments can also come from a file or stdin:

```sh
paper call write_html @write-html.json
printf '%s' '{"nodeIds":["1-2"]}' | paper call get_computed_styles -
```

Responses are JSON by default, preserving MCP text, images, embedded resources,
structured content, and annotations without loss. Use `--text` when a tool
returns text content and only that text is wanted:

```sh
paper call get_basic_info --text
```

Use compact JSON for scripts:

```sh
paper --compact call get_selection
```

Every process initializes a standards-compliant Streamable HTTP MCP session and
loads Paper's required `paper-mcp-instructions` guide before invoking any other
Paper tool.

## All MCP methods

`call` covers every Paper tool advertised by `tools/list`. The generic commands
can reach additional MCP capabilities if Paper advertises them in the future:

```sh
paper request prompts/list
paper request resources/read '{"uri":"paper://example"}'
paper notify notifications/cancelled '{"requestId":42,"reason":"cancelled"}'
```

Set a different endpoint with `--url` or `PAPER_MCP_URL`:

```sh
PAPER_MCP_URL=http://127.0.0.1:29979/mcp paper status
```

Run `paper help` or `paper help call` for the complete command reference.

## Safety

The active Paper file is the target. Read-only tools inspect it, while tools
such as `write_html`, `update_styles`, and `delete_nodes` modify it immediately.
The CLI intentionally does not weaken or omit write capabilities; the calling
agent is responsible for confirming destructive actions and following Paper's
working-node lifecycle.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
