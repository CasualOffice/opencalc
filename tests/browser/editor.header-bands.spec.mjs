// The header bands, sized from what they must contain (`UX-CHR-09`,
// `docs/88` §6).
//
// `HEADER_W` was a hard 46px and `HEADER_H` a hard 24px. Both are wrong, in
// opposite directions:
//
//   - **Too narrow at the bottom.** At the header's own font, `1048576`
//     measures ~50px against a 46px band, so the last rows draw a truncated
//     number. `docs/88` derived this from the source and could not observe it —
//     three attempts to drive the viewport there stopped at the data edge. It
//     is observable through the Name Box, which jumps past the data, and a
//     screenshot at `A1048576` shows the leading digit cut off at the band's
//     edge.
//   - **Too wide everywhere else.** At rows 1-99 the label needs 6-16px and the
//     band spends 46, giving away ~30px of grid width on every frame of every
//     ordinary sheet.
//
// LibreOffice sizes the band from the bold width of `"8888"` plus padding and
// steps it at 5, 6 and 7 digits. Stepping matters: a band that tracked the
// exact widest label would reflow the whole grid while scrolling.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  await page.waitForTimeout(300);
}

/** The band's width, and the width of the widest label it is currently showing. */
const bandVsLabel = (page) => page.evaluate(() => {
  const ed = window.opencalcEditor;
  const c = document.querySelector("canvas");
  const ctx = c.getContext("2d");
  // The header's own font, as `drawRowHeaders` sets it.
  ctx.save();
  ctx.font = "12px system-ui, sans-serif";
  // The bottom-most row the viewport can reach is the widest label there is.
  const widest = ctx.measureText("1048576").width;
  ctx.restore();
  return { band: ed.HW, widest: Math.round(widest) };
});

test("the widest row label fits inside the row band", async ({ page }) => {
  await boot(page);
  await page.fill("#cell-ref", "A1048576");
  await page.press("#cell-ref", "Enter");
  await page.waitForTimeout(600);
  expect(await page.inputValue("#cell-ref"), "the viewport never reached the last row, so this proves nothing").toBe("A1048576");

  const { band, widest } = await bandVsLabel(page);
  // Padding on both sides, or the digits touch the gridline and the band edge.
  expect(band, `the band is ${band}px and the label needs ${widest}px, so the last rows draw a truncated number`)
    .toBeGreaterThanOrEqual(widest + 8);
});

/// **And it must not spend that width on a sheet that never needs it.**
///
/// The band is sized from the digits actually reachable, stepped, so an
/// ordinary sheet gets its ~30px of grid back.
test("an ordinary sheet does not pay for seven digits", async ({ page }) => {
  await boot(page);
  const { band } = await bandVsLabel(page);
  expect(band, `a sheet showing two-digit rows still spends ${band}px on its row band`).toBeLessThanOrEqual(40);
});

/// **The column band is the tallest of the five competitors at 24px.**
///
/// 20px is where three of the four sit, and is exactly Excel's default row
/// height — which is what makes the band read as "one row tall" rather than as
/// a bar.
test("the column band is 20px", async ({ page }) => {
  await boot(page);
  expect(await page.evaluate(() => window.opencalcEditor.HH)).toBe(20);
});

/// **The corner says "select all", which it did not.**
///
/// It contained two freeze-drag handles and nothing else — a white rounded pill
/// above a grey bar, which reads as debris. Excel draws a right-angled triangle
/// in the lower right of the corner box; Sheets and OnlyOffice draw one too.
test("the corner draws a select-all mark, and clicking it selects the sheet", async ({ page }) => {
  await boot(page);
  const mark = await page.evaluate(() => window.opencalcEditor.cornerMark());
  expect(mark, "the corner draws no select-all mark").not.toBe(null);

  // Clicked at the mark's own coordinates, reported by the renderer, rather
  // than at a point the test computed the same way the renderer did — that
  // would agree with a mark drawn in the wrong place.
  const box = await page.locator("canvas").boundingBox();
  await page.mouse.click(box.x + mark.x + mark.w / 2, box.y + mark.y + mark.h / 2);
  await page.waitForTimeout(250);

  // The Name Box keeps the anchor (`A1`) for a whole-sheet selection, as Excel
  // does, so the selection itself is what gets read — the first version of this
  // asserted on the Name Box and failed against a correct implementation.
  const sel = await page.evaluate(() => {
    const st = window.opencalcEditor.state;
    return { kind: st.selKind, r0: st.sel.row, ext: st.selExtent ?? null, anchor: st.sel };
  });
  expect(sel.kind, `clicking the corner left the selection as ${JSON.stringify(sel)}`).toBe("all");
});
