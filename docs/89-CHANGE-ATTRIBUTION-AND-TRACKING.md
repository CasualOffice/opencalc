# 89 — Who changed this cell

## Outcome

**The model carries no authorship, and that is the whole problem.** Not a
missing panel, not an unwired feature — an `Operation` has no author, the wire
has no author, and `undo_would_discard` says so out loud in
`casual-calc-transaction/src/lib.rs:437`: *"that is also what makes the check
need no authorship, which the model does not carry."* Every question `HIST-02`
asks bottoms out there.

So this note is mostly about **where an author comes from and what it costs to
keep**, and only then about what to draw. It proposes one slice —
`lastEditedBy` per cell, retained on the wire and shown on hover — and
**refuses** Excel's Track Changes, with the reasoning written down rather than
left as an omission.

### How to read the markings

- **[built]** — in the tree today, cited `file:line`.
- **[proposed]** — this note's decision, not yet code.
- **[refused]** — considered and declined, with why.

---

## 1. What exists, and what it stops at

**[built] Identity exists at the connection.** `ADR-014` has the server verify a
host-signed JWT against a JWKS endpoint, and `presence.rs:47` carries a
`Presence { name, color, … }` per participant. The editor draws it: faces,
initials, a cursor per person (`editor.presence.js:172`).

**[built] Live editing is broadcast.** `COL-35` sends an in-progress edit to
peers, so you see somebody typing in a cell.

**[built] Nothing is retained.** The moment that edit lands it becomes an
`Operation`, and an `Operation` has no author field. Presence is *ephemeral by
construction* — it answers "who is here now", never "who did this".

**[built] The op log cannot answer it either.** `SAVE-09` measured that: no
timestamps anywhere, no per-revision author, 400–600 ops retained, evicted 30
seconds after the last participant leaves. It is a resume buffer. Reaching for
it is the mistake `HIST-01` made and this note will not repeat.

**[built] Version history is snapshots.** `SAVE-08`, `HIST-01`, `HIST-03`: a
version is a whole workbook, stored per document, compressed by the host. It
answers *what the document was*, and by design says nothing about *who*.

So the gap is exact: **the identity is known at the moment of the edit and
thrown away one layer later.**

---

## 2. Two features, routinely confused

`HIST-02` opens by separating them, and the separation is the reason this note
can propose something small.

| | question | Excel | Sheets |
| --- | --- | --- | --- |
| **History** | what was the document | AutoRecover + SharePoint versions | named versions |
| **Attribution** | who last changed this cell | Track Changes | "Edited by X" on hover |
| **Review** | is this change accepted | Track Changes accept/reject | suggesting mode |

History is done. **Attribution is the cheap one and answers most of the
question people actually ask.** Review is a workflow, and §5 refuses it.

---

## 3. Where an author comes from

An author has to be attached at the only place that knows it, and there is
exactly one: the moment a local edit is created, in the session that created it.
Anywhere later is a guess.

**[proposed] `Operation` does not grow an author field.** It is the wrong home
for three reasons and each is load-bearing:

1. **Every `Operation` is on the undo stack.** An author on the operation is an
   author on every inverse, doubling a field nobody reads on the way back.
2. **`Operation` is the wire format.** `CHT-07` established that adding a
   variant is a loud break and adding a field inside a whole-vector replace is a
   *quiet* one. Authorship on the operation would be the quiet kind.
3. **The author is a property of the *arrival*, not of the edit.** The same
   operation replayed from a version restore was not authored by whoever is
   restoring it — `restore_version` applies a batch of ordinary edits
   (`SAVE-08`), and stamping those with the restorer's name would rewrite
   history rather than record it.

**[proposed] It travels beside the operation, on the envelope.** The wire
already carries `WireOperation { op, strings, formulas, styles, runs }` — a
container for what the operation *refers to* rather than what it is. An
`author: Option<AuthorId>` there is the same shape as `runs` in `COL-62`: absent
means "not stated", an old peer skips the key, and **`PROTOCOL_VERSION` does not
move** for the same reason it did not then.

---

## 4. What is kept, and what it costs

This is where a design either survives 1M cells or does not.

