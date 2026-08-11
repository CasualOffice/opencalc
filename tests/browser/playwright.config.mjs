// The browser-smoke gate's configuration.
//
// Serves the webapp with the project's own `serve.py` rather than a test-only
// static server. That script exists because a plain server was not enough —
// Chrome reuses an ES module across a same-URL navigation, so an edited
// `editor.js` kept running its old code — and a gate that serves the app
// differently from the way it is developed is a gate that can pass on a build
// nobody can reproduce.

import { defineConfig, devices } from "@playwright/test";

/// Nothing else in the project listens here; 8099 is `serve.py`'s own default
/// and is often already running on a developer's machine.
const PORT = Number(process.env.OPENCALC_SMOKE_PORT ?? 8123);
/// The collaboration gate's two extra servers. Kept in step with
/// `collab.spec.mjs`, which reads the same variables with the same defaults.
const ORIGIN_PORT = Number(process.env.OPENCALC_ORIGIN_PORT ?? 8124);
const COLLAB_PORT = Number(process.env.OPENCALC_COLLAB_PORT ?? 8125);
const SECRET = process.env.OPENCALC_TEST_SECRET ?? "browser-tests-shared-secret";

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
      reuseExistingServer: !process.env.CI,
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
      reuseExistingServer: !process.env.CI,
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
      reuseExistingServer: !process.env.CI,
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
  ],
});
