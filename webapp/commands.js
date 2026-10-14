// The command registry (UX-SHELL-06).
//
// One place that answers "what does command X do, and may it run now?" for the
// ribbon, the File page and command search. Before this, the ribbon looked a
// command up as whatever DOM node happened to carry its `data-oc-command`, so a
// verb that lived only in the right-click menu — Format Cells, Paste Special,
// Insert Function — simply did not exist as far as the ribbon was concerned:
// 23 of the 197 it names were absent at boot, and it hid the buttons.
//
// Two kinds of entry, one lookup:
//
//  - **registered** — `register(id, {label, run, ...})`, for commands that have
//    no menu node of their own. The editor core registers these at boot.
//  - **owned by a node** — every `[data-oc-command]` in the menus and the
//    hidden toolbar. Running one clicks that node, so the behaviour is the
//    menu's own, never a second copy of it.
//
// Imported without a query string by both `editor.core.js` and `ribbon.js`, so
// there is exactly one instance per editor module graph (the `MNT-005` rule
// `editor.js` documents). It is instanced *with* the editor and never apart from
// it: a registered command's `run` is a closure over that editor's own state.
//
// **Node lookup is scoped to the editor's root**, which the editor sets with
// `setRoot()` when it is mounted. An `<opencalc-sheet>` lives in a shadow root,
// and `document.querySelector` cannot see into one: every node-owned command —
// most of the ribbon — was absent there.

const registered = new Map();

/// The editor's command rules, installed by `editor.selection.js` so this
/// module needs no import back into the editor: `(id) => {hidden, disabled}`.
let policy = () => ({ hidden: false, disabled: false });
export function setPolicy(fn) { policy = fn; }
export function isRegistered(id) { return registered.has(id); }
export function registeredIds() { return [...registered.keys()]; }

/// Register a command that has no DOM node. `spec`: `{ label, run, shortcut?,
/// enabled?: () => boolean, checked?: () => boolean }`.
export function register(id, spec) {
  registered.set(id, { id, ...spec });
}

/// The `Document` or `ShadowRoot` the editor's menus and toolbar live in.
let root = null;
export function setRoot(r) { root = r; }
const scope = () => root || (typeof document === "undefined" ? null : document);

function nodeFor(id) {
  const r = scope();
  if (!r) return null;
  return r.querySelector(`[data-oc-command="${CSS.escape(id)}"]:not(.oc-ribbon *):not(.rb-backstage *)`);
}

/// The command, or `null` if this build has no such verb.
export function command(id) {
  const r = registered.get(id);
  if (r) {
    return {
      ...r,
      hidden: () => policy(id).hidden,
      enabled: () => { const p = policy(id); return !p.hidden && !p.disabled && (r.enabled ? r.enabled() : true); },
    };
  }
  const n = nodeFor(id);
  if (!n) return null;
  return {
    id,
    node: n,
    // A command's name, not its tooltip sentence: "Format painter — click to
    // pick up…" is cut at the dash, "Bold (Ctrl+B)" at the bracket.
    label: (n.querySelector(".mi-label")?.textContent || n.getAttribute("aria-label") || n.textContent || "")
      .trim().split(/\s+[—–-]\s+|\s*\(/)[0].trim(),
    run: () => n.click(),
    hidden: () => n.classList.contains("oc-cmd-hidden"),
    enabled: () => !n.disabled && !n.classList.contains("oc-cmd-hidden"),
    checked: () => {
      const p = n.getAttribute("aria-pressed") ?? n.getAttribute("aria-checked");
      return p === "true";
    },
  };
}

export function has(id) { return !!command(id); }

/// Run a command. Returns false when the command is absent or disabled.
export function run(id) {
  const c = command(id);
  if (!c) return false;
  if (c.enabled && !c.enabled()) return false;
  c.run();
  return true;
}

/// Every command this build can run, for search. Registered ones first, then
/// node-owned ones that are verbs (a bare family id like `data` is a menu's own
/// button, not a command).
export function all() {
  const out = new Map();
  for (const id of registered.keys()) {
    const c = command(id);
    if (!c.hidden()) out.set(id, c);
  }
  const r = scope();
  if (r) {
    for (const n of r.querySelectorAll("[data-oc-command]")) {
      const id = n.dataset.ocCommand;
      if (out.has(id) || !id.includes(".")) continue;
      if (n.closest(".oc-ribbon, .rb-backstage")) continue;
      const c = command(id);
      if (c && c.label && !c.hidden()) out.set(id, c);
    }
  }
  return [...out.values()];
}
