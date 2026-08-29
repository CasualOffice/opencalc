// Remote participants: who is here, where they are, and what they are
// mid-edit.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  A1,
  PRESENCE_FACES,
  activeEl,
  announceCollabSelection,
  byId,
  canvas,
  closePresence,
  collabRoster,
  collabSession,
  collaborate,
  colors,
  draw,
  editHome,
  editSurface,
  el,
  ensureVisible,
  fInput,
  imageCache,
  inline,
  invalidateGrowth,
  liveEl,
  off,
  on,
  openPresence,
  presenceOpen,
  recalculateNow,
  refMirrors,
  renderTabs,
  select,
  setReadOnly,
  setTip,
  sheetNameAt,
  state,
  status,
  statusError,
  stopCollaborating,
  switchSheet,
  syncClock,
  t,
  wasm,
} from "./editor.core.js";

export function wasStopped(err) {
  return /OC-IMP-0007/.test(String(err && err.message ? err.message : err));
}

export function clearKeepWaiting() {
  byId("keep-waiting")?.remove();
}

export function offerKeepWaiting(what, again) {
  const bar = byId("tb-status");
  if (!bar || !bar.parentNode) return;
  clearKeepWaiting();
  const note = document.createElement("span");
  note.className = "warn";
  note.textContent = ` — stopped ${what} before it finished`;
  bar.append(note);
  const btn = document.createElement("button");
  btn.id = "keep-waiting";
  btn.className = "oc-btn keep-waiting";
  btn.textContent = "Keep waiting";
  btn.addEventListener("click", () => { btn.remove(); again(); });
  bar.parentNode.insertBefore(btn, bar.nextSibling);
}

export function relativeTime(iso) {
  if (!iso) return "";
  const then = Date.parse(iso.endsWith("Z") ? iso : iso + "Z");
  if (!Number.isFinite(then)) return "";
  const secs = Math.round((Date.now() - then) / 1000);
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} minute${mins === 1 ? "" : "s"} ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return new Date(then).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

export function mirrorFor(surface) {
  let m = refMirrors.get(surface);
  if (m) return m;
  m = document.createElement("div");
  m.className = "ref-mirror";
  m.setAttribute("aria-hidden", "true");
  // Inserted as a sibling so it shares the surface's containing block.
  surface.parentNode.insertBefore(m, surface);
  refMirrors.set(surface, m);
  return m;
}

export function syncMirrorBox(surface, m) {
  const cs = getComputedStyle(surface);
  for (const prop of [
    "fontFamily", "fontSize", "fontWeight", "fontStyle", "letterSpacing",
    "lineHeight", "textIndent", "paddingTop", "paddingRight", "paddingBottom",
    "paddingLeft", "borderTopWidth", "borderRightWidth", "borderBottomWidth",
    "borderLeftWidth", "boxSizing", "whiteSpace", "textAlign",
  ]) m.style[prop] = cs[prop];
  m.style.left = surface.offsetLeft + "px";
  m.style.top = surface.offsetTop + "px";
  m.style.width = surface.offsetWidth + "px";
  m.style.height = surface.offsetHeight + "px";
}

export function mirrorEdit() {
  if (!editSurface) return;
  if (editSurface === inline) fInput.value = inline.value;
  else inline.value = fInput.value;
  // Every path that changes the text of an open edit passes through here — the
  // keystroke handler, reference insertion, autocomplete, the anchor cycle — so
  // this is where the others find out what is being typed. Hooking the `input`
  // event instead would miss every programmatic change, which is most of the
  // interesting ones in a formula.
  announceCollabSelection();
}

export function collaborators() {
  return [...collabRoster.values()];
}

export function participantName(who) {
  const name = typeof who?.name === "string" ? who.name.trim() : "";
  return name || t("presence.someone", "someone");
}

export function participantInitials(name) {
  const words = String(name ?? "").trim().split(/\s+/).filter(Boolean);
  const first = (w) => Array.from(w)[0] ?? "";
  const out = words.length > 1 ? first(words[0]) + first(words[1]) : first(words[0] ?? "");
  return (out || "?").toUpperCase();
}

export function participantChannels(color) {
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(color);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join("") : hex[1];
    return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
  }
  // A named colour or an `rgb()`, which `participantColor` passes through when
  // the browser agrees it is a colour. Ask the browser what it resolved to.
  const probe = new Option().style;
  probe.color = color;
  const m = /^rgba?\(([^)]+)\)$/.exec(probe.color);
  if (!m) return null;
  const parts = m[1].split(/[,\s/]+/).filter(Boolean).map(Number).slice(0, 3);
  return parts.length === 3 && parts.every(Number.isFinite) ? parts : null;
}

