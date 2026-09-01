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

/// **A redirect leaves one socket, not three** (`COL-60`).
///
/// `redirect()` closes the old socket and calls `open()` immediately. The old
/// socket's `onclose` fires a task later, and until this row it ran against
/// whatever `socket` had become: `stopTimers()` killing the *new* socket's
/// timers, `joined = false`, and a `setTimeout(open, …)` opening a third
/// connection. `close()` never had the bug because it sets `closed` first.
///
/// Counted rather than reasoned about, which is what the row asked for. The
/// WebSocket is stubbed because the alternative is a second live node
/// answering `stopped` with `elsewhere`, and what is under test is this
/// module's bookkeeping, not a server's.
test("a redirect leaves exactly one socket open", async ({ page }) => {
  const result = await page.evaluate(async () => {
    const opened = [];
    class StubSocket {
      constructor(url) {
        this.url = String(url);
        this.readyState = 1;
        this.closed = false;
        opened.push(this);
        setTimeout(() => this.onopen && this.onopen(), 0);
      }
      send() {}
      close() {
        if (this.closed) return;
        this.closed = true;
        // Asynchronous, as a real one is — the whole defect lives in the gap
        // between `close()` returning and `onclose` running.
        setTimeout(() => this.onclose && this.onclose(), 0);
      }
    }
    const real = window.WebSocket;
    window.WebSocket = StubSocket;
    try {
      const { collaborate } = await import("/collab.js");
      const handle = collaborate({
        url: "wss://full.example/collab",
        token: "t",
        document: "d",
        // Every method answered, not only the ones this path is known to
        // call: a missing one throws inside a handler where no assertion
        // would ever see it, and the failure reads as the transport
        // misbehaving.
        wasm: new Proxy(
          {},
          { get: (_t, k) => (k === "protocol_version" ? () => 1 : () => undefined) },
        ),
        onStatus: () => {},
        onDocument: () => {},
        onPresence: () => {},
      });
      await new Promise((r) => setTimeout(r, 50));
      const before = opened.length;
      // The node is full and names another.
      opened[opened.length - 1].onmessage({
        data: JSON.stringify({
          type: "stopped",
          reason: "full",
          elsewhere: "wss://spare.example/collab",
        }),
      });
      // Long enough for the old socket's `onclose`, and for the retry it used
      // to schedule (full jitter over the initial backoff) to have fired.
      await new Promise((r) => setTimeout(r, 1500));
      const live = opened.filter((s) => !s.closed).length;
      handle.close?.();
      return { before, total: opened.length, live, urls: opened.map((s) => s.url) };
    } finally {
      window.WebSocket = real;
    }
  });

  expect(result.before, "the first connection was never made").toBe(1);
  expect(
    result.live,
    `after the redirect ${result.live} sockets are open, not 1: ${result.urls.join(", ")}`,
  ).toBe(1);
  expect(
    result.total,
    `the redirect opened ${result.total} sockets in total; one replacement is expected, `
      + `a third is the superseded onclose scheduling its own reconnect: ${result.urls.join(", ")}`,
  ).toBe(2);
  expect(result.urls[1]).toContain("spare.example");
});
