//! Restoring a version: the edits that carry this document back to a snapshot.
//!
//! # Why a diff and not a rewind
//!
//! Revisions are positional. `log[i]` is what took the document from
//! `first + i` to `first + i + 1` ([`session::ServerSession`](crate::session)),
//! and every connected client, every resume key and
//! [`oldest_rebasable`](crate::session::ServerSession::oldest_rebasable) is
//! defined against that numbering. Rewriting it invalidates all three at once,
//! silently, on every other participant — so under ADR-011 the only available
//! move is forward. `COL-50` and [`TransformError::Unsupported`] say the same
//! thing from the other side: a log that cannot be replayed reproducibly cannot
//! be rewound either.
//!
//! So restoring is **a new operation**. This module computes the difference
//! between the document as it stands and the workbook a snapshot holds, and
//! expresses it as one [`Operation::Batch`] of ordinary edits. Consequences,
//! all of them intended:
//!
//! - the restore is itself undoable, in one step, because a batch has one
//!   combined inverse;
//! - co-editors see it as edits arriving, because that is what it is;
//! - revision numbers only ever increase;
//! - nothing is deleted from the past.
//!
//! [`TransformError::Unsupported`]: crate::transform::TransformError::Unsupported
//!
//! # What a restore cannot say, and why it is counted
//!
//! The operation set is closed, and it is narrower than the model. Five sheet
//! fields and eight workbook fields have no operation that carries them —
//! `Sheet::images`, `Sheet::outline`, `Workbook::properties` and the rest. A
//! restore that quietly left those at their current values would be silent data
//! loss, which this project does not do, so every one of them is **counted and
//! named** in [`RestoreReport::unexpressed`] instead. A host that needs a total
//! restore opens the snapshot as a document rather than diffing onto this one.
//!
//! # Identifier spaces
//!
//! A snapshot is a whole other [`Workbook`], with its own string, style and
//! formula tables. `SharedString(7)` there is not `SharedString(7)` here, and an
//! operation carrying the foreign id commits without error and means something
//! else — the failure [`wire`](crate::wire) exists to prevent. Every value this
//! module lifts out of the snapshot is therefore re-interned into the live
//! workbook as it is lifted, memoised per id so a million cells sharing one
//! style intern it once.
//!
//! Rich text is re-interned **with its runs**, through
//! [`Workbook::intern_rich_text`], rather than through the plain-string path.
//!
//! # What a restore weighs
//!
//! Measured by `version_tests::measure_snapshot_cost`: a changed cell costs
//! about **84 bytes** on the wire, as a `SetCell` inside the batch's
//! [`WireOperation`](crate::wire::WireOperation). Two ceilings follow, and only
//! the first is this crate's.
//!
//! The undo entry is the batch's inverse, so a restore that rewrites *n* cells
//! holds *n* inverse `SetCell`s on the stack until the depth bound reaches
//! them. That is the price of "the restore is itself undoable" and it is why
//! [`plan`] compares cells by **meaning** rather than by identifier — a diff
//! that rewrote every populated cell would put a second copy of the document on
//! the undo stack for no change at all.
//!
//! The other ceiling belongs to the deployment: the collaboration server caps a
//! WebSocket message at 4 MiB by default (`Limits::max_message_bytes`), a
//! restore travels as **one** batch in **one** submission, and
//! [`ClientSession::flush`](crate::session::ClientSession::flush) does not split
//! a chunk by size. So a collaborative restore of more than roughly **50 000
//! changed cells** exceeds the frame — a bound nothing here can enforce and
//! nothing there names.

use std::collections::{BTreeMap, BTreeSet};

use casual_calc_model::{
    Cell, CellRef, CellValue, FormulaHandle, RetainedPart, RetainedRel, Sheet, SheetId, StringId,
    StyleId, Workbook,
};

use crate::{Operation, SheetFields, SheetMetadata};

/// A part of the model a restore could not express as an operation.
///
/// Not a warning to be logged and forgotten: it is the record of a difference
/// between the snapshot and the document that the restore **left standing**,
/// which is exactly the thing a user would otherwise discover by noticing it
/// later. `sheet` is `None` for a workbook-level field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unexpressed {
    /// The sheet the field belongs to, by index in the restored document, or
    /// `None` when the field is the workbook's own.
    pub sheet: Option<usize>,
    /// The model field, named as it is spelled in the model.
    pub field: &'static str,
}

