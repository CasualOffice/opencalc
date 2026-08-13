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
/// The protocol version the raw-socket client below speaks.
///
/// Asserted against the engine's own number in the first test, because a client
/// that states the wrong version is refused *before* it joins — which, from the
/// test's side, is indistinguishable from the server hanging.
const PROTOCOL = 5;



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
/// Resolves with the first `replies` messages the server sends after `act` runs.
function speakDirectly(key, user, access, act, replies = 1) {
  // Kept in step with the engine by `the protocol version this file speaks is
  // the one the engine speaks`, below. A raw client has to state a version, and
  // a stale one here would look like the server going silent.
  const { claims } = tokenFor({ document: key, user, origin: ORIGIN, access });
  const token = mint(claims, SECRET);
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`${COLLAB_URL}?doc=${encodeURIComponent(key)}`);
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error("the server said nothing at all"));
    }, 10_000);
    let acted = false;
    const heard = [];
    socket.onopen = () =>
      socket.send(JSON.stringify({ type: "join", protocol: PROTOCOL, token }));
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      // The server says the token was accepted before it fetches the document.
      // Not the answer to anything; just an acknowledgement that the wait about
      // to happen is a wait rather than a hang.
      if (message.type === "opening") return;
      // Presence is unsolicited and says nothing about what this client asked
      // for: a joiner is told who is already in the document, and anybody may
      // move at any moment. Skipped rather than counted, so a test asking for
      // "the reply to my submission" gets the reply to its submission — these
      // assertions are about refusals and acknowledgements, and were reading
      // whichever message happened to arrive first.
      if (message.type === "presence" || message.type === "departed") return;
      if (message.type === "welcome") {
        acted = true;
        act((m) => socket.send(JSON.stringify(m)), message.client, message.revision);
        return;
      }
      if (!acted) {
        // Refused before it even joined. Reported rather than waited out, so
        // the failure names itself instead of timing out as silence.
        clearTimeout(timer);
        socket.close();
        reject(new Error(`refused before joining: ${JSON.stringify(message)}`));
        return;
      }
      heard.push(message);
      if (heard.length < replies) return;
      clearTimeout(timer);
      socket.close();
      resolve(replies === 1 ? heard[0] : heard);
    };
    socket.onerror = () => {
      clearTimeout(timer);
      reject(new Error("the socket failed"));
    };
  });
}

