// Did this run test the code in *this* working tree?
//
// Every other spec in this directory assumes the answer is yes, and until
// `CI-025` nothing checked it. Several agents run this suite on one machine;
// `serve.py` looks identical from the outside whichever checkout it serves;
// `reuseExistingServer` was `!process.env.CI`, so a local run attached to
// whatever was already on its port. A run therefore loaded *another tree's*
// editor, tested code its author had never written, and passed. Two hours went
// into a fix that was never loaded. It was found only by probing the served
// source from inside the page and recognising the other checkout's byte count.
//
// A green run that tested somebody else's code is worse than a red one:
// nothing about it invites a second look. So the question gets asked out loud,
// three ways.
//
// 1. **The bytes on the wire are the bytes on disk.** Hashed from inside the
//    page, because the page's own fetch stack is what the failure was about —
//    a Node-side check would have proved something about Node.
// 2. **The stamp reaches the modules the page never names.** `collab.js` and
//    the wasm glue are imported as `./collab.js?b=${BUILD}`; `BUILD` is
//    whatever `?v=` arrived on `editor.js`. If that tag does not move when
//    `collab.js` moves, Chrome keeps serving the module it already resolved.
// 3. **The stamp is made of bytes, not of mtimes.** Editing a module must move
//    it; a checkout that only rewrites timestamps must not. The rule this
//    replaced got both backwards.
//
// (1) and (2) need the served editor and so belong in this suite. (3) needs
// only `serve.py` and a scratch directory, but it is the property the other
// two rest on, and splitting it into a second gate file would put the halves
// of one argument in two places.

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

/// The tree this run is supposed to be testing.
const WEBAPP = path.resolve(fileURLToPath(new URL("../../webapp", import.meta.url)));

/// The same set `serve.py` calls the editor's code: top level and `pkg/`.
const CODE = /\.(js|css|html|wasm)$/;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function codeFiles(root) {
  const names = [];
  for (const folder of ["", "pkg"]) {
    const base = path.join(root, folder);
    let entries;
    try {
      entries = fs.readdirSync(base);
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!CODE.test(entry)) continue;
      if (!fs.statSync(path.join(base, entry)).isFile()) continue;
      names.push(folder ? `${folder}/${entry}` : entry);
    }
  }
  return names.sort();
}

async function identity(port) {
  const res = await fetch(`http://127.0.0.1:${port}/__opencalc__`, {
    signal: AbortSignal.timeout(10_000),
  });
  expect(res.ok, `serve.py on ${port} answers ${"/__opencalc__"}`).toBe(true);
  return res.json();
}

