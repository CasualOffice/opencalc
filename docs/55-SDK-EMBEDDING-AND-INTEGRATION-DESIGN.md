# 55 — SDK embedding and integration: design

**Status: partly built, and overtaken in one direction.** This was written as
the design to agree before implementing, per [AGENTS.md](../AGENTS.md) — the
embed surface so far had been built ad hoc while chasing a CSS-isolation
report, and that is exactly the do-over this project does not accept.

Since then the packages named in §2 have shipped: `sdk/packages/engine`,
`sdk/packages/sheet` and `sdk/packages/react` are published at `0.0.0` by an
`sdk-v*` tag, they carry type declarations (`SDK-009`), and all five samples in
§8 exist including `sdk/examples/host-toolbar` (`SDK-010`, which also added
`run(id)`).

**§12's decisions were not answered first, and §12 keeps its text.** Its closing
sentence — *"Nothing beyond the prototype in §10 gets built until these are
answered"* — is a promise the work did not keep, not a paragraph to quietly
correct; `DOC-034` is the row. Read §10 as the state at the time of writing and
§12 as still open, with the caveat that decision 3 (renaming the `--oc-*`
tokens, *"cheap now and impossible after the first release"*) is being decided
by the calendar.

The goal, stated plainly: **someone installs a package into their React (or Vue,
Svelte, plain-HTML) project, and gets a spreadsheet.** The engine is Rust
compiled to WebAssembly, so "installs a package" has to include getting a
multi-megabyte `.wasm` binary served from *their* origin by *their* bundler,
which is the single hardest part of this and the part most wasm libraries get
wrong.

---

## 1. What comparable products do

Researched rather than recalled, because the conventions here are worth
matching: a host integrating a spreadsheet has usually integrated one before.

| Product | Mount | Configuration | Theming | Events |
| --- | --- | --- | --- | --- |
| **Univer** | `container: 'app'` — a plain element id, light DOM | `createUniver({ locale, locales, theme, presets, plugins })`; presets are pre-composed plugin bundles | theme object; light/dark with design tokens | `univerAPI.addEvent(univerAPI.Event.X, cb)`; ~8 categories, `Before*`/past-tense pairs |
| **Handsontable** | `new Handsontable(el, options)`, light DOM | one flat options object | CSS themes + variables | hooks named `before*` / `after*`; `before*` can veto by returning `false`; every hook carries a `source` saying who triggered it |
| **AG Grid** | `createGrid(el, options)` | one options object | **Theming API → CSS custom properties**, `--ag-` prefix, kebab-case, *typed suffixes*: `Color`, `Border`, `Width`, `Height`, `Padding`, `Spacing`, `Shadow`, `FontFamily`, `Duration` | `on<EventName>` options + `api.addEventListener` |
| **Player.js / Vimeo** (iframe SDKs) | `<iframe>` + `postMessage` | query params + methods | n/a | envelope `{ context, version, method, event, value, listener }`; iframe emits `ready` advertising its `methods` and `events`; `listener` id correlates responses |

Four things worth stealing outright:

1. **AG Grid's typed token suffixes.** `--ag-accent-color` tells you it takes a
   colour; `--ag-spacing` tells you it takes a length. Our tokens are currently
   `--oc-bg`, `--oc-accent` — untyped and abbreviated. Renaming is cheap now and
   impossible after the first release.
2. **Handsontable's `before*` veto and `source` argument.** A host that cannot
   cancel an edit cannot enforce its own permissions, and a host that cannot
   tell *its own* programmatic change from a user's keystroke will loop forever
   echoing its own writes back to its store.
3. **Univer's split between a preset and the plugins under it** — one line for
   the common case, full control when needed.
4. **Player.js's `ready` handshake that advertises the supported methods and
   events.** It makes version skew between a host's SDK shim and the embedded
   build detectable instead of mysterious.

