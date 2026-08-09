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
//   sheet.theme({ accent: "#c026d3", bg: "#fbfbfd" });
//   sheet.configure({ calculation: "manual" });
//   await sheet.open(bytes);          // an .xlsx as a Uint8Array
//   const saved = sheet.save();
//
// The element is deliberately thin. Everything it exposes is something the
// engine already answers; it adds no state of its own, because a second copy
// of the workbook is a second thing to keep in step.

const HERE = new URL(".", import.meta.url);
/// The cache-buster the dev server stamps on this module, reused for the
/// editor's own assets so an embedded editor is never a build behind the page.
const BUILD = new URL(import.meta.url).searchParams.get("v") || "dev";

/// The chrome regions a host can show or hide.
///
/// The app header is off by default: the brand mark, the alpha badge, the file
/// button and the settings gear belong to *this* project's demo page, and an
/// embedded editor is the host's product, not ours.
const CHROME = ["header", "menubar", "toolbar", "formulabar", "tabs", "statusbar"];
const CHROME_DEFAULT = { header: false };

/// The theme tokens a host may set, without the `--oc-` prefix.
///
/// Named rather than open-ended so a typo is a thrown error at the call site
/// instead of a colour that silently does not change.
const TOKENS = [
  "bg", "fg", "icon", "muted", "faint", "disabled",
  "border", "border-2", "border-hover", "surface", "card", "grid",
  "accent", "accent-hover", "accent-ink", "sel-tint", "find-tint",
  "ok", "danger", "freeze-line", "mono",
  "table-header", "table-band", "tooltip-bg", "tooltip-fg",
  "shadow-btn", "shadow-pop",
];

/// Fetch the editor's markup and stylesheet once, however many elements mount.
let assets = null;
function loadAssets() {
  if (!assets) {
    assets = Promise.all([
      fetch(new URL(`editor.html?v=${BUILD}`, HERE)).then((r) => r.text()),
      fetch(new URL(`editor.css?v=${BUILD}`, HERE)).then((r) => r.text()),
    ]).then(([html, css]) => ({ markup: bodyOf(html), css }));
  }
  return assets;
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
  #editor = null;
  #chrome = { ...CHROME_DEFAULT };

  connectedCallback() {
    if (!this.#ready) this.#ready = this.#mount();
  }

  async #mount() {
    const root = this.attachShadow({ mode: "open" });
    const { markup, css } = await loadAssets();

    // `@font-face` inside a shadow root is ignored by the CSS engine — font
    // faces resolve against the document, not the tree. The rules are hoisted
    // once into the page so the bundled metric-compatible faces (Carlito for
    // Calibri, and the rest) are available to the canvas, which is what makes a
    // cell's font render the same on a machine that has none of them.
    hoistFontFaces(css);

    const style = document.createElement("style");
    style.textContent = css;
    root.append(style);

    const shell = document.createElement("div");
    shell.className = "editor-body";
    shell.innerHTML = markup;
    root.append(shell);
    this.#shell = shell;
    this.#applyChrome();

    const editor = await import(new URL(`editor.js?v=${BUILD}`, HERE));
    editor.setMountRoot(root);
    await editor.start();
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
      // Absent means shown, except where CHROME_DEFAULT says otherwise.
      const shown = this.#chrome[name] ?? true;
      this.#shell.classList.toggle(`oc-hide-${name}`, !shown);
    }
  }

  /// Set theme tokens: `{ accent: "#c026d3", bg: "#fff" }`.
  ///
  /// Written onto this element, where they cross the shadow boundary as custom
  /// properties do. Anything not named keeps the editor's own default, so a
  /// host overrides two colours rather than restating thirty.
  theme(tokens) {
    for (const [name, value] of Object.entries(tokens)) {
      if (!TOKENS.includes(name)) {
        throw new Error(
          `unknown OpenCalc theme token "${name}" — one of: ${TOKENS.join(", ")}`,
        );
      }
      if (value === null) this.style.removeProperty(`--oc-${name}`);
      else this.style.setProperty(`--oc-${name}`, value);
    }
    // The canvas caches the resolved tokens: it paints thousands of cells a
    // frame and cannot re-read the computed style for each. Without this the
    // chrome would restyle instantly and the grid would keep the old colours
    // until something else forced a repaint.
    this.#editor?.refreshTheme?.();
  }

  /// Light, dark, or follow the host's `prefers-color-scheme`.
  ///
  /// Set on this element rather than on `<html>`, so two embedded editors on
  /// one page can differ and neither restyles the page around it.
  setColorScheme(scheme) {
    if (scheme === "auto") this.removeAttribute("data-theme");
    else this.setAttribute("data-theme", scheme);
    this.#editor?.refreshTheme?.();
  }

  /// Engine configuration — the host-facing knobs, by the same names the Rust
  /// `SessionConfig` uses.
  ///
  ///   { calculation: "auto" | "manual" }
  async configure(options = {}) {
    const editor = await this.ready;
    if (options.calculation) editor.wasmApi().session_set_calculation_mode(options.calculation);
    return this;
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
let fontsHoisted = false;
function hoistFontFaces(css) {
  if (fontsHoisted) return;
  fontsHoisted = true;
  const faces = css.match(/@font-face\s*\{[^}]*\}/g);
  if (!faces) return;
  const style = document.createElement("style");
  style.dataset.opencalcFonts = "";
  // The URLs are relative to the stylesheet, which is not where this style
  // element lives — so they are resolved against it explicitly.
  style.textContent = faces
    .join("\n")
    .replace(/url\("\.\/([^"]+)"\)/g, (_, path) => `url("${new URL(path, HERE)}")`);
  document.head.append(style);
}

if (!customElements.get("opencalc-sheet")) {
  customElements.define("opencalc-sheet", OpenCalcSheet);
}

export { OpenCalcSheet, TOKENS as THEME_TOKENS };
