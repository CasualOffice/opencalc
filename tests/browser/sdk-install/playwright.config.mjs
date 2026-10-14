// The `@opencalc/sheet` install gate (`SDK-012`).
//
//   cd tests/browser && npx playwright test -c sdk-install/playwright.config.mjs
//
// Its own configuration rather than a spec in the browser suite, because the
// browser suite serves `webapp/` through `serve.py` — the tree where every
// file exists and every module URL is stamped. This one serves nothing but an
// installed tarball through a plain static server, which is the only way to see
// a file the package forgot.

import { defineConfig, devices } from "@playwright/test";

const PORT = Number(process.env.OPENCALC_SDK_INSTALL_PORT ?? 8321);

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.mjs",
  timeout: 90_000,
  expect: { timeout: 30_000 },
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [["github"], ["list"]] : [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // Builds, packs and installs before it listens, so the wait covers it.
    command: "node fixture.mjs",
    url: `http://127.0.0.1:${PORT}/index.html`,
    reuseExistingServer: false,
    timeout: 180_000,
    stdout: "pipe",
    env: { OPENCALC_SDK_INSTALL_PORT: String(PORT) },
  },
});
