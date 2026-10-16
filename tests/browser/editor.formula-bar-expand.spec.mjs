// The expanded formula bar, measured against Excel for Microsoft 365 on
// Windows.
//
// What Excel does, and where it is written down:
//
// - `Ctrl+Shift+U` "Expand or collapse the formula bar" — Microsoft Support,
//   *Keyboard shortcuts in Excel*, "Work in cells or the Formula bar".
// - `Alt+Enter` "Start a new line in the same cell" — the same page.
// - The chevron at the far right of the bar, and dragging the bar's bottom edge
//   with the two-headed arrow to make it taller — Microsoft Q&A
//   (learn.microsoft.com/en-us/answers/a/11458124) and Ablebits' *Formula bar
//   in Excel*; Excel keeps the dragged height for the session.
//
// The bar is chrome, not a popover: it grows *into the layout*, so the grid
// moves down by exactly what the bar gained and the column headers are never
// covered. Every geometry check below is phrased that way, because the failure
// it guards against — a bar that overlays the sheet, or a grid that loses more
// than the bar took — reads perfectly well in the CSS.

import { expect, test } from "@playwright/test";

const LONG = "=IF(SUM(A1:A20)>1000,VLOOKUP(B2,Sheet2!A:D,4,FALSE),IFERROR(INDEX(C:C,MATCH(D2,E:E,0)),\"not found\"))"
  + "&IF(AND(F1>0,G1<100),TEXTJOIN(\", \",TRUE,H1:H9),SUBSTITUTE(TRIM(I1),\" \",\"_\"))"
  + "&IFS(J1=1,\"one\",J1=2,\"two\",J1=3,\"three\",TRUE,\"many\")"
  + "&ROUND(AVERAGEIFS(K:K,L:L,\">=\"&M1,N:N,\"<\"&O1),2)";

async function boot(page, width = 1280, height = 900) {
  await page.setViewportSize({ width, height });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await page.waitForTimeout(200);
}

const rect = (page, sel) => page.evaluate((s) => {
  const el = document.querySelector(s);
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { x: r.x, y: r.y, w: r.width, h: r.height, top: r.top, bottom: r.bottom, left: r.left, right: r.right };
}, sel);

const fx = (page) => page.evaluate(() => {
  const el = document.querySelector("#formula-input");
  const cs = getComputedStyle(el);
  return {
    lh: parseFloat(cs.lineHeight),
    clientH: el.clientHeight, scrollH: el.scrollHeight,
    clientW: el.clientWidth, scrollW: el.scrollWidth, offsetW: el.offsetWidth,
    scrollTop: el.scrollTop, overflowY: cs.overflowY, whiteSpace: cs.whiteSpace,
  };
});

/// Where the caret is drawn, in viewport coordinates — measured with a clone of
/// the textarea's text box, since a `<textarea>` will not say.
const caret = (page) => page.evaluate(() => {
  const el = document.querySelector("#formula-input");
  const cs = getComputedStyle(el);
  const m = document.createElement("div");
  for (const p of ["fontFamily", "fontSize", "fontWeight", "letterSpacing", "lineHeight", "paddingTop",
    "paddingLeft", "paddingRight", "borderTopWidth", "borderLeftWidth", "boxSizing", "whiteSpace", "wordBreak", "overflowWrap", "tabSize"]) m.style[p] = cs[p];
  m.style.position = "fixed"; m.style.visibility = "hidden"; m.style.left = "0"; m.style.top = "0";
  m.style.width = el.clientWidth + "px";
  m.style.whiteSpace = "pre-wrap";
  m.textContent = el.value.slice(0, el.selectionStart);
  const mark = document.createElement("span");
  mark.textContent = "​";
  m.appendChild(mark);
  document.body.appendChild(m);
  const r = el.getBoundingClientRect();
  const top = r.top + mark.offsetTop - el.scrollTop;
  const out = { x: r.left + mark.offsetLeft - el.scrollLeft, top, bottom: top + parseFloat(cs.lineHeight), boxTop: r.top, boxBottom: r.top + el.clientHeight };
  m.remove();
  return out;
});

