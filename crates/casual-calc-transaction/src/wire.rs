//! Making an operation mean the same thing on another replica.
//!
//! [`Operation`] is fast because a cell refers to its formula and its style by
//! **handle** — an index into the workbook's own arena and style table. That is
//! right for local work and wrong on a wire: `FormulaHandle(7)` is the seventh
//! formula *this* workbook happens to have interned, and the seventh of another
//! workbook is a different formula, or none at all.
//!
//! The failure is silent and worse than losing the formula. A chunk carrying a
//! foreign handle commits without error; the writer then finds a handle
//! indexing nothing and drops **the whole cell**, not merely its formula.
//!
//! # Why a side table rather than a portable mirror of the op set
//!
//! The obvious fix is a parallel `WireOperation` tree carrying expressions and
//! styles by value. It is also a second definition of every operation, kept in
//! step by hand, for a problem that lives in exactly one type — `Cell` is the
//! only thing in the model holding a handle, since even `DefinedName` carries
//! its expression by value.
//!
//! So the operation travels unchanged and takes its meanings with it:
//! [`WireOperation`] is an operation plus the formulas and styles its handles
//! refer to. The receiver interns those into its own tables and rewrites the
//! handles to match. One type, one transform matrix, one `apply`.

use std::collections::BTreeMap;

use casual_calc_formula::Expr;
use casual_calc_model::{CellValue, FormulaHandle, Sheet, StringId, Style, StyleId, Workbook};

use crate::Operation;

/// An operation together with what its handles mean.
///
/// Produced with [`WireOperation::of`] against the workbook the operation was
/// written on, and turned back into an [`Operation`] with
/// [`WireOperation::localise`] against the workbook it is arriving at.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireOperation {
    /// The operation, with the sender's handles still in it.
    pub op: Operation,
    /// Every formula the operation's handles refer to, by the sender's index.
    #[serde(with = "interned_keys")]
    pub formulas: BTreeMap<FormulaHandle, Expr>,
    /// Every style the operation's ids refer to, by the sender's index.
    #[serde(with = "interned_keys")]
    pub styles: BTreeMap<StyleId, Style>,
    /// Every string the operation's ids refer to, by the sender's index.
    ///
    /// The third interned id, and the one COL-12 missed. A `StringId` is as
    /// replica-local as a `FormulaHandle` or a `StyleId` — the tables are
    /// separate and number independently — so a cell whose text is
    /// `SharedString(1)` means *the sender's* first string and not the
    /// receiver's.
    ///
    /// Without this the failure is silent and total: two participants each type
    /// a word, each interns it as id 1, and each ends up seeing the other's
    /// operation resolve to their own word. Nothing errors, both sides are
    /// self-consistent, and the documents differ.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "interned_keys"
    )]
    pub strings: BTreeMap<StringId, String>,
    /// The runs of any string above that is **rich text** (`COL-62`).
    ///
    /// Separate from `strings` rather than replacing it, and optional, because
    /// that is what keeps this off `PROTOCOL_VERSION`. This is the `Draft`
    /// case, not the `CHT-07` one: `WireOperation` does not
    /// `deny_unknown_fields`, so an old peer skips this and reads the rest
    /// exactly as it does today — flattened, which is the behaviour it already
    /// had — and a new peer receiving a message without it concludes "not rich
    /// text", which is what such a sender means. Nobody is misled, so nobody is
    /// refused, and a bump would cost every unupgraded tab its session to fix
    /// formatting.
    ///
    /// Without it, `localise` re-interned through `intern_string` and the runs
    /// were dropped: two people editing one cell of mixed bold and plain
    /// converged on the flattened text, silently. `COL-12` fixed the *identity*
    /// half of that class — an id meaning different things to two participants
    /// — and this is the *content* half, where the id resolved correctly and
    /// what it resolved to had been made poorer.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "interned_keys"
    )]
    pub runs: BTreeMap<StringId, Vec<casual_calc_model::TextRun>>,
}

