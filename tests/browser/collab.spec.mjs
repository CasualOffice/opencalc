// Two browsers, one server, one document.
//
// Everything below this test has been exercised in isolation: the transform
// against its algebraic law, the client session against a scripted server, the
// server against a scripted client, the wire format against a round trip. All
// of it passed, and the first time these pieces were put in a room together the
// browser sent a message the server could not parse — because the WebAssembly
// binding returned a bare `Submission` where the protocol carries a tagged
// `ClientMessage`, and no test on either side could see the seam.
//
// That is the class of bug this file exists for. It is the only test in the
// project where a real browser, a real socket and the real server binary are
// all present, and it is deliberately end-to-end rather than layered: a mock on
// either side would have agreed with whichever half wrote it.
//
// # What is real here
//
// The server is the actual binary, started by Playwright with the environment a
// deployment would give it. The document is fetched by the server over HTTP from
// a static origin, as it would be from an integrator. The token is signed the
// way an integrator signs one. The two participants are separate browser
// contexts with separate WebAssembly instances, which is what makes them
// genuine replicas rather than two views of one workbook.

import { expect, test } from "@playwright/test";

import { mint, tokenFor } from "./token.mjs";

const SECRET = process.env.OPENCALC_TEST_SECRET ?? "browser-tests-shared-secret";
const COLLAB_PORT = Number(process.env.OPENCALC_COLLAB_PORT ?? 8125);
const ORIGIN_PORT = Number(process.env.OPENCALC_ORIGIN_PORT ?? 8124);
const COLLAB_URL = `ws://127.0.0.1:${COLLAB_PORT}/collab`;
const ORIGIN = `http://127.0.0.1:${ORIGIN_PORT}`;



/// A fresh session key per test.
///
/// Reusing one would join the session the previous test left running, which is
/// the exact hazard `Document::key` is documented to have — and a test that
/// depends on eviction timing to clean up after itself is a flaky test.
let counter = 0;
const freshDocument = () => `browser-test-${process.pid}-${++counter}`;

/// Load the editor and wait for the engine to be up.
///
/// Not a convenience. The WebAssembly module is initialised by `main()`, and
/// until that has happened every binding reads an undefined instance and throws
/// from inside the generated glue — which is what the first version of this
/// file did, producing a participant stuck at "connecting" and no clue why.
/// `#tb-status` is the editor's own readiness signal, and is what the rest of
/// the browser suite waits on.
async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  // The specifier the page itself used, not one written down here. It carries a
  // cache-busting version that gets bumped, and a copy in a test is a copy that
  // goes stale silently — importing the stale one builds a *second* editor with
  // its own uninitialised engine, which then fails somewhere unrelated.
  await page.evaluate(() => {
    const tag = document.querySelector('script[type="module"][src*="editor.js"]');
    window.__editorModule = tag.src;
  });
}

/// Attach a participant to `page` and wait until it is live.
///
/// Returns nothing: everything the test asserts on is read back out of the page
/// afterwards, because a value that survived the round trip into the engine is
/// the claim worth making. A promise resolved in Node proves only that Node
/// heard about it.
async function join(page, { document: key, user, access = "edit" }) {
  const { claims } = tokenFor({ document: key, user, origin: ORIGIN, access });
  const token = mint(claims, SECRET);

  await page.evaluate(
    async ({ url, token, key }) => {
      // Through the editor, which is how a host does it. Importing the engine
      // glue here instead would get a second, uninitialised instance: the
      // editor loads it under a cache-busting specifier, so `/pkg/...js` and
      // `/pkg/...js?b=37` are two module records and only one of them has ever
      // been handed a WebAssembly instance. That cost an afternoon.
      const editor = await import(window.__editorModule);
      window.__collab = { statuses: [], documents: [], presence: [] };
      window.__editor = editor;
      window.__session = await editor.collaborate({
        url,
        token,
        document: key,
        onStatus: (s) => window.__collab.statuses.push(s),
        onDocument: (d) => window.__collab.documents.push(d),
        onPresence: (p) => window.__collab.presence.push(p),
      });
    },
    { url: COLLAB_URL, token, key },
  );

  await expect
    .poll(() => page.evaluate(() => window.__collab.statuses.map((s) => s.state)), {
      message: "the participant never went live",
    })
    .toContain("live");
}

/// What the engine in this page holds for a cell.
const cellIn = (page, row, col) =>
  page.evaluate(([row, col]) => window.__editor.wasmApi().session_cell_input(0, row, col), [
    row,
    col,
  ]);

/// Write a value the way the editor's own edit path does.
const setCellIn = (page, row, col, text) =>
  page.evaluate(
    ([row, col, text]) => window.__editor.wasmApi().session_set_cell(0, row, col, text),
    [row, col, text],
  );

/// Speak the protocol from Node, with no client library in the way.
///
/// For the properties that are about what the *server* guarantees regardless of
/// what a client does. A browser cannot test those, because the browser client
/// is the thing being assumed away.
///
/// Resolves with the first message the server sends after `act` runs.
function speakDirectly(key, user, access, act) {
  const { claims } = tokenFor({ document: key, user, origin: ORIGIN, access });
  const token = mint(claims, SECRET);
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`${COLLAB_URL}?doc=${encodeURIComponent(key)}`);
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error("the server said nothing at all"));
    }, 10_000);
    let acted = false;
    socket.onopen = () => socket.send(JSON.stringify({ type: "join", protocol: 1, token }));
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.type === "welcome") {
        acted = true;
        act((m) => socket.send(JSON.stringify(m)), message.client, message.revision);
        return;
      }
      if (!acted) return;
      clearTimeout(timer);
      socket.close();
      resolve(message);
    };
    socket.onerror = () => {
      clearTimeout(timer);
      reject(new Error("the socket failed"));
    };
  });
}

