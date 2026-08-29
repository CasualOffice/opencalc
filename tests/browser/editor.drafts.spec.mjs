// Browser drafts and crash recovery — `SAVE-03`, `docs/83` §4.
//
// Before this, closing a tab or crashing lost everything since the last
// download. `beforeunload` was the whole of the defence, and it only ever helps
// the user who is closing deliberately.
//
// Two properties are what these tests exist for, and neither is about storage:
//
//   1. **A recovered document is offered, never applied.** A version somebody
//      did not ask for is its own defect, so the assertions below check what is
//      on *screen* after a recovery bar appears, not only that the bar appeared.
//   2. **Declining is not deleting.** A draft the user put off is offered again
//      next time. "A draft is never deleted because the user ignored it."
//
// The cadence is driven at its real numbers in one test and shortened in the
// rest. Shortened rather than bypassed on purpose: a test that calls "write a
// draft now" proves the writer works and says nothing about whether anything
// would ever call it, which is the half that was missing.

import { expect, test } from "@playwright/test";

/// Boot and wait for `initDrafts()` to have *finished*.
///
/// Deliberately **not** the usual `#tb-status` = `engine v…` wait. Two reasons,
/// and the second was found rather than reasoned:
///
///   - Drafts start after the engine reports itself, so waiting on the status
///     line makes "no draft was written" indistinguishable from "has not got
///     there yet", which is the assertion three of these tests turn on.
///   - The status line is *written to* by the draft paths. A mutation that made
///     the editor apply a recovered draft on boot changed the status text, so
///     the run went red in this helper instead of on the assertion that the
///     document had been overwritten — a failure that proved the mutation had
///     been noticed, not that the property was checked.
///
/// `initialised` is set on every exit from `initDrafts`, including the two that
/// decide not to autosave, so it means "the editor has finished deciding".
async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect
    .poll(() => page.evaluate(() => !!window.opencalcEditor?.draftStateForTest?.().initialised), {
      timeout: 30_000,
    })
    .toBe(true);
}

/// The same scheduler, wound tighter. `pollMs` follows `quiesceMs` down so the
/// poll can still see the quiet window.
async function fastCadence(page, quiesceMs = 400) {
  await page.evaluate((q) => {
    window.opencalcEditor.setDraftPolicyForTest({ quiesceMs: q, pollMs: 50 });
    window.opencalcEditor.restartDraftSchedulerForTest();
  }, quiesceMs);
}

const type = (page, row, text) =>
  page.evaluate(([r, t]) => window.opencalcEditor.wasmApi().session_set_cell(0, r, 0, t), [row, text]);
const cell = (page, row) =>
  page.evaluate((r) => window.opencalcEditor.wasmApi().session_cell_input(0, r, 0), row);
const drafts = (page) => page.evaluate(() => window.opencalcEditor.listDrafts());
const state = (page) => page.evaluate(() => window.opencalcEditor.draftStateForTest());

/// Wait for the store to hold `n` drafts, so the tests do not race a timer.
const settleTo = (page, n) => expect.poll(async () => (await drafts(page)).length, { timeout: 20_000 }).toBe(n);

// --- The cadence ------------------------------------------------------------

/// **The numbers are the collaboration server's, not a second set.**
///
/// `server/casual-calc-collab-server/src/lifecycle.rs:32-43` already decides
/// when a document is durable enough. `docs/83` §4.1 copies it rather than
/// inventing a second policy, and the whole reason that note is one design and
/// not three is that these numbers are shared. A drift here is the drift the
/// copy was meant to prevent, and nothing else in the tree would notice it.
test("the autosave cadence is the collaboration server's, copied", async ({ page }) => {
  await boot(page);
  const policy = await page.evaluate(() => window.opencalcEditor.draftPolicy());
  expect(policy.quiesceMs, "SavePolicy::default().quiesce_ms").toBe(5_000);
  expect(policy.ceilingMs, "SavePolicy::default().ceiling_ms").toBe(60_000);
  expect(policy.everyEdits, "SavePolicy::default().every_revisions").toBe(200);
});

