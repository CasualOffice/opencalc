# Next.js

Next needs two things the other examples do not, and both are worth
understanding rather than copying blindly.

## 1. The WebAssembly has to be copied into `public/`

Turbopack — Next's default bundler — does not treat `.wasm` as an emitted
asset, so `new URL("./engine.wasm", import.meta.url)` does not produce a
servable file. This is not a bug we can work around from inside the package;
it is how the bundler treats the file type.

The fix is a copy step, which is what the Next community converged on for every
wasm package:

```json
"postinstall": "opencalc-assets ./public/opencalc"
```

and then telling the element where they went:

```jsx
<opencalc-sheet assets-url="/opencalc/" />
```

**Re-run it after upgrading.** `npm update` bumps the JavaScript and leaves
whatever is in `public/` alone, so the shim and the engine can drift apart. The
SDK checks their versions at load and fails loudly naming this command, rather
than letting you debug a function that "stopped working".

The alternatives people reach for — an API route that streams the file out of
`node_modules`, or a webpack config Turbopack ignores — are worse versions of
the same idea.

## 2. It must not render on the server

The element touches `window` at import and draws to a canvas. `"use client"`
alone is not enough: a client component is still rendered on the server for the
initial HTML. `next/dynamic` with `ssr: false` is what actually keeps it out,
and the `loading` placeholder is what stops the layout jumping when it arrives.
