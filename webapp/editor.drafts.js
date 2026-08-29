// Browser drafts and crash recovery — `SAVE-03`, `docs/83` §4 and §8 Phase B.
//
// Until this file existed the document lived in wasm memory and nowhere else.
// Closing the tab, a reload, or a crash discarded everything since the last
// download, and `beforeunload` was the whole of the defence — a dialog that
// only helps the user who is about to close deliberately, and does nothing at
// all for the tab the operating system killed.
//
// What this adds is **insurance, not a save**. A draft is a copy of the
// document written to this browser's own IndexedDB, offered back on the next
// boot, and never applied to anything by itself.
//
// Three decisions from `docs/83` are load-bearing here and none of them is
// re-opened in this file:
//
// **The cadence is the collaboration server's, copied rather than invented**
// (`server/casual-calc-collab-server/src/lifecycle.rs:32-43`): quiesce 5 s,
// ceiling 60 s. It is not a preference — it is forced by what a save costs.
// Measured for `docs/83` §4.2, `session_save()` blocks the main thread for
// **424–436 ms at 300 k cells** against an **8 ms** IndexedDB write of the
// resulting 868 KB. Serialization is the whole cost and storage is none of it,
// which is the opposite of the intuition. So the write cannot happen while
// somebody is typing, and after five seconds of quiet nobody is. The ceiling is
// the case that hurts: sustained editing in a large workbook buys one ~0.4 s
// hitch a minute. That is accepted and named (`SAVE-06`), not hidden.
//
// **A recovered document is offered, never applied.** A version somebody did
// not ask for is its own defect. Nothing here touches the open document until a
// user has read what the draft is, when it was written and how far ahead of the
// last save it is, and clicked.
//
// **No draft in a host-owned mode.** `docs/83` §3.3: a host's document must not
// leave a copy in the user's browser storage as a side effect of being opened,
// and an editor autosaving underneath a host is the second writer
// `editor.core.js`'s `wopi` preset was written about.
//
// ### On `session_edits_applied()`, which is the only dirty signal there is
//
// It counts up and never down (`crates/casual-calc-wasm/src/history.rs:14`), so
// **undo does not wind it back** and `isDirty()` reports true for a document
// undone to the state it was saved in. That is the safe direction and the
// editor chose it deliberately, but it means an autosave keyed naively on
// `isDirty()` writes drafts nobody needs and offers them back afterwards.
//
// Three things follow, and none of them is a boolean:
//
// - A draft is written only when the counter has **moved since the last draft**,
//   so a quiesce that follows no edit writes nothing.
// - The record carries how far **ahead of the last save** it was at capture, so
//   the bar states a difference rather than the word "unsaved".
// - `docs/83` §4.3 asks that a draft level with the last save not be offered.
//   That is answered in the **writer**, not at display time: a draft is only
//   written for a document that differs from its last save, and the tab deletes
//   its own draft once the document is saved. Re-deciding it when the bar is
//   drawn would mean comparing a count from a session that has ended against a
//   baseline that no longer exists, which is how a *recovered* document's draft
//   would quietly stop being offered.
//
// What this does not fix, and cannot from here: after an edit and an undo the
// difference reads 2 and the document is where it was. The honest number is the
// counter's, the counter is the engine's, and correcting it is an engine change
// (`SAVE-06`'s neighbourhood), not a tally in the editor — this repository has
// twice learned what a rule that enumerates its own subjects costs.
//
// ### `FID-36`, and why Phase B does not wait for it
//
// Measured for `docs/83` §1.5: `set_cell("typo")` followed by `undo` yields a
// file whose `sheetData` is empty and whose `sharedStrings.xml` still contains
// `<t>typo</t>`. The undo is correct; the interned string survives and the
// writer emits it. For one download that is a curiosity. Under autosave it is a
// schedule.
//
// Phase B proceeds, and says so, for three reasons. The defect is in the
// writer, in `crates/` — it is not fixable from `webapp/` and holding crash
// recovery for it trades "text you took back is in a local file" against "an
// afternoon of work is gone", which is not a close call. A draft never leaves
// the machine it was made on (`docs/83` §7), so the residue does not travel.
// And the accumulation is bounded here by construction: a tab holds **one**
// draft, rewritten in place, so successive autosaves replace the residue rather
// than stacking it, and Discard deletes the bytes outright — which is precisely
// why `docs/83` §1.5 requires a delete and not only a replace.

import {
  BUILD,
  byId,
  confirmModal,
  documentName,
  editorIsThePage,
  errText,
  getCapabilities,
  isDirty,
  openBytes,
  savedAtEditsForDraft,
  status,
  statusError,
  wasm,
} from "./editor.core.js";

// --- The policy -------------------------------------------------------------

