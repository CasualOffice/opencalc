# OpenCalc

[![CI](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml/badge.svg)](https://github.com/CasualOffice/opencalc/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Status: Alpha](https://img.shields.io/badge/status-alpha-yellow.svg)](#status)

**An embeddable spreadsheet engine in Rust.** Reads and writes `.xlsx`, calculates
it, renders a live grid, and runs the whole engine client-side in WebAssembly —
so co-editing scales without sticky sessions and the editor keeps working when
the network hiccups.

[**Live demo**](https://casualoffice.github.io/opencalc/editor.html) ·
[Docs](docs/00-README.md) · [Deployment](docs/65-RUNNING-IT.md) ·
[SDK](sdk/)

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

**Formats** — `.xlsx` round-trips as a semantic fixed point (gated). CSV/TSV/PSV
with RFC 4180 quoting. Reads what other writers actually emit: OOXML booleans in
either spelling, theme + tint and legacy indexed colours, shared formulas, and
multi-area `sqref`.

**Calc** — 347 of the spec's 356 functions. Dynamic arrays that spill and refuse
(`#SPILL!`) rather than overwrite. `LET` and `LAMBDA` with first-class function
values, plus `MAP`/`REDUCE`/`SCAN`/`BYROW`/`BYCOL`. Automatic or manual mode taken
from the file's own `<calcPr>`.

**Features** — pivot tables with eleven aggregates and `GETPIVOTDATA`; charts
written as real chart parts, not pictures; conditional formatting; data
validation; tables and autofilter; comments; number formats including sections
and `[Red]`.

**Editor** — a WASM canvas grid: shared inline/formula-bar editing, autocomplete
and argument hints, reference picking, F4 anchor cycling, a range finder,
find & replace, multi-sheet tabs, undo/redo.

**Collaboration** — server-mediated OT with presence, resume and pipelining. Any
node relays to the document's leader, so no sticky sessions and a plain load
balancer works.

For what is supported and *how each claim was verified*, see the
[support matrix](docs/18-SUPPORT-MATRIX.md).

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
crates/     15 crates: model, formula, eval, layout, import, export, render, sdk, wasm
server/     collaboration server, and a worked-example host
webapp/     the browser editor
sdk/        npm packages
docs/       design records and the execution tracker
```

Contributor guide: [AGENTS.md](AGENTS.md).

## Status

**Alpha.** The engine, editor and embeddable SDK are live and the co-editing
stack runs, but interfaces still move and the SDK is published at `0.0.0`.

Known gaps are tracked in [docs/14](docs/14-EXECUTION-TRACKER.md) — notably no
PDF export, no ODS, and no WOPI integration yet.

## License

[Apache-2.0](LICENSE). No trademark wall, no mandatory in-product attribution, no
separate licence on binaries.
