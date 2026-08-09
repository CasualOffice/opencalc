# @opencalc/engine

**The [OpenCalc](https://github.com/CasualOffice/opencalc) spreadsheet engine as
WebAssembly, with no UI.** Open an `.xlsx`, recalculate it, edit it, write it
back — from a worker, a server route, a build step or a test. No DOM, no CSS,
no canvas.

If you want a spreadsheet your users can *see*, you want
[`@opencalc/sheet`](https://www.npmjs.com/package/@opencalc/sheet) instead;
this is the layer underneath it.

- 📖 **[Documentation](https://calc.casualoffice.org/)**
- 🏗 **[Architecture](https://github.com/CasualOffice/opencalc/blob/main/docs/02-ARCHITECTURE.md)**
- 🦀 **[Source](https://github.com/CasualOffice/opencalc)** — Rust, Apache-2.0

> **Alpha.** `0.0.x` is a preview. The binding surface is generated from the
> Rust host facade and will change; pin the version you test against.

## Install

```sh
npm install @opencalc/engine
```

## Use

```js
import init, { session_open, session_cells, session_save } from "@opencalc/engine";

await init();                                  // compile the wasm module

const key = session_open(new Uint8Array(bytes), "budget.xlsx");
const cells = JSON.parse(session_cells(key, 0, 0, 0, 20, 10));
const out = session_save(key);                 // deterministic .xlsx bytes
```

Under a bundler that resolves `new URL(…, import.meta.url)` — Vite, webpack,
Rollup, Parcel — `init()` finds the binary on its own. Elsewhere, hand it the
URL you serve it from:

```js
await init(new URL("./casual_calc_wasm_bg.wasm", import.meta.url));
```

## What it does

- **Reads and writes real `.xlsx`.** Import → edit → write is a *semantic fixed
  point*, gated by test. Anything the model does not represent is reproduced
  verbatim rather than dropped.
- **Calculates.** 347 of the 356 functions in the spec, dynamic arrays that
  spill, `LET` and `LAMBDA`, pivot tables and `GETPIVOTDATA`. Automatic or
  manual, taken from the file's own `<calcPr>`.
- **Is deterministic.** The same input and the same version produce the same
  values and the same bytes, every time — which is what makes golden-file
  regression testing possible. The clock and any seed are *supplied by the
  host*, never sampled.
- **Is safe with untrusted files.** Packages are admitted under explicit entry,
  path, size, expansion and resource limits. Macros are preserved as opaque
  bytes and **never executed**; no external references are fetched.

## Caveats

- **The binary is ~12 MB.** It carries the whole function library, the layout
  engine and the font substitution tables. Serve it compressed and cache it;
  `Content-Encoding: br` takes a large bite out of that.
- **Compiling WebAssembly needs `'wasm-unsafe-eval'`** in your `script-src` if
  you run a Content Security Policy. That keyword permits wasm and nothing
  else — it does not re-enable `eval()`.
- **The API is generated, not hand-designed.** It is a `wasm-bindgen` bridge
  built for the editor, so it is function-per-operation and passes JSON across
  the boundary. A curated TypeScript surface is planned; this is not it yet.

## License

Apache-2.0 © [CasualOffice](https://github.com/CasualOffice)