/// `lifecycle.rs:32-43`, copied. One set of numbers for every host is the whole
/// reason `docs/83` is one design note rather than three.
///
/// `everyEdits` is `every_revisions`; the server's unit is a revision and ours
/// is an applied edit, which is the closest thing a single-client session has.
const DEFAULT_POLICY = Object.freeze({
  quiesceMs: 5_000,
  ceilingMs: 60_000,
  everyEdits: 200,
  /// How often the engine's counter is read. Polled rather than pushed, for the
  /// reason the desktop title poll already gives: a push from every write path
  /// is a list of write paths, and the one left out is always the one added
  /// last. A counter read and two comparisons is what the steady state costs.
  pollMs: 250,
});

let policy = { ...DEFAULT_POLICY };

/// The cadence in force, for a host and for the test that pins it to the
/// server's numbers.
export function draftPolicy() {
  return Object.freeze({ ...policy });
}

/// Shorten the cadence so a test can drive the **real** scheduler rather than a
/// back door into the writer. A test that calls "write a draft now" proves the
/// writer works and says nothing about whether anything would ever call it.
export function setDraftPolicyForTest(partial) {
  policy = { ...policy, ...(partial || {}) };
  return draftPolicy();
}

// --- The store --------------------------------------------------------------
//
// IndexedDB, two object stores: `meta` for the record and `bytes` for the
// snapshot. Split because the recovery bar reads every record on boot and has
// no use for a megabyte of `.xlsx` per entry — `localStorage` is not an option
// at all, being synchronous, string-only and ~5 MB against a measured ~1 GB
// quota.

const DB_NAME = "opencalc-drafts";
const DB_VERSION = 1;

/// The record shape's own version, and the answer to "a draft from a newer
/// build".
///
/// A build that changes what a record holds bumps this. A record whose `schema`
/// is **greater** than this build's is one this build was not written to read:
/// it is listed, marked, and left alone. Guessing at a shape from the future is
/// how a recovery feature hands somebody a corrupted document and calls it
/// their work.
const SCHEMA = 1;

/// Why autosave is not running, when it is not. `""` means it is.
let storageFault = "";

let dbPromise = null;

function openDb() {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    let req;
    try {
      // Private browsing in some engines does not expose `indexedDB` at all,
      // and Firefox's private mode throws from `open()` rather than returning
      // a request that errors. Both are the same answer to the caller.
      if (typeof indexedDB === "undefined" || !indexedDB) throw new Error("no IndexedDB in this browser");
      req = indexedDB.open(DB_NAME, DB_VERSION);
    } catch (e) {
      reject(e);
      return;
    }
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains("meta")) db.createObjectStore("meta", { keyPath: "id" });
      if (!db.objectStoreNames.contains("bytes")) db.createObjectStore("bytes", { keyPath: "id" });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error("IndexedDB refused to open"));
    // A tab holding an older version open blocks the upgrade indefinitely. That
    // is not an error the platform ever resolves on its own, so it is one here:
    // a promise that never settles would hang every caller for the life of the
    // page.
    req.onblocked = () => reject(new Error("another tab is holding an older draft store open"));
  });
  // A rejected promise cached forever would make one transient failure
  // permanent for the page; a rejected *open* is not transient, so it is.
  // What must not persist is the rejection handler being unobserved.
  dbPromise.catch(() => {});
  return dbPromise;
}

function txDone(tx) {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error || new Error("draft transaction failed"));
    tx.onabort = () => reject(tx.error || new Error("draft transaction aborted"));
  });
}

function reqDone(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error("draft request failed"));
  });
}

/// Every metadata record in the store, newest first.
export async function listDrafts() {
  const db = await openDb();
  const tx = db.transaction("meta", "readonly");
  const rows = await reqDone(tx.objectStore("meta").getAll());
  return (rows || [])
    .filter((r) => r && typeof r.id === "string")
    .sort((a, b) => (b.at || 0) - (a.at || 0));
}

async function readDraftBytes(id) {
  const db = await openDb();
  const tx = db.transaction("bytes", "readonly");
  const row = await reqDone(tx.objectStore("bytes").get(id));
  return row ? row.bytes : null;
}

async function deleteDraft(id) {
  const db = await openDb();
  const tx = db.transaction(["meta", "bytes"], "readwrite");
  tx.objectStore("meta").delete(id);
  tx.objectStore("bytes").delete(id);
  await txDone(tx);
}

/// A store failure a test has asked for. `""` in every shipped path — see
/// [`breakDraftStoreForTest`] for why this is a variable rather than a stub.
let injectedWriteFault = "";

/// One write, both stores, one transaction.
///
/// The atomicity matters more than it looks: a metadata record without its
/// bytes is an entry the recovery bar offers and cannot open, which is §5.7's
/// failure mode manufactured by our own writer rather than by a crash.
async function putDraft(meta, bytes) {
  if (injectedWriteFault === "quota") {
    const err = new Error("The quota has been exceeded.");
    err.name = "QuotaExceededError";
    throw err;
  }
  const db = await openDb();
  const tx = db.transaction(["meta", "bytes"], "readwrite");
  tx.objectStore("bytes").put({ id: meta.id, bytes });
  tx.objectStore("meta").put(meta);
  await txDone(tx);
}

