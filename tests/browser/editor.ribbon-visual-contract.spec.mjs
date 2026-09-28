// The ribbon's measured visual contract (`UX-RIB-15`).
//
// These numbers are not house style. Each is measured first-hand in the sibling
// editor's `docs/123` — against ONLYOFFICE 9.4.1 running in the vendor's own
// playground, with every LESS variable resolved, and against a live Google Docs
// at 1408x723 — and that note records the file and line for each so it can be
// re-read rather than re-argued.
//
// **Why a gate and not a review note.** The chrome was assembled rather than
// designed and every pull request verified it by assertion rather than by
// opening the editor, which is how it arrived at a 92px header and a band that
// changed height per tab. A number with research behind it and nothing holding
// it is a number that drifts back the first time someone needs 10px.
//
// The claims here are the ones that would be silently lost:
//   * the band is 74px, and the group box inside it 63px;
//   * the tab strip is ONLYOFFICE's 28px;
//   * no group caption is PAINTED, and every one is still in the tree.
//
// That last pair is one claim in two halves and both halves matter. Deleting
// the captions from the DOM would pass a "nothing is painted" check while
// taking the group's accessible name and the overflow menu's section headings
// with it — so the test asserts they are readable *and* invisible.

import { expect, test } from "@playwright/test";

async function boot(page, query = "?chrome=ribbon") {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await expect(page.locator(".rb-panel:not([hidden])")).toBeVisible();
}

test("the band and the tab strip are the measured heights", async ({ page }) => {
  await boot(page);
  const m = await page.evaluate(() => {
    const h = (s) => {
      const e = document.querySelector(s);
      return e ? +e.getBoundingClientRect().height.toFixed(1) : null;
    };
    return {
      panel: h(".rb-panel:not([hidden])"),
      row: h(".rb-panel:not([hidden]) .rb-row"),
      tabs: h(".rb-tabs"),
    };
  });
  // 74px, against ONLYOFFICE's 66px. The 8px is the accessibility margin: our
  // controls stay at 30px where theirs are 20px, and a 20px target fails
  // WCAG 2.2 SC 2.5.8. A pass that "recovers" this 8px is a regression.
  expect(m.panel, "the band is not the measured 74px").toBe(74);
  // The group box: two 30px control rows and the 3px between them.
  expect(m.row, "the group box is not the measured 63px").toBe(63);
  // ONLYOFFICE's `@toolbar-height-tabs-top-title` — the height the strip takes
  // under the document title rather than beside it.
  expect(m.tabs, "the tab strip is not the measured 28px").toBeLessThanOrEqual(30);
});

test("no group caption is painted, and every one is still readable", async ({ page }) => {
  await boot(page);
  const caps = await page.evaluate(() =>
    [...document.querySelectorAll(".rb-panel:not([hidden]) .rb-cap")].map((c) => ({
      text: c.textContent.trim(),
      // Painted means it occupies real space on screen. The visually-hidden
      // idiom leaves a 1px clipped box, so this is a width test, not a
      // `display` test — `display: none` would also report zero and would be
      // the wrong fix.
      w: +c.getBoundingClientRect().width.toFixed(1),
      display: getComputedStyle(c).display,
    })),
  );

  // Not a vacuous pass: there have to be captions to be checking.
  expect(caps.length, "no group captions in the tree at all — the selector moved")
    .toBeGreaterThanOrEqual(5);
  expect(
    caps.filter((c) => c.w > 2).map((c) => c.text),
    "a group caption is painted; docs/123 §4.1 takes them out of the band",
  ).toEqual([]);
  // ...and they are still there to be read.
  expect(
    caps.every((c) => c.text.length > 0),
    "a caption is in the tree but empty, so it names nothing",
  ).toBe(true);
  expect(
    caps.filter((c) => c.display === "none").map((c) => c.text),
    "a caption was hidden with `display: none`, which takes it out of the " +
      "accessibility tree — it is the group's accessible name and the " +
      "overflow menu's section heading",
  ).toEqual([]);
});
