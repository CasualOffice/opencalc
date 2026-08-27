// Copy, cut and paste, including the cross-application HTML flavour.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  cellsFromClipboardHtml,
  clipToOS,
  draw,
  effectiveRange,
  lastClipTsv,
  state,
  status,
  stopMarch,
  wasm,
} from "./editor.core.js";

export function htmlText(raw) {
  return String(raw).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

export function download(data, name, type) {
  const blob = new Blob([data], { type });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}

// `clipToOS` arms the engine's own clipboard and starts the marching ants before
// it tries the system one, and reports failure only for that last step. So a
// refusal there does not mean the cut or copy did not happen — it means other
// applications will not see it. Reporting "blocked" said the opposite of what
// the engine had done: the ants were drawn, the cut was armed, and the next
// paste moved the data. `stopMarch`'s own comment names this pairing from the
// other side — "the visible signal said cancelled and the state said
// otherwise, which is the worst possible pairing for an action that deletes."
//
// The cut is not undone, because it works: inside this application it pastes
// exactly as it should. Only the half that failed is named.
const OS_CLIPBOARD_REFUSED = "the system clipboard was refused — other applications will not see it";

export async function doCopy() {
  status.textContent = (await clipToOS(effectiveRange(), false))
    ? "copied"
    : `copied here — ${OS_CLIPBOARD_REFUSED}`;
}

export async function doCut() {
  status.textContent = (await clipToOS(effectiveRange(), true))
    ? "cut"
    : `cut here — ${OS_CLIPBOARD_REFUSED}`;
}

export function doPasteMode(mode) {
  try {
    if (!wasm.session_clip_has()) { status.textContent = "clipboard is empty"; return; }
    wasm.session_clip_paste_mode(state.sheet, state.sel.row, state.sel.col, mode);
    if (!wasm.session_clip_has()) stopMarch(); // a cut was consumed
    draw();
    status.textContent = `pasted ${mode}`;
  } catch { status.textContent = "paste blocked"; }
}

export async function clipboardHtml(event) {
  const fromEvent = event?.clipboardData?.getData("text/html");
  if (fromEvent) return fromEvent;
  try {
    for (const item of await navigator.clipboard.read()) {
      if (item.types.includes("text/html")) return await (await item.getType("text/html")).text();
    }
  } catch {}
  return "";
}

export async function doPaste(event) {
  try {
    let osText = event?.clipboardData?.getData("text/plain") ?? "";
    if (!osText) { try { osText = await navigator.clipboard.readText(); } catch {} }
    // Internal rich paste when the OS clipboard is unchanged from our copy (or
    // unreadable but we hold a snapshot); else paste the external text.
    if (wasm.session_clip_has() && (osText === lastClipTsv || osText === "")) {
      wasm.session_clip_paste(state.sheet, state.sel.row, state.sel.col);
    } else {
      // From another application. Prefer the HTML flavour, which is the only
      // one carrying formatting — the plain text is the same grid with every
      // style thrown away, which is what this used to be able to do.
      const cells = cellsFromClipboardHtml(await clipboardHtml(event));
      if (cells) {
        wasm.session_paste_html(state.sheet, state.sel.row, state.sel.col, JSON.stringify(cells));
      } else {
        wasm.session_paste_tsv(state.sheet, state.sel.row, state.sel.col, osText);
      }
    }
    if (!wasm.session_clip_has()) stopMarch(); // a cut was consumed
    draw();
    status.textContent = "pasted";
  } catch { status.textContent = "paste blocked"; }
}

export function decodeTextBytes(bytes) {
  const enc = (label) => new TextEncoder().encode(new TextDecoder(label).decode(bytes));
  // Byte-order marks are definitive, so they are checked before anything else.
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) return enc("utf-16le");
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) return enc("utf-16be");
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return bytes.slice(3); // UTF-8 BOM: strip it, the rest is already UTF-8
  }
  // No BOM. A UTF-16 file without one still gives itself away: half its bytes
  // are zero. Sniffing beats failing, and the fallback is plain UTF-8.
  const probe = bytes.subarray(0, Math.min(bytes.length, 512));
  let zeros = 0;
  for (const b of probe) if (b === 0) zeros += 1;
  if (probe.length > 8 && zeros > probe.length / 4) {
    return enc(bytes[0] === 0 ? "utf-16be" : "utf-16le");
  }
  return bytes;
}