/// Serializing a map whose key is an interned id.
///
/// # Why this exists
///
/// JSON object keys are strings, always. A `serde_json` map with an integer key
/// therefore writes the number as a string and parses it back on the way in —
/// but only for the *primitive* integer types, which it recognises by the hint
/// the key's deserializer asks with. [`StyleId`] and [`StringId`] wrap a
/// `NonZeroU32`, whose deserializer asks for something `serde_json`'s map-key
/// path does not special-case, and the parse fails with `invalid type: string
/// "8", expected a nonzero u32`.
///
/// The half that matters: **serializing works perfectly.** A sender produces a
/// message that looks completely correct and no receiver can read it. The
/// server dropped every such message without a word, so a browser could join a
/// document, type, see its own text locally, and silently send nothing anybody
/// else would ever get — for every text edit and every style edit there is.
///
/// It was invisible to the tests because the round-trip tests that go through
/// JSON carried operations with these tables *empty*, and the tests that
/// carried them full round-tripped through [`localise`](WireOperation::localise)
/// rather than through serde. Each half was covered and the crossing was not.
///
/// Nothing about the format changes: the wire looked like this already. What
/// changes is that it can now be read back.
mod interned_keys {
    use std::collections::BTreeMap;
    use std::fmt::Display;
    use std::str::FromStr;

    use serde::de::{Error as _, MapAccess, Visitor};
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S, K, V>(map: &BTreeMap<K, V>, out: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Display,
        V: serde::Serialize,
    {
        out.collect_map(map.iter().map(|(k, v)| (k.to_string(), v)))
    }

    pub(super) fn deserialize<'de, D, K, V>(input: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: FromStr + Ord,
        K::Err: Display,
        V: serde::Deserialize<'de>,
    {
        struct Keyed<K, V>(std::marker::PhantomData<(K, V)>);

        impl<'de, K, V> Visitor<'de> for Keyed<K, V>
        where
            K: FromStr + Ord,
            K::Err: Display,
            V: serde::Deserialize<'de>,
        {
            type Value = BTreeMap<K, V>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a map keyed by interned ids written as decimal strings")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
                let mut map = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<String, V>()? {
                    // A key that is not a number is a corrupt or hostile
                    // message, and refusing it is right: the alternative is
                    // dropping one entry of a table and localising an operation
                    // against a meaning that is no longer there.
                    let key = key.parse().map_err(M::Error::custom)?;
                    map.insert(key, value);
                }
                Ok(map)
            }
        }

        input.deserialize_map(Keyed(std::marker::PhantomData))
    }
}

impl WireOperation {
    /// Package `op` with the meanings its handles have in `workbook`.
    ///
    /// A handle that resolves to nothing is left out rather than guessed at, so
    /// [`Self::localise`] sees the same absence the sender had rather than a
    /// silently different formula.
    #[must_use]
    pub fn of(op: Operation, workbook: &Workbook) -> Self {
        let mut formulas = BTreeMap::new();
        let mut styles = BTreeMap::new();
        let mut strings = BTreeMap::new();
        let mut runs = BTreeMap::new();
        visit_strings(&op, &mut |id| {
            if let Some(text) = workbook.strings.get(id) {
                strings.insert(id, text.to_owned());
            }
            // Carried beside the characters, never instead of them: a receiver
            // that cannot read this still gets the text.
            if let Some(found) = workbook.strings.runs(id)
                && !found.is_empty()
            {
                runs.insert(id, found.to_vec());
            }
        });
        visit(&op, &mut |formula, style| {
            if let Some(handle) = formula
                && let Some(expr) = workbook.formula(handle)
            {
                formulas.insert(handle, expr.clone());
            }
            if let Some(id) = style
                && let Some(found) = workbook.styles.get(id)
            {
                styles.insert(id, found.clone());
            }
        });
        Self {
            op,
            formulas,
            styles,
            strings,
            runs,
        }
    }

