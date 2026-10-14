# 99 — Macros and scripts

**Status: Accepted, 2026-10-15** — owner: *"for macros choose what onlyoffice is
doing"*. §2 is decided as below. Tracker row `XL-FEAT-038`; phases `MACRO-01..`.

## 1. What Excel users mean by "macros"

Three different things, and an alternative has to answer each:

| Need | Excel | Today in OpenCalc |
|---|---|---|
| Automate a repeated task | VBA macros (desktop), Office Scripts (TypeScript, web) | Nothing runs |
| Record what I did and replay it | Macro recorder (VBA, or Office Scripts "Record actions") | Nothing |
| My own worksheet function | VBA UDFs, LAMBDA, JS custom functions | LAMBDA works |
| Open a colleague's `.xlsm` | Runs its VBA (after a trust prompt) | Opens, keeps `vbaProject.bin` byte-for-byte, never runs it |

## 2. The decision: OnlyOffice's model — JavaScript macros, VBA converted, not run

Decided by the owner: do what OnlyOffice does. OnlyOffice runs **JavaScript
macros** against its **Office API** — an object model shaped like Excel's
(`Api.GetActiveSheet().GetRange("A1").SetValue(…)`), so a VBA user recognises
`Worksheet` and `Range` — runs them from **View ▸ Macros** with an **autostart**
option, keeps them **in the document**, adds **custom functions** through
`Api.AddCustomFunction` (a JSDoc `@customfunction` comment declares the
signature; the function may be async and sees its own cell as `this.address`),
and handles **VBA by conversion, not execution** (its AI plugin, or a rewrite
against the API). References: ONLYOFFICE API docs on converting VBA macros and
on adding custom functions; the 2023 blog post on transforming Microsoft Office
macros.

**What OpenCalc adds to that model:**

- **The same API names.** We implement an OnlyOffice-compatible subset of the
  spreadsheet Office API (`Api`, `ApiWorksheet`, `ApiRange`, …, starting with
  what recorded macros use) so a macro written for OnlyOffice runs here
  unchanged, and ours runs there. Compatibility is a tested claim: a corpus of
  OnlyOffice's own published macro samples runs against both
  (`MACRO-01` acceptance).
- **One engine underneath.** The Office API is a thin layer over the same
  workbook operations the agent server uses ([docs/97](97-AGENT-ACCESS.md)), so
  a macro, a recorded action and an agent produce the same operations.
- **VBA conversion through our own agent path**: a Convert VBA command hands the
  module text (read from `vbaProject.bin`, MACRO-05) to the AI assistant the
  user configures, with the API reference as context, and shows the result as
  an editable macro — never run without the user choosing Run.

**Not doing:** a VBA interpreter. VBA reaches outside the workbook (COM,
files, shell, Windows APIs); a faithful runtime is years of work and unsafe in a
browser or on a shared server. Neither OnlyOffice nor Excel for the web runs it.

## 3. UX — Excel's shape, our engine

- **View ▸ Macros** (OnlyOffice's place) and **Alt+F8** (Excel's key) open the
  Macros window: the workbook's macros and custom functions, New, Rename,
  Delete, Run, **Autostart** per macro, and an editor beside the list.
- **Developer tab** (Excel's name) carries Macros, Record Macro, Convert VBA
  and View VBA for `.xlsm`.
- **Recorder.** Record Actions captures what the user does as script lines —
  every editor command already has an id in one registry
  (`webapp/commands.js`), and cell edits are operations — then shows the
  script, editable.
- **Editor.** A side panel with a code editor (syntax highlight, the API's
  types for completion), Run, and an output pane; errors point at the line.
- **Undo.** A script run is **one undo step**, whatever it changed.
- **Trust.** Scripts never run on open. A workbook's scripts run only after the
  user chooses Run (and once per workbook, a plain-language trust prompt).
- **Collaboration.** A script's edits are operations like any other: they
  converge, show in presence as the user who ran it, and undo in one step.

## 4. Storage and fidelity

- **Scripts live in the workbook** as a custom package part (xlsx/xlsm), so
  they travel with the file; Excel keeps unknown parts, so a round trip
  through Excel does not lose them. Named in the compatibility report when a
  target format cannot hold them (CSV, ODS).
- **VBA**: `vbaProject.bin` stays byte-for-byte (today's behaviour), and a
  **read-only VBA viewer** decodes its modules (MS-OVBA decompression) so a
  user can see the macros their file carries. VBA signatures follow
  XL-FEAT-117.

## 5. Security bounds (contract)

Sandbox per run: a Web Worker (browser/desktop) or an isolate (server), no
network, no DOM, no filesystem, a time limit and a memory limit, the workbook
API as the only capability; a runaway script is terminated and its edits
rolled back as one step. Script source is untrusted input everywhere it is
displayed.

## 6. Phases

| Row | Scope |
|---|---|
| `MACRO-01` | Macro engine: sandboxed worker running JavaScript against an OnlyOffice-compatible Office API subset (over the docs/97 operations); a run is one undo step; limits |
| `MACRO-02` | Macros window (View ▸ Macros, Alt+F8) with editor, autostart; macros stored in the workbook (read OnlyOffice's storage too, if documented) |
| `MACRO-03` | Record Actions → script |
| `MACRO-04` | Custom functions via `Api.AddCustomFunction` with JSDoc `@customfunction` (async allowed), recalc-safe and sandboxed |
| `MACRO-05` | Read-only VBA viewer for `.xlsm`, and Convert VBA → macro through the user's AI assistant |