// --- The lease --------------------------------------------------------------
//
// `docs/83` §5.8. Two tabs autosaving to one draft id would interleave
// snapshots of two different documents under one name, and the recovery bar
// would offer the result as though it were a document. So a tab claims a
// **slot** and keeps it: the first tab to claim slot 0 has it, the second takes
// slot 1, and the panel shows two entries rather than one that is neither.
//
// Slots rather than random ids because a draft has to be *reusable*: a tab that
// reloads should overwrite the draft it wrote before rather than leave an
// orphan behind and start another, and a random id per session turns a day of
// reloads into a list of drafts nobody can tell apart.
//
// **A slot holding a draft the user has been offered and not answered is not
// free.** This is the defect a first run of this file walked straight into: the
// tab reloads, the bar offers `slot-0`, the tab claims `slot-0` because no
// other tab holds it, and the first autosave writes the recovered session's
// work over the work it was offering to recover. The lease therefore reserves
// those slots as well as the ones live tabs hold, and only Discard — a user
// saying so, on an entry that named the document — puts one back.
//
// What that costs, stated: a user who reloads repeatedly and answers the bar
// each time never accumulates, but one who ignores it accumulates one draft per
// session. They are *different* unsaved work — after a reload the document on
// screen is the file, not the previous draft — so keeping them is right rather
// than duplicative, and `docs/83` §4.3 forbids deleting one because it was
// ignored. The bound is the quota, and running into it is §5.5's named,
// standing state rather than a silent stop.

const CHANNEL = "opencalc-drafts";
/// How long to listen for other tabs before taking the lowest free slot. A
/// `BroadcastChannel` message to a live tab is a same-process post, so this is
/// two orders of magnitude more than it needs; the cost is paid once, off the
/// path of the first paint, and never again.
const LEASE_WAIT_MS = 150;

let channel = null;
let mySlot = null;
let leasePromise = null;
/// Slots that hold a draft this page is offering back. Filled by `initDrafts`
/// before the scheduler starts, so the first write cannot land on one.
let reservedSlots = new Set();

function slotOf(id) {
  const m = /^slot-(\d+)$/.exec(String(id || ""));
  return m ? Number(m[1]) : null;
}

function claimSlot() {
  if (leasePromise) return leasePromise;
  // A tab that opened a draft for review adopts that draft's slot: the document
  // it is holding *is* that draft, so writing a newer version of it over the
  // old one is the one case where overwriting is exactly right — and it is what
  // keeps reviewing a draft from leaving a second copy behind for ever.
  const adopted = slotOf(openedDraftId);
  if (adopted !== null) {
    mySlot = adopted;
    leasePromise = Promise.resolve(adopted);
    return leasePromise;
  }
  leasePromise = new Promise((resolve) => {
    let held = new Set();
    try {
      channel = new BroadcastChannel(CHANNEL);
    } catch {
      // No channel: one tab's worth of coordination, which is what a browser
      // without `BroadcastChannel` can offer. Said out loud rather than left as
      // a silent single-slot assumption.
      channel = null;
    }
    if (!channel) {
      let slot = 0;
      while (reservedSlots.has(slot)) slot += 1;
      mySlot = slot;
      resolve(slot);
      return;
    }
    channel.onmessage = (e) => {
      const msg = e && e.data;
      if (!msg) return;
      // Somebody else is asking. Answer with the slot this tab holds — and only
      // once it holds one, because answering "0" while still deciding is how
      // two tabs both wait for each other and both take 0.
      if (msg.type === "who" && mySlot !== null) {
        channel.postMessage({ type: "holding", slot: mySlot });
      } else if (msg.type === "holding" && typeof msg.slot === "number") {
        held.add(msg.slot);
      }
    };
    channel.postMessage({ type: "who" });
    setTimeout(() => {
      let slot = 0;
      while (held.has(slot) || reservedSlots.has(slot)) slot += 1;
      mySlot = slot;
      // Announce unprompted as well as on request: a tab that opened during
      // this window may have missed the `who`.
      try { channel.postMessage({ type: "holding", slot }); } catch {}
      resolve(slot);
    }, LEASE_WAIT_MS);
  });
  return leasePromise;
}

/// The slot this tab holds, for a test. `null` until the lease resolves.
export function draftSlotForTest() {
  return mySlot;
}

// --- Writing a draft --------------------------------------------------------

/// The counter at the last draft this tab wrote. `null` means it has written
/// none, which is not the same as zero.
let draftedAtEdits = null;
let lastDraftAt = 0;
let lastEditsSeen = null;
let lastEditAt = 0;
let timer = null;
let writing = false;
/// The draft this tab is under review of, if it booted from one. Excluded from
/// its own recovery bar: offering somebody the document they are looking at is
/// noise, and clicking Review on it would open a third copy.
let openedDraftId = null;

