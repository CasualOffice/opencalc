# OpenCalc

[![CI](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml/badge.svg)](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Status: Alpha](https://img.shields.io/badge/status-alpha-yellow.svg)](#status)

**An Excel-compatible spreadsheet you can run anywhere.** A deterministic Rust
engine that reads, calculates and writes `.xlsx`, `.xlsm` and `.ods`, with an
editor that looks and works like Excel — ribbon, dialogs, shortcuts, the formula
bar — running entirely client-side in WebAssembly. Embed it in a web page, run it
as a desktop app, put it behind your file store, or let an AI agent drive it.

[**Live demo**](https://calc.casualoffice.org/editor.html) ·
[Site](https://calc.casualoffice.org) · [Docs](docs/00-README.md) ·
[Deployment](docs/65-RUNNING-IT.md) · [SDK](sdk/)

---

## Quick start

Two containers, and you are co-editing a spreadsheet with someone over a link:

```sh
cp .env.example .env       # then change OPENCALC_SHARED_SECRET
docker compose up --build
open http://localhost:8080
```

New spreadsheet → **Share** → send the link. A cluster (two nodes behind nginx,
coordinating through Redis) is `docker compose -f docker-compose.cluster.yml up`.

## Embed it in a web app

```sh
npm install @opencalc/sheet
```

```js
import "@opencalc/sheet";           // <opencalc-sheet style="height:600px">

const sheet = document.querySelector("opencalc-sheet");
await sheet.ready;

sheet.theme({ light: { accentColor: "#7c3aed" } });
sheet.chrome({ statusbar: false });
sheet.commands({ hidden: ["file.open"] });

await sheet.configure({ access: "view" });   // enforced in the engine, not by
await sheet.open(bytes, "budget.xlsx");      // hiding buttons

sheet.on("cellsChanged", async (e) => {
  if (e.source !== "api") persist(await sheet.save());
});
```

The editor is a custom element with a shadow root, so host CSS cannot reach in
and its CSS cannot reach out. The WebAssembly binary is served from *your*
origin — a Web Worker cannot be constructed from a cross-origin URL, and a CDN
would foreclose ever moving the engine off the main thread.

## Embed it in a Rust host

```rust
use casual_calc_sdk::{Environment, SessionConfig, WorkbookSession};

let config = SessionConfig::new()
    .with_limits(limits)          // what an untrusted upload may allocate
    .with_undo_depth(200)
    .with_environment(Environment { now: today_serial, seed });

let mut session = WorkbookSession::open_with(bytes, config)?;
```

Time and randomness are **supplied, never sampled**. An engine that reaches for
the wall clock cannot be tested, replayed, or agreed on by two hosts.

## Put it in a file store

Nextcloud, ownCloud, SharePoint, Moodle and Alfresco all install an editor the
same way — WOPI. Start the adapter and paste one URL into their settings:

```sh
docker compose --profile wopi up -d
#  →  http://your-host:8090/hosting/discovery
```

Set `OPENCALC_WOPI_ALLOWED_HOSTS` to the file stores you integrate with (it is
required), and `OPENCALC_BRAND_NAME` if it should carry your name rather than
ours. [docs/74](docs/74-WOPI-INTEGRATION.md).

## What it does

**Excel's editor** — Excel's ribbon, File page, dialogs and keyboard (Go To,
Find & Replace, Format Cells, Insert Function with argument help, Name Manager,
Insert Hyperlink, Paste Special, Goal Seek…), with Excel's message boxes and
wording. Merged cells, wrapping, rotated text, frozen panes, row and column
autofit, and an expandable formula bar you can resize. Open and save run off the
page's thread, so a large file never freezes the grid.

**Formats** — `.xlsx` round-trips as a semantic fixed point (gated), and an
opened file saved without edits comes back byte for byte. `.xlsm` keeps its VBA
project byte for byte. `.ods` reads and writes. CSV/TSV/PSV with RFC 4180
quoting. PDF export embeds subsets of only the glyphs it draws.

**Calc** — 494 built-in functions, each with an implementation behind it.
Dynamic arrays that spill and refuse (`#SPILL!`) rather than overwrite. `LET`
and `LAMBDA` with first-class function values, plus `MAP`/`REDUCE`/`SCAN`/
`BYROW`/`BYCOL`. Automatic or manual mode taken from the file's own
`<calcPr>`.

**Features** — pivot tables with eleven aggregates and `GETPIVOTDATA`; charts
written as real chart parts, not pictures; conditional formatting; data
validation; tables and autofilter; notes and comments; hyperlinks to places,
defined names and the web; number formats including sections and `[Red]`.

**Macros** — JavaScript macros against an ONLYOFFICE-compatible `Api`, stored in
the workbook, run in a sandboxed worker as one undo step. Custom worksheet
functions through `Api.AddCustomFunction`, with Excel's `#BUSY!` while they
compute. A macro reaches the network only after the user allows the site in
Excel's Security Notice. VBA is shown read-only (Developer ▸ Visual Basic,
Alt+F11) and never run.

**Every script** — Japanese, Chinese and Korean render, measure and export with
fonts that ship with it and load only when a sheet needs them, with Excel's East
Asian font names mapped. Input methods compose straight into a cell.

**Collaboration** — server-mediated OT with presence, resume and pipelining. Any
node relays to the document's leader, so no sticky sessions and a plain load
balancer works.

**Agents** — an MCP server (`server/casual-calc-mcp`) lets Claude or any MCP
client read and edit workbooks through the same operations the editor uses.
[docs/97](docs/97-AGENT-ACCESS.md).

**Desktop** — a native app for macOS, Windows and Linux (`desktop/`, Tauri)
around the same editor.

For what is supported and *how each claim was verified*, see the
[execution tracker](docs/14-EXECUTION-TRACKER.md); for where OpenCalc still
differs from Excel, and why, see [docs/96](docs/96-EXCEL-FIT-GAP.md).

## Why it is built this way

**The engine runs in the browser.** The server orders operations rather than
rendering pixels, so a client can be offline briefly without the document
freezing, and nodes scale horizontally without consistent hashing.

**Nothing is lost silently.** Unmodelled parts of a workbook — VBA, OLE objects,
form controls, custom XML — are retained byte-for-byte, and anything the model
*cannot* keep is counted and named in a compatibility report rather than dropped
quietly.

**Determinism is a gated contract.** The same input and version produce identical
model, values, layout and bytes, which is what makes regressions testable against
golden files.

## Workspace

```
crates/     16 crates: model, formula, eval, layout, text, import, export, ods, render, sdk, wasm
server/     collaboration, WOPI and MCP servers, and a worked-example host
webapp/     the browser editor
desktop/    the desktop app (Tauri)
sdk/        npm packages
docs/       design records and the execution tracker
```

Contributor guide: [AGENTS.md](AGENTS.md).

## Status

**Alpha.** The engine, editor, desktop app, embeddable SDK and co-editing stack
all run, but interfaces still move and the SDK is published at `0.0.0`.

Known gaps are tracked in [docs/14](docs/14-EXECUTION-TRACKER.md), and every
remaining difference from Excel is listed with its reason in
[docs/96](docs/96-EXCEL-FIT-GAP.md).

## License

[Apache-2.0](LICENSE). No trademark wall, no mandatory in-product attribution, no
separate licence on binaries.
