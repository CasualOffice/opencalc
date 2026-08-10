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
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `python3 ../../webapp/serve.py ${PORT}`,
    url: `http://127.0.0.1:${PORT}/editor.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
