# 78 — How the engine reaches the platform

**For** `ADR-019`. **Status: Accepted** — the register
([08](08-ADR-REGISTER.md)) is where an ADR's status lives, and it records
ADR-019 as Accepted; the tracker row of the same name is `Done`. Nothing here
is built *as a consequence*: it describes what was built, argues that it is
right, and asked for the decision the register had been carrying as pending
since Phase 0. What the decision unblocked is `TAURI-001`
([44](44-TAURI-DESKTOP-SHELL-DESIGN.md) §Open decisions).

## The promise, and what happened instead

Four documents state, in the present tense, that the engine depends on a small
**host-capability trait** — [19](19-WORKSPACE-SCAFFOLD-DESIGN.md) boundary
invariant 7, [02](02-ARCHITECTURE.md) §Host targets,
[40](40-FORMULA-AND-CALC-ENGINE-ARCHITECTURE.md), and until recently
[44](44-TAURI-DESKTOP-SHELL-DESIGN.md). The ADR register lists it as pending,
"to be Accepted at Phase 0".

Phases 0–1E, 2 and 3 have shipped. **No such type exists** — the name appears
only in prose. What exists is three separate things:

| Capability | How it enters the engine | Shape |
| --- | --- | --- |
| Clock | `Environment`, installed onto `Workbook::volatile_now` | a **value** |
| Cancellation | `Cancel`, implemented for any `Fn() -> bool` | a **predicate** |
| Threads / parallelism | nothing | absent |

One `std::thread` exists in the entire workspace, in a stack-depth test.

## The question this actually asks

Not "why was the trait never written". The interesting question is whether the
thing that grew instead is **worse**, and it is not. It is better, for reasons
that only became visible by building it.

### A single trait bundles things that have nothing to do with each other

A clock, a cancellation source and a thread pool share one property: the engine
cannot supply them itself. That is not enough to make them one interface.
Anything implementing a combined trait implements *all* of it — including the
browser host, which has no threads to offer and would supply a stub for a third
of the contract. A stub in a trait implementation is a lie the type system
helps you tell.

### Each seam is narrower than a trait method

`Instant::now` **panics** on `wasm32-unknown-unknown`, which is the target that
needs cancellation most. A trait method `fn now(&self) -> Instant` cannot be
implemented there at all. `Cancel` sidesteps it: the token is `Fn() -> bool`,
so a browser closes over `performance.now()`, a native host over `Instant`, a
test over a counter, and `casual-calc-model` never names a clock type.

That is not a workaround. It is the seam being the right size — a *question*
("should I stop?") rather than a *capability* ("here is a clock").

Likewise the clock is a **value**, not a method, because the thing the engine
needs is what `TODAY()` should say, resolved once. A method would let it change
mid-recalculation, which is a determinism defect waiting to be written.

### A seam for a capability nothing uses is speculative

There is no threading seam because nothing in the engine is parallel. Adding
one now means designing an interface against no caller, on a target that cannot
implement it. When parallel recalculation is built it will say what it needs;
guessing first is how you get an interface that is wrong and hard to change.

## What the invariant should say instead

The *intent* of boundary invariant 7 is kept, and is worth keeping — it is
simply not a statement about a trait. The rule that holds:

> **No engine crate names a platform.** There is no `#[cfg(target_*)]` in
> `crates/`. Anything the engine cannot compute for itself enters as a value or
> a predicate the host supplies, in the narrowest form that works.

That is checkable, so it is checked: `tools/check-host-seams.py` fails if an
engine crate acquires a `cfg(target_*)`. It passes today at **zero**
occurrences, which is the point — the invariant was being kept by care, and is
now kept by CI.

**One real divergence, named rather than hidden.** `casual-calc-render` gates
text shaping behind a Cargo *feature*, on for native and off for WebAssembly
(ADR-018). That is a build difference, and it is deliberate: it is a
*dependency* choice made per profile, not the engine branching on where it is
running. The distinction matters — a feature is chosen by whoever assembles the
build, and a `cfg(target_os)` is chosen by the engine behind everyone's back.

## The decision asked for

**Reject** the single dual-host capability trait. **Accept** capability-per-
seam, with the rule above replacing boundary invariant 7's wording, and the
gate enforcing it.

If instead the trait is still wanted, that is a real refactor with a real cost
and it should be a row, not a sentence in four documents.

## What this unblocks

`TAURI-001` is the first work that cannot start without an answer: a desktop
shell has threads, a real clock and a filesystem, and needs to know whether it
implements an interface or supplies values. Under this proposal it supplies
values, and there is nothing to implement.

## What it does not settle

`RND-11` needs a *different* ADR — putting general geometry into `PaintItem`
changes what the display list **is**, which is ADR-008's territory, not this
one. They are separate decisions and conflating them would hide both.
