//! Model + snapshot tests. The empty-workbook byte-stable round-trip is the
//! Phase 0 exit-gate condition (`docs/06-ROADMAP-AND-DELIVERY.md`).

use crate::{
    Cell, CellRange, CellRef, CellValue, CustomFilter, DataValidation, DvKind, DvOperator,
    FilterOp, FilterRule, Id, IdGenerator, SCHEMA_VERSION, Sheet, SheetId, StringId, StringTable,
    Workbook,
};

fn wb_id() -> Id {
    Id::from_parts(1, 1)
}

#[test]
fn id_is_nonzero_and_hex_roundtrips() {
    assert!(Id::new(0).is_none());
    let id = Id::from_parts(0xABCD, 0x1234);
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"000000000000abcd0000000000001234\"");
    let back: Id = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn id_generator_produces_unique_nonzero_ids() {
    let mut generator = IdGenerator::new(7);
    let a = generator.next_id();
    let b = generator.next_id();
    assert_ne!(a, b);
    assert_ne!(a.get(), 0);
}

#[test]
fn empty_workbook_snapshot_is_byte_stable() {
    let wb = Workbook::new(wb_id());
    let first = wb.to_snapshot().unwrap();
    let reopened = Workbook::from_snapshot(&first).unwrap();
    assert_eq!(wb, reopened);
    let second = reopened.to_snapshot().unwrap();
    assert_eq!(
        first, second,
        "snapshot must be byte-identical across a round-trip"
    );
    // The empty workbook omits its empty `sheets` vec.
    assert_eq!(
        String::from_utf8(first).unwrap(),
        r#"{"schemaVersion":0,"workbookId":"00000000000000010000000000000001"}"#
    );
}

#[test]
fn populated_workbook_roundtrips_byte_stably() {
    let mut wb = Workbook::new(wb_id());
    let hello = wb.intern_string("hello");
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(42.0)));
    sheet.cells.set(
        CellRef::new(3, 1),
        Cell::value(CellValue::SharedString(hello)),
    );
    wb.sheets.push(sheet);

    let first = wb.to_snapshot().unwrap();
    let reopened = Workbook::from_snapshot(&first).unwrap();
    assert_eq!(wb, reopened);
    let second = reopened.to_snapshot().unwrap();
    assert_eq!(first, second);
    assert_eq!(reopened.schema_version, SCHEMA_VERSION);
}

#[test]
fn blank_cells_are_not_stored() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::Number(1.0)));
    assert_eq!(sheet.cells.len(), 1);
    // Overwriting with a blank cell evicts it.
    sheet.cells.set(CellRef::new(0, 0), Cell::default());
    assert_eq!(sheet.cells.len(), 0);
    assert!(sheet.cells.is_empty());
}

#[test]
fn cells_iterate_in_row_major_order() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    sheet
        .cells
        .set(CellRef::new(5, 0), Cell::value(CellValue::Number(1.0)));
    sheet
        .cells
        .set(CellRef::new(0, 9), Cell::value(CellValue::Number(2.0)));
    sheet
        .cells
        .set(CellRef::new(0, 1), Cell::value(CellValue::Number(3.0)));
    let order: Vec<CellRef> = sheet.cells.iter().map(|(r, _)| r).collect();
    assert_eq!(
        order,
        vec![CellRef::new(0, 1), CellRef::new(0, 9), CellRef::new(5, 0)]
    );
}

#[test]
fn strings_intern_dedupe_and_resolve() {
    let mut table = StringTable::new();
    let a = table.intern("hello");
    let b = table.intern("world");
    let a2 = table.intern("hello");
    assert_eq!(a, a2);
    assert_ne!(a, b);
    assert_eq!(table.len(), 2);
    assert_eq!(table.get(a), Some("hello"));
    assert_eq!(table.get(b), Some("world"));
    // An id from another namespace does not resolve here.
    assert_eq!(table.get(StringId(Id::from_parts(1, 1))), None);
}

#[test]
fn dangling_string_reference_is_rejected() {
    let mut wb = Workbook::new(wb_id());
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    // A shared-string id that was never interned.
    sheet.cells.set(
        CellRef::new(0, 0),
        Cell::value(CellValue::SharedString(StringId(Id::from_parts(
            0x5354_5200_0000_0000,
            99,
        )))),
    );
    wb.sheets.push(sheet);
    let err = wb.validate().unwrap_err();
    assert_eq!(err.code(), "OC-MDL-0001");
}

#[test]
fn duplicate_sheet_ids_are_rejected() {
    let mut wb = Workbook::new(wb_id());
    let dup = SheetId(Id::from_parts(2, 1));
    wb.sheets.push(Sheet::new(dup, "A"));
    wb.sheets.push(Sheet::new(dup, "B"));
    let err = wb.validate().unwrap_err();
    assert_eq!(err.code(), "OC-MDL-0001");
}

