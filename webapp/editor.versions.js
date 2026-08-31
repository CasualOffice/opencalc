// Version history that survives a reload (`HIST-03`), stored compressed
// (`SAVE-13`).
//
// `SAVE-08` built the store host-agnostic — `into_parts`/`from_parts`, and the
// clock the caller's — and `HIST-01` reached it from the editor. What neither
// did was carry it across a reload, so a version lived until the tab closed and
// no longer. **A history you lose by pressing F5 is not a history**, which is
// the gap this closes.
//
// ## Where the compression lives, and why not in the engine
//
// `SAVE-13` measured 17.82 MiB at 300k cells against 1.61 MiB gzipped, so a
// 50 MiB budget holds two versions uncompressed and thirty-one compressed. The
// byte ceiling, not the count, is what binds on a real workbook.
//
// It is done here rather than in the engine, and that was a deliberate refusal
// rather than an omission: putting a compressor into a core wasm-bound crate to
// spend ~300ms per capture is a design decision, and **the host has cheaper
// options** — a browser gets `CompressionStream` off the main thread for free.
// The engine's `byte_len` stays uncompressed, which keeps the retention
// arithmetic conservative rather than wrong: a store that budgeted against
// compressed sizes would hold more than it could restore if the codec changed.
//
// ## What is stored
//
// Two rows per version, the same split `editor.drafts.js` uses for drafts: the
// metadata is small JSON and enough to draw the whole panel, and the bytes are
// fetched only when somebody actually restores. Opening the panel therefore
// costs a few kilobytes, not a few megabytes.

import { documentName } from "./editor.sheets.js";

const DB_NAME = "opencalc-versions";
const DB_VERSION = 1;

let dbPromise = null;

function open() {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    let req;
    try {
      // Private browsing in some engines does not expose `indexedDB` at all,
      // and Firefox's private mode throws from `open()` rather than returning a
      // request that errors. Both are the same answer to the caller, and the
      // answer is "no history here", never a broken editor.
      if (typeof indexedDB === "undefined" || !indexedDB) throw new Error("no IndexedDB in this browser");
      req = indexedDB.open(DB_NAME, DB_VERSION);
    } catch (e) { reject(e); return; }
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains("meta")) db.createObjectStore("meta", { keyPath: "key" });
      if (!db.objectStoreNames.contains("bytes")) db.createObjectStore("bytes", { keyPath: "key" });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error || new Error("IndexedDB refused to open"));
    // A tab holding an older version open blocks the upgrade forever, and that
    // is not something the platform resolves on its own.
    req.onblocked = () => reject(new Error("another tab is holding an older database open"));
  }).catch((e) => { dbPromise = null; throw e; });
  return dbPromise;
}

const done = (req) => new Promise((resolve, reject) => {
  req.onsuccess = () => resolve(req.result);
  req.onerror = () => reject(req.error);
});

/// Which document a version belongs to.
///
/// Keyed by the document's name, so opening a different file does not show the
/// previous one's history. An unnamed workbook gets its own bucket rather than
/// sharing with every other unnamed one — which would mean starting a new
/// workbook inherited the last one's versions, and a version list that offers
/// to restore somebody else's document is worse than no list.
function docKey() {
  return documentName() || "__untitled__";
}

/// `CompressionStream` if this browser has it, otherwise identity.
///
/// Safari shipped it in 16.4 and every current engine has it, but a host may be
/// older or may be a webview with it disabled. The uncompressed path is not a
/// fallback to be ashamed of — it is what the engine already hands over — so
/// the flag travels with the row and decompression reads it rather than
/// guessing from the bytes.
async function deflate(bytes) {
  if (typeof CompressionStream === "undefined") return { data: bytes, codec: "" };
  try {
    const stream = new Blob([bytes]).stream().pipeThrough(new CompressionStream("gzip"));
    const packed = new Uint8Array(await new Response(stream).arrayBuffer());
    // Refuse a "compression" that made it bigger. Tiny snapshots do this, and
    // storing the larger of the two to be able to say the word gzip would be
    // spending the user's quota on a label.
    if (packed.byteLength >= bytes.byteLength) return { data: bytes, codec: "" };
    return { data: packed, codec: "gzip" };
  } catch {
    return { data: bytes, codec: "" };
  }
}

