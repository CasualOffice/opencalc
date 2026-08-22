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

test("a pasted cell keeps the edges it declared", async ({ page }) => {
  const problems = await boot(page);
  // LibreOffice puts `border-bottom:1px solid #000000` on the header cell and
  // nothing on the others — the case the mapping was deferred over, and the
  // only border any of the three captures actually carries.
  await pasteInto(page, "A1", fixture("libreoffice"));

  const header = await cell(page, 0, 0);
  expect(header.format.bd?.b, "1px solid is a thin line").toBe("thin");
  expect(header.format.bd?.t ?? null, "and it declared no top edge").toBeNull();
  expect(header.format.bd?.l ?? null, "nor a left one").toBeNull();

  const below = await cell(page, 1, 0);
  expect(below.format.bd ?? null, "a cell that declared nothing gets nothing").toBeNull();

  expect(problems, "pasting logged nothing").toEqual([]);
});

test("pasted edges are mapped by weight and style, and never invented", async ({ page }) => {
  const problems = await boot(page);
  // Hand-built rather than captured, because no producer emits all of these —
  // and each one is a distinct branch of the mapping. `border-collapse` is on
  // the table to prove it is not mistaken for an edge.
  const html = `<table style="border-collapse:collapse"><tr>
    <td style="border:2px solid #FF0000">medium</td>
    <td style="border-bottom:3px solid #00FF00">thick</td>
    <td style="border-left:1px dashed #0000FF">dashed</td>
    <td style="border-top:1px double #000000">double</td>
    <td style="border:1px solid #000;border-top:none">none beats the shorthand</td>
    <td style="border:0px solid #000">zero width is no line</td>
  </tr></table>`;
  await pasteInto(page, "A1", html);

  const at = async (c) => (await cell(page, 0, c)).format.bd ?? {};
  expect((await at(0)).t, "2px is medium, and the shorthand sets all four").toBe("medium");
  expect((await at(0)).l).toBe("medium");
  expect((await at(1)).b, "3px is thick").toBe("thick");
  expect((await at(2)).l, "a dashed line keeps its style, not its weight").toBe("dashed");
  expect((await at(3)).t).toBe("double");

  const overridden = await at(4);
  expect(overridden.t ?? null, "an explicit `none` beats the shorthand").toBeNull();
  expect(overridden.b, "while the edges it did not override survive").toBe("thin");

  expect(await at(5), "a zero-width border is not a line").toEqual({});

  expect(problems, "pasting logged nothing").toEqual([]);
});

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
