# 86 — Shipping the desktop app: identity, settings, chrome, platforms, builds and updates

**Status: proposed.** Nothing in §4 through §9 is built, and §1.5 is a defect
that is open rather than fixed. §1, §2 and §3 are measured against this worktree
at `52d2d44`, and every path, line number and quoted string in them is
reproducible by the commands in §13.

**Why this exists.** [44](44-TAURI-DESKTOP-SHELL-DESIGN.md) and
[81](81-DESKTOP-SHELL-COMPOSITION.md) settled *how* the desktop app is put
together — a binary on `casual-calc-sdk` and Tauri, in its own Cargo workspace,
with no crate between (`ADR-023`). `TAURI-005` gave it a window, `TAURI-004` the
operating system's menu bar, `TAURI-003` native calc, `SAVE-02` in-place save
and `UX-DESK-01` a window that no longer wears a web page's header. What none of
them decided is what it takes to hand the thing to a person: who the user *is*,
where their preferences live, what the window still gets wrong, what the three
operating systems do differently, what a release contains that a pull request's
build does not, and what happens the day after the release.

Six questions, one note, because they share a shell and a release. They are
**built** separately, and §10 says in what order and why.

**Relates to** `TAURI-001` (the row that stays `Partial`), `TAURI-003`/`-004`/
`-005`, `UX-DESK-01` (the chrome this starts from), `UX-DESK-04` and `UX-DESK-05`
(open chrome defects this does not fix), `SAVE-02` and `SAVE-05` ([83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md)),
`COL-40` and [72](72-SESSION-ACCESS-CONTROL.md) (whose identity rule decides §4),
`COL-46`/`COL-50` (what a shared session still risks), `CI-014` (the hazard a
second workspace carries), `DEP-07` (the release pattern already used once),
`ADR-019`/[78](78-HOST-CAPABILITY-SEAMS.md) (capability per seam), and
`ADR-023`/[81](81-DESKTOP-SHELL-COMPOSITION.md), which this note extends rather
than reopens.

---

## 1. Six things checking changed

The brief this note answers carries six premises. Four are exactly right, one is
right but incomplete in a way that changes the design, and one names rows that do
not exist. Each is stated before the design, because three of them would have
produced a worse plan. §1.5 is not the brief's at all: it is a defect found by
checking a claim **this note itself made**, and it is the reason §4 has a
consumer rather than a placeholder.

### 1.1 The three rows this note was written for do not exist

The brief asks for a design note against three TAURI rows numbered above five,
and against their siblings. Grepping the trackers for them returns nothing: the
series runs `001` through `005`, `TAURI-002` through `TAURI-005` are closed and
live in [14a](14a-ARCHIVE-CLOSED-WORK.md), and `TAURI-001` is the only live one.

This is not a quibble about numbering. `tools/check-doc-references.py` rule 3
holds every document to *"a tracker id is defined by a tracker row"*, so a
design note that cited its own not-yet-filed rows would turn `main` red the
moment it was committed — the exact class of failure that gate exists for. So
this note **names no new id anywhere in its prose.** §10 describes the rows it
asks for by their content, and the ids are assigned when the rows are filed.

### 1.2 The profile is not the prerequisite for desktop collaboration. The **token** is

The brief says a profile is *"a prerequisite for collaboration working on
desktop at all"*. Measured, the prerequisite is one layer down and points the
other way.

[72](72-SESSION-ACCESS-CONTROL.md) states the rule as a security property:
*"Identity is the host's, never the client's. The name and colour a participant
is shown under come from the token. The editor has no way to set its own, and
must not acquire one."* The code says the same thing twice more — `presence.rs`
comments its `name` field *"Display name, from the token"*, and `webapp/collab.js`
says a presence message *"carries no identity — the server takes that from the
token, because presence is the one surface where a claimed name would be
believed."*

So a desktop profile name **cannot** reach `who.name`, and a design that made it
do so would be deleting a security rule three files independently keep. What is
actually missing on desktop is that `MODE_PRESETS.desktop` carries
`canShare: true` (`webapp/editor.core.js:934`) while the desktop user has no
issuer to get a token from. §4 decides that, and it decides it *without* a
profile.

### 1.3 "File associations" is not a setting; it is a bundle field that does not exist

The brief lists file associations among the things a desktop settings surface
should carry. Two measurements move it out of Settings entirely:

* `desktop/tauri.conf.json` has no `bundle.fileAssociations`, and
  `desktop/src/main.rs` never reads `std::env::args()` nor handles a
  `RunEvent::Opened`. Nothing in the binary can open a file it was launched
  with. Registering the association today would make a double-clicked `.xlsx`
  open a **blank window** — strictly worse than not registering it.
* Where the association is declared is a *build* decision on all three
  platforms: `Info.plist` `CFBundleDocumentTypes` baked at bundle time on
  macOS, registry keys written by the NSIS installer on Windows, a `.desktop`
  MIME entry on Linux. None of them is a runtime toggle.

And the one runtime part — *make OpenCalc the default for `.xlsx`* — cannot be
done programmatically on Windows 10 or later at all; the user must go through
the operating system's own Default Apps page. §5 therefore refuses a "file
associations" checkbox and says what replaces it.

### 1.4 The native menu bar is drawn on **all three** platforms, not only macOS

`desktop/src/main.rs:150` calls `app.set_menu(menu)`. Tauri 2.11.5 documents
that method as: *"Sets the app-wide menu and returns the previous one. If a
window was not created with an explicit menu or had one set explicitly, this
menu will be assigned to it."* (`AppHandle::set_menu`, `tauri-2.11.5/src/app.rs:956`).
`desktop/tauri.conf.json`'s window is not created with an explicit menu, so it
receives this one.

That means macOS gets the global bar at the top of the screen and Windows and
Linux get a native menu bar **inside the window** — and `editor.css:1901`'s
`.oc-chrome-native #menubar { display: none }` is therefore correct on all
three rather than only on macOS. That is not obvious from either the CSS or the
Rust, which is why it is written down here. Its consequence — that the window's
usable height differs per platform in a way no test in this repository can
observe — is §7.2.

### 1.5 Every file this editor saves misattributes itself to whoever saved it last elsewhere

Found while writing §4.3, which originally claimed the engine models no author.
It models two, and the second one is currently wrong in every file this
application writes.

