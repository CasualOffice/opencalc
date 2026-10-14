//! What-If Analysis — Goal Seek (XL-FEAT-032).
//!
//! The search runs on a **scratch copy** of the workbook: each trial input is
//! written there and only the cells it reaches are recalculated. The session's
//! own workbook is not touched, so a search of a hundred trials makes no undo
//! step, no collaboration operation and no change anybody else can see. The
//! host commits the answer — once, as an ordinary edit — if the person keeps it.

use casual_calc_eval::Recalculator;
use casual_calc_eval::goal_seek::{GoalSeekOptions, GoalSeekResult, goal_seek};
use casual_calc_model::{Cell, CellRef, CellValue};

use crate::WorkbookSession;

/// Why Goal Seek refused to start. The `Display` text is Excel's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalSeekError {
    /// The sheet index is past the end.
    SheetNotFound,
    /// **Set cell** holds no formula, so nothing it shows can change.
    SetCellNotFormula,
    /// **By changing cell** holds a formula (or a spilled value, or text):
    /// Goal Seek only changes a constant number.
    ChangingCellNotValue,
}

impl std::fmt::Display for GoalSeekError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::SheetNotFound => "Reference is not valid.",
            Self::SetCellNotFormula => "Cell must contain a formula.",
            Self::ChangingCellNotValue => "Cell must contain a value.",
        })
    }
}

impl std::error::Error for GoalSeekError {}

impl WorkbookSession {
    /// Excel's Goal Seek: find the number for `changing` that makes the formula
    /// in `set_cell` evaluate to `target`, both on `sheet`.
    ///
    /// Nothing is recorded or changed — see the module notes. The result says
    /// whether a solution was found and, either way, the input that came
    /// closest and the value it gave.
    ///
    /// # Errors
    ///
    /// [`GoalSeekError`] when `set_cell` is not a formula, or `changing` is not
    /// an empty cell or a constant number.
    pub fn goal_seek(
        &self,
        sheet: usize,
        set_cell: CellRef,
        target: f64,
        changing: CellRef,
        opts: GoalSeekOptions,
    ) -> Result<GoalSeekResult, GoalSeekError> {
        let sh = self
            .workbook
            .sheets
            .get(sheet)
            .ok_or(GoalSeekError::SheetNotFound)?;
        if sh.cells.get(set_cell).is_none_or(|c| c.formula.is_none()) {
            return Err(GoalSeekError::SetCellNotFormula);
        }
        let existing = sh.cells.get(changing).cloned().unwrap_or_default();
        let spilled = existing
            .flags
            .contains(casual_calc_model::CellFlags::SPILL_CHILD);
        if existing.formula.is_some() || spilled {
            return Err(GoalSeekError::ChangingCellNotValue);
        }
        let start = match existing.value {
            CellValue::Number(n) => n,
            CellValue::Empty => 0.0,
            _ => return Err(GoalSeekError::ChangingCellNotValue),
        };

        let mut scratch = self.workbook.clone();
        if self.stale {
            // Manual calculation left caches behind; the search must start
            // from what the formulas actually say.
            casual_calc_eval::recalculate(&mut scratch);
        }
        let mut recalc = Recalculator::new();
        let read = move |x: f64| -> Option<f64> {
            let cell = Cell {
                value: CellValue::Number(x),
                ..existing.clone()
            };
            scratch.sheets[sheet].cells.set(changing, cell);
            recalc.recalculate(&mut scratch, &[(sheet, changing)]);
            match scratch.sheets[sheet].cells.get(set_cell).map(|c| &c.value) {
                Some(CellValue::Number(n)) => Some(*n),
                _ => None,
            }
        };
        Ok(goal_seek(read, target, start, opts))
    }
}

#[cfg(test)]
mod tests {
    use super::GoalSeekError;
    use crate::WorkbookSession;
    use casual_calc_eval::goal_seek::GoalSeekOptions;
    use casual_calc_model::CellRef;

    fn session(cells: &[(u32, u32, &str)]) -> WorkbookSession {
        let mut s = WorkbookSession::with_sheet();
        for &(r, c, input) in cells {
            let op = s.input_edit(0, CellRef::new(r, c), input);
            s.edit(op).unwrap();
        }
        s
    }

    #[test]
    fn finds_root_two_without_changing_the_session() {
        let s = session(&[(0, 0, "1"), (0, 1, "=A1^2")]);
        let applied = s.edits_applied();
        let before = s.workbook().clone();
        let r = s
            .goal_seek(
                0,
                CellRef::new(0, 1),
                2.0,
                CellRef::new(0, 0),
                GoalSeekOptions::default(),
            )
            .unwrap();
        assert!(r.found, "{r:?}");
        assert!((r.input - std::f64::consts::SQRT_2).abs() < 1e-3, "{r:?}");
        // Nothing the search tried reached the session: no edit, no undo
        // step, and the workbook — every cell, every cached value — as it was.
        assert_eq!(s.edits_applied(), applied);
        assert_eq!(s.workbook(), &before);
    }

    #[test]
    fn follows_the_target_through_a_chain_of_formulas() {
        // B1 = A1*2, C1 = B1+10, goal C1 = 30 → A1 = 10.
        let s = session(&[(0, 0, "3"), (0, 1, "=A1*2"), (0, 2, "=B1+10")]);
        let r = s
            .goal_seek(
                0,
                CellRef::new(0, 2),
                30.0,
                CellRef::new(0, 0),
                GoalSeekOptions::default(),
            )
            .unwrap();
        assert!(r.found, "{r:?}");
        assert!((r.input - 10.0).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn refuses_what_excel_refuses() {
        let s = session(&[(0, 0, "1"), (0, 1, "=A1^2"), (0, 2, "=A1"), (0, 3, "text")]);
        let opts = GoalSeekOptions::default();
        // Set cell must be a formula.
        assert_eq!(
            s.goal_seek(0, CellRef::new(0, 0), 2.0, CellRef::new(0, 0), opts),
            Err(GoalSeekError::SetCellNotFormula)
        );
        // By changing cell must be a value: not a formula, not text.
        assert_eq!(
            s.goal_seek(0, CellRef::new(0, 1), 2.0, CellRef::new(0, 2), opts),
            Err(GoalSeekError::ChangingCellNotValue)
        );
        assert_eq!(
            s.goal_seek(0, CellRef::new(0, 1), 2.0, CellRef::new(0, 3), opts),
            Err(GoalSeekError::ChangingCellNotValue)
        );
        assert_eq!(
            GoalSeekError::SetCellNotFormula.to_string(),
            "Cell must contain a formula."
        );
        assert_eq!(
            GoalSeekError::ChangingCellNotValue.to_string(),
            "Cell must contain a value."
        );
    }

    #[test]
    fn an_empty_changing_cell_starts_from_zero() {
        let s = session(&[(0, 1, "=A1*4")]);
        let r = s
            .goal_seek(
                0,
                CellRef::new(0, 1),
                10.0,
                CellRef::new(0, 0),
                GoalSeekOptions::default(),
            )
            .unwrap();
        assert!(r.found, "{r:?}");
        assert!((r.input - 2.5).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn no_solution_says_so_with_the_closest_value() {
        let s = session(&[(0, 0, "2"), (0, 1, "=A1^2+1")]);
        let r = s
            .goal_seek(
                0,
                CellRef::new(0, 1),
                0.0,
                CellRef::new(0, 0),
                GoalSeekOptions::default(),
            )
            .unwrap();
        assert!(!r.found, "{r:?}");
        assert!(r.value >= 1.0 && r.value < 1.1, "{r:?}");
    }
}