/// **At quiesce, and not before.**
///
/// `session_save()` was measured at 424–436 ms of blocked main thread for 300k
/// cells (`docs/83` §4.2), so an autosave that fires while somebody is typing is
/// a twelve-frame stall in the middle of their sentence. The scheduler waits for
/// the quiet window, and this is the test that it does.
test("a draft is written when editing stops, and not while it is going on", async ({ page }) => {
  await boot(page);
  await fastCadence(page, 1_500);
  await type(page, 40, "still typing");
  // Well inside the quiet window: nothing yet.
  await page.waitForTimeout(400);
  expect(await drafts(page), "wrote a draft while the user was still typing").toHaveLength(0);
  await settleTo(page, 1);
  expect((await state(page)).reason, "the trigger that fired").toBe("quiesced");
});

/// **The ceiling is measured from the start of the session, not from the first
/// draft.**
///
/// Somebody typing steadily never quiesces, so the quiesce rule never fires —
/// and a ceiling measured from "the last draft" has no last draft to measure
/// from. An hour of continuous work would have produced nothing at all, which
/// is the exact case `ceiling_ms` exists for: `lifecycle.rs` calls it "the
/// longest a session may go without saving", and a session starts at the start.
test("continuous editing still gets a draft, at the ceiling", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    // Quiesce far out of reach, so only the ceiling can fire.
    window.opencalcEditor.setDraftPolicyForTest({ quiesceMs: 60_000, ceilingMs: 600, pollMs: 50 });
    window.opencalcEditor.restartDraftSchedulerForTest();
  });
  // Keep the counter moving so the quiet window never opens.
  const typing = page.evaluate(async () => {
    for (let i = 0; i < 25; i += 1) {
      window.opencalcEditor.wasmApi().session_set_cell(0, 40, 0, `keystroke ${i}`);
      await new Promise((r) => setTimeout(r, 80));
    }
  });
  await settleTo(page, 1);
  expect((await state(page)).reason, "a draft arrived, but not from the ceiling").toBe("ceiling");
  await typing;
});

/// **A tab that goes away inside the quiet window does not take the last edits
/// with it.**
///
/// This is the server's `SaveReason::Closing` in the shape a browser tab has,
/// and without it the five-second quiesce is a hole in the promise: type, close
/// the tab, and the last thing typed was never written. `visibilitychange` and
/// not `beforeunload`, because `beforeunload` does not run for the case this
/// feature is named after — a tab the operating system killed.
test("a tab going away writes what has not been drafted yet", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    // Quiesce and ceiling both out of reach: nothing but the hidden path can
    // produce a draft here.
    window.opencalcEditor.setDraftPolicyForTest({ quiesceMs: 60_000, ceilingMs: 60_000, pollMs: 50 });
    window.opencalcEditor.restartDraftSchedulerForTest();
  });
  await type(page, 40, "typed and then closed");
  await page.waitForTimeout(300);
  expect(await drafts(page), "the quiet window has not passed; nothing should be written yet").toHaveLength(0);

  // The platform event, from the state that produces it. A test cannot
  // background a tab, but this is the same getter the handler reads and the
  // same event the browser dispatches.
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await settleTo(page, 1);
  expect((await state(page)).reason).toBe("hidden");
  const [row] = await drafts(page);
  expect(row.ahead, "the draft did not carry the edit that was about to be lost").toBeGreaterThan(0);
});

/// **A quiet document is not a document worth drafting.** Without this the
/// editor would rewrite an identical snapshot every five seconds for the whole
/// time a workbook sits open — 424 ms of main thread each, for nothing.
test("a quiesce with nothing new writes no draft", async ({ page }) => {
  await boot(page);
  await fastCadence(page, 300);
  await page.waitForTimeout(1_500);
  expect(await drafts(page), "a document nobody touched produced a draft").toHaveLength(0);
});

