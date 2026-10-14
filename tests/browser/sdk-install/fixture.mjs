// Build `@opencalc/sheet`, pack it, install the tarball into a throwaway app,
// and serve that app the way an integrator without a bundler would: plain
// static files, nothing stamped, nothing rewritten.
//
//   node tests/browser/sdk-install/fixture.mjs        (Playwright starts it)
//
// This is the install half of the `SDK-012` gate. The package had been
// assembled from a hand-written file list since before the editor was split
// into topic modules, so the published element imported `editor.js`, which
// re-exports `editor.core.js`, which was not in the tarball — and every check
// passed, because every check ran the editor out of `webapp/`, where all the
// files are. The only thing that can see a package missing a file is a copy
// of the package with nothing else beside it.
//
// The engine comes from `OPENCALC_WASM_DIR` when set (CI points it at the
// `webapp/pkg` the browser suite already built), otherwise from where
// `build-packages.mjs` looks by default.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(fileURLToPath(new URL("../../..", import.meta.url)));
const PORT = Number(process.env.OPENCALC_SDK_INSTALL_PORT ?? 8321);

const run = (cmd, args, cwd) =>
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });

// 1. Assemble the packages with the script that publishes them — not a copy of
//    its logic, or this would test the copy.
process.stdout.write(run(process.execPath, [path.join(ROOT, "sdk", "build-packages.mjs")], ROOT));

// 2. Pack, exactly as `release-sdk.yml` does: `npm pack` applies `files` and
//    `.npmignore`, which is part of what decides what a user receives.
//    One fixed directory, wiped at the start of each run rather than at the
//    end: Playwright stops this server with SIGKILL, so an exit handler never
//    runs, and a temp directory per run left 20 MB behind every time. Under
//    `test-results/`, it is gitignored and in CI's failure upload.
const work = path.join(ROOT, "tests", "browser", "test-results", "sdk-install-fixture");
fs.rmSync(work, { recursive: true, force: true });
fs.mkdirSync(work, { recursive: true });
const tarballs = path.join(work, "tarballs");
fs.mkdirSync(tarballs);
const packed = run("npm", ["pack", "--silent", "--pack-destination", tarballs],
  path.join(ROOT, "sdk", "packages", "sheet")).trim().split("\n").pop();
const tarball = path.join(tarballs, packed);

// 3. Install it into an app that has never seen this repository.
const app = path.join(work, "app");
fs.mkdirSync(app);
fs.writeFileSync(path.join(app, "package.json"),
  JSON.stringify({ name: "opencalc-install-fixture", private: true, version: "0.0.0" }, null, 2));
run("npm", ["install", "--no-audit", "--no-fund", "--prefer-offline", "--ignore-scripts", tarball], app);

// No bundler: an import map is how a plain page resolves the bare specifier,
// and it points into `node_modules` as installed.
fs.writeFileSync(path.join(app, "index.html"), `<!doctype html>
<html><head><meta charset="utf-8"><title>install fixture</title>
<script type="importmap">
{ "imports": { "@opencalc/sheet": "./node_modules/@opencalc/sheet/dist/embed.js" } }
</script></head>
<body style="margin:0">
  <opencalc-sheet id="sheet" style="display:block;height:600px;width:1100px"></opencalc-sheet>
  <script type="module">import "@opencalc/sheet";</script>
</body></html>
`);

process.stdout.write(`sdk-install: installed ${packed} into ${app}\n`);

// 4. Serve it statically. Correct media types — `application/wasm` matters to
//    `instantiateStreaming` — and nothing else: no stamping, no rewriting, no
//    fallback to a file from somewhere else.
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".wasm": "application/wasm",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".txt": "text/plain; charset=utf-8",
};

/// The installed package's JavaScript modules, as paths under `dist/`.
const installed = path.join(app, "node_modules", "@opencalc", "sheet", "dist");
const modules = fs.readdirSync(installed, { recursive: true })
  .filter((f) => f.endsWith(".js"))
  .map((f) => f.split(path.sep).join("/"))
  .sort();

const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://fixture");
  if (url.pathname === "/__installed") {
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(modules));
    return;
  }
  const file = path.join(app, decodeURIComponent(url.pathname));
  if (!file.startsWith(app)) {
    res.writeHead(403).end();
    return;
  }
  let target = file;
  try {
    if (fs.statSync(target).isDirectory()) target = path.join(target, "index.html");
    const body = fs.readFileSync(target);
    res.writeHead(200, {
      "content-type": TYPES[path.extname(target)] ?? "application/octet-stream",
      "cache-control": "no-store",
    });
    res.end(body);
  } catch {
    process.stdout.write(`sdk-install: 404 ${url.pathname}\n`);
    res.writeHead(404, { "content-type": "text/plain" }).end(`not in the installed package: ${url.pathname}`);
  }
});

server.listen(PORT, "127.0.0.1", () => {
  process.stdout.write(`sdk-install: serving ${app} on http://127.0.0.1:${PORT}/\n`);
});
