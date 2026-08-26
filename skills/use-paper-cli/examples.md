# Paper CLI Examples

Replace values in angle brackets before running commands. For generic tools,
always run `paper schema <tool-name>` first and adapt the JSON to the live
schema.

## 1. Check connectivity and active context

Use the short status for a health check:

```sh
paper status --short
```

Expected connected shape:

```json
{
  "connected": true,
  "endpoint": "http://127.0.0.1:29979/mcp",
  "protocolVersion": "<negotiated-version>",
  "server": {
    "name": "paper-desktop",
    "version": "<installed-version>"
  }
}
```

Then verify the active design:

```sh
paper context --short
paper call get_basic_info --text
paper call get_selection --text
```

Use `paper context --short --file-id <file-id>` when several files are open. The
short form includes file/page names, artboard names and dimensions, selected
node names, and the selection count without exposing raw node IDs.

Do not continue with a mutation if the reported file, page, or selection is not
the user's intended target.

## 2. List files and open one explicitly

List structured file data when IDs are needed:

```sh
paper files --limit 50
```

List names only for a quick interactive overview:

```sh
paper files --names
```

Open by file ID:

```sh
paper open <file-id>
```

Open a specific page through a full URL:

```sh
paper open 'https://app.paper.design/file/<file-id>' --page-id <page-id>
```

Verify the new context:

```sh
paper call get_basic_info --text
```

There is intentionally no fuzzy name matching. If two files have similar names,
inspect the structured list and choose an exact ID or URL.

## 3. Discover an unfamiliar tool

Search the live catalog:

```sh
paper tools --names
```

Read a tool's exact schema:

```sh
paper schema find_nodes
```

Build arguments only after reading the schema:

```sh
cat > /tmp/paper-find-nodes.json <<'JSON'
{
  "replace": "these example fields with the live schema"
}
JSON

paper call find_nodes @/tmp/paper-find-nodes.json
```

If Paper rejects the arguments, do not keep guessing. Re-read the schema and
correct the JSON.

## 4. Inspect one selected design surface

Start with the current selection:

```sh
paper call get_selection --text
```

For each relevant tool, inspect the schema first:

```sh
paper schema get_node_info
paper schema get_tree_summary
paper schema get_computed_styles
paper schema get_jsx
```

Then call only the tools needed for the task:

```sh
paper call get_node_info '{"nodeId":"<node-id>"}' --text
```

For tools with larger or evolving argument objects, use a file:

```sh
cat > /tmp/paper-styles.json <<'JSON'
{
  "replace": "with arguments from paper schema get_computed_styles"
}
JSON

paper call get_computed_styles @/tmp/paper-styles.json
```

Use hierarchy and computed styles for exact values. A screenshot is supporting
visual evidence, not the only source for dimensions, colors, or typography.

## 5. Capture a screenshot directly to disk

Generic tool workflow:

```sh
paper schema get_screenshot
paper call get_screenshot '{"nodeId":"<node-id>","scale":1}' \
  --output captures/current-state.jpg
```

Equivalent convenience command:

```sh
paper screenshot --active-artboard \
  --output captures/artboard.jpg \
  --scale 1

paper screenshot --selected \
  --output captures/selection.png

paper screenshot --artboard "Dashboard — Desktop" \
  --output captures/dashboard.jpg \
  --file-id <file-id>

paper screenshot <node-id> \
  --output captures/current-state.jpg
```

Supply exactly one of the positional node ID, `--selected`,
`--active-artboard`, or `--artboard`.

- Use `--selected` when one visible design node is selected.
- Use `--active-artboard` when the selection belongs to the desired artboard,
  or when the active page has only one artboard.
- Use `--artboard` when several artboards exist and the exact full name is
  known.
- Use a raw node ID for automation that already resolved a specific node, or
  for a node that is not naturally addressed by selection or artboard name.

The CLI does not guess when multiple nodes are selected, multiple artboards are
possible, or duplicate artboard names exist. It reports human-readable names
and asks for a narrower target.

The success response is concise JSON:

```json
{
  "bytes": 41509,
  "mimeType": "image/jpeg",
  "path": "captures/current-state.jpg"
}
```

If the expected format is unknown, omit the extension:

```sh
paper screenshot <node-id> --output captures/current-state
```

The CLI adds a supported extension from the returned MIME type.

To replace an existing capture only after confirming the path:

```sh
paper screenshot <node-id> \
  --output captures/current-state.jpg \
  --scale 1 \
  --force
```

Never combine `--text` and `--output`.

## 6. Preserve raw image JSON for automation

Omit output flags to keep the complete MCP result:

```sh
paper --compact call get_screenshot '{"nodeId":"<node-id>","scale":1}' \
  > /tmp/paper-screenshot.json
```

This intentionally preserves image base64 and annotations in structured JSON.
Do not use `--text`; image content is not terminal text.

## 7. Export an asset

Discover the current export schema:

```sh
paper schema export
```

Create an arguments file that exactly matches it:

```sh
cat > /tmp/paper-export.json <<'JSON'
{
  "replace": "with the live export arguments and destination"
}
JSON

paper call export @/tmp/paper-export.json
```

After export, verify the returned paths and inspect the actual files. Prefer SVG
for flat vector assets and an appropriately scaled raster format for
photographic or alpha-heavy content.

## 8. Make a targeted text or style edit

First verify the file and target:

```sh
paper call get_basic_info --text
paper call get_selection --text
paper call get_node_info '{"nodeId":"<node-id>"}' --text
```

Read the mutation schema:

```sh
paper schema set_text_content
paper schema update_styles
```

Create arguments from the live schema:

```sh
cat > /tmp/paper-edit.json <<'JSON'
{
  "replace": "with one focused update matching the live schema"
}
JSON

paper call <set_text_content-or-update_styles> @/tmp/paper-edit.json
```

Verify the result:

```sh
paper call get_node_info '{"nodeId":"<node-id>"}' --text
paper screenshot <verification-node-id> \
  --output captures/after-edit.jpg \
  --scale 1
```

Finish the editing lifecycle:

```sh
paper schema finish_working_on_nodes
paper call finish_working_on_nodes '{}'
```

Follow the live schema if it requires arguments.

## 9. Add a coherent visual group with HTML

Use this only when the user has authorized a design mutation.

```sh
paper call get_basic_info --text
paper call get_selection --text
paper schema write_html
```

Keep large markup out of the shell command:

```sh
cat > /tmp/paper-write-html.json <<'JSON'
{
  "replace": "with one visual group and the live write_html arguments"
}
JSON

paper call write_html @/tmp/paper-write-html.json
```

Then inspect returned nodes, capture the changed region, and call
`finish_working_on_nodes`. Do not add several unrelated page sections in one
`write_html` call.

## 10. Work with comments

Discover comment tools from the live catalog:

```sh
paper tools --names
paper schema list_comment_threads
```

List threads using arguments from the schema:

```sh
paper call list_comment_threads '<arguments-from-live-schema>'
```

Before changing a thread's status, inspect both the thread and the mutation
schema:

```sh
paper schema get_comment_thread
paper schema set_comment_thread_status
```

Do not resolve or reopen comments unless the user asked for that state change.

## 11. Handle an existing output file

The safe default refuses replacement:

```sh
paper screenshot <node-id> --output captures/review.jpg
```

Typical error:

```text
error: output file captures/review.jpg already exists; pass --force to overwrite it
```

Choose a new name when history matters:

```sh
paper screenshot <node-id> --output captures/review-2.jpg
```

Use `--force` only when the existing artifact is intentionally disposable:

```sh
paper screenshot <node-id> --output captures/review.jpg --force
```

## 12. Recover from common failures

### Paper is disconnected

```sh
paper status --short
```

Confirm Paper Desktop is running and a file is open. A disconnected short
status exits nonzero and reports null server/protocol details.

### A tool is unknown

```sh
paper tools --names
```

Use the live name; do not assume an old name still exists.

### Arguments are invalid

```sh
paper schema <tool-name>
```

Rebuild the JSON from that schema.

### `--text` rejects the response

Keep structured JSON:

```sh
paper call <tool-name> '<arguments>'
```

Or write one returned image:

```sh
paper call <tool-name> '<arguments>' --output <image-path>
```

### Multiple images were returned

Do not force them into one output file. Preserve the raw JSON and process each
image deliberately:

```sh
paper --compact call <tool-name> '<arguments>' > /tmp/paper-result.json
```

## 13. Final verification report

After completing the work, report:

```text
Paper file/page:
Target surface:
Read tools used:
Mutation tools used:
Verification performed:
Remaining uncertainty:
```

Describe nodes by their visible role or name. Do not expose raw node IDs in the
user-facing report.