**[refused] A per-cell author string.** `Cell` is the hottest struct in the
model. A `String` per cell at a million cells is a million allocations to say
"Priya" a million times, and the store is already careful enough that
`PERF-11` changed how *every reference* is stored to avoid exactly this.

**[proposed] An interned `AuthorId(u32)` and a side table.** The same shape the
model already uses for strings and styles: `Cell` grows one `Option<AuthorId>`
— four bytes, and `Option<NonZeroU32>` makes it four, not eight — and the
workbook holds `authors: Vec<Author>` where `Author { name, id }`. Ten
collaborators cost ten entries.

**[proposed] A timestamp is *not* per cell.** "Edited by Priya" is the question
people ask; "edited by Priya at 14:32:07" is a question they ask about one cell,
occasionally. Per-cell time is eight more bytes on every cell to answer it
always. Instead the **version** carries the time (`Version::captured_at_ms`,
already built), and the answer is "changed by Priya, some time after the version
you are looking at". Excel's own Track Changes stores time per change and is
also the feature people turn off because of what it costs the file.

**[proposed] It is not persisted to `.xlsx`.** OOXML's revision tracking is a
different, larger format and `IO-*` would have to round-trip it. Attribution
lives in the session and in the version store, and a saved-and-reopened file
starts blank. **Said out loud in the UI**, because attribution that silently
vanishes on save is worse than none: the panel says "since this file was
opened".

---

## 5. What this refuses

**[refused] Excel's Track Changes.** Accept/reject is not a display of who did
what; it is a *workflow* with a review state per change, a merge model, and a
document that is simultaneously two documents. It needs its own storage, its own
conflict rules against OT (`ADR-011`), and a UI with three modes. It is a
quarter of a product, and proposing it inside a row about attribution is how a
P2 becomes a year. If it is ever built it gets its own note.

**[refused] Sheets' full edit history — every change, forever.** That is the op
log idea wearing a different hat, and `SAVE-09` already measured why it does not
exist: retaining every operation is unbounded, and bounding it makes it a
resume buffer again. Last-writer-per-cell is bounded by the sheet, which is the
property that makes it affordable.

**[refused] Attribution without a collaboration server.** In a local session
there is one author and it is you; a name would be noise. `capabilities` already
distinguishes the modes, and the marker simply does not appear.

---

## 6. What a user sees

**[proposed]** Nothing new in the chrome. `docs/88` spent a note establishing
that the chrome has a fixed budget and this feature does not earn a region.

- Hovering a cell that somebody else last changed adds one line to the tooltip
  that already explains errors and numbers-stored-as-text
  (`DATA-NT-01` built that hover): *"Last changed by Priya."*
- The existing per-participant colour (`presence.rs:51`) tints that line, so the
  name and the cursor you saw are visibly the same person.
- **No marker on the cell.** The grid already draws a red corner for errors and
  a green one for numeric text; a third would make a shared sheet look like a
  defect list. The information is available on ask, which is what it is worth.

---

## 7. The work, ranked

| # | what | depends on |
| --- | --- | --- |
| 1 | `Author`/`AuthorId` in the model, interned, with `Cell.last_edited_by` | — |
| 2 | The session stamps local edits with its own author | 1 |
| 3 | `author` on `WireOperation`, no version bump, old peers skip it | 1 |
| 4 | The server relays it unchanged — it is not the server's to invent | 3 |
| 5 | The editor's hover line, tinted by presence colour | 1, 2 |
| 6 | A version restore does **not** re-stamp; the original author survives | 1 |

Item 6 is the one to get wrong. `restore_version` applies ordinary edits, and
the naive implementation stamps them with whoever pressed the button —
rewriting the past into a single author's name. There should be a test that
restores somebody else's work and asserts the attribution did not move.

---

## 8. The open question

**Is a name enough, or does this need a stable identity?**

`Presence.name` is a display string the host supplies. Two people called *Alex*
are one author under a name and two under a JWT `sub`. Using `sub` is more
correct and means the model holds something that identifies a *person*, which is
a privacy surface a spreadsheet did not previously have — and `AGENTS.md` puts
that decision with the product owner rather than with an implementer.

The proposal here is **the host's opaque author id, defaulting to the presence
name**, so a host that has real identity can supply it and one that has not is
no worse off than today.
