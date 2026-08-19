//! A reference as *stored* against a cell, and as *resolved* to an address.
//!
//! Stage one of [docs/75](../../../docs/75-RELATIVE-FORMULA-SHARING-DESIGN.md)
//! (`PERF-11`). Nothing uses these yet: this is the pure addition the design
//! stages first, so the conversion can be argued about and tested before any
//! storage changes shape.
//!
//! # Why two types and not a convention
//!
//! Today every reference in a formula holds an absolute address, and every
//! reader of `CellReference::row` gets one. If stored formulas become relative
//! — which is the whole point of `PERF-11`, so a filled-down column can share
//! one AST — then a reader holding a *stored* formula gets a delta where it
//! used to get an address.
//!
//! There are such readers in the parser, the evaluator, the shifter, import's
//! shared-formula path, export, the structural rewrite (`FID-24`) and the cut
//! repointing (`UX-CUT-03`). A single missed one does not crash: it computes a
//! plausible wrong answer, in a spreadsheet, silently. That is the worst failure
//! this project can produce.
//!
//! So the two forms are different **types**, and the compiler finds every site.
//! A doc comment saying "remember this is relative now" would not, and the
//! change is far too wide to review by eye.

use crate::reference::CellReference;

/// A reference as it is stored in a shared formula: relative to the cell that
/// holds it, unless `$`-anchored.
///
/// `A1` stored in `B1` and `A2` stored in `B2` are the same `StoredRef` — one
/// column left, same row — which is what lets one AST serve a whole filled-down
/// column.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoredRef {
    /// The qualifying sheet name, if any. Never relative: a sheet is named or
    /// it is the formula's own.
    pub sheet: Option<String>,
    /// Columns from the holding cell, or the absolute column when anchored.
    pub col: i64,
    /// Rows from the holding cell, or the absolute row when anchored.
    pub row: i64,
    /// `$`-anchored column: `col` is an address, not an offset.
    pub col_absolute: bool,
    /// `$`-anchored row: `row` is an address, not an offset.
    pub row_absolute: bool,
    /// Named no row — a whole column, as in `A:A`.
    pub row_implicit: bool,
    /// Named no column — a whole row, as in `$1:$2`.
    pub col_implicit: bool,
}

/// A reference resolved against the cell holding it: an address on a sheet.
///
/// This is what every consumer wants — the evaluator, the dependency graph, the
/// printer. It is deliberately the *only* form carrying `u32` coordinates, so a
/// function taking one cannot be handed an unresolved offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedRef {
    /// The qualifying sheet name, if any.
    pub sheet: Option<String>,
    /// Zero-based column.
    pub col: u32,
    /// Zero-based row.
    pub row: u32,
    /// Whether the column was `$`-anchored. Carried through because it decides
    /// what a *copy* does, and a resolved reference still has to round-trip.
    pub col_absolute: bool,
    /// Whether the row was `$`-anchored.
    pub row_absolute: bool,
    /// Named no row — a whole column.
    pub row_implicit: bool,
    /// Named no column — a whole row.
    pub col_implicit: bool,
}

/// Where a formula lives: the origin its relative references are measured from.
///
/// Not stored per cell. The origin **is** the cell's own address, which the
/// sheet already knows because the cell is kept at it — so sharing an AST costs
/// nothing per cell, which is the part docs/40 got wrong by asking for a
/// per-cell origin to be stored alongside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    /// Zero-based row of the cell holding the formula.
    pub row: u32,
    /// Zero-based column of the cell holding the formula.
    pub col: u32,
}

impl Origin {
    /// The origin of the cell at `(row, col)`.
    #[must_use]
    pub const fn at(row: u32, col: u32) -> Self {
        Self { row, col }
    }
}

