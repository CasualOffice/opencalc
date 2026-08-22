// Locale, message lookup and the user-facing wording of errors and tips.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  byId,
  locale,
  menuMnemonics,
  messages,
  on,
  qsa,
  renderPresence,
  select,
  setLocale,
  sigEl,
  status,
  tipEl,
  tipTimer,
  updateCellMode,
  wasStopped,
} from "./editor.core.js";

export function fmtNum(n) {
  return Number.isFinite(n) ? (Math.round(n * 1e6) / 1e6).toLocaleString() : String(n);
}

export function friendlyFormulaError(err) {
  const m = err.replace(/^\[OC-[A-Z0-9-]+\]\s*/, ""); // drop the internal code
  if (/end of input/i.test(m)) return "the formula looks incomplete — check for a missing value or a closing ‘)’.";
  const t = m.match(/unexpected token:\s*(.+)$/i);
  if (t) {
    const sym = { Star: "*", Plus: "+", Minus: "-", Slash: "/", Caret: "^", RParen: "‘)’", LParen: "‘(’", Comma: "‘,’", Percent: "‘%’" }[t[1].trim()] || t[1].trim();
    return `unexpected ${sym} — check the formula syntax.`;
  }
  return m;
}

export function t(key, fallback) {
  return messages.get(locale)?.[key] ?? fallback;
}

export function setMessages(forLocale, map) {
  messages.set(forLocale, { ...(messages.get(forLocale) ?? {}), ...map });
  relabel();
}

export function setLocalePicker(on) {
  const box = byId("locale-picker");
  if (box) box.hidden = !on;
  syncLocalePicker();
}

export function syncLocalePicker() {
  const select = byId("locale-select");
  if (!select) return;
  const locales = availableLocales();
  if (select.options.length !== locales.length) {
    select.textContent = "";
    for (const code of locales) {
      const option = document.createElement("option");
      option.value = code;
      // The language's own name where the platform knows it, because a picker
      // that lists "German" to a German speaker is a picker for someone else.
      let label = code;
      try {
        label = new Intl.DisplayNames([code], { type: "language" }).of(code.split("-")[0]) ?? code;
      } catch { /* an unknown tag keeps its code, which is still choosable */ }
      option.textContent = label;
      select.append(option);
    }
    select.onchange = () => setLocale(select.value);
  }
  select.value = locale;
}

export function getLocale() {
  return locale;
}

export function availableLocales() {
  return ["en-US", ...[...messages.keys()].filter((l) => l !== "en-US")].sort();
}

export function relabelMenubar() {
  menuMnemonics.clear();
  for (const btn of qsa(".menubar .menu-top")) {
    const english = btn.dataset.ocLabel ?? btn.textContent;
    const name = t(`command.${btn.dataset.ocCommand}`, english);
    const index = Number(btn.dataset.ocMenuIndex ?? -1);
    let at = [...name].findIndex((ch) => !menuMnemonics.has(ch.toLowerCase()));
    if (at < 0) at = 0; // every letter taken: no mnemonic, but still labelled
    const key = name[at].toLowerCase();
    if (!menuMnemonics.has(key) && index >= 0) menuMnemonics.set(key, index);
    // The letter is wrapped so it can be underlined without changing layout.
    // Built as nodes rather than markup: a translated label is host-supplied
    // text and must not be able to inject elements.
    btn.textContent = "";
    btn.append(name.slice(0, at));
    const mn = document.createElement("span");
    mn.className = "mn";
    mn.textContent = name[at];
    btn.append(mn, name.slice(at + 1));
    btn.setAttribute("aria-keyshortcuts", `Alt+${name[at].toUpperCase()}`);
  }
}

export function relabel() {
  relabelMenubar();
  // The roster's own strings come from the catalogue too, and it is built in
  // JS rather than carried in the markup, so a language change has to rebuild
  // it. A no-op outside a session.
  renderPresence();
  for (const node of qsa("[data-oc-label]")) {
    if (node.classList.contains("menu-top")) continue; // handled above
    const id = node.dataset.ocCommand;
    const english = node.dataset.ocLabel;
    const text = id ? t(`command.${id}`, english) : english;
    const slot = node.querySelector(".mi-label");
    if (slot) slot.textContent = text;
    else node.textContent = text;
  }
  for (const node of qsa("[data-oc-tip]")) {
    const text = t(`tip.${node.dataset.ocCommand ?? node.id}`, node.dataset.ocTip);
    // Write it back where the tooltip is actually read from. A tipified node
    // has no `title` any more — setting one would translate nothing and
    // resurrect the native bubble beside our own.
    if (node.dataset.tip !== undefined) {
      node.dataset.tip = text;
      node.setAttribute("aria-label", text);
    } else {
      node.title = text;
    }
  }
  updateCellMode();
}

export function setTip(node, text) {
  if (node.dataset.tip !== undefined) node.dataset.tip = text;
  else node.title = text;
  node.setAttribute("aria-label", text);
}

export function tipify(node) {
  const t = node.getAttribute("title");
  if (!t) return;
  node.dataset.tip = t;
  if (!node.getAttribute("aria-label")) node.setAttribute("aria-label", t);
  node.removeAttribute("title");
}

export function showTip(node) {
  if (!tipEl || !node.dataset.tip) return;
  tipEl.textContent = node.dataset.tip;
  tipEl.hidden = false;
  const r = node.getBoundingClientRect();
  const tw = tipEl.offsetWidth, th = tipEl.offsetHeight;
  let left = Math.max(6, Math.min(r.left + r.width / 2 - tw / 2, window.innerWidth - tw - 6));
  let top = r.bottom + 6;
  if (top + th > window.innerHeight - 6) top = r.top - th - 6;
  tipEl.style.left = left + "px";
  tipEl.style.top = top + "px";
  tipEl.classList.add("show");
}

export function hideTip() {
  clearTimeout(tipTimer);
  if (tipEl) { tipEl.classList.remove("show"); tipEl.hidden = true; }
}

export function friendlyOpenError(err, name, isText) {
  const text = String(err && err.message ? err.message : err);
  // Checked first, and by code: a stopped open is not a complaint about the
  // file, and telling someone their perfectly good workbook is unreadable
  // because it was large is worse than saying nothing.
  if (wasStopped(err)) {
    return `${name} was taking too long, so it was stopped — nothing was loaded`;
  }
  if (/is not a format this build can open/.test(text)) return text;
  if (/zip|central directory|not a valid/i.test(text)) {
    return `${name} is not a readable .xlsx — if it is an older .xls, re-save it as .xlsx first`;
  }
  if (/limit|too (large|many)|bound/i.test(text)) {
    return `${name} exceeds this build's size limits and was not opened`;
  }
  if (/utf-?8|invalid|encoding/i.test(text) && isText) {
    return `${name} is not text this build can decode — try saving it as UTF-8 CSV`;
  }
  return `could not open ${name}: ${text}`;
}

export function hideSignatureTip() { if (sigEl) sigEl.hidden = true; }

export function errText(e) {
  return String((e && e.message) || e).replace(/^Error:\s*/, "");
}

export function statusError(text) {
  status.textContent = "";
  const span = document.createElement("span");
  span.className = "err";
  span.textContent = text;
  status.appendChild(span);
}
