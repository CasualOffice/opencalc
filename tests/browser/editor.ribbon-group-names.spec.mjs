// Group names by default, icon labels off (owner, 2026-10-14: "we can have
// group name.. but naming each icon that i disagreed on"). XL-UX-006/007/010.
//
// Excel captions every group, puts a dialog launcher in the corner of Font,
// Alignment, Number and Clipboard, and labels a folded group with its name.
// The ribbon had all three behind an opt-in setting, so Format Cells had no
// mouse path from Home and a narrow window showed rows of bare icons.

import { expect, test } from "@playwright/test";

async function boot(page, width) {
  await page.setViewportSize({ width, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => { try { localStorage.removeItem("oc.ribbon.labels"); } catch {} });
  await page.reload();
  await expect(page.locator(".oc-ribbon .rb-panel:not([hidden])")).toBeVisible();
}

test("every group on Home is captioned by default", async ({ page }) => {
  await boot(page, 1600);
  const caps = await page.locator(".rb-panel:not([hidden]) .rb-group:not(.is-folded) > .rb-cap").evaluateAll((cs) =>
    cs.map((c) => ({ text: c.textContent.trim(), shown: getComputedStyle(c).display !== "none" && c.getBoundingClientRect().height > 0 })));
  expect(caps.length, "no groups").toBeGreaterThan(4);
  expect(caps.filter((c) => !c.shown).map((c) => c.text), "groups with no visible name").toEqual([]);
  expect(caps.map((c) => c.text)).toContain("Font");
});

test("buttons carry no text label by default", async ({ page }) => {
  await boot(page, 1600);
  const labelled = await page.locator(".rb-panel:not([hidden]) .rb-group:not(.is-folded) .rb-l .rb-label").evaluateAll((ls) =>
    ls.filter((l) => l.getClientRects().length > 0).map((l) => l.textContent));   // drawn, not merely styled
  expect(labelled, "icon buttons should not be named under the icon").toEqual([]);
});

test("Font's dialog launcher opens Format Cells", async ({ page }) => {
  await boot(page, 1600);
  const launch = page.locator(".rb-panel:not([hidden]) .rb-group", { has: page.locator(".rb-cap-text", { hasText: /^Font$/ }) }).locator(".rb-launch");
  await expect(launch, "no visible launcher on Font").toBeVisible();
  await launch.click();
  await expect(page.locator(".oc-modal-box, [role=dialog]").filter({ hasText: "Format Cells" }).first()).toBeVisible();
});

test("a folded group shows its name", async ({ page }) => {
  await boot(page, 900);
  const folded = page.locator(".rb-panel:not([hidden]) .rb-group.is-folded .rb-folded");
  expect(await folded.count(), "900px should fold something on Home").toBeGreaterThan(0);
  const names = await folded.evaluateAll((bs) => bs.map((b) => {
    const l = b.querySelector(".rb-label");
    return { name: b.getAttribute("aria-label"), shown: !!l && l.getClientRects().length > 0 && l.textContent.trim().length > 0 };
  }));
  expect(names.filter((n) => !n.shown).map((n) => n.name), "folded groups with no visible name").toEqual([]);
});
