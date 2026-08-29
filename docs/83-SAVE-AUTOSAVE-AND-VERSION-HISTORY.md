# 83 — Save, autosave, recovery and version history

## Outcome

One save model for four hosts. `Ctrl+S` writes the document to **its save
target** — a value resolved once, at open, from the same capability set
`editor.core.js` already computes — and never creates a second document. Around
that: a local draft so a closed tab does not lose work, a recovery presentation
that never restores anything the user did not ask for, and a version history
built from **snapshots**, not from replaying the collaboration log.

The last of those is the note's main negative result. The collaboration server's
append-only op log looks like a version history and is not one, for four
measured reasons; and `COL-50` independently rules out the replay a log-based
history would need. Snapshots are not the cheap-looking option — they are the
only one that survives contact with `ADR-011`'s OT.

### How to read the markings

This repository's rule is that a document stating a contract the code does not
keep is a defect. So every claim here is marked:

- **[built]** — in the tree today, cited `file:line`, and where it mattered,
  run rather than read.
- **[proposed]** — this note's decision, not yet code.
- **[refused]** — deliberately not being done, with the reason.

Nothing below describes proposed behaviour in the present tense.

---

## 1. What exists today

### 1.1 The keystroke

`Ctrl+S` is bound in the editor's global key handler and calls `saveAs("native")`
— **[built]**, `webapp/editor.core.js:7076-7084`. That reaches `doSaveNative()`
(`webapp/editor.sheets.js:393`), which asks the engine what the session's own
format is, warns about what that format cannot carry, and calls `download()`.

`download()` (`webapp/editor.clipboard.js:36-53`) is the one funnel every save
route passes through. In a browser tab it builds a `Blob` and clicks an anchor.
In the desktop shell it hands the bytes to `window.__opencalcNative.save()`.

Measured, against the running editor:

```
== standalone ==
capabilities: {"canOpen":true,"canSaveAs":true,"canPrint":true,"canShare":true,"ownsFile":false,"chrome":"web","readOnly":false,"mode":"standalone"}
edits_applied at boot: 24
isDirty at boot: false
session_format: xlsx
session_save_loss: ""
edits_applied after one edit: 25
isDirty after one edit: true
downloads after Ctrl+S: ["opencalc.xlsx"]
isDirty after Ctrl+S: false
localStorage keys: ["oc-theme"]
indexedDB databases: []
showSaveFilePicker present: function
storage estimate: {"quotaMB":1023,"usageMB":0}
navigator.storage.persisted: false
```

So: `Ctrl+S` produces a download named `opencalc.xlsx`; **nothing about the
document is persisted anywhere** (the only `localStorage` key present is the
theme, and there is no IndexedDB database at all); and the File System Access
API is available in this browser and unused.

### 1.2 The capability vocabulary this note extends

A mode is a set of capabilities, not a name with an `if` per site — **[built]**,
`webapp/editor.core.js:826-1090`. Seven axes (`editor.core.js:894`), five named
presets (`:902-917`), one table mapping capabilities to command ids
(`CAPABILITY_COMMANDS`, `:1039-1053`), and one `refuse()` that both tells the
user and emits `commandRefused` to the host (`:1082-1090`).

`Ctrl+S` already answers to that table: `canSaveAs: [/^file\.download/]`
(`:1052`), checked at `editor.core.js:7078-7080`. Measured across four modes:

```
== mode=embedded ==
  capabilities: {"canOpen":false,"canSaveAs":false,"canPrint":true,"canShare":false,"ownsFile":true,"chrome":"embedded","readOnly":false,"mode":"embedded"}
  downloads: []
  commandRefused events: [{"id":"file.download","capability":"canSaveAs","ownsFile":true,"mode":"embedded"}]
== mode=wopi ==
  capabilities: {"canOpen":false,"canSaveAs":false,"canPrint":true,"canShare":false,"ownsFile":true,"chrome":"web","readOnly":false,"mode":"wopi"}
  downloads: []
  commandRefused events: [{"id":"file.download","capability":"canSaveAs","ownsFile":true,"mode":"wopi"}]
== mode=viewer ==
  capabilities: {"canOpen":false,"canSaveAs":true,"canPrint":true,"canShare":false,"ownsFile":false,"chrome":"web","readOnly":true,"mode":"viewer"}
  downloads: ["opencalc.xlsx"]
  commandRefused events: []
== mode=desktop ==
  capabilities: {"canOpen":true,"canSaveAs":true,"canPrint":true,"canShare":true,"ownsFile":false,"chrome":"native","readOnly":false,"mode":"desktop"}
  downloads: ["opencalc.xlsx"]
  commandRefused events: []
```

This is the half of the problem that is already solved, and it is why this note
extends the vocabulary rather than building a parallel one: the host-owned case
already refuses out loud and already tells the host, over an event surface a
host can act on.

### 1.3 What the desktop shell knows — and does not

The shell tracks the document **name** and the dirty flag, pushed from the
webview on a 250 ms poll — **[built]**, `desktop/src/session.rs:80-100`,
`webapp/editor.core.js:7662-7680`. The title bar is built from those two
(`desktop/src/title.rs:39`).

