#!/usr/bin/env node
// Assemble the publishable npm packages from the repository.
//
//   node sdk/build-packages.mjs
//
// The editor is developed as loose files under `webapp/` and served straight
// off disk, because a build step between typing and seeing is a tax paid on
// every iteration. This script is where that becomes a package: it is the only
// place that knows the layout `<opencalc-sheet>` expects at runtime, which is
// everything sitting flat beside `embed.js`.
//
// Expects the WebAssembly build to exist already:
//
//   wasm-pack build crates/casual-calc-wasm --release --target web --out-dir pkg
//
// It is not run from here on purpose — it takes minutes, and CI caches it
// separately from the seconds this takes.

import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WEBAPP = join(ROOT, "webapp");
// `OPENCALC_WASM_DIR` takes the engine from elsewhere — CI points it at the
// `webapp/pkg` that `tools/build-wasm.py` already built for the browser suite,
// rather than compiling the same crate twice in one job.
const WASM = process.env.OPENCALC_WASM_DIR
  ? resolve(ROOT, process.env.OPENCALC_WASM_DIR)
  : join(ROOT, "crates", "casual-calc-wasm", "pkg");
const PACKAGES = join(ROOT, "sdk", "packages");

const die = (message) => {
  process.stderr.write(`build-packages: ${message}\n`);
  process.exit(1);
};

const readJson = async (path) => JSON.parse(await readFile(path, "utf8"));

// ---------------------------------------------------------------------------
// One version, asserted rather than assumed.
//
// Three package.json files with a version field is three chances to publish a
// set that does not agree with itself — and @opencalc/react pins an exact
// @opencalc/sheet, so a mismatch is an install that cannot resolve.
// ---------------------------------------------------------------------------

const manifests = Object.fromEntries(
  await Promise.all(
    ["sheet", "react", "engine"].map(async (name) => [
      name,
      await readJson(join(PACKAGES, name, "package.json")),
    ]),
  ),
);

const version = manifests.sheet.version;
for (const [name, pkg] of Object.entries(manifests)) {
  if (pkg.version !== version) {
    die(`version mismatch: sheet is ${version} but ${name} is ${pkg.version}`);
  }
}
if (manifests.react.dependencies["@opencalc/sheet"] !== version) {
  die(
    `@opencalc/react depends on @opencalc/sheet ` +
      `${manifests.react.dependencies["@opencalc/sheet"]}, not ${version}`,
  );
}

// ---------------------------------------------------------------------------
// The engine must be built, and be the real thing.
// ---------------------------------------------------------------------------

const WASM_FILES = [
  "casual_calc_wasm.js",
  "casual_calc_wasm.d.ts",
  "casual_calc_wasm_bg.wasm",
  "casual_calc_wasm_bg.wasm.d.ts",
];

for (const file of WASM_FILES) {
  try {
    await stat(join(WASM, file));
  } catch {
    die(
      `missing ${file}. Build the engine first:\n` +
        `  wasm-pack build crates/casual-calc-wasm --release --target web --out-dir pkg`,
    );
  }
}

// `pkg/` is a build directory, not a tracked one, so whatever is sitting in it
// may be months old — and a stale engine publishes perfectly happily. Size is a
// poor test of that (an optimised build is *smaller*), so the real check is
// that the bindings still export what this release depends on. These four are
// the newest arrivals; a build that predates any of them is not this release.
const SENTINELS = [
  "session_read_only",     // access levels   (SDK-004)
  "session_spill_owners",  // text overflow   (UX-B02)
  "session_create_pivot",  // pivot tables    (PIV-01)
  "session_create_chart",  // chart authoring (CHT-01)
];
const bindings = await readFile(join(WASM, "casual_calc_wasm.js"), "utf8");
const missing = SENTINELS.filter((fn) => !bindings.includes(`export function ${fn}`));
if (missing.length) {
  die(
    `the engine in ${WASM} is stale — it does not export ${missing.join(", ")}.\n` +
      `  Rebuild it:\n` +
      `  wasm-pack build crates/casual-calc-wasm --release --target web --out-dir pkg`,
  );
}

const wasmBytes = (await stat(join(WASM, "casual_calc_wasm_bg.wasm"))).size;
if (wasmBytes < 500_000) {
  die(`casual_calc_wasm_bg.wasm is only ${wasmBytes} bytes — that is not a full build`);
}

