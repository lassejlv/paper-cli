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
cargo install --git https://github.com/lassejlv/paper-cli --locked
```

This installs the binary as `paper`.

## Upgrade

Update both the globally installed `use-paper-cli` agent skill and the CLI:

```sh
paper upgrade
```

This command does not connect to Paper Desktop. It updates the skill with the
`skills` CLI, then reinstalls `paper-cli` from this repository. It requires
`npx`, Cargo, and internet access. Progress and subprocess diagnostics go to
stderr; successful stdout is concise JSON describing both updated components.

The skill must have been installed globally through `npx skills`. If either
prerequisite is missing, the command fails before changing either component.

## Check connectivity

Print Paper's complete initialization response:

```sh
paper status
```

For a concise health check with only the endpoint, server, and protocol:

```sh
paper status --short
```

The short form emits JSON and exits nonzero if Paper Desktop cannot be reached.

Inspect the current file, page, artboards, and selection without raw node IDs:

```sh
paper context --short
paper context --short --file-id 01M0YQM4F9KY2YBG2MFCP1J27Q
```

Without `--short`, `paper context` preserves the raw `get_basic_info` and
`get_selection` MCP results as structured JSON.

## List and open files

List open and recently accessed Paper files as structured MCP JSON:

```sh
paper files
paper files --limit 100
```

For an interactive name-only list:

```sh
paper files --names
```

Open a file by ID, route, or full Paper URL, optionally at a page:

```sh
paper open 01M0YQM4F9KY2YBG2MFCP1J27Q
paper open 'https://app.paper.design/file/01M0YQM4F9KY2YBG2MFCP1J27Q' --page-id 1-0
```

These are thin adapters over Paper's live `list_files` and `open_file` tools.

## Discover and call tools

Discover capabilities instead of guessing tool names or arguments:

```sh
paper tools --names
paper schema get_node_info
paper schema get_screenshot
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

## Capture images

Decode a screenshot directly to disk through the generic `call` workflow:

```sh
paper call get_screenshot '{"nodeId":"1-2"}' --output captures/screenshot.jpg
```

The convenience command can resolve a human-friendly target before delegating
to that same live Paper tool and output path:

```sh
paper screenshot --active-artboard --output captures/artboard.jpg
paper screenshot --selected --output captures/selection.png
paper screenshot --artboard "Dashboard — Desktop" --output captures/dashboard.jpg
paper screenshot 1-2 --output captures/screenshot.jpg --scale 1
paper screenshot 1-2 --output captures/screenshot.jpg --file-id 01M0YQM4F9KY2YBG2MFCP1J27Q
```

Supply exactly one target:

- `--selected` requires exactly one selected node.
- `--active-artboard` uses the single selected node's artboard, then falls back
  to the active page only when that page has exactly one artboard.
- `--artboard "<exact-name>"` matches one full artboard name without fuzzy
  matching.
- A positional node ID remains available when a known non-selection node is the
  intended target or automation already has its ID.

Ambiguous selections and artboard names fail without guessing. Errors list
human-readable names, not raw node IDs. Pass `--file-id` to keep selection,
artboard resolution, and capture scoped to the same Paper file.

The output extension must match the returned MIME type. If the path has no
extension, the CLI infers one for supported image formats. Parent directories
are created automatically. Existing files are protected by default:

```sh
# Fails if captures/screenshot.jpg already exists
paper screenshot 1-2 --output captures/screenshot.jpg

# Explicitly replace it
paper screenshot 1-2 --output captures/screenshot.jpg --force
```

On success, stdout contains concise JSON with the final path, MIME type, and
byte count.

## Choose an output mode

With no output option, responses remain structured JSON, preserving MCP text,
images, embedded resources, structured content, and annotations without loss:

```sh
paper call get_screenshot '{"nodeId":"1-2"}'
```

Use `--text` only when a tool returns text content:

```sh
paper call get_basic_info --text
```

The CLI rejects `--text` for images and other non-text MCP content so base64 is
not accidentally dumped to the terminal. Use `--output <path>` to decode one
image, or omit both options to preserve the complete JSON response.

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
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Releasing

Create a GitHub release for the intended tag and publish it as a prerelease. The
`Release binaries` workflow then:

1. Tests the tagged source.
2. Builds native Intel and ARM binaries for macOS, Linux, and Windows.
3. Packages each binary with this README and license.
4. Attaches all six archives and `SHA256SUMS` to the prerelease.
5. Promotes it to the normal/latest release only after every build and upload
   succeeds.

Release assets:

```text
paper-x86_64-apple-darwin.tar.gz
paper-aarch64-apple-darwin.tar.gz
paper-x86_64-unknown-linux-gnu.tar.gz
paper-aarch64-unknown-linux-gnu.tar.gz
paper-x86_64-pc-windows-msvc.zip
paper-aarch64-pc-windows-msvc.zip
SHA256SUMS
```

GitHub prereleases are already visible when the workflow starts. Promotion is
delayed until all assets exist, but the prerelease itself is not private during
the build.