**It does not know the path.** `native_open` reads the file the panel returned
and immediately reduces it: `let name = dialog::base_name(&path.to_string_lossy())`
(`desktop/src/main.rs:196`). `base_name` exists precisely to throw the directory
away — its doc comment says a name reaching the save panel with a separator in
it "is a save that lands somewhere the user did not choose"
(`desktop/src/dialog.rs`, `base_name`). `native_save` does the same on the way
back out (`desktop/src/main.rs:289`).

That is not an oversight. It is a stated invariant, `desktop/src/main.rs:23-26`:

> **Bytes cross this bridge, never paths.** `lib.rs` states it as the shell's
> invariant and it is enforced here by shape: no command accepts a path and no
> command hands one out, so a webview cannot ask this process to read a file the
> user did not choose in a panel.

**In-place save on the desktop is therefore not a missing feature; it is a
feature the current security shape forbids.** §3.2 is how this note gets it back
without giving that shape up.

### 1.4 The dirty-state primitive, and where it is wrong

`session_edits_applied()` counts edits up and never down — **[built]**,
`crates/casual-calc-wasm/src/history.rs:14`, whose doc comment (`:7-11`) says the
host records it at each save and compares afterwards. `markSaved()` and
`isDirty()` are that comparison (`webapp/editor.sheets.js:346-353`), and
`beforeunload` is wired to it (`webapp/editor.core.js:9099-9103`).

The design of that is right — the alternative, a tally in the editor, is a list
of every write path and is one omission from being wrong. But measured, the
counter has three properties a save design has to know about:

```
session_edits_applied() around no-op / undo / redo:
{"afterEdit":1,"afterSameValue":2,"afterUndo":2,"afterSecondUndo":2,"afterRedo":3}

CLAIM 1 — editor isDirty() across edit-then-undo:
{"atSave":false,"afterEdit":true,"afterUndo":true}
```

1. Re-setting a cell to the value it already holds counts as an edit (1 → 2).
2. **Undo does not decrement it** (2 → 2, and a second undo leaves it at 2).
3. Redo increments again (2 → 3).

So `isDirty()` reports **true** for a document the user has undone back to the
state it was saved in. That is the safe direction and the editor chose it
deliberately (`webapp/editor.sheets.js:336-340`), but it means an autosave keyed
naively on `isDirty()` writes snapshots for documents that have not changed, and
a recovery bar keyed on it offers to recover nothing. §4.1 says what to do
instead. This is not a defect in `session_edits_applied` — it does exactly what
its doc comment promises — it is a defect in reading it as "the file and the
document differ".

### 1.5 A measured finding that constrains autosave: undone text is written out

Comparing `session_save()` of an untouched new workbook against the same
workbook after one `set_cell` and one `undo`:

```
clean 1540 bytes / undone 1851 bytes
== clean.xlsx
  [Content_Types].xml       557    _rels/.rels               295
  xl/workbook.xml           283    xl/_rels/workbook.xml.rels 296
  xl/worksheets/sheet1.xml  245
== undone.xlsx
  [Content_Types].xml       693    _rels/.rels               295
  xl/workbook.xml           283    xl/_rels/workbook.xml.rels 437
  xl/sharedStrings.xml      199    xl/worksheets/sheet1.xml  245

sheet1 identical: True
sheet1: <worksheet …><sheetData></sheetData></worksheet>
sharedStrings in undone: <sst … count="1" uniqueCount="1"><si><t xml:space="preserve">typo</t></si></sst>
```

The undo restored the cells correctly — `sheet1.xml` is byte-identical and holds
no cells at all — but the interned string survives and the writer emits a
`sharedStrings.xml` part for it. **The text of an undone edit is written into the
saved file.**

For a one-off download that is a curiosity. For autosave it is not: every draft
snapshot would carry the text of everything typed and taken back during the
session, on a schedule, to durable storage. It is a row to file (§9, in the
`FID-` series) and it is a reason the recovery panel must let a user *delete* a
draft outright rather than only replace it.

---

## 2. The single rule

> **A document has one save target. `Ctrl+S` commits the document to that
> target and says where it went. It never creates a second document, and it
> never silently changes the target.**

**[proposed]** The save target is a resolved value, exactly like a capability
set, computed at open and reported alongside it:

| target | meaning | resolved when |
| --- | --- | --- |
| `file` | a path the user chose in a platform panel, held by the desktop shell | `__opencalcNative` is present *and* the shell holds an opened path |
| `handle` | a `FileSystemFileHandle` the user granted | the browser has one for this document and permission is granted |
| `host` | the host application decides | `ownsFile` is true |
| `server` | the collaboration server already holds it | a live collab session |
| `download` | a copy leaves, and the document keeps no target | nothing above applies |

`ownsFile` wins over everything, as it already does for `canOpen`
(`webapp/editor.core.js:954-962`). `server` wins over `file` and `handle`: when
the server holds the document, a client writing the file underneath it is a
second writer, which is the failure `wopi` mode's preset comment already names
(`editor.core.js:835-839`).

Three consequences make the four hosts one design rather than four cases.