// ---------------------------------------------------------------------------
// @opencalc/sheet — what the element loads, found by following it.
//
// This used to be a list: `editor.js`, `editor.css`, `editor.html`, the wasm
// and the fonts. It was right when it was written and wrong from the day the
// editor was split into topic modules (`MNT-005`): `editor.js` became a
// one-line re-export of `editor.core.js`, which was not on the list, so the
// published element could not load its own editor — and nothing noticed,
// because every gate ran the editor out of `webapp/`, where every file exists
// (`SDK-012`). The shell (`ribbon.js`, its model, styles and icon sprite),
// collaboration and Insert Function's lazy `fn-info.js` were missing too.
//
// So the package is now *derived*: start from the entry points and follow
// every file the code names — static imports, `import()`, `new URL(...)`,
// `fetch(...)`, a stylesheet named in a list — and every `url()` a stylesheet
// names. A module added next month ships because something imports it, not
// because somebody remembered this file. What static reading cannot see (a
// file name assembled at run time from parts) is what the install test,
// `tests/browser/sdk-install/`, is for: it installs the packed tarball into an
// empty app and boots the element from it, so a file the walk missed is a 404
// in CI rather than in a user's console.
//
// Everything sits flat beside `embed.js`, as in `webapp/`, because that is
// where `new URL(".", import.meta.url)` looks for it. `pkg/` is the one
// directory that comes from elsewhere: the engine build.
// ---------------------------------------------------------------------------

/// Where the element starts. `embed.js` is the package's entry; `editor.js` is
/// named as well because `embed.js` reaches it through a URL built at run time.
const ENTRIES = ["embed.js", "editor.js"];

/// Not loaded by the browser, so no walk finds them, and shipped on purpose:
/// `embed.d.ts` is the package's public contract (SDK-009), and the engine's
/// declarations ride along with its bindings.
const DECLARATIONS = [
  "embed.d.ts",
  ...WASM_FILES.filter((f) => f.endsWith(".d.ts")).map((f) => `pkg/${f}`),
];

/// File types the editor loads by name. A literal ending in one of these and
/// naming a file that exists is a dependency; anything else is just a string.
const ASSET = /\.(?:m?js|css|html|svg|wasm|json|woff2?|ttf|otf)$/;

/// A package path to the file it is copied from.
const sourceOf = (rel) =>
  rel.startsWith("pkg/") ? join(WASM, rel.slice("pkg/".length)) : join(WEBAPP, rel);

const isFile = async (path) => {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
};

