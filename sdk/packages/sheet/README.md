# @opencalc/sheet

**A real spreadsheet in your application, as one custom element.** Reads and
writes `.xlsx`, evaluates 347 worksheet functions, builds pivot tables and
charts — and does all of it in your user's browser, with no server and no
network call.

The engine is [OpenCalc](https://github.com/CasualOffice/opencalc): a
deterministic spreadsheet engine written in Rust and compiled to WebAssembly.
This package is the browser SDK for it.

- 📖 **[Integration guide](https://calc.casualoffice.org/docs.html)** — the full reference
- 🧪 **[Live demo](https://calc.casualoffice.org/embed.html)** — theming, chrome and access levels, changed live
- 💾 **[Runnable examples](https://github.com/CasualOffice/opencalc/tree/main/sdk/examples)** — vanilla, React, Next.js, read-only viewer
- 🦀 **[Source](https://github.com/CasualOffice/opencalc)** — Apache-2.0

> **Alpha.** `0.0.x` is a preview: the API described here works, but it is not
> stable yet and a rename is a minor version. Pin the version you test against.

## Install

```sh
npm install @opencalc/sheet
```

## Use

```html
<opencalc-sheet id="sheet" style="height: 600px"></opencalc-sheet>
```

```js
import "@opencalc/sheet";

const sheet = document.getElementById("sheet");
await sheet.ready;

await sheet.open(bytes, "budget.xlsx");   // an ArrayBuffer or Uint8Array
const saved = await sheet.save();         // .xlsx bytes back out
```

**It has no intrinsic height.** Give it one — a pixel height, a flex child that
stretches, or `height: 100%` inside a sized parent. Left alone it collapses to
zero and looks like a failed load.

The element gives itself a shadow root, so your stylesheet cannot reach into it
and its stylesheet cannot reach out onto your page.

## Serving the engine

The WebAssembly binary and the bundled fonts are served **from your own
origin** — never from a CDN we run. Two reasons, and the second is the one that
matters: the cache headers on a multi-megabyte binary should belong to whoever
pays for the traffic, and a Web Worker cannot be constructed from a
cross-origin URL, so a CDN would foreclose ever moving the engine off the main
thread.

**Vite, webpack, Rollup, Parcel** — nothing to do. The package resolves its
assets with `new URL(…, import.meta.url)`, which these bundlers emit as hashed
files from your build output.

**Next.js** — Turbopack does not treat `.wasm` as an emitted asset, so copy the
assets and point at them:

```jsonc
// package.json
"scripts": { "postinstall": "opencalc-assets ./public/opencalc" }
```

```html
<opencalc-sheet assets-url="/opencalc/"></opencalc-sheet>
```

Keep it in `postinstall`: `npm update` moves the JavaScript and leaves
`public/` alone. If the two drift, the element **refuses to start and names
this command** rather than letting you debug a function that "stopped working".

## Configure

```js
await sheet.configure({
  calculation: "auto",   // | "manual" — default: whatever the file asks for
  access: "edit",        // | "view" | "preview"
  locale: "de-DE",
  messages: { "de-DE": { "command.file": "Datei" } },
});
```

`calculation` is taken from the file's own `<calcPr>` unless you override it —
a workbook saved with calculation off opens that way, because its author turned
it off for a reason.

### Theming

Themes are CSS custom properties, the one thing that crosses a shadow boundary
— which is what makes them the API rather than an implementation detail. Names
are typed by suffix: `Color` takes a colour, `Shadow` a box-shadow,
`FontFamily` a font stack.

```js
sheet.theme({ accentColor: "#7c3aed" });          // both schemes

sheet.theme({                                     // per scheme
  light: { backgroundColor: "#fbf9f4" },
  dark:  { backgroundColor: "#17150f" },
});

sheet.setColorScheme("dark");                     // | "light" | "auto"
sheet.resetTheme();
```

**Use the per-scheme form for anything but an accent.** Tokens are written as
inline custom properties, and an inline style beats every rule in the
stylesheet — including the dark-mode block. Set `backgroundColor` once and you
have set it for dark mode too.

### Chrome and commands

```js
sheet.chrome({ toolbar: false, statusbar: false });
// header · menubar · toolbar · formulabar · tabs · statusbar · localePicker

sheet.commands({
  hidden: ["file.open", "insert.pivottable"],
  disabled: ["insert.chart"],
});

await sheet.listCommands();   // every id
```

Hidden and disabled differ on purpose. A capability you have not implemented
yet should be **disabled** — someone who cannot see a thing assumes it does not
exist and stops looking. One that makes no sense in your product should be
**hidden**.

Ids come from the English label path: `Format ▸ Alignment ▸ Left` is
`format.alignment.left`. They stay English so translating never renumbers them.

### Read-only, and preview

Two different things, and conflating them gives you both bad outcomes at once:
a viewer that reads as a broken editor because it is full of greyed-out menus,
and a thumbnail that invites clicking on things it will refuse.

|                          | `view`                          | `preview`                |
| ------------------------ | ------------------------------- | ------------------------ |
| What it is               | an access level                 | a presentation           |
| Chrome                   | all of it, minus what writes    | none                     |
| Select and copy          | yes                             | yes                      |
| Zoom, sheets, find, export | yes                           | no                       |
| For                      | a permission system's read-only | a thumbnail, a list row  |

Both refuse writes **in the engine**, not by hiding buttons. A read-only mode
enforced only in the UI is read-only right up until somebody calls the API.

## Events

```js
const stop = await sheet.on("cellsChanged", (e) => {
  if (e.source === "api") return;      // our own write — do not loop
  save(e.range);
});

sheet.on("beforeCellsChanged", (e) => {          // before* can be cancelled
  if (!user.canEdit(e.range)) e.preventDefault();
});
```

| Event | Cancellable | Payload |
| --- | --- | --- |
| `beforeCellsChanged` / `cellsChanged` | before only | `{ sheet, range, value, source }` |
| `selectionChanged` | — | `{ sheet, range, activeCell }` |
| `calculationChanged` | — | `{ mode, needsRecalculation }` |
| `undoStateChanged` | — | `{ canUndo, canRedo }` |

**Always check `source`.** A host that persists on change and loads on mount
will echo its own writes back to itself forever without it. It is one of
`user`, `api`, `paste`, `fill`, `undo`, `redo`, `import`.

One event per *operation*, carrying a range — a paste of a hundred thousand
cells is one event, not a hundred thousand.

## React

```sh
npm install @opencalc/react
```

```jsx
import { OpenCalcSheet } from "@opencalc/react";

<OpenCalcSheet
  style={{ height: 600 }}
  engine={{ calculation: "manual" }}
  ui={{ theme: { light: { accentColor: "#7c3aed" } } }}
  onCellsChanged={handleChange}
/>
```

Vue and Svelte need no wrapper — both pass objects to custom elements as
properties and listen for DOM events natively.

## Content Security Policy

```
script-src  'self' 'wasm-unsafe-eval';
style-src   'self' 'unsafe-inline';
font-src    'self';
connect-src 'self';
```

`'wasm-unsafe-eval'` is the narrow keyword that permits WebAssembly compilation
and nothing else — it does **not** re-enable `eval()`. To avoid
`style-src 'unsafe-inline'`, pass a nonce (`<opencalc-sheet nonce="…">`, which
covers the hoisted `@font-face` block) and set theme tokens from your own
stylesheet instead of `theme()`, since nonces do not apply to style attributes:

```css
opencalc-sheet { --oc-accent-color: #7c3aed; }
```

## Known limits

- **Each element is its own engine.** Three thumbnails are fine; fifty should be
  paged in and out rather than mounted at once.
- **No collaborative editing.** This is a single-user view-and-edit surface.
- **A pivot created here exports as its cells**, not as a live Excel pivot.
  Figures, formatting and layout are correct and open anywhere.
- **No custom worksheet functions yet.** A JavaScript callback would make
  recalculation non-reproducible, which is a guarantee we are not ready to give
  up quietly.
- **Localization covers menus, submenus and toolbar tooltips.** Panels, dialogs
  and status messages are still English.

## License

Apache-2.0 © [CasualOffice](https://github.com/CasualOffice)