/// The counter reading at the moment a recovered draft was loaded, or `null`
/// when this page is showing an ordinary document.
///
/// `session_open_as` resets the engine's counter, and the editor's saved-at
/// baseline is not reset with it — every other open path calls `markSaved()`
/// immediately afterwards, which a recovered draft must not, because a
/// recovered draft has no file behind it. So this module keeps its own baseline
/// for the one document where the editor's is meaningless.
let recoveredBaseline = null;

function editsApplied() {
  try {
    return wasm.session_edits_applied();
  } catch {
    return Number.NaN;
  }
}

/// The counter reading this document's "nothing to keep" state corresponds to.
function baselineEdits() {
  return recoveredBaseline === null ? savedAtEditsForDraft() : recoveredBaseline;
}

/// How far the document is ahead of the last save — the sentence the recovery
/// bar states instead of the word "unsaved" (`docs/83` §4.3).
///
/// It is the counter's difference and nothing cleverer, which means it
/// over-counts after an undo: an edit and its undo leave the document where it
/// was and the number at 2. That is the engine's counter being what its doc
/// comment says it is, and correcting it here would be the tally over write
/// paths this editor has twice refused to keep.
function aheadNow() {
  const now = editsApplied();
  if (!Number.isFinite(now)) return 0;
  return Math.max(0, now - baselineEdits());
}

/// Whether a draft is worth writing at all.
///
/// **Not `isDirty()` on its own.** The question is not "does the document
/// differ from the file" — that is true forever after one undo — but "has
/// anything happened since the last draft", which is what makes rewriting it
/// worth 424 ms of the main thread.
function hasSomethingToDraft() {
  const now = editsApplied();
  if (!Number.isFinite(now)) return false;
  return now !== (draftedAtEdits === null ? baselineEdits() : draftedAtEdits);
}

/// The one write. Returns the reason it fired, or `""`.
async function writeDraft(reason) {
  if (writing || storageFault) return "";
  writing = true;
  try {
    const at = editsApplied();
    let bytes;
    let format = "xlsx";
    try {
      // `session_save()` and not `session_save_native()`: a draft is not a
      // deliverable. Writing a `.csv` draft would discard the second sheet the
      // user is about to lose, which is the one thing a draft exists to stop.
      bytes = wasm.session_save();
      format = wasm.session_format();
    } catch (e) {
      // The engine could not serialize. Nothing to store, and nothing the user
      // can do about it — but autosave is now not happening, and the rule is
      // that it never stops quietly.
      faultAutosave(`Autosave off — the document could not be prepared (${errText(e)})`);
      return "";
    }
    const slot = await claimSlot();
    const meta = {
      id: `slot-${slot}`,
      schema: SCHEMA,
      build: String(BUILD),
      engine: engineVersion(),
      name: documentName() || "Untitled",
      format,
      edits: at,
      // Carried so the bar can state a *difference* rather than the word
      // "unsaved". Computed at capture rather than at display, because by the
      // time a draft is offered the session that wrote it is gone and its
      // baseline with it.
      ahead: aheadNow(),
      at: Date.now(),
      size: bytes ? bytes.length : 0,
    };
    try {
      await putDraft(meta, bytes);
    } catch (e) {
      // `docs/83` §5.5. There is no version ring to evict in Phase B — the ring
      // is `SAVE-08` — so the ladder has one rung left, and it is the one that
      // must never be skipped: **stop, and say so, standing rather than as a
      // toast**. What is deliberately not done is deleting a draft the user has
      // not seen to make room for one they have not seen either.
      if (e && (e.name === "QuotaExceededError" || /quota/i.test(String(e.message || e)))) {
        faultAutosave("Autosave off — no storage space");
      } else {
        faultAutosave(`Autosave off — this browser is not storing drafts (${errText(e)})`);
      }
      return "";
    }
    draftedAtEdits = at;
    lastDraftAt = Date.now();
    // Asked once, at the first successful write, which is the point at which
    // there is something to protect and the request has a reason a user can
    // evaluate. A prompt at boot for a document nobody has edited is the dialog
    // everybody dismisses. Refused persistence is not an error: the draft is
    // written either way, it is merely evictable.
    requestPersistenceOnce();
    return reason;
  } finally {
    writing = false;
  }
}

function engineVersion() {
  try { return String(wasm.version()); } catch { return ""; }
}

let askedToPersist = false;
function requestPersistenceOnce() {
  if (askedToPersist) return;
  askedToPersist = true;
  try {
    navigator.storage?.persist?.().catch(() => {});
  } catch {}
}

// --- The standing indicator -------------------------------------------------
//
// `docs/83` §5.5: "a standing indicator in the status bar … not a toast". A
// toast for this is a message that appears at the moment the user is typing and
// is gone by the time they wonder whether their work is safe. The whole point
// of the state is that it persists, so its report does.

function faultAutosave(message) {
  if (storageFault === message) return;
  storageFault = message;
  const el = byId("autosave-state");
  if (el) {
    el.textContent = message;
    el.hidden = false;
  }
  stopScheduler();
}

/// Why autosave is not running, or `""` when it is.
export function autosaveFault() {
  return storageFault;
}