`DocumentProperties::last_modified_by` is *"Who saved it last — ODF
`dc:creator`, OOXML `cp:lastModifiedBy`"*
(`crates/casual-calc-model/src/workbook.rs:184`). It is read on import
(`crates/casual-calc-import/src/lib.rs:79`, `crates/casual-calc-ods/src/lib.rs:410`)
and written on export
(`crates/casual-calc-export/src/lib.rs`, `core_properties_xml` and
`core_properties_part`). **Nothing anywhere assigns it except a reader.** The
only two lines in `crates/` that put a value into it are the two importers,
`crates/casual-calc-import/src/lib.rs:79` and
`crates/casual-calc-ods/src/lib.rs:410` — both of them copying what the file
already said.

So a workbook that Excel recorded as last saved by Alice, opened in OpenCalc,
edited by Bob and saved, still says Alice. The file is not corrupt and nothing
is lost — it is *asserting something false*, in the one field a support engineer
or an auditor would read to find out who touched it. `session_set_doc_properties`
even documents the correct behaviour in the course of refusing to expose the
field — *"`lastModifiedBy` is set by whoever saves"*
(`crates/casual-calc-wasm/src/io.rs:43`) — which is the shape `ADR-019` found
four times: a document stating a rule the code does not keep.

The desktop shell is the one host that can know who saved, which is why the fix
lands here (§4.3) rather than in the engine, and why it needs no engine change
at all: the write path is complete and clearing the field already removes the
element from the file.

### 1.6 The rest of the brief is confirmed

* **No profile concept exists.** `grep -rn profile desktop/src/` returns
  nothing.
* **No updater exists.** `grep -rn updater` over the whole repository returns
  nothing: no `tauri-plugin-updater` dependency, no `plugins.updater` block, no
  public key, no manifest.
* **`desktop/src/menu.rs` has no settings entry**, and cannot have one: it holds
  ids the webview gave it and defines no menu of its own, which is the whole
  point of `TAURI-004`.
* **CI builds all three platforms already** — `.deb`, `.app`, NSIS, with the
  webview dependencies installed and the WebAssembly built fresh in the job
  (`.github/workflows/ci.yml`, the `desktop` job), asserted by
  `tools/check-desktop-build.py`.
* **`release-sdk.yml` is the pattern**, and it already reserves the tag: its
  header comment lists `desktop-v0.0.0    reserved — the Tauri app`, and
  [15](15-CI-AND-RELEASE-GATES.md) §"Release tags" publishes the same row.

---

## 2. What the desktop app is today, measured

| Fact | Where |
| --- | --- |
| Version is `0.0.0` | `desktop/tauri.conf.json:4` |
| Identifier is `org.casualoffice.opencalc` | `desktop/tauri.conf.json:5` |
| One window, `1280×800`, `editor.html?chrome=native` | `desktop/tauri.conf.json` `app.windows[0]` |
| Shell state is app-wide and single-window — *"One window, one of these"* | `desktop/src/main.rs`, `struct Shell` |
| The bridge is seven functions on `window.__opencalcNative` | `desktop/src/main.rs`, `BOOTSTRAP` |
| Bytes cross the bridge, never paths | `desktop/src/lib.rs`, restated by `SAVE-02` |
| The only `cfg(target_os)` in the binary is the macOS application menu | `desktop/src/main.rs:99` |
| Settings are theme, scroll speed and accent | `webapp/editor.html`, `#settings-panel` |
| Settings persist in `localStorage` under `oc-theme`, `oc-accent`, `oc-scroll` | `webapp/editor.core.js:9503`–`:9513` |
| `desktop` mode has `canShare: true` | `webapp/editor.core.js:934` |
| The Share dialog asks the user to paste a token | `webapp/editor.presence.js`, `shareStart()` |

### 2.1 Six capabilities every comparator has and this window does not

Checked against Excel, OnlyOffice Desktop Editors and LibreOffice Calc on
macOS, Windows and Linux. Each of these is a thing a person notices in the first
five minutes, and each is absent from the binary rather than merely unstyled:

1. **Window state is not remembered.** Every launch is `1280×800` wherever the
   window manager puts it. All three comparators restore size, position and
   maximised state.
2. **A file cannot be opened by double-clicking it** (§1.3).
3. **A file cannot be dropped on the window.** `desktop/src/main.rs` registers
   no drag-drop handler; the webview's own drop target is the browser's, and it
   has no path.
4. **There is no File ▸ Open Recent.** All three comparators have one.
5. **There is a single window and a single document.** `Shell` is app-managed
   with one `target: Mutex<SaveTarget>`, so a second window would share one save
   target with the first — a `Ctrl+S` in window B writing over window A's file.
   This is the one item on this list that is a *latent defect* rather than an
   absence, and it is why §6 refuses multi-window rather than leaving it open.
6. **There is no update path at all** (§1.6).

### 2.2 The settings that exist are stored where nobody can find them

`localStorage` in a Tauri window is the platform webview's storage: WebKit's
data store under `~/Library/WebKit/…` on macOS, the WebView2 user-data folder
on Windows, WebKitGTK's under `$XDG_DATA_HOME` on Linux. Three consequences,
and the third is what decides §5:

* the user cannot find it, back it up, or copy it to another machine;
* it is discarded if the webview's data directory is reset, and the bundle
  identifier is part of that path;
* **Rust cannot read it.** The updater has to know the user's channel and
  consent *before the webview has settled*, and the save panel has to know the
  default directory before the webview is asked anything. A preference only the
  page can read is a preference the shell cannot act on.

---

## 3. The rule the six answers share

**The editor decides what; the shell decides how; the platform decides where.**

That is `TAURI-004`'s division restated for the things this note adds, and it is
what keeps the shell from growing a second copy of the editor. Concretely:

* A *preference* is one fact, named once. The editor owns its meaning and its
  control; the shell owns the file it is written to. There is never a desktop
  copy of a web setting.
* A *capability* the browser cannot have (a file panel, a title bar, an
  installer) lives in the shell and is offered to the editor as a function, not
  as a policy.
* A *platform difference* lives in `desktop/`, and nowhere else.
  `tools/check-host-seams.py` scopes to `crates/` — it lists
  `git ls-files 'crates/**/*.rs'` and looks for `cfg(target_*)` — so
  `desktop/src/main.rs:99`'s `#[cfg(target_os = "macos")]` is not a violation
  and never was. **The shell is where platform differences are allowed to
  live**, and this note says so explicitly because the gate's name does not.

---

## 4. Identity

### 4.1 The decision: a desktop **profile**, which is not an identity

A single JSON file, owned by the shell, holding the preferences and state the
shell has to act on. It has a `display_name`, and **that name is never sent to
anybody.**

Everything about how it is stored:

| | |
| --- | --- |
| Where | Tauri's `app_config_dir()` — `~/Library/Application Support/org.casualoffice.opencalc/` (macOS), `%APPDATA%\org.casualoffice.opencalc\` (Windows), `$XDG_CONFIG_HOME/org.casualoffice.opencalc/` (Linux, falling back to `~/.config`) |
| What | one file, `profile.json` |
| Shape | `{ "schema": 1, … }` — an integer, checked on read; an unknown higher number is read as far as it parses and **never written back over**, so a newer build's file survives an older build opening it |
| Written | to a sibling temporary file and renamed, so a crash mid-write leaves the previous file rather than half of the new one |
| Missing or unreadable | defaults, silently, with a line in the log. A profile is a convenience; refusing to start because a preferences file is corrupt is a worse failure than losing a theme |

Contents, and nothing else — every field has a consumer in this note:

* `display_name` — §4.3.
* `theme`, `accent`, `scroll_speed` — the three that exist today, moved out of
  `localStorage` (§5.2).
* `update.channel`, `update.automatic`, `update.skipped_version`,
  `update.last_checked` — §9.
* `files.default_directory`, `files.recent[]` — the save panel's starting point
  and File ▸ Open Recent.
* `window.width`, `window.height`, `window.x`, `window.y`, `window.maximized`.
* `collab.endpoint` — the collaboration server URL, so it is not retyped. **Not
  the token** (§4.4).

**First run does nothing.** No wizard, no account, no dialog. `display_name`
defaults to the operating system's account display name; the file is created on
the first write, not at startup. A person who downloads a spreadsheet editor
should be editing a spreadsheet, not answering questions about themselves.

**The alternative this beats** is an account: a sign-in on first launch, an
identity held by a service, a token minted by us. That is a product, not a
setting — it needs a service, a password reset, a privacy policy, a deletion
path and somebody to operate them. §12.1 asks the product owner whether that
product is coming, because if it is, §4.3 and §4.4 are the wrong answers and the
right ones are cheap to write *after* the answer and expensive to unpick before
it.

### 4.2 Why a file and not the OS keychain or a database

Keychain access on macOS prompts, on Linux depends on a running secret service
that a headless or minimal desktop may not have, and on Windows is per-user DPAPI
— three different failure modes for data that is not secret. A theme is not a
credential. The one field that *would* be a credential is deliberately not
stored at all (§4.4).

A database is refused for the reason `ADR-023` refuses a crate: it would be a
dependency earning its place by having exactly one consumer holding roughly ten
keys.

### 4.3 What `display_name` is for: the author line the engine already writes

It is the **local author line**, and it has a consumer today. This paragraph
originally said the opposite; checking changed it, and the finding is §1.5.

`Workbook::properties` carries `last_modified_by` — *"Who saved it last — ODF
`dc:creator`, OOXML `cp:lastModifiedBy`"*
(`crates/casual-calc-model/src/workbook.rs:184`). The importer reads it
(`crates/casual-calc-import/src/lib.rs:79`), the exporter writes it back into
the retained `docProps/core.xml` surgically rather than replacing the part
(`crates/casual-calc-export/src/lib.rs`, `core_properties_part`), and clearing
the field in the model removes the element from the file. The whole mechanism
is built, tested and round-tripped. **The only thing missing is a caller**, and
that is what the profile supplies.

**The decision.** On a desktop save, the shell sets `last_modified_by` from the
profile before serialising — or clears it, if the user has said not to record a
name. Never leaves it as it was.

*Never leaves it* is the part that matters, and §1.5 is why. It is also why the
setting is a genuine choice rather than a switch with an obviously right side:

| | the file afterwards |
| --- | --- |
| Record my name | says who actually saved it |
| Do not record a name | says nothing, rather than saying somebody who did not |

