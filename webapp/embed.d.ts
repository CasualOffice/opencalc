// Types for `@opencalc/sheet`.
//
// Written by hand and kept beside `embed.js`, because `embed.js` is hand-written
// too — there is no build step to generate them from, and a declaration produced
// from JSDoc would describe the comments rather than the code.
//
// Everything here is checked against the implementation by `sdk/types/`, which
// type-checks a consumer using the public surface. A declaration nothing
// compiles against is a second source of truth that drifts (`SDK-009`).

/// A rectangle of cells, inclusive on every side, as every range in this engine
/// is.
export interface CellRange {
  r0: number;
  c0: number;
  r1: number;
  c1: number;
}

/// Where a change came from.
///
/// `"api"` is this host's own write. An editor that saves on `cellsChanged`
/// without checking this saves its own echo, forever.
export type ChangeSource = "user" | "api" | "collab" | "undo";

export interface CellsChangedEvent {
  sheet: number;
  range: CellRange;
  value?: unknown;
  source: ChangeSource;
}

/// Fired *before* the write lands. Returning `false` refuses it.
export type BeforeCellsChangedEvent = CellsChangedEvent;

export interface SelectionChangedEvent {
  sheet: number;
  range: CellRange;
  activeCell: { row: number; col: number };
}

export interface CalculationChangedEvent {
  mode: "automatic" | "manual";
  needsRecalculation: boolean;
}

export interface UndoStateChangedEvent {
  canUndo: boolean;
  canRedo: boolean;
}

/// The events the editor emits, and what each one carries.
export interface OpenCalcEventMap {
  beforeCellsChanged: BeforeCellsChangedEvent;
  cellsChanged: CellsChangedEvent;
  selectionChanged: SelectionChangedEvent;
  calculationChanged: CalculationChangedEvent;
  undoStateChanged: UndoStateChangedEvent;
}

/// What a user may do. Enforced in the engine, not by hiding chrome.
///
/// `"view"` is a *workspace* somebody may not write to: the chrome stays and
/// only the commands that would write are removed. `"preview"` is a
/// *presentation* — a thumbnail or an inline attachment — with no chrome at
/// all. They are different things, and conflating them gives either a viewer
/// that looks like a broken editor or a thumbnail that invites clicks it will
/// refuse.
export type Access = "edit" | "view" | "preview";

/// Regions of chrome that can be shown or hidden by name.
///
/// Named regions rather than CSS selectors: a host reaching into the shadow
/// root would break the moment that markup moved.
export interface ChromeRegions {
  header?: boolean;
  toolbar?: boolean;
  formulaBar?: boolean;
  statusbar?: boolean;
  sheetTabs?: boolean;
}

/// Commands to hide or disable, by id.
///
/// Hidden and disabled differ on purpose: a capability the host has not built
/// yet should be *disabled*, because a user who cannot see a thing assumes it
/// does not exist. One that makes no sense in the host's product should be
/// *hidden*. `listCommands()` returns every id this build has.
export interface CommandRules {
  hidden?: readonly string[];
  disabled?: readonly string[];
}

/// The theme tokens the element accepts. Any subset.
export interface ThemeTokens {
  backgroundColor?: string;
  textColor?: string;
  mutedTextColor?: string;
  faintTextColor?: string;
  iconColor?: string;
  disabledColor?: string;
  borderColor?: string;
  borderHoverColor?: string;
  controlBorderColor?: string;
  surfaceColor?: string;
  popoverBackgroundColor?: string;
  gridlineColor?: string;
  accentColor?: string;
  accentHoverColor?: string;
  accentContrastColor?: string;
  selectionColor?: string;
  findHighlightColor?: string;
  freezeLineColor?: string;
  successColor?: string;
  dangerColor?: string;
  tableHeaderColor?: string;
  tableBandColor?: string;
  /// How far a surface sits above the page: `raised` is attached to it (a
  /// control), `overlay` floats above it (a menu, a dialog, a toast).
  elevationRaised?: string;
  elevationOverlay?: string;
  /// Derived from `accentColor` and `dangerColor`. Set one only to override the
  /// mix — the defaults follow whatever accent you choose.
  accentRing?: string;
  accentSoft?: string;
  dangerRing?: string;
  [token: string]: string | undefined;
}

export interface ConfigureOptions {
  /// Recalculate on every edit, or only when asked.
  calculation?: "automatic" | "manual";
  /// BCP-47, e.g. `"de-DE"`.
  locale?: string;
  /// Message catalogues, keyed by language code.
  messages?: Record<string, Record<string, string>>;
  /// What the user may do. Prefer this to the two booleans below.
  access?: Access;
  /// Sugar for `access: "view"`. One axis underneath, so they cannot disagree.
  readOnly?: boolean;
  /// Sugar for `access: "preview"`.
  preview?: boolean;
}

/// The editor handle `ready` resolves to.
///
/// Deliberately opaque: it is the editor module, and its surface is not part of
/// this package's contract. Reach for the element's own methods instead.
export interface OpenCalcEditor {
  wasmApi(): unknown;
  [key: string]: unknown;
}

/// The `<opencalc-sheet>` custom element.
export declare class OpenCalcSheet extends HTMLElement {
  /// Resolves once the engine is up and the grid is on screen.
  readonly ready: Promise<OpenCalcEditor>;

  /// The access level in force.
  readonly access: Access;

  /// Show or hide chrome by region.
  chrome(regions: ChromeRegions): this;

  /// Hide or disable individual commands by id.
  commands(rules: CommandRules): Promise<this>;

  /// Every command id this build has, so the list can be discovered rather
  /// than read from documentation that may have moved on.
  listCommands(): Promise<string[]>;

  /// Listen for an event. Resolves to an unsubscribe function.
  on<K extends keyof OpenCalcEventMap>(
    name: K,
    handler: (event: OpenCalcEventMap[K]) => void | boolean,
  ): Promise<() => void>;

  /// Stop listening.
  off<K extends keyof OpenCalcEventMap>(
    name: K,
    handler: (event: OpenCalcEventMap[K]) => void | boolean,
  ): Promise<void>;

  /// Override theme tokens.
  theme(tokens: ThemeTokens): this;

  /// Drop every override, back to the built-in theme.
  resetTheme(): this;

  /// Force light or dark, or follow the page.
  setColorScheme(scheme: "light" | "dark" | "auto"): this;

  /// Apply engine-level configuration.
  configure(options?: ConfigureOptions): Promise<this>;

  /// Open an `.xlsx` (or CSV/TSV/PSV) from bytes.
  open(bytes: ArrayBuffer | Uint8Array | ArrayLike<number>, name?: string): Promise<unknown>;

  /// The workbook as `.xlsx` bytes.
  save(): Promise<Uint8Array>;
}

/// The token names the element understands, for a host building a theme picker.
export declare const THEME_TOKENS: readonly string[];

declare global {
  interface HTMLElementTagNameMap {
    "opencalc-sheet": OpenCalcSheet;
  }
}