/// **A new document whose edit count lands on the old one's is still drafted.**
///
/// The engine's counter restarts at zero for a new session, so "edits now"
/// against "edits at the last draft" is a comparison between two numbers about
/// two different workbooks. Almost always they differ and the scheduler is
/// accidentally right; when they agree it concludes nothing has happened, and
/// the quiesce that should have captured the new document captures nothing —
/// while the previous document's draft sits in this tab's slot looking like the
/// current work.
///
/// The count is read out of the editor rather than assumed, and the new session
/// is driven to exactly that number, because that is the only state in which
/// the two readings collide. Two earlier cuts of this test proved nothing and
/// are worth recording:
///
///   - Making *some other* number of edits passes whether the collision is
///     handled or not, because then the two counts differ and the comparison is
///     accidentally right.
///   - Doing `session_new()` and the edits in **one** `evaluate` is a state no
///     user can reach: the scheduler polls between tasks, so a counter that
///     falls to zero and climbs back inside a single task is never observed
///     falling at all. That version failed with the fix in place, which is the
///     test being wrong rather than the code.
///
/// So the shape below is the one a person produces: File ▸ New, and then typing
/// — separate tasks, inside one quiet window.
test("a new document is drafted even when its edit count lands on the old one's", async ({ page }) => {
  await boot(page);
  await fastCadence(page, 1_200);
  await type(page, 40, "document one");
  await settleTo(page, 1);
  const [before] = await drafts(page);
  const collideAt = (await state(page)).draftedAtEdits;
  expect(collideAt, "no draft count to collide with").toBeGreaterThan(0);

  // File ▸ New's effect on the engine: a session counting from zero. Long
  // enough for the poll to see it, well short of the quiet window so no draft
  // of the empty document intervenes.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(250);
  expect(await drafts(page), "a draft landed before the collision was set up").toHaveLength(1);

  // Then typing, up to exactly the count the previous draft was taken at.
  await page.evaluate((n) => {
    const a = window.opencalcEditor.wasmApi();
    for (let i = 0; i < n; i += 1) a.session_set_cell(0, 41, 0, `document two, edit ${i}`);
  }, collideAt);
  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_edits_applied()),
    "the collision was not set up",
  ).toBe(collideAt);

  await expect
    .poll(async () => (await drafts(page))[0].at, { timeout: 20_000 })
    .toBeGreaterThan(before.at);

  // And the bytes really are the new document's. Reviewed in this window,
  // because a blocked popup is the fallback and it is the path a test can drive.
  await boot(page);
  await page.evaluate(() => { window.open = () => null; });
  await page.locator("#oc-recovery-review-slot-0").click();
  await page.locator(".oc-modal:not([hidden]) .oc-btn.primary").click();
  await expect.poll(() => cell(page, 41)).toContain("document two");
  expect(await cell(page, 40), "the draft still holds the document that was replaced").toBe("");
});

// --- Offered, never applied -------------------------------------------------

/// **The acceptance test of `docs/83` §8 Phase B.**
///
/// Edit, lose the page, come back — and find the work offered rather than
/// applied. The second half is the one that matters and the one a bar alone
/// does not give you: the document on screen after the reload must still be the
/// *file*. Applying a recovered snapshot because it exists is the defect this
/// whole design is arranged around.
test("a draft is offered back on the next open, and is not applied", async ({ page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "an hour of work");
  await settleTo(page, 1);

  await boot(page);

  const bar = page.locator("#oc-recovery");
  await expect(bar, "nothing offered the work back").toBeVisible();
  await expect(bar).toContainText("Unsaved work from an earlier session");
  await expect(bar, "the bar states a difference, not the word unsaved").toContainText("1 edit ahead of the last save");
  // `History` derives only `Debug, Default`, so a snapshot cannot carry an undo
  // stack even by accident. A user must not find that out by pressing Ctrl+Z.
  await expect(bar).toContainText("Undo history is not recovered");

  expect(await cell(page, 40), "the recovered draft was applied without being asked for").toBe("");
});

/// **Declining is not deleting.**
///
/// `docs/83` §4.3: "Doing neither leaves the draft. A draft is never deleted
/// because the user ignored it." Putting the bar away has to be free — otherwise
/// the one click a hurried user makes is the click that loses the work.
test("putting the bar away keeps the draft, and the next open offers it again", async ({ page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "put this off");
  await settleTo(page, 1);

  await boot(page);
  await expect(page.locator("#oc-recovery")).toBeVisible();
  await page.locator("#oc-recovery-later-slot-0").click();
  await expect(page.locator("#oc-recovery"), "declining left the bar up").toBeHidden();
  expect(await drafts(page), "declining deleted the draft").toHaveLength(1);

  await boot(page);
  await expect(page.locator("#oc-recovery"), "the draft was lost by being declined once").toBeVisible();
  await expect(page.locator("#oc-recovery")).toContainText("1 edit ahead of the last save");
});

