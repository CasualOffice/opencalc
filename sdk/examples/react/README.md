# React

```bash
npm install && npm run dev
```

`src/OpenCalcSheet.jsx` is the whole wrapper — about seventy lines, most of
them comments. Copy it into your project rather than depending on it if you
want to change how config is diffed.

## What the wrapper is actually for

The element is already framework-agnostic; React needs three specific things
from a bridge, and each is a bug in someone's wrapper right now.

**It must not remount on every render.** `ui={{ chrome: { header: false } }}` is
a *new object* on each render. An effect that depends on it and rebuilds the
element would throw away the workbook every time the parent re-renders. Config
is compared by value and applied imperatively.

**Strict Mode mounts twice.** In development `useEffect` runs, cleans up, and
runs again. Mounting must be idempotent and teardown must be real, or you get
two engines and a leak that never reproduces in production.

**Events are not synthetic.** React's event system does not carry custom DOM
events, so listeners attach directly.

## Vite needs nothing

`@opencalc/sheet` resolves its WebAssembly with `new URL(…, import.meta.url)`,
which Vite emits as a hashed asset from your own origin. No plugin, no config.
Next.js is different — see `../next`.
