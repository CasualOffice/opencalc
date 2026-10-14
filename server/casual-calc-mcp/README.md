# casual-calc-mcp

An [MCP](https://modelcontextprotocol.io) server that lets an AI agent open,
read, edit, recalculate and save spreadsheets with the OpenCalc engine
(design: [docs/97](../../docs/97-AGENT-ACCESS.md), row `AGENT-01`).

It speaks MCP over **stdio** — JSON-RPC 2.0, one message per line — using the
initialize-based protocol revisions `2025-11-25`, `2025-06-18` and
`2025-03-26`. It has no network code.

## Build and run

```sh
cargo build --release -p casual-calc-mcp
target/release/casual-calc-mcp --root ~/Documents/spreadsheets
```

The client launches the binary and talks to it on stdin/stdout; you do not run
it by hand except to check it starts. Diagnostics go to stderr.

## The `--root` rule

The server reads and writes files **only under the directories given with
`--root`** (repeat the flag for several). Every path is canonicalised — `..`
and symlinks resolved by the operating system — and refused if the result is
outside every root. A relative path is taken relative to the first root.
**With no `--root`, every file tool refuses.**

Other bounds: files larger than the engine's open limit are refused
(`--max-file-bytes N` lowers it, never raises it); at most 10,000 cells per
read or write; at most 16 workbooks open at once.

## Tools

| Tool | Does |
|---|---|
| `open_workbook` | Open `.xlsx`/`.xlsm`/`.ods`/`.csv`/`.tsv`/`.psv`; returns a handle, the sheets, and the import compatibility report |
| `list_sheets` | Names, used ranges, visibility, protection |
| `read_range` | Values, formulas, or both (with display text) for an A1 range |
| `write_cells` | A 2-D array at an anchor, read as typed entry reads it; refused cells come back with the reason |
| `evaluate` | A formula against the workbook, without writing it |
| `recalculate` | Full recalculation |
| `run` | `insert_rows`, `delete_rows`, `insert_columns`, `delete_columns`, `set_number_format`, `add_sheet`, `rename_sheet` |
| `save` | To the original path or a new one; returns the save-loss report |
| `close` | Release a handle |

## Claude Code

```sh
claude mcp add opencalc -- /path/to/casual-calc-mcp --root /path/to/workbooks
```

or in a project's `.mcp.json`:

```json
{
  "mcpServers": {
    "opencalc": {
      "command": "/path/to/casual-calc-mcp",
      "args": ["--root", "/path/to/workbooks"]
    }
  }
}
```

## Claude Desktop

In `claude_desktop_config.json` (Settings → Developer → Edit Config):

```json
{
  "mcpServers": {
    "opencalc": {
      "command": "/path/to/casual-calc-mcp",
      "args": ["--root", "/Users/me/Documents/spreadsheets"]
    }
  }
}
```

Use absolute paths: the client may launch the server from any directory.