impl ResolvedRef {
    /// Store this reference against `origin`.
    ///
    /// An anchored axis keeps its address; a relative one becomes an offset.
    #[must_use]
    pub fn store(&self, origin: Origin) -> StoredRef {
        StoredRef {
            sheet: self.sheet.clone(),
            col: if self.col_absolute {
                i64::from(self.col)
            } else {
                i64::from(self.col) - i64::from(origin.col)
            },
            row: if self.row_absolute {
                i64::from(self.row)
            } else {
                i64::from(self.row) - i64::from(origin.row)
            },
            col_absolute: self.col_absolute,
            row_absolute: self.row_absolute,
            row_implicit: self.row_implicit,
            col_implicit: self.col_implicit,
        }
    }
}

impl StoredRef {
    /// Resolve against the cell that holds the formula.
    ///
    /// `None` when the result would fall outside the sheet. That is a real
    /// case, not a defensive check: a formula referring one column left, shared
    /// down a column, is off the sheet when it reaches column A — and Excel
    /// answers `#REF!` there rather than wrapping to the far edge. Returning
    /// `None` makes the caller decide, and makes it impossible to silently
    /// produce an address on the wrong side of the sheet.
    #[must_use]
    pub fn resolve(&self, origin: Origin) -> Option<ResolvedRef> {
        let col = if self.col_absolute {
            self.col
        } else {
            self.col + i64::from(origin.col)
        };
        let row = if self.row_absolute {
            self.row
        } else {
            self.row + i64::from(origin.row)
        };
        if col < 0 || row < 0 {
            return None;
        }
        let (col, row) = (u32::try_from(col).ok()?, u32::try_from(row).ok()?);
        if col > crate::reference::MAX_COL || row > crate::reference::MAX_ROW {
            return None;
        }
        Some(ResolvedRef {
            sheet: self.sheet.clone(),
            col,
            row,
            col_absolute: self.col_absolute,
            row_absolute: self.row_absolute,
            row_implicit: self.row_implicit,
            col_implicit: self.col_implicit,
        })
    }
}

impl From<&CellReference> for ResolvedRef {
    fn from(r: &CellReference) -> Self {
        Self {
            sheet: r.sheet.clone(),
            col: r.col,
            row: r.row,
            col_absolute: r.col_absolute,
            row_absolute: r.row_absolute,
            row_implicit: r.row_implicit,
            col_implicit: r.col_implicit,
        }
    }
}

