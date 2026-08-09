#!/usr/bin/env node
// Copy the engine into a directory the host serves itself.
//
// Needed because the WebAssembly binary and the bundled fonts must come from
// the integrator's **own origin**. Not from a CDN we run: a Web Worker cannot
// be constructed from a cross-origin URL, so shipping the engine from a CDN
// would foreclose ever moving it off the main thread — quietly, at the moment
// that is hardest to undo. The cache headers on a multi-megabyte binary should
// also belong to whoever pays for the traffic.
//
// Bundlers that resolve `new URL(..., import.meta.url)` (Vite, webpack, Rollup,
// Parcel) do not need this. Turbopack does not treat `.wasm` as an emitted
// asset, so Next.js does.
//
//   npx opencalc-assets ./public/opencalc
//
// Keep it in `postinstall`. `npm update` moves the JavaScript in node_modules
// and leaves the copy alone, and the two drifting apart is exactly what the
// version manifest written here exists to catch at load.

import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = resolve(HERE, "../dist");

const args = process.argv.slice(2).filter((a) => a !== "--");
if (args.includes("-h") || args.includes("--help") || args.length !== 1) {
  process.stdout.write(
    "Copy the OpenCalc engine into a directory you serve.\n\n" +
      "  opencalc-assets <dir>\n\n" +
      "Then point the element at it:\n\n" +
      '  <opencalc-sheet assets-url="/opencalc/"></opencalc-sheet>\n\n' +
      "Typically run from postinstall so it cannot fall behind the package:\n\n" +
      '  "postinstall": "opencalc-assets ./public/opencalc"\n',
  );
  process.exit(args.length === 1 ? 0 : 1);
}

const target = resolve(process.cwd(), args[0]);

// Refuse to write outside the project. A mistyped path here does not fail
// loudly on its own — it silently scatters twenty megabytes somewhere.
if (!target.startsWith(process.cwd())) {
  process.stderr.write(
    `opencalc-assets: refusing to write outside the project (${target}).\n`,
  );
  process.exit(1);
}

const { version } = JSON.parse(
  await readFile(resolve(HERE, "../package.json"), "utf8"),
);

await mkdir(target, { recursive: true });
await cp(DIST, target, { recursive: true });

// The manifest the element checks at load. Written last, so an interrupted
// copy leaves no claim that the directory is complete.
await writeFile(
  resolve(target, "opencalc-assets.json"),
  `${JSON.stringify({ name: "@opencalc/sheet", version }, null, 2)}\n`,
);

process.stdout.write(
  `opencalc-assets: copied @opencalc/sheet ${version} to ${target}\n` +
    `  point the element at the URL this is served from, e.g. assets-url="/opencalc/"\n`,
);
