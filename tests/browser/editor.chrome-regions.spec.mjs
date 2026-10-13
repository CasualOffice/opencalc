// Hiding chrome by region, without hiding things that are not chrome.
//
// Retaken for the ribbon shell (UX-SHELL-21): the menu bar it was written
// against is parked off-screen there, so the old assertions measured a box no
// one could see and the roster with it.
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

/// Where the roster is and whether it can be seen, with or without a session.
///
/// The roster's markup sits in the menu bar, which the ribbon shell parks
/// off-screen; the shell seats it beside Share in the title bar, or at the end
/// of the tab row when a host has hidden the title bar. Measured on the page,
/// not on classes: the promise is that a person can see it.
const roster = (page, { withPresence }) =>
  page.evaluate((show) => {
    const presence = document.getElementById("presence");
    // The roster carries `hidden` until there is a collaborative session, which
    // is the state most of these assertions turn on.
    if (show) presence.removeAttribute("hidden");
    else presence.setAttribute("hidden", "");
    const r = presence.getBoundingClientRect();
    const s = getComputedStyle(presence);
    const header = document.querySelector(".app-header");
    return {
      presenceSeen: s.display !== "none" && s.visibility !== "hidden" && r.width > 0 && r.height > 0
        && r.right <= innerWidth && r.left >= 0 && r.top >= 0 && r.bottom <= innerHeight,
      inTitleBar: !!presence.closest(".app-header"),
      inTabRow: !!presence.closest(".rb-tabs"),
      headerShown: getComputedStyle(header).display !== "none",
      tabRowHeight: document.querySelector(".rb-tabs").getBoundingClientRect().height,
    };
  }, withPresence);

async function bootRibbon(page, query = "") {
  await page.goto(`/editor.html?sample=demo${query}`);
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await expect(page.locator(".oc-ribbon .rb-tabs")).toBeVisible();
}

/// **The roster can be seen in the shell at all.** It lives in the menu bar's
/// markup, and the ribbon parks that bar off-screen: before this, a co-editor
/// was present in the document and visible to nobody.
test("the collaborator roster sits in the title bar, on screen", async ({ page }) => {
  await bootRibbon(page);
  const at = await roster(page, { withPresence: true });
  expect(at.inTitleBar, "the roster is not in the title bar").toBe(true);
  expect(at.presenceSeen, "the roster is off-screen").toBe(true);
});

/// **A host hiding the title bar keeps the roster.**
test("hiding the title bar keeps the collaborator roster", async ({ page }) => {
  await bootRibbon(page, "&hide=header");
  const at = await roster(page, { withPresence: true });
  expect(at.headerShown, "the host asked for no title bar").toBe(false);
  expect(at.inTabRow, "the roster went with the title bar").toBe(true);
  expect(at.presenceSeen, "the roster is present but cannot be seen").toBe(true);
});

/// **With nobody else present, it costs no room.** Seating the roster in the
/// tab row must not grow the row for the overwhelmingly common single-user
/// embed.
test("with no roster to show, the tab row is no taller", async ({ page }) => {
  await bootRibbon(page);
  const plain = await roster(page, { withPresence: false });
  await bootRibbon(page, "&hide=header");
  const at = await roster(page, { withPresence: false });
  expect(at.presenceSeen).toBe(false);
  expect(at.tabRowHeight, "the tab row grew with nobody to show").toBe(plain.tabRowHeight);
});