async function setCell(page, row, col, text) {
  await page.evaluate(([r, c, t]) => {
    window.opencalcEditor.wasmApi().session_set_cell(0, r, c, t);
    window.opencalcEditor.selectForTest(r, c);
  }, [row, col, text]);
  await page.waitForTimeout(150);
}

test.describe("expanded formula bar", () => {
  /// **It pushes the grid; it does not cover it.** The grid's top moves down by
  /// exactly the bar's growth, the headers stay below the bar, and the Name Box
  /// and fx stay where they were, aligned to the top.
  test("expanding grows the bar into the layout and moves the grid down by the same amount", async ({ page }) => {
    await boot(page);
    const bar0 = await rect(page, ".formula-bar");
    const grid0 = await rect(page, "#grid");
    const name0 = await rect(page, "#cell-ref");
    const fx0 = await rect(page, "#fx-insert");
    const chev0 = await rect(page, "#fx-expand");

    await page.click("#fx-expand");
    await page.waitForTimeout(200);
    const bar1 = await rect(page, ".formula-bar");
    const grid1 = await rect(page, "#grid");
    const f = await fx(page);

    const grew = bar1.h - bar0.h;
    expect(f.clientH, `expanded, the box shows ${f.clientH}px — under three ${f.lh}px lines`).toBeGreaterThanOrEqual(3 * f.lh);
    expect(grew, "the bar did not grow").toBeGreaterThan(f.lh);
    expect(Math.abs((grid1.top - grid0.top) - grew),
      `the grid moved ${grid1.top - grid0.top}px for a bar that grew ${grew}px`).toBeLessThanOrEqual(0.5);
    expect(Math.abs((grid0.h - grid1.h) - grew),
      `the grid lost ${grid0.h - grid1.h}px for a bar that grew ${grew}px`).toBeLessThanOrEqual(1);
    expect(grid1.top, "the grid (and its column headers) start under the bar's bottom edge").toBeGreaterThanOrEqual(bar1.bottom - 0.5);
    // The canvas's own pixel buffer follows: a stale one paints the old height.
    const buf = await page.evaluate(() => {
      const c = document.querySelector("#grid");
      return { css: c.getBoundingClientRect().height, px: c.height / (window.devicePixelRatio || 1) };
    });
    expect(Math.abs(buf.css - buf.px), "the canvas was not resized with the bar").toBeLessThanOrEqual(1);

    const name1 = await rect(page, "#cell-ref");
    const fx1 = await rect(page, "#fx-insert");
    const chev1 = await rect(page, "#fx-expand");
    expect(Math.abs(name1.top - name0.top), "the Name Box moved").toBeLessThanOrEqual(0.5);
    expect(Math.abs(fx1.top - fx0.top), "the fx button moved").toBeLessThanOrEqual(0.5);
    expect(Math.abs(chev1.top - chev0.top), "the chevron left the top line").toBeLessThanOrEqual(0.5);
    await expect(page.locator("#fx-expand")).toHaveAttribute("aria-expanded", "true");
  });

  /// **The bottom edge is a splitter.** Excel shows a two-headed arrow there
  /// and lets the bar be dragged taller; the height snaps to whole lines.
  test("the bottom edge drags the bar taller, with a resize cursor, and the grid follows", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.waitForTimeout(150);
    const bar0 = await rect(page, ".formula-bar");
    const grid0 = await rect(page, "#grid");
    const { lh } = await fx(page);

    const x = bar0.left + bar0.w / 2, y = bar0.bottom - 1;
    const hit = await page.evaluate(([px, py]) => {
      const el = document.elementFromPoint(px, py);
      return { cls: el?.className ?? "", cursor: el ? getComputedStyle(el).cursor : "" };
    }, [x, y]);
    expect(hit.cursor, `the bar's bottom edge has cursor "${hit.cursor}" (${hit.cls})`).toMatch(/^(ns|row)-resize$/);

    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x, y + 40, { steps: 4 });
    await page.mouse.move(x, y + 5 * lh, { steps: 4 });
    await page.mouse.up();
    await page.waitForTimeout(150);
    const bar1 = await rect(page, ".formula-bar");
    const grid1 = await rect(page, "#grid");
    const grew = bar1.h - bar0.h;
    expect(Math.abs(grew - 5 * lh), `dragged ${5 * lh}px, the bar grew ${grew}px`).toBeLessThanOrEqual(lh / 2 + 0.5);
    expect(Math.abs((grid1.top - grid0.top) - grew), "the grid did not follow the drag").toBeLessThanOrEqual(0.5);
    const f = await fx(page);
    expect(f.clientH % lh <= 10.5, "the dragged height is not whole lines plus padding").toBe(true);

    // Dragging up past one line collapses, as it does in Excel.
    const b = await rect(page, ".formula-bar");
    await page.mouse.move(x, b.bottom - 1);
    await page.mouse.down();
    await page.mouse.move(x, b.top + 4, { steps: 6 });
    await page.mouse.up();
    await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
  });

  /// The reference tint is a mirror laid over the box. Found by looking: after
  /// a drag the mirror kept the old height, and a coloured formula was cut off
  /// at the line where the box used to end.
  test("dragging the bar while a formula is tinted keeps the tint the box's size", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.keyboard.type("=LET(");
    for (let i = 1; i <= 7; i += 1) {
      await page.keyboard.press("Alt+Enter");
      await page.keyboard.type(`    x${i}, A${i}*${i},`);
    }
    const b = await rect(page, ".formula-bar");
    const { lh } = await fx(page);
    const x = b.left + b.w / 2;
    await page.mouse.move(x, b.bottom - 1);
    await page.mouse.down();
    await page.mouse.move(x, b.bottom - 1 + 4 * lh, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);
    const m = await page.evaluate(() => {
      const el = document.querySelector("#formula-input");
      const mirror = el.parentNode.querySelector(".ref-mirror");
      return { tinted: el.classList.contains("tinted"), box: el.offsetHeight, mirror: mirror?.offsetHeight,
        scroll: [el.scrollTop, mirror?.scrollTop] };
    });
    expect(m.tinted, "nothing is tinted; the test is vacuous").toBe(true);
    expect(m.mirror, `the tint is ${m.mirror}px tall over a ${m.box}px box`).toBe(m.box);
    expect(m.scroll[1], "the tint is scrolled differently from the text").toBe(m.scroll[0]);
  });

  test("dragging the edge of a collapsed bar expands it", async ({ page }) => {
    await boot(page);
    const bar0 = await rect(page, ".formula-bar");
    const { lh } = await fx(page);
    const x = bar0.left + bar0.w / 2, y = bar0.bottom - 1;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x, y + 3 * lh, { steps: 5 });
    await page.mouse.up();
    await expect(page.locator(".formula-bar")).toHaveClass(/expanded/);
    await expect(page.locator("#fx-expand")).toHaveAttribute("aria-expanded", "true");
  });

  /// **The size is the session's.** Collapse and expand again — by the chevron
  /// or by Ctrl+Shift+U — and the bar returns to the height it was dragged to.
  test("collapsing and expanding again returns to the dragged height", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.waitForTimeout(150);
    const { lh } = await fx(page);
    const b = await rect(page, ".formula-bar");
    const x = b.left + b.w / 2;
    await page.mouse.move(x, b.bottom - 1);
    await page.mouse.down();
    await page.mouse.move(x, b.bottom - 1 + 4 * lh, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);
    const dragged = (await rect(page, ".formula-bar")).h;
    expect(dragged, "the drag did nothing").toBeGreaterThan(b.h + 2 * lh);

    await page.click("#fx-expand");
    await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
    await page.click("#fx-expand");
    await page.waitForTimeout(100);
    expect((await rect(page, ".formula-bar")).h, "the chevron did not return to the dragged height").toBeCloseTo(dragged, 0);

    await page.locator("#grid").focus();
    await page.keyboard.press("Control+Shift+u");
    await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
    await page.keyboard.press("Control+Shift+u");
    await page.waitForTimeout(100);
    expect((await rect(page, ".formula-bar")).h, "Ctrl+Shift+U did not return to the dragged height").toBeCloseTo(dragged, 0);
  });

  /// Excel takes the chord in the middle of an edit, in either surface.
  test("Ctrl+Shift+U toggles while editing in the bar, and the edit survives", async ({ page }) => {
    await boot(page);
    await page.click("#formula-input");
    await page.keyboard.type("=SUM(1,2");
    await page.keyboard.press("Control+Shift+u");
    await expect(page.locator(".formula-bar")).toHaveClass(/expanded/);
    expect(await page.evaluate(() => document.activeElement?.id), "the chord took focus out of the bar").toBe("formula-input");
    await page.keyboard.type(")");
    expect(await page.inputValue("#formula-input")).toBe("=SUM(1,2)");
    await page.keyboard.press("Control+Shift+u");
    await expect(page.locator(".formula-bar")).not.toHaveClass(/expanded/);
    await page.keyboard.press("Enter");
    await expect.poll(() => page.evaluate(() =>
      JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, 0, 0, 0, 0))[0]?.t ?? "")).toBe("3");
  });

  /// **Wrapping, and a scrollbar when it still does not fit.**
  test("a long formula wraps at the box width and a scrollbar appears when it overflows", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.fill("#formula-input", LONG + LONG + LONG + LONG);
    const f = await fx(page);
    expect(f.scrollW, "the expanded box scrolls sideways — it does not wrap").toBeLessThanOrEqual(f.clientW);
    expect(f.scrollH, "the text did not overflow the box; the test is vacuous").toBeGreaterThan(f.clientH);
    // A scrollbar is drawn by an `auto` box that overflows. Its *width* cannot
    // be asserted here: Playwright's headless Chromium runs with
    // `--hide-scrollbars`, so `offsetWidth - clientWidth` is 0 for every box.
    expect(f.overflowY, "the expanded box cannot scroll vertically").toMatch(/auto|scroll/);
    // And it really scrolls: the end of the text is reachable.
    const end = await page.evaluate(() => {
      const el = document.querySelector("#formula-input");
      el.scrollTop = el.scrollHeight;
      return el.scrollTop;
    });
    expect(end, "the box overflows but does not scroll").toBeGreaterThan(0);
  });

  /// **Alt+Enter, indentation, and a caret that stays in view.**
  test("Alt+Enter breaks lines, indentation is kept, and the caret stays in view", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.keyboard.type("=LET(");
    for (let i = 1; i <= 9; i += 1) {
      await page.keyboard.press("Alt+Enter");
      await page.keyboard.type(`    x${i}, ${i},`);
    }
    await page.keyboard.press("Alt+Enter");
    await page.keyboard.type("    x1)");
    const v = await page.inputValue("#formula-input");
    expect(v.split("\n").length, "Alt+Enter did not break the line").toBe(11);
    expect(v, "the indentation was not kept").toContain("\n    x5, 5,");
    const f = await fx(page);
    expect(f.whiteSpace, "the box would collapse indentation").toMatch(/pre-wrap|break-spaces/);
    expect(f.scrollH, "eleven lines fit; the caret test is vacuous").toBeGreaterThan(f.clientH);
    const c = await caret(page);
    expect(c.top, "the caret's line is above the visible box").toBeGreaterThanOrEqual(c.boxTop - 0.5);
    expect(c.bottom, "the caret's line is below the visible box").toBeLessThanOrEqual(c.boxBottom + 0.5);

  });

  /// A text value keeps its line breaks and indentation through a commit.
  /// (A *formula's* do not yet: the engine re-serialises formula text on
  /// commit, dropping whitespace — a separate row, not this one.)
  test("a multi-line text entry commits with its line breaks and indentation", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.keyboard.type("Total");
    await page.keyboard.press("Alt+Enter");
    await page.keyboard.type("    indented");
    await page.keyboard.press("Enter");
    await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
    await page.waitForTimeout(150);
    expect(await page.inputValue("#formula-input")).toBe("Total\n    indented");
  });

  /// **Collapsed, a multi-line value shows its first line** — not the middle
  /// of it, and not a 28px scrollbar.
  test("collapsed, a multi-line cell shows its first line", async ({ page }) => {
    await boot(page);
    await setCell(page, 2, 1, "first line\nsecond line\nthird line");
    expect(await page.inputValue("#formula-input")).toContain("first line\nsecond");
    const f = await fx(page);
    expect(f.scrollTop, "the collapsed bar is not showing the first line").toBe(0);
    expect(f.clientH, "the collapsed bar shows more than one line").toBeLessThan(2 * f.lh);
    expect(f.overflowY, "the one-line bar would draw a scrollbar for a multi-line value").toBe("hidden");
    // And expanding shows all three.
    await page.click("#fx-expand");
    await page.waitForTimeout(100);
    const g = await fx(page);
    expect(g.scrollH, "expanded, the three lines do not fit").toBeLessThanOrEqual(g.clientH);
  });

  /// **The in-cell editor and the bar are one edit.**
  test("an in-cell edit shows in the expanded bar, line breaks and all", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.locator("#grid").focus();
    await page.evaluate(() => window.opencalcEditor.selectForTest(1, 1));
    await page.keyboard.type("alpha");
    await page.keyboard.press("Alt+Enter");
    await page.keyboard.type("beta");
    await expect(page.locator("#formula-input")).toHaveValue("alpha\nbeta");
  });

  /// **Autocomplete opens at the caret,** not at the bar's bottom-left corner,
  /// which in a three-line box is a line or two and half a screen away.
  test("the autocomplete list and argument hint open at the caret in the expanded bar", async ({ page }) => {
    await boot(page);
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.keyboard.type("=1+2+3+4+5+6+7+8+9+10+");
    await page.keyboard.press("Alt+Enter");
    await page.keyboard.type("    1+SU");
    await expect(page.locator("#ac-menu")).toBeVisible();
    const c = await caret(page);
    const ac = await rect(page, "#ac-menu");
    expect(Math.abs(ac.top - c.bottom), `the list opens at y=${ac.top}, the caret line ends at ${c.bottom}`).toBeLessThanOrEqual(6);
    // At the start of the word being completed, which is two characters back.
    expect(c.x - ac.left, `the list opens at x=${ac.left}, the caret is at ${c.x}`).toBeGreaterThanOrEqual(0);
    expect(c.x - ac.left, `the list opens at x=${ac.left}, the caret is at ${c.x}`).toBeLessThanOrEqual(30);

    await page.keyboard.type("M(");
    await expect(page.locator("#sig-tip")).toBeVisible();
    const c2 = await caret(page);
    const sig = await rect(page, "#sig-tip");
    expect(Math.abs(sig.top - c2.bottom), `the hint opens at y=${sig.top}, the caret line ends at ${c2.bottom}`).toBeLessThanOrEqual(6);
    expect(sig.left, "the hint opens far right of the caret").toBeLessThanOrEqual(c2.x);
    expect(c2.x - sig.left, "the hint opens far left of the caret").toBeLessThanOrEqual(80);
  });

  test("on a phone, expanded, the page does not scroll sideways and the grid still moves down", async ({ page }) => {
    await boot(page, 390, 800);
    const grid0 = await rect(page, "#grid");
    const bar0 = await rect(page, ".formula-bar");
    await page.click("#fx-expand");
    await page.click("#formula-input");
    await page.fill("#formula-input", LONG);
    await page.waitForTimeout(150);
    const bar1 = await rect(page, ".formula-bar");
    const grid1 = await rect(page, "#grid");
    expect(Math.abs((grid1.top - grid0.top) - (bar1.h - bar0.h)), "the grid did not move by the bar's growth").toBeLessThanOrEqual(0.5);
    const sx = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(sx, "the page scrolls sideways").toBeLessThanOrEqual(0);
    const f = await fx(page);
    expect(f.scrollW).toBeLessThanOrEqual(f.clientW);
  });

  test("dark mode: the expanded bar and its edge draw from the theme tokens", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await boot(page);
    await page.click("#fx-expand");
    const c = await page.evaluate(() => {
      const bar = document.querySelector(".formula-bar");
      const probe = document.createElement("div");
      probe.style.background = "var(--oc-background-color)";
      probe.style.borderColor = "var(--oc-border-color)";
      bar.appendChild(probe);
      const p = getComputedStyle(probe);
      const b = getComputedStyle(bar);
      const out = { bg: b.backgroundColor, tokenBg: p.backgroundColor, edge: b.borderBottomColor, tokenEdge: p.borderTopColor,
        text: getComputedStyle(document.querySelector("#formula-input")).color };
      probe.remove();
      return out;
    });
    expect(c.bg).toBe(c.tokenBg);
    expect(c.edge).toBe(c.tokenEdge);
    expect(c.text, "dark text on a dark bar").not.toBe("rgb(0, 0, 0)");
  });
});