/// Resolve `spec`, as written in package file `from`, to a package path — or
/// null when it is absolute, has a scheme, or leaves the package.
function resolveSpec(from, spec) {
  const path = spec.split(/[?#]/)[0];
  if (!path || /^[a-z][\w+.-]*:/i.test(path) || path.startsWith("/")) return null;
  const url = new URL(path, `file:///package/${from}`);
  if (!url.pathname.startsWith("/package/")) return null;
  return decodeURIComponent(url.pathname.slice("/package/".length));
}

/// Every file reference in one package file, as `{ spec, strict }`. A strict
/// reference is a module specifier or a stylesheet `url()`: if it names
/// nothing, the graph is broken and the build stops rather than shipping it.
function referencesIn(rel, text) {
  const refs = [];
  if (rel.endsWith(".js")) {
    // Comments go first. They mention file names constantly, and a comment
    // saying the page links `index.html` is not a dependency.
    const code = text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
    for (const m of code.matchAll(/(?:\bfrom|\bimport)\s*\(?\s*(["'`])([^"'`$?#]+)/g)) {
      refs.push({ spec: m[2], strict: m[2].startsWith(".") });
    }
    // Any other literal that starts with a file name: `new URL("./x", ...)`,
    // `` fetch(`editor.html?v=${BUILD}`) ``, `["editor.css", "ribbon.css"]`.
    // Not an `href=` written into markup: a link is somewhere to go, not
    // something to load, and the About box's link home is not a reason to
    // ship the home page and the whole site behind it.
    for (const m of code.matchAll(/(?<!href=)["'`]((?:\.{1,2}\/)?\w[\w./-]*)(?=[?#"'`$])/g)) {
      if (ASSET.test(m[1])) refs.push({ spec: m[1], strict: false });
    }
  } else if (rel.endsWith(".css")) {
    const code = text.replace(/\/\*[\s\S]*?\*\//g, "");
    for (const m of code.matchAll(/url\(\s*(["']?)([^"')]+)\1\s*\)|@import\s+(["'])([^"']+)\3/g)) {
      const spec = m[2] ?? m[4];
      if (!spec.startsWith("data:") && !spec.startsWith("#")) refs.push({ spec, strict: true });
    }
  } else if (rel.endsWith(".html")) {
    // What a page loads: any `src=`, and a `<link>`'s `href` — not an anchor's.
    for (const m of text.matchAll(/<(?:link\b[^>]*?\bhref|[a-z]+\b[^>]*?\bsrc)\s*=\s*"([^"#][^"]*)"/g)) {
      refs.push({ spec: m[1], strict: false });
    }
  }
  return refs;
}

const shipped = new Set();
const pending = [...ENTRIES];
const broken = [];
while (pending.length) {
  const rel = pending.pop();
  if (shipped.has(rel)) continue;
  shipped.add(rel);
  if (!/\.(?:js|css|html)$/.test(rel)) continue;
  const text = await readFile(sourceOf(rel), "utf8");
  for (const { spec, strict } of referencesIn(rel, text)) {
    const target = resolveSpec(rel, spec);
    if (target === null || shipped.has(target)) continue;
    if (await isFile(sourceOf(target))) pending.push(target);
    else if (strict) broken.push(`${rel} -> ${spec}`);
  }
}
if (broken.length) {
  die(`these references name files that do not exist:\n  ${broken.join("\n  ")}`);
}

// A font's licence travels with the font: any `LICENSE*` in a directory the
// walk shipped something from.
for (const dir of new Set([...shipped].map((rel) => dirname(rel)))) {
  if (dir === "." || dir === "pkg") continue;
  for (const name of await readdir(join(WEBAPP, dir))) {
    if (/^LICENSE/i.test(name)) shipped.add(`${dir}/${name}`);
  }
}
for (const rel of DECLARATIONS) shipped.add(rel);

const dist = join(PACKAGES, "sheet", "dist");
await rm(dist, { recursive: true, force: true });
for (const rel of [...shipped].sort()) {
  if (rel === "embed.js") continue; // written below, stamped with the version
  await mkdir(dirname(join(dist, rel)), { recursive: true });
  await cp(sourceOf(rel), join(dist, rel));
}

// `embed.js` carries the version so it can refuse to boot against an engine
// from a different release. Replacing a literal is blunt, but the alternative
// is a second file to import at runtime for one string.
const source = await readFile(join(WEBAPP, "embed.js"), "utf8");
const NEEDLE = 'const VERSION = "dev";';
if (source.split(NEEDLE).length !== 2) {
  die(`expected exactly one \`${NEEDLE}\` in webapp/embed.js to stamp`);
}
await writeFile(
  join(dist, "embed.js"),
  source.replace(NEEDLE, `const VERSION = ${JSON.stringify(version)};`),
);

// The same manifest `opencalc-assets` writes, so the package's own copy passes
// the check it applies to a copied one. Without it every default install logs
// a warning about assets it is itself shipping.
await writeFile(
  join(dist, "opencalc-assets.json"),
  `${JSON.stringify({ name: "@opencalc/sheet", version }, null, 2)}\n`,
);

// ---------------------------------------------------------------------------
// @opencalc/engine — the bindings alone.
// ---------------------------------------------------------------------------

const enginePkg = join(PACKAGES, "engine");
for (const file of WASM_FILES) {
  await cp(join(WASM, file), join(enginePkg, file));
}

// ---------------------------------------------------------------------------
// Report, so a wrong tarball is visible before it is published rather than
// after.
// ---------------------------------------------------------------------------

const sizeOf = async (path) => {
  const entry = await stat(path);
  if (!entry.isDirectory()) return entry.size;
  const names = await readdir(path);
  const sizes = await Promise.all(names.map((n) => sizeOf(join(path, n))));
  return sizes.reduce((a, b) => a + b, 0);
};

const mb = (bytes) => `${(bytes / 1_048_576).toFixed(1)} MB`;
const fontCount = [...shipped].filter((rel) => /\.(?:woff2?|ttf|otf)$/.test(rel)).length;

process.stdout.write(
  `build-packages: assembled ${version}\n` +
    `  @opencalc/sheet   ${mb(await sizeOf(dist))} unpacked ` +
    `(engine ${mb(wasmBytes)}, ${fontCount} fonts, ${shipped.size} files)\n` +
    [...shipped]
      .filter((rel) => !rel.startsWith("fonts/"))
      .sort()
      .map((rel) => `      ${rel}\n`)
      .join("") +
    `  @opencalc/engine  ${mb(await sizeOf(join(enginePkg, "casual_calc_wasm_bg.wasm")))} binary\n` +
    `  @opencalc/react   no build step\n`,
);
