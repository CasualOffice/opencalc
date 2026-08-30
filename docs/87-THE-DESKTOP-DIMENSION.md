# 87 — The desktop dimension: what an office application is expected to be

## Outcome

[12](12-COMPETITIVE-ANALYSIS.md) compares the **editor** against five products
and does it well. It does not compare the **application**. Its "first five
minutes" (§4.7) is measured in a browser tab, its keyboard table is Windows
chords in an HTML menu, and eight subjects that decide whether a desktop office
application is taken seriously do not occur in its 1,428 lines at all:
installers, code signing, auto-update, file associations, drag-and-drop of a
file onto the window, multiple windows, first run, and localisation. "Recent
files" occurs once, as a clause.

This note is that missing dimension, and its finding is narrower and worse than
"we are behind on features":

> **The desktop application cannot be reached from the operating system, cannot
> be left safely, and cannot be saved from its own File menu.** Double-clicking
> an `.xlsx` will never open it, because no file association is declared.
> Closing the window with unsaved work asks nothing, because nothing handles the
> close. And `File` has `New`, `Open…`, `Download ▸`, `Page setup…`,
> `Print…` and `Properties…` — but no `Save`, no `Save As`, and no
> `Open Recent`. `Ctrl+S` works and is named in no menu and in no help panel.

None of those four is a feature gap. Each is an application-shell gap that a
user meets in the first two minutes, before any spreadsheet feature is exercised
at all.

### How to read the markings

The repository's rule is that a document stating a contract the code does not
keep is a defect, and this document is about products it cannot run. So every
claim carries its provenance:

- **[tree]** — checked in this checkout, cited `file:line`. If it says something
  is absent, the search patterns are given.
- **[documented]** — read in vendor or platform documentation during this pass,
  with the source named.
- **[unverified]** — believed and not checked here. Every one of these says what
  would settle it. Do not brief from an [unverified] line as though it were
  measured; [12](12-COMPETITIVE-ANALYSIS.md) §"Read this first" records six
  times that cost real work.

Nothing here describes proposed behaviour in the present tense.

---

## 1. What this compares against, and how far to trust it

Six products, chosen because between them they cover every desktop convention a
user might arrive carrying: **Microsoft Excel** (Windows and macOS, which differ
enough to count separately), **Google Sheets** (the web incumbent, and the
counter-example — what a desktop app is *not*), **LibreOffice Calc**,
**OnlyOffice Desktop Editors**, **Apple Numbers**, and **WPS Office**.

**The honest limit of this pass**: none of the six was run. Their behaviour here
is either read in documentation during this pass (**[documented]**) or believed
and marked **[unverified]**. What *was* run and read is this repository, and
every claim about it is **[tree]** with a line number. That asymmetry is
deliberate — the expensive mistake in [12](12-COMPETITIVE-ANALYSIS.md) was never
about a competitor, it was about us.

**The comparison is not the point.** The six converge so completely on the
subjects below that the interesting question is not "who does what" but "what
does a user assume without being told". A convention nobody advertises is one
everybody has, and its absence is felt as the application being broken rather
than as a feature being missing. That is why this document ranks by **when a
user meets it** and not by effort or by category.

### 1.1 The bar, stated concretely

Three of these were verified verbatim during this pass and are worth having in
front of you, because "a File menu" is not a specification and these are.

**LibreOffice Calc's File menu, in order** [documented,
`help.libreoffice.org/latest/en-US/text/shared/menu/PickList.html`]: New · Open ·
Open Remote · **Recent Documents** · Close · Wizards · **Templates** · Compare
Document · Merge Document · **Reload** · **Versions** · Save · Save As · Save
Remote · Save a Copy · Save All · Export · Export As · Export as PDF · Preview
in Web Browser · **Print Preview** · Send · Print · Printer Settings ·
**Properties** · **Digital Signatures** · Exit.

**OnlyOffice's File tab** [documented, `helpcenter.onlyoffice.com`]: Create ·
**Create from Template** · Open · **Open Recent** · Save · Save As · Export to
PDF · Print · **Protect** · **File Properties** · **Open File Location** ·
Advanced Settings · Help.

**Numbers' File menu** [documented, across `support.apple.com/guide/numbers`]:
New (⌘N, straight into the template chooser) · Open (⌘O) · Save (⌘S) · Save As
(⌥⇧⌘S) · Duplicate (⇧⌘S) · Export To ▸ · Reduce File Size · **Revert To ▸
Browse All Versions / Last Opened / Last Saved** · Print (⌘P) · Close (⌘W) /
Close All (⌥⌘W).

Ours, for comparison, is eight items and none of the bolded ones. §2.2 has it.

### 1.2 What users actually complain about, which is not what a matrix predicts

The brief asked for the first ten minutes rather than a feature list. The
strongest articulation found in the complaint corpus for LibreOffice and Numbers
migrations is not about a feature at all:

> *"the UX is just a lot worse, and it isn't easy to go from one application to
> another because they're slightly different enough that your productivity takes
> a hit from all the small papercuts."*
> [documented, `news.ycombinator.com/item?id=45744762`]

Two calibrations from the same corpus, both worth holding on to. **Performance
opinions form at about 2,000 rows, not at a million** — the loudest review found
reports a 20-second load and per-row scroll lag on a 2,000-row conditionally
formatted sheet [documented, `alternativeto.net`], which is two orders of
magnitude below this repository's stated target. And the complaints that end an
evaluation in the first session are **file fidelity on open, substituted fonts
reflowing the layout, scroll latency, a shortcut that does nothing, and a paste
that silently drops formatting** — pivot depth and Power Query are what people
write essays about afterwards. That ordering is the same one §2 uses, arrived at
independently.

---

## 2. What all of them have and we do not, ranked by when a user meets it

### 2.1 Before the application is even open

**(a) Double-clicking a spreadsheet will never open OpenCalc.** [tree]

`desktop/tauri.conf.json` has no `fileAssociations` key — the whole file is
seven top-level keys and the `bundle` block declares `active`, `targets`,
`icon`, `category`, `copyright`, `shortDescription`, `longDescription`, `macOS`
and `linux`, and nothing else. There is no `CFBundleDocumentTypes`, no exported
UTI, no Linux MIME declaration, and no argv or `RunEvent` handling in
`desktop/src/main.rs` for a path the OS would hand over on launch.

