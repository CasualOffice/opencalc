//! The per-cell byte gate that `docs/23` and `docs/30` T1 promised.
//!
//! Both say the per-cell byte ceiling "is asserted by the memory benchmark".
//! It was not asserted anywhere: the benchmark harness validates its report's
//! *shape* in CI and measures no memory at all, so the project's headline
//! capacity claim — 1,000,000+ populated cells within a bounded budget — rested
//! on nothing that could fail.
//!
//! # What is measured, and what is not
//!
//! The **per-cell record**, exactly, with `size_of`. That is the term the
//! budget is made of: a million cells cost a million times this, so every byte
//! here is a megabyte there.
//!
//! Not the allocator's true footprint. Measuring that means a counting global
//! allocator, which needs `unsafe`, and this workspace forbids it — a rule
//! worth more than a more precise number. The store is a `BTreeMap`, whose
//! nodes add roughly a tenth on top of the payload asserted here, so the
//! figures below are a floor rather than an estimate of resident memory.

use std::mem::size_of;

use casual_calc_model::{Cell, CellRef, CellValue, FormulaHandle, StringId, StyleId};

/// The ceiling for one cell record.
///
/// **This is above the design's intent, not at it.** `docs/23` describes the
/// style and string slots as *interned indices* — "a cell holds a 32-bit id,
/// not text" — but `StyleId` and `StringId` both wrap a 128-bit `Id`, so
/// `Option<StyleId>` alone is 32 bytes and `CellValue` is another 32. The
/// number here records what a cell costs today so that it cannot quietly grow;
/// bringing it down to what the design describes is a change to the id types
/// and their snapshot encoding, which is an ADR rather than an edit.
const CELL_CEILING: usize = 80;

/// The ceiling for one entry in the store: its address and its record.
///
/// A `BTreeMap` holds keys and values in separate arrays inside a node, so this
/// is the payload for one cell, excluding node metadata.
const ENTRY_CEILING: usize = 88;

/// The capacity target from `docs/30` T1.
const TARGET_CELLS: usize = 1_000_000;

/// The budget that target has to fit inside, in bytes.
///
/// A hundred and twenty-eight megabytes of cell payload for a million cells.
/// Chosen to be met today with headroom rather than to be aspirational: a gate
/// that already fails teaches nothing.
const BUDGET_BYTES: usize = 128 * 1024 * 1024;

#[test]
fn a_cell_record_stays_within_its_ceiling() {
    assert!(
        size_of::<Cell>() <= CELL_CEILING,
        "a cell is {} bytes against a ceiling of {CELL_CEILING}. At the 1M-cell \
         target every byte added here is a megabyte of the budget.",
        size_of::<Cell>()
    );
}

#[test]
fn a_million_cells_fit_the_budget() {
    let per_entry = size_of::<CellRef>() + size_of::<Cell>();
    let total = per_entry * TARGET_CELLS;

    assert!(
        per_entry <= ENTRY_CEILING,
        "one stored cell is {per_entry} bytes against {ENTRY_CEILING}"
    );
    assert!(
        total <= BUDGET_BYTES,
        "a million cells cost {} MB of payload against a budget of {} MB",
        total / 1_048_576,
        BUDGET_BYTES / 1_048_576
    );
}

/// Where a cell's bytes actually go.
///
/// Written as an assertion rather than a comment because the point of it is to
/// fail: the two id types are three quarters of a cell between them, and if
/// either is ever narrowed to the index `docs/23` describes, this test is what
/// says so and asks for the ceiling to come down with it.
#[test]
fn the_id_types_are_most_of_what_a_cell_costs() {
    let value = size_of::<CellValue>();
    let style = size_of::<Option<StyleId>>();
    let formula = size_of::<Option<FormulaHandle>>();

    assert_eq!(
        size_of::<StringId>(),
        16,
        "a shared-string id is a 128-bit Id, though docs/23 calls it a 32-bit id"
    );
    assert_eq!(
        style, 32,
        "Option<StyleId> is 32 bytes: a u128 has no spare bit pattern for the \
         None case, so the option costs a second word of alignment"
    );
    assert_eq!(
        formula, 8,
        "the formula handle is a real 32-bit index, which is what the others \
         were meant to be"
    );
    assert!(
        value + style >= size_of::<Cell>() * 3 / 4,
        "the value and style slots are {} of a cell's {} bytes",
        value + style,
        size_of::<Cell>()
    );
}