**A target is acquired, never guessed.** When the target is `download` and the
user presses `Ctrl+S`, the editor does not download — it *acquires a target*:
the platform save panel on desktop, `showSaveFilePicker` in a browser that has
it, and a name prompt where neither exists. Downloading is what `File ▸ Download`
does. This is the whole of the `opencalc (1).xlsx` problem: today the keystroke
that means "keep my work" makes a new file every time.

**A refused save is still an answer.** `refuse()` already emits `commandRefused`
with the capability and `ownsFile` (`editor.core.js:1082-1090`), and a host
listening to it is how `Ctrl+S` reaches the host's own save. The target `host`
formalises what is already measured behaviour: the keystroke is not swallowed,
it is forwarded.

**Save is complete when the write is, not when it is started.** Today it is not
— see §5.1, which is a live defect this rule exists to prevent recurring.

---

## 3. `Ctrl+S` in the four hosts

### 3.1 Browser tab

**[proposed]** Three layers, in this order:

1. **Draft — every engine, always.** The `session_save()` bytes plus metadata,
   in IndexedDB, on the cadence in §4. Not a file, not a save; insurance.
2. **Handle — where the File System Access API exists.** `Ctrl+S` on a document
   with a granted handle writes that file in place. Measured available in
   Chromium: `showSaveFilePicker: "function"`, `showOpenFilePicker: "function"`,
   `FileSystemFileHandle: "function"`, `navigator.storage.persist: "function"`.
3. **Download — demoted.** `File ▸ Download` keeps every format it has, and
   `Ctrl+S` stops being a download.

**Browser specifics, stated precisely.** The File System Access API's
`showSaveFilePicker`/`showOpenFilePicker` ship in Chromium desktop browsers
(Chrome, Edge, Opera) and in no released Firefox or Safari, and in no mobile
browser including Chrome for Android. Firefox and Safari therefore get layers 1
and 3 only: a draft that survives a closed tab, and an explicit download. That is
a real two-tier product and this note is not going to pretend otherwise — it is
the cost of the decision and it is smaller than the cost of having no in-place
save anywhere.

*This paragraph is stated from knowledge of the platform, not measured here.*
Chromium was measured directly (above). The pinned Playwright build's Firefox and
WebKit binaries are not installed on this machine, so the negative half is
asserted rather than run — `SAVE-07` in §9 is the row to make it a gate.

**A handle costs a click per session.** A `FileSystemFileHandle` is
structured-cloneable and can be stored in IndexedDB, but the *permission* is not
persisted across a page load: after a reload the editor must call
`requestPermission()`, and that requires a user gesture. So "reopen the tab and
`Ctrl+S` writes the file" is one click away, not zero. The decision is to spend
that click **on the first save, not on boot** — a permission prompt at boot for a
document nobody has edited yet is the dialog everyone dismisses.

**A draft is evictable.** Measured: `navigator.storage.persisted: false`, quota
937–1100 MB. The editor asks for `navigator.storage.persist()` at the first
autosave — the point at which there is something to protect and the request has
a reason the user can evaluate. Refused persistence is not an error; the draft is
still written, and §5.5 says what happens when the space runs out.

### 3.2 Desktop (Tauri)

**[proposed]** `Ctrl+S` writes back to the file that was opened.

The invariant at `desktop/src/main.rs:23-26` stays, restated more precisely
rather than relaxed:

> A path may cross **into** this process only from a platform panel, and never
> crosses back out.

Concretely: `Shell` gains one slot beside `opened` and `staged`
(`desktop/src/main.rs:51-56`) — `target: Mutex<Option<PathBuf>>` — set by
`native_open` from the path it already has in hand at `:195`, and re-set by
`native_save` from the path the save panel returned at `:288`. One new command,
`native_save_target(bytes)`, writes to that slot. It takes no path and returns
only a base name, so the shape that enforces the invariant is unchanged: the
webview still cannot name a destination, it can only say "the one the user
already chose".

`guard_save()` still applies (`desktop/src/session.rs:144-150`), so a mode
without `canSaveAs` cannot reach it.

**The write is atomic**: a temporary file in the target's own directory, then a
rename. A save that fails part-way must not leave the user with a truncated
version of the file they had. This is the whole of §5.2.

**What it costs.** The shell now holds a path for the life of the window, which
is state the webview could previously not influence at all. The mitigation is
that the slot is only ever written from a value a platform panel returned, and
`take`-semantics are wrong here (unlike `staged`) precisely because the point is
that it persists — so it is the one piece of shell state that must be *cleared*
on `File ▸ New` and on an open that fails. Missing that clear is how a new
document overwrites the last one, and it is the acceptance test in §8.

### 3.3 Embedded / host-owned (`ownsFile: true`)

**[built, and correct]** Nothing changes about the refusal. Measured above:
`Ctrl+S` in `embedded` and `wopi` produces no download and emits
`{"id":"file.download","capability":"canSaveAs","ownsFile":true,"mode":"…"}`.

**[proposed]** One addition: the event gains the save target, so a host can tell
"the user asked me to save" from "the user asked for a copy and is not allowed
one". The host's save is *its* transport — WOPI's `PutFile` for the `wopi`
preset, whatever the embedder wants for `embedded`.

