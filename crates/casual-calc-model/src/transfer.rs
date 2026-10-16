//! Handing a workbook from one engine to another **of the same build**, in
//! bounded parts (`PERF-16`).
//!
//! The browser editor opens and saves in a Web Worker so the page stays
//! responsive. A worker has its own WebAssembly instance and its own memory —
//! without `SharedArrayBuffer`, which needs cross-origin isolation the static
//! host cannot send (see `webapp/macro.worker.js`), nothing can be shared — so
//! the model has to cross as bytes, and the side that installs it pays to read
//! them on **its** thread. Read as one snapshot that is one long call: 128 ms
//! for 250,000 cells, measured, on the thread the grid is drawn from. Read in
//! parts, each a few milliseconds, the frames go on between them.
//!
//! # Why not the snapshot
//!
//! [`Workbook::to_snapshot`] is the collaboration wire format (ADR-010): it
//! carries formulas in their *absolute* form, so writing one re-resolves every
//! formula cell, and reading one re-relativises and re-interns them. Neither is
//! needed between two engines of one build, which share the in-memory form,
//! and both are paid by the thread this exists to protect. This format is
//! **never persisted and never crosses a build**: the worker loads the very
//! wasm the page did (`?b=BUILD`). It is not a protocol, so ADR-016's
//! `PROTOCOL_VERSION` does not apply; the schema version is still checked, as a
//! guard against exactly the build skew that must not happen.
//!
//! # Bounds
//!
//! The reader is held to [`SnapshotLimits`] — bytes and populated cells —
//! counted as parts arrive, so an oversized transfer is refused before it has
//! been read, not after. Every cell is checked against the workbook it joins
//! with [`Workbook::validate_cell`], the rule [`Workbook::validate`] applies,
//! and the rest of `validate` runs at [`TransferReader::finish`]. Parts must
//! arrive in order and the last one states how many came before it, so a
//! truncated transfer is refused rather than installed short.
//!
//! # Determinism
//!
//! The workbook that comes out serialises to the same snapshot as the one that
//! went in: the arena crosses verbatim, so formula handles keep their indices,
//! and cells cross in row-major order. Tested below.

use std::collections::VecDeque;

use casual_calc_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::store::CellStore;
use crate::workbook::{SCHEMA_VERSION, SnapshotLimits, Workbook};

/// One part on the way across.
#[derive(Serialize, Deserialize)]
enum Part {
    /// Everything but the cells and the formula arena. First, exactly once.
    Header(Box<Workbook>),
    /// The next run of the formula arena, in order. All of them come before
    /// any cell, so a cell's handle can be checked the moment it arrives.
    Formulas(Vec<Expr>),
    /// The next run of one sheet's cells, row-major.
    Cells { sheet: usize, cells: CellStore },
    /// The last part: how many formulas and cells were sent.
    End { formulas: u64, cells: u64 },
}

/// The sending side: owns the workbook it sends.
///
/// Built from a **copy** when the original stays in use (the editor's save),
/// which fixes the moment being saved: an edit made between two parts is not
/// in the file, rather than half in it.
#[derive(Debug)]
pub struct TransferWriter {
    header: Option<Workbook>,
    formulas: VecDeque<Expr>,
    sheets: Vec<CellStore>,
    sheet: usize,
    sent_formulas: u64,
    sent_cells: u64,
    total: u64,
    done: bool,
}

impl TransferWriter {
    /// Take `workbook` apart for sending.
    pub fn new(mut workbook: Workbook) -> Self {
        let formulas: VecDeque<Expr> = std::mem::take(&mut workbook.formulas).into();
        let sheets: Vec<CellStore> = workbook
            .sheets
            .iter_mut()
            .map(|s| std::mem::take(&mut s.cells))
            .collect();
        let total = formulas.len() as u64 + sheets.iter().map(|s| s.len() as u64).sum::<u64>();
        Self {
            header: Some(workbook),
            formulas,
            sheets,
            sheet: 0,
            sent_formulas: 0,
            sent_cells: 0,
            total,
            done: false,
        }
    }

    /// Formulas plus cells: what progress is counted in.
    #[must_use]
    pub fn total_items(&self) -> u64 {
        self.total
    }

    /// Formulas plus cells sent so far.
    #[must_use]
    pub fn sent_items(&self) -> u64 {
        self.sent_formulas + self.sent_cells
    }