**And one thing to learn from rather than steal.** AG Grid and Handsontable
both *support* shadow DOM and both carry scars from it: AG Grid has a
long-standing bug where **mouse drag range selection does not work inside a
shadow root** ([#2626](https://github.com/ag-grid/ag-grid/issues/2626)), and
each grid instance duplicates the whole stylesheet unless you point them at a
shared `themeStyleContainer`
([#7968](https://github.com/ag-grid/ag-grid/issues/7968)). Handsontable hit the
same class of problem and ended up telling users to put styles in the document
head, outside the boundary.

Both are consequences of rendering a grid as **DOM cells**: event retargeting
across the boundary breaks the drag, and per-instance `<style>` blocks multiply.
We render the grid to a **canvas** and listen on `window`, which sidesteps the
first — verified, not assumed: drag-selecting inside the prototype embed
correctly reports `5R x 4C` with live Sum/Avg/Min/Max. The second applies to us
exactly as written, and §4a says what to do about it.

Sources: [Univer installation](https://docs.univer.ai/guides/sheets/getting-started/installation),
[Univer general API](https://docs.univer.ai/guides/sheets/features/core/general-api),
[Handsontable hooks](https://handsontable.com/docs/javascript-data-grid/api/hooks/),
[AG Grid theme parameters](https://www.ag-grid.com/javascript-data-grid/theming-parameters/),
[Player.js spec](https://github.com/embedly/player.js/blob/master/SPEC.rst),
[MDN postMessage](https://developer.mozilla.org/en-US/docs/Web/API/Window/postMessage),
[vite-plugin-wasm](https://www.npmjs.com/package/vite-plugin-wasm),
[Turbopack `.wasm` handling](https://github.com/vercel/next.js/discussions/75430),
[React 19 custom-element support](https://react.dev/blog/2024/12/05/react-19),
[Custom Elements Everywhere](https://custom-elements-everywhere.com/),
[AG Grid shadow-DOM drag bug](https://github.com/ag-grid/ag-grid/issues/2626),
[AG Grid style injection in shadow DOM](https://github.com/ag-grid/ag-grid/issues/7968).

---

## 2. Packages

Three, because three different people install them.

| Package | Contains | For |
| --- | --- | --- |
| `@opencalc/engine` | the wasm binary, its glue, and a typed JS API over `session_*`. **No DOM, no CSS.** | a server-side or headless host: read a workbook, recalculate, write it back |
| `@opencalc/sheet` | the editor as a custom element, its stylesheet and fonts. Depends on `@opencalc/engine`. | anyone who wants the grid on screen |
| `@opencalc/react` | `<OpenCalcSheet />` — props → config, refs → the imperative API, hooks for events | React hosts, which is most of them |

`@opencalc/vue` and `@opencalc/svelte` are the same twenty lines each and can
follow demand rather than anticipation.

**Why the engine is separate.** A host that wants to convert `.xlsx` to JSON in
a worker should not download a grid renderer and a font pack to do it. It is
also the honest layering — the Rust workspace already keeps `casual-calc-sdk`
below the WASM bridge, and this mirrors it.

---

## 3. The hard part: shipping WebAssembly into someone else's build

This decides more of the design than anything else.

**Settled constraint: the assets are served from the host's own origin.** Not
from unpkg, not from a CDN we run, not from `node_modules` at runtime. Two
reasons, and the second is the one that makes it non-negotiable:

1. A multi-megabyte binary's cache headers should belong to whoever is paying
   for the traffic and answering for the uptime.
2. **A Web Worker cannot be constructed from a cross-origin URL.** If the engine
   is ever to run off the main thread — and for a 1M-cell recalculation it
   should, see §3d — the worker script and the wasm it instantiates have to be
   same-origin with the page. Shipping from a CDN would foreclose that, quietly,
   at the moment it is hardest to undo.

So the question is not *whether* the host serves the assets but how little work
that costs them. Two modes, both same-origin by construction.

### 3a. Default — resolved from the module URL

```js
new URL("./opencalc_bg.wasm", import.meta.url)
```

Vite and webpack 5 both understand this and emit the file as an asset with a
hashed name. It is the mode that needs no configuration, and it is what
`@opencalc/sheet` uses when the host says nothing.

**Where it fails, and we must say so in the docs rather than let people find
out:** Turbopack (Next.js's default dev bundler) does not treat `.wasm` as an
asset the way webpack did, so `import.meta.url` resolution does not produce a
servable file. Next.js is too common to treat as an edge case.

### 3b. Explicit — the host copies the assets

```js
mount(el, { assetsUrl: "/opencalc/" });
```

The package ships a `dist/assets/` directory (wasm + fonts); the host copies it
into `public/` — one line in `postinstall`, or a documented `cp`. This is the
mode we recommend for Next.js, and the one to reach for whenever the default
does not resolve.

We provide the copy step as a bin: `npx opencalc-assets ./public/opencalc`.

This is not our invention; it is what the Next.js community converged on for
every wasm package after Turbopack stopped honouring webpack's asset rules. The
alternatives people try — an API route that reads from `node_modules`, or a
webpack config Turbopack ignores — are worse versions of the same idea. Making
the copy a supported one-liner is the difference between "works on Next" and a
support thread.

### 3c. Inlined — zero configuration, at a size cost

A build variant with the wasm base64'd into the JS. It roughly quadruples parse
cost and defeats streaming compilation, so it is **not** the default and the
docs will say why. It exists because "it must work in a CodeSandbox with no
config" is a real requirement for evaluation, and an evaluation that fails at
step one never becomes an integration. Same-origin by construction, since there
is no separate file to fetch.

### 3d. Why this leaves the Web Worker open

Running the engine in a worker keeps a full recalculation off the main thread,
which is the difference between a grid that stutters on a large workbook and one
that does not. It needs the worker script and the wasm to be same-origin — which
§3's constraint guarantees — and it makes every engine call asynchronous.

Since §5e's API is promise-returning in both transports anyway, moving the
engine into a worker later does **not** change the public surface. That is the
argument for settling the origin question now and the worker question when the
performance work demands it: one is a door that closes, the other is not.

What a worker *does* change is how the canvas gets its data. Today the editor
asks the engine for the visible cells and paints them synchronously inside one
frame. Across a worker that becomes a round trip, so either the paint waits (a
frame behind) or the host keeps a small cache of the visible window and
reconciles. That is a real design job, not a flag, and it belongs to the
performance increment rather than to this one.

### Fonts

The same problem, quieter. The editor bundles metric-compatible faces (Carlito
for Calibri and the rest) so a cell renders the same on a machine that has none
of the originals. They follow `assetsUrl`. A host that would rather not ship
1.5 MB of fonts can set `fonts: false` and accept that a `.xlsx` written on a
machine with Calibri will lay out differently — stated as the trade it is.

**Open question (1):** do we also publish a single-file UMD build on a CDN
(`<script src="https://unpkg.com/@opencalc/sheet">`) for the no-bundler case?
It is the fastest possible evaluation path and the easiest thing to get subtly
wrong.

---

## 4. Mount: shadow DOM or iframe

Both are defensible; they fail differently. This is the decision I most want
agreed before building.

| | Shadow DOM (custom element) | iframe |
| --- | --- | --- |
| Host CSS cannot reach in | ✅ (with `all: initial` for inherited properties) | ✅ absolutely |
| Our CSS cannot leak out | ✅ | ✅ |
| Host JS cannot reach in | ❌ `el.shadowRoot` is open; `closed` only inconveniences | ✅ real boundary |
| A host crash takes the editor | ✅ same realm | ❌ survives |
| API cost | direct calls, shared heap | every call is async + structured-cloned |
| Passing a 6 MB `.xlsx` | a reference | a transferable `ArrayBuffer` — fine |
| Focus, keyboard, IME | native | needs care; the host's global hotkeys and ours can both fire |
| Drag-and-drop a file onto the grid | works | needs bridging |
| Printing, screenshots, PDF | in-page | awkward |
| SSR (Next.js) | needs `ssr: false`, as any canvas does | same |
| Overlay positioning | **our real cost** — see below | free, it is its own viewport |
| Drag range selection | ✅ verified in the prototype (canvas + `window` listeners) — the thing that breaks DOM-cell grids | ✅ |
| Stylesheet cost per instance | one parse per element unless shared — §4a | one document each, worse |

**The overlay cost is not hypothetical.** Building the shadow-DOM prototype hit
it twice in an hour:

- `contain: layout paint` on the host — added for isolation — makes the element
  a containing block for `position: fixed`, and every menu, tooltip and popover
  in this editor is fixed and positioned from `getBoundingClientRect()`. Every
  dropdown opened as far down the page as the element sat. Removed.
- `100vh` on the editor shell measures the *window* even inside a shadow root,
  so the editor laid itself out at full window height inside a 500 px slot.

Neither is hard once known. Both are the same class: **the shadow root shares a
viewport and a stacking context with a page we do not control**, and every
floating layer has to be right in that shared space forever. An iframe has its
own viewport and the question never arises.

### 4b. One module instance per element

The editor keeps its state at **module scope** — one engine binding, one
selection, one geometry cache. Mounting the same module into two shadow roots
therefore shares and races all of it: three elements on one page left all three
stuck at "loading engine…", because each `start()` re-entered the same
initialisation.

Each element imports its own copy of `editor.js` (a distinct URL is what makes
the module instance distinct) and passes a key down so the wasm glue is
instantiated separately too. That is what makes a page of preview thumbnails
possible at all.

It is not free: each instance is its own JS module and its own wasm linear
memory. The compiled wasm *code* is shared by the browser, so the marginal cost
is the heap, which for a thumbnail-sized workbook is small — but a page with
fifty previews should be paging them in and out rather than mounting fifty
engines, and the docs should say so.

The alternative — refactoring every module-scope binding into per-instance
state — is the better end state and a large change. This is the honest version
of "later".

### 4a. Sharing the stylesheet

The prototype injects a `<style>` into every shadow root, which is exactly the
duplication AG Grid warns about: four editors on a page parse the stylesheet
four times. `adoptedStyleSheets` with a single module-level `CSSStyleSheet`
fixes it — one parse, N roots, and the sheet stays live so a token change
propagates everywhere at once. Baseline in every browser we target; the
`<style>` fallback stays for anything older.

**Recommendation: ship both, with one API.** `mount: "shadow" | "iframe"`,
default `shadow`. The public API is promise-based either way, so switching is
one config line and a host that hits a stacking-context fight with their own
modal library has an answer that is not "sorry". The cost is one transport
adapter — real, but bounded, because §5's command/event surface is the only
thing that crosses it.

**Open question (2):** is "both" worth the second transport, or do we pick one?
My inclination is both, precisely because the failure modes are so different
and neither is rare.

---

## 5. The surface that crosses the boundary

Designed once, transport-independent. In the shadow mount these are direct
calls; in the iframe mount they are the postMessage protocol in §9. That is the
whole reason to define them as a *surface* rather than as methods.

### 5a. Commands

Everything the UI can do is a **command with a stable id**. This is what lets a
host hide or disable individual buttons rather than whole regions — the request
that started this section.

```
file.new      file.open      file.save        file.export.csv
edit.undo     edit.redo      edit.cut         edit.copy        edit.paste
format.bold   format.italic  format.numfmt    format.painter
insert.rows   insert.table   insert.chart     insert.pivot
data.sort     data.filter    data.validation  data.pivot.refresh
view.freeze   view.gridlines view.zoom
tools.calculation.auto       tools.calculation.manual          tools.calculate
```

```js
ui: {
  commands: {
    hidden:   ["file.open", "file.save"],   // gone from menus and toolbar
    disabled: ["insert.chart"],             // visible, greyed, still discoverable
  },
}
```

Hidden and disabled are different on purpose. A feature a host has not
implemented yet should be *disabled* — a user who cannot see a thing assumes it
does not exist and stops looking. A feature that makes no sense in the host's
product should be *hidden*.

`api.execute("edit.undo")` runs one by id, so a host can put our commands on
their own toolbar and skip our chrome entirely.

### 5b. Chrome regions

Coarse-grained, for laying out:

```js
ui: { chrome: { header: false, menubar: true, toolbar: true,
                formulabar: true, tabs: true, statusbar: true } }
```

The app header — brand mark, alpha badge, file button, settings gear — is
**off by default when embedded**. It is this project's demo chrome; an embedded
editor is the host's product, not ours.

### 5c. Theme tokens

Adopting AG Grid's typed-suffix convention, which means renaming what exists:

| Now | Proposed | Why |
| --- | --- | --- |
| `--oc-bg` | `--oc-background-color` | a reader can tell what it takes |
| `--oc-fg` | `--oc-text-color` | |
| `--oc-accent` | `--oc-accent-color` | |
| `--oc-border` | `--oc-border-color` | it is a colour, not a border shorthand — the current name invites `1px solid red` and silently breaks |
| `--oc-shadow-pop` | `--oc-popover-shadow` | |
| `--oc-mono` | `--oc-mono-font-family` | |

Set on the host element, where they cross the shadow boundary — custom
properties are the one thing that does, which is what makes them the theming
API rather than an implementation detail. `theme()` validates names and throws
on a typo, because a colour that silently does not change is a bad afternoon.

**Open question (3):** rename now (breaking nothing, since nothing is published)
or keep the short names? I favour renaming: this is the last cheap moment.

### 5d. Events

`before*` / past-tense pairs, `before*` cancellable, every event carrying a
`source` — Handsontable's design, and it is right for the reasons in §1.

```js
sheet.on("beforeCellChange", (e) => {
  if (e.source === "api") return;           // our own write, do not loop
  if (!user.canEdit(e.range)) e.preventDefault();
});
```

| Event | Cancellable | Payload |
| --- | --- | --- |
| `ready` | — | `{ version, methods, events }` |
| `beforeCellChange` / `cellChanged` | ✅ | `{ sheet, range, values, source }` |
| `selectionChanged` | — | `{ sheet, range, activeCell }` |
| `beforeSheetChange` / `sheetChanged` | ✅ | `{ from, to }` |
| `sheetAdded` / `sheetRemoved` / `sheetRenamed` | ✅ (before) | `{ index, name }` |
| `beforeOpen` / `opened` | ✅ | `{ name, bytes, report }` — `report` is the compatibility report, which is how a host surfaces "this file had a feature we degraded" |
| `beforeSave` / `saved` | ✅ | `{ bytes }` |
| `dirtyChanged` | — | `{ dirty }` — for the host's own "unsaved changes" guard |
| `calculationChanged` | — | `{ mode, needsRecalculation }` |
| `undoStateChanged` | — | `{ canUndo, canRedo, undoLabel, redoLabel }` |
| `commandExecuted` | ✅ (as `beforeCommand`) | `{ id, args }` |
| `error` | — | `{ code, message, detail }` |

`source` is one of `user`, `api`, `paste`, `fill`, `undo`, `redo`, `import`,
`recalc`.

**Open question (4):** `beforeCellChange` firing per edit is the useful
granularity, but a paste of 100 000 cells must not be 100 000 events. Proposal:
one event per *operation* carrying a range, not per cell — which matches how the
transaction layer already batches. Confirm that is the right granularity.

### 5e. Imperative API

```js
const sheet = await mount(el, config);

await sheet.open(bytes, "budget.xlsx");
const bytes = await sheet.save();               // .xlsx
const csv   = await sheet.export("csv");

sheet.getCell("Sheet1!B2");                     // { value, formula, format }
sheet.setCell("Sheet1!B2", "=SUM(A:A)", { source: "api" });
sheet.getRange("Sheet1!A1:D20");                // 2-D array
sheet.setRange("Sheet1!A1:D20", values);

sheet.getSelection(); sheet.select("Sheet1!A1:C3");
sheet.recalculate(); sheet.undo(); sheet.redo();
sheet.execute("insert.chart", { kind: "column" });
sheet.theme({ accentColor: "#7c3aed" });
sheet.chrome({ toolbar: false });
sheet.destroy();
```

Everything returns a promise in the iframe transport and a value in the shadow
transport — so the API is **promise-returning in both**, because an API whose
shape depends on a config flag is a trap.

### 5f. Read-only, and what it actually means

Two distinct products, and conflating them is how read-only modes end up
leaky:

One axis, `access`, rather than two booleans that can disagree:

| `access` | What it is | Chrome | For |
| --- | --- | --- | --- |
| `edit` | the editor | all | the editor |
| `view` | an **access level** — this person is working in the sheet and may not change it | all of it, minus every command that writes | a permission system's read-only |
| `preview` | a **presentation** — not a workspace | none | a thumbnail, a row in a file list, an attachment rendered inline |

The distinction is not cosmetic and conflating them produces both of the bad
outcomes in this area: a viewer that reads as a *broken editor* because it is
full of greyed-out menus, and a thumbnail that invites clicking things it will
then refuse.

`view` keeps the application: scroll, select, navigate sheets, zoom, copy, find,
follow links, expand outlines, read comments, export, print, recalculate. Only
the writing commands come off the menus.

`preview` keeps almost nothing. Selection and copy still work, because they cost
nothing and refusing them only annoys — but there is no chrome and nothing that
suggests there is anything to do here.

Preview **overrides** the host's chrome preferences rather than replacing them,
so leaving preview restores what the host had chosen. Replacing them meant
leaving preview restored "whatever the host asked for", which by then was
preview's own emptiness — the chrome never came back.

Read-only is **not** "hide the toolbar". Every one of these has to be true or
it is a suggestion rather than a mode:

- typing, Delete, paste, fill and drag-fill are refused at the point of entry,
  with the same status message the protection feature already uses;
- the clipboard still *copies* — a viewer you cannot copy out of is hostile, and
  copy changes nothing;
- undo/redo are unavailable because there is nothing to undo;
- structural edits (insert/delete row, rename sheet, add sheet) are gone;
- **the engine refuses too.** Nothing may rely on the UI alone: a host that
  calls `setCell` on a read-only session gets an error, because otherwise
  "read-only" means "read-only unless you know the API".

That last point is why this belongs in `SessionConfig` on the Rust side and not
only in `ui`. Proposal: `engine.readOnly` gates the transaction layer, and
`ui.readOnly` is derived from it rather than being a second switch that can
disagree.

Sheet protection already models something adjacent — `SheetProtection`, per-cell
`locked`, and `guard_protected` in the WASM bridge. Read-only is the
session-wide version and should reuse that refusal path rather than inventing a
parallel one, so there is one place where "this edit is not allowed" is decided.

Which commands a viewer keeps is a **whitelist**, deliberately. With a
blacklist, an editing command added later and not added to the list leaks into
read-only mode; with a whitelist the worst case is that something harmless is
hidden until someone notices. The first pass got two entries wrong in exactly
the way that argues for it: `view.*` looked safe but freezing panes, hiding
gridlines and showing formulas all live in the *workbook* and go through
`SetSheetMetadata`, which a read-only session refuses — so offering them would
offer something that then fails; and the format painter looked like a viewer
tool but reads a style from one cell and *writes* it to another.

### 5g. Localization

Two injection points, because they serve two different people.

**The host supplies it**, for a product that already knows its user's language:

```js
mount(el, {
  ui: {
    locale: "de-DE",
    // Override or extend the bundled strings. A host with its own glossary
    // ("Arbeitsmappe" vs "Datei") must be able to match it.
    messages: { "command.file.save": "Speichern", … },
  },
});
```

**The user chooses it**, from a picker in the footer — next to the sheet tabs and
the `Ready`/`Calculate` indicator, which is where a status-bar language control
belongs and where LibreOffice and Google Sheets both put theirs. Off by default,
because most hosts drive it from their own account settings and a second
language control that disagrees with the first is worse than none:

```js
ui: { chrome: { localePicker: true }, locales: ["en-US", "de-DE", "hi-IN"] }
```

Three things are localizable and they are not the same job:

| | What | Where it lives |
| --- | --- | --- |
| UI strings | menus, panels, messages | a JSON message catalogue per locale, lazily fetched from `assetsUrl` so a host ships one language or twenty |
| Number and date **display** | `1.234,56`, `31.12.2026` | the layout crate's number-format engine, which already resolves month and day names per the format's own language |
| Function names and argument separators | `SUMME` vs `SUM`, `;` vs `,` | **out of scope, deliberately** — see below |

Excel localizes function names in the UI while storing the English ones in the
file. Doing that halfway is worse than not doing it: a user who types `SUMME`
and gets `#NAME?` learns the feature is broken, and a file that stores a
localized name is unreadable everywhere else. It is a whole increment (a
per-locale name table, a parser that accepts both, a printer that emits the
user's language and a writer that emits English) and it should be one, not a
side effect of shipping a message catalogue.

`ui.locale` also drives `dir="rtl"` for Arabic and Hebrew, which the grid has to
honour in cell alignment and in the column order — another reason it is engine
configuration and not a stylesheet.

### 5h. Extension points

- `registerFunction(name, arity, fn)` — a host's own worksheet function. Needs a
  decision: a JS callback means the engine is no longer deterministic or
  replayable, which is a first-principles guarantee of this project. Proposal:
  allow it, mark any workbook using one as *host-extended* in the compatibility
  report, and refuse to treat such a recalculation as reproducible.
- `registerCommand(id, handler)` — put a host action in our menus.

**Open question (5):** custom functions vs determinism. I lean toward shipping
it with the caveat recorded in the report, because every competitor has it and
the alternative is that hosts fork the engine.

---

## 6. Configuration, whole

```js
mount(element, {
  mount: "shadow",                 // | "iframe"
  assetsUrl: "/opencalc/",         // §3b; omit for import.meta.url resolution

  engine: {                        // mirrors Rust SessionConfig exactly
    readOnly: false,               // refused in the engine, not just the UI — §5f
    calculation: "auto",           // | "manual" | null → take it from the file
    environment: { now: 45888, seed: 7 },
    undoDepth: 200,
    limits: { maxXmlElements: 5_000_000, maxXmlDepth: 128 },
  },

  ui: {
    chrome: { header: false },
    commands: { hidden: ["file.open"], disabled: [] },
    theme: { accentColor: "#7c3aed" },
    colorScheme: "auto",           // | "light" | "dark"
    locale: "en-US",
  },

  on: { cellChanged, selectionChanged },   // or sheet.on(...) later
});
```

`engine` deliberately uses the same names as the Rust `SessionConfig`, so the
JS docs and the Rust docs describe one thing.

---

## 7. React

```jsx
import { OpenCalcSheet } from "@opencalc/react";

<OpenCalcSheet
  ref={ref}
  style={{ height: 600 }}
  engine={{ calculation: "manual" }}
  ui={{ chrome: { header: false }, theme: { accentColor: "#7c3aed" } }}
  onCellChanged={(e) => save(e)}
  onReady={(api) => api.open(bytes)}
/>
```

**React 19 changed what this wrapper has to do.** It passes Custom Elements
Everywhere: a prop that matches a property on the element instance is assigned
as a *property* rather than stringified into an attribute, so `engine={{…}}`
and `ui={{…}}` arrive as objects. On React 18 and earlier they do not — a
non-primitive prop becomes `[object Object]` — so the wrapper marshals
explicitly there. Events need a listener either way, because React's synthetic
system does not carry custom DOM events. The wrapper therefore has one code
path that is thin on 19 and does real work on 18, and the support matrix says
so rather than quietly misbehaving on the older one.

Three more things it must get right, each of which is a bug in someone's React
wrapper right now:

1. **Not remounting on every render.** Config objects are new identities each
   render; the wrapper diffs by value and calls the imperative API rather than
   tearing the element down.
2. **SSR.** The element touches `window` at import. The package exports a
   client-only entry and the docs show `next/dynamic` with `ssr: false`.
3. **Strict Mode double-mount.** `useEffect` runs twice in development; mounting
   two engines and leaking one is the classic symptom. Mount is idempotent and
   `destroy()` is real.

---

## 8. What ships where

- **`sdk/` in this repo** — runnable samples, one directory each:
  `sdk/examples/vanilla`, `sdk/examples/react`, `sdk/examples/next` and
  `sdk/examples/viewer` (view-only and preview on one page, because the
  difference is easier to see than to explain). Small enough to read, complete
  enough to copy. They double as integration tests for the packaging.

  A fifth was named here and never built: a host-toolbar sample putting *our*
  commands on *their* buttons with all our chrome off. `commands()` and
  `chrome()` both exist, so it is buildable — it is tracked as `SDK-010` rather
  than quietly dropped from this list, because an integrator reading it was
  promised something.
- **A documentation page on the marketing site** — `webapp/docs.html` : install,
  quick start, configuration, theming, events, API reference, framework guides,
  the wasm-serving recipes per bundler. Generated from the same source as this
  design where it can be, so the two cannot drift.
- **This document** — the *why*. The docs page is the *how*.

---

## 9. The postMessage protocol (iframe transport)

Modelled on Player.js, which solved this in public a decade ago.

### Envelope

```jsonc
{ "oc": 1, "id": 17, "type": "call",   "name": "setCell", "args": [...] }
{ "oc": 1, "id": 17, "type": "result", "value": ... }
{ "oc": 1, "id": 17, "type": "error",  "error": { "code": "OC-…", "message": "…" } }
{ "oc": 1,           "type": "event",  "name": "cellChanged", "value": {...} }
```

- `oc: 1` is the protocol version and the discriminator. **Every listener drops
  anything without it**, because a page is full of other people's messages and
  a shared pipe that trusts its input is an XSS.
- `id` correlates a result with its call. Monotonic per frame.
- Cancellable events are a **call in the other direction**: the frame sends
  `{ type: "event", name: "beforeCellChange", id }` and waits for
  `{ type: "result", id, value: { prevented: true } }` with a timeout, after
  which it proceeds. A host that hangs must not freeze the grid.

### Handshake

1. Host creates the iframe with `?origin=<host origin>`.
2. Frame posts `{ oc: 1, type: "event", name: "ready", value: { version, protocol: 1, methods: [...], events: [...] } }` to that origin — **never `*`**.
3. Host validates `event.origin` against the frame's own origin and only then
   begins calling.

Advertising `methods` and `events` is what makes version skew between a host's
pinned SDK shim and a newer embedded build detectable rather than mysterious.

### Origin discipline

- The host passes its origin in; the frame replies only to that origin.
- The host checks `event.origin` *and* `event.source === iframe.contentWindow`
  on every message. Origin alone is not enough when several frames share one
  origin.
- No `*`, in either direction, ever.

**Open question (6):** does the iframe transport need to support a *cross-origin*
deployment (frame served from `cdn.opencalc.dev`), or only same-origin? Cross-
origin is the stronger isolation story and the harder one: no file drag-and-drop
without bridging, and clipboard permissions get involved.

---

## 10. What is already true

Built while chasing the CSS report, before this document existed. Listed so the
proposal is not confused with the state of the tree:

- `SessionConfig` in the Rust SDK: limits, calculation mode, environment, undo
  depth; `open_with` honours the file's own `<calcPr calcMode>`.
- `session_calculation_mode` / `session_set_calculation_mode` /
  `session_needs_recalculation`, and a Tools ▸ Calculation menu with a
  `Calculate` indicator in the status bar.
- Every editor token namespaced `--oc-*` (short names — §5c proposes renaming).
- `setMountRoot()` in `editor.js`: every DOM lookup goes through a mount root,
  so the same code runs as a page or inside a shadow root.
- `<opencalc-sheet>` in `webapp/embed.js` with `theme()`, `chrome()`,
  `setColorScheme()`, `configure()`, `open()`, `save()`.
- `webapp/embed.html`: the editor inside a page that sets Comic Sans, RTL,
  uppercase, magenta borders and `--bg: hotpink`, none of which reaches in.

Two defects found and fixed there, both recorded in §4 because they are
evidence about the transport choice rather than incidents: `contain` breaking
fixed-position overlays, and `100vh` measuring the window inside a shadow root.

---

## 11. Settled, so it is not re-litigated

- **Telemetry: out of scope for the SDK.** It belongs only to a server-hosted
  co-editing deployment, which is a different product with a different consent
  story. Nothing in the embed phones home.
- **Luckysheet: not researched, and does not need to be.** It became Univer,
  which is in §1.
- **Assets are same-origin.** §3, and the reason is the worker in §3d.
- **Keeping an integration current is the integrator's job**, not ours. No
  auto-update, no compatibility shims for old versions, no phoning home to
  check. We publish versions and a changelog; they choose when to take one.

  One narrow exception, and it is a footgun the copy-to-public strategy in §3b
  creates rather than a maintenance service: `npm update` bumps the JS but
  leaves whatever was copied into `public/` alone, so a host can end up running
  a new shim against last release's wasm. The engine's version is checked
  against the shim's at load and a mismatch is a loud error naming the fix
  (`npx opencalc-assets`), because the alternative is a bug report about a
  function that "stopped working". That is the whole of what the §9 handshake's
  version field is for — not skew management, just catching a stale copy.

---

---

## 12. Decisions needed

1. ~~CDN single-file build, or bundler-only?~~ **Answered: no CDN.** Assets are
   same-origin with the host, because a Web Worker cannot be built from a
   cross-origin URL and shipping from a CDN would foreclose the worker before
   we get to choose. §3.
2. Both transports, or pick one?
3. Rename the theme tokens to typed names now?
4. One change event per operation-with-range, rather than per cell?
5. Custom JS functions, given they break replayable determinism?
6. ~~Cross-origin iframe deployment in scope?~~ **Answered: no.** A cross-origin
   frame means *we* host it, which is the thing §3 rules out. The iframe
   transport, if built, is served from the host's own origin like everything
   else — so it is an isolation boundary, not a hosting model.
7. Is `preview` a mode, or a preset over `readOnly` + no chrome?
8. **Web Worker: now or later?** Same-origin assets (§3) keep the door open, and
   the promise-returning API means moving the engine into a worker does not
   change the public surface. The open part is *when*: it changes how the canvas
   gets its visible cells, which is a design job rather than a flag. §3d argues
   for later, with the performance increment. Confirm.
9. Localized **function names** (`SUMME`) — out of scope for now, per §5g?

Nothing beyond the prototype in §10 gets built until these are answered.
