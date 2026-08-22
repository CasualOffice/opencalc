// The editor's entry point, and the only file the page names directly.
//
// It owns the cache-buster and nothing else. `editor.html`'s script tag carries
// a `?v=` that the dev server stamps, and a module's *sub*-imports carry no
// query at all — so when the editor was split into topic modules (`MNT-005`),
// the page resolved `./editor.js?v=38` while those modules resolved
// `./editor.core.js`. Two URLs are two module instances: every top-level
// listener registered twice, one keystroke committed two edits, and a single
// undo popped a phantom and looked like it had done nothing.
//
// Keeping the tag here means `editor.core.js` has exactly one URL. The name
// stays `editor.js` because it is the public one: `editor.html` names it and
// the browser gates find the module by it.
globalThis.__opencalcBuild =
  new URL(import.meta.url).searchParams.get("v") || "dev";

export * from "./editor.core.js";
