// Scrolling the grid must not make the chrome do work (UX-SHELL-22).
//
// The editor refreshes its toolbar state on every frame, a scroll included,
// and rewrote the same attribute values each time. The ribbon watches those
// buttons; every same-value write was still a mutation record, so the ribbon
// re-synced — and rebuilt the phone bar, displayed or not — on every frame:
// 16,110 DOM mutations inside the chrome during 1.5s of scrolling. On a slow
// machine that halved the frame rate (43 frames instead of 91 at 4x CPU
// throttling), and it surfaced as an accessibility test failing on CI because
// the view "did not move far enough".
//
// Counted rather than timed: a mutation count is the same on every machine,
// where a frame rate is not.

import { expect, test } from "@playwright/test";

test("a scroll causes no DOM work in the ribbon or the phone bar", async ({ page }) => {
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    api.session_new();
    const line = Array.from({ length: 12 }, (_, c) => (c % 3 ? "12345" : "Item text")).join("\t");
    api.session_paste_tsv(0, 0, 0, Array.from({ length: 800 }, () => line).join("\n"));
  });
  await page.waitForTimeout(400);

  const m = await page.evaluate(async () => {
    let chrome = 0, frames = 0;
    const ribbon = document.querySelector(".oc-ribbon");
    const syncs0 = ribbon._syncs || 0;
    const o = new MutationObserver((ms) => {
      for (const r of ms) if (r.target.closest?.(".oc-ribbon, .rb-mobile")) chrome++;
    });
    o.observe(document.body, { subtree: true, attributes: true, childList: true, characterData: true });
    const grid = document.querySelector("#grid");
    const v0 = window.opencalcEditor.geo.rowIdx[0];
    for (; frames < 60; frames++) {
      grid.dispatchEvent(new WheelEvent("wheel", { deltaY: 60, bubbles: true, cancelable: true }));
      await new Promise((r) => requestAnimationFrame(r));
    }
    await new Promise((r) => setTimeout(r, 100));
    o.disconnect();
    return { chrome, frames, syncs: (ribbon._syncs || 0) - syncs0, moved: window.opencalcEditor.geo.rowIdx[0] - v0 };
  });
  expect(m.moved, "the view did not scroll, so this proves nothing").toBeGreaterThan(20);
  // Nothing about the ribbon changes when the view moves; a handful allows for
  // one legitimate sync, never one per frame.
  // The cause, not just the symptom: the ribbon re-ran every control's state
  // on every frame even where that changed nothing on screen.
  expect(m.syncs, `the ribbon re-synced ${m.syncs} times over ${m.frames} frames`).toBeLessThanOrEqual(3);
  expect(m.chrome, `${m.chrome} chrome mutations over ${m.frames} frames of scrolling`).toBeLessThan(m.frames);
});

/// The phone bar skips painting while it is not displayed, so it has to paint
/// on the way in: a desktop window narrowed to a phone gets a full bar, not an
/// empty one.
test("a window narrowed to a phone gets a populated command bar", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/editor.html?sample=demo");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.setViewportSize({ width: 390, height: 800 });
  await expect(page.locator(".rb-micons .rb-mbtn").first()).toBeVisible();
  expect(await page.locator(".rb-micons .rb-mbtn").count()).toBeGreaterThan(5);
});
