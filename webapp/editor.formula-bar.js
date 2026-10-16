// The formula bar's height: collapsed, expanded, and dragged.
//
// Excel for Microsoft 365 on Windows has three ways to change it, and they are
// one piece of state:
//
// - the chevron at the far right of the bar,
// - `Ctrl+Shift+U` ("Expand or collapse the formula bar", *Keyboard shortcuts
//   in Excel*), and
// - dragging the bar's bottom edge, where the pointer becomes a two-headed
//   arrow. The height dragged to is kept for the session, so collapsing and
//   expanding again returns to it rather than to the default.
//
// The bar is chrome inside the editor's column layout, not a popover, so a
// taller bar *pushes the grid down*: the canvas is whatever the layout leaves,
// and `onResize` is how the caller re-measures it. Nothing here positions the
// grid.
//
// Heights are whole lines. A dragged height that ended mid-line would show
// half a line of formula, which reads as clipped text rather than as a choice.
//
// No editor state is imported on purpose: this module is DOM in, DOM out, so it
// can be read and tested without the rest of the editor in one's head.

/// How many lines the bar opens to before anyone drags it. Excel's default
/// expanded bar holds about three lines of text.
export const DEFAULT_LINES = 3;
/// The floor while dragging: below two lines the box is the collapsed bar
/// again, so the drag collapses it instead of leaving a one-line "expanded" bar.
const MIN_LINES = 2;
/// The grid always keeps at least this much of the editor's height, so a drag
/// cannot push the column headers off the bottom of the window.
const GRID_FLOOR_PX = 120;

function metrics(input) {
  const cs = getComputedStyle(input);
  const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.4 || 18;
  const pad = (parseFloat(cs.paddingTop) || 0) + (parseFloat(cs.paddingBottom) || 0)
    + (parseFloat(cs.borderTopWidth) || 0) + (parseFloat(cs.borderBottomWidth) || 0);
  return { lh, pad };
}

/// Where the caret is drawn in a `<textarea>`, in viewport coordinates.
///
/// A textarea will not say, so the text up to the caret is laid out in an
/// invisible clone with the same box and the same wrapping, and a marker at its
/// end is measured. `x` is the caret's left edge; `top`/`bottom` bound the line
/// it sits on. Returns `null` for anything that is not a textarea.
export function caretPoint(el, at = el?.selectionStart ?? 0) {
  if (!el || el.tagName !== "TEXTAREA") return null;
  const cs = getComputedStyle(el);
  const m = document.createElement("div");
  for (const p of [
    "fontFamily", "fontSize", "fontWeight", "fontStyle", "letterSpacing", "lineHeight", "textIndent",
    "paddingTop", "paddingLeft", "paddingRight", "borderTopWidth", "borderLeftWidth", "borderRightWidth",
    "boxSizing", "wordBreak", "overflowWrap", "tabSize", "textTransform", "wordSpacing",
  ]) m.style[p] = cs[p];
  m.style.position = "fixed";
  m.style.left = "0";
  m.style.top = "0";
  m.style.visibility = "hidden";
  m.style.pointerEvents = "none";
  // The clone wraps at the width the textarea's text actually has — its client
  // width, which excludes a vertical scrollbar — not at its outer width.
  m.style.boxSizing = "border-box";
  m.style.borderRightWidth = "0";
  m.style.width = el.clientWidth + (parseFloat(cs.borderLeftWidth) || 0) + "px";
  m.style.whiteSpace = cs.whiteSpace === "pre" ? "pre" : "pre-wrap";
  m.textContent = el.value.slice(0, at);
  const mark = document.createElement("span");
  mark.textContent = "​";
  m.appendChild(mark);
  document.body.appendChild(m);
  const r = el.getBoundingClientRect();
  const lh = parseFloat(cs.lineHeight) || mark.offsetHeight;
  const top = r.top + mark.offsetTop - el.scrollTop;
  const out = { x: r.left + mark.offsetLeft - el.scrollLeft, top, bottom: top + lh };
  m.remove();
  return out;
}

/// Scroll a textarea so the caret's whole line is inside it.
///
/// The browser does this for typing but not for every programmatic change —
/// `Alt+Enter` rewrites the value and moves the selection itself, and a box that
/// has just been made shorter keeps whatever scroll it had.
export function keepCaretInView(el) {
  const c = caretPoint(el);
  if (!c) return;
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const top = r.top + (parseFloat(cs.borderTopWidth) || 0);
  const bottom = top + el.clientHeight;
  if (c.top < top) el.scrollTop -= top - c.top;
  else if (c.bottom > bottom) el.scrollTop += c.bottom - bottom;
}