/// What a restore will do, and what it cannot do.
///
/// Produced by [`plan`]. The operation has already had every identifier
/// re-interned into the live workbook, so it is applied through the ordinary
/// [`apply`](crate::apply) — there is nothing special about it downstream, and
/// that is the point.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoreReport {
    /// The edits, as one batch. Empty batch when the document already matches
    /// the snapshot; [`apply`](crate::apply) treats that as changing nothing,
    /// so it leaves no undo entry.
    pub op: Operation,
    /// Cells written or cleared.
    pub cells_changed: usize,
    /// Sheets the snapshot has that the document had lost.
    pub sheets_added: usize,
    /// Sheets the document has that the snapshot did not.
    pub sheets_removed: usize,
    /// Differences the operation set cannot carry. See the module docs.
    pub unexpressed: Vec<Unexpressed>,
}

impl RestoreReport {
    /// Whether this restore would change nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(&self.op, Operation::Batch(ops) if ops.is_empty())
    }
}

/// Memo tables for one restore, so a repeated meaning is interned once.
///
/// Without these, `intern` allocates a `String` key **per call even on a hit**
/// (`StringTable::intern_runs`), so a million cells sharing one string cost a
/// million allocations to discover they share it.
#[derive(Default)]
struct Interner {
    strings: BTreeMap<StringId, StringId>,
    styles: BTreeMap<StyleId, StyleId>,
    formulas: BTreeMap<FormulaHandle, FormulaHandle>,
}

impl Interner {
    fn string(&mut self, live: &mut Workbook, from: &Workbook, id: StringId) -> StringId {
        if let Some(&mine) = self.strings.get(&id) {
            return mine;
        }
        // Rich text keeps its runs. Going through the plain path would flatten
        // a cell's formatting into its text and say nothing about it.
        let mine = match from.strings.runs(id) {
            Some(runs) => live.intern_rich_text(runs.to_vec()),
            None => live.intern_string(from.strings.get(id).unwrap_or_default()),
        };
        self.strings.insert(id, mine);
        mine
    }

    fn style(&mut self, live: &mut Workbook, from: &Workbook, id: StyleId) -> Option<StyleId> {
        if let Some(&mine) = self.styles.get(&id) {
            return Some(mine);
        }
        // A style id that resolves to nothing in the snapshot is dropped rather
        // than carried: carried, it would index the live table and silently
        // name some other style. Same rule as `wire::WireOperation::localise`.
        let style = from.styles.get(id)?.clone();
        let mine = live.intern_style(style);
        self.styles.insert(id, mine);
        Some(mine)
    }

    fn formula(
        &mut self,
        live: &mut Workbook,
        from: &Workbook,
        handle: FormulaHandle,
    ) -> Option<FormulaHandle> {
        if let Some(&mine) = self.formulas.get(&handle) {
            return Some(mine);
        }
        // Stored form on both sides. A workbook's arena holds each formula
        // relative to the cell that carries it (`PERF-11`), and a restore never
        // moves a cell — the address is the same in both documents — so the
        // relative tree means the same thing here as there and needs no
        // re-origining.
        let expr = from.formula(handle)?.clone();
        let mine = live.store_formula(expr);
        self.formulas.insert(handle, mine);
        Some(mine)
    }

    /// The snapshot's cell, said in the live workbook's identifiers.
    fn cell(&mut self, live: &mut Workbook, from: &Workbook, cell: &Cell) -> Cell {
        Cell {
            value: match &cell.value {
                CellValue::SharedString(id) => {
                    CellValue::SharedString(self.string(live, from, *id))
                }
                CellValue::InlineString(id) => {
                    CellValue::InlineString(self.string(live, from, *id))
                }
                other => (*other).clone(),
            },
            style: cell.style.and_then(|id| self.style(live, from, id)),
            formula: cell.formula.and_then(|h| self.formula(live, from, h)),
            flags: cell.flags,
        }
    }
}

