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
(the same core runs native on Tauri). Stateless helpers:

- `version()` — engine version.
- `eval_formula(text)` — parse + evaluate a self-contained formula.
- `render_xlsx(bytes, w, h, dpi)` — import → recalc → layout viewport → PNG.
- `describe_xlsx(bytes)` — a short summary of an opened workbook.
- `function_catalog()` — the supported functions and signatures, which drives the
  editor's autocomplete and argument hint.
- `font_families()` / `font_css_stack(name)` — the families the engine renders
  faithfully, from the shared substitution table.
- `formula_ref_spans(text)` — each cell reference in formula text with its
  character span, for the editor's range finder.

Everything else is the **session** API (`session_*`): open a workbook, read the
visible cells, edit through undoable operations, recalculate, and write back.
The editor holds no model of its own — the canvas is drawn from what the session
reports, so the engine is the single source of truth for values, formatting and
geometry. That is why features that look like UI live behind `session_*` calls:

- `session_pivots` / `session_set_pivot` / `session_refresh_pivot` — the pivot
  panel sends a whole definition and redraws from what comes back, so it cannot
  drift from the model, and each change is one undo step covering both the layout
  and the figures it produced.
- `session_chart_defs` / `session_set_chart` — the same for charts, including the
  anchor, so dragging one on the canvas is an ordinary undoable edit.
- `session_calculation_mode` / `session_set_calculation_mode` /
  `session_needs_recalculation` — automatic or manual calculation, taken from the
  file's own `<calcPr>` on open.
- `session_spill_owners` — which cells outside the visible window can still be
  showing text inside it. The editor asks the engine for the visible cells; this
  is what keeps a long label from vanishing when its own column scrolls off.

Both the browser canvas and the PNG backend draw real glyphs, from bundled
metric-compatible faces (Carlito for Calibri, Liberation Sans for Arial, and so
on), so a sheet looks the same on a machine that has none of the original fonts
installed.
