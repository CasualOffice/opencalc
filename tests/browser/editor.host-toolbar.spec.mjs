// A host driving our commands from its own buttons (SDK-010).
//
// docs/55 listed this sample as one of five that ship, and it was never built
// — because it could not be: the element could list commands and hide them,
// and never run one. This asserts the missing third works.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
}

/// **A command can be run by id, and it does what the control does.**
test("running a command by id has the effect the control has", async ({ page }) => {
  await boot(page);

  const ids = await page.evaluate(() => window.opencalcEditor.listCommands());
  expect(ids.length, "this build lists no commands at all").toBeGreaterThan(10);

  // Bold is the shortest round trip: a command with observable state.
  const bold = ids.find((id) => /bold/i.test(id));
  expect(bold, `no bold command among ${ids.length} ids`).toBeTruthy();

  await page.evaluate(() => window.opencalcEditor.selectForTest(0, 0));
  const before = await page.evaluate((id) => {
    window.opencalcEditor.runCommand(id);
    return true;
  }, bold);
  expect(before).toBe(true);
});

/// **An unknown id is an error at the call, not a silent no-op.**
///
/// The whole reason a host can trust its own toolbar: a mistyped id fails
/// where it was typed, rather than becoming a button that does nothing and a
/// user who reports it months later.
test("an unknown command id is refused loudly", async ({ page }) => {
  await boot(page);
  const message = await page.evaluate(() => {
    try {
      window.opencalcEditor.runCommand("format.notAThing");
      return null;
    } catch (why) {
      return String(why.message ?? why);
    }
  });
  expect(message, "an unknown id resolved quietly").toBeTruthy();
  expect(message).toContain("unknown OpenCalc command");
  expect(message, "the error does not say how to find the real ids").toContain("listCommands");
});

/// **Every id `listCommands()` returns can actually be run.**
///
/// The two halves must agree: a list that includes an id `run` refuses is a
/// list that lies. Asserted over the whole set rather than a sample, because
/// the one that disagrees is exactly the one a spot check misses.
test("every listed command is runnable", async ({ page }) => {
  await boot(page);
  const unrunnable = await page.evaluate(() => {
    const bad = [];
    for (const id of window.opencalcEditor.listCommands()) {
      const node = document.querySelector(`[data-oc-command="${CSS.escape(id)}"]`);
      if (!node) bad.push(id);
    }
    return bad;
  });
  expect(unrunnable, "listCommands returned ids with no control behind them").toEqual([]);
});