/// Whether two values mean the same thing in two different workbooks.
///
/// Numbers compare by **bits**, not by `==`: a cached `NaN` result would
/// otherwise never compare equal to itself and every recalculated error cell
/// would be rewritten on every restore, and `0.0 == -0.0` would hide a real
/// difference. Determinism is ordered above convenience here (AGENTS.md).
fn same_value(live: &Workbook, mine: &CellValue, from: &Workbook, theirs: &CellValue) -> bool {
    match (mine, theirs) {
        (CellValue::Number(a), CellValue::Number(b)) => a.to_bits() == b.to_bits(),
        (CellValue::SharedString(a), CellValue::SharedString(b))
        | (CellValue::InlineString(a), CellValue::InlineString(b)) => {
            live.strings.get(*a) == from.strings.get(*b)
                && live.strings.runs(*a) == from.strings.runs(*b)
        }
        _ => mine == theirs,
    }
}

/// Whether a cell already holds what the snapshot holds.
///
/// Compared by **meaning**, never by identifier. Comparing ids would call two
/// identical cells different whenever the two tables happened to number them
/// differently, which is almost always — and a restore that rewrites every
/// populated cell is one that cannot be undone in a bounded amount of memory.
fn same_cell(live: &Workbook, mine: &Cell, from: &Workbook, theirs: &Cell) -> bool {
    if mine.flags != theirs.flags {
        return false;
    }
    if !same_value(live, &mine.value, from, &theirs.value) {
        return false;
    }
    let style = match (mine.style, theirs.style) {
        (None, None) => true,
        (Some(a), Some(b)) => live.styles.get(a) == from.styles.get(b),
        _ => false,
    };
    if !style {
        return false;
    }
    match (mine.formula, theirs.formula) {
        (None, None) => true,
        (Some(a), Some(b)) => live.formula(a) == from.formula(b),
        _ => false,
    }
}

/// Sheet fields that no operation carries, checked one by one.
///
/// Written as an explicit list rather than derived from `Sheet`, because the
/// point is to fail loudly when the model grows: a field added to `Sheet` and
/// not added here is a field a restore drops in silence. The paired
/// `every_sheet_field_is_carried_by_an_operation_or_counted_as_unexpressed`
/// test is what holds the two
/// together.
fn unexpressed_sheet_fields(
    index: usize,
    mine: &Sheet,
    theirs: &Sheet,
    out: &mut Vec<Unexpressed>,
) {
    let mut note = |field| {
        out.push(Unexpressed {
            sheet: Some(index),
            field,
        });
    };
    if mine.outline != theirs.outline {
        note("outline");
    }
    if mine.images != theirs.images {
        note("images");
    }
    if mine.format_pr != theirs.format_pr {
        note("format_pr");
    }
    if mine.carried != theirs.carried {
        note("carried");
    }
    if mine.retained_refs != theirs.retained_refs {
        note("retained_refs");
    }
}

/// Workbook fields that no operation carries.
///
/// `carried` is the set of retained part paths the sheet bundles are bringing
/// back with a chart, so a part the restore *does* recover is not also reported
/// as one it could not — a report that cries wolf on every restore is a report
/// nobody reads.
fn unexpressed_workbook_fields(
    mine: &Workbook,
    theirs: &Workbook,
    carried: &BTreeSet<String>,
    out: &mut Vec<Unexpressed>,
) {
    let mut note = |field| {
        out.push(Unexpressed { sheet: None, field });
    };
    if mine.default_font_name != theirs.default_font_name
        || mine.default_font_size_hp != theirs.default_font_size_hp
    {
        note("default_font");
    }
    if mine.theme_colors != theirs.theme_colors {
        note("theme_colors");
    }
    if mine.settings != theirs.settings {
        note("settings");
    }
    if mine.properties != theirs.properties {
        note("properties");
    }
    if mine.cell_styles != theirs.cell_styles {
        note("cell_styles");
    }
    if mine.date1904 != theirs.date1904 {
        note("date1904");
    }
    let here: BTreeSet<&str> = mine
        .retained_parts
        .iter()
        .map(|p| p.path.as_str())
        .collect();
    let there: BTreeSet<&str> = theirs
        .retained_parts
        .iter()
        .map(|p| p.path.as_str())
        .collect();
    let unrecovered = there.difference(&here).any(|path| !carried.contains(*path));
    // A part that arrived after the snapshot stays: only a chart part nothing
    // references any more is swept, and an external link or a pivot cache is
    // neither. Reported in both directions, because "the restore left something
    // here" and "the restore could not bring something back" are both
    // differences a user would otherwise find by accident.
    let unremoved = here.difference(&there).next().is_some();
    if unrecovered || unremoved {
        note("retained_parts");
    }
    if mine.retained_refs != theirs.retained_refs {
        note("retained_refs");
    }
}