    /// Rewrite the handles so they mean the same thing in `workbook`.
    ///
    /// Interning is by value, so two replicas that arrive at the same formula
    /// or the same style converge on whatever their own tables already hold
    /// rather than growing a duplicate each time an operation crosses.
    #[must_use]
    pub fn localise(self, workbook: &mut Workbook) -> Operation {
        let Self {
            mut op,
            formulas,
            styles,
            strings,
            runs,
        } = self;

        let mut formula_map = BTreeMap::new();
        for (theirs, expr) in formulas {
            formula_map.insert(theirs, workbook.store_formula(expr));
        }
        let mut style_map = BTreeMap::new();
        for (theirs, style) in styles {
            style_map.insert(theirs, workbook.intern_style(style));
        }

        let mut string_map = BTreeMap::new();
        for (theirs, text) in strings {
            // Rich text is interned **with its runs**, the way `restore` does
            // it. Going through the plain path here is what flattened it.
            let mine = match runs.get(&theirs) {
                Some(found) => workbook.intern_rich_text(found.clone()),
                None => workbook.intern_string(&text),
            };
            string_map.insert(theirs, mine);
        }
        visit_strings_mut(&mut op, &mut |id| {
            // An id with no accompanying text is left alone rather than
            // dropped: unlike a formula handle it addresses a *value*, and
            // clearing it would erase the cell's contents rather than degrade
            // them. Leaving it is wrong in a visible way; dropping it is wrong
            // in an invisible one.
            if let Some(mine) = string_map.get(id) {
                *id = *mine;
            }
        });

        visit_mut(&mut op, &mut |formula, style| {
            // A handle with no accompanying meaning is dropped, not kept: kept,
            // it would index this workbook's arena and silently name some other
            // replica's formula.
            *formula = formula.and_then(|handle| formula_map.get(&handle).copied());
            *style = style.and_then(|id| style_map.get(&id).copied());
        });
        op
    }
}

/// Visit every interned **string** id an operation carries.
///
/// Separate from [`visit`] because the slot is a different shape: a string id
/// lives inside a [`CellValue`], not beside it, and a cell has exactly one
/// value where it has both a formula handle and a style id.
fn visit_strings(op: &Operation, f: &mut impl FnMut(StringId)) {
    let mut note = |value: &CellValue| {
        if let CellValue::SharedString(id) | CellValue::InlineString(id) = value {
            f(*id);
        }
    };
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => note(&cell.value),
        Operation::SetValue { value, .. } => note(value),
        Operation::InsertSheet { sheet, .. } => {
            for (_, cell) in sheet.cells.iter() {
                note(&cell.value);
            }
        }
        Operation::Batch(ops) => {
            for member in ops {
                visit_strings(member, f);
            }
        }
        _ => {}
    }
}

/// The same walk, rewriting each id through `f`.
fn visit_strings_mut(op: &mut Operation, f: &mut impl FnMut(&mut StringId)) {
    fn rewrite(value: &mut CellValue, f: &mut impl FnMut(&mut StringId)) {
        if let CellValue::SharedString(id) | CellValue::InlineString(id) = value {
            f(id);
        }
    }
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => rewrite(&mut cell.value, f),
        Operation::SetValue { value, .. } => rewrite(value, f),
        Operation::InsertSheet { sheet, .. } => {
            let addresses: Vec<_> = sheet.cells.iter().map(|(at, _)| at).collect();
            for at in addresses {
                if let Some(mut cell) = sheet.cells.get(at).cloned() {
                    rewrite(&mut cell.value, f);
                    sheet.cells.set(at, cell);
                }
            }
        }
        Operation::Batch(ops) => {
            for member in ops {
                visit_strings_mut(member, f);
            }
        }
        _ => {}
    }
}

/// Visit every handle-bearing slot an operation carries.
///
/// Two slots, not one: a cell holds both a formula handle and a style id,
/// and [`Operation::SetStyle`] holds a bare style id with no cell around it.
/// Missing the second is how a style crosses replicas meaning something else —
/// which it did, until this walk covered it.
fn visit(op: &Operation, f: &mut impl FnMut(Option<FormulaHandle>, Option<StyleId>)) {
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => f(cell.formula, cell.style),
        Operation::SetStyle { style, .. } => f(None, *style),
        Operation::InsertSheet { sheet, .. } => {
            for (_, cell) in sheet.cells.iter() {
                f(cell.formula, cell.style);
            }
        }
        Operation::Batch(ops) => {
            for member in ops {
                visit(member, f);
            }
        }
        _ => {}
    }
}

/// The same walk, rewriting each slot through `f`.
fn visit_mut(
    op: &mut Operation,
    f: &mut impl FnMut(&mut Option<FormulaHandle>, &mut Option<StyleId>),
) {
    match op {
        Operation::SetCell {
            cell: Some(cell), ..
        } => f(&mut cell.formula, &mut cell.style),
        Operation::SetStyle { style, .. } => f(&mut None, style),
        Operation::InsertSheet { sheet, .. } => rewrite_sheet(sheet, f),
        Operation::Batch(ops) => {
            for member in ops {
                visit_mut(member, f);
            }
        }
        _ => {}
    }
}

