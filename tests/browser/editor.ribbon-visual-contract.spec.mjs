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
//   * the band is the measured height and the tab strip ONLYOFFICE's 28px;
//   * the band is a CARD — a different shade from the field behind it;
//   * every group caption is painted, and none is hidden with `display: none`.
//
// **The caption claim was reversed once, deliberately.** `docs/123` §4.1 argues
// for deleting the caption and the argument is sound — ONLYOFFICE has no
// caption element and the row costs ~13.5px. The sibling's *shipped product*
// paints them anyway. Matched to the running build rather than to its note: the
// thing being copied is the one that looks right on screen, and this
// repository's own rule is that checking means running. Applied to somebody
// else's repository, that means believing their build over their prose.
//
// `display: none` stays refused either way: it takes the group's accessible
// name and the overflow menu's section heading with it, and "invisible" and
// "absent" look identical in a screenshot.

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
  // The band carries the card's padding on top of the 74px token, which is why
  // this is a range rather than the token: the sibling's own band measures 90.
  expect(m.panel, `the band is ${m.panel}px, outside the measured range`)
    .toBeGreaterThanOrEqual(80);
  expect(m.panel, `the band is ${m.panel}px, outside the measured range`)
    .toBeLessThanOrEqual(96);
  expect(m.row, "the group box is not the measured 63px").toBe(63);
  // ONLYOFFICE's `@toolbar-height-tabs-top-title` — the height the strip takes
  // under the document title rather than beside it.
  expect(m.tabs, "the tab strip is not the measured 28px").toBeLessThanOrEqual(30);
});

/// **The band is a card, not a strip.**
///
/// The single thing that made the sibling's chrome read as an application
/// rather than a web page, and the one the first port attempt missed entirely:
/// the geometry was right and nothing separated the band from the field behind
/// it, because `--bg` had been aliased to a near-white token. A card that is
/// the same colour as its background is not a card.
test("the band is a card on a field, not white on white", async ({ page }) => {
  await boot(page);
  const c = await page.evaluate(() => {
    const rgb = (el) => getComputedStyle(el).backgroundColor;
    const parse = (s) => (s.match(/[\d.]+/g) || []).slice(0, 3).map(Number);
    const card = parse(rgb(document.querySelector(".rb-panel:not([hidden])")));
    const field = parse(rgb(document.querySelector(".oc-ribbon")));
    return { card, field, radius: getComputedStyle(document.querySelector(".rb-panel:not([hidden])")).borderRadius };
  });
  expect(c.card.length, "the card has no resolved background").toBe(3);
  expect(c.field.length, "the field has no resolved background").toBe(3);
  // Manhattan distance in 8-bit RGB. Small but non-zero is the whole point:
  // these two shades must differ enough to read as separate surfaces.
  const delta = c.card.reduce((a, v, i) => a + Math.abs(v - c.field[i]), 0);
  expect(delta, `the card and the field are the same shade (${c.card} vs ${c.field}) — ` +
    "the band does not read as a card").toBeGreaterThan(8);
  expect(c.radius, "the card has no corner radius").not.toBe("0px");
});

test("every group caption is painted, and none is display:none", async ({ page }) => {
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
      folded: !!c.closest(".rb-group.is-folded"),
    })),
  );

  // Not a vacuous pass: there have to be captions to be checking.
  expect(caps.length, "no group captions in the tree at all — the selector moved")
    .toBeGreaterThanOrEqual(5);
  // Painted, matching the sibling's shipped build. A folded group is the one
  // exception: its launcher already prints the name, so its caption is
  // visually hidden (and must still not be `display: none`).
  const unfolded = caps.filter((c) => !c.folded);
  expect(
    unfolded.filter((c) => c.w <= 2).map((c) => c.text),
    "a group caption is not painted; the sibling's shipped ribbon paints them",
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