/// The retained chart bytes a sheet's restored chart list will need.
///
/// A chart is modelled for display only; its XML lives in a retained part, and
/// deleting the chart deleted the part. Putting the chart back without its
/// bytes would write a reference to a part the package does not contain, which
/// Excel refuses to open. [`Operation::SetSheetMetadata`] already carries a
/// `restore` payload for exactly this — it is what undoing a chart deletion
/// uses — so a restore fills it the same way.
fn retained_for_charts(live: &Workbook, from: &Workbook, sheet: &Sheet) -> crate::RetainedBytes {
    let missing: BTreeSet<&str> = sheet
        .charts
        .iter()
        .filter_map(|c| c.part.as_deref())
        .filter(|path| !live.retained_parts.iter().any(|p| p.path == *path))
        .collect();
    if missing.is_empty() {
        return crate::RetainedBytes::default();
    }
    // Rels are matched on the target's **file name**, which is the rule
    // `take_retained` uses when a chart deletion sweeps them out. The two
    // directions have to agree or an undo of a restore drops what the restore
    // put back.
    let files: BTreeSet<&str> = missing
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p))
        .collect();
    let parts: Vec<RetainedPart> = from
        .retained_parts
        .iter()
        .filter(|p| missing.contains(p.path.as_str()))
        .cloned()
        .collect();
    let rels: Vec<RetainedRel> = from
        .retained_rels
        .iter()
        .filter(|r| {
            r.target
                .rsplit('/')
                .next()
                .is_some_and(|file| files.contains(file))
        })
        .cloned()
        .collect();
    crate::RetainedBytes { parts, rels }
}