**[refused]** Autosave in any `ownsFile` mode. The host owns durability; an
editor autosaving underneath it is the second writer that
`editor.core.js:835-839` was written about, and under WOPI it would fight the
lock refresh cycle (`docs/74`, "Locks are refreshed on a timer"). The draft in
§4 is also not written: a host's document must not leave a copy in the user's
browser storage as a side effect of being opened.

### 3.4 Collaborative

Every keystroke is already durable on the server before `Ctrl+S` is pressed, and
the server already has an autosave policy — **[built]**,
`server/casual-calc-collab-server/src/lifecycle.rs:32-43`:

```rust
quiesce_ms: 5_000,      // milliseconds of no edits after which to save
ceiling_ms: 60_000,     // the longest a session may go without saving
every_revisions: 200,
```

So `Ctrl+S` cannot mean "make this durable". **[proposed]** It means **mark a
named version** — the point in the history the user wants to be able to come
back to. That is what the keystroke is *for* in a product where saving is
continuous, and it is what Sheets does with `Ctrl+S`.

The status line says so rather than pretending: "Saved — everyone's changes are
kept automatically. Version marked."

**The four hosts are one rule because the rule is about the target, not the
mechanism.** Each host commits to the most durable place it can reach; only the
place differs.

---

## 4. Autosave and crash recovery

### 4.1 What is written, where, how often

**[proposed]**

- **What.** The `session_save()` bytes — the document in its own format — plus a
  metadata record: draft id, document name, the format, `session_edits_applied()`
  at capture, wall-clock time, and the byte length. `session_save()` and not
  `session_save_native()`: a draft is not a deliverable, and writing a `.csv`
  draft would discard the second sheet the user is about to lose
  (`session_save_loss()`, `crates/casual-calc-wasm/src/io.rs:284`, says exactly
  what a native save cannot carry).
- **Where.** IndexedDB, one object store for metadata and one for bytes. Not
  `localStorage`: it is synchronous, string-only, and ~5 MB.
- **How often.** **The server's policy, copied exactly.** Quiesce 5 s, ceiling
  60 s. One set of numbers for all four hosts is the whole reason this is one
  design note rather than three.

The cadence is not a preference dressed up. It is forced by what a save costs.

### 4.2 What a save costs, and why the cadence is quiesce-first

Measured, in the running editor, bulk-filled by paste:

```
session_save(): {"rows":2000,"cells":20000,"saveMs":61,"snapshotKB":60}
session_save(): {"rows":10000,"cells":100000,"saveMs":150,"snapshotKB":291}
session_save(): {"rows":30000,"cells":300000,"saveMs":434,"snapshotKB":868}
five repeats at 300k cells: [{"ms":436,"kb":868},{"ms":429,"kb":868},{"ms":435,"kb":868},{"ms":425,"kb":868},{"ms":424,"kb":868}]
IndexedDB write of the 300k-cell snapshot: {"snapshotKB":868,"writeMs":8,"quotaMB":937,"usageMB":0.1}
```

and the effect on frames:

```
CLAIM 2 — main-thread block during save at 300k cells:
{"worstFrameGapIdleMs":18,"worstFrameGapDuringSaveMs":196,"budgetFor60fpsMs":16.7}
```

Three things follow, and they decide the design:

1. **Serialization is the cost; storage is not.** 424–436 ms to serialize,
   8 ms to write 868 KB to IndexedDB. Any design that optimises the storage
   layer is optimising the wrong 2%.
2. **`session_save()` blocks the main thread**, by a factor of twelve over the
   60 fps budget at 300k cells — and 300k cells is a third of the stated 1M-cell
   target (`docs/30`). It cannot run while somebody is typing.
3. **Snapshots are small.** 868 KB for 300k cells. A twenty-deep local version
   ring on a document that size is ~17 MB against a ~1 GB quota. Storage is not
   the constraint on how much history a browser can keep.

**Decision: autosave serializes on the main thread, at quiesce.** After 5 s of
no edits nobody is typing, so a 434 ms stall costs nothing anybody can see. The
ceiling is the case that hurts: under sustained editing in a large workbook the
user gets one ~0.4 s hitch per minute. That is accepted, and it is said out loud
here rather than discovered later.

**[refused, for now] Serializing in a worker.** It is the right long-term answer
and it is expensive: there is no cross-thread transfer of the model today, so a
worker means a second wasm instance and a way to feed it — which is a design of
its own and would hold up every phase behind it. `SAVE-06` (§9) is the row, and
the trigger for doing it is a measured complaint about the ceiling stall, not a
guess. Choosing the cheap correct thing now and naming its cost is better than
holding in-place save hostage to a threading design.

### 4.3 How a recovered document is presented

**[proposed]** The rule is one sentence: **a recovered document is offered, never
applied.**

On boot, the editor lists every draft it holds — it does not try to match one to
the document being opened. Matching by name is a guess (two files called
`budget.xlsx`, one on the desktop and one in Downloads) and a wrong guess here
silently hands a user somebody else's work. Instead, a bar appears when any draft
exists:

> **Unsaved work from an earlier session** — `budget.xlsx`, 14:32 today, 41 edits
> ahead of the last save. **[Review]** **[Discard]**

