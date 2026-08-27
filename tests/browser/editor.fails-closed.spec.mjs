// Two guards that failed open, and one draft that was destroyed.
//
// The pattern in all three: a `catch` or a clear that looked like tidy-up and
// was actually a decision. None of them logged anything a user would see.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

test("a host permission handler that throws is a refusal, not consent", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.on("beforeCellsChanged", () => {
      throw new Error("the host's own rule blew up");
    });
    window.opencalcEditor.selectForTest(4, 4);
  });

  // Reading a crash as "allowed" is how a host's own rule gets bypassed by a
  // bug inside it. The edit must not land.
  const wrote = await page.evaluate(() => window.opencalcEditor.commit("sneaky", false));
  expect(wrote).toBe(false);
  expect(
    await page.evaluate(() => window.opencalcEditor.wasmApi().session_cell_input(0, 4, 4)),
  ).toBe("");
});

test("a host notification handler that throws is reported, not swallowed", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    window.opencalcEditor.on("cellsChanged", () => {
      throw new Error("the host's store rejected it");
    });
    window.opencalcEditor.selectForTest(5, 5);
  });

  // The edit has already happened, so this is not a veto — but the change now
  // exists here and nowhere else, which is exactly the shape of the submission
  // this project once dropped without a word. The console is not where a user
  // is looking.
  const wrote = await page.evaluate(() => window.opencalcEditor.commit("kept", false));
  expect(wrote, "the edit itself still stands").toBe(true);
  await expect(page.locator("#tb-status")).toContainText(/may not have saved/i);
});

test("a half-typed comment survives a trip to another cell", async ({ page }) => {
  await boot(page);
  await page.evaluate(() => {
    const a = window.opencalcEditor.wasmApi();
    a.session_set_cell(0, 0, 0, "figure");
    window.opencalcEditor.selectForTest(0, 0);
    window.opencalcEditor.runCommand("insert.note");
  });
  const box = page.locator("#side-panel-body textarea");
  await expect(box).toBeVisible();

  await box.fill("Checking this against the figure in B4 before I post —");

  // The reason a user leaves: to re-read the number they are about to quote.
  // The draft used to be emptied outright at this point, with no undo and no
  // prompt.
  await page.evaluate(() => window.opencalcEditor.selectForTest(3, 1));
  await expect(box, "the draft does not follow you to another thread").toHaveValue("");

  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  await expect(box, "and it is waiting when you come back").toHaveValue(
    "Checking this against the figure in B4 before I post —",
  );
});
