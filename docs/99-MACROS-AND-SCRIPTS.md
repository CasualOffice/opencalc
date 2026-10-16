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

### 5a. Network access, only when the user allows it (`MACRO-13`)

Owner decision, 2026-10-17: a macro or custom function may reach the network
**only after the user is warned and allows it**, and it must not become a
security hole. Without consent, §5's "no network" holds unchanged.

- **The sandbox never holds a network primitive.** `fetch`, `XMLHttpRequest`,
  `WebSocket`, `EventSource`, `importScripts` and `navigator.sendBeacon` stay
  removed by the lockdown. What a script calls `fetch` is a stub that posts
  the request to the host page. The host checks it, asks the user, makes the
  request itself with fixed options, and returns a bounded copy of the reply.
  Untrusted code can never touch a real `fetch`, so no lockdown escape turns
  into network access.
- **The request is fixed by us, not by the script:**
  - methods: `GET`, `HEAD`, `POST`;
  - `credentials: "omit"`, so no cookies and no HTTP auth;
  - `referrerPolicy: "no-referrer"`, `cache: "no-store"`, `mode: "cors"`;
  - `redirect: "error"`, so a redirect cannot carry the request to a site the
    user did not allow;
  - headers: only `Accept` and `Content-Type`;
  - body up to 1 MB;
  - the reply is read as text up to 5 MB, with only `status`, `ok`, `text()`
    and `json()` exposed, and no response headers;
  - 10 s per request, 4 at once, and 60 a minute per workbook.
- **Which addresses can be asked for at all:**
  - `https:` only;
  - no user name or password in the URL, and only the default port;
  - no IP addresses (v4 or v6) and no single-label hosts;
  - no `localhost` and none of `.local`, `.localhost`, `.internal`, `.lan`,
    `.home.arpa` or `.test`;
  - never OpenCalc's own origin, nor the collaboration server's.

  This keeps a workbook from probing the user's local network or acting as
  them on our own server. DNS that points a public name at a private address
  cannot be detected from a page; that limit is stated, and browsers' Private
  Network Access checks are the backstop.
- **The host's policy comes first.** An embedder's Content-Security-Policy
  (`connect-src`) applies to the host's request as to any other, and is never
  worked around. A host capability `canUseNetwork` turns the feature off
  entirely, and an optional host allowlist limits it to named sites. Nothing
  is ever allowed without `canRunMacros`, or in read-only.
- **Consent, in Excel's shape.** The first request to a site pauses the call
  (a custom function shows `#BUSY!`) and shows Excel's security pattern:
  - a *Security Warning* message bar;
  - a *Security Notice* dialog naming the function or macro and the site
    (scheme and host only), saying that data from this workbook can be sent
    to it, with **Enable** and **Disable**.

  Disable makes the stub's promise reject with a `NetworkBlocked` error, so an
  unhandled call is `#VALUE!` as in Excel. Asks for the same site are
  combined into one prompt.
- **Where a grant lives.** A grant covers one site, for one workbook (its
  document id, XL-FEAT-113) and the exact sources that asked (a hash).
  - It is kept **on this device only**, never in the file and never sent to
    co-editors, so a file cannot grant itself and one user cannot grant for
    another.
  - Editing a function's or macro's source invalidates its grants.
  - Grants are listed in File ▸ Options ▸ Trust Center, where each can be
    removed.
- **Untrusted output.** A reply is data: it reaches cells only through the
  function's return value or the macro API, never as HTML, script or a
  formula.

**As built** (precision the implementation added; nothing above is relaxed):

- The stub reaches a script as the *name* `fetch` — a parameter of the
  compiled source — so `self.fetch`, `globalThis.fetch` and every prototype
  route still meet the lockdown's `SandboxViolation`. One stub per source, so
  the page knows which source asked; the page checks that against the sources
  it loaded and takes nothing else from the worker.
- "The exact sources that asked": for a macro, its source; for custom
  functions, **every** custom-function source loaded into the worker, since
  they share one global scope and any one of them can change what another
  sends. The SHA-256 is computed by the page. Editing any of them asks again,
  and the first request after an edit deletes the grants the old hash held.
- **The document id.** XL-FEAT-113's carried id is not built yet. Until it
  is, a grant is keyed by the half of its design that needs nothing in the
  file: the SHA-256 of the opened bytes, or, for a workbook that did not come
  from bytes (new, joined, recovered), an id for this tab only. This is
  narrower than the carried id — a grant does not survive a save, so the
  person is asked again — never wider.
- **The clock.** A call's or a run's time limit stops while the Security
  Notice is up (the person's time) and while its requests are in flight, the
  latter for at most 60 s in total per call or run, so a slow allowed site
  cannot hold a function open for ever.
- A macro's run ends when no request is waiting on the page, so an
  ONLYOFFICE-style `(async function () { await fetch(…) })()` is waited for;
  a rejection it leaves unhandled fails the run and nothing is applied.
- The Security Notice is a message box above any open dialog (the Macros
  window included); its focus starts on **Disable**, and Escape, the close
  button and Enter-on-Disable all answer Disable. After Disable the bar stays,
  with **Enable Content**, until the tab closes, as Excel's does.
- Grants are stored under `localStorage["opencalc.network-grants.v1"]`; the
  Trust Center is File ▸ Options ▸ Trust Center ▸ Trust Center Settings… ▸
  External Content; a Remove there applies on OK and is forgotten on Cancel,
  as Excel's Trust Center does.

## 6. Phases

| Row | Scope |
|---|---|
| `MACRO-01` | Macro engine: sandboxed worker running JavaScript against an OnlyOffice-compatible Office API subset (over the docs/97 operations); a run is one undo step; limits |
| `MACRO-02` | Macros window (View ▸ Macros, Alt+F8) with editor, autostart; macros stored in the workbook (read OnlyOffice's storage too, if documented) |
| `MACRO-03` | Record Actions → script |
| `MACRO-04` | Custom functions via `Api.AddCustomFunction` with JSDoc `@customfunction` (async allowed), recalc-safe and sandboxed |
| `MACRO-13` | Network access only after the user allows it (§5a) |
| `MACRO-05` | Read-only VBA viewer for `.xlsm`, and Convert VBA → macro through the user's AI assistant |
