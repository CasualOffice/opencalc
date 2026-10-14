// The public site's contract: what every page of it must keep, whoever
// redesigns it next.
//
// The site is the first thing anybody sees of the project, and its failures
// are invisible from the inside: a page that slides sideways on a phone, an
// animation that ignores a visitor who asked for none, an illustration a
// screen reader announces as "image", grey text nobody can read in the dark,
// a link to an anchor that was renamed. None of those break a build. This pins
// them mechanically, page by page, so a new page is held to the same bar the
// first time it is added to `PAGES`.

import { expect, test } from "@playwright/test";

const PAGES = ["index", "docs", "deploy", "download", "embed"];

/// Open a page without waiting on the editor the embed demo boots.
async function open(page, name) {
  await page.goto(`/${name}.html`, { waitUntil: "domcontentloaded" });
  await page.waitForLoadState("load").catch(() => {});
}

/// Scroll the whole page so every reveal has had its chance to run.
async function scrollThrough(page) {
  await page.evaluate(async () => {
    for (let y = 0; y < document.documentElement.scrollHeight; y += 400) {
      window.scrollTo(0, y);
      await new Promise((r) => setTimeout(r, 30));
    }
    window.scrollTo(0, 0);
  });
}

for (const name of PAGES) {
  test.describe(`${name}.html`, () => {
    test("does not scroll sideways at 360px, and cuts nothing off", async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 780 });
      await open(page, name);
      await page.waitForTimeout(400);
      const r = await page.evaluate(() => {
        const unreachable = [];
        for (const el of document.querySelectorAll("body *")) {
          const box = el.getBoundingClientRect();
          if (box.width === 0 || box.right <= window.innerWidth + 1) continue;
          let n = el.parentElement, ok = false;
          while (n && n !== document.documentElement) {
            const ox = getComputedStyle(n).overflowX;
            if ((ox === "auto" || ox === "scroll") && n.scrollWidth > n.clientWidth + 1) { ok = true; break; }
            n = n.parentElement;
          }
          if (!ok) unreachable.push(`${el.tagName.toLowerCase()}.${String(el.className?.baseVal ?? el.className ?? "").split(" ")[0]}`);
        }
        return { width: document.documentElement.scrollWidth, unreachable: [...new Set(unreachable)] };
      });
      expect(r.width, "the document is no wider than a 360px phone").toBeLessThanOrEqual(361);
      expect(r.unreachable, "wide content must scroll inside its own box, not be cut off").toEqual([]);
    });

    test("has landmarks, one h1, and a way to every main destination on a phone", async ({ page }) => {
      await page.setViewportSize({ width: 360, height: 780 });
      await open(page, name);
      const shape = await page.evaluate(() => ({
        header: !!document.querySelector("body > header, header.site-header"),
        main: document.querySelectorAll("main").length,
        footer: !!document.querySelector("footer"),
        h1: document.querySelectorAll("h1").length,
        skip: !!document.querySelector('a[href="#main"]'),
      }));
      expect(shape).toEqual({ header: true, main: 1, footer: true, h1: 1, skip: true });

      // On a phone the desktop nav is hidden; something visible must open a menu
      // that reaches the same places.
      const wanted = ["docs.html", "download.html", "deploy.html", "embed.html", "editor.html"];
      const visibleNow = async () =>
        page.evaluate(() => [...document.querySelectorAll("header a[href]")]
          .filter((a) => a.getClientRects().length && getComputedStyle(a).visibility !== "hidden")
          .map((a) => a.getAttribute("href")));
      let reachable = await visibleNow();
      if (!wanted.every((w) => reachable.some((h) => h.includes(w)))) {
        const toggle = page.locator("header summary, header button[aria-expanded]").first();
        await expect(toggle, "a visible menu button on a phone").toBeVisible();
        await toggle.click();
        reachable = await visibleNow();
      }
      for (const w of wanted) {
        expect(reachable.some((h) => h.includes(w)), `${w} is reachable from the header at 360px`).toBe(true);
      }
    });

    test("every illustration is named or hidden from assistive tech", async ({ page }) => {
      await open(page, name);
      const unnamed = await page.evaluate(() => {
        const out = [];
        const hidden = (el) => !!el.closest('[aria-hidden="true"]');
        for (const svg of document.querySelectorAll("svg")) {
          if (hidden(svg) || svg.closest("svg") !== svg) continue;
          if (getComputedStyle(svg).display === "none") continue;
          const named = svg.getAttribute("role") === "img" &&
            ((svg.getAttribute("aria-label") || "").trim() || svg.querySelector("title"));
          if (!named) out.push(`svg.${svg.getAttribute("class") || "(no class)"} in ${svg.parentElement?.className || svg.parentElement?.tagName}`);
        }
        for (const img of document.querySelectorAll("img")) if (!img.hasAttribute("alt") && !hidden(img)) out.push(`img ${img.src}`);
        for (const f of document.querySelectorAll("iframe")) if (!(f.getAttribute("title") || "").trim()) out.push(`iframe ${f.src}`);
        // An element that is a picture by role needs a name too.
        for (const el of document.querySelectorAll('[role="img"]')) {
          if (!hidden(el) && !(el.getAttribute("aria-label") || el.getAttribute("aria-labelledby"))) out.push(`[role=img] ${el.className}`);
        }
        return out;
      });
      expect(unnamed, "unnamed images (give them role=img and a label, or aria-hidden)").toEqual([]);
    });

    test("every internal link resolves, anchors included", async ({ page, request, baseURL }) => {
      await open(page, name);
      const links = await page.evaluate(() =>
        [...document.querySelectorAll("a[href]")]
          .map((a) => a.href)
          .filter((h) => h.startsWith(location.origin)));
      const pages = new Map();
      const problems = [];
      for (const href of [...new Set(links)]) {
        const url = new URL(href);
        const key = url.pathname;
        if (!pages.has(key)) {
          const res = await request.get(new URL(key, baseURL).toString());
          pages.set(key, { status: res.status(), body: res.status() === 200 ? await res.text() : "" });
        }
        const got = pages.get(key);
        if (got.status !== 200) { problems.push(`${href} → ${got.status}`); continue; }
        const id = decodeURIComponent(url.hash.slice(1));
        if (id && !got.body.includes(`id="${id}"`)) problems.push(`${href} → no element #${id}`);
      }
      expect(problems).toEqual([]);
    });

    test("dark mode applies through the tokens", async ({ page }) => {
      await page.emulateMedia({ colorScheme: "light" });
      await open(page, name);
      const light = await page.evaluate(() => ({
        bg: getComputedStyle(document.body).backgroundColor,
        brand: getComputedStyle(document.documentElement).getPropertyValue("--brand").trim(),
      }));
      await page.emulateMedia({ colorScheme: "dark" });
      const dark = await page.evaluate(() => ({
        bg: getComputedStyle(document.body).backgroundColor,
        brand: getComputedStyle(document.documentElement).getPropertyValue("--brand").trim(),
      }));
      const sum = (c) => c.match(/\d+/g).slice(0, 3).map(Number).reduce((a, b) => a + b, 0);
      expect(sum(light.bg), `light background ${light.bg}`).toBeGreaterThan(600);
      expect(sum(dark.bg), `dark background ${dark.bg}`).toBeLessThan(120);
      expect(dark.brand, "the brand colour is re-tuned for a dark ground").not.toBe(light.brand);
      expect(light.brand.toLowerCase()).toBe("#0e7a55");
    });

    for (const scheme of ["light", "dark"]) {
      test(`text meets WCAG AA contrast in ${scheme}`, async ({ page }) => {
        // Reduced motion, so everything is in its final, fully opaque state.
        await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
        await open(page, name);
        const failures = await page.evaluate(() => {
          const parse = (c) => {
            const m = c.match(/rgba?\(([^)]+)\)/);
            if (!m) return null;
            const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
            return [p[0], p[1], p[2], p.length > 3 ? p[3] : 1];
          };
          const over = (top, under) => {
            const a = top[3];
            return [0, 1, 2].map((i) => top[i] * a + under[i] * (1 - a)).concat(1);
          };
          const lum = ([r, g, b]) => {
            const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
            return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
          };
          const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
          // The background actually under an element: its own and its
          // ancestors' fills, composited from the root up. `null` when an
          // image or gradient is involved, which this cannot judge.
          const background = (el) => {
            const chain = [];
            for (let n = el; n; n = n.parentElement) chain.push(n);
            let bg = [255, 255, 255, 1];
            for (const n of chain.reverse()) {
              const cs = getComputedStyle(n);
              if (cs.backgroundImage !== "none" && n !== document.documentElement) return null;
              const c = parse(cs.backgroundColor);
              if (c && c[3] > 0) bg = over(c, bg);
            }
            return bg;
          };
          const out = [];
          const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
          const seen = new Set();
          for (let t = walker.nextNode(); t; t = walker.nextNode()) {
            if (!t.nodeValue.trim()) continue;
            const el = t.parentElement;
            if (!el || seen.has(el)) continue;
            seen.add(el);
            if (el.closest('svg, [aria-hidden="true"], script, style, noscript, .sr-only, .skip, opencalc-sheet, [hidden]')) continue;
            const r = el.getBoundingClientRect();
            if (!r.width || !r.height) continue;
            const cs = getComputedStyle(el);
            if (cs.visibility === "hidden") continue;
            const bg = background(el);
            const fg = parse(cs.color);
            if (!bg || !fg) continue;
            const c = ratio(over(fg, bg), bg);
            const px = parseFloat(cs.fontSize), bold = parseInt(cs.fontWeight, 10) >= 700;
            const need = px >= 24 || (bold && px >= 18.66) ? 3 : 4.5;
            if (c < need - 0.01) out.push(`${el.tagName.toLowerCase()}.${String(el.className).split(" ")[0]} "${t.nodeValue.trim().slice(0, 30)}" ${c.toFixed(2)}:1 < ${need}`);
          }
          return [...new Set(out)];
        });
        expect(failures, "text below WCAG AA").toEqual([]);
      });
    }
  });
}