/// Rebuild a sheet's cells through `f`.
///
/// The store has no mutable iterator, and giving it one to serve this would
/// widen a type used everywhere for the sake of a path used once.
fn rewrite_sheet(
    sheet: &mut Sheet,
    f: &mut impl FnMut(&mut Option<FormulaHandle>, &mut Option<StyleId>),
) {
    let existing: Vec<_> = sheet
        .cells
        .iter()
        .map(|(at, cell)| (at, cell.clone()))
        .collect();
    for (at, mut cell) in existing {
        f(&mut cell.formula, &mut cell.style);
        sheet.cells.set(at, cell);
    }
}

/// Whether an operation refers to a formula or a style by handle.
///
/// What to check before sending an operation that has not been through
/// [`WireOperation::of`]: such an operation is not wrong here, only meaningless
/// anywhere else.
#[must_use]
pub fn carries_handles(op: &Operation) -> bool {
    let mut found = false;
    visit(op, &mut |formula, style| {
        found |= formula.is_some() || style.is_some();
    });
    found
}

/// A formula handle that resolves to nothing in `workbook`.
///
/// A server can use this to refuse an operation rather than commit one whose
/// cell the writer will silently drop.
#[must_use]
pub fn dangling_handle(op: &Operation, workbook: &Workbook) -> Option<FormulaHandle> {
    let mut dangling = None;
    visit(op, &mut |formula, _| {
        if let Some(handle) = formula
            && workbook.formula(handle).is_none()
        {
            dangling = Some(handle);
        }
    });
    dangling
}

