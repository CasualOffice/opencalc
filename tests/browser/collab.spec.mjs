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

const SECRET = process.env.OPENCALC_TEST_SECRET ?? "browser-smoke-only-not-a-deployment-secret";
const COLLAB_PORT = Number(process.env.OPENCALC_COLLAB_PORT ?? 8125);
const ORIGIN_PORT = Number(process.env.OPENCALC_ORIGIN_PORT ?? 8124);
const COLLAB_URL = `ws://127.0.0.1:${COLLAB_PORT}/collab`;
/// The second server, which forgets an idle document in about half a second.
/// Kept in step with `playwright.config.mjs`, which reads the same variable
/// with the same default.
const EVICT_PORT = Number(process.env.OPENCALC_EVICT_PORT ?? 8126);
const EVICT_URL = `ws://127.0.0.1:${EVICT_PORT}/collab`;
const ORIGIN = `http://127.0.0.1:${ORIGIN_PORT}`;
/// The protocol version the raw-socket client below speaks.
///
/// Asserted against the engine's own number in the first test, because a client
/// that states the wrong version is refused *before* it joins — which, from the
/// test's side, is indistinguishable from the server hanging.
const PROTOCOL = 7;



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
async function join(page, { document: key, user, access = "edit", url = COLLAB_URL }) {
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
    { url, token, key },
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

  /// **A peer's edit cannot hold this tab (`COL-43`).**
  ///
  /// `collab_receive` called plain `session.recalculate()`, so a relayed edit
  /// that triggered an expensive pass froze the browser it arrived in exactly
  /// as an oversized open used to — and nothing the person in front of it did
  /// could stop that. The reason it stayed unfixed is that with no budget set
  /// the cancellable call is bit-identical, so no test could be made to fail.
  /// This one sets a budget.
  ///
  /// What a cancelled pass *means* is the part that needed deciding, and both
  /// halves are asserted here: the operation is applied and the document
  /// converges on cell content regardless, and only derived values are left
  /// behind — reported, rather than presented as final.
  test("a relayed edit that runs out of recalculation budget still converges", async ({
    browser,
  }) => {
    const key = freshDocument();
    const alice = await browser.newPage();
    const bob = await browser.newPage();
    await boot(alice);
    await boot(bob);

    // Bob gives a relayed batch no time at all. Set *before* joining, because
    // the transport captures the budget when it connects.
    await bob.evaluate(async () => {
      const editor = await import(window.__editorModule);
      editor.setTimeBudgetsForTest(10_000, 0);
    });

    await join(alice, { document: key, user: { id: "u-alice", name: "Alice", color: "#c0392b" } });
    await join(bob, { document: key, user: { id: "u-bob", name: "Bob", color: "#2980b9" } });

    // Enough formulas that a recalculation has somewhere to be stopped.
    //
    // **Above `CANCEL_CHECK_INTERVAL`, which is 4096.** The engine asks the
    // cancel token every few thousand evaluations rather than every cell, so a
    // workbook of a few hundred formulas finishes without ever asking and a
    // zero budget changes nothing. The first draft of this test used 400 and
    // passed against the unfixed code for exactly that reason.
    //
    // Pasted in one operation rather than set cell by cell: 6000 separate
    // edits would be 6000 recalculations of a growing sheet before the test
    // even starts.
    await alice.evaluate(() => {
      const rows = [];
      for (let r = 1; r <= 6000; r += 1) rows.push(`${r}\t=A${r}*2+1`);
      window.__editor.wasmApi().session_paste_tsv(0, 0, 0, rows.join("\n"));
    });
    await setCellIn(alice, 7000, 0, "the edit that matters");

    // **Converged**: the operation is in Bob's document, whatever became of the
    // values. This is the half that must never regress.
    await expect
      .poll(() => cellIn(bob, 7000, 0), {
        message: "a relayed edit was lost when its recalculation was cut short",
        timeout: 20_000,
      })
      .toBe("the edit that matters");

    // **And said so**: at least one relayed batch was reported as leaving
    // values behind, rather than a half-fresh sheet presented as final.
    await expect
      .poll(
        () =>
          bob.evaluate(() =>
            window.__collab.documents.some((d) => d.reason === "remote" && d.stale === true),
          ),
        {
          message:
            "no relayed batch reported stale values, so the recalculation was never cancellable",
          timeout: 20_000,
        },
      )
      .toBe(true);

    // Bob is still a working editor: a tab that survived by being dead is not
    // the outcome. Typing locally still lands.
    await setCellIn(bob, 600, 0, "bob still works");
    expect(await cellIn(bob, 600, 0)).toBe("bob still works");

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

  // COL-36. The test above is the good case: the server remembers the resume
  // key and the work arrives. This is what happens when it does not — a
  // restart, a rebalance to another node, a document evicted while the tab was
  // away, a key aged out of a bounded map. All ordinary in a deployment, and
  // all indistinguishable from the client's side: an unrecognised key produces
  // no refusal at all, only a `welcome`, whose snapshot replaces the whole
  // document and everything unacknowledged with it.
  //
  // Nothing here can save the work — those operations were written against a
  // revision this client no longer holds, and replaying them untransformed is
  // the divergence the transform exists to prevent. **Saying so is the whole
  // remedy**, which is why the assertions below are as much about the loss
  // being real as about it being announced: a notice that fires when nothing
  // was lost trains people to ignore the one that matters.
  test("work lost to a reconnect the server could not resume is announced, not hidden", async ({
    browser,
  }) => {
    const key = freshDocument();
    const away = await browser.newContext();
    const alice = await away.newPage();
    await boot(alice);
    // Alone, and against the server that forgets: eviction needs an empty
    // roster, so a second participant would hold the document open and there
    // would be nothing to reconnect *into*.
    await join(alice, { document: key, user: { id: "u-alice", name: "Alice" }, url: EVICT_URL });

    // Nothing is edited before the disconnect on purpose. `evict_if_idle`
    // refuses to let go of a document with unsaved work — correctly, since the
    // host has to get it back — so a single edit here would keep the document
    // resident and the resume key with it, and the test would quietly exercise
    // the happy path instead.
    await away.setOffline(true);
    await alice.evaluate(() => window.__session.reconnect());
    await expect
      .poll(() => alice.evaluate(() => window.__collab.statuses.map((s) => s.state)))
      .toContain("reconnecting");

    await setCellIn(alice, 30, 1, "typed into the void");
    expect(await alice.evaluate(() => window.__editor.wasmApi().collab_unacknowledged())).toBe(true);

    // Long enough for the server to notice the empty roster and let go:
    // `OPENCALC_IDLE_EVICTION_MS` is 500ms on this instance and the tick is
    // 100ms, so this is several times the margin needed. Alice cannot reconnect
    // meanwhile — the context is offline — so the roster stays empty.
    await alice.waitForTimeout(3_000);
    await away.setOffline(false);

    // Announced. `unsentEdits` rather than a bare "lost", because the user has
    // to be told *what* went, not merely that something did.
    await expect
      .poll(() => alice.evaluate(() => window.__collab.statuses), {
        message: "work was discarded by a reconnect and nothing said so",
        timeout: 30_000,
      })
      .toContainEqual({ state: "lost", detail: "unsentEdits" });
    expect(
      await alice.evaluate(() => window.__collab.documents.some((d) => d.reason === "lost")),
    ).toBe(true);

    // The loss is real: the snapshot replaced what she typed. Asserted because
    // an announcement that does not correspond to a loss is worse than none.
    expect(await cellIn(alice, 30, 1)).toBe("");
    // And it was a welcome, not a resume — the mechanism, not just the symptom.
    expect(
      await alice.evaluate(() => window.__collab.documents.some((d) => d.reason === "resumed")),
    ).toBe(false);

    // **Not overwritten.** The transport withholds "live" while work is known
    // lost, so the notice cannot be erased a moment later by a status line
    // saying everything is fine — which is how the `refused` state was already
    // going unnoticed on this same socket.
    const afterLoss = await alice.evaluate(() => {
      const at = window.__collab.statuses.map((s) => s.state).lastIndexOf("lost");
      return window.__collab.statuses.slice(at + 1).map((s) => s.state);
    });
    expect(afterLoss).not.toContain("live");

    // And it clears on its own terms: an edit made *after* the loss, once the
    // server has acknowledged it, is the first moment "collaborating" is true
    // again rather than merely connected.
    await setCellIn(alice, 31, 1, "written after the loss");
    await expect
      .poll(
        () =>
          alice.evaluate(() => {
            const at = window.__collab.statuses.map((s) => s.state).lastIndexOf("lost");
            return window.__collab.statuses.slice(at + 1).map((s) => s.state);
          }),
        { message: "the session never recovered after announcing the loss", timeout: 30_000 },
      )
      .toContain("live");

    await away.close();
  });

  /// **A desynced client rejoins, and the notice for what that cost stays put
  /// (`COL-56`).**
  ///
  /// `COL-55` made the engine stop honestly: the first arrival it cannot merge
  /// latches the `ClientSession`, every later `receive` answers `Desynced`, and
  /// `flush`/`resend` seal so the half-rebased work cannot be pushed into the
  /// shared document. That is the whole of the engine's part — it has no
  /// transport, so it can report and it cannot recover.
  ///
  /// **What was measured on the untouched tree**, and it is worse than the row
  /// said. The row expected a one-shot `desynced` line overwritten by the next
  /// status. There was no line at all: this editor's `onStatus` has no branch
  /// for `desynced`, so `#tb-status` kept saying **"collaborating"** over a
  /// document that had stopped being shared, and no reconnect was attempted —
  /// `0` new sockets, `collab_unacknowledged()` still true, revision still 0
  /// while the peer's edit was committed at 1.
  ///
  /// A plain `reconnect()` is not the recovery either, and that was measured
  /// too: the server recognises the resume key, answers `resumed`, and
  /// `collab_resume` deliberately keeps the latch — so the next arrival threw
  /// `Desynced` again. The key has to be dropped, which is what makes the
  /// rejoin an ordinary join the server already serves.
  ///
  /// The pair used here is the one `COL-44` deliberately leaves unanswered:
  /// move-columns against move-columns. Two real browsers, the real server, and
  /// a genuine transform refusal — not an injected frame, because an injected
  /// frame would prove the editor handles a message the engine never latched
  /// on.
  test("a client that cannot merge an arrival rejoins, and the notice survives the next status", async ({
    browser,
  }) => {
    const key = freshDocument();
    const one = await browser.newPage();
    const two = await browser.newPage();
    await boot(one);
    await boot(two);
    await join(one, { document: key, user: { id: "u-one", name: "One" } });
    await join(two, { document: key, user: { id: "u-two", name: "Two" } });

    // Two instruments on the desyncing participant, and both are load-bearing.
    //
    // Counting sockets is how "it rejoined" is asserted as a *mechanism* rather
    // than inferred from a status line — the defect was precisely a status that
    // did not correspond to anything happening.
    //
    // Swallowing this participant's own submissions is what keeps its drag
    // outstanding when the peer's arrives, which is the state a transform
    // refusal needs: a chunk the server has already acknowledged is not rebased
    // against anything and can refuse nothing. It also keeps the loss real
    // after the rejoin, since nothing this client wrote ever reached the
    // server.
    await one.evaluate(() => {
      window.__sockets = 0;
      const Real = window.WebSocket;
      const Counting = function (...args) {
        window.__sockets += 1;
        return new Real(...args);
      };
      Counting.prototype = Real.prototype;
      for (const state of ["CONNECTING", "OPEN", "CLOSING", "CLOSED"]) Counting[state] = Real[state];
      window.WebSocket = Counting;

      window.__swallowed = 0;
      const send = Real.prototype.send;
      Real.prototype.send = function (data) {
        try {
          if (typeof data === "string" && JSON.parse(data).type === "submit") {
            window.__swallowed += 1;
            return;
          }
        } catch {
          // Not JSON, so not a submission. Let it through.
        }
        return send.call(this, data);
      };
    });

    await one.evaluate(() => {
      const engine = window.__editor.wasmApi();
      engine.session_set_cell(0, 0, 2, "42");
      engine.session_move_columns(0, 2, 1, 0);
    });
    await expect
      .poll(() => one.evaluate(() => window.__swallowed), {
        message: "the drag never reached a flush, so nothing was outstanding",
      })
      .toBeGreaterThan(0);

    // The peer's drag, which cannot be rebased past the one above.
    await two.evaluate(() => window.__editor.wasmApi().session_move_columns(0, 1, 3, 7));

    await expect
      .poll(() => one.evaluate(() => window.__collab.statuses.map((s) => s.state)), {
        message: "the arrival was merged after all, so the premise is gone",
      })
      .toContain("desynced");

    // **The rejoin.** A new socket, a second `welcome`, and the loss named
    // before the snapshot replaced the document — `why: "desynced"`, so a host
    // can tell this apart from a server that merely forgot the client.
    await expect
      .poll(() => one.evaluate(() => window.__collab.documents.map((d) => d.reason)), {
        message: "a desynced client never rejoined: it stayed on a session it had abandoned",
        timeout: 30_000,
      })
      .toEqual(["joined", "lost", "joined"]);
    expect(
      await one.evaluate(() => window.__collab.documents.find((d) => d.reason === "lost").why),
    ).toBe("desynced");
    expect(await one.evaluate(() => window.__sockets)).toBeGreaterThan(0);

    // And it is a participant again rather than a page that reconnected: the
    // engine's latch is gone, the abandoned work with it, and the revision has
    // caught up with the peer's committed drag.
    expect(await one.evaluate(() => window.__editor.wasmApi().collab_unacknowledged())).toBe(false);
    expect(
      await one.evaluate(() => window.__editor.wasmApi().collab_revision()),
    ).toBeGreaterThan(0);
    await setCellIn(two, 9, 0, "after the rejoin");
    await expect
      .poll(() => cellIn(one, 9, 0), {
        message: "the rejoined client is still deaf to the peer it rejoined because of",
      })
      .toBe("after the rejoin");

    // **The half that made this invisible.** The notice has to be on screen,
    // and it has to still be there after the next ordinary status update. A
    // plain reconnect is that update: it resumes, and `resumed` said "live"
    // unconditionally — so the line naming the lost edits went back to
    // "collaborating" with nothing having been acknowledged.
    const bar = one.locator("#tb-status");
    await expect(bar).toHaveText(/were not saved/);
    await one.evaluate(() => window.__session.reconnect());
    await expect
      .poll(() => one.evaluate(() => window.__collab.documents.map((d) => d.reason)), {
        message: "the reconnect never landed, so nothing later was tested",
        timeout: 30_000,
      })
      .toContain("resumed");
    await expect(bar).toHaveText(/were not saved/);

    await one.close();
    await two.close();
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

test.describe("seeing each other type", () => {
  /// **The gap COL-35 names, in the words it was reported in: "i can only see
  /// the edit when it's done, not while typing".**
  ///
  /// Until this, a participant's work appeared only when they pressed Enter and
  /// the cell editor closed. Two people could be filling the same cell and
  /// neither knew until one of them lost.
  ///
  /// Driven through the real keyboard on the real grid, because the interesting
  /// part is the whole path and most of it is not in any one place: a keystroke
  /// opens the in-cell editor, the editor announces a draft on the presence
  /// channel, the server relays it to the others without echoing it back, and
  /// the peer's roster carries it into the paint. Calling `present()` directly
  /// would skip the half of that which lives in the editor.
  ///
  /// **Nothing is committed.** That is the other half of the claim, and it is
  /// asserted rather than described: the observer's *document* must stay empty
  /// while the text is on their screen, because a draft is presence and never
  /// an operation (ADR-011).
  test("a peer sees a cell being typed into before it is committed, and sees it go on Escape", async ({
    browser,
  }) => {
    const key = freshDocument();
    const typist = await browser.newPage();
    const watcher = await browser.newPage();
    await boot(typist);
    await boot(watcher);
    await join(typist, { document: key, user: { id: "u-ada", name: "Ada" } });
    await join(watcher, { document: key, user: { id: "u-g", name: "Grace" } });

    // What the watcher can see of the typist, as the grid reads it when it
    // paints: the roster entry, which is the only input to the draft it draws.
    const draftSeenBy = (page) =>
      page.evaluate(() => window.__editor.collaborators()[0]?.editing ?? null);

    // What the cell holds before anybody types, so the assertion below can be
    // about the draft rather than about the fixture.
    const before = await cellIn(watcher, 0, 0);

    await typist.locator("#grid").focus();
    // A plain character on the grid opens the in-cell editor with that
    // character in it — the ordinary way anybody starts typing in a cell.
    await typist.keyboard.type("hello");

    await expect
      .poll(() => draftSeenBy(watcher), {
        message: "typing was invisible to the other participant (COL-35)",
      })
      .toMatchObject({ at: [0, 0], text: "hello" });

    // And it is genuinely uncommitted: the cell is **unchanged** on both sides,
    // so nothing entered the document, the history or the applied log on either
    // side of the wire.
    //
    // Unchanged rather than empty: the session opens `minimal.xlsx`, whose A1
    // already holds a value, and asserting `""` here tested the fixture rather
    // than the property. The claim worth making is that a draft does not become
    // a value — which is exactly "what was there is still there".
    expect(await cellIn(watcher, 0, 0)).toBe(before);
    expect(await cellIn(typist, 0, 0)).toBe(before);

    // It keeps up as she types, rather than showing the first burst and
    // stopping — the throttle drops nothing, it only delays.
    await typist.keyboard.type(" there");
    await expect
      .poll(() => draftSeenBy(watcher), { message: "the draft stopped following the typing" })
      .toMatchObject({ text: "hello there" });

    // Abandoned. The draft must go from every other screen, rather than leaving
    // a cell that looks permanently occupied by somebody who walked away.
    //
    // Asserted together with "she is still here", because the two failures are
    // otherwise indistinguishable: a participant dropped from the roster —
    // expired, disconnected — also has no draft, and would pass a test that
    // only asked about the draft.
    await typist.keyboard.press("Escape");
    await expect
      .poll(
        () =>
          watcher.evaluate(() => {
            const who = window.__editor.collaborators()[0];
            return { stillHere: !!who, editing: who?.editing ?? null };
          }),
        { message: "an abandoned edit left a ghost behind" },
      )
      .toEqual({ stillHere: true, editing: null });
    // Escape abandons: the cell is what it was before any of this.
    expect(await cellIn(watcher, 0, 0)).toBe(before);

    await typist.close();
    await watcher.close();
  });

  /// And committing does what it always did: the value arrives as an operation,
  /// and the draft that previewed it goes.
  ///
  /// Worth its own test because the two are easy to get wrong together — a
  /// draft that is never cleared looks identical to a working one until
  /// somebody presses Enter.
  test("committing turns the draft into a value and clears it", async ({ browser }) => {
    const key = freshDocument();
    const typist = await browser.newPage();
    const watcher = await browser.newPage();
    await boot(typist);
    await boot(watcher);
    await join(typist, { document: key, user: { id: "u-ada", name: "Ada" } });
    await join(watcher, { document: key, user: { id: "u-g", name: "Grace" } });

    await typist.locator("#grid").focus();
    await typist.keyboard.type("committed");
    await expect
      .poll(() => watcher.evaluate(() => window.__editor.collaborators()[0]?.editing?.text ?? null))
      .toBe("committed");

    await typist.keyboard.press("Enter");
    await expect
      .poll(() => cellIn(watcher, 0, 0), { message: "the committed value never arrived" })
      .toBe("committed");
    await expect
      .poll(
        () =>
          watcher.evaluate(() => {
            const who = window.__editor.collaborators()[0];
            return { stillHere: !!who, editing: who?.editing ?? null };
          }),
        { message: "the draft outlived the edit it was previewing" },
      )
      .toEqual({ stillHere: true, editing: null });

    await typist.close();
    await watcher.close();
  });

  /// A participant who vanishes mid-word takes the word with them.
  ///
  /// The disconnect case, which no amount of message-sending can cover: there
  /// is nothing to send. It works because the draft is carried *by* the presence
  /// entry, so removing the participant removes it — which is the reason for
  /// putting it there and not in a channel of its own.
  test("a participant who disconnects mid-word leaves no draft behind", async ({ browser }) => {
    const key = freshDocument();
    const typist = await browser.newPage();
    const watcher = await browser.newPage();
    await boot(typist);
    await boot(watcher);
    await join(typist, { document: key, user: { id: "u-ada", name: "Ada" } });
    await join(watcher, { document: key, user: { id: "u-g", name: "Grace" } });

    // What the cell holds before anybody types — the assertion at the end is
    // that a draft did not become a value, not that the sheet started empty.
    const before = await cellIn(watcher, 0, 0);

    await typist.locator("#grid").focus();
    await typist.keyboard.type("half-typ");
    await expect
      .poll(() => watcher.evaluate(() => window.__editor.collaborators()[0]?.editing?.text ?? null))
      .toBe("half-typ");

    await typist.close();
    await expect
      .poll(() => watcher.evaluate(() => window.__editor.collaborators().length), {
        message: "the draft of somebody who left was still on the grid",
      })
      .toBe(0);
    // And the half-typed word left nothing behind in the document either.
    expect(await cellIn(watcher, 0, 0)).toBe(before);

    await watcher.close();
  });
});

test.describe("the participant roster", () => {
  // **COL-33, in the words it was reported in against the running demo: "i
  // can't see here which profiles are collaborating... i see the name".**
  //
  // The cursors were already painted, and that was the whole of it: the only
  // evidence anybody else existed was a coloured label beside their cell, which
  // you can only read if you happen to be looking at that cell. A participant
  // four hundred rows down, or on another sheet, was drawn nowhere at all.
  //
  // Everything below asserts on what a user can see — the text of the chip, the
  // rows of the list, the pixels on the canvas, which tab is active — and not
  // on `collaborators()`. The roster data was already correct before this
  // control existed; "the data is there" is precisely the state that shipped.

  const label = (page) => page.locator("#presence-label");
  const rows = (page) => page.locator("#presence-menu .presence-item");

  /// Open the roster the way a person does, and wait for it to say it is open.
  async function openRoster(page) {
    await page.locator("#presence-btn").click();
    await expect(page.locator("#presence-btn")).toHaveAttribute("aria-expanded", "true");
    await expect(page.locator("#presence-menu")).toBeVisible();
  }

  /// Whether anything on this page's canvas is painted in the other
  /// participant's colour.
  ///
  /// Their colour is assigned by the server and read back rather than assumed:
  /// a hard-coded palette entry would pass or fail depending on which client id
  /// the run happened to allocate.
  async function paintedInTheirColour(page) {
    const colour = await page.evaluate(() => window.__editor.collaborators()[0]?.color ?? null);
    if (!colour) return "no roster entry";
    return page.evaluate((hex) => {
      const bare = hex.replace("#", "");
      const want = [0, 2, 4].map((i) => parseInt(bare.slice(i, i + 2), 16));
      const canvas = document.querySelector("#grid");
      const { data } = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
      for (let i = 0; i < data.length; i += 4) {
        if (
          Math.abs(data[i] - want[0]) < 8 &&
          Math.abs(data[i + 1] - want[1]) < 8 &&
          Math.abs(data[i + 2] - want[2]) < 8
        ) {
          return "painted";
        }
      }
      return "not painted";
    }, colour);
  }

  test("the roster names who is here, says what they are doing, and clears them when they leave", async ({
    browser,
  }) => {
    const key = freshDocument();
    const mine = await browser.newPage();
    await boot(mine);

    // No session, no control. The editor is single-player most of the time and
    // a permanent "only you" chip is noise, not information.
    await expect(mine.locator("#presence")).toBeHidden();

    await join(mine, { document: key, user: { id: "u-ada", name: "Ada Lovelace" } });

    // In a session and on her own — which is an answer to "who is
    // collaborating", and an absent control is not.
    await expect(mine.locator("#presence")).toBeVisible();
    await expect(label(mine)).toHaveText("Only you");
    await openRoster(mine);
    await expect(mine.locator("#presence-menu .presence-empty")).toBeVisible();
    await expect(rows(mine)).toHaveCount(0);

    const theirs = await browser.newPage();
    await boot(theirs);
    await join(theirs, { document: key, user: { id: "u-grace", name: "Grace Hopper" } });
    await theirs.evaluate(() => window.__session.present(0, [7, 3, 7, 3]));

    // Someone arrived **while the list was open**, which is the case a control
    // rebuilt on every presence message has to survive.
    await expect(label(mine)).toHaveText("1 other");
    await expect(rows(mine)).toHaveCount(1);
    await expect(rows(mine).locator(".presence-name")).toHaveText("Grace Hopper");
    // Where, in the terms a spreadsheet user thinks in: sheet and cell.
    await expect(rows(mine).locator(".presence-where")).toContainText("!D8");
    // And legible to a screen reader without opening anything.
    await expect(mine.locator("#presence-btn")).toHaveAttribute("aria-label", /Grace Hopper/);

    // It follows her. A roster filled in once at join reads identically in a
    // screenshot and is useless in practice.
    await theirs.evaluate(() => window.__session.present(0, [20, 3, 20, 3]));
    await expect(rows(mine).locator(".presence-where")).toContainText("!D21");

    // Typing is called typing, in words — not only in a colour or a pulse,
    // which is nothing at all to a reader who cannot see either.
    await theirs.locator("#grid").focus();
    await theirs.keyboard.type("half-typ");
    await expect(rows(mine).locator(".presence-typing")).toHaveText("typing");
    // And the roster follows the draft rather than the selection: the cell she
    // is typing in is where she is.
    await expect(rows(mine).locator(".presence-where")).toContainText("!A1");

    // Abandoned: she is still here, and no longer typing.
    await theirs.keyboard.press("Escape");
    await expect(rows(mine).locator(".presence-typing")).toHaveCount(0);
    await expect(rows(mine)).toHaveCount(1);

    // Gone. A roster that keeps someone who left is worse than no roster: it
    // says the cell they were in is still somebody's.
    await theirs.close();
    await expect(rows(mine)).toHaveCount(0);
    await expect(mine.locator("#presence-menu .presence-empty")).toBeVisible();
    await expect(label(mine)).toHaveText("Only you");

    await mine.close();
  });

  test("clicking a participant takes you to where they are", async ({ browser }) => {
    const key = freshDocument();
    const mine = await browser.newPage();
    const theirs = await browser.newPage();
    await boot(mine);
    await boot(theirs);
    await join(mine, { document: key, user: { id: "u-ada", name: "Ada Lovelace" } });
    await join(theirs, { document: key, user: { id: "u-grace", name: "Grace Hopper" } });

    // Four hundred rows down: far below the fold, which is the case the cursor
    // alone cannot help with. Being told somebody is in D401 and having to go
    // and find D401 is half a feature.
    await theirs.evaluate(() => window.__session.present(0, [400, 3, 400, 3]));

    await openRoster(mine);
    await expect(rows(mine).locator(".presence-where")).toContainText("!D401");
    // Nothing of hers is on screen yet — asserted, so that the assertion after
    // the click is about the click.
    expect(await paintedInTheirColour(mine)).toBe("not painted");

    await rows(mine).first().click();

    await expect
      .poll(() => paintedInTheirColour(mine), {
        message: "clicking a participant did not bring their cursor into view",
      })
      .toBe("painted");
    // The list closes behind it, like every other menu here.
    await expect(mine.locator("#presence-menu")).toBeHidden();
    // And it moved the **view**, not the selection: the active cell is where
    // the next keystroke lands, and a control that quietly moved it would be a
    // control that quietly types your work somewhere else.
    await expect(mine.locator("#cell-ref")).toHaveValue("A1");

    await mine.close();
    await theirs.close();
  });

  test("a participant on another sheet is marked as elsewhere, and going to them switches sheets", async ({
    browser,
  }) => {
    const key = freshDocument();
    const mine = await browser.newPage();
    const theirs = await browser.newPage();
    await boot(mine);
    await boot(theirs);
    await join(mine, { document: key, user: { id: "u-ada", name: "Ada Lovelace" } });
    await join(theirs, { document: key, user: { id: "u-grace", name: "Grace Hopper" } });

    // A second sheet, added through the control a person clicks, which also
    // moves this browser onto it.
    await mine.locator('[aria-label="Add sheet"]').click();
    await expect(mine.locator(".sheet-tab")).toHaveCount(2);
    await expect(mine.locator(".sheet-tab").nth(1)).toHaveClass(/active/);

    // She stays on the first sheet, where her cursor is drawn on no tab this
    // browser is looking at.
    await theirs.evaluate(() => window.__session.present(0, [2, 1, 2, 1]));

    await openRoster(mine);
    await expect(rows(mine)).toHaveCount(1);
    await expect(rows(mine).first()).toHaveClass(/elsewhere/);
    await expect(rows(mine).locator(".presence-where")).toContainText("!B3");

    await rows(mine).first().click();
    await expect(mine.locator(".sheet-tab").nth(0)).toHaveClass(/active/);

    await mine.close();
    await theirs.close();
  });
});

test.describe("undo between participants", () => {
  /// **Pressing Ctrl+Z must reach the other participant.**
  ///
  /// Undo used to change the author's document and stop there: history and the
  /// workbook moved, nothing entered the outgoing log, and the peer kept the
  /// edit. Nothing later contradicts that, so the two documents simply differ
  /// from then on — the worst shape a collaboration bug can take.
  ///
  /// The real keyboard shortcut on the real grid, in a real second browser,
  /// because the interesting part is the whole path: history → outgoing log →
  /// flush → server → peer.
  test("an undo in one browser reaches the other", async ({ browser }) => {
    const key = freshDocument();
    const author = await browser.newPage();
    await boot(author);
    await join(author, { document: key, user: { id: "u-a", name: "Ada" } });

    const peer = await browser.newPage();
    await boot(peer);
    await join(peer, { document: key, user: { id: "u-g", name: "Grace" } });

    await setCellIn(author, 3, 0, "before-undo");
    await expect
      .poll(() => cellIn(peer, 3, 0), { message: "the peer never saw the edit" })
      .toBe("before-undo");

    // The control a person actually presses.
    await author.locator("#grid").focus();
    await author.keyboard.press("ControlOrMeta+z");

    await expect
      .poll(() => cellIn(author, 3, 0), { message: "the author's own undo did not apply" })
      .toBe("");
    await expect
      .poll(() => cellIn(peer, 3, 0), { message: "the undo never reached the peer" })
      .toBe("");
  });

  /// **Undoing an insert somebody has filled is refused, and says why.**
  ///
  /// The tracker's gate for `COL-28`, in the shape docs/69 specifies. Ada
  /// inserts a row, Grace types into it, Ada presses Ctrl+Z. The stored inverse
  /// deletes that row — and Grace's data with it. Her own history holds "typed
  /// into row 10", not "here is row 10's content", so no undo anywhere brings
  /// it back.
  ///
  /// Two things are asserted, and the second is the one that is easy to skip:
  /// the data survives on **both** browsers, and Ada is **told**. A refusal
  /// nobody sees is a button that appears to do nothing, which is the failure
  /// this policy explicitly chose against — and the editor swallowed every undo
  /// error in a bare `catch {}` until this test existed.
  test("undoing an insert a peer has filled is refused, and says so", async ({ browser }) => {
    const key = freshDocument();
    const author = await browser.newPage();
    await boot(author);
    await join(author, { document: key, user: { id: "u-a", name: "Ada" } });

    const peer = await browser.newPage();
    await boot(peer);
    await join(peer, { document: key, user: { id: "u-g", name: "Grace" } });

    // A marker below the band, so the test can *observe* the insert reaching
    // Grace rather than assuming it. Without this she types before she has
    // applied it, her cell lands at the pre-insert address, and the two
    // documents disagree for a reason that has nothing to do with undo.
    await setCellIn(author, 20, 0, "sentinel");
    await expect.poll(() => cellIn(peer, 20, 0)).toBe("sentinel");

    // Ada inserts row 10 (index 9); the marker shifts to 21 everywhere.
    await author.evaluate(() => window.__editor.wasmApi().session_insert_rows(0, 9, 1));
    await expect
      .poll(() => cellIn(peer, 21, 0), { message: "the peer never applied the insert" })
      .toBe("sentinel");

    // Only now does Grace type into the row Ada just made.
    await setCellIn(peer, 9, 0, "grace was here");
    await expect
      .poll(() => cellIn(author, 9, 0), { message: "the author never saw the peer's edit" })
      .toBe("grace was here");

    await author.locator("#grid").focus();
    await author.keyboard.press("ControlOrMeta+z");

    // Refused, and said out loud.
    await expect(author.locator("#tb-status .err")).toContainText(/undo would remove/i);

    // And nothing moved, on either side.
    await expect
      .poll(() => cellIn(author, 9, 0), { message: "the undo ran anyway" })
      .toBe("grace was here");
    await expect
      .poll(() => cellIn(peer, 9, 0), { message: "the peer lost the row" })
      .toBe("grace was here");
  });

  /// And redo, which is a fresh intention rather than the cancellation of one.
  test("a redo in one browser reaches the other", async ({ browser }) => {
    const key = freshDocument();
    const author = await browser.newPage();
    await boot(author);
    await join(author, { document: key, user: { id: "u-a", name: "Ada" } });

    const peer = await browser.newPage();
    await boot(peer);
    await join(peer, { document: key, user: { id: "u-g", name: "Grace" } });

    await setCellIn(author, 4, 0, "restored");
    await expect.poll(() => cellIn(peer, 4, 0)).toBe("restored");

    await author.locator("#grid").focus();
    await author.keyboard.press("ControlOrMeta+z");
    await expect.poll(() => cellIn(peer, 4, 0)).toBe("");

    await author.keyboard.press("ControlOrMeta+Shift+z");
    await expect
      .poll(() => cellIn(peer, 4, 0), { message: "the redo never reached the peer" })
      .toBe("restored");
  });
});

/// **A refused session must not lock the editor out of trying again.**
///
/// `stopped` is terminal: the transport has closed the socket and will not
/// reconnect, because reconnecting would be refused for the same reason. But
/// only an explicit `stopCollaborating()` cleared the editor's `collabSession`,
/// so after any refusal the editor went on believing it was in a session that
/// no longer existed, and the next `collaborate()` threw "already in a
/// collaborative session".
///
/// The user-visible consequence: a token that expires — or any other refusal —
/// could only be recovered from by reloading the page, which throws away
/// whatever they had locally. Found by driving the real editor against the real
/// server, not by reading.
test("a refused session can be rejoined without reloading the page", async ({ browser }) => {
  const key = freshDocument();
  const page = await browser.newPage();
  await boot(page);

  // A token the server cannot read at all, which ends in `stopped`.
  const refused = await page.evaluate(
    async ({ url, key }) => {
      const editor = await import(window.__editorModule);
      window.__editor = editor;
      const seen = [];
      await editor.collaborate({
        url,
        token: "not-a-token",
        document: key,
        onStatus: (s) => seen.push(s.state),
      });
      // Give the refusal time to arrive and the transport to close.
      await new Promise((r) => setTimeout(r, 2000));
      return seen;
    },
    { url: COLLAB_URL, key },
  );
  expect(refused).toContain("stopped");

  // And now a *valid* join on the same page must work. Before the fix this
  // threw, and the only way out was a reload.
  await join(page, { document: key, user: { id: "u-ada", name: "Ada", color: "0891B2" } });

  await setCellIn(page, 0, 0, "rejoined");
  expect(await cellIn(page, 0, 0)).toBe("rejoined");

  await page.close();
});