// --- The scheduler ----------------------------------------------------------

let lastReason = "";

/// The trigger that fired last, for a test that has to assert *which* one did —
/// the same reason `SaveReason` exists on the server.
export function lastDraftReason() {
  return lastReason;
}

function tick() {
  const now = Date.now();
  const edits = editsApplied();
  // **The counter went down, so this is not the same document any more.**
  //
  // `session_edits_applied()` only ever counts up *within a session*
  // (`crates/casual-calc-wasm/src/history.rs:14`), and `session_new()` and
  // `session_open_as()` both start a session at zero. So a fall is not an
  // anomaly to tolerate — it is the one unambiguous signal that File ▸ New or
  // File ▸ Open has replaced what is on screen.
  //
  // Found by measuring rather than by reading, and it is worse than it looks.
  // Everything below compares a count against `draftedAtEdits`, a count taken
  // against the *previous* document; after a replacement those two numbers are
  // about different workbooks, and where they happen to agree — File ▸ New
  // followed by exactly as many edits as the last draft had — the scheduler
  // concludes nothing has happened and **the new document is never drafted at
  // all**, while the old document's draft sits in this tab's slot looking like
  // the current work.
  //
  // The slot is deliberately kept and reused. A replacement is not silent: the
  // editor confirms before discarding unsaved work (`editor.core.js`, File ▸
  // New / File ▸ Open), so by the time this fires the user has said that the
  // previous document goes.
  if (Number.isFinite(edits) && Number.isFinite(lastEditsSeen) && edits < lastEditsSeen) {
    draftedAtEdits = null;
    recoveredBaseline = null;
    openedDraftId = null;
    lastDraftAt = now;
  }
  if (edits !== lastEditsSeen) {
    lastEditsSeen = edits;
    lastEditAt = now;
  }
  // **The work is somewhere the user chose, so the insurance is spent.** A
  // completed save is the one event that makes this tab's own draft redundant,
  // and dropping it then is what keeps the store from growing a copy per
  // session for a user who saves normally.
  //
  // Two guards, both load-bearing. Only this tab's own draft — the recovery bar
  // is full of other sessions' work and none of it is settled by this one being
  // saved. And **never for a document recovered from a draft**: it has no file
  // behind it, so `isDirty()` going false there means the counter has wandered
  // back to a baseline, not that anything is safe.
  if (recoveredBaseline === null && draftedAtEdits !== null && mySlot !== null && !isDirty()) {
    const id = `slot-${mySlot}`;
    draftedAtEdits = null;
    deleteDraft(id).then(() => refreshRecoveryBar()).catch(() => {});
    return;
  }
  if (!hasSomethingToDraft()) return;
  let reason = "";
  if (now - lastEditAt >= policy.quiesceMs) reason = "quiesced";
  else if (lastDraftAt && now - lastDraftAt >= policy.ceilingMs) reason = "ceiling";
  else if (
    draftedAtEdits !== null &&
    Number.isFinite(edits) &&
    edits - draftedAtEdits >= policy.everyEdits
  ) reason = "edits";
  if (!reason) return;
  writeDraft(reason)
    .then((fired) => { if (fired) lastReason = fired; })
    .catch((e) => faultAutosave(`Autosave off — this browser is not storing drafts (${errText(e)})`));
}

function startScheduler() {
  if (timer) return;
  lastEditsSeen = editsApplied();
  lastEditAt = Date.now();
  // **From the start of the session, not from the first draft.** Left at 0 the
  // ceiling is dead until a draft has already been written, and the case it
  // exists for is precisely the one where none has: somebody typing steadily
  // from a fresh page never quiesces, so the quiesce rule never fires and a
  // ceiling measured from "the last draft" has no last draft to measure from.
  // An hour of continuous work would have produced nothing at all. The server's
  // wording is the right reading — `ceiling_ms` is "the longest a session may
  // go without saving" (`lifecycle.rs`), and a session starts here.
  lastDraftAt = Date.now();
  timer = setInterval(tick, policy.pollMs);
  wireQuietWrite();
}

function stopScheduler() {
  if (!timer) return;
  clearInterval(timer);
  timer = null;
}

/// The trigger the server calls `Closing`, in the shape a browser tab has.
///
/// Without it the quiet window is a hole in the promise: a user types and then
/// closes the tab, backgrounds it, or switches to another app inside those five
/// seconds, and the last edits were never written. "Closing a tab stops losing
/// work" has to mean the last thing they typed, and quiesce alone does not.
///
/// **`visibilitychange`, and not `beforeunload`.** `beforeunload` does not fire
/// for the case this feature is named after — a tab the operating system killed
/// — and a browser is free to discard a backgrounded tab without ever running
/// it. `visibilityState === "hidden"` is the last point a page is reliably
/// given, and it is also the *cheap* one: the tab is not on screen, so the
/// ~0.4 s `session_save()` measured for a large workbook costs nobody a frame.
///
/// The write is best-effort by construction — an IndexedDB transaction started
/// as a tab goes away may not commit — which is why it is an addition to the
/// quiesce rule and not a replacement for it.
let quietWriteWired = false;
function wireQuietWrite() {
  if (quietWriteWired) return;
  quietWriteWired = true;
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "hidden") return;
    if (!timer || storageFault || !hasSomethingToDraft()) return;
    writeDraft("hidden")
      .then((fired) => { if (fired) lastReason = fired; })
      .catch(() => {});
  });
}