#[cfg(test)]
mod tests {
    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, Workbook};

    use super::*;

    fn workbook(namespace: u64) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(namespace, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(namespace, 2)), "S"));
        wb
    }

    fn bold() -> Style {
        Style {
            bold: true,
            ..Style::default()
        }
    }

    #[test]
    fn a_formula_crosses_to_a_workbook_that_has_never_seen_it() {
        // The sender has interned other formulas first, so its handle is not
        // one the receiver would allocate — which is the whole failure.
        let mut sender = workbook(1);
        sender.store_formula(casual_calc_formula::parse("1").unwrap());
        sender.store_formula(casual_calc_formula::parse("2").unwrap());
        let handle = sender.store_formula(casual_calc_formula::parse("3+4").unwrap());
        assert_eq!(handle, casual_calc_model::FormulaHandle(2));

        let mut cell = Cell::value(CellValue::Number(7.0));
        cell.formula = Some(handle);
        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        };

        let wire = WireOperation::of(op, &sender);
        let mut receiver = workbook(2);
        let localised = wire.localise(&mut receiver);

        let Operation::SetCell {
            cell: Some(cell), ..
        } = localised
        else {
            panic!("still a cell edit")
        };
        let landed = cell.formula.and_then(|h| receiver.formula(h)).cloned();
        assert_eq!(
            landed,
            Some(casual_calc_formula::parse("3+4").unwrap()),
            "the expression arrived, whatever index it took here"
        );
        assert_ne!(
            cell.formula,
            Some(handle),
            "and it is the receiver's handle"
        );
    }

    #[test]
    fn a_style_crosses_too_including_the_bare_one_set_by_set_style() {
        let mut sender = workbook(1);
        sender.intern_style(Style {
            italic: true,
            ..Style::default()
        });
        let id = sender.intern_style(bold());

        let mut receiver = workbook(2);
        let localised = WireOperation::of(
            Operation::SetStyle {
                sheet: 0,
                at: CellRef::new(1, 1),
                style: Some(id),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::SetStyle {
            style: Some(landed),
            ..
        } = localised
        else {
            panic!("still a style edit")
        };
        assert_eq!(
            receiver.styles.get(landed).cloned(),
            Some(bold()),
            "SetStyle carries a bare id with no cell around it, and it travels"
        );
    }

    #[test]
    fn interning_by_value_does_not_duplicate_what_the_receiver_already_has() {
        let mut sender = workbook(1);
        let theirs = sender.intern_style(bold());
        let mut receiver = workbook(2);
        let mine = receiver.intern_style(bold());

        let localised = WireOperation::of(
            Operation::SetStyle {
                sheet: 0,
                at: CellRef::new(0, 0),
                style: Some(theirs),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::SetStyle {
            style: Some(landed),
            ..
        } = localised
        else {
            panic!("still a style edit")
        };
        assert_eq!(
            landed, mine,
            "the same style is the same id, not a second one"
        );
    }

    #[test]
    fn a_sheet_full_of_cells_travels_with_all_of_them() {
        let mut sender = workbook(1);
        let handle = sender.store_formula(casual_calc_formula::parse("9*9").unwrap());
        let style = sender.intern_style(bold());
        let mut sheet = Sheet::new(SheetId(Id::from_parts(1, 9)), "added");
        for row in 0..3u32 {
            let mut cell = Cell::value(CellValue::Number(f64::from(row)));
            cell.formula = Some(handle);
            cell.style = Some(style);
            sheet.cells.set(CellRef::new(row, 0), cell);
        }

        let mut receiver = workbook(2);
        let localised = WireOperation::of(
            Operation::InsertSheet {
                index: 0,
                sheet: Box::new(sheet),
            },
            &sender,
        )
        .localise(&mut receiver);

        let Operation::InsertSheet { sheet, .. } = localised else {
            panic!("still a sheet insert")
        };
        for (_, cell) in sheet.cells.iter() {
            assert_eq!(
                cell.formula.and_then(|h| receiver.formula(h)).cloned(),
                Some(casual_calc_formula::parse("9*9").unwrap()),
                "every cell in the sheet, not just the first"
            );
            assert_eq!(
                cell.style.and_then(|s| receiver.styles.get(s)).cloned(),
                Some(bold())
            );
        }
    }

    #[test]
    fn a_handle_the_sender_could_not_resolve_is_dropped_rather_than_carried() {
        // Carrying it would index the receiver's arena and silently name some
        // other formula — a wrong answer where the sender had none.
        let sender = workbook(1);
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(casual_calc_model::FormulaHandle(99));
        let wire = WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(cell),
            },
            &sender,
        );
        assert!(wire.formulas.is_empty());

        let mut receiver = workbook(2);
        receiver.store_formula(casual_calc_formula::parse("1+1").unwrap());
        let Operation::SetCell {
            cell: Some(cell), ..
        } = wire.localise(&mut receiver)
        else {
            panic!("still a cell edit")
        };
        assert_eq!(cell.formula, None, "dropped, not silently rebound");
    }

    #[test]
    fn carrying_no_handles_is_detectable() {
        let plain = Operation::SetValue {
            sheet: 0,
            at: CellRef::new(0, 0),
            value: CellValue::Number(1.0),
        };
        assert!(!carries_handles(&plain));

        let mut wb = workbook(1);
        let handle = wb.store_formula(casual_calc_formula::parse("1").unwrap());
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(handle);
        assert!(carries_handles(&Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        }));
    }

    #[test]
    fn a_dangling_handle_is_reportable_so_a_server_can_refuse_it() {
        let wb = workbook(1);
        let mut cell = Cell::value(CellValue::Number(1.0));
        cell.formula = Some(casual_calc_model::FormulaHandle(3));
        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(cell),
        };
        assert_eq!(
            dangling_handle(&op, &wb),
            Some(casual_calc_model::FormulaHandle(3))
        );
    }
}

#[cfg(test)]
mod string_tests {
    //! The third interned id.
    //!
    //! COL-12 established that a `FormulaHandle` and a `StyleId` are
    //! replica-local and must be translated rather than trusted. A `StringId`
    //! is exactly as local, from a table that numbers exactly as independently,
    //! and it was left out — found when two participants typing one word each
    //! ended up reading the other's.

    use casual_calc_model::{Cell, CellRef, CellValue, Id, Sheet, SheetId, Workbook};

    use super::*;
    use crate::Operation;