    /// The next part, holding at most `max_items` formulas or cells (at least
    /// one), or `None` once the last has been returned.
    ///
    /// # Errors
    ///
    /// [`ModelError::Snapshot`] if a part cannot be serialised.
    pub fn next_part(&mut self, max_items: usize) -> Result<Option<Vec<u8>>, ModelError> {
        let max = max_items.max(1);
        if let Some(header) = self.header.take() {
            return Ok(Some(serde_json::to_vec(&Part::Header(Box::new(header)))?));
        }
        if !self.formulas.is_empty() {
            let n = max.min(self.formulas.len());
            let run: Vec<Expr> = self.formulas.drain(..n).collect();
            self.sent_formulas += n as u64;
            return Ok(Some(serde_json::to_vec(&Part::Formulas(run))?));
        }
        while self.sheet < self.sheets.len() {
            let store = &mut self.sheets[self.sheet];
            if store.is_empty() {
                self.sheet += 1;
                continue;
            }
            let cells = store.split_first(max);
            self.sent_cells += cells.len() as u64;
            let part = Part::Cells {
                sheet: self.sheet,
                cells,
            };
            return Ok(Some(serde_json::to_vec(&part)?));
        }
        if self.done {
            return Ok(None);
        }
        self.done = true;
        Ok(Some(serde_json::to_vec(&Part::End {
            formulas: self.sent_formulas,
            cells: self.sent_cells,
        })?))
    }
}

/// The receiving side: assembles a workbook from parts, checking as it goes.
///
/// Nothing is installed anywhere until [`finish`](Self::finish) returns, so a
/// transfer that is abandoned — cancelled, refused, or cut short — leaves
/// whatever the receiver already had exactly as it was.
#[derive(Debug)]
pub struct TransferReader {
    workbook: Option<Workbook>,
    limits: SnapshotLimits,
    bytes: u64,
    cells: u64,
    formulas: u64,
    /// The sheet the last cell part was for; parts arrive in sheet order.
    sheet: usize,
    ended: bool,
}

const OUT_OF_ORDER: ModelError = ModelError::Invariant("transfer part out of order");

impl TransferReader {
    /// A reader held to `limits`.
    #[must_use]
    pub fn new(limits: SnapshotLimits) -> Self {
        Self {
            workbook: None,
            limits,
            bytes: 0,
            cells: 0,
            formulas: 0,
            sheet: 0,
            ended: false,
        }
    }

    /// Read one part.
    ///
    /// # Errors
    ///
    /// [`ModelError::SnapshotTooLarge`] past either limit (bytes are counted
    /// before the part is parsed), [`ModelError::Snapshot`] for bytes that are
    /// not a part, and [`ModelError::Invariant`] for a part out of order or a
    /// cell naming something the workbook does not have.
    pub fn push(&mut self, part: &[u8]) -> Result<(), ModelError> {
        if self.ended {
            return Err(OUT_OF_ORDER);
        }
        self.bytes = self.bytes.saturating_add(part.len() as u64);
        if self.bytes > self.limits.max_bytes {
            return Err(ModelError::SnapshotTooLarge {
                what: "bytes",
                limit: self.limits.max_bytes,
                asked: self.bytes,
            });
        }
        match serde_json::from_slice::<Part>(part)? {
            Part::Header(header) => {
                if self.workbook.is_some() {
                    return Err(OUT_OF_ORDER);
                }
                if header.schema_version != SCHEMA_VERSION {
                    return Err(ModelError::Invariant("unsupported schema version"));
                }
                if !header.formulas.is_empty() || header.sheets.iter().any(|s| !s.cells.is_empty())
                {
                    return Err(ModelError::Invariant("transfer header carries cells"));
                }
                self.workbook = Some(*header);
            }
            Part::Formulas(run) => {
                let wb = self.workbook.as_mut().ok_or(OUT_OF_ORDER)?;
                // Formulas all come before the first cell, so that a cell's
                // handle can be checked against the whole arena on arrival.
                if self.cells > 0 {
                    return Err(OUT_OF_ORDER);
                }
                self.formulas += run.len() as u64;
                wb.formulas.extend(run);
            }
            Part::Cells { sheet, cells } => {
                let wb = self.workbook.as_mut().ok_or(OUT_OF_ORDER)?;
                if sheet < self.sheet || sheet >= wb.sheets.len() {
                    return Err(OUT_OF_ORDER);
                }
                self.sheet = sheet;
                self.cells += cells.len() as u64;
                if self.cells > self.limits.max_populated_cells as u64 {
                    return Err(ModelError::SnapshotTooLarge {
                        what: "populated cells",
                        limit: self.limits.max_populated_cells as u64,
                        asked: self.cells,
                    });
                }
                for (_, cell) in cells.iter() {
                    wb.validate_cell(cell)?;
                }
                let store = &mut wb.sheets[sheet].cells;
                // Row-major and ascending: a part that starts at or before
                // where the last one ended would *replace* cells, which no
                // honest sender produces.
                if let (Some((_, last)), Some((first, _))) = (store.bounds(), cells.bounds())
                    && first <= last
                {
                    return Err(OUT_OF_ORDER);
                }
                store.absorb(cells);
            }
            Part::End { formulas, cells } => {
                if self.workbook.is_none() || formulas != self.formulas || cells != self.cells {
                    return Err(ModelError::Invariant("transfer cut short"));
                }
                self.ended = true;
            }
        }
        Ok(())
    }