/// Restart the poll at the current `pollMs`, for a test that shortened it.
export function restartDraftSchedulerForTest() {
  stopScheduler();
  storageFault = "";
  const el = byId("autosave-state");
  if (el) { el.hidden = true; el.textContent = ""; }
  startScheduler();
}

// --- The recovery bar -------------------------------------------------------
//
// `docs/83` §4.3. The rule is one sentence — **a recovered document is offered,
// never applied** — and everything below is the consequence.
//
// The bar does **not** try to match a draft to the document being opened.
// Matching by name is a guess (two files called `budget.xlsx`, one on the
// desktop and one in Downloads) and a wrong guess here silently hands somebody
// else's work to a user. So every draft is listed, named, dated, and left for a
// person to decide about.

function timeLabel(ms) {
  const d = new Date(ms);
  if (!Number.isFinite(d.getTime())) return "an unknown time";
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const sameDay = new Date().toDateString() === d.toDateString();
  return sameDay ? `${time} today` : `${time} on ${d.toLocaleDateString()}`;
}

/// Whether a record is one this build can act on, and why not when it is not.
function unreadableReason(meta) {
  if (!meta) return "written in a form this version does not recognise";
  // A reason recorded by an attempt that already failed wins over the schema
  // number, because `markUnreadable` raises the number to park the entry and
  // the two mean different things: "this build is too old for it" is not the
  // same sentence as "the bytes are cut short", and telling a user to go and
  // find a newer OpenCalc when the file is truncated sends them nowhere.
  if (typeof meta.unreadable === "string" && meta.unreadable) return meta.unreadable;
  if (typeof meta.schema !== "number") return "written in a form this version does not recognise";
  if (meta.schema > SCHEMA) return `written by a newer version of OpenCalc (draft format ${meta.schema}, this build reads ${SCHEMA})`;
  return "";
}

/// The drafts worth offering: every one in the store except the one this page
/// is already showing.
///
/// There is deliberately no "is this still ahead of the last save" test here.
/// `docs/83` §4.3 asks for one, and the honest place to answer it is the
/// *writer*: a draft is only ever written for a document that differs from its
/// last save, and the tab deletes its own draft the moment the document is
/// saved (see `tick`). Answering it again at display time would mean comparing
/// a counter from a session that has ended against a baseline that no longer
/// exists — which is how a recovered document's own draft would quietly stop
/// being offered, the one outcome this bar must never produce.
function offerable(rows) {
  return rows.filter((r) => r.id !== openedDraftId);
}

let dismissed = new Set();

function bar() {
  return byId("oc-recovery");
}

/// Draw the bar from the store. Hidden when there is nothing to offer.
export async function refreshRecoveryBar() {
  const el = bar();
  if (!el) return [];
  let rows = [];
  try {
    rows = offerable(await listDrafts());
  } catch {
    // The store is unreadable. That is the autosave fault's business, not the
    // bar's: an empty bar is the honest presentation of "there are no drafts we
    // can see", and the standing indicator is where the reason lives.
    rows = [];
  }
  const showing = rows.filter((r) => !dismissed.has(r.id));
  el.replaceChildren();
  if (!showing.length) {
    el.hidden = true;
    return rows;
  }
  for (const meta of showing) el.append(entryFor(meta));
  el.hidden = false;
  return rows;
}