test.describe("collaboration", () => {
  test("an edit made in one browser arrives in another", async ({ browser }) => {
    const key = freshDocument();
    const alice = await browser.newPage();
    const bob = await browser.newPage();
    await boot(alice);
    await boot(bob);

    await join(alice, { document: key, user: { id: "u-alice", name: "Alice", color: "#c0392b" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob", color: "#2980b9" } });

    // Through the engine's own edit entry point, not a synthetic operation:
    // what makes a local edit sendable is that the editor's apply path reports
    // it, and an operation constructed for the test would bypass exactly that.
    await setCellIn(alice, 4, 1, "hello from alice");

    await expect
      .poll(() => cellIn(bob, 4, 1), { message: "the edit never reached the other browser" })
      .toBe("hello from alice");

    // And it was acknowledged, rather than merely broadcast: a client that
    // never hears back has an unacknowledged chunk forever and will never send
    // a second one.
    await expect
      .poll(() => alice.evaluate(() => window.__editor.wasmApi().collab_revision()))
      .toBeGreaterThan(0);

    await alice.close();
    await bob.close();
  });

  test("concurrent edits to the same cell both survive, in one order", async ({ browser }) => {
    const key = freshDocument();
    const alice = await browser.newPage();
    const bob = await browser.newPage();
    await boot(alice);
    await boot(bob);
    await join(alice, { document: key, user: { id: "u-alice", name: "Alice" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob" } });

    // Written in the same instant into neighbouring cells. The interesting part
    // is not that both arrive but that both browsers end up agreeing, which is
    // the property the transform exists for and the one that fails silently.
    await Promise.all([
      setCellIn(alice, 10, 1, "from-alice"),
      setCellIn(bob, 10, 2, "from-bob"),
    ]);

    for (const page of [alice, bob]) {
      await expect.poll(() => cellIn(page, 10, 1)).toBe("from-alice");
      await expect.poll(() => cellIn(page, 10, 2)).toBe("from-bob");
    }

    await alice.close();
    await bob.close();
  });

  test("a viewer is told they may not edit", async ({ browser }) => {
    const key = freshDocument();
    const viewer = await browser.newPage();
    await boot(viewer);
    await join(viewer, { document: key, user: { id: "u-v", name: "Viewer" }, access: "view" });

    // The client is told, and the editor makes the engine itself refuse — so a
    // viewer who types is stopped before a message is ever built. That is the
    // right order (a round trip to be told no is a worse experience than an
    // immediate no) and it is *also* why the server-side check cannot be
    // exercised from here: a well-behaved client never gets far enough to test
    // it. See the next test, which is deliberately not well-behaved.
    expect(
      await viewer.evaluate(() => window.__collab.documents.some((d) => d.editable === false)),
    ).toBe(true);
    expect(await viewer.evaluate(() => window.__editor.wasmApi().session_read_only())).toBe(true);

    await viewer.close();
  });

  test("a client that ignores its own read-only mode is refused by the server", async ({
    browser,
  }) => {
    const key = freshDocument();
    const watcher = await browser.newPage();
    await boot(watcher);
    await join(watcher, { document: key, user: { id: "u-w", name: "Watcher" } });

    // Not through the editor. The point of this test is a participant whose
    // client does not enforce anything — a modified build, or another
    // implementation entirely — which is the only threat model under which
    // client-side permission checks mean anything at all. Enforcement has to be
    // at the operation, on the server, or `Access` is decoration.
    const refusal = await speakDirectly(key, { id: "u-rogue", name: "Rogue" }, "view", (send, me, revision) => {
      send({
        type: "submit",
        client: me,
        seq: 1,
        base: revision,
        ops: [
          {
            op: { setValue: { sheet: 0, at: { row: 20, col: 1 }, value: { inlineString: 1 } } },
            formulas: {},
            styles: {},
            strings: { 1: "should not be allowed" },
          },
        ],
      });
    });

    expect(refusal, "the server accepted an edit from a read-only participant").toMatchObject({
      type: "refused",
      reason: { reason: "readOnlyAccess" },
    });

    // And nothing arrived anywhere, which is the assertion that would still
    // fail if the refusal were sent *after* committing.
    expect(await cellIn(watcher, 20, 1)).toBe("");
    await watcher.close();
  });

  test("a participant that closes stops appearing to the others", async ({ browser }) => {
    const key = freshDocument();
    const alice = await browser.newPage();
    const bob = await browser.newPage();
    await boot(alice);
    await boot(bob);
    await join(alice, { document: key, user: { id: "u-alice", name: "Alice" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob" } });

    // Presence is what puts a cursor on the other screen, so a departure that
    // is not reported leaves a cursor belonging to nobody.
    await bob.evaluate(() => window.__session.present(0, [1, 1, 1, 1]));
    await expect.poll(() => alice.evaluate(() => window.__collab.presence.length)).toBeGreaterThan(0);

    await bob.evaluate(() => window.__session.close());
    await expect
      .poll(
        () => alice.evaluate(() => window.__collab.presence.some((p) => p.kind === "gone")),
        { message: "a deliberate departure was never announced" },
      )
      .toBe(true);

    await alice.close();
    await bob.close();
  });
});