impl From<&ResolvedRef> for CellReference {
    fn from(r: &ResolvedRef) -> Self {
        Self {
            sheet: r.sheet.clone(),
            col: r.col,
            row: r.row,
            col_absolute: r.col_absolute,
            row_absolute: r.row_absolute,
            row_implicit: r.row_implicit,
            col_implicit: r.col_implicit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(row: u32, col: u32) -> ResolvedRef {
        ResolvedRef {
            sheet: None,
            col,
            row,
            col_absolute: false,
            row_absolute: false,
            row_implicit: false,
            col_implicit: false,
        }
    }

    /// **The same relative formula stores identically wherever it sits.**
    ///
    /// This is the whole of `PERF-11` in one assertion: `A1` in `B1` and `A2`
    /// in `B2` are one stored reference, so the trees holding them are equal
    /// and `PERF-09`'s interning collapses a filled-down column to one AST.
    #[test]
    fn a_filled_down_reference_stores_to_one_value() {
        let first = resolved(0, 0).store(Origin::at(0, 1)); // A1 held in B1
        let second = resolved(1, 0).store(Origin::at(1, 1)); // A2 held in B2
        let third = resolved(2, 0).store(Origin::at(2, 1)); // A3 held in B3
        assert_eq!(first, second);
        assert_eq!(second, third);
        assert_eq!(first.col, -1, "one column left");
        assert_eq!(first.row, 0, "same row");
    }

    /// **A distinct formula stays distinct.**
    ///
    /// The control, and it matters more than it reads: a normalisation that
    /// collapsed genuinely different formulas would corrupt documents while
    /// making the memory numbers look excellent.
    #[test]
    fn different_shapes_do_not_collapse_together() {
        let one_left = resolved(0, 0).store(Origin::at(0, 1));
        let two_left = resolved(0, 0).store(Origin::at(0, 2));
        let one_up = resolved(0, 1).store(Origin::at(1, 1));
        assert_ne!(one_left, two_left);
        assert_ne!(one_left, one_up);
    }

    /// **`$` keeps an address, and does not move with the cell.**
    ///
    /// `$A$1` means A1 wherever the formula is copied to, so it must not
    /// normalise to an offset — otherwise every anchored reference would drift
    /// down a filled column, which is precisely what `$` exists to prevent.
    #[test]
    fn an_anchored_reference_stores_its_address() {
        let mut anchored = resolved(0, 0);
        anchored.col_absolute = true;
        anchored.row_absolute = true;

        let here = anchored.store(Origin::at(0, 1));
        let far = anchored.store(Origin::at(500, 7));
        assert_eq!(
            here, far,
            "an anchored reference stored differently by position"
        );
        assert_eq!(here.row, 0);
        assert_eq!(here.col, 0);
    }

    /// **A half-anchored reference normalises only its relative half.**
    #[test]
    fn a_mixed_reference_normalises_one_axis() {
        let mut mixed = resolved(3, 0); // $A4 — column anchored, row relative
        mixed.col_absolute = true;
        let stored = mixed.store(Origin::at(5, 2));
        assert_eq!(stored.col, 0, "the anchored column kept its address");
        assert_eq!(stored.row, -2, "the relative row became an offset");
    }

    /// **Storing and resolving is a round trip**, for every combination of
    /// anchoring. If it were not, a formula would change meaning simply by
    /// being written down and read back.
    #[test]
    fn storing_then_resolving_returns_the_original() {
        for row_absolute in [false, true] {
            for col_absolute in [false, true] {
                for origin in [Origin::at(0, 0), Origin::at(4, 9), Origin::at(1000, 3)] {
                    let mut original = resolved(7, 5);
                    original.row_absolute = row_absolute;
                    original.col_absolute = col_absolute;

                    let round = original.store(origin).resolve(origin);
                    assert_eq!(
                        round.as_ref(),
                        Some(&original),
                        "r{row_absolute} c{col_absolute} at {origin:?} did not survive"
                    );
                }
            }
        }
    }

    /// **A reference that would fall off the sheet resolves to nothing.**
    ///
    /// One column left of column A is not column `4294967295`; Excel answers
    /// `#REF!`. Wrapping would put a formula on the far edge of the sheet and
    /// compute a confident wrong number.
    #[test]
    fn a_reference_off_the_sheet_does_not_wrap() {
        let one_left = resolved(0, 0).store(Origin::at(0, 1));
        assert!(
            one_left.resolve(Origin::at(0, 1)).is_some(),
            "B1 can see A1"
        );
        assert!(
            one_left.resolve(Origin::at(0, 0)).is_none(),
            "one column left of A wrapped instead of failing"
        );

        let one_up = resolved(0, 0).store(Origin::at(1, 0));
        assert!(
            one_up.resolve(Origin::at(0, 0)).is_none(),
            "one row above row 1 wrapped"
        );
    }

    /// **And one that would fall off the far edge, likewise.**
    #[test]
    fn a_reference_past_the_last_row_does_not_resolve() {
        let one_down = resolved(1, 0).store(Origin::at(0, 0));
        assert!(
            one_down
                .resolve(Origin::at(crate::reference::MAX_ROW, 0))
                .is_none(),
            "one row past the last row resolved to something"
        );
    }

    /// **The bridge to the existing type is lossless**, so stage two can move
    /// the evaluator across without changing what anything means.
    #[test]
    fn conversion_to_and_from_the_existing_reference_is_lossless() {
        let original = CellReference {
            sheet: Some("Sheet 2".to_owned()),
            col: 3,
            row: 11,
            col_absolute: true,
            row_absolute: false,
            row_implicit: false,
            col_implicit: true,
        };
        let there: ResolvedRef = (&original).into();
        let back: CellReference = (&there).into();
        assert_eq!(back, original);
    }
}