    fn workbook_with(words: &[&str]) -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "S"));
        for word in words {
            wb.intern_string(word);
        }
        wb
    }

    fn text_at(wb: &Workbook, at: CellRef) -> Option<String> {
        let cell = wb.sheets[0].cells.get(at)?;
        match &cell.value {
            CellValue::SharedString(id) | CellValue::InlineString(id) => {
                wb.strings.get(*id).map(str::to_owned)
            }
            _ => None,
        }
    }

    #[test]
    fn text_crossing_replicas_stays_the_text_that_was_typed() {
        // The bug, exactly. Each replica interns one word, so each holds a
        // different string at id 1. Without translation the receiver resolves
        // the sender's id against its own table and reads its own word — no
        // error, both sides self-consistent, the documents different.
        let sender = workbook_with(&["mine"]);
        let mine = sender.strings.id_at(0).expect("interned");

        let op = Operation::SetCell {
            sheet: 0,
            at: CellRef::new(0, 0),
            cell: Some(Cell::value(CellValue::SharedString(mine))),
        };
        let wire = WireOperation::of(op, &sender);
        assert_eq!(wire.strings.len(), 1, "the text travels with the id");

        let mut receiver = workbook_with(&["theirs"]);
        let localised = wire.localise(&mut receiver);
        crate::apply(&mut receiver, localised).unwrap();

        assert_eq!(
            text_at(&receiver, CellRef::new(0, 0)).as_deref(),
            Some("mine"),
            "the receiver read its own string table instead of the sender's"
        );
    }

    #[test]
    fn interning_is_by_value_so_a_shared_word_does_not_duplicate() {
        // Both replicas already know the word; crossing must converge on the
        // receiver's existing id rather than growing the table every time an
        // operation arrives.
        let sender = workbook_with(&["shared"]);
        let theirs = sender.strings.id_at(0).unwrap();
        let mut receiver = workbook_with(&["shared"]);
        let before = receiver.strings.len();

        let wire = WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(Cell::value(CellValue::SharedString(theirs))),
            },
            &sender,
        );
        let _ = wire.localise(&mut receiver);
        assert_eq!(receiver.strings.len(), before, "no duplicate was interned");
    }

    #[test]
    fn a_value_set_without_a_cell_carries_its_text_too() {
        // `SetValue` holds the value directly rather than inside a `Cell`, and
        // a walk that covers one shape and not the other is how half the edits
        // cross correctly and half do not.
        let sender = workbook_with(&["typed"]);
        let id = sender.strings.id_at(0).unwrap();
        let wire = WireOperation::of(
            Operation::SetValue {
                sheet: 0,
                at: CellRef::new(1, 1),
                value: CellValue::SharedString(id),
            },
            &sender,
        );
        assert_eq!(wire.strings.len(), 1);

        let mut receiver = workbook_with(&["something else"]);
        let localised = wire.localise(&mut receiver);
        crate::apply(&mut receiver, localised).unwrap();
        assert_eq!(
            text_at(&receiver, CellRef::new(1, 1)).as_deref(),
            Some("typed")
        );
    }

    #[test]
    fn a_batch_carries_every_string_its_members_use() {
        let sender = workbook_with(&["one", "two"]);
        let (a, b) = (
            sender.strings.id_at(0).unwrap(),
            sender.strings.id_at(1).unwrap(),
        );
        let wire = WireOperation::of(
            Operation::Batch(vec![
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(0, 0),
                    cell: Some(Cell::value(CellValue::SharedString(a))),
                },
                Operation::SetCell {
                    sheet: 0,
                    at: CellRef::new(1, 0),
                    cell: Some(Cell::value(CellValue::SharedString(b))),
                },
            ]),
            &sender,
        );
        assert_eq!(wire.strings.len(), 2);

        let mut receiver = workbook_with(&["x", "y", "z"]);
        let localised = wire.localise(&mut receiver);
        crate::apply(&mut receiver, localised).unwrap();
        assert_eq!(
            text_at(&receiver, CellRef::new(0, 0)).as_deref(),
            Some("one")
        );
        assert_eq!(
            text_at(&receiver, CellRef::new(1, 0)).as_deref(),
            Some("two")
        );
    }

    #[test]
    fn an_operation_with_no_text_carries_no_strings() {
        let sender = workbook_with(&["unused"]);
        let wire = WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(Cell::value(CellValue::Number(1.0))),
            },
            &sender,
        );
        assert!(wire.strings.is_empty(), "nothing to carry, nothing carried");
    }
}

