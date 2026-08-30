// The browser-smoke gate's configuration.
//
// Serves the webapp with the project's own `serve.py` rather than a test-only
// static server. That script exists because a plain server was not enough —
// Chrome reuses an ES module across a same-URL navigation, so an edited
// `editor.js` kept running its old code — and a gate that serves the app
// differently from the way it is developed is a gate that can pass on a build
// nobody can reproduce.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig, devices } from "@playwright/test";

/// Nothing else in the project listens here; 8099 is `serve.py`'s own default
/// and is often already running on a developer's machine.
const PORT = Number(process.env.OPENCALC_SMOKE_PORT ?? 8123);
/// The collaboration gate's two extra servers. Kept in step with
/// `collab.spec.mjs`, which reads the same variables with the same defaults.
const ORIGIN_PORT = Number(process.env.OPENCALC_ORIGIN_PORT ?? 8124);
const COLLAB_PORT = Number(process.env.OPENCALC_COLLAB_PORT ?? 8125);
/// A second collaboration server, configured to let go of an idle document
/// almost immediately. Its own process rather than a shorter eviction on the
/// one above, because eviction is global to a server: tuning the shared one
/// down to seconds would have every other test racing a timer it does not care
/// about, and a gate that makes twenty unrelated tests flaky to make one
/// possible is a bad trade.
const EVICT_PORT = Number(process.env.OPENCALC_EVICT_PORT ?? 8126);
const SECRET = process.env.OPENCALC_TEST_SECRET ?? "browser-smoke-only-not-a-deployment-secret";

/// This checkout's `webapp/`, which is the tree the run is supposed to test.
const WEBAPP = path.resolve(fileURLToPath(new URL("../../webapp", import.meta.url)));

/// What is on a port, if anything.
///
/// `null` means nothing is listening. An object means something is, and it is
/// whatever `serve.py`'s identity endpoint said — `{}` when the occupant is
/// listening but is not our server, or is not answering.
async function occupant(port) {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/__opencalc__`, {
      signal: AbortSignal.timeout(2000),
    });
    return res.ok ? await res.json() : {};
  } catch (err) {
    // Nothing accepted the connection: the port is free, which is the only
    // outcome that lets the run proceed. Everything else — a timeout, a socket
    // hangup, a body that is not JSON — is an occupied port.
    const code = err?.cause?.code ?? err?.code;
    if (code === "ECONNREFUSED" || code === "ECONNRESET") return null;
    return {};
  }
}

function sameDir(a, b) {
  if (!a || !b) return false;
  try {
    return fs.realpathSync(a) === fs.realpathSync(b);
  } catch {
    return path.resolve(a) === path.resolve(b);
  }
}

/// Refuse to run against a server this run did not start.
///
/// `reuseExistingServer` used to be `!process.env.CI`, so a local run attached
/// to whatever was already on its port. Several agents run this suite on one
/// machine, `serve.py` looks the same from the outside whichever checkout it
/// serves, and the module URLs did not move when a module changed — so a run
/// silently loaded *another tree's* editor and passed. Two hours went into a
/// fix that was never loaded (`CI-025`). A green run that tested somebody
/// else's code is worse than a red one, because nothing about it invites a
/// second look.
///
/// `reuseExistingServer: false` below is the enforcement — Playwright will not
/// attach to a stranger. This is the *message*: Playwright's own refusal ends
/// with "or set reuseExistingServer:true", which is the one thing that must
/// not be done here, and it cannot say whose server it found.
///
/// Runs at config load, because the web servers are started before
/// `globalSetup` and a check that runs after them is a check that has already
/// let the run attach. Skipped in worker processes, which load this same file
/// while our own servers are up and would otherwise all refuse.
async function refuseForeignServers() {
  const ports = [
    ["OPENCALC_SMOKE_PORT", PORT, "the editor (serve.py)"],
    ["OPENCALC_ORIGIN_PORT", ORIGIN_PORT, "the integrator's origin"],
    ["OPENCALC_COLLAB_PORT", COLLAB_PORT, "the collaboration server"],
    ["OPENCALC_EVICT_PORT", EVICT_PORT, "the idle-eviction collaboration server"],
  ];
  const busy = [];
  for (const [envVar, port, role] of ports) {
    const who = await occupant(port);
    if (!who) continue;
    let whose = "something this run did not start";
    if (who.root && sameDir(who.root, WEBAPP)) {
      whose = `a serve.py left over from an earlier run of this checkout (pid ${who.pid})`;
    } else if (who.root) {
      whose = `another checkout's serve.py: ${who.root} (pid ${who.pid})\n      this checkout is ${WEBAPP}`;
    }
    busy.push(`  port ${port} (${envVar}) — ${role}\n      ${whose}`);
  }
  if (!busy.length) return;
  throw new Error(
    [
      "browser-smoke will not run against a server it did not start.",
      "",
      ...busy,
      "",
      "A run that attaches to another checkout's server tests that tree and",
      "passes on code you never wrote (CI-025). Stop the server, or give this",
      "run four ports nothing else is on — these are only an example:",
      "",
      `  OPENCALC_SMOKE_PORT=${PORT + 100} OPENCALC_ORIGIN_PORT=${ORIGIN_PORT + 100} \\`,
      `  OPENCALC_COLLAB_PORT=${COLLAB_PORT + 100} OPENCALC_EVICT_PORT=${EVICT_PORT + 100} npm test`,
      "",
    ].join("\n"),
  );
}

