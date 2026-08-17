//! Model + snapshot tests. The empty-workbook byte-stable round-trip is the
//! Phase 0 exit-gate condition (`docs/06-ROADMAP-AND-DELIVERY.md`).

use crate::{
    Cell, CellRange, CellRef, CellValue, ChartKind, ChartView, CustomFilter, DataValidation,
    DvKind, DvOperator, FilterOp, FilterRule, Id, IdGenerator, SCHEMA_VERSION, Sheet, SheetId,
    StringId, StringTable, Workbook,
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
        r#"{"schemaVersion":1,"workbookId":"00000000000000010000000000000001"}"#
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
    // An id past the end of this table does not resolve here. The check that
    // used to live here — a namespace tag distinguishing a string from a style
    // — is now the type system's job, and cost twelve bytes in every cell
    // (docs/58).
    assert_eq!(table.get(StringId::at(99)), None);
}

#[test]
fn dangling_string_reference_is_rejected() {
    let mut wb = Workbook::new(wb_id());
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    // A shared-string id that was never interned.
    sheet.cells.set(
        CellRef::new(0, 0),
        Cell::value(CellValue::SharedString(StringId::at(99))),
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

// ---------------------------------------------------------------------------
// Chart identity (COL-02, stage 2). Every other sheet collection is identified
// by where it points — a comment by its cell, a hyperlink and a validation by
// their range, a conditional format by its OOXML priority. Charts are the
// exception: two may cover the same cells, so they carry an id.
// ---------------------------------------------------------------------------

#[test]
fn chart_ids_are_assigned_in_document_order() {
    let mut sheet = Sheet::new(SheetId(wb_id()), "S");
    for _ in 0..3 {
        sheet.charts.push(ChartView::new(
            CellRange::new(CellRef::new(0, 0), CellRef::new(4, 4)),
            ChartKind::Column,
        ));
    }
    sheet.assign_chart_ids();

    assert_eq!(
        sheet.charts.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "the same file always numbers its charts the same way"
    );
}

#[test]
fn assigning_ids_leaves_existing_ones_alone() {
    let mut sheet = Sheet::new(SheetId(wb_id()), "S");
    let range = CellRange::new(CellRef::new(0, 0), CellRef::new(4, 4));
    let mut kept = ChartView::new(range, ChartKind::Column);
    kept.id = 7;
    sheet.charts.push(kept);
    sheet.charts.push(ChartView::new(range, ChartKind::Bar));

    sheet.assign_chart_ids();

    assert_eq!(
        sheet.charts[0].id, 7,
        "an assigned id is identity, not a slot"
    );
    assert_eq!(
        sheet.charts[1].id, 8,
        "the new one clears the high-water mark"
    );
}

#[test]
fn an_id_survives_an_insertion_before_it_but_an_index_does_not() {
    // The whole point. Under concurrency two editors both name "chart 0", and
    // an insertion by either renumbers the other's target.
    let mut sheet = Sheet::new(SheetId(wb_id()), "S");
    let range = CellRange::new(CellRef::new(0, 0), CellRef::new(4, 4));
    sheet.charts.push(ChartView::new(range, ChartKind::Column));
    sheet.assign_chart_ids();
    let target = sheet.charts[0].id;

    let mut inserted = ChartView::new(range, ChartKind::Pie);
    inserted.id = sheet.next_chart_id();
    sheet.charts.insert(0, inserted);

    assert_ne!(
        sheet.charts[0].id, target,
        "index 0 is now a different chart"
    );
    assert_eq!(
        sheet.charts.iter().position(|c| c.id == target),
        Some(1),
        "the id still finds the original"
    );
}

#[test]
fn a_snapshot_without_chart_ids_round_trips_byte_identically() {
    // ADR-010: an additive field must not change the bytes of a snapshot
    // written before it existed.
    let json = r#"{"anchor":{"start":{"row":0,"col":0},"end":{"row":4,"col":4}},"kind":"column"}"#;
    let chart: ChartView = serde_json::from_str(json).expect("older snapshot still loads");

    assert_eq!(chart.id, 0, "unassigned, not defaulted to a real id");
    assert_eq!(
        serde_json::to_string(&chart).unwrap(),
        json,
        "and writing it back produces the same bytes"
    );
}

/// ADR-010 for `RetainedRel::external`, in both directions.
///
/// `RetainedRel` is `deny_unknown_fields`, so the compatibility question is not
/// whether an old reader tolerates the new field — it will not, and that is what
/// `SCHEMA_VERSION` is for — but whether the *new* reader still accepts a
/// snapshot written without it, and whether a workbook that has no external
/// relationship still serializes to the bytes it always did. `#[serde(default)]`
/// answers the first and `skip_serializing_if` the second, which is what keeps
/// `SCHEMA_VERSION` at 1.
#[test]
fn a_snapshot_without_the_external_flag_round_trips_byte_identically() {
    let json = r#"{"source":"xl/workbook.xml","id":"rId9","relType":"…/externalLink","target":"externalLinks/externalLink1.xml"}"#;
    let rel: crate::RetainedRel = serde_json::from_str(json).expect("older snapshot still loads");

    assert!(
        !rel.external,
        "absent means a part path, which is the default"
    );
    assert_eq!(
        serde_json::to_string(&rel).unwrap(),
        json,
        "and writing it back produces the same bytes"
    );

    // The flag is only written when it is true, and then it survives — an
    // external target that came back as a part path would be resolved against
    // the source part on the next save and reach nothing.
    let external = crate::RetainedRel {
        external: true,
        target: "file:///other.xlsx".into(),
        ..rel
    };
    let bytes = serde_json::to_string(&external).unwrap();
    assert!(bytes.contains(r#""external":true"#), "{bytes}");
    assert_eq!(
        serde_json::from_str::<crate::RetainedRel>(&bytes).unwrap(),
        external
    );
}

/// Retained parts survive a model snapshot, bytes intact.
///
/// Worth pinning because a design document asserted the opposite — that a
/// collaboration session restoring from a snapshot would silently lose every
/// unrecognised chart and VBA part — and built a rule on it. It does not: the
/// retention side table is serialized like everything else. The rule was
/// removed; this keeps the fact from drifting back into doubt.
#[test]
fn retained_parts_survive_a_model_snapshot() {
    let mut wb = Workbook::new(wb_id());
    wb.retained_parts.push(crate::RetainedPart {
        path: "xl/charts/chart1.xml".into(),
        bytes: b"<c:chartSpace/>".to_vec(),
        content_type: Some("application/vnd.chart+xml".into()),
    });

    let back = Workbook::from_snapshot(&wb.to_snapshot().unwrap()).unwrap();

    assert_eq!(back.retained_parts.len(), 1);
    assert_eq!(back.retained_parts[0].bytes, b"<c:chartSpace/>");
    assert_eq!(
        back.retained_parts[0].content_type.as_deref(),
        Some("application/vnd.chart+xml"),
        "the content-type override too, without which the package is invalid"
    );
}

/// A retained part costs roughly four times its size in a snapshot.
///
/// `serde_json` writes `Vec<u8>` as an array of decimal numbers — `[171,171,…]`
/// — so a megabyte of embedded image becomes about four megabytes of JSON.
/// That is fine for a golden fixture and expensive for something written every
/// two hundred revisions and read on every cold start, which is what drove
/// storing retained parts once per session rather than in each snapshot.
#[test]
fn retained_bytes_cost_about_four_times_their_size_in_a_snapshot() {
    let mut wb = Workbook::new(wb_id());
    wb.retained_parts.push(crate::RetainedPart {
        path: "xl/media/image1.png".into(),
        bytes: vec![0xAB; 1024],
        content_type: Some("image/png".into()),
    });

    let baseline = Workbook::new(wb_id()).to_snapshot().unwrap().len();
    let overhead = wb.to_snapshot().unwrap().len() - baseline;

    assert!(
        (3 * 1024..=5 * 1024).contains(&overhead),
        "expected roughly 4x for 1 KiB of retained bytes, measured {overhead}"
    );
}

/// The grid bound is one number with two names, and the two must agree.
///
/// `casual-calc-formula` has carried `MAX_ROW`/`MAX_COL` since whole-column
/// references landed — but as an *evaluator* bound, the extent `A:A` spans, and
/// nothing on the admission side ever consulted it. FID-18 is what that gap
/// costs: a file naming row 4,294,967,295 imported unbounded because the only
/// copy of the limit lived in a crate the importer's address parser did not ask.
/// Now the model states it, so if either copy is ever "fixed" alone this fails
/// rather than letting the two drift into disagreeing about what a sheet is.
#[test]
fn the_grid_bound_agrees_with_the_formula_crate() {
    assert_eq!(crate::GRID_MAX_ROW, casual_calc_formula::MAX_ROW);
    assert_eq!(crate::GRID_MAX_COL, casual_calc_formula::MAX_COL);
    // And it is the limit docs/21 publishes: 2^20 rows x 2^14 columns.
    assert_eq!(u64::from(crate::GRID_MAX_ROW) + 1, 1 << 20);
    assert_eq!(u64::from(crate::GRID_MAX_COL) + 1, 1 << 14);
}

#[test]
fn an_address_past_the_grid_is_not_in_it() {
    assert!(CellRef::new(0, 0).in_grid());
    assert!(CellRef::new(crate::GRID_MAX_ROW, crate::GRID_MAX_COL).in_grid());
    assert!(!CellRef::new(crate::GRID_MAX_ROW + 1, 0).in_grid());
    assert!(!CellRef::new(0, crate::GRID_MAX_COL + 1).in_grid());
    // The shape FID-18 arrived in: `ZZZZ4294967295`.
    assert!(!CellRef::new(4_294_967_294, 475_253).in_grid());
    assert!(
        !CellRange::new(CellRef::new(0, 0), CellRef::new(4_294_967_294, 475_253)).in_grid(),
        "one corner outside is a rectangle outside"
    );
}

// --- Snapshot admission (SEC-013, docs/21) -----------------------------------

mod snapshot_limits {
    use crate::{Id, ModelError, Sheet, SheetId, SnapshotLimits, Workbook};

    fn a_workbook() -> Workbook {
        let mut wb = Workbook::new(Id::from_parts(1, 1));
        wb.sheets
            .push(Sheet::new(SheetId(Id::from_parts(2, 1)), "Sheet1"));
        wb
    }

    /// **A snapshot larger than the ceiling is refused before it is parsed.**
    ///
    /// Before the bytes reach `serde_json`, because a limit applied after
    /// parsing has already paid for the allocation it exists to prevent — the
    /// whole point is not to build the thing.
    #[test]
    fn an_oversized_snapshot_is_refused_before_parsing() {
        let bytes = a_workbook().to_snapshot().expect("serialises");
        let tight = SnapshotLimits {
            max_bytes: (bytes.len() - 1) as u64,
            ..SnapshotLimits::default()
        };

        match Workbook::from_snapshot_with(&bytes, tight) {
            Err(ModelError::SnapshotTooLarge { what, limit, asked }) => {
                assert_eq!(what, "bytes");
                assert_eq!(limit, (bytes.len() - 1) as u64);
                assert_eq!(asked, bytes.len() as u64);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }

        // **Not merely malformed.** A well-formed snapshot that is too big must
        // not be reported as corruption, or an operator goes looking for a bad
        // file instead of a limit.
        let refused = Workbook::from_snapshot_with(&bytes, tight).unwrap_err();
        assert_eq!(refused.code(), "OC-MDL-0005");
        assert_ne!(refused.code(), "OC-MDL-0004");
    }

    /// **A snapshot inside the ceiling still loads.**
    #[test]
    fn an_ordinary_snapshot_is_unaffected() {
        let bytes = a_workbook().to_snapshot().expect("serialises");
        let loaded = Workbook::from_snapshot(&bytes).expect("a small snapshot loads");
        assert_eq!(loaded.sheets.len(), 1);
    }

    /// **The shipped ceilings are finite, and above what the engine supports.**
    #[test]
    fn the_defaults_are_bounded_and_usable() {
        let d = SnapshotLimits::default();
        assert!(d.max_bytes > 0 && d.max_bytes < u64::MAX / 2);
        assert!(d.max_populated_cells >= 1_000_000, "below the T1 target");
        // The same ceiling admission uses, so a workbook cannot enter by one
        // door at a size the other refuses.
        assert_eq!(d.max_populated_cells, 8_000_000);
    }
}