    /// Formulas plus cells read so far.
    #[must_use]
    pub fn received_items(&self) -> u64 {
        self.formulas + self.cells
    }

    /// The assembled workbook.
    ///
    /// # Errors
    ///
    /// [`ModelError::Invariant`] if the last part has not arrived, or if the
    /// workbook fails the checks of [`Workbook::validate`] that are not
    /// per-cell (those were applied as each cell arrived).
    pub fn finish(self) -> Result<Workbook, ModelError> {
        if !self.ended {
            return Err(ModelError::Invariant("transfer cut short"));
        }
        let mut workbook = self.workbook.ok_or(OUT_OF_ORDER)?;
        // Derived state, rebuilt as `from_snapshot` rebuilds it.
        workbook.reindex_formulas();
        workbook.validate_shape()?;
        Ok(workbook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cell, CellRef, CellValue, Id, Sheet, SheetId};
    use casual_calc_formula::parse;

    fn sample(rows: u32) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        for s in 0..2u64 {
            let mut sheet = Sheet::new(SheetId(Id::from_parts(2, s + 1)), format!("S{s}"));
            for r in 0..rows {
                let text = wb.intern_string(&format!("t{r}"));
                sheet.cells.set(
                    CellRef::new(r, 0),
                    Cell::value(CellValue::Number(f64::from(r) * 0.1)),
                );
                sheet.cells.set(
                    CellRef::new(r, 1),
                    Cell::value(CellValue::SharedString(text)),
                );
                let expr = parse(&format!("A{}*2+{}", r + 1, r % 7)).unwrap();
                let handle =
                    wb.store_formula_at(expr, casual_calc_formula::stored::Origin::at(r, 2));
                let mut cell = Cell::value(CellValue::Number(f64::from(r)));
                cell.formula = Some(handle);
                sheet.cells.set(CellRef::new(r, 2), cell);
            }
            // A table with a header row and one without: the header count is
            // a field whose serde default once disagreed with its skip rule.
            for (id, headers) in [(1, 1), (2, 0)] {
                sheet.tables.push(crate::Table {
                    id: id + 10 * s as u32,
                    name: format!("T{s}_{id}"),
                    display_name: format!("T{s}_{id}"),
                    range: crate::CellRange {
                        start: CellRef::new(0, 4 * id),
                        end: CellRef::new(5, 4 * id),
                    },
                    header_row_count: headers,
                    totals_row_count: 0,
                    columns: vec![crate::TableColumn {
                        id: 1,
                        name: "C".to_owned(),
                        totals_row_function: None,
                        totals_row_label: None,
                        calculated_column_formula: None,
                        totals_row_formula: None,
                    }],
                    auto_filter: None,
                    style: Default::default(),
                    attrs: Default::default(),
                });
            }
            wb.sheets.push(sheet);
        }
        wb
    }

    /// A table's header row survives the trip — and the snapshot, which
    /// shares its serialisation. It did not: the field's default was 0.
    #[test]
    fn a_table_keeps_its_header_row() {
        let wb = sample(3);
        let got = receive(&send(wb.clone(), 100)).unwrap();
        let counts = |w: &Workbook| -> Vec<u32> {
            w.sheets
                .iter()
                .flat_map(|s| s.tables.iter().map(|t| t.header_row_count))
                .collect()
        };
        assert_eq!(counts(&got), vec![1, 0, 1, 0]);
        let snap = Workbook::from_snapshot(&wb.to_snapshot().unwrap()).unwrap();
        assert_eq!(counts(&snap), vec![1, 0, 1, 0]);
    }

    fn send(wb: Workbook, max: usize) -> Vec<Vec<u8>> {
        let mut w = TransferWriter::new(wb);
        let mut parts = Vec::new();
        while let Some(p) = w.next_part(max).unwrap() {
            parts.push(p);
        }
        parts
    }

    fn receive(parts: &[Vec<u8>]) -> Result<Workbook, ModelError> {
        let mut r = TransferReader::new(SnapshotLimits::default());
        for p in parts {
            r.push(p)?;
        }
        r.finish()
    }