#[test]
fn unknown_snapshot_fields_are_rejected() {
    let bytes = br#"{"schemaVersion":0,"workbookId":"00000000000000010000000000000001","bogus":1}"#;
    assert!(Workbook::from_snapshot(bytes).is_err());
}

// --- Autofilter -----------------------------------------------------------

fn eq(value: &str) -> CustomFilter {
    CustomFilter {
        op: FilterOp::Equal,
        value: value.into(),
    }
}

#[test]
fn wildcards_cover_the_contains_begins_ends_shapes() {
    // Excel has no dedicated operators for these; they are `equal` + wildcards.
    assert!(eq("*ap*").matches("Grape", None)); // contains
    assert!(eq("Gr*").matches("Grape", None)); // begins with
    assert!(eq("*pe").matches("Grape", None)); // ends with
    assert!(!eq("*pe").matches("Grapes", None));
    assert!(eq("Gr?pe").matches("Grape", None)); // single-char
    assert!(!eq("Gr?pe").matches("Grpe", None)); // `?` is exactly one
    assert!(eq("grape").matches("GRAPE", None)); // case-insensitive
    assert!(eq("*").matches("", None)); // `*` matches empty
}

#[test]
fn wildcard_backtracks_instead_of_committing_to_the_first_guess() {
    // A greedy non-backtracking matcher lets the first `*` eat "abab" and then
    // fails on the trailing "ab" that is still required.
    assert!(eq("*ab").matches("abab", None));
    assert!(eq("*a*b*c").matches("xaybzc", None));
    assert!(!eq("*a*b*c").matches("xaybz", None));
}

#[test]
fn wildcard_stays_linear_on_a_pathological_pattern() {
    // Would hang under a naive exponential matcher.
    let text = "a".repeat(200);
    assert!(!eq("*a*a*a*a*a*a*a*a*b").matches(&text, None));
}

#[test]
fn ordering_filters_compare_numerically_when_both_sides_are_numbers() {
    let gt = CustomFilter {
        op: FilterOp::GreaterThan,
        value: "9".into(),
    };
    // Numeric, not lexicographic — "10" sorts before "9" as text.
    assert!(gt.matches("10", Some(10.0)));
    assert!(!gt.matches("9", Some(9.0)));
    // No numeric value: falls back to text ordering rather than dropping the row.
    assert!(gt.matches("beta", None));
}

#[test]
fn nan_fails_every_ordering_comparison() {
    for op in [
        FilterOp::GreaterThan,
        FilterOp::GreaterThanOrEqual,
        FilterOp::LessThan,
        FilterOp::LessThanOrEqual,
    ] {
        let f = CustomFilter {
            op,
            value: "1".into(),
        };
        assert!(!f.matches("NaN", Some(f64::NAN)), "{op:?} let NaN through");
    }
}

#[test]
fn two_comparisons_join_with_and_or_or() {
    let between = FilterRule::Custom {
        first: CustomFilter {
            op: FilterOp::GreaterThanOrEqual,
            value: "10".into(),
        },
        second: Some(CustomFilter {
            op: FilterOp::LessThanOrEqual,
            value: "20".into(),
        }),
        and: true,
    };
    assert!(between.matches("15", Some(15.0)));
    assert!(!between.matches("25", Some(25.0)));

    let outside = FilterRule::Custom {
        first: CustomFilter {
            op: FilterOp::LessThan,
            value: "10".into(),
        },
        second: Some(CustomFilter {
            op: FilterOp::GreaterThan,
            value: "20".into(),
        }),
        and: false,
    };
    assert!(outside.matches("25", Some(25.0)));
    assert!(!outside.matches("15", Some(15.0)));
}

#[test]
fn value_lists_match_blanks_through_the_empty_string() {
    let rule = FilterRule::Values(vec!["Apple".into(), String::new()]);
    assert!(rule.matches("Apple", None));
    assert!(rule.matches("apple", None)); // case-insensitive
    assert!(rule.matches("", None)); // the blank entry
    assert!(!rule.matches("Pear", None));
}

#[test]
fn filter_hidden_is_separate_from_hand_hidden_rows() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(1, 1)), "S");
    sheet.hidden_rows.insert(3);
    sheet.filter_hidden.insert(5);
    assert!(sheet.is_row_hidden(3));
    assert!(sheet.is_row_hidden(5));
    assert!(!sheet.is_row_hidden(4));

    // Clearing the filter must not disturb the hand-hidden row.
    sheet.filter_hidden.clear();
    assert!(sheet.is_row_hidden(3));
    assert!(!sheet.is_row_hidden(5));
}

// --- Outline grouping -----------------------------------------------------

fn outlined(levels: &[(u32, u8)]) -> Sheet {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(1, 1)), "S");
    for &(row, level) in levels {
        sheet.row_outline_levels.insert(row, level);
    }
    sheet
}