- **Review** opens the draft **as a separate document**, next to the one already
  open. The user compares and decides. Nothing is merged.
- **Discard** deletes that draft, after a confirmation naming the document.
- Doing neither leaves the draft. A draft is never deleted because the user
  ignored it.

"41 edits ahead" is why §1.4's finding matters. The draft records
`session_edits_applied()` at capture, so the bar can state a *difference* rather
than the word "unsaved" — and a draft whose count equals the count at the last
save is not offered at all, which is the case `isDirty()` alone gets wrong after
an undo.

**A recovered draft carries no undo history.** Measured:

```
CLAIM 3 — what survives a snapshot round trip:
{"canUndo":false,"canRedo":false,"undoLabel":"","editsApplied":0,"format":"xlsx"}
```

This is structural, not an oversight: `History`
(`crates/casual-calc-transaction/src/lib.rs:1262`) derives only `Debug, Default`
— no `Serialize` — so it cannot be persisted even by accident. The recovery bar
says so: "Undo history is not recovered." **[refused]** Making `History`
serializable to fix this: it would put the inverse of every operation into
durable storage for the life of a draft, which multiplies the privacy surface
§1.5 already found a hole in, for a feature nobody has asked for.

---

## 5. The failure modes, named

`docs/12` §3.19 says our failure states are where the product feels unfinished.
Each of these has a decided behaviour, not a "TODO: handle".

### 5.1 A save that fails — **and the one that is already broken**

**[built, and wrong]** On the desktop today, the document is marked clean before
the write is attempted. `doSaveNative()` calls `download(...)` at
`webapp/editor.sheets.js:405` and `markSaved()` at `:406`. `download()`'s native
branch is deliberately not awaited (`webapp/editor.clipboard.js:41-46`):

```js
// Deliberately not awaited: `download` is synchronous for every existing
// caller, and making it async would change five call sites into a shape
// where a forgotten `await` silently drops the save.
native.save(bytes, ext).catch((err) => console.error("[opencalc] save", err));
```

So `markSaved()` runs before the platform save panel has even appeared. A
cancelled panel, a refused capability, or a failed write all leave the document
marked **saved** — the title-bar bullet clears, `beforeunload` stops guarding,
and the only trace is a line in a console the user cannot see. This is exactly
the failure state `docs/12` §3.19 warns about, and it is a row (`SAVE-01`), not a
paragraph.

**[proposed]** The rule: **`markSaved()` is called by the completion of a write,
never by the start of one.** `download()` returns a promise; the save routes
await it; a rejection leaves the document dirty and puts the reason on screen,
naming the file's base name and the platform's error text. A cancelled panel is
not an error and produces no message, but it does not mark clean either.

### 5.2 A disk that is full

The atomic write in §3.2 is what makes this survivable: the temporary file fails
to write, the rename never happens, and the file the user had is untouched. The
message names the volume, not the exception. The document stays dirty, and the
next `Ctrl+S` retries.

The browser side has no equivalent — a `Blob` download's failure is the
browser's to report — which is one more reason `Ctrl+S` should not be a
download.

### 5.3 Two windows on one file

**[proposed] Detect on save, not on open.** The shell records the target file's
size and mtime when it opens or writes it. Before an in-place save it compares.
If they differ, the save is refused and the user is offered **Save As** or
**Overwrite**, with the other file's mtime named.

**What that costs, stated plainly:** two windows can both edit the same file, and
only the second one to save is told. A lock file would prevent that, and lock
files strand documents when a process dies — every user of a shared drive has met
one. Detection degrades to an informed choice; a stale lock degrades to a file
nobody can open. **[refused]**, therefore: lock files.

### 5.4 A file changed underneath us

The same mechanism, and it is why the mechanism is mtime-and-size rather than
"did we write it last". A file changed by another application, by a sync client,
or by the user's own second window all present identically and all get the same
refusal.

### 5.5 A quota exceeded

An IndexedDB write can reject with `QuotaExceededError` at any time — measured
quota is 937–1100 MB and the browser may reduce it. **[proposed]** In order:

1. Delete the oldest *version-ring* entries for the current document (§6), which
   is what the ring is bounded for.
2. Delete drafts for documents the user has already discarded.
3. If a write still fails: **stop autosaving and say so persistently** — a
   standing indicator in the status bar reading "Autosave off — no storage
   space", not a toast. `Ctrl+S` still works; the draft does not.

**Never**: silently stopping, or deleting a draft the user has not seen.

### 5.6 A browser that denies the handle