/// The edits that carry `live` back to `snapshot`.
///
/// `live` is taken by `&mut` because the snapshot's meanings are interned into
/// its tables as they are lifted out — the same growth
/// [`localise`](crate::wire::WireOperation::localise) performs when an operation
/// arrives from a peer. The workbook's **content** is untouched: nothing is
/// applied here, and an operation that is never applied leaves only unreferenced
/// table entries behind.
///
/// Sheets are matched by [`SheetId`], not by name or position, so a restore
/// after a rename still recognises the sheet it is restoring.
#[must_use]
pub fn plan(live: &mut Workbook, snapshot: &Workbook) -> RestoreReport {
    let mut ops = Vec::new();
    let mut interner = Interner::default();
    let mut cells_changed = 0usize;
    let mut sheets_added = 0usize;
    let mut sheets_removed = 0usize;
    let mut unexpressed = Vec::new();
    // The retained part paths the sheet bundles are bringing back with a chart.
    let mut carried: BTreeSet<String> = BTreeSet::new();

    let target_ids: Vec<SheetId> = snapshot.sheets.iter().map(|s| s.id).collect();
    let wanted: BTreeSet<SheetId> = target_ids.iter().copied().collect();

    // `order` models the sheet list as the batch will leave it, so every index
    // an operation carries is the index that operation will see. A batch is
    // applied member by member, so indices computed against the *starting*
    // document are wrong from the second member onward.
    let mut order: Vec<SheetId> = live.sheets.iter().map(|s| s.id).collect();

    // 1. Sheets the snapshot does not have. Highest index first, so removing one
    //    does not renumber the next.
    for index in (0..order.len()).rev() {
        if !wanted.contains(&order[index]) {
            ops.push(Operation::RemoveSheet { index });
            order.remove(index);
            sheets_removed += 1;
        }
    }

    // 2. Sheets the document has lost. Inserted whole, with every identifier
    //    re-interned, so nothing more is needed for them below.
    let mut inserted: BTreeSet<SheetId> = BTreeSet::new();
    for (position, id) in target_ids.iter().enumerate() {
        if order.contains(id) {
            continue;
        }
        let source = &snapshot.sheets[position];
        let mut sheet = source.clone();
        let addresses: Vec<CellRef> = source.cells.iter().map(|(at, _)| at).collect();
        for at in addresses {
            let Some(cell) = source.cells.get(at) else {
                continue;
            };
            let localised = interner.cell(live, snapshot, cell);
            sheet.cells.set(at, localised);
            cells_changed += 1;
        }
        // Deliberately **not** counted as carried. `apply` gives `InsertSheet`
        // no `restore` payload, so a re-inserted sheet arrives with its chart
        // list and without the retained XML those charts are drawn from —
        // which `unexpressed_workbook_fields` must therefore still report.
        // Suppressing it here would be the report telling a comfortable lie.
        let index = position.min(order.len());
        ops.push(Operation::InsertSheet {
            index,
            sheet: Box::new(sheet),
        });
        order.insert(index, *id);
        inserted.insert(*id);
        sheets_added += 1;
    }

    // 3. Tab order. Selection sort into the snapshot's order: at most one move
    //    per sheet, and each is expressed in the coordinates it will run in.
    for (position, want) in target_ids.iter().enumerate() {
        let Some(from) = order.iter().position(|id| id == want) else {
            continue;
        };
        if from != position {
            ops.push(Operation::MoveSheet { from, to: position });
            let id = order.remove(from);
            order.insert(position, id);
        }
    }

    // 4. Content, for the sheets that were already here.
    for (index, id) in order.iter().enumerate() {
        if inserted.contains(id) {
            continue;
        }
        let Some(theirs) = snapshot.sheets.iter().find(|s| s.id == *id) else {
            continue;
        };
        let Some(mine) = live.sheets.iter().find(|s| s.id == *id) else {
            continue;
        };
        // Cloned out of the borrow: the cell walk interns into `live`, and the
        // sheet cannot be borrowed across that.
        let mine = mine.clone();

        if mine.name != theirs.name {
            ops.push(Operation::RenameSheet {
                index,
                name: theirs.name.clone(),
            });
        }
        if mine.tab_color != theirs.tab_color {
            ops.push(Operation::SetTabColor {
                sheet: index,
                color: theirs.tab_color.clone(),
            });
        }

        // Cells. The union of both address sets: one side alone would miss
        // either the cells to write or the cells to clear.
        let mut addresses: BTreeSet<CellRef> = theirs.cells.iter().map(|(at, _)| at).collect();
        addresses.extend(mine.cells.iter().map(|(at, _)| at));
        for at in addresses {
            let want = theirs.cells.get(at);
            let have = mine.cells.get(at);
            match (have, want) {
                (Some(a), Some(b)) if same_cell(live, a, snapshot, b) => {}
                (None, None) => {}
                (_, Some(b)) => {
                    let cell = interner.cell(live, snapshot, b);
                    ops.push(Operation::SetCell {
                        sheet: index,
                        at,
                        cell: Some(cell),
                    });
                    cells_changed += 1;
                }
                (Some(_), None) => {
                    ops.push(Operation::SetCell {
                        sheet: index,
                        at,
                        cell: None,
                    });
                    cells_changed += 1;
                }
            }
        }

        // Positional metadata, as one bundle. `apply` narrows `changed` to what
        // actually differs and the inverse inherits the narrowing, so undo
        // touches only the fields the restore touched.
        let data = SheetMetadata::capture(theirs);
        let changed = data.diff(&SheetMetadata::capture(&mine));
        if !changed.is_empty() {
            let restore = retained_for_charts(live, snapshot, theirs);
            carried.extend(restore.parts.iter().map(|part| part.path.clone()));
            ops.push(Operation::SetSheetMetadata {
                sheet: index,
                data: Box::new(data),
                changed: SheetFields::ALL.intersection(changed),
                restore,
            });
        }

        unexpressed_sheet_fields(index, &mine, theirs, &mut unexpressed);
    }

    // 5. Workbook level.
    if live.defined_names != snapshot.defined_names {
        ops.push(Operation::SetDefinedNames(snapshot.defined_names.clone()));
    }
    unexpressed_workbook_fields(live, snapshot, &carried, &mut unexpressed);

    RestoreReport {
        op: Operation::Batch(ops),
        cells_changed,
        sheets_added,
        sheets_removed,
        unexpressed,
    }
}