/// Start a `serve.py` on a port the operating system picks.
///
/// Never a fixed number: three other suites may be running on this machine,
/// and a test that reserves a port to prove a point about port collisions
/// would be a poor joke. The port comes back out of the line the script
/// prints.
function startServer(root) {
  return new Promise((resolve, reject) => {
    const proc = spawn("python3", [path.join(root, "serve.py"), "0"], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    const giveUp = setTimeout(() => {
      proc.kill("SIGKILL");
      reject(new Error(`serve.py never announced a port; it said: ${out}`));
    }, 15_000);
    proc.stdout.on("data", (chunk) => {
      out += chunk;
      const found = out.match(/http:\/\/localhost:(\d+)\//);
      if (found) {
        clearTimeout(giveUp);
        resolve({ proc, port: Number(found[1]) });
      }
    });
    proc.stderr.on("data", (chunk) => {
      out += chunk;
    });
    proc.on("error", (err) => {
      clearTimeout(giveUp);
      reject(err);
    });
  });
}

/// Rewrite a file and force its timestamp somewhere new.
///
/// The timestamp is set explicitly so the test says what it means on a
/// filesystem with coarse mtimes: the assertions below are about *bytes*, and
/// a same-second rewrite going unnoticed would be a property of the clock
/// rather than of the server.
function writeAt(file, bytes, secondsFromNow) {
  fs.writeFileSync(file, bytes);
  const when = new Date(Date.now() + secondsFromNow * 1000);
  fs.utimesSync(file, when, when);
}

test("the editor the browser loaded came from this working tree", async ({ page }) => {
  const local = {};
  for (const name of codeFiles(WEBAPP)) {
    local[name] = sha256(fs.readFileSync(path.join(WEBAPP, name)));
  }
  // A guard on the guard: an empty manifest would let every comparison below
  // pass by agreeing about nothing.
  expect(Object.keys(local).length, "the working tree has modules to check").toBeGreaterThan(10);
  expect(local["collab.js"], "collab.js is one of them").toBeTruthy();

  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  const url = new URL(page.url());
  const served = await identity(Number(url.port));

  // The load-bearing assertion. A port is not an identity: the other
  // checkout's server answered every request perfectly.
  expect(
    fs.realpathSync(served.root),
    `the server on port ${url.port} is serving another checkout`,
  ).toBe(fs.realpathSync(WEBAPP));

  // And not merely the same directory name — the same files, byte for byte,
  // fetched by the page itself rather than reported by the server about
  // itself. That is the probe that found `CI-025` in the first place, and the
  // reason it is done from in here rather than from Node: the page's own fetch
  // stack is the thing that was loading the wrong code.
  //
  // The three entry pages are excluded because the server rewrites their asset
  // tags on the way out; the manifest comparison below still covers their
  // bytes on disk. A script's import specifiers are stamped the same way, so
  // the one thing the server is known to have inserted — `?v=<stamp>` — is
  // taken back out before hashing. Nothing else is normalised: any other
  // difference from the working tree still fails.
  const fetched = await page.evaluate(async ({ names, stamp }) => {
    const hex = (buf) =>
      [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
    const out = {};
    for (const name of names) {
      // Asked for by the URL the page itself would use. A script asked for
      // without a stamp is answered with a re-export of the stamped one, which
      // is the right answer to that question and the wrong thing to hash.
      const res = await fetch(new URL(`${name}?v=${stamp}`, location.href).href, {
        cache: "no-store",
      });
      if (!res.ok) {
        out[name] = `HTTP ${res.status}`;
        continue;
      }
      if (name.endsWith(".wasm")) {
        out[name] = hex(await crypto.subtle.digest("SHA-256", await res.arrayBuffer()));
        continue;
      }
      const text = (await res.text()).split(`?v=${stamp}`).join("");
      out[name] = hex(
        await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)),
      );
    }
    return out;
  }, { names: Object.keys(local).filter((name) => !name.endsWith(".html")), stamp: served.stamp });

  const expected = Object.fromEntries(
    Object.entries(local).filter(([name]) => !name.endsWith(".html")),
  );
  expect(fetched).toEqual(expected);

  // The server's own account of the tree, including the pages it rewrites.
  const reported = Object.fromEntries(
    Object.entries(served.modules).map(([name, m]) => [name, m.sha256]),
  );
  expect(reported).toEqual(local);
});

test("every module the editor imports carries the tree's stamp", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  const url = new URL(page.url());
  const { stamp } = await identity(Number(url.port));
  expect(stamp, "serve.py reports a stamp").toMatch(/^[0-9a-f]{16}$/);

  // `BUILD` is the single value `editor.core.js` interpolates into every
  // dynamic import — `./collab.js?b=${BUILD}`, `./pkg/casual_calc_wasm.js`,
  // `./pkg/casual_calc_wasm_bg.wasm`. If it is the tree's stamp then all of
  // them move when any module's bytes move; if it is the string "dev", none of
  // them ever move, which is the state `CI-025` found.
  const build = await page.evaluate(() => globalThis.__opencalcBuild);
  expect(build, "the page's build tag is the served tree's stamp").toBe(stamp);
  expect(
    await page.evaluate(() => window.opencalcEditor?.BUILD),
    "the module the dynamic imports read agrees",
  ).toBe(stamp);

  // The assertions above are about variables. This one is about the URLs the
  // browser actually went and got: *every* script this page loaded, not only
  // the one the markup names. Before `CI-025` this list held eighteen bare
  // URLs — `editor.core.js`, every topic module — and `?b=dev` on the engine.
  const scripts = await page.evaluate(() =>
    performance
      .getEntriesByType("resource")
      .map((e) => e.name)
      .filter((n) => /\.js(\?|$)/.test(n)),
  );
  expect(scripts.length, "the page loaded its modules").toBeGreaterThan(10);
  const unstamped = scripts.filter((n) => !n.includes(`=${stamp}`));
  expect(unstamped, "every module the editor loaded carries the tree's stamp").toEqual([]);
  expect(scripts.some((n) => n.includes("editor.core.js"))).toBe(true);
  expect(scripts.some((n) => n.includes("casual_calc_wasm.js"))).toBe(true);

  // `collab.js` is loaded only once a session starts, so it is not in that
  // list — and it is the file `CI-025` was about. The chain that reaches it is
  // checked instead: the specifier in the served source, and the value it
  // interpolates.
  const core = await (await page.request.get(`/editor.core.js?v=${stamp}`)).text();
  expect(core, "collab.js is imported under the build tag").toContain(
    "./collab.js?b=${BUILD}",
  );
});