function entryFor(meta) {
  const row = document.createElement("div");
  row.className = "oc-recovery-row";
  row.dataset.draftId = meta.id;

  const strong = document.createElement("strong");
  strong.textContent = "Unsaved work from an earlier session";
  row.append(strong);

  // `textContent`, never markup. The name comes out of a file the user opened,
  // and this repository has already paid once for re-parsing a status line that
  // quoted a workbook.
  const ahead = typeof meta.ahead === "number" ? meta.ahead : 0;
  const detail = document.createElement("span");
  detail.className = "oc-recovery-detail";
  detail.textContent =
    ` — ${meta.name || "Untitled"}, ${timeLabel(meta.at)}, ${ahead} edit${ahead === 1 ? "" : "s"} ahead of the last save.`;
  row.append(detail);

  const note = document.createElement("span");
  note.className = "oc-recovery-note";
  const bad = unreadableReason(meta);
  // Said here rather than discovered afterwards. `History` derives only
  // `Debug, Default` and carries no `Serialize` (`casual-calc-transaction`), so
  // a snapshot cannot carry an undo stack even by accident — this is a
  // structural limit, and a user who finds it out by pressing Ctrl+Z has been
  // let down by the bar rather than by the engine.
  note.textContent = bad ? ` This draft was ${bad}.` : " Undo history is not recovered.";
  row.append(note);

  const actions = document.createElement("span");
  actions.className = "oc-recovery-actions";

  if (bad) {
    // §5.7 and the newer-build case share one answer: keep the entry, mark it,
    // and hand the bytes over so a user can take them somewhere else. A
    // recovery feature that silently discards the thing it failed to recover is
    // worse than no recovery feature.
    const take = document.createElement("button");
    take.className = "oc-btn";
    take.id = `oc-recovery-download-${meta.id}`;
    take.textContent = "Download the file";
    take.addEventListener("click", () => { downloadDraft(meta).catch((e) => statusError(errText(e))); });
    actions.append(take);
  } else {
    const review = document.createElement("button");
    review.className = "oc-btn primary";
    review.id = `oc-recovery-review-${meta.id}`;
    review.textContent = "Review";
    review.addEventListener("click", () => { reviewDraft(meta).catch((e) => statusError(errText(e))); });
    actions.append(review);
  }

  const discard = document.createElement("button");
  discard.className = "oc-btn";
  discard.id = `oc-recovery-discard-${meta.id}`;
  discard.textContent = "Discard";
  discard.addEventListener("click", () => { discardDraft(meta).catch((e) => statusError(errText(e))); });
  actions.append(discard);

  // **Declining is not deleting.** Dismissing the bar puts it away for this
  // page and leaves the draft exactly where it is, so the next boot offers it
  // again. A draft is never deleted because the user ignored it — only because
  // they said so, on an entry that named the document.
  const later = document.createElement("button");
  later.className = "oc-btn";
  later.id = `oc-recovery-later-${meta.id}`;
  later.textContent = "Not now";
  later.addEventListener("click", () => {
    dismissed.add(meta.id);
    refreshRecoveryBar().catch(() => {});
  });
  actions.append(later);

  row.append(actions);
  return row;
}