export function participantInk(color) {
  const rgb = participantChannels(color);
  if (!rgb) return "#ffffff";
  // Rec. 709 luma — green carries most of the perceived brightness, which a
  // plain average of the channels gets wrong for yellows and blues.
  const luma = (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255;
  return luma > 0.6 ? "#0b0d12" : "#ffffff";
}

export function participantFace(who) {
  const color = participantColor(who.color);
  const face = el("span", "presence-face", participantInitials(who.name));
  // Assigned through CSSOM, which parses a *value* and drops what it cannot
  // parse. It cannot become markup, and `participantColor` has already refused
  // anything the browser does not read as a colour.
  face.style.background = color;
  face.style.color = participantInk(color);
  if (who.editing) face.classList.add("typing");
  return face;
}

export function presenceCell(who) {
  const ok = (n) => Number.isInteger(n) && n >= 0;
  const draft = who?.editing;
  // A draft wins over the selection: while a formula is being written the
  // selection walks off to pick references (see `collabDraft`), so the cell
  // they are *in* is the one they are typing into, not the one highlighted.
  if (draft && Array.isArray(draft.at) && draft.at.length === 2 && draft.at.every(ok)) {
    return { r0: draft.at[0], c0: draft.at[1], r1: draft.at[0], c1: draft.at[1] };
  }
  const sel = who?.selection;
  if (!Array.isArray(sel) || sel.length !== 4 || !sel.every(ok)) return null;
  return {
    r0: Math.min(sel[0], sel[2]),
    c0: Math.min(sel[1], sel[3]),
    r1: Math.max(sel[0], sel[2]),
    c1: Math.max(sel[1], sel[3]),
  };
}

export function presenceWhere(who) {
  const sheet = Number.isInteger(who?.sheet) ? who.sheet : -1;
  const named = sheet >= 0 ? sheetNameAt(sheet) : null;
  // A sheet index this client cannot name is still worth reporting: it means
  // they are on a sheet that was added or removed under us, and "Sheet 3" is
  // more use than an empty line.
  const where = typeof named === "string" && named ? named : `${t("presence.sheet", "Sheet")} ${sheet + 1}`;
  const at = presenceCell(who);
  const a = at ? A1(at.r0, at.c0) : "";
  const b = at ? A1(at.r1, at.c1) : "";
  const ref = !at ? "" : a === b ? a : `${a}:${b}`;
  return {
    text: ref ? `${where}!${ref}` : where,
    // Their sheet is not the one on screen, so going to them means leaving it.
    elsewhere: sheet >= 0 && sheet !== state.sheet,
  };
}

export function presenceRow(who) {
  const name = participantName(who);
  const where = presenceWhere(who);
  const row = el("button", "presence-item");
  row.type = "button";
  row.setAttribute("role", "menuitem");
  // Read back after a rebuild to restore focus; never interpolated into a
  // selector, because a client id is not this editor's string to trust.
  row.dataset.client = String(who.client ?? "");
  if (where.elsewhere) row.classList.add("elsewhere");
  row.appendChild(participantFace(who));
  const who2 = el("span", "presence-who");
  who2.appendChild(el("span", "presence-name", name));
  who2.appendChild(el("span", "presence-where", where.text));
  row.appendChild(who2);
  const typing = who.editing ? t("presence.typing", "typing") : "";
  if (typing) row.appendChild(el("span", "presence-typing", typing));
  // The visible text plus what the row does, because "Grace Hopper Budget!D8"
  // read aloud does not say that activating it goes there.
  row.setAttribute(
    "aria-label",
    `${t("presence.goto", "Go to")} ${name}, ${where.text}${typing ? `, ${typing}` : ""}`,
  );
  row.addEventListener("click", () => {
    closePresence();
    jumpToParticipant(who);
  });
  return row;
}

export function renderPresence() {
  const box = byId("presence");
  const btn = byId("presence-btn");
  const faces = byId("presence-faces");
  const label = byId("presence-label");
  const menu = byId("presence-menu");
  // A host that removed this chrome, or a call before the mount is bound.
  if (!box || !btn || !faces || !label || !menu) return;

  // No session, nothing to say. The editor is single-player most of the time
  // and a permanent "only you" chip is noise in a bar that carries none.
  if (!collabSession) {
    box.hidden = true;
    if (presenceOpen) closePresence();
    return;
  }
  box.hidden = false;

  // Sorted by name, not by arrival: the list is read to *find* somebody, and
  // an order that changes as people move is an order nobody can search.
  const others = collaborators().slice().sort((a, b) => {
    const byName = participantName(a).localeCompare(participantName(b));
    return byName || String(a.client ?? "").localeCompare(String(b.client ?? ""));
  });

  faces.textContent = "";
  for (const who of others.slice(0, PRESENCE_FACES)) faces.appendChild(participantFace(who));
  if (others.length > PRESENCE_FACES) {
    faces.appendChild(
      el("span", "presence-face presence-more", `+${others.length - PRESENCE_FACES}`),
    );
  }

  const count = others.length;
  label.textContent =
    count === 0
      ? t("presence.alone", "Only you")
      : count === 1
        ? t("presence.one", "1 other")
        : t("presence.many", "%n others").replace("%n", String(count));
  // `setTip` writes the tooltip *and* the accessible name, through whichever
  // surface this control ended up using. The names are in it because a screen
  // reader user should not have to open a menu to learn whether they are alone.
  setTip(
    btn,
    count === 0
      ? t("presence.tip-alone", "Collaborators — you are the only one here")
      : `${t("presence.tip", "Collaborators")} — ${label.textContent}: ${others
          .map(participantName)
          .join(", ")}`,
  );

  // The list itself is only built while it is on screen. Presence arrives about
  // six times a second per person who is typing, and rebuilding twenty rows
  // nobody is looking at, sixscore times a second, is work the grid wants for
  // drawing. `openPresence` builds it on the way open.
  if (!presenceOpen) {
    menu.textContent = "";
    return;
  }

  // What had focus, so a rebuild under an open menu does not steal it.
  const focused = presenceItems().includes(activeEl()) ? activeEl().dataset.client : null;
  const scrolled = menu.scrollTop;
  menu.textContent = "";
  if (!others.length) {
    // A disabled item rather than a bare line of text: a `role="menu"` with no
    // `menuitem` in it is a menu a screen reader reads as empty, which is not
    // the same thing as being told you are on your own.
    const empty = el("div", "presence-empty", t("presence.empty", "You are the only one here."));
    empty.setAttribute("role", "menuitem");
    empty.setAttribute("aria-disabled", "true");
    menu.appendChild(empty);
  } else {
    for (const who of others) menu.appendChild(presenceRow(who));
  }
  menu.scrollTop = scrolled;
  if (focused !== null) {
    for (const item of presenceItems()) {
      if (item.dataset.client === focused) { item.focus(); break; }
    }
  }
}

export function presenceItems() {
  const menu = byId("presence-menu");
  return menu ? [...menu.querySelectorAll(".presence-item")] : [];
}

export function jumpToParticipant(who) {
  const sheet = Number.isInteger(who?.sheet) ? who.sheet : -1;
  if (sheet >= 0 && sheet !== state.sheet) {
    let count = 0;
    try { count = JSON.parse(wasm.session_sheet_names()).length; } catch {}
    // A sheet this client does not have is one that was deleted under it (or
    // added and not yet applied): there is nowhere to go, so the jump keeps the
    // view it has rather than switching to an index that does not exist.
    if (sheet < count) switchSheet(sheet);
  }
  const at = presenceCell(who);
  if (at) {
    // `ensureVisible` twice rather than fresh geometry: it is the same scroll
    // every other jump in this editor performs, so it cannot disagree with
    // them, and it moves the minimum — a participant already on screen does not
    // throw the view around. Far corner first so the near one wins when their
    // range is bigger than the viewport; the top-left of a block is the part
    // you want to be looking at.
    ensureVisible(at.r1, at.c1);
    ensureVisible(at.r0, at.c0);
  }
  draw();
  canvas?.focus();
  // Said out loud, because for a screen-reader user the whole effect of this
  // click is a canvas that scrolled.
  if (liveEl) liveEl.textContent = `${participantName(who)} — ${presenceWhere(who).text}`;
}

export function wirePresence() {
  const box = byId("presence");
  const btn = byId("presence-btn");
  const menu = byId("presence-menu");
  if (!box || !btn || !menu) return;

  // Put back where it belongs: `buildMenuBar()` appends File…Help *after*
  // whatever the markup held, and `hdr-collapse` re-appends itself last, so
  // relying on markup order would leave the roster to the left of the File
  // menu. Right-most but one, beside the collapse caret.
  const bar = byId("menubar");
  if (bar) bar.insertBefore(box, byId("hdr-collapse") ?? null);

  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    if (presenceOpen) closePresence(); else openPresence();
  });
  btn.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      openPresence();
      const items = presenceItems();
      (e.key === "ArrowDown" ? items[0] : items[items.length - 1])?.focus();
      e.preventDefault();
    } else if (e.key === "Escape" && presenceOpen) {
      closePresence();
      e.preventDefault();
    }
  });

  // Outside-click closes, like every other popover here. `composedPath` rather
  // than `contains(e.target)`: inside a shadow root a click is retargeted to
  // the host element, so `contains` answers "no" for clicks that were in fact
  // ours — the path is the one view of the click that crosses the boundary.
  document.addEventListener("click", (e) => {
    if (!presenceOpen) return;
    const path = e.composedPath ? e.composedPath() : [e.target];
    if (!path.includes(box)) closePresence();
  });

  menu.addEventListener("keydown", (e) => {
    const items = presenceItems();
    const at = items.indexOf(activeEl());
    const step = (i) => { items[((i % items.length) + items.length) % items.length]?.focus(); };
    if (e.key === "ArrowDown") { step(at + 1); e.preventDefault(); }
    else if (e.key === "ArrowUp") { step(at - 1); e.preventDefault(); }
    else if (e.key === "Home") { step(0); e.preventDefault(); }
    else if (e.key === "End") { step(items.length - 1); e.preventDefault(); }
    else if (e.key === "Escape") { closePresence(true); e.preventDefault(); }
    // Tab out of a menu closes it. Not prevented: the focus is going somewhere
    // sensible on its own, and trapping it here would trap it for good.
    else if (e.key === "Tab") closePresence();
  });

  renderPresence();
}

