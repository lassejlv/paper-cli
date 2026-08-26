---
name: use-paper-cli
description: Operates Paper Desktop through the paper Rust CLI when native MCP access is unavailable. Use when an agent needs to inspect, create, edit, comment on, capture, export, or otherwise work with Paper design files from shell commands.
---

# Use Paper CLI

Use `paper` as a generic, schema-driven bridge to Paper Desktop's live MCP
server. Paper's live tool catalog and schemas are authoritative; never rely on
a remembered argument shape.

## Preconditions

1. Check whether the CLI is installed:

   ```sh
   command -v paper
   ```

2. If it is missing, report that clearly. Do not silently install software.
   Suggested installation from this repository:

   ```sh
   cargo install --path .
   ```

3. Paper Desktop must be running. A Paper file must be open for design tools.
4. Verify connectivity before doing work:

   ```sh
   paper status --short
   ```

The default endpoint is `http://127.0.0.1:29979/mcp`. Use `--url` or
`PAPER_MCP_URL` only when the user has configured another endpoint.

## Core rule: discover, then call

The CLI intentionally does not mirror Paper's tool catalog. Discover tools and
read the exact live schema before each unfamiliar or consequential call:

```sh
paper tools --names
paper schema <tool-name>
paper call <tool-name> '<json-object>'
```

Do not guess parameter names, accepted values, or response shapes. Re-read a
schema after a Paper update or after a validation error.

Use the generic commands as the foundation:

- `paper call` invokes any advertised Paper tool.
- `paper schema` reads one live tool definition.
- `paper request` sends another MCP request after initialization.
- `paper notify` sends an MCP notification after initialization.

Use convenience commands only where they fit exactly:

- `paper status --short`
- `paper files [--limit 50] [--names]`
- `paper open <file-id-or-url> [--page-id <id>]`
- `paper screenshot <node-id> --output <path> [--scale 1] [--file-id <id>] [--force]`

## Standard workflow

### 1. Establish file context

```sh
paper status --short
paper files
paper call get_basic_info --text
paper call get_selection --text
```

Use `paper files`, not `--names`, when file IDs are needed. If several files are
open, target one explicitly:

```sh
paper open <file-id-or-url> --page-id <page-id>
```

Pass `fileId` to tools that support it when ambiguity is possible. Do not assume
the most recently active file is the intended one.

### 2. Inspect before mutation

Start with read-only tools. Read schemas before invoking hierarchy, style, JSX,
font, token, comment, screenshot, or export tools:

```sh
paper schema get_node_info
paper schema get_tree_summary
paper schema get_computed_styles
paper schema get_screenshot
```

Resolve the target from `get_selection` or an explicit user-provided node. Do
not choose a similarly named node when multiple nodes fit.

### 3. Choose the output mode

**Structured JSON — default**

Use when automation needs the complete MCP result, including annotations,
structured content, images, or embedded resources:

```sh
paper call get_selection
paper --compact call get_selection
```

**Text**

Use only when the response is text-only:

```sh
paper call get_basic_info --text
```

The CLI rejects `--text` when any non-text content is present. Do not work
around this protection or print base64 into the terminal.

**Image file**

Use `--output` when exactly one image is expected:

```sh
paper call get_screenshot '{"nodeId":"<node-id>"}' --output captures/screen.jpg
paper screenshot <node-id> --output captures/screen.jpg --scale 1
```

The extension must match the returned MIME type. Omit the extension when the
CLI should infer it. Parent directories are created automatically. Never add
`--force` unless replacing that exact file is intended.

### 4. Supply arguments safely

Use inline JSON for small inputs:

```sh
paper call <tool-name> '{"key":"value"}'
```

Use a file for long HTML, style updates, token batches, or other complex input:

```sh
paper call <tool-name> @arguments.json
```

Use stdin when another command produces the arguments:

```sh
printf '%s' '{"key":"value"}' | paper call <tool-name> -
```

Arguments for `paper call` must be a JSON object. Keep stdout available for
machine-readable results; diagnostics and failures are written to stderr.

### 5. Make focused changes

When the user authorizes design changes:

1. Read the mutation tool's current schema.
2. Respect tool annotations such as `readOnlyHint` and `destructiveHint`.
3. Before typographic styling, call `get_font_family_info`.
4. Prefer targeted text/style/move/duplicate operations over rewriting a large
   subtree.
5. Keep each `write_html` call to one coherent visual group.
6. Preserve unrelated nodes and existing design tokens.
7. Verify returned node information rather than trusting remembered node IDs.

Before deleting a node because its parent appears wrong, call `get_node_info`
and verify the relationship.

### 6. Verify and finish

After meaningful changes:

1. Re-read the changed node or subtree.
2. Capture a screenshot at scale 1 for layout review; use scale 2 only for fine
   typography or small details.
3. Confirm the actual state matches the request.
4. Call `finish_working_on_nodes` when creation or editing is complete:

   ```sh
   paper schema finish_working_on_nodes
   paper call finish_working_on_nodes '{}'
   ```

If the live schema requires arguments, follow it instead of the example.

## Safety rules

- Paper tools act on the active file unless a supported `fileId` is supplied.
- Read-only inspection does not authorize mutation.
- Ask before destructive or unrelated mutations unless the request already
  authorizes them.
- `paper open` changes Paper's active context; use it deliberately.
- `--force` overwrites a local output file and must be explicit.
- Do not use `paper-gen://` unless the user explicitly requested image
  generation and the relevant Paper guide has been loaded.
- Do not expose raw node IDs in the final user-facing response.
- Never report completion from a successful exit code alone; inspect the final
  Paper state.

## Error handling

- Connection failure: confirm Paper Desktop is running and a file is open, then
  retry `paper status --short`.
- Unknown tool: run `paper tools --names`; the catalog may have changed.
- Invalid arguments: run `paper schema <tool-name>` again and rebuild the JSON.
- `--text` rejection: omit `--text` for structured JSON or use `--output` for
  one image.
- MIME/extension mismatch: use a compatible extension or omit it for inference.
- Existing output file: choose another path or use `--force` only after
  confirming replacement.
- Multiple returned images: keep structured JSON and handle each image
  deliberately; `--output` writes exactly one image.

## Completion report

State:

- The Paper file and page used, without exposing raw node IDs.
- What was inspected or changed.
- Which Paper tools or convenience commands were used.
- How the final state was verified.
- Any unresolved ambiguity, unsupported output, or pending destructive action.

## Detailed examples

Read [examples.md](examples.md) for copy-paste workflows covering file
selection, design inspection, screenshots, exports, mutations, comments, and
failure recovery.
