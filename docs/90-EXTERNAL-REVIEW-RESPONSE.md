# 90 — An external review, taken apart

## Outcome

A reviewer read the repository and made eleven recommendations. **Four are
adopted, three are already done, three are modified, and one is refused** — and
the refusal is the one that would have looked like the most work.

The rule applied throughout: a recommendation is checked against the tree before
it is agreed with. Two of the strongest-sounding items turned out to be already
built, and one turned out to describe automation that cannot exist here. Neither
is a criticism of the reviewer, who could not have known; it is the reason this
note exists rather than a checklist of nods.

---

## Already done, verified rather than assumed

### The SDK API is already asynchronous

> *"Bad future API: `sheet.getCell()`. Better: `await sheet.getCell()`. That
> decision is much harder to change after 1.0."*

Correct, and already the case. Every method on the published surface that
touches the workbook is `async`: `open`, `save`, `run`, `listCommands`,
`commands`, `configure`. The five synchronous ones — `chrome`, `theme`,
`setColorScheme`, `resetTheme`, `connectedCallback` — set CSS variables on the
host element and would never cross a worker boundary.

It was not luck. [55](55-SDK-EMBEDDING-AND-INTEGRATION-DESIGN.md) §12 decision 8
defers the Web Worker *because* "the promise-returning API means moving the
engine into a worker does not change the public surface". The door was left
open on purpose.

### The IR boundary is an accepted ADR

> *"Make the conceptual boundary explicit … then establish a hard rule."*

`ADR-022` — detection below the format crates, dispatch above them — is that
rule, accepted. `ADR-023` governs what earns a crate. What is missing is not a
decision; it is a **diagram**, which is a documentation task and is taken below.

### Collaboration has not infected the model

The reviewer noticed this and was right. It is now `ADR-025`, raised because the
underlying invariant had been rediscovered as a defect three times.

---

## Adopted

### 1. Invariants graduate from commit messages into ADRs

**The strongest point in the review, and it came with evidence once looked for.**
The rule "an interned id is replica-local" appeared in **zero of twenty-four
ADRs** and had been found as a defect three separate times — `COL-12` for
strings, `COL-62` for rich-text runs, and designed around in `HIST-02` for
authors. Two shipped defects and one avoided.

`ADR-025` records it in the shape the review asked for: invariant, decision,
tradeoff, future constraint. Two more follow (`DOC-052`).

The reviewer's split is adopted as a working rule:

    commit — discovery, evidence, implementation detail
    ADR    — invariant, decision, tradeoff, future constraint

### 2. The compatibility outcome vocabulary

`PASS` / `VISUAL_DIFF` / `SEMANTIC_DIFF` / `REPAIRED_BY_EXCEL` /
`FEATURE_DROPPED` / `CORRUPTED` is a better vocabulary than the one in use,
which is a boolean. **`REPAIRED_BY_EXCEL` is the valuable one**: a file Excel
opens *after silently repairing it* currently passes every check here and is not
a pass, and nothing in this repository can presently say so.

Adopted for the oracle that can run unattended, and for the manual checklist
below. Filed as `IO-12`.

### 3. The README understates the repository

Two stars against hundreds of tests, an engine, collaboration, WOPI, three SDK
packages and desktop builds. The reviewer's point is that discoverability lags
engineering, and it is correct. Filed as `DOC-053`.

### 4. Do not rush 1.0, and say what 1.0 requires

Already policy; what is adopted is writing the **precondition list** down —
external integrations, corpus families, stable command/event/serialization/
access-control APIs, worker-ready architecture — so that "not yet" is a
measurable position rather than a feeling. Filed into `DOC-053`.

---

## Modified

### The Web Worker: the concern is right, the priority is already evidenced

> *"I would prioritize moving the WASM engine toward a Web Worker."*

The reviewer argues from first principles that large sheets will block
interaction. **This repository has already measured it**: `SAVE-06` records
424–436 ms serializing at 300k cells, and a 196 ms worst frame gap against 18 ms
idle — twelve times the 60 fps budget `PERF-D-01` fought for.

So the concern is not speculative and does not need to be argued. What the
review misses is that the *API* half is already done (above), which is the half
that is expensive to change later. The remaining half is moving the engine,
which is a project rather than a decision, and it now has a measured trigger
rather than a hunch. No new row: `SAVE-06` and `docs/55` §12 decision 8 already
hold it, and duplicating them would be the drift this repository keeps gating
against.

### Positioning the SDK as the primary product

A product decision, not an engineering one, and the product owner's to make. The
engineering consequence — that the SDK's contract must be stable before it is
marketed as the front door — is already `DOC-034` and the 1.0 preconditions
above.

### "Formalize the IR" as new work

Modified into a diagram in the README (`DOC-053`) rather than a new
architectural exercise, because the decisions already exist and are accepted.
Writing them a second time in prose would create two statements of one fact,
which is the failure mode `REL-04`, `REL-05` and `check-theme-tokens` all exist
to prevent.

---

## Refused

### `compat/excel-2016/ … excel-365/`

> *"For every meaningful capability: OpenCalc → XLSX → Excel; Excel → XLSX →
> OpenCalc; …"*

**The classification is adopted and the directory tree is refused**, because
that tree implies automation that cannot exist here.

Running Excel requires a licence per version, a Windows runner with Office
installed, and automation of an application with no supported headless mode. CI
here runs on GitHub-hosted runners, where the organisation's entire allowance is
twenty concurrent jobs (`CI-030`). None of that is a reason to give up on Excel
fidelity; it is a reason not to build a directory layout that **looks
automated and is not**.

That is the specific failure this repository keeps catching in itself: a green
check that answers a narrower question than it prints. A `compat/excel-365/`
directory populated by hand, on somebody's laptop, at whatever moment they last
remembered, is exactly such a check — and it would be trusted more than an
honest "we have not tried this in Excel", because it has a folder.

**What is built instead** (`IO-12`): the six outcome classes applied to the
LibreOffice oracle, which already runs unattended in three modes, and a manual
Excel checklist that is *named as manual*, carries the version and date it was
last exercised, and goes stale visibly.

---

## What the review got right that is uncomfortable

Two of these are worth stating plainly rather than filed away.

**Architectural knowledge really is in the commit log.** The rebuttal —
"`AGENTS.md` says long reasoning belongs beside the diff" — is true and does not
answer the point. Beside the diff is right for *why this code is like this*. It
is wrong for *what may never be done*, because the second is needed by somebody
who is not reading that diff.

**The desktop score of 5.5/10 is fair.** In the days around this review the
desktop shipped a build macOS reported as damaged (`REL-02`), a `.deb` that
could not install on the most common LTS (`REL-03`), `TODAY()` returning 1899
(`CALC-VOL-01`), and clipboard chords running document commands mid-edit
(`TAURI-016`). Every one was found by a user rather than by a test. That is what
5.5 measures, and the number is not the interesting part — the pattern is: the
desktop has the thinnest automated coverage of anything here, and it is the
surface people actually touch.
