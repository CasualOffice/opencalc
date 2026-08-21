// Which redirect a client will follow (DEP-09).
//
// A full node answers `stopped` with an `elsewhere` naming a node with room.
// That value decides where the client sends its token next, so it is checked
// rather than followed: a WebSocket URL only, and never a downgrade from an
// encrypted session to a plain one.
//
// Driven through the real module in a real page — `new URL` resolution and
// protocol handling are the browser's, and re-implementing them in a test
// harness would test the harness.

import { expect, test } from "@playwright/test";

const decide = (page, current, elsewhere) =>
  page.evaluate(
    async ([c, e]) => (await import("/collab.js")).redirectTarget(c, e),
    [current, elsewhere],
  );

test.beforeEach(async ({ page }) => {
  await page.goto("/editor.html");
});

test("a websocket url on the same scheme is followed", async ({ page }) => {
  expect(await decide(page, "wss://a.example/collab", "wss://b.example/collab")).toBe(
    "wss://b.example/collab",
  );
  expect(await decide(page, "ws://a.example/collab", "ws://b.example/collab")).toBe(
    "ws://b.example/collab",
  );
});

/// **An encrypted session is never sent to a plain one.**
///
/// The token travels on this connection. A `stopped` that moves it to `ws://`
/// takes it off TLS, and the browser's mixed-content refusal would be the only
/// thing standing in the way.
test("a downgrade from wss to ws is refused", async ({ page }) => {
  expect(await decide(page, "wss://a.example/collab", "ws://b.example/collab")).toBeNull();
});

/// Only a socket. Anything else is a different protocol being dialled with a
/// credential attached.
test("a non-websocket scheme is refused", async ({ page }) => {
  for (const bad of [
    "https://b.example/collab",
    "http://b.example/collab",
    "file:///etc/passwd",
    "javascript:alert(1)",
    "data:text/html,<script>1</script>",
  ]) {
    expect(await decide(page, "wss://a.example/collab", bad), bad).toBeNull();
  }
});

/// Nothing to follow, and nothing that parses, are both "stay put".
test("an absent or unparseable target is refused", async ({ page }) => {
  expect(await decide(page, "wss://a.example/collab", null)).toBeNull();
  expect(await decide(page, "wss://a.example/collab", "")).toBeNull();
  expect(await decide(page, "wss://a.example/collab", "http://[not a url")).toBeNull();
});

/// A relative target resolves against the connection it arrived on, so an
/// operator may configure a path rather than a whole URL.
test("a relative target resolves against the current endpoint", async ({ page }) => {
  expect(await decide(page, "wss://a.example/collab", "/other")).toBe("wss://a.example/other");
});