export function adoptCollabDocument(event) {
  if (event.reason === "joined") {
    // The whole workbook was replaced by the session's snapshot, so this is the
    // same refresh a file open needs and for the same reason — every cache
    // below is keyed to a document that is no longer there.
    invalidateGrowth();
    imageCache.clear();
    syncClock();
    try { state.sheet = wasm.session_active_sheet(); } catch { state.sheet = 0; }
    state.scrollX = state.scrollY = 0;
    renderTabs();
    select(0, 0);
    // The engine refuses the edit, not the toolbar. A viewer whose buttons were
    // merely hidden is one bug away from editing a document they may not.
    if (event.editable === false) setReadOnly(true);
    return;
  }
  // A remote edit. Cheaper than a join — the model is continuous — but the
  // sheet list can have changed too, since adding or renaming one is an
  // ordinary operation like any other.
  invalidateGrowth();
  renderTabs();
  draw();

  // The edit itself is in the document; its recalculation ran out of budget.
  // Said out loud and offered a way to finish, exactly as a local one is
  // (`COL-43`): a sheet that is a mixture of fresh and stale values must not be
  // presented as final, and the difference between "somebody else typed" and
  // "these numbers do not follow from the formulas above them" is the whole
  // reason the outcome is reported at all.
  if (event.stale) {
    statusError("calculation stopped — some values are still out of date");
    offerKeepWaiting("calculating", () => recalculateNow(-1));
  }
}