// `TEST_WORKER_INDEX` is set only in Playwright's worker processes. They load
// this config too, and by then the run's own servers are listening.
//
// `--list` starts no servers and loads nothing into a browser, so it cannot be
// wrong about which tree it tested. Refusing it would be a gate firing on a
// command it does not apply to, which is how people learn to route around one.
const startsServers =
  process.env.TEST_WORKER_INDEX === undefined && !process.argv.includes("--list");
if (startsServers) await refuseForeignServers();

export default defineConfig({
  testDir: ".",
  // The editor boots a WebAssembly module and paints a canvas; a CI runner
  // under load is slower at both than any laptop.
  timeout: 60_000,
  expect: { timeout: 15_000 },
  // Serial: the specs share one server and assert on a freshly seeded
  // document, and a flaky gate is worse than a missing one.
  workers: 1,
  // No retries. This gate exists to catch a real breakage, and a retry turns
  // an intermittent one — exactly the kind worth knowing about — into a pass.
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [["github"], ["list"]] : [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    // Copy and paste go through the async Clipboard API, as they do for a real
    // user who has already granted the page access. Without this the editor's
    // copy silently rejects and the paste tests would be testing the fallback.
    permissions: ["clipboard-read", "clipboard-write"],
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: [
    {
      command: `python3 ../../webapp/serve.py ${PORT}`,
      url: `http://127.0.0.1:${PORT}/editor.html`,
      // Never reused, locally or in CI. This used to be `!process.env.CI`,
      // which meant a local run adopted whatever was on the port — and on a
      // machine running several agents' suites at once that was routinely
      // another checkout's `serve.py`, serving another tree's `webapp/`
      // (`CI-025`). The same applies to the three servers below: a shared
      // collaboration server would give one suite another suite's documents.
      // `refuseForeignServers()` above says whose server it found before
      // Playwright gets as far as its own, less helpful, refusal.
      reuseExistingServer: false,
      timeout: 30_000,
      stdout: "ignore",
      stderr: "pipe",
    },
    // The integrator's origin, as far as the collaboration server is concerned:
    // somewhere it fetches a package from over HTTP. A session starts from the
    // *file*, so something has to be serving one.
    {
      command: `python3 -m http.server ${ORIGIN_PORT} --bind 127.0.0.1 --directory ../../fixtures/generated`,
      url: `http://127.0.0.1:${ORIGIN_PORT}/minimal.xlsx`,
      reuseExistingServer: false,
      timeout: 30_000,
      stdout: "ignore",
      stderr: "pipe",
    },
    // The real server binary, with the environment a deployment gives it.
    //
    // `cargo run` rather than a prebuilt path so the gate cannot pass against a
    // stale binary somebody built last week — which is the same reason the
    // WebAssembly module is rebuilt rather than reused.
    {
      command: "cargo run --locked -p casual-calc-collab-server",
      url: `http://127.0.0.1:${COLLAB_PORT}/healthz`,
      reuseExistingServer: false,
      // A cold compile of the server and its dependency tree, on a CI runner.
      timeout: 300_000,
      cwd: "../..",
      stdout: "ignore",
      stderr: "pipe",
      env: {
        OPENCALC_BIND: `127.0.0.1:${COLLAB_PORT}`,
        // HS256. The server warns that a process holding a shared secret can
        // mint tokens as well as check them; for a test that is the point.
        OPENCALC_SHARED_SECRET: SECRET,
        OPENCALC_AUDIENCE: "opencalc-test",
        // The origin is plain HTTP on loopback. Both of these are exactly the
        // "local development only" case each setting documents, and neither is
        // defaulted on.
        OPENCALC_ALLOW_PLAIN_CALLBACKS: "1",
        OPENCALC_ALLOWED_HOSTS: "127.0.0.1",
        // Short enough that a test can watch a participant time out, long
        // enough that a loaded runner does not evict one mid-assertion.
        OPENCALC_TICK_MS: "100",
        OPENCALC_PRESENCE_TTL_MS: "3000",
        RUST_LOG: "casual_calc_collab_server=debug,warn",
      },
    },
    // The same binary, set to forget an idle document almost at once.
    //
    // This is what makes the unresumed reconnect reachable from a test at all:
    // a resume key belongs to its document, so evicting the document is how the
    // server comes to *not recognise* one — the same state a restart, a
    // rebalance to another node, or a key ageing out of a bounded map leaves
    // behind. All four are ordinary in a deployment; only this one can be
    // provoked in a few hundred milliseconds.
    {
      command: "cargo run --locked -p casual-calc-collab-server",
      url: `http://127.0.0.1:${EVICT_PORT}/healthz`,
      reuseExistingServer: false,
      timeout: 300_000,
      cwd: "../..",
      stdout: "ignore",
      stderr: "pipe",
      env: {
        OPENCALC_BIND: `127.0.0.1:${EVICT_PORT}`,
        OPENCALC_SHARED_SECRET: SECRET,
        OPENCALC_AUDIENCE: "opencalc-test",
        OPENCALC_ALLOW_PLAIN_CALLBACKS: "1",
        OPENCALC_ALLOWED_HOSTS: "127.0.0.1",
        OPENCALC_TICK_MS: "100",
        OPENCALC_PRESENCE_TTL_MS: "3000",
        // The whole point of this instance. A document whose roster has been
        // empty for half a second is let go, taking its resume keys with it.
        OPENCALC_IDLE_EVICTION_MS: "500",
        RUST_LOG: "casual_calc_collab_server=debug,warn",
      },
    },
  ],
});
