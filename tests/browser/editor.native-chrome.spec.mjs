// A desktop app should behave like a desktop app.
//
// The seam that makes that possible without a second menu definition. The
// editor already funnels every menu item and toolbar button through one command
// id (`data-oc-command`, `listCommands()`, `runCommand(id)`), so a native menu
// does not need its own copy of the File/Edit/View tree — it needs the same
// tree in a form a native menu builder can read, and a way to turn the HTML bar
// off when the operating system is drawing one instead.
//
// The model is derived from the **live DOM** rather than from the `MENUS`
// literal it is built from. That is deliberate: the DOM is what `runCommand`
// dispatches against, and `applyCommandRules()` hides items in read-only mode,
// so a DOM-derived model cannot drift from what is actually clickable. A model
// read from the literal would describe a menu the app might not have.

import { expect, test } from "@playwright/test";

async function boot(page, query = "") {
  await page.goto(`/editor.html${query}`);
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

const model = (page) => page.evaluate(() => window.opencalcEditor.menuModel());

test("the menu model describes the whole menu bar", async ({ page }) => {
  await boot(page);
  const menus = await model(page);

  expect(menus.length, "eight top-level menus").toBeGreaterThanOrEqual(8);
  const names = menus.map((m) => m.label);
  expect(names).toContain("File");
  expect(names).toContain("Help");

  const file = menus.find((m) => m.label === "File");
  expect(file.id, "the id a native menu dispatches with").toBe("file");
  expect(file.items.length, "File has items").toBeGreaterThan(3);

  // Separators are carried, because a native menu without them is a wall.
  const anySep = menus.some((m) => m.items.some((i) => i.kind === "separator"));
  expect(anySep, "separators survive into the model").toBe(true);

  // Submenus stay nested rather than being flattened.
  const anySub = menus.some((m) => m.items.some((i) => i.kind === "submenu" && i.items.length));
  expect(anySub, "submenus stay nested").toBe(true);
});

test("every command in the model can be dispatched", async ({ page }) => {
  await boot(page);
  const menus = await model(page);
  const ids = await page.evaluate(() => window.opencalcEditor.listCommands());

  const leaves = [];
  const walk = (items) => {
    for (const i of items) {
      if (i.kind === "submenu") walk(i.items);
      else if (i.kind === "item") leaves.push(i);
    }
  };
  for (const m of menus) walk(m.items);

  expect(leaves.length, "the model has leaves").toBeGreaterThan(30);
  // The whole point: a native menu holds these ids and calls runCommand with
  // them. Any id the model invents is a menu entry that does nothing.
  const unknown = leaves.map((l) => l.id).filter((id) => !ids.includes(id));
  expect(unknown, "no id in the model is unknown to runCommand").toEqual([]);
});

test("chrome=native hides the HTML bar and gives the height to the grid", async ({ page }) => {
  await boot(page);
  const webGrid = await page.locator("#grid").boundingBox();
  const barHeight = await page.evaluate(() => document.getElementById("menubar").getBoundingClientRect().height);
  expect(barHeight, "there is a bar to reclaim").toBeGreaterThan(20);

  await boot(page, "?chrome=native");
  await expect(page.locator("#menubar")).toBeHidden();
  const nativeGrid = await page.locator("#grid").boundingBox();

  // Not merely hidden — the space it occupied goes to the sheet, which is the
  // only reason a desktop app hiding its own menu bar is an improvement.
  expect(nativeGrid.height, "the grid grows by roughly the bar").toBeGreaterThan(webGrid.height + barHeight - 4);
});

test("commands still run when the operating system owns the menu", async ({ page }) => {
  await boot(page, "?chrome=native");
  await expect(page.locator("#menubar")).toBeHidden();

  // The native menu dispatches by id into a bar the user cannot see. If hiding
  // the bar detached or disabled its nodes, every native menu entry would throw
  // and the desktop app would have a menu that does nothing.
  const ran = await page.evaluate(() => window.opencalcEditor.runCommand("file.properties"));
  expect(ran).toBe(true);
  await expect(page.locator(".oc-modal:not([hidden])")).toBeVisible();
});
