// What the status bar says must agree with what the engine did.
//
// `clipToOS` arms the engine's clipboard and starts the marching ants *first*,
// then tries the system clipboard, and returns false only if that last write
// fails. `doCut` read that false as "cut blocked" — so on a clipboard
// permission denial the user was told the cut had not happened while the ants
// were drawn and the cut was armed, and the next Ctrl+V moved the data.
//
// `stopMarch`'s own comment names this failure from the other side: "The
// visible signal said cancelled and the state said otherwise, which is the
// worst possible pairing for an action that deletes." This is that pairing,
// inverted.
//
// The honest answer is not to un-arm the cut — it works perfectly well inside
// the application — but to say which half failed.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/** Refuse the system clipboard the way a browser without permission does. */
async function denySystemClipboard(page) {
  await page.evaluate(() => {
    const deny = () => Promise.reject(new Error("clipboard permission denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { write: deny, writeText: deny, read: deny, readText: deny },
    });
  });
}

test("a refused system clipboard does not claim the cut was blocked", async ({ page }) => {
  await boot(page);
  await denySystemClipboard(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 8, 0, "movable");
    window.opencalcEditor.selectForTest(8, 0);
  });

  await page.evaluate(() => window.opencalcEditor.doCut());

  // The engine's cut is armed — so saying it was blocked is false, and the next
  // paste proves it by moving the data.
  await expect
    .poll(() => page.evaluate(() => window.opencalcEditor.wasmApi().session_clip_has()))
    .toBe(true);
  const said = await page.locator("#tb-status").textContent();
  expect(said, `status said: ${said}`).not.toMatch(/blocked/i);
  // And it says which half failed, rather than implying nothing happened.
  expect(said).toMatch(/clipboard/i);
});

test("the armed cut still moves the data, which is why 'blocked' was wrong", async ({ page }) => {
  await boot(page);
  await denySystemClipboard(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 8, 0, "movable");
    window.opencalcEditor.selectForTest(8, 0);
  });
  await page.evaluate(() => window.opencalcEditor.doCut());

  await page.evaluate(() => {
    window.opencalcEditor.selectForTest(8, 2);
    window.opencalcEditor.doPasteMode("all");
  });
  const api = (r, c) =>
    page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
  await expect.poll(() => api(8, 2)).toBe("movable");
  expect(await api(8, 0), "a cut moves rather than copies").toBe("");
});

test("a working clipboard still reports plainly", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_cell(0, 8, 0, "movable");
    window.opencalcEditor.selectForTest(8, 0);
  });
  await page.evaluate(() => window.opencalcEditor.doCut());
  // The ordinary path must not grow a caveat nobody needs to read.
  await expect(page.locator("#tb-status")).toHaveText("cut");
});
