// A consumer of the published surface, compiled under `strict`.
//
// This is the gate for SDK-009. The declarations are hand-written beside
// hand-written JavaScript, so nothing forces them to describe the code — except
// somebody compiling against them. That is this file.
//
// It is deliberately written the way a host writes it, not the way a test does:
// every call here is one the README or docs page tells an integrator to make.

import type {
  Access,
  CellsChangedEvent,
  ChromeRegions,
  CommandRules,
  ConfigureOptions,
  SelectionChangedEvent,
  ThemeTokens,
} from "@opencalc/sheet";
import { OpenCalcSheet, THEME_TOKENS } from "@opencalc/sheet";

/// The element is reachable through the DOM by its tag, without a cast.
///
/// This is what `HTMLElementTagNameMap` buys, and the thing a host notices
/// first: `document.querySelector("opencalc-sheet")` returning `Element` means
/// every call below needs a cast.
function mounted(): OpenCalcSheet {
  const found = document.querySelector("opencalc-sheet");
  if (!found) throw new Error("no sheet");
  // No cast: the tag map gives this the element's own type, which is the
  // difference between a typed host and one writing `as any` at every call.
  return found;
}

export async function open(bytes: ArrayBuffer): Promise<Uint8Array> {
  const sheet = mounted();
  await sheet.ready;

  const options: ConfigureOptions = {
    calculation: "manual",
    locale: "de-DE",
    access: "edit",
  };
  await sheet.configure(options);

  const chrome: ChromeRegions = { toolbar: false, statusbar: false };
  sheet.chrome(chrome);

  const rules: CommandRules = { hidden: ["file.open"], disabled: ["insert.chart"] };
  await sheet.commands(rules);

  const tokens: ThemeTokens = { accentColor: "#1f6f4a", backgroundColor: "#fff" };
  sheet.theme(tokens).setColorScheme("dark");

  const ids: string[] = await sheet.listCommands();
  if (ids.length === 0) sheet.resetTheme();

  const level: Access = sheet.access;
  if (level !== "edit") throw new Error("read-only");

  await sheet.open(bytes, "quarter.xlsx");
  return sheet.save();
}

/// Events are typed per name: the handler's parameter is inferred, and asking
/// for a field the event does not carry is a compile error rather than
/// `undefined` at runtime.
export async function watch(): Promise<() => void> {
  const sheet = mounted();
  const stop = await sheet.on("cellsChanged", (event: CellsChangedEvent) => {
    // The check the docs tell every integrator to make.
    if (event.source === "api") return;
    void event.range.r0;
    void event.sheet;
  });

  await sheet.on("selectionChanged", (event: SelectionChangedEvent) => {
    void event.activeCell.row;
  });

  // Inferred, without the annotation.
  await sheet.on("undoStateChanged", (event) => {
    const enabled: boolean = event.canUndo;
    void enabled;
  });

  // `beforeCellsChanged` may refuse the write by returning false.
  await sheet.on("beforeCellsChanged", (event) => event.source !== "user");

  return stop;
}

/// The class and the token list are both exported values, not only types.
export function names(): readonly string[] {
  void OpenCalcSheet;
  return THEME_TOKENS;
}
