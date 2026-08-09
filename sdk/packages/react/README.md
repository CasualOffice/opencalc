# @opencalc/react

**React bindings for [`@opencalc/sheet`](https://www.npmjs.com/package/@opencalc/sheet)** —
an embeddable spreadsheet that reads and writes real `.xlsx`, evaluates 347
worksheet functions, and builds pivot tables and charts entirely in the
browser.

- 📖 **[Integration guide](https://calc.casualoffice.org/docs.html)**
- 🧪 **[Live demo](https://calc.casualoffice.org/embed.html)**
- 💾 **[Runnable React example](https://github.com/CasualOffice/opencalc/tree/main/sdk/examples/react)**
- 🦀 **[Source](https://github.com/CasualOffice/opencalc)** — Apache-2.0

> **Alpha.** `0.0.x` is a preview: the API works but is not stable yet. Pin the
> version you test against.

## Install

```sh
npm install @opencalc/react
```

`@opencalc/sheet` comes with it. React 18 or 19 is a peer dependency.

## Use

```jsx
import { OpenCalcSheet } from "@opencalc/react";

export function Budget({ onChange }) {
  return (
    <OpenCalcSheet
      style={{ height: 600 }}
      engine={{ calculation: "manual", access: "edit" }}
      ui={{
        theme: { light: { accentColor: "#7c3aed" } },
        chrome: { statusbar: false },
        commands: { hidden: ["file.open"] },
      }}
      onCellsChanged={(e) => {
        if (e.source !== "api") onChange(e.range);
      }}
    />
  );
}
```

**Give it a height.** The element has no intrinsic size; left alone it
collapses to zero and looks like a failed load.

### Props

| Prop | Type | What it does |
| --- | --- | --- |
| `engine` | object | Passed to `configure()`: `calculation`, `access`, `locale`, `messages` |
| `ui.theme` | object | Theme tokens — use the `{ light, dark }` form for anything but an accent |
| `ui.chrome` | object | Show or hide whole regions |
| `ui.commands` | object | `{ hidden, disabled }` by command id |
| `ui.colorScheme` | string | `"light"`, `"dark"` or `"auto"` |
| `onReady` | function | Called with the element once the grid is up |
| `onCellsChanged` | function | `{ sheet, range, value, source }` |
| `onSelectionChanged` | function | `{ sheet, range, activeCell }` |
| `style`, `className` | — | Applied to the host element |

### The imperative API

Anything not covered by props is on the element itself, through a ref:

```jsx
const sheet = useRef(null);

await sheet.current.open(bytes, "budget.xlsx");
const saved = await sheet.current.save();

<OpenCalcSheet ref={sheet} style={{ height: 600 }} />
```

### Next.js

`"use client"` is not enough on its own — a client component is still rendered
on the server for the initial HTML, and the element touches `window` at import:

```jsx
const Sheet = dynamic(() => import("./SheetClient"), { ssr: false });
```

Turbopack also does not treat `.wasm` as an emitted asset, so copy the engine
into `public/` and point at it. See the
[`sheet` README](https://www.npmjs.com/package/@opencalc/sheet#serving-the-engine).

## Why a wrapper at all

Three things it has to get right, each of which is a bug in someone's React
wrapper right now:

1. **Not remounting on every render.** Config objects are new identities each
   render, so a naive effect tears the engine down and rebuilds it — losing the
   workbook — every time the parent re-renders. Config is applied imperatively
   and compared by value.
2. **Strict Mode's double mount.** `useEffect` runs twice in development.
   Mounting is idempotent and the cleanup is real, or you get two engines and a
   leak that only shows up in dev.
3. **Events.** React's synthetic system does not carry custom DOM events, so
   listeners are attached directly and torn down on change.

On React 19 object props reach a custom element as *properties*; on 18 and
earlier they stringify. This never passes objects as props, so it behaves the
same on both.

It is about eighty lines —
[read it](https://github.com/CasualOffice/opencalc/blob/main/sdk/packages/react/index.js)
and copy it if you want to change how config is diffed.

## License

Apache-2.0 © [CasualOffice](https://github.com/CasualOffice)
