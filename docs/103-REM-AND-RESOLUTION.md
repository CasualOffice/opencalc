# 103 — Sizes in `rem`, and a grid that follows the screen

**Status: Decided 2026-11-05, not yet built.** Owner: *"we need to shift from
px to rem, and make it adjust based on zoom and resolution"*. The decisions
below were taken under the owner's standing delegation ("take decisions; no
decision is pending from me"). Rows: `UX-REM-01`..`04`.

## What is wrong today

- **The chrome cannot follow the user's text size.** `editor.css`,
  `ribbon.css` and `ribbon.tokens.css` state about 1,700 lengths in `px` and
  none in `rem` (measured 2026-11-05: 1421 + 247 + 33). A user who sets a larger
  default font in the browser, which is how low-vision users enlarge text
  without zooming the whole page, gets the same 12–13 px ribbon and dialogs.
  Only the marketing site (`style.css`) uses `rem`.
- **The grid does not follow a resolution change.** `resize()`
  (`webapp/editor.geometry.js:230`) sizes the canvas's backing store from
  `devicePixelRatio`, but it runs only on a window `resize`. Dragging the window
  to a monitor with different scaling changes `devicePixelRatio` without a
  resize, and the grid stays rasterised at the old ratio: blurry on the denser
  screen, or oversized on the other.
- **Hairlines blur at fractional scaling.** At 125% and 150% Windows scaling
  (`devicePixelRatio` 1.25, 1.5), a 1 px grid line drawn at a whole CSS pixel
  falls between device pixels and is drawn as two grey lines.

## The rule: three surfaces, three units

| Surface | Unit | Scales with | Why |
| --- | --- | --- | --- |
| **Chrome**: ribbon, formula bar, name box, sheet tabs, status bar, menus, dialogs, panes, tooltips | `rem` | the user's default text size, browser zoom, OS display scaling | Excel's chrome follows Windows text scaling. In a browser the equivalent is the user's default font size, and `rem` is the unit that carries it. |
| **Hairlines**: 1 px borders, 2 px focus rings and selection outlines, dividers | `px` | browser zoom and OS scaling only | A border that grows to 1.25 px is drawn blurred. Excel's gridlines and borders stay one device line. |
| **Grid**: cells, row and column headers, column widths, row heights, cell text | document units, drawn in CSS px × app zoom × `devicePixelRatio` on the canvas | the app's Zoom slider, browser zoom, OS scaling | Excel's grid follows its Zoom control and the display's DPI, not the system text size; column widths are stored in the file, not in the UI's units. Header text scales with Zoom, as Excel's does. |

Media queries use `em`, which resolves against the browser's default font size
and not the root element, so the layout's breakpoints move with the user's text
size and never depend on a stylesheet's own root rule.

**Conversion.** `1rem` is 16 px at the browser's default. Every `px` length in
the chrome stylesheets becomes `N/16 rem`, rounded to four decimal places,
**except** hairlines of 2 px or less in `border`, `outline`, `box-shadow`
offsets and dividers. At the default 16 px root every element measures exactly
what it does today, so the existing pixel assertions in the browser specs stay
valid and serve as the regression check for the conversion.

**JavaScript.** Positions computed from grid geometry (a menu placed at a
cell's corner, a drag preview) stay in px, because they come from the canvas.
Sizes the script *chooses* (a dialog's width, a pane's default width, the
ribbon's collapse thresholds) go through one helper, `rem(n)`, which reads the
root font size once per layout change. The ribbon's collapse points are
measured in rem, so a larger text size collapses groups earlier instead of
overflowing.

## Resolution

- **Re-rasterise on a ratio change.** Listen on
  `matchMedia("(resolution: <current>dppx)")`; when it stops matching, call
  `resize()` and re-arm the listener at the new ratio. This covers moving
  between monitors, OS scaling changes, and browser zoom (which also fires
  `resize`; the duplicate is cheap, as `resize()` is idempotent).
- **Snap hairlines to device pixels.** Grid lines, borders and the selection
  outline are drawn at `(Math.round(x × dpr) + 0.5) / dpr` in CSS units, so a
  one-device-pixel line lands on one device pixel at any ratio.
- **The app Zoom slider stays grid-only**, as in Excel, where View ▸ Zoom
  scales the sheet and never the ribbon.

## Embedding

`rem` resolves against the **host page's** root element, and a shadow root
cannot change that. A host using the common `html { font-size: 62.5% }` would
shrink an embedded editor's chrome to 62.5%. Detected rather than ignored: at
mount, the embed compares the host root's computed font size with a probe at
`font-size: medium` (the user's default). On a mismatch it warns once, naming
the cause, and uses the iframe mount (`mount="frame"`, [100](100-MULTI-EMBED.md)),
whose own root is ours. Until that mount exists (`UX-SHELL-24`), the warning
stands alone and the row stays open.

## Gates

- `tools/check-css-units.py`, in CI from the day this was decided, as a
  **ratchet**: each chrome stylesheet has a recorded count of non-hairline
  `px` lengths (1,354 in total on 2026-11-05: 1131 + 205 + 18), any rise fails,
  and any fall fails until the baseline is lowered in the same commit. No new
  `px` gets in while the conversion is under way, and at 0 it is the full rule.
  Seen red on an added `padding: 3px`, with the `1px` border beside it allowed.
- A browser spec at a 20 px root: the ribbon band, the formula bar, a dialog
  and the status bar each measure 1.25× their 16 px size, and the grid's column
  widths do not change.
- A browser spec at `deviceScaleFactor` 1, 1.25, 1.5 and 2: the canvas backing
  store equals its CSS size × the ratio after an emulated ratio change with no
  `resize` event. It is red today, because nothing re-rasterises.

## Rejected

- **Scaling the chrome with the app Zoom slider.** Excel does not; and it
  would make the slider resize the controls you use to operate it.
- **`calc(N * var(--oc-unit))` everywhere** instead of `rem`. That would
  survive a host's 62.5% root, but at the cost of 1,700 unreadable lengths and
  a unit nobody else uses; the embed mismatch is better handled once, at
  mount.
- **CSS `zoom` on the embed's host to cancel a host's root size.** It changes
  `getBoundingClientRect` and pointer coordinates under the canvas's hit
  testing, which would trade a visible problem for a silent one.