`requestPermission()` returns `"denied"`. **[proposed]** The save target falls
back to `download` **for this session only**, the editor says the target changed
("This tab can no longer write to `budget.xlsx`. `Ctrl+S` will download a copy
instead."), and the handle is kept — a denial is often a mis-click, and the next
session asks again. The draft continues either way, which is the point of the
draft being the bottom layer and not the top one.

### 5.7 A draft that will not open

A truncated or corrupt draft — a crash mid-write is the realistic cause. The
recovery panel keeps the entry, marks it unreadable, and offers the raw bytes as
a download so a user can take them to another tool. It is not deleted
automatically. A recovery feature that silently discards the thing it failed to
recover is worse than no recovery feature.

### 5.8 Two tabs on one draft

Two tabs of the same document autosaving to one draft id would interleave
snapshots from two different documents under one name. **[proposed]** A
`BroadcastChannel` lease: the first tab to claim a draft id keeps it; a second
tab claims a new one and its recovery entry is labelled with the time, so the
panel shows two entries rather than one corrupted one.

---

## 6. Version history

### 6.1 The negative result: the op log is not a history

`HIST-01` and `docs/12` §3.19 both say the collaboration server's append-only op
log "*is* a history; nothing reads it as one". Measured against the code, that is
not true, and the reasons are not small.

**[built]** `ServerSession` is
`{ revision: u64, log: Vec<Operation>, first: u64, accepted: BTreeMap<…> }`
(`crates/casual-calc-transaction/src/session.rs:498`). Four things follow:

1. **There are no timestamps.** Not in `Operation`, not in `WireOperation`, not
   in `Submission`. `ServerSession::commit` takes no clock at all. A history
   whose entries cannot be dated is not a history.
2. **There is no per-revision author.** The only identity in the log is
   `accepted: BTreeMap<ClientId, (seq, revision)>` — the *last* chunk per client,
   overwritten on every submission. Attribution of a given revision to a person
   is not recoverable. (The Redis log carries a node and client id per *batch*,
   and nothing maps a `ClientId` to a user durably.)
3. **The retained window is minutes, not versions.** `compact_behind`
   (`session.rs:845`) drains everything older than
   `every × retain_intervals` — with the defaults, ~400–600 operations behind
   head, and the drained ops are gone. In cluster mode the Redis list is capped
   at `LOG_MAX_ENTRIES = 10_000` batches (`cluster/redis.rs:152`) with
   `LOG_TTL_MS = 60 * 60 * 1000` (`:159`), refreshed on append — one idle hour
   deletes the log.
4. **Nothing is persisted.** Snapshots live in RAM, and the whole
   `DocumentSession` is evicted `idle_eviction_ms` after the last participant
   leaves — default **30_000 ms** (`server/…/net.rs:208`). Thirty seconds after
   everyone closes the tab, the log does not exist.

None of that is a defect. The log is a **resume buffer** and it is correctly
scoped for resume: `oldest_rebasable()` (`session.rs:555`) is exactly the
guarantee `docs/61` needs, and asking for anything older is refused loudly with
`Refusal::TooFarBehind`. It has the *shape* of a history and none of the
properties of one. **Reading it as free history is the mistake this section
exists to prevent.**

### 6.2 The constraint that settles the design: `COL-50`

`COL-50` (Open, P1, `docs/14`): an insert meeting a delete does not converge for
a formula **range** — `=SUM(A1:A8)` settles as `A1:A8` in one order and `A1:A7`
in the other, "and each is the answer Excel gives for the sequence that produced
it". 68 pairs, one shape. It needs no formula in flight; two ordinary concurrent
structural edits are enough.

Separately, `transform` can refuse outright:
`TransformError::Unsupported { subject, against }`
(`crates/casual-calc-transaction/src/transform.rs:90`).

**A log containing operations that cannot be transformed cannot be replayed**, and
a log whose replay order changes the answer cannot be replayed *reproducibly*. So:

> **[decided] Version history is snapshot-based. It never replays the op log.**

A snapshot is bytes; restoring one requires no transform and no ordering
argument. This is `COL-50` and `TransformError` constraining the design, and it
is the reason to build the expensive-looking thing: the cheap-looking one is not
merely riskier, it is unavailable.

It also means history is **not blocked on `COL-50`**, which matters for the
schedule.

### 6.3 Storage, retention, naming

**[proposed]**

| | local (no server) | with the collaboration server |
| --- | --- | --- |
| where | IndexedDB, this browser | server-side store, next to the document |
| what | `session_save()` bytes + metadata | the same |
| automatic ring | last 20 autosave snapshots, or 50 MB, whichever binds first | policy of the deployment |
| explicit versions | one per `Ctrl+S`, kept until deleted | the same |
| named versions | user-supplied name, never evicted by the ring | the same |
| attribution | this device only | **not yet** — see below |
| survives | tab close, browser restart, crash | everything |
| does not survive | cleared site data, a different browser, a different machine | — |

**Naming.** Three tiers, in the user's words: *autosave* (time only), *saved*
(time, from an explicit `Ctrl+S`), *named* (the user typed a name). Only the
first is evictable by the ring. This is Sheets' model and it is the right one —
the automatic entries are noise until the moment they are the only thing left.

**Attribution is refused for now.** `HIST-02` is the row for change tracking, and
§6.1(2) is why it cannot be lifted out of the existing log. A history that showed
version times but not authors is honest; one that guessed authors from
`accepted` would be wrong in exactly the multi-editor case it is for.

**What it degrades to without a server**, said plainly and in the UI: a
per-device, per-browser list that clearing site data destroys. It is labelled
**"Recent versions (this browser)"** and never "Version history" — the second
promises something Sheets delivers and this does not.

### 6.4 Restore semantics

> **[decided] Restoring a version is a new operation, never a rewrite of
> history.**

This is not a preference. Revisions are positional — `log[i]` is what took the
document from `first + i` to `first + i + 1` (`session.rs:498-501`) — and every
connected client, every resume key, and `oldest_rebasable()` are all defined
against that numbering. Rewriting it invalidates all three at once, silently, on
every other participant. Under `ADR-011`'s OT only one of the two options is
available, and it is this one.

Concretely: restore computes the difference between the current document and the
target snapshot and submits it as ordinary edit operations. Consequences, all of
them correct:

- The restore is itself undoable.
- Co-editors see it as edits arriving, because that is what it is.
- The version list gains an entry ("Restored to *14:32 today*"), so the restore
  is itself in the history.
- Nothing is ever deleted from the past.

**[refused]** Deleting a version from history. A named version can be *hidden*
from the list; its bytes stay until the ring evicts them. "Delete this version"
is a promise about someone else's copy that a distributed system cannot keep.

---

## 7. What this note deliberately does not do

- **No merge of a recovered draft into the open document.** Offer and compare,
  never merge. There is no three-way merge for a spreadsheet that is right often
  enough to be applied without being read.
- **No per-cell edit provenance** (`HIST-02`). It needs a durable attributed log,
  which §6.1 shows does not exist and this design does not build.
- **No autosave, and no local draft, in `ownsFile` modes.** §3.3.
- **No worker-based serialization in the first three phases.** §4.2, `SAVE-06`.
- **No lock files for the two-windows case.** §5.3.
- **No serializable undo history.** §4.3.
- **No history rewrite and no version deletion.** §6.4.
- **No cloud storage of local drafts.** A draft never leaves the machine it was
  made on. Making it leave is a different product with a different privacy
  posture, and §1.5 is a reminder of what would leave with it.
- **`Ctrl+S` is not made a no-op anywhere**, including collaboration. A keystroke
  that does nothing reads as a broken editor — the reasoning already written at
  `editor.core.js:7070-7075`.

---

## 8. Build sequence

Three phases, each shipping something a user can see on its own, in the order
that puts the most-used host first.

### Phase A — desktop in-place save

**Delivers:** `Ctrl+S` writes back to the file that was opened. The title bar's
bullet clears when the write completes and not before.

- `SAVE-01` first, alone: the write completes before `markSaved()`. It is a live
  defect on the current desktop build and it is a prerequisite — an in-place save
  whose failure is invisible is worse than a download.
- `Shell.target`, `native_save_target`, the atomic write, the clear on
  `File ▸ New`.
- The mtime/size check of §5.3–5.4.

**Acceptance:** a Tauri test that opens a fixture, edits, presses `Ctrl+S`, and
asserts the bytes on disk at the *original path* changed and the title bullet
cleared; plus one that makes the write fail and asserts the document is still
dirty. Both must be seen red before the fix.

### Phase B — browser draft, recovery, and the handle

**Delivers:** closing the tab stops losing work; and in Chromium, `Ctrl+S` writes
a real file.

- The IndexedDB draft store, the quiesce/ceiling cadence copied from
  `lifecycle.rs:32-43`, and the recovery bar of §4.3.
- `showSaveFilePicker` acquisition, handle persistence, the permission click.
- The target resolution of §2, and `Ctrl+S` stops being a download.
- The quota ladder of §5.5.

**Acceptance:** a browser test that edits, kills the page without a clean
unload, reloads, and asserts the recovery bar appears with the right edit
delta — and that the document on screen is the *file*, not the draft, until
Review is clicked.

### Phase C — version history

**Delivers:** a past.

- The version ring, the three naming tiers, the panel.
- Restore-as-new-operation, and the "Restored to …" entry.
- The server-side store, and the honest local degradation of §6.3.

**Acceptance:** restore a version in a two-client collaborative session and
assert both replicas converge on the restored content **and** that revision
numbers only ever increased — the property that distinguishes a new operation
from a rewrite.

Phase C is last because it is the only one that needs the other two: a history
with no autosave has nothing to keep.

---

## 9. Rows to file

Ids checked against `docs/14`, `docs/14a`, `docs/53` and `docs/67`. `SAVE-` is an
unused prefix; `HIST-01`/`HIST-02` exist, so history rows continue from `-03`.

| id | title | why | P |
| --- | --- | --- | --- |
| `SAVE-01` | A failed or cancelled desktop save still marks the document saved | `editor.sheets.js:405-406` calls `markSaved()` immediately after `download()`, whose native branch is deliberately not awaited (`editor.clipboard.js:41-46`). The bullet clears and `beforeunload` stops guarding before the panel has appeared. Found by reading; needs a test that fails the write. | **P1** |
| `SAVE-02` | Desktop in-place save (Phase A) | §3.2. Extends the shell with a `target` slot and one command; the "bytes, never paths" invariant is restated, not relaxed. | P1 |
| `SAVE-03` | Browser draft + crash recovery (Phase B) | §4. Cadence copied from `lifecycle.rs:32-43`. | P1 |
| `SAVE-04` | File System Access handle as a save target (Phase B) | §3.1. Chromium only; the two-tier outcome is the accepted cost. | P2 |
| `SAVE-05` | `saveTarget` resolution and the `Ctrl+S` rule (§2) | The seam the other rows depend on; extends `resolveCapabilities()` rather than paralleling it. | P1 |
| `SAVE-06` | Serialize autosave off the main thread | §4.2. `session_save()` measured at 424–436 ms for 300k cells, 196 ms worst frame gap. Deferred deliberately; needs a cross-thread model design. Trigger is a measured complaint, not a guess. | P3 |
| `SAVE-07` | No gate asserts which browsers have the File System Access API | §3.1's negative half is asserted, not measured — the pinned Playwright build's Firefox and WebKit binaries are absent locally. A capability probe in `browser-smoke` across all three engines would make it a fact. | P3 |
| `SAVE-08` | Snapshot-based version history (Phase C) | §6. Explicitly not log replay; `COL-50` and `TransformError::Unsupported` are why. Continues `HIST-01`'s work. | P1 |
| `SAVE-09` | `HIST-01` and `docs/12` §3.19 both state the op log "is a history"; measured, it is not | §6.1: no timestamps, no per-revision author, ~400–600 ops retained, evicted 30 s after the last participant leaves, never persisted. Under `docs/14`'s rule this is a row, not a doc edit — and it changes `HIST-01`'s estimate materially, because the storage it assumed exists does not. | P2 |
| *(FID series)* | The text of an undone edit is written into the saved `.xlsx` | §1.5, measured: `set_cell("typo")` then `undo` yields a file whose `sheetData` is empty and whose `sharedStrings.xml` contains `<t>typo</t>`. A one-off download makes it a curiosity; autosave makes it a schedule. Number to be assigned from the live `FID-` sequence when the row is filed. | P2 |

`HIST-01` should not be closed by this note — it is the row this note is the
design for, and it moves to `Designed` with `SAVE-08` carrying the build.

**Why the last two rows carry a `SAVE-` number rather than continuing the
`HIST-` series.** They are that series' natural continuation and arguably belong
in it. But `tools/check-doc-references.py` fails on any id whose *prefix a
tracker already uses* and whose row does not exist yet. `HIST-` is such a prefix;
`SAVE-` is used by no tracker and so is not checked. Drafting this section with
the next two `HIST-` numbers turned the gate red on an otherwise clean tree —
run and confirmed, two errors, one per id — which is the gate doing exactly its
job: a document must not cite a row that has not been filed.

Whoever files these may move them into the `HIST-` series in the same commit that
creates the rows, and update the citations here at the same time. The ordering is
the whole point: **the row exists before the id is cited**, never the other way
round.

---

## 10. Reproducing the measurements

An editor was already served at `http://127.0.0.1:8123/editor.html` (the
repository's own `webapp/serve.py`); no second server was started. The probes are
Playwright scripts driving that page and reading the wasm surface directly
through `window.opencalcEditor.wasmApi()`.

- Capability/keystroke sweep: boot at `?mode=…` for each of the five presets,
  press `Ctrl+S`, record `page.on("download")` and the `commandRefused` events.
- Save cost: `session_new()`, then `session_paste_tsv` of *n* × 10 cells, then
  `performance.now()` around `session_save()`; five repeats at the largest size.
- Frame gap: a `requestAnimationFrame` loop recording the worst inter-frame gap,
  once idle and once with a `session_save()` in the middle.
- Undo residue: `session_save()` before and after `set_cell` + `undo`, written to
  disk and compared entry-by-entry with Python's `zipfile`.

**A note on line numbers.** Every `webapp/` citation above is against **`main`**,
which is what the served editor was built from and what this document lands on.
The branch this note was written on is behind `main` on `webapp/` only — `main`
has a seventh capability axis, `canShare`, from `529d0a7` (merged), where the
branch has six. So a reader checking these citations against an older `webapp/`
will find them offset by a few dozen lines; against `main` they resolve.
Citations into `crates/`, `server/` and `desktop/` are byte-identical in both and
resolve either way.

---

## References

- [12 — Competitive Analysis](12-COMPETITIVE-ANALYSIS.md) §3.19, §8 item 2
- [14 — Execution Tracker](14-EXECUTION-TRACKER.md) — `HIST-01`, `HIST-02`, `COL-50`
- [44 — Tauri Desktop Shell](44-TAURI-DESKTOP-SHELL-DESIGN.md), [81 — Desktop Shell Composition](81-DESKTOP-SHELL-COMPOSITION.md)
- [55 — SDK Embedding & Integration](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md), [78 — Host Capability Seams](78-HOST-CAPABILITY-SEAMS.md)
- [56 — Collaboration Concurrency](56-COLLABORATION-CONCURRENCY-DESIGN.md) (ADR-011), [61 — Collaboration Resume](61-COLLABORATION-RESUME.md)
- [69 — Collaborative Undo Policy](69-COLLABORATIVE-UNDO-POLICY.md), [74 — WOPI Integration](74-WOPI-INTEGRATION.md)
- [30 — Performance & Capacity Targets](30-PERFORMANCE-AND-CAPACITY-TARGETS.md)
