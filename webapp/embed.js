// OpenCalc as an embeddable element: `<opencalc-sheet>`.
//
// The editor is the same code that runs the standalone page; the only
// difference is where it is mounted. Here it goes inside a **shadow root**,
// which is the one thing in the platform that actually stops a host page's
// stylesheet from reaching in — and, just as importantly, stops the editor's
// own selectors from reaching out onto the host's page.
//
// A shadow boundary blocks selectors, not inheritance: `font`, `color`,
// `line-height`, `letter-spacing` and `direction` still cross it. `editor.css`
// severs those with `all: initial` on `:host`. What it deliberately does *not*
// sever is custom properties, because `all` never touches them — which is
// exactly what makes `--oc-*` the theming API rather than an implementation
// detail.
//
//   <opencalc-sheet></opencalc-sheet>
//
//   const sheet = document.querySelector("opencalc-sheet");
//   sheet.theme({ accentColor: "#c026d3", backgroundColor: "#fbfbfd" });
//   sheet.configure({ calculation: "manual", readOnly: true });
//   await sheet.open(bytes);          // an .xlsx as a Uint8Array
//   const saved = sheet.save();
//
// The element is deliberately thin. Everything it exposes is something the
// engine already answers; it adds no state of its own, because a second copy
// of the workbook is a second thing to keep in step.

/// Where the package's own files sit, used when the host says nothing.
const HERE = new URL(".", import.meta.url);

/// Resolve the asset base for one element.
///
/// `assets-url` exists because `import.meta.url` resolution does not work
/// everywhere: Turbopack does not treat `.wasm` as an emitted asset, so a
/// Next.js host copies the files into `public/` and points at them. It is also
/// the only mode where the host controls cache headers on a multi-megabyte
/// binary.
///
/// Resolved against the *document*, not this module, because a host writes the
/// path they serve from (`/opencalc/`) and not one relative to wherever their
/// bundler happened to put our JavaScript.
function assetBase(el) {
  const given = el.getAttribute("assets-url") ?? el.assetsUrl;
  if (!given) return HERE;
  return new URL(given.endsWith("/") ? given : `${given}/`, document.baseURI);
}
/// The cache-buster the dev server stamps on this module, reused for the
/// editor's own assets so an embedded editor is never a build behind the page.
const BUILD = new URL(import.meta.url).searchParams.get("v") || "dev";

/// The chrome regions a host can show or hide.
///
/// The app header is off by default: the brand mark, the alpha badge, the file
/// button and the settings gear belong to *this* project's demo page, and an
/// embedded editor is the host's product, not ours.
const CHROME = [
  "header", "menubar", "toolbar", "formulabar", "tabs", "statusbar", "localePicker",
];
// The header is this project's own branding; the language picker is off
// because most hosts drive the language from their account settings, and a
// second control that disagrees with the first is worse than none.
const CHROME_DEFAULT = { header: false, localePicker: false };

/// The theme tokens a host may set, without the `--oc-` prefix.
///
/// Named rather than open-ended so a typo throws at the call site instead of
/// silently not changing a colour. The names are **typed by suffix** — `Color`
/// takes a colour, `Shadow` a box-shadow, `FontFamily` a font stack — which is
/// AG Grid's convention and worth matching: `borderColor` cannot be mistaken
/// for somewhere to put `1px solid red`.
///
/// Accepted in camelCase (`accentColor`) and written as kebab-case custom
/// properties (`--oc-accent-color`), so the JS reads like JS and the CSS reads
/// like CSS.
const TOKENS = [
  "backgroundColor", "textColor", "mutedTextColor", "faintTextColor",
  "iconColor", "disabledColor",
  "borderColor", "borderHoverColor", "controlBorderColor",
  "surfaceColor", "popoverBackgroundColor", "gridlineColor",
  "accentColor", "accentHoverColor", "accentContrastColor",
  "selectionColor", "findHighlightColor", "freezeLineColor",
  "successColor", "dangerColor",
  "tableHeaderColor", "tableBandColor",
  "tooltipBackgroundColor", "tooltipTextColor",
  "controlShadow", "popoverShadow", "monoFontFamily",
];

