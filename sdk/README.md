# `sdk/` — the browser SDK

The published packages and the examples that prove they work.

## Packages

Assembled by [`build-packages.mjs`](build-packages.mjs) and published by
[`release-sdk.yml`](../.github/workflows/release-sdk.yml) on an `sdk-v*` tag.

| Package | Contains |
| --- | --- |
| [`packages/sheet`](packages/sheet) | `@opencalc/sheet` — the editor as `<opencalc-sheet>`, plus the engine, the fonts and the `opencalc-assets` CLI |
| [`packages/react`](packages/react) | `@opencalc/react` — the React wrapper, no build step |
| [`packages/engine`](packages/engine) | `@opencalc/engine` — the wasm bindings alone, no DOM |

Each package's `dist/` is build output and is not tracked. To produce it:

```sh
wasm-pack build crates/casual-calc-wasm --release --target web --out-dir pkg
node sdk/build-packages.mjs
```

The editor itself lives in [`webapp/`](../webapp) as loose files served
straight off disk — a build step between typing and seeing is a tax paid on
every iteration. `build-packages.mjs` is the single place that knows the layout
the element expects at runtime, which is everything flat beside `embed.js`.

## Examples

Runnable examples of embedding OpenCalc, one directory each. They are small
enough to read in a sitting and complete enough to copy, and they double as
integration tests for the packaging: if the `.wasm` stops resolving under a
bundler, one of these stops working.

| Example | Shows |
| --- | --- |
| [`examples/vanilla`](examples/vanilla) | the smallest thing that works — one script tag, no build step |
| [`examples/react`](examples/react) | the React wrapper: props for config, a ref for the imperative API, events as callbacks |
| [`examples/next`](examples/next) | Next.js, including the asset copy Turbopack needs |
| [`examples/viewer`](examples/viewer) | a read-only viewer and a preview thumbnail — the two are not the same thing |

The full reference is [docs/55](../docs/55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md)
and the guide on the site.

## A note on where the WebAssembly comes from

Every example serves the engine **from its own origin**. Not from a CDN, not
from `node_modules` at runtime. Two reasons:

1. The cache headers on a multi-megabyte binary should belong to whoever pays
   for the traffic.
2. A Web Worker cannot be constructed from a cross-origin URL, so shipping from
   a CDN would foreclose ever moving the engine off the main thread — quietly,
   at the moment that is hardest to undo.

Under Vite and webpack that is automatic. Under Next.js with Turbopack it is
not, and `examples/next` shows the copy step.
