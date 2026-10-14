# 99 — Macros and scripts

**Status: Proposed, 2026-10-15** — owner asked for macros (*"lets also add macros
… with UX"*). One decision below (§2) needs the owner before building. Tracker
row `XL-FEAT-038`; phases `MACRO-01..`.

## 1. What Excel users mean by "macros"

Three different things, and an alternative has to answer each:

| Need | Excel | Today in OpenCalc |
|---|---|---|
| Automate a repeated task | VBA macros (desktop), Office Scripts (TypeScript, web) | Nothing runs |
| Record what I did and replay it | Macro recorder (VBA, or Office Scripts "Record actions") | Nothing |
| My own worksheet function | VBA UDFs, LAMBDA, JS custom functions | LAMBDA works |
| Open a colleague's `.xlsm` | Runs its VBA (after a trust prompt) | Opens, keeps `vbaProject.bin` byte-for-byte, never runs it |

## 2. The decision: scripts, not a VBA runtime

**Recommended: OpenCalc Scripts — JavaScript/TypeScript in a sandbox, over one
workbook API — and VBA kept, shown and convertible, but not executed.**

- **Why not run VBA.** VBA is a full language with a COM object model (`Range`,
  `Worksheets`, `Application`, `ActiveX`, file and shell access, Windows APIs).
  An interpreter faithful enough to run real-world macros is a multi-year
  project, and the macros that matter most are exactly the ones that reach
  outside the workbook — the part that is both hardest to emulate and most
  dangerous to run in a browser or on a shared server. Microsoft made the same
  call: Excel for the web does not run VBA; it runs Office Scripts.
- **Why scripts.** A sandboxed script over a typed workbook API covers
  "automate", "record and replay" and "custom function" with one engine, runs
  the same in the browser, the desktop app and the agent server, and can be
  made safe by construction (no network, no DOM, time and memory limits).
- **One API.** The script API is the same tool set as agent access
  ([docs/97](97-AGENT-ACCESS.md)): read/write ranges, formulas, formats,
  sheets, structural edits, recalculate. A recorded macro, an agent and a
  script are three callers of one surface, not three surfaces.

**The alternative** — a VBA interpreter for a subset (cell reads/writes,
loops, `MsgBox`) — would run simple recorded macros as they are, at the cost of
a second language to secure and a long tail of "this macro almost works".
Not recommended; recorded here so the choice is explicit.

## 3. UX — Excel's shape, our engine

- **Developer / Automate.** A ribbon tab (Excel's *Automate* tab on the web):
  Record Actions, Scripts (list), New Script, Run, and View VBA for `.xlsm`.
- **Macros dialog (Alt+F8).** Excel's list: Run, Step Into (later), Edit,
  Delete, with the workbook's scripts.
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
| `MACRO-01` | Script engine: sandboxed worker, the workbook API (shared with docs/97 tools), run → one undo step, limits |
| `MACRO-02` | Scripts panel + editor, Macros dialog (Alt+F8), Automate tab; scripts stored in the workbook |
| `MACRO-03` | Record Actions → script |
| `MACRO-04` | Custom functions from scripts (`=MYFN(...)`), recalc-safe (pure, sandboxed) |
| `MACRO-05` | Read-only VBA viewer for `.xlsm`; later, a VBA → script converter for recorded-style macros |