/// `accentColor` -> `--oc-accent-color`.
const cssVar = (name) => "--oc-" + name.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());

/// How many elements have mounted, so each gets a distinct module instance.
let instances = 0;

/// The one stylesheet object every mount adopts. Null where constructable
/// stylesheets are unavailable, in which case each root gets its own `<style>`.
let sharedSheet = null;

/// Fetch the editor's markup and stylesheet once per asset base.
///
/// Keyed by base rather than global: two elements pointed at different builds
/// is unusual but not incoherent, and a single cache would serve one of them
/// the other's markup.
const assets = new Map();
function loadAssets(base) {
  const key = String(base);
  if (!assets.has(key)) {
    assets.set(key, Promise.all([
      fetch(new URL(`editor.html?v=${BUILD}`, base)).then((r) => r.text()),
      fetch(new URL(`editor.css?v=${BUILD}`, base)).then((r) => r.text()),
    ]).then(([html, css]) => {
      if (typeof CSSStyleSheet === "function" && "replaceSync" in CSSStyleSheet.prototype) {
        sharedSheet = new CSSStyleSheet();
        sharedSheet.replaceSync(css);
      }
      return { markup: bodyOf(html), css };
    }));
  }
  return assets.get(key);
}

/// The editor page's body, minus its scripts.
///
/// Taken from the page rather than duplicated here so the two cannot drift:
/// a control added to the editor appears in an embed without anyone
/// remembering to copy it across.
function bodyOf(html) {
  const open = html.indexOf("<body");
  const start = html.indexOf(">", open) + 1;
  const end = html.lastIndexOf("</body>");
  return html
    .slice(start, end)
    .replace(/<script[\s\S]*?<\/script>/g, "");
}

class OpenCalcSheet extends HTMLElement {
  #shell = null;
  #ready = null;
  #access = "edit";
  /// Host-supplied tokens, kept per scheme — see `theme()`.
  #tokens = { light: {}, dark: {} };
  #watchScheme = null;
  #editor = null;
  #chrome = { ...CHROME_DEFAULT };