/// Wires the bar's chevron state, height and bottom-edge splitter.
///
/// `onResize` runs whenever the bar's height changes, so the caller can
/// re-measure whatever sits below it. Returns `{ toggle, isExpanded }`.
export function initFormulaBarSizer({ bar, input, button, workArea, onResize }) {
  // The session's choice, in whole lines. Not persisted across reloads: Excel
  // keeps it "for the remainder of the session", and a reload is a new one.
  let lines = DEFAULT_LINES;

  const grip = document.createElement("div");
  grip.className = "fx-resize";
  grip.setAttribute("aria-hidden", "true");
  grip.title = "Drag to resize the formula bar";
  bar.appendChild(grip);

  const isExpanded = () => bar.classList.contains("expanded");

  function maxLines() {
    const { lh, pad } = metrics(input);
    // What the bar may take is what the editor has, less the grid's floor and
    // less everything else in the column that is not the bar.
    const host = bar.parentElement;
    if (!host) return 40;
    const hostH = host.getBoundingClientRect().height;
    const others = hostH - bar.getBoundingClientRect().height;
    const gridH = workArea ? workArea.getBoundingClientRect().height : 0;
    const fixed = others - gridH; // ribbon, menu, status bar
    const room = hostH - fixed - GRID_FLOOR_PX - (bar.offsetHeight - input.offsetHeight);
    return Math.max(MIN_LINES, Math.floor((room - pad) / lh));
  }

  function apply(n) {
    const { lh, pad } = metrics(input);
    bar.style.setProperty("--oc-fx-box-h", `${Math.round(n * lh + pad)}px`);
  }

  function set(on) {
    bar.classList.toggle("expanded", on);
    if (button) {
      button.setAttribute("aria-expanded", on ? "true" : "false");
      const label = on ? "Collapse the formula bar" : "Expand the formula bar";
      button.setAttribute("aria-label", label);
      button.title = `${label} (Ctrl+Shift+U)`;
    }
    if (on) apply(Math.min(lines, maxLines()));
    else {
      bar.style.removeProperty("--oc-fx-box-h");
      // Collapsed, the bar shows the value's first line, as Excel's does —
      // unless the caret is in it, in which case it shows the caret's line.
      if (document.activeElement !== input) input.scrollTop = 0;
    }
    onResize?.();
    if (document.activeElement === input) keepCaretInView(input);
    return on;
  }

  // --- The splitter -------------------------------------------------------
  let drag = null;
  grip.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const { lh } = metrics(input);
    drag = {
      id: e.pointerId, y0: e.clientY, lh,
      // A collapsed bar starts from one line, an expanded one from what it shows.
      from: isExpanded() ? Math.round((input.clientHeight - metrics(input).pad) / lh) : 1,
      max: maxLines(),
    };
    try { grip.setPointerCapture(e.pointerId); } catch { /* synthetic pointer */ }
    bar.classList.add("fx-resizing");
  });
  grip.addEventListener("pointermove", (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    const want = drag.from + Math.round((e.clientY - drag.y0) / drag.lh);
    if (want < MIN_LINES) {
      if (isExpanded()) set(false);
      return;
    }
    const n = Math.min(want, drag.max);
    lines = n;
    if (!isExpanded()) set(true);
    else { apply(n); onResize?.(); }
  });
  const end = (e) => {
    if (!drag || e.pointerId !== drag.id) return;
    drag = null;
    bar.classList.remove("fx-resizing");
    try { grip.releasePointerCapture(e.pointerId); } catch { /* already released */ }
  };
  grip.addEventListener("pointerup", end);
  grip.addEventListener("pointercancel", end);
  // A double-click on the splitter toggles, the same command as the chevron.
  grip.addEventListener("dblclick", (e) => { e.preventDefault(); set(!isExpanded()); });

  // A window that gets shorter must not leave the bar taller than the room.
  window.addEventListener("resize", () => {
    if (!isExpanded()) return;
    apply(Math.min(lines, maxLines()));
    onResize?.();
  });

  return { toggle: () => set(!isExpanded()), isExpanded, set };
}
