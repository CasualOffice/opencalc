// File → Properties, and that what it saves reaches the file.
//
// `DocumentProperties` has been in the model — nine fields — imported from
// `docProps/core.xml` and written back to it, since long before any way existed
// to look at one. A workbook opened here kept its title and author perfectly and
// showed neither; one created here went out with none (`UX-META-01`).
//
// The round-trip assertion is the one that matters. A dialog that edits a field
// nothing persists is worse than no dialog: it tells somebody their document is
// attributed when it is not.

import { expect, test } from "@playwright/test";

async function boot(page) {
  const problems = [];
  page.on("pageerror", (e) => problems.push(e.message));
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

test("properties set in the dialog survive a save and reopen", async ({ page }) => {
  const problems = await boot(page);

  const result = await page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    api.session_set_doc_properties(
      "Q3 Forecast",
      "Finance",
      "Board pack",
      "q3, forecast, board",
      "A. Person",
    );

    // Out to `.xlsx` and back in, which is the only thing that proves the
    // fields reached `docProps/core.xml` rather than a field in memory.
    const bytes = api.session_save();
    api.session_open(bytes);
    return JSON.parse(api.session_doc_properties());
  });

  expect(result.title).toBe("Q3 Forecast");
  expect(result.subject).toBe("Finance");
  expect(result.description).toBe("Board pack");
  expect(result.creator).toBe("A. Person");
  // Keywords are a list in the model and a comma-separated string at this
  // boundary, so a round-trip has to survive being split and rejoined.
  expect(result.keywords).toBe("q3, forecast, board");

  expect(problems, "the round-trip logged nothing").toEqual([]);
});

test("the dialog shows what the document says, editable and not", async ({ page }) => {
  await boot(page);

  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_set_doc_properties(
      "Ledger", "", "", "", "Bookkeeper");
    window.opencalcEditor.documentPropertiesDialog();
  });

  await expect(page.locator("#oc-modal-title")).toHaveText("Document properties");

  const inputs = page.locator("#oc-modal-body .oc-input");
  await expect(inputs).toHaveCount(5);
  await expect(inputs.nth(0)).toHaveValue("Ledger");
  await expect(inputs.nth(4)).toHaveValue("Bookkeeper");

  // The file's own history is shown but not editable: `created`, `modified` and
  // `lastModifiedBy` are what happened, not opinions, and a text box inviting
  // somebody to rewrite them would invite writing a false history into a
  // document.
  const facts = page.locator("#oc-modal-body .oc-props-fact");
  await expect(facts).toHaveCount(4);
  // Empty is stated rather than blank — a gap reads as a bug.
  await expect(facts.nth(0)).toHaveText("—");

  await page.keyboard.press("Escape");
  await expect(page.locator("#oc-modal")).toBeHidden();
});

test("Escape abandons an edit rather than saving it", async ({ page }) => {
  await boot(page);

  const kept = await page.evaluate(async () => {
    const e = window.opencalcEditor, api = e.wasmApi();
    api.session_set_doc_properties("Original", "", "", "", "");
    e.documentPropertiesDialog();
    document.querySelector("#oc-modal-body .oc-input").value = "Typed but abandoned";
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await new Promise((r) => setTimeout(r, 50));
    return JSON.parse(api.session_doc_properties()).title;
  });

  expect(kept, "a cancelled dialog changes nothing").toBe("Original");
});
