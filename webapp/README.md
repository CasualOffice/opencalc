# webapp — OpenCalc marketing site & WebAssembly demo

A zero-server static site: a landing page plus a live demo that runs the engine
as WebAssembly (formula evaluator + open-`.xlsx`→render). CI builds and deploys
it to GitHub Pages via [`.github/workflows/pages.yml`](../.github/workflows/pages.yml).

## Build & run locally

Requires [`wasm-pack`](https://drager.github.io/wasm-pack/) and the
`wasm32-unknown-unknown` target.

```sh
# Compile the engine to WebAssembly into webapp/pkg/
wasm-pack build crates/casual-calc-wasm --target web --out-dir "$PWD/webapp/pkg"

# A sample workbook for the "Load sample" button:
cp fixtures/generated/minimal.xlsx webapp/sample.xlsx

# Serve (module scripts need HTTP, not file://):
python3 webapp/serve.py    # http://localhost:8099
```

`webapp/pkg/` and `webapp/sample.xlsx` are build artifacts and are git-ignored.

## What the demo exposes

The `casual-calc-wasm` bridge is a thin transport over the host-agnostic engine
(the same core runs native on Tauri):

- `version()` — engine version.
- `eval_formula(text)` — parse + evaluate a self-contained formula.
- `render_xlsx(bytes, w, h, dpi)` — import → recalc → layout viewport → PNG.
- `describe_xlsx(bytes)` — a short summary of an opened workbook.

Cell **text** is rendered as highlighted regions for now (glyph shaping is in
progress); the formula evaluator and the import→calc→render pipeline are real.