/// **Review opens the draft beside the document, not over it.**
///
/// `docs/83` §7 refuses a merge outright — there is no three-way merge for a
/// spreadsheet that is right often enough to apply without being read — so the
/// two documents are put side by side and a person decides. The original tab is
/// asserted untouched, because "opened it somewhere" is not the property; "did
/// not touch what was already there" is.
test("Review opens the draft as a separate document and leaves this one alone", async ({ context, page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "the draft's own text");
  await settleTo(page, 1);

  await boot(page);
  const opened = context.waitForEvent("page");
  await page.locator("#oc-recovery-review-slot-0").click();
  const other = await opened;
  await other.waitForLoadState();
  expect(other.url(), "the second tab is not showing the draft").toContain("draft=slot-0");

  await expect
    .poll(() => other.evaluate(() => !!window.opencalcEditor?.draftStateForTest?.().initialised), {
      timeout: 30_000,
    })
    .toBe(true);
  expect(await cell(other, 40), "the draft did not arrive in the tab that was opened for it").toBe("the draft's own text");
  expect(await cell(page, 40), "reviewing a draft changed the document that was already open").toBe("");
  expect(await drafts(page), "reviewing a draft consumed it").toHaveLength(1);
  await other.close();
});

/// **Discard asks, and names the document.**
///
/// It is the only route that deletes work, so it is the only one that
/// confirms — and a cancelled confirmation has to leave the draft where it was,
/// or the dialog is decoration.
test("Discard confirms first, and cancelling keeps the draft", async ({ page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "not junk");
  await settleTo(page, 1);

  await boot(page);
  await page.locator("#oc-recovery-discard-slot-0").click();
  const modal = page.locator(".oc-modal:not([hidden])");
  await expect(modal, "the one destructive verb here did not ask").toBeVisible();
  await expect(modal).toContainText(/discard the draft/i);
  await modal.locator("button", { hasText: /cancel/i }).click();
  expect(await drafts(page), "cancelling deleted it anyway").toHaveLength(1);

  await page.locator("#oc-recovery-discard-slot-0").click();
  await page.locator(".oc-modal:not([hidden]) .oc-btn.primary").click();
  await expect.poll(async () => (await drafts(page)).length).toBe(0);
  await expect(page.locator("#oc-recovery")).toBeHidden();
});

// --- The failure modes ------------------------------------------------------

/// **Quota exceeded: stop, and say so where it stays said.**
///
/// `docs/83` §5.5. There is no version ring to evict in Phase B, so the ladder
/// has one rung and it is the one that must never be skipped. A toast would be
/// gone by the time the user wondered whether their work was safe, and the
/// whole point of the state is that it persists — so the assertion is that the
/// indicator is still there after further editing and another quiesce.
test("a full origin stops autosave and keeps saying so", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.setDraftPolicyForTest({ quiesceMs: 300, pollMs: 50 });
    window.opencalcEditor.breakDraftStoreForTest("quota");
    window.opencalcEditor.restartDraftSchedulerForTest();
  });
  await type(page, 40, "no room for this");

  const badge = page.locator("#autosave-state");
  await expect(badge, "autosave stopped without saying anything").toBeVisible();
  await expect(badge).toHaveText("Autosave off — no storage space");

  // Standing, not a toast.
  await type(page, 41, "still no room");
  await page.waitForTimeout(1_000);
  await expect(badge, "the indicator did not stand").toBeVisible();
  expect(await page.evaluate(() => window.opencalcEditor.autosaveFault())).toBe("Autosave off — no storage space");
});

/// **A browser that will not store anything still opens the editor.**
///
/// Private browsing is the realistic case: `indexedDB.open` rejects, or
/// `indexedDB` is not there at all. Two things have to be true — the editor
/// works, and it does not pretend the work is being kept.
test("a browser with no draft storage opens, edits, and says autosave is off", async ({ page }) => {
  await boot(page);
  await page.evaluate(async () => {
    window.opencalcEditor.breakDraftStoreForTest("unavailable");
    await window.opencalcEditor.initDrafts();
  });

  const badge = page.locator("#autosave-state");
  await expect(badge, "an editor that stores nothing said nothing").toBeVisible();
  await expect(badge).toContainText("Autosave off");

  await type(page, 40, "still usable");
  expect(await cell(page, 40), "storage refusing broke the editor").toBe("still usable");
});