test("the stamp is made of bytes, not of timestamps", async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "opencalc-stamp-"));
  // `serve.py` takes its root from its own location, so a copy of the script
  // beside a copy of the tree is a second checkout as far as it is concerned.
  // That is also what makes this reachable at all: it is the shape of the two
  // checkouts that produced `CI-025`.
  for (const name of ["serve.py", ...codeFiles(WEBAPP).filter((n) => !n.includes("/"))]) {
    fs.copyFileSync(path.join(WEBAPP, name), path.join(tmp, name));
  }
  let running;
  try {
    running = await startServer(tmp);
    const { port } = running;

    const first = await identity(port);
    expect(fs.realpathSync(first.root), "the copy reports its own root").toBe(
      fs.realpathSync(tmp),
    );

    // Twice with nothing touched: a stamp that wanders on its own would
    // invalidate every module on every request and prove nothing when it
    // changed.
    expect((await identity(port)).stamp).toBe(first.stamp);

    const collab = path.join(tmp, "collab.js");
    const original = fs.readFileSync(collab);

    // A module the page never names, and the file `CI-025` was about. Under
    // the mtime-of-`editor*.js` rule this changed nothing at all.
    writeAt(collab, Buffer.concat([original, Buffer.from("\n// one more line\n")]), 10);
    const grown = (await identity(port)).stamp;
    expect(grown, "editing collab.js moves the stamp").not.toBe(first.stamp);

    // Same length, one bit different: the stamp is content, not size.
    const flipped = Buffer.from(original);
    flipped[flipped.length - 2] ^= 0x20;
    writeAt(collab, flipped, 20);
    const changed = (await identity(port)).stamp;
    expect(changed, "a same-size edit moves the stamp").not.toBe(first.stamp);
    expect(changed).not.toBe(grown);

    // And the page's tag moved with it, which is the half that reaches the
    // browser.
    const html = await (await fetch(`http://127.0.0.1:${port}/editor.html`)).text();
    expect(html).toContain(`editor.js?v=${changed}`);

    // The other direction, and the one an mtime rule gets wrong: a checkout, a
    // branch switch or a `touch` rewrites timestamps without changing a byte.
    // Same bytes, same stamp.
    writeAt(collab, flipped, 3600);
    expect(
      (await identity(port)).stamp,
      "a newer timestamp over identical bytes is not a new build",
    ).toBe(changed);
  } finally {
    running?.proc.kill("SIGKILL");
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