    /// What arrives is what was sent: the same snapshot, byte for byte, at
    /// every part size including one item per part.
    #[test]
    fn a_transfer_reproduces_the_workbook() {
        let wb = sample(300);
        let want = String::from_utf8(wb.to_snapshot().unwrap()).unwrap();
        for max in [1, 7, 64, 100_000] {
            let parts = send(wb.clone(), max);
            let got = receive(&parts).unwrap();
            let got_snapshot = String::from_utf8(got.to_snapshot().unwrap()).unwrap();
            assert_eq!(got_snapshot, want, "part size {max}");
            // And the derived index came back: storing a formula already in
            // the arena returns its existing handle rather than appending.
            let mut got = got;
            let before = got.formulas.len();
            got.store_formula(got.formulas[0].clone());
            assert_eq!(got.formulas.len(), before, "part size {max}");
        }
    }

    /// Bounded parts really are bounded.
    #[test]
    fn parts_hold_at_most_the_asked_number_of_items() {
        let wb = sample(100);
        let mut w = TransferWriter::new(wb);
        assert_eq!(w.total_items(), w.formulas.len() as u64 + 600);
        let mut n = 0;
        while let Some(p) = w.next_part(16).unwrap() {
            n += 1;
            match serde_json::from_slice::<Part>(&p).unwrap() {
                Part::Formulas(f) => assert!(f.len() <= 16),
                Part::Cells { cells, .. } => assert!(cells.len() <= 16),
                _ => {}
            }
        }
        assert!(n > 600 / 16);
    }

    /// A transfer cut short is refused, not installed short.
    #[test]
    fn a_truncated_transfer_is_refused() {
        let parts = send(sample(50), 10);
        let err = receive(&parts[..parts.len() - 2]).expect_err("refused");
        assert!(err.to_string().contains("cut short"), "{err}");
        // Missing the last part only.
        let err = receive(&parts[..parts.len() - 1]).expect_err("refused");
        assert!(err.to_string().contains("cut short"), "{err}");
        // Missing a part from the middle, the last one present: only the
        // counts the last part carries can tell.
        let mut gapped = parts.clone();
        gapped.remove(parts.len() - 2);
        let err = receive(&gapped).expect_err("refused");
        assert!(err.to_string().contains("cut short"), "{err}");
    }

    /// Parts out of order are refused: a cell before the arena, a repeated
    /// header, a cell part that rewinds.
    #[test]
    fn parts_out_of_order_are_refused() {
        let parts = send(sample(50), 10);
        let mut r = TransferReader::new(SnapshotLimits::default());
        assert!(r.push(&parts[1]).is_err(), "a part before the header");

        let mut r = TransferReader::new(SnapshotLimits::default());
        r.push(&parts[0]).unwrap();
        assert!(r.push(&parts[0]).is_err(), "a second header");

        // The first cell part pushed twice rewinds the sheet.
        let first_cells = parts
            .iter()
            .position(|p| {
                matches!(
                    serde_json::from_slice::<Part>(p).unwrap(),
                    Part::Cells { .. }
                )
            })
            .unwrap();
        let mut r = TransferReader::new(SnapshotLimits::default());
        for p in &parts[..=first_cells] {
            r.push(p).unwrap();
        }
        assert!(r.push(&parts[first_cells]).is_err(), "a rewound cell part");
    }

    /// The snapshot limits hold, counted as parts arrive.
    #[test]
    fn the_snapshot_limits_hold() {
        let parts = send(sample(50), 10);
        let mut r = TransferReader::new(SnapshotLimits {
            max_bytes: u64::MAX,
            max_populated_cells: 120,
        });
        let err = parts
            .iter()
            .try_for_each(|p| r.push(p))
            .expect_err("refused");
        assert!(
            matches!(
                err,
                ModelError::SnapshotTooLarge {
                    what: "populated cells",
                    ..
                }
            ),
            "{err}"
        );

        let mut r = TransferReader::new(SnapshotLimits {
            max_bytes: 64,
            max_populated_cells: usize::MAX,
        });
        let err = r.push(&parts[0]).expect_err("refused");
        assert!(
            matches!(err, ModelError::SnapshotTooLarge { what: "bytes", .. }),
            "{err}"
        );
    }

    /// A cell naming a string the workbook does not have is refused on
    /// arrival, by the rule `validate` applies.
    #[test]
    fn a_dangling_cell_is_refused() {
        let mut wb = sample(5);
        wb.sheets[0].cells.set(
            CellRef::new(99, 0),
            Cell::value(CellValue::SharedString(crate::StringId(
                std::num::NonZero::new(9_999).unwrap(),
            ))),
        );
        let parts = send(wb, 1000);
        let err = receive(&parts).expect_err("refused");
        assert!(err.to_string().contains("dangling string"), "{err}");
    }
}