#[cfg(test)]
mod rich_text_on_the_wire {
    use super::*;
    use casual_calc_model::{Cell, CellRef, CellValue, Id, RunFont, Sheet, SheetId, TextRun};

    fn book() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1"));
        wb
    }

    /// Rich text keeps its runs when it crosses to another participant
    /// (`COL-62`).
    ///
    /// `WireOperation` stored each string as a plain `String` and `localise`
    /// re-interned it with `intern_string`, so `StringTable::runs` was dropped
    /// on the way. Two people editing a cell with mixed bold and plain text
    /// converged on the **flattened** text, silently — the formatting simply
    /// stopped existing for whoever received it.
    ///
    /// `COL-12` fixed the *identity* half of this class, where an id meant
    /// different things to two participants. This is the *content* half: the id
    /// resolved correctly and what it resolved to had been made poorer.
    ///
    /// `restore.rs` already had the right shape — it re-interns through
    /// `intern_rich_text` precisely so a restore does not flatten — which is
    /// why the bug surfaced there rather than being fixed there.
    /// A peer that has never heard of runs still reads the message
    /// (`COL-62`).
    ///
    /// This is the claim that keeps the change off `PROTOCOL_VERSION`, so it is
    /// asserted rather than argued. An old peer's `WireOperation` has no `runs`
    /// field and does not `deny_unknown_fields`, so it skips the key and reads
    /// the rest exactly as it does today — flattened, which is the behaviour it
    /// already had. Nobody is misled, so nobody is refused.
    #[test]
    fn a_peer_that_has_never_heard_of_runs_still_reads_a_message_carrying_them() {
        let mut sender = book();
        let id = sender.intern_rich_text(vec![TextRun {
            text: "Bold".to_owned(),
            font: Some(RunFont {
                bold: true,
                ..RunFont::default()
            }),
        }]);
        let wire = WireOperation::of(
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(0, 0),
                cell: Some(Cell::value(CellValue::SharedString(id))),
            },
            &sender,
        );
        let json = serde_json::to_string(&wire).expect("the wire form serialises");
        assert!(
            json.contains("runs"),
            "the fixture must actually carry runs, or this proves nothing about ignoring them"
        );

        // An old peer's shape: every field this message has except `runs`.
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        #[allow(dead_code)]
        struct OldPeer {
            op: serde_json::Value,
            #[serde(default)]
            formulas: serde_json::Value,
            #[serde(default)]
            styles: serde_json::Value,
            #[serde(default)]
            strings: serde_json::Value,
        }
        let read: Result<OldPeer, _> = serde_json::from_str(&json);
        assert!(
            read.is_ok(),
            "a peer without `runs` must still read the message: refusing it would cost every \
             unupgraded tab its session to deliver formatting it cannot show anyway"
        );
    }

    #[test]
    fn a_rich_string_crosses_with_its_formatting() {
        let mut sender = book();
        let runs = vec![
            TextRun {
                text: "Total".to_owned(),
                font: Some(RunFont {
                    bold: true,
                    ..RunFont::default()
                }),
            },
            TextRun {
                text: " so far".to_owned(),
                font: None,
            },
        ];
        let id = sender.intern_rich_text(runs.clone());
        let at = CellRef::new(0, 0);
        let op = Operation::SetCell {
            sheet: 0,
            at,
            cell: Some(Cell::value(CellValue::SharedString(id))),
        };

        let wire = WireOperation::of(op, &sender);

        // A second participant, whose table has never held this string.
        let mut receiver = book();
        let landed = wire.localise(&mut receiver);
        let CellValue::SharedString(theirs) = ({
            let Operation::SetCell { cell: Some(c), .. } = &landed else {
                panic!("the operation changed shape crossing the wire");
            };
            c.value.clone()
        }) else {
            panic!("the value stopped being a shared string");
        };

        assert_eq!(
            receiver.strings.get(theirs),
            Some("Total so far"),
            "the characters must survive at all"
        );
        assert_eq!(
            receiver.strings.runs(theirs).map(<[TextRun]>::to_vec),
            Some(runs),
            "the formatting came across with the text, not only the characters — \
             two people editing one bold-and-plain cell must not converge on plain"
        );
    }
}
