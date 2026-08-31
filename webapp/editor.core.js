// OpenCalc canvas grid editor. The WASM engine owns the workbook, computes the
// layout + display text, and recalculates; this file draws the grid and text on
// a canvas and routes edits back to the engine.
// The glue + wasm binary are loaded in main() with a build tag on the URL so a
// rebuilt engine is never shadowed by a stale browser cache.
//
// The tag is this file's own cache-buster, which the dev server stamps from the
// file's mtime. It was a hand-pinned constant, which meant a rebuilt engine
// kept the same module URL and Chrome went on serving the previous glue from
// its module map — `no-store` does not evict an ES module that is already
// resolved. The same mistake cost an afternoon of testing a fix that had never
// reached the browser; deriving the tag rather than remembering to bump it is
// what stops it recurring.
// Supplied by `editor.boot.js`, which is the file the page names and so the
// only one whose URL carries the tag. Falling back to this module's own
// query keeps it working if it is ever loaded directly.
// Exported because a draft records which build wrote it (`editor.drafts.js`):
// a recovery bar that cannot say what produced an entry cannot refuse one from
// a version it was not written to read.
export const BUILD =
  globalThis.__opencalcBuild ||
  new URL(import.meta.url).searchParams.get("v") ||
  "dev";
// The topic modules `MNT-005` split this file into.
//
// This file keeps the shared mutable state and the 74 functions that write it;
// everything that only *reads* moved out, so no function body had to change.
// The imports below are what this file still calls; the re-exports keep the
// module's public surface — `window.opencalcEditor` is this namespace — exactly
// what it was.
import { paintList } from "./editor.paintlist.js";
import {
  applyChart,
  chartAt,
  chartDragRect,
  chartHandlePoints,
  currentChart,
  refreshChartPanel,
} from "./editor.charts.js";
import {
  clipboardHtml,
  decodeTextBytes,
  doCopy,
  doCut,
  doPaste,
  doPasteMode,
  download,
  htmlText,
} from "./editor.clipboard.js";
import {
  buildCfPanel,
  documentPropertiesDialog,
  buildColorMenu,
  buildDvPanel,
  buildPagePanel,
  buildTablePanel,
  cellMenu,
  cellStyleGallery,
  closeSheetMenu,
  conditionDialog,
  confirmModal,
  customFormatDialog,
  deleteSheetWithConfirm,
  formatCellsDialog,
  headerMenu,
  hyperlinkDialog,
  manageCfRules,
  openColumnFilter,
  openColumnFilterForTest,
  openNameBoxList,
  openNameManager,
  openValidationMenu,
  panelActions,
  panelLabel,
  panelRangeReadout,
  pasteSpecialDialog,
  positionMenu,
  refreshTablePanel,
  refreshValidationPrompt,
  reportImportIssues,
  sheetMenu,
  sizeDialog,
  sortDialog,
  tableDialog,
  textToColumnsDialog,
  togglePanel,
} from "./editor.dialogs.js";
import {
  closeFind,
  colFromLetters,
  colFromName,
  colName,
  findStep,
  gotoMatch,
  gotoName,
  navTarget,
  openFind,
  openReplace,
  parseA1Cell,
  parseNameRange,
  replaceAll,
  replaceOne,
  runFind,
} from "./editor.find.js";
import {
  adjustDecimals,
  applyPainter,
  applyTableStyle,
  armPainter,
  bdIcon,
  clearFormats,
  formatSel,
  hideFillOptions,
  mergeInSel,
  mergeVariant,
  setAlign,
  setBorder,
  setFill,
  setFontColor,
  setFontName,
  setFontSize,
  setIndent,
  setNumberFormat,
  setRotation,
  setTextOverflow,
  setValign,
  setVertAlign,
  showFillOptions,
  stepFontSize,
  toggleBold,
  toggleBorder,
  toggleItalic,
  toggleMerge,
  toggleStrike,
  toggleUnderline,
  toggleWrap,
  updateFill,
} from "./editor.formatting.js";
import {
  anchorPoint,
  anchoredRect,
  boundaryAt,
  cellAt,
  cellPx,
  clampScroll,
  colAtX,
  colWidthOf,
  edgeVelocity,
  effectiveRange,
  ensureVisible,
  filterButtonAt,
  freezeHandleAt,
  freezeHit,
  fscreenX,
  fscreenY,
  fzOffset,
  hiddenMarkAt,
  mergeAt,
  outlineToggleAt,
  relayout,
  resize,
  rowAtPx,
  rowAtY,
  rowOffsetPx,
  screenX,
  screenY,
  selRect,
  sheetNameAt,
  snapLeading,
  spanX,
  spanY,
  unhideMark,
  updateGridCounts,
  updateResize,
  usedBounds,
  valueExtent,
} from "./editor.geometry.js";
import {
  availableLocales,
  errText,
  fmtNum,
  friendlyFormulaError,
  friendlyOpenError,
  getLocale,
  hideSignatureTip,
  hideTip,
  relabel,
  relabelMenubar,
  setLocalePicker,
  setMessages,
  setTip,
  showTip,
  statusError,
  syncLocalePicker,
  t,
  tipify,
} from "./editor.i18n.js";
import {
  applyAccent,
  applyTheme,
  borderWidth,
  cellFg,
  cellFont,
  cellLineH,
  contrastInk,
  currentTheme,
  drawChartSelection,
  drawCollaboratorDraft,
  drawCollaborators,
  drawEdge,
  drawFilterRegion,
  FILTER_ARROW_W,
  drawFreezeDividers,
  drawMoveDropIndicator,
  drawMoveGhost,
  drawTableOutlines,
  moveGhostForTest,
  drawFreezeHandles,
  drawImages,
  drawRuns,
  drawStretched,
  drawTraceArrows,
  fontStack,
  growthBefore,
  imageFor,
  measureRowHeight,
  neededRowHeight,
  paintRefTokens,
  quadClip,
  refreshTheme,
  registerSuppliedFonts,
  runFont,
  runsWidth,
  textWidth,
  textWidthStatsForTest,
  resetTextWidthStatsForTest,
  textY,
  tintColor,
  wrapLines,
} from "./editor.paint.js";
import {
  applyPivot,
  currentPivot,
  pivotAdd,
  pivotAt,
  pivotBlocks,
  pivotChip,
  pivotDrop,
  pivotDropIndex,
  pivotFieldIsNumeric,
  pivotInsert,
  pivotItemPicker,
  pivotPlacement,
  refreshAllPivots,
  refreshPivotHere,
  refreshPivotPanel,
} from "./editor.pivot.js";
import {
  adoptCollabDocument,
  clearKeepWaiting,
  collabDraft,
  collaborators,
  jumpToParticipant,
  mirrorEdit,
  mirrorFor,
  offerKeepWaiting,
  participantChannels,
  participantColor,
  participantFace,
  participantInitials,
  participantInk,
  participantName,
  presenceCell,
  presenceItems,
  presenceRow,
  presenceWhere,
  relativeTime,
  renderPresence,
  setShareDefaults,
  shareDialog,
  syncMirrorBox,
  wasStopped,
  wirePresence,
} from "./editor.presence.js";
import {
  acceptAutocomplete,
  addColumnRange,
  addRange,
  addRowRange,
  afterFilterChange,
  allRanges,
  applyCommandRules,
  applySort,
  autoSum,
  autofillToNeighbour,
  autofitColumn,
  autofitColumnForTest,
  autofitRow,
  autofitRowForTest,
  callAtCaret,
  cancelEdit,
  clearSelection,
  commandId,
  commit,
  commitToSelection,
  ctrlA,
  currentFnToken,
  currentTable,
  cycleAnchors,
  deleteLines,
  doRedo,
  doUndo,
  el,
  emit,
  emitStateEvents,
  endInline,
  fillWithin,
  followHyperlink,
  gridHandlesForTest,
  inStringLiteral,
  insertLines,
  insertRef,
  listCommands,
  looksLikeHeader,
  off,
  on,
  parseColor,
  personalFilterForTest,
  personalViewForTest,
  positionInline,
  refAcceptable,
  refreshFormulaBar,
  removeDuplicates,
  renderAutocomplete,
  resetToOrigin,
  runCommand,
  menuModel,
  setNativeChrome,
  scrollStateForTest,
  seed,
  selectAll,
  selectColsSpan,
  selectColumn,
  selectForTest,
  selectRow,
  selectRowsSpan,
  cellBoxForTest,
  selectionRectForTest,
  shiftIsRisky,
  sortRange,
  sortTarget,
  startInline,
  stepFrom,
  stepWithin,
  toggleFilter,
  tryEdit,
  updateCellMode,
  updateNameBox,
  updateStats,
  wasmApi,
} from "./editor.selection.js";
import {
  applyPersonalFilter,
  calcMode,
  clearAll,
  clearMyView,
  clearMyViewForTest,
  commentAuthor,
  commentStamp,
  commitFreezeDrag,
  doSave,
  isDirty,
  markSaved,
  savedAtEditsForDraft,
  doSaveDelimited,
  doSaveNative,
  moveTab,
  needsRecalc,
  openBytes,
  documentName,
  setDocumentName,
  printSheet,
  readOnly,
  readOutline,
  readThread,
  recalculateNow,
  renameSheet,
  resetView,
  newDocument,
  downloadItems,
  saveAs,
  saveSheetView,
  setCalculationMode,
  setCellProtection,
  setCommentAuthor,
  setEndMode,
  setFreeze,
  setReadOnly,
  setViewOption,
  setZoom,
  setZoomForTest,
  sheetProtectedNow,
  switchSheet,
  toggleSheetProtected,
  viewOptions,
  wireZoom,
} from "./editor.sheets.js";

// Browser drafts and crash recovery (`SAVE-03`). Imported for `initDrafts()`,
// which `main()` calls; the rest is re-exported because `window.opencalcEditor`
// **is** this namespace, so a host or a test that cannot see the draft state
// cannot check that autosave is running — and an autosave nobody can observe is
// how one stops without anybody noticing.
import { initDrafts } from "./editor.drafts.js";
import { forgetVersions, loadVersions, persistVersions } from "./editor.versions.js";

/// Re-read this document's versions from storage (`HIST-03`).
///
/// Boot does this once, and a test that changes documents in one tab needs it
/// again — as would a host that swaps the open document without a reload, which
/// is what `setDocumentName` amounts to. Exported rather than inlined into the
/// test so the reload path a user gets and the one a test exercises are the
/// same function.
export function reloadVersionsForTest() {
  return loadVersions(wasm);
}
export {
  autosaveFault,
  breakDraftStoreForTest,
  draftPolicy,
  draftSlotForTest,
  draftStateForTest,
  initDrafts,
  lastDraftReason,
  listDrafts,
  refreshRecoveryBar,
  restartDraftSchedulerForTest,
  setDraftPolicyForTest,
} from "./editor.drafts.js";

export {
  applyChart,
  chartAt,
  chartDragRect,
  chartHandlePoints,
  currentChart,
  refreshChartPanel,
} from "./editor.charts.js";
export {
  clipboardHtml,
  decodeTextBytes,
  doCopy,
  doCut,
  doPaste,
  doPasteMode,
  download,
  htmlText,
} from "./editor.clipboard.js";
export {
  buildCfPanel,
  documentPropertiesDialog,
  buildColorMenu,
  buildDvPanel,
  buildPagePanel,
  buildTablePanel,
  cellMenu,
  cellStyleGallery,
  closeSheetMenu,
  conditionDialog,
  confirmModal,
  customFormatDialog,
  deleteSheetWithConfirm,
  formatCellsDialog,
  headerMenu,
  hyperlinkDialog,
  manageCfRules,
  openColumnFilter,
  openColumnFilterForTest,
  openNameBoxList,
  openNameManager,
  openValidationMenu,
  panelActions,
  panelLabel,
  panelRangeReadout,
  pasteSpecialDialog,
  positionMenu,
  refreshTablePanel,
  refreshValidationPrompt,
  reportImportIssues,
  sheetMenu,
  sizeDialog,
  sortDialog,
  tableDialog,
  textToColumnsDialog,
  togglePanel,
} from "./editor.dialogs.js";
export {
  closeFind,
  colFromLetters,
  colFromName,
  colName,
  findStep,
  gotoMatch,
  gotoName,
  navTarget,
  openFind,
  openReplace,
  parseA1Cell,
  parseNameRange,
  replaceAll,
  replaceOne,
  runFind,
} from "./editor.find.js";
export {
  adjustDecimals,
  applyPainter,
  applyTableStyle,
  armPainter,
  bdIcon,
  clearFormats,
  formatSel,
  hideFillOptions,
  mergeInSel,
  mergeVariant,
  setAlign,
  setBorder,
  setFill,
  setFontColor,
  setFontName,
  setFontSize,
  setIndent,
  setNumberFormat,
  setRotation,
  setTextOverflow,
  setValign,
  setVertAlign,
  showFillOptions,
  stepFontSize,
  toggleBold,
  toggleBorder,
  toggleItalic,
  toggleMerge,
  toggleStrike,
  toggleUnderline,
  toggleWrap,
  updateFill,
} from "./editor.formatting.js";
export {
  anchorPoint,
  anchoredRect,
  boundaryAt,
  cellAt,
  cellPx,
  clampScroll,
  colAtX,
  colWidthOf,
  edgeVelocity,
  effectiveRange,
  ensureVisible,
  filterButtonAt,
  freezeHandleAt,
  freezeHit,
  fscreenX,
  fscreenY,
  fzOffset,
  hiddenMarkAt,
  mergeAt,
  outlineToggleAt,
  relayout,
  resize,
  rowAtPx,
  rowAtY,
  rowOffsetPx,
  screenX,
  screenY,
  selRect,
  sheetNameAt,
  snapLeading,
  spanX,
  spanY,
  unhideMark,
  updateGridCounts,
  updateResize,
  usedBounds,
  valueExtent,
} from "./editor.geometry.js";
export {
  availableLocales,
  errText,
  fmtNum,
  friendlyFormulaError,
  friendlyOpenError,
  getLocale,
  hideSignatureTip,
  hideTip,
  relabel,
  relabelMenubar,
  setLocalePicker,
  setMessages,
  setTip,
  showTip,
  statusError,
  syncLocalePicker,
  t,
  tipify,
} from "./editor.i18n.js";
export {
  applyAccent,
  applyTheme,
  borderWidth,
  cellFg,
  cellFont,
  cellLineH,
  contrastInk,
  currentTheme,
  drawChartSelection,
  drawCollaboratorDraft,
  drawCollaborators,
  drawEdge,
  drawFilterRegion,
  drawFreezeDividers,
  drawFreezeHandles,
  drawImages,
  moveGhostForTest,
  drawRuns,
  drawStretched,
  drawTraceArrows,
  fontStack,
  growthBefore,
  imageFor,
  measureRowHeight,
  neededRowHeight,
  paintRefTokens,
  quadClip,
  refreshTheme,
  registerSuppliedFonts,
  runFont,
  runsWidth,
  textWidth,
  textWidthStatsForTest,
  resetTextWidthStatsForTest,
  textY,
  tintColor,
  wrapLines,
} from "./editor.paint.js";
export {
  applyPivot,
  currentPivot,
  pivotAdd,
  pivotAt,
  pivotBlocks,
  pivotChip,
  pivotDrop,
  pivotDropIndex,
  pivotFieldIsNumeric,
  pivotInsert,
  pivotItemPicker,
  pivotPlacement,
  refreshAllPivots,
  refreshPivotHere,
  refreshPivotPanel,
} from "./editor.pivot.js";
export {
  adoptCollabDocument,
  clearKeepWaiting,
  collabDraft,
  collaborators,
  jumpToParticipant,
  mirrorEdit,
  mirrorFor,
  offerKeepWaiting,
  participantChannels,
  participantColor,
  participantFace,
  participantInitials,
  participantInk,
  participantName,
  presenceCell,
  presenceItems,
  presenceRow,
  presenceWhere,
  relativeTime,
  renderPresence,
  setShareDefaults,
  shareDialog,
  syncMirrorBox,
  wasStopped,
  wirePresence,
} from "./editor.presence.js";
export {
  acceptAutocomplete,
  addColumnRange,
  addRange,
  addRowRange,
  afterFilterChange,
  allRanges,
  applyCommandRules,
  applySort,
  autoSum,
  autofillToNeighbour,
  autofitColumn,
  autofitColumnForTest,
  autofitRow,
  autofitRowForTest,
  callAtCaret,
  cancelEdit,
  clearSelection,
  commandId,
  commit,
  commitToSelection,
  ctrlA,
  currentFnToken,
  currentTable,
  cycleAnchors,
  deleteLines,
  doRedo,
  doUndo,
  el,
  emit,
  emitStateEvents,
  endInline,
  extendSelectionForTest,
  fillWithin,
  followHyperlink,
  gridHandlesForTest,
  inStringLiteral,
  insertLines,
  insertRef,
  listCommands,
  looksLikeHeader,
  off,
  on,
  parseColor,
  personalFilterForTest,
  personalViewForTest,
  positionInline,
  refAcceptable,
  refreshFormulaBar,
  removeDuplicates,
  renderAutocomplete,
  resetToOrigin,
  runCommand,
  menuModel,
  setNativeChrome,
  scrollStateForTest,
  seed,
  selectAll,
  selectColsSpan,
  selectColumn,
  selectForTest,
  selectRow,
  selectRowsSpan,
  cellBoxForTest,
  selectionRectForTest,
  shiftIsRisky,
  sortRange,
  sortTarget,
  startInline,
  stepFrom,
  stepWithin,
  toggleFilter,
  tryEdit,
  updateCellMode,
  updateNameBox,
  updateStats,
  wasmApi,
} from "./editor.selection.js";
export {
  applyPersonalFilter,
  calcMode,
  clearAll,
  clearMyView,
  clearMyViewForTest,
  commentAuthor,
  commentStamp,
  commitFreezeDrag,
  doSave,
  isDirty,
  markSaved,
  savedAtEditsForDraft,
  doSaveDelimited,
  doSaveNative,
  moveTab,
  needsRecalc,
  openBytes,
  documentName,
  setDocumentName,
  printSheet,
  readOnly,
  readOutline,
  readThread,
  recalculateNow,
  renameSheet,
  resetView,
  newDocument,
  downloadItems,
  saveAs,
  saveSheetView,
  setCalculationMode,
  setCellProtection,
  setCommentAuthor,
  setEndMode,
  setFreeze,
  setReadOnly,
  setViewOption,
  setZoom,
  setZoomForTest,
  sheetProtectedNow,
  switchSheet,
  toggleSheetProtected,
  viewOptions,
  wireZoom,
} from "./editor.sheets.js";

export let init, wasm;

// --- White-labelling --------------------------------------------------------
//
// An integrator reselling a spreadsheet editor cannot ship one with somebody
// else's name in the toolbar, which is why every product in this market has it.
// Both of these come off the page's own URL so an embedding host sets them
// without a build: the WOPI adapter appends them when it points its iframe here
// (docs/74), and an SDK embedder can do the same.
//
// Bounded and escaped at the edge. `BRAND` reaches innerHTML in the About
// dialog, and `ACCENT` reaches a CSS custom property — a colour that is allowed
// to be arbitrary text can close the declaration and start another.
const PARAMS = new URL(location.href).searchParams;
const BRAND = (PARAMS.get("brand") || "OpenCalc").slice(0, 60);
const ACCENT = /^#[0-9a-f]{3,8}$|^[a-z]{3,20}$/i.test(PARAMS.get("accent") || "")
  ? PARAMS.get("accent")
  : null;

/// Chrome regions to hide, from `?hide=header,statusbar`.
///
/// The regions and the `oc-hide-*` classes are **the same ones `embed.js`
/// uses** — not a second vocabulary. `<opencalc-sheet>` already hides the
/// header by default, on the stated grounds that "an embedded editor is the
/// host's product, not ours". A host that iframes `editor.html` directly had
/// no way to say the same thing, so `casual-calc-host` rendered its own header
/// *and* got this one: two headers stacked, which is what a user running the
/// Docker stack sees. The comment in that page even claims it "can keep its
/// own chrome without forking the editor's HTML" — the intent was right and
/// nothing implemented it.
///
/// Filtered against the known list rather than trusted: this arrives on a URL,
/// so anybody who can hand somebody a link chooses it, and an unfiltered value
/// would be an arbitrary class on the document root.
const CHROME_REGIONS = [
  "header", "menubar", "toolbar", "formulabar", "tabs", "statusbar", "localepicker",
];
const HIDDEN_CHROME = (PARAMS.get("hide") || "")
  .split(",")
  .map((name) => name.trim().toLowerCase())
  .filter((name) => CHROME_REGIONS.includes(name));

/// Text that is about to become markup.

// **The brand has no wordmark in the chrome to reach any more** (`UX-CHR-03`).
//
// It used to be written into `.tb-brand`, the product name in the branding
// strip. That strip names the *document* now, because none of the five
// applications this editor is measured against names its product inside the
// document window — and an integrator reselling the editor wants *no* product
// name in the chrome even more than they want their own. What is left carrying
// the brand is the tab's own title and `Help ▸ About`, which is where five of
// five put it, and `editor.branding` asserts both.
if (BRAND !== "OpenCalc") document.title = BRAND;
if (ACCENT) document.documentElement.style.setProperty("--oc-accent-color", ACCENT);

// Applied to the root, because `.oc-hide-header` and its siblings are written
// as descendant selectors and the header is a child of `<body>` here where in
// the embed element it is a child of the shell.
for (const region of HIDDEN_CHROME) {
  document.documentElement.classList.add(`oc-hide-${region}`);
}

// --- Modes, as capabilities --------------------------------------------------
//
// There used to be one chrome and one flag. The same editor booted three ways
// — page, desktop window, someone else's iframe — presented an identical 195
// commands, `File ▸ Open` and six Download entries in every one of them, and
// the only thing gating anything anywhere was `readOnly()`. `?embed=1` was
// read by nothing at all.
//
// That is not a cosmetic gap. An **embedded** editor sits inside a page whose
// document belongs to the *host*: `File ▸ Open` there replaces the host's
// document from inside the host's own UI, and Download hands out a copy the
// host never authorised. Under WOPI the host additionally owns save and
// versioning, so the editor's own Save is not a convenience — it is a second
// writer disagreeing with the first.
//
// So a mode is **a set of capabilities**, not a name with an `if` for each
// place that cares. A capability is a question about what this deployment is
// allowed to do; a preset is a named composition of answers; and
// `applyCommandRules()` is the single place that turns them into chrome. There
// is deliberately no second dispatch path and no second menu definition —
// `TAURI-004` establishes that there is one menu model with two presentations,
// and a mode is a third presentation of the same model, not a third model.
//
/// The axes. Every one is a *permission*, phrased so that `true` is the
/// permissive answer.
///
/// The standalone editor is all-`true`, which keeps the property that made this
/// table worth having: "the default changes nothing" is checkable by looking
/// rather than by remembering.
///
/// - `canOpen`   — may the editor replace the document it is showing? Covers
///                 `File ▸ New` as well as `File ▸ Open`: both discard what is
///                 on screen and put something else there, which is the thing
///                 a host cannot allow, and calling only one of them "open"
///                 would leave the other as a hole in the same wall.
/// - `canSaveAs` — may the user take a copy out (the Download submenu, Ctrl+S)?
/// - `canPrint`  — may the user put it on paper?
/// - `ownsFile`  — does the **host** own the document? This is not a fourth
///                 permission but a statement about who the file belongs to,
///                 and the other three are read in its light (below).
/// - `chrome`    — `"web"` (our page, our header and menu bar), `"native"`
///                 (the OS draws the menu bar — `TAURI-004`), `"embedded"`
///                 (we are inside somebody else's product, so our branding
///                 strip is duplication rather than chrome).
/// - `readOnly`  — the workbook cannot be edited. The *engine* is what enforces
///                 this (`session_read_only`); the capability is how a preset
///                 asks for it, and boot hands it to the engine so the two
///                 cannot drift.
/// - `canShare`  — may the user start a collaborative session (`File ▸ Share…`)?
///                 Held `false` in every preset until `COL-46` closed — a
///                 `$`-anchored formula rebased across a concurrent insert
///                 landed as `$E$1` on one replica and `$D$1` on the other with
///                 no error raised, and a Share button that walks two people
///                 into a silent divergence is worse than no button. `COL-46`
///                 is Done, so it is `true` in `standalone` and `desktop`.
///                 **A host still owns the document**, so it stays `false` in
///                 `embedded` and `wopi`: starting a session there is the
///                 host's decision, not ours, and it can turn it on with
///                 `setCapabilities({ canShare: true })`.
///
///                 `COL-50` is still open and the dialog names it: a range
///                 formula meeting a concurrent insert **and** delete on the
///                 same axis can still settle differently, because growing a
///                 range and clamping one do not commute and each answer is the
///                 one Excel gives for its own order. That needs a range
///                 formula plus two concurrent structural edits, where `COL-46`
///                 needed one ordinary insert beside one ordinary formula —
///                 which is why one held the feature and the other does not.
export const CAPABILITIES = ["canOpen", "canSaveAs", "canPrint", "canShare", "ownsFile", "chrome", "readOnly"];

const CHROMES = ["web", "native", "embedded"];

/// Named compositions. A preset is the whole answer, not a patch on another
/// preset: reading one tells you what that mode is without chasing a chain.
const MODE_PRESETS = {
  // Today's editor, and the default. Every permission granted, our own chrome.
  standalone: { canOpen: true, canSaveAs: true, canPrint: true, canShare: true, ownsFile: false, chrome: "web", readOnly: false },
  // The desktop shell. Same document ownership as standalone — the user's own
  // file, opened by the user — but the operating system draws the menu bar.
  desktop: { canOpen: true, canSaveAs: true, canPrint: true, canShare: true, ownsFile: false, chrome: "native", readOnly: false },
  // Inside somebody else's page. The host owns the document *and* the chrome.
  embedded: { canOpen: false, canSaveAs: false, canPrint: true, canShare: false, ownsFile: true, chrome: "embedded", readOnly: false },
  // A WOPI frame. The host owns the document — including save and versioning,
  // which is why `canSaveAs` starts false — but the editor **is** the frame and
  // draws its own chrome, the way Office Online does inside a WOPI host. The
  // difference from `embedded` is one axis, which is the point of composing
  // them rather than writing two mode branches.
  wopi: { canOpen: false, canSaveAs: false, canPrint: true, canShare: false, ownsFile: true, chrome: "web", readOnly: false },
  // A published sheet. Read-only, and a copy is still allowed: a viewer that
  // cannot print or export is a screenshot with extra steps, and `READ_ONLY_SAFE`
  // has always let both through.
  viewer: { canOpen: false, canSaveAs: true, canPrint: true, canShare: false, ownsFile: false, chrome: "web", readOnly: true },
};

export const MODES = Object.keys(MODE_PRESETS);

/// **The mode somebody asked for, or `null` when nobody did.**
///
/// Filtered against the known list rather than trusted, for the same reason
/// `?hide=` is: it arrives on a URL, so anybody who can hand somebody a link
/// chooses it. An unknown value is not a mode, so it reads as nobody having
/// asked, and the default below decides.
///
/// `?chrome=native` is kept as an alias for `?mode=desktop`. `desktop/src/main.rs`
/// appends it and `editor.native-chrome.spec.mjs` asserts it; a URL contract a
/// shipped host already uses is not something to break for tidiness.
///
/// "Was this chosen?" is a different question from "what is it?", and both
/// `askedMode()` and `setMountRoot()` have to ask the first one: an explicit
/// `?mode=` is a ceiling somebody set deliberately, and a default must never
/// overwrite it (`UX-EMBED-02`).
function explicitMode() {
  const asked = (PARAMS.get("mode") || "").trim().toLowerCase();
  if (MODES.includes(asked)) return asked;
  if (PARAMS.get("chrome") === "native") return "desktop";
  return null;
}

/// Whether this document is inside somebody else's page.
///
/// The frame half of the embedding question, answerable at module-eval time —
/// which it has to be, because `capabilityModeName` is initialised from
/// `askedMode()` on the next line but one. The shadow-root half cannot be:
/// `<opencalc-sheet>` calls `setMountRoot()` *after* this module has finished
/// evaluating, so that case is caught there instead.
function insideAnotherPage() {
  try {
    return window.top !== window;
  } catch {
    // A cross-origin parent throws on access, which is itself the answer — and
    // the answer is "yes", so the throw must not be read as "no".
    return true;
  }
}

/// The mode this page was booted in: what was asked for, or what the mount says.
function askedMode() {
  const asked = explicitMode();
  if (asked) return asked;
  // **An embed that says nothing is an embed, not a standalone editor.**
  //
  // Measured before this line existed: a framed `editor.html` and an
  // `<opencalc-sheet>` both resolved to `standalone`, so both were
  // byte-identical to our own page — `canOpen` true, so `File ▸ New` and
  // `File ▸ Open` were listed *and runnable* and a visitor could replace the
  // host's document from inside the host's own page; `canSaveAs` true, so
  // eight download entries the host never authorised; `canShare` true, in
  // somebody else's product, when starting a session is the host's decision;
  // and `chrome: "web"`, so our branding strip sat inside their page.
  //
  // Restrictive-by-default is the opposite of the `?mode=nonsense` rule above,
  // and deliberately so: a typo on *our* page must not take a user's Save
  // away, but a document that belongs to a host must not be handed out on the
  // strength of the host having said nothing. An embedder who does want the
  // wider set asks for it — this changes the default, not the ceiling.
  return insideAnotherPage() ? "embedded" : "standalone";
}

/// Host overrides, applied on top of the preset. Empty until a host calls
/// `setCapabilities`.
let capabilityOverrides = {};
let capabilityModeName = askedMode();

/// Preset + overrides, with `ownsFile` read last.
///
/// **`ownsFile` forces `canOpen` off and cannot be overridden back on.** There
/// is no host use for "the document is mine, and also let the user swap it for
/// another one from inside my page" — a host that wants a different document
/// loads a different document. `canSaveAs` is deliberately *not* forced the
/// same way: "download a copy" is a genuine permission a host grants per user
/// (WOPI has a flag for exactly it), so a host may turn it back on, and doing
/// so is that host authorising it rather than the editor assuming.
function resolveCapabilities() {
  const preset = MODE_PRESETS[capabilityModeName] || MODE_PRESETS.standalone;
  const caps = { ...preset, ...capabilityOverrides };
  if (!CHROMES.includes(caps.chrome)) caps.chrome = preset.chrome;
  if (caps.ownsFile) caps.canOpen = false;
  caps.mode = capabilityModeName;
  return Object.freeze(caps);
}

let capabilities = resolveCapabilities();

/// The resolved set, for a host — and for a test, which is the only way to
/// assert a mode without reading back the chrome it produced.
///
/// `readOnly` is **the engine's answer or the mode's, whichever is restrictive**.
/// The engine is the only thing that can actually refuse an edit, and a host may
/// put a session into read-only directly through `setReadOnly` without touching
/// a mode at all; a capability set that reported `readOnly: false` for such a
/// session would be describing an editor that does not exist. The other way
/// round matters too — between `resolveCapabilities()` and boot handing
/// `readOnly` to the engine there is a window where the mode is the only thing
/// that knows, and the menu must already be right in it.
export function getCapabilities() {
  let engineReadOnly = false;
  try { engineReadOnly = readOnly(); } catch { engineReadOnly = false; }
  if (engineReadOnly === capabilities.readOnly) return capabilities;
  return Object.freeze({ ...capabilities, readOnly: capabilities.readOnly || engineReadOnly });
}

/// Override individual capabilities, or switch mode outright with `{ mode }`.
///
/// The host surface is `window.opencalcEditor`, which is this module namespace,
/// so this *is* the existing surface rather than a new one beside it. Partial
/// on purpose: a host that wants Download back in an embedded editor says
/// `{ canSaveAs: true }` and does not have to restate the other five.
export function setCapabilities(partial) {
  const wasReadOnly = capabilities.readOnly;
  if (partial && typeof partial === "object") {
    if (typeof partial.mode === "string" && MODES.includes(partial.mode)) {
      capabilityModeName = partial.mode;
      // A new mode is a new baseline; carrying the previous mode's overrides
      // into it is how a host ends up in a state no preset describes.
      capabilityOverrides = {};
    }
    for (const key of CAPABILITIES) {
      if (partial[key] !== undefined) capabilityOverrides[key] = partial[key];
    }
  }
  capabilities = resolveCapabilities();
  applyModeChrome();
  // A `readOnly` capability that only changed the menu would be a viewer you
  // could still type into — the engine is what refuses an edit. Pushed **only
  // when this call moved it**, because `setReadOnly(false)` on every unrelated
  // capability change would silently unlock a session a host had locked
  // directly, which is the same class of quiet override in the other direction.
  // `setReadOnly` applies the rules itself, so the branches do not double up.
  if (capabilities.readOnly !== wasReadOnly) setReadOnly(capabilities.readOnly);
  else applyCommandRules();
  return getCapabilities();
}

/// Chrome that is decided by the mode rather than by a command.
///
/// `setNativeChrome` is `TAURI-004`'s, unchanged — the native bar is handed
/// over by adding a class to the root and the nodes stay in the document.
/// `oc-chrome-embedded` is its sibling for the third presentation.
///
/// **`oc-chrome-embedded` is stamped on this mount, not on the page.**
///
/// It used to go on `document.documentElement` unconditionally, which was
/// harmless only while the embedded chrome was unreachable from a shadow-root
/// mount. Now that an `<opencalc-sheet>` resolves to `embedded` by default
/// (`UX-EMBED-02`), that `<html>` is the *host's*: the class would cross the
/// shadow boundary in the one direction the boundary exists to prevent and
/// hide the host's own `.app-header`. `.editor-body` is `<body>` for a page
/// mount and the shell element for a shadow mount, and it is an ancestor of
/// `.app-header` in both, so `.oc-chrome-embedded .app-header` still bites
/// without the stylesheet changing.
/// **Hiding a menu requires evidence that another menu exists** (`UX-CHR-02`).
///
/// `?chrome=native` is a *request*, and it arrives on a URL — anybody who can
/// hand somebody a link chooses it. `UX-DESK-01` treated it as proof, so
/// `.oc-chrome-native #menubar { display: none }` fired in an ordinary browser
/// too: the branding strip went, the menu bar went, and **nothing drew a menu
/// in their place.** No File menu, no View menu, and — because the only theme
/// control at the time was a `<select>` inside the gear that lived in the
/// hidden strip — no way to change the theme at all. A user was stranded by a
/// query string.
///
/// Nothing in the browser suite could see it, which is why it shipped: those
/// tests run in a browser, where `?chrome=native` changes CSS and no native bar
/// exists to notice the absence of. The tests now install the shell's bridge to
/// say "a native menu exists", which is the only way to tell the two apart.
///
/// Three signals, in the order they can arrive, and any one is enough:
///
/// - **`window.__TAURI__`** — Tauri installs its API bundle at document start
///   (`withGlobalTauri` in `desktop/tauri.conf.json`), so this is true before
///   this module evaluates. It is the only one that arrives early enough to
///   avoid a frame with both bars, which is the whole reason to check it.
/// - **`window.__opencalcNative`** — the shell's own bridge, and the object
///   `applyCommandRules()` already calls `publishMenu()` on. The same
///   `BOOTSTRAP` that installs it publishes the menu, so its presence *is* the
///   native bar. It is injected on `PageLoadEvent::Finished` — after this
///   editor has booted — so it cannot be the only check.
/// - **`opencalc-native-ready`** — dispatched by that bootstrap once the menu
///   has been published. The backstop: a shell that changes how it installs
///   itself still lands here.
///
/// Latching rather than re-asking: once a native menu has been drawn it does
/// not un-draw, and a `setCapabilities` call in between must not make the bar
/// reappear underneath it.
let nativeMenuSeen = false;

function nativeMenuIsDrawn() {
  if (nativeMenuSeen) return true;
  if (typeof window !== "undefined" && (window.__opencalcNative || window.__TAURI__)) {
    nativeMenuSeen = true;
  }
  return nativeMenuSeen;
}

export function applyModeChrome() {
  const native = capabilities.chrome === "native";
  setNativeChrome(native);
  placeNativeChrome(native);
  // **The two halves of desktop chrome, separated.** Density and the branding
  // strip need no native bar to be correct — the strip holds identity and the
  // gear, `placeNativeChrome()` has already moved the status line and the
  // roster somewhere visible, and Settings is reachable from the menus. The
  // *menu bar* is the one region whose removal takes a capability away with
  // nothing replacing it, so it alone waits for the evidence.
  const root = document.documentElement;
  const menuGone = native && nativeMenuIsDrawn();
  const changed = root.classList.contains("oc-native-menu") !== menuGone;
  root.classList.toggle("oc-native-menu", menuGone);
  const scope = ocRoot === document ? document.documentElement : qs(".editor-body");
  scope?.classList.toggle("oc-chrome-embedded", capabilities.chrome === "embedded");
  applyModeLabels();
  // The bar's height is the sheet's the moment the bar goes, and that moment is
  // now sometimes *after* boot — the shell's bridge arrives on page-load-finish.
  // Without this the canvas keeps the size it was laid out at and the grid ends
  // 30px short of the window it is supposed to fill. Guarded because the first
  // `applyModeChrome()` of a boot runs before the canvas is bound.
  if (changed) { try { resize(); } catch {} }
}

// The backstop signal, armed once. `applyModeChrome()` is idempotent, so
// re-running it on an event that may never come costs nothing.
if (typeof window !== "undefined" && typeof window.addEventListener === "function") {
  window.addEventListener("opencalc-native-ready", () => {
    nativeMenuSeen = true;
    try { applyModeChrome(); } catch {}
  }, { once: true });
}

/// **Wording that belongs to the chrome, not to the command** (`TAURI-009`).
///
/// Reported on the desktop build: a desktop application offers *"Download ▸
/// Excel"* where every desktop application on every platform says *Save As*.
/// Downloading is what a browser does — a file lands in a folder the user did
/// not choose and the document has no home to go back to; a desktop app writes
/// a file. Both sentences are true, in different shells, of the same command.
///
/// So the **id does not move.** `commandId()` derives it from the English label
/// at build time, and those ids are a published dispatch surface — the desktop
/// shell's native menu holds `file.download.excel-xlsx` and nothing else, hosts
/// name them in `setCommandRules`, and `CAPABILITY_COMMANDS` matches them with
/// `/^file\.download/`. Renaming the *menu* must not rename the *command*, or
/// `canSaveAs` silently stops governing the six entries it exists to govern.
/// Only `data-oc-label` and the visible text change.
///
/// `data-oc-label` rather than the rendered text alone, because `relabel()`
/// re-derives every label from it on a locale change and would otherwise put
/// the web wording back in a native window. The web wording is parked in
/// `data-oc-label-web` the first time this runs, so leaving desktop chrome
/// restores it — `setCapabilities({ mode })` is a host surface and every other
/// part of this move is reversible.
const NATIVE_LABELS = {
  // The submenu itself. Its contents are conversions the engine says it can
  // write, and "export" is what every desktop office application calls that.
  "file.download": "Export",
  // The one entry that is not a conversion: it writes the document back in the
  // kind of file it came from, without adopting the result as the window's save
  // target. Excel calls exactly that "Save a Copy".
  "file.download.same-format-as-opened": "Save a copy…",
};

function applyModeLabels() {
  const native = capabilities.chrome === "native";
  for (const [id, nativeLabel] of Object.entries(NATIVE_LABELS)) {
    const node = qs(`[data-oc-command="${CSS.escape(id)}"]`);
    if (!node) continue; // the menu bar is built after the first boot call
    if (node.dataset.ocLabelWeb === undefined) node.dataset.ocLabelWeb = node.dataset.ocLabel ?? "";
    const label = native ? nativeLabel : node.dataset.ocLabelWeb;
    node.dataset.ocLabel = label;
    // The same lookup `relabel()` does, so a catalogue entry for this command
    // still wins and this is not a second translation path.
    const text = t(`command.${id}`, label);
    const slot = node.querySelector(".mi-label");
    if (slot) slot.textContent = text;
    else node.textContent = text;
  }
}

/// Where each node's markup put it, so a mode change can put it back.
///
/// Keyed by the node, holding the parent and the *next sibling* rather than an
/// index: an index is wrong the moment anything else in that parent moves, and
/// `buildMenuBar()` inserts eight buttons into the menu bar after boot.
const chromeHome = new WeakMap();

/// Send a node home, remembering where home was the first time it is asked.
function homeChrome(node) {
  const home = chromeHome.get(node);
  if (!home || !home.parent) return;
  // `insertBefore(node, null)` appends, which is exactly right for a node that
  // was the last child — and `next` is `null` in precisely that case.
  if (node.parentElement !== home.parent || node.nextElementSibling !== home.next) {
    home.parent.insertBefore(node, home.next);
  }
}

/// Move a node, recording where it came from the first time.
function moveChrome(node, into, before) {
  if (!chromeHome.has(node)) {
    chromeHome.set(node, { parent: node.parentElement, next: node.nextElementSibling });
  }
  if (node.parentElement !== into || node.nextElementSibling !== (before || null)) {
    into.insertBefore(node, before || null);
  }
}

/// **Desktop chrome moves two things rather than hiding them** (`UX-DESK-01`).
///
/// The rest of the native presentation is `editor.css`'s — one class, a set of
/// metrics, and `display: none` on the branding strip. Two nodes cannot be done
/// that way, because hiding them would take a capability away rather than
/// relocate it, and both live inside a region desktop chrome removes:
///
/// - **`#tb-status`** *used* to be moved here. It is the engine version, the
///   open/save progress line and every error the editor reports, and it sat in
///   the branding strip — so desktop chrome, which drops that strip, had to
///   relocate it or lose it. `UX-CHR-03` deleted the *product* strip's contents
///   outright and authored `#tb-status` into `.bottom-bar` in `editor.html`,
///   where Excel, LibreOffice and OnlyOffice all keep document state, for every
///   chrome. Nothing in that argument was ever desktop-only; it was above the
///   menu bar because the branding strip was. So this function no longer names
///   it: a node that is already where every mode wants it needs no mode to
///   move it.
/// - **`#presence`** is the collaborator roster, and `COL-33` put it in the
///   menu bar *specifically* so it would not fold away with the page header —
///   then `.oc-chrome-native #menubar { display: none }` folded the menu bar
///   away instead, and took it. Desktop mode has `canShare: true`, so this was
///   a session whose participants the desktop user could not see.
///
/// Reversible, because `setCapabilities({ mode })` can turn native chrome off
/// again and a one-way move would leave the page's own header permanently
/// missing its status line.
function placeNativeChrome(on) {
  const bottom = qs(".bottom-bar");
  const presence = byId("presence");
  if (!bottom) return;
  for (const node of [presence]) {
    if (!node) continue;
    // Before the language picker, which is the last item of the status bar's
    // left-hand group. `?? null` rather than a second lookup: `insertBefore`
    // with a null reference appends, which is where they belong if the picker
    // ever goes.
    if (on) moveChrome(node, bottom, byId("locale-picker"));
    else homeChrome(node);
  }
}

/// Which capability governs which command ids.
///
/// **Regexes over ids, matching `READ_ONLY_SAFE` exactly** — the mechanism
/// `applyCommandRules()` already uses — rather than a name check inside the
/// sweep. That is what makes "hidden because a capability says so" true by
/// construction: there is one table, and a reader can see every command a mode
/// takes away without reading the loop.
///
/// Ids come from the English label path (`File ▸ Download ▸ CSV (.csv)` →
/// `file.download.csv-csv`), so `/^file\.download/` covers the submenu opener
/// and all five entries under it — the six the audit counted.
const CAPABILITY_COMMANDS = {
  // `toolbar.open` is the hidden `<input type=file>` the menu item clicks.
  // Hiding the menu item alone would leave the input runnable through
  // `runCommand`, which is the whole point of listing it here.
  //
  // `header.open` used to be a third entry: the branding strip's folder button.
  // The button is gone (`UX-CHR-01`) and so is the id — `listCommands()` reads
  // the live DOM, so there is nothing left for a pattern here to match.
  canOpen: [/^file\.new$/, /^file\.open$/, /^toolbar\.open$/],
  // `file.save` joins the six Download entries rather than getting a capability
  // of its own: it is the same permission — may this user take the document out
  // of the editor — asked by the command that writes it back to its own file.
  // A mode that hid the submenu and left Save listed would have taken nothing
  // away, which is the failure this table exists to prevent.
  canSaveAs: [/^file\.download/, /^file\.save$/],
  canPrint: [/^file\.print$/],
  // `File ▸ Share…`. Off in every preset while `COL-46` is open, so the command
  // is absent from `listCommands()` *and* refused by `runCommand` — the rule
  // this table exists to keep: a command taken off the menu and still runnable
  // from a script has not been taken away, it has been hidden from the one
  // party who could have declined it.
  canShare: [/^file\.share$/],
};

/// Commands that exist in **one chrome only** (`TAURI-009`).
///
/// Deliberately not a capability. A capability is a permission a host grants —
/// "may this user take a copy out" — and a host can turn one on. This is a fact
/// about the shell the editor is running in, and no host setting changes it:
/// `File ▸ Save` commits the document to the file the shell holds
/// (`docs/83` §2), and in a browser tab there is no such file. `Ctrl+S` there
/// writes a copy into the downloads folder, which the Download submenu already
/// says out loud — a second entry saying "Save" for the same act would be the
/// desktop vocabulary leaking into the web build, which is `TAURI-009`'s own
/// complaint pointing the other way.
///
/// Routed through `capabilityForbids` rather than a class set by hand, because
/// `applyCommandRules()` restores anything it did not itself hide: a
/// `oc-cmd-hidden` added from outside that function is removed on its next
/// pass. One gate, one table, and `listCommands()`/`menuModel()`/`runCommand`
/// all agree by construction.
const CHROME_ONLY = {
  native: [/^file\.save$/],
};

/// True when some capability of this mode, or the chrome it runs in, forbids
/// the command.
export function capabilityForbids(id) {
  for (const [cap, patterns] of Object.entries(CAPABILITY_COMMANDS)) {
    if (capabilities[cap] === false && patterns.some((re) => re.test(id))) return true;
  }
  for (const [chrome, patterns] of Object.entries(CHROME_ONLY)) {
    if (capabilities.chrome !== chrome && patterns.some((re) => re.test(id))) return true;
  }
  return false;
}

/// Which capability forbade it, for a message and for the host's event.
function forbiddenBy(id) {
  for (const [cap, patterns] of Object.entries(CAPABILITY_COMMANDS)) {
    if (capabilities[cap] === false && patterns.some((re) => re.test(id))) return cap;
  }
  return null;
}

/// Refuse a command the mode does not have, **out loud and to the host**.
///
/// Two audiences, and both are needed. The user gets a sentence, because a
/// keystroke that silently does nothing reads as a broken editor rather than as
/// a deliberate boundary. The *host* gets `commandRefused` on the existing
/// event surface, and that is the half that makes `ownsFile` more than a
/// prohibition: under WOPI the host owns save, so Ctrl+S is not a mistake to
/// swallow — it is the user asking for a save the host is the one who can
/// perform. A host with no listener loses nothing; `emit` is a no-op then.
///
/// Returns `true`, so a caller reads as `if (refuse(id)) return;`.
function refuse(id, message) {
  const capability = forbiddenBy(id);
  emit("commandRefused", { id, capability, ownsFile: capabilities.ownsFile, mode: capabilities.mode });
  statusError(message);
  return true;
}

// --- Mount root -------------------------------------------------------------
//
// Every DOM lookup goes through here rather than through `document`, so the
// same editor runs as a page *and* inside a shadow root. That is what lets a
// host embed it without its own stylesheet reaching in: a shadow boundary is
// the only thing in the platform that actually stops CSS, and it also means the
// editor's own selectors cannot leak out onto the host's page.
//
// `document` is still right for three things and they are left alone:
// `createElement` (nodes are not scoped), listeners that must catch events
// anywhere on the page (a mouse-up after the pointer leaves the grid), and the
// clipboard. Anything that *finds* an element in our markup, or parents a
// floating layer over it, uses these.
let ocRoot = document;
/// The node floating layers (menus, tooltips, submenus) attach to.
///
/// Inside a shadow root that is the root itself: appending to `document.body`
/// would put the menu outside the boundary, where our stylesheet does not reach
/// and the host's does — so it would come out unstyled and inherit theirs.
export let ocOverlayHost = document.body;
/// The element carrying `data-theme` and the accent override.
///
/// The page's `<html>` when running as a page; the host element when embedded,
/// so a theme switch inside one embedded editor does not restyle the page
/// around it — or a second editor beside it.
export let ocThemeHost = document.documentElement;

/// Point the editor at a mount root. Called by the embed wrapper before `main`.
///
/// **A root that is not the document decides the mode as well as the DOM.**
/// `askedMode()` cannot see this case: it runs while this module is still
/// evaluating, and `<opencalc-sheet>` calls this afterwards — so a shadow-root
/// embed reached `resolveCapabilities()` as `standalone` and got every
/// permission there is (`UX-EMBED-02`). An explicit `?mode=` still wins, for
/// the same reason it does in `askedMode()`: this changes the default, not the
/// ceiling.
export function setMountRoot(root) {
  ocRoot = root;
  ocOverlayHost = root === document ? document.body : root;
  ocThemeHost = root === document ? document.documentElement : root.host;
  if (root !== document && explicitMode() === null) {
    capabilityModeName = "embedded";
    capabilities = resolveCapabilities();
  }
}

/// Whether this editor **is** the page, rather than a part of somebody else's.
///
/// Two ways it is not, and they are the two ways this project is embedded: a
/// shadow root (`<opencalc-sheet>`, which calls `setMountRoot` before `main`)
/// and a frame (the landing page's demo, and any host that iframes
/// `editor.html`).
///
/// Added for `editor.drafts.js`, and the reason is a privacy rule rather than a
/// layout one. `docs/83` §3.3 refuses a local draft wherever the document is
/// somebody else's — "a host's document must not leave a copy in the user's
/// browser storage as a side effect of being opened". The `ownsFile` capability
/// says that when a host sets it, but the presets are chosen by `?mode=`, and
/// **an embed that says nothing gets `standalone`**. So the capability alone
/// would have every `<opencalc-sheet>` on the web quietly writing its host's
/// document into the visitor's IndexedDB.
export function editorIsThePage() {
  if (ocRoot !== document) return false;
  try {
    return window.top === window;
  } catch {
    // A cross-origin parent throws on access, which is itself the answer.
    return false;
  }
}

export const byId = (id) => ocRoot.getElementById(id);
export const qs = (sel) => ocRoot.querySelector(sel);
export const qsa = (sel) => ocRoot.querySelectorAll(sel);
/// The focused element *within this mount*. A shadow root reports its own.
export const activeEl = () => ocRoot.activeElement;


// Header strip sizes. Zero when the sheet hides its headers (OOXML's
// showRowColHeaders="0"), which is what makes the grid start at the very
// top-left corner: everything else measures the body as "past HW/HH", so the
// whole layout follows from these two numbers.
// **Both were hard constants, and both were wrong** (`UX-CHR-09`, `docs/88` §6).
//
// The column band was 24px, the tallest of the five competitors. 20px is where
// three of the four sit, and it is exactly Excel's default row height — which is
// what makes the band read as "one row tall" rather than as a bar.
//
// The row band was a fixed 46px, wrong at both ends. At rows 1-99 the label
// needs 6-16px and the band spent 46, giving away ~30px of grid width on every
// frame of every ordinary sheet. At the bottom it was too narrow: `1048576`
// measures 50px at the header's font against a 46px band, so the last rows drew
// a **truncated number** — confirmed by driving the Name Box to `A1048576`,
// which is the only route that jumps past the data edge.
//
// So the row band is derived from the digits actually reachable, and **stepped**
// rather than continuous: a band that tracked the exact widest label would
// reflow the entire grid while scrolling. LibreOffice's rule.
const HEADER_H = 20; // column-header height (px)
const HEADER_W_MIN = 30; // never narrower than this, however few the digits
// Widths for 1..7 digits, measured once at the header's own font and cached.
// Indexed by digit count, so `ROW_BAND[3]` is the band for rows up to 999.
let rowBandCache = null;
function rowBandFor(digits) {
  if (!rowBandCache) {
    const probe = document.createElement("canvas").getContext("2d");
    probe.font = "12px system-ui, sans-serif";
    rowBandCache = [0];
    for (let d = 1; d <= 7; d++) {
      // `8` is the widest digit in most faces, which is why LibreOffice sizes
      // from `"8888"` rather than from the label actually on screen — the band
      // must not change when 1999 scrolls past 8888.
      rowBandCache.push(Math.ceil(probe.measureText("8".repeat(d)).width) + 10);
    }
  }
  const d = Math.max(1, Math.min(7, digits));
  return Math.max(HEADER_W_MIN, rowBandCache[d]);
}
// Seeded at the two-digit band; `syncHeaderMetrics()` sizes it properly on
// the first frame, before anything is drawn.
export let HW = 30;
export let HH = HEADER_H;
// Outline gutter: the strip left of the row headers / above the column headers
// holding the group rails and their collapse toggles. Zero-width unless the
// sheet actually has an outline, so a normal sheet is unaffected.
const OUTLINE_STEP = 11; // px of indent per nesting level
let GW = 0;
let GH = 0;
let outlineRowMax = 0;
let outlineColMax = 0;
export let outlineToggles = []; // [{x,y,w,h,index,columns}] rebuilt each frame
/// Where the select-all triangle was drawn, so a test clicks the mark rather
/// than a coordinate it computed the same way the renderer did.
let cornerMarkBox = null;
export function cornerMark() { return cornerMarkBox; }
// Re-read the active sheet's header visibility. Called at the top of measure(),
// so every frame lays out against the current setting.
function syncHeaderMetrics() {
  let hidden = false;
  try { hidden = !!(wasm && wasm.session_headers_hidden(state.sheet)); } catch {}
  // The deepest nesting on each axis sizes its gutter. Asking for zero lines
  // returns just the maximum, so this costs nothing on a sheet with no outline.
  outlineRowMax = outlineColMax = 0;
  if (wasm && !hidden) {
    try {
      outlineRowMax = JSON.parse(wasm.session_outline(state.sheet, 0, 0, false)).max || 0;
      outlineColMax = JSON.parse(wasm.session_outline(state.sheet, 0, 0, true)).max || 0;
    } catch {}
  }
  GW = outlineRowMax ? outlineRowMax * OUTLINE_STEP + 8 : 0;
  GH = outlineColMax ? outlineColMax * OUTLINE_STEP + 8 : 0;
  // HW/HH are the *total* inset the grid starts after, so every existing
  // geometry consumer follows the gutter without knowing it exists. The header
  // strips themselves draw from GW/GH inward.
  // The band is sized for the deepest row currently reachable, not for the
  // deepest row that exists — otherwise every sheet pays for 1,048,576.
  // `state.scroll.row` plus a viewport's worth is what the next frame can show,
  // and the step means the answer only changes at a power of ten.
  const deepest = Math.max(1, (state.scroll?.row ?? 0) + 200, (state.sel?.row ?? 0) + 1);
  HW = hidden ? 0 : rowBandFor(String(deepest).length) + GW;
  HH = hidden ? 0 : HEADER_H + GH;
}
// One indent level, in px — Excel's is about three space-widths.
const INDENT_PX = 10;
// Zoom is applied to the *canvas context*, not to the geometry: the grid keeps
// measuring in engine pixels and only the drawing (and the viewport it has to
// fill) is scaled. That keeps every offset the engine reports directly
// comparable with what is drawn — the alternative, scaling column widths and
// row heights, would put drawn and modelled geometry back out of step.
export const ZOOM_MIN = 0.25, ZOOM_MAX = 2;
export let COL_W = 64;
export let ROW_H = 20;

export const state = {
  sheet: 0,
  scrollX: 0, // absolute content pixel offset (left of the viewport)
  scrollY: 0, // absolute content pixel offset (top of the viewport)
  firstRow: 0, // first visible row (derived from scrollY in measure())
  zoom: 1,     // canvas magnification (Ctrl+wheel / View ▸ Zoom)
  firstCol: 0, // first visible column (derived from scrollX in measure())
  sel: { row: 0, col: 0 }, // focus cell
  anchor: { row: 0, col: 0 }, // selection anchor
  selKind: "cells", // "cells" | "rows" | "cols" | "all"
  endMode: false, // Excel's End mode: armed by `End`, spent by the next arrow
  ranges: [], // committed extra rectangles for a multi-range (Ctrl+click) selection
  dragging: false,
  headerDrag: null, // "row" | "col" | null — which axis a header drag extends
  editing: false,
  resize: null, // active header resize: { axis:"col"|"row", index, previewPx, scope }
  freezeDrag: null, // active freeze-divider drag: { axis:"col"|"row", px, py }
  // Dragging an already-selected header to reorder it: { axis, at, count, before }.
  // Sheets' rule, and it is the one that leaves drag-to-extend alone — grabbing
  // a header *outside* the selection still selects and extends, as it always
  // did. `before` is in pre-move coordinates, which is where the indicator the
  // user is looking at actually sits.
  moveDrag: null,
  fill: null, // active drag-fill: { src:{r0,c0,r1,c1}, dst:{...} }
};
let fillHandleRect = null; // screen rect of the fill handle (for hit-testing)
// The two touch range handles, in grid units, refreshed by every `draw()`.
// `null` on either corner means that corner is scrolled out of the body and so
// is neither drawn nor grabbable. See the design note above `touchHandleAt`.
let touchHandles = { tl: null, br: null };
// The live handle drag: which corner the finger has, if any. Module scope
// rather than closure scope because `autoScrollTick` reads it.
let handleDrag = null;
// Whether a finger has ever touched this grid. See `touchHandlesOn`.
let touchSeen = false;
export let validationChevron = null; // {x,y,w,h,values} of the active cell's list-dropdown button
let commentCells = new Set(); // "r,c" of cells with a note in view (for hover tooltip)
// "r,c" of linked cells in view. Kept as a set rather than asked per cell so a
// screenful of links costs one call, and so the click handler can tell in O(1)
// whether a click is on a link before doing anything slower.
let linkCells = new Set();
// Tables on the current sheet, refreshed each draw. Held so the header
// filter-button hit test does not have to ask the engine on every mousedown.
let tablesInView = [];
// Decoded pictures, keyed by package part.
//
// An <img> decodes asynchronously, so the first frame after a load has nothing
// to draw and a redraw has to be asked for once it does — otherwise the picture
// only appears the next time something else happens to repaint.
export const imageCache = new Map();

// The pixel rectangle an anchored object occupies.
//
// Three things this has to get right, each of which was wrong before:
//
//  * the far edge is the *trailing* edge of the last row and column, not the
//    leading one — `colXAt(c1)` drew every frame a column and a row short;
//  * `fscreen*` rather than the raw accessors, so an edge scrolled out of the
//    window still has a position. The old code bailed out when the top-left was
//    not drawn, which made a whole chart vanish the moment its first row
//    scrolled off the top;
//  * the EMU offsets, so an edge sits where it was dragged instead of snapping
//    to the nearest gridline.
/// EMUs per CSS pixel at 96 dpi — OOXML's own constant.
const EMU_PER_PX = 9525;
export const emuToPx = (emu) => emu / EMU_PER_PX;
const pxToEmu = (px) => Math.round(px * EMU_PER_PX);
/// Whether a rectangle is entirely outside the drawable area.
export const offCanvas = (r) =>
  r.x + r.w < HW || r.y + r.h < HH || r.x > canvas.clientWidth || r.y > canvas.clientHeight;

// Pictures anchored on the sheet, drawn under the charts and over the cells.

// Chart frames from the last paint, for hit-testing; the selected chart; and
// the drag in progress. A chart floats over the grid rather than occupying
// cells, so none of this can come from the cell hit test.
export let chartFrames = [];
export let chartSel = null;
export let chartDrag = null;
/// Handle radius, and the slop a click gets when aiming at one.
export const CHART_HANDLE = 4;

// Bytes of chart payload the last frame pulled across the WASM boundary.
//
// Pinned by `editor.frame-budget.spec.mjs` (`CHT-13`). Until that row this
// frame asked for every chart's series with **every point resolved** and used
// the anchor rectangle out of it, so the cost of drawing a chart tracked the
// size of the range its series named — 2 MB a frame for a chart on an empty
// sheet whose series name whole columns. A byte count is the thing that was
// wrong; a wall-clock assertion on it would be flaky.
let chartPayloadBytes = 0;
export const chartPayloadBytesForTest = () => chartPayloadBytes;

// Every chart on the sheet, at its anchored cells.
//
// A chart is anchored in *cells*, which is why it moves with the rows under it
// and why this has to be recomputed each frame rather than positioned once.
function drawCharts(withQuad) {
  if (!wasm) return;
  chartFrames = [];
  let charts = [];
  // Anchors only. The picture comes from `session_chart_items` below, per
  // chart and only for the ones on screen; asking for the *data* here as well
  // resolved every series of every chart, on and off screen, to build a
  // rectangle out of eight integers of the answer (`CHT-13`).
  try {
    const payload = wasm.session_chart_frames(state.sheet);
    chartPayloadBytes = payload.length;
    charts = JSON.parse(payload);
  } catch { return; }
  for (const [index, ch] of charts.entries()) {
    const { x: x0, y: y0, w, h } = anchoredRect(ch);
    // Kept for hit-testing: a chart floats over cells rather than occupying
    // them, so clicking one cannot go through the cell grid. Recorded even
    // when it is off-screen, so the frame list is a faithful account of where
    // every chart is rather than of which ones happen to be visible.
    chartFrames.push({ index, x: x0, y: y0, w, h });
    if (offCanvas({ x: x0, y: y0, w, h })) continue;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, w, h);
    ctx.clip();
    // From the engine's own display list, which is what the PNG renderer draws
    // from too (`RND-10`). The frame is passed in rather than derived there: a
    // chart is anchored in cells, and the scroll offset, the frozen panes and
    // the zoom that turn those into pixels are all resolved here, every frame.
    // Deriving them a second time inside the engine would be a second thing to
    // keep in step, which is the fault this removes.
    try {
      // The frame goes in as **twips**, because that is the unit the layout
      // works in — `PX = 15.0` there is 1440/96. Passing CSS pixels made every
      // chart's plot area come out negative, so `push_chart` drew its frame and
      // returned before a single bar: an empty box where a chart had been.
      // `paintList` converts the items back on the way out.
      const T = 1440 / 96;
      paintList(
        ctx,
        JSON.parse(
          wasm.session_chart_items(
            state.sheet,
            index,
            Math.round(x0 * T),
            Math.round(y0 * T),
            Math.round(w * T),
            Math.round(h * T),
          ),
        ),
      );
    } catch {
      // A chart that cannot be laid out is a hole in the picture; it must not
      // take the rest of the frame with it.
    }
    ctx.restore();
  }
  drawChartSelection();
}

// The selected chart's outline and its eight handles, plus the live outline
// while one is being dragged.
//
// Drawn after every chart so it is never painted over by a chart stacked on
// top of the selected one.

/// The eight resize handles, corners first so a corner wins where two overlap.

// The rectangle a drag is currently proposing, in pixels.

// One chart: frame, title, then whichever picture its kind calls for.

/// Reserve the legend's side of the frame, shrinking `plot` to what is left.
///
/// Returns the rectangle the legend gets, or `null` when the frame is too small
/// to give it one — a legend that leaves no room for the plot has cost more
/// than it explains.

/// Swatch and name per series, stacked down the side or run across the foot.

// Series colours, taken from the workbook's theme accents so a chart matches
// the file it came from rather than a palette invented here.

// The value range a chart's axis has to cover, always including zero so a bar's
// length is proportional to its value.





// Category labels under the plot, thinned to whatever fits: overlapping labels
// are less readable than fewer of them.

// A cell's text colour. The cell's own wins; a table supplies one where the
// cell has none, because a table style's colours are part of the style, not of
// the cells — and because the block a table paints is light whatever the
// application theme is, so the grid's own text colour would vanish on it.
// The table text colour at a cell, or null where the cell's own style wins.
export function tableTextAt(r, c) {
  for (const t of tablesInView) {
    if (r < t.r0 || r > t.r1 || c < t.c0 || c > t.c1) continue;
    const isHeader = t.headers > 0 && r === t.r0;
    const isTotals = t.totals > 0 && r === t.r1;
    // The header's colour is part of the style — white on a Medium accent —
    // and the body's is whatever reads against the light block a table is.
    return isHeader || isTotals ? "#" + t.headerText : "#" + t.bodyText;
  }
  return null;
}
let errorCells = new Set();
let numericTextCells = new Set(); // "r,c" of cells holding a number stored as text

// What each spreadsheet error actually means, in the terms that caused it.
// "#VALUE!" alone tells you something broke; it does not tell you what to look
// for, which is the whole difficulty of fixing someone else's sheet.
const ERROR_HELP = {
  "#DIV/0!": "Dividing by zero, or by an empty cell.",
  "#VALUE!": "A value has the wrong type — often text where a number is expected.",
  "#REF!": "A reference points at cells that no longer exist (deleted rows or columns).",
  "#NAME?": "An unrecognised name — check the function spelling, or a missing defined name.",
  "#NUM!": "A number the calculation can't represent — out of range, or not a real result.",
  "#N/A": "A lookup found no match.",
  "#NULL!": "Two ranges that were intersected don't overlap.",
  "#SPILL!": "A result needs more room than is free.",
  "#CALC!": "The calculation produced nothing — a filter that matched no rows, usually.",
};
export let dragPos = null; // latest pointer {px,py} during a selection/fill drag
let autoRaf = 0; // rAF handle for edge auto-scroll while dragging

// The normalized selection rectangle (inclusive) from anchor..focus.
const DEFAULT_SCROLL_DAMP = 0.8; // rows-per-wheel factor; tunable in settings
let scrollDamp = DEFAULT_SCROLL_DAMP;

export let canvas;
export let ctx;
export let wrap;
export let inline;
export let selStats;
let vscroll;
let vthumb;
let hscroll;
let hthumb;
export let fInput;
export let cellRef;
let commentTip;
export let status;

/// A theme token's resolved value.
///
/// Read from the mount's own root rather than `document.body`, or an embedded
/// editor would paint its canvas from the *host page's* tokens while its chrome
/// used ours.
const css = (name) =>
  getComputedStyle(ocRoot === document ? document.body : ocRoot.host)
    .getPropertyValue(name)
    .trim();
export let colors = {};
export function readColors() {
  colors = {
    bg: css("--oc-background-color") || "#fff",
    fg: css("--oc-text-color") || "#0b0d12",
    muted: css("--oc-muted-text-color") || "#7b8391",
    grid: css("--oc-gridline-color") || "#f0f1f4",
    headerBg: css("--oc-surface-color") || "#f6f7f9",
    accent: css("--oc-accent-color") || "#2f6df6",
    sel: css("--oc-selection-color") || "rgba(47,109,246,.10)",
    // Distinct from the selection tint: a find hit and the active cell must not
    // read as the same thing.
    findHit: css("--oc-find-highlight-color") || "rgba(245,158,11,.28)",
    // Table banding. Deliberately faint and theme-derived: a band is a reading
    // aid, and one strong enough to compete with a fill or a conditional
    // format would make the user's own formatting harder to see, not easier.
    tableHeader: css("--oc-table-header-color") || "rgba(47,109,246,.16)",
    tableBand: css("--oc-table-band-color") || "rgba(127,140,170,.09)",
    // Read from the theme rather than hardcoded: the freeze divider sits on the
    // grid, so it has to darken and lighten with it. `colors.freezeLine` was
    // already consulted at the draw site but never populated here, so the
    // fallback was always what showed.
    freezeLine: css("--oc-freeze-line-color") || "#5f6368",
  };
}


// Per-frame geometry of the visible window: the engine supplies each visible
// column's width and row's height (real `.xlsx` sizes), and we accumulate them
// into leading-edge offsets so drawing and hit-testing honor variable sizing.
export const geo = {
  colW: [], // width (px) of the i-th visible column (firstCol + i)
  colX: [], // canvas x of its leading edge (includes the HW header)
  rowH: [],
  rowY: [],
  cols: 0, // columns whose leading edge is within the viewport
  rows: 0,
};

export const MIN_LINE = 8; // conservative floor used to bound how many lines to fetch
export const RESIZE_GRAB = 5; // px proximity to a header boundary that arms a resize
export let geoItems = []; // cells for the visible window, fetched in measure(), reused by draw()
export let sheetMerges = []; // merged ranges of the active sheet, refreshed each draw
let dragTab = -1; // index of the sheet tab being dragged (reorder)

// The height a cell needs, or null if it cannot grow its row. Shared by the
// per-frame measure and the document-wide growth map below — if these two ever
// disagreed, the drawn rows and the scroll offsets would disagree with them.
// Measured heights, keyed by everything that can change one. Sheets repeat text
// heavily — a label column is the same string a thousand times — and measuring
// each occurrence separately was the bulk of a rebuild. Cleared on zoom, which
// is the only thing that changes the metrics without changing the key.
export let heightMemo = new Map();



// --- Auto-height growth, document-wide ------------------------------------
//
// A wrapped or rotated cell makes its row taller than the height the engine
// knows about. Measuring text is the host's job, so the engine cannot account
// for it — and when only the *visible* rows were measured, everything derived
// from engine offsets was wrong past the first grown row: scroll anchoring
// hitched at row boundaries, the scrollbar extent came up short, scroll-into-view
// under-scrolled so the selection crept off screen, and a resize drag started
// offset by the accumulated growth.
//
// So the growth is computed for every candidate row on the sheet, once, and
// folded into the offsets through `rowOffsetPx` / `rowAtPx`. Rebuilt whenever
// the document, zoom or column widths change.
export let growthRows = [];   // ascending row indices that grow
// growthPrefix[i] = summed growth of the first i entries, so it has one more
// element than growthRows and growthPrefix[n] is the total.
export let growthPrefix = [0];
export let growthTotal = 0;
export let growthDirty = true;

export function invalidateGrowth() { growthDirty = true; }

// Zoom changes the metrics without changing any memo key, so the memo has to go.
export function clearHeightMemo() { heightMemo = new Map(); }

export function rebuildGrowth() {
  growthDirty = false;
  growthRows = [];
  growthPrefix = [0];
  growthTotal = 0;
  if (!wasm) return;
  let payload;
  try { payload = JSON.parse(wasm.session_autofit_candidates(state.sheet)); } catch { return; }
  const base = payload.default || 20;
  const extra = new Map();
  // Column widths are looked up once per distinct column, not once per cell: a
  // wrapped column of ten thousand rows is one lookup, not ten thousand.
  const widths = new Map();
  for (const it of payload.cells || []) {
    // Column width matters only for wrapping, and comes from the engine rather
    // than the drawn window — a candidate may be far outside the viewport.
    let cw = widths.get(it.c);
    if (cw === undefined) { cw = colWidthOf(it.c); widths.set(it.c, cw); }
    const needed = neededRowHeight(it, cw);
    if (needed === null || needed <= base) continue;
    const grow = needed - base;
    if (grow > (extra.get(it.r) || 0)) extra.set(it.r, grow);
  }
  growthRows = [...extra.keys()].sort((a, b) => a - b);
  let acc = 0;
  for (const r of growthRows) {
    acc += extra.get(r);
    growthPrefix.push(acc);
  }
  growthTotal = acc;
  if (payload.truncated) {
    // Never silently: a partial map means offsets past the cut are wrong again.
    status.textContent = "too many auto-height rows to measure — scrolling may drift";
  }
}

// A column's width from the engine, cached per frame-independent lookup.

// Total growth of rows strictly before `row`.

// Effective offset of a row's top edge: what the engine says, plus the growth of
// every grown row above it.

// Inverse of `rowOffsetPx`. Growth is monotonic, so subtracting the growth above
// the current guess and re-asking the engine converges in a couple of steps.

// Absolute screen position of a column's left / row's top edge (any index).
// Freeze-aware screen position: frozen lines (index < fc/fr) are pinned and
// ignore the scroll offset; body lines subtract it. In the no-freeze case this
// equals screenX/screenY. Used where geometry must line up under frozen panes
// (merged-cell painting spans across cells, so it can't use per-cell geo).
// Drawn geometry wins where it exists: measure() grows rows for wrapped text
// and tall fonts (auto row height), which the engine's offsets know nothing
// about. Mixing the two made a merge drift from its own cells by the accumulated
// growth — and since that growth depends on which rows are in the window, the
// drift changed on every scroll step and the block appeared to slide over the
// grid. Engine offsets remain the fallback for lines outside the window.
// The trailing (right/bottom) edge of a line: from the drawn geometry when the
// line itself is drawn, else the start of the following line.
export const fscreenXEnd = (col) => (colXAt(col) !== undefined ? colXAt(col) + colWAt(col) : fscreenX(col + 1));
export const fscreenYEnd = (row) => (rowYAt(row) !== undefined ? rowYAt(row) + rowHAt(row) : fscreenY(row + 1));
// The merge covering (row,col), if any.
// Whether a merge intersects the current (effective) selection.

// The canvas font string for a cell (family + size from its style, or defaults).
// Font size in px: the cell's own size (pt→px at 96dpi), else the 11pt default
// the toolbar reports for an unstyled cell (kept in sync so "11" isn't a lie).
// Cache the CSS font stack per requested family. font_css_stack (wasm) routes a
// name through the shared substitution table (Calibri→Carlito, Arial→Liberation
// Sans, …) + the bundled @font-face fonts, so a cell's font renders as its
// metric-compatible face on every machine instead of silently falling back to
// the system font. Cached because draw() asks per cell, but families repeat.
export const _fontStackCache = new Map();
// The font families the toolbar picker offers, from the engine's substitution
// table (single source of truth) — every entry renders as a bundled face, so
// the list can never offer a family this build would have to guess at. Typed
// names outside the list still work; they go through the same table.
let _fontFamilies = null;
function fontFamilies() {
  if (!_fontFamilies) {
    try { _fontFamilies = JSON.parse(wasm.font_families()); } catch { _fontFamilies = []; }
  }
  return _fontFamilies;
}

// The font for one rich-text run: the run's own properties where it states
// them, the cell's where it does not. A run inherits rather than replaces —
// `<rPr>` carries only what differs, so treating an absent property as a reset
// would drop the cell's font on every partially-formatted string.

// Total width of a rich string, measured run by run — each run has its own
// font, so measuring the concatenated text with one font gives a width that is
// wrong wherever the runs differ, and the alignment then drifts.

// Draw a rich string starting at `x` on baseline `y`, returning the width used.
// Baseline y for a single line given the cell's vertical alignment.

// Word-wrap a cell's text to `maxW` px (hard-breaking over-long words).

// Measure the wrap, (re)build `geo` from engine sizes at the current pixel
// scroll, and return `{ w, h }`. Scrolling is fluid: `scrollX/scrollY` are
// absolute content offsets, so the first visible line can be partially clipped.
function measure() {
  syncHeaderMetrics();
  const rect = wrap.getBoundingClientRect();
  const v = { w: rect.width / state.zoom, h: rect.height / state.zoom };
  if (!wasm) {
    geo.colW = geo.colX = geo.colIdx = geo.rowH = geo.rowY = geo.rowIdx = [];
    geo.colOf = new Map(); geo.rowOf = new Map();
    geo.cols = geo.rows = 0;
    state.freeze = { fc: 0, fr: 0, bodyX0: HW, bodyY0: HH };
    return v;
  }
  // Frozen panes: the top `fr` rows and left `fc` columns stay pinned; the rest
  // scroll. The drawn line list is [frozen 0..fc-1 at fixed x] + [scrolling fsc..].
  const fz = JSON.parse(wasm.session_frozen(state.sheet));
  const fc = fz.cols, fr = fz.rows;
  const frozenW = fc > 0 ? wasm.session_col_offset_px(state.sheet, fc) : 0;
  const frozenH = fr > 0 ? rowOffsetPx(fr) : 0;
  const bodyX0 = HW + frozenW, bodyY0 = HH + frozenH;
  state.freeze = { fc, fr, bodyX0, bodyY0 };

  // First scrolling line + its sub-pixel clip. scrollX/Y move the scrolling
  // region, whose content origin is column `fc` / row `fr`.
  const absX = frozenW + state.scrollX;
  const fsc = Math.max(fc, wasm.session_col_at_px(state.sheet, Math.round(absX)));
  const subX = absX - wasm.session_col_offset_px(state.sheet, fsc);
  const absY = frozenH + state.scrollY;
  const fsr = Math.max(fr, rowAtPx(absY));
  const subY = absY - rowOffsetPx(fsr);
  state.firstCol = fsc;
  state.firstRow = fsr;

  const colCap = Math.max(4, Math.ceil((v.w - bodyX0) / MIN_LINE) + 2);
  const rowCap = Math.max(4, Math.ceil((v.h - bodyY0) / MIN_LINE) + 2);
  const frozenColW = fc > 0 ? JSON.parse(wasm.session_col_px(state.sheet, 0, fc)) : [];
  const scrollColW = JSON.parse(wasm.session_col_px(state.sheet, fsc, colCap));
  geo.colIdx = []; geo.colW = [];
  for (let c = 0; c < fc; c++) { geo.colIdx.push(c); geo.colW.push(frozenColW[c]); }
  for (let i = 0; i < scrollColW.length; i++) { geo.colIdx.push(fsc + i); geo.colW.push(scrollColW[i]); }
  const frozenRowH = fr > 0 ? JSON.parse(wasm.session_row_px(state.sheet, 0, fr)) : [];
  const scrollRowH = JSON.parse(wasm.session_row_px(state.sheet, fsr, rowCap));
  geo.rowIdx = []; geo.rowH = [];
  for (let r = 0; r < fr; r++) { geo.rowIdx.push(r); geo.rowH.push(frozenRowH[r]); }
  for (let i = 0; i < scrollRowH.length; i++) { geo.rowIdx.push(fsr + i); geo.rowH.push(scrollRowH[i]); }

  // Live resize preview: override affected line sizes (matched by real index).
  if (state.resize) {
    const rz = state.resize;
    const idxArr = rz.axis === "col" ? geo.colIdx : geo.rowIdx;
    const wArr = rz.axis === "col" ? geo.colW : geo.rowH;
    for (let i = 0; i < idxArr.length; i++) {
      const idx = idxArr[i];
      const hit = rz.scope === "all" || (rz.scope === "band" ? idx >= rz.b0 && idx <= rz.b1 : idx === rz.index);
      if (hit) wArr[i] = rz.previewPx;
    }
  }

  // **Trim the line lists to the window before anything is fetched for them.**
  //
  // `colCap`/`rowCap` are floored by `MIN_LINE = 8` because a line *can* be
  // 8px, so the cap has to assume they all are. On an ordinary sheet they are
  // not: a 1280x800 view of 100px columns asked the engine for 157 columns and
  // 73 rows — 2920 cells — to draw 13 by 29. Everything downstream then
  // measured, mapped and considered eight times the cells the frame could
  // show, and it is why the cost tracked visible columns (`PERF-D-01`): narrow
  // columns mean more of the fetch is real.
  //
  // The cap stays as it is — it is the safe over-estimate that makes one
  // engine call enough. This throws away what the sizes prove is off-screen.
  //
  // One line past the edge is kept deliberately: it is where a spilling label
  // clips, and what `colAtX`/`rowAtY` fall back to. Zero-width hidden lines do
  // not advance the cursor, so a run of hidden columns is crossed rather than
  // mistaken for the edge. Rows are trimmed on their *stored* heights, before
  // auto-height grows them — growth only makes a row taller, so this can
  // over-fetch and never under-fetch.
  const keepLines = (idx, sizes, origin, split, splitOrigin, limit, fallback) => {
    let p = origin;
    let keep = 0;
    for (let i = 0; i < idx.length; i++) {
      if (i === split) p = splitOrigin;
      keep = i + 1;
      if (p >= limit) break;
      p += sizes[i] ?? fallback;
    }
    return keep;
  };
  const keepC = keepLines(geo.colIdx, geo.colW, HW, fc, bodyX0 - subX, v.w, COL_W);
  const keepR = keepLines(geo.rowIdx, geo.rowH, HH, fr, bodyY0 - subY, v.h, ROW_H);
  geo.colIdx.length = keepC; geo.colW.length = keepC;
  geo.rowIdx.length = keepR; geo.rowH.length = keepR;

  // Index → slot maps (needed by the accessors and auto-height below).
  geo.colOf = new Map(); geo.colIdx.forEach((c, i) => geo.colOf.set(c, i));
  geo.rowOf = new Map(); geo.rowIdx.forEach((r, i) => geo.rowOf.set(r, i));

  // Fetch the visible cells once (covering the frozen bands), reused by draw,
  // and grow rows that contain wrapped text / tall fonts (auto row height).
  const lastRowIdx = geo.rowIdx[geo.rowIdx.length - 1] ?? fsr;
  const lastColIdx = geo.colIdx[geo.colIdx.length - 1] ?? fsc;
  const firstRowIdx = fr > 0 ? 0 : fsr;
  const firstColIdx = fc > 0 ? 0 : fsc;
  geoItems = JSON.parse(
    wasm.session_cells(state.sheet, firstRowIdx, firstColIdx, lastRowIdx, lastColIdx),
  );
  // Text spills across empty neighbours, so a label whose own cell is outside
  // the window can still be showing inside it — and only the *nearest*
  // populated cell on each side can, since anything beyond is blocked by it.
  // Without these a long label vanished the instant its own column scrolled
  // off, taking the visible half of the text with it. Excel keeps drawing it.
  //
  // Two calls at most, whatever the row count: everything between the furthest
  // owner and the window is empty by definition, so one window per side picks
  // up exactly the owners.
  try {
    const span = JSON.parse(
      wasm.session_spill_owners(state.sheet, firstRowIdx, lastRowIdx, firstColIdx, lastColIdx),
    );
    // **Reduce to the nearest populated cell per row, and filter after.**
    //
    // The band between the furthest owner and the window is not empty — only
    // the *columns outside the span* are — so this used to push every
    // populated cell in it, which on a wide sheet is unbounded and was hundreds
    // of items a frame.
    //
    // Filtering to spillable text *before* reducing was also wrong, and wrong
    // in a way that showed on screen: a blocking **number** between a far label
    // and the window never entered the set, so the spill scan ran straight
    // through the column that should have stopped it and drew the label into a
    // window Excel would not show it in. The nearest cell wins whatever it
    // holds; only then is it asked whether it can spill.
    const gather = (a, b, fromLeft) => {
      const nearest = new Map();
      for (const it of JSON.parse(
        wasm.session_cells(state.sheet, firstRowIdx, a, lastRowIdx, b),
      )) {
        const held = nearest.get(it.r);
        if (held === undefined || (fromLeft ? it.c > held.c : it.c < held.c)) {
          nearest.set(it.r, it);
        }
      }
      for (const it of nearest.values()) {
        // Only text can spill: a number too wide for its cell becomes `#`
        // inside it, and a wrapped or clipped cell stays put by definition.
        if (it.t && !it.n && !it.w && !it.cl && !it.shrink) geoItems.push(it);
      }
    };
    if (span.left !== null && firstColIdx > 0) gather(span.left, firstColIdx - 1, true);
    if (span.right !== null) gather(lastColIdx + 1, span.right, false);
  } catch { /* nothing outside the window is the common case */ }
  // Rows the workbook sized itself are pinned: auto-height must not override an
  // imported height (nor one the user dragged), exactly as Excel stops
  // auto-fitting a row once its height is set. Without this every styled row of
  // an opened file was silently re-heighted by the editor.
  // (geo.rowIdx is the frozen band followed by the scrolled body, so ask per
  // contiguous run rather than over the gap between them.)
  const pinnedRows = new Set();
  for (let i = 0; i < geo.rowIdx.length; ) {
    let j = i;
    while (j + 1 < geo.rowIdx.length && geo.rowIdx[j + 1] === geo.rowIdx[j] + 1) j++;
    const flags = JSON.parse(wasm.session_row_pinned(state.sheet, geo.rowIdx[i], j - i + 1));
    flags.forEach((p, k) => { if (p) pinnedRows.add(geo.rowIdx[i] + k); });
    i = j + 1;
  }
  for (const it of geoItems) {
    if (!it.t) continue;
    const ci = geo.colOf.get(it.c), ri = geo.rowOf.get(it.r);
    if (ci === undefined || ri === undefined) continue;
    // A hidden row is 0 px — growing it would make it reappear.
    if (geo.rowH[ri] <= 0 || pinnedRows.has(it.r)) continue;
    const needed = neededRowHeight(it, geo.colW[ci]);
    if (needed !== null && needed > geo.rowH[ri]) geo.rowH[ri] = needed;
  }

  // Positions: frozen lines from HW/HH, scrolling lines from bodyX0/Y0 − sub.
  geo.colX = new Array(geo.colIdx.length);
  let x = HW; geo.cols = 0;
  for (let i = 0; i < geo.colIdx.length; i++) {
    if (i === fc) x = bodyX0 - subX;
    geo.colX[i] = x;
    if (x < v.w) geo.cols = i + 1;
    x += geo.colW[i] ?? COL_W; // hidden lines are 0 — must NOT fall back to COL_W
  }
  geo.rowY = new Array(geo.rowIdx.length);
  let y = HH; geo.rows = 0;
  for (let i = 0; i < geo.rowIdx.length; i++) {
    if (i === fr) y = bodyY0 - subY;
    geo.rowY[i] = y;
    if (y < v.h) geo.rows = i + 1;
    y += geo.rowH[i] ?? ROW_H; // hidden lines are 0 — must NOT fall back to ROW_H
  }
  return v;
}

// Pixel width for an OOXML border line-style token.

// Draw one cell-border edge from a "style:color" spec (color may be empty).

// Size + position the custom scrollbar thumbs from the current scroll and the
// used extent (plus a buffer so you can always scroll a little past the data).
export let scrollMeta = { maxScrollY: 1, maxScrollX: 1, vSpan: 1, hSpan: 1 };

// Hold the scroll inside the content extent. Wheeling or paging past the end
// used to leave the grid parked in blank space with no way back but scrolling
// the other way — the thumb had already bottomed out, so it gave no hint that
// anything had moved.
// The scrollbar tracks' own size, which changes with the window and nothing
// else. Dropped by `invalidateTrackSize` rather than re-read every frame.
const trackSize = { h: 0, w: 0 };
export function invalidateTrackSize() { trackSize.h = 0; trackSize.w = 0; }

function updateScrollbars(v) {
  if (!wasm) return;
  const b = usedBounds();
  const viewH = v.h - HH, viewW = v.w - HW;
  // A fixed buffer past the data, not one measured from the current scroll.
  // Including `scrollY` made the extent grow as you scrolled, so the end could
  // never be reached: scrolling past the data kept going forever, and every one
  // of those frames did the geometry work again. That is the drag past the last
  // row, and it is also why `clampScroll` had nothing to clamp to.
  const contentH = Math.max(rowOffsetPx(b.rows + 30), viewH + 1);
  const contentW = Math.max(
    wasm.session_col_offset_px(state.sheet, b.cols + 8),
    viewW + 1,
  );
  // Read once and remembered. `clientHeight` on an element whose layout was
  // just dirtied forces a synchronous reflow, and this ran every frame — so the
  // scrollbar paid to flush the whole editor's layout to learn a number that
  // only changes when the window does.
  if (trackSize.h === 0) {
    trackSize.h = vscroll.clientHeight;
    trackSize.w = hscroll.clientWidth;
  }
  const trackH = trackSize.h, trackW = trackSize.w;
  const thumbH = Math.max(28, trackH * Math.min(1, viewH / contentH));
  const thumbW = Math.max(28, trackW * Math.min(1, viewW / contentW));
  const maxScrollY = Math.max(1, contentH - viewH);
  const maxScrollX = Math.max(1, contentW - viewW);
  const vSpan = Math.max(1, trackH - thumbH), hSpan = Math.max(1, trackW - thumbW);
  vthumb.style.height = thumbH + "px";
  vthumb.style.top = Math.min(vSpan, (state.scrollY / maxScrollY) * vSpan) + "px";
  hthumb.style.width = thumbW + "px";
  hthumb.style.left = Math.min(hSpan, (state.scrollX / maxScrollX) * hSpan) + "px";
  // Hide a scrollbar when there's nothing to scroll on that axis.
  vscroll.style.display = contentH > viewH + 1 ? "block" : "none";
  hscroll.style.display = contentW > viewW + 1 ? "block" : "none";
  scrollMeta = { maxScrollY, maxScrollX, vSpan, hSpan };
}

// A header boundary under the pointer, if any (for resize hit-testing).

// Screen position/size of a drawn column/row (or default/undefined if not drawn).
export const colWAt = (col) => (geo.colOf.has(col) ? geo.colW[geo.colOf.get(col)] : COL_W);
export const rowHAt = (row) => (geo.rowOf.has(row) ? geo.rowH[geo.rowOf.get(row)] : ROW_H);
export const colXAt = (col) => (geo.colOf.has(col) ? geo.colX[geo.colOf.get(col)] : undefined);
export const rowYAt = (row) => (geo.rowOf.has(row) ? geo.rowY[geo.rowOf.get(row)] : undefined);
export const firstDrawnCol = () => geo.colIdx[0] ?? 0;
export const firstDrawnRow = () => geo.rowIdx[0] ?? 0;
// The first drawn line of the *scrolling body* — past the frozen band, which
// occupies the first fc/fr entries of colIdx/rowIdx.
export const firstBodyCol = () => geo.colIdx[state.freeze.fc] ?? state.firstCol;
export const firstBodyRow = () => geo.rowIdx[state.freeze.fr] ?? state.firstRow;

// The clipped [x, x+w) pixel span covering columns c0..c1, kept inside the pane
// those columns belong to. Both limits are pane-relative, which matters twice
// over when something is frozen: with fractional scroll the first body column
// is drawn a sliver *behind* the freeze line (so an unclamped span paints a
// strip into the frozen band that slides as you scroll), and a range whose
// start has scrolled out must clamp to the body's left edge rather than
// collapse to zero width (which made the whole selection disappear).


// The clip rect of the quadrant a cell belongs to (whole body when no freeze).

// --- App-header collapse ----------------------------------------------------
// The header bar (branding, status, Open, Settings) is 52px of chrome that a
// user working in a large sheet may not want. Collapsing hands that space to
// the grid; the toggle stays in the menu bar, which is never hidden, so there
// is always a way back. Remembered across sessions — it is a workspace
// preference, not a property of the document.
const HEADER_COLLAPSE_KEY = "oc.headerCollapsed";
let headerCollapsed = false;
/// Is this cell inside the current selection — the anchor range or any banked
/// range of a multi-range selection (`UX-SEL-06`)?
///
/// A whole-column or whole-row selection is expressed as a rect spanning the
/// sheet, so this needs no special case for either.
function cellIsSelected(r, c) {
  const a = selRect();
  if (r >= a.r0 && r <= a.r1 && c >= a.c0 && c <= a.c1) return true;
  for (const rg of state.ranges) {
    if (r >= rg.r0 && r <= rg.r1 && c >= rg.c0 && c <= rg.c1) return true;
  }
  return false;
}

function setHeaderCollapsed(collapsed) {
  headerCollapsed = collapsed;
  const hdr = qs(".app-header");
  const btn = byId("hdr-collapse");
  // A class, not the `hidden` attribute: `.app-header { display: flex }` is a
  // stronger rule than the UA's `[hidden] { display: none }`, so the attribute
  // alone leaves the bar on screen.
  if (hdr) hdr.classList.toggle("collapsed", collapsed);
  if (btn) {
    btn.classList.toggle("collapsed", collapsed);
    btn.setAttribute("aria-expanded", collapsed ? "false" : "true");
    const label = collapsed ? "Show the page header" : "Hide the page header";
    btn.title = label;
    btn.setAttribute("aria-label", label);
  }
  try { localStorage.setItem(HEADER_COLLAPSE_KEY, collapsed ? "1" : "0"); } catch {}
  resize(); // the canvas re-fits into (or out of) the reclaimed space
}

// Set the magnification and re-fit the canvas. Clamped to 25–200%, Excel's
// range; 100% is exact so Ctrl+Alt+0 always lands back on crisp text. (Plain
// Ctrl+0 is Excel's hide-column and is bound that way here.)


export function draw() {
  const v = measure();
  ctx.clearRect(0, 0, v.w, v.h);
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, v.w, v.h);

  const rectSel = selRect();
  // The highlighted rectangle spans the whole row/column/sheet for header/corner
  // selections, and stays clamped to the visible body otherwise.
  let sX, sY;
  if (state.selKind === "all") {
    sX = { x: HW, w: v.w - HW };
    sY = { y: HH, h: v.h - HH };
  } else if (state.selKind === "rows") {
    sX = { x: HW, w: v.w - HW };
    sY = spanY(rectSel.r0, rectSel.r1, v);
  } else if (state.selKind === "cols") {
    sX = spanX(rectSel.c0, rectSel.c1, v);
    sY = { y: HH, h: v.h - HH };
  } else {
    sX = spanX(rectSel.c0, rectSel.c1, v);
    sY = spanY(rectSel.r0, rectSel.r1, v);
  }

  // The pane quadrants (one whole body when nothing is frozen). Selection tint,
  // gridlines and cells are each drawn clipped to their quadrant so frozen and
  // scrolling panes never bleed into one another.
  const F = state.freeze;
  // geo index where body lines begin: the first `fc`/`fr` entries of colIdx/
  // rowIdx are the frozen lines, the rest are the scrolling body.
  const nCol = geo.colX.length, nRow = geo.rowY.length;
  const splitC = Math.min(F.fc, nCol), splitR = Math.min(F.fr, nRow);
  // Each quadrant carries the geo line-index ranges it owns, so gridlines are
  // drawn per pane. This matters because with fractional scroll the first body
  // line sits at bodyX0/Y0 − sub (just inside the frozen band's clip); gating
  // by index keeps that stray line out of the frozen panes.
  const quads = F.fc || F.fr
    ? [
        { x: HW, y: HH, w: F.bodyX0 - HW, h: F.bodyY0 - HH, ci0: 0, ci1: splitC, ri0: 0, ri1: splitR },
        { x: F.bodyX0, y: HH, w: v.w - F.bodyX0, h: F.bodyY0 - HH, ci0: splitC, ci1: nCol, ri0: 0, ri1: splitR },
        { x: HW, y: F.bodyY0, w: F.bodyX0 - HW, h: v.h - F.bodyY0, ci0: 0, ci1: splitC, ri0: splitR, ri1: nRow },
        { x: F.bodyX0, y: F.bodyY0, w: v.w - F.bodyX0, h: v.h - F.bodyY0, ci0: splitC, ci1: nCol, ri0: splitR, ri1: nRow },
      ].filter((q) => q.w > 0 && q.h > 0)
    : [{ x: HW, y: HH, w: Math.max(0, v.w - HW), h: Math.max(0, v.h - HH), ci0: 0, ci1: nCol, ri0: 0, ri1: nRow }];

  // Selection tint + gridlines, per quadrant. Grid lines can be hidden per
  // sheet (Excel's showGridLines="0") — the selection tint still paints.
  let gridHidden = false;
  try { gridHidden = wasm && wasm.session_gridlines_hidden(state.sheet); } catch {}
  for (const q of quads) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(q.x, q.y, q.w, q.h);
    ctx.clip();
    // **The tint is not painted here any more** (`UX-SEL-06`). It used to be,
    // and every opaque fill drawn afterwards erased it: table shading, banded
    // rows, conditional formatting, any cell with a background. Selecting a
    // column tinted the empty cells and none of the filled ones, so in a table
    // the only surviving mark was the active cell's outline — reported exactly
    // that way. It is painted below instead, after the fills. This pass keeps
    // the gridlines, which must stay under everything.
    if (gridHidden) { ctx.restore(); continue; }
    ctx.strokeStyle = colors.grid;
    ctx.lineWidth = 1;
    ctx.beginPath();
    // Vertical lines: the columns this quadrant owns, spanning its height. Skip
    // the trailing line when it is the body's fractional left edge (ci1===fc),
    // which the freeze divider covers instead — this is what stopped the stray
    // gridline bleeding into a frozen pane.
    for (let i = q.ci0; i <= q.ci1; i++) {
      if (i === q.ci1 && q.ci1 === splitC && (F.fc || F.fr)) continue;
      const x = Math.floor(i < geo.colX.length ? geo.colX[i] : v.w) + 0.5;
      ctx.moveTo(x, q.y);
      ctx.lineTo(x, q.y + q.h);
    }
    for (let i = q.ri0; i <= q.ri1; i++) {
      if (i === q.ri1 && q.ri1 === splitR && (F.fc || F.fr)) continue;
      const y = Math.floor(i < geo.rowY.length ? geo.rowY[i] : v.h) + 0.5;
      ctx.moveTo(q.x, y);
      ctx.lineTo(q.x + q.w, y);
    }
    ctx.stroke();
    ctx.restore();
  }

  // Outer body clip (keeps content out of the header strips). Cells additionally
  // clip to their own quadrant below.
  ctx.save();
  ctx.beginPath();
  ctx.rect(HW, HH, Math.max(0, v.w - HW), Math.max(0, v.h - HH));
  ctx.clip();

  // Cell fills + text (fetched in measure(), reused here).
  const lastCol = (geo.colIdx[geo.colIdx.length - 1] ?? state.firstCol) + 1;
  const items = geoItems;
  sheetMerges = wasm ? JSON.parse(wasm.session_merges(state.sheet)) : [];

  // Clip a cell's drawing to its pane quadrant (a no-op when nothing is frozen).
  const frozen = F.fc || F.fr;
  const withQuad = (row, col, fn) => {
    if (!frozen) { fn(); return; }
    const q = quadClip(row, col, v);
    ctx.save();
    ctx.beginPath();
    ctx.rect(q.x, q.y, q.w, q.h);
    ctx.clip();
    fn();
    ctx.restore();
  };
  // Draw once per pane, clipped to it — for overlays (selection outline, copy
  // marching ants, drag-fill preview) whose rect may legitimately span more
  // than one pane, so clipping to a single cell's quadrant would truncate them
  // while not clipping at all lets them stroke across the frozen bands.
  const perQuad = (fn) => {
    if (!frozen) { fn(); return; }
    for (const q of quads) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(q.x, q.y, q.w, q.h);
      ctx.clip();
      fn();
      ctx.restore();
    }
  };
  // A merge that straddles a freeze line is not one rectangle but one per pane:
  // its frozen half is pinned while its body half scrolls, so a single rect
  // built from a pinned left edge and a scrolling right edge is wrong the
  // moment they disagree — and once the body half scrolls out it goes
  // *negative*, which canvas draws as a block flipped back over the frozen
  // pane, sliding with every wheel tick. Each entry here is one pane's slice:
  // its clip, its own geometry, and whether it holds the merge's anchor cell
  // (the half that carries the text).
  function mergeSlices(m) {
    const clipRect = { x: HW, y: HH, w: v.w - HW, h: v.h - HH };
    if (!frozen) {
      return [{
        clip: clipRect, anchor: true,
        x: fscreenX(m.c0), y: fscreenY(m.r0),
        w: fscreenXEnd(m.c1) - fscreenX(m.c0), h: fscreenYEnd(m.r1) - fscreenY(m.r0),
      }];
    }
    // Per axis: the frozen half spans c0…fc-1 (clamped at the freeze line), the
    // body half spans max(c0,fc)…c1. Each is measured in its own pane's frame.
    const xs = [], ys = [];
    if (m.c0 < F.fc) {
      const x = fscreenX(m.c0);
      xs.push({ x, w: Math.min(fscreenXEnd(Math.min(m.c1, F.fc - 1)), F.bodyX0) - x, c0: HW, c1: F.bodyX0, anchor: true });
    }
    if (m.c1 >= F.fc) {
      const x = fscreenX(Math.max(m.c0, F.fc));
      xs.push({ x, w: fscreenXEnd(m.c1) - x, c0: F.bodyX0, c1: v.w, anchor: m.c0 >= F.fc });
    }
    if (m.r0 < F.fr) {
      const y = fscreenY(m.r0);
      ys.push({ y, h: Math.min(fscreenYEnd(Math.min(m.r1, F.fr - 1)), F.bodyY0) - y, r0: HH, r1: F.bodyY0, anchor: true });
    }
    if (m.r1 >= F.fr) {
      const y = fscreenY(Math.max(m.r0, F.fr));
      ys.push({ y, h: fscreenYEnd(m.r1) - y, r0: F.bodyY0, r1: v.h, anchor: m.r0 >= F.fr });
    }
    const out = [];
    for (const sx of xs) {
      for (const sy of ys) {
        if (sx.w <= 0 || sy.h <= 0 || sx.c1 <= sx.c0 || sy.r1 <= sy.r0) continue;
        out.push({
          clip: { x: sx.c0, y: sy.r0, w: sx.c1 - sx.c0, h: sy.r1 - sy.r0 },
          anchor: sx.anchor && sy.anchor,
          x: sx.x, y: sy.y, w: sx.w, h: sy.h,
        });
      }
    }
    return out;
  }
  const withSliceClip = (s, fn) => {
    ctx.save();
    ctx.beginPath();
    ctx.rect(s.clip.x, s.clip.y, s.clip.w, s.clip.h);
    ctx.clip();
    fn();
    ctx.restore();
  };

  // Charts, drawn over the grid at their anchors. Read-only: the engine parses
  // the chart part for display and still writes it back from its own bytes, so
  // nothing here can change the file.
  drawImages();
  drawCharts(withQuad);

  // Manual page breaks, as the dashed rules Excel draws. A break that is only
  // visible once you print is a break you set by accident and never find.
  if (wasm) {
    let brk = { rows: [], cols: [] };
    try { brk = JSON.parse(wasm.session_page_breaks(state.sheet)); } catch {}
    if (brk.rows.length || brk.cols.length) {
      ctx.save();
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([6, 4]);
      for (const r of brk.rows) {
        const y = rowYAt(r);
        if (y === undefined) continue;
        ctx.beginPath();
        ctx.moveTo(HW, y + 0.5);
        ctx.lineTo(canvas.clientWidth, y + 0.5);
        ctx.stroke();
      }
      for (const c of brk.cols) {
        const x = colXAt(c);
        if (x === undefined) continue;
        ctx.beginPath();
        ctx.moveTo(x + 0.5, HH);
        ctx.lineTo(x + 0.5, canvas.clientHeight);
        ctx.stroke();
      }
      ctx.restore();
    }
  }

  // Tables: header shading and banded rows.
  //
  // This has to run before the cell pass, not after it: the fills are opaque,
  // and painting them later covers the text. The block used to sit near the end
  // of draw() with a comment claiming otherwise — it got away with it only
  // because the two colours it used were 16%-alpha washes.
  // Before the cell pass, because the *text* consults it: a header cell in a
  // filter range has to keep the arrow's width clear, and reading this after
  // the labels were laid out would have measured them against the previous
  // frame's filters — wrong for exactly one frame, which is the frame in which
  // a table is created.
  refreshFilterInfo();
  tablesInView = [];
  if (wasm) {
    try { tablesInView = JSON.parse(wasm.session_tables(state.sheet)); } catch {}
    for (const t of tablesInView) {
      // Colours come from the table's own style name resolved against the
      // workbook theme, not from constants here: a file whose author chose a
      // green style has to open green.
      //
      // The body is filled too, not just the bands. A table is a light block in
      // Excel whatever the application theme is, and painting only the bands
      // left light stripes across a dark grid with unreadable text on them.
      for (let r = t.r0; r <= t.r1; r++) {
        const ry = rowYAt(r);
        if (ry === undefined) continue;
        const rh = rowHAt(r);
        const isHeader = t.headers > 0 && r === t.r0;
        const isTotals = t.totals > 0 && r === t.r1;
        // Bands count from the first *data* row, so a header does not shift
        // the stripe pattern by one and make the first data row look banded
        // when it should not be.
        const dataIndex = r - t.r0 - (t.headers > 0 ? 1 : 0);
        const banded = t.stripes && !isHeader && !isTotals && dataIndex % 2 === 1;
        for (let c = t.c0; c <= t.c1; c++) {
          const cx = colXAt(c);
          if (cx === undefined) continue;
          const cw = colWAt(c);
          // A column stripe alternates on top of the row banding, and an
          // emphasised first/last column reads as banded too.
          const colBanded = t.colStripes && (c - t.c0) % 2 === 1;
          const emphasised = (t.firstCol && c === t.c0) || (t.lastCol && c === t.c1);
          const fill = (isHeader || isTotals)
            ? "#" + t.headerFill
            : (banded || colBanded || emphasised) ? "#" + t.bandFill : "#" + t.bodyFill;
          withQuad(r, c, () => { ctx.fillStyle = fill; ctx.fillRect(cx, ry, cw, rh); });
        }
        // The rule under the header is what a Light style has instead of a
        // fill, so it is drawn for every family rather than only where the
        // header is coloured.
        if (isHeader || isTotals) {
          const y = isHeader ? ry + rh - 1 : ry;
          for (let c = t.c0; c <= t.c1; c++) {
            const cx = colXAt(c);
            if (cx === undefined) continue;
            withQuad(r, c, () => {
              ctx.fillStyle = "#" + t.border;
              ctx.fillRect(cx, y, colWAt(c), 1);
            });
          }
        }
      }
    }
  }

  // Data bars sit behind the value: drawn after the cell fills so they read as
  // part of the cell, before the text so they never obscure it.
  ctx.textBaseline = "middle";
  // **The geometry comes from the engine, not from here.** These numbers used
  // to be written out twice — once in `casual-calc-render` and once in this
  // file — and agreed only because somebody had copied them across, which is
  // the two-renderers-can-disagree shape `conditional.rs` argues against,
  // reintroduced one layer up (`RND-08`). The canvas still paints; it no longer
  // decides.
  const bar = dataBarStyle();
  for (const it of items) {
    if (it.bar === undefined) continue;
    const bx = colXAt(it.c), by = rowYAt(it.r);
    if (bx === undefined || by === undefined) continue;
    const bw = Math.max(0, (colWAt(it.c) - 2 * bar.padX) * it.bar);
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#" + (it.barc || bar.defaultColor);
      ctx.globalAlpha = bar.alpha;
      ctx.fillRect(bx + bar.padX, by + bar.padY, bw, rowHAt(it.r) - 2 * bar.padY);
      ctx.globalAlpha = 1;
    });
  }
  for (const it of items) {
    if (!it.bg && !it.grad) continue;
    const x = colXAt(it.c);
    const y = rowYAt(it.r);
    if (x === undefined || y === undefined) continue;
    const w = colWAt(it.c) - 1, h = rowHAt(it.r) - 1;
    withQuad(it.r, it.c, () => {
      if (it.grad) {
        // A gradient replaces the fill rather than joining it: `<fill>` holds
        // a pattern or a gradient, never both. The angle is measured from the
        // horizontal, as OOXML states it.
        const rad = ((it.grad.deg || 0) * Math.PI) / 180;
        const g = ctx.createLinearGradient(
          x + 1, y + 1,
          x + 1 + Math.cos(rad) * w, y + 1 + Math.sin(rad) * h,
        );
        for (const stop of it.grad.stops) {
          g.addColorStop(Math.min(1, Math.max(0, stop.p)), "#" + stop.c);
        }
        ctx.fillStyle = g;
        ctx.fillRect(x + 1, y + 1, w, h);
        return;
      }
      if (it.pat) {
        // A pattern's *background* fills the cell and its foreground draws the
        // motif on top. Painting only the foreground — as a solid — is what
        // made every patterned cell look like a flat block of the wrong
        // colour. The motifs are approximated by density rather than matched
        // hatch for hatch: the alternative is eighteen bitmaps for something
        // almost no sheet uses, and a wrong-density hatch still reads as a
        // hatch.
        ctx.fillStyle = it.bg2 ? "#" + it.bg2 : colors.bg;
        ctx.fillRect(x + 1, y + 1, w, h);
        const density = { gray125: 0.125, gray0625: 0.0625, lightGray: 0.25,
          mediumGray: 0.5, darkGray: 0.75, lightGrid: 0.25, lightTrellis: 0.3,
          darkGrid: 0.6, darkTrellis: 0.65 }[it.pat] ?? 0.4;
        ctx.save();
        ctx.globalAlpha = density;
        ctx.fillStyle = "#" + it.bg;
        ctx.fillRect(x + 1, y + 1, w, h);
        ctx.restore();
        return;
      }
      ctx.fillStyle = "#" + it.bg;
      ctx.fillRect(x + 1, y + 1, w, h);
    });
  }
  // **The selection tint, over everything that paints a background**
  // (`UX-SEL-06`).
  //
  // Below the table shading and the cell fills, and above nothing else — the
  // text pass runs after this, so a selected cell's value stays as readable as
  // an unselected one's.
  //
  // `--oc-selection-color` is translucent by design (`rgba(…, .10)`), which is
  // what makes one pass serve both cases: over white it composites to exactly
  // the tint this used to draw, and over a table's fill it reads as the same
  // selection rather than replacing the fill.
  if (state.selKind !== "none") {
    for (const q of quads) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(q.x, q.y, q.w, q.h);
      ctx.clip();
      ctx.fillStyle = colors.sel;
      ctx.fillRect(sX.x, sY.y, sX.w, sY.h);
      // Banked ranges of a multi-range selection get the same tint.
      for (const rg of state.ranges) {
        const ex = spanX(rg.c0, rg.c1, v), ey = spanY(rg.r0, rg.r1, v);
        ctx.fillRect(ex.x, ey.y, ex.w, ey.h);
      }
      ctx.restore();
    }
  }

  // The merges touching each drawn row. Built once per frame so the text pass
  // can ask "is this cell inside a merge?" with a lookup over the one or two
  // merges on its row, instead of scanning the whole sheet's merge list for
  // every cell it paints (which made draw() O(visible cells × total merges),
  // on the unthrottled wheel-scroll path).
  const mergesByRow = new Map();
  for (const m of sheetMerges) {
    for (const r of geo.rowIdx) {
      if (r < m.r0 || r > m.r1) continue;
      const list = mergesByRow.get(r);
      if (list) list.push(m);
      else mergesByRow.set(r, [m]);
    }
  }
  const drawnMergeAt = (r, c) => mergesByRow.get(r)?.find((m) => c >= m.c0 && c <= m.c1);

  // Cells that hold text — these block a neighbor's overflow. A merged block is
  // occupied over its whole span (not just its anchor), or a neighbour's text
  // would spill across it.
  const occupied = new Set();
  for (const it of items) if (it.t) occupied.add(it.r + "," + it.c);
  for (const [r, list] of mergesByRow) {
    for (const c of geo.colIdx) {
      if (list.some((m) => c >= m.c0 && c <= m.c1)) occupied.add(r + "," + c);
    }
  }
  // Cells carrying centre-across, so a label can find the extent of its run.
  const contCols = new Set();
  for (const it of items) if (it.a === "cont") contCols.add(it.r + "," + it.c);
  for (const it of items) {
    if (!it.t) continue;
    // Merged text belongs to the merge pass below, which lays it out across the
    // whole block. Drawing it here too anchors right/centre alignment to the
    // *anchor cell* instead, leaving a fragment outside the block that the
    // merge pass never covers — a ghost of the label beside it.
    if (drawnMergeAt(it.r, it.c)) continue;
    // The row must be in the window, but the *column* need not: a spill owner
    // fetched from outside it is here precisely so its text can reach in, and
    // `colXAt` has no answer for a column that was never drawn.
    const yTop = rowYAt(it.r);
    if (yTop === undefined) continue;
    const drawnHere = geo.colOf.has(it.c);
    if (!drawnHere && (it.w || it.cl || it.shrink)) continue; // cannot spill
    const x = drawnHere ? colXAt(it.c) : fscreenX(it.c);
    const cellW = drawnHere ? colWAt(it.c) : fscreenXEnd(it.c) - x;
    // **A filter arrow is part of the cell, so the label does not get all of
    // it.** `drawFilterRegion` puts the arrow in the right-hand
    // `FILTER_ARROW_W` of every header cell in a filter range, and the text
    // pass laid the label out against the whole cell — so a label wider than
    // what was left was drawn *underneath* its own control ("Revenue" reading
    // as "Revenu", with the arrow on the last letter). Every use of the cell's
    // width below is the drawable width instead: the fit test, the spill scan,
    // shrink-to-fit, the clip, and right/centre alignment. Zero everywhere
    // else, so nothing but a filter header changes.
    const arrowW = filterArrowReserve(it.r, it.c, cellW);
    const w = Math.max(0, cellW - arrowW);
    const h = rowHAt(it.r);
    const y = textY(it, yTop, h, cellLineH(it));
    ctx.font = cellFont(it);
    // `it.a` carries the OOXML mode, not just an edge: fill, justify,
    // centre-across and distributed all need their own layout. `align` is the
    // edge each of them starts from.
    const mode = it.a;
    const align =
      mode === "r" ? "right" : mode === "c" || mode === "cont" ? "center" : "left";

    // Wrapped cells: multi-line, clipped to the (auto-grown) cell — no overflow.
    if (it.w) {
      const lh = cellLineH(it);
      const lines = wrapLines(it, w - 8);
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.font = cellFont(it);
      ctx.fillStyle = cellFg(it);
      // Indent shifts the text off its leading edge (Excel: ~3 space-widths
      // per level), so an indented label lines up under its parent.
      const ind = (it.in || 0) * INDENT_PX;
      const tx = align === "right" ? x + w - 5 - ind : align === "center" ? x + w / 2 : x + 5 + ind;
      ctx.textAlign = align;
      const block = lines.length * lh;
      // Vertical justify/distribute spread the lines over the cell's height
      // instead of stacking them at one edge. Justify puts the first line hard
      // at the top and the last hard at the bottom; distribute leaves an equal
      // gap outside them too.
      const spread = it.va === "vj" || it.va === "vd";
      const slack = Math.max(0, h - 6 - block);
      const gaps = it.va === "vd" ? lines.length + 1 : Math.max(1, lines.length - 1);
      const step = spread && lines.length > 1 ? slack / gaps : 0;
      let ly =
        (spread
          ? yTop + 3 + (it.va === "vd" ? step : 0)
          : it.va === "t"
            ? yTop + 3
            : it.va === "b"
              ? yTop + h - block - 3
              : yTop + Math.max(0, (h - block) / 2)) + lh / 2;
      const stretch = mode === "just" || mode === "dist";
      lines.forEach((ln, i) => {
        // Justify stretches every line but the last; distributed stretches that
        // one too, which is the only difference between the two.
        const last = i === lines.length - 1;
        if (stretch && !(last && mode === "just")) {
          drawStretched(ln, x + 5 + ind, w - 10 - ind, ly);
        } else {
          ctx.fillText(ln, tx, ly);
        }
        ly += lh + step;
      });
      ctx.restore();
      continue;
    }
    // Rotated text: draw it under a transform anchored at the cell's centre and
    // clipped to the cell. OOXML's encoding is 0-90 counter-clockwise, 91-180 for
    // (value - 90) clockwise, and 255 for letters stacked without rotating.
    if (it.rot) {
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.fillStyle = cellFg(it);
      ctx.textAlign = "center";
      if (it.rot === 255) {
        // Stacked: one glyph per line, upright.
        const lh = cellLineH(it);
        const chars = [...String(it.t)];
        let sy = yTop + lh / 2 + 2;
        for (const ch of chars) {
          if (sy > yTop + h) break;
          ctx.fillText(ch, x + w / 2, sy);
          sy += lh;
        }
      } else {
        const deg = it.rot <= 90 ? -it.rot : it.rot - 90;
        ctx.translate(x + w / 2, yTop + h / 2);
        ctx.rotate((deg * Math.PI) / 180);
        ctx.fillText(String(it.t), 0, 0);
      }
      ctx.restore();
      continue;
    }
    // "Fill" repeats the text until the cell is full — Excel's separator-row
    // idiom. Repeating is the mode, so it happens before any overflow scan.
    if (mode === "fill") {
      const unit = textWidth(ctx.font, String(it.t));
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, w, h);
      ctx.clip();
      ctx.fillStyle = cellFg(it);
      ctx.textAlign = "left";
      if (unit > 0.5) {
        // Guarded on a real width: a zero-width string would loop forever.
        for (let fx = x + 5; fx < x + w; fx += unit) ctx.fillText(String(it.t), fx, y);
      }
      ctx.restore();
      continue;
    }
    // "Center across selection": centre over this cell plus the run of empty
    // cells to its right. It looks like a merge but merges nothing, so the
    // cells underneath stay individually addressable.
    if (mode === "cont") {
      // The span is the run of *cells that also carry the mode* — that is how
      // OOXML encodes the group, one `centerContinuous` per cell. Centring over
      // every empty neighbour instead would fling a lone label into the middle
      // of the viewport.
      let last = it.c;
      const inFroz = F.fc > 0 && it.c < F.fc;
      const hi = inFroz ? F.fc : lastCol;
      while (last + 1 < hi && geo.colOf.has(last + 1) && contCols.has(it.r + "," + (last + 1))) last += 1;
      const spanR = colXAt(last) + colWAt(last);
      ctx.save();
      if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
      ctx.beginPath();
      ctx.rect(x, yTop, spanR - x, h);
      ctx.clip();
      ctx.fillStyle = cellFg(it);
      ctx.textAlign = "center";
      ctx.fillText(String(it.t), (x + spanR) / 2, y);
      ctx.restore();
      continue;
    }
    // Superscript / subscript on the *cell* font: drawn smaller and offset,
    // and applied before measuring so the spill scan and alignment use the size
    // actually drawn rather than the nominal one.
    let supShift = 0;
    if (it.sup) {
      const px = Math.max(6, cellPx(it) * 0.72);
      const weight = it.b ? "600 " : "";
      const slant = it.i ? "italic " : "";
      ctx.font = `${slant}${weight}${px}px ${fontStack(it.fn)}`;
      supShift = it.sup === "superscript" ? -cellPx(it) * 0.32 : cellPx(it) * 0.18;
    }
    let text = it.t;
    // Rich text is measured run by run: each has its own font, so measuring
    // the concatenation with the cell's font gives a width that is wrong
    // wherever they differ — and the spill scan below would then borrow the
    // wrong number of neighbouring columns.
    let tw = it.runs ? runsWidth(it) : textWidth(ctx.font, text);

    // Text overflows across adjacent EMPTY cells (Excel behavior). Extend the
    // clip rectangle left/right over blank neighbours until the text fits or a
    // non-empty cell blocks it.
    //
    // The scan stays inside the cell's own pane: a frozen cell can't borrow the
    // body's columns (they scroll out from under it) and a body cell can't
    // reach back under the frozen band. Bounding by index rather than leaving
    // it to the clip is what makes the *extent* right — the text is then cut at
    // the real boundary instead of being laid out as if the pane went on.
    const inFrozenCols = F.fc > 0 && it.c < F.fc;
    const spillLo = inFrozenCols ? 0 : Math.max(state.firstCol, F.fc);
    const spillHi = inFrozenCols ? F.fc : lastCol; // exclusive
    let clipL = x, clipR = x + w;
    // A header label that has been shortened says so.
    //
    // The boundary here is a *control*, not the cell's edge, and that is what
    // makes a hard clip the wrong choice in this one place: a reader cannot
    // tell a truncated label from one the arrow is sitting on top of, which is
    // the exact confusion being fixed. The ellipsis is the mark that
    // distinguishes them. Everywhere else a clipped cell still simply clips —
    // there the cell edge is itself the signal, and Excel does the same.
    if (arrowW && !it.runs && tw > w - 8) {
      const room = Math.max(0, w - 8);
      // One estimate from the width already measured, then a walk. Header
      // labels are short, and this runs only for the ones that do not fit, so
      // it costs a handful of measurements on a frame with a table on it.
      let cut = Math.min(text.length, Math.max(0, Math.floor((room / tw) * text.length)));
      while (cut > 0 && ctx.measureText(text.slice(0, cut) + "…").width > room) cut -= 1;
      while (cut < text.length && ctx.measureText(text.slice(0, cut + 1) + "…").width <= room) cut += 1;
      text = cut > 0 ? text.slice(0, cut) + "…" : "";
      tw = ctx.measureText(text).width;
    }
    // A number never spills, not even into an empty neighbour: Excel fills the
    // cell with "#" instead, because a number cut off mid-digits still reads as
    // a real — and wrong — value. This holds under "clip" too, for the same
    // reason.
    if (it.n && tw > w - 8) {
      const hashW = textWidth(ctx.font, "#") || 1;
      text = "#".repeat(Math.max(1, Math.floor((w - 8) / hashW)));
      tw = textWidth(ctx.font, text);
    // "Clip" (`it.cl`) is the third state of the overflow control: stop at the
    // cell edge rather than borrowing empty neighbours. Excel has no such
    // setting — it always spills — so this skips the spill scan entirely and
    // leaves the span at the cell's own bounds.
    // Shrink-to-fit joins `clip` in skipping the spill scan: both mean "stay
    // inside this cell", so borrowing a neighbour first and then shrinking
    // would leave the text scaled down *and* overhanging.
    } else if (!it.cl && !it.shrink && tw > w - 8) {
      if (align !== "right") {
        let c = it.c;
        // Stop at a non-empty cell OR a column that isn't drawn (e.g. the gap
        // between a frozen band and the scrolling body) — colXAt is undefined
        // there and would make the clip NaN.
        // The scan may cross columns that are not drawn: a label whose own
        // cell is left of the window spills *into* it, and stopping at the
        // first undrawn column would stop at the very first step. `fscreen*`
        // has a position for any column, drawn or not. The pane bounds still
        // apply, so a frozen cell cannot borrow the body's columns.
        while (clipR - x < tw + 8 && c + 1 < spillHi && !occupied.has(it.r + "," + (c + 1))) {
          c += 1;
          clipR = fscreenXEnd(c);
        }
      }
      if (align !== "left") {
        let c = it.c;
        // Same reasoning as the rightward scan: a right-aligned label whose
        // cell is off the *right* edge spills back into the window, and the
        // columns it crosses on the way are not drawn either.
        while (x + w - clipL < tw + 8 && c - 1 >= spillLo && !occupied.has(it.r + "," + (c - 1))) {
          c -= 1;
          clipL = fscreenX(c);
        }
      }
    }

    ctx.save();
    if (frozen) { const q = quadClip(it.r, it.c, v); ctx.beginPath(); ctx.rect(q.x, q.y, q.w, q.h); ctx.clip(); }
    ctx.beginPath();
    ctx.rect(clipL, yTop, clipR - clipL, h);
    ctx.clip();
    // Shrink-to-fit: scale the font down until the text fits its own cell,
    // rather than spilling or clipping. Applied after the spill scan so it
    // overrides it — the whole point of the setting is that the text stays
    // inside, so borrowing a neighbour first would defeat it.
    if (it.shrink && tw > w - 8) {
      const scale = Math.max(0.4, (w - 8) / tw);
      const px = Math.max(5, cellPx(it) * scale);
      const weight = it.b ? "600 " : "";
      const slant = it.i ? "italic " : "";
      ctx.font = `${slant}${weight}${px}px ${fontStack(it.fn)}`;
      tw = textWidth(ctx.font, text);
    }
    ctx.fillStyle = cellFg(it);
    let tx;
    const ind = (it.in || 0) * INDENT_PX;
    if (align === "right") { ctx.textAlign = "right"; tx = x + w - 5 - ind; }
    else if (align === "center") { ctx.textAlign = "center"; tx = x + w / 2; }
    else { ctx.textAlign = "left"; tx = x + 5 + ind; }
    if (it.runs && text === it.t) {
      // Rich text: each run carries its own font, so it is drawn piece by
      // piece. Only when the text is the cell's own — a "#####" overflow
      // placeholder has no runs to speak of.
      const total = runsWidth(it);
      const startX = align === "right" ? tx - total : align === "center" ? tx - total / 2 : tx;
      const savedAlign = ctx.textAlign;
      ctx.textAlign = "left";
      drawRuns(it, startX, y);
      ctx.textAlign = savedAlign;
      ctx.restore();
      continue;
    }
    ctx.fillText(text, tx, y + supShift);
    if (it.u || it.st) {
      const lw = Math.min(tw, clipR - clipL - 8);
      const lx = align === "right" ? tx - lw : align === "center" ? tx - lw / 2 : tx;
      ctx.strokeStyle = ctx.fillStyle;
      ctx.lineWidth = 1;
      if (it.u) {
        ctx.beginPath();
        ctx.moveTo(lx, y + 7.5);
        ctx.lineTo(lx + lw, y + 7.5);
        ctx.stroke();
        // A double or accounting underline is a second rule below the first.
        // Drawing one line for all four kinds is what made a ledger's
        // accounting underline read as an ordinary one.
        if (it.uk === "double" || it.uk === "doubleAccounting") {
          ctx.beginPath();
          ctx.moveTo(lx, y + 10.5);
          ctx.lineTo(lx + lw, y + 10.5);
          ctx.stroke();
        }
      }
      if (it.st) { ctx.beginPath(); ctx.moveTo(lx, y); ctx.lineTo(lx + lw, y); ctx.stroke(); }
    }
    ctx.restore();
  }

  // A table's own outline, over its fills and its text.
  //
  // Before the cell borders below on purpose: a border the *file* puts on a
  // cell is the author's and wins over the style's edge.
  if (tablesInView.length) drawTableOutlines(withQuad, tablesInView, geo);

  // Cell borders (from the engine), drawn over fills/text.
  for (const it of items) {
    if (!it.bd) continue;
    const x = colXAt(it.c);
    const yTop = rowYAt(it.r);
    if (x === undefined || yTop === undefined) continue;
    const w = colWAt(it.c);
    const h = rowHAt(it.r);
    withQuad(it.r, it.c, () => {
      drawEdge(it.bd.l, x, yTop, x, yTop + h);
      drawEdge(it.bd.r, x + w, yTop, x + w, yTop + h);
      drawEdge(it.bd.t, x, yTop, x + w, yTop);
      drawEdge(it.bd.b, x, yTop + h, x + w, yTop + h);
      // Diagonals: one line description, drawn along either corner-to-corner
      // direction the cell asks for — a cell can carry both, forming a cross.
      if (it.bd.d) {
        if (it.bd.dd) drawEdge(it.bd.d, x, yTop, x + w, yTop + h);
        if (it.bd.du) drawEdge(it.bd.d, x, yTop + h, x + w, yTop);
      }
    });
  }

  // Merged ranges: paint each as one cell — erase interior gridlines, redraw the
  // top-left cell's fill + text across the span, outline it.
  for (const m of sheetMerges) {
    const it = items.find((t) => t.r === m.r0 && t.c === m.c0);
    const selected = mergeInSel(m);
    for (const s of mergeSlices(m)) {
      const { x: mx, y: my, w: mw, h: mh } = s;
      if (mx > v.w || my > v.h || mx + mw < HW || my + mh < HH) continue;
      withSliceClip(s, () => {
        ctx.fillStyle = it && it.bg ? "#" + it.bg : colors.bg;
        ctx.fillRect(mx, my, mw, mh);
        if (selected) { ctx.fillStyle = colors.sel; ctx.fillRect(mx, my, mw, mh); }
        ctx.strokeStyle = colors.grid;
        ctx.lineWidth = 1;
        ctx.strokeRect(Math.floor(mx) + 0.5, Math.floor(my) + 0.5, Math.round(mw) - 1, Math.round(mh) - 1);
        // The text belongs to the anchor's half; the other half of a straddling
        // merge shows only the fill, as its own pane's slice of the block.
        if (it && it.t && s.anchor) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(mx, my, mw, mh);
          ctx.clip();
          ctx.font = cellFont(it);
          ctx.fillStyle = cellFg(it);
          const al = it.a === "r" ? "right" : it.a === "c" ? "center" : "left";
          ctx.textAlign = al;
          const tx = al === "right" ? mx + mw - 5 : al === "center" ? mx + mw / 2 : mx + 5;
          ctx.fillText(it.t, tx, my + mh / 2);
          ctx.restore();
        }
      });
    }
  }

  // Find highlights: every match on this sheet gets a soft tint, so "3 of 47"
  // is answerable by looking at the sheet rather than by stepping through it.
  // The current match keeps the ordinary selection, which stays distinct.
  if (findState.matches.length && !findBar.hidden) {
    ctx.fillStyle = colors.findHit;
    for (const m of findState.matches) {
      if (m.s !== undefined && m.s !== state.sheet) continue;
      const hx = colXAt(m.c), hy = rowYAt(m.r);
      if (hx === undefined || hy === undefined) continue;
      withQuad(m.r, m.c, () => ctx.fillRect(hx + 1, hy + 1, colWAt(m.c) - 1, rowHAt(m.r) - 1));
    }
  }

  // Range finder: while a formula is being edited, outline each block it
  // references, one colour per reference, matching the order they appear in the
  // text. Drawn under the selection so the active cell still reads as active.
  // A reference to another sheet is not outlined here — it is not on screen.
  if (refSpans.length) {
    ctx.lineWidth = 1.5;
    ctx.setLineDash([]);
    refSpans.forEach((r, i) => {
      if (r.sh) return;
      const rx = spanX(r.c0, r.c1, v), ry = spanY(r.r0, r.r1, v);
      if (rx.w <= 0 || ry.h <= 0) return;
      ctx.strokeStyle = REF_COLORS[i % REF_COLORS.length];
      perQuad(() => ctx.strokeRect(rx.x + 0.75, ry.y + 0.75, rx.w - 1.5, ry.h - 1.5));
    });
  }

  // Range border (multi-cell selections only) + focus-cell border. A single-cell
  // selection is drawn solely by the focus border below, which spans a merge —
  // otherwise this would stroke a box around just the merge's anchor cell,
  // showing a spurious interior line inside the merge.
  ctx.strokeStyle = colors.accent;
  ctx.lineWidth = 2;
  const singleCell = rectSel.r0 === rectSel.r1 && rectSel.c0 === rectSel.c1;
  if (state.selKind === "cells" && !singleCell && sX.w > 0 && sY.h > 0) {
    perQuad(() => ctx.strokeRect(sX.x + 1, sY.y + 1, sX.w - 1, sY.h - 1));
  }
  const fm = mergeAt(state.sel.row, state.sel.col);
  if (fm) {
    // One outline per pane the merge occupies, each in that pane's geometry.
    for (const s of mergeSlices(fm)) {
      withSliceClip(s, () => {
        ctx.strokeStyle = colors.accent;
        ctx.lineWidth = 2;
        ctx.strokeRect(s.x + 1, s.y + 1, s.w - 1, s.h - 1);
      });
    }
  } else {
    withQuad(state.sel.row, state.sel.col, () => {
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 2;
      const fx = colXAt(state.sel.col);
      const fy = rowYAt(state.sel.row);
      if (fx !== undefined && fy !== undefined) {
        ctx.strokeRect(fx + 1, fy + 1, colWAt(state.sel.col) - 1, rowHAt(state.sel.row) - 1);
      }
    });
  }

  // Marching-ants outline around the copy/cut source (animated dash offset).
  if (clipMarch && clipMarch.sheet === state.sheet) {
    const mx = spanX(clipMarch.c0, clipMarch.c1, v), my = spanY(clipMarch.r0, clipMarch.r1, v);
    if (mx.w > 1 && my.h > 1) {
      ctx.save();
      ctx.strokeStyle = colors.accent;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([5, 3]);
      ctx.lineDashOffset = -marchOffset;
      perQuad(() => ctx.strokeRect(mx.x + 0.75, my.y + 0.75, mx.w - 1.5, my.h - 1.5));
      ctx.setLineDash([]);
      ctx.restore();
    }
  }

  // Drag-fill preview outline.
  if (state.fill && state.fill.dst) {
    const d = state.fill.dst;
    const dx = spanX(d.c0, d.c1, v), dy = spanY(d.r0, d.r1, v);
    ctx.strokeStyle = colors.accent;
    ctx.lineWidth = 1;
    ctx.setLineDash([4, 3]);
    perQuad(() => ctx.strokeRect(dx.x + 0.5, dy.y + 0.5, dx.w - 1, dy.h - 1));
    ctx.setLineDash([]);
  }

  // Other participants, drawn after the local selection so a cursor landing on
  // the same cell is still visible, and before the overlays below so it never
  // hides a control the user can click.
  drawCollaborators(v, perQuad);

  // Fill handle at the selection's bottom-right corner (cell selections only).
  fillHandleRect = null;
  if (state.selKind === "cells" && !state.fill) {
    const hc = colXAt(rectSel.c1), hr = rowYAt(rectSel.r1);
    if (hc !== undefined && hr !== undefined) {
      const hx = hc + colWAt(rectSel.c1), hy = hr + rowHAt(rectSel.r1);
      // Not drawn where the primary pointer is coarse and the touch handle has
      // taken the same corner. **Found by looking at a screenshot**: the 6px
      // square peeked out from beside the 14px circle and read as a second,
      // smaller thing to grab — on a device where it has never been grabbable,
      // because the mouse path that owns it hit-tests within 5px of a point.
      // `fillHandleRect` is still recorded either way, so a mouse on a hybrid
      // device (which keeps the square, its primary pointer being fine) loses
      // nothing.
      if (!(coarsePrimaryPointer() && touchHandlesOn())) {
        withQuad(rectSel.r1, rectSel.c1, () => {
          ctx.fillStyle = colors.accent;
          ctx.fillRect(hx - 3, hy - 3, 6, 6);
          ctx.strokeStyle = colors.bg;
          ctx.lineWidth = 1;
          ctx.strokeRect(hx - 3.5, hy - 3.5, 7, 7);
        });
      }
      fillHandleRect = { x: hx, y: hy };
    }
  }

  // Touch range handles, on the block's two diagonal corners. Positions are
  // recomputed every frame and remembered for hit-testing, exactly as the fill
  // handle above is; a corner that has scrolled out of the body gets `null`
  // rather than a clamped position, so a handle is never grabbable somewhere
  // its corner is not.
  touchHandles = { tl: null, br: null };
  if (touchHandlesOn() && state.selKind === "cells" && !state.fill) {
    // The outward offset is clamped back inside the corner's own pane.
    //
    // **Found by screenshotting a selection on A1**: the top-left handle's
    // centre came out at (HW − 7, HH − 7), which is inside the header corner
    // box, so the body clip erased it entirely — while `touchHandleAt` went on
    // answering "tl" for a finger there. An affordance that is invisible and
    // still live is the worst of both: nothing tells the user it is there, and
    // pressing the headers starts a range drag they did not ask for. Clamping
    // the drawn point is what makes the two agree, since both read this.
    const pen = (row, col, x, y) => {
      const q = quadClip(row, col, v);
      return {
        x: Math.min(Math.max(x, q.x + TOUCH_HANDLE_R), q.x + q.w - TOUCH_HANDLE_R),
        y: Math.min(Math.max(y, q.y + TOUCH_HANDLE_R), q.y + q.h - TOUCH_HANDLE_R),
      };
    };
    const x0 = colXAt(rectSel.c0), y0 = rowYAt(rectSel.r0);
    const x1 = colXAt(rectSel.c1), y1 = rowYAt(rectSel.r1);
    if (x0 !== undefined && y0 !== undefined) {
      touchHandles.tl = pen(rectSel.r0, rectSel.c0, x0 - TOUCH_HANDLE_R, y0 - TOUCH_HANDLE_R);
    }
    if (x1 !== undefined && y1 !== undefined) {
      touchHandles.br = pen(
        rectSel.r1, rectSel.c1,
        x1 + colWAt(rectSel.c1) + TOUCH_HANDLE_R,
        y1 + rowHAt(rectSel.r1) + TOUCH_HANDLE_R,
      );
    }
    const dot = (h, row, col) => {
      if (!h) return;
      withQuad(row, col, () => {
        ctx.beginPath();
        ctx.arc(h.x, h.y, TOUCH_HANDLE_R, 0, Math.PI * 2);
        ctx.fillStyle = colors.accent;
        ctx.fill();
        // A ring in the page background, so the handle stays visible against a
        // dark cell fill and against the selection tint alike.
        ctx.strokeStyle = colors.bg;
        ctx.lineWidth = 2;
        ctx.stroke();
      });
    };
    dot(touchHandles.tl, rectSel.r0, rectSel.c0);
    dot(touchHandles.br, rectSel.r1, rectSel.c1);
  }

  // Autofilter header buttons, drawn before the validation chevron so the
  // active cell's own dropdown wins if the two ever land on the same cell.
  // (`refreshFilterInfo()` ran before the cell pass: the *text* has to know
  // where the arrows are, so reading the model here would have laid this
  // frame's labels out against last frame's arrows.)
  drawFilterButtons(withQuad);
  drawTraceArrows(withQuad);

  // Data-validation dropdown button on the active cell (if it has a list rule).
  validationChevron = null;
  if (wasm && state.selKind === "cells" && !state.fill) {
    let vals = null;
    try { const vj = wasm.session_validation_at(state.sheet, state.sel.row, state.sel.col); if (vj !== "null") vals = JSON.parse(vj); } catch {}
    const cx = colXAt(state.sel.col), cy = rowYAt(state.sel.row);
    if (vals && cx !== undefined && cy !== undefined) {
      const cw = colWAt(state.sel.col), ch = rowHAt(state.sel.row);
      const bw = 17, bx = cx + cw, by = cy;
      withQuad(state.sel.row, state.sel.col, () => {
        ctx.fillStyle = colors.accent;
        ctx.fillRect(bx, by, bw, ch);
        ctx.strokeStyle = "#fff"; ctx.lineWidth = 1.6; ctx.lineJoin = "round";
        const mx = bx + bw / 2, my = by + ch / 2;
        ctx.beginPath();
        ctx.moveTo(mx - 4, my - 2); ctx.lineTo(mx, my + 2.5); ctx.lineTo(mx + 4, my - 2);
        ctx.stroke();
      });
      validationChevron = { x: bx, y: by, w: bw, h: ch, values: vals };
    }
  }

  // The rule's input hint, shown while its cell is selected — Excel's "Input
  // Message". A constraint explained only after you have typed something wrong
  // is explained too late, and the wording was being carried through every save
  // without ever being shown.
  refreshValidationPrompt();

  // Error indicators — a small marker in the top-left of any cell holding an
  // error value, so a broken formula is findable by eye instead of only by
  // reading every cell. Hovering explains it (see the mousemove handler).
  errorCells = new Set();
  for (const it of items) {
    if (!it.er) continue;
    errorCells.add(it.r + "," + it.c);
    const ex = colXAt(it.c), ey = rowYAt(it.r);
    if (ex === undefined || ey === undefined) continue;
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#e5484d";
      ctx.beginPath();
      ctx.moveTo(ex + 1, ey + 1);
      ctx.lineTo(ex + 8, ey + 1);
      ctx.lineTo(ex + 1, ey + 8);
      ctx.closePath();
      ctx.fill();
    });
  }

  // **Numbers stored as text** (`DATA-NT-01`). A green triangle in the
  // top-right, which is Excel's convention and deliberately not the top-left
  // where the red error marker sits — a cell can be neither, and putting them
  // in the same corner would make one hide the other.
  //
  // This is the cue that was missing. `SUM` over a column of text returns 0,
  // which is what Excel returns too, and a person reading that zero had no way
  // to tell a correct empty sum from a column an importer turned into strings.
  numericTextCells = new Set();
  for (const it of items) {
    if (!it.nt) continue;
    numericTextCells.add(it.r + "," + it.c);
    const nx = colXAt(it.c), ny = rowYAt(it.r);
    if (nx === undefined || ny === undefined) continue;
    const nw = colWAt(it.c);
    withQuad(it.r, it.c, () => {
      ctx.fillStyle = "#2f9e44";
      ctx.beginPath();
      ctx.moveTo(nx + nw - 8, ny + 1);
      ctx.lineTo(nx + nw - 1, ny + 1);
      ctx.lineTo(nx + nw - 1, ny + 8);
      ctx.closePath();
      ctx.fill();
    });
  }

  // Hyperlinks: underline them and tint them, which is the only cue that a
  // cell is clickable. Drawn before the comment markers so a cell with both
  // still shows its note triangle on top.
  linkCells = new Set();
  if (wasm) {
    const lr0 = geo.rowIdx[0] ?? state.firstRow, lc0 = geo.colIdx[0] ?? state.firstCol;
    const lr1 = geo.rowIdx[geo.rowIdx.length - 1] ?? lr0;
    const lc1 = geo.colIdx[geo.colIdx.length - 1] ?? lc0;
    let links = [];
    try { links = JSON.parse(wasm.session_hyperlink_cells(state.sheet, lr0, lc0, lr1, lc1)); } catch {}
    for (const lk of links) {
      linkCells.add(lk.r + "," + lk.c);
      const lx = colXAt(lk.c), ly = rowYAt(lk.r);
      if (lx === undefined || ly === undefined) continue;
      const lw = colWAt(lk.c), lh = rowHAt(lk.r);
      withQuad(lk.r, lk.c, () => {
        ctx.strokeStyle = getComputedStyle(ocThemeHost)
          .getPropertyValue("--oc-accent-color").trim() || "#3b82f6";
        ctx.lineWidth = 1;
        ctx.beginPath();
        // Sit the rule on the text baseline rather than the cell floor, or it
        // reads as a bottom border instead of an underline.
        const baseline = ly + lh - Math.max(4, Math.round(lh * 0.22));
        ctx.moveTo(lx + 3, baseline + 0.5);
        ctx.lineTo(lx + lw - 3, baseline + 0.5);
        ctx.stroke();
      });
    }
  }

  // Comment indicators — a small red triangle in each commented cell's corner.
  commentCells = new Set();
  if (wasm) {
    const r0 = geo.rowIdx[0] ?? state.firstRow, c0 = geo.colIdx[0] ?? state.firstCol;
    const r1 = geo.rowIdx[geo.rowIdx.length - 1] ?? r0, c1 = geo.colIdx[geo.colIdx.length - 1] ?? c0;
    let cmts = [];
    try { cmts = JSON.parse(wasm.session_comments(state.sheet, r0, c0, r1, c1)); } catch {}
    for (const cm of cmts) {
      commentCells.add(cm.r + "," + cm.c);
      const cx = colXAt(cm.c), cy = rowYAt(cm.r);
      if (cx === undefined || cy === undefined) continue;
      const cw = colWAt(cm.c);
      withQuad(cm.r, cm.c, () => {
        ctx.fillStyle = "#e5484d";
        ctx.beginPath();
        ctx.moveTo(cx + cw - 7, cy); ctx.lineTo(cx + cw, cy); ctx.lineTo(cx + cw, cy + 7);
        ctx.closePath(); ctx.fill();
      });
    }
  }
  ctx.restore(); // end body clip


  // Which columns/rows are covered by the current selection (for header tint).
  const rr = selRect();
  const colInSel = (c) => state.selKind === "all" ||
    (state.selKind === "cols" ? c >= rr.c0 && c <= rr.c1 : state.selKind === "cells" && c >= rr.c0 && c <= rr.c1);
  const rowInSel = (r) => state.selKind === "all" ||
    (state.selKind === "rows" ? r >= rr.r0 && r <= rr.r1 : state.selKind === "cells" && r >= rr.r0 && r <= rr.r1);

  // Headers (painted over the body edges), with the selected band tinted.
  // A sheet may hide them (OOXML showRowColHeaders="0"); HW/HH are then 0, the
  // grid runs to the corner, and this whole block is skipped.
  if (HW || HH) {
  ctx.fillStyle = colors.headerBg;
  ctx.fillRect(0, 0, v.w, HH);
  ctx.fillRect(0, 0, HW, v.h);
  // **A select-all mark in the corner** (`UX-CHR-09`). The corner held two
  // freeze-drag handles and nothing that said what clicking it does — a white
  // pill above a grey bar, which reads as debris rather than as a control.
  // Excel draws a right-angled triangle in the lower right of the corner box;
  // Sheets and OnlyOffice draw one too. The click already selected the sheet;
  // what was missing was anything saying so.
  if (HW && HH) {
    const pad = 4;
    const size = Math.min(HW, HH) - pad * 2;
    cornerMarkBox = { x: HW - pad - size, y: HH - pad - size, w: size, h: size };
    ctx.save();
    ctx.fillStyle = colors.headerText || "#6b7280";
    ctx.globalAlpha = 0.65;
    ctx.beginPath();
    ctx.moveTo(cornerMarkBox.x + size, cornerMarkBox.y);
    ctx.lineTo(cornerMarkBox.x + size, cornerMarkBox.y + size);
    ctx.lineTo(cornerMarkBox.x, cornerMarkBox.y + size);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  // Column headers, frozen segment then scrolling segment (each clipped so the
  // scrolling headers can't bleed into the frozen band).
  const drawColHeaders = (clipX, clipW, wantFrozen) => {
    if (clipW <= 0) return;
    ctx.save(); ctx.beginPath(); ctx.rect(clipX, GH, clipW, HH - GH); ctx.clip();
    for (let i = 0; i < geo.cols; i++) {
      if (geo.colW[i] <= 0) continue;
      if ((geo.colIdx[i] < F.fc) !== wantFrozen) continue;
      const c = geo.colIdx[i];
      if (colInSel(c)) { ctx.fillStyle = colors.sel; ctx.fillRect(geo.colX[i], GH, geo.colW[i], HH - GH); }
      ctx.fillStyle = colInSel(c) ? colors.accent : colors.muted;
      ctx.fillText(colName(c), geo.colX[i] + geo.colW[i] / 2, GH + (HH - GH) / 2);
    }
    ctx.restore();
  };
  if (F.fc) drawColHeaders(HW, F.bodyX0 - HW, true);
  drawColHeaders(F.bodyX0, v.w - F.bodyX0, false);

  const drawRowHeaders = (clipY, clipH, wantFrozen) => {
    if (clipH <= 0) return;
    ctx.save(); ctx.beginPath(); ctx.rect(GW, clipY, HW - GW, clipH); ctx.clip();
    for (let i = 0; i < geo.rows; i++) {
      if (geo.rowH[i] <= 0) continue;
      if ((geo.rowIdx[i] < F.fr) !== wantFrozen) continue;
      const r = geo.rowIdx[i];
      if (rowInSel(r)) { ctx.fillStyle = colors.sel; ctx.fillRect(GW, geo.rowY[i], HW - GW, geo.rowH[i]); }
      ctx.fillStyle = rowInSel(r) ? colors.accent : colors.muted;
      ctx.fillText(String(r + 1), GW + (HW - GW) / 2, geo.rowY[i] + geo.rowH[i] / 2);
    }
    ctx.restore();
  };
  if (F.fr) drawRowHeaders(HH, F.bodyY0 - HH, true);
  drawRowHeaders(F.bodyY0, v.h - F.bodyY0, false);
  drawHiddenMarkers();
  drawOutlineGutter(v);
  } // end headers
  drawFreezeDividers(v);
  // The lifted band first and the drop line over it: the line is the precise
  // half of the feedback, so nothing translucent may sit on top of it.
  drawMoveGhost(v);
  // After the dividers, so a drop indicator over a frozen band is still visible.
  drawMoveDropIndicator(v);
  drawFreezeHandles();

  // The in-cell editor is a DOM element over the canvas: keep it on its cell as
  // the grid scrolls or resizes under it (grid-wrap's overflow clips it once the
  // cell leaves the viewport), instead of leaving it parked mid-air.
  if (editSurface === inline) positionInline();
  updateNameBox();
  updateNumberFormatReadout();
  announceCell();
  scheduleA11yGrid();
  updateGridCounts();
  updateCellMode();
  updateScrollbars(v);
  updateStats();
  announceCollabSelection();
  if (wasm) refreshFormulaBar();
  if (wasm && activePanel) refreshPanel();
  // Last, so a host's listener sees the state the frame just painted rather
  // than the one it started from.
  if (wasm) emitStateEvents();
}

// px proximity to the freeze divider that arms a drag.
//
// Was 4, which is under half a fingertip and barely a mouse's worth of slop:
// the divider is a 1px line, so a user aiming at it missed more often than not
// and reasonably concluded a frozen pane could not be moved or removed at all.
// The menu's Unfreeze was the only reliable route, and it is two clicks inside
// a popup nobody opens looking for it.
//
// 8 is the smallest that reliably catches a deliberate aim without stealing
// clicks from the cell beside it — the divider sits on a gridline, so a grab
// zone much wider starts swallowing selections.
export const FREEZE_GRAB = 8;

// Prominent, draggable freeze dividers (Sheets-style), drawn on top of the
// headers. During a drag the line follows the pointer as a live preview.


// Grab handles for *creating* a freeze, in the corner box above the row header.
//
// `freezeHit` below can only find a divider that already exists, because the
// divider is drawn at the freeze line and there is no line at zero. So every
// drag gesture worked on a freeze somebody had already made through the menu,
// and there was no gesture that made one — the affordance a user goes looking
// for first. These are the two handles Sheets puts in the corner: drag the
// right-hand one out to freeze columns, the bottom one down to freeze rows.
//
// They live *inside* the corner box on purpose. The obvious alternative — a
// grab zone on the body's leading edge — sits exactly where column A's cells
// are, and would swallow ordinary selection clicks forever after.
export const FREEZE_HANDLE = 7; // px thickness of a corner grab handle

// Is the pointer on a freeze divider (draggable to change or remove the freeze)?
// Only in the body region (col divider below the column header, row divider
// right of the row header), so it never conflicts with header-border resize.

// Commit a freeze-divider drag: the new frozen count is the line/column under
// the pointer; dragging into the header (px<=HW / py<=HH) removes that axis.

// Show Sum/Avg/Count of the selection (only for a multi-cell selection), like
// a real spreadsheet's status bar.



// --- Enter / Tab navigation ------------------------------------------------
//
// Two Excel behaviours that both need somewhere to remember a little context.
//
// Inside a multi-cell selection, Enter and Tab walk *within* the block and wrap
// at its edges, leaving the selection itself alone. `state.sel` is the active
// cell everywhere else in this file, and the block is derived from anchor+sel —
// so moving `sel` would shrink the very selection being walked. `navBlock` holds
// the block still while `sel` moves inside it; `selRect` prefers it when set.
//
// And a run of Tabs remembers the column it started in, so Enter returns there
// and drops a row — which is what makes tabbing across a record and pressing
// Enter land at the start of the next one rather than wherever you stopped.
export let navBlock = null;   // {r0,c0,r1,c1} being walked, or null
let tabOrigin = null;  // column a Tab run began in

// Any selection change by other means ends both runs.
function resetNavRuns() {
  navBlock = null;
  tabOrigin = null;
}

// The block Enter/Tab should walk, or null for a single cell.

// Step the active cell within `b`, wrapping at the edges. `axis` is "row" for
// Enter (down the column, then to the next column) or "col" for Tab.

// Enter: inside a block, walk it; otherwise return to the Tab run's origin
// column and drop a row, or just drop a row.
export function enterStep(back) {
  const b = navTarget();
  if (b) { navBlock = b; stepWithin(b, "row", back); return; }
  const col = tabOrigin !== null ? tabOrigin : state.sel.col;
  tabOrigin = null;
  // Merge-aware for the same reason the arrows are: Enter on a vertically
  // merged cell otherwise lands back inside it and the cursor never moves.
  const to = stepFrom(state.sel.row, state.sel.col, back ? -1 : 1, 0);
  select(to.row, col);
}

// Tab: inside a block, walk it; otherwise move sideways, remembering where the
// run began.
function tabStep(back) {
  const b = navTarget();
  if (b) { navBlock = b; stepWithin(b, "col", back); return; }
  if (tabOrigin === null) tabOrigin = state.sel.col;
  const origin = tabOrigin;
  const to = stepFrom(state.sel.row, state.sel.col, 0, back ? -1 : 1);
  select(to.row, to.col);
  tabOrigin = origin; // `select` cleared it; this run is still going
}

/// Step one cell from (row, col) in the direction (dr, dc), treating a merge as
/// the single cell it looks like.
///
/// `select` snaps any coordinate inside a merge back to the merge's top-left
/// anchor, which is right for a click and wrong for a step: arrowing right out
/// of B2:D2 computed (1,2), `select` snapped it back to (1,1), and the selection
/// **never moved**. Left and up worked, because the anchor *is* the top-left, so
/// the failure was asymmetric — which is why it read as a frozen keyboard rather
/// than as a merge rule. Excel treats a merge as one cell: you leave from its
/// far edge and land on the first cell past it.

export function select(row, col) {
  let r = Math.max(0, row);
  let c = Math.max(0, col);
  const m = mergeAt(r, c); // clicking a merged cell selects its anchor (top-left)
  if (m) { r = m.r0; c = m.c0; }
  state.sel = { row: r, col: c };
  state.anchor = { row: r, col: c };
  state.selKind = "cells";
  state.ranges = [];
  extending = false;
  lastFill = null;
  hideFillOptions();
  resetNavRuns();
  ensureVisible();
  draw();
}

// Ctrl/Cmd+click: bank the current range and start a fresh active range at
// (row, col) without clearing the banked ones — builds a multi-range selection.
// Ctrl/Cmd+click a column/row header: same idea as addRange, but the fresh
// active range is a whole column/row instead of a single cell.

// Extend the selection to (row, col), keeping the active cell where it is.
//
// The corner that travels is `state.anchor`, not `state.sel`. That reads
// backwards until you remember that `state.sel` is the **active cell**
// everywhere in this file (UX-NV4) — and in Excel and Sheets the active cell
// stays where the selection began while the far corner follows the keyboard or
// the pointer. Moving `sel` instead put the active cell at the end of the
// travel, so selecting B2:B4 with Shift+Down and typing wrote into B4: the
// value landed in the last cell the user passed over rather than the one the
// selection was still highlighting.
//
// `selRect` takes the min and max of the two, so which end moves makes no
// difference to the block; it decides only where the active cell ends up.
export function extend(row, col) {
  state.anchor = { row: Math.max(0, row), col: Math.max(0, col) };
  state.selKind = "cells";
  extending = true;
  // Follow the travelling corner, not the active cell, which is not moving.
  ensureVisible(state.anchor.row, state.anchor.col);
  draw();
}

// Scroll the body just enough to show a cell — the active one by default, or
// any cell (arrow-key point mode follows the cell it is pointing at, which is
// not the selection).
/// Round a scroll offset up to the next whole column / row boundary.
///
/// Scrolling a cell into view by the *bare minimum* puts its far edge against
/// the far edge of the viewport, which leaves the **leading** edge somewhere in
/// the middle of whatever column or row happens to be there. The remainder is a
/// different size for every target, so arrowing across a sheet makes the first
/// visible column look like it is growing and shrinking — the grid appears to be
/// resizing itself rather than scrolling, and scrolling back makes it "expand"
/// again (UX-GRID-02). Excel never shows a partial leading column: it scrolls
/// until that column is whole, and so does this.
///
/// Snapping *up* only ever scrolls further in the direction already being
/// travelled, so the cell that prompted the scroll stays on screen. `limit` is
/// its own leading edge — itself a boundary — and is both the furthest this may
/// go and the answer when the cell is wider or taller than the viewport, where
/// no aligned offset can show it and showing it wins over aligning.
///
/// Rows go through `rowAtPx`/`rowOffsetPx` rather than the engine directly,
/// because a row grown by wrapped or rotated text is taller than the engine
/// thinks and those two are the pair that knows it.
/// `px` and `limit` are body-space (what `state.scrollX`/`scrollY` hold), while
/// the engine indexes absolute sheet pixels — hence `frozen`, the width or
/// height of the frozen band, added on the way in and taken off on the way out.
/// Getting that conversion wrong would misalign only on sheets with a frozen
/// pane, which is exactly the sort of thing that survives a demo.

/// How the engine says a data bar is drawn.
///
/// Read once: it is a set of constants compiled into the engine, so asking per
/// frame would cross the WebAssembly boundary sixty times a second to be told
/// the same thing. The fallback matches the engine's own values and exists so a
/// build predating `session_data_bar_style` still draws bars rather than
/// throwing in the paint loop.
let dataBarStyleCache = null;
function dataBarStyle() {
  if (dataBarStyleCache) return dataBarStyleCache;
  try {
    dataBarStyleCache = JSON.parse(wasm.session_data_bar_style());
  } catch {
    dataBarStyleCache = { padX: 1, padY: 2, alpha: 0.45, defaultColor: "638EC6" };
  }
  return dataBarStyleCache;
}

/// Arm or disarm End mode, and say so.
///
/// Announced in the status bar because an armed mode with no indicator is a
/// keystroke that behaves differently for reasons the user cannot see — Excel
/// shows "End Mode" for exactly that reason.

// --- Touch range selection ------------------------------------------------
//
// **A phone had no route to a range at all.** A drag on the grid pans, which is
// the only thing a drag can mean when it is also the scroll gesture — so the
// single way to select `A1:C5` on a phone was to type it into the Name Box.
// Everything a spreadsheet is for downstream of a range (sum, chart, sort,
// format, fill, copy) was therefore reachable only by someone who already knew
// A1 notation and was willing to type it.
//
// **The idiom is not invented here.** Google Sheets and Excel on a phone solve
// this the same way, and so does every text field on both platforms: a tap
// selects, and two draggable handles extend what is selected. Matching that
// exactly is the whole design. A spreadsheet that invents its own touch
// gesture charges the user for choosing it — they arrive already knowing this
// one, and anything else has to be discovered.
//
// So: two circular handles, at the block's **top-left** and **bottom-right**.
// Drag either and that corner follows the finger while the other stays put.
//
// Four decisions that were not obvious:
//
//   - **Which corner becomes the active cell.** `state.sel` is the active cell
//     and `state.anchor` is the travelling corner (see `extend`), and typing
//     goes to `state.sel`. Grabbing a handle therefore pins the *opposite*
//     corner into `state.sel` and drives `state.anchor` with the finger. Drag
//     the bottom-right handle out to C5 and the active cell is A1 — which is
//     where Excel and Sheets both leave it, and where a user who then types
//     expects the text to land.
//
//   - **The handles are offset outward, not centred on the corner.** Centred,
//     a target big enough for a finger (Apple asks 44px, Android 48) reaches
//     halfway across the corner cell in both directions, so every tap near the
//     end of a selection grabs a handle instead of moving the selection. Offset
//     by their own radius along the diagonal, most of the target lies outside
//     the block, which is where a finger reaching for "extend this" already
//     goes. The grab radius is still a compromise and is written down as one:
//     `TOUCH_HANDLE_GRAB` is 18, a 36px target rather than 44, because the
//     remaining 8px would be spent swallowing the neighbouring cell — and a
//     missed grab costs one more tap, while a stolen tap moves the selection
//     the user was building.
//
//   - **They are shown once a finger has been used, not by media query.**
//     `(pointer: coarse)` answers "is the primary pointer coarse", which is
//     false on a touchscreen laptop and on a tablet with a keyboard case
//     attached — both of which are fingers when a finger is what is on the
//     glass. `touchstart` on the grid is the evidence that settles it, and it
//     arrives before the synthesised click that selects, so the handles are
//     already on for the very first tap's repaint. A media-query match at load
//     is taken as evidence too, so a phone that starts on the formula bar still
//     gets them.
//
//   - **Only for cell selections.** A whole row, column or select-all has no
//     corner on screen to grab, and Sheets shows no handles for those either.
//
// The handles deliberately draw *over* the 6px fill square at the bottom-right:
// on a touch device that square was never hittable (the mouse path tests within
// 5px of it), so there is nothing to lose and a second dot beside the handle
// would only read as a second thing to grab.
const TOUCH_HANDLE_R = 7; // drawn radius, grid units
const TOUCH_HANDLE_GRAB = 18; // grab radius — see the note above

/// Whether the device's *primary* pointer is a finger. Narrower than
/// `touchHandlesOn`: a touchscreen laptop answers false here and true there,
/// which is the distinction that decides whether a mouse-only affordance can be
/// retired from the frame.
function coarsePrimaryPointer() {
  try { return window.matchMedia("(pointer: coarse)").matches; } catch { return false; }
}

/// Whether the touch selection handles are live for this session.
function touchHandlesOn() {
  return touchSeen || coarsePrimaryPointer();
}

/// Which handle, if either, is under (px, py) — grid units, as everywhere else
/// in the pointer paths. Nearest wins, so a single-cell selection (whose two
/// handles are one cell apart and can overlap on a short row) never traps the
/// finger on the wrong corner.
function touchHandleAt(px, py) {
  if (!touchHandlesOn() || state.selKind !== "cells" || state.fill) return null;
  let best = null;
  let bestD = TOUCH_HANDLE_GRAB;
  for (const corner of ["tl", "br"]) {
    const h = touchHandles[corner];
    if (!h) continue;
    const d = Math.hypot(px - h.x, py - h.y);
    if (d <= bestD) { best = corner; bestD = d; }
  }
  return best;
}

/// Begin a handle drag. Returns false when the finger was not on a handle.
function startHandleDrag(px, py) {
  const corner = touchHandleAt(px, py);
  if (!corner) return false;
  // Read the block *before* clearing the Enter/Tab navigation block: while one
  // is running it, and not anchor..sel, is what `selRect` reports and what the
  // handles were drawn from.
  const r = selRect();
  resetNavRuns();
  endInline();
  hideFillOptions();
  const fixed = corner === "tl" ? { row: r.r1, col: r.c1 } : { row: r.r0, col: r.c0 };
  const moving = corner === "tl" ? { row: r.r0, col: r.c0 } : { row: r.r1, col: r.c1 };
  state.sel = fixed;
  state.anchor = moving;
  state.selKind = "cells";
  state.ranges = [];
  handleDrag = { corner };
  state.dragging = true;
  dragPos = { px, py };
  return true;
}

/// Move the dragged corner to the cell under (px, py), clamped into the body so
/// a finger over a header still maps to a cell.
function moveHandleDrag(px, py) {
  const rect = canvas.getBoundingClientRect();
  const z = state.zoom || 1;
  const f = state.freeze || { bodyX0: HW, bodyY0: HH };
  dragPos = { px, py };
  // `rect` is CSS pixels and `px` is grid units, so the far edges are divided
  // before they are compared. (The mouse path at the `state.dragging` branch of
  // `mousemove` does not divide, which only shows up above 100% zoom.)
  const cx = Math.min(Math.max(px, f.bodyX0 + 1), rect.width / z - 2);
  const cy = Math.min(Math.max(py, f.bodyY0 + 1), rect.height / z - 2);
  const hit = cellAt(cx, cy);
  if (hit && (hit.row !== state.anchor.row || hit.col !== state.anchor.col)) {
    state.anchor = { row: hit.row, col: hit.col };
    draw();
  }
  maybeAutoScroll();
}

/// End a handle drag, however it ended.
function endHandleDrag() {
  if (!handleDrag) return;
  handleDrag = null;
  state.dragging = false;
  dragPos = null;
  stopAutoScroll();
  // The gesture is over, so the Name Box goes back to naming the active cell.
  // `extending` is only ever cleared by `select`, and a handle drag does not go
  // through it — so a block built with Shift+arrow and then adjusted by a finger
  // was left reading "8R x 5C" for the rest of the session, with nothing on
  // screen saying which cell the next keystroke would land in.
  extending = false;
  draw();
}

/// The handles as a test can see them: where they are, and what they are for.
export function touchHandlesForTest() {
  return {
    on: touchHandlesOn(),
    tl: touchHandles.tl ? { ...touchHandles.tl } : null,
    br: touchHandles.br ? { ...touchHandles.br } : null,
    grab: TOUCH_HANDLE_GRAB,
    dragging: handleDrag ? handleDrag.corner : null,
    active: { row: state.sel.row, col: state.sel.col },
  };
}

// --- Edge auto-scroll while drag-selecting --------------------------------
// When the pointer is dragged into the 28px band at a viewport edge (or past
// it), scroll the body toward the pointer and keep extending the selection —
// like every real spreadsheet. Runs on rAF until the pointer leaves the band
// or the drag ends.
export const AUTOSCROLL_EDGE = 28;
function autoScrollTick() {
  autoRaf = 0;
  if (!state.dragging || !dragPos) return;
  const { sx, sy, cx, cy } = edgeVelocity();
  if (sx === 0 && sy === 0) return; // pointer back inside — stop
  const SPEED = 0.5;
  const x0 = state.scrollX, y0 = state.scrollY;
  state.scrollX = Math.max(0, state.scrollX + sx * SPEED);
  state.scrollY = Math.max(0, state.scrollY + sy * SPEED);
  // If the scroll is pinned at 0 against the edge (velocity points off-sheet
  // but nothing can move), idle instead of redrawing every frame forever.
  if (state.scrollX === x0 && state.scrollY === y0) return;
  if (state.headerDrag === "col") selectColumn(colAtX(cx), true);
  else if (state.headerDrag === "row") selectRow(rowAtY(cy), true);
  else if (handleDrag) {
    // A touch handle moves `anchor`, not `sel` — the active cell is the corner
    // the finger is *not* holding. Sharing this loop with the mouse and writing
    // `sel` here would have the active cell jump to the far end the moment the
    // drag reached the edge of the screen, and nowhere else.
    const hit = cellAt(cx, cy);
    if (hit) { state.anchor = { row: hit.row, col: hit.col }; state.selKind = "cells"; }
  } else {
    const hit = cellAt(cx, cy);
    if (hit) { state.sel = { row: hit.row, col: hit.col }; state.selKind = "cells"; }
  }
  draw();
  autoRaf = requestAnimationFrame(autoScrollTick);
}
function maybeAutoScroll() {
  const { sx, sy } = edgeVelocity();
  if (state.dragging && dragPos && (sx !== 0 || sy !== 0)) {
    if (!autoRaf) autoRaf = requestAnimationFrame(autoScrollTick);
  } else {
    stopAutoScroll();
  }
}
function stopAutoScroll() { if (autoRaf) { cancelAnimationFrame(autoRaf); autoRaf = 0; } }

// Absolute px of the frozen band before `count` lines on this axis.

// Turn an engine parse error ("[OC-FML-0001] unexpected token: Star") into a
// message a spreadsheet user can act on.

// Hidden-region markers: a run of zero-width lines is a hidden band. Draw a
// small accent double-bar at each gap in the header strips, and remember the
// spans so a double-click on a marker can unhide them.
export let hiddenColMarks = [];
export let hiddenRowMarks = [];
function drawHiddenMarkers() {
  hiddenColMarks = [];
  hiddenRowMarks = [];
  let run = null;
  for (let i = 0; i < geo.colIdx.length; i++) {
    if (geo.colW[i] <= 0) {
      if (!run) run = { x: geo.colX[i], from: geo.colIdx[i], to: geo.colIdx[i] };
      else run.to = geo.colIdx[i];
    } else if (run) { hiddenColMarks.push(run); run = null; }
  }
  if (run) hiddenColMarks.push(run);
  run = null;
  for (let i = 0; i < geo.rowIdx.length; i++) {
    if (geo.rowH[i] <= 0) {
      if (!run) run = { y: geo.rowY[i], from: geo.rowIdx[i], to: geo.rowIdx[i] };
      else run.to = geo.rowIdx[i];
    } else if (run) { hiddenRowMarks.push(run); run = null; }
  }
  if (run) hiddenRowMarks.push(run);

  // A bare double-bar marked the spot but read as a freeze line or a selection
  // edge, and it only responded to a double-click — so a hidden row was easy to
  // notice and hard to get back. Draw an actual control: an accent handle with
  // two chevrons pointing apart, the same "expand from here" idiom Sheets uses.
  // Single click unhides (see the mousedown handler); hovering says so.
  ctx.save();
  ctx.fillStyle = colors.accent;
  for (const m of hiddenColMarks) {
    if (m.x < HW) continue;
    const h = Math.min(14, HH - 6);
    const top = (HH - h) / 2;
    ctx.fillRect(m.x - 4, top, 8, h);
    ctx.fillStyle = "#fff";
    const cy = top + h / 2;
    // ‹ ›
    ctx.beginPath();
    ctx.moveTo(m.x - 1.2, cy - 3); ctx.lineTo(m.x - 3.2, cy); ctx.lineTo(m.x - 1.2, cy + 3);
    ctx.closePath();
    ctx.moveTo(m.x + 1.2, cy - 3); ctx.lineTo(m.x + 3.2, cy); ctx.lineTo(m.x + 1.2, cy + 3);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = colors.accent;
  }
  for (const m of hiddenRowMarks) {
    if (m.y < HH) continue;
    const w = Math.min(18, HW - 6);
    const left = (HW - w) / 2;
    ctx.fillRect(left, m.y - 4, w, 8);
    ctx.fillStyle = "#fff";
    const cx = left + w / 2;
    // Chevrons pointing up and down, away from the collapsed band.
    ctx.beginPath();
    ctx.moveTo(cx - 3, m.y - 1.2); ctx.lineTo(cx, m.y - 3.2); ctx.lineTo(cx + 3, m.y - 1.2);
    ctx.closePath();
    ctx.moveTo(cx - 3, m.y + 1.2); ctx.lineTo(cx, m.y + 3.2); ctx.lineTo(cx + 3, m.y + 1.2);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = colors.accent;
  }
  ctx.restore();
}

// The hidden-band handle under a canvas point, if any. Shared by the click
// handler and the hover cursor so the two can never disagree about the target.

// Reveal the band a handle stands for.

// The outline gutter: a rail per nesting level with a collapse toggle at each
// group's summary line. Rows get the strip left of the row headers, columns the
// one above the column headers; both are zero-width when the axis has no groups.
function drawOutlineGutter(v) {
  outlineToggles = [];
  if (!wasm || (!GW && !GH)) return;
  ctx.save();
  ctx.font = "10px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";

  // One toggle: a bordered box holding − when the group is open, + when shut.
  const toggle = (bx, by, size, collapsed) => {
    ctx.fillStyle = colors.bg;
    ctx.fillRect(bx, by, size, size);
    ctx.strokeStyle = colors.muted;
    ctx.lineWidth = 1;
    ctx.strokeRect(bx + 0.5, by + 0.5, size - 1, size - 1);
    ctx.strokeStyle = colors.fg;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(bx + 2.5, by + size / 2);
    ctx.lineTo(bx + size - 2.5, by + size / 2);
    if (collapsed) {
      ctx.moveTo(bx + size / 2, by + 2.5);
      ctx.lineTo(bx + size / 2, by + size - 2.5);
    }
    ctx.stroke();
  };

  if (GW) {
    const info = readOutline(state.firstRow, geo.rows, false);
    ctx.beginPath();
    ctx.rect(0, HH, GW, v.h - HH);
    ctx.clip();
    for (let i = 0; i < geo.rows; i++) {
      if (geo.rowH[i] <= 0) continue;
      const r = geo.rowIdx[i];
      const line = info[r];
      if (!line) continue;
      const depth = Math.max(0, line.l - (line.b ? 1 : 0));
      const cx = 4 + depth * OUTLINE_STEP;
      // A rail along the lines inside a group, so the extent is visible.
      if (line.l > 0) {
        ctx.fillStyle = colors.muted;
        ctx.globalAlpha = 0.45;
        ctx.fillRect(4 + (line.l - 1) * OUTLINE_STEP + 4, geo.rowY[i], 1, geo.rowH[i]);
        ctx.globalAlpha = 1;
      }
      if (!line.b) continue;
      const size = 9;
      const by = geo.rowY[i] + (geo.rowH[i] - size) / 2;
      toggle(cx, by, size, line.c);
      outlineToggles.push({ x: cx, y: by, w: size, h: size, index: r, columns: false });
    }
  }
  ctx.restore();

  if (GH) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(HW, 0, v.w - HW, GH);
    ctx.clip();
    const info = readOutline(state.firstCol, geo.cols, true);
    for (let i = 0; i < geo.cols; i++) {
      if (geo.colW[i] <= 0) continue;
      const c = geo.colIdx[i];
      const line = info[c];
      if (!line) continue;
      const depth = Math.max(0, line.l - (line.b ? 1 : 0));
      const cy = 4 + depth * OUTLINE_STEP;
      if (line.l > 0) {
        ctx.fillStyle = colors.muted;
        ctx.globalAlpha = 0.45;
        ctx.fillRect(geo.colX[i], 4 + (line.l - 1) * OUTLINE_STEP + 4, geo.colW[i], 1);
        ctx.globalAlpha = 1;
      }
      if (!line.b) continue;
      const size = 9;
      const bx = geo.colX[i] + (geo.colW[i] - size) / 2;
      toggle(bx, cy, size, line.c);
      outlineToggles.push({ x: bx, y: cy, w: size, h: size, index: c, columns: true });
    }
    ctx.restore();
  }
}

// Outline data for the visible window, keyed by line index. Frozen panes mean
// the window is not one contiguous run, so this covers from line 0 when a
// freeze is in play rather than trying to stitch two spans together.

// The outline toggle under a canvas point, if any.

// Draw one line stretched to `width` by widening the gaps between words, the
// way horizontal justify/distribute lay out. A line with nothing to stretch (one
// word, or wider than the space already) is drawn plainly rather than having its
// glyphs pulled apart.

// The column/row index at a canvas x/y (for header clicks + hit-testing).
// Whole-sheet selection (the top-left corner box). The viewport stays put.
// Progressive Ctrl+A (Excel): the first press selects the contiguous block of
// data around the cursor — the table you are standing in — the second widens to
// the whole used region, and the third to the entire sheet.
//
// It used to jump straight to the used region from A1, which on a loaded sheet
// reads as "select everything" and loses the one selection you actually wanted:
// this block. A blank cursor cell has no block, so it starts at the used region.
// Whole-row selection; the focus stays at column 0 so the view doesn't jump.
// Whole-column selection; the focus stays at row 0.
// Shift+Space: promote the current selection to its full rows.
// Ctrl+Space: promote the current selection to its full columns.

// The cell range a formatting/clipboard op should touch, expanded for whole
// row/column/sheet selections to the used extent along the spanning axis.
// Every rectangle in the selection: the committed extra ranges plus the active
// one. Formatting and stats fold over this so a Ctrl+click multi-range behaves
// as one selection.
// Double-click a column boundary: size the column to its widest cell, measured
// with each cell's real font (family/size/bold/italic) so larger text fits.
// Double-click a row boundary: size the row to its tallest cell, honoring each
// cell's font size, wrap (wrapped to the column width), and explicit newlines.

// Run a formatting op over the whole selection (every range), then redraw.
// `link` is an optional {slot, tint} from the theme row; -1 means "not from the
// theme", which is how the engine tells a themed colour from a literal one.

// A standard color palette (grays row + hue columns at 5 lightness levels),
// shared by the font- and fill-color popovers, plus recent colors and a custom
// hex entry so any RRGGBB the engine supports is reachable.
export const COLOR_PALETTE = [
  "000000", "434343", "666666", "999999", "b7b7b7", "cccccc", "d9d9d9", "efefef", "f3f3f3", "ffffff",
  "980000", "ff0000", "ff9900", "ffff00", "00ff00", "00ffff", "4a86e8", "0000ff", "9900ff", "ff00ff",
  "e6b8af", "f4cccc", "fce5cd", "fff2cc", "d9ead3", "d0e0e3", "c9daf8", "cfe2f3", "d9d2e9", "ead1dc",
  "dd7e6b", "ea9999", "f9cb9c", "ffe599", "b6d7a8", "a2c4c9", "a4c2f4", "9fc5e8", "b4a7d6", "d5a6bd",
  "cc4125", "e06666", "f6b26b", "ffd966", "93c47d", "76a5af", "6d9eeb", "6fa8dc", "8e7cc3", "c27ba0",
];
export let recentColors = [];
export function pushRecent(hex) {
  const h = (hex || "").toUpperCase();
  if (!h) return;
  recentColors = [h, ...recentColors.filter((c) => c !== h)].slice(0, 10);
}
// Build a color popover into `menu`; `onPick(hex)` applies ("" clears).
// Manage Rules: the sheet's conditional formats in the order they are actually
// evaluated, with the reorder / stop-if-true / delete controls that order needs
// to be meaningful. Without this, rules could only be added and cleared
// wholesale, and which of two overlapping rules won was invisible.

// Format Cells (Ctrl+1): the number/font/alignment/fill/border controls in one
// place. The toolbar has all of these, but scattered — this is the dialog people
// reach for when they want to set several at once and see them together.

// The named cell-style gallery. Applying one writes its formatting *and*
// records which style the cells belong to, so the association survives a save —
// that link is the whole point of a named style over ad-hoc formatting.

// Parse the colour notations people actually paste: `#abc`, `abc`, `#aabbcc`,
// `aabbcc`, `rgb(1,2,3)` / `rgba(...)`, and `hsl(h,s%,l%)`. Returns `RRGGBB` or
// null. Accepting only 6-digit hex rejected half of what a designer copies.

// Shade a colour toward white (positive tint) or black (negative), the same way
// OOXML's `tint` attribute does — so the theme row's lighter/darker variants are
// the ones the file itself would produce.

// Flip `locked` / `hidden` over the selection. Both are style bits, so this
// goes through the same undoable range-styling path as bold.

// Whether the current sheet is protected, as the engine holds it.

// Hand the engine the wall clock and, optionally, a fresh random seed.
//
// The engine deliberately reads no clock of its own, so nothing volatile works
// until this has run. Called once at startup and again on every explicit
// recalculation; `reseed` is what makes RAND reroll rather than repeat.
let volatileSeed = 1;

// "Now", read in exactly one place.
//
// The engine reads no clock of its own — `syncClock` below is what hands it
// one — and the *static* stamps (Ctrl+; and Ctrl+Shift+;) have to come from
// the same reading, or the editor would hold two clocks that can disagree:
// a date typed into a cell one second either side of `session_set_clock` and
// a `TODAY()` beside it could then name different days, with nothing in the
// document to explain it. One seam is also what makes the pair testable —
// a fake clock installed on the page moves both.
export function hostNow() { return new Date(); }

export function syncClock(reseed = false) {
  if (!wasm) return;
  if (reseed) volatileSeed = (volatileSeed * 1103515245 + 12345) >>> 0;
  const now = hostNow();
  // Excel's epoch is 1899-12-30, and the serial is local time, not UTC — a
  // UTC serial puts TODAY() on the wrong day for most of the world's evening.
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60000);
  const serial = local.getTime() / 86400000 + 25569;
  // Swallowed: no user asked for this. It runs at boot and on every recalc,
  // before a session may even exist, and there is no control here whose failure
  // a message would explain.
  try { wasm.session_set_clock(serial, volatileSeed); } catch {}
}

// The sheet's display switches, as the engine holds them.
// Flip one of them. Undoable like any other sheet-level change.
// Subscript / superscript. Pressing the one already applied turns it off.
// One three-way choice: "overflow" (spill into empty neighbours), "wrap", or
// "clip" (stop at the cell edge). Wrap and clip are mutually exclusive, so the
// engine sets them together rather than exposing two toggles that can disagree.
// --- Format painter ---------------------------------------------------------
// Pick up the active cell's formatting, then apply it to the next selection.
// A single click paints once and disarms; a double-click stays armed so a
// format can be brushed onto several places, as in Excel. Escape cancels.
export let painter = null; // { row, col, sticky }
export function setPainter(next) {
  painter = next;
  const btn = byId("tb-painter");
  if (btn) btn.setAttribute("aria-pressed", next ? "true" : "false");
  canvas.style.cursor = next ? "copy" : "cell";
}
// Apply the picked-up format to a range. Returns whether it painted, so the
// caller can decide whether the click was consumed.



// Sort the current selection's rows by the active cell's column. A single-cell
// selection sorts the whole used data region (rows 1..end, keeping a header row
// out only if the caller selected a body range). Ascending unless `desc`.
// The block a sort should act on: the selection, or — for a lone cell — the
// whole used area, which is Excel's "sort this column" gesture.

// Whether a range's first row looks like column headings, by Excel's rule of
// thumb: a heading is text sitting over data that is not. Getting this wrong in
// either direction is destructive — sorting the heading into the middle of the
// data, or treating the first record as a heading and leaving it behind — so
// the answer is only ever a *default* for the dialog's checkbox.


// Run a sort, excluding the heading row when there is one.

// The custom number-format dialog. The engine understands far more codes than
// the preset menu offers — scientific, section colours, a text section — and
// without somewhere to type one, none of that is reachable. Previews against
// the active cell's own value, so you can see what the code does to *your*
// data before applying it.

// Remove duplicate rows from the selection (or the used area for a lone cell),
// keeping the first of each. Asks first, since it deletes rows, and reports how
// many went — "removed 0" is a useful answer too.

// The Sort dialog: choose up to three keys and say whether row 1 is a heading.
// The single-click A→Z / Z→A menu items stay for the common case; this is for
// when "sort by region, then by total descending" is what you actually meant.
// --- Autofilter -----------------------------------------------------------
// Per-column filtering, held by the engine rather than here: the rules live on
// the sheet (so they save to .xlsx and undo as one step) and the rows they hide
// are a set of their own, separate from rows hidden by hand. Clearing a filter
// therefore releases exactly the rows it hid.
export let filterInfo = null;    // the *sheet's* own filter: {r0,c0,r1,c1,cols:Set<absCol>,hidden} or null
let filterRegions = [];   // every filter on the sheet, tables included
export let filterHidden = 0;     // rows hidden by all of them together
export let filterButtons = [];   // hit targets rebuilt each frame by drawFilterButtons()

/// How much of this cell belongs to a filter arrow rather than to its label.
///
/// Only the header row of a filter range has one, and only when the column is
/// wide enough for `drawFilterRegion` to draw it at all — the two decisions are
/// the same decision, so they are made from the same numbers. Everywhere else
/// this is 0 and the text pass behaves exactly as it did.
function filterArrowReserve(row, col, cellW) {
  if (!(cellW >= 18)) return 0;
  for (const region of filterRegions) {
    if (row === region.r0 && col >= region.c0 && col <= region.c1) return FILTER_ARROW_W;
  }
  return 0;
}

function refreshFilterInfo() {
  filterInfo = null;
  filterRegions = [];
  filterHidden = 0;
  if (!wasm) return;
  try {
    const j = wasm.session_filter_info(state.sheet);
    if (j && j !== "null") {
      filterInfo = JSON.parse(j);
      filterInfo.cols = new Set(filterInfo.cols);
    }
  } catch {}
  try {
    // Regions come from the model rather than from table geometry, so a table
    // column carrying a rule draws as filtered like any other.
    const payload = JSON.parse(wasm.session_filter_regions(state.sheet));
    filterHidden = payload.hidden || 0;
    filterRegions = payload.regions.map((r) => ({ ...r, cols: new Set(r.cols) }));
  } catch {}
}

// Hide the rows a value-set excludes, for this participant only (COL-32).
//
// The engine computes *which* rows so that the personal and shared paths cannot
// disagree about the same tick-boxes; only where the answer is stored differs.

// Drop every personal view. A first-class command because undo will not do it:
// a personal view is not a document edit, so undo reverses the last change to
// the *document* instead.

// Turn the filter on over the current block, or off if one is already on.

// Draw a dropdown button on each header cell in the filter range, recording the
// hit targets. A column carrying a rule gets the accent treatment so it is
// obvious at a glance which columns are narrowing the view.
//
// `withQuad` is draw()'s pane clipper, passed in because it closes over the
// frame's frozen-pane geometry: a button on a header scrolled under a frozen
// pane must be clipped to its own pane, not painted over the frozen one.
function drawFilterButtons(withQuad) {
  filterButtons = [];
  // Every header that carries filter buttons: the sheet's own autofilter, plus
  // each table's. A table brings its own — Excel turns them on with the table,
  // and without them a table header reads as an ordinary shaded row.
  for (const region of filterRegions) drawFilterRegion(withQuad, region);
}


// Ink that reads against a cell fill (`RRGGBB`, no `#`). Uses the sRGB relative
// luminance the WCAG contrast ratio is built on, so a mid-tone fill flips at the
// point where white actually starts winning rather than at a guessed midpoint.
// No fill means the sheet background, so fall back to the theme foreground.


// The button under a canvas point, if any.

// The per-column dropdown: a searchable checklist plus a conditions submenu.

// Report what the filter now hides. The repaint already happened inside
// tryEdit, and draw() refreshes `filterInfo`, so this only reads the result.

// The two-comparison condition dialog for one column.
// --- Data validation (dropdown lists) -------------------------------------
// Open the value picker for the active cell's list validation.

// --- Tool side panel (data validation / conditional formatting / notes) ---
// One right-docked panel, tool-switched. It stays open while you keep selecting
// cells; the "Apply to range" readout tracks the live selection, and Apply acts
// on whatever is selected at click time.
export let activePanel = null;        // 'dv' | 'cf' | 'note' | 'table' | 'page' | null
export let panelRangeEls = [];        // range readouts to keep in sync on selection change
export let panelNote = null;          // { ta, addrEl, cell } while the note panel is open
// Half-typed replies, kept against the cell they were started on. Moving the
// selection used to empty the textarea outright: the reasoning was right — a
// draft belongs to its own thread and must not follow you to someone else's —
// but the remedy destroyed it, with no undo and no prompt, for a user who
// clicked another cell to re-read a figure they were about to quote.
const noteDrafts = new Map();

export const A1range = (s) =>
  (s.r0 === s.r1 && s.c0 === s.c1) ? A1(s.r0, s.c0) : `${A1(s.r0, s.c0)}:${A1(s.r1, s.c1)}`;


// The table panel: name, style and the six banding switches, all applied the
// moment you change them.
//
// Every control here is one `session_*` call and therefore one undo step — the
// panel holds no state of its own, so it cannot disagree with the workbook.

// The table under the cursor, as the engine reports it, or null.

// Write the style name and banding flags. Anything not named keeps its current
// value, so a single checkbox does not silently reset the other five.

// Rebuild the panel from the workbook after any change, so what it shows is
// what the model holds rather than what the last click intended.

// --- Chart panel -----------------------------------------------------------
//
// Which chart is being edited is remembered rather than looked up from the
// cursor: a chart floats over cells rather than occupying them, so the
// selection is not where it is.
export let panelChart = null;

const CHART_KINDS = [
  ["column", "Column"], ["bar", "Bar"], ["line", "Line"],
  ["area", "Area"], ["pie", "Pie"], ["doughnut", "Doughnut"], ["scatter", "Scatter"],
];

const LEGEND_POSITIONS = [
  ["", "None"], ["r", "Right"], ["b", "Bottom"], ["t", "Top"], ["l", "Left"],
];





export function buildChartPanel(body) {
  const c = currentChart();
  if (!c) {
    body.appendChild(el("div", "panel-note", "Select a chart, or Insert ▸ Chart."));
    return;
  }
  if (c.imported) {
    body.appendChild(el("div", "panel-note",
      "From the file. Changing anything here replaces the chart definition Excel "
      + `saved, and the formatting ${BRAND} does not model goes with it.`));
  }

  panelLabel(body, "Type");
  const kind = el("select", "panel-select");
  for (const [value, label] of CHART_KINDS) {
    const o = el("option", null, label);
    o.value = value;
    kind.appendChild(o);
  }
  kind.value = c.kind;
  kind.addEventListener("change", () => { c.kind = kind.value; applyChart(c); });
  body.appendChild(kind);

  const textField = (label, get, set) => {
    panelLabel(body, label);
    const input = el("input", "panel-field");
    input.type = "text";
    input.value = get();
    const commit = () => { set(input.value); applyChart(c); };
    input.addEventListener("change", commit);
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.stopPropagation(); input.blur(); }
      else if (e.key === "Escape") { e.stopPropagation(); input.value = get(); input.blur(); }
    });
    body.appendChild(input);
  };
  textField("Title", () => c.title, (v) => { c.title = v; });
  textField("Horizontal axis title", () => c.xTitle, (v) => { c.xTitle = v; });
  textField("Vertical axis title", () => c.yTitle, (v) => { c.yTitle = v; });

  panelLabel(body, "Legend");
  const legend = el("select", "panel-select");
  for (const [value, label] of LEGEND_POSITIONS) {
    const o = el("option", null, label);
    o.value = value;
    legend.appendChild(o);
  }
  legend.value = c.legend || "";
  legend.addEventListener("change", () => {
    c.legend = legend.value || null;
    applyChart(c);
  });
  body.appendChild(legend);

  panelLabel(body, "Series");
  c.series.forEach((s, i) => {
    const row = el("div", "chart-series");
    const name = el("input", "panel-field");
    name.type = "text";
    name.placeholder = "name";
    name.value = s.name;
    name.addEventListener("change", () => { s.name = name.value; applyChart(c); });
    const values = el("input", "panel-field");
    values.type = "text";
    values.placeholder = "Sheet1!$B$2:$B$9";
    values.value = s.values;
    values.addEventListener("change", () => { s.values = values.value; applyChart(c); });
    const remove = el("button", "pivot-mini pivot-remove", "✕");
    remove.title = "Remove this series";
    remove.addEventListener("click", () => {
      // The last one goes with the chart: a chart plotting nothing is a blank
      // rectangle nobody can tell from a bug.
      if (c.series.length === 1) {
        statusError("a chart needs at least one series — delete the chart instead");
        return;
      }
      c.series.splice(i, 1);
      applyChart(c);
    });
    const head = el("div", "chart-series-head");
    head.appendChild(name);
    head.appendChild(remove);
    row.appendChild(head);
    row.appendChild(values);
    body.appendChild(row);
  });

  const add = el("button", "panel-btn-ghost", "Add series from selection");
  add.addEventListener("click", () => {
    const r = effectiveRange();
    const sheetName = sheetNameAt(state.sheet);
    c.series.push({
      name: "",
      categories: c.series[0] ? c.series[0].categories : null,
      values: `${/^[A-Za-z_][A-Za-z0-9_.]*$/.test(sheetName) ? sheetName : `'${sheetName.replace(/'/g, "''")}'`}!$${colName(r.c0)}$${r.r0 + 1}:$${colName(r.c1)}$${r.r1 + 1}`,
    });
    applyChart(c);
  });
  body.appendChild(add);

  panelLabel(body, "Category labels");
  const cats = el("input", "panel-field");
  cats.type = "text";
  cats.placeholder = "Sheet1!$A$2:$A$9";
  cats.value = c.series[0]?.categories || "";
  cats.addEventListener("change", () => {
    // One category reference for the whole chart: OOXML lets each series carry
    // its own, but a chart whose series are labelled differently plots points
    // that do not line up, which is a mistake rather than a feature.
    for (const s of c.series) s.categories = cats.value || null;
    applyChart(c);
  });
  body.appendChild(cats);

  panelActions(body, "Move here", () => {
    const r = effectiveRange();
    c.anchor = [r.r0, r.c0, Math.max(r.r1, r.r0 + 8), Math.max(r.c1, r.c0 + 4)];
    applyChart(c);
  }, "Delete", async () => {
    if (!await confirmModal("Delete chart", "Delete this chart?", "Delete")) return;
    tryEdit(() => wasm.session_delete_chart(panelChart.sheet, panelChart.index));
    panelChart = null;
    refreshChartPanel();
  });
}

// Where each chart was last painted, in canvas pixels, plus which one is
// selected.
//
// Exposed because a chart is drawn on a canvas and has no DOM node: without
// this there is no way for an automated check — or a screen reader shim, or a
// screenshot differ — to say *where* a chart is, and verifying drag and resize
// means guessing at coordinates and reading the answer off a screenshot. It is
// a read-only view of state the renderer already computed.
window.openCalcChartFrames = () => ({
  frames: chartFrames.map((f) => ({ ...f })),
  selected: chartSel && chartSel.sheet === state.sheet ? chartSel.index : null,
  handles: chartSel
    ? chartHandlePoints(chartFrames.find((f) => f.index === chartSel.index) || { x: 0, y: 0, w: 0, h: 0 })
    : [],
});

// A press on a chart or one of its handles. Returns whether it was consumed.
function chartMouseDown(px, py) {
  // A handle on the already-selected chart wins over the frame under it: the
  // handles sit on the border, so a corner is inside both.
  if (chartSel && chartSel.sheet === state.sheet) {
    const frame = chartFrames.find((f) => f.index === chartSel.index);
    if (frame) {
      const points = chartHandlePoints(frame);
      for (let i = 0; i < points.length; i++) {
        const [hx, hy] = points[i];
        if (Math.abs(px - hx) <= CHART_HANDLE + 3 && Math.abs(py - hy) <= CHART_HANDLE + 3) {
          chartDrag = { index: chartSel.index, handle: i, x0: px, y0: py, px, py };
          return true;
        }
      }
    }
  }
  // Topmost first: charts overlap, and the one drawn last is the one you see.
  for (let i = chartFrames.length - 1; i >= 0; i--) {
    const f = chartFrames[i];
    if (px >= f.x && px <= f.x + f.w && py >= f.y && py <= f.y + f.h) {
      chartSel = { sheet: state.sheet, index: f.index };
      panelChart = { ...chartSel };
      chartDrag = { index: f.index, handle: null, x0: px, y0: py, px, py };
      if (activePanel === "chart") refreshChartPanel();
      draw();
      return true;
    }
  }
  if (chartSel) { chartSel = null; draw(); }
  return false;
}

// Commit a move or resize: pixels become cells, because a chart is anchored in
// cells and has to move with the rows under it.
function chartMouseUp() {
  if (!chartDrag) return;
  const rect = chartDragRect();
  const moved = Math.abs(chartDrag.px - chartDrag.x0) > 2
    || Math.abs(chartDrag.py - chartDrag.y0) > 2;
  const index = chartDrag.index;
  chartDrag = null;
  if (!rect || !moved) { draw(); return; }
  // Cell **plus offset**, not the nearest cell. Snapping to gridlines is why a
  // drag appeared to do nothing until it crossed one and then jumped a whole
  // column, and why a chart never came to rest where it was dropped.
  const from = anchorPoint(rect.x, rect.y);
  // The far corner is measured past the trailing edge of the cell it lands in,
  // which is what `to_offset` means — and what makes it survive a round trip
  // through `<xdr:to>`, whose offset is measured into the cell after.
  const to = anchorPoint(rect.x + rect.w, rect.y + rect.h);
  let r1 = Math.max(from.row, to.row - 1);
  let c1 = Math.max(from.col, to.col - 1);
  let def;
  try {
    def = JSON.parse(wasm.session_chart_defs(state.sheet))[index];
  } catch { return; }
  if (!def) return;
  def.anchor = [from.row, from.col, r1, c1];
  def.fromOffset = [pxToEmu(from.dx), pxToEmu(from.dy)];
  // Degenerate frames get no trailing offset: it would be measured from a cell
  // edge the frame does not reach.
  const degenerate = to.row - 1 < from.row || to.col - 1 < from.col;
  def.toOffset = degenerate ? [0, 0] : [pxToEmu(to.dx), pxToEmu(to.dy)];
  panelChart = { sheet: state.sheet, index };
  applyChart(def);
}

/// The cell a pixel lands in and how far into it, which is exactly what a
/// drawing anchor stores.

// Insert ▸ Chart: build one over the block under the cursor.
//
// The range is read the way Excel reads it — a text first row is headers, a
// text first column is labels — so the common case is one click. Everything the
// guess got wrong is in the panel, live.
function chartDialog(kind) {
  const r = effectiveRange();
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch { /* fall through to the selection */ }
  }
  try {
    const index = wasm.session_create_chart(
      state.sheet, bounds.r0, bounds.c0, bounds.r1, bounds.c1, kind);
    panelChart = { sheet: state.sheet, index };
  } catch (e) { statusError(errText(e)); return; }
  invalidateGrowth();
  draw();
  openPanel("chart");
  status.textContent = "chart added — drag it to move, or use Move here";
}

// --- Pivot panel -----------------------------------------------------------
//
// Excel's PivotTable Fields pane, in one column: a field list you drag from,
// four areas you drag into, and the options underneath.
//
// The panel holds no state of its own. Every change rebuilds the whole
// definition from the DOM, sends it as one `session_set_pivot`, and redraws
// from what came back — so the panel cannot drift from the workbook, and each
// change is one undo step covering both the layout and the figures it produced.
//
// Which pivot is being edited is remembered rather than looked up from the
// cursor, because a pivot with nothing on its axes has written nothing yet and
// so covers no cell to find it by.
export let panelPivot = null;

export const PIVOT_AREAS = [
  ["filters", "Filters"],
  ["cols", "Columns"],
  ["rows", "Rows"],
  ["values", "Values"],
];

export const PIVOT_AGGREGATES = [
  ["sum", "Sum"], ["count", "Count"], ["countNums", "Count numbers"],
  ["average", "Average"], ["max", "Max"], ["min", "Min"],
  ["product", "Product"], ["stdDev", "StdDev"], ["stdDevp", "StdDevp"],
  ["var", "Var"], ["varp", "Varp"],
];

// Sort cycles rather than opening a menu: three states, one button, and the
// glyph says which one it is in.
export const PIVOT_SORTS = [
  ["ascending", "↑", "A→Z / smallest first"],
  ["descending", "↓", "Z→A / largest first"],
  ["dataSource", "⇅", "source order"],
];

// The name of the pivot whose report covers a cell, or "" — the guard the
// editor checks before letting anything be typed there.



// Send the whole definition. `p` is the object `session_pivots` handed out,
// mutated in place by whichever control was touched.


// Where a field currently sits, so the field list can grey out what is in use.

export function buildPivotPanel(body) {
  const p = currentPivot();
  if (!p) {
    body.appendChild(el("div", "panel-note",
      "Select a cell inside a pivot table, or Insert ▸ PivotTable."));
    return;
  }

  panelLabel(body, "Name");
  const name = el("input", "panel-field");
  name.type = "text";
  name.value = p.name;
  name.addEventListener("change", () => {
    const want = name.value.trim();
    if (!want || want === p.name) { name.value = p.name; return; }
    p.name = want;
    applyPivot(p);
  });
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.stopPropagation(); name.blur(); }
    else if (e.key === "Escape") { e.stopPropagation(); name.value = p.name; name.blur(); }
  });
  body.appendChild(name);

  const src = el("div", "panel-range",
    `${sheetNameAt(p.sourceSheet) || "?"}!${A1(p.source[0], p.source[1])}:${A1(p.source[2], p.source[3])}`);
  body.appendChild(src);
  if (p.imported) {
    body.appendChild(el("div", "panel-note",
      `From the file. Refreshing rewrites it in ${BRAND}'s layout and replaces `
      + "the definition Excel saved."));
  }

  // --- the field list, and the four areas ---------------------------------
  panelLabel(body, "Fields");
  const list = el("div", "pivot-fields");
  p.fields.forEach((label, index) => {
    const chip = el("div", "pivot-chip pivot-chip-src", label || `Column${index + 1}`);
    chip.draggable = true;
    const where = pivotPlacement(p, index);
    if (where) chip.classList.add("in-use");
    chip.title = where ? `in ${where}` : "drag into an area below";
    chip.addEventListener("dragstart", (e) => {
      e.dataTransfer.setData("text/plain", JSON.stringify({ from: "fields", field: index }));
      e.dataTransfer.effectAllowed = "copy";
    });
    // Double-click is the keyboard-free shortcut Excel has too: it drops the
    // field where its type suggests — numbers summarize, text groups.
    chip.addEventListener("dblclick", () => {
      if (where) return;
      const target = pivotFieldIsNumeric(p, index) ? "values" : "rows";
      pivotAdd(p, target, index, p[target].length);
    });
    list.appendChild(chip);
  });
  body.appendChild(list);

  for (const [key, title] of PIVOT_AREAS) {
    panelLabel(body, title);
    const zone = el("div", "pivot-zone");
    zone.dataset.area = key;
    if (!p[key].length) zone.appendChild(el("div", "pivot-zone-empty", "drag a field here"));
    p[key].forEach((f, i) => zone.appendChild(pivotChip(p, key, f, i)));
    zone.addEventListener("dragover", (e) => {
      e.preventDefault();
      zone.classList.add("over");
    });
    zone.addEventListener("dragleave", () => zone.classList.remove("over"));
    zone.addEventListener("drop", (e) => {
      e.preventDefault();
      zone.classList.remove("over");
      let payload;
      try { payload = JSON.parse(e.dataTransfer.getData("text/plain")); } catch { return; }
      pivotDrop(p, key, payload, pivotDropIndex(zone, e.clientY));
    });
    body.appendChild(zone);
  }

  // --- options -------------------------------------------------------------
  panelLabel(body, "Totals");
  for (const [key, label] of [["rowGrandTotals", "Grand total row"],
                              ["colGrandTotals", "Grand total column"]]) {
    const row = el("label", "panel-check");
    const box = el("input");
    box.type = "checkbox";
    box.checked = !!p[key];
    box.addEventListener("change", () => { p[key] = box.checked; applyPivot(p); });
    row.appendChild(box);
    row.appendChild(el("span", null, label));
    body.appendChild(row);
  }

  panelLabel(body, "Style");
  const style = el("select", "panel-select");
  for (const [label, value] of TABLE_STYLES) {
    const o = el("option", null, label);
    o.value = value;
    style.appendChild(o);
  }
  style.value = p.style || "TableStyleMedium2";
  style.addEventListener("change", () => { p.style = style.value; applyPivot(p); });
  body.appendChild(style);

  panelActions(body, "Refresh", () => {
    try {
      wasm.session_refresh_pivot(panelPivot.sheet, panelPivot.index);
      status.textContent = "pivot refreshed";
    } catch (e) { statusError(errText(e)); }
    invalidateGrowth();
    draw();
    refreshPivotPanel();
  }, "Delete", async () => {
    if (!await confirmModal("Delete pivot table",
      `Delete “${p.name}” and the report it wrote?`, "Delete")) return;
    tryEdit(() => wasm.session_delete_pivot(panelPivot.sheet, panelPivot.index));
    panelPivot = null;
    refreshPivotPanel();
  });
}

// Whether a field's data reads as numbers, which is what decides whether a
// double-click summarizes it or groups by it — Excel's own rule.

// One placed field: its name, the controls its area gives it, and a remove
// button.

// The page filter's checklist, inline under its chip rather than in a popup:
// the panel is already a narrow column, and a floating list over it would cover
// the thing being filtered.

// Which slot a drop lands in: above the first chip whose midpoint is below the
// pointer. Order is the nesting order, so this is not cosmetic — dropping
// Region above Product is a different report from the other way round.



// A field carries what its new area needs and drops what it does not: an
// aggregate is meaningless on the row axis, and a sort order is meaningless on
// a measure.

// Insert ▸ PivotTable: build one over the block under the cursor, on a new
// sheet.
//
// A new sheet is Excel's own default and the only destination that cannot
// collide with something already on the grid. Nothing else is asked: every
// choice a dialog would make is in the panel, live, and one undo step away.
async function pivotDialog() {
  const here = pivotAt(state.sel.row, state.sel.col);
  if (here) { panelPivot = { sheet: state.sheet, index: here.index }; openPanel("pivot"); return; }

  const r = effectiveRange();
  let bounds = r;
  if (r.r0 === r.r1 && r.c0 === r.c1) {
    try {
      const blk = JSON.parse(wasm.session_block_bounds(state.sheet, r.r0, r.c0));
      if (blk) bounds = { r0: blk.r0, c0: blk.c0, r1: blk.r1, c1: blk.c1 };
    } catch { /* fall through to the selection */ }
  }
  if (bounds.r1 <= bounds.r0) {
    statusError("select a table with a header row and at least one row of data");
    return;
  }
  const source = state.sheet;
  let dest;
  try {
    dest = wasm.session_add_sheet();
    const index = wasm.session_create_pivot(
      source, bounds.r0, bounds.c0, bounds.r1, bounds.c1, dest, 0, 0, "");
    panelPivot = { sheet: dest, index };
  } catch (e) { statusError(errText(e)); return; }
  renderTabs();
  switchSheet(dest);
  invalidateGrowth();
  draw();
  openPanel("pivot");
  status.textContent = "drag a field into Values to see the report";
}

// --- Localization -------------------------------------------------------------
//
// Two injection points, because they serve two different people: a host that
// already knows its user's language supplies one, and a user who wants to
// choose picks from the footer.
//
// Messages are keyed by **command id**, which is derived from the *English*
// label. That is deliberate and load-bearing: translating the labels must not
// renumber the command API, so `commandId()` always sees English and only the
// rendered text changes. Getting this backwards would mean `format.bold`
// becoming `format.fett` for a German host, and every `commands({hidden})` list
// silently ceasing to match.
//
// What is covered today: menu items, submenu headings and toolbar tooltips —
// the labels a user reads constantly. Panels and dialogs are still English
// until their catalogues are written; a missing key falls back to the English
// string rather than showing a key, so partial coverage degrades to "some of it
// is translated" rather than to gibberish.

export let locale = "en-US";
export const messages = new Map();

/// Look a message up, falling back to the English text it was keyed from.

/// Install a catalogue. Merges, so a host can override three strings without
/// restating the language.

export function setLocale(next) {
  locale = next || "en-US";
  syncLocalePicker();
  relabel();
}

/// Show or hide the footer language control, and fill it.



/// The locales a catalogue has been supplied for, plus the built-in one.

/// Which menu each Alt mnemonic opens. Rebuilt whenever the labels change,
/// because a translated menu bar has different letters free.
export const menuMnemonics = new Map();

/// Label the top-level menu bar and assign its Alt mnemonics.
///
/// Not always the first letter: File and Format both start with F, so the
/// naive rule left Format unreachable *and* advertising a shortcut belonging to
/// File. Take the first character not already claimed — which is how Windows
/// menus have always assigned these, and which has to be recomputed per
/// language rather than baked in at build time.

/// Re-render every label that came from a catalogue.
///
/// Cheaper and far less error-prone than rebuilding the menus: each labelled
/// node remembers its English source in a data attribute, so relabelling is a
/// pass over the DOM rather than a teardown.

// --- Events ------------------------------------------------------------------
//
// `before*` / past-tense pairs, `before*` cancellable, every event carrying a
// `source`. That is Handsontable's design and it is right for two reasons a
// host feels immediately: without cancellation it cannot enforce its own
// permissions, and without a `source` it cannot tell its own programmatic
// write from a user's keystroke — so it echoes its own change back into its
// store and loops.
//
// Granularity is **one event per operation**, carrying a range. A paste of a
// hundred thousand cells is one `cellsChanged`, not a hundred thousand of them,
// which matches how the transaction layer already batches and is the only
// version that stays usable at size.

export const listeners = new Map();

/// Subscribe. Returns an unsubscribe function, so a caller need not keep the
/// handler around to remove it later.


/// Emit, returning false if a `before*` handler cancelled.
///
/// A throwing handler must not take the editor down with it: a host's bug in a
/// change listener would otherwise make the grid unusable, which is a far worse
/// failure than the one they wrote.

/// The last state reported, so a change is only announced when it changed.
export const lastReported = { selection: "", dirty: null, calc: "", undo: "" };

/// Emit whatever has changed since the previous frame.
///
/// Polled from `draw()` rather than fired at each mutation site: there are
/// dozens of those and one of them will always be forgotten, whereas the paint
/// is the one place everything already funnels through.

// --- Commands ---------------------------------------------------------------
//
// Every menu item and toolbar button carries a stable id, so a host can hide or
// disable *individual* controls rather than whole regions — and so read-only
// can take editing off the menus rather than only refusing the keystroke.
//
// Ids are derived from the English label path (`Format ▸ Alignment ▸ Left` →
// `format.alignment.left`) rather than hand-assigned. That keeps them
// predictable and impossible to forget on a new item; the cost is that
// renaming a label renames its id, which the docs state plainly and which we
// treat as a breaking change like any other API rename.


/// Commands a viewer may still run.
///
/// A **whitelist**, deliberately. With a blacklist, an editing command added
/// later and not added to the list leaks into read-only mode; with a whitelist
/// the worst case is that something harmless is hidden until someone notices.
/// Getting that backwards is the difference between a cosmetic bug and a
/// workbook a viewer could change.
const READ_ONLY_SAFE = [
  // Reading the sheet, in every sense.
  /^edit\.copy$/, /^edit\.select-all$/,
  // No toolbar entry: every control on it applies formatting, the format
  // painter included — it reads a style from one cell and *writes* it to
  // another, which a first pass wrongly let through.
  // Zoom is the editor's own state, not the workbook's — a viewer must be able
  // to zoom.
  /^view\.zoom/,
  // Recalculating is not an edit: it produces the values the formulas already
  // imply, and a viewer that cannot compute is showing stale numbers.
  /^tools\.calculation/,
  // Getting a copy out, and putting it on paper.
  /^file\.download/, /^file\.print$/,
  /^help\./,
];

// Everything else in `view.` is deliberately absent. Freezing panes, hiding
// gridlines, showing formulas and showing zeros all live in the *workbook* —
// they travel in the file and go through `SetSheetMetadata`, which a read-only
// session refuses. Offering them would be offering something that then fails.

export const isReadOnlySafe = (id) => READ_ONLY_SAFE.some((re) => re.test(id));

/// Host-supplied `{ hidden: [], disabled: [] }`.
export let commandRules = { hidden: [], disabled: [] };

/// Every command id present in this mount, for a host that wants to discover
/// them rather than read a list in the docs that can go stale.

/// Run a command by id.
///
/// The SDK could **list** commands and **hide or disable** them, and never run
/// one — so a host with its own toolbar could discover our commands, suppress
/// ours, and had no way to put one on its own button. That is the case an
/// embedder actually has: a product that already owns its chrome (`SDK-010`).
///
/// A command *is* its control, so running one is activating that control rather
/// than a second dispatch table beside the first. Two tables drift, and the
/// drift shows up as a menu item that works and a host button that does
/// nothing.
///
/// Refuses an unknown id rather than doing nothing quietly: a host that
/// mistypes learns at the call, not from a user reporting a dead button.
/// Refuses a *disabled* one too, because `commands({ disabled })` is a promise
/// the host made to itself and honouring it only in the menu would be no
/// promise at all.

/// Hide or disable commands by id.
export function setCommandRules(rules) {
  commandRules = {
    hidden: rules?.hidden ?? [],
    disabled: rules?.disabled ?? [],
  };
  applyCommandRules();
}

/// Apply the host's rules *and* read-only, which hides every editing command.
///
/// Hidden rather than disabled for read-only: a viewer is not a broken editor,
/// and a menu of greyed-out things a user can never enable is worse than a
/// short menu. A host that would rather grey them can say so per command.

// --- Calculation mode ------------------------------------------------------
//
// Excel's Formulas ▸ Calculation Options. A workbook saved with calculation
// turned off opens that way, so this is not a preference the editor invents —
// it is state the file carries and the user has to be able to see and change.



// --- Stopping a long job (`SEC-017`) ---------------------------------------
//
// The engine can be stopped mid-job; until now nothing here asked it to, so a
// workbook that was inside every admission bound and simply enormous held the
// tab's one thread until it finished.
//
// **The limit is a number set up front, not a Stop button, and that is forced
// rather than chosen.** JavaScript and WebAssembly share the one thread a tab
// has: while the engine runs, no click handler runs, so a Stop button's
// listener could not fire until the job it was meant to stop had already
// returned. What the editor can do is bound the job before starting it and
// hand the choice back afterwards — which is what "Keep waiting" is.

/// How long an open may hold the thread before it is stopped.
export let openBudgetMs = 10_000;
/// The same for a full recalculation (F9), which is the other long job.
export let recalcBudgetMs = 5_000;

/// Set both limits, for the browser gate.
///
/// Exported because the only honest test of a cancellation is one that provokes
/// it, and provoking a ten-second limit for real would mean a ten-second test
/// and a workbook big enough to need one. A negative value means no limit.
export function setTimeBudgetsForTest(openMs, recalcMs) {
  openBudgetMs = openMs;
  recalcBudgetMs = recalcMs;
}

/// Whether an engine error is "this job was stopped" rather than "this file is
/// bad".
///
/// Matched on the stable diagnostic code (docs/20) rather than the prose: the
/// sentence is allowed to be reworded, the code is not.

/// Take away any outstanding "Keep waiting" offer.

/// Offer to re-run `again` with no limit at all.
///
/// A limit the user cannot overrule is not a limit, it is a refusal: a workbook
/// that genuinely takes twenty seconds is still that user's workbook, and this
/// is the whole of the "cancel" affordance a single-threaded host can honestly
/// offer — the choice comes *after* the stop, because nothing the user does can
/// arrive during one.
///
/// The button is a sibling of the status text rather than a child of it: the
/// status bar is one ellipsised line with a `max-width`, so a control inside it
/// is a control clipped out of reach.

// F9: recompute everything, and reseed first so RAND rerolls — Excel does the
// same. Not an undoable edit and it does not dirty the document: the values it
// produces are the ones the formulas already imply.
//
// `budgetMs` is how long it may run; a negative one means no limit, which is
// what "Keep waiting" retries with.

/// Whether the session refuses edits.

/// Open the workbook for reading only, or release it.

// Whether an edit is waiting on a manual calculation, for the status bar —
// Excel writes "Calculate" there and it is the only cue that what is on screen
// is not what the formulas say.

// Alt+F5 — recompute the pivot under the cursor from its source.

// Ctrl+Alt+F5 — every pivot in the workbook.
//
// One refusal does not fail the command: the others are still worth
// recomputing, and the ones that could not are named rather than counted.

// Page setup: everything OOXML records about printing a sheet, all of which was
// being carried through every save with nothing able to change it.
//
// Applied on change, one `session_set_page_setup` call each, so every switch is
// its own undo step and the panel never holds state the workbook does not.

// Open the sheet as a printable page and hand it to the browser's print dialog.
//
// A separate window rather than a print stylesheet over the app: the grid is a
// canvas, so there is nothing for a stylesheet to lay out across pages.

export function openPanel(tool) {
  const panel = byId("side-panel");
  activePanel = tool;
  panelRangeEls = [];
  panelNote = null;
  byId("side-panel-title").textContent =
    tool === "dv" ? "Data validation"
      : tool === "cf" ? "Conditional formatting"
      : tool === "table" ? "Table"
      : tool === "pivot" ? "PivotTable fields"
      : tool === "chart" ? "Chart"
      : tool === "page" ? "Page setup"
      : tool === "stats" ? "Column stats"
      : tool === "history" ? "Version history"
      : "Comments";
  const body = byId("side-panel-body");
  body.textContent = "";
  if (tool === "dv") buildDvPanel(body);
  else if (tool === "cf") buildCfPanel(body);
  else if (tool === "table") buildTablePanel(body);
  else if (tool === "pivot") buildPivotPanel(body);
  else if (tool === "chart") buildChartPanel(body);
  else if (tool === "page") buildPagePanel(body);
  else if (tool === "stats") buildStatsPanel(body);
  else if (tool === "history") buildHistoryPanel(body);
  else buildNotePanel(body);
  panel.hidden = false;
  resize(); // the grid narrows — refit the canvas to its new width
}

// --- Column stats -----------------------------------------------------------
//
// What a person needs when somebody sends them a column: how much of it there
// is, how much is missing, and *what it is made of*. The type distribution is
// the part the status bar cannot give — it is how you find the one text cell
// wrecking a SUM, or the 300 numbers stored as text that make a filter behave
// oddly. The engine computes it (`session_column_stats`); this only lays it out.
/// Version history (`HIST-01`).
///
/// The engine and SDK half was built by `SAVE-08` and nothing could reach it.
/// Undo was the only route backwards and it dies with the tab, which is the
/// largest single gap against every competitor named in `docs/12`.
///
/// **What a version is here is a snapshot, not a replayed log.** `docs/83`'s
/// main negative result: the collaboration server's op log looks like a history
/// and is not one — no timestamps, no per-revision author, a few hundred ops
/// retained, evicted thirty seconds after the last participant leaves
/// (`SAVE-09`) — and `COL-50` independently rules out the replay a log-based
/// history would need.
///
/// The clock is passed in from here, because the engine has none (`AGENTS.md`).
function buildHistoryPanel(body) {
  const render = () => {
    body.textContent = "";

    let versions = [];
    try { versions = JSON.parse(wasm.session_versions() || "[]"); }
    catch (why) { statusError(errText(why)); return; }

    const bar = el("div", "hist-actions");
    const keep = el("button", "btn", "Save a version");
    keep.title = "Capture the document as it is now";
    keep.addEventListener("click", () => {
      try {
        const r = JSON.parse(wasm.session_capture_version("named", nameBox.value, Date.now()));
        // Written to storage on the same action that created it, rather than on
        // a timer: a version the user asked for and a reload lost would be
        // worse than no version, because they would believe they had it.
        persistVersions(wasm).catch(() => {});
        // `stored: false` is not a failure — the store resolves a capture of an
        // unchanged document to the version already holding that state, and
        // saying so is more honest than writing a duplicate.
        status.textContent = r.stored ? "version saved" : "no changes since the last version";
        nameBox.value = "";
        render();
      } catch (why) { statusError(errText(why)); }
    });
    const nameBox = el("input", "hist-name");
    nameBox.placeholder = "Name this version (optional)";
    nameBox.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { e.preventDefault(); keep.click(); }
      e.stopPropagation();
    });
    bar.appendChild(nameBox);
    bar.appendChild(keep);
    body.appendChild(bar);

    if (!versions.length) {
      const empty = el("div", "hist-empty",
        "No versions yet. Saving one keeps a copy of the document you can come back to.");
      body.appendChild(empty);
      return;
    }

    const used = el("div", "hist-budget",
      `${versions.length} version${versions.length === 1 ? "" : "s"}, ${fmtBytes(wasm.session_versions_bytes())}`);
    body.appendChild(used);

    const list = el("div", "hist-list");
    for (const v of versions) {
      const row = el("div", "hist-row");
      const when = new Date(v.at);
      const label = v.name || (v.kind === "saved" ? "Saved" : v.kind === "named" ? "Named" : "Autosave");
      // `textContent` throughout: a version name is text the user typed.
      row.appendChild(el("span", "hist-when", when.toLocaleString()));
      row.appendChild(el("span", "hist-label", label));
      row.appendChild(el("span", "hist-size", fmtBytes(v.bytes)));

      const restore = el("button", "btn hist-restore", "Restore");
      restore.addEventListener("click", async () => {
        let plan;
        try { plan = JSON.parse(wasm.session_plan_restore(v.id)); }
        catch (why) { statusError(errText(why)); return; }
        if (plan.empty) { status.textContent = "this version matches the document already"; return; }

        // **The plan is shown before the restore, and the losses are named.**
        // "This will change 412 cells" and "this will change 412 cells and
        // cannot bring back two images" are different sentences, and only the
        // second lets somebody decline for the right reason.
        const lines = [`${plan.cellsChanged} cell${plan.cellsChanged === 1 ? "" : "s"} will change.`];
        if (plan.sheetsAdded) lines.push(`${plan.sheetsAdded} sheet(s) will come back.`);
        if (plan.sheetsRemoved) lines.push(`${plan.sheetsRemoved} sheet(s) will be removed.`);
        if (plan.unexpressed.length) {
          lines.push(`${plan.unexpressed.length} thing(s) cannot be restored: ${plan.unexpressed.join(", ")}.`);
        }
        lines.push("Your work as it is now is kept as a version first, so this can be undone.");
        if (!(await confirmModal(`Restore "${label}"?`, lines.join(" "), "Restore"))) return;

        try {
          const done = JSON.parse(wasm.session_restore_version(v.id, Date.now()));
          // One `Operation::Batch` of ordinary edits: it travels to
          // collaborators as edits and costs exactly one undo step, because a
          // batch has one combined inverse.
          // The restore captured the present as a version before it landed;
          // that one is the way back, so it has to reach storage too.
          persistVersions(wasm).catch(() => {});
          status.textContent = `restored — ${done.cellsChanged} cell(s) changed, undo will put it back`;
          draw();
          renderTabs();
          render();
        } catch (why) { statusError(errText(why)); }
      });
      row.appendChild(restore);

      const hide = el("button", "btn-quiet hist-hide", "Hide");
      hide.title = "Remove from this list. The version is kept.";
      hide.addEventListener("click", () => {
        wasm.session_hide_version(v.id);
        // Hiding is a display choice, and losing it on reload would un-hide
        // everything the user had tidied away — which is why the manifest
        // carries hidden entries rather than omitting them.
        persistVersions(wasm).catch(() => {});
        render();
      });
      row.appendChild(hide);
      list.appendChild(row);
    }
    body.appendChild(list);
  };
  render();
}

/// Bytes as a person reads them. The store counts uncompressed bytes, which is
/// what the retention arithmetic uses — `SAVE-13` is why compressing them is the
/// host's job and not the engine's.
function fmtBytes(n) {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i += 1; }
  return `${n < 10 && i > 0 ? n.toFixed(1) : Math.round(n)} ${units[i]}`;
}

/// Turn the selection's numbers-stored-as-text into numbers (`DATA-NT-01`).
///
/// The engine keeps `"10"` as text and is right to — coercing on the way in
/// would silently change somebody's data. What this adds is a way to say "these
/// were meant to be numbers", which is what Excel's *Convert to Number* does.
///
/// It reports the count rather than doing it silently: converting nothing and
/// converting four hundred cells look identical on a screen the user is not
/// scrolled to.
function convertTextToNumbers() {
  const r = effectiveRange();
  let changed = 0;
  try { changed = wasm.session_convert_text_to_numbers(state.sheet, r.r0, r.c0, r.r1, r.c1); }
  catch (why) { statusError(errText(why)); return; }
  if (!changed) {
    status.textContent = "nothing in the selection is a number stored as text";
    return;
  }
  recalculateNow();
  draw();
  status.textContent = `converted ${changed} cell${changed === 1 ? "" : "s"} to numbers`;
}

function buildStatsPanel(body) {
  const render = () => {
    body.textContent = "";
    let s = effectiveRange();
    // A single cell means "this column", which is what the feature is called
    // and what Google's does. Asking somebody to select the column first is
    // work the app can do — and reporting on one cell would make the panel a
    // worse status bar rather than a different tool. Bounded to the used rows,
    // so it describes the data rather than a million empty addresses.
    if (s.r0 === s.r1 && s.c0 === s.c1) {
      const b = usedBounds();
      s = { r0: 0, c0: s.c0, r1: Math.max(0, b.rows - 1), c1: s.c1 };
    }
    let d = null;
    try { d = JSON.parse(wasm.session_column_stats(state.sheet, s.r0, s.c0, s.r1, s.c1) || "null"); }
    catch (why) { statusError(errText(why)); return; }
    if (!d) return;

    const num = (v) =>
      v === null || v === undefined ? "—"
        : Math.abs(v) >= 1e15 || (v !== 0 && Math.abs(v) < 1e-9) ? v.toExponential(4)
          : String(Math.round(v * 1e10) / 1e10);

    const section = (title) => {
      const h = el("div", "stats-head", title);
      body.appendChild(h);
      const t = el("div", "stats-rows");
      body.appendChild(t);
      return t;
    };
    const line = (into, label, value, hint) => {
      const row = el("div", "stats-row");
      row.appendChild(el("span", "stats-label", label));
      const v = el("span", "stats-value", value);
      if (hint) v.title = hint;
      row.appendChild(v);
      into.appendChild(row);
    };

    const head = el("div", "stats-range", d.cols === 1 ? `Column ${colName(s.c0)}` : A1range(s));
    body.appendChild(head);

    const counts = section("Counts");
    line(counts, "Cells", String(d.cells));
    line(counts, "With a value", String(d.count));
    // Named apart, always. A blank is not a zero, and the commonest question
    // about a column somebody sent you is how much of it is missing.
    line(counts, "Empty", String(d.empty));
    line(
      counts,
      "Unique",
      d.uniqueExact ? String(d.unique) : `${d.unique}+`,
      d.uniqueExact ? "" : "counted up to the distinct-value limit",
    );
    if (d.truncated) {
      line(counts, "Scanned", "partial", "the range was larger than the scan budget");
    }

    if (d.numeric && d.numeric.count > 0) {
      const n = section("Numbers");
      line(n, "Count", String(d.numeric.count));
      line(n, "Sum", num(d.numeric.sum));
      line(n, "Average", num(d.numeric.avg));
      line(n, "Median", num(d.numeric.median));
      line(n, "Min", num(d.numeric.min));
      line(n, "Max", num(d.numeric.max));
      line(n, "Std dev", num(d.numeric.stdev), "sample (n−1), as STDEV.S");
    }

    // The reason this panel exists rather than a wider status bar.
    const t = section("What it is made of");
    const kinds = [
      ["Numbers", d.types.number],
      ["Dates", d.types.date],
      ["Text", d.types.text],
      ["Booleans", d.types.boolean],
      ["Errors", d.types.error],
    ].filter(([, n]) => n > 0);
    if (!kinds.length) line(t, "Nothing", "—");
    for (const [label, n] of kinds) line(t, label, String(n));
    if (d.types.numberAsText > 0) {
      // The finding, not a statistic: these look like numbers and do not add up.
      const row = el("div", "stats-row warn");
      row.appendChild(el("span", "stats-label", "Numbers stored as text"));
      row.appendChild(el("span", "stats-value", String(d.types.numberAsText)));
      t.appendChild(row);
    }
    if (d.types.formula > 0) line(t, "Formulas", String(d.types.formula), "counted across the kinds above");
    for (const [code, n] of Object.entries(d.errors || {})) line(t, code, String(n));

    if (d.frequency && d.frequency.length) {
      const f = section("Most common");
      for (const e of d.frequency) line(f, e.value === "" ? "(blank)" : e.value, String(e.count));
      if (d.frequencyOther && d.frequencyOther.values > 0) {
        line(
          f,
          `${d.frequencyOther.values} other value${d.frequencyOther.values === 1 ? "" : "s"}`,
          String(d.frequencyOther.count),
        );
      }
    }
  };

  render();
  // Recomputed as the selection moves, which is how the panel is used: click a
  // column, read it, click the next.
  panelStatsRefresh = render;
}
let panelStatsRefresh = null;

export function closePanel() {
  const panel = byId("side-panel");
  if (panel.hidden) return;
  panel.hidden = true;
  activePanel = null;
  panelRangeEls = [];
  panelNote = null;
  resize();
  canvas.focus();
}

// Toggle a tool: clicking its button again (or a different tool) re-targets.

// Keep the panel in step with the selection (called from draw()).
function refreshPanel() {
  if (activePanel === "dv" || activePanel === "cf") {
    const t = A1range(effectiveRange());
    for (const r of panelRangeEls) r.textContent = t;
  } else if (activePanel === "table") {
    // Moving into a different table has to re-target the panel, or renaming
    // would rename whichever table it was opened on.
    const t = currentTable();
    const shown = byId("side-panel-body").dataset.table;
    const key = t ? `${t.name}@${t.r0},${t.c0}` : "";
    if (key !== shown) {
      byId("side-panel-body").dataset.table = key;
      refreshTablePanel();
    }
  } else if (activePanel === "pivot") {
    // Clicking into a different pivot's report re-targets the panel. Clicking
    // outside every report does not: a pivot with nothing on its axes has
    // written no cells to be found by, and dropping the panel the moment the
    // cursor moved would make it unusable exactly when it is being set up.
    const here = pivotAt(state.sel.row, state.sel.col);
    if (here && (!panelPivot || panelPivot.sheet !== state.sheet || panelPivot.index !== here.index)) {
      panelPivot = { sheet: state.sheet, index: here.index };
      refreshPivotPanel();
    }
  } else if (activePanel === "stats" && panelStatsRefresh) {
    panelStatsRefresh();
  } else if (activePanel === "note" && panelNote) {
    const addr = A1(state.sel.row, state.sel.col);
    if (addr !== panelNote.cell) {
      // Parked, not discarded. The draft still does not follow the cursor to
      // another thread — it waits on the one it belongs to, and comes back if
      // the user does. An emptied box parks nothing, so posting a reply and
      // moving on leaves no stale text to reappear later.
      if (panelNote.ta.value.trim()) noteDrafts.set(panelNote.cell, panelNote.ta.value);
      else noteDrafts.delete(panelNote.cell);
      panelNote.cell = addr;
      panelNote.ta.value = noteDrafts.get(addr) ?? "";
      panelNote.refresh();
    }
  }
}



// Who new comments and replies are signed as. There is no account to read a
// name from in the browser, so it is asked for once and kept; an empty name is
// allowed and simply leaves the comment unsigned rather than blocking the edit.

// The timestamp new comments carry, in the shape OOXML wants (`dT`). Produced
// here rather than in the engine so the engine stays a pure function of its
// inputs — the same edits always yield the same workbook.

// "3 minutes ago" / "8 Aug 2026" — a thread is read by when things were said
// relative to now, and an absolute timestamp makes that arithmetic the reader's
// problem. The full stamp stays on the `title` for anyone who needs it.


function buildNotePanel(body) {
  const addrEl = el("div", "panel-range", A1(state.sel.row, state.sel.col));
  body.appendChild(addrEl);

  const thread = el("div", "cmt-thread");
  body.appendChild(thread);

  const ta = el("textarea", "panel-field");
  ta.rows = 3; ta.spellcheck = false;
  body.appendChild(ta);

  const who = el("input", "panel-field cmt-author");
  who.type = "text"; who.placeholder = "Your name (optional)";
  who.value = commentAuthor();
  who.addEventListener("change", () => setCommentAuthor(who.value.trim()));
  body.appendChild(who);

  const hint = el("div", "panel-hint", "");
  body.appendChild(hint);

  // `render` reads the thread back from the model after every change rather
  // than patching the DOM it just built, so the panel can never drift from
  // what was actually stored.
  const render = () => {
    const row = state.sel.row, col = state.sel.col;
    const t = readThread(row, col);
    thread.textContent = "";
    addrEl.textContent = A1(row, col);
    const entry = (e, isRoot) => {
      const box = el("div", "cmt-entry" + (isRoot ? " cmt-root" : ""));
      const head = el("div", "cmt-head");
      head.appendChild(el("span", "cmt-who", e.author || "Anonymous"));
      const when = el("span", "cmt-when", relativeTime(e.created));
      if (e.created) when.title = e.created;
      head.appendChild(when);
      box.appendChild(head);
      box.appendChild(el("div", "cmt-text", e.text));
      return box;
    };
    if (t) {
      if (t.resolved) thread.appendChild(el("div", "cmt-resolved", "Resolved"));
      thread.appendChild(entry(t, true));
      for (const r of t.replies) thread.appendChild(entry(r, false));
    } else {
      thread.appendChild(el("div", "panel-hint", "No comment on this cell yet."));
    }
    ta.placeholder = t ? "Reply…" : "Type a comment…";
    hint.textContent = t
      ? "Save replies to the thread. Edit rewrites the first comment; Delete removes the whole thread."
      : "Comments attach to the active cell. Select another cell to see its thread.";
    return t;
  };
  let current = render();

  panelNote = { ta, addrEl, render: () => { current = render(); }, cell: A1(state.sel.row, state.sel.col) };

  const actions = el("div", "panel-actions");
  const button = (label, cls, fn) => {
    const b = el("button", cls, label);
    b.addEventListener("click", fn);
    actions.appendChild(b);
    return b;
  };
  // The primary verb changes with the thread's state, because "Save" on an
  // existing thread is ambiguous — it could mean reply or rewrite, and those
  // are very different things to do to someone else's comment.
  const primary = button("Save", "primary", () => {
    const text = ta.value.trim();
    if (!text) return;
    const author = who.value.trim();
    setCommentAuthor(author);
    try {
      if (current) wasm.session_reply_comment(state.sheet, state.sel.row, state.sel.col, text, author, commentStamp());
      else wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, text, author, commentStamp());
    } catch (e) { statusError(errText(e)); return; }
    ta.value = "";
    current = render();
    refreshPanelButtons();
    draw();
  });
  const resolveBtn = button("Resolve", null, () => {
    if (!current) return;
    try { wasm.session_resolve_comment(state.sheet, state.sel.row, state.sel.col, !current.resolved); }
    catch (e) { statusError(errText(e)); return; }
    current = render();
    refreshPanelButtons();
    draw();
  });
  button("Delete", "danger", () => {
    // On a refusal the panel is left exactly as it was. Emptying the box
    // regardless said "deleted" for a comment that is still on the cell, and
    // the next redraw put it back.
    try { wasm.session_set_comment(state.sheet, state.sel.row, state.sel.col, "", "", ""); }
    catch (e) { statusError(errText(e)); return; }
    ta.value = "";
    current = render();
    refreshPanelButtons();
    draw();
  });
  body.appendChild(actions);

  function refreshPanelButtons() {
    primary.textContent = current ? "Reply" : "Save";
    resolveBtn.hidden = !current;
    resolveBtn.textContent = current && current.resolved ? "Reopen" : "Resolve";
  }
  refreshPanelButtons();
  panelNote.refresh = () => { current = render(); refreshPanelButtons(); };

  setTimeout(() => ta.focus(), 0);
}

// Insert or edit the hyperlink on the active cell.
//
// Two destinations rather than one: an external address, and a location inside
// this workbook. The schema treats them as independent — a link can carry both,
// meaning "open that document at this anchor" — so the dialog does too instead
// of making the user pick a mode.

// Open a link. An external target goes to the browser; an internal one is a
// navigation within the workbook, which is why they are not the same code path.

// Turn the selection into a table, or convert one back to a range.
//
// The header question is asked rather than guessed: whether the first row is a
// header decides the column names, and a wrong guess leaves every structured
// reference pointing at the wrong column — silently, since the formulas still
// resolve.
// The style names offered in the picker. A small, representative set rather
// than all sixty Excel ships: every family, and the six accents inside the
// family people actually use.
// Excel's totals-row functions. Each writes SUBTOTAL with the matching 10x
// code, so the total follows the filter rather than counting hidden rows.
export const TOTALS_FUNCTIONS = [
  ["None", ""],
  ["Sum", "sum"],
  ["Average", "average"],
  ["Count", "count"],
  ["Count numbers", "countNums"],
  ["Max", "max"],
  ["Min", "min"],
  ["Std dev", "stdDev"],
  ["Var", "var"],
];

export const TABLE_STYLES = [
  ["None", ""],
  ["Light blue", "TableStyleLight2"],
  ["Light green", "TableStyleLight7"],
  ["Blue", "TableStyleMedium2"],
  ["Orange", "TableStyleMedium3"],
  ["Grey", "TableStyleMedium4"],
  ["Gold", "TableStyleMedium5"],
  ["Steel", "TableStyleMedium6"],
  ["Green", "TableStyleMedium7"],
  ["Dark blue", "TableStyleDark2"],
];

// Create a table under the cursor, or open the panel on the one already there.
//
// Creation asks nothing: everything it could ask — name, style, banding,
// headers — is in the panel, live, and creating is one undo step. A modal that
// asked four questions before showing you anything was both slower and blind.

// A yes/no question in the shared modal. Resolves true only on the confirm
// button; Escape, the ✕ and the backdrop all mean "no", because this is only
// used to guard destructive steps.

// Excel's four merge verbs. "Across" merges each row of the selection
// separately — the one people reach for on a header band — and "& center"
// is a merge plus a centre, which is how it is nearly always used.

// Grow/shrink font: step to the next/previous size on a standard ladder, based
// on the active cell's current size (default 11pt). Beyond the ladder, step ±2.
export const SIZE_LADDER = [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72];
// AutoSum: write =SUM(…) over the run of numbers directly above the cursor, or
// to its left when there is nothing above.
//
// Excel guesses the range and is right nearly every time, which is what makes
// the shortcut worth having; an AutoSum that made you select the range first
// would just be typing SUM with extra steps.

// Current border palette state (chosen line style + color, "" = automatic).
export let borderStyle = "thin";
export let borderColor = "";

// Custom tooltips: convert native `title`s on the chrome to styled, faster
// tooltips (keeping an aria-label for a11y), shown on hover after a short delay.
export let tipEl = null;
export let tipTimer = 0;
function initTooltips() {
  tipEl = document.createElement("div");
  tipEl.className = "tooltip";
  tipEl.hidden = true;
  ocOverlayHost.appendChild(tipEl);
  // Promote existing titles to data-tip so the native bubble doesn't also show.
  for (const node of qsa(".toolbar [title], .app-header [title], .formula-bar [title], .side-panel [title]")) {
    tipify(node);
  }
  document.addEventListener("mouseover", (e) => {
    const node = e.target.closest("[data-tip]");
    if (!node) return;
    clearTimeout(tipTimer);
    tipTimer = setTimeout(() => showTip(node), 380);
  });
  document.addEventListener("mouseout", (e) => {
    if (e.target.closest("[data-tip]")) hideTip();
  });
  document.addEventListener("mousedown", hideTip);
}
// Set a control's tooltip after boot, through whichever surface it is using.
//
// `tipify` moves `title` onto `data-tip` and *deletes the attribute*, so code
// that later assigns `node.title` does two wrong things at once: the custom
// tooltip goes on showing the text it captured at boot, and the native bubble
// — the thing tipify exists to suppress — reappears underneath it. Undo and
// Redo are the only tooltips in the editor that change after boot, so they were
// the only ones that could show this, and they did: both stayed "Undo (Ctrl+Z)"
// for the life of the page instead of naming the edit they would reverse.
// Move an element's `title` to `data-tip` (+ aria-label), suppressing the native tip.

// A 20×20 icon sketching a cell with the placement's edges emphasized.
export const BD_TITLES = {
  all: "All borders", inner: "Inner borders", outer: "Outer border",
  horizontal: "Inside horizontal", vertical: "Inside vertical", none: "Clear borders",
  top: "Top border", bottom: "Bottom border", left: "Left border", right: "Right border",
  topandbottom: "Top and bottom border", bottomdouble: "Bottom double border",
  bottomthick: "Thick bottom border",
  diagdown: "Diagonal down", diagup: "Diagonal up", diagboth: "Both diagonals",
  nodiag: "Clear diagonals",
};
// Build the border palette into #border-menu (once).
function buildBorderMenu() {
  const menu = byId("border-menu");
  menu.textContent = "";
  const grid = el("div", "bd-grid");
  for (const kind of ["all", "inner", "outer", "horizontal", "vertical", "none",
                      "top", "bottom", "left", "right",
                      "topandbottom", "bottomdouble", "bottomthick",
                      "diagdown", "diagup", "diagboth", "nodiag"]) {
    const b = el("button", "bd-cell");
    b.title = BD_TITLES[kind];
    b.setAttribute("aria-label", BD_TITLES[kind]);
    // oc-safe-html: `bdIcon` returns one of a fixed set of literal SVGs.
    b.innerHTML = bdIcon(kind);
    b.addEventListener("click", (e) => { e.stopPropagation(); setBorder(kind); menu.hidden = true; canvas.focus(); });
    grid.appendChild(b);
  }
  menu.appendChild(grid);
  const sty = el("select", "tb-select bd-style");
  for (const [v, t] of [["thin", "Thin"], ["medium", "Medium"], ["thick", "Thick"], ["dashed", "Dashed"], ["dotted", "Dotted"], ["double", "Double"]]) {
    const o = el("option", null, t); o.value = v; sty.appendChild(o);
  }
  sty.value = borderStyle;
  sty.addEventListener("click", (e) => e.stopPropagation());
  sty.addEventListener("change", () => { borderStyle = sty.value; });
  menu.appendChild(sty);
  const sw = el("div", "bd-swatches");
  for (const c of ["", "000000", "2f6df6", "e5484d", "16a34a", "f59e0b", "8b5cf6", "64748b"]) {
    const b = el("button", "bd-color" + (c === borderColor ? " on" : ""));
    b.dataset.color = c;
    if (c) { b.style.background = "#" + c; b.title = "#" + c; }
    else { b.textContent = "A"; b.title = "Automatic color"; }
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      borderColor = c;
      sw.querySelectorAll(".bd-color").forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      const btn = byId("tb-border");
      btn.style.setProperty("--oc-x-border-swatch", c ? "#" + c : "currentColor");
    });
    sw.appendChild(b);
  }
  menu.appendChild(sw);
}
// Delete key / "Clear contents": clear values + formulas, keep formatting.
//
// All three of these are refusable — `guard_protected` rejects the range on a
// protected sheet — and all three used to swallow the refusal, so the Delete
// key on a protected sheet was a key that did nothing and said nothing. That is
// the same failure the undo path was fixed for (see `doUndo`).
// "Clear formats": drop styling, keep values + formulas.
// "Clear all": also drop styles.
// --- Find & replace -------------------------------------------------------
export let findBar;
export let findInput;
export let replaceInput;
export let findCount;
export let findCase;
export let findWhole;
export let findValues;
export let findAllSheets;
export let findWildcards;
export const findState = { matches: [], idx: -1 };

// Replace only the current match, then re-search and jump to the next one.

// Undo/redo can add, remove, or reorder sheets, so rebuild the tab bar (which
// also re-clamps the active sheet if it vanished) before redrawing the grid.
//
// The failure is **said out loud**, where it used to be swallowed by a bare
// `catch {}`. A collaborative undo can now be refused — undoing an insert that
// somebody else has since filled would delete their work, and no undo stack
// anywhere holds it (docs/69). A refusal nobody sees is a button that appears
// to do nothing, which is the worse of the two failures that policy chose
// between, and it would have arrived silently through this line.
// Export the active sheet as delimited text (CSV/TSV/PSV).
// Download in the format the session was **opened from**, under that format's
// own extension and content type.
//
// The engine has known all three since `WOPI-05` — `session_save_native`
// follows the format, `session_format` and `session_format_content_type` name
// it, and `session_save_loss` says what it cannot carry — and nothing on this
// page asked any of them (`WASM-01`). So a `.csv` could only leave the editor
// through an explicit "CSV" export that writes whichever tab is in front and
// labels every delimited file `text/csv`, which is the wrong type for two of
// the three.
// The TSV we last wrote to the OS clipboard. On paste we compare the OS
// clipboard to this: if it still matches, our richer internal snapshot is
// authoritative (formulas + styles); otherwise the user copied from elsewhere
// and we fall back to plain TSV.
export let lastClipTsv = null;

// Marching-ants outline around the copy/cut source, animated by a dash offset.
let clipMarch = null;      // { sheet, r0, c0, r1, c1, cut } or null
let marchOffset = 0;
let marchRaf = 0;
let marchLast = 0;
function marchTick(t) {
  if (!clipMarch) { marchRaf = 0; return; }
  // Reduced motion: keep the dashed outline, drop the crawl. It marks the copy
  // source just as well standing still, and stopping the loop also stops a
  // repaint every 80 ms.
  if (REDUCED_MOTION.matches) { marchRaf = 0; return; }
  if (t - marchLast > 80) { marchOffset = (marchOffset + 1) % 8; marchLast = t; draw(); }
  marchRaf = requestAnimationFrame(marchTick);
}
// Honour `prefers-reduced-motion`: the marching ants are decoration, and the
// dashed outline says the same thing standing still.
const REDUCED_MOTION = window.matchMedia
  ? window.matchMedia("(prefers-reduced-motion: reduce)")
  : { matches: false };

function startMarch(s, cut) {
  clipMarch = { sheet: state.sheet, r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c1, cut };
  if (!marchRaf) marchRaf = requestAnimationFrame(marchTick);
  draw();
}
export function stopMarch() {
  if (!clipMarch) return;
  // Tell the *engine*, not just the animation. Clearing the marquee alone left
  // a pending cut armed, so Esc — then a click elsewhere, then Ctrl+V — still
  // moved the data and emptied the source the user believed they had spared.
  // The visible signal said cancelled and the state said otherwise, which is
  // the worst possible pairing for an action that deletes.
  // Swallowed because this is teardown, not a command: it also runs from File
  // ▸ New and from a load, where the session it would clear is being replaced
  // anyway, and a message there would name a failure the user did not cause.
  try { wasm.session_clip_clear(); } catch {}
  clipMarch = null;
  if (marchRaf) { cancelAnimationFrame(marchRaf); marchRaf = 0; }
  draw();
}

export async function clipToOS(s, cut) {
  wasm.session_clip_copy(state.sheet, s.r0, s.c0, s.r1, s.c1, cut);
  startMarch(s, cut);
  const tsv = wasm.session_copy_tsv(state.sheet, s.r0, s.c0, s.r1, s.c1);
  lastClipTsv = tsv;
  // Write a formatted HTML table alongside the plain-text TSV so external apps
  // (Excel, Sheets, mail, docs) receive styling; fall back to text-only when
  // ClipboardItem isn't available or is blocked.
  try {
    const html = wasm.session_copy_html(state.sheet, s.r0, s.c0, s.r1, s.c1);
    if (navigator.clipboard.write && typeof ClipboardItem !== "undefined") {
      const item = new ClipboardItem({
        "text/html": new Blob([html], { type: "text/html" }),
        "text/plain": new Blob([tsv], { type: "text/plain" }),
      });
      await navigator.clipboard.write([item]);
      return true;
    }
  } catch { /* fall through to text-only */ }
  try { await navigator.clipboard.writeText(tsv); return true; }
  catch { return false; }
}
// Paste-special: reproduce only part of the internal clipboard.
/// Parse clipboard `text/html` into the cells the engine applies.
///
/// See `docs/68-CLIPBOARD-HTML-PASTE.md`. The short version: `DOMParser` gives a
/// document with **no browsing context**, so nothing in it executes and nothing
/// in it fetches; the nodes are never inserted anywhere; and only `textContent`
/// and a fixed list of attributes are read, so `href`, `src` and every `on*`
/// handler are not consulted at all. A value that is never read cannot leak.
///
/// Returns `null` when the HTML holds no table, which is how a paste of ordinary
/// rich text falls through to the plain-text path.
export function cellsFromClipboardHtml(html) {
  let doc;
  try {
    doc = new DOMParser().parseFromString(html, "text/html");
  } catch {
    return null;
  }
  const table = doc.querySelector("table");
  if (!table) return null;

  const hex = (value) => {
    if (!value) return null;
    const text = String(value).trim().toLowerCase();
    let m = /^#?([0-9a-f]{6})$/.exec(text);
    if (m) return m[1].toUpperCase();
    m = /^#?([0-9a-f]{3})$/.exec(text);
    if (m) return m[1].split("").map((c) => c + c).join("").toUpperCase();
    m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/.exec(text);
    if (m) {
      return [1, 2, 3]
        .map((i) => Math.min(255, Number(m[i])).toString(16).padStart(2, "0"))
        .join("")
        .toUpperCase();
    }
    // Named and functional colours are dropped rather than guessed: a wrong
    // fill is louder than a missing one.
    return null;
  };

  // `style` is read as text and split on `;`. Deliberately not via
  // `getComputedStyle`, which would need the node in a live document — the one
  // thing this must never do.
  const declarations = (el) => {
    const out = {};
    for (const part of (el.getAttribute("style") ?? "").split(";")) {
      const at = part.indexOf(":");
      if (at < 0) continue;
      out[part.slice(0, at).trim().toLowerCase()] = part.slice(at + 1).trim();
    }
    return out;
  };

  // One CSS edge declaration — `1px solid #000000` — as the OOXML line-style
  // token the model stores, or `null` for "no line here".
  //
  // `groove`, `ridge`, `inset` and `outset` become a solid line of the same
  // weight. Excel has no such styles, and a line where a line was asked for is
  // closer than nothing; only `none`, `hidden` and a zero width mean no edge.
  const edgeFrom = (value) => {
    const text = String(value ?? "").trim().toLowerCase();
    if (!text) return null;
    const kind = /\b(none|hidden|solid|double|dashed|dotted|groove|ridge|inset|outset)\b/.exec(text);
    // A declaration with a width but no keyword is a solid line, as in CSS.
    const line = kind ? kind[1] : "solid";
    if (line === "none" || line === "hidden") return null;
    let px = 1;
    const w = /(\d+(?:\.\d+)?)\s*(px|pt)?/.exec(text);
    if (w) px = w[2] === "pt" ? Number(w[1]) * (4 / 3) : Number(w[1]);
    if (/\bthin\b/.test(text)) px = 1;
    if (/\bmedium\b/.test(text)) px = 2;
    if (/\bthick\b/.test(text)) px = 3;
    if (!(px > 0)) return null;
    const style =
      line === "double" ? "double"
      : line === "dashed" ? "dashed"
      : line === "dotted" ? "dotted"
      : px < 1.5 ? "thin"
      : px < 2.5 ? "medium"
      : "thick";
    const colour = /#[0-9a-f]{3}(?:[0-9a-f]{3})?\b/.exec(text) ?? /rgba?\([^)]*\)/.exec(text);
    return { style, color: colour ? hex(colour[0]) : null };
  };

  const cells = [];
  const rows = [...table.querySelectorAll("tr")];
  // Rows and columns are counted, because a cell that spans rows pushes the
  // cells below it sideways. Without this a merged header silently shifts every
  // row under it one column left.
  const taken = new Map();
  rows.forEach((tr, r) => {
    let c = 0;
    for (const td of tr.children) {
      if (!/^(td|th)$/i.test(td.tagName)) continue;
      while (taken.get(`${r},${c}`)) c += 1;
      const style = { ...declarations(td.closest("tr") ?? td), ...declarations(td) };
      // Producers still wrap runs in <font>/<b>/<i> inside the cell; those carry
      // formatting for the whole cell in practice.
      const inner = td.querySelector("font, span, b, i, u, s, strike");
      const innerStyle = inner ? declarations(inner) : {};
      const merged = { ...style, ...innerStyle };
      const weight = merged["font-weight"] ?? "";
      const decoration = `${merged["text-decoration"] ?? ""} ${merged["text-decoration-line"] ?? ""}`;
      const align = (merged["text-align"] ?? td.getAttribute("align") ?? "").toLowerCase();
      const valign = (merged["vertical-align"] ?? td.getAttribute("valign") ?? "").toLowerCase();
      const wrap = (merged["white-space"] ?? "").toLowerCase();
      const size = /^(\d+(?:\.\d+)?)pt/.exec(merged["font-size"] ?? "");
      const rs = Math.max(1, Number(td.getAttribute("rowspan") ?? 1) || 1);
      const cs = Math.max(1, Number(td.getAttribute("colspan") ?? 1) || 1);

      // Per-edge longhand beats the shorthand, and an explicit `border-top:
      // none` beats it too — which is why presence of the key is tested rather
      // than the truthiness of what it parsed to. `border-collapse` is a
      // different property and is never consulted: this maps the edges a cell
      // declares for itself, which is what the model stores per cell.
      const borders = {};
      for (const side of ["top", "right", "bottom", "left"]) {
        const own = `border-${side}`;
        const edge =
          merged[own] !== undefined ? edgeFrom(merged[own])
          : merged.border !== undefined ? edgeFrom(merged.border)
          : null;
        if (edge) borders[side] = edge;
      }

      cells.push({
        dr: r,
        dc: c,
        rs,
        cs,
        text: (td.textContent ?? "").replace(/ /g, " ").trim(),
        bold:
          !!td.querySelector("b, strong") ||
          weight === "bold" ||
          (Number(weight) >= 600),
        italic: !!td.querySelector("i, em") || (merged["font-style"] ?? "").includes("italic"),
        underline: !!td.querySelector("u") || decoration.includes("underline"),
        strike: !!td.querySelector("s, strike, del") || decoration.includes("line-through"),
        wrap: wrap === "normal" || wrap === "pre-wrap",
        color: hex(merged.color),
        fill: hex(merged["background-color"] ?? merged.background ?? td.getAttribute("bgcolor")),
        font: (merged["font-family"] ?? "").split(",")[0].replace(/["']/g, "").trim() || null,
        sizeHp: size ? Math.round(Number(size[1]) * 2) : null,
        align: ["left", "center", "right", "justify"].includes(align) ? align : null,
        valign: ["top", "middle", "bottom"].includes(valign) ? valign : null,
        // Excel and LibreOffice each carry the number format in their own
        // non-standard property. Neither is guessed at from the text.
        numberFormat:
          merged["mso-number-format"]?.replace(/\\/g, "").replace(/^"|"$/g, "") ??
          td.getAttribute("sdnum")?.split(";").pop() ??
          null,
        borders: Object.keys(borders).length ? borders : null,
      });

      for (let dr = 0; dr < rs; dr += 1) {
        for (let dc = 0; dc < cs; dc += 1) taken.set(`${r + dr},${c + dc}`, true);
      }
      c += cs;
    }
  });
  return cells.length ? cells : null;
}

/// Read the clipboard's HTML flavour, if the browser will give it to us.


// Place the in-cell editor over the cell being edited. Called on every redraw
// as well as on open: it is a DOM element over the canvas, so without this it
// stays parked where the cell *was* while the grid scrolls out from under it.
// A merged cell gets the whole block, not just its anchor's one-cell box.

// --- The edit session -------------------------------------------------------
// A cell can be edited from two surfaces: the in-cell overlay and the formula
// bar. `editSurface` is whichever <input> currently holds the edit, and every
// piece of formula intelligence below (autocomplete, click-to-insert a
// reference, the invalid-formula outline, commit/revert) is keyed off it rather
// than off the in-cell editor. Without this the formula bar is a dumb text box:
// the same typing that autocompletes in a cell does nothing up there.
export let editSurface = null;

/// Tell the desktop shell whether a cell is open for editing (`TAURI-012`).
///
/// **A native menu accelerator is consumed before the webview sees the key.**
/// In a browser, `Cmd+T` mid-formula reaches this module's key handler and
/// cycles the reference's anchors, which is what Excel's own Mac table says it
/// should. In a desktop window the menu ate it first and opened a modal over a
/// half-typed formula — so the shell build was *worse* than the browser one.
///
/// The shell cannot know what a keystroke means; only this module knows whether
/// a cell is open. So it says, and the shell releases the colliding chords for
/// as long as the edit lasts.
///
/// A no-op in a browser, where `window.__opencalcNative` is absent and the key
/// already arrives here — which is the behaviour being restored.
let lastEditingReported = null;
function reportEditing(editing) {
  if (editing === lastEditingReported) return;
  lastEditingReported = editing;
  const native = window.__opencalcNative;
  if (native && native.setEditing) native.setEditing(editing).catch(() => {});
}
// The cell's text when the edit began, for Escape to restore.
export let editOriginal = "";
// How the edit started: typing a fresh value ("Enter") or opening the existing
// one with F2 / double-click / the formula bar ("Edit"). Excel's status bar
// distinguishes these, and it is about the *gesture*, not about whether the
// cell happened to be empty.
export let editMode = "Enter";
// The cell an in-progress edit belongs to, which reference picking can navigate
// away from.
export let editHome = null;

/// Whether the active cell would refuse an edit right now.
///
/// Two questions, in the order that makes the second one cheap: a cell's
/// `locked` flag means nothing at all until the sheet is protected, which is
/// why an ordinary sheet — where every cell is locked by default — is still
/// perfectly editable.
///
/// **The fallback is `locked`, and that is the engine's own default**, not a
/// guess: `session_cell_protection` answers `locked: true` for a cell with no
/// style of its own, which is OOXML's default and Excel's. Failing open here
/// would let the editor accept an edit the engine is about to refuse, which is
/// the exact trap this guard exists to close.
export function activeCellLocked() {
  if (!sheetProtectedNow()) return false;
  try {
    return !!JSON.parse(
      wasm.session_cell_protection(state.sheet, state.sel.row, state.sel.col)).locked;
  } catch {
    return true;
  }
}

/// The bottom of what the user can actually see, in layout pixels.
///
/// **Not `window.innerHeight`.** A software keyboard does not resize the page:
/// it shrinks the *visual* viewport and leaves the layout viewport alone.
/// `.editor-body` is `height: 100vh`, so the grid keeps its full height and the
/// keyboard is simply drawn over the bottom of it. A pinch or page zoom does
/// the same thing by a different route, and wants the same answer, which is why
/// this is phrased as "what is visible" rather than as keyboard handling.
const visibleBottomPx = () => {
  const vv = window.visualViewport;
  if (!vv) return window.innerHeight;
  return Math.min(window.innerHeight, vv.offsetTop + vv.height);
};

/// Daylight left under the in-cell editor when it has to be scrolled clear.
const EDIT_CLEARANCE = 8;

/// Scroll the sheet until the cell being edited is somewhere the user can see.
///
/// Measured on a 390×844 phone before this existed: editing a cell near the
/// foot of the grid put the in-cell editor at y=738, and an iOS keyboard starts
/// at 508. You typed into a box 230px underneath the keyboard, with the formula
/// bar as the only readable copy of what you were writing. Nothing in `webapp/`
/// referenced `visualViewport` at all.
///
/// Called when an edit opens and when the visual viewport resizes under an open
/// one — never per frame, so it is off the scroll and paint paths entirely.
/// Not exported: `editor.js` re-exports this module wholesale, and this is an
/// internal reaction to a browser event rather than anything a host calls.
function keepEditVisible() {
  if (!wasm || !state.editing || editSurface !== inline) return;
  const box = inline.getBoundingClientRect();
  if (box.height <= 0) return;
  const over = box.bottom + EDIT_CLEARANCE - visibleBottomPx();
  if (over <= 0) return;
  // Never lift the cell above the body origin. `positionInline` clamps the box
  // to its own pane, so a cell scrolled up under the column headers or a frozen
  // band would leave the editor parked on the header while the value it belongs
  // to is somewhere else entirely.
  const z = state.zoom || 1;
  const f = state.freeze || { bodyY0: 0 };
  const room = box.top - (wrap.getBoundingClientRect().top + f.bodyY0 * z);
  const move = Math.min(over, Math.max(0, room));
  if (move <= 0) return;
  state.scrollY += move / z;
  clampScroll();
  // `draw()` rather than a scheduled frame: this runs once per edit or per
  // viewport resize, and `positionInline` — which puts the box back on its cell
  // — happens inside it.
  draw();
}

export function beginEdit(surface, initial, caretAtEnd = false) {
  // A read-only session refuses the write anyway; refusing here means the user
  // is told before typing rather than after, which is the difference between a
  // mode and a trap.
  if (readOnly()) {
    statusError("this workbook is open for reading only");
    return;
  }
  // The same argument, for the same reason, one rule down. A protected sheet
  // refused the write at *commit*, so the editor opened, the user typed, and
  // Enter threw the value away — `editor.selection.js` still carries the note
  // that "a protected sheet appeared to accept the value". Excel does not open
  // the editor on a locked cell at all, and now neither does this.
  //
  // The message is the engine's own sentence, word for word: one rule that
  // refuses in two places must not explain itself in two ways.
  if (activeCellLocked()) {
    statusError("this sheet is protected — unprotect it to change locked cells");
    return;
  }
  // A pivot's report is written by the engine and rewritten on every refresh.
  // Excel refuses the edit rather than letting a typed value stand until an
  // unrelated action wipes it, and so does this — a value that survives only
  // until something else erases it is worse than one never accepted.
  const owner = pivotBlocks(state.sel.row, state.sel.col);
  if (owner) {
    statusError(`this cell is part of the pivot table “${owner}” — change it in the fields panel`);
    return;
  }
  editSurface = surface;
  reportEditing(true);
  state.editing = true;
  // Where this edit will be written. Reference picking may walk to another
  // sheet, so the target cannot simply be "wherever the selection is now".
  editHome = { sheet: state.sheet, row: state.sel.row, col: state.sel.col };
  editMode = initial !== undefined ? "Enter" : "Edit";
  refSpans = [];
  editOriginal = wasm.session_cell_input(state.sheet, state.sel.row, state.sel.col);
  if (surface === inline) {
    inline.style.display = "block";
    positionInline();
  }
  if (initial !== undefined) surface.value = initial;
  else surface.value = editOriginal;
  // **`preventScroll`, and it is not a nicety.**
  //
  // The grid scrolls *virtually*: the canvas is one screen tall and draws
  // whatever `state.scrollX/scrollY` say. The inline editor, though, is a real
  // `<textarea>` inside `.grid-wrap` — and `overflow: hidden` stops a *user*
  // scrolling that container, not the browser. Focusing a descendant it
  // considers out of view makes it scroll the container to reveal it, and that
  // native `scrollTop` is pure corruption: the canvas keeps drawing from
  // `state.scrollY` while the element it lives in has moved underneath it.
  //
  // The symptom was reported as "typing =2*A2 scrolled the canvas somewhere
  // weird". It is worse than a scroll — the column headers leave the screen,
  // only a band of rows draws, and the editor box detaches and floats near the
  // bottom of the window, because everything is offset twice.
  surface.focus({ preventScroll: true });
  // Typing a character starts a fresh value; opening the editor selects what is
  // already there so the next keystroke replaces it — except under F2, which
  // exists precisely to *amend* the value, so it puts the caret at the end.
  if (initial === undefined) {
    if (caretAtEnd) surface.setSelectionRange(surface.value.length, surface.value.length);
    else surface.select();
  }
  // The ordinary case once a keyboard is already up: Enter walks to the next
  // row and the editor reopens further down. Nothing resizes, so the
  // `visualViewport` listener never fires — this is the path that catches it.
  keepEditVisible();
  // Opening an existing formula highlights the cells it reads straight away,
  // rather than waiting for the first keystroke.
  updateRefSpans();
  updateCellMode();
  // The others learn about this edit at its first character rather than at its
  // first *change*: typing a letter on the grid sets the value programmatically,
  // which fires no `input` event, so the opening keystroke would otherwise be
  // the one nobody else ever saw.
  announceCollabSelection();
}


// End the edit without committing. `refocus` false leaves focus alone — used
// when the caller is about to move it somewhere specific.
export function endEdit(refocus = true) {
  const was = editSurface;
  editSurface = null;
  reportEditing(false);
  state.editing = false;
  // The range-finder outlines are painted into the canvas, so dropping them
  // needs a repaint — otherwise they linger over the grid after the edit ends.
  const hadSpans = refSpans.length > 0;
  refSpans = [];
  paintRefTokens();
  for (const surface of [inline, fInput]) {
    const m = refMirrors.get(surface);
    if (m) { m.textContent = ""; m.style.display = "none"; }
    surface.classList.remove("tinted");
  }
  inline.style.display = "none";
  hideAutocomplete();
  hideSignatureTip();
  formulaRefDrag = null;
  pointMode = null;
  inline.classList.remove("invalid");
  fInput.classList.remove("invalid");
  if (refocus && was) canvas.focus();
  updateCellMode();
  // However the edit ended — committed, escaped, or handed to another cell —
  // this participant is no longer typing, and the others are told so by a
  // presence entry with no draft in it. There is no separate "stopped" message
  // to lose, which is what makes an abandoned edit cost nothing to clean up.
  announceCollabSelection();
  if (hadSpans) draw();
}
// Kept as the name the rest of the editor already calls.
// --- Range finder -----------------------------------------------------------
// While a formula is being edited, each reference in it gets a colored outline
// on the grid, so "which cells does this formula actually read?" is answerable
// by looking rather than by parsing the text in your head. The spans come from
// the engine's scanner, so a function name is never mistaken for a reference.
// --- Trace precedents / dependents ----------------------------------------
//
// "Why is this cell wrong?" is usually answered by seeing what it reads, or what
// reads it. The blocks come from the same walk the recalculator uses, so an
// arrow can never point somewhere recalculation would not follow.
export let traceBlocks = [];   // [{s,r0,c0,r1,c1}] on the active sheet
export let traceMode = null;   // "prec" | "dep"

function toggleTrace(mode) {
  if (traceMode === mode) { clearTrace(); return; }
  traceMode = mode;
  try {
    traceBlocks = JSON.parse(
      wasm.session_trace(state.sheet, state.sel.row, state.sel.col, mode === "dep"),
    );
  } catch { traceBlocks = []; }
  // Only this sheet's blocks can be drawn; a cross-sheet arrow has nowhere to
  // point, so it is reported rather than silently dropped.
  const here = traceBlocks.filter((b) => b.s === state.sheet);
  const elsewhere = traceBlocks.length - here.length;
  traceBlocks = here;
  const n = traceBlocks.length;
  const what = mode === "dep"
    ? (n === 1 ? "dependent" : "dependents")
    : (n === 1 ? "precedent" : "precedents");
  status.textContent = n
    ? `${n} ${what}${elsewhere ? ` (+${elsewhere} on other sheets)` : ""}`
    : `no ${mode === "dep" ? "dependents" : "precedents"}`;
  draw();
}

function clearTrace() {
  if (!traceMode && !traceBlocks.length) return;
  traceMode = null;
  traceBlocks = [];
  draw();
}

// Draw an arrow from each traced block to the active cell (or the reverse for
// dependents), plus a box around the block itself.

// --- Reference tinting in the text ----------------------------------------
//
// The grid outlines say *where* each reference points; this says *which* piece
// of the formula each outline belongs to, by tinting the reference tokens in
// the same colours. A plain `<input>`/`<textarea>` cannot colour a substring, so
// a mirror element sits exactly behind the editing surface rendering the same
// text with coloured spans, and the surface itself draws its text transparent
// while keeping its caret. The mirror is inert — no pointer events, hidden from
// assistive tech — so selection, IME and every key behave as they did.
export const refMirrors = new WeakMap();


// Copy the metrics that decide where each glyph lands. Getting any of these
// wrong shows up immediately as text that drifts out of register with the caret.


export let refSpans = []; // [{s,e,r0,c0,r1,c1,sh}] for the formula being edited
// Excel/Sheets use a small rotating palette, one color per distinct reference.
export const REF_COLORS = ["#1a73e8", "#e37400", "#0f9d58", "#a142f4", "#d93025", "#12b5cb"];

export function updateRefSpans() {
  const text = editSurface ? editSurface.value : "";
  const next = text.startsWith("=") && wasm
    ? (() => { try { return JSON.parse(wasm.formula_ref_spans(text)); } catch { return []; } })()
    : [];
  // Only repaint when the set actually changed — this runs on every keystroke.
  const same = next.length === refSpans.length &&
    next.every((r, i) => {
      const p = refSpans[i];
      return p && r.r0 === p.r0 && r.c0 === p.c0 && r.r1 === p.r1 && r.c1 === p.c1 && r.sh === p.sh;
    });
  refSpans = next;
  // The text can change without the *set* of references changing (typing inside
  // a token, or anywhere else in the formula), so the tint is repainted every
  // time while only the grid outlines are gated on a real change.
  paintRefTokens();
  if (!same) draw();
}

// Arrow-key point mode: with the caret somewhere a reference may go, the arrow
// keys build one by moving a pointer over the grid instead of moving the text
// caret — Excel's "point mode". Shift extends it into a range. The state mirrors
// `formulaRefDrag`, so insertRef keeps replacing the same span of text.
export let pointMode = null; // {anchor:{row,col}, cur:{row,col}, start, end}

function pointStep(dr, dc, extend) {
  if (!editSurface) return false;
  if (!pointMode) {
    if (!refAcceptable()) return false;
    // Excel starts from the cell being edited and steps off it.
    const cur = {
      row: Math.max(0, state.sel.row + dr),
      col: Math.max(0, state.sel.col + dc),
    };
    const at = editSurface.selectionStart;
    pointMode = { anchor: cur, cur, start: at, end: at };
  } else {
    const cur = {
      row: Math.max(0, pointMode.cur.row + dr),
      col: Math.max(0, pointMode.cur.col + dc),
    };
    pointMode.cur = cur;
    if (!extend) pointMode.anchor = cur; // a plain arrow moves the whole point
  }
  const a = pointMode.anchor, c = pointMode.cur;
  const r0 = Math.min(a.row, c.row), r1 = Math.max(a.row, c.row);
  const c0 = Math.min(a.col, c.col), c1 = Math.max(a.col, c.col);
  insertRef(r0 === r1 && c0 === c1 ? A1(r0, c0) : `${A1(r0, c0)}:${A1(r1, c1)}`);
  // Keep the cell being pointed at on screen, without disturbing the selection.
  ensureVisible(c.row, c.col);
  draw();
  return true;
}

// F4 cycles the anchoring of the reference under the caret, in Excel's order:
// A1 → $A$1 → A$1 → $A1 → A1. A range cycles both endpoints together, and a
// sheet qualifier is left alone. Returns whether anything was rewritten.
export const A1_PART = /^(\$?)([A-Za-z]+)(\$?)([0-9]+)$/;

// Excel shows the in-progress text on both surfaces at once. Mirror the one
// being typed in onto the other — never the reverse, or the two carets fight.

// Abandon the edit and put the cell's own text back on both surfaces.

// --- Formula editing UX: autocomplete, reference insertion, validation -----
export const A1 = (row, col) => colName(col) + (row + 1);

// --- Name box (cell-ref input): show address / drag size, jump on Enter -----
// Reflect the selection into the name box unless the user is typing in it. While
// drag-selecting a block, show Excel's "3R x 2C" size readout.
// --- Assistive announcements + cell mode -----------------------------------
// The structural tree is `rebuildA11yGrid` below; this is the running
// commentary beside it. A live region is what announces a *change* — moving the
// selection, growing it — which a static tree cannot do on its own.
let updateNumberFormatReadout = () => {};
export let liveEl;
export let modeEl;
let lastAnnounced = "";
function announceCell() {
  if (!liveEl || !wasm) return;
  const ref = A1(state.sel.row, state.sel.col);
  const r = selRect();
  const size = r.r0 === r.r1 && r.c0 === r.c1
    ? ""
    : `, ${r.r1 - r.r0 + 1} by ${r.c1 - r.c0 + 1} selected`;
  let text = "";
  try {
    const it = JSON.parse(wasm.session_cells(state.sheet, state.sel.row, state.sel.col, state.sel.row, state.sel.col))[0];
    text = it && it.t ? it.t : "empty";
  } catch { text = ""; }
  const msg = `${ref}${size}. ${text}`;
  // Re-announcing the same string is silence in most screen readers, and noise
  // in the rest; only speak on an actual change.
  if (msg === lastAnnounced) return;
  lastAnnounced = msg;
  liveEl.textContent = msg;
}

// --- The accessibility tree ------------------------------------------------
//
// A DOM mirror of the cells currently on screen. The canvas cannot expose
// anything structurally, so without this a screen reader has a live region and
// nothing else: no way to read across a row, no column headers, no sense of
// where in the sheet you are.
//
// Only the visible window is mirrored — a million cells of DOM would defeat the
// point of drawing to a canvas at all. That is invisible to the reader because
// every cell carries its **absolute** `aria-rowindex`/`aria-colindex` against
// the sheet's declared counts, which is what those attributes exist for: "row
// 4,201 of 1,048,576" stays true when only rows 4,190–4,230 are in the DOM.
//
// It mirrors `geo.rowIdx`/`geo.colIdx` — the indices the renderer just drew —
// so a hidden or filtered-out row is absent from the mirror for the same reason
// it is absent from the screen, without a second notion of what is visible.
let a11yEl;
let a11ySignature = "";

// Caps so an unusually large window cannot make the rebuild expensive. Past the
// cap the reader still gets the cells nearest the top-left of the view, which is
// where navigation is.
const A11Y_MAX_ROWS = 60;
const A11Y_MAX_COLS = 40;

const a11yCellId = (row, col) => `a11y-${row}-${col}`;

// Rebuilding the accessibility mirror is not scroll work.
//
// The mirror is hundreds of DOM nodes, and its guard against a redundant
// rebuild hashes `JSON.stringify(geoItems)` — every visible cell, every frame.
// While scrolling that signature genuinely changes each frame, so the guard
// never fires: the tree is rebuilt continuously, the layout is dirtied
// continuously, and the scrollbar's `clientHeight` read has to flush all of it.
// A CPU profile of a scroll put 36.8% in `updateScrollbars` and 6.6% here —
// 43% of the frame in two functions that do not draw a cell.
//
// Deferred **only while the view is moving**. An edit is not a scroll: the
// mirror is the only DOM this canvas has, so it is what a screen reader reads
// *and* what a test reads, and delaying it by even a frame delays the
// announcement of a paste. That distinction was not free — deferring
// unconditionally broke `a copied value pastes as itself`, which reads
// `#a11y-7-0` and had nothing to read for 120ms. The test was right and the
// first version of this was wrong.
//
// ...and deferring *until the motion stops* was wrong too (`A11Y-01`). The
// settle timer was re-armed on every frame, so a scroll that never settles
// never rebuilds: measured on an unmodified tree, 1.5s of continuous wheel
// scrolling left the mirror **220 rows behind the screen**, and the bound is
// the length of the gesture rather than any number in this file. A canvas grid
// has no other accessible representation, so for a screen-reader user that is
// not a stale detail — it is the grid showing somewhere they are not.
//
// So: deferred, but with a ceiling. While the view moves the mirror is rebuilt
// at most once per `A11Y_MAX_STALE_MS`, and the settle timer still fires
// afterwards so the tree that comes to rest is exact.
//
// **What that trades.** Between the ceiling and the next rebuild the mirror is
// still stale — up to a quarter second of it — which is the deliberate half of
// the design: a mirror that is never stale is a mirror rebuilt every frame,
// which is the 43%-of-frame cost `PERF-D-01` removed. The number is chosen
// against what a reader can actually consume: announcing one row of cells takes
// well over a second in any screen reader, so a quarter second cannot let a
// reader *finish* a row that was already gone when it started. And it caps the
// rate at 4 rebuilds/second where the pre-`PERF-D-01` path did 60 — 6.7% of the
// cost, for a bound that does not depend on how long somebody holds a scroll.
//
// **What it costs, measured** (`tests/browser/frame-profile.mjs`, 1.5s of
// continuous scrolling, before and after on the same machine and server):
// the median frame is 16.7ms either way. The tail is where it shows: on a
// 41-column window — 1200 mirrored cells, the worst case this file allows —
// max frame goes from 17.7ms to ~25ms and frames over 20ms from 0-1 to ~5 per
// 86, which is the four rebuilds a second arriving. One rebuild is ~5ms of JS
// plus the layout of ~1300 nodes. On a 13-column window it is at the edge of
// measurable. So: roughly four slightly-long frames per second, while the wheel
// or a finger is actually moving, and nothing at all while reading, editing or
// navigating by keyboard.
const A11Y_MAX_STALE_MS = 250;
let a11ySettle = 0;
let a11yRebuilds = 0;
let a11yFreshAt = 0;
let movingUntil = 0;
export const a11yRebuildCountForTest = () => a11yRebuilds;
/// The staleness ceiling, so the gate asserts the contract rather than a second
/// copy of the number.
export const a11yMaxStaleMsForTest = () => A11Y_MAX_STALE_MS;

/// What a frame actually fetched, against what it can show.
///
/// The `PERF-D-01` measurement could see that frame cost tracked *visible
/// columns* and not sheet size, but not why. It was this: `colCap`/`rowCap`
/// are floored by `MIN_LINE`, so a view of ordinary-width columns asked the
/// engine for roughly eight times the lines it could draw. A wall-clock
/// assertion on that would be flaky; the count is the thing that was wrong and
/// the thing worth pinning.
export function frameWindowForTest() {
  return {
    colIdx: geo.colIdx.length,
    rowIdx: geo.rowIdx.length,
    cols: geo.cols,
    rows: geo.rows,
    geoItems: geoItems.length,
    spillCols: geoItems.filter((it) => geo.colOf.get(it.c) === undefined).map((it) => it.c),
  };
}
/// Called by the scroll paths: the view is moving, so the mirror can wait.
export function viewIsMoving() { movingUntil = performance.now() + 90; }

function scheduleA11yGrid() {
  clearTimeout(a11ySettle);
  const now = performance.now();
  if (now >= movingUntil) {
    // Standing still — an edit, a selection, a format. Announce it now.
    rebuildA11yGrid();
    return;
  }
  // Moving. Wait for the view to settle — but no longer than the ceiling allows
  // the mirror to describe a screen that is not there. Always through a timer,
  // never inline: this runs from `draw`, inside the animation frame, and a
  // rebuild there dirties layout the browser then has to resolve before it can
  // paint. Off the frame it is the same work in a task of its own.
  const settle = Math.min(120, Math.max(0, A11Y_MAX_STALE_MS - (now - a11yFreshAt)));
  a11ySettle = setTimeout(() => {
    a11ySettle = 0;
    rebuildA11yGrid();
  }, settle);
}

function rebuildA11yGrid() {
  if (!a11yEl || !wasm || !geo.rowIdx || !geo.colIdx) return;
  a11yRebuilds += 1;
  // Before the early-out below, not after: a signature that has not changed
  // means the mirror is *already* true for this frame, and re-checking it 16ms
  // later costs a `JSON.stringify` of the whole window for no gain.
  a11yFreshAt = performance.now();
  const rows = geo.rowIdx.slice(0, A11Y_MAX_ROWS);
  const cols = geo.colIdx.slice(0, A11Y_MAX_COLS);
  if (!rows.length || !cols.length) return;

  const sel = selRect();
  const active = a11yCellId(state.sel.row, state.sel.col);

  // Rebuilding identical DOM every frame churns the accessibility tree and
  // makes some readers re-announce the whole grid, so only rebuild when what
  // the mirror shows has actually changed. The cell payload is part of the
  // signature, so an edit refreshes it without needing an edit counter.
  const signature = [
    state.sheet, rows[0], rows[rows.length - 1], cols[0], cols[cols.length - 1],
    sel.r0, sel.c0, sel.r1, sel.c1, geoItems.length,
  ].join(",") + "|" + JSON.stringify(geoItems);
  if (signature === a11ySignature) {
    canvas.setAttribute("aria-activedescendant", active);
    return;
  }
  a11ySignature = signature;

  const byKey = new Map();
  for (const it of geoItems) byKey.set(it.r + "," + it.c, it.t);

  const frag = document.createDocumentFragment();
  // A header row of column letters, so "column D" is something the reader can
  // say rather than something the user has to count to.
  const head = el("div", null);
  head.setAttribute("role", "row");
  head.setAttribute("aria-rowindex", "1");
  const corner = el("div", null, "");
  corner.setAttribute("role", "columnheader");
  corner.setAttribute("aria-colindex", "1");
  head.appendChild(corner);
  for (const c of cols) {
    const h = el("div", null, colName(c));
    h.setAttribute("role", "columnheader");
    // +2: the row-header column is index 1, and ARIA indices are 1-based.
    h.setAttribute("aria-colindex", String(c + 2));
    head.appendChild(h);
  }
  frag.appendChild(head);

  for (const r of rows) {
    const row = el("div", null);
    row.setAttribute("role", "row");
    // +2: the column-header row is index 1.
    row.setAttribute("aria-rowindex", String(r + 2));
    const rh = el("div", null, String(r + 1));
    rh.setAttribute("role", "rowheader");
    rh.setAttribute("aria-colindex", "1");
    row.appendChild(rh);
    for (const c of cols) {
      const text = byKey.get(r + "," + c);
      const cell = el("div", null, text || "");
      cell.id = a11yCellId(r, c);
      cell.setAttribute("role", "gridcell");
      cell.setAttribute("aria-colindex", String(c + 2));
      // A cell with no accessible name is skipped outright by some readers, so
      // an empty one says it is empty instead of vanishing from the row and
      // making the columns misalign as they are read across.
      if (!text) cell.setAttribute("aria-label", `${A1(r, c)} empty`);
      if (r >= sel.r0 && r <= sel.r1 && c >= sel.c0 && c <= sel.c1) {
        cell.setAttribute("aria-selected", "true");
      }
      row.appendChild(cell);
    }
    frag.appendChild(row);
  }

  a11yEl.textContent = "";
  a11yEl.appendChild(frag);
  canvas.setAttribute("aria-activedescendant", active);
}

// The counts a reader phrases "row 12 of N" against.
//
// Deliberately the **navigable** extent, not the used range: the same
// `+30 rows / +8 columns` past the data that the scrollbars offer. Counting
// only used rows would announce "row 40 of 6" the moment anyone scrolled below
// the data, since the mirror covers the screen and the screen goes further than
// the data does. The +1 on each is the header row and column, which are part of
// the grid as far as ARIA is concerned.

// Excel's status-bar mode word. Ready → Enter (typing a fresh value) → Edit
// (F2 into an existing one) → Point (picking a reference mid-formula).

// True while the selection is being extended from the keyboard; cleared the
// moment the selection is set outright.
export let extending = false;

// "AB" -> zero-based column index, or null.
// "B12" -> {row,col}, or null.
// Jump to a typed cell (B12) or range (A1:C5). Unknown names report to status.
// Parse what the Name Box accepts into a selection box: `B7`, `A1:C9`, a whole
// column band `A:C`, or a whole row band `2:5`. Returns null if it is not one of
// those — a defined name, most likely, which the caller tries next.

// Column letters to a zero-based index, or null.

// Delimited text arrives in whatever encoding produced it. The engine reads
// UTF-8, so anything else has to be converted here — a UTF-16 export opened as
// UTF-8 is not slightly wrong, it is unreadable.

// Turn an engine error into something that says what to do about it.

// Anything the importer had to drop or degrade, said once, plainly. The report
// exists in the engine; nothing surfaced it, so a lossy import looked clean.

// Fill the selection from its own first row or column, in an explicit mode.
// Which axis is decided by the selection's shape: a tall block fills down, a
// wide one fills right — the same reading the fill handle gives it.
function fillSelection(mode) {
  const s = effectiveRange();
  const rows = s.r1 - s.r0 + 1, cols = s.c1 - s.c0 + 1;
  if (rows < 2 && cols < 2) { status.textContent = "select the cells to fill"; return; }
  const down = rows >= cols;
  const src = down
    ? { r0: s.r0, c0: s.c0, r1: s.r0, c1: s.c1 }
    : { r0: s.r0, c0: s.c0, r1: s.r1, c1: s.c0 };
  tryEdit(() => wasm.session_fill_mode(
    state.sheet, src.r0, src.c0, src.r1, src.c1, s.r0, s.c0, s.r1, s.c1, mode));
  lastFill = { src, dst: { ...s } };
  status.textContent = `filled — ${mode}`;
}

// The fill-options button Excel drops at the corner of a fill. A fill has to
// guess between copying and continuing a series, and this is how you tell it it
// guessed wrong — without which the only recourse is undo and a different drag.
export let lastFill = null;



// Paste Special (Ctrl+Alt+V): what to paste, and what to do with it when it
// lands. The context submenu covers the common three; this is the rest —
// transpose and the arithmetic combinations.

// Text to columns: split the selected column on a delimiter into the columns to
// its right. Runs entirely on values already in the sheet, so it needs no
// clipboard and no import path.

// Insert Function: the catalogue, searchable, with each entry's signature and
// summary. The `fx` beside the formula bar was decorative — the only way to find
// a function was to already know its name and start typing it.
function insertFunctionDialog() {
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const modal = byId("oc-modal");
  const body = byId("oc-modal-body");
  byId("oc-modal-title").textContent = "Insert function";
  body.textContent = "";

  const search = el("input", "panel-field");
  search.placeholder = "Search functions";
  search.spellcheck = false;
  const list = el("div", "fn-list");
  const detail = el("div", "panel-hint");
  body.append(search, list, detail);

  let chosen = null;
  const render = () => {
    const q = search.value.trim().toUpperCase();
    const items = (fnCatalog || []).filter((f) => !q || f.n.includes(q)).slice(0, 200);
    list.textContent = "";
    for (const f of items) {
      const b = el("button", "fn-row", f.n);
      b.addEventListener("click", () => {
        chosen = f;
        list.querySelectorAll(".fn-row").forEach((x) => x.classList.remove("on"));
        b.classList.add("on");
        detail.textContent = `${f.sig || f.n + "(…)"}${f.d ? " — " + f.d : ""}`;
      });
      b.addEventListener("dblclick", () => { chosen = f; ok.click(); });
      list.appendChild(b);
    }
    if (!items.length) list.appendChild(el("div", "panel-hint", "No matching function"));
  };
  search.addEventListener("input", render);
  render();

  const actions = el("div", "oc-confirm-actions");
  const cancel = el("button", "oc-btn", "Cancel");
  const ok = el("button", "oc-btn primary", "Insert");
  actions.append(cancel, ok);
  body.appendChild(actions);
  modal.hidden = false;

  const close = () => { modal.hidden = true; body.textContent = ""; };
  cancel.addEventListener("click", () => { close(); canvas.focus(); });
  ok.addEventListener("click", () => {
    const f = chosen || (fnCatalog || [])[0];
    close();
    if (!f) { canvas.focus(); return; }
    // Open an edit on the active cell and drop the call in with the caret
    // between the parentheses, ready for arguments.
    beginEdit(inline, "=" + f.n + "()");
    const at = inline.value.length - 1;
    inline.setSelectionRange(at, at);
    updateRefSpans();
  });
  search.focus();
}

// A caret on the Name Box listing the workbook's defined names. They are
// otherwise only reachable by typing one exactly, which means knowing it exists.

// Ctrl+F3 Name Manager: list defined names to navigate to or delete.
// Whether the caret (end of `before`) sits inside a "..." string literal, so we
// must not inject a cell reference or a function name there. Treats "" as an
// escaped quote within a string.
let fnCatalog = null;            // lazily-loaded function catalog
export let acState = null;             // active autocomplete: {matches, idx, start}
export let formulaRefDrag = null;      // click/drag ref insertion: {anchor, start, end}
export let acEl;

// The function-name token being typed just before the caret, if the caret sits
// somewhere a function name is valid (after =, an operator, "(", or ",").

function showAutocomplete() {
  const tok = currentFnToken();
  if (!tok) { hideAutocomplete(); return; }
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const up = tok.text.toUpperCase();
  const matches = fnCatalog.filter((f) => f.n.startsWith(up)).slice(0, 8);
  if (!matches.length) { hideAutocomplete(); return; }
  acState = { matches, idx: 0, start: tok.start };
  renderAutocomplete();
}


export function hideAutocomplete() { acState = null; if (acEl) acEl.hidden = true; }

// --- Argument hint ----------------------------------------------------------
// Once the caret is inside a function's parentheses the name list is no longer
// what you need — the question becomes "which argument am I typing?". This
// shows the signature with that argument emphasised, the way Excel and Sheets
// do, and follows the caret through nested calls.
export let sigEl;

// The innermost call the caret sits inside: its function name and the index of
// the argument being typed. Commas inside nested calls or string literals do
// not count, which is the whole difficulty.

export function updateSignatureTip() {
  // The name list takes precedence: both anchored to the caret would collide,
  // and while it is open the user is still choosing a function.
  if (!sigEl) return;
  const call = acState ? null : callAtCaret();
  if (!call) { sigEl.hidden = true; return; }
  if (!fnCatalog) { try { fnCatalog = JSON.parse(wasm.function_catalog()); } catch { fnCatalog = []; } }
  const fn = fnCatalog.find((f) => f.n === call.name);
  if (!fn) { sigEl.hidden = true; return; }
  const inner = /\(([^]*)\)\s*$/.exec(fn.sig);
  const args = inner ? inner[1].split(",").map((a) => a.trim()) : [];
  sigEl.textContent = "";
  const name = document.createElement("span");
  name.className = "sig-name";
  name.textContent = fn.n;
  sigEl.append(name, document.createTextNode("("));
  args.forEach((a, i) => {
    if (i) sigEl.append(document.createTextNode(", "));
    const span = document.createElement("span");
    // A trailing "…" argument keeps matching once the caret runs past the
    // named ones, which is what makes SUM(a, b, c, …) highlight sensibly.
    const variadic = i === args.length - 1 && /^[.…]/.test(a);
    span.className = "sig-arg" + (i === call.arg || (variadic && call.arg > i) ? " active" : "");
    span.textContent = a;
    sigEl.appendChild(span);
  });
  sigEl.append(document.createTextNode(")"));
  sigEl.hidden = false;
  const r = (editSurface ?? inline).getBoundingClientRect();
  const w = sigEl.offsetWidth;
  sigEl.style.left = Math.max(4, Math.min(r.left, window.innerWidth - 4 - w)) + "px";
  sigEl.style.top = r.bottom + 2 + "px";
}


// Whether the caret sits where a cell reference may be inserted by clicking.

// Insert a reference at the caret while editing. While a gesture that keeps
// adjusting the same reference is in progress — a mouse drag, or arrow-key
// point mode — the previously inserted text is replaced rather than appended.

export let tabsEl;

// Reset the viewport + selection to the top-left (e.g. on a sheet switch).

// Per-sheet view memory: switching sheets preserves each sheet's selection and
// scroll position (Excel/Sheets behavior) instead of slamming back to A1. Keyed
// by sheet NAME so it survives add/delete/reorder/undo without index-shift bugs
// (a rename just drops that one sheet's remembered view — acceptable).
export const sheetViews = new Map(); // sheet name → { scrollX, scrollY, sel, anchor, selKind }
// `keepEdit` leaves an in-progress edit open — used when a formula is picking a
// reference on another sheet, where switching sheets is part of *authoring* the
// formula rather than abandoning it.

// (Re)build the bottom sheet-tab bar from the engine's sheet list.
export function renderTabs() {
  const names = JSON.parse(wasm.session_sheet_names());
  let vis = [];
  try { vis = JSON.parse(wasm.session_sheet_visibility()); } catch {}
  let prot = [];
  try { prot = JSON.parse(wasm.session_sheet_protected()); } catch {}
  if (state.sheet >= names.length) state.sheet = names.length - 1;
  // Never sit on a hidden sheet — the tab is gone, so there would be no way
  // back to a visible one.
  if (vis[state.sheet] && vis[state.sheet] !== "visible") {
    const firstVisible = names.findIndex((_, i) => (vis[i] || "visible") === "visible");
    if (firstVisible >= 0) state.sheet = firstVisible;
  }
  tabsEl.textContent = "";
  names.forEach((name, i) => {
    // Hidden sheets keep their index (everything addresses sheets by it) but
    // get no tab; they are reachable through the tab context menu.
    if ((vis[i] || "visible") !== "visible") return;
    const b = document.createElement("button");
    b.className = "sheet-tab" + (i === state.sheet ? " active" : "");
    b.textContent = name;
    let tc = "";
    try { tc = wasm.session_tab_color(i); } catch (_) {}
    if (tc) {
      b.style.setProperty("--oc-x-tab-color", "#" + tc);
      b.classList.add("colored");
    }
    if (prot[i]) {
      // A protected sheet reads the same as any other until you try to edit it,
      // so say so on the tab itself.
      b.classList.add("protected");
      b.title = name + " (protected)";
    }
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", i === state.sheet ? "true" : "false");
    b.addEventListener("mousedown", (e) => {
      // Mid-formula, clicking a tab is part of writing the formula: keep the
      // edit alive and do not let the click blur the editor.
      if (refAcceptable()) { e.preventDefault(); switchSheet(i, true); }
    });
    b.addEventListener("click", () => { if (!editSurface) switchSheet(i); });
    b.addEventListener("dblclick", () => renameSheet(i, b));
    b.addEventListener("contextmenu", (e) => { e.preventDefault(); sheetMenu(i, e.clientX, e.clientY); });
    // Drag to reorder.
    b.draggable = true;
    b.addEventListener("dragstart", (e) => { dragTab = i; e.dataTransfer.effectAllowed = "move"; b.classList.add("dragging"); });
    b.addEventListener("dragend", () => { dragTab = -1; b.classList.remove("dragging"); });
    b.addEventListener("dragover", (e) => { e.preventDefault(); e.dataTransfer.dropEffect = "move"; });
    b.addEventListener("drop", (e) => { e.preventDefault(); moveTab(dragTab, i); });
    tabsEl.appendChild(b);
  });
  const add = document.createElement("button");
  add.className = "sheet-add";
  add.title = "Add sheet";
  add.setAttribute("aria-label", "Add sheet");
  // oc-safe-html: a literal SVG icon.
  add.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="icon-sm"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>';
  add.addEventListener("click", () => {
    try {
      const i = wasm.session_add_sheet();
      switchSheet(i);
      renderTabs();
    } catch (e) { statusError(errText(e)); }
  });
  tabsEl.appendChild(add);

  // All-sheets menu. The strip scrolls once there are more tabs than fit, but
  // scrolling only helps if you already know where you are going — this lists
  // every sheet, hidden ones included, and jumps straight to it.
  const all = document.createElement("button");
  all.className = "sheet-add sheet-all";
  all.title = "All sheets";
  all.setAttribute("aria-label", "All sheets");
  // oc-safe-html: a literal SVG icon.
  all.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="icon-sm"><line x1="4" y1="7" x2="20" y2="7"/><line x1="4" y1="12" x2="20" y2="12"/><line x1="4" y1="17" x2="20" y2="17"/></svg>';
  all.addEventListener("click", (e) => {
    e.stopPropagation();
    closeSheetMenu();
    const menu = document.createElement("div");
    menu.className = "popmenu ctx-menu";
    menu.id = "sheet-ctx";
    names.forEach((n, idx) => {
      const hiddenHere = (vis[idx] || "visible") !== "visible";
      const b = document.createElement("button");
      b.className = "menu-item" + (idx === state.sheet ? " on" : "");
      b.textContent = n + (hiddenHere ? "  (hidden)" : "") + (prot[idx] ? "  🔒" : "");
      b.addEventListener("click", () => {
        closeSheetMenu();
        // Jumping to a hidden sheet reveals it: the alternative is switching to
        // something with no tab and no way back.
        // A refused reveal stays put and says why: switching anyway would land
        // on the sheet with no tab that this branch exists to avoid.
        if (hiddenHere) {
          try { wasm.session_set_sheet_visibility(idx, "visible"); }
          catch (err) { statusError(errText(err)); return; }
        }
        switchSheet(idx);
        renderTabs();
        draw();
      });
      menu.appendChild(b);
    });
    const r = all.getBoundingClientRect();
    positionMenu(menu, r.left, r.top - 4);
  });
  tabsEl.appendChild(all);

  // Keep the active tab in view once the strip overflows.
  //
  // By hand, not `scrollIntoView`. That walks **every** scrollable ancestor,
  // and when the editor is embedded the outermost one is somebody else's page:
  // drawing the tab strip scrolled the whole landing page down to the iframe,
  // taking the hero with it. `block: "nearest"` bounds how far each ancestor
  // moves, not which ancestors move (`UX-EMBED-01`).
  //
  // Only the strip's own `scrollLeft` is touched, so nothing outside this
  // element can move however deeply it is nested.
  const activeTab = tabsEl.querySelector(".sheet-tab.active");
  if (activeTab) {
    const left = activeTab.offsetLeft;
    const right = left + activeTab.offsetWidth;
    const viewLeft = tabsEl.scrollLeft;
    const viewRight = viewLeft + tabsEl.clientWidth;
    if (left < viewLeft) {
      tabsEl.scrollLeft = left;
    } else if (right > viewRight) {
      tabsEl.scrollLeft = right - tabsEl.clientWidth;
    }
  }
}

// Reorder sheet tabs, keeping the active sheet tracked through the shift.

// Inline-rename a sheet tab.

// Right-click context menu for a sheet tab.

// Append a context menu at (x,y), flipping up/left if it would overflow.

// Show or hide the selected cell's data-validation input hint.
//
// A tooltip pinned under the cell rather than a status-bar line: it belongs to
// the cell, and it has to survive the status bar being used for something else.

// A thrown value as a sentence.
//
// A `JsError` from the engine stringifies as "Error: …", so interpolating it
// after the word "error" read "error: Error: this sheet is protected".

// Put a message in the status bar as an error, without going through innerHTML.
//
// The wording can come from the file — a data-validation rule carries the
// author's own text — so interpolating it into markup would let a workbook
// inject nodes into the page.


// Ctrl +/- structural edits, axis chosen by the selection kind: whole-column
// selection acts on columns, whole-row on rows, otherwise rows (Excel's default
// for a cell selection). `count` spans the selection.

// Right-click menu on a cell: clipboard + structural row/column edits.
// Trimmed right-click menu: fast verbs only. Multi-option groups (Paste
// special, Insert, Delete, Hide, Clear, Sort) fold into submenus so the menu
// stays short; the heavier editors (validation / conditional format / notes)
// live in the side panel, reached from the toolbar.
/// Whether a cell shift needs confirming before it runs.
///
/// **A probe that cannot answer means "warn", not "proceed".** This began at
/// `false` inside a swallowing `catch`, so a probe that threw asserted the
/// *safe* answer and the confirmation was skipped for precisely the shift that
/// was about to break formulas — the one case where the warning is the only
/// thing standing between the user and silent corruption (`UX-SHIFT-01`).
///
/// The cost of the two answers is not symmetric, which is the whole argument:
/// guessing "risky" wrongly costs one extra dialog, and guessing "safe" wrongly
/// costs formulas that now point at different cells with nothing on screen to
/// say so.
///
/// Takes the probe as a callback so the failing case is reachable from a test
/// without a wasm module that can be made to throw on demand.


// Ask for a row height / column width in pixels, seeded with the current one.
// A plain prompt rather than a styled modal: it is a single number, and the
// alternative was no way to set an exact size at all.

// The context menu for a row or column header. Same chrome as the cell menu,
// but every verb names — and acts on — the band that was right-clicked.

// Ctrl+D / Ctrl+R: fill the selection from its own first row / first column.
// The source is that edge, the destination is the rest of the block — which is
// exactly the drag-fill the handle performs, without the dragging.

// Double-clicking the fill handle fills down to the extent of the neighbouring
// column's data — Excel's "finish this column" gesture, which beats dragging
// when the table is a thousand rows long. Uses the column to the left, falling
// back to the one on the right, as Excel does.

// Live-update the drag-fill target box (extends the source in the dominant axis).

// Live-update the previewed size of the line being dragged.

// The formula bar's expand toggle, in one place.
//
// The `fx` chevron and Ctrl+Shift+U are the same command, so they share the
// same three lines: `resize()` is not optional — the bar growing changes the
// canvas's height, and skipping it leaves the grid painting under the bar.
export function toggleFormulaBarExpanded() {
  const bar = qs(".formula-bar");
  if (!bar) return false;
  const on = bar.classList.toggle("expanded");
  const btn = byId("fx-expand");
  if (btn) btn.setAttribute("aria-expanded", on ? "true" : "false");
  resize();
  return on;
}

// Excel's Ctrl+Shift+O — select every cell on this sheet that carries a note.
//
// A multi-range bank, the same structure Ctrl+click builds: the first cell
// becomes the active one and the rest are banked, so the operations that read
// the bank (bold, Delete, copy) act on all of them, and the Name Box names the
// one the caret is on.
//
// The range asked of the engine is the whole sheet rather than `usedBounds()`.
// A note is anchored to a cell whether or not that cell has a value, and a
// note beside empty cells is the most likely kind — bounding the query by the
// used range would silently miss exactly those.
export function selectCommentedCells() {
  let cells = [];
  try { cells = JSON.parse(wasm.session_comments(state.sheet, 0, 0, 1048575, 16383)); }
  catch (err) { statusError(errText(err)); return; }
  if (!cells.length) { status.textContent = "no cells with notes on this sheet"; return; }
  // Built backwards on purpose. `addRange` banks whatever is active and makes
  // its argument the new active cell, so walking forwards leaves the caret on
  // the *last* note — and Excel leaves it on the first, which is also the only
  // useful place to leave it: the point of the chord is to start reading the
  // notes, and reading starts at the top.
  select(cells[cells.length - 1].r, cells[cells.length - 1].c);
  for (let i = cells.length - 2; i >= 0; i -= 1) addRange(cells[i].r, cells[i].c);
  status.textContent = cells.length === 1
    ? "1 cell with a note" : `${cells.length} cells with notes`;
}

// How far Alt+Down looks up and down the column for entries. Excel stops at a
// blank; this stops at a blank *or* here, so a column of a million filled cells
// cannot turn one keystroke into a million engine calls.
const PICK_LIST_SCAN = 1000;

// Excel's Alt+Down on a cell with no validation rule: the text already entered
// in this column, offered as a list to pick from.
//
// Text only, and the contiguous run only — both are Excel's rules and both
// earn their place. Numbers are excluded because a column of amounts would
// produce a list nobody can use; formulas are excluded because picking one
// would paste its source text, not its value. The run stops at a blank so two
// unrelated tables stacked in one column do not offer each other's entries.
export function openColumnPickList() {
  const col = state.sel.col;
  const seen = new Set();
  const values = [];
  const scan = (step) => {
    for (let i = 1; i <= PICK_LIST_SCAN; i += 1) {
      const r = state.sel.row + i * step;
      if (r < 0) break;
      let v = "";
      try { v = wasm.session_cell_input(state.sheet, r, col); } catch { break; }
      if (!v) break;                                   // a blank ends the run
      if (v.startsWith("=")) continue;                 // a formula, not an entry
      if (v.trim() === "" || Number.isFinite(Number(v))) continue;
      if (seen.has(v)) continue;
      seen.add(v);
      values.push(v);
    }
  };
  scan(-1);
  scan(1);
  if (!values.length) { status.textContent = "no entries in this column to pick from"; return; }
  values.sort((a, b) => a.localeCompare(b));
  closeSheetMenu();
  const menu = document.createElement("div");
  menu.className = "popmenu ctx-menu dv-menu";
  menu.id = "sheet-ctx";
  for (const val of values) {
    const b = document.createElement("button");
    b.textContent = val;
    b.addEventListener("click", () => {
      closeSheetMenu();
      tryEdit(() => wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, val));
      canvas.focus();
    });
    menu.appendChild(b);
  }
  // Under the active cell, the way the validation dropdown places itself. The
  // active cell is on screen (every `select` calls `ensureVisible`), but the
  // geometry lookups still answer `undefined` off-screen, so both are guarded.
  const rect = canvas.getBoundingClientRect();
  const x = colXAt(col) ?? 0;
  const y = rowYAt(state.sel.row) ?? 0;
  positionMenu(menu, rect.left + x, rect.top + y + (rowHAt(state.sel.row) ?? 0));
}

function wireEvents() {
  // The real paste event, which is the only place `clipboardData` exists.
  //
  // Reading the clipboard through `navigator.clipboard.read()` needs a
  // permission prompt and returns nothing in Firefox; the event carries every
  // flavour the source application offered, for free, because the user just
  // asked for it. That is what makes formatting recoverable at all.
  document.addEventListener("paste", (e) => {
    // Not while a cell editor or a dialog input has focus — there the browser's
    // own text paste is exactly right.
    const target = e.target;
    if (target && /^(input|textarea)$/i.test(target.tagName ?? "")) return;
    if (target?.isContentEditable) return;
    e.preventDefault();
    void doPaste(e);
  });

  // Autofilter header buttons open on click, not mousedown — see the note in
  // the mousedown handler. stopPropagation keeps any already-armed
  // dismiss-on-next-click from closing the menu we are about to open.
  canvas.addEventListener("click", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    const fb = filterButtonAt(px, py);
    if (!fb) return;
    e.stopPropagation();
    openColumnFilter(
      fb.col,
      rect.left + fb.x * state.zoom,
      rect.top + (fb.y + fb.h) * state.zoom,
    );
  });

  // A header that is already selected, and so would start a *move* rather than
  // a new selection. This is the same test the mousedown below makes, named
  // once so the pointer can say so before the button goes down: a gesture that
  // only exists once you have guessed it exists is a gesture nobody finds.
  const grabbableHeaderAt = (px, py) => {
    const sr = selRect();
    if (py < HH && px >= HW) {
      const c = colAtX(px);
      return state.selKind === "cols" && c >= sr.c0 && c <= sr.c1;
    }
    if (px < HW && py >= HH) {
      const r = rowAtY(py);
      return state.selKind === "rows" && r >= sr.r0 && r <= sr.r1;
    }
    return false;
  };

  canvas.addEventListener("mousedown", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    // While editing a formula at a reference position, clicking a cell inserts
    // its reference instead of moving the selection. preventDefault keeps the
    // inline input focused (no blur/commit); a drag turns it into a range.
    // A plain click on a linked cell follows it, as Excel does. Guarded three
    // ways so it cannot hijack ordinary work: not while picking a formula
    // reference, not with a modifier held (Ctrl-click is how you select a
    // linked cell without leaving), and not on a second click of a cell that is
    // already selected — that is someone starting a drag or a rename, not
    // asking to navigate.
    if (!refAcceptable() && !e.ctrlKey && !e.metaKey && !e.shiftKey && e.button === 0) {
      const hit = cellAt(px, py);
      if (
        hit
        && linkCells.has(hit.row + "," + hit.col)
        && !(hit.row === state.sel.row && hit.col === state.sel.col)
      ) {
        e.preventDefault();
        select(hit.row, hit.col);
        followHyperlink(hit.row, hit.col);
        draw();
        return;
      }
    }
    if (refAcceptable()) {
      const hit = cellAt(px, py);
      if (hit) {
        e.preventDefault();
        formulaRefDrag = { anchor: hit, start: editSurface.selectionStart, end: editSurface.selectionStart };
        insertRef(A1(hit.row, hit.col));
        hideAutocomplete();
        return;
      }
    }
    // A chart: its frame floats over the cells, so it is tested before them or
    // a click would land on whatever is underneath and the chart could never
    // be picked up.
    if (e.button === 0 && chartMouseDown(px, py)) {
      if (editSurface && !commit(editSurface.value, false)) return;
      e.preventDefault();
      canvas.focus();
      return;
    }
    // An outline collapse toggle in the gutter.
    const ot = outlineToggleAt(px, py);
    if (ot) {
      tryEdit(() => wasm.session_toggle_outline(state.sheet, ot.index, ot.columns));
      return;
    }
    // A hidden-band handle in either header: one click brings the band back.
    // Double-click still works (below) for anyone who learned it that way.
    const hm = hiddenMarkAt(px, py);
    if (hm) { unhideMark(hm); return; }
    // An autofilter header button. Swallowed here so the press neither moves
    // the selection nor starts a drag; the menu itself opens on the `click`
    // that follows (see the canvas click handler) because `positionMenu` arms
    // its dismiss-on-next-click with a zero timeout — open from mousedown and
    // that timer fires before the click, so the click closes the menu on the
    // spot and it only appears while the button is held.
    if (filterButtonAt(px, py)) {
      if (editSurface && !commit(editSurface.value, false)) return;
      e.preventDefault();
      return;
    }
    // The active cell's data-validation dropdown button.
    if (validationChevron) {
      const c = validationChevron;
      if (px >= c.x && px <= c.x + c.w && py >= c.y && py <= c.y + c.h) {
        openValidationMenu(); canvas.focus(); return;
      }
    }
    // Any other click while editing first commits the in-progress edit (like
    // Excel) rather than discarding it. If the value is an invalid formula the
    // commit is refused and the click is swallowed so the user stays in the cell.
    if (editSurface && !commit(editSurface.value, false)) return;
    // The fill handle (bottom-right of the selection) starts a drag-fill.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      endInline();
      state.fill = { src: selRect(), dst: null };
      canvas.focus();
      return;
    }
    // Dragging the freeze divider (in the body) changes/removes the freeze.
    const fh = freezeHit(px, py);
    if (fh) { endInline(); state.freezeDrag = { axis: fh.axis, px, py }; return; }
    // A header boundary starts a column/row resize instead of a selection.
    const hb = boundaryAt(px, py);
    if (hb) {
      endInline();
      const cur = hb.axis === "col"
        ? JSON.parse(wasm.session_col_px(state.sheet, hb.index, 1))[0] || COL_W
        : JSON.parse(wasm.session_row_px(state.sheet, hb.index, 1))[0] || ROW_H;
      // Scope: whole sheet, the selected band (if the line is in it), or just one.
      const r = selRect();
      let scope = "one", b0 = hb.index, b1 = hb.index;
      if (state.selKind === "all") scope = "all";
      else if (hb.axis === "col" && state.selKind === "cols" && hb.index >= r.c0 && hb.index <= r.c1) { scope = "band"; b0 = r.c0; b1 = r.c1; }
      else if (hb.axis === "row" && state.selKind === "rows" && hb.index >= r.r0 && hb.index <= r.r1) { scope = "band"; b0 = r.r0; b1 = r.r1; }
      // `originPx` is what the line measured before the drag. The preview
      // writes straight to the sheet, so this is what has to go back before
      // the recorded edit runs — otherwise its inverse would be the last
      // previewed size and undo would step back to a size nobody chose.
      state.resize = { axis: hb.axis, index: hb.index, previewPx: cur, originPx: cur, scope, b0, b1 };
      return;
    }
    // Header clicks: select-all (corner), or a whole column/row — supporting
    // Shift-extend, Ctrl/Cmd multi-select (banking a range per addRange), and
    // drag-to-extend across adjacent headers (state.headerDrag drives both the
    // mousemove handler and edge auto-scroll below).
    const fnew = freezeHandleAt(px, py);
    if (fnew) { endInline(); state.freezeDrag = { axis: fnew.axis, px, py }; return; }
    if (px < HW && py < HH) { selectAll(); canvas.focus(); return; }
    if (py < HH && px >= HW) {
      endInline();
      const c = colAtX(px);
      const sr = selRect();
      // Already selected, no modifier: this is a move, not a new selection.
      if (state.selKind === "cols" && !e.shiftKey && !e.metaKey && !e.ctrlKey
          && c >= sr.c0 && c <= sr.c1) {
        // `px0`/`py0` is where the band was grabbed. The ghost follows the
        // pointer by *that* offset rather than centring on it, so the band
        // keeps the grip the user took — grab a wide band near its right edge
        // and it does not jump left the instant you move.
        state.moveDrag = {
          axis: "col", at: sr.c0, count: sr.c1 - sr.c0 + 1, before: sr.c0,
          px0: px, py0: py,
        };
        state.dragging = true;
        // The whole drag used to read as `cell`, because the mousemove branch
        // that runs it returns before the idle-hover block that sets the
        // cursor — so nothing on screen, the pointer included, said a drag was
        // in progress. Set once here and put back on mouseup.
        canvas.style.cursor = "grabbing";
        canvas.focus();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) addColumnRange(c);
      else selectColumn(c, e.shiftKey);
      state.headerDrag = "col";
      state.dragging = true;
      canvas.focus();
      return;
    }
    if (px < HW && py >= HH) {
      endInline();
      const r = rowAtY(py);
      const sr = selRect();
      if (state.selKind === "rows" && !e.shiftKey && !e.metaKey && !e.ctrlKey
          && r >= sr.r0 && r <= sr.r1) {
        state.moveDrag = {
          axis: "row", at: sr.r0, count: sr.r1 - sr.r0 + 1, before: sr.r0,
          px0: px, py0: py,
        };
        state.dragging = true;
        canvas.style.cursor = "grabbing";
        canvas.focus();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) addRowRange(r);
      else selectRow(r, e.shiftKey);
      state.headerDrag = "row";
      state.dragging = true;
      canvas.focus();
      return;
    }
    const hit = cellAt(px, py);
    // **The right button does not move the selection here.**
    //
    // `mousedown` fires *before* `contextmenu`, so a right-click used to reach
    // the `select()` below and collapse the selection to the clicked cell
    // before the menu opened. Reported from a running editor: `Ctrl+A` took the
    // block, a right-click inside it left one cell selected, and every verb in
    // the menu then acted on that cell rather than on what the user was looking
    // at — Delete included.
    //
    // The `contextmenu` handler already owns this policy and gets it right:
    // right-clicking *inside* a selection keeps it, outside moves it, which is
    // what Excel and Sheets both do. It just never got the chance. Buttons
    // other than the primary one are left to it.
    if (hit && e.button === 0) {
      endInline();
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey) { addRange(hit.row, hit.col); state.dragging = true; }
      else if (e.shiftKey) extend(hit.row, hit.col);
      else { select(hit.row, hit.col); state.dragging = true; }
      canvas.focus();
    } else if (hit) {
      canvas.focus();
    }
  });
  canvas.addEventListener("mousemove", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    if (chartDrag) { chartDrag.px = px; chartDrag.py = py; draw(); return; }
    if (state.freezeDrag) { state.freezeDrag.px = px; state.freezeDrag.py = py; draw(); return; }
    if (state.moveDrag) {
      const d = state.moveDrag;
      // Where the ghost is. Kept on the drag rather than in a module variable
      // so it cannot outlive the gesture that owns it.
      d.px = px;
      d.py = py;
      // The cursor is *not* set here. This branch returns before the idle-hover
      // block below, and nothing else writes the cursor while a move drag is
      // live, so the `grabbing` the mousedown set is still what is showing —
      // a second assignment here would be a line no mutation could falsify.
      // The drop lands *before* a line, so the half of it the pointer is in
      // decides which side — otherwise the last column on the sheet could never
      // be dropped after.
      if (d.axis === "col") {
        const c = Math.max(0, colAtX(px));
        const mid = colXAt(c) !== undefined ? colXAt(c) + colWAt(c) / 2 : px;
        d.before = px > mid ? c + 1 : c;
      } else {
        const r = Math.max(0, rowAtY(py));
        const mid = rowYAt(r) !== undefined ? rowYAt(r) + rowHAt(r) / 2 : py;
        d.before = py > mid ? r + 1 : r;
      }
      draw();
      return;
    }
    if (state.resize) { updateResize(px, py); return; }
    if (state.fill) { updateFill(px, py); return; }
    if (formulaRefDrag) {
      const hit = cellAt(px, py);
      if (hit) {
        const a = formulaRefDrag.anchor;
        const r0 = Math.min(a.row, hit.row), r1 = Math.max(a.row, hit.row);
        const c0 = Math.min(a.col, hit.col), c1 = Math.max(a.col, hit.col);
        const ref = (r0 === r1 && c0 === c1) ? A1(r0, c0) : `${A1(r0, c0)}:${A1(r1, c1)}`;
        insertRef(ref);
      }
      return;
    }
    if (state.dragging && state.headerDrag) {
      // Dragging across column/row headers extends the whole-column/row
      // selection to the header under the pointer (anchor stays put).
      dragPos = { px, py };
      if (state.headerDrag === "col") {
        const c = colAtX(Math.max(HW + 1, px));
        if (c !== state.sel.col) selectColumn(c, true);
      } else {
        const r = rowAtY(Math.max(HH + 1, py));
        if (r !== state.sel.row) selectRow(r, true);
      }
      maybeAutoScroll();
      return;
    }
    if (state.dragging) {
      dragPos = { px, py };
      const f = state.freeze || { bodyX0: HW, bodyY0: HH };
      // Clamp into the body so a pointer over a header still maps to a cell.
      const cx = Math.min(Math.max(px, f.bodyX0 + 1), rect.width - 2);
      const cy = Math.min(Math.max(py, f.bodyY0 + 1), rect.height - 2);
      const hit = cellAt(cx, cy);
      // Set the focus directly (no ensureVisible — it would fight auto-scroll
      // and make the selection flicker/vanish). Anchor stays put.
      if (hit && (hit.row !== state.sel.row || hit.col !== state.sel.col)) {
        state.sel = { row: hit.row, col: hit.col };
        state.selKind = "cells";
        draw();
      }
      maybeAutoScroll();
      return;
    }
    // Idle hover: fill cursor over the handle, resize cursor over a boundary.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      canvas.style.cursor = "crosshair";
      return;
    }
    if (outlineToggleAt(px, py)) { canvas.style.cursor = "pointer"; return; }
    // A hidden-band handle: say what it does, since a collapsed band is the one
    // thing on the grid you cannot select your way out of.
    const hmh = hiddenMarkAt(px, py);
    if (hmh) {
      canvas.style.cursor = "pointer";
      const n = hmh.mark.to - hmh.mark.from + 1;
      const what = hmh.axis === "col" ? "column" : "row";
      status.textContent = `${n} hidden ${what}${n === 1 ? "" : "s"} — click to show`;
      return;
    }
    const fh = freezeHit(px, py);
    const hb = fh ? null : boundaryAt(px, py);
    const fnew = freezeHandleAt(px, py);
    // Resizing wins over grabbing: the boundary is a few pixels inside the
    // band, and a user aiming at the line means the line.
    canvas.style.cursor = (fnew || fh || hb)
      ? ((fnew || fh || hb).axis === "col" ? "col-resize" : "row-resize")
      : grabbableHeaderAt(px, py) ? "grab"
      : "cell";
    // Comment tooltip on hover.
    const hit = !hb && py >= HH && px >= HW ? cellAt(px, py) : null;
    if (hit && errorCells.has(hit.row + "," + hit.col)) {
      // Hovering an error explains it, and names the formula that produced it —
      // the two things you need before you can fix it.
      let code = "", input = "";
      try {
        code = JSON.parse(wasm.session_cells(state.sheet, hit.row, hit.col, hit.row, hit.col))[0]?.t || "";
        input = wasm.session_cell_input(state.sheet, hit.row, hit.col);
      } catch {}
      const help = ERROR_HELP[code] || "This formula could not be calculated.";
      commentTip.textContent = input.startsWith("=") ? `${code} — ${help}\n${input}` : `${code} — ${help}`;
      commentTip.style.whiteSpace = "pre-line";
      commentTip.style.left = (px + 14) + "px";
      commentTip.style.top = (py + 8) + "px";
      commentTip.hidden = false;
    } else if (hit && numericTextCells.has(hit.row + "," + hit.col)) {
      // **The marker has to say what it means** (`DATA-NT-01`). A green corner
      // nobody can interpret is decoration; the whole point is that a zero from
      // `SUM` was unexplained, and an unexplained triangle beside it is no
      // better. It names the consequence and the fix, in that order.
      commentTip.textContent =
        "Number stored as text — SUM and AVERAGE skip this cell.\n"
        + "Data ▸ Convert text to numbers fixes the selection.";
      commentTip.style.whiteSpace = "pre-line";
      commentTip.style.left = (px + 14) + "px";
      commentTip.style.top = (py + 8) + "px";
      commentTip.hidden = false;
    } else if (hit && linkCells.has(hit.row + "," + hit.col)) {
      let link = null;
      try { link = JSON.parse(wasm.session_hyperlink_at(state.sheet, hit.row, hit.col)); } catch {}
      const dest = link && (link.tooltip || link.target || link.location);
      if (dest) {
        commentTip.textContent = dest;
        commentTip.style.whiteSpace = "";
        commentTip.style.left = (px + 14) + "px";
        commentTip.style.top = (py + 8) + "px";
        commentTip.hidden = false;
        canvas.style.cursor = "pointer";
      } else commentTip.hidden = true;
    } else if (hit && commentCells.has(hit.row + "," + hit.col)) {
      // The hover shows the whole thread, not just the opening remark: a reply
      // is usually the part that answers the question the cell raises, and
      // hiding it behind "open the panel" makes the indicator half-useful.
      const t = readThread(hit.row, hit.col);
      if (t && t.text) {
        const line = (e) => (e.author ? `${e.author}: ` : "") + e.text;
        const lines = [line(t), ...t.replies.map(line)];
        if (t.resolved) lines.unshift("✓ Resolved");
        commentTip.textContent = lines.join("\n");
        commentTip.style.whiteSpace = "pre-line";
        commentTip.style.left = (px + 14) + "px";
        commentTip.style.top = (py + 8) + "px";
        commentTip.hidden = false;
      } else commentTip.hidden = true;
    } else {
      commentTip.hidden = true;
    }
  });
  window.addEventListener("mouseup", (e) => {
    if (chartDrag) { chartMouseUp(); return; }
    if (state.moveDrag) {
      const d = state.moveDrag;
      state.moveDrag = null;
      state.dragging = false;
      // A drop inside the band or on either edge is a drop onto itself. The
      // engine treats it as an empty batch, but not calling at all keeps the
      // undo history free of a step that changed nothing.
      const inside = d.before >= d.at && d.before <= d.at + d.count;
      if (!inside) {
        try {
          if (d.axis === "col") wasm.session_move_columns(state.sheet, d.at, d.count, d.before);
          else wasm.session_move_rows(state.sheet, d.at, d.count, d.before);
          // The band travels, so the selection follows it to where it landed.
          const landed = d.before < d.at ? d.before : d.before - d.count;
          if (d.axis === "col") selectColumn(landed, false), extend(selRect().r1, landed + d.count - 1);
          else selectRow(landed, false), extend(landed + d.count - 1, selRect().c1);
          status.textContent = d.axis === "col"
            ? `moved ${d.count} column${d.count === 1 ? "" : "s"}`
            : `moved ${d.count} row${d.count === 1 ? "" : "s"}`;
        } catch (why) { statusError(errText(why)); }
        invalidateGrowth();
      }
      // The pointer is not moving — it has just been let go — so nothing else
      // recomputes the cursor until the user moves it again, and it would stay
      // `grabbing` over a sheet that is no longer being dragged. Read *after*
      // the move, so it answers for the band where it landed.
      {
        const r = canvas.getBoundingClientRect();
        const ux = (e.clientX - r.left) / state.zoom;
        const uy = (e.clientY - r.top) / state.zoom;
        canvas.style.cursor = grabbableHeaderAt(ux, uy) ? "grab" : "cell";
      }
      draw();
      return;
    }
    if (state.freezeDrag) {
      const d = state.freezeDrag;
      state.freezeDrag = null;
      commitFreezeDrag(d.axis, d.px, d.py);
      renderTabs();
      draw();
      return;
    }
    if (formulaRefDrag) {
      const caret = formulaRefDrag.end;
      formulaRefDrag = null;
      const surface = editSurface ?? inline;
      surface.focus();
      surface.setSelectionRange(caret, caret);
      return;
    }
    if (state.resize) {
      const r = state.resize;
      state.resize = null;
      // A narrower column wraps to more lines, so its rows grow differently.
      invalidateGrowth();
      const px = r.previewPx;
      try {
        // Put the previewed line back to where the drag found it, so the
        // operation recorded below inverts to the original size rather than to
        // the last frame of the drag.
        if (r.scope === "one" && r.originPx !== undefined) {
          wasm.session_preview_line_size(state.sheet, r.index, r.originPx, r.axis === "col");
        }
        if (r.axis === "col") {
          if (r.scope === "all") wasm.session_set_all_col_width(state.sheet, px);
          else if (r.scope === "band") wasm.session_set_col_width_range(state.sheet, r.b0, r.b1, px);
          else wasm.session_set_col_width(state.sheet, r.index, px);
        } else {
          if (r.scope === "all") wasm.session_set_all_row_height(state.sheet, px);
          else if (r.scope === "band") wasm.session_set_row_height_range(state.sheet, r.b0, r.b1, px);
          else wasm.session_set_row_height(state.sheet, r.index, px);
        }
        status.textContent = r.scope === "one" ? "resized" : "resized all";
      } catch (e) { statusError(errText(e)); }
      draw();
    }
    if (state.fill) {
      const f = state.fill;
      state.fill = null;
      const d = f.dst;
      if (d && (d.r0 !== f.src.r0 || d.c0 !== f.src.c0 || d.r1 !== f.src.r1 || d.c1 !== f.src.c1)) {
        // Ctrl inverts the default, as in Excel: a series-looking source copies
        // instead, and a plain one becomes a series.
        const mode = (e.ctrlKey || e.metaKey) ? "copy" : "auto";
        try {
          wasm.session_fill_mode(state.sheet, f.src.r0, f.src.c0, f.src.r1, f.src.c1,
                                 d.r0, d.c0, d.r1, d.c1, mode);
          status.textContent = "filled";
        } catch (err) { statusError(errText(err)); }
        state.anchor = { row: d.r0, col: d.c0 };
        state.sel = { row: d.r1, col: d.c1 };
        state.selKind = "cells";
        // Remember the fill so the options popup can redo it differently.
        lastFill = { src: f.src, dst: d };
        showFillOptions(d);
      }
      draw();
    }
    const wasDragging = state.dragging || state.headerDrag;
    state.dragging = false;
    state.headerDrag = null;
    dragPos = null;
    stopAutoScroll();
    // The format painter paints whatever was just selected — after the drag
    // finishes, so brushing across a block applies to the whole block.
    if (painter && wasDragging) applyPainter(effectiveRange());
  });

  // Scroll redraws are coalesced to one per frame.
  //
  // A trackpad delivers wheel events at 100-120Hz and a thumb drag emits one
  // per mousemove, and each was calling `draw()` synchronously — so the main
  // thread was asked for two or three full repaints inside a single frame, of
  // which the browser can only ever show the last. The surplus is not just
  // wasted, it is what makes the scroll feel heavy: the handler runs long
  // enough to push the frame past its budget, so the thing being drawn arrives
  // late. Moving the offsets stays synchronous, because it is cheap and the
  // scrollbar geometry reads it; only the painting waits for the frame.
  let drawFrame = 0;
  const scheduleDraw = () => {
    if (drawFrame) return;
    drawFrame = requestAnimationFrame(() => { drawFrame = 0; draw(); });
  };

  // Custom scrollbar thumb dragging.
  let sbDrag = null;
  const startThumb = (axis, el) => (e) => {
    e.preventDefault();
    const start = axis === "v" ? e.clientY : e.clientX;
    sbDrag = { axis, start, scroll0: axis === "v" ? state.scrollY : state.scrollX };
    el.classList.add("drag");
  };
  vthumb.addEventListener("mousedown", startThumb("v", vthumb));
  hthumb.addEventListener("mousedown", startThumb("h", hthumb));
  // Clicking the track pages toward the click — a viewport at a time, the way
  // every other scrollbar behaves. Without it the track was inert and the only
  // way to move a long way was to drag the thumb precisely.
  const pageFromTrack = (axis, track, thumb) => (e) => {
    if (e.target !== track) return; // the thumb handles its own drag
    const v = { w: wrap.clientWidth / state.zoom, h: wrap.clientHeight / state.zoom };
    const page = axis === "v" ? Math.max(1, v.h - HH) : Math.max(1, v.w - HW);
    const r = thumb.getBoundingClientRect();
    const before = axis === "v" ? e.clientY < r.top : e.clientX < r.left;
    if (axis === "v") state.scrollY = Math.max(0, state.scrollY + (before ? -page : page));
    else state.scrollX = Math.max(0, state.scrollX + (before ? -page : page));
    clampScroll();
    draw();
  };
  vscroll.addEventListener("mousedown", pageFromTrack("v", vscroll, vthumb));
  hscroll.addEventListener("mousedown", pageFromTrack("h", hscroll, hthumb));
  window.addEventListener("mousemove", (e) => {
    if (!sbDrag) return;
    if (sbDrag.axis === "v") {
      const d = e.clientY - sbDrag.start;
      state.scrollY = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.vSpan) * scrollMeta.maxScrollY);
    } else {
      const d = e.clientX - sbDrag.start;
      state.scrollX = Math.max(0, sbDrag.scroll0 + (d / scrollMeta.hSpan) * scrollMeta.maxScrollX);
    }
    scheduleDraw();
  });
  window.addEventListener("mouseup", () => {
    if (sbDrag) { vthumb.classList.remove("drag"); hthumb.classList.remove("drag"); sbDrag = null; }
  });
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom, py = (e.clientY - rect.top) / state.zoom;
    // A right-click in a header targets that row or column. cellAt() returns
    // null over the header strips, so this used to fall through and open the
    // cell menu against whatever was selected *before* — every verb in it then
    // acted on the wrong target, including Delete.
    if (HH && py < HH && px >= HW) {
      const col = colAtX(px);
      const r = selRect();
      const covered = state.selKind === "cols" && col >= r.c0 && col <= r.c1;
      if (!covered) selectColumn(col, false);
      headerMenu("col", e.clientX, e.clientY);
      return;
    }
    if (HW && px < HW && py >= HH) {
      const row = rowAtY(py);
      const r = selRect();
      const covered = state.selKind === "rows" && row >= r.r0 && row <= r.r1;
      if (!covered) selectRow(row, false);
      headerMenu("row", e.clientX, e.clientY);
      return;
    }
    if (HW && HH && px < HW && py < HH) { selectAll(); cellMenu(e.clientX, e.clientY); return; }
    const hit = cellAt(px, py);
    if (hit && state.selKind === "cells") {
      const r = selRect();
      const inside = hit.row >= r.r0 && hit.row <= r.r1 && hit.col >= r.c0 && hit.col <= r.c1;
      if (!inside) select(hit.row, hit.col);
    }
    cellMenu(e.clientX, e.clientY);
  });
  canvas.addEventListener("dblclick", (e) => {
    const rect = canvas.getBoundingClientRect();
    const px = (e.clientX - rect.left) / state.zoom;
    const py = (e.clientY - rect.top) / state.zoom;
    // The fill handle: double-click fills down to the neighbouring column's
    // last row. Checked before the header/boundary cases because the handle can
    // sit anywhere in the body.
    if (fillHandleRect && Math.abs(px - fillHandleRect.x) <= 5 && Math.abs(py - fillHandleRect.y) <= 5) {
      autofillToNeighbour();
      return;
    }
    // Double-clicking a hidden-band handle unhides it too (single click already
    // did, on mousedown — this only catches the second click of a fast pair).
    const hmd = hiddenMarkAt(px, py);
    if (hmd) { unhideMark(hmd); return; }
    // Double-clicking a column or row boundary auto-fits it to its content.
    const hb = boundaryAt(px, py);
    if (hb) {
      if (hb.axis === "col") autofitColumn(hb.index);
      else autofitRow(hb.index);
      return;
    }
    const hit = cellAt(px, py);
    if (hit) {
      select(hit.row, hit.col);
      startInline();
    }
  });
  wrap.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      // Ctrl/⌘+wheel zooms, as everywhere else. The step is proportional so it
      // feels even at both ends of the range.
      if (e.ctrlKey || e.metaKey) {
        setZoom(state.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
        return;
      }
      // Fluid pixel scrolling: move the absolute content offset directly, so the
      // grid glides smoothly instead of snapping a whole row/column at a time.
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? wrap.clientHeight : 1;
      // Lock to the dominant axis.
      //
      // A trackpad does not deliver a pure axis: a two-finger vertical scroll
      // carries a pixel or two of `deltaX` from the hand's own drift, tens of
      // times a second. Adding every one of them moved the sheet sideways while
      // the user watched it move down — and it accumulates rather than
      // cancelling, because a hand drifts one way, so the grid ends up somewhere
      // nobody put it. Reported from the desktop app as "scrolling up and down
      // scrolled slightly right", which is exactly what it is.
      //
      // A ratio rather than a fixed threshold, so it scales with how hard the
      // gesture is thrown, and generous enough that a genuinely diagonal scroll
      // keeps both components — locking those would make the grid fight the
      // hand.
      const AXIS_LOCK = 3;
      let dx = e.deltaX;
      let dy = e.deltaY;
      const ax = Math.abs(dx);
      const ay = Math.abs(dy);
      if (ay > ax * AXIS_LOCK) dx = 0;
      else if (ax > ay * AXIS_LOCK) dy = 0;
      // Shift turns a vertical wheel horizontal — the convention everywhere, and
      // the only way to pan sideways on a mouse with one wheel.
      if (e.shiftKey && dx === 0) {
        state.scrollX += dy * unit * scrollDamp;
      } else {
        state.scrollY += dy * unit * scrollDamp;
        state.scrollX += dx * unit * scrollDamp;
      }
      clampScroll();
      viewIsMoving();
      scheduleDraw();
    },
    { passive: false },
  );

  // --- Touch: pan and pinch ------------------------------------------------
  // The grid is a canvas this editor scrolls itself, and every ancestor is
  // `overflow: clip`, `hidden` or `visible` — there is nothing for a browser to
  // scroll natively. With no touch listeners either, a phone could show the
  // first screenful of a workbook and never anything else. Tap and double-tap
  // worked the whole time, because the browser synthesises click and dblclick,
  // so the app looked fine until you tried to reach row 30.
  //
  // Deliberately touch events rather than pointer events: every selection path
  // here is built on mouse events, and unifying them under `pointerdown` would
  // rewrite the working half to fix the missing one.
  const TAP_SLOP = 8; // px a finger may wander before it is a drag, not a tap
  const LONG_PRESS_MS = 500; // a held finger is a right-click
  const GLIDE_DECAY = 0.94; // per 16ms frame
  const GLIDE_MIN = 0.04; // px/ms below which the glide has stopped
  // How long a finger's last measured velocity stays worth throwing with. A
  // finger that pauses produces no `touchmove`, so the only evidence of a pause
  // is the gap before the release — which makes this gap the one signal
  // available and a hard cut-off the wrong way to read it. Decayed rather than
  // cut: a cut-off threw a genuine flick away whenever the release arrived late
  // (a loaded machine, a slow phone) and kept full speed for a finger that had
  // paused just inside it. Both errors are invisible; the first reads as "the
  // flick did not take", which is exactly what a user calls laggy.
  const GLIDE_STALE_MS = 300;
  let pan = null;
  let pinch = null;
  let pressTimer = null;
  let pressFired = false;
  let glide = null;

  const cancelPress = () => {
    if (pressTimer !== null) { clearTimeout(pressTimer); pressTimer = null; }
  };
  const stopGlide = () => {
    if (glide) { cancelAnimationFrame(glide.raf); glide = null; }
  };
  // Inertia. Without it the sheet stops dead the instant the finger leaves,
  // which is the single thing that makes a touch surface feel like a web page
  // rather than an application. The velocity is the finger's, so the content
  // moves against it — the same sign as the drag itself.
  const startGlide = (vx, vy) => {
    stopGlide();
    if (Math.hypot(vx, vy) < GLIDE_MIN) return;
    let last = performance.now();
    const step = (now) => {
      // Clamped, because a backgrounded tab resumes with one enormous frame and
      // would otherwise fling the sheet to its last row.
      const dt = Math.min(32, now - last);
      last = now;
      state.scrollX -= vx * dt;
      state.scrollY -= vy * dt;
      clampScroll();
      // A glide is a scroll that the finger has already let go of, and the two
      // scroll paths that say so — `wheel` and `touchmove` — both stop firing
      // the moment the throw begins. So this was the one kind of motion the
      // accessibility mirror's deferral never saw: measured at **40 rebuilds
      // in a single fling**, the whole per-frame cost `PERF-D-01` removed,
      // still there on the platform with the least frame budget to spare.
      viewIsMoving();
      scheduleDraw();
      const decay = GLIDE_DECAY ** (dt / 16);
      vx *= decay;
      vy *= decay;
      if (Math.hypot(vx, vy) < GLIDE_MIN) { glide = null; return; }
      glide.raf = requestAnimationFrame(step);
    };
    glide = { raf: requestAnimationFrame(step) };
  };
  const spread = (touches) =>
    Math.hypot(
      touches[0].clientX - touches[1].clientX,
      touches[0].clientY - touches[1].clientY,
    );

  canvas.addEventListener(
    "touchstart",
    (e) => {
      if (state.editing) return;
      // The evidence that a finger is what is pointing at this grid. Set before
      // the gesture is classified, and before the synthesised click that a tap
      // turns into, so the repaint that tap causes already carries handles. (It
      // is below the `state.editing` guard rather than above it: a touch that
      // the handler declines to act on is not evidence about anything, and the
      // next one latches it in any case.)
      touchSeen = true;
      if (e.touches.length === 1) {
        const t = e.touches[0];
        // Touching a gliding sheet stops it, as it does everywhere else.
        stopGlide();
        // A selection handle takes the gesture off the pan before the pan is
        // set up: on a phone a drag is the scroll, so the handles are the only
        // thing that can mean "extend this range" and they have to win.
        // `preventDefault` suppresses the synthesised click too, which is what
        // stops a grab that turns out to be a tap from moving the selection to
        // the cell under the handle.
        const rect = canvas.getBoundingClientRect();
        const hpx = (t.clientX - rect.left) / (state.zoom || 1);
        const hpy = (t.clientY - rect.top) / (state.zoom || 1);
        if (startHandleDrag(hpx, hpy)) {
          pan = null;
          pinch = null;
          pressFired = false;
          cancelPress();
          canvas.focus();
          draw();
          e.preventDefault();
          return;
        }
        pressFired = false;
        pan = {
          x: t.clientX, y: t.clientY, sx: state.scrollX, sy: state.scrollY, moved: false,
          lastX: t.clientX, lastY: t.clientY, lastT: performance.now(), vx: 0, vy: 0,
        };
        pinch = null;
        // A held finger raises the same `contextmenu` the right button does,
        // rather than growing a second, thinner menu that drifts from it. That
        // handler already understands headers, the corner and cells.
        cancelPress();
        pressTimer = setTimeout(() => {
          pressTimer = null;
          if (!pan || pan.moved) return;
          const at = { x: pan.x, y: pan.y };
          pan = null; // the press became a menu, so the finger is no longer panning
          pressFired = true;
          canvas.dispatchEvent(new MouseEvent("contextmenu", {
            bubbles: true, cancelable: true, clientX: at.x, clientY: at.y,
          }));
        }, LONG_PRESS_MS);
      } else if (e.touches.length === 2) {
        // A second finger cancels the pan rather than fighting it: the midpoint
        // of a pinch drifts, and panning to it makes the sheet lurch. A handle
        // drag ends for the same reason, keeping whatever range it had reached.
        pan = null;
        endHandleDrag();
        cancelPress();
        stopGlide();
        pinch = { spread: spread(e.touches), zoom: state.zoom };
        e.preventDefault();
      }
    },
    { passive: false },
  );

  canvas.addEventListener(
    "touchmove",
    (e) => {
      if (pinch && e.touches.length === 2) {
        const now = spread(e.touches);
        if (pinch.spread > 0) setZoom(pinch.zoom * (now / pinch.spread));
        e.preventDefault();
        return;
      }
      if (handleDrag && e.touches.length === 1) {
        const t = e.touches[0];
        const rect = canvas.getBoundingClientRect();
        const z = state.zoom || 1;
        moveHandleDrag((t.clientX - rect.left) / z, (t.clientY - rect.top) / z);
        e.preventDefault();
        return;
      }
      if (!pan || e.touches.length !== 1) return;
      const t = e.touches[0];
      const dx = t.clientX - pan.x;
      const dy = t.clientY - pan.y;
      // Below the slop it is still a tap, and preventing default here would stop
      // the click the browser is about to synthesise — which is what selects a
      // cell.
      if (!pan.moved && Math.hypot(dx, dy) < TAP_SLOP) return;
      // Past the slop it is a drag, so it is not a press however long it is
      // held. A menu appearing mid-swipe would stop the sheet under the finger.
      cancelPress();
      pan.moved = true;
      // Velocity for the glide, smoothed: a single frame's delta is noisy, and
      // the last sample before release is the one that decides the throw.
      const now = performance.now();
      const dt = now - pan.lastT;
      if (dt > 0) {
        pan.vx = pan.vx * 0.7 + ((t.clientX - pan.lastX) / dt) * 0.3;
        pan.vy = pan.vy * 0.7 + ((t.clientY - pan.lastY) / dt) * 0.3;
      }
      pan.lastX = t.clientX;
      pan.lastY = t.clientY;
      pan.lastT = now;
      // The sheet follows the finger: drag up and the content comes up, which
      // means scrolling down. Raw pixels, like the wheel handler above, and
      // without its damping — a finger is already 1:1 with the screen.
      state.scrollX = pan.sx - dx;
      state.scrollY = pan.sy - dy;
      clampScroll();
      viewIsMoving();
      scheduleDraw();
      e.preventDefault();
    },
    { passive: false },
  );

  canvas.addEventListener(
    "touchend",
    (e) => {
      // A handle drag ends here and nowhere else, and it must not also select:
      // the click the browser synthesises where the finger lifts would land on
      // whatever cell the handle was dragged to and collapse the range that was
      // just built — the one outcome that makes the gesture useless.
      if (handleDrag) {
        endHandleDrag();
        cancelPress();
        e.preventDefault();
        return;
      }
      // A pan must not also select. The browser synthesises a click where the
      // finger lifts, so without this every swipe would move the selection to
      // wherever the drag happened to end — scrolling a sheet would silently
      // change which cell you were on.
      if (pan && pan.moved) e.preventDefault();
      // The menu is open; the click the browser is about to synthesise would
      // land on the document and close it again immediately.
      if (pressFired) { e.preventDefault(); pressFired = false; }
      cancelPress();
      // A finger lifted while still moving throws the sheet, in proportion to
      // how recently it was last seen moving.
      if (pan && pan.moved) {
        const idle = performance.now() - pan.lastT;
        const freshness = Math.max(0, 1 - idle / GLIDE_STALE_MS);
        startGlide(pan.vx * freshness, pan.vy * freshness);
      }
      pan = null;
      if (e.touches.length < 2) pinch = null;
    },
    { passive: false },
  );
  canvas.addEventListener("touchcancel", () => {
    cancelPress();
    endHandleDrag();
    pan = null;
    pinch = null;
  });

  canvas.addEventListener("keydown", async (e) => {
    if (state.editing) return;
    // Alt+Down is Excel's in-column pick list: the validation dropdown when the
    // cell has one, and otherwise the text already entered in this column.
    //
    // It used to be the first of those only, and with no list to open the chord
    // fell through to the plain ArrowDown below and **moved the selection down
    // one row** — the second rebinding `docs/12` §4.1 names. A chord that moves
    // the cursor when the user asked for a list is the cheap kind of wrong
    // (no undo needed) and still the expensive kind to live with, because it
    // fires on the way to typing and the typing then lands a row late.
    //
    // `preventDefault` unconditionally, including when there is nothing to
    // offer: falling through was the bug, and "no entries" is an answer.
    if (e.altKey && !e.ctrlKey && !e.metaKey && e.key === "ArrowDown") {
      if (validationChevron) openValidationMenu();
      else openColumnPickList();
      e.preventDefault();
      return;
    }
    const mod = e.ctrlKey || e.metaKey;

    // Alt+= — AutoSum. Outside the Ctrl/Cmd branch on purpose: Alt is not one
    // of those, and putting it there meant the key never fired and the "="
    // fell through to starting an edit.
    if (e.altKey && !mod && (e.key === "=" || e.key === "+")) {
      autoSum();
      e.preventDefault();
      return;
    }

    // Alt+PageUp/PageDown page sideways (Excel parity).
    //
    // **Outside the Ctrl/Cmd branch**, where it spent its life one nesting
    // level too deep: inside `if (mod)` it answered only to Ctrl+Alt+PgUp/PgDn,
    // so Excel's own binding did nothing at all and the feature was unreachable
    // by the keys it documents (`UX-PAGE-01`). It sits with the other Alt
    // shortcuts, which are out here for exactly the same reason — Alt is not
    // one of `mod`, and Alt+= had already been moved for it.
    //
    // Placed before the branch rather than after, so Ctrl+Alt keeps paging
    // sideways as it always did instead of falling through to the sheet switch.
    if (e.altKey && (e.key === "PageDown" || e.key === "PageUp")) {
      const v = wrap.clientWidth / state.zoom;
      state.scrollX += (e.key === "PageDown" ? 1 : -1) * Math.max(1, v - HW);
      clampScroll();
      draw();
      e.preventDefault();
      return;
    }

    // Keyboard shortcuts.
    if (mod) {
      // Ctrl+Arrow: jump to the data-edge (Excel block-jump).
      const arrow = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (arrow) {
        // Jump from the travelling corner when extending, so a second
        // Ctrl+Shift+Arrow carries on from where the first one reached rather
        // than measuring again from the stationary active cell.
        const from = e.shiftKey ? state.anchor : state.sel;
        const to = JSON.parse(wasm.session_edge(state.sheet, from.row, from.col, arrow[0], arrow[1]));
        if (e.shiftKey) extend(to.row, to.col); else select(to.row, to.col);
        e.preventDefault(); return;
      }
      // Ctrl+PageDown / PageUp switch sheets (Excel parity).
      if (e.key === "PageDown") { const n = JSON.parse(wasm.session_sheet_names()).length; if (state.sheet < n - 1) switchSheet(state.sheet + 1); e.preventDefault(); return; }
      if (e.key === "PageUp") { if (state.sheet > 0) switchSheet(state.sheet - 1); e.preventDefault(); return; }
      const k = e.key.toLowerCase();
      // Ctrl+T creates a table over the selection, as in Excel.
      if (k === "t") { tableDialog(); e.preventDefault(); return; }
      // Excel's Ctrl+` — show formulas instead of results.
      if (k === "`") { setViewOption("formulas"); e.preventDefault(); return; }
      // Print the sheet, not the app: the grid is a canvas, so the browser's
      // own print of this page would produce one clipped screenshot.
      // Same rule as Ctrl+S, same table: `canPrint` governs `file.print`, and
      // the chord is that command however it is reached.
      if (k === "p") {
        e.preventDefault();
        if (capabilityForbids("file.print")
          && refuse("file.print", "printing is not available in this mode")) return;
        printSheet();
        return;
      }
      // Shift extends rather than collapses. Both of these called `select`
      // unconditionally, so Ctrl+Shift+End — "select everything from here
      // down", one of the most-used keys there is — threw the selection away
      // and left a single cell. The sibling handlers a few lines up (Ctrl+arrow)
      // and below (plain Home/End) both branch on it already.
      if (k === "home") {
        if (e.shiftKey) extend(0, 0); else select(0, 0);
        e.preventDefault(); return;
      }
      if (k === "end") {
        const b = usedBounds();
        if (e.shiftKey) extend(b.rows - 1, b.cols - 1); else select(b.rows - 1, b.cols - 1);
        e.preventDefault(); return;
      }
      // Ctrl+D / Ctrl+R: fill the selection down from its top row / right from
      // its left column — the fastest way to copy a formula over a block.
      if (k === "d" && !e.shiftKey) { fillWithin("down"); e.preventDefault(); return; }
      if (k === "r" && !e.shiftKey) { fillWithin("right"); e.preventDefault(); return; }
      // Excel's number-format shortcuts. These are muscle memory for anyone who
      // uses a spreadsheet daily, and the codes are Excel's own — `0.00` for
      // Number rather than something tidier, because a file saved here has to
      // read the same as one saved there.
      if (e.shiftKey) {
        const NUMFMT = {
          "~": "General", "`": "General",
          "!": "#,##0.00", "1": "#,##0.00",
          "$": "$#,##0.00", "4": "$#,##0.00",
          "%": "0%", "5": "0%",
          "^": "0.00E+00", "6": "0.00E+00",
          "#": "d-mmm-yy", "3": "d-mmm-yy",
          "@": "h:mm AM/PM", "2": "h:mm AM/PM",
        };
        const code = NUMFMT[e.key] || NUMFMT[k];
        if (code) { setNumberFormat(code); e.preventDefault(); return; }
      }
      // Ctrl+9 hides rows and Ctrl+0 hides columns in Excel. Zoom reset moved
      // to Ctrl+Alt+0: a shortcut that does something *else* in Excel is worse
      // than one that is missing, because the finger memory is already wrong.
      //
      // **That decision stands, and the menu was the thing that was wrong.**
      // View ▸ Zoom ▸ 100% advertised `Ctrl+0` while the handler below bound
      // that chord to hide-column, so the menu was teaching a user a keystroke
      // that destroys the view instead of resetting it — a label promising one
      // thing and delivering another is worse than either binding, because it
      // is the app itself telling the user to press it. Overruling the rule
      // above instead would trade a wrong label for a wrong *chord*, and the
      // chord is the half that is already in an Excel user's fingers. The label
      // now reads `Ctrl+Alt+0`, which is what actually resets the zoom.
      if (k === "0" && e.altKey) { setZoom(1); e.preventDefault(); return; }
      if (k === "9" && !e.shiftKey) {
        const r = effectiveRange();
        tryEdit(() => wasm.session_hide_rows(state.sheet, r.r0, r.r1));
        e.preventDefault(); return;
      }
      if (k === "0" && !e.shiftKey) {
        const r = effectiveRange();
        tryEdit(() => wasm.session_hide_cols(state.sheet, r.c0, r.c1));
        e.preventDefault(); return;
      }
      // Ctrl+K — Insert Hyperlink. The menu has advertised this all along
      // without anything listening for it.
      if (k === "k") { hyperlinkDialog(); e.preventDefault(); return; }
      // Ctrl+; stamps today's date, Ctrl+Shift+; the time. Both are *static*
      // values, not TODAY()/NOW() — that is the whole point of them, and it is
      // why they need no clock in the calc engine.
      //
      // **Both characters the key can send.** `Ctrl+Shift+;` does not arrive as
      // `";"` — Shift changes the character, so the event carries `e.key === ":"`
      // with `e.code === "Semicolon"` [measured]. The `e.shiftKey` branch three
      // lines down was therefore unreachable from the moment it was written:
      // the time stamp existed in full and no keystroke could ever reach it,
      // which is why `docs/12` §4.1 measured the chord as doing nothing. This is
      // the whole class of bug that reading cannot see — the code is correct and
      // the door to it is locked.
      if (e.key === ";" || e.key === ":") {
        const now = hostNow();
        const text = e.shiftKey
          ? `${now.getHours()}:${String(now.getMinutes()).padStart(2, "0")}`
          : `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
        tryEdit(() => wasm.session_set_cell(state.sheet, state.sel.row, state.sel.col, text));
        e.preventDefault(); return;
      }
      // Ctrl+1 — Format Cells, the shortcut every spreadsheet user already has
      // in their fingers.
      if (k === "1" && !e.shiftKey) { formatCellsDialog(); e.preventDefault(); return; }
      // Ctrl+Alt+V — Paste Special, as everywhere else.
      if (k === "v" && e.altKey) { pasteSpecialDialog(); e.preventDefault(); return; }
      if (k === "b") { toggleBold(); e.preventDefault(); return; }
      if (k === "i") { toggleItalic(); e.preventDefault(); return; }
      // Ctrl+Shift+U is Excel's "expand the formula bar", and it used to fall
      // into the line below and **underline the selection** instead. That is
      // the rebinding this file already argues against twice (Ctrl+9/0,
      // Ctrl+Shift+L), and it was the worst-behaved instance of it: the chord
      // an Excel user presses to *see more of a formula* silently changed the
      // document's formatting, so the cost was an undo plus a moment spent
      // wondering what had just been typed. `docs/12` §4.1 recorded it as doing
      // nothing, because an underline on an empty cell is invisible — the
      // measurement was right about the user's experience and wrong about the
      // model, which is the more expensive half.
      if (k === "u" && e.shiftKey) { toggleFormulaBarExpanded(); e.preventDefault(); return; }
      if (k === "u") { toggleUnderline(); e.preventDefault(); return; }
      // Excel's Ctrl+Shift+O — select every cell on the sheet carrying a note,
      // so "where did I leave comments?" is one keystroke instead of a scroll.
      if (e.shiftKey && k === "o") { selectCommentedCells(); e.preventDefault(); return; }
      if (e.shiftKey && (k === "7" || k === "&")) { toggleBorder(); e.preventDefault(); return; }
      // Excel's Ctrl+Shift+L is Toggle Filter, and it is among the most-used
      // chords in daily spreadsheet work. It used to left-align here, borrowed
      // from Word — so the one key an Excel user hits without thinking made a
      // silent formatting change and no filter. The rule this repository
      // already states for Ctrl+9/0 applies with more force: a shortcut that
      // does something *else* in the app being migrated from is worse than one
      // that is missing, because the finger memory is already wrong.
      if (e.shiftKey && k === "l") { toggleFilter(); e.preventDefault(); return; }
      // Centre and right keep theirs: neither chord means anything in Excel, so
      // neither can contradict it. Left-align has no chord rather than a made-up
      // one — it is a toolbar button, and inventing a key to restore symmetry
      // would be inventing exactly the problem above.
      if (e.shiftKey && (k === "e" || k === "r")) {
        setAlign(k === "e" ? "center" : "right"); e.preventDefault(); return;
      }
      if (e.key === " ") { if (e.shiftKey) ctrlA(); else selectColsSpan(); e.preventDefault(); return; } // Ctrl+Space cols; Ctrl+Shift+Space all
      if (k === "a") { ctrlA(); e.preventDefault(); return; }
      // Ctrl+Shift+F is Excel's Format Cells (on its Font tab). It used to fall
      // into the `k === "f"` line below and open the **find bar** — a second
      // route to a dialog Ctrl+F already opens, paid for with the one chord a
      // migrating user presses to change a font. Nothing is lost by moving it:
      // Ctrl+F still finds, and Ctrl+Shift+F was never a second finder anybody
      // reached for on purpose. The dialog opens on its own first tab rather
      // than on Font — `formatCellsDialog()` takes no tab argument, and
      // inventing one is a change to `editor.dialogs.js`, not to a chord.
      if (k === "f" && e.shiftKey) { formatCellsDialog(); e.preventDefault(); return; }
      if (k === "f") { openFind(); e.preventDefault(); return; }
      // Ctrl+H is Excel's Replace. Every piece of it already existed —
      // #replace-input, #replace-all, session_replace_all — reachable only
      // by opening Find and tabbing across, which is not the shortcut.
      if (k === "h") { openReplace(); e.preventDefault(); return; }
      if (k === "g") { cellRef.focus(); e.preventDefault(); return; } // Go-To / Name box
      if (e.key === "F3") { const r = canvas.getBoundingClientRect(); openNameManager(r.left + 120, r.top + 90); e.preventDefault(); return; } // Name Manager
      if (k === "z" && !e.shiftKey) { doUndo(); e.preventDefault(); return; }
      if (k === "y" || (k === "z" && e.shiftKey)) { doRedo(); e.preventDefault(); return; }
      // The document's own format, not `.xlsx` regardless. `doSave` is the raw
      // Excel path: it ignored what was opened, never asked `session_save_loss()`
      // — whose binding says the sentence must be said *before* the download,
      // because afterwards the file is already on disk — set no status, and had
      // no `catch`, so inside this async listener a throw became an unhandled
      // rejection and a failed save produced neither a file nor a message.
      //
      // The File menu already named the hazard: its first Download entry is
      // "Same format as opened", because the others are conversions, and a
      // conversion chosen by accident is how a `.csv` comes back as a package
      // under its own name. Ctrl+S is the save nobody opens a menu for, and it
      // was the one doing the converting.
      // Ctrl+S answers to the same capability the Download submenu does, read
      // from the same table. A mode that takes six menu entries away and leaves
      // the chord that does the seventh has taken nothing away — and Ctrl+S is
      // precisely the one nobody opens a menu for. Refused out loud rather than
      // ignored: a chord that silently does nothing reads as a broken editor,
      // and under WOPI the honest answer is that saving belongs to the host.
      if (k === "s") {
        e.preventDefault();
        if (capabilityForbids("file.download") && refuse("file.download", capabilities.ownsFile
          ? "saving is the host application's — this editor does not own the document"
          : "downloading is not available in this mode")) return;
        await saveAs("native");
        return;
      }
      // **`Ctrl+O` and `Ctrl+N` were not bound at all** (`TAURI-012`).
      //
      // The browser's own Open-file and New-window dialogs took them. In a
      // desktop window that is plainly wrong; in a tab it is still not what
      // somebody inside a spreadsheet means by either chord. They route to the
      // same two File menu entries rather than to their own implementations, so
      // a capability that hides the menu item hides the chord with it — the
      // `Ctrl+S` reasoning above, applied to its neighbours.
      if (k === "o" || k === "n") {
        e.preventDefault();
        // Through `runCommand`, not through a second implementation. The id is
        // the same one the File menu entry carries, so a capability that hides
        // the entry refuses the chord too — `runCommand` returns false rather
        // than acting — and New keeps its confirmation, which is the whole
        // reason it must not be reimplemented here: it is the most destructive
        // verb in the application.
        runCommand(k === "o" ? "file.open" : "file.new");
        return;
      }
      if (k === "c") { await doCopy(); e.preventDefault(); return; }
      if (k === "x") { await doCut(); e.preventDefault(); return; }
      if (k === "v" && e.shiftKey) { doPasteMode("values"); e.preventDefault(); return; }
      // Ctrl+V is *not* handled here. Letting the browser raise its own
      // `paste` event is what puts `clipboardData` in our hands, and that is
      // the only way to read the `text/html` flavour without asking for
      // clipboard-read permission. The listener below does the work.
      if (k === "v") { return; }
      // Ctrl+Shift+"+" inserts rows/columns, Ctrl+"-" deletes them (Excel).
      // "+" arrives as key "+" or as "=" with Shift depending on the layout.
      if ((e.key === "+" || (k === "=" && e.shiftKey))) { insertLines(); e.preventDefault(); return; }
      if (e.key === "-" || e.key === "_") { deleteLines(); e.preventDefault(); return; }
      // Ctrl+Backspace — scroll the view back to the active cell (Excel).
      //
      // This is the one binding in the file that had to be added rather than
      // merely corrected. Unbound, the chord fell out of this branch and into
      // the `switch` below, where `case "Backspace"` **clears the selection's
      // contents**: a user who scrolled away, pressed the chord Excel gives
      // them for "take me back", and got their data deleted at a cell they
      // could not even see. A missing shortcut does nothing; an unbound chord
      // that lands on a destructive one is worse than missing, and it is worse
      // precisely because the user believes they pressed something safe.
      //
      // `ensureVisible()` with no arguments is the active cell, and it is the
      // same call every other jump in the editor makes — the name box, find,
      // and a peer's cursor all reach the same scroll offsets through it, so
      // this cannot land somewhere they would not. `draw()` is its companion:
      // `ensureVisible` only moves `state.scrollX/Y`, it paints nothing.
      // Nothing about the selection changes, which is Excel's behaviour too —
      // the view moves to the cell, not the cell to the view.
      if (e.key === "Backspace") { ensureVisible(); draw(); e.preventDefault(); return; }
    }

    // A Shift-step continues from the corner that is travelling — which is
    // `state.anchor` (see `extend`) — while a plain step moves the active cell.
    // After a plain `select` the two are the same cell, so this is only ever
    // different mid-extension, which is exactly when it matters.
    const move = (dr, dc) => {
      if (e.shiftKey) {
        const to = stepFrom(state.anchor.row, state.anchor.col, dr, dc);
        extend(to.row, to.col);
      } else {
        const to = stepFrom(state.sel.row, state.sel.col, dr, dc);
        select(to.row, to.col);
      }
    };
    // **End mode** (Excel): `End` on its own arms it and moves nothing; the next
    // arrow jumps to the edge of the data block, and `End` then `Home` goes to
    // the last used cell. It is how a keyboard user crosses a large sheet
    // without holding a modifier, and it was the one navigation idiom missing
    // (`UX-END-01`).
    //
    // Handled before the switch below, because armed `End` changes what the
    // *next* key means — and because arming it must not also move the cursor,
    // which is what `End` used to do.
    if (state.endMode && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const jump = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (jump) {
        setEndMode(false);
        const from = e.shiftKey ? state.anchor : state.sel;
        const to = JSON.parse(wasm.session_edge(state.sheet, from.row, from.col, jump[0], jump[1]));
        if (e.shiftKey) extend(to.row, to.col); else select(to.row, to.col);
        e.preventDefault();
        return;
      }
      if (e.key === "Home") {
        // Excel's End,Home — the bottom-right of the used range.
        setEndMode(false);
        const b = usedBounds();
        const at = { r: Math.max(0, b.rows - 1), c: Math.max(0, b.cols - 1) };
        if (e.shiftKey) extend(at.r, at.c); else select(at.r, at.c);
        e.preventDefault();
        return;
      }
      // Anything else cancels it, as in Excel — an armed mode that survives an
      // unrelated keystroke fires on a later arrow the user has forgotten about.
      if (e.key !== "End" && e.key !== "Shift") setEndMode(false);
    }
    switch (e.key) {
      case "ArrowUp": move(-1, 0); e.preventDefault(); break;
      case "ArrowDown": move(1, 0); e.preventDefault(); break;
      case "Enter": enterStep(e.shiftKey); e.preventDefault(); break;
      case "ArrowLeft": move(0, -1); e.preventDefault(); break;
      case "ArrowRight": move(0, 1); e.preventDefault(); break;
      case "Tab": tabStep(e.shiftKey); e.preventDefault(); break;
      case "Home": if (e.shiftKey) extend(state.anchor.row, 0); else select(state.sel.row, 0); e.preventDefault(); break;
      // **`End` alone arms End mode and moves nothing**, which is what Excel
      // does. It used to jump to the last used column — which is Excel's
      // *`End` then `Right`*, so the shortcut existed while the mode it belongs
      // to did not, and the two-key idiom a spreadsheet user has in their
      // fingers did nothing.
      case "End": setEndMode(!state.endMode); e.preventDefault(); break;
      case "PageDown": { const p = Math.max(1, geo.rows - 1); move(p, 0); e.preventDefault(); break; }
      case "PageUp": { const p = Math.max(1, geo.rows - 1); move(-p, 0); e.preventDefault(); break; }
      case "Backspace": case "Delete":
        // A selected chart is what Delete acts on, as in Excel — the cells
        // under it are not selected and must not be cleared instead.
        if (chartSel && chartSel.sheet === state.sheet) {
          tryEdit(() => wasm.session_delete_chart(chartSel.sheet, chartSel.index));
          chartSel = null;
          panelChart = null;
          if (activePanel === "chart") refreshChartPanel();
          status.textContent = "chart deleted";
        } else {
          clearSelection();
        }
        e.preventDefault();
        break;
      case "F2": {
        if (e.shiftKey) openPanel("note"); // Shift+F2 → note (Excel parity)
        else startInline(undefined, true);
        e.preventDefault(); break;
      }
      // Plain F5 is Go To (the name box). Alt+F5 refreshes the pivot under the
      // cursor and Ctrl+Alt+F5 refreshes every one, both Excel's bindings.
      case "F5":
        if (e.altKey && (e.ctrlKey || e.metaKey)) refreshAllPivots();
        else if (e.altKey) refreshPivotHere();
        else cellRef.focus();
        e.preventDefault();
        break;
      // F9 recalculates, F11 (Shift) adds a sheet — both Excel's.
      case "F9":
        recalculateNow();
        e.preventDefault();
        break;
      case "F11":
        if (e.shiftKey) {
          try { switchSheet(wasm.session_add_sheet()); renderTabs(); }
          catch (err) { statusError(errText(err)); }
          e.preventDefault();
        }
        break;
      case " ": if (e.shiftKey) selectRowsSpan(); else startInline(" "); e.preventDefault(); break; // Shift+Space → whole rows
      case "Escape":
        if (painter) { setPainter(null); status.textContent = "format painter off"; e.preventDefault(); }
        else if (clipMarch) { stopMarch(); e.preventDefault(); }
        else if (chartSel) { chartSel = null; draw(); e.preventDefault(); }
        break;
      default:
        if (e.key.length === 1 && !mod) { startInline(e.key); e.preventDefault(); }
    }
  });
  // Both editing surfaces share one set of handlers: the in-cell editor and the
  // formula bar behave identically (autocomplete, Escape reverts, Enter commits
  // and advances, Tab commits and moves right), because they are two views of
  // the same edit session.
  for (const surface of [inline, fInput]) {
    surface.addEventListener("input", () => {
      // Typing in the formula bar with no edit open starts one.
      if (!editSurface) beginEdit(surface, surface.value);
      surface.classList.remove("invalid");
      // Typing anything ends point mode and leaves the reference it built in
      // place — a programmatic value change (insertRef) fires no input event,
      // so this only trips on real keystrokes.
      pointMode = null;
      mirrorEdit();
      showAutocomplete();
      updateSignatureTip();
      updateRefSpans();
    });
    // Moving the caret changes which argument you are in, without changing the
    // text — keyup and click are when the new position is readable.
    surface.addEventListener("keyup", (e) => {
      if (e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End") updateSignatureTip();
    });
    surface.addEventListener("click", () => updateSignatureTip());
    surface.addEventListener("keydown", (e) => {
      // Autocomplete navigation takes priority when its menu is open.
      if (acState) {
        if (e.key === "ArrowDown") { acState.idx = (acState.idx + 1) % acState.matches.length; renderAutocomplete(); e.preventDefault(); return; }
        if (e.key === "ArrowUp") { acState.idx = (acState.idx - 1 + acState.matches.length) % acState.matches.length; renderAutocomplete(); e.preventDefault(); return; }
        if (e.key === "Enter" || e.key === "Tab") { acceptAutocomplete(); e.preventDefault(); return; }
        if (e.key === "Escape") { hideAutocomplete(); e.preventDefault(); return; }
      }
      // Arrow keys build a reference when the caret sits where one may go
      // (point mode); everywhere else they move the text caret as usual.
      const step = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
      if (step && (pointMode || refAcceptable())) {
        if (pointStep(step[0], step[1], e.shiftKey)) { e.preventDefault(); return; }
      }
      if (e.key === "F4") { if (cycleAnchors()) e.preventDefault(); }
      // Alt+Enter breaks the line inside the cell instead of committing.
      //
      // This used to be `&& surface === inline`, because "the formula bar is a
      // single line" — true of an `<input>` and false the moment `UX-CHR-08`
      // stopped it being one. Excel takes Alt+Enter in the formula bar, and the
      // bar and the in-cell editor hold the same text, so refusing it in one of
      // the two places that edit a cell was an inconsistency the markup imposed
      // rather than a decision anybody made.
      else if (e.key === "Enter" && e.altKey) {
        e.preventDefault();
        const at = surface.selectionStart;
        surface.value = surface.value.slice(0, at) + "\n" + surface.value.slice(surface.selectionEnd);
        surface.setSelectionRange(at + 1, at + 1);
        mirrorEdit();
        // Only the in-cell editor is positioned over a cell; the bar is not.
        if (surface === inline) positionInline();
      }
      // Ctrl+Enter fills the whole selection with the entry, as in Excel.
      else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { commitToSelection(surface.value); e.preventDefault(); }
      else if (e.key === "Enter") { commit(surface.value, true); e.preventDefault(); }
      else if (e.key === "Escape") { cancelEdit(); e.preventDefault(); }
      // Excel's "enter mode": while *typing a fresh value*, an arrow commits and
      // moves. It must not while amending an existing value (the arrows move the
      // caret) or mid-formula (they pick references), which is what `editMode`
      // and the leading `=` distinguish.
      else if (
        editMode === "Enter" &&
        !surface.value.trim().startsWith("=") &&
        ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)
      ) {
        if (commit(surface.value, false)) {
          const d = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }[e.key];
          select(state.sel.row + d[0], state.sel.col + d[1]);
        }
        e.preventDefault();
      }
      else if (e.key === "Tab") {
        // Shift+Tab while editing moves left, which it previously could not.
        if (commit(surface.value, false)) tabStep(e.shiftKey);
        e.preventDefault();
      }
    });
  }
  // Clicking into the formula bar opens an edit on the active cell, so a click
  // there is the same gesture as F2 in the grid. Clicking it *during* an in-cell
  // edit hands that same edit over — the text is already mirrored, so only the
  // surface changes and the cell keeps showing what is typed.
  fInput.addEventListener("focus", () => {
    if (!editSurface) beginEdit(fInput);
    else if (editSurface === inline) editSurface = fInput;
  });
  // Leaving the formula bar commits, as Excel does. Two paths must not: handing
  // the edit back to the in-cell editor, and clicking a cell to pick a
  // reference (that path calls preventDefault, so no blur fires at all — the
  // formulaRefDrag guard is belt and braces).
  fInput.addEventListener("blur", (e) => {
    if (e.relatedTarget === inline || formulaRefDrag) return;
    if (editSurface === fInput) commit(fInput.value, false);
  });
  // Name box: Enter jumps to the typed cell/range; Escape reverts. Focus selects
  // all so it's ready to retype.
  cellRef.addEventListener("focus", () => cellRef.select());
  cellRef.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { gotoName(cellRef.value); canvas.focus(); updateNameBox(); e.preventDefault(); }
    else if (e.key === "Escape") { canvas.focus(); updateNameBox(); e.preventDefault(); }
  });

  // Find & replace bar.
  findInput.addEventListener("input", runFind);
  findInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { findStep(e.shiftKey ? -1 : 1); e.preventDefault(); }
    else if (e.key === "Escape") { closeFind(); e.preventDefault(); }
  });
  replaceInput.addEventListener("keydown", (e) => { if (e.key === "Escape") closeFind(); });
  for (const box of [findCase, findWhole, findValues, findAllSheets, findWildcards]) {
    box.addEventListener("change", runFind);
  }
  byId("find-next").addEventListener("click", () => findStep(1));
  byId("find-prev").addEventListener("click", () => findStep(-1));
  byId("replace-one").addEventListener("click", replaceOne);
  byId("replace-all").addEventListener("click", replaceAll);
  byId("find-close").addEventListener("click", closeFind);

  // The control behind `toolbar.delete-sheet` (see `editor.html`). Wired
  // through `byId`, which resolves against **this mount's root**, so an
  // embedded editor wires its own button inside its own shadow tree and two
  // editors on one page cannot answer for each other. Wiring it with
  // `document.getElementById` at module scope — the obvious way — returns null
  // in an embed, and the command would then be *listed* by an editor that
  // could not run it.
  //
  // It acts on the active sheet, which is the one the tabs, the grid and every
  // other sheet-level command already mean by "this sheet".
  byId("tb-delete-sheet").addEventListener("click", () => deleteSheetWithConfirm(state.sheet));

  // Popover menus: click toggles, outside-click / Escape closes, only one open.
  const menus = [];
  // The menus are `position: fixed` (to escape the toolbar's overflow clip), so
  // anchor each under its trigger button in viewport coordinates, flipping to
  // stay on-screen at the right and bottom edges.
  function anchorMenu(menu, btn) {
    const r = btn.getBoundingClientRect();
    menu.style.left = "0px";
    menu.style.top = "0px";
    menu.style.maxHeight = "";
    const mw = menu.offsetWidth, mh = menu.offsetHeight;
    let left = r.left;
    if (left + mw > window.innerWidth - 4) left = Math.max(4, window.innerWidth - 4 - mw);

    // Below the button if it fits, above if that fits better, and **clamped**
    // if neither does.
    //
    // It used to flip up unconditionally and clamp the result to 4px, which put
    // the tallest menu — Format — at the very top of the page, nowhere near the
    // button that opened it. Embedding made it routine rather than rare: an
    // editor half-way down a page has little room below and a menu taller than
    // the space above it.
    const below = window.innerHeight - 4 - (r.bottom + 4);
    const above = r.top - 8;
    let top;
    if (mh <= below || below >= above) {
      top = r.bottom + 4;
      if (mh > below) menu.style.maxHeight = Math.max(120, below) + "px";
    } else {
      top = Math.max(4, r.top - 4 - Math.min(mh, above));
      if (mh > above) menu.style.maxHeight = Math.max(120, above) + "px";
    }
    menu.style.left = left + "px";
    menu.style.top = top + "px";
  }
  function wirePopup(btnId, menuId, onItem) {
    const btn = byId(btnId);
    const menu = byId(menuId);
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      menu.hidden = !open;
      if (!menu.hidden) anchorMenu(menu, btn);
    });
    for (const item of menu.querySelectorAll("button")) {
      item.addEventListener("click", () => { onItem(item); menu.hidden = true; canvas.focus(); });
    }
  }
  document.addEventListener("click", () => { for (const m of menus) m.hidden = true; });

  // Color popovers: custom toggle so the hex field doesn't close the menu;
  // rebuilt on each open so the Recent row stays current.
  for (const [btnId, menuId, onPick, noneLabel] of [
    ["tb-fontcolor", "fontcolor-menu", setFontColor, "Automatic"],
    ["tb-fillcolor", "fillcolor-menu", setFill, "No fill"],
  ]) {
    const btn = byId(btnId);
    const menu = byId(menuId);
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      if (open) { buildColorMenu(menu, onPick, noneLabel); menu.hidden = false; anchorMenu(menu, btn); }
    });
  }
  wirePopup("tb-numfmt", "numfmt-menu", (b) =>
    (b.dataset.nf === "__custom__" ? customFormatDialog() : setNumberFormat(b.dataset.nf)));
  // Border palette: custom toggle (its placement buttons apply and close, but
  // the style select and color swatches must not), built once here.
  buildBorderMenu();
  // The palette is built once, so its "current pick" marks would go stale the
  // moment anything changed them. Re-sync on every open, and mirror the colour
  // onto the toolbar button so the choice is visible without opening the menu.
  function syncBorderPicks() {
    const menu = byId("border-menu");
    for (const sw of menu.querySelectorAll(".bd-color")) {
      sw.classList.toggle("on", (sw.dataset.color || "") === borderColor);
    }
    const sel = menu.querySelector(".bd-style");
    if (sel) sel.value = borderStyle;
    const btn = byId("tb-border");
    btn.style.setProperty("--oc-x-border-swatch", borderColor ? "#" + borderColor : "currentColor");
  }
  {
    const btn = byId("tb-border");
    const menu = byId("border-menu");
    menus.push(menu);
    btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = menu.hidden;
      for (const m of menus) m.hidden = true;
      menu.hidden = !open;
      if (!menu.hidden) { syncBorderPicks(); anchorMenu(menu, btn); }
    });
    syncBorderPicks();
  }
  // Styled tooltips over the chrome (converts native titles, incl. the border
  // palette just built above).
  initTooltips();
  wirePopup("tb-wrap", "wrap-menu", (b) => setTextOverflow(b.dataset.ov));
  wirePopup("tb-merge", "merge-menu", (b) => mergeVariant(b.dataset.mg));
  wirePopup("tb-rotate", "rotate-menu", (b) => setRotation(+b.dataset.rot));
  wirePopup("tb-valign", "valign-menu", (b) => setValign(b.dataset.va));
  wirePopup("tb-freeze", "freeze-menu", (b) => setFreeze(b.dataset.fz));
  wirePopup("tb-sort", "sort-menu", (b) =>
    (b.dataset.sort === "custom" ? sortDialog() : sortRange(b.dataset.sort === "desc")));
  byId("tb-filter").addEventListener("click", (e) => {
    e.stopPropagation();
    toggleFilter();
  });
  // Tool side panel: toolbar buttons toggle it; the header ✕ and Esc close it.
  // The three buttons these lines wired left the toolbar with `tbg-tools`
  // (`UX-CHR-06`). The commands did not: the menus call `togglePanel` directly
  // now, rather than clicking a button that is no longer there.
  byId("side-panel-close").addEventListener("click", () => closePanel());

  byId("tb-size-up").addEventListener("click", () => { stepFontSize(1); canvas.focus(); });
  byId("tb-size-down").addEventListener("click", () => { stepFontSize(-1); canvas.focus(); });
  byId("tb-bold").addEventListener("click", () => { toggleBold(); canvas.focus(); });
  byId("tb-italic").addEventListener("click", () => { toggleItalic(); canvas.focus(); });
  byId("tb-underline").addEventListener("click", () => { toggleUnderline(); canvas.focus(); });
  byId("tb-strike").addEventListener("click", () => { toggleStrike(); canvas.focus(); });
  byId("tb-indent-more").addEventListener("click", () => { setIndent(1); canvas.focus(); });
  byId("tb-indent-less").addEventListener("click", () => { setIndent(-1); canvas.focus(); });
  {
    const pb = byId("tb-painter");
    pb.addEventListener("click", () => { painter ? setPainter(null) : armPainter(false); canvas.focus(); });
    pb.addEventListener("dblclick", () => { armPainter(true); canvas.focus(); });
  }
  // --- The number-format readout (`UX-CHR-06`) ------------------------------
  //
  // **Named, not echoed.** Showing the raw code (`_($* #,##0.00_);_($* (#,...`)
  // would fill the control with something only a spreadsheet author can read,
  // and Accounting's code is 44 characters. The names here are the ones the
  // menu directly above already uses, so the readout and the way to change it
  // say the same word.
  //
  // Anything not in this table is a custom code and reads `Custom` — which is
  // true, is what Excel and OnlyOffice both do, and is shorter than any code
  // would be.
  const NF_NAMES = new Map([
    ["", "General"],
    ["0", "Number"], ["0.00", "Number"],
    ["#,##0", "Thousands"], ["#,##0.00", "Thousands"],
    ["0%", "Percent"], ["0.00%", "Percent"],
    ["$#,##0.00", "Currency"],
    ['_($* #,##0.00_);_($* (#,##0.00);_($* "-"??_);_(@_)', "Accounting"],
    ["yyyy-mm-dd", "Short date"], ["dddd, mmmm d, yyyy", "Long date"],
    ["h:mm:ss AM/PM", "Time"],
    ["0.00E+00", "Scientific"],
    ["@", "Text"],
  ]);
  // The active cell's format, cached on the coordinates it was read for.
  // `updateNumberFormatReadout` is called from the paint tail, so it runs on
  // every frame including every scroll frame; without this it would add a wasm
  // call and a `JSON.parse` to a path with a 60fps budget.
  let nfSeen = null;
  window.__ocInvalidateNfReadout = () => { nfSeen = null; };
  updateNumberFormatReadout = function updateNumberFormatReadout() {
    const label = byId("tb-numfmt-label");
    if (!label || !wasm) return;
    const key = `${state.sheet}:${state.sel.row}:${state.sel.col}`;
    let nf = "";
    try { nf = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col)).nf || ""; }
    catch { return; }
    const seen = `${key}|${nf}`;
    if (seen === nfSeen) return;
    nfSeen = seen;
    const name = NF_NAMES.get(nf) ?? "Custom";
    label.textContent = name;
    // The code itself is not thrown away, it moves to the tooltip: an author who
    // needs to know whether this is `0.00` or `#,##0.00` can still find out, and
    // both read `Number`.
    byId("tb-numfmt").title = nf ? `Number format: ${name} (${nf})` : "Number format: General";
  };
  byId("tb-currency").addEventListener("click", () => { setNumberFormat("$#,##0.00"); canvas.focus(); });
  // These are toggles, not one-way switches: pressing the button that is already
  // applied returns the cell to General, which is the only way back without
  // hunting through the number menu.
  const toggleFormat = (code) => () => {
    let current = "";
    try { current = JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col)).nf || ""; }
    catch {}
    setNumberFormat(current === code ? "" : code);
    canvas.focus();
  };
  byId("tb-percent").addEventListener("click", toggleFormat("0%"));
  byId("tb-comma").addEventListener("click", toggleFormat("#,##0.00"));
  byId("tb-inc-dec").addEventListener("click", () => { adjustDecimals(1); canvas.focus(); });
  byId("tb-dec-dec").addEventListener("click", () => { adjustDecimals(-1); canvas.focus(); });
  for (const b of qsa(".tb-align")) {
    b.addEventListener("click", () => { setAlign(b.dataset.al); canvas.focus(); });
  }
  // --- Font family / font size comboboxes ----------------------------------
  // Editable text box + a custom dropdown. NOT a native <input list=datalist>:
  // Chrome filters datalist options by the input's current text, so as soon as
  // the box mirrors the selected cell's font the dropdown collapsed to that one
  // entry ("only Calibri, no other fonts"). This list always opens in full,
  // narrows as you type, and still accepts any typed value.
  //
  // `values`: [{v, label, title}] — v is what gets applied, label what is shown.
  // `apply(v)`: commits the value. `preview`: render each row in its own face.
  function wireCombo({ input, caret, menu, values, apply, preview }) {
    menus.push(menu);
    const wrap = input.parentElement;
    let rows = [];       // the currently rendered option buttons
    let active = -1;     // index into rows of the highlighted option
    let committed = "";  // last value applied, so blur doesn't re-apply it

    const isOpen = () => !menu.hidden;
    function close() {
      menu.hidden = true;
      input.setAttribute("aria-expanded", "false");
      active = -1;
    }
    function highlight(i) {
      if (!rows.length) return;
      active = (i + rows.length) % rows.length;
      rows.forEach((b, n) => b.classList.toggle("active", n === active));
      rows[active].scrollIntoView({ block: "nearest" });
    }
    function choose(v) {
      input.value = v;
      committed = v;
      close();
      apply(v);
      canvas.focus();
    }
    // Build the option rows, narrowed to `filter` (a substring match, as the
    // box doubles as the search field). Returns how many rows are showing.
    function build(filter) {
      const f = (filter || "").trim().toLowerCase();
      const all = typeof values === "function" ? values() : values; // lazy: engine-sourced lists
      const list = f ? all.filter((o) => o.label.toLowerCase().includes(f)) : all;
      const cur = input.value.trim().toLowerCase();
      menu.textContent = "";
      active = -1;
      rows = list.map((o, i) => {
        const b = document.createElement("button");
        b.type = "button";
        b.className = "combo-item";
        b.setAttribute("role", "option");
        b.textContent = o.label;
        b.dataset.v = o.v; // label ≠ value for "Default" (which applies "")
        if (o.title) b.title = o.title;
        if (preview) b.style.fontFamily = fontStack(o.v);
        if (o.v.toLowerCase() === cur) { b.classList.add("checked"); active = i; }
        // mousedown must not blur the input (blur would close the menu first).
        b.addEventListener("mousedown", (e) => e.preventDefault());
        b.addEventListener("click", (e) => { e.stopPropagation(); choose(o.v); });
        menu.appendChild(b);
        return b;
      });
      return rows.length;
    }
    function open(filter) {
      for (const m of menus) if (m !== menu) m.hidden = true;
      if (!build(filter)) { close(); return; }
      menu.hidden = false;
      input.setAttribute("aria-expanded", "true");
      menu.style.minWidth = wrap.offsetWidth + "px";
      anchorMenu(menu, wrap);
      highlight(active < 0 ? 0 : active);
    }

    // The shared outside-click handler only sets `hidden`, which would leave
    // aria-expanded stale — close() properly on any click that isn't ours (the
    // caret and the option rows both stopPropagation).
    document.addEventListener("click", close);
    caret.addEventListener("mousedown", (e) => e.preventDefault()); // keep focus
    caret.addEventListener("click", (e) => {
      e.stopPropagation();
      if (isOpen()) { close(); return; }
      input.focus();
      open(""); // the caret always shows the whole list, never a filtered one
    });
    input.addEventListener("focus", () => { committed = input.value; input.select(); });
    input.addEventListener("input", () => open(input.value));
    input.addEventListener("blur", () => {
      close();
      // Typed-then-clicked-away still commits (the old <input> "change" path),
      // but only when the text actually changed — otherwise every focus pass
      // would push a redundant undo entry.
      if (input.value.trim() !== committed.trim()) { committed = input.value; apply(input.value.trim()); }
    });
    input.addEventListener("keydown", (e) => {
      e.stopPropagation(); // the grid's global handler must not see these keys
      if (e.key === "ArrowDown") { isOpen() ? highlight(active + 1) : open(""); e.preventDefault(); }
      else if (e.key === "ArrowUp") { if (isOpen()) { highlight(active - 1); e.preventDefault(); } }
      else if (e.key === "Enter") {
        e.preventDefault();
        if (isOpen() && rows[active]) choose(rows[active].dataset.v);
        else { committed = input.value; close(); apply(input.value.trim()); canvas.focus(); }
      } else if (e.key === "Escape") {
        e.preventDefault();
        // First Escape closes the list, a second reverts the box to the cell.
        // Order matters: mark the typed text as "committed" so the blur below
        // doesn't apply it, then blur (refreshFormulaBar skips a focused
        // control), then let the refresh restore the cell's real value.
        if (isOpen()) close();
        else { committed = input.value; canvas.focus(); refreshFormulaBar(); }
      } else if (e.key === "Tab") close();
    });
    return { close };
  }

  // Font family: every family the engine renders faithfully, each row drawn in
  // its own face. Substituted families say so in their tooltip rather than
  // pretending to be the real thing.
  const kindNote = { exact: "", metric: " — renders as %s (metric-compatible)", generic: " — renders as %s (closest match)" };
  wireCombo({
    input: byId("tb-font"),
    caret: byId("tb-font-caret"),
    menu: byId("font-menu"),
    values: () => [{ v: "", label: "Default", title: "Clear the font (use the workbook default)" }].concat(
      fontFamilies().map((f) => ({
        v: f.n,
        label: f.n,
        title: f.n + (kindNote[f.k] || "").replace("%s", f.f),
      }))),
    apply: (v) => setFontName(v.trim()),
    preview: true,
  });
  // Font size: the Excel ladder, but any typed size is accepted and clamped to
  // Excel's 1–409 pt range; a blank/zero clears the explicit size.
  wireCombo({
    input: byId("tb-size"),
    caret: byId("tb-size-caret"),
    menu: byId("size-menu"),
    values: [{ v: "", label: "Default", title: "Clear the size (use the workbook default)" }]
      .concat(SIZE_LADDER.map((n) => ({ v: String(n), label: String(n) }))), // same ladder as A▲/A▼
    apply: (v) => {
      const raw = parseFloat(v);
      setFontSize(Number.isFinite(raw) && raw > 0 ? Math.min(409, Math.max(1, raw)) : 0);
    },
  });
  // What the picker offers is the engine's list, not the markup's. The static
  // one in `editor.html` is a second table of supported formats and drifted
  // from the first — it omitted `.tab`, which the engine has always read — and
  // a format the engine grows would be unpickable until somebody remembered to
  // edit the HTML too (`WASM-01`).
  try {
    const offered = JSON.parse(wasm.openable_extensions());
    if (offered.length) {
      byId("tb-open").accept = offered.join(",");
    }
  } catch {}
  // **Which document this is, and whether it has been written out.**
  //
  // Two consumers of one fact, which is why they are one poller. The desktop
  // shell's window title is the original (`desktop/src/title.rs` renders
  // `figures.xlsx — OpenCalc` with an unsaved marker); the browser's document
  // strip is `UX-CHR-03`'s, and it exists **because a browser tab has no title
  // bar**. All five applications `docs/88` measures name the document in one,
  // and the desktop shell does too — so the region is deleted there and kept
  // here, carrying the same two values the title bar carries and nothing else.
  //
  // Polled, not pushed, for the same reason `isDirty()` is derived rather than
  // tallied: a push from every write path is a list of write paths, and the
  // one that gets left out is always the one added last — which shows up as a
  // window that claims to be saved while it is not. Nothing crosses the bridge
  // and nothing is written to the DOM unless the answer changed, so the steady
  // state costs one comparison.
  //
  // `textContent`, never markup: a file name is text somebody else chose, and
  // this one arrives from a file picker.
  {
    const nameEl = byId("doc-name");
    const stateEl = byId("doc-state");
    let lastName, lastDirty, first = true;
    const publish = () => {
      const name = documentName();
      const dirty = isDirty();
      if (!first && name === lastName && dirty === lastDirty) return;
      first = false;
      lastName = name;
      lastDirty = dirty;
      // "Untitled workbook" rather than a blank: a strip whose whole job is to
      // say which document you have open must say something when the answer is
      // "one you started here", which is what File ▸ New calls it too.
      if (nameEl) nameEl.textContent = name || "Untitled workbook";
      // A browser tab's vocabulary. `isDirty()` here means "changes that have
      // not been written out", which is the sentence the New and Open
      // confirmations already use — there is no file on disk to be behind.
      if (stateEl) stateEl.textContent = dirty ? "Unsaved changes" : "Saved";
      const native = window.__opencalcNative;
      if (native) native.setDocument(name, dirty).catch(() => {});
    };
    publish();
    setInterval(publish, 250);

    // --- Renaming, from the strip (`UX-CHR-10`) ---------------------------
    //
    // The button is swapped for an input rather than the input being always
    // present and styled to look like text: an always-live text field in the
    // title area is something a stray keystroke edits, and this is the one
    // string that reaches a native window title.
    const renameEl = byId("doc-rename");
    if (nameEl && renameEl) {
      const stop = (commit) => {
        if (renameEl.hidden) return;
        // Read before hiding: hiding first drops the value on some engines.
        const typed = renameEl.value;
        renameEl.hidden = true;
        nameEl.hidden = false;
        if (commit) setDocumentName(typed);
        // `first` forces the next poll to write even if the name did not
        // change, so an abandoned rename cannot leave a stale strip.
        first = true;
        publish();
        canvas?.focus();
      };
      nameEl.addEventListener("click", () => {
        // **Seeded with what the strip is showing, not with `documentName()`.**
        // That returns `null` until a file has been opened, so on a new
        // workbook clicking the name replaced "Untitled workbook" with an empty
        // box — the label vanished and nothing appeared to take its place,
        // which reads as a control that does not work rather than as a rename
        // waiting for input. Reported exactly that way.
        renameEl.value = documentName() || nameEl.textContent.trim();
        // **Sized to the label it replaces**, with room to type a little more.
        // A fixed width made the strip jump the moment the field appeared —
        // the name moved sideways under the pointer that had just clicked it,
        // which is the opposite of what a rename should feel like. Bounded so a
        // very long name cannot push the save state off the strip.
        const w = Math.min(420, Math.max(160, nameEl.offsetWidth + 28));
        renameEl.style.width = `${w}px`;
        renameEl.hidden = false;
        nameEl.hidden = true;
        renameEl.focus();
        renameEl.select();
      });
      renameEl.addEventListener("keydown", (e) => {
        if (e.key === "Enter") { e.preventDefault(); stop(true); }
        else if (e.key === "Escape") { e.preventDefault(); stop(false); }
        // The grid's own key handling must not see a keystroke meant for a name
        // — `t` would otherwise start a cell edit behind the rename.
        e.stopPropagation();
      });
      // Clicking away commits, which is what every competitor's title field
      // does and what a user who has typed a name and moved on expects.
      renameEl.addEventListener("blur", () => stop(true));
    }
  }

  // The desktop shell replaces the webview's file picker with the platform's.
  //
  // Capture phase on `#tb-open`, because every route to File ▸ Open ends in a
  // click on it — the menu item, the header button at :7070, the toolbar. One
  // listener here covers all of them, and a route added later is covered too.
  // In a browser tab `__opencalcNative` is absent and the input *is* the
  // picker, so this does nothing.
  byId("tb-open").addEventListener("click", (e) => {
    const native = window.__opencalcNative;
    if (!native) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    // The same gate the `change` handler applies, for the same reason: this is
    // the point where a file becomes the document, and a mode where the host
    // owns the file has to be refused here or hiding the button is decoration.
    if (capabilityForbids("file.open")) {
      refuse("file.open", "opening a file is the host application's — this editor does not own the document");
      return;
    }
    (async () => {
      const picked = await native.open();
      if (!picked) return; // cancelled: the document is untouched
      if (isDirty() && !(await confirmModal(
        "Open another workbook?",
        "This workbook has changes that have not been saved. Opening another discards them, and undo will not bring them back.",
        "Discard and open",
      ))) {
        return;
      }
      status.textContent = `opening ${picked.name}…`;
      await new Promise((r) => setTimeout(r, 0));
      if (openBytes(picked.bytes, picked.name)) {
        markSaved();
        native.setDocument(picked.name, false);
      }
    })().catch((err) => statusError(errText(err)));
  }, true);

  byId("tb-open").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    // The gate, not just the button. Hiding `File ▸ Open` and the header button
    // takes the command out of the *chrome*; this is the one place a file
    // actually becomes the document, and a mode where the host owns the file
    // has to be refused here or the hiding is decoration. It matters because
    // this is a `<input type=file>` sitting in the host's own page: a stale
    // reference, an extension, or the host's own code reaching in could still
    // click it, and the result would be the host's document replaced.
    if (capabilityForbids("file.open")) {
      e.target.value = "";
      refuse("file.open", "opening a file is the host application's — this editor does not own the document");
      return false;
    }
    // A large file takes a moment to parse; say so rather than appearing to
    // have ignored the click.
    status.textContent = `opening ${file.name}…`;
    // A macrotask, not `requestAnimationFrame`: rAF does not fire in a
    // backgrounded tab, so waiting on it here hung the open entirely whenever
    // the window was not in front.
    await new Promise((r) => setTimeout(r, 0));
    if (isDirty() && !(await confirmModal(
      "Open another workbook?",
      "This workbook has changes that have not been downloaded. Opening another discards them, and undo will not bring them back.",
      "Discard and open",
    ))) {
      e.target.value = "";
      status.textContent = "";
      return false;
    }
    const ok = openBytes(new Uint8Array(await file.arrayBuffer()), file.name);
    // Whatever was just opened is the baseline, not whatever preceded it.
    if (ok) markSaved();
    e.target.value = ""; // allow re-opening the same file
    return ok;
  });
  byId("fx-insert").addEventListener("click", (e) => {
    e.stopPropagation();
    insertFunctionDialog();
  });
  byId("fx-expand").addEventListener("click", (e) => {
    e.stopPropagation();
    // A long formula is unreadable in a one-line box; expanding gives it room
    // without opening a dialog that would lose the caret. Shared with
    // Ctrl+Shift+U, which is the same command from the keyboard.
    toggleFormulaBarExpanded();
  });
  byId("name-box-list").addEventListener("click", (e) => {
    e.stopPropagation();
    openNameBoxList();
  });
  byId("tb-undo").addEventListener("click", doUndo);
  byId("tb-redo").addEventListener("click", doRedo);

  // --- Progressive toolbar collapse (Excel-ribbon style) ---
  // Each group tagged data-collapse=<priority> collapses in-place into its
  // "Label ▾" button — whose flyout holds the group's live tools — whenever the
  // single toolbar row would overflow, lowest priority first. Groups re-expand
  // as the window widens. Never a scrollbar, never a second row.
  const toolbarEl = qs(".toolbar");
  const collapsibles = [...toolbarEl.querySelectorAll(".tb-group[data-collapse]")]
    .map((groupEl) => ({
      groupEl,
      btn: toolbarEl.querySelector(`.tb-collapsed[data-for="${groupEl.id}"]`),
      flyout: toolbarEl.querySelector(`.tb-flyout[data-flyout="${groupEl.id}"]`),
      prio: +groupEl.dataset.collapse,
    }))
    .sort((a, b) => a.prio - b.prio); // lowest priority number collapses first
  const flyouts = collapsibles.map((c) => c.flyout);
  const closeFlyouts = () => {
    for (const f of flyouts) f.hidden = true;
    const mf = byId("tb-more-flyout");
    if (mf) mf.hidden = true;
  };
  const expandGroup = (c) => {
    while (c.flyout.firstChild) c.groupEl.appendChild(c.flyout.firstChild);
    c.groupEl.hidden = false; c.btn.hidden = true; c.flyout.hidden = true;
  };
  const collapseGroup = (c) => {
    while (c.groupEl.firstChild) c.flyout.appendChild(c.groupEl.firstChild);
    c.groupEl.hidden = true; c.btn.hidden = false;
  };
  const fits = () => toolbarEl.scrollWidth <= toolbarEl.clientWidth + 1;
  // The last stage. Collapsing every group still left the bar 791px wide, so
  // any viewport under that clipped it with nothing left to fold — 401px of
  // toolbar off-screen on a 390px phone, and 23px on an iPad Mini. Whatever
  // does not fit after the groups have folded moves in here, from the trailing
  // edge inwards, so the controls nearest the left survive longest.
  const moreBtn = byId("tb-more");
  const moreFlyout = byId("tb-more-flyout");
  // Undo/redo never move: if exactly one group survives a 320px screen it has
  // to be the one that takes back a mistake.
  const KEEP = "tb-keep";
  const restoreFromMore = () => {
    while (moreFlyout.firstChild) toolbarEl.insertBefore(moreFlyout.firstChild, moreBtn);
    moreBtn.hidden = true;
    moreFlyout.hidden = true;
  };
  function overflowToMore() {
    moreBtn.hidden = false;
    // Guarded rather than `while (!fits())`: a layout that can never fit would
    // otherwise spin forever, and a toolbar is not worth hanging the tab for.
    for (let guard = 0; !fits() && guard < 60; guard += 1) {
      const movable = [...toolbarEl.children].filter(
        (el) => el !== moreBtn && el !== moreFlyout && !el.classList.contains(KEEP)
          && !el.classList.contains("tb-flyout"),
      );
      if (!movable.length) break;
      // Taken from the end and inserted at the front of the flyout, so the
      // original left-to-right order is preserved both in here and on the way
      // back out.
      moreFlyout.insertBefore(movable[movable.length - 1], moreFlyout.firstChild);
    }
  }
  function reflowToolbar() {
    restoreFromMore();
    for (const c of collapsibles) expandGroup(c); // reset to fully expanded
    for (const c of collapsibles) { if (fits()) break; collapseGroup(c); }
    if (!fits()) overflowToMore();
  }
  moreBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    const open = moreFlyout.hidden;
    for (const m of menus) m.hidden = true;
    closeFlyouts();
    moreFlyout.hidden = !open;
    moreBtn.setAttribute("aria-expanded", open ? "true" : "false");
    if (open) anchorMenu(moreFlyout, moreBtn);
  });
  for (const c of collapsibles) {
    c.btn.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = c.flyout.hidden;
      for (const m of menus) m.hidden = true;
      closeFlyouts();
      c.flyout.hidden = !open;
      if (!c.flyout.hidden) anchorMenu(c.flyout, c.btn);
    });
  }
  // A click *inside* a flyout must not dismiss it — the flyout holds the live
  // controls of the collapsed group (the font box, color swatches, …), and
  // clicking into one would otherwise close the panel out from under it.
  document.addEventListener("click", (e) => { if (!e.target.closest(".tb-flyout")) closeFlyouts(); });
  reflowToolbar();

  // --- Toolbar as one composite control (roving tabindex) ------------------
  // Every button and field in the toolbar was its own tab stop, so reaching the
  // grid by keyboard meant ~40 presses of Tab. A toolbar is one stop: Tab moves
  // past it, arrows move within it. Re-scanned on each interaction because the
  // collapse machinery moves controls in and out of flyouts.
  {
    const toolbar = qs(".toolbar");
    const items = () => [...toolbar.querySelectorAll("button, input, select")]
      .filter((el) => !el.disabled && el.offsetParent !== null);
    const rove = (list, at) => {
      for (const el of list) el.tabIndex = -1;
      const n = list.length;
      const target = list[((at % n) + n) % n];
      target.tabIndex = 0;
      target.focus();
    };
    const syncStops = () => {
      // Reset *every* control, including those parked in a collapsed group's
      // flyout: they are invisible now, but they keep their tabindex and would
      // become extra tab stops the moment the flyout opened.
      for (const el of toolbar.querySelectorAll("button, input, select")) el.tabIndex = -1;
      const list = items();
      if (!list.length) return;
      const focused = list.findIndex((el) => el === activeEl());
      list[Math.max(0, focused)].tabIndex = 0;
    };
    syncStops();
    toolbar.addEventListener("focusin", syncStops);
    toolbar.addEventListener("keydown", (e) => {
      // A text field owns its own arrow keys (the font and size boxes), so only
      // step between controls when the caret is not in one.
      const inField = activeEl() && activeEl().tagName === "INPUT";
      if (inField && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;
      const list = items();
      const at = list.indexOf(activeEl());
      if (at < 0) return;
      if (e.key === "ArrowRight" || e.key === "ArrowDown") { rove(list, at + 1); e.preventDefault(); }
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp") { rove(list, at - 1); e.preventDefault(); }
      else if (e.key === "Home") { rove(list, 0); e.preventDefault(); }
      else if (e.key === "End") { rove(list, list.length - 1); e.preventDefault(); }
    });
    // Re-establish a single stop after the toolbar reflows into/out of flyouts.
    window.addEventListener("resize", syncStops);
  }

  // Track whether the user is driving by keyboard, so the grid's focus ring
  // appears for them and not on every mouse click (see the CSS note).
  window.addEventListener("keydown", (e) => {
    if (e.key === "Tab" || e.key.startsWith("Arrow")) ocThemeHost.dataset.kbnav = "1";
  }, true);
  window.addEventListener("mousedown", () => { delete ocThemeHost.dataset.kbnav; }, true);

  buildMenuBar();

  // The page-header collapse toggle, kept right-most: buildMenuBar() appends
  // File…Help, so re-append it afterwards rather than relying on markup order.
  {
    const btn = byId("hdr-collapse");
    const bar = byId("menubar");
    if (btn && bar) {
      bar.appendChild(btn);
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        setHeaderCollapsed(!headerCollapsed);
      });
      let saved = false;
      try { saved = localStorage.getItem(HEADER_COLLAPSE_KEY) === "1"; } catch {}
      if (saved) setHeaderCollapsed(true);
    }
  }

  window.addEventListener("resize", () => { invalidateTrackSize(); resize(); reflowToolbar(); });
  // A software keyboard fires none of the above. It shrinks the *visual*
  // viewport and leaves `window.innerHeight` — and therefore the whole layout —
  // exactly as it was, so `resize` above never runs and the grid goes on
  // believing it owns the part of the screen the keyboard is now covering.
  // `visualViewport` is the only event that says so; nothing in the webapp
  // listened to it. `scroll` is deliberately *not* wired: on a zoomed page the
  // user pans the visual viewport themselves, and scrolling the sheet out from
  // under them each time they did would be worse than the problem.
  window.visualViewport?.addEventListener("resize", keepEditVisible);

  // ---- Application menu bar (File / Edit / View / …) -----------------------
  // Declarative menu → item table. Every item delegates to an existing handler
  // or toolbar control, so there are no parallel implementations to drift.
  function buildMenuBar() {
    const bar = byId("menubar");
    if (!bar) return;
    const q = (sel) => qs(sel);
    const clickEl = (sel) => () => { const n = q(sel); if (n) n.click(); };
    const rng = () => effectiveRange();
    const fmtHas = (k) => {
      try { return !!JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col))[k]; }
      catch { return false; }
    };
    const gridOn = () => { try { return !wasm.session_gridlines_hidden(state.sheet); } catch { return true; } };
    const viewOn = (k) => { try { return !!viewOptions()[k]; } catch { return false; } };
    const cellProt = (k) => {
      try {
        return !!JSON.parse(
          wasm.session_cell_protection(state.sheet, state.sel.row, state.sel.col))[k];
      } catch { return false; }
    };
    // The active cell's number format and alignment, for the submenu ticks — a
    // menu that never shows what is already applied makes you guess.
    const curFmt = (key) => {
      try { return JSON.parse(wasm.session_cell_format(state.sheet, state.sel.row, state.sel.col))[key] || ""; }
      catch { return ""; }
    };
    const nfIs = (code) => () => curFmt("nf") === code;
    const alIs = (token) => () => curFmt("al") === token;
    const vaIs = (token) => () => curFmt("va") === token;
    const vtIs = (token) => () => curFmt("vt") === token;
    const headersOn = () => { try { return !wasm.session_headers_hidden(state.sheet); } catch { return true; } };
    // `clearSelection` rather than a second copy of it: the copy had its own
    // swallowing catch, so the same command reported a refusal from the Delete
    // key and said nothing from the menu.
    const clearContents = () => clearSelection();
    const nf = (code) => () => setNumberFormat(code);

    const showModal = (title, html) => {
      byId("oc-modal-title").textContent = title;
      // oc-safe-html: both callers pass literal markup built in this file
      // (the shortcut table and the About text). It must stay that way —
      // this helper is not for anything a document can influence.
      // oc-safe-html: see the note above.
      byId("oc-modal-body").innerHTML = html;
      byId("oc-modal").hidden = false;
    };
    // Every row here is a promise, and each one has been pressed. The list said
    // `F3` for the name manager for as long as it has existed and no keystroke
    // has ever answered — the chord is `Ctrl+F3` (`docs/12` §9.6). A help page
    // that names a chord doing nothing is worse than one that omits it: the
    // user does not conclude the shortcut is missing, they conclude the editor
    // is broken, and they are half right.
    function showShortcuts() {
      const rows = [
        // **The file chords come first, and `Ctrl+S` was missing entirely**
        // (`TAURI-012`). This dialog is the only in-app shortcut reference, and
        // the one chord nobody opens a menu for was the one it did not list.
        ["New / Open / Save", "Ctrl+N / Ctrl+O / Ctrl+S"],
        ["Undo / Redo", "Ctrl+Z / Ctrl+Shift+Z"],
        ["Cut / Copy / Paste", "Ctrl+X / Ctrl+C / Ctrl+V"],
        ["Bold / Italic / Underline", "Ctrl+B / Ctrl+I / Ctrl+U"],
        ["Find / Replace", "Ctrl+F / Ctrl+H"],
        ["Select all", "Ctrl+A"],
        ["Insert / delete line", "Ctrl++ / Ctrl+−"],
        ["Edit cell", "F2 / Enter"],
        ["Go to (name box)", "Ctrl+G / F5"],
        ["Insert date / time", "Ctrl+; / Ctrl+Shift+;"],
        ["Format cells", "Ctrl+1 / Ctrl+Shift+F"],
        ["Expand formula bar", "Ctrl+Shift+U"],
        ["Select cells with notes", "Ctrl+Shift+O"],
        ["Pick from this column", "Alt+Down"],
        ["Name manager", "Ctrl+F3"],
      ];
      showModal("Keyboard shortcuts", rows.map(([a, b]) =>
        `<div class="kb-row"><span>${a}</span><span>${b.replace(/(\S+)/g, "<kbd>$1</kbd>").replace(/<kbd>\/<\/kbd>/g, "/")}</span></div>`).join(""));
    }
    /// **Where the version lives** (`UX-CHR-03`).
    ///
    /// It used to be a badge in the branding strip — `Alpha` and
    /// `engine v0.0.0`, development state in the chrome of every session, which
    /// not one of Excel, LibreOffice, OnlyOffice, Sheets or Numbers does. All
    /// five put it behind Help ▸ About, and so does this.
    ///
    /// Read from `wasm.version()` rather than written out here. The literal
    /// this replaces said `v0.0.0` and was right by coincidence: two statements
    /// of one version drift, and the one nobody looks at is the one in the
    /// dialog nobody opens.
    function showAbout() {
      let engine = "";
      try { engine = wasm ? String(wasm.version()) : ""; } catch {}
      showModal(`About ${BRAND}`,
        `<p>${htmlText(BRAND)} — a deterministic, embeddable spreadsheet engine for <code>.xlsx</code>, CSV, TSV and PSV.</p>
         <p style="margin-top:10px;color:var(--oc-muted-text-color)">Engine <b>v${htmlText(engine)}</b> · Alpha · <a href="./index.html">Home</a></p>`);
    }

    const MENUS = [
      ["File", [
        ["New", async () => {
          // The most destructive verb in the application, and the only one that
          // did not ask. `session_new` replaces the session outright and `seed`
          // then clears the history, so Ctrl+Z recovers nothing — while merge,
          // merge-across, delimited export and a lossy save all confirm first.
          if (isDirty() && !(await confirmModal(
            "Start a new workbook?",
            "This workbook has changes that have not been downloaded. Starting a new one discards them, and undo will not bring them back.",
            "Discard and start new",
          ))) return;
          // `newDocument()` clears the opened name **and the save target**.
          // Without it `Ctrl+S` on a brand-new workbook writes over the file
          // that was open before it — the document on screen saved into
          // somebody else's file, which is the worst shape a save bug takes.
          // The desktop shell also drops the target when it sees `file.new`
          // go past, but that is a second definition of one fact and this is
          // the one that holds in a browser tab too.
          stopMarch(); wasm.session_new(); newDocument(); imageCache.clear(); state.sheet = 0; seed(); renderTabs();
          // A new workbook is a different document and must not inherit
          // the previous one's versions (`HIST-03`). Forgetting is done
          // *after* `newDocument()` clears the name, so it removes the
          // bucket the new document will use rather than the old one's —
          // which would have deleted the history of the file just closed.
          forgetVersions().catch(() => {});
        }],
        ["Open…", clickEl("#tb-open")],
        // **The save nobody opens a menu for, finally in the menu**
        // (`TAURI-009`). `Ctrl+S` has committed the document to its own file
        // since `SAVE-02`, and it was reachable by that chord alone: a desktop
        // application whose File menu offers no Save is one a user assumes
        // cannot save.
        //
        // Native chrome only, and `CHROME_ONLY` is the gate. In a browser tab
        // this same call writes a copy into the downloads folder — there is no
        // file to commit to — and the Download submenu below already says that
        // in the right words. Two entries doing one thing under two names is
        // the confusion this row is about, pointing the other way.
        ["Save", () => saveAs("native"), "Ctrl+S"],
        // Built from `writable_extensions()` rather than written out here, the
        // same way Open is built from `openable_extensions()`. A format the
        // engine learns then appears without anyone remembering — `.ods` and
        // `.xlsm` were reachable by the engine and absent from this menu for
        // exactly as long as this list was a list.
        //
        // `"download"` is the intent: these write a copy. `Ctrl+S` saves in
        // place, and the entry that gives back the kind of file that was
        // opened must not quietly become that.
        //
        // "Download" is the *web* wording and the id is derived from it; in
        // desktop chrome the same node reads "Export" and its first entry reads
        // "Save a copy…" (`NATIVE_LABELS`, `TAURI-009`). The id stays
        // `file.download.*` in both, because that is what the native menu, the
        // host command rules and `canSaveAs` all dispatch on.
        { sub: "Download", items: downloadItems() },
        // The only route into the collaboration server that is not a host
        // writing JavaScript. Hidden while `COL-46` is open — `canShare` is
        // `false` in every mode preset — so this line is present and
        // unreachable by design rather than by omission. §Share in
        // `editor.presence.js` has the reasoning; `CAPABILITY_COMMANDS` has the
        // gate.
        //
        // Inside the New/Open/Download group rather than after the separator,
        // so that hiding it leaves the File menu byte-for-byte the one that is
        // there today. A hidden item between two separators would draw two
        // rules against nothing — `menuModel()` collapses those for the native
        // menu, and nothing collapses them for the HTML one.
        ["Share…", () => shareDialog()],
        "sep",
        ["Page setup…", () => openPanel("page")],
        // Excel inserts both a row and a column break at the active cell, and
        // only the one that applies for a whole-row/column selection.
        ["Page break here", () => {
          const r = effectiveRange();
          const rowAt = state.selKind === "cols" ? 0xffffffff : r.r0;
          const colAt = state.selKind === "rows" ? 0xffffffff : r.c0;
          tryEdit(() => wasm.session_toggle_page_break(state.sheet, rowAt, colAt));
        }],
        ["Print…", () => printSheet(), "Ctrl+P"],
        "sep",
        // Where every spreadsheet puts it, and where somebody looking for "who
        // wrote this" will look first (`UX-META-01`).
        ["Properties…", () => documentPropertiesDialog()],
        // `HIST-01`. In File rather than Tools, because it is a fact about
        // *this document* — which is where Sheets, OnlyOffice and Excel put it.
        ["Version history…", () => openPanel("history")],
      ]],
      ["Edit", [
        ["Undo", doUndo, "Ctrl+Z"],
        ["Redo", doRedo, "Ctrl+Shift+Z"],
        "sep",
        ["Cut", doCut, "Ctrl+X"],
        ["Copy", doCopy, "Ctrl+C"],
        ["Paste", doPaste, "Ctrl+V"],
        "sep",
        ["Find & replace…", openFind, "Ctrl+F"],
        ["Select all", selectAll, "Ctrl+A"],
        "sep",
        { sub: "Clear", items: [
          ["Values", clearContents],
          ["Formatting", clearFormats],
          ["All", clearAll],
        ] },
        // Fill from the top/left of the selection into the rest of it. The fill
        // handle can do the same, but only by dragging — and a mode you can only
        // reach by dragging is one most people never find.
        { sub: "Fill", items: [
          ["Fill down", () => fillWithin("down"), "Ctrl+D"],
          ["Fill right", () => fillWithin("right"), "Ctrl+R"],
          "sep",
          ["Fill series", () => fillSelection("series")],
          ["Growth series", () => fillSelection("growth")],
          ["Copy cells", () => fillSelection("copy")],
          ["Formatting only", () => fillSelection("formats")],
          ["Without formatting", () => fillSelection("values")],
        ] },
      ]],
      ["View", [
        { sub: "Freeze", items: [
          ["Up to selection", clickEl('#freeze-menu [data-fz="sel"]')],
          ["Top row", clickEl('#freeze-menu [data-fz="row"]')],
          ["First column", clickEl('#freeze-menu [data-fz="col"]')],
          ["Unfreeze", clickEl('#freeze-menu [data-fz="none"]')],
        ] },
        // Both toggles report a refusal. Swallowing it left the tick unmoved
        // with no reason given, which reads as a menu item that is broken.
        ["Gridlines", () => {
          try { wasm.session_set_gridlines_hidden(state.sheet, gridOn()); }
          catch (err) { statusError(errText(err)); }
          draw();
        }, null, gridOn],
        // "Cell markings" = the A/B/C and 1/2/3 strips. Deliberately not called
        // "headers": that word belongs to the page header this menu bar can
        // collapse, and having both under one name is a coin-flip every time.
        ["Cell markings", () => {
          try { wasm.session_set_headers_hidden(state.sheet, headersOn()); }
          catch (err) { statusError(errText(err)); }
          resize();
        }, null, headersOn],
        // Both are per-sheet OOXML view flags that were being carried through
        // every save without ever being shown.
        ["Formulas instead of results", () => setViewOption("formulas"), "Ctrl+`", () => viewOn("formulas")],
        ["Zero values", () => setViewOption("zeros"), null, () => viewOn("zeros")],
        // **Theme is a display option, so it lives with the display options**
        // (`UX-CHR-01`). It was a `<select>` inside the settings gear popover:
        // two clicks, in the branding strip, and a different mental model from
        // the four siblings directly above this line. Excel and Sheets both put
        // the appearance toggle in a View menu; neither hides it behind a gear
        // in the title area.
        //
        // Ticked from `currentTheme()` — the *choice* — not from the rendered
        // theme. `document.documentElement.dataset.theme` is absent for "Auto"
        // and for a light system alike, so reading it back would tick Light in
        // a window the user had set to Auto.
        { sub: "Theme", items: [
          ["Auto", () => applyTheme("auto"), null, () => currentTheme() === "auto"],
          ["Light", () => applyTheme("light"), null, () => currentTheme() === "light"],
          ["Dark", () => applyTheme("dark"), null, () => currentTheme() === "dark"],
        ] },
        { sub: "Zoom", items: [
          ["50%", () => setZoom(0.5), null, () => state.zoom === 0.5],
          ["75%", () => setZoom(0.75), null, () => state.zoom === 0.75],
          // `Ctrl+Alt+0`, not `Ctrl+0`. Ctrl+0 is Excel's hide-column and this
          // editor binds it that way; the label used to say Ctrl+0 and so sent
          // users to a chord that hides their columns. See the keydown handler.
          ["100%", () => setZoom(1), "Ctrl+Alt+0", () => state.zoom === 1],
          ["150%", () => setZoom(1.5), null, () => state.zoom === 1.5],
          ["200%", () => setZoom(2), null, () => state.zoom === 2],
        ] },
        "sep",
        // Not `clickEl("#tb-settings")`. That reached the panel through a
        // button inside the app header, so it needed the header un-collapsed
        // first — and in desktop chrome there is no header to un-collapse
        // (`UX-DESK-01`). `openSettings()` is the one entry point and picks
        // its own presentation.
        ["Settings…", () => openSettings()],
      ]],
      ["Insert", [
        ["Rows above", () => tryEdit(() => { const r = rng(); wasm.session_insert_rows(state.sheet, r.r0, r.r1 - r.r0 + 1); })],
        ["Rows below", () => tryEdit(() => { const r = rng(); wasm.session_insert_rows(state.sheet, r.r1 + 1, r.r1 - r.r0 + 1); })],
        ["Columns left", () => tryEdit(() => { const r = rng(); wasm.session_insert_columns(state.sheet, r.c0, r.c1 - r.c0 + 1); })],
        ["Columns right", () => tryEdit(() => { const r = rng(); wasm.session_insert_columns(state.sheet, r.c1 + 1, r.c1 - r.c0 + 1); })],
        "sep",
        ["Delete rows", () => tryEdit(() => { const r = rng(); wasm.session_delete_rows(state.sheet, r.r0, r.r1 - r.r0 + 1); })],
        ["Delete columns", () => tryEdit(() => { const r = rng(); wasm.session_delete_columns(state.sheet, r.c0, r.c1 - r.c0 + 1); })],
        "sep",
        ["Note", () => togglePanel("note")],
        "sep",
        // The context menu and Ctrl+T both reach this, but neither is somewhere
        // you look for a feature you have not met yet.
        ["Table…", () => tableDialog(), "Ctrl+T"],
        ["PivotTable…", () => pivotDialog()],
        { sub: "Chart", items: CHART_KINDS.map(([kind, label]) => [label, () => chartDialog(kind)]) },
        ["Hyperlink…", () => hyperlinkDialog(), "Ctrl+K"],
      ]],
      ["Format", [
        ["Bold", clickEl("#tb-bold"), "Ctrl+B", () => fmtHas("b")],
        ["Italic", clickEl("#tb-italic"), "Ctrl+I", () => fmtHas("i")],
        ["Underline", clickEl("#tb-underline"), "Ctrl+U", () => fmtHas("u")],
        ["Strikethrough", clickEl("#tb-strike"), null, () => fmtHas("st")],
        // Both are one `vertAlign` on the font, so they are mutually exclusive:
        // choosing one replaces the other rather than stacking.
        ["Superscript", () => setVertAlign("superscript"), null, vtIs("superscript")],
        ["Subscript", () => setVertAlign("subscript"), null, vtIs("subscript")],
        "sep",
        { sub: "Protection", items: [
          // Both only bite while the sheet is protected, so the sheet switch
          // sits in the same menu rather than somewhere else entirely.
          ["Locked", () => setCellProtection("locked"), null, () => cellProt("locked")],
          ["Hide formula", () => setCellProtection("hidden"), null, () => cellProt("hidden")],
          "sep",
          ["Protect this sheet", () => toggleSheetProtected(), null, sheetProtectedNow],
        ] },
        ["Cell styles…", () => cellStyleGallery()],
        ["Conditional formatting rules…", () => manageCfRules()],
        { sub: "Trace", items: [
          ["Trace precedents", () => toggleTrace("prec")],
          ["Trace dependents", () => toggleTrace("dep")],
          ["Clear trace arrows", () => clearTrace()],
        ] },
        { sub: "Alignment", items: [
          ["Left", () => setAlign("left"), null, alIs("left")],
          ["Center", () => setAlign("center"), null, alIs("center")],
          ["Right", () => setAlign("right"), null, alIs("right")],
          // The OOXML modes that are more than an edge. `centerContinuous` is
          // Excel's "Center Across Selection" — it looks merged but merges
          // nothing, so the cells underneath stay addressable.
          ["Fill (repeat text)", () => setAlign("fill")],
          ["Justify", () => setAlign("justify")],
          ["Center across selection", () => setAlign("centerContinuous")],
          ["Distributed", () => setAlign("distributed")],
          ["Clear (General)", () => setAlign("")],
          "sep",
          ["Top", () => setValign("top"), null, vaIs("t")],
          ["Middle", () => setValign("middle"), null, vaIs("m")],
          ["Bottom", () => setValign("bottom"), null, vaIs("b")],
          ["Justify (vertical)", () => setValign("justify")],
          ["Distributed (vertical)", () => setValign("distributed")],
        ] },
        { sub: "Text overflow", items: [
          ["Overflow", () => setTextOverflow("overflow"), null, () => !fmtHas("w") && !fmtHas("cl")],
          ["Wrap", () => setTextOverflow("wrap"), null, () => fmtHas("w")],
          ["Clip", () => setTextOverflow("clip"), null, () => fmtHas("cl")],
        ] },
        ["Merge cells", clickEl("#tb-merge")],
        "sep",
        { sub: "Number", items: [
          ["Automatic", nf(""), null, nfIs("")],
          ["Number (0.00)", nf("0.00"), null, nfIs("0.00")],
          ["Thousands (#,##0)", nf("#,##0"), null, nfIs("#,##0")],
          ["Percent (0%)", nf("0%"), null, nfIs("0%")],
          ["Currency", nf("$#,##0.00"), null, nfIs("$#,##0.00")],
          ["Short date", nf("yyyy-mm-dd"), null, nfIs("yyyy-mm-dd")],
          ["Time", nf("h:mm:ss AM/PM"), null, nfIs("h:mm:ss AM/PM")],
          ["Scientific", nf("0.00E+00"), null, nfIs("0.00E+00")],
          ["Text", nf("@"), null, nfIs("@")],
        ] },
        ["Custom number format…", () => customFormatDialog()],
        ["Conditional formatting…", () => togglePanel("cf")],
        "sep",
        ["Clear formatting", clearFormats],
      ]],
      ["Data", [
        { sub: "Sort range", items: [
          ["A → Z", clickEl('#sort-menu [data-sort="asc"]')],
          ["Z → A", clickEl('#sort-menu [data-sort="desc"]')],
          ["Custom sort…", () => sortDialog()],
        ] },
        ["Remove duplicates…", () => removeDuplicates()],
        ["Text to columns…", () => textToColumnsDialog()],
        // `DATA-NT-01`. Beside the other repairs an imported sheet needs,
        // because that is where the problem comes from: a CSV or a paste whose
        // numbers arrived quoted.
        ["Convert text to numbers", () => convertTextToNumbers()],
        ["Filter", () => toggleFilter()],
        ["Clear all filters", () => { if (!filterInfo) { status.textContent = "no filter"; return; } tryEdit(() => wasm.session_clear_filter_rules(state.sheet)); afterFilterChange(); }],
        ["Clear my view", () => clearMyView()],
        ["Column stats…", () => openPanel("stats")],
        ["Data validation…", () => togglePanel("dv")],
        "sep",
        ["PivotTable fields…", () => pivotDialog()],
        ["Refresh pivot", () => refreshPivotHere(), "Alt+F5"],
        ["Refresh all pivots", () => refreshAllPivots(), "Ctrl+Alt+F5"],
        "sep",
        ["Hide rows", () => tryEdit(() => { const r = rng(); wasm.session_hide_rows(state.sheet, r.r0, r.r1); })],
        ["Hide columns", () => tryEdit(() => { const r = rng(); wasm.session_hide_cols(state.sheet, r.c0, r.c1); })],
        { sub: "Group", items: [
          ["Group rows", () => tryEdit(() => { const r = effectiveRange(); wasm.session_group(state.sheet, r.r0, r.r1, false); })],
          ["Group columns", () => tryEdit(() => { const r = effectiveRange(); wasm.session_group(state.sheet, r.c0, r.c1, true); })],
          ["Ungroup rows", () => tryEdit(() => { const r = effectiveRange(); wasm.session_ungroup(state.sheet, r.r0, r.r1, false); })],
          ["Ungroup columns", () => tryEdit(() => { const r = effectiveRange(); wasm.session_ungroup(state.sheet, r.c0, r.c1, true); })],
          "sep",
          ["Expand all", () => tryEdit(() => { wasm.session_show_outline_level(state.sheet, 7, false); wasm.session_show_outline_level(state.sheet, 7, true); })],
          ["Collapse all", () => tryEdit(() => { wasm.session_show_outline_level(state.sheet, 0, false); wasm.session_show_outline_level(state.sheet, 0, true); })],
          ["Show level 1", () => tryEdit(() => wasm.session_show_outline_level(state.sheet, 1, false))],
          ["Show level 2", () => tryEdit(() => wasm.session_show_outline_level(state.sheet, 2, false))],
        ] },
        "sep",
        ["Unhide rows/columns in selection", () => tryEdit(() => { const r = rng(); wasm.session_unhide_rows(state.sheet, r.r0, r.r1); wasm.session_unhide_cols(state.sheet, r.c0, r.c1); })],
        // Unhide-in-selection cannot help when you no longer know where the
        // hidden band is — or when it is outside whatever is selected. This
        // always works.
        ["Unhide all rows and columns", () => { const b = usedBounds(); tryEdit(() => { wasm.session_unhide_rows(state.sheet, 0, Math.max(b.rows, 1) + 1000); wasm.session_unhide_cols(state.sheet, 0, Math.max(b.cols, 1) + 1000); }); status.textContent = "all rows and columns shown"; }],
      ]],
      ["Tools", [
        // Not `clickEl("#tb-settings")`. That reached the panel through a
        // button inside the app header, so it needed the header un-collapsed
        // first — and in desktop chrome there is no header to un-collapse
        // (`UX-DESK-01`). `openSettings()` is the one entry point and picks
        // its own presentation.
        ["Settings…", () => openSettings()],
        ["Name manager…", () => openNameManager(160, 120)],
        "sep",
        // Excel's Formulas ▸ Calculation Options. A workbook saved with
        // calculation off opens that way, so the state has to be visible and
        // changeable rather than assumed.
        { sub: "Calculation", items: [
          ["Automatic", () => setCalculationMode("auto"), null, () => calcMode() === "auto"],
          ["Manual", () => setCalculationMode("manual"), null, () => calcMode() === "manual"],
          "sep",
          ["Calculate now", () => recalculateNow(), "F9"],
        ] },
      ]],
      ["Help", [
        ["Keyboard shortcuts", showShortcuts],
        [`About ${BRAND}`, showAbout],
      ]],
    ];

    const drops = [];
    const subs = [];
    const topBtns = [];
    let openIdx = -1;
    const closeSubs = () => { for (const s of subs) s.hidden = true; };
    const closeMenus = () => {
      for (const d of drops) d.hidden = true;
      closeSubs();
      for (const b of topBtns) b.setAttribute("aria-expanded", "false");
      openIdx = -1;
    };
    // Place a submenu beside the row that owns it, flipping and clamping to stay
    // on screen — and **measure it while it is shown**.
    //
    // It used to measure `sub` while the caller still had it `hidden`, and
    // `[hidden]` is `display: none`, so `offsetWidth` and `offsetHeight` were
    // both 0. Every test below then compared an edge against a panel of no
    // size: `left + 0 > innerWidth - 4` cannot be true for a trigger that is
    // itself on screen, so the flip and the bottom clamp were dead code from
    // the day they were written. On a desktop there is always room to the
    // right and nothing looked wrong.
    //
    // On a 390px phone it put all fourteen submenus off the edge — Format ▸
    // Number opened at x=377.3 with a width of 178, so 165px of it was past
    // the screen and the user saw nothing happen. Unhiding first is the whole
    // fix; no paint can occur between here and the assignment at the end,
    // because it is one synchronous task.
    const positionSub = (sub, btn) => {
      const r = btn.getBoundingClientRect();
      sub.hidden = false;
      sub.style.left = "0px"; sub.style.top = "0px"; sub.style.maxHeight = "";
      const sw = sub.offsetWidth, sh = sub.offsetHeight;
      let left = r.right - 3, top = r.top - 5;
      if (left + sw > window.innerWidth - 4) left = Math.max(4, r.left - sw + 3);
      // Still past the right edge after the flip — a panel wider than the
      // window has nowhere to go, so pin it to the left margin rather than
      // leaving it hanging off whichever side lost.
      if (left + sw > window.innerWidth - 4) left = 4;
      if (top + sh > window.innerHeight - 4) top = Math.max(4, window.innerHeight - 4 - sh);
      // Taller than the screen: clamp and let `.popmenu`'s own `overflow-y`
      // carry the rest, which is what `anchorMenu` already does for the
      // top-level drops. Without this a long submenu simply runs off the
      // bottom and the items past the fold cannot be reached at all.
      if (sh > window.innerHeight - 8) { top = 4; sub.style.maxHeight = (window.innerHeight - 8) + "px"; }
      sub.style.left = left + "px"; sub.style.top = top + "px";
    };
    // Tick every item under `root` that has a predicate.
    //
    // Called per menu *and* per submenu: a submenu is appended to the body, not
    // to its parent dropdown, so refreshing only the dropdown left every nested
    // item — Alignment, Number, Zoom, Text overflow, Protection — permanently
    // unticked whatever the cell actually had.
    const refreshChecks = (root) => {
      for (const b of root.querySelectorAll("button")) {
        if (b._check) b.querySelector(".mi-check").textContent = b._check() ? "✓" : "";
      }
    };
    const runItem = (action) => {
      try { action && action(); } catch (err) { statusError(errText(err)); }
      closeMenus();
    };
    const renderItems = (container, items, isTop, path = "") => {
      for (const it of items) {
        if (it === "sep") {
          const s = document.createElement("div"); s.className = "menu-sep"; container.appendChild(s); continue;
        }
        if (it.sub) {
          const b = document.createElement("button");
          // oc-safe-html: empty scaffolding; the label is set with textContent below.
          b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span><span class="mi-caret">&#9656;</span>`;
          b.dataset.ocLabel = it.sub;
          b.querySelector(".mi-label").textContent =
            t(`command.${commandId(path, it.sub)}`, it.sub);
          const sub = document.createElement("div"); sub.className = "menu-sub popmenu"; sub.hidden = true;
          ocOverlayHost.appendChild(sub); subs.push(sub);
          b.dataset.ocCommand = commandId(path, it.sub);
          sub.dataset.ocFor = commandId(path, it.sub);
          renderItems(sub, it.items, false, commandId(path, it.sub));
          // `positionSub` does the unhiding, because it has to measure the
          // panel to know where to put it and a hidden one measures 0×0.
          const openSub = () => { closeSubs(); refreshChecks(sub); positionSub(sub, b); };
          // Hover opens a submenu only where hovering is a thing.
          //
          // A tap does not just produce a `click`: Chrome replays the whole
          // mouse sequence at the touch point, `mouseenter` first. That opened
          // the submenu, and on a narrow screen the submenu has nowhere to go
          // but back over the row that opened it — so the `click` that followed
          // a few hundred microseconds later hit whichever *submenu item* had
          // landed under the finger and ran it. On a phone, tapping "Clear ▸"
          // silently performed one of the clears and closed the menu.
          //
          // Checked per event rather than once at build time so a device that
          // gains or loses a mouse is answered as it is now, not as it was when
          // the menu bar was constructed.
          b.addEventListener("mouseenter", () => {
            if (window.matchMedia("(hover: hover) and (pointer: fine)").matches) openSub();
          });
          b.addEventListener("click", (e) => { e.stopPropagation(); openSub(); });
          container.appendChild(b); continue;
        }
        const [label, action, key, check] = it;
        const b = document.createElement("button");
        // oc-safe-html: scaffolding plus a shortcut label from the static
        // command table; the menu label itself is set with textContent.
        // oc-safe-html: see the note above.
        b.innerHTML = `<span class="mi-check"></span><span class="mi-label"></span>${key ? `<span class="mi-key">${key}</span>` : ""}`;
        b.dataset.ocCommand = commandId(path, label);
        b.dataset.ocLabel = label;
        b.querySelector(".mi-label").textContent = t(`command.${b.dataset.ocCommand}`, label);
        if (check) b._check = check;
        if (isTop) b.addEventListener("mouseenter", closeSubs);
        b.addEventListener("click", (e) => { e.stopPropagation(); runItem(action); });
        container.appendChild(b);
      }
    };

    const openMenu = (i) => {
      closeMenus();
      for (const m of menus) m.hidden = true; // close toolbar popovers
      closeFlyouts();
      const drop = drops[i];
      refreshChecks(drop); // against the focus cell / view state
      drop.hidden = false;
      anchorMenu(drop, topBtns[i]);
      topBtns[i].setAttribute("aria-expanded", "true");
      openIdx = i;
    };

    // Alt mnemonics: the first letter of each top-level menu, which is unique
    // across File/Edit/View/Insert/Format/Data/Tools/Help. Underlined only while
    // Alt is held, as on Windows — a permanently underlined letter reads as a
    // link, and on a Mac Alt is a compose key, so the hint stays out of the way
    // until it is relevant.
    MENUS.forEach(([name, items], i) => {
      const btn = document.createElement("button");
      btn.className = "menu-top";
      btn.dataset.ocLabel = name;
      btn.dataset.ocMenuIndex = String(i);
      btn.setAttribute("role", "menuitem");
      // Roving tabindex: the bar is one tab stop, not nine — Tab moves past it,
      // arrows move within it, which is what a menubar is supposed to do.
      btn.tabIndex = i === 0 ? 0 : -1;
      btn.setAttribute("aria-haspopup", "true"); btn.setAttribute("aria-expanded", "false");
      const drop = document.createElement("div"); drop.className = "menu-drop popmenu"; drop.hidden = true;
      drop.setAttribute("role", "menu");
      ocOverlayHost.appendChild(drop);
      btn.dataset.ocCommand = commandId("", name);
      drop.dataset.ocFor = commandId("", name);
      renderItems(drop, items, true, commandId("", name));
      btn.addEventListener("click", (e) => { e.stopPropagation(); openIdx === i ? closeMenus() : openMenu(i); });
      btn.addEventListener("mouseenter", () => { if (openIdx >= 0 && openIdx !== i) openMenu(i); });
      bar.appendChild(btn); topBtns.push(btn); drops.push(drop);
    });
    // Labels and mnemonics together, because the second is derived from the
    // first: translating a menu changes which letters are free.
    relabelMenubar();

    // --- Menu-bar overflow ---------------------------------------------------
    // The bar had no collapse of any kind, so at 390px it sliced "Help" in half
    // and at 320px it took "Tools" with it. A menu that is half-drawn is not a
    // menu: there is nothing to click and nothing to say it is missing.
    //
    // Trailing menus move into a "⋯" list, which is the same trade the toolbar
    // already makes — one indirection for the rarely-used end of the bar, and
    // no scrollbar. Alt+letter keeps working throughout, because it opens the
    // drop by index and never consults the button's position.
    const moreMenuBtn = byId("menu-more");
    const moreMenuDrop = byId("menu-more-drop");
    if (moreMenuBtn && moreMenuDrop) {
      // Markup order put it first; the menus are appended after it, so it has
      // to be moved to the end or the overflow sits to the *left* of the bar it
      // overflows. And its drop goes where every other drop lives, because the
      // bar clips its own overflow and would otherwise clip the drop too.
      bar.appendChild(moreMenuBtn);
      ocOverlayHost.appendChild(moreMenuDrop);
      const barFits = () => bar.scrollWidth <= bar.clientWidth + 1;
      const openFromOverflow = (i) => {
        moreMenuDrop.hidden = true;
        moreMenuBtn.setAttribute("aria-expanded", "false");
        openMenu(i);
        // Anchored to the "⋯" rather than to a button that is not on the bar,
        // or the drop would be positioned against a hidden element at x=0.
        anchorMenu(drops[i], moreMenuBtn);
      };
      function reflowMenubar() {
        for (const b of topBtns) b.hidden = false;
        moreMenuBtn.hidden = true;
        if (barFits()) return;
        moreMenuBtn.hidden = false;
        for (let i = topBtns.length - 1; i > 0 && !barFits(); i -= 1) topBtns[i].hidden = true;
        moreMenuDrop.textContent = "";
        topBtns.forEach((b, i) => {
          if (!b.hidden) return;
          const item = document.createElement("button");
          item.type = "button";
          item.setAttribute("role", "menuitem");
          item.textContent = b.dataset.ocLabel || b.textContent.trim();
          item.addEventListener("click", (e) => { e.stopPropagation(); openFromOverflow(i); });
          moreMenuDrop.appendChild(item);
        });
      }
      moreMenuBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const open = moreMenuDrop.hidden;
        closeMenus();
        moreMenuDrop.hidden = !open;
        moreMenuBtn.setAttribute("aria-expanded", open ? "true" : "false");
        if (open) anchorMenu(moreMenuDrop, moreMenuBtn);
      });
      document.addEventListener("click", () => { moreMenuDrop.hidden = true; });
      window.addEventListener("resize", reflowMenubar);
      reflowMenubar();
    }

    // Alt+letter opens the matching menu; holding Alt alone reveals which letter
    // each menu answers to.
    document.addEventListener("keydown", (e) => {
      if (e.key === "Alt") { bar.classList.add("show-mnemonics"); return; }
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      const i = menuMnemonics.get((e.key || "").toLowerCase());
      if (i === undefined) return;
      e.preventDefault();
      openMenu(i);
      const first = drops[i].querySelector("button");
      if (first) first.focus();
    });
    const clearMnemonics = () => bar.classList.remove("show-mnemonics");
    document.addEventListener("keyup", (e) => { if (e.key === "Alt") clearMnemonics(); });
    // Alt+Tab and the like leave the key "held" as far as this document is
    // concerned, so drop the hint whenever the window loses focus too.
    window.addEventListener("blur", clearMnemonics);

    // Keyboard navigation for the menu bar and the open menu.
    const focusTop = (i) => {
      const n = topBtns.length;
      const at = ((i % n) + n) % n;
      for (const [j, b] of topBtns.entries()) b.tabIndex = j === at ? 0 : -1;
      topBtns[at].focus();
      return at;
    };
    // The items of the open drop, in visual order, skipping separators.
    const dropItems = () => (openIdx < 0 ? [] : [...drops[openIdx].querySelectorAll("button")]);
    const focusItem = (list, i) => {
      if (!list.length) return;
      const n = list.length;
      list[((i % n) + n) % n].focus();
    };
    bar.addEventListener("keydown", (e) => {
      const i = topBtns.indexOf(activeEl());
      if (i < 0) return;
      if (e.key === "ArrowRight") { const at = focusTop(i + 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "ArrowLeft") { const at = focusTop(i - 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "Home") { const at = focusTop(0); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "End") { const at = focusTop(topBtns.length - 1); if (openIdx >= 0) openMenu(at); e.preventDefault(); }
      else if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        openMenu(i);
        focusItem(dropItems(), 0);
        e.preventDefault();
      } else if (e.key === "Escape") { closeMenus(); e.preventDefault(); }
    });
    // Within an open menu: up/down through the items, left/right to the
    // neighbouring menu, Escape back to the bar.
    document.addEventListener("keydown", (e) => {
      if (openIdx < 0) return;
      const list = dropItems();
      const at = list.indexOf(activeEl());
      if (at < 0) return;
      if (e.key === "ArrowDown") { focusItem(list, at + 1); e.preventDefault(); }
      else if (e.key === "ArrowUp") { focusItem(list, at - 1); e.preventDefault(); }
      else if (e.key === "Home") { focusItem(list, 0); e.preventDefault(); }
      else if (e.key === "End") { focusItem(list, list.length - 1); e.preventDefault(); }
      else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        const next = focusTop(openIdx + (e.key === "ArrowRight" ? 1 : -1));
        openMenu(next);
        focusItem(dropItems(), 0);
        e.preventDefault();
      } else if (e.key === "Escape") { const i = openIdx; closeMenus(); focusTop(i); e.preventDefault(); }
    });

    document.addEventListener("click", (e) => {
      if (openIdx < 0) return;
      const inMenu = e.target.closest(".menubar, .menu-drop, .menu-sub");
      if (!inMenu) closeMenus();
    });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenus(); });

    // Help modal close wiring.
    const modal = byId("oc-modal");
    byId("oc-modal-x").addEventListener("click", () => { modal.hidden = true; });
    modal.addEventListener("click", (e) => { if (e.target === modal) modal.hidden = true; });
    document.addEventListener("keydown", (e) => { if (e.key === "Escape") modal.hidden = true; });
  }
  // Esc closes the tool panel (when no context menu is open and not editing).
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && activePanel && !state.editing && !byId("sheet-ctx")) {
      closePanel();
    }
  });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => { readColors(); draw(); });

  wirePresence();
  wireSettings();
}



/// Open the Settings panel from anywhere — assigned by [`wireSettings`].
///
/// The menu items used to reach it by un-collapsing the app header and then
/// clicking the gear inside it, which is the whole of why hiding that header
/// was not a one-line change: with the header gone there is no gear on screen,
/// `setHeaderCollapsed(false)` has nothing to reveal, and the panel opened
/// inside a `display: none` ancestor. One entry point, and it decides its own
/// presentation from whether anything is there to anchor to.
let openSettings = () => {};

function wireSettings() {
  const gear = byId("tb-settings");
  const panel = byId("settings-panel");
  const scrim = byId("settings-scrim");

  /// Is the gear actually on screen? `getClientRects()` rather than a class or
  /// a mode check: the header is hidden by desktop chrome, by `?hide=header`,
  /// by `?mode=embedded` and by the collapse caret, and the question this has
  /// to answer is the same in all four — *can this panel point at anything*.
  const gearOnScreen = () => !!gear && gear.getClientRects().length > 0;

  /// Place the panel, in whichever of its two forms applies.
  ///
  /// The anchored form is `anchorMenu()`'s arithmetic, kept local because that
  /// one is a closure inside `wireEvents()` and this panel needs the *other*
  /// branch as well. Flips above the button when there is no room below and
  /// clamps to the viewport at both edges, so a short window does not put half
  /// of Settings off the bottom.
  const place = () => {
    const anchored = gearOnScreen();
    panel.classList.toggle("anchored", anchored);
    scrim.hidden = anchored;
    // `aria-modal` only in the dialog form. Claiming it for a popover the user
    // can click straight past is a lie to a screen reader.
    if (anchored) panel.removeAttribute("aria-modal");
    else panel.setAttribute("aria-modal", "true");
    if (!anchored) {
      // The stylesheet's own centring; nothing to compute.
      panel.style.left = panel.style.top = "";
      return;
    }
    const r = gear.getBoundingClientRect();
    panel.style.left = "0px";
    panel.style.top = "0px";
    const pw = panel.offsetWidth, ph = panel.offsetHeight;
    let left = Math.max(4, Math.min(r.right - pw, window.innerWidth - 4 - pw));
    const below = window.innerHeight - 4 - (r.bottom + 6);
    const top = ph <= below ? r.bottom + 6 : Math.max(4, r.top - 6 - ph);
    panel.style.left = `${left}px`;
    panel.style.top = `${top}px`;
  };

  const close = () => {
    panel.hidden = true;
    scrim.hidden = true;
  };
  /// **The click that opened it must not also close it.**
  ///
  /// The gear stops propagation, so it never had this problem. A menu item does
  /// not: its click bubbles to the outside-click listener below in the same
  /// task, where the panel is already open and the target is not inside it, and
  /// Settings opened and shut again too fast to see. Cleared on a timeout
  /// rather than a flag the listener resets, because the listener may not run
  /// at all — the panel can be opened from a keyboard accelerator with no click
  /// behind it.
  let openingClick = false;
  const open = () => {
    panel.hidden = false;
    openingClick = true;
    setTimeout(() => { openingClick = false; }, 0);
    place();
    // The dialog form is reached from a menu, so the pointer is nowhere near
    // it; without this the user has a dialog on screen and the keyboard still
    // in the grid. The anchored form is left alone — a popover that steals
    // focus from the sheet on a stray gear click is worse than one that does
    // not.
    // Theme used to be this panel's first control and so was what took focus.
    // It is `View ▸ Theme` now (`UX-CHR-01`), so focus goes to whatever the
    // panel's first focusable is — asked of the DOM rather than named, or the
    // next control to move takes the keyboard with it.
    if (!panel.classList.contains("anchored")) {
      panel.querySelector("select, input, button:not(.settings-close)")?.focus();
    }
  };
  openSettings = open;

  gear.addEventListener("click", (e) => {
    e.stopPropagation();
    if (panel.hidden) open();
    else close();
  });
  byId("settings-close").addEventListener("click", close);
  scrim.addEventListener("click", close);
  document.addEventListener("click", (e) => {
    if (panel.hidden || openingClick) return;
    if (!panel.contains(e.target) && e.target !== gear && !gear.contains(e.target)) close();
  });
  // Esc, which is what closes every other overlay here. Capture, so it reaches
  // this before the grid's own Escape handling cancels an edit instead.
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !panel.hidden) { close(); canvas?.focus(); }
  }, true);
  // A window that changed size moved the gear; a panel still pinned to where it
  // used to be is the defect `anchorMenu()`'s callers already re-run for.
  window.addEventListener("resize", () => { if (!panel.hidden) place(); });
  for (const b of qsa("#set-accent button")) {
    b.addEventListener("click", () => applyAccent(b.dataset.c));
  }

  const scroll = byId("set-scroll");
  const scrollVal = byId("set-scroll-val");
  const setScroll = (v, persist) => {
    scrollDamp = v;
    scroll.value = String(v);
    scrollVal.textContent = v.toFixed(2);
    if (persist) localStorage.setItem("oc-scroll", String(v));
  };
  scroll.addEventListener("input", () => setScroll(parseFloat(scroll.value), true));

  // Restore saved preferences (default scroll speed is 0.80).
  //
  // Theme is still restored here even though its control has moved to
  // `View ▸ Theme`: this runs at boot and the menu does not, and a window that
  // came up light because nobody had opened a menu yet would be the whole
  // feature lost. `applyTheme` is what records the choice `currentTheme()`
  // reports, so the tick in that menu is right on the first open.
  applyTheme(localStorage.getItem("oc-theme") || "auto");
  const accent = localStorage.getItem("oc-accent");
  if (accent) applyAccent(accent);
  const savedScroll = parseFloat(localStorage.getItem("oc-scroll"));
  setScroll(Number.isFinite(savedScroll) ? savedScroll : DEFAULT_SCROLL_DAMP, false);
}

// Seed a small demo workbook with formulas.


// Resolve the markup's elements against the mount root.
//
// Deferred rather than resolved at import time: an embedded editor sets its
// mount root *after* the module loads, and a binding taken against `document`
// before that points at nothing — or, worse, at the same id somewhere in the
// host's page.
function bindElements() {
  canvas = byId("grid");
  wrap = byId("grid-wrap");
  inline = byId("inline-edit");
  selStats = byId("sel-stats");
  vscroll = byId("vscroll");
  vthumb = byId("vthumb");
  hscroll = byId("hscroll");
  hthumb = byId("hthumb");
  fInput = byId("formula-input");
  cellRef = byId("cell-ref");
  commentTip = byId("comment-tip");
  status = byId("tb-status");
  findBar = byId("find-bar");
  findInput = byId("find-input");
  replaceInput = byId("replace-input");
  findCount = byId("find-count");
  findCase = byId("find-case");
  findWhole = byId("find-whole");
  findValues = byId("find-values");
  findAllSheets = byId("find-all-sheets");
  findWildcards = byId("find-wildcards");
  liveEl = byId("grid-live");
  modeEl = byId("cell-mode");
  a11yEl = byId("grid-a11y");
  acEl = byId("ac-menu");
  sigEl = byId("sig-tip");
  tabsEl = byId("sheet-tabs");
  ctx = canvas.getContext("2d");
}


/// Load a workbook from bytes, whatever the host got them from.
///
/// Extracted from the file-input handler so an embedded editor opens a
/// workbook by the same path a picked file does — two implementations of
/// "open" is two places for the post-load bookkeeping below to be forgotten.
///
/// `budgetMs` is how long the engine may hold the thread before the open is
/// stopped; a negative one means "no limit", which is what "Keep waiting"
/// retries with.

/// Re-read the theme tokens and repaint.
///
/// The canvas caches them: it paints thousands of cells a frame and cannot ask
/// for a computed style per cell. So a host changing `--oc-accent-color` restyles the
/// chrome instantly and would leave the grid on the old colours until something
/// else forced a repaint — this is what closes that gap.

/// Scroll to the top-left and select A1.
///
/// What a thumbnail means: a preview scrolled to row 14 of someone else's
/// workbook is not a preview of it.

/// Re-measure and repaint after the element's box changed.
///
/// Hiding a chrome region gives the grid more room, and the canvas is sized in
/// device pixels from a measured box — so without this the grid keeps the old
/// height and leaves a gap where the toolbar used to be.

/// Autofit a column, for the browser gate. See [`autofitRowForTest`].

/// Autofit a row, for the browser gate.
///
/// Exported because the alternative was asserting nothing about autofit: it is
/// reached by double-clicking a row boundary, which a test can only simulate by
/// guessing at pixel coordinates on a canvas — and a test that guesses wrong
/// passes for the wrong reason.

/// Where the body is scrolled to, and where the selection is, for the browser
/// gate.
///
/// Exported because the property worth asserting — that the leading edge sits on
/// a whole column and a whole row — is invisible from outside: the grid is a
/// canvas, so a test can only see it by sampling pixels and deciding what a
/// column boundary looks like, which is a test of the sampling. The number the
/// renderer actually uses is the claim.

/// Set the magnification, for the browser gate.
///
/// Routed through the editor's own `setZoom` rather than assigning
/// `state.zoom`, so the gate exercises what the menu does — including whatever
/// clamping and repaint that entails.

/// Move the selection, for the browser gate — the keyboard path, without the
/// keyboard.
///
/// `ensureVisible` is what decides the scroll offset, and it is reached by
/// arrowing off the edge of the viewport. Synthesising that needs the canvas
/// focused and the right number of key events for the window size, which is a
/// test that measures the harness.
/// Routed through the editor's own `select`, not a copy of it: a test that
/// reimplements the path it is testing agrees with itself.

/// The selected rectangle, for the browser gate.
///
/// `scrollStateForTest` reports the *active cell*, which is all a step needs;
/// an extend has to be asserted on the rectangle, because the bug it guards
/// against is precisely a selection collapsing to one cell while the active cell
/// looks right.

/// The raw engine bindings, for a host that needs something the element does
/// not wrap. Deliberately the same object the editor itself uses: a second
/// session would be a second workbook.

// --- Collaboration ----------------------------------------------------------
//
// The transport lives in `collab.js` and knows nothing about a grid. This is
// the other half: what the editor does when a document arrives, when somebody
// else's edit lands, and when a participant moves.
//
// The join is *here*, rather than left to the host, for one concrete reason.
// The engine is a module-scope binding imported under a cache-busting
// specifier, and each mounted element imports its own copy — so a host that
// imported the glue itself would get a second, uninitialised instance and every
// call would throw from inside the generated bindings. Handing the host a
// `collaborate()` that closes over *this* editor's engine removes a decision
// nobody can make correctly from outside.

/// The live session, or null.
export let collabSession = null;

/// Everyone else, by client id. What draws a remote cursor, and what a host
/// reads to render a participant list.
export const collabRoster = new Map();

/// What was last announced — sheet, selection and draft — so presence is sent
/// when it changes and not on every frame. `draw()` polls this the way
/// `emitStateEvents` does, for the same reason: there are dozens of places the
/// selection changes and one of them will always be forgotten.
let collabAnnounced = "";

/// The floor between two presence messages, and the trailing timer that makes
/// the floor safe.
///
/// A draft changes on every keystroke, and a message per keystroke is a message
/// per keystroke for every other participant to receive, parse and repaint. A
/// touch-typist runs at about eight a second, so this collapses a burst into
/// roughly six a second while leaving the *first* keystroke immediate — a
/// leading edge, because the interesting moment for everyone else is the one
/// where somebody starts typing in a cell.
///
/// The trailing timer is not optional. Throttling by dropping would drop the
/// **last** keystroke of a burst, which is the one that stays on screen: peers
/// would sit looking at "=SUM(A1:A9" for as long as the author admired their
/// finished formula.
const PRESENCE_THROTTLE_MS = 150;
let collabAnnounceTimer = null;
let collabAnnouncedAt = 0;

/// Whether a reconnect discarded work this client had not sent yet.
///
/// Sticky on purpose. Every other collaboration status describes a condition
/// that will pass; this one describes cells that are gone, and the "live" the
/// reconnect would otherwise emit a moment later erases the only notice the
/// user ever gets. The transport withholds "live" until an edit made *after*
/// the loss has been acknowledged, so this clears when editing demonstrably
/// works again rather than when the socket merely reopens.
let collabLostUnsent = false;

/// Join a collaborative session.
///
/// `url` is the server's WebSocket endpoint and `token` the host-signed token
/// that says who this is and what they may do. Returns the session handle, or
/// throws if one is already open — joining twice would leave two transports
/// submitting the same edits under different client ids.
export async function collaborate({ url, token, document: documentKey, onStatus, onDocument, onPresence } = {}) {
  if (collabSession) throw new Error("already in a collaborative session");
  // What the Share dialog needs in order to say what is being shared, recorded
  // **here** rather than in the dialog, because this is the one function every
  // route goes through: a host calling `collaborate()` from its own UI would
  // otherwise leave `File ▸ Share…` offering an invite link with an empty
  // `doc=`. The transport's handle does not carry the key back — it returns
  // `{ present, close, flush, latency, ping, reconnect }` — so this is the only
  // place that knows it.
  //
  // The **token is deliberately not recorded**: it is a credential, and putting
  // it here would prefill it into a DOM input on the next Share dialog. See
  // §Share in `editor.presence.js`.
  setShareDefaults({ url, document: documentKey });
  const { collaborate: connect } = await import(`./collab.js?b=${BUILD}`);
  collabSession = connect({
    url,
    token,
    document: documentKey,
    wasm,
    // The same budget a local recalculation gets. A peer's edit is not entitled
    // to more of this tab than the person sitting at it (`COL-43`).
    recalcBudgetMs,
    onStatus: (event) => {
      // The status line is the only place the editor says this out loud, and
      // "reconnecting" is the one a user needs to see before they wonder why
      // their typing stopped mattering.
      //
      // `lost` outranks the rest and **sticks**: it is the one state that
      // reports work already gone rather than a condition that will pass, so
      // letting the next "collaborating" overwrite it turns a data-loss notice
      // into a flicker. It clears when the user next edits successfully, which
      // is the point at which they have demonstrably read the grid.
      if (event.state === "lost") {
        collabLostUnsent = true;
        status.textContent =
          "reconnected to a different server — edits you made while disconnected were not saved";
      } else if (event.state === "live") {
        collabLostUnsent = false;
        status.textContent = "collaborating";
      } else if (collabLostUnsent) {
        // Held. Anything else this transport wants to say can wait: the
        // transport does not send "live" again until an edit made *after* the
        // loss has been acknowledged, so this clears when editing demonstrably
        // works, not merely when the socket comes back.
      } else if (event.state === "reconnecting") status.textContent = "reconnecting…";
      else if (event.state === "refused") status.textContent = `not saved: ${event.detail}`;
      else if (event.state === "stopped") status.textContent = `disconnected: ${event.detail}`;

      // **A session the transport has finished with is finished here too.**
      //
      // `stopped` is terminal — the transport has closed the socket and will
      // not reconnect, because reconnecting would be refused for the same
      // reason. But only an explicit `stopCollaborating()` used to clear
      // `collabSession`, so the editor went on believing it was in a session
      // that no longer existed, and the next `collaborate()` threw "already in
      // a collaborative session".
      //
      // The result: a token that expired, or any other refusal, could only be
      // recovered from by reloading the page — which throws away whatever the
      // user had locally. Observed live: a refused join left the editor unable
      // to rejoin the same document with a valid token seconds later.
      if (event.state === "stopped") {
        collabSession = null;
        collabRoster.clear();
        forgetCollabAnnouncement();
        renderPresence();
        draw();
      }
      onStatus?.(event);
    },
    onDocument: (event) => {
      adoptCollabDocument(event);
      onDocument?.(event);
    },
    onPresence: (event) => {
      if (event.kind === "gone") collabRoster.delete(event.client);
      else collabRoster.set(event.client, event);
      renderPresence();
      draw();
      onPresence?.(event);
    },
  });
  // Shown as soon as there is a session, before anybody else has moved: "only
  // you" is an answer to "who is collaborating", and an absent control is not.
  renderPresence();
  return collabSession;
}

/// Leave, if in a session. Safe to call when not.
export function stopCollaborating() {
  collabSession?.close();
  collabSession = null;
  collabRoster.clear();
  forgetCollabAnnouncement();
  renderPresence();
}

/// Forget what was last announced, and cancel anything queued to announce.
///
/// Both halves matter when a session ends. The key has to go or the *next*
/// session's first selection looks unchanged and is never sent; the timer has
/// to go or it fires into a closed session, and — worse — a draft queued from
/// the old session would be the first thing the new one said.
function forgetCollabAnnouncement() {
  collabAnnounced = "";
  collabAnnouncedAt = 0;
  clearTimeout(collabAnnounceTimer);
  collabAnnounceTimer = null;
}

/// The other participants, as a host would show them.

// --- The participant roster (COL-33) ----------------------------------------
//
// `drawCollaborators` puts a cursor where somebody is standing, which answers
// "who is in this cell" and nothing else. It cannot answer "who is in this
// document" — a cursor two thousand rows down or on another sheet is drawn
// nowhere — and that is what was reported against the running demo: *"i can't
// see here which profiles are collaborating... i see the name"*. The name was
// the only evidence anybody else existed, and only if you happened to be
// looking at their cell.
//
// So: a face stack in the menu bar that opens a roster of who is here, what
// they are doing, and a click that takes you to them.
//
// **Every string in here is somebody else's text.** A name arrives in the
// token, which an integrator minted from whatever their user typed; a sheet
// name comes out of a workbook, which is a file somebody uploaded. SEC-001 is
// the rule that neither may build DOM in this origin, so this constructs nodes
// and assigns `textContent`. There is no markup in this section, and there is
// no interpolation into a selector either — a client id with a quote in it
// would otherwise throw out of `querySelector` and take the roster with it.
//
// Nothing below touches the document, the history or the outgoing log.

/// How many faces the stack shows before the rest become "+n".
///
/// Three, because the stack shares a 30px bar with eight menus and the collapse
/// caret, and every extra face is 13px taken from them. Everybody is in the
/// list; the stack is a summary of it, not the roster.
export const PRESENCE_FACES = 3;

/// Whether the roster popup is open.
export let presenceOpen = false;

/// A participant's display name, never empty.
///
/// `someone` matches what the cursor tag draws for a nameless participant, so
/// the list and the grid agree about who that is.

/// One or two letters for a face, from a name that may be anything at all.
///
/// `Array.from` rather than `name[0]`: a name starting with an emoji or any
/// astral-plane character has a first *code unit* that is half a surrogate
/// pair, and half a pair renders as a replacement box.

/// The r/g/b of a colour `participantColor` has already vouched for, or null.

/// Black or white on a participant's colour, whichever can be read.
///
/// Computed rather than assumed, because the palette is the server's and a
/// deployment may replace it: white initials on `#FDD835` are invisible, and an
/// unreadable name is the exact failure this control exists to fix.

/// A coloured initial for the stack and for each row of the list.

/// A cell reference from a presence entry, or null.
///
/// Every field is validated rather than trusted. Presence comes from another
/// client by way of the server, which bounds it but does not promise it is
/// sensible, and `A1(undefined, undefined)` is a label reading "undefined".

/// Where a participant is, as a person would say it: `Budget!D8`.

/// One row of the roster: who, where, and whether they are mid-word.

/// Rebuild the face stack and the roster from the live roster map.
///
/// Called on every presence message, which during a burst of typing is roughly
/// six a second. That is why the open list restores focus and scroll position
/// afterwards: somebody arrow-keying down the roster while a peer types would
/// otherwise have the focus thrown back to the page under them, and a list that
/// jumps to the top every time somebody presses a key cannot be read.

/// The focusable rows of the open roster, in the order they are shown.

export function openPresence() {
  const menu = byId("presence-menu");
  const btn = byId("presence-btn");
  if (!menu || !btn || !collabSession) return;
  presenceOpen = true;
  renderPresence(); // current as of the moment it opens, not of the last event
  menu.hidden = false;
  btn.setAttribute("aria-expanded", "true");
}

/// Close the roster. `refocus` returns focus to the button, which is what
/// Escape must do — closing a menu that had focus and dropping focus on the
/// floor strands a keyboard user at the top of the document.
export function closePresence(refocus = false) {
  const menu = byId("presence-menu");
  const btn = byId("presence-btn");
  presenceOpen = false;
  if (menu) menu.hidden = true;
  if (btn) {
    btn.setAttribute("aria-expanded", "false");
    if (refocus) btn.focus();
  }
}

/// Take this user to what a participant has selected.
///
/// Being told somebody is in P47 and having to go and find P47 is half a
/// feature, and it is the half the roster would otherwise be.
///
/// It moves the **view**, never this user's selection. Your active cell is
/// where your next keystroke lands, and a control that quietly moved it would
/// be a control that quietly types your work somewhere else. Their cursor is
/// already painted by `drawCollaborators`, so arriving is enough to see them.
///
/// Nothing here writes to the document, the history or the outgoing log.
/// Switching sheets does announce *this* client's own presence — the same
/// message `switchSheet` already sends when a tab is clicked, from the same
/// code, because this client really did move.

/// Wire the roster control. Called once, from `wireEvents`.

/// Take on what the transport just did to the model.

/// Paint the other participants — their selection, what they are typing, and a
/// name tag — each in their own colour.
///
/// The roster was already being kept — presence arrives, `collabRoster` is
/// updated and `draw()` is called — and then nothing read it. Co-editing worked
/// and looked exactly like editing alone, which is the failure this fixes: the
/// point of seeing somebody else's cursor is knowing not to type there.
///
/// Colour and name both come from the server, which takes them from the token.
/// A client naming itself is the one place a claimed identity would be believed.
/// A participant's colour as canvas will actually accept it.
///
/// The server's palette is bare hex — `0891B2`, no `#` — and an invalid
/// `strokeStyle` is **silently ignored** by canvas rather than throwing, so
/// every cursor would have quietly inherited whatever colour was set last. Every
/// participant in the accent colour looks like a working feature, which is the
/// kind of wrong that never gets reported.


/// Paint what a participant is typing, in the cell they are typing it into.
///
/// The point of COL-35, in one function: until this existed, somebody else's
/// work appeared only when they pressed Enter, so two people could fill the same
/// cell and neither found out until one of them lost.
///
/// Drawn as their in-cell editor would look on their screen — an opaque box in
/// their colour over whatever the cell currently holds — because that is what it
/// is. A wash over the old value would leave two overlapping strings and read as
/// a rendering fault.
///
/// **The text is untrusted.** It came from another participant, through the
/// server, which bounds its length and does not otherwise vouch for it. It goes
/// to `fillText` on the canvas and never near markup: SEC-001 is the rule, and
/// this is the newest path in the editor that carries somebody else's text.

/// What this participant is part-way through typing, for the others to watch.
///
/// Null unless a cell editor is open on *this* sheet. The draft belongs to
/// `editHome` rather than to the selection, because writing a formula walks the
/// selection off to pick references — and a peer would otherwise watch the text
/// hop around the grid with it.
///
/// The cross-sheet case reports nothing rather than guessing: a presence message
/// names one sheet, and a draft on the sheet you are not looking at is not
/// something a grid can draw. It comes back the moment the author does.

/// Tell the others where this participant is looking and what they are typing,
/// when either has changed.
///
/// Called from `draw()` — which covers every way a selection can move — and
/// directly from the editing path, which `draw()` does not cover: a keystroke
/// changes the text without changing anything the grid repaints.
///
/// Presence is ephemeral and never transformed (ADR-011), so sending it late
/// costs nothing and losing one costs nothing either. That is what lets this be
/// throttled at all.
export function announceCollabSelection() {
  if (!collabSession) return;
  const r = selRect();
  const draft = collabDraft();
  const key = `${state.sheet}:${r.r0},${r.c0},${r.r1},${r.c1}|${
    draft ? `${draft.at[0]},${draft.at[1]}:${draft.text}` : ""
  }`;
  if (key === collabAnnounced) return;

  const since = Date.now() - collabAnnouncedAt;
  if (since < PRESENCE_THROTTLE_MS) {
    // Too soon. Come back at the end of the window and read the state *then*,
    // rather than sending this one late — by then the user will have typed
    // more, and what everybody wants to see is where they got to.
    if (!collabAnnounceTimer) {
      collabAnnounceTimer = setTimeout(() => {
        collabAnnounceTimer = null;
        announceCollabSelection();
      }, PRESENCE_THROTTLE_MS - since);
    }
    return;
  }
  collabAnnounced = key;
  collabAnnouncedAt = Date.now();
  collabSession.present(state.sheet, [r.r0, r.c0, r.r1, r.c1], draft);
}

/// Distinguishes this module instance's engine from any other on the page.
///
/// The editor is a module with module-scope state — one `wasm` binding, one
/// `state`, one geometry cache — so a second element mounting the *same* module
/// would share and race all of it. Each element therefore imports its own copy
/// of this module and its own copy of the wasm glue, which is what this key
/// varies. See `docs/55` §4b for what that costs.
let instanceKey = "";

/// Fetch and register the faces a host offers, when it says it offers some.
///
/// **Opt-in, and it has to be.** The obvious version probes `/api/fonts` on
/// every boot and treats a 404 as "no font service" — which works, and logs
/// `Failed to load resource: 404` to the console of every deployment that does
/// not run one, which is most of them. A `fetch` rejection can be caught; the
/// browser logging a failed request cannot. A diagnostic that cries wolf on
/// every boot is worse than no diagnostic, so nothing is fetched unless asked.
///
/// Ask with `?fonts` on the editor URL — bare for the conventional
/// `/api/fonts`, or `?fonts=/some/other/path` for anything else.
///
/// Failures past that point are logged and never fatal: having been told the
/// service is there, a face that will not load is worth a line, because the
/// realistic cause is a fetch that returned an error page and the symptom
/// otherwise shows up much later looking like a renderer bug.

async function main() {
  bindElements();
  // The document lives in wasm memory and nowhere else — no autosave, no draft.
  // Closing the tab, reloading, or pressing Back discarded it without a word.
  //
  // The browser decides the wording and ignores any we supply; all a page can
  // do is say that there is something to lose. Nothing is shown when the
  // document is clean, so this never becomes the dialog everybody clicks
  // through without reading.
  window.addEventListener("beforeunload", (e) => {
    if (!isDirty()) return;
    e.preventDefault();
    e.returnValue = "";
  });
  // The mode's chrome, before first paint rather than after it — hiding a bar
  // later shows it for a frame and then takes the space back under the user.
  // A desktop shell draws the menu bar itself; an embedded editor drops our
  // branding strip, because inside somebody else's page it is theirs.
  //
  // `?chrome=native` still resolves here, via `askedMode()`, so the shipped
  // desktop host keeps the URL it already appends.
  try { applyModeChrome(); } catch {}
  const mod = await import(`./pkg/casual_calc_wasm.js?b=${BUILD}${instanceKey}`);
  init = mod.default;
  wasm = mod;
  // Resolved against **this module**, not the page. A bare relative string is
  // resolved against the document, so the binary was fetched from wherever the
  // host's HTML happened to live — which worked only while the page sat beside
  // the engine, and 404'd the moment an example in another directory embedded
  // it. `import.meta.url` is the whole reason a bundler can emit this as an
  // asset and still have it found.
  await init(new URL(`./pkg/casual_calc_wasm_bg.wasm?b=${BUILD}`, import.meta.url));

  // A handle for a host that embeds this page rather than importing the module
  // itself — an iframe's exports cannot be reached from the parent frame, and a
  // host has no way to name the cache-busted specifier this module was loaded
  // under. Importing `import.meta.url` returns this same instance from the
  // module cache rather than building a second one.
  if (typeof window !== "undefined") {
    try { window.opencalcEditor = await import(import.meta.url); } catch {}
  }

  // Faces the deployment supplies, registered before anything can be rendered
  // to a PNG. Best-effort on purpose: a host that offers no `/api/fonts` is the
  // normal case, and the editor itself never needs them — the browser draws
  // every cell a user looks at with its own faces. This is for `render_png`,
  // where a missing face is a picture full of boxes.
  await registerSuppliedFonts();

  COL_W = wasm.default_col_px();
  ROW_H = wasm.default_row_px();
  readColors();
  wasm.session_new();
  // Before anything is evaluated: nothing volatile works until the host has
  // handed the engine a clock.
  syncClock(true);
  wireEvents();
  // **Again, because `wireEvents()` re-homes one of the nodes it moved.**
  //
  // The first call is before the wasm import, so the mode's chrome is decided
  // before the first paint rather than shown and then taken away. But
  // `wirePresence()` ends with `bar.insertBefore(box, hdr-collapse)` —
  // deliberately, so the roster survives `buildMenuBar()` appending File…Help
  // after it — and that put the collaborator roster straight back into the menu
  // bar desktop chrome hides. Measured, not assumed: `#tb-status` relocated and
  // `#presence` did not, and nothing about reading either function said why.
  //
  // Idempotent, so this is a re-assertion rather than a second policy: every
  // move in `placeNativeChrome()` is a no-op when the node is already where the
  // mode wants it.
  try { applyModeChrome(); } catch {}
  // Toolbar controls get command ids from their element ids (`tb-bold` →
  // `toolbar.bold`), so a host hides a button by name rather than by reaching
  // into the shadow root with a selector that will move.
  for (const node of qsa("[id^='tb-']")) {
    if (!node.dataset.ocCommand) node.dataset.ocCommand = `toolbar.${node.id.slice(3)}`;
  }
  // There is no `header.open` any more. The branding strip's folder button was
  // a second route to `File ▸ Open` sitting in the one region that should carry
  // identity and document state only, and in desktop chrome — where the strip
  // is not drawn — it left `listCommands()` naming a command with nothing to
  // click (`UX-CHR-01`, `UX-DESK-05`). Both the button and the id went; the
  // capability is `File ▸ Open`'s and the operating system's own menu bar.
  //
  // Tooltips are the toolbar's only text, so they are what a translated
  // toolbar translates. The English one is kept as the fallback.
  //
  // Read from `data-tip` as well as `title`: the custom tooltip layer runs
  // first and *moves* `title` onto `data-tip` to suppress the native bubble,
  // so by the time this ran only the handful of nodes outside its reach still
  // had a `title` at all. Sixty-four of the seventy-five tooltips were
  // therefore never registered for translation, and `tip.*` — a documented
  // part of the SDK's localization surface — silently did nothing for them.
  for (const node of qsa("[title], [data-tip]")) {
    if (!node.dataset.ocTip) node.dataset.ocTip = node.dataset.tip || node.title;
  }
  relabel();
  seed();
  renderTabs();
  resize();
  // The status bar's zoom controls. After `resize()`, so the first readout is
  // drawn from the zoom the grid is actually laid out at.
  wireZoom();
  // The mode's commands, once the menus exist and the toolbar has reflowed —
  // both are built by `wireEvents()` above, and `applyCommandRules()` reads the
  // live DOM rather than the `MENUS` literal (`TAURI-004`).
  //
  // A `readOnly` preset is handed to the **engine**, which is the only thing
  // that can actually refuse an edit; `setReadOnly` then applies the rules
  // itself. Everything else takes the plain call. In `standalone` — every
  // capability true, `readOnly` false — this hides nothing and disables
  // nothing, which is the whole of "the default changes nothing".
  if (getCapabilities().readOnly) setReadOnly(true);
  else applyCommandRules();
  status.textContent = `engine v${wasm.version()}`;

  // Give the grid the keyboard (`UX-FOCUS-01`).
  //
  // Every grid shortcut is bound on the canvas, so all of them needed it to be
  // focused, and nothing ever focused it: the editor opened with focus on
  // `<body>` and Ctrl/Cmd+A, Ctrl/Cmd+X, the arrows and the rest did nothing
  // until the user happened to click a cell. On macOS Cmd+A ran the browser's
  // own select-all over the page instead, which is how it was found. The 53
  // `canvas.focus()` calls that put focus *back* after a dialog were all there;
  // the one that puts it there to begin with was missing.
  //
  // Only when nothing else has it, and only when this *is* the page.
  //
  // The `activeElement` check alone could never do that second part. Inside an
  // iframe it asks about the iframe's **own** document, where nothing is
  // focused, so it passed every time — and focusing the canvas made the parent
  // browser scroll the frame into view. On the landing page, which embeds the
  // editor below the fold, that scrolled the hero off the top: every visitor
  // arrived to a page whose headline, badge and opening line had been pushed
  // out of sight by the demo underneath them. The intent below was written
  // down and not implemented (`UX-EMBED-01`).
  //
  // `window.top === window` is the question that was actually meant. An embed
  // waits to be clicked, the way any widget in someone else's page should.
  // `preventScroll` is belt as well as braces: it keeps a later focus from
  // moving a host page even if this runs somewhere unforeseen.
  const isTopLevel = (() => {
    try {
      return window.top === window;
    } catch {
      // A cross-origin parent throws on access, which is itself the answer.
      return false;
    }
  })();
  if (isTopLevel && (!document.activeElement || document.activeElement === document.body)) {
    canvas.focus({ preventScroll: true });
  }

  // Drafts, last: it reads the store, may put a bar on screen, and — when the
  // page was opened with `?draft=` — replaces the seeded document with the
  // recovered one. All three want an editor that already exists.
  //
  // **Awaited, and its failure is not fatal.** `initDrafts` catches its own
  // storage failures and reports them as a standing indicator rather than
  // throwing; this `catch` is the belt for anything it did not anticipate,
  // because a browser that will not store a draft must still open the editor.
  // A crash-recovery feature that stops the editor booting has recovered
  // nothing.
  await initDrafts().catch((err) => console.error("[opencalc] drafts", err));
  // **The history has to outlive the tab** (`HIST-03`). Failure is silent by
  // design: a browser with no IndexedDB, a full quota or a corrupted row must
  // produce an editor with no history, never an editor that will not start.
  loadVersions(wasm)
    .then((n) => { if (n) status.textContent = `${n} saved version${n === 1 ? "" : "s"} restored`; })
    .catch((err) => console.error("[opencalc] versions", err));
}

/// Start the editor against the current mount root.
///
/// Exported so an embed wrapper can point the editor at a shadow root first;
/// the page host below starts it immediately, as it always did.
export async function start(key = "") {
  instanceKey = key ? `&i=${encodeURIComponent(key)}` : "";
  return main();
}

// A page mount starts itself. An embedded one imports this module, calls
// `setMountRoot`, and then `start` — by which time this has already run against
// a document with no `#grid` in it, so it is skipped.
if (byId("grid")) {
  main().catch((err) => {
    if (status) status.textContent = `failed: ${err}`;
    else console.error(err);
  });
}

/// Where the grid's clickable chrome is, in canvas pixels, for the browser gate.
///
/// Returns the geometry a *user* aims at — the hidden-band handles and the two
/// corner freeze handles — so a test clicks the same pixels rather than
/// re-deriving them from assumptions about header widths. The alternative is a
/// test that hunts for a five-pixel target and goes flaky the first time a
/// default changes.
// The personal-view state a browser test needs (COL-32). Row visibility is
// asked of the *engine*, not of the DOM, because a row hidden by a personal
// view collapses to zero pixels and "did not render" is indistinguishable from
// "rendered somewhere else".

// Apply a personal filter without driving the dropdown, so a test can assert
// the *policy* (nothing relayed, nothing saved, subtotal unmoved) rather than
// the menu's markup.

// Clearing, and opening a column's dropdown, from a test. Both go through the
// same functions the menu and the header button call, so a test cannot pass
// against a path a user never takes.


