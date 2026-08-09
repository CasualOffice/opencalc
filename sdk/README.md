# `sdk/` — integration examples

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