/// **A host that owns the document gets no draft at all.**
///
/// `docs/83` §3.3 and §7. The host owns durability, and a host's document must
/// not leave a copy in the user's browser storage as a side effect of being
/// opened — which is a privacy statement, not a performance one, and is the
/// same second-writer problem the `wopi` preset already refuses `Ctrl+S` for.
test("a host-owned document leaves no copy in this browser", async ({ page }) => {
  await boot(page, "?mode=wopi");
  expect(await page.evaluate(() => window.opencalcEditor.getCapabilities().ownsFile)).toBe(true);
  await page.evaluate(() => {
    window.opencalcEditor.setDraftPolicyForTest({ quiesceMs: 200, pollMs: 50 });
  });
  await type(page, 40, "the host's document");
  await page.waitForTimeout(1_500);
  expect(await drafts(page), "a host's document was copied into browser storage").toHaveLength(0);
  await expect(page.locator("#oc-recovery")).toBeHidden();
});

/// **An editor inside somebody else's page writes nothing either.**
///
/// The capability test above is what a host *says*; this is what is true
/// whatever it says. `ownsFile` comes from a preset chosen by `?mode=`, and an
/// embed that sets no mode resolves to `standalone` — every permission granted.
/// So `ownsFile` alone would leave every `<opencalc-sheet>` and every framed
/// `editor.html` writing its host's document into the visitor's browser
/// storage, which is the side effect `docs/83` §3.3 exists to forbid.
test("an editor embedded in another page writes no draft", async ({ page }) => {
  // A frame is the cheaper of the two embeddings to build and exercises the
  // same predicate the shadow-root mount does. `docs.html` is a host page that
  // is not itself the editor, so the only editor on the origin is the framed
  // one and an empty store cannot be somebody else's doing.
  await page.goto("/docs.html");
  await page.evaluate(() => {
    const f = document.createElement("iframe");
    f.id = "oc-frame";
    f.src = "/editor.html";
    f.style.cssText = "width:900px;height:600px";
    document.body.append(f);
  });
  await expect
    .poll(() => page.frames().length, { timeout: 30_000 })
    .toBeGreaterThan(1);
  const inner = page.frames()[1];
  await expect
    .poll(() => inner.evaluate(() => !!window.opencalcEditor?.draftStateForTest?.().initialised), {
      timeout: 30_000,
    })
    .toBe(true);
  expect(
    await inner.evaluate(() => window.opencalcEditor.draftStateForTest().running),
    "a framed editor started autosaving",
  ).toBe(false);

  await inner.evaluate(() => window.opencalcEditor.wasmApi().session_set_cell(0, 40, 0, "the host's document"));
  await page.waitForTimeout(1_000);
  expect(
    await inner.evaluate(() => window.opencalcEditor.listDrafts()),
    "a framed editor copied its host's document into browser storage",
  ).toHaveLength(0);
});

/// **A draft from a newer build is kept, marked, and not opened.**
///
/// Guessing at a record shape from the future is how a recovery feature hands
/// somebody a corrupted document and calls it their work. It is not deleted
/// either: `docs/83` §5.7's rule is that a recovery feature which silently
/// discards what it failed to recover is worse than none, and the bytes are
/// offered as a download so they can be taken elsewhere.
test("a draft written by a newer build is kept and offered as a file, not opened", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => new Promise((resolve, reject) => {
    const req = indexedDB.open("opencalc-drafts", 1);
    req.onsuccess = () => {
      const db = req.result;
      const tx = db.transaction(["meta", "bytes"], "readwrite");
      tx.objectStore("meta").put({
        id: "slot-0", schema: 99, build: "future", engine: "9.9.9",
        name: "tomorrow.xlsx", format: "xlsx", edits: 12, ahead: 5,
        at: Date.now() - 60_000, size: 3,
      });
      tx.objectStore("bytes").put({ id: "slot-0", bytes: new Uint8Array([1, 2, 3]) });
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    };
    req.onerror = () => reject(req.error);
  }));

  await boot(page);
  const bar = page.locator("#oc-recovery");
  await expect(bar).toBeVisible();
  await expect(bar).toContainText("newer version of OpenCalc");
  await expect(page.locator("#oc-recovery-review-slot-0"), "offered to open a record it cannot read").toHaveCount(0);
  await expect(page.locator("#oc-recovery-download-slot-0"), "gave the user no way to take the bytes elsewhere").toBeVisible();
  expect(await drafts(page), "deleted a draft it merely could not read").toHaveLength(1);
});