export function participantColor(raw) {
  if (typeof raw !== "string" || !raw) return colors.accent;
  const hex = raw.trim();
  if (/^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) return hex;
  if (/^([0-9a-f]{3}|[0-9a-f]{6})$/i.test(hex)) return `#${hex}`;
  // Anything else — a named colour, an rgb() — is passed through only if the
  // browser agrees it is a colour, so a malformed token cannot blank a cursor.
  const probe = new Option().style;
  probe.color = hex;
  return probe.color ? hex : colors.accent;
}

export function collabDraft() {
  if (!editSurface || !editHome) return null;
  if (editHome.sheet !== state.sheet) return null;
  return { at: [editHome.row, editHome.col], text: editSurface.value };
}

// --- Share ------------------------------------------------------------------
//
// The route into a collaborative session, and the reason it is closed by
// default.
//
// # What was missing
//
// Below the line there is a clustered OT server with a leader per document,
// epoch-fenced appends, relay, resume and presence, exercised by two real
// browsers in CI. Above the line there was **nothing**: `listCommands()`
// matched nothing against `share|collab|invite`, and `collaborate()` could only
// be reached by a host writing JavaScript against the module namespace.
//
// `docs/12` §3.22 says a session is joined "by putting `?doc=` on the URL". It
// is not: nothing in the editor has ever read `?doc=` off the page URL —
// `collab.js:150` puts `doc` on the *WebSocket* URL, from the key the caller
// already passed in. There was no user-reachable route at all, by query string
// or otherwise. That is a finding for `docs/12`, not something to fix by prose.
//
// # Why it is behind a capability, off by default
//
// `COL-46` is an open **P0**: a `$`-anchored formula rebased across a
// concurrent insert lands as `$E$1` on one replica and `$D$1` on the other,
// with no error raised anywhere. Two replicas of one document holding different
// formulas is the worst failure class this system has, and a Share button that
// walked a user into it silently would be worse than no button — they would
// have been told the feature was ready.
//
// So the route is **built and complete**, and the door has two locks, because
// they protect different people:
//
//   1. `canShare` is `false` in every mode preset, so `File ▸ Share…` is absent
//      from a plain editor and `runCommand("file.share")` is refused. Closing
//      `COL-46` is then a one-word change in `MODE_PRESETS` rather than a
//      feature to build, which is the point of wiring it now.
//   2. A host that turns it on (`setCapabilities({ canShare: true })`) gets the
//      divergence named, in the dialog, at the moment of sharing, and cannot
//      start a session without acknowledging it. A host flipping a flag is not
//      the person whose formulas diverge.
//
// # Why nothing here auto-connects
//
// `?collab=` and `?doc=` **prefill** the fields and never start a session. An
// editor that opened a WebSocket to a URL-supplied host on load would be an
// automatic network fetch decided by whoever handed the user the link, which
// `AGENTS.md` §"Engineering priorities" 3 rules out. The endpoint is shown, and
// a human presses the button.
//
// The token is deliberately **not** read from the URL and never put into the
// invite link. It is a credential; a credential on a URL is a credential in
// browser history, in the referrer, and in whatever the link was pasted into.
// The host mints one per user — that is what `tokenFor` does in the collab gate
// — so the link carries the document and the recipient brings their own.

