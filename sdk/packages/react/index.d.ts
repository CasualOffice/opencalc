// Types for `@opencalc/react`.
//
// Hand-written beside `index.js` for the same reason as `@opencalc/sheet`'s:
// the source is hand-written JavaScript, and there is no build step to generate
// declarations from. Checked by `sdk/types/`, which compiles a consumer against
// the public surface — a declaration nothing compiles against drifts (SDK-009).

import type { ComponentPropsWithoutRef, ForwardRefExoticComponent, RefAttributes } from "react";
import type {
  CellsChangedEvent,
  ChromeRegions,
  CommandRules,
  ConfigureOptions,
  OpenCalcSheet as OpenCalcSheetElement,
  SelectionChangedEvent,
  ThemeTokens,
} from "@opencalc/sheet";

/// Presentation, kept apart from `engine` because they are applied by different
/// calls and change at different times.
export interface OpenCalcUi {
  theme?: ThemeTokens;
  chrome?: ChromeRegions;
  commands?: CommandRules;
}

export interface OpenCalcSheetProps
  extends Omit<ComponentPropsWithoutRef<"div">, "onSelect" | "children"> {
  /// Engine-level configuration — access, locale, calculation mode.
  engine?: ConfigureOptions;
  /// Everything visual.
  ui?: OpenCalcUi;
  /// The engine is up and the grid is on screen.
  onReady?: () => void;
  /// Cells changed. Check `source`: `"api"` is this host's own write, and a
  /// component that saves without checking saves its own echo forever.
  onCellsChanged?: (event: CellsChangedEvent) => void;
  onSelectionChanged?: (event: SelectionChangedEvent) => void;
}

/// The spreadsheet, as a React component.
///
/// The ref is the **custom element itself**, so a host that needs to drive it
/// imperatively — `open`, `save`, `listCommands` — has the same API the plain
/// element offers, rather than a second one wrapped around it.
export declare const OpenCalcSheet: ForwardRefExoticComponent<
  OpenCalcSheetProps & RefAttributes<OpenCalcSheetElement>
>;

export default OpenCalcSheet;
