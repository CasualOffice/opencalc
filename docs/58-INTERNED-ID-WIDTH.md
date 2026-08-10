# 58 — The width of an interned id

**Status: Accepted** (ADR-013). Triggered by [PERF-01](14-EXECUTION-TRACKER.md),
which added the missing 1M-cell gate and measured what a cell actually costs.

> **Decision.** `StyleId` and `StringId` become `NonZeroU32` instead of a
> 128-bit `Id`. They are already indices; only their box is large.

## What the measurement showed

A `Cell` is **80 bytes**. Three quarters of that is two fields:

| Field | Bytes | Why |
| --- | --- | --- |
| `value: CellValue` | 32 | its largest variant carries a `StringId`, which is a `u128` |
| `style: Option<StyleId>` | 32 | a `u128` has no spare bit pattern, so `Option` costs a second word |
| `formula: Option<FormulaHandle>` | 8 | a real `u32` index — what the other two were meant to be |

At the 1,000,000-cell target from [30](30-PERFORMANCE-AND-CAPACITY-TARGETS.md),
each byte here is a megabyte there.

## They are already indices

This is not a proposal to *make* them indices. They are, and have been:

```rust
fn id_for(index: u32) -> StyleId {
    StyleId(Id::from_parts(STYLE_NAMESPACE, index as u64 + 1))
}
```

Ninety-six of the hundred and twenty-eight bits are a **constant** — a
namespace tag and zero padding — wrapped around a `u32`. `index_of` unpacks it
and returns `None` when the tag does not match.

[23](23-CELL-STORE-REPRESENTATION.md) describes exactly this and says so in as
many words: *"a cell holds a 32-bit id, not text"*. The document and the
intent agree; only the representation does not.

## What the namespace tag buys, and what it does not

It catches an id used against the wrong table. That is worth something, but
less than it appears:

- **The type system already does it.** `StyleId` and `StringId` are distinct
  newtypes, so passing one where the other is expected does not compile. The
  tag is a runtime re-check of something the compiler has already refused.
- **It does not make an id portable, which is what it looks like it does.**
  [COL-12](14-EXECUTION-TRACKER.md) established that these ids are
  replica-local: an id from another workbook's table is meaningless here
  whatever its tag says, which is why the wire format translates them rather
  than trusting them. The tag can tell a `StyleId` from a `StringId`; it cannot
  tell *this* workbook's style 7 from *another* workbook's style 7, and that is
  the confusion that actually happens.

So it costs twelve bytes per cell for a check the compiler makes for free, and
does not deliver the guarantee its shape suggests.

## What changes

- `StyleId` and `StringId` become `NonZeroU32`, leaving the `id_newtype!` macro
  for `SheetId`, `NumberFormatId` and `DefinedNameId`, which do not sit in a
  cell and are not on the budget.
- Non-zero rather than plain `u32` for the **niche**: `Option<StyleId>` becomes
  4 bytes rather than 8, and `CellValue` shrinks with `StringId` inside it. The
  tables already number from one, so nothing changes about how they count.
- `index_of` stops being fallible in practice; it keeps its signature so the
  tables' callers do not change.

**Measured: a cell is 32 bytes, from 80 — a 60% reduction**, and better than
the 32–40 estimated, because narrowing `StringId` shrank `CellValue` from 32 to
16 as well. One stored cell, address included, is 40 bytes rather than 88. At
the target that is **40 MB of payload instead of 88 MB**. The gate from PERF-01
records it; the point of a gate is that the number is observed, not claimed.

## The cost, which is real

**The snapshot format changes.** An id is serialized `transparent`, so today it
is a 32-character hex string and afterwards it is a number. Old snapshots do
not load.

That contradicts nothing in [ADR-010](08-ADR-REGISTER.md) — which promises that
*additive* changes keep old goldens byte-identical — but it is not additive, so
`SCHEMA_VERSION` goes from 0 to 1 and `validate()` refuses the older value with
the error it already has for exactly this.

The project is alpha, nothing is published to crates.io, and no golden snapshot
is committed, so the practical blast radius is a rebuild. Doing this after a
stable release would be a migration; doing it now is a version bump.

## Alternatives

- **Leave it.** Defensible — 80 bytes still meets the budget with headroom. It
  is also 40 MB at the target for no benefit, and the longer ids stay wide the
  more expensive the format change becomes.
- **Narrow every id.** `SheetId` and the rest are few per workbook and never in
  a cell, so it would be churn for no measurable gain, and `SheetId` has a
  better claim on stability than a style index does.
- **Keep the tag in a debug assertion.** Retains the check where it is useful —
  during development — at no runtime cost. Worth doing if the tables' tests
  turn out to depend on it.