async function inflate(row) {
  if (row.codec !== "gzip") return new Uint8Array(row.data);
  const stream = new Blob([row.data]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

/// Write the session's versions to storage, replacing what was there.
///
/// Called after a capture and after a hide or rename, because all three change
/// what the next reload should show. The whole manifest is rewritten rather
/// than diffed: it is a few kilobytes, and a diff that got it wrong would drop
/// a version silently.
export async function persistVersions(wasm) {
  let manifest;
  try { manifest = JSON.parse(wasm.session_versions_manifest() || "[]"); }
  catch { return; }
  const doc = docKey();
  let db;
  try { db = await open(); } catch { return; }

  // Bytes first, then the manifest. If the tab dies between the two, storage
  // holds bytes no manifest names — wasted space, cleaned up on the next write
  // — rather than a manifest naming bytes that are not there, which would be a
  // version the panel offers and cannot restore.
  for (const v of manifest) {
    const key = `${doc}:${v.id}`;
    const existing = await done(db.transaction("bytes").objectStore("bytes").get(key)).catch(() => null);
    if (existing) continue;   // snapshots are immutable; only metadata changes
    let raw;
    try { raw = wasm.session_version_bytes(v.id); } catch { continue; }
    const { data, codec } = await deflate(raw);
    const tx = db.transaction("bytes", "readwrite");
    await done(tx.objectStore("bytes").put({ key, data, codec })).catch(() => {});
  }

  const tx = db.transaction("meta", "readwrite");
  const store = tx.objectStore("meta");
  await done(store.put({ key: doc, versions: manifest })).catch(() => {});

  // Drop bytes for versions the retention ring evicted, so the store does not
  // grow forever behind a list that shrank.
  const live = new Set(manifest.map((v) => `${doc}:${v.id}`));
  const btx = db.transaction("bytes", "readwrite");
  const all = await done(btx.objectStore("bytes").getAllKeys()).catch(() => []);
  for (const key of all) {
    if (typeof key === "string" && key.startsWith(`${doc}:`) && !live.has(key)) {
      const dtx = db.transaction("bytes", "readwrite");
      await done(dtx.objectStore("bytes").delete(key)).catch(() => {});
    }
  }
}

/// Load this document's versions back into the session.
///
/// Returns how many came back, so a caller can say so rather than leaving the
/// user to notice. Failure is silent by design: a browser with no IndexedDB, a
/// full quota or a corrupted row must produce an editor with no history, never
/// an editor that will not start.
export async function loadVersions(wasm) {
  const doc = docKey();
  let db;
  try { db = await open(); } catch { return 0; }
  const row = await done(db.transaction("meta").objectStore("meta").get(doc)).catch(() => null);
  if (!row || !Array.isArray(row.versions)) return 0;

  let restored = 0;
  for (const v of row.versions) {
    const key = `${doc}:${v.id}`;
    const stored = await done(db.transaction("bytes").objectStore("bytes").get(key)).catch(() => null);
    if (!stored) continue;   // a manifest entry with no bytes is not offered
    let bytes;
    try { bytes = await inflate(stored); } catch { continue; }
    try {
      wasm.session_version_add(JSON.stringify(v), bytes);
      restored += 1;
    } catch { /* one unreadable version must not take the rest with it */ }
  }
  return restored;
}

/// Forget this document's history. For `File ▸ New`, which is a different
/// document and must not inherit one.
export async function forgetVersions() {
  const doc = docKey();
  let db;
  try { db = await open(); } catch { return; }
  await done(db.transaction("meta", "readwrite").objectStore("meta").delete(doc)).catch(() => {});
  const keys = await done(db.transaction("bytes").objectStore("bytes").getAllKeys()).catch(() => []);
  for (const key of keys) {
    if (typeof key === "string" && key.startsWith(`${doc}:`)) {
      await done(db.transaction("bytes", "readwrite").objectStore("bytes").delete(key)).catch(() => {});
    }
  }
}
