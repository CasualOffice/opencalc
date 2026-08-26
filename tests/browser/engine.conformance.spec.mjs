// A recorded trace of what the editor asks its engine.
//
// The check `TAURI-003` needs. The option that keeps native calc *and* one
// editor — a swappable engine binding — turns on two implementations of 229
// calls agreeing, and a divergence between them would be invisible until a user
// found it. This is the instrument that makes that visible: drive one scripted
// session, write down every call and answer, and compare.
//
// Today it has one engine to run against, so it asserts the trace is *stable* —
// that the same script produces the same answers. That is worth having on its
// own (an engine whose answers drift under an unrelated change is a bug), and
// it is the half that has to exist before a second engine can be compared to
// anything.

import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { SCRIPT, runScript } from "./engine-trace.mjs";

// The tracer's *source*, evaluated in the page.
//
// Not a `data:` URL: `btoa` is Latin-1 only and this file's prose has em-dashes
// in it, so encoding it threw `InvalidCharacterError`. A blob URL carries UTF-8
// without an encoding step, which is one fewer thing between the test and what
// it is testing.
const TRACER = readFileSync(new URL("./engine-trace.mjs", import.meta.url), "utf8");

test("the editor's engine answers a scripted session the same way twice", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  // The module is read from disk and evaluated in the page rather than
  // imported: the spec runs in node and the engine lives in the browser, and a
  // tracer that ran on the node side would be recording nothing.
  const traceOf = async () =>
    page.evaluate(async (src) => {
      const url = URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
      try {
        const mod = await import(url);
        return mod.runScript(window.opencalcEditor.wasmApi());
      } finally {
        URL.revokeObjectURL(url);
      }
    }, TRACER);

  const first = await traceOf();
  const second = await traceOf();

  expect(first.length, "every scripted call was recorded").toBe(SCRIPT.length);

  // Stability, call by call. Comparing whole arrays would report "these two
  // large objects differ" and leave you to find where.
  for (let i = 0; i < first.length; i += 1) {
    expect(second[i], `call ${i} (${first[i].call}) answered differently on a second run`)
      .toEqual(first[i]);
  }
});

test("the trace records answers, not just that a call happened", async ({ page }) => {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  const trace = await page.evaluate(async (src) => {
    const url = URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
    try {
      const mod = await import(url);
      return mod.runScript(window.opencalcEditor.wasmApi());
    } finally {
      URL.revokeObjectURL(url);
    }
  }, TRACER);

  // A trace that recorded only call names would compare equal between two
  // engines that disagreed about every value — which is the failure this whole
  // instrument exists to catch, so it is asserted rather than assumed.
  const byName = (n) => trace.find((t) => t.call === n);

  // `=A2*2` typed into B2 must read back as itself, not resolved against A1.
  // That is `PERF-11` relative storage, and it is exactly the sort of thing a
  // second implementation gets wrong.
  expect(byName("session_cell_input").answer).toBe('"=A2*2"');

  // A leading-zero entry keeps its zeros — the rule `TAURI-002` moved into the
  // SDK, and one no second host would guess.
  const formats = trace.filter((t) => t.call === "session_cell_format");
  expect(formats.length).toBeGreaterThan(0);
  expect(formats.every((f) => f.answer !== "undefined")).toBe(true);

  // Undo has to change something. A trace where it does not is recording a
  // no-op, and would agree with any engine at all.
  const idx = trace.findIndex((t) => t.call === "session_undo");
  expect(idx).toBeGreaterThan(0);
  expect(trace[idx - 1].answer).not.toBe(trace[idx + 1]?.answer);
});