#[test]
fn outline_band_walks_back_to_the_summary_below() {
    // Rows 1..3 nested under a summary at row 4 (OOXML's default placement).
    let sheet = outlined(&[(1, 1), (2, 1), (3, 1)]);
    assert_eq!(sheet.outline_band(4, false), Some((1, 3)));
    // The summary line is never part of its own band.
    assert!(!(1..=3).contains(&4));
}

#[test]
fn outline_band_walks_forward_when_the_summary_is_above() {
    let mut sheet = outlined(&[(5, 1), (6, 1)]);
    sheet.outline.summary_below = false;
    assert_eq!(sheet.outline_band(4, false), Some((5, 6)));
}

#[test]
fn a_line_with_no_deeper_neighbours_has_no_band() {
    let sheet = outlined(&[(1, 1), (2, 1)]);
    // Row 0 is above the group, so nothing hangs off it.
    assert_eq!(sheet.outline_band(0, false), None);
    // Row 1 is *inside* the group and at the same level as row 2 — not a summary.
    assert_eq!(sheet.outline_band(1, false), None);
}

#[test]
fn nested_groups_resolve_to_their_own_depth() {
    // level: r1=1 r2=2 r3=2 r4=1 (inner summary), r5=0 (outer summary)
    let sheet = outlined(&[(1, 1), (2, 2), (3, 2), (4, 1)]);
    // The inner summary at row 4 takes only the deeper run above it.
    assert_eq!(sheet.outline_band(4, false), Some((2, 3)));
    // The outer summary at row 5 takes everything nested below level 0.
    assert_eq!(sheet.outline_band(5, false), Some((1, 4)));
}

#[test]
fn forward_band_stops_at_the_deepest_recorded_line() {
    // Guards the summary-above walk against running past the outline map.
    let mut sheet = outlined(&[(1, 1), (2, 1)]);
    sheet.outline.summary_below = false;
    assert_eq!(sheet.outline_band(0, false), Some((1, 2)));
}

#[test]
fn column_bands_follow_summary_right() {
    let mut sheet = Sheet::new(SheetId(Id::from_parts(1, 1)), "S");
    for c in 1..=3 {
        sheet.col_outline_levels.insert(c, 1);
    }
    assert_eq!(sheet.outline_band(4, true), Some((1, 3)));
    sheet.outline.summary_right = false;
    assert_eq!(sheet.outline_band(0, true), Some((1, 3)));
}

// --- Data validation ------------------------------------------------------

fn dv(kind: DvKind, op: DvOperator, f1: &str, f2: &str) -> DataValidation {
    DataValidation {
        kind,
        operator: op,
        formula1: f1.into(),
        formula2: f2.into(),
        ..DataValidation::none(CellRange::new(CellRef::new(0, 0), CellRef::new(9, 0)))
    }
}

#[test]
fn whole_number_rules_reject_fractions_and_text() {
    let rule = dv(DvKind::Whole, DvOperator::Between, "1", "10");
    assert_eq!(rule.accepts("5", Some(5.0)), Some(true));
    assert_eq!(rule.accepts("5.5", Some(5.5)), Some(false));
    assert_eq!(rule.accepts("50", Some(50.0)), Some(false));
    assert_eq!(rule.accepts("abc", None), Some(false));
}

#[test]
fn text_length_measures_characters_not_bytes() {
    let rule = dv(DvKind::TextLength, DvOperator::LessThanOrEqual, "3", "");
    assert_eq!(rule.accepts("abc", None), Some(true));
    assert_eq!(rule.accepts("abcd", None), Some(false));
    // Three characters, more than three bytes.
    assert_eq!(
        rule.accepts("héllo".get(..4).unwrap_or("hé"), None),
        Some(true)
    );
}

#[test]
fn blank_follows_allow_blank() {
    let mut rule = dv(DvKind::Decimal, DvOperator::GreaterThan, "0", "");
    assert_eq!(rule.accepts("   ", None), Some(true));
    rule.allow_blank = false;
    assert_eq!(rule.accepts("   ", None), Some(false));
}

#[test]
fn a_custom_rule_is_not_judged_here() {
    // It needs the formula engine, and blocking input on a rule this layer does
    // not understand would be worse than letting it through.
    let rule = dv(DvKind::Custom, DvOperator::Between, "=A1>0", "");
    assert_eq!(rule.accepts("anything", None), None);
}

#[test]
fn an_unparseable_operand_rejects_nothing() {
    // A range-reference bound cannot be compared here; refusing every value
    // would make the cell uneditable.
    let rule = dv(DvKind::Decimal, DvOperator::GreaterThan, "$B$1", "");
    assert_eq!(rule.accepts("5", Some(5.0)), Some(true));
}

#[test]
fn between_tolerates_reversed_bounds() {
    let rule = dv(DvKind::Decimal, DvOperator::Between, "10", "1");
    assert_eq!(rule.accepts("5", Some(5.0)), Some(true));
}