/// Host-supplied defaults for the Share dialog: `{ url, token, document }`.
///
/// A host that already knows its endpoint and can mint a token sets these once
/// and the user never types anything. Every field is optional.
let shareDefaults = {};

/// Set them. Returns what is now held, with the token reported as present
/// rather than echoed — a getter that hands a credential back to any script on
/// the page is a second way to leak the thing the section above protects.
export function setShareDefaults(partial) {
  if (partial && typeof partial === "object") shareDefaults = { ...shareDefaults, ...partial };
  return { url: shareDefaults.url || "", document: shareDefaults.document || "", token: !!shareDefaults.token };
}

/// A document key for a session that does not have one yet.
///
/// Random rather than derived from the file name: the key is what the server
/// keys a document by, so two people who both have a `Budget.xlsx` and guess
/// the obvious key would land in each other's session.
function newDocumentKey() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `doc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/// The link a collaborator opens. Document and endpoint, never the token.
function inviteLink(url, key) {
  const link = new URL(location.origin + location.pathname);
  if (url) link.searchParams.set("collab", url);
  link.searchParams.set("doc", key);
  return link.toString();
}

/// A labelled text field for the dialog.
function shareField(id, label, value, placeholder) {
  const wrap = el("label", "share-field");
  wrap.appendChild(el("span", "share-field-label", label));
  const input = document.createElement("input");
  input.type = "text";
  input.id = id;
  input.className = "oc-input";
  input.value = value || "";
  if (placeholder) input.placeholder = placeholder;
  // Every field here is an address or an opaque token; a browser that
  // capitalises or spell-checks them is corrupting them.
  input.autocapitalize = "off";
  input.autocomplete = "off";
  input.spellcheck = false;
  wrap.appendChild(input);
  return wrap;
}

export function shareDialog() {
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  if (!modal || !body) return;
  byId("oc-modal-title").textContent = t("share.title", "Share this workbook");
  body.textContent = "";
  const close = () => { modal.hidden = true; body.textContent = ""; };
  body.appendChild(collabSession ? shareLive(close) : shareStart(close));
  modal.hidden = false;
}

/// The dialog for a session that is already running.
function shareLive(close) {
  const box = el("div", "share-body");
  const key = shareDefaults.document || "";
  const others = collabRoster.size;
  box.appendChild(
    el("p", "share-note",
      others === 0
        ? "Sharing — nobody else has joined yet."
        : `Sharing — ${others} other${others === 1 ? "" : "s"} connected.`),
  );

  const link = shareField("share-link", "Invite link", inviteLink(shareDefaults.url, key));
  link.querySelector("input").readOnly = true;
  box.appendChild(link);
  box.appendChild(
    el("p", "share-hint",
      "The link carries the document, not a token — whoever opens it needs one of their own from this deployment."),
  );

  const row = el("div", "oc-confirm-actions");
  const copy = document.createElement("button");
  copy.className = "oc-btn";
  copy.id = "share-copy";
  copy.textContent = "Copy link";
  copy.addEventListener("click", async () => {
    const input = byId("share-link");
    try { await navigator.clipboard.writeText(input.value); copy.textContent = "Copied"; }
    catch { input.select(); copy.textContent = "Press Ctrl+C"; }
  });
  const stop = document.createElement("button");
  stop.className = "oc-btn primary";
  stop.id = "share-stop";
  stop.textContent = "Stop sharing";
  stop.addEventListener("click", () => {
    stopCollaborating();
    status.textContent = "stopped sharing";
    close();
  });
  row.append(copy, stop);
  box.appendChild(row);
  return box;
}

/// The dialog for starting one, and the `COL-46` warning that gates it.
function shareStart(close) {
  const box = el("div", "share-body");

  // Named, and specific. "Collaboration is experimental" is the sentence that
  // gets skipped; a sentence saying which of your formulas can silently become
  // a different formula on someone else's screen is not.
  const warn = el("div", "share-warning");
  warn.id = "share-warning";
  warn.appendChild(el("strong", "", "Known defect: COL-46 (open, P0)"));
  warn.appendChild(
    el("p", "",
      "A formula with a $-anchored reference, rebased across an insert somebody else makes at "
      + "the same moment, can end up different on the two screens — $E$1 here, $D$1 there — with "
      + "no error shown to either of you. Check anchored formulas by hand after concurrent edits."),
  );
  box.appendChild(warn);

  const params = new URL(location.href).searchParams;
  box.appendChild(shareField(
    "share-url", "Collaboration server",
    shareDefaults.url || params.get("collab") || "",
    "wss://collab.example.com/collab",
  ));
  box.appendChild(shareField(
    "share-doc", "Document",
    shareDefaults.document || params.get("doc") || newDocumentKey(),
  ));
  // Not prefilled from the URL, on purpose — see the note at the top of this
  // section. A host that can mint one sets it through `setShareDefaults`.
  box.appendChild(shareField(
    "share-token", "Access token",
    shareDefaults.token || "",
    "the token this deployment issued you",
  ));

  const ack = el("label", "share-ack");
  const check = document.createElement("input");
  check.type = "checkbox";
  check.id = "share-ack";
  ack.appendChild(check);
  ack.appendChild(el("span", "", "I have read COL-46 and will check anchored formulas."));
  box.appendChild(ack);

  const row = el("div", "oc-confirm-actions");
  const cancel = document.createElement("button");
  cancel.className = "oc-btn";
  cancel.textContent = "Cancel";
  cancel.addEventListener("click", close);
  const start = document.createElement("button");
  start.className = "oc-btn primary";
  start.id = "share-start";
  start.textContent = "Start sharing";
  // Disabled rather than refusing on click: a button that can be pressed and
  // then complains has already taught the user to press first and read after.
  start.disabled = true;
  check.addEventListener("change", () => { start.disabled = !check.checked; });
  start.addEventListener("click", async () => {
    const url = byId("share-url").value.trim();
    const key = byId("share-doc").value.trim();
    const token = byId("share-token").value.trim();
    if (!url || !key || !token) {
      statusError("sharing needs a server, a document and a token");
      return;
    }
    start.disabled = true;
    start.textContent = "Connecting…";
    try {
      shareDefaults = { ...shareDefaults, url, document: key };
      await collaborate({ url, token, document: key });
      close();
    } catch (err) {
      // Left open, with the message in it. Closing the dialog on a failure
      // throws away the endpoint and the token just typed, which is exactly
      // what is needed in order to try again.
      start.disabled = false;
      start.textContent = "Start sharing";
      byId("share-error")?.remove();
      const failed = el("p", "share-error", String(err && err.message ? err.message : err));
      failed.id = "share-error";
      box.appendChild(failed);
      statusError(`could not start sharing: ${failed.textContent}`);
    }
  });
  row.append(cancel, start);
  box.appendChild(row);
  return box;
}
