// A primary button keeps its fill on hover (owner, 2026-10-14: "on hover it
// changed to white color with white color").
//
// The shared `.oc-btn:hover` rule set the light surface colour as background,
// and it outranked `.oc-btn.primary`, so OK/Create/Apply turned into white text
// on a near-white button the moment the pointer reached them.

import { expect, test } from "@playwright/test";

const luminance = (rgb) => {
  const [r, g, b] = rgb.match(/[\d.]+/g).slice(0, 3).map(Number).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a, b) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

for (const colorScheme of ["light", "dark"]) {
  test(`primary buttons stay readable on hover (${colorScheme})`, async ({ browser }) => {
    const page = await (await browser.newContext({ colorScheme })).newPage();
    await page.goto("/editor.html?sample=demo");
    await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
    for (const [open, sel] of [
      ["insert.table", "#oc-modal .oc-btn.primary"],
      ["data.data-validation", "#side-panel .oc-btn.primary"],
    ]) {
      await page.evaluate((id) => window.opencalcEditor.runCommand(id), open);
      const btn = page.locator(sel).first();
      await btn.hover();
      await page.waitForTimeout(150);
      const { fg, bg } = await btn.evaluate((e) => {
        const s = getComputedStyle(e);
        // Resolve whatever colour syntax the browser returns to rgb().
        const probe = document.createElement("canvas").getContext("2d");
        probe.fillStyle = s.backgroundColor; const bg = probe.fillStyle;
        probe.fillStyle = s.color; const fg = probe.fillStyle;
        const hex = (h) => h.startsWith("#") ? `rgb(${parseInt(h.slice(1, 3), 16)},${parseInt(h.slice(3, 5), 16)},${parseInt(h.slice(5, 7), 16)})` : h;
        return { fg: hex(fg), bg: hex(bg) };
      });
      expect(contrast(fg, bg), `${sel} on hover is ${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      await page.keyboard.press("Escape");
    }
  });
}