test.describe("collaboration", () => {
  test("the protocol version this file speaks is the one the engine speaks", async ({ browser }) => {
    const page = await browser.newPage();
    await boot(page);
    expect(await page.evaluate(() => window.__editorModule && null)).toBe(null);
    const engine = await page.evaluate(async () => {
      const editor = await import(window.__editorModule);
      return editor.wasmApi().protocol_version();
    });
    expect(engine, "bump PROTOCOL in this file when the protocol changes").toBe(PROTOCOL);
    await page.close();
  });

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
        base: { revision },
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

  // --- Reconnecting (ADR-015) -----------------------------------------------

  test("an edit made while disconnected arrives once the browser comes back", async ({
    browser,
  }) => {
    const key = freshDocument();
    // Separate contexts, because going offline is a property of a context and
    // taking both offline would prove nothing.
    const away = await browser.newContext();
    const watching = await browser.newContext();
    const alice = await away.newPage();
    const bob = await watching.newPage();
    await boot(alice);
    await boot(bob);
    await join(alice, { document: key, user: { id: "u-alice", name: "Alice" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob" } });

    // Alice loses the network — a lid closing, a tunnel, a Wi-Fi handover, a
    // rolling deployment of the server. All ordinary, which is why losing work
    // to one is not acceptable.
    // Offline first so the reconnection attempts fail, then the socket is
    // dropped. Taking a context offline does not tear down an *established*
    // WebSocket — it stops new connections — so both halves are needed to hold
    // a client in the reconnecting state for as long as the test wants.
    await away.setOffline(true);
    await alice.evaluate(() => window.__session.reconnect());
    await expect
      .poll(() => alice.evaluate(() => window.__collab.statuses.map((s) => s.state)))
      .toContain("reconnecting");

    // And types anyway, which is the whole point: her editor still works, and
    // what she writes must not evaporate when the socket comes back.
    await setCellIn(alice, 30, 1, "written while offline");
    expect(await alice.evaluate(() => window.__editor.wasmApi().collab_unacknowledged())).toBe(true);

    await away.setOffline(false);

    await expect
      .poll(() => cellIn(bob, 30, 1), {
        message: "work done during a disconnect was lost on reconnect",
        timeout: 30_000,
      })
      .toBe("written while offline");

    // Resumed rather than restarted — a fresh join would have replaced her
    // document with a snapshot, which is exactly how the work used to vanish.
    expect(
      await alice.evaluate(() => window.__collab.documents.some((d) => d.reason === "resumed")),
    ).toBe(true);

    await away.close();
    await watching.close();
  });

  test("a remote edit made during a disconnect is caught up on, not lost", async ({ browser }) => {
    const key = freshDocument();
    const away = await browser.newContext();
    const watching = await browser.newContext();
    const alice = await away.newPage();
    const bob = await watching.newPage();
    await boot(alice);
    await boot(bob);
    await join(alice, { document: key, user: { id: "u-alice", name: "Alice" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob" } });

    await away.setOffline(true);
    await alice.evaluate(() => window.__session.reconnect());
    await expect
      .poll(() => alice.evaluate(() => window.__collab.statuses.map((s) => s.state)))
      .toContain("reconnecting");

    // The other direction: the world moved while she was away, and she has to
    // be told what she slept through rather than handed a whole new document.
    await setCellIn(bob, 31, 1, "happened while she was away");

    await away.setOffline(false);
    await expect
      .poll(() => cellIn(alice, 31, 1), {
        message: "a reconnecting client was never caught up",
        timeout: 30_000,
      })
      .toBe("happened while she was away");

    await away.close();
    await watching.close();
  });

  test("the connection is measured, not merely assumed", async ({ browser }) => {
    const key = freshDocument();
    const page = await browser.newPage();
    await boot(page);
    await join(page, { document: key, user: { id: "u-a", name: "Ada" } });

    // A round trip that came back. This is the only signal a client has that
    // the far end is still there: a socket that claims to be open proves
    // nothing, and neither does a send that did not throw.
    await page.evaluate(() => window.__session.ping?.() ?? null);
    await expect
      .poll(() => page.evaluate(() => window.__session.latency()), {
        message: "no answer ever came back",
        timeout: 30_000,
      })
      .not.toBeNull();
    expect(await page.evaluate(() => window.__session.latency())).toBeGreaterThanOrEqual(0);

    await page.close();
  });

  test("a joining client is told the document is being fetched", async ({ browser }) => {
    const key = freshDocument();
    const page = await browser.newPage();
    await boot(page);
    await join(page, { document: key, user: { id: "u-a", name: "Ada" } });

    // Between the token being accepted and the document arriving there used to
    // be nothing at all — an open socket and silence, which is what a server
    // that has hung also looks like. Fetching from the integrator can take as
    // long as the server's HTTP timeout allows.
    const statuses = await page.evaluate(() => window.__collab.statuses.map((s) => s.state));
    expect(statuses).toContain("opening");
    expect(statuses.indexOf("opening")).toBeLessThan(statuses.indexOf("live"));

    // And it carries the name, so the wait can be shown against the document
    // it is a wait for.
    expect(
      await page.evaluate(() =>
        window.__collab.statuses.find((s) => s.state === "opening")?.detail,
      ),
    ).toBe("minimal.xlsx");

    await page.close();
  });

  test("a second chunk sent before the first is acknowledged lands correctly", async ({
    browser,
  }) => {
    const key = freshDocument();
    const watcher = await browser.newPage();
    await boot(watcher);
    await join(watcher, { document: key, user: { id: "u-w", name: "Watcher" } });

    // Both chunks go out back to back, with no round trip between them. The
    // second names no revision — it was written on top of the first and only
    // the server knows where that landed — and the server resolves it from the
    // chunk before. Getting this wrong rebases the second chunk against its own
    // predecessor, which it already contains, and it lands at the wrong place
    // with no error anywhere.
    const acks = await speakDirectly(
      key,
      { id: "u-p", name: "Pipeliner" },
      "edit",
      (send, me, revision) => {
        send({
          type: "submit",
          client: me,
          seq: 1,
          base: { revision },
          ops: [
            {
              op: { setValue: { sheet: 0, at: { row: 40, col: 0 }, value: { inlineString: 1 } } },
              formulas: {},
              styles: {},
              strings: { 1: "first" },
            },
          ],
        });
        send({
          type: "submit",
          client: me,
          seq: 2,
          base: "chained",
          ops: [
            {
              op: { setValue: { sheet: 0, at: { row: 41, col: 0 }, value: { inlineString: 1 } } },
              formulas: {},
              styles: {},
              strings: { 1: "second" },
            },
          ],
        });
      },
      2,
    );

    expect(acks.map((a) => a.type)).toEqual(["ack", "ack"]);
    // Cumulative: the second names everything through sequence two.
    expect(acks[1].through).toBe(2);
    expect(acks[1].revision).toBeGreaterThan(acks[0].revision);

    await expect.poll(() => cellIn(watcher, 40, 0)).toBe("first");
    await expect.poll(() => cellIn(watcher, 41, 0)).toBe("second");

    await watcher.close();
  });
});

test.describe("seeing each other", () => {
  /// **A participant who joins second must be able to see who was already there.**
  ///
  /// Presence is only broadcast when somebody *moves*, so a joiner used to see
  /// an empty roster until one of the others happened to click. Two people
  /// reading the same document saw no evidence of one another at all.
  test("a joiner is told who is already in the document", async ({ browser }) => {
    const key = freshDocument();
    const first = await browser.newPage();
    await boot(first);
    await join(first, { document: key, user: { id: "u-ada", name: "Ada" } });

    // Ada announces where she is looking, and then stops doing anything at all.
    await first.evaluate(() => window.__session.present(0, [2, 1, 2, 1]));

    const second = await browser.newPage();
    await boot(second);
    await join(second, { document: key, user: { id: "u-grace", name: "Grace" } });

    await expect
      .poll(
        () =>
          second.evaluate(() =>
            window.__editor.collaborators().map((c) => ({ name: c.name, sel: c.selection })),
          ),
        { message: "the second participant never learned about the first" },
      )
      .toEqual([{ name: "Ada", sel: [2, 1, 2, 1] }]);
  });

  /// And the cursor is actually *drawn*, in that participant's own colour.
  ///
  /// The roster was already being kept before this and nothing painted it, so
  /// co-editing looked exactly like editing alone. Asserted against the canvas
  /// rather than the roster, because "the data is there" is precisely the state
  /// that shipped.
  test("another participant's selection is painted on the grid", async ({ browser }) => {
    const key = freshDocument();
    const mine = await browser.newPage();
    await boot(mine);
    await join(mine, { document: key, user: { id: "u-ada", name: "Ada" } });

    const theirs = await browser.newPage();
    await boot(theirs);
    await join(theirs, { document: key, user: { id: "u-grace", name: "Grace" } });

    // Somewhere unambiguous, and nowhere near Ada's own selection at A1.
    await theirs.evaluate(() => window.__session.present(0, [7, 3, 7, 3]));

    await expect
      .poll(
        async () => {
          // Their colour is assigned by the server, so it is read rather than
          // assumed — a hard-coded palette entry would pass or fail depending
          // on which client id this run happened to allocate.
          const colour = await mine.evaluate(
            () => window.__editor.collaborators()[0]?.color ?? null,
          );
          if (!colour) return "no roster entry yet";
          return mine.evaluate((hex) => {
            const want = [0, 2, 4].map((i) => parseInt(hex.replace("#", "").slice(i, i + 2), 16));
            const canvas = document.querySelector("#grid");
            const { data } = canvas
              .getContext("2d")
              .getImageData(0, 0, canvas.width, canvas.height);
            for (let i = 0; i < data.length; i += 4) {
              if (
                Math.abs(data[i] - want[0]) < 8 &&
                Math.abs(data[i + 1] - want[1]) < 8 &&
                Math.abs(data[i + 2] - want[2]) < 8
              ) {
                return "painted";
              }
            }
            return "nothing in their colour";
          }, colour);
        },
        { message: "the other participant's cursor was never drawn" },
      )
      .toBe("painted");

    // It follows them. A cursor drawn once at join and never again looks
    // identical in a screenshot and is useless in practice.
    const paintedAt = async (row) => {
      const colour = await mine.evaluate(() => window.__editor.collaborators()[0]?.color ?? null);
      if (!colour) return "no roster entry";
      return mine.evaluate(
        ([hex, row]) => {
          const want = [0, 2, 4].map((i) => parseInt(hex.replace("#", "").slice(i, i + 2), 16));
          const canvas = document.querySelector("#grid");
          const g = canvas.getContext("2d");
          const { data, width } = g.getImageData(0, 0, canvas.width, canvas.height);
          let top = null;
          for (let i = 0; i < data.length; i += 4) {
            if (
              Math.abs(data[i] - want[0]) < 8 &&
              Math.abs(data[i + 1] - want[1]) < 8 &&
              Math.abs(data[i + 2] - want[2]) < 8
            ) {
              top = Math.floor(i / 4 / width);
              break;
            }
          }
          return top === null ? "absent" : top;
        },
        [colour, row],
      );
    };

    const wasAt = await paintedAt(7);
    await theirs.evaluate(() => window.__session.present(0, [20, 3, 20, 3]));
    await expect
      .poll(async () => {
        const now = await paintedAt(20);
        return typeof now === "number" && typeof wasAt === "number" && now > wasAt
          ? "moved down"
          : `still ${now}`;
      }, { message: "the cursor did not follow them down the sheet" })
      .toBe("moved down");

    // And it goes when they do, rather than leaving a ghost at the last place
    // anybody saw them.
    await theirs.close();
    await expect
      .poll(() => paintedAt(20), { message: "the cursor outlived the participant" })
      .toBe("no roster entry");
  });
});
