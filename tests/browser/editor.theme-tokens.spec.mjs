// A host's theme has to reach the whole of the chrome, not most of it.
//
// The focus ring and the text selection were written as `rgba(47, 109, 246, …)`
// — the *default* accent, spelled out. A host that set a brand accent got
// brand-coloured buttons and blue focus rings. It was invisible to us because
// our own accent is that blue, which is the way this class of bug always hides
// (`UX-TOKEN-01`).
//
// Asserted on computed style, resolved by the browser, so `color-mix` and the
// token chain are exercised rather than assumed.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// Resolve a custom property to the value the browser computed.
const token = (page, name) =>
  page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(),
    name,
  );

/// **A brand accent reaches the derived ring and selection.**
///
/// This is the defect: they used to be a fixed blue, so only the accent itself
/// followed the brand.
test("a brand accent reaches the focus ring and the selection tint", async ({ page }) => {
  await boot(page, "?accent=%23ff0055");

  expect(await token(page, "--oc-accent-color")).toBe("#ff0055");
  for (const derived of ["--oc-accent-ring", "--oc-accent-soft"]) {
    const value = await token(page, derived);
    expect(value, `${derived} is empty — the token chain is broken`).not.toBe("");
    // Resolved by the browser, so this is the colour that will actually paint.
    expect(
      value.replace(/\s/g, ""),
      `${derived} did not follow the brand accent: ${value}`,
    ).toMatch(/255,0,85|ff0055/i);
  }
});

/// **Without a brand, the defaults still resolve.** The control: a broken
/// `color-mix` or a missing token would leave these empty, and an empty
/// `box-shadow` colour is an invisible focus ring rather than a wrong one.
test("the derived tokens resolve to something without any branding", async ({ page }) => {
  await boot(page);
  for (const name of ["--oc-accent-ring", "--oc-accent-soft", "--oc-danger-ring", "--oc-elevation-overlay"]) {
    expect(await token(page, name), `${name} resolved to nothing`).not.toBe("");
  }
});

/// **`dangerColor` does something.**
///
/// It sat in the published theme list while `--oc-danger-color` appeared
/// nowhere in the stylesheet, so a host that set it changed nothing at all and
/// had no way to find out.
test("the advertised danger token is honoured", async ({ page }) => {
  await boot(page);
  const danger = await token(page, "--oc-danger-color");
  expect(danger, "--oc-danger-color is not defined").not.toBe("");

  const applied = await page.evaluate(() => {
    document.documentElement.style.setProperty("--oc-danger-color", "rgb(1, 2, 3)");
    const el = document.createElement("div");
    el.className = "cf-preview bad";
    document.body.append(el);
    const seen = getComputedStyle(el).color;
    el.remove();
    return seen;
  });
  expect(applied, "setting dangerColor changed nothing").toBe("rgb(1, 2, 3)");
});

/// **Overlay depth is one decision, not four.**
///
/// Menus, popovers, dialogs and toasts all float above the page. They carried
/// four different hand-written shadows, none of which changed in dark mode.
test("floating surfaces share one elevation, and it is theme-aware", async ({ page }) => {
  await boot(page);
  const light = await token(page, "--oc-elevation-overlay");
  await page.emulateMedia({ colorScheme: "dark" });
  const dark = await token(page, "--oc-elevation-overlay");

  expect(light).not.toBe("");
  expect(dark).not.toBe("");
  expect(dark, "the overlay shadow is the same in dark mode — it is not theme-aware").not.toBe(light);
});

/// **The ported chrome's tokens resolve, and the colour half really is aliased.**
///
/// The ribbon and Compact chromes are drawn against a visual contract ported
/// from the sibling editor's `docs/123` (`UX-RIB-14`), whose stylesheet depends
/// on 29 custom properties that did not exist here. `tools/check-chrome-tokens.py`
/// proves statically that each one is *declared*; only a browser can prove they
/// **resolve**, because the value travels through an alias chain into the
/// `--oc-*` spine and, for two of them, through `color-mix`.
///
/// The second half is the one worth having. An undefined custom property is
/// dropped in silence — no error, no warning, an element that merely looks
/// plausible at the wrong size — and a *literal* in the colour half is worse
/// still: it resolves perfectly, passes every light-mode check, and is simply
/// dark-blind. So the assertion is not "it has a value" but "it has a
/// **different** value once the theme flips", which a hardcoded hex cannot
/// satisfy.
test("the ported chrome tokens resolve, and the colour half follows the theme", async ({ page }) => {
  // The geometry half: measured numbers from docs/123, identical in both
  // themes by definition — a band does not change height in the dark.
  const GEOMETRY = {
    "--h-header": "58px",
    "--h-tab": "28px",
    "--ribbon-band-h": "74px",
    "--icon-control": "18px",
    "--fs-ribbon-tab": "13.5px",
  };
  // The colour half: aliases onto `--oc-*`, so each must MOVE with the theme.
  const COLOURS = ["--ink", "--muted", "--faint", "--line", "--line-strong", "--bg", "--surface"];

  await boot(page, "?chrome=ribbon");
  const light = {};
  for (const name of [...Object.keys(GEOMETRY), ...COLOURS]) {
    const value = await token(page, name);
    expect(value, `${name} resolved to nothing — the token spine is not loaded`).not.toBe("");
    light[name] = value;
  }
  for (const [name, expected] of Object.entries(GEOMETRY)) {
    expect(light[name], `${name} is not the measured value from docs/123`).toBe(expected);
  }

  await page.emulateMedia({ colorScheme: "dark" });
  await boot(page, "?chrome=ribbon");
  for (const name of COLOURS) {
    const dark = await token(page, name);
    expect(dark, `${name} resolved to nothing in dark mode`).not.toBe("");
    expect(
      dark,
      `${name} is identical in both themes (${dark}) — it is a literal, not an alias ` +
        "onto the --oc-* spine, so the chrome is dark-blind here",
    ).not.toBe(light[name]);
  }
  // Geometry does not move with the theme.
  for (const [name, expected] of Object.entries(GEOMETRY)) {
    expect(await token(page, name), `${name} changed with the theme`).toBe(expected);
  }
});