test.describe("motion", () => {
  test("reduced motion: nothing animates and every reveal is in its final state", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    for (const name of PAGES) {
      await open(page, name);
      await scrollThrough(page);
      await page.waitForTimeout(300);
      const r = await page.evaluate(() => {
        const running = document.getAnimations()
          .filter((a) => a.playState === "running")
          .map((a) => `${a.animationName || a.transitionProperty || a.constructor.name} on ${a.effect?.target?.className?.baseVal ?? a.effect?.target?.className ?? "?"}`);
        const named = [...document.querySelectorAll("body *")]
          .filter((el) => !el.closest("opencalc-sheet"))
          .filter((el) => getComputedStyle(el).animationName !== "none")
          .map((el) => `${el.tagName.toLowerCase()}.${String(el.className?.baseVal ?? el.className).split(" ")[0]}`);
        const faded = [...document.querySelectorAll(".rv, .seq > *, .pipe .node, .il-wrap .pop")]
          .filter((el) => parseFloat(getComputedStyle(el).opacity) < 1)
          .map((el) => el.className?.baseVal ?? el.className);
        return {
          motionClass: document.documentElement.classList.contains("motion"),
          running, named: [...new Set(named)], faded,
          story: document.querySelector("[data-story]")?.getAttribute("data-step") ?? "none",
        };
      });
      expect(r, `${name}.html under prefers-reduced-motion`).toEqual({
        motionClass: false, running: [], named: [], faded: [], story: "none",
      });
    }
  });

  test("motion allowed: the hero story plays, can be paused, and the page reveals", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await open(page, "index");
    await expect(page.locator("html")).toHaveClass(/\bmotion\b/);
    const story = page.locator("[data-story]");
    // It starts from step 0 and moves on by itself.
    await expect(story).toHaveAttribute("data-step", /^[0-4]$/, { timeout: 5_000 });
    const first = await story.getAttribute("data-step");
    await expect.poll(() => story.getAttribute("data-step"), { timeout: 8_000 }).not.toBe(first);

    const pause = page.locator(".story-toggle");
    await expect(pause).toBeVisible();
    await pause.click();
    await expect(pause).toHaveAttribute("aria-pressed", "true");
    const held = await story.getAttribute("data-step");
    await page.waitForTimeout(4_000);
    expect(await story.getAttribute("data-step"), "paused means paused").toBe(held);

    // Below the fold is hidden until it is scrolled to, then revealed.
    const tile = page.locator("#product .tile").first();
    expect(await tile.evaluate((el) => getComputedStyle(el).opacity)).toBe("0");
    await tile.scrollIntoViewIfNeeded();
    await expect.poll(() => tile.evaluate((el) => getComputedStyle(el).opacity)).toBe("1");
  });
});