The web editor is deliberately unchanged: a browser tab has no profile, and
`session_set_doc_properties` refuses to expose this field on purpose
(`crates/casual-calc-wasm/src/io.rs:43` — *"`lastModifiedBy` is set by whoever
saves"*). This note makes that sentence true on the one host that knows who is
saving, and does not turn the field into a text box anywhere.

**`creator`** — the original author — is not touched. It is the file's history,
and a saver that rewrote it would be doing the thing
`crates/casual-calc-model/src/workbook.rs:138` warns about from the other
direction.

Comments and tracked changes are the other consumers this name would have, and
the engine models neither. They are named here so that when they arrive the
name is already there rather than being invented a second time.

### 4.4 What a collaborative session does with the profile: **nothing**

This is the decision, and it is a refusal.

The desktop app **does not send its profile name into a session, and does not
mint its own token.** A participant's name and colour continue to come from the
token the deployment issued, exactly as [72](72-SESSION-ACCESS-CONTROL.md)
requires and as `webapp/collab.js` already implements.

What `File ▸ Share…` therefore means on desktop, unchanged from the browser: the
user supplies the endpoint and a token their deployment gave them, and the
profile remembers the endpoint so it is typed once. The token is asked for every
session and stored nowhere.

**Two alternatives, and why each loses.**

*Let the desktop mint its own token.* The server accepts HMAC verification with a
shared secret for development and standalone use
(`server/casual-calc-collab-server/src/verify.rs`), so a desktop app holding
that secret could sign a token naming itself. It would work, and it would make
every desktop install an issuer: the secret is a per-deployment signing key, and
a copy of it on every laptop is a copy of the ability to mint *any* name, *any*
document key and `owner: true`. `COL-40` built the owner claim specifically so
that not every editor could lock every other editor out; a client-side issuer
hands that back. Refused.

*Turn `canShare` off on desktop.* Honest, and it removes a capability that
already works for the deployment that has a server — which is the only
deployment where a collaborative session means anything. It would also be a
regression against `UX-DESK-01`, which moved the collaborator roster into the
status bar precisely because `desktop` has `canShare: true`. Refused.

**What follows, and must be built with it.** Today the desktop Share dialog is
the browser's, and it asks for a token with the placeholder *"the token this
deployment issued you"* — which is meaningless to a solo user who downloaded an
alpha. The desktop path needs one sentence of copy that says so plainly, and
that sentence is the deliverable: *sharing needs a collaboration server; if you
do not run one, this does nothing.* A dialog that asks for a credential nobody
can obtain, and then fails on connect, is the worst of the three options
available here.

### 4.5 The two-machines question, answered by not asking it

`User::id` is documented as *"Two connections with the same id are the same
person on two devices."* The profile deliberately holds **no** stable client id
of its own. There is nothing on a desktop machine that can honestly claim to be
the same person as another machine, and a random per-install UUID sent as an
identity would be a claim the client made about itself — §4.4 again, one level
quieter. If a desktop install ever needs to be recognised across sessions, that
is the account question in §12.1.

---

## 5. Settings

### 5.1 The decision: one panel, in the webview, with a desktop section

Settings stay HTML in the webview. `UX-DESK-01` already moved `#settings-panel`
out of `.app-header` into the overlay host and gave it two forms — anchored under
the gear when the gear is on screen, a centred `aria-modal` dialog when it is not
— and that second form exists *because* of the desktop, where the branding strip
and its gear are gone. The desktop reaches it through the same command id in the
operating system's own menu, which is what `TAURI-004` made possible.

**The alternative this beats: a native settings window.** A second Tauri window
with platform controls would look right on each platform and would cost a second
UI toolkit, a second theme implementation, a second place for `theme` to live,
and a second surface to keep in step with the first. That is precisely
`ADR-023`'s argument against `casual-calc-tauri` — *"a wrapper around a wrapper
is not a seam; it is a place for the two sides to drift"* — applied to a
preferences dialog. A settings panel is not where this project should spend its
one deviation from "the editor is the editor".

The panel gains a section that is present only when the shell is
(`window.__opencalcNative` is defined). It holds five things, and each of them
is a thing the shell can actually do:

| Setting | Why it is here and not in the web panel |
| --- | --- |
| **Updates** — current version, channel, "check automatically", "Check now" | There is no update path in a browser tab |
| **Your name in files you save** — the profile name, or "do not record a name" | §4.3. A browser tab has nobody to name, which is why `session_set_doc_properties` refuses the field |
| **Default save location** | The browser's Save is a download; there is no directory to choose |
| **Restore the last document on launch** | A tab has no launch |
| **Reopen window where it was** | A tab has no window |

### 5.2 Persistence moves; the settings do not

The panel is the same panel. What changes is what is underneath it: the editor
reads and writes preferences through one accessor, whose browser implementation
is `localStorage` and whose desktop implementation is two more bridge functions
that read and write `profile.json`.

**This is not tidiness.** §2.2 is the argument: the updater and the save panel
are Rust, they need the channel and the directory, and they cannot read
`localStorage`. Once the file exists for those two, putting `theme` in it costs
one accessor and buys a preference the user can back up and that survives a
webview data reset.

The seam is the shape `ADR-019` and [78](78-HOST-CAPABILITY-SEAMS.md) settled
for everything else the host supplies: a capability per seam, offered as a
function, not a trait describing all of them.

### 5.3 What is refused from the settings panel

* **A "file associations" checkbox** (§1.3). The association is declared at
  bundle time; the only runtime action is *make OpenCalc the default*, which
  Windows 10 and later refuse to let an application do to itself. What the
  panel gets instead is a line stating which types this build registers and a
  button that opens the operating system's own default-apps page — which is
  what OnlyOffice and LibreOffice both do, and which cannot lie about having
  succeeded.
* **A font path, a cache size, a thread count.** No seam exists for the third
  ([44](44-TAURI-DESKTOP-SHELL-DESIGN.md) §"The platform seams" — *"absent, and
  not merely unimplemented"*), and the other two are engine policy the engine
  does not currently take.
* **A telemetry toggle**, because there is no telemetry and a switch that
  turns off something that does not exist is a claim, not a control.
* **Anything about theme placement.** A separately filed chrome row (`UX-CHR-01`)
  moves the theme control out of the gear popover and into the View menu, which
  already holds Gridlines, Cell markings, Formulas instead of results and Zero
  values (`webapp/editor.core.js:8903`–`:8919`), and takes the open-file folder
  icon out of the branding strip. This note does not design around the current
  placement and does not move it either.

---

## 6. Chrome: what is left after `UX-DESK-01`

`UX-DESK-01` removed the branding strip, hid the HTML menu bar, relocated
`#tb-status` and `#presence` into the status bar, and tightened the control
metrics — toolbar 49→37, formula bar 41→33, status bar 41→31. What is left is
not styling. It is §2.1's list, and it is the difference between a window and an
application.

### 6.1 What this note decides to build

**Window state is remembered.** Size, position and maximised state into
`profile.json` on close, restored on launch, clamped to a monitor that currently
exists — a window restored to a display that has been unplugged is a window the
user cannot find, and it is the one bug every implementation of this feature
ships once.

**A file can be opened by double-clicking it, and by dropping it on the
window.** `bundle.fileAssociations` for **all seven** extensions the engine opens — `.xlsx`, `.xlsm`, `.ods`, `.csv`, `.tsv`, `.tab`, `.psv`. This paragraph said four until `TAURI-010` machine-checked it, missing `.ods` (`ODS-01`), `.xlsm` (`IO-08`) and `.tab` — **the same second-list-that-drifts mistake this section warns about two paragraphs above**, made by the warning itself. It is a list no document should hold: the shell now asks `SessionFormat::for_extension` and a test fails when the two disagree, which is why this sentence can be trusted and the last one could not —
which is the set `openable_extensions()` already reports, asked of the SDK
rather than listed, so the panel and the association cannot disagree — plus the
argv path on Windows and Linux and `RunEvent::Opened` on macOS, which is the only
way a Mac delivers a file to an already-running app. Drag-drop is handled in the
shell, because the webview's drop event has no path and the shell's invariant is
that a path enters this process only from the platform.

**File ▸ Open Recent**, from `profile.json`, with entries that no longer exist
shown and struck out rather than silently dropped — a recent list that quietly
forgets is a list that makes the user doubt their memory instead of their disk.

### 6.2 What this note refuses, and why

**Multi-window and multi-document.** One window, one workbook, and the second
window is refused rather than deferred — because `Shell` holds one
`Mutex<SaveTarget>` for the whole application, so a second window today would
share the first window's save target and `Ctrl+S` in one would overwrite the
other's file. That is a data-loss defect, not a missing feature, and the correct
response to a latent data-loss defect is to keep the invariant that makes it
unreachable ("one window") and write down what it would take to lift it: `Shell`
becomes per-window state keyed by window label, and every one of the nine
commands in `invoke_handler!` is re-checked against it. That is a row, not a
paragraph.

**A ribbon.** Excel and OnlyOffice both have one; LibreOffice offers one and
does not default to it. It is a whole-editor redesign that would apply to the
browser too, it is owned by [12](12-COMPETITIVE-ANALYSIS.md) and
[47](47-UX-AND-FEATURE-MAP.md), and it is not a desktop question.

**A custom title bar.** Tauri can draw one; every OS then gets our window
controls instead of its own, and macOS loses the traffic-light behaviours users
expect. The document name is already in the OS title bar (`desktop/src/title.rs`).

**The macOS proxy icon** — the small document icon in the title bar that can be
dragged and command-clicked for the path. It is genuinely Mac-native and it is
one platform's polish; it is named here so that not doing it is a decision.

**`UX-DESK-04` and `UX-DESK-05`.** Both are open chrome rows found by
`UX-DESK-01`'s own worker, both are in `webapp/`, and neither is this note's.
Folding them in would put four agents in one file.

---

## 7. Per-OS behaviour

### 7.1 What differs, and where the difference lives

Every row of this table is `desktop/`'s business. None of it may appear in
`crates/` (§3).

| | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Menu bar | global, top of screen | in-window, drawn natively | in-window, drawn natively (but see §7.3) |
| Application menu | required, first, holds About and Quit — `desktop/src/main.rs:99` already builds it | none; Exit lives in File | none; Quit lives in File |
| Modifier | ⌘ | Ctrl | Ctrl |
| Accelerator spelling | resolved by the platform from `CmdOrCtrl` — `desktop/src/menu.rs` deliberately does not branch | as macOS | as macOS |
| Window controls | left | right | window-manager's choice |
| File panels | `NSOpenPanel`/`NSSavePanel` | common item dialog | GTK / portal |
| A file delivered to a running app | `RunEvent::Opened` | argv on a second instance | argv on a second instance |
| Config | `~/Library/Application Support/<id>/` | `%APPDATA%\<id>\` | `$XDG_CONFIG_HOME/<id>/` |
| Bundle | `.app` in `.dmg` | NSIS `.exe` | `.deb` and AppImage |
| Unsigned install | Gatekeeper says *damaged* | SmartScreen hides Run behind **More info** | nothing |
| Self-update | replaces the `.app` | runs the NSIS installer | AppImage only (§9.4) |

### 7.2 What stays identical, and one number that does not

Identical: the menu *model*, because it is the editor's and there is only one
(`TAURI-004`); every command id; the engine, because
`tools/check-host-seams.py` keeps `crates/` free of `cfg(target_*)`; the
keyboard bindings the editor registers, since the editor writes `Ctrl+…` and
`desktop/src/menu.rs` hands the platform `CmdOrCtrl` to resolve; and the file
format on every platform.

**Not identical: the window's usable height.** An in-window native menu bar on
Windows and Linux consumes vertical pixels that macOS spends on the global bar
instead. `UX-DESK-01`'s measurement of the grid growing by roughly the hidden
bar's height was taken on macOS, and the browser test that pins it
(`tests/browser/editor.native-chrome.spec.mjs`) runs in a **browser**, where
`?chrome=native` changes CSS and no native menu bar exists at all. So no test in
this repository can see the height a real Windows or Linux menu bar takes.

Nothing in the layout may assume a number. The grid is already sized from the
element it is in rather than from arithmetic on known bar heights, and that is
the property to keep and to state: **the desktop layout is measured, never
computed from per-platform constants.**

### 7.3 Two things we do not know, said as not known

* **Some Linux desktops export the menu globally.** Unity-style appmenu
  environments can lift a GTK menu bar out of the window and into a panel, so a
  Linux window may or may not draw its own bar depending on the desktop
  environment. The assumption is that it draws its own; if it does not, the
  window gains back that height and nothing else changes, because §7.2's rule is
  that nothing depends on the number. That is the whole mitigation, and it is
  why the rule is worth having.
* **None of the Windows or Linux behaviour in §7.1 has been observed on this
  machine.** It is read from Tauri's source and its documentation, and from the
  platform conventions. The `desktop` job builds on all three but runs none of
  them — a bundle is produced and its existence asserted, and nothing opens a
  window.

  This is stated rather than implied because a design note that reads as tested
  is worse than one that reads as reasoned. **The first release is what will
  surface it**, and that is an argument for shipping the alpha, not against it:
  three platforms of window behaviour cannot be learned any other way, and the
  cheapest way to learn is to hand it to somebody with a Windows machine.

---

## 8. Builds

### 8.1 What per-PR CI already does, and what it deliberately does not

The `desktop` job builds `.deb` on Linux, `.app` on macOS and an NSIS installer
on Windows; installs `libwebkit2gtk-4.1-dev` and the rest of the Linux
prerequisites; builds `casual-calc-wasm` fresh into `webapp/pkg/` so
`generate_context!` has something to embed; runs the desktop workspace's own
tests on Linux; asserts a bundle exists *on disk* rather than trusting an exit
code; and uploads each with a seven-day retention. `tools/check-desktop-build.py`
asserts the job exists on all three, that it bundles rather than compiles, that
`bundle.active` is still true, and that `icons/icon.ico` is present.

It is unsigned on purpose — *"a job that needs a secret is a job that cannot run
on a fork's pull request."*

### 8.2 What a release needs on top

1. **A version that is not `0.0.0`.** `desktop/tauri.conf.json:4`. The first
   desktop release is **`0.1.0`**, not `0.0.1`: `0.0.x` is the SDK's preview
   line and reusing it would make two unrelated artefacts look like one
   sequence. The desktop version moves independently of the SDK's — they are
   different products on the same tag namespace, which is exactly why the
   namespace is component-scoped.
2. **`.dmg` on macOS.** CI builds `.app` because a disk image needs a mountable
   volume and that is a flake a pull request should not buy. A release should:
   a bare `.app` downloaded from a browser arrives as a folder in Downloads,
   and §9.3 explains why that placement is specifically dangerous here.
3. **Both macOS architectures.** `macos-latest` is Apple Silicon, so today
   nothing in this repository has ever built an Intel Mac binary. A release
   builds `universal-apple-darwin`, or it says in its notes that it is Apple
   Silicon only. Silently shipping one architecture as "macOS" is the failure
   to avoid.
4. **AppImage on Linux**, alongside the `.deb`. Two reasons and the second is
   decisive: a `.deb` built on `ubuntu-latest` links a glibc that older
   distributions do not have, and **Tauri's updater cannot update a `.deb`**
   (§9.4). Auto-update on Linux *is* the AppImage.
5. **Checksums.** A `SHA256SUMS` file listing every artefact. With no code
   signing, a checksum published beside the download is the only thing a
   careful user can check at all, and it is the thing the site's bypass
   instructions must point at.
6. **Signature files.** `tauri build` emits a `.sig` beside each updater
   artefact when the signing environment variables are set (§9.2). Without them
   the updater has nothing to verify and every update is refused.
7. **A manifest** — §9.
8. **Release notes that say what is not signed**, in the words the operating
   system will use (§9.3), and a link to the site's bypass instructions.

### 8.3 How the tag triggers it

A new release workflow, watching `desktop-v*` — already reserved by
`release-sdk.yml`'s header and by [15](15-CI-AND-RELEASE-GATES.md)'s table.
`tools/check-release-hold.py` is the contract it must satisfy, and it is worth
restating because it constrains the design rather than merely checking it:

* no `push.branches` trigger, so merging cannot publish;
* the tag pattern is component-scoped, so tagging the desktop cannot fire the
  SDK's release;
* any `workflow_dispatch` carries a `dry_run` input defaulting to **true**.

Two things copied from `release-sdk.yml` because they were learned there:

* **The tag is checked against the packaged version before anything is
  published** — `desktop-v0.1.0` against `tauri.conf.json`'s `version`, refusing
  rather than shipping something the repository cannot be searched for.
* **Credentials live on the `release` GitHub environment**, not in repository
  secrets, so a release inherits that environment's protection rules.

And one thing that is *not* copied: `release-sdk.yml` publishes with
`--prerelease`. The desktop release must not (§9.5).

---

## 9. Auto-update

### 9.1 The channel

**One channel.** There is no beta line, no nightly, and no per-user opt-in to a
faster ring, because there is one release stream and a channel selector with one
entry is a control that teaches the user nothing. The profile carries
`update.channel` anyway, set to `"stable"`, so that adding a second is a value
and not a migration.

`update.automatic` defaults to **the answer in §12.2**, which is the product
owner's, and the shell must not guess it. A "Check now" button exists regardless
and is the only path that can produce a dialog the user did not ask for.

### 9.2 Where the key lives, and what its loss costs

Tauri's updater verifies its own **minisign** signature, generated by
`tauri signer generate`. This is unrelated to a Developer ID or an Authenticode
certificate: it is our key, over our artefacts, checked by our binary. The
decision that there is no OS code signing does not touch it.

* The **private key** and its password are secrets on the `release` GitHub
  environment, read by the release workflow as `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` so that `tauri build` emits a `.sig` per
  artefact. They are never in the repository and never on a developer machine
  that also has push rights.
* The **public key** goes in `desktop/tauri.conf.json` under
  `plugins.updater.pubkey`, and is therefore compiled into every shipped binary.

**If the private key is lost, it cannot be replaced for anybody already
installed.** Every installed copy verifies against the public key baked into it,
so a manifest signed with a new key fails verification, silently, on every
machine — the user is stranded on the version they have and the only route
forward is a manual download of a build carrying the new public key. This is the
one secret in this repository whose loss cannot be repaired by redeploying:
`PKG_PASS` can be reissued, an image registry credential can be rotated, this
cannot. It therefore also has an **offline copy**, held wherever this project
holds the things it cannot regenerate, and losing both is a decision to
re-onboard every user by hand.

If the key is ever believed compromised: a new key, a new public key in a new
release, release notes saying so, and every prior install requires a manual
update. There is no revocation.

### 9.3 What "unsigned" actually does, and why the updater is the fix rather than a nicety

* **macOS.** A `.dmg` or `.app` downloaded by a browser carries
  `com.apple.quarantine`. Gatekeeper's dialog for an unsigned quarantined app
  says the application *"is damaged and can't be opened"* — Apple's wording,
  and it is misleading, because nothing is damaged. The site's instructions must
  quote that exact sentence, because it is what the user will search for.
* **Windows.** SmartScreen shows "Windows protected your PC" and hides the
  installer behind **More info → Run anyway**.
* **Linux.** Nothing.

Here is the part that makes the updater load-bearing: **an update installed by
the updater does not go through either of those.** The bytes are fetched and
written by our own process, so no quarantine attribute is applied and no
Mark-of-the-Web reaches the NSIS installer. The hostile experience is the *first
install only*; every subsequent version arrives clean. Without an updater, every
user repeats the *damaged* dialog on every release, forever.

**One macOS failure mode this creates, and it must be handled.** An app the user
runs directly from Downloads without moving it may be running under App
Translocation, from a randomised read-only path. An in-place update writing to
its own bundle path fails there. The shell must detect that its executable path
is not where it expects and say so in words — *move OpenCalc to Applications and
try again* — rather than reporting a failed update with no cause. This is a
direct consequence of not code signing, and it is why §8.2 asks for a `.dmg`:
a disk image is what teaches a Mac user to drag the app to Applications.

### 9.4 The manifest, and the platforms it can actually serve

A static `latest.json` attached to the GitHub Release, generated by the release
workflow from the artefacts it just built, with `endpoints` pointing at
`https://github.com/CasualOffice/opencalc/releases/latest/download/latest.json`.
Its shape is Tauri's: a `version`, a `pub_date`, `notes`, and a `platforms` map
from target triple to `{ url, signature }`.

Which artefacts can appear in that map is not a free choice:

| Platform | Updater artefact | `.deb` / `.dmg` |
| --- | --- | --- |
| macOS | `.app.tar.gz` + `.sig` | the `.dmg` is for humans, not the updater |
| Windows | the NSIS installer, zipped, + `.sig` | — |
| Linux | the AppImage + `.sig` | **the `.deb` cannot be updated** |

A `.deb` is owned by `dpkg`; an application replacing its own files under
`/usr` would need root and would fight the package manager. So on a `.deb`
install the shell **does not offer to install**: it detects the absence of the
AppImage environment, and where an update exists it says so and links to the
download page. Silently doing nothing there is the failure to avoid — a user who
has switched auto-update on and never sees an update believes they are current.

**The alternative this beats: hosting the manifest on the site.** A `latest.json`
served from GitHub Pages, where `pages.yml` already publishes `webapp/`, would
give a stable URL under our control that can be edited without touching a
release. It is the better answer the day there are two channels, or the day a
release must be withdrawn without deleting artefacts people have already
downloaded. It loses today because it needs the release workflow to write into
the site's deploy, which is a second publishing path for a first release that
has one artefact set — and §9.5 shows the problem it solves has a cheaper
solution.

### 9.5 Two things about GitHub Releases that decide the shape

**`releases/latest` ignores prereleases.** `release-sdk.yml` publishes with
`--prerelease`, and that is right for npm. If the desktop release copied it, the
`latest/download` URL would 404, the updater would treat it as an error,
background checks would fail silently, and nothing anywhere would say the update
path was dead. So **the desktop release is not marked prerelease**, and the
alpha warning lives where it can be read: in the version number, in the release
notes and on the site.

**And it is checked, not remembered.** After publishing, the release workflow
fetches `https://github.com/CasualOffice/opencalc/releases/latest/download/latest.json`
and fails if it does not return the version just tagged. That is three lines,
and it is the only thing that would catch the failure above — which is otherwise
invisible until a user does not get an update they were never told about.

**Withdrawing a bad release is one click.** Marking it prerelease in the GitHub
UI makes `latest` fall back to the previous release; clients already on the bad
version then see a manifest naming an older version and are told there is no
update, which leaves them stuck but stops the spread. That is an acceptable
worst case for an alpha and it needs no infrastructure, which is the second half
of why §9.4 chose GitHub over Pages.

### 9.6 What happens when it fails, and when the user declines

**The rule: an update never costs work.** The install step is gated on the same
dirty check `Ctrl+S` uses. The NSIS installer closes the running application; if
the document has unsaved changes, the shell refuses to install and offers to
save first. An auto-update that discarded an unsaved workbook would be the
single worst bug this application could have, and it is a bug that only exists
because updating is asynchronous with editing.

Failures, by who asked:

* **A background check that fails** — network, DNS, a 404, a malformed manifest —
  is logged and otherwise silent. The user did not ask; telling them their
  update check failed is noise about a thing they were not thinking about.
* **A check the user pressed "Check now" for** always answers in words: up to
  date, an update is available, or why it could not tell.
* **A download or install that fails** always says so, names what it was doing,
  and leaves the installed application untouched. Tauri's updater downloads to a
  temporary location and installs as one step, so a failure mid-download is a
  discarded file, not a half-replaced application. The App Translocation case
  (§9.3) gets its own sentence rather than a generic failure.

Declining:

* **"Later"** dismisses it for this run. The next launch may offer it again.
* **"Skip this version"** writes `update.skipped_version` into the profile, and
  that version is never offered again. A later version is.
* There is no "never ask again" separate from turning the automatic check off,
  because two controls for one intention is how a user ends up believing they
  are current when they are not.
* Declining **never re-prompts in the same session**, and an update is never
  offered while a modal dialog or a cell editor is open.

---

## 10. The order to build in

Eight rows. The dependencies are real, not tidy: four of them cannot be started
before the profile file exists, and the updater cannot be finished before the
release workflow can sign.

1. **The profile file** — `profile.json`, the schema integer, the atomic write,
   the config directory per platform, and the accessor the editor reads
   preferences through. *Depends on nothing.* Everything below depends on it.
   Its acceptance test is a test that writes a profile, corrupts it, and asserts
   the app starts with defaults.
2. **Window state and Open Recent** — the smallest real user-visible win, and
   the first consumer of (1). *Depends on 1.*
3. **The desktop settings section** — the five rows in §5.1, and moving
   `theme`/`accent`/`scroll` off `localStorage`. *Depends on 1.* Its acceptance
   test is that a preference set in the panel survives clearing the webview's
   storage.
4. **The author line** — §1.5 and §4.3. A desktop save sets
   `last_modified_by` from the profile, or clears it. **This one is a defect,
   not a feature**, and it is the only row here whose absence makes files this
   application writes assert something false — so it is sized and prioritised as
   a defect even though it arrives with the profile. *Depends on 1* for the name
   and the consent, and on nothing in the engine: the write path is complete.
   Its acceptance test opens a fixture whose `cp:lastModifiedBy` names somebody
   else, saves it, and asserts the element names the profile — and, with the
   setting off, that the element is gone rather than stale. It fails today in
   both directions.
5. **File associations and file-open** — `bundle.fileAssociations`, argv,
   `RunEvent::Opened`, drag-drop. *Depends on nothing, but must not be merged
   before it can actually open the file*, which is the whole of §1.3. Its
   acceptance test is a unit test over the argv/`Opened` path, because a
   double-click cannot be tested in CI.
6. **The share copy** — the desktop Share dialog says what a collaboration
   server is and that sharing does nothing without one. *Depends on nothing.*
   Smallest row here, and the one that stops the first bug report.
7. **The release workflow** — version bump to `0.1.0`, `desktop-v*`, `.dmg`,
   AppImage, both macOS architectures, checksums, the tag-versus-version check,
   the `release` environment, and the post-publish fetch of §9.5. *Depends on
   nothing*, and can run in parallel with 1–5. Its acceptance is a `dry_run`
   dispatch that produces every artefact and publishes none.
8. **The updater** — the plugin, the public key, the manifest generation, the
   signature step, the dirty-document gate, the decline states, the `.deb` and
   translocation cases. *Depends on 7*, because there is nothing to sign or to
   point at until a release exists, and on 1 for the channel and consent.

Rows 1 and 7 are the two that unblock everything and share nothing; they are the
pair to start with. Rows 2, 3, 4 and 6 all touch `webapp/` or the same bridge,
so they are one worker in sequence rather than four in parallel — the rule
[67](67-REPOSITORY-REMEDIATION-PLAN.md) already states for waves.

`TAURI-001` stays `Partial` after all eight. Its three open measurements — Tauri
IPC transport cost, the native GPU backend, parallel-recalc partitioning —
are untouched by this note and still need a running shell, which is a thing this
note makes more likely rather than something it delivers.

---

## 11. What this note deliberately does not do

* **It does not invent an account.** §4.1's alternative, deferred to §12.1.
* **It does not let a client name itself.** §4.4, and it is the one refusal here
  that is a security property rather than a scope decision.
* **It does not add code signing**, and does not design as though it might
  arrive; §9.3 is written for the unsigned case and would only get simpler.
* **It does not make the shell a second editor.** No native settings window, no
  second menu definition, no Tauri command that decides *when*.
* **It does not open a second window** (§6.2), and says what it would take.
* **It does not touch `crates/`.** Nothing in this note is an engine change, and
  `tools/check-host-seams.py` should go on passing without exemption.
* **It does not build a beta channel, a rollback, or a delta update.** Each is
  a real thing and each needs more than one release to be worth having.
* **It does not name a new gate script.** `tools/check-doc-claims.py` rule 1
  fails a document naming a `tools/check-*.py` that does not exist, so the
  gates §10's rows should add are described in their rows and named when they
  are written — which is the correct order anyway.

---

## 12. Both questions answered, 2026-08-30

Kept in full below rather than deleted, because a decision is worth what the
alternative it was chosen over is.

**Q1 — no account. The profile is local user information only:** a name, a
timezone, and preferences. Asked directly, the answer was *"its just local user
information.. like name, timzone and other prefernses"*. That is what §4 already
proposes, so §4 stands unchanged: the profile never becomes an identity on the
wire, the token stays the sole source of `who.name`, and §4.5's deliberate
absence of a stable client id is a decision rather than a gap.

**A timezone is a new element and it is not merely a preference.** `NOW()`,
`TODAY()` and every date rendering depend on it, and this engine already has a
clock seam — `ADR-019` made the clock a *value* rather than a method
specifically so it cannot change mid-recalculation. So the profile's timezone
must reach the engine through that existing seam and must not become a second
one. That is the whole of what this note decides about it; the semantics of a
workbook whose author's timezone differs from its reader's is a larger question,
and one this note deliberately leaves alone rather than answering badly in a
paragraph.

**Q2 — the desktop application works offline, and that is the constraint, not a
preference.** Asked whether it may contact the internet unasked, the answer was
*"desktop should work offline"*. So:

- Nothing in the application may **wait** on the network. An update check is
  never on a path a user is blocked behind — not at launch, not at open, not at
  save.
- A failed or absent network is a **normal state**, not an error to report. The
  update section shows when it last checked and offers a manual check; it does
  not warn.
- Every feature except collaboration and the update check works with no network
  at all, which is already true of the engine and must stay true of the shell.

This does not settle whether the *check* is on by default — §9 keeps that as the
one line of first-run copy, because "works offline" and "tells you when there is
a new version" are compatible, and the honest way to have both is to ask once.

## 12a. The questions, as they were put

Both are stated rather than decided, because deciding either silently would be
making a product decision inside a design note.

### 12.1 Is there going to be an OpenCalc account?

§4 answers "who is the desktop user" with *nobody, locally* — a profile that
holds preferences and never claims an identity — and answers "how does a desktop
user collaborate" with *they bring a token from a deployment they already have*.
Both answers are correct for a self-hosted product and both are wrong for a
product with an OpenCalc-operated collaboration service.

If such a service is coming, the profile becomes an account, `display_name`
acquires a real consumer, the Share dialog stops asking for a token and starts
asking for a sign-in, and §4.5's deliberate absence of a stable client id
becomes a gap. None of that is expensive to build *after* the answer; all of it
is expensive to unpick if it is guessed at now. **The answer changes §4 and
nothing else in this note**, which is why the rest can be built either way.

### 12.2 May the desktop app contact the internet without being asked?

`desktop/tauri.conf.json` describes the product as *"a calculation engine that
runs entirely on this machine"*. `AGENTS.md` §"Engineering priorities" 3 says
**no automatic network fetches**, and while that rule is about the engine and an
update check is the shell's, a user who read the description will not draw that
line.

So: does `update.automatic` default to **on** or **off**?

* **On** is what every desktop application does, and it is the only setting
  under which most users are ever on a current version — which matters more than
  usual here, because §9.3 means a manual update costs them the *damaged*
  dialog again.
* **Off** is the only setting that keeps the sentence above literally true, and
  the only one under which the application makes no request the user did not
  make.

There is a middle answer — ask once, on first run, in one line — and it is the
one this note would take if it were choosing. It is not choosing. It is worth
noting that this is the same shape as the Share dialog's `COL-46` acknowledgement:
a thing the user is told plainly once, rather than a default nobody sees.

---

## 13. Reproducing the measurements in §1 and §2

Every claim above is one command. From the repository root:

```sh
# §1.1 — the TAURI series, in full
grep -no 'TAURI[-0-9]*' docs/14-EXECUTION-TRACKER.md docs/14a-ARCHIVE-CLOSED-WORK.md | sort -u -t: -k3

# §1.2 — identity comes from the token, in three places
grep -n 'from the token' server/casual-calc-collab-server/src/presence.rs
grep -n 'claimed name would be believed' webapp/collab.js
grep -n 'desktop: {' webapp/editor.core.js

# §1.3 — no associations, no argv, no RunEvent::Opened
grep -n 'fileAssociations' desktop/tauri.conf.json ; echo "exit=$?"
grep -n 'env::args\|RunEvent\|drag\|drop' desktop/src/main.rs

# §1.4 — the app-wide menu, and what Tauri says it does
grep -n 'set_menu' desktop/src/main.rs
grep -n -B4 'pub fn set_menu' \
  ~/.cargo/registry/src/*/tauri-2.11.5/src/app.rs

# §1.5 — the author line: read on import, written on export, assigned nowhere
grep -rn 'last_modified_by *=' crates/ | grep -v '=='   # only the two importers
grep -rn 'last_modified_by' crates/ | grep -v '///'    # read, written, never set
sed -n '41,46p' crates/casual-calc-wasm/src/io.rs      # the rule, stated

# §1.6 — no profile, no updater, anywhere
grep -rn 'profile' desktop/src/ ; echo "exit=$?"
grep -rn 'updater' --include='*.rs' --include='*.toml' --include='*.json' \
  --include='*.yml' . ; echo "exit=$?"

# §2 — version, window, single shell state, settings storage
grep -n '"version"' desktop/tauri.conf.json
grep -n 'struct Shell' -A 8 desktop/src/main.rs
grep -n 'oc-theme\|oc-accent\|oc-scroll' webapp/editor.core.js

# §3 — the host-seams gate scopes to crates/
grep -n "ENGINE = \|ls-files" tools/check-host-seams.py

# §7.2 — the native-chrome test runs in a browser
grep -n 'chrome=native' tests/browser/editor.native-chrome.spec.mjs

# §8.1 — what the desktop CI job builds
grep -n 'bundles:\|artifact:' .github/workflows/ci.yml

# §8.3 — the tag namespace, already reserved twice
grep -n 'desktop-v' .github/workflows/release-sdk.yml docs/15-CI-AND-RELEASE-GATES.md
```

## References

- [44](44-TAURI-DESKTOP-SHELL-DESIGN.md) — the shell's shape, its seams, and the
  three measurements a running window still owes
- [81](81-DESKTOP-SHELL-COMPOSITION.md) — `ADR-023`: no crate between the app and
  the SDK, and what a separate Cargo workspace costs
- [78](78-HOST-CAPABILITY-SEAMS.md) — `ADR-019`: a capability per seam, which is
  the shape §5.2's preferences accessor takes
- [72](72-SESSION-ACCESS-CONTROL.md) — identity is the host's, never the
  client's; the rule §4.4 refuses to break
- [83](83-SAVE-AUTOSAVE-AND-VERSION-HISTORY.md) — `Ctrl+S` in four hosts, and the
  dirty-state primitive §9.6's install gate reuses
- [15](15-CI-AND-RELEASE-GATES.md) — the PR job contract and the release tag
  namespace
- [12](12-COMPETITIVE-ANALYSIS.md) — what Excel, OnlyOffice and LibreOffice do,
  which §6 is measured against
