// Pasting from another spreadsheet application.
//
// The defect these exist for was not a mapping gap: paste read only
// `navigator.clipboard.readText()`, so formatting was discarded by
// construction. See docs/68-CLIPBOARD-HTML-PASTE.md.
//
// Fixtures are real `text/html` captures, committed as inert files. They are
// delivered through a synthetic `paste` event carrying a `text/html` flavour,
// which is what a browser hands the page when somebody presses Ctrl+V — the
// automation cannot put a styled payload on the real OS clipboard.

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";

const fixture = (name) =>
  readFileSync(new URL(`../../fixtures/clipboard/${name}.html`, import.meta.url), "utf8");

async function boot(page) {
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  return problems;
}

/// Paste `html` at `ref`, the way the browser delivers a real Ctrl+V.
async function pasteInto(page, ref, html, text = "") {
  await page.fill("#cell-ref", ref);
  await page.press("#cell-ref", "Enter");
  await page.locator("#grid").focus();
  await page.evaluate(
    ([html, text]) => {
      const data = new DataTransfer();
      data.setData("text/html", html);
      if (text) data.setData("text/plain", text);
      document.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true }));
    },
    [html, text],
  );
  // The handler is async (it may consult the clipboard), so the assertion has
  // to wait for the grid rather than for the event.
  await page.waitForTimeout(300);
}

/// What the engine holds for a cell, values and formatting together.
const cell = (page, row, col) =>
  page.evaluate(
    ([row, col]) => {
      const wasm = window.opencalcEditor.wasmApi();
      return {
        text: wasm.session_cell_input(0, row, col),
        format: JSON.parse(wasm.session_cell_format(0, row, col)),
      };
    },
    [row, col],
  );

for (const producer of ["excel", "libreoffice", "sheets"]) {
  test(`a table copied from ${producer} keeps its values and formatting`, async ({ page }) => {
    const problems = await boot(page);
    await pasteInto(page, "A1", fixture(producer));

    // Values first: without these the styles below would be decorating the
    // wrong cells, and every producer lays this fixture out the same way.
    expect((await cell(page, 0, 0)).text, "the header").toBe("Item");
    expect((await cell(page, 0, 1)).text).toBe("Qty");
    expect((await cell(page, 1, 0)).text).toBe("Widget");
    // `3.5`, not `3.50`: the engine stores a number, and the trailing zero is
    // the *format's* job. Asserting the literal text here would be asserting
    // that a paste turns numbers into strings.
    expect((await cell(page, 1, 1)).text).toBe("3.5");
    expect((await cell(page, 2, 0)).text).toBe("Total row");

    const header = await cell(page, 0, 0);
    // The format JSON uses `1`, not `true`.
    expect(header.format.b, "the header is bold in every one of them").toBe(1);
    expect(header.format.al, "and centred").toBe("center");

    const widget = await cell(page, 1, 0);
    expect(widget.format.i, "Widget is italic").toBe(1);

    // The number format, where the producer carries one. Excel writes
    // `mso-number-format` and LibreOffice writes `sdnum`; Google Sheets emits
    // neither, so there is nothing to recover and the cell keeps the general
    // format. Stated per producer rather than skipped, so the difference is
    // recorded rather than discovered.
    const qty = await cell(page, 1, 1);
    if (producer === "sheets") {
      expect(qty.format.nf ?? null, "Sheets carries no number format").toBeNull();
    } else {
      expect(qty.format.nf, `${producer} carries the number format`).toBe("0.00");
    }

    // A colspan is a merge, which is structure rather than decoration: getting
    // it wrong shifts everything after it.
    const merges = await page.evaluate(() => JSON.parse(window.opencalcEditor.wasmApi().session_merges(0)));
    expect(merges, "the two-column total row is one merged cell").toContainEqual(
      expect.objectContaining({ r0: 2, c0: 0, r1: 2, c1: 1 }),
    );

    expect(problems, "pasting logged nothing").toEqual([]);
  });
}

test("hostile clipboard markup pastes its text and does nothing else", async ({ page }) => {
  const problems = await boot(page);
  const requests = [];
  page.on("request", (r) => requests.push(r.url()));

  await pasteInto(page, "A1", fixture("hostile"));

  // The text lands, because a paste is still a paste.
  expect((await cell(page, 0, 0)).text).toBe("hover me");
  expect((await cell(page, 1, 0)).text).toBe("image");

  // And nothing else happened. `DOMParser` gives a document with no browsing
  // context: scripts do not run and subresources are never fetched. Asserted
  // rather than assumed, because this is the one property that makes parsing
  // untrusted markup acceptable at all.
  expect(await page.evaluate(() => window.__pwned ?? null), "no script ran").toBeNull();
  expect(
    requests.filter((u) => u.includes("127.0.0.1:9")),
    "nothing in the markup was fetched",
  ).toEqual([]);
  expect(problems).toEqual([]);
});

test("a clipboard with no HTML still pastes as text", async ({ page }) => {
  await boot(page);
  await pasteInto(page, "A1", "", "one\ttwo\nthree\tfour");
  expect((await cell(page, 0, 0)).text).toBe("one");
  expect((await cell(page, 1, 1)).text).toBe("four");
});
