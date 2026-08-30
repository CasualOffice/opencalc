// The formula bar as a bar, not as a form (`UX-CHR-08`, `docs/88` §4).
//
// Three of these are composition. The fourth is a defect found by running the
// editor: expanding the bar produced an 86px-tall box containing **one
// vertically-centred line**, surrendering 58px of grid for nothing, because
// `#formula-input` was an `<input type="text">` and an input never wraps
// whatever `white-space` it computes. The in-cell editor in this same product
// is already a `<textarea>`, for exactly this reason, with the comment saying
// so.

import { expect, test } from "@playwright/test";

// Long enough that it *cannot* fit one line at 1280px — about 460 characters
// against a bar roughly 1100px wide. An earlier version of this file used a
// 105-character formula, which fitted comfortably and made every wrapping
// assertion below vacuously true: nothing wrapped because nothing needed to.
const LONG = "=IF(SUM(A1:A20)>1000,VLOOKUP(B2,Sheet2!A:D,4,FALSE),IFERROR(INDEX(C:C,MATCH(D2,E:E,0)),\"not found\"))"
  + "&IF(AND(F1>0,G1<100),TEXTJOIN(\", \",TRUE,H1:H9),SUBSTITUTE(TRIM(I1),\" \",\"_\"))"
  + "&IFS(J1=1,\"one\",J1=2,\"two\",J1=3,\"three\",TRUE,\"many\")"
  + "&ROUND(AVERAGEIFS(K:K,L:L,\">=\"&M1,N:N,\"<\"&O1),2)"
  + "&CONCAT(P1:P8)&XLOOKUP(Q1,R:R,S:S,\"none\",0,1)";

async function boot(page, width = 1280) {
  await page.setViewportSize({ width, height: 900 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(300);
}

const box = (page, sel) => page.evaluate((s) => {
  const el = document.querySelector(s);
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { x: Math.round(r.x), right: Math.round(r.right), w: Math.round(r.width), h: Math.round(r.height), top: Math.round(r.top) };
}, sel);

/// **58px of grid for one line.**
test("expanding the formula bar shows more than one line", async ({ page }) => {
  await boot(page);
  await page.click("#formula-input");
  await page.fill("#formula-input", LONG);

  const gridBefore = await box(page, "#grid");
  await page.click("#fx-expand");
  await page.waitForTimeout(300);

  // **Measured by what the user has to do to see the end of their formula.**
  //
  // Two earlier versions of this check passed against the exact defect they
  // were written for. `scrollHeight / lineHeight` reports the *box*, so an
  // `<input>` in an 86px box divides into four lines that do not exist; and
  // `scrollWidth > clientWidth` does not discriminate on an `<input>` either.
  // Both measured the container while claiming to measure the text.
  //
  // So: put the caret at the end, and ask whether the control had to scroll
  // sideways to get there. A single-line control must. A wrapping one never
  // does, because the end of the text is already on screen — which is the whole
  // point of expanding the bar.
  const lines = await page.evaluate(() => {
    const el = document.querySelector("#formula-input");
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
    el.dispatchEvent(new Event("select"));
    const cs = getComputedStyle(el);
    const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.2;
    return { scrolled: el.scrollLeft, lh: Math.round(lh), textH: el.scrollHeight, boxH: Math.round(el.getBoundingClientRect().height), tag: el.tagName };
  });
  expect(lines.scrolled,
    `the ${lines.tag} scrolled ${lines.scrolled}px sideways to reach the end of the formula, so the ${lines.boxH}px box is showing one line`).toBe(0);

  // And the grid pays only for what is used: the height the bar took must be
  // the height the text needed, not a fixed box with a line floating in it.
  const gridAfter = await box(page, "#grid");
  const surrendered = gridBefore.h - gridAfter.h;
  expect(surrendered,
    `the grid gave up ${surrendered}px to show ${lines.textH}px of text`).toBeLessThanOrEqual(lines.textH + 24);
});

/// **Alt+Enter, which the old comment said this bar could not hold.**
///
/// It was gated on `surface === inline` because "the formula bar is a single
/// line" — true of an `<input>` and false the moment it stops being one. Excel
/// takes Alt+Enter in the formula bar.
test("Alt+Enter breaks a line in the formula bar, and Enter still commits", async ({ page }) => {
  await boot(page);
  await page.click("#formula-input");
  await page.type("#formula-input", "one");
  await page.keyboard.press("Alt+Enter");
  await page.type("#formula-input", "two");
  expect(await page.inputValue("#formula-input"),
    "Alt+Enter did not put a line break in the formula bar").toBe("one\ntwo");

  // The regression this change most easily causes: a <textarea> takes Enter as
  // a newline, where the bar must commit. If this breaks, typing in the formula
  // bar stops working entirely.
  await page.keyboard.press("Enter");
  await page.waitForTimeout(250);
  const cell = await page.evaluate(() =>
    JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, 0, 0, 0, 0))[0]?.t ?? "");
  expect(cell, "Enter stopped committing — the textarea swallowed it").toMatch(/one[\s\S]*two/);
});

/// **One flat bar with a single seam, not two bordered controls on a plate.**
test("the bar is one surface with a seam, and the input has no box of its own", async ({ page }) => {
  await boot(page);
  const input = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector("#formula-input"));
    return { bw: cs.borderTopWidth, radius: cs.borderTopLeftRadius, bg: cs.backgroundColor };
  });
  expect(parseFloat(input.bw), "the formula input still draws its own border").toBe(0);
  expect(parseFloat(input.radius), "the formula input still has rounded corners").toBe(0);

  const seam = await page.evaluate(() => {
    const cs = getComputedStyle(document.querySelector(".name-box"));
    return parseFloat(cs.borderRightWidth);
  });
  expect(seam, "there is no 1px seam after the Name Box").toBe(1);
});

/// **The expand chevron belongs at the far right.**
///
/// Unanimous across Excel, LibreOffice, OnlyOffice and Sheets. Ours sat at
/// x=160, 26px from the Name Box's own caret, where it reads as a second
/// dropdown on the Name Box rather than a control for the bar.
test("the expand control is at the right end of the bar", async ({ page }) => {
  await boot(page, 1280);
  const bar = await box(page, ".formula-bar");
  const chev = await box(page, "#fx-expand");
  const nameCaret = await box(page, "#name-box-list");

  expect(bar.right - chev.right,
    `the expand chevron is ${chev.x}px from the left, not at the bar's right edge (${bar.right})`).toBeLessThanOrEqual(16);
  expect(chev.x - nameCaret.right,
    "the expand chevron is still sitting next to the Name Box's caret").toBeGreaterThan(200);
});
