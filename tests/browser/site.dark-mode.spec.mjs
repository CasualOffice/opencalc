// Every page of the site has to survive dark mode, not just the one that was
// looked at.
//
// Dark mode was added as a token swap in `style.css` and verified on the
// landing page — which was the only page whose styles live entirely in that
// file. `deploy.html` and `docs.html` carry their own `<style>` blocks with
// literal light values for inline code and callouts, so they shipped painting
// white boxes onto a dark ground (`UX-SITE-01`).
//
// The check is deliberately mechanical rather than a screenshot comparison: it
// asks whether anything of consequence is still painting itself near-white, and
// a new page with its own stylesheet gets caught the first time it is added.

import { expect, test } from "@playwright/test";

const PAGES = ["index", "deploy", "docs"];

for (const name of PAGES) {
  test(`${name}.html paints nothing white in dark mode`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(`/${name}.html`, { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(600);

    const offenders = await page.evaluate(() => {
      const out = [];
      for (const el of document.querySelectorAll("*")) {
        const m = getComputedStyle(el).backgroundColor.match(/^rgba?\((\d+), (\d+), (\d+)(?:, ([\d.]+))?/);
        if (!m) continue;
        const [r, g, b] = [+m[1], +m[2], +m[3]];
        const alpha = m[4] === undefined ? 1 : parseFloat(m[4]);
        // Transparent things paint nothing; tiny things are rules and dots.
        if (alpha < 0.5) continue;
        const box = el.getBoundingClientRect();
        if (box.height < 24 || box.width < 24) continue;
        if (r > 235 && g > 235 && b > 235) {
          out.push(`${el.tagName.toLowerCase()}.${String(el.className || "").split(" ")[0]}`);
        }
      }
      return [...new Set(out)];
    });

    expect(
      offenders,
      "these still paint a near-white surface on a dark page",
    ).toEqual([]);

    // And the page itself actually went dark — an empty offender list would
    // otherwise pass on a page that never applied the theme at all.
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const [r, g, b] = bg.match(/\d+/g).map(Number);
    expect(r + g + b, `body background ${bg} is dark`).toBeLessThan(180);
  });
}
