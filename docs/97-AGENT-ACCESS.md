# 97 — Agent access: an AI agent drives a workbook

**Status: Accepted by the owner, 2026-10-15** (*"add these in our list … and lets
start covering it"*). Tracker row `XL-FEAT-122`; phases are rows `AGENT-01..`.

## Why

An external review (2026-10-15) pointed at a comparable project that ships an
MCP server, a CLI that runs any command, and a scripting channel into the
running app. OpenCalc has the pieces underneath — the SDK opens, edits,
recalculates and saves every format we support, and every editor command has
an id in one registry (`webapp/commands.js`) — but nothing an agent can call.
Excel has nothing like this either: here OpenCalc can be ahead, not just level.

## The decision

One set of **tools**, defined once in Rust over the SDK, exposed three ways:

1. **MCP server over stdio** (`AGENT-01`) — for Claude and any MCP client.
2. **CLI** (`AGENT-02`) — the same tools as subcommands, for scripts and CI.
3. **Live sessions** (`AGENT-03`) — the same tools against a document someone
   has open, through the collaboration server as an ordinary participant.

Not a second engine, not a second command set: a tool is a thin, typed wrapper
over an SDK call, so an agent and a person get the same answers.

## Tools (first set)

| Tool | Does | Returns |
|---|---|---|
| `open_workbook` | Open a file under an allowed root (xlsx, xlsm, ods, csv/tsv) | handle, sheets with used ranges, the **import compatibility report** |
| `list_sheets` | Sheets, sizes, hidden/protected state | |
| `read_range` | Values, formulas, or both, for an A1 range | a bounded grid; truncation is stated, never silent |
| `write_cells` | Values or formulas into a range, read the way typed entry reads them | cells written, any refused (protection, validation) and why |
| `evaluate` | A formula against the workbook without writing it | value or error |
| `recalculate` | Full recalculation | |
| `run` | Structural edits: insert/delete rows and columns, sort, number format, add sheet, rename sheet | |
| `save` | To the original path or a new one, in a chosen format | the **save-loss report** |
| `close` | Release the handle | |

**No silent data loss** holds for agents as for people: `open_workbook` and
`save` always return the compatibility and loss reports, so an agent cannot
convert a file into something poorer without being told what was lost.

## Security bounds (part of the contract, not tuning)

- **Filesystem roots.** The server reads and writes only under roots given at
  start (`--root`, repeatable). Paths are canonicalised and refused if they
  escape a root (`..`, symlinks out). No root, no file access.
- **No network.** The tools never fetch anything; formulas that would
  (`WEBSERVICE`) are not implemented (`XL-FEAT-120`).
- **Bounds.** The SDK's existing limits apply to open (file size, cell count);
  `read_range` returns at most 10,000 cells per call; handles are capped and
  closed on exit.
- **Live sessions** (`AGENT-03`) use the collaboration server's existing token
  model: an agent is admitted only with a token a host minted, with the same
  read-only / capability rules as a person, and shows in presence as an agent.

## Our UX touch

- An agent in a live document **appears in the roster as an agent** (its own
  badge), its edits arrive as operations, converge like anyone's, and **each
  tool call is one undo step** for the person who owns the document.
- Tool results are written for a model to act on: cell addresses in A1,
  errors named as Excel names them (`#DIV/0!`), refusals with the reason.

## Phases

| Row | Scope | Gate |
|---|---|---|
| `AGENT-01` | MCP server (stdio, JSON-RPC 2.0) with the tools above, roots and bounds | An end-to-end test drives a real MCP session: open → read → write a formula → evaluate → save; a path outside the roots is refused; the reports are returned |
| `AGENT-02` | CLI over the same tools (`opencalc-agent read book.xlsx A1:C10`, `write`, `eval`, `convert`) | Each subcommand tested against a fixture |
| `AGENT-03` | Live: the tools against an open document through the collaboration server | Two-participant test: an agent's edit converges in a browser, shows in presence, and undoes in one step |
| `AGENT-04` | Editor command parity: `run` accepts any command id from the registry that has an engine-level meaning | Every registry command classified as engine / UI-only; the engine ones callable |