/// **A draft whose bytes will not open is kept, marked for what is actually
/// wrong with it, and offered as a file.**
///
/// `docs/83` §5.7: a truncated draft is the realistic shape of a crash caught
/// mid-write. The entry stays, because a recovery feature that silently
/// discards what it failed to recover is worse than none — and the sentence has
/// to be the true one. Parking the entry raises its schema number, so without
/// care a cut-short file reports itself as "written by a newer version of
/// OpenCalc" and sends the user off to look for an upgrade that does not exist.
test("a draft that will not open is kept and says what is actually wrong", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => new Promise((resolve, reject) => {
    const req = indexedDB.open("opencalc-drafts", 1);
    req.onsuccess = () => {
      const tx = req.result.transaction(["meta", "bytes"], "readwrite");
      tx.objectStore("meta").put({
        id: "slot-0", schema: 1, build: "dev", engine: "0.0.0",
        name: "cut-short.xlsx", format: "xlsx", edits: 9, ahead: 9,
        at: Date.now() - 30_000, size: 4,
      });
      // Half a zip header and nothing else: what a crash mid-write leaves.
      tx.objectStore("bytes").put({ id: "slot-0", bytes: new Uint8Array([0x50, 0x4b, 0x03, 0x04]) });
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    };
    req.onerror = () => reject(req.error);
  }));

  await boot(page);
  await page.evaluate(() => { window.open = () => null; });
  await page.locator("#oc-recovery-review-slot-0").click();
  await page.locator(".oc-modal:not([hidden]) .oc-btn.primary").click();

  const bar = page.locator("#oc-recovery");
  await expect(bar, "a draft that would not open was not marked").toContainText(/cut short/i);
  await expect(bar, "a truncated file was blamed on the build's age").not.toContainText("newer version of OpenCalc");
  await expect(page.locator("#oc-recovery-download-slot-0"), "no way to take the bytes elsewhere").toBeVisible();
  expect(await drafts(page), "deleted the thing it failed to recover").toHaveLength(1);
});

/// **Two tabs do not write over each other.**
///
/// `docs/83` §5.8. One draft id shared by two tabs interleaves snapshots of two
/// different documents under one name, and the bar then offers the result as
/// though it were somebody's work. The lease gives the second tab its own slot.
test("two tabs editing at once keep two drafts, not one", async ({ context, page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "tab one");
  await settleTo(page, 1);

  const second = await context.newPage();
  await boot(second);
  await fastCadence(second);
  await type(second, 41, "tab two");
  await settleTo(second, 2);

  const rows = await drafts(second);
  const ids = rows.map((r) => r.id).sort();
  expect(ids, "the second tab wrote over the first tab's draft").toEqual(["slot-0", "slot-1"]);
  await second.close();
});

/// **A reloaded tab does not write over the draft it is offering.**
///
/// This is the defect the first cut of this feature walked into, and it is
/// invisible to reading: the tab reloads, the bar offers `slot-0` because no
/// other tab holds it, the lease hands `slot-0` straight back, and the first
/// autosave of the new session overwrites the work it was offering to recover.
/// The user watches a bar promising their afternoon and gets an empty sheet.
test("a reloaded tab claims a new slot rather than the draft it is offering", async ({ page }) => {
  await boot(page);
  await fastCadence(page);
  await type(page, 40, "the first session's work");
  await settleTo(page, 1);

  await boot(page);
  await fastCadence(page);
  await type(page, 41, "the second session's work");
  await settleTo(page, 2);

  const rows = await drafts(page);
  expect(rows.map((r) => r.id).sort(), "the reloaded tab reused the offered slot").toEqual(["slot-0", "slot-1"]);
  expect((await state(page)).slot, "the tab took the slot it was offering back").toBe(1);

  // And the first session's work is still recoverable: the bar offers both.
  await boot(page);
  await expect(page.locator(".oc-recovery-row")).toHaveCount(2);
});