async function downloadDraft(meta) {
  const bytes = await readDraftBytes(meta.id);
  if (!bytes) { statusError(`the draft of ${meta.name} has no stored bytes`); return; }
  const blob = new Blob([bytes], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${(meta.name || "draft").replace(/\.[^.]+$/, "")}-draft.${meta.format || "xlsx"}`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/// **Review — the whole of "offered, never applied".**
///
/// A draft opens as a *separate document*, in its own tab, beside the one
/// already on screen. Nothing is merged and nothing is replaced: `docs/83` §7
/// refuses a merge outright, because there is no three-way merge for a
/// spreadsheet that is right often enough to be applied without being read.
///
/// A blocked popup is the one case where that is not available. It falls back
/// to loading in this window, **behind a confirmation that says so** — which is
/// the user asking for it, not the editor deciding.
async function reviewDraft(meta) {
  const url = new URL(location.href);
  url.searchParams.set("draft", meta.id);
  let opened = null;
  try { opened = window.open(url.toString(), "_blank"); } catch { opened = null; }
  if (opened) {
    status.textContent = `opened the draft of ${meta.name} in another tab`;
    return;
  }
  const ok = await confirmModal(
    `Open the draft of ${meta.name || "Untitled"}?`,
    `This browser would not open a second tab, so the draft from ${timeLabel(meta.at)} will replace what is on screen here. Nothing is merged, the draft is kept either way, and undo history is not recovered.`,
    "Open the draft here",
  );
  if (!ok) return;
  await loadDraftHere(meta);
}

async function loadDraftHere(meta) {
  const bytes = await readDraftBytes(meta.id);
  if (!bytes) { markUnreadable(meta, "stored without its bytes"); return; }
  const ok = openBytes(bytes, meta.name || `draft.${meta.format || "xlsx"}`);
  if (!ok) {
    // §5.7. The draft is kept and marked; `openBytes` has already put the
    // engine's own sentence in the status bar.
    markUnreadable(meta, "not readable — the file may have been cut short by a crash");
    return;
  }
  openedDraftId = meta.id;
  // A recovered draft has no file behind it, so the document stays dirty and
  // `beforeunload` stays armed. `markSaved()` here would be the editor telling
  // the user their recovered work is on disk somewhere, which it is not — and
  // this module keeps its own baseline instead, because the engine's counter
  // was reset by the open and the editor's was not.
  recoveredBaseline = editsApplied();
  status.textContent = `recovered ${meta.name} from ${timeLabel(meta.at)} — undo history is not recovered`;
  await refreshRecoveryBar();
}

/// Record on the entry that this build cannot read it, without deleting it.
function markUnreadable(meta, why) {
  statusError(`the draft of ${meta.name} is ${why}`);
  openDb()
    .then(async (db) => {
      const tx = db.transaction("meta", "readwrite");
      // `schema: Infinity` would not survive a structured clone; a number one
      // past this build is the same statement and does.
      tx.objectStore("meta").put({ ...meta, schema: SCHEMA + 1, unreadable: why });
      await txDone(tx);
      await refreshRecoveryBar();
    })
    .catch(() => {});
}

async function discardDraft(meta) {
  const ok = await confirmModal(
    `Discard the draft of ${meta.name || "Untitled"}?`,
    `The unsaved work from ${timeLabel(meta.at)} is deleted from this browser and cannot be brought back. The document open here is not affected.`,
    "Discard the draft",
  );
  if (!ok) return;
  try {
    await deleteDraft(meta.id);
  } catch (e) {
    statusError(`could not discard the draft: ${errText(e)}`);
    return;
  }
  // The slot is free again. Only a user saying so releases one — which is the
  // asymmetry the lease is built on, and the reason Discard confirms first.
  const slot = slotOf(meta.id);
  if (slot !== null) reservedSlots.delete(slot);
  status.textContent = `discarded the draft of ${meta.name}`;
  await refreshRecoveryBar();
}

// --- Boot -------------------------------------------------------------------

/// Whether `initDrafts` has finished. A test that asserts "no draft was
/// written" has to know the difference between *decided not to* and *has not
/// got there yet*, and there is no other signal for it: the status line says
/// `engine v…` before this runs.
let initialised = false;

/// Start drafts for this page. Never throws: a failure here must not be the
/// reason an editor does not open.
export async function initDrafts() {
  initialised = false;
  const caps = getCapabilities();
  // `docs/83` §3.3 and §7. The host owns durability, and a host's document must
  // not leave a copy in the user's browser storage as a side effect of being
  // opened. Nothing is written, nothing is read, and no bar appears.
  if (caps.ownsFile) { initialised = true; return { autosave: false, why: "the host owns this document" }; }
  // The same refusal, for the case the capability cannot see. `ownsFile` is
  // what a host *says*; this is what is true whatever it says. An embed that
  // sets no mode resolves to `standalone` — every permission granted — so
  // without this every `<opencalc-sheet>` and every framed `editor.html` would
  // write its host's document into the visitor's browser storage, which is the
  // exact side effect `docs/83` §3.3 forbids.
  if (!editorIsThePage()) { initialised = true; return { autosave: false, why: "this editor is part of another page" }; }

  const asked = new URLSearchParams(location.search).get("draft");
  if (asked) {
    try {
      const rows = await listDrafts();
      const meta = rows.find((r) => r.id === asked);
      if (meta) {
        const bad = unreadableReason(meta);
        if (bad) statusError(`the draft of ${meta.name} was ${bad}`);
        else await loadDraftHere(meta);
      } else {
        statusError("that draft is no longer in this browser");
      }
    } catch (e) {
      statusError(`could not read the draft: ${errText(e)}`);
    }
  }

  // The bar's own entries are what the lease has to keep away from, so this
  // runs **before** the scheduler exists, not merely before its first write:
  // the whole failure is a race between the first autosave and knowing which
  // slots are spoken for.
  try {
    const offered = await refreshRecoveryBar();
    reservedSlots = new Set(offered.map((r) => slotOf(r.id)).filter((n) => n !== null));
  } catch {}

  // Probe the store before promising anything. An editor that says nothing and
  // stores nothing is the state this whole row exists to end.
  try {
    await openDb();
  } catch (e) {
    faultAutosave(`Autosave off — this browser is not storing drafts (${errText(e)})`);
    initialised = true;
    return { autosave: false, why: storageFault };
  }
  startScheduler();
  initialised = true;
  return { autosave: true, why: "" };
}

/// For a test: everything this module holds, without reaching into the DOM.
export function draftStateForTest() {
  return {
    initialised,
    fault: storageFault,
    slot: mySlot,
    reserved: [...reservedSlots],
    draftedAtEdits,
    recoveredBaseline,
    ahead: aheadNow(),
    openedDraftId,
    reason: lastReason,
    running: !!timer,
  };
}

/// Make the store fail the way a real browser makes it fail.
///
/// Neither of these can be provoked from inside a page any other way. A test
/// cannot open a private-browsing profile, and it cannot fill a ~1 GB quota in
/// a gate that has to finish in a minute — but both arrive here as one specific
/// rejection from one specific call, and that is what is reproduced:
///
/// - `"unavailable"` rejects the `open`, which is what Firefox's private mode
///   does and what a browser with `indexedDB` withheld does.
/// - `"quota"` opens and then rejects the write with `QuotaExceededError`,
///   which is what a full origin does and, importantly, does so **after** the
///   editor has already promised itself autosave — the order that makes §5.5's
///   "stop and say so" the interesting case rather than a boot-time refusal.
export function breakDraftStoreForTest(kind = "unavailable") {
  stopScheduler();
  storageFault = "";
  const el = byId("autosave-state");
  if (el) { el.hidden = true; el.textContent = ""; }
  if (kind === "quota") {
    injectedWriteFault = "quota";
  } else {
    injectedWriteFault = "";
    dbPromise = Promise.reject(new Error("IndexedDB is not available in this browser"));
    dbPromise.catch(() => {});
  }
}