This is the first thing a user does and the last thing they will forgive. Every
one of the six registers at least `.xlsx` and `.csv` and appears in
"Open With" [unverified — this pass did not check any vendor's declared
association list, only that ours is empty; the claim is the weakest kind, an
appeal to what everybody knows]. Until this exists, a user's entire corpus of
files continues to open in whatever they were using before, and OpenCalc is
something they must remember to launch — which is the same as not having
installed it.

It also makes the rest of this section moot in practice: `Open Recent` matters
less when *nothing* arrives through the file system.

The archived row `UX-DESK-02` already recorded "file associations are
unconfigured" in its own not-done list, and it closed anyway. That is how a
known gap becomes an invisible one.

**(b) The first launch is a blank grid with no way in but the menu bar.** [tree]

`desktop/tauri.conf.json` opens one window straight onto
`editor.html?chrome=native`. There is no start screen, no document chooser, no
template picker, no recent list, no sign-in, no tour, and no licence prompt.

Half of that is a genuine advantage and §3 claims it. The other half is not:
Excel's Start screen, LibreOffice's Start Center and Numbers' template chooser
all answer the question *"what do I do now"* for a user who has just installed
something. What was verified this pass is the Office recent-files list — on by
default, per-application, pinnable, and synchronised across a signed-in user's
devices [documented, Microsoft Support, "Customize the list of recently used
files in Office apps" and "Open files from the File menu"]. The rest of the
first-run comparison is [unverified]. A blank grid answers the question only for
someone who already knows.

The detail worth copying is that Excel treats this as **a setting, not a
posture**: `File ▸ Options ▸ General ▸ Start up options ▸ "Show the Start screen
when this application starts"`, and unchecking it opens a blank workbook
directly [documented, Microsoft Support, "Customize how Excel starts"]. So the
choice we have made by default is one Excel offers — we simply offer no other.
The complaint in §2.4(s) is that there is nothing behind the door, not that the
door opens onto a grid.

**(c) The installers are unsigned, and the release notes teach the bypass.**
[tree]

Decided, not overlooked: `TAURI-008` records the product owner's decision of
2026-08-30 that alpha builds ship unsigned. `.github/workflows/release-desktop.yml`
carries the Gatekeeper and SmartScreen bypass text and offers `SHA256SUMS.txt`
as the only thing a user can check. This note does not reopen that decision. It
records the cost, because the cost lands here and not on the release: teaching
a user to run `xattr -dr com.apple.quarantine` is teaching them to disarm the
check that protects them from the *next* download.

### 2.2 The first minute — the File menu

The File menu is where a user looks for proof that this is an application. Ours,
verbatim from `webapp/editor.core.js:8804-8862` and mirrored onto the native
menu bar by `desktop/src/menu.rs`: [tree]

    New
    Open…
    Download ▸  (Same format as opened, xlsx, xlsm, ods, csv, tsv, psv)
    Share…      (hidden unless the host grants `canShare`)
    ─
    Page setup…
    Page break here
    Print…                Ctrl+P
    ─
    Properties…

**(d) There is no `Save` and no `Save As`.** [tree] No `["Save"` entry exists
anywhere in the `MENUS` literal. `Ctrl+S` is bound (`webapp/editor.core.js:7851`)
and in the desktop shell it writes back to the opened file through a temp file,
`sync_all` and a rename — real, careful work that closed as `SAVE-02`. And there
is **no menu item for it**. `Download ▸` is next to where `Save` should be and
is a different verb: it writes a copy.

This is worse than a missing feature, because the feature exists. A user who
does not try the keystroke concludes the application cannot save, and the
evidence in front of them supports that conclusion.

**(e) `Ctrl+S` is not in the keyboard help either.** [tree] `showShortcuts()`
(`webapp/editor.core.js:8777-8795`) is a hand-written fourteen-row table. It
lists `Ctrl+Shift+O` for "select cells with notes". It does not list `Ctrl+S`,
`Ctrl+P`, or anything under File. So the one surface that exists to tell a user
about invisible keystrokes omits the most important invisible keystroke in the
application.

**(f) There is no `Open Recent`.** [tree] No match for `openRecent`,
`open_recent`, `recentFile`, `recent_file`, `MRU`, `NSDocumentController` or
`jumpList` anywhere in `desktop/`, `webapp/` or `crates/`. The only file
identity the shell keeps is the current save target, held in memory for the life
of the window and never written to disk (`desktop/src/save.rs`).

All six have this, it is on by default in Office and pinnable [documented], and
on macOS and Windows the OS *also* surfaces it — the Dock menu, the taskbar jump
list, `File ▸ Open Recent` [documented for Office; [unverified] for the exact
Dock/jump-list behaviour of each]. It is the second-most-used item in a File
menu after Save. Being the only application without it is conspicuous in a way
that a missing chart subtype is not.

**(g) `Ctrl+O` and `Ctrl+N` do nothing.** [tree] Grep for the bindings finds
nothing; `File ▸ New` and `File ▸ Open…` carry no accelerator, so the native
menu shows none either. About fifty other chords *are* bound
(`webapp/editor.core.js:7610-8074`, and the inventory is genuinely deep —
`Ctrl+Shift+~!$%^#@` number formats, `End`-mode, `Alt+=`, `F4` anchor cycling,
`Ctrl+Alt+V`). The two that every single application on the machine binds are
the two that are missing.

**(h) On Windows and Linux there is no way to quit from a menu.** [tree]
`build_menu` (`desktop/src/main.rs:95-114`) adds the application submenu — About,
separator, Quit — only under `#[cfg(target_os = "macos")]`. Elsewhere the menu
bar is exactly the webview's File/Edit/View/Insert/Format/Data/Tools/Help, and
`File` ends at `Properties…`. Excel, LibreOffice, OnlyOffice and WPS all end
File with `Exit` [unverified as a set].

One detail that changes the fix rather than the finding: Tauri's
`PredefinedMenuItem::quit` is documented as **unsupported on Linux**
[documented, docs.rs], so a Linux `File ▸ Exit` has to be an ordinary item
calling `app.exit`. Copying the macOS block will produce a menu entry that does
nothing there.

### 2.3 The first ten minutes

**(i) Closing the window with unsaved work asks nothing.** [tree]

There is no `on_window_event`, no `WindowEvent::CloseRequested`, no
`prevent_close` anywhere in `desktop/src/` — the grep returns nothing. The only
defence is `beforeunload` (`webapp/editor.core.js:10040`), which is a browser-tab
mechanism; the native close button and the macOS `Quit` item
(`PredefinedMenuItem::quit`, `desktop/src/main.rs:104`) do not route through it.

The mitigation is real and should be stated: the draft autosave
(`webapp/editor.drafts.js`, `SAVE-03`) *is* enabled in the desktop shell — the
`desktop` preset sets `ownsFile: false` (`webapp/editor.core.js:934`) and
`initDrafts` only refuses when `ownsFile` is true or the editor is not the page
(`webapp/editor.drafts.js:936,943`). So work survives, in the WebView's
IndexedDB, and is offered back on next launch.

That is not the same as asking. Every one of the six prompts on close
[unverified as a set — not checked this pass]. What *was* verified is the shape
of the fallback they all also have, and it is instructive: Excel's AutoRecover
default interval is **10 minutes**, stated primarily rather than inferred —
*"Permissible values are integers from 1 to 120 minutes. The default value is 10
minutes"* [documented, `learn.microsoft.com`, `Excel.AutoRecover.Time`]. On
relaunch after a crash Excel presents the **Document Recovery** pane listing
what it holds, each entry carrying Open/View, Save As, Delete, Close and **Show
Repairs** [documented, Microsoft Support, "Recover your Office files"]. That is
a *second* line of defence behind the close prompt, not a replacement for it.

A recovery bar on the next launch is what you fall back on after a crash, not
what you offer instead of a question. And a user who does not relaunch, or who
relaunches after the WebView data directory has been cleared, has lost the work
with no dialog ever having appeared.

**This is the highest-severity item in this document**, because it is silent
data loss in the ordinary path, not the exceptional one.

**(j) Only one window, and no way to open a second document.** [tree]
`desktop/tauri.conf.json` declares one window; `desktop/src/main.rs` only ever
fetches `"main"`; there is no `WebviewWindowBuilder` anywhere. The shell's own
comment states the model: "One window, one of these"
(`desktop/src/main.rs:56`).

Comparing two workbooks side by side is a spreadsheet's most ordinary
multi-window task, and Excel has `View ▸ New Window`, `Arrange All` and
`View Side by Side` for exactly it [documented]. Numbers and LibreOffice are
document-per-window by construction [[unverified] for the current versions].
Opening a second file here replaces the first.

**(k) Dragging a file onto the window does nothing.** [tree] No
`dragDropEnabled` key, and no `DragDrop`/`FileDrop` handler in `desktop/`. The
editor page has no body-level file-drop handler either — the only one in the
repository is in `webapp/app.js`, which no HTML page loads. Tauri v2 defaults
`dragDropEnabled` to true, which means the shell *intercepts* the drop and then
does nothing with it, so the webview cannot handle it either.

**Two adjacent gestures are deliberately not being claimed here, because the
research says they are not conventions.** Dropping a CSV *into an open sheet* to
import it is not something Excel supports either — the documented paths are
double-click to open as a new workbook, or `Data ▸ Get & Transform` [documented,
Microsoft Support]. And dragging a range *out* to the desktop has no Windows
mechanism at all any more: Shell Scrap Objects (`.shs`) were removed in Vista as
a malware vector and were never replaced [documented, and corroborated by the
absence of any successor API]. So the gap is precisely **drop-a-file-on-the-
window-to-open-it**, which is universal, and not the two flashier gestures
around it.

**(l) The window forgets its size and position between launches.** [tree] The
1280×800 in `desktop/tauri.conf.json` is a constant; there is no
`tauri-plugin-window-state` and no persisted bounds.

On macOS this is a platform default we are opting out of rather than a feature
we have not added. **System Settings ▸ Desktop & Dock ▸ "Close windows when
quitting an application" ships *off***, which means quitting an application
remembers its open windows and reopens them at the same size and position
[documented, `support.apple.com/en-us/102318`]. A Mac user's expectation here is
not "nice if it did"; it is that every other application on the machine already
does. It is noticed every single launch.

**(m) On macOS the application menu is three items where the platform expects
seven, and the Edit menu is not the system's.** [tree]

`build_menu` starts from `Menu::new(app)` — empty — and appends About, a
separator and Quit. Tauri's own `Menu::default()` "creates a menu filled with
default menu items and submenus" [documented, docs.rs], and
`PredefinedMenuItem` already offers `copy`, `cut`, `paste`, `select_all`,
`undo`, `redo`, `services`, `hide`, `hide_others`, `show_all`, `minimize`,
`maximize`, `fullscreen`, `close_window` and `bring_all_to_front`
[documented, docs.rs]. **None of them is used.** Apple's guidance is that an app
includes App, File, Edit, View, **Window** and Help, and that the App menu holds
About, Settings (⌘,), Services, Hide, Hide Others, Show All and Quit
[documented, Apple Human Interface Guidelines — read via a secondary summary
this pass, because the HIG page renders client-side and could not be fetched
directly].

Missing, concretely: **Settings/Preferences at ⌘,** — Settings exists only as an
HTML dialog under `Tools ▸ Settings…` with no accelerator; **Services**, which
is how a macOS user sends a selection to another application; **Hide/Hide
Others**; and the entire **Window** menu, so there is no Minimize, no Zoom, no
window list.

**(n) A reasoned consequence of (m), and the single thing most worth running:
⌘C in a text field may copy the wrong thing.** [unverified]

The Edit menu's Cut/Copy/Paste are ordinary `MenuItem::with_id` entries whose
accelerators become `CmdOrCtrl+X/C/V` (`desktop/src/menu.rs`), dispatching
`runCommand("edit.copy")` into the webview. Those handlers are `doCopy`/`doCut`/
`doPaste` (`webapp/editor.clipboard.js:90-140`), and every one of them operates
on `effectiveRange()` — the **grid selection** — with no check for what has
focus. [tree, for all of that.]

In a browser tab this cannot bite: `Ctrl+C` is bound on the canvas keydown
handler, so a focused `<input>` never reaches it. In the desktop shell a native
menu accelerator is consumed by the menu *before* the webview sees the key. So
the prediction is that ⌘C with the caret in the formula bar, the Name Box or a
dialog field copies the cell range instead of the selected text — and ⌘V pastes
a range into the sheet instead of text into the field.

**This is a prediction from reading, not a measurement**, and it is exactly the
class of thing this repository has been wrong about in both directions. What
settles it is one macOS build and four keystrokes. If it reproduces it is a P1
correctness defect, not a polish item, and the fix is the standard predefined
Edit items plus a focus check in the three handlers.

**(o) ⌘T, which a Mac Excel user presses constantly, opens the wrong dialog —
and interrupts the formula they were typing to do it.** [tree, both halves]

**Excel for Mac overloads ⌘T deliberately, and disambiguates it by mode.**
Microsoft's own Mac shortcut table gives ⌘T for *both* "cycle absolute/relative
references" and "create Table"; which one fires depends on whether a formula is
being edited [documented, `support.microsoft.com` — "Keyboard shortcuts in
Excel", Mac tab, verified directly this pass]. It is one of the few chords a
spreadsheet user types dozens of times an hour.

We took half of that pair and none of the disambiguation. `Insert ▸ Table…`
carries the accelerator `Ctrl+T` (`webapp/editor.core.js:8951`), the grid handler
binds `k === "t"` to `tableDialog()` under `mod = e.ctrlKey || e.metaKey`
(`webapp/editor.core.js:7679`), and `desktop/src/menu.rs` translates the label to
`CmdOrCtrl+T`. Anchor cycling is on `F4` only (`webapp/editor.core.js:8040` →
`cycleAnchors`) — right for Windows, and on a Mac laptop reachable only with
`fn` unless the user has changed a system setting.

**The desktop shell then removes the one thing that could have saved it.** A
native menu accelerator is consumed by the menu bar *before the webview sees the
key*, so ⌘T fires even while a cell is being edited — exactly the mode in which
Excel means the *other* thing. In a browser tab the keystroke would at least
reach the inline editor. Here the Mac Excel user's most-typed chord opens a
modal over a half-typed formula.

This is the concrete form of the general problem: there is **no platform
keyboard idiom anywhere in the editor**. `mod = e.ctrlKey || e.metaKey`
(`webapp/editor.core.js:7630`) folds ⌘ into Ctrl for every chord, there is no
`navigator.platform`/`isMac` test in `webapp/` at all, no ⌘ glyph outside one
code comment, and every accelerator label in the menus and in the help panel is
the Windows string.

Folding the modifier is a defensible shortcut. Assuming the *chords* are the
same is not, and Microsoft's own table is the evidence — on Mac Excel, insert-date
is **Control+;** while insert-time is **⌘;** (the modifiers differ within one
pair), and AutoSum is **⇧⌘T** against Windows' **Alt+=**, which is not a
translation of anything [documented, same page]. Windows' ribbon **KeyTips**
(press `Alt`, then letters) have no automatic Mac equivalent at all: Excel for
Mac ships an opt-in KeyTips activation setting that is **disabled by default**
[documented for the Windows behaviour; the Mac setting's exact wording
unverified].

**(p) No spell check, anywhere, and it is switched off where it would matter
most.** [tree] Every occurrence of `spellcheck` in `webapp/` sets it to
`false` — the inline cell editor (`webapp/editor.html:450`), the formula bar
(`:412`), and the **comment box** (`webapp/editor.core.js:5051`). Turning it off
on a formula field is right. Turning it off on the field where a user writes an
English sentence is not, and it is the one place the browser would have given us
the platform dictionary for free. Excel has `F7`; LibreOffice checks as you type
by default [[unverified] for the current defaults].

**(q) Print goes through a popup window, and there is no preview.** [tree]
`printSheet()` (`webapp/editor.sheets.js:281-296`) calls `window.open` and then
`print()`. Inside a native window that is a browser artefact: it depends on a
popup succeeding, and what a user gets is the WebView's print sheet rather than
the application's. There is no print preview, no Page Layout view and no
page-break preview — [12](12-COMPETITIVE-ANALYSIS.md) §3.17 already says so and
is still right. Page Setup itself is complete and better than Google Sheets'
(`webapp/editor.dialogs.js:1195-1324`) — [12](12-COMPETITIVE-ANALYSIS.md) §3.17
says so and this pass did not find otherwise. What Excel's **Sheet** tab has
that ours does not, now that it has been read verbatim [documented,
`support.microsoft.com`, "Page Setup"]: **Columns to repeat at left** (we have
the rows half only), a **comments and notes** dropdown ("At end of sheet" / "As
displayed on sheet"), a **cell errors as** dropdown (as displayed / blank / `--`
/ `#N/A`), **page order** (down-then-over vs over-then-down), **black and
white**, **draft quality**, and **first page number**. Each is a checkbox-sized
item; together they are the difference between a page-setup dialog and Excel's.

Worth noting from the other direction: LibreOffice's Sheet tab offers **print
formulas** and **print zero values**, which Excel has no equivalent for
[documented, `help.libreoffice.org`]. Parity with Excel is not the ceiling here.

**(r) PDF export exists and the application cannot reach it.** [tree]
`IO-03` is `Partial`: `crates/casual-calc-render/src/pdf.rs` is a real paginator
and writer, verified against poppler. `IO-10` names the residue, and one clause
of it is the one that matters here — it is "not wired to the WASM bridge, the
editor, the desktop shell or the server". So the desktop release notes say
"Notably absent: version history, and headers and footers in PDF export", which
reads as though PDF export is present with a caveat. From the application, there
is no route to it at all. **That is a document stating a contract the code does
not keep, and under this repository's own rule it is a row rather than a
reworded sentence.**

### 2.4 The first week

**(s) No templates.** [tree] No template gallery, no `New from template`, no
`.xltx` — `xltx` occurs nowhere, and `CANDIDATE_EXTENSIONS`
(`crates/casual-calc-wasm/src/io.rs`) is xlsx, xlsm, ods, csv, tsv, tab, psv.
`File ▸ New` seeds a demo workbook. All six ship a template chooser
[[unverified] as a set]. There is a Cell Styles gallery
(`webapp/editor.dialogs.js:319`), which is a different thing.

**(t) No update mechanism.** [tree] No `tauri-plugin-updater`, no `plugins`
block in `tauri.conf.json`, no endpoint, no signing key. An alpha that cannot
update itself is an alpha that stays at the version it was downloaded at.
`TAURI-006` owns this.

**(u) English only, and the shell cannot be translated at all.** [tree]

This is the subject [12](12-COMPETITIVE-ANALYSIS.md) is silent on — it has no
occurrence of localisation, i18n, translation or RTL. The state is more
interesting than "absent":

- The **editor** has a working message layer: `t(key, fallback)`,
  `setMessages`, a locale `<select>`, and `relabel()` re-deriving the menu-bar
  mnemonics from translated labels (`webapp/editor.i18n.js`). Menu items and
  tooltips all route through it. This is real infrastructure and it closed as
  `SDK-005`.
- **No catalogue ships.** `availableLocales()` is `["en-US", ...messages.keys()]`
  and nothing populates `messages`; the picker is `hidden` until a host turns it
  on. So the mechanism exists for an embedding host and is inert for our own
  application, which has no host.
- The **Rust shell** has no message layer at all. `"Open"`, `"Save As"`,
  `"OpenCalc"`, `"Untitled"` and every `SaveError` string are English literals
  in `desktop/src/`.
- **RTL is not just untranslated, it is refused**: `webapp/editor.css:59` sets
  `direction: ltr` on the root, and there are no logical properties and no
  `[dir]` selectors.
- **Complex scripts do not render in the native backend.** `P1C-003` records
  that the bundled families cover Latin and Hebrew only — no Arabic,
  Devanagari, Thai or CJK. On screen this does not bite, because shaping is off
  in the WebAssembly build by decision (ADR-018) and the browser shapes the
  text; it bites in anything drawn natively.

Two things a localised spreadsheet needs that are *not* in the tree and are not
in that list either, both [tree]: the formula parser accepts only `,` as an
argument separator (`crates/casual-calc-formula/src/lex.rs`), and there is no
localised function-name layer. A German or French user typing `=SUMME(A1;A5)`
— or `1,5` — gets an error. Excel and LibreOffice both accept the locale's
separator and localised names and translate on save [documented for LibreOffice's
ODS formula namespace, which the repository's own
`crates/casual-calc-ods/src/lib.rs` already reasons about; [unverified] for
Excel's current behaviour]. This is the deepest gap in this document and the
only one that reaches the engine.

**(v) No accessibility story above the webview.** [tree, then [unverified]]

The grid accessibility is genuinely good and [12](12-COMPETITIVE-ANALYSIS.md)
§4.6 measures it: a DOM mirror with absolute `aria-rowindex`, a live region, a
roving-tabindex menubar, `prefers-contrast` and `prefers-reduced-motion`
handling. That is better than the category.

What is unknown is whether any of it survives the desktop shell. There is no
`accesskit` in `desktop/`, and the shell cannot add one — `unsafe_code =
"forbid"` rules out the raw pointer work. The ARIA tree lives in a WKWebView /
WebView2 / WebKitGTK, all three of which do bridge ARIA to the platform
accessibility API in principle. **Nobody has run a screen reader against the
desktop build**, so the honest statement is: the web build is measured, the
desktop build is assumed. There is also no `forced-colors` block anywhere in
`webapp/`, so Windows High Contrast Mode is unhandled [tree].

An enterprise or government evaluation asks for a VPAT against Section 508 /
EN 301 549 and expects an Accessibility Checker and alt text on objects
[[unverified]; this pass did not verify current procurement requirements]. We
have none of those artefacts.

---

## 3. What we have that they do not

Short, and honest about it. [12](12-COMPETITIVE-ANALYSIS.md) §5 already lists
ten general advantages and they are not repeated. These five are specific to
being a *desktop application*, and every claim about our own side is [tree]:

1. **The native menu bar cannot drift from the in-app menu, because there is
   only one definition.** `desktop/src/menu.rs` holds no menu: the webview hands
   over `menuModel()` and the shell translates presentation only, including
   `Ctrl` → `CmdOrCtrl`. Every competitor with a web and a desktop build
   maintains two menu definitions. The property is worth naming because §2's
   fixes must not break it — an `Open Recent` submenu is the first thing that
   will want to be defined natively, and it should be defined in the editor and
   published like everything else.

2. **The webview cannot ask the host to touch an arbitrary path.** Bytes cross
   the bridge, never paths (`UX-DESK-02`); `Session` is default-deny until the
   webview reports capabilities, and `ownsFile` forces `canOpen` off on the Rust
   side because a report can be stale or forged (`desktop/src/session.rs`). For
   an application whose whole job is opening untrusted files this is a stronger
   boundary than the category's, and it is why we need no equivalent of Excel's
   Protected View [documented] — nothing in a file can execute at all.

3. **Genuinely, verifiably offline, with no licence that can lapse.**
   `frontendDist` embeds `webapp/` at compile time; the engine is WebAssembly
   loaded from an embedded asset URL; open and save go through `std::fs`. The
   only two network paths are the collaboration socket, which requires an
   explicit call, and a font service behind a `?fonts` query parameter the
   desktop URL does not set. No account, no telemetry, no licence check, no
   first-run sign-in.

   The comparison is sharper than "we work offline too". Microsoft 365 that
   cannot verify its subscription for *"an extended period of time (usually
   around 30 days)"* enters **reduced functionality mode**, in which users
   *"will be able to open and print your documents but you won't be able to edit
   them or to create new ones"* [documented, Microsoft Support]. Google Sheets
   offline requires Chrome or Edge specifically, an installed extension, no
   private browsing, one account per browser profile, and can be switched off
   entirely by an administrator [documented, `support.google.com/docs/answer/6388102`,
   verified directly this pass]. LibreOffice is the one competitor whose offline
   position genuinely matches ours, and its first run is as clean as ours — a
   dismissible Tip of the Day, no registration, no telemetry consent
   [documented, `help.libreoffice.org`]. **So this advantage is real against
   four of the six and shared with the fifth**, and §2.1(b) should not be read
   as claiming otherwise.

4. **Crash recovery that is strictly better than Excel's for a local file, and
   is offered rather than applied.** The draft is written to IndexedDB on a 5 s
   quiesce / 60 s ceiling, only when the edit counter has moved, and is
   *presented* on next launch rather than restored (`webapp/editor.drafts.js`).

   The comparison is the surprising part. **Excel's AutoSave is cloud-only** —
   *"AutoSave is enabled by default in Microsoft 365 when a file is stored on
   OneDrive, OneDrive for Business, or SharePoint Online… If the file is saved
   to another location… then AutoSave is disabled"* [documented, Microsoft
   Support, "What is AutoSave?", verified directly this pass]. A local `.xlsx`
   in Excel therefore has **no autosave at all** — only the 10-minute
   AutoRecover snapshot of §2.3(i). Ours quiesces at five seconds. That is a
   genuine, defensible lead on the exact axis a spreadsheet is trusted for, and
   it makes §2.3(i) more galling rather than less: the durability is already
   better than the incumbent's and the *question on close* is what is missing.

5. **`File ▸ Properties…` exists, and Numbers has no equivalent at all.**
   `UX-META-01` built a properties dialog that reads and writes OOXML core
   properties. Numbers exposes no document-properties surface [documented by
   absence — no Apple page for one was found this pass, which for a product
   with per-feature guide pages is weak evidence of presence]. A small thing,
   and the only place in this document where we are ahead on File-menu surface.

---

## 4. Which of these are already tracked

Checked against `docs/14`, `docs/14a`, `docs/53` and `docs/67`. **Do not read a
row id here as scope**: several of these rows own the subject and not the item.

### Already tracked — cite the row, do not file a new one

| Item | Row | Status | Note |
| --- | --- | --- | --- |
| 2.1(c) unsigned installers | `TAURI-008` | Open | Decided: no signing for alpha |
| 2.4(s) auto-update, desktop settings, per-OS behaviour | `TAURI-006` | Open | Explicitly bundles six pieces including the updater and a Settings entry in the native menu |
| Release artifacts and installer formats | `TAURI-007` | Open | **Its description is stale**: it says there is no `release-desktop.yml`, and `.github/workflows/release-desktop.yml` exists and builds `.deb`, `.AppImage`, `.dmg` and NSIS |
| The shell overall | `TAURI-001` | Partial | |
| 2.3(p)/(q) PDF and print residue | `IO-03` Partial, `IO-10` Open | | `IO-10` already contains "not wired to … the desktop shell" |
| Open with no filename, format sniffing | `IO-09` | Open | The row an association-launched or dropped file would land on |
| 2.4(t) complex-script coverage | `P1C-003` | Partial | Font decision, not a shaper |
| Save/autosave/history as one design | `SAVE-05` Designed, `SAVE-08`, `SAVE-09`, `HIST-01`, `HIST-02` | | Version history is out of this note's scope |
| Digital signatures on documents | `SIGN-01` | Open | |
| Desktop chrome placement defects | `UX-CHR-01`, `UX-DESK-04`, `UX-DESK-05` | Open | Cosmetic relative to §2 |

### Genuinely new — nothing in any tracker mentions them

Grep across all four trackers finds no row for any of these:

**recent files** · **file associations and "Open With"** · **drag-and-drop of a
file onto the window** · **multiple windows** · **first run / start screen** ·
**templates** · **spell check** · **close-with-unsaved-work on the native
window** · **`Save`/`Save As`/`Ctrl+O`/`Ctrl+N` missing from the menu and the
help panel** · **the macOS application and Window menus** · **the macOS keyboard
idiom, including the ⌘T collision** · **the ⌘C-in-a-text-field prediction** ·
**window state persistence** · **single-instance** ·
**localisation of the application UI, RTL, and the formula argument separator** ·
**print preview / page-layout view** · **repeat-columns-at-left** · **the release
notes' PDF claim**

`SDK-005` (Done) is the closest thing to a localisation row and is a different
claim: host-supplied catalogues for an *embedded* editor, which is precisely the
mechanism §2.4(u) says exists and is inert for our own application.

---

## 5. What each new gap costs, and what it depends on

Cost is stated as a shape, not a number. "Small" means one file and its test;
"medium" means a design decision first; "large" means it changes something
structural.

| Gap | Cost | Depends on |
| --- | --- | --- |
| Close-with-unsaved-work prompt | **Small.** `on_window_event` + `prevent_close`, asking the webview whether it is dirty (the shell already polls `isDirty()` every 250 ms for the title bar) and raising a native three-button dialog through `tauri-plugin-dialog`, which is already a dependency | Nothing. This should not wait for anything on this list |
| `File ▸ Save` and `Save As…` menu items; `Ctrl+O`, `Ctrl+N`; the help panel | **Small.** Entries in the `MENUS` literal and two key bindings; they publish to the native bar for free | Deciding what `Save` does in a browser tab, where there is no target — `SAVE-04` (File System Access API) is the answer and `Save` must not read as `Download` before it lands |
| macOS application menu, Window menu, standard Edit items | **Small–medium.** Every piece is a `PredefinedMenuItem` Tauri already ships; the medium part is that this is the **first native-only menu content**, so it must not become a second menu definition — §3(1) is the property to preserve | The `Settings…` entry belongs to `TAURI-006`, which already claims it |
| ⌘C-in-a-text-field | **Small if it reproduces.** Standard predefined Edit items plus a focus check in `doCopy`/`doCut`/`doPaste` | A macOS build. **Run it before designing it** |
| macOS keyboard idiom: ⌘T for anchors, and the accelerator labels | **Medium, because it is a decision and not a table.** Either the chords are per-platform — in which case the menu model needs a platform axis, and §3(1)'s single-definition property has to survive it — or they are not, in which case say so in the help panel rather than leaving a Mac user to discover it on a modal over their formula | Nothing technical. It needs the answer to "how much may the two hosts diverge", which [12](12-COMPETITIVE-ANALYSIS.md) §10.2 already names as an architecture question |
| Window state persistence | **Small.** `tauri-plugin-window-state`, or persist bounds ourselves | A settings store, which `TAURI-006` is designing. Do not invent a second one |
| Drag-and-drop a file onto the window | **Small.** Set `dragDropEnabled` deliberately and handle `DragDrop` in Rust, reusing the `native_open` path — bytes across the bridge, not the path | The same "which extensions" answer `openable_extensions()` already gives; `IO-09` for a file whose extension lies |
| Recent files | **Medium.** Needs a persisted store, a privacy answer (a recent list is a record of what a user opened), a pin/clear affordance, and — because of §3(1) — a way for the *editor* to own a submenu whose contents the shell supplies | A settings/profile store: `TAURI-006`. Should be designed with it, not before it |
| File associations and "Open With" | **Medium, and the ceiling is lower than it looks.** The declaration is small; the launch path is not — argv on Windows/Linux, `RunEvent::Opened` on macOS, and the already-running case. Two documented constraints shape it: **Windows will not let an application make itself the default** (*"Windows does not allow programmatic changes to default apps without user interaction in system UI"*, enforced by the `UCPD.sys` filter driver), so the realistic outcome is appearing in "Open With" and in Settings, never claiming `.xlsx` on install; and on macOS the split between `UTExportedTypeDeclarations` (types we own) and `UTImportedTypeDeclarations` (types we merely consume) has to be right — Collabora shipped a fix for exactly that mistake, and getting it backwards claims OOXML as ours | Single-instance; and a decision on multiple windows, because "already running" has no good answer while there is one window |
| Single instance | **Medium.** A plugin, plus routing the second launch's file into the first process | Multiple windows. Also: two processes today share one WebView data directory, so the draft store's `BroadcastChannel` coordination does not span them — **[unverified], and worth measuring before anything makes two processes likely** |
| Multiple windows | **Large.** One `WorkbookSession` per window, one draft slot per window, one save target per window, and the menu model becomes per-window | The largest single dependency in this table. It gates associations-while-running and single-instance |
| First run / start screen / templates | **Medium**, and they are one piece of work: the screen that has no document open is the screen that offers recent files and templates | Recent files. A template format decision (`.xltx` vs an `.xlsx` marked as a template) |
| Spell check | **Medium.** The cheap 80% is deleting `spellcheck = false` on the comment box and the inline cell editor and letting the WebView's dictionary work; the expensive part is a sheet-wide `F7` pass with a proper dialog | Nothing for the cheap part |
| Print preview / page-layout view | **Medium–large**, and it changes shape once PDF is wired: a preview is a rendering of the paginated document, and `crates/casual-calc-render/src/pdf.rs` now paginates | `IO-10`. Do not build a third pagination path |
| Wiring PDF export to the application | **Small–medium**, and it is already inside `IO-10` | `IO-10` |
| Repeat-columns-at-left | **Small.** The rows equivalent is already wired | Nothing |
| Application localisation: ship catalogues, translate the shell, RTL | **Large.** Three separate pieces — a catalogue format and at least one real language; a message layer in `desktop/src/`; and RTL, which is a layout change and not a string change | A decision this note cannot make (see §7) |
| Locale-aware formula input: argument separator and localised function names | **Large, and it reaches the engine.** Parse and *display* both have to move, and a file must round-trip through the canonical form | The same decision. This is the one that changes crate-level contracts, so it should be decided before it is scheduled, not after |
| Native accessibility verification | **Small to measure, unknown to fix.** Run VoiceOver, NVDA and Narrator against a desktop build and write down what happens | A build on each platform |
| `forced-colors` support for Windows High Contrast | **Small.** A media block beside the existing `prefers-contrast` one | Nothing |
| The release notes' PDF claim | **Trivial**, and it is a row rather than an edit by this repository's own rule | Whether PDF gets wired first, in which case the sentence becomes true |

---

## 6. What this deliberately excludes, and why

- **Macros, VBA, Apps Script, add-in marketplaces.** Refused by design —
  `AGENTS.md` states "no macro execution" as a security bound, and
  [12](12-COMPETITIVE-ANALYSIS.md) §8 item 7 already ranks the absence. A note
  about desktop conventions should not relitigate a security decision.
- **Everything [12](12-COMPETITIVE-ANALYSIS.md) already covers**: formula and
  function parity, chart types and subtypes, pivot depth, Flash Fill, Goal Seek,
  pictures and shapes, comment threading, find-and-replace residuals, mobile and
  touch. This note repeats none of it. Where the two overlap — print, PDF,
  accessibility — this note says only what the desktop dimension adds.
- **Collaboration and version history.** Decided and built or designed
  (ADR-011 through ADR-017, `SAVE-05`, `SAVE-08`), and not a desktop-convention
  question.
- **Shell integrations beyond the launch path**: macOS Quick Look previews and
  Spotlight indexing, the Windows Explorer preview handler and Search iFilter,
  the Share sheet, OLE embedding. All real conventions; all of them presuppose
  file associations, and none is met in the first ten minutes. They belong in a
  later pass, and are named here so their absence is a decision rather than an
  oversight. One sizing note for whoever takes it: a Windows `IFilter` must be a
  **native in-process COM server**, since managed-code filters have been blocked
  since Windows 7, and it runs sandboxed in `SearchFilterHost.exe` with no disk,
  network or UI access and a 100 MB working-set cap [documented,
  `learn.microsoft.com`]. That is a real piece of work, not a manifest entry.
- **Dragging a range out of the application** as a desktop clipping. Excluded
  because it is no longer a convention rather than because it is hard: the
  Windows mechanism was withdrawn in Vista (§2.3(k)).
- **Cloud storage integration** (OneDrive, Drive, iCloud). Against the
  self-hosting position in [12](12-COMPETITIVE-ANALYSIS.md) §5 item 7, and a
  product question rather than a convention.
- **Effort estimates in hours.** [12](12-COMPETITIVE-ANALYSIS.md) §3.17 records
  what happened the last time this repository costed work from a premise it had
  not checked (`IO-06`), and the premise there was smaller than most of §5's.

---

## 7. Questions that are the product owner's

Three, and none of them has a defensible engineering-only answer:

1. **Is one window a decision or an omission?** §5 shows it gating file
   associations-while-running, single instance, and the start screen. A
   document-per-window application is a different application from a
   single-window one, and choosing late is the expensive way to choose.

2. **Which languages, and does the formula language move with them?** Shipping a
   translated UI is a medium job. Accepting `=SUMME(A1;A5)` and `1,5` is a large
   one that reaches the parser, the display layer and the round-trip contract.

   **The prior art says these are separable, and all three majors separate
   them** [documented]. LibreOffice has a **"Use English function names"**
   checkbox — *"function names can be localized. By default, the check box is
   off, which means the localized function names are used"* — with independently
   configurable parameter (`;`), array-column (`;`) and array-row (`|`)
   separators. Excel keys the separator off the OS regional list separator, with
   `Application.UseSystemSeparators` defaulting to true and a manual override in
   Advanced options. Google Sheets makes locale (`File ▸ Settings ▸ Locale`,
   which sets currency, date and number formatting) explicitly independent of UI
   language, and offers its own "always use English function names" option.

   So the question is not "do they come together" — the industry answer is that
   they are two settings. It is **which of the three axes we commit to first**:
   UI strings, regional input/display formatting, or function names. A
   translated UI with an English formula language is a coherent shipped
   position, and it is much cheaper than the other order.

3. **Is an accessibility conformance artefact needed, and by when?** The grid
   work is already better than the category, and this is the one question with a
   hard external clock on it [documented]:
   - **Section 508** incorporates **WCAG 2.0 Level AA** by reference — not 2.1.
   - **EN 301 549 V3.2.1** (the current harmonised European standard)
     incorporates **WCAG 2.1 Level AA**. **V4.1.1**, expected to incorporate
     **WCAG 2.2 AA**, is anticipated in the EU Official Journal around
     **October 2026**.
   - The **European Accessibility Act** deadline for new products was
     **28 June 2025**; products already on the market have until 2030.
   - Microsoft, Google and Apple all publish per-product **ACRs/VPATs**; Google
     publishes one specific to Google Sheets.

   So a procurement conversation will ask for an ACR naming Section 508 and
   EN 301 549, and the goalposts move in about a year. A VPAT, an Accessibility
   Checker (Excel's spreadsheet-specific rules are concrete and copyable — alt
   text, table header rows, meaningful sheet-tab names, and *"cells don't use
   red-only formatting for negative numbers"*) and alt text on chart and image
   objects are the artefacts. **Whether any of it matters depends entirely on
   who the desktop build is being sold to, and nobody has said.**

---

## 8. How this was checked, and what would refute it

**The repository half was run and read.** Every [tree] claim above cites a line
in this checkout at `1be6a01`. Negative claims name the patterns searched.
`desktop/tauri.conf.json`, `desktop/src/main.rs`, `desktop/src/menu.rs`,
`desktop/Cargo.toml`, `webapp/editor.core.js`'s `MENUS` literal and
`showShortcuts`, `webapp/editor.clipboard.js`, `webapp/editor.drafts.js` and
`webapp/editor.i18n.js` were read directly rather than summarised.

**The application was not run.** No desktop build was produced during this pass
— `cargo-tauri` is not installed here — so nothing above is a measurement of the
shipped application. The three items most likely to be wrong for that reason,
and how to settle each:

- **§2.3(n), ⌘C in a text field.** Build on macOS, click into the formula bar,
  select a word, press ⌘C. If it copies the word, the prediction is wrong and
  something in the accelerator path is doing more than this note found.
- **§2.3(i), close with unsaved work.** Edit a cell, click the window's close
  button. If a dialog appears, `beforeunload` is reaching the native close after
  all and the item is wrong.
- **§2.4(v), screen readers.** Run VoiceOver against the built app. The ARIA
  mirror may bridge through the WebView perfectly, in which case the desktop a11y
  concern reduces to the native menu and the Window menu.

**The competitors were not run — they were read.** About 200 searches and 150
page fetches against vendor documentation stand behind the [documented] marks,
and the ones that matter most were fetched directly rather than summarised:
Excel's AutoRecover default (`learn.microsoft.com`, `Excel.AutoRecover.Time`),
the AutoSave-is-cloud-only statement, the Mac keyboard-shortcut table, Excel's
Page Setup Sheet tab, the header/footer field-code table, LibreOffice's Calc
Formula options page, Google's offline requirements, and the Windows default-apps
platform policy.

**Where the reading was thin, and it is thin in named places.** Reddit,
LibreOffice's Bugzilla and ask-site, and the review aggregators all refused
automated fetches, so §1.2's complaint corpus is solid on its main themes and
absent on the long tail — notably **there are no accessibility complaints about
any competitor in it**, which is a gap in the search rather than a finding.
Several Microsoft support URLs for the Paste Special and Copy-as-Picture dialogs
returned 404, so those are cited through the VBA enumerations instead. And two
things could not be settled either way: what each of the six actually declares as
file associations, and the current first-run flows for Numbers and WPS.

**One contradiction inside a vendor's own documentation, recorded rather than
resolved**: Microsoft's agile-encryption page states the default hash as SHA-2 in
its prose and `SHA1` in its own settings table. If OOXML encryption is ever
implemented here, both need checking against a real file rather than either
sentence.

**What would make this document as a whole wrong** is a single thing: if
`docs/14`'s `TAURI-006` turns out to already contain the File-menu and
application-menu work under "chrome and per-OS behaviour". It was read and it
does not — it names a profile, settings, chrome, per-OS behaviour, builds and
auto-update, and a Settings entry in the native menu. But it is the row a reader
should open before acting on §4, because a design that has been written since
this pass would change the answer.