  connectedCallback() {
    if (!this.#ready) this.#ready = this.#mount();
    // Follow the OS while no explicit scheme is set, so a host that supplied a
    // dark palette gets it when the user's machine turns dark at sunset.
    if (!this.#watchScheme) {
      this.#watchScheme = matchMedia("(prefers-color-scheme: dark)");
      this.#watchScheme.addEventListener("change", () => {
        if (!this.hasAttribute("data-theme")) this.#applyTokens();
      });
    }
  }

  async #mount() {
    const root = this.attachShadow({ mode: "open" });
    const base = assetBase(this);
    const { markup, css } = await loadAssets(base);

    // `@font-face` inside a shadow root is ignored by the CSS engine — font
    // faces resolve against the document, not the tree. The rules are hoisted
    // once into the page so the bundled metric-compatible faces (Carlito for
    // Calibri, and the rest) are available to the canvas, which is what makes a
    // cell's font render the same on a machine that has none of them.
    hoistFontFaces(css, base);

    // One `CSSStyleSheet` shared by every mount rather than a `<style>` each.
    // This is the duplication AG Grid warns about in shadow DOM: four editors on
    // a page would otherwise parse the whole stylesheet four times. Adopting a
    // single sheet also means a change to it reaches every instance at once.
    if (sharedSheet) {
      root.adoptedStyleSheets = [sharedSheet];
    } else {
      const style = document.createElement("style");
      style.textContent = css;
      root.append(style);
    }

    const shell = document.createElement("div");
    shell.className = "editor-body";
    shell.innerHTML = markup;
    root.append(shell);
    this.#shell = shell;
    this.#applyChrome();

    // A *fresh* module per element, not the shared one. `editor.js` keeps its
    // state at module scope — one engine binding, one selection, one geometry
    // cache — so two elements importing the same instance share and race all of
    // it: mounting three left all three stuck at "loading engine…". Varying the
    // URL is what gives each its own module scope, and `start(key)` does the
    // same for the wasm glue underneath so each element gets its own workbook.
    const key = String(++instances);
    const editor = await import(
      /* @vite-ignore */ new URL(`editor.js?v=${BUILD}&i=${key}`, base).href
    );
    editor.setMountRoot(root);
    await editor.start(key);
    this.#editor = editor;
    this.dispatchEvent(new CustomEvent("ready"));
    return editor;
  }

  /// Resolves once the engine is up and the grid is on screen.
  get ready() {
    return this.#ready ?? (this.#ready = this.#mount());
  }

  /// Show or hide chrome: `{ toolbar: false, statusbar: false }`.
  ///
  /// Named regions rather than CSS: a host reaching into the shadow root to
  /// hide `.toolbar` would break the moment that markup moved, and this is a
  /// contract the editor can keep.
  chrome(regions) {
    for (const [name, show] of Object.entries(regions)) {
      if (!CHROME.includes(name)) {
        throw new Error(
          `unknown OpenCalc chrome region "${name}" — one of: ${CHROME.join(", ")}`,
        );
      }
      this.#chrome[name] = !!show;
    }
    this.#applyChrome();
    // Hiding a region changes how much room the grid has.
    this.#editor?.relayout?.();
    return this;
  }

  #applyChrome() {
    if (!this.#shell) return;
    for (const name of CHROME) {
      // Preview *overrides* the host's preference rather than replacing it.
      // Overwriting `#chrome` meant leaving preview restored "whatever the host
      // asked for" — which by then was preview's own all-off, so the chrome
      // never came back.
      const shown = this.#access === "preview" ? false : this.#chrome[name] ?? true;
      this.#shell.classList.toggle(`oc-hide-${name.toLowerCase()}`, !shown);
      if (name === "localePicker") this.#editor?.setLocalePicker?.(shown);
    }
  }

  /// Hide or disable individual commands by id.
  ///
  ///   sheet.commands({ hidden: ["file.open"], disabled: ["insert.chart"] });
  ///
  /// Hidden and disabled differ on purpose. A capability the host has not
  /// implemented yet should be *disabled* — a user who cannot see a thing
  /// assumes it does not exist and stops looking. One that makes no sense in
  /// the host's product should be *hidden*.
  ///
  /// `listCommands()` returns every id this build has, so the list can be
  /// discovered rather than read from documentation that may have moved on.
  async commands(rules) {
    const editor = await this.ready;
    editor.setCommandRules(rules);
    return this;
  }

  /// Listen for an event. Returns an unsubscribe function.
  ///
  ///   const stop = await sheet.on("cellsChanged", (e) => {
  ///     if (e.source === "api") return;   // our own write — do not loop
  ///     save(e.range);
  ///   });
  ///
  /// `before*` events can be cancelled, either by calling `preventDefault()` or
  /// by returning `false`:
  ///
  ///   sheet.on("beforeCellsChanged", (e) => {
  ///     if (!user.canEdit(e.range)) e.preventDefault();
  ///   });
  async on(name, handler) {
    const editor = await this.ready;
    return editor.on(name, handler);
  }

  async off(name, handler) {
    const editor = await this.ready;
    editor.off(name, handler);
    return this;
  }

  /// Every command id in this build.
  async listCommands() {
    const editor = await this.ready;
    return editor.listCommands();
  }

  /// Set theme tokens.
  ///
  ///   sheet.theme({ accentColor: "#7c3aed" });                 // both schemes
  ///   sheet.theme({ light: { backgroundColor: "#fbf9f4" },     // per scheme
  ///                 dark:  { backgroundColor: "#171512" } });
  ///
  /// The per-scheme form exists because the flat one cannot express it. Tokens
  /// are written as inline custom properties on this element, and an inline
  /// style beats every rule in the stylesheet — including the dark-mode block.
  /// So a host that set `backgroundColor` once got that colour in dark mode
  /// too, and its careful dark palette silently did nothing. Here the element
  /// keeps both sets and writes whichever the effective scheme calls for,
  /// re-applying when the scheme changes under it.
  ///
  /// Calls **merge**, so setting one token does not restate the other
  /// twenty-six. `null` clears a single token back to the editor's own
  /// default, and [`resetTheme`](#resetTheme) clears the lot.
  ///
  /// Tokens are validated: a typo throws rather than quietly not changing a
  /// colour.
  theme(tokens) {
    const scoped = tokens && (tokens.light || tokens.dark);
    if (scoped) {
      this.#tokens.light = { ...this.#tokens.light, ...(tokens.light ?? {}) };
      this.#tokens.dark = { ...this.#tokens.dark, ...(tokens.dark ?? {}) };
    } else {
      for (const scheme of ["light", "dark"]) {
        this.#tokens[scheme] = { ...this.#tokens[scheme], ...tokens };
      }
    }
    for (const set of [tokens?.light, tokens?.dark, scoped ? null : tokens]) {
      for (const name of Object.keys(set ?? {})) {
        if (!TOKENS.includes(name)) {
          throw new Error(
            `unknown OpenCalc theme token "${name}" — one of: ${TOKENS.join(", ")}`,
          );
        }
      }
    }
    this.#applyTokens();
    return this;
  }

  /// Drop every host-supplied token and go back to the editor's own palette.
  ///
  /// Needed because `theme()` merges: without this there is no way to undo a
  /// preset, and a host offering a theme picker would accumulate whatever
  /// every previously-chosen theme happened to set.
  resetTheme() {
    this.#tokens = { light: {}, dark: {} };
    this.#applyTokens();
    return this;
  }

  /// Which scheme is actually in force: the attribute if set, else the host's
  /// `prefers-color-scheme`.
  get #effectiveScheme() {
    const attr = this.getAttribute("data-theme");
    if (attr === "light" || attr === "dark") return attr;
    return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  #applyTokens() {
    const set = this.#tokens[this.#effectiveScheme];
    // Every known token is cleared first, so switching schemes does not leave
    // the other one's colours behind on tokens the new set does not mention.
    for (const name of TOKENS) this.style.removeProperty(cssVar(name));
    for (const [name, value] of Object.entries(set)) {
      if (value !== null && value !== undefined) this.style.setProperty(cssVar(name), value);
    }
    // The canvas caches the resolved tokens: it paints thousands of cells a
    // frame and cannot re-read a computed style per cell. Without this the
    // chrome restyles instantly and the grid keeps the old colours until
    // something else forces a repaint.
    this.#editor?.refreshTheme?.();
  }

  /// Light, dark, or follow the host's `prefers-color-scheme`.
  ///
  /// Set on this element rather than on `<html>`, so two embedded editors on
  /// one page can differ and neither restyles the page around it.
  setColorScheme(scheme) {
    if (scheme === "auto") this.removeAttribute("data-theme");
    else this.setAttribute("data-theme", scheme);
    // Re-applies the per-scheme tokens, not merely the stylesheet's.
    this.#applyTokens();
    return this;
  }

  /// Engine configuration — the host-facing knobs, by the same names the Rust
  /// `SessionConfig` uses.
  ///
  ///   {
  ///     calculation: "auto" | "manual",
  ///     access: "edit" | "view" | "preview",
  ///     locale: "de-DE",
  ///     messages: { "de-DE": { "command.format.bold": "Fett" } },
  ///   }
  ///
  /// Messages are keyed by **command id**, which is derived from the English
  /// label — so translating a menu never renumbers the command API. A missing
  /// key falls back to the English string, which means a partial catalogue
  /// degrades to "some of it is translated" rather than to visible keys.
  ///
  /// **`edit`** — the editor.
  ///
  /// **`view`** is an *access level*: this person is working in the sheet and
  /// may not change it. They get the whole application minus the editing —
  /// scroll, select, navigate sheets, zoom, copy, find, follow links, expand
  /// outlines, read comments, export, print, recalculate. The chrome stays;
  /// only the commands that would write are taken off it. It is what a
  /// permission system means by read-only.
  ///
  /// **`preview`** is a *presentation*: a thumbnail, a row in a file list, an
  /// attachment rendered inline. Not a workspace. No chrome at all, and no
  /// affordances suggesting there is anything to do here — someone may still
  /// select a range and copy it, because that costs nothing and refusing it
  /// only annoys. The point is that it reads as a picture of a document rather
  /// than an application someone has been locked out of.
  ///
  /// Conflating them produces the two worst outcomes in this area: a viewer
  /// that looks like a broken editor because it is full of greyed-out menus,
  /// and a thumbnail that invites clicking on things it will then refuse.
  ///
  /// Both refuse writes **in the engine**, not by hiding chrome — the UI is
  /// how it is communicated, not how it is enforced.
  async configure(options = {}) {
    const editor = await this.ready;
    if (options.calculation) {
      editor.wasmApi().session_set_calculation_mode(options.calculation);
    }
    // Catalogues before the locale, or switching to a language whose strings
    // have not arrived yet falls back to English and then never re-renders.
    for (const [code, map] of Object.entries(options.messages ?? {})) {
      editor.setMessages(code, map);
    }
    if (options.locale) editor.setLocale(options.locale);
    // `readOnly` / `preview` booleans remain as sugar over the same axis, so
    // there is one source of truth and they cannot disagree.
    let access = options.access;
    if (access === undefined && options.preview !== undefined) {
      access = options.preview ? "preview" : "edit";
    }
    if (access === undefined && options.readOnly !== undefined) {
      access = options.readOnly ? "view" : "edit";
    }
    if (access !== undefined) this.#setAccess(access, editor);
    return this;
  }

  #setAccess(access, editor) {
    if (!["edit", "view", "preview"].includes(access)) {
      throw new Error(`unknown OpenCalc access "${access}" — one of: edit, view, preview`);
    }
    this.#access = access;
    editor.setReadOnly(access !== "edit");
    // `#applyChrome` reads `#access`, so preview hides everything and leaving
    // it restores exactly what the host had chosen — not everything, and not
    // preview's own emptiness.
    this.#applyChrome();
    editor.relayout?.();
    // A preview shows the top-left of the sheet. Removing the chrome changes
    // how much grid there is, and whatever was scrolled into view for an
    // editor is not what a thumbnail should show.
    if (access === "preview") editor.resetToOrigin?.();
  }

  /// The access level in force.
  get access() {
    return this.#access;
  }

  /// Open an `.xlsx` (or CSV/TSV/PSV) from bytes.
  async open(bytes, name = "workbook.xlsx") {
    const editor = await this.ready;
    return editor.openBytes(new Uint8Array(bytes), name);
  }

  /// The workbook as `.xlsx` bytes.
  async save() {
    const editor = await this.ready;
    return editor.wasmApi().session_save();
  }
}

/// Copy the stylesheet's `@font-face` rules into the page, once.
///
/// They are the one kind of rule a shadow root cannot host: font faces are
/// resolved per document, so a face declared only inside the shadow tree is
/// never registered and every bundled family silently falls back.
const fontsHoisted = new Set();
function hoistFontFaces(css, base) {
  if (fontsHoisted.has(String(base))) return;
  fontsHoisted.add(String(base));
  const faces = css.match(/@font-face\s*\{[^}]*\}/g);
  if (!faces) return;
  const style = document.createElement("style");
  style.dataset.opencalcFonts = "";
  // The URLs are relative to the stylesheet, which is not where this style
  // element lives — so they are resolved against it explicitly.
  style.textContent = faces
    .join("\n")
    .replace(/url\("\.\/([^"]+)"\)/g, (_, path) => `url("${new URL(path, base)}")`);
  document.head.append(style);
}

if (!customElements.get("opencalc-sheet")) {
  customElements.define("opencalc-sheet", OpenCalcSheet);
}

export { OpenCalcSheet, TOKENS as THEME_TOKENS };
