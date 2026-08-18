// Hiding chrome by region, without hiding things that are not chrome.
//
// A host embedding the editor turns regions off by name. The menu bar is one of
// them — and the roster of who else is in the document lives *inside* it, put
// there by COL-33 precisely so it would not fold away with the page header.
// `display: none` on the bar took the whole subtree, so a host that hid the
// menus also lost the ability to see who they were working with, which is not a
// menu (`UX-CHROME-01`).
//
// Asserted on computed style rather than on the class being present: the class
// being applied is not the promise, the element being visible is.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// Turn the region off the way the embed API does — a class on an ancestor.
const hideMenubar = (page, { withPresence }) =>
  page.evaluate((show) => {
    const presence = document.getElementById("presence");
    // The roster carries `hidden` until there is a collaborative session, which
    // is the state most of these assertions turn on.
    if (show) presence.removeAttribute("hidden");
    else presence.setAttribute("hidden", "");
    document.body.classList.add("oc-hide-menubar");
    const seen = (el) => {
      const s = getComputedStyle(el);
      return s.display !== "none" && s.visibility !== "hidden";
    };
    const bar = document.getElementById("menubar");
    const menuItem = bar.querySelector('[data-oc-label], .menu-btn, button:not(.presence-btn)');
    return {
      barHeight: bar.getBoundingClientRect().height,
      presenceSeen: seen(presence),
      menuItemSeen: menuItem ? seen(menuItem) : null,
    };
  }, withPresence);

/// **A host hiding the menu bar keeps the roster.**
test("hiding the menu bar keeps the collaborator roster", async ({ page }) => {
  await boot(page);
  const at = await hideMenubar(page, { withPresence: true });

  expect(at.presenceSeen, "the roster went with the menu bar").toBe(true);
  expect(at.barHeight, "the roster is present but has no height to be seen in").toBeGreaterThan(0);
});

/// **The menus themselves do go.** Otherwise "hide the menu bar" hides nothing
/// and the first assertion passes for the wrong reason.
test("hiding the menu bar does hide the menus", async ({ page }) => {
  await boot(page);
  const at = await hideMenubar(page, { withPresence: true });
  expect(at.menuItemSeen, "a menu item survived a hidden menu bar").toBe(false);
});

/// **With nobody else present, a hidden menu bar is genuinely gone.**
///
/// The control for the fix above: keeping the bar alive to carry the roster
/// must not leave an empty strip on every single-user embed, which would be a
/// visible regression for the overwhelmingly common case.
test("with no roster to show, a hidden menu bar takes up no room", async ({ page }) => {
  await boot(page);
  const at = await hideMenubar(page, { withPresence: false });
  expect(at.presenceSeen).toBe(false);
  expect(at.barHeight, "an empty strip is left where the menu bar was").toBe(0);
});
