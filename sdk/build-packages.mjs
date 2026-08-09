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
const WASM = join(ROOT, "crates", "casual-calc-wasm", "pkg");
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
// @opencalc/sheet — everything flat beside embed.js, because that is where
// `new URL(".", import.meta.url)` looks for it.
// ---------------------------------------------------------------------------

const dist = join(PACKAGES, "sheet", "dist");
await rm(dist, { recursive: true, force: true });
await mkdir(join(dist, "pkg"), { recursive: true });

for (const file of ["editor.js", "editor.css", "editor.html"]) {
  await cp(join(WEBAPP, file), join(dist, file));
}
for (const file of WASM_FILES) {
  await cp(join(WASM, file), join(dist, "pkg", file));
}
await cp(join(WEBAPP, "fonts"), join(dist, "fonts"), { recursive: true });

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
const fontCount = (await readdir(join(dist, "fonts"))).length;

process.stdout.write(
  `build-packages: assembled ${version}\n` +
    `  @opencalc/sheet   ${mb(await sizeOf(dist))} unpacked ` +
    `(engine ${mb(wasmBytes)}, ${fontCount} fonts)\n` +
    `  @opencalc/engine  ${mb(await sizeOf(join(enginePkg, "casual_calc_wasm_bg.wasm")))} binary\n` +
    `  @opencalc/react   no build step\n`,
);
