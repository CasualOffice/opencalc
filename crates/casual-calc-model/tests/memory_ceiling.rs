//! The per-cell byte gate that `docs/23` and `docs/30` T1 promised.
//!
//! Both said the per-cell byte ceiling "is asserted by the memory benchmark".
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
//! nodes add roughly a tenth on top of the payload asserted here, so these
//! figures are a floor rather than an estimate of resident memory.

use std::mem::size_of;

use casual_calc_model::{Cell, CellRef, CellValue, FormulaHandle, StringId, StyleId};

/// The ceiling for one cell record.
///
/// Thirty-two bytes, down from eighty: `StyleId` and `StringId` became
/// `NonZeroU32` under [ADR-013](../../../docs/58-INTERNED-ID-WIDTH.md), having
/// been a `u32` index inside a 128-bit box with a constant namespace tag around
/// it. The ceiling is what a cell costs **today**, so that growth is a decision
/// somebody makes rather than something that happens.
const CELL_CEILING: usize = 32;

/// The ceiling for one entry in the store: its address and its record.
///
/// A `BTreeMap` holds keys and values in separate arrays inside a node, so this
/// is the payload for one cell, excluding node metadata.
const ENTRY_CEILING: usize = 40;

/// The capacity target from `docs/30` T1.
const TARGET_CELLS: usize = 1_000_000;

/// The budget that target has to fit inside, in bytes.
///
/// Sixty-four megabytes of cell payload for a million cells — halved along with
/// the cell, because a budget that stays wide after the thing inside it shrank
/// stops being a gate and becomes a ceiling nothing can touch.
const BUDGET_BYTES: usize = 64 * 1024 * 1024;

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

/// Where a cell's bytes go, pinned so a widening is visible as such.
///
/// This is the test that made ADR-013's case: it recorded a `StringId` at
/// sixteen bytes and an `Option<StyleId>` at thirty-two, against a design
/// document describing both as interned indices. Now it records that they are.
#[test]
fn the_interned_ids_are_indices_and_cost_what_an_index_costs() {
    assert_eq!(
        size_of::<StringId>(),
        4,
        "a shared-string id is an index, which is what docs/23 always said"
    );
    assert_eq!(
        size_of::<Option<StyleId>>(),
        4,
        "and `NonZeroU32` leaves a spare bit pattern, so the option is free — \
         which is why the tables number from one"
    );
    assert_eq!(
        size_of::<Option<FormulaHandle>>(),
        8,
        "the formula handle is a plain u32, so its option still costs a word"
    );
    assert!(
        size_of::<CellValue>() <= 16,
        "the value slot is {} bytes; it is the f64 plus a tag now, not an id",
        size_of::<CellValue>()
    );
}
