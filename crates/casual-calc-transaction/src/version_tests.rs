//! Version history: what a version costs, what bounds the set, and what a
//! restore is.
//!
//! The property the collaborative test at the bottom exists for is the one that
//! distinguishes this design from the one ADR-011 forbids: **revision numbers
//! only ever increase**. A rewind would move them, and everything defined
//! against that numbering — every resume key, every client's base, and
//! `oldest_rebasable` — would be wrong on every other participant at once, and
//! silently.

use casual_calc_formula::stored::Origin;
use casual_calc_model::{
    Cell, CellRange, CellRef, CellValue, Id, RunFont, Sheet, SheetId, Style, TextRun, Workbook,
};

use crate::{
    Operation, restore,
    session::{ClientId, ClientSession, Commit, ServerSession},
    version::{RetentionPolicy, VersionError, VersionKind, VersionSnapshot, VersionStore},
};

fn book(namespace: u64) -> Workbook {
    let mut workbook = Workbook::new(Id::from_parts(namespace, 1));
    workbook
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(namespace, 2)), "Sheet1"));
    workbook
}

fn number(workbook: &mut Workbook, row: u32, col: u32, value: f64) {
    workbook.sheets[0].cells.set(
        CellRef::new(row, col),
        Cell::value(CellValue::Number(value)),
    );
}

fn text(workbook: &mut Workbook, row: u32, col: u32, value: &str) {
    let id = workbook.intern_string(value);
    workbook.sheets[0].cells.set(
        CellRef::new(row, col),
        Cell::value(CellValue::SharedString(id)),
    );
}

/// What two workbooks must agree on for a restore to have worked.
///
/// The formula's **stored tree**, never its handle: two workbooks intern into
/// their own arenas, so identical formulas legitimately carry different handle
/// numbers, and comparing the numbers would call a correct restore a failure.
/// Strings likewise resolve to their text and their runs.
fn observe(workbook: &Workbook) -> String {
    let mut out = Vec::new();
    for (index, sheet) in workbook.sheets.iter().enumerate() {
        let mut cells: Vec<String> = sheet
            .cells
            .iter()
            .map(|(at, cell)| {
                let value = match cell.value {
                    CellValue::SharedString(id) | CellValue::InlineString(id) => format!(
                        "{:?}/{:?}",
                        workbook.strings.get(id),
                        workbook.strings.runs(id)
                    ),
                    ref other => format!("{other:?}"),
                };
                let formula = cell
                    .formula
                    .and_then(|handle| workbook.formula(handle))
                    .map_or_else(String::new, |expr| format!("={expr:?}"));
                let style = cell
                    .style
                    .and_then(|id| workbook.styles.get(id))
                    .map_or_else(String::new, |style| format!("#{style:?}"));
                format!("{}:{}={value}{formula}{style}", at.row, at.col)
            })
            .collect();
        cells.sort();
        out.push(format!(
            "[{index} {} {:?} merges{:?}] {}",
            sheet.name,
            sheet.tab_color,
            sheet.merges,
            cells.join(",")
        ));
    }
    out.join(" | ")
}

/// Apply a restore plan the way a session does, and hand back the report.
fn restore_onto(live: &mut Workbook, snapshot: &Workbook) -> restore::RestoreReport {
    let report = restore::plan(live, snapshot);
    crate::apply(live, report.op.clone()).expect("the restore applies");
    report
}

// ---------------------------------------------------------------------------
// The ring: what bounds the set, and what is discarded first
// ---------------------------------------------------------------------------

#[test]
fn the_ring_takes_the_oldest_autosave_first() {
    let mut store = VersionStore::with_policy(RetentionPolicy {
        max_autosave: 3,
        max_bytes: 50 << 20,
    });
    let mut workbook = book(1);
    let mut ids = Vec::new();
    for step in 0..5 {
        number(&mut workbook, 0, 0, f64::from(step));
        let captured = store
            .capture(
                &workbook,
                VersionKind::Autosave,
                None,
                1_000 + i64::from(step),
                0,
            )
            .expect("captures");
        assert!(captured.stored, "step {step} changed the document");
        ids.push(captured.id);
    }
    assert_eq!(store.len(), 3, "the ring holds its ceiling and no more");
    let kept: Vec<_> = store.versions().map(|version| version.id).collect();
    assert_eq!(
        kept,
        ids[2..],
        "the three newest survived, oldest first out"
    );
}

#[test]
fn a_named_version_is_never_evicted_by_the_ring() {
    let mut store = VersionStore::with_policy(RetentionPolicy {
        max_autosave: 2,
        max_bytes: 50 << 20,
    });
    let mut workbook = book(1);

    number(&mut workbook, 0, 0, 1.0);
    let named = store
        .capture(
            &workbook,
            VersionKind::Autosave,
            Some("before the rewrite".to_owned()),
            1_000,
            0,
        )
        .expect("captures")
        .id;
    for step in 0..6 {
        number(&mut workbook, 0, 0, f64::from(step + 10));
        store
            .capture(&workbook, VersionKind::Autosave, None, 2_000, 0)
            .expect("captures");
    }

    let kept: Vec<_> = store.versions().map(|version| version.id).collect();
    assert!(
        kept.contains(&named),
        "six autosaves went by and the named version is still here: {kept:?}"
    );
    assert_eq!(
        store.get(named).expect("still stored").version.kind,
        VersionKind::Named,
    );
    assert_eq!(kept.len(), 3, "one named plus the two-autosave ceiling");
}

#[test]
fn naming_a_version_takes_it_out_of_the_ring() {
    let mut store = VersionStore::with_policy(RetentionPolicy {
        max_autosave: 1,
        max_bytes: 50 << 20,
    });
    let mut workbook = book(1);
    number(&mut workbook, 0, 0, 1.0);
    let first = store
        .capture(&workbook, VersionKind::Autosave, None, 1_000, 0)
        .expect("captures")
        .id;
    assert!(store.name(first, "keep me"));

    number(&mut workbook, 0, 0, 2.0);
    store
        .capture(&workbook, VersionKind::Autosave, None, 2_000, 0)
        .expect("captures");

    assert!(
        store.get(first).is_some(),
        "naming it after the fact is what makes the ring leave it alone"
    );
}

#[test]
fn a_capture_that_cannot_fit_is_refused_and_evicts_nothing() {
    let mut workbook = book(1);
    for row in 0..40u32 {
        text(
            &mut workbook,
            row,
            0,
            "a string long enough to take up room",
        );
    }

    // Two versions the ring may not evict, and then a budget with no room left.
    let mut store = VersionStore::new();
    number(&mut workbook, 0, 5, 1.0);
    let first = store
        .capture(&workbook, VersionKind::Saved, None, 1_000, 0)
        .expect("first fits")
        .id;
    number(&mut workbook, 0, 5, 2.0);
    let second = store
        .capture(&workbook, VersionKind::Saved, None, 2_000, 0)
        .expect("second fits")
        .id;
    store.set_policy(RetentionPolicy {
        max_autosave: 20,
        max_bytes: store.total_bytes(),
    });

    number(&mut workbook, 0, 5, 3.0);
    let refused = store.capture(&workbook, VersionKind::Autosave, None, 3_000, 0);
    assert!(
        matches!(refused, Err(VersionError::Full { .. })),
        "refused loudly rather than dropping somebody's saved version: {refused:?}"
    );
    let kept: Vec<_> = store.versions().map(|version| version.id).collect();
    assert_eq!(
        kept,
        vec![first, second],
        "the failing capture destroyed nothing on its way to failing"
    );
}

#[test]
fn a_refused_capture_does_not_first_destroy_the_autosaves_it_could_have_evicted() {
    // The case above has nothing evictable, so it passes whether or not
    // feasibility is decided in advance. This one has two autosaves the ring
    // *may* take and a budget that still would not fit the new version — the
    // shape where "evict, then discover it was not enough" destroys work for
    // nothing. An eviction is justified only by the version that replaces it.
    let mut workbook = book(1);
    for row in 0..40u32 {
        text(
            &mut workbook,
            row,
            0,
            "a string long enough to take up room",
        );
    }

    let mut store = VersionStore::new();
    number(&mut workbook, 0, 5, 1.0);
    store
        .capture(
            &workbook,
            VersionKind::Named,
            Some("the one that matters".to_owned()),
            1_000,
            0,
        )
        .expect("the named version fits");
    let mut autosaves = Vec::new();
    for step in 0..2 {
        number(&mut workbook, 0, 5, f64::from(step + 2));
        autosaves.push(
            store
                .capture(&workbook, VersionKind::Autosave, None, 2_000, 0)
                .expect("the autosaves fit")
                .id,
        );
    }

    // A budget with room for the named version and almost nothing else.
    let named_bytes = store
        .versions()
        .find(|version| version.kind == VersionKind::Named)
        .expect("it is here")
        .byte_len as u64;
    store.set_policy(RetentionPolicy {
        max_autosave: 20,
        max_bytes: named_bytes + 8,
    });

    number(&mut workbook, 0, 5, 99.0);
    let refused = store.capture(&workbook, VersionKind::Autosave, None, 3_000, 0);
    assert!(
        matches!(refused, Err(VersionError::Full { .. })),
        "there was never room, so it had to be refused: {refused:?}"
    );
    let kept: Vec<_> = store.versions().map(|version| version.id).collect();
    for id in autosaves {
        assert!(
            kept.contains(&id),
            "autosave {id} was evicted for a capture that then failed: {kept:?}"
        );
    }
}

#[test]
fn one_snapshot_larger_than_the_whole_budget_is_refused_before_anything_moves() {
    let mut store = VersionStore::with_policy(RetentionPolicy {
        max_autosave: 20,
        max_bytes: 8,
    });
    let mut workbook = book(1);
    number(&mut workbook, 0, 0, 1.0);
    let refused = store.capture(&workbook, VersionKind::Named, Some("big".to_owned()), 1, 0);
    assert!(
        matches!(refused, Err(VersionError::TooLarge { .. })),
        "a ceiling that yields under pressure is not one: {refused:?}"
    );
    assert!(store.is_empty());
}

#[test]
fn an_unchanged_document_does_not_add_a_version() {
    let mut store = VersionStore::new();
    let mut workbook = book(1);
    number(&mut workbook, 0, 0, 1.0);

    let first = store
        .capture(&workbook, VersionKind::Autosave, None, 1_000, 0)
        .expect("captures");
    let again = store
        .capture(&workbook, VersionKind::Autosave, None, 2_000, 0)
        .expect("captures");

    assert!(first.stored);
    assert!(
        !again.stored,
        "an autosave on a quiet document must not push the versions that differ out of the ring"
    );
    assert_eq!(again.id, first.id);
    assert_eq!(store.len(), 1);

    // A *name* is an intention rather than an observation, so it always stores.
    let named = store
        .capture(
            &workbook,
            VersionKind::Autosave,
            Some("the same, on purpose".to_owned()),
            3_000,
            0,
        )
        .expect("captures");
    assert!(named.stored);
    assert_eq!(store.len(), 2);
}

#[test]
fn a_store_that_comes_back_out_of_order_still_numbers_monotonically() {
    let mut store = VersionStore::new();
    let mut workbook = book(1);
    for step in 0..3 {
        number(&mut workbook, 0, 0, f64::from(step));
        store
            .capture(&workbook, VersionKind::Saved, None, 1_000, 0)
            .expect("captures");
    }
    let mut parts: Vec<VersionSnapshot> = store.into_parts();
    parts.reverse();
    let highest = parts
        .iter()
        .map(|entry| entry.version.id)
        .max()
        .expect("three");

    let mut reloaded = VersionStore::from_parts(RetentionPolicy::default(), parts);
    let listed: Vec<_> = reloaded.versions().map(|version| version.id).collect();
    assert!(
        listed.windows(2).all(|pair| pair[0] < pair[1]),
        "a cursor's order is not the store's order: {listed:?}"
    );
    number(&mut workbook, 0, 0, 99.0);
    let next = reloaded
        .capture(&workbook, VersionKind::Saved, None, 9_000, 0)
        .expect("captures")
        .id;
    assert!(
        next > highest,
        "a version from storage and one captured after it cannot collide"
    );
}

#[test]
fn hiding_a_version_keeps_its_bytes() {
    let mut store = VersionStore::new();
    let mut workbook = book(1);
    number(&mut workbook, 0, 0, 1.0);
    let id = store
        .capture(&workbook, VersionKind::Saved, None, 1_000, 0)
        .expect("captures")
        .id;

    assert!(store.hide(id));
    assert_eq!(store.visible().count(), 0, "gone from the list");
    assert!(
        store.get(id).is_some(),
        "and still here: deleting somebody else's copy is a promise this cannot keep"
    );
    assert!(store.unhide(id));
    assert_eq!(store.visible().count(), 1);
}

// ---------------------------------------------------------------------------
// The restore: a diff across two identifier spaces
// ---------------------------------------------------------------------------

#[test]
fn a_restore_puts_back_what_changed_and_clears_what_arrived() {
    let mut past = book(1);
    number(&mut past, 0, 0, 1.0);
    number(&mut past, 1, 0, 2.0);

    let mut live = past.clone();
    number(&mut live, 0, 0, 999.0);
    number(&mut live, 5, 5, 42.0);

    let report = restore_onto(&mut live, &past);
    assert_eq!(observe(&live), observe(&past));
    assert_eq!(report.cells_changed, 2, "one rewritten, one cleared");
    assert!(
        live.sheets[0].cells.get(CellRef::new(5, 5)).is_none(),
        "a cell the snapshot never had is cleared, not left standing"
    );
}

#[test]
fn a_restore_does_not_rewrite_cells_that_already_match() {
    // The cost property. The two workbooks are built independently, so their
    // string and style tables number everything differently; a diff that
    // compared identifiers rather than meanings would rewrite every populated
    // cell, and the undo entry for that is a second copy of the document.
    let mut past = book(1);
    let mut live = book(1);
    // The same sheet — a restore matches sheets by identity — and two string
    // tables that number the same text differently, because the live one
    // interned something else first.
    live.sheets[0].id = past.sheets[0].id;
    let _ = live.intern_string("interned here and nowhere else");
    for row in 0..50u32 {
        text(&mut past, row, 0, "shared");
        text(&mut live, row, 0, "shared");
    }
    assert_ne!(
        past.strings.get(casual_calc_model::StringId::at(0)),
        live.strings.get(casual_calc_model::StringId::at(0)),
        "the two tables really do number their strings differently"
    );
    text(&mut live, 7, 0, "changed");

    let report = restore::plan(&mut live, &past);
    assert_eq!(
        report.cells_changed, 1,
        "exactly the one cell that differs: {:?}",
        report.op
    );
}

#[test]
fn a_restore_carries_a_formula_across_two_identifier_spaces() {
    // The failure this guards is silent: a foreign `FormulaHandle` applied
    // here indexes *our* arena, and the writer that finds it dangling drops the
    // whole cell rather than the formula. The snapshot interns other formulas
    // first, so its handle is not one the live workbook would allocate.
    let mut past = book(1);
    past.store_formula_at(
        casual_calc_formula::parse("1+1").expect("parses"),
        Origin::at(0, 0),
    );
    past.store_formula_at(
        casual_calc_formula::parse("2+2").expect("parses"),
        Origin::at(0, 0),
    );
    let handle = past.store_formula_at(
        casual_calc_formula::parse("A1*3").expect("parses"),
        Origin::at(4, 1),
    );
    past.sheets[0].cells.set(
        CellRef::new(4, 1),
        Cell {
            value: CellValue::Number(0.0),
            formula: Some(handle),
            ..Cell::default()
        },
    );

    let mut live = book(1);
    live.sheets[0].id = past.sheets[0].id;
    number(&mut live, 4, 1, 7.0);

    restore_onto(&mut live, &past);

    let cell = live.sheets[0]
        .cells
        .get(CellRef::new(4, 1))
        .expect("the cell survived")
        .clone();
    let restored = cell
        .formula
        .and_then(|handle| live.formula(handle))
        .expect("the formula came across, and resolves here");
    assert_eq!(
        format!("{restored:?}"),
        format!("{:?}", past.formula(handle).expect("in the snapshot")),
        "the stored tree is relative to the cell, and the cell did not move"
    );
    assert_eq!(observe(&live), observe(&past));
}

#[test]
fn a_restore_keeps_rich_text_runs() {
    // `wire::WireOperation` carries a string as a `String` and re-interns it
    // with `intern_string`, which drops the runs. A restore is not on that
    // path and must not acquire the same flattening by imitation.
    let mut past = book(1);
    let id = past.intern_rich_text(vec![
        TextRun {
            text: "bold".to_owned(),
            font: Some(RunFont {
                bold: true,
                ..RunFont::default()
            }),
        },
        TextRun {
            text: " plain".to_owned(),
            font: None,
        },
    ]);
    past.sheets[0]
        .cells
        .set(CellRef::new(0, 0), Cell::value(CellValue::SharedString(id)));

    let mut live = book(1);
    live.sheets[0].id = past.sheets[0].id;
    text(&mut live, 0, 0, "flat");

    restore_onto(&mut live, &past);

    let cell = live.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .expect("the cell is here");
    let CellValue::SharedString(restored) = cell.value else {
        panic!("expected a shared string, got {:?}", cell.value);
    };
    assert_eq!(live.strings.get(restored), Some("bold plain"));
    assert_eq!(
        live.strings.runs(restored).map(<[TextRun]>::len),
        Some(2),
        "the formatting came back with the text, not only the characters"
    );
}

#[test]
fn a_restore_puts_back_a_deleted_sheet_and_removes_an_added_one() {
    let mut past = book(1);
    number(&mut past, 0, 0, 1.0);
    let gone = SheetId(Id::from_parts(1, 7));
    let mut second = Sheet::new(gone, "Gone");
    second
        .cells
        .set(CellRef::new(2, 2), Cell::value(CellValue::Number(5.0)));
    past.sheets.push(second);

    let mut live = past.clone();
    live.sheets.remove(1);
    live.sheets
        .push(Sheet::new(SheetId(Id::from_parts(1, 8)), "Added later"));

    let report = restore_onto(&mut live, &past);
    assert_eq!(report.sheets_added, 1);
    assert_eq!(report.sheets_removed, 1);
    assert_eq!(observe(&live), observe(&past));
}

#[test]
fn a_restore_puts_the_tabs_back_in_order() {
    let mut past = book(1);
    for n in 1..4u64 {
        past.sheets.push(Sheet::new(
            SheetId(Id::from_parts(1, 10 + n)),
            format!("S{n}"),
        ));
    }
    let mut live = past.clone();
    live.sheets.reverse();
    live.sheets[0].name = "renamed".to_owned();

    restore_onto(&mut live, &past);
    let order: Vec<&str> = live.sheets.iter().map(|s| s.name.as_str()).collect();
    let want: Vec<&str> = past.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        order, want,
        "every index an operation carried was the index it ran at"
    );
}

#[test]
fn a_restore_puts_back_sheet_metadata_and_a_tab_colour() {
    let mut past = book(1);
    past.sheets[0]
        .merges
        .push(CellRange::new(CellRef::new(0, 0), CellRef::new(0, 3)));
    past.sheets[0].tab_color = Some("FF0000".to_owned());
    past.sheets[0].hidden_rows.insert(4);

    let mut live = past.clone();
    live.sheets[0].merges.clear();
    live.sheets[0].tab_color = None;
    live.sheets[0].hidden_rows.clear();

    restore_onto(&mut live, &past);
    assert_eq!(observe(&live), observe(&past));
    assert_eq!(live.sheets[0].hidden_rows, past.sheets[0].hidden_rows);
}

#[test]
fn a_restore_counts_what_the_operation_set_cannot_carry() {
    // No silent data loss: the operation set is narrower than the model, and
    // the difference is reported rather than left for a user to discover by
    // noticing it later.
    let mut past = book(1);
    past.sheets[0].outline.summary_below = !past.sheets[0].outline.summary_below;
    past.properties.title = "the title it had".to_owned();
    past.theme_colors = vec!["112233".to_owned()];

    let mut live = book(1);
    live.sheets[0].id = past.sheets[0].id;

    let report = restore::plan(&mut live, &past);
    let named: Vec<&str> = report.unexpressed.iter().map(|entry| entry.field).collect();
    assert!(named.contains(&"outline"), "{named:?}");
    assert!(named.contains(&"properties"), "{named:?}");
    assert!(named.contains(&"theme_colors"), "{named:?}");
    assert_eq!(
        report.unexpressed[0].sheet,
        Some(0),
        "a sheet field says which sheet"
    );
}

#[test]
fn a_restored_sheet_says_it_could_not_bring_its_charts_bytes_back() {
    // `InsertSheet` carries a `Sheet`, and a `Sheet` carries a chart *list*.
    // The chart's XML lives in a retained part at workbook level, and no
    // operation puts one of those back alongside an inserted sheet. A restore
    // that re-added the sheet and quietly left the bytes behind would write a
    // package Excel refuses to open, so it has to say so.
    let mut past = book(1);
    let gone = SheetId(Id::from_parts(1, 7));
    let mut charted = Sheet::new(gone, "Charted");
    let mut chart = casual_calc_model::ChartView::new(
        CellRange::new(CellRef::new(0, 0), CellRef::new(9, 4)),
        casual_calc_model::ChartKind::Column,
    );
    chart.part = Some("xl/charts/chart1.xml".to_owned());
    charted.charts.push(chart);
    past.sheets.push(charted);
    past.retained_parts.push(casual_calc_model::RetainedPart {
        path: "xl/charts/chart1.xml".to_owned(),
        bytes: b"<chartSpace/>".to_vec(),
        content_type: None,
    });

    let mut live = past.clone();
    live.sheets.remove(1);
    live.retained_parts.clear();

    let report = restore::plan(&mut live, &past);
    assert_eq!(report.sheets_added, 1);
    let named: Vec<&str> = report.unexpressed.iter().map(|entry| entry.field).collect();
    assert!(
        named.contains(&"retained_parts"),
        "the bytes the chart is drawn from did not come back, and nothing said so: {named:?}"
    );
}

/// Adding a field to [`Sheet`] must break this test to compile.
///
/// Every field is in exactly one of three buckets, and the point of writing
/// them out is that a field added to the model and forgotten here is a field a
/// restore drops **in silence** — the failure mode this project does not
/// accept. A destructuring with no `..` is the only pin that cannot be
/// forgotten, because it fails at the compiler rather than at a reviewer.
#[test]
fn every_sheet_field_is_carried_by_an_operation_or_counted_as_unexpressed() {
    let sheet = Sheet::new(SheetId(Id::from_parts(9, 1)), "S");
    let Sheet {
        // Matched on, never restored: identity is what pairs the two sheets up.
        id: _,
        // Carried by `RenameSheet`.
        name: _,
        // Carried by `SetCell`, one per differing address.
        cells: _,
        // Carried by `SetTabColor`.
        tab_color: _,
        // Carried by `SetSheetMetadata` — the twenty-three fields of
        // `SheetMetadata`, which its own macro keeps in step with `capture`,
        // `diff` and `install_masked`.
        merges: _,
        view: _,
        columns: _,
        rows: _,
        hidden_rows: _,
        hidden_cols: _,
        row_outline_levels: _,
        col_outline_levels: _,
        collapsed_rows: _,
        collapsed_cols: _,
        validations: _,
        conditional_formats: _,
        comments: _,
        hyperlinks: _,
        print: _,
        charts: _,
        sort_state: _,
        tables: _,
        pivots: _,
        auto_filter: _,
        protection: _,
        visibility: _,
        filter_hidden: _,
        // Counted as unexpressed: no operation carries these, and
        // `restore::plan` names each one it finds a difference in.
        outline: _,
        images: _,
        format_pr: _,
        carried: _,
        retained_refs: _,
    } = sheet;
}

// ---------------------------------------------------------------------------
// docs/83 §8: the collaborative acceptance test
// ---------------------------------------------------------------------------

/// Two clients, one server, and a restore in the middle of the session.
///
/// Asserts both halves of the property, because either alone is satisfied by a
/// design ADR-011 rules out: the replicas converge on the restored content
/// **and** the revision number only ever increased. A rewind converges too —
/// and moves the numbering out from under every resume key in the deployment.
#[test]
fn a_restore_converges_two_clients_and_revision_numbers_only_increase() {
    let mut base = book(1);
    for row in 0..6u32 {
        number(&mut base, row, 0, f64::from(row));
    }
    text(&mut base, 0, 1, "as it was");
    // A formula too, so the restore has to carry a tree across two arenas and
    // not merely a number: a handle that means something else on the peer is
    // the failure this whole exchange is watched for.
    let handle = base.store_formula_at(
        casual_calc_formula::parse("SUM(A1:A6)").expect("parses"),
        Origin::at(7, 0),
    );
    base.sheets[0].cells.set(
        CellRef::new(7, 0),
        Cell {
            value: CellValue::Number(15.0),
            formula: Some(handle),
            ..Cell::default()
        },
    );

    let mut server = ServerSession::new();
    let mut authoritative = base.clone();
    let mut alice_book = base.clone();
    let mut bob_book = base.clone();
    let mut alice = ClientSession::new(ClientId(1), 0);
    let mut bob = ClientSession::new(ClientId(2), 0);

    let mut store = VersionStore::new();
    let version = store
        .capture(
            &base,
            VersionKind::Named,
            Some("as it was".to_owned()),
            1_000,
            0,
        )
        .expect("captures")
        .id;

    let mut revisions = vec![server.revision()];
    let settle = |server: &mut ServerSession,
                  authoritative: &mut Workbook,
                  from: &mut ClientSession,
                  from_book: &Workbook,
                  other: &mut ClientSession,
                  other_book: &mut Workbook,
                  revisions: &mut Vec<u64>| {
        while let Some(submission) = from.flush(from_book) {
            let outcome = server
                .commit(authoritative, &submission)
                .expect("the server commits");
            let (ops, revision) = match outcome {
                Commit::Applied { ops, revision } => (ops, revision),
                Commit::Duplicate { revision } => (Vec::new(), revision),
            };
            revisions.push(revision);
            from.acknowledge(submission.seq, revision);
            for op in &ops {
                other
                    .receive(other_book, op, revision)
                    .expect("the peer applies");
            }
        }
    };

    // Ordinary editing, from both sides — including replacing the formula, so
    // the restore has to put one back rather than leave one alone.
    alice
        .edit(
            &mut alice_book,
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(7, 0),
                cell: Some(Cell::value(CellValue::Number(0.0))),
            },
        )
        .expect("local edit");
    alice
        .edit(
            &mut alice_book,
            Operation::SetCell {
                sheet: 0,
                at: CellRef::new(2, 0),
                cell: Some(Cell::value(CellValue::Number(222.0))),
            },
        )
        .expect("local edit");
    settle(
        &mut server,
        &mut authoritative,
        &mut alice,
        &alice_book.clone(),
        &mut bob,
        &mut bob_book,
        &mut revisions,
    );
    bob.edit(
        &mut bob_book,
        Operation::SetCell {
            sheet: 0,
            at: CellRef::new(4, 0),
            cell: Some(Cell::value(CellValue::Number(444.0))),
        },
    )
    .expect("local edit");
    settle(
        &mut server,
        &mut authoritative,
        &mut bob,
        &bob_book.clone(),
        &mut alice,
        &mut alice_book,
        &mut revisions,
    );
    assert_eq!(
        observe(&alice_book),
        observe(&bob_book),
        "before the restore"
    );

    // Alice restores. It is an ordinary edit on her replica and an ordinary
    // submission on the wire.
    let snapshot = store
        .get(version)
        .expect("still stored")
        .workbook()
        .expect("the bytes are a workbook");
    let report = restore::plan(&mut alice_book, &snapshot);
    assert!(!report.is_empty(), "there is something to restore");
    alice
        .edit(&mut alice_book, report.op)
        .expect("the restore applies locally");
    settle(
        &mut server,
        &mut authoritative,
        &mut alice,
        &alice_book.clone(),
        &mut bob,
        &mut bob_book,
        &mut revisions,
    );

    assert_eq!(
        observe(&alice_book),
        observe(&bob_book),
        "both replicas converge on the restored content"
    );
    assert_eq!(
        observe(&alice_book),
        observe(&authoritative),
        "and on what the server holds"
    );
    assert_eq!(
        observe(&alice_book),
        observe(&snapshot),
        "which is the snapshot's content"
    );
    assert!(
        revisions.windows(2).all(|pair| pair[0] <= pair[1]),
        "revision numbers only ever increased: {revisions:?}"
    );
    assert!(
        revisions.last() > revisions.first(),
        "and the restore moved them forward rather than back: {revisions:?}"
    );
}

// ---------------------------------------------------------------------------
// What a version costs
// ---------------------------------------------------------------------------

/// What a snapshot costs, and therefore how many of them a budget holds.
///
/// Ignored by default because it builds a workbook of a size a unit test has no
/// business building on every run. Run it deliberately:
///
/// ```text
/// cargo test -p casual-calc-transaction --release measure_snapshot_cost \
///     -- --ignored --nocapture
/// ```
///
/// The numbers it prints are what [`RetentionPolicy`]'s defaults were chosen
/// against, and they are the reason the byte ceiling rather than the count
/// ceiling is the one that binds on a real workbook.
#[test]
#[ignore = "a cost measurement, not a gate: run it with --ignored --nocapture"]
fn measure_snapshot_cost() {
    for cells in [10_000usize, 100_000, 300_000] {
        let cols = 20u32;
        let rows = cells as u32 / cols;
        let mut workbook = book(1);
        let style = workbook.intern_style(Style {
            bold: true,
            ..Style::default()
        });
        for row in 0..rows {
            for col in 0..cols {
                let value = if col % 4 == 0 {
                    let id = workbook.intern_string(&format!("label {}", row % 500));
                    CellValue::SharedString(id)
                } else {
                    CellValue::Number(f64::from(row * cols + col) * 1.5)
                };
                workbook.sheets[0].cells.set(
                    CellRef::new(row, col),
                    Cell {
                        value,
                        style: (col % 3 == 0).then_some(style),
                        ..Cell::default()
                    },
                );
            }
        }

        let start = std::time::Instant::now();
        let bytes = workbook.to_snapshot().expect("serializes");
        let capture_ms = start.elapsed().as_secs_f64() * 1e3;

        let start = std::time::Instant::now();
        let read_back = Workbook::from_snapshot(&bytes).expect("reads back");
        let parse_ms = start.elapsed().as_secs_f64() * 1e3;

        let mut live = read_back.clone();
        live.sheets[0].cells.set(
            CellRef::new(0, 0),
            Cell::value(CellValue::Number(123_456.0)),
        );
        let start = std::time::Instant::now();
        let report = restore::plan(&mut live, &read_back);
        let plan_ms = start.elapsed().as_secs_f64() * 1e3;

        // What a restore weighs **on the wire**, which is a different ceiling
        // from what it weighs in the store: the collaboration server caps a
        // WebSocket message at 4 MiB by default
        // (`server/…/net.rs`, `Limits::max_message_bytes`), and a restore is
        // one `Batch` in one submission — `ClientSession::flush` does not split
        // by size. So the number that matters is bytes per changed cell.
        let mut wide = read_back.clone();
        for row in 0..rows.min(500) {
            wide.sheets[0]
                .cells
                .set(CellRef::new(row, 1), Cell::value(CellValue::Number(-1.0)));
        }
        let wide_report = restore::plan(&mut wide, &read_back);
        let wire = crate::wire::WireOperation::of(wide_report.op, &wide);
        let wire_bytes = serde_json::to_vec(&wire).expect("serializes").len();
        let per_cell = wire_bytes as f64 / wide_report.cells_changed.max(1) as f64;

        let policy = RetentionPolicy::default();
        let fits = policy.max_bytes / bytes.len().max(1) as u64;
        println!(
            "{cells:>7} cells: snapshot {:>8.2} MiB  capture {capture_ms:>7.1} ms  \
             parse {parse_ms:>7.1} ms  one-cell plan {plan_ms:>7.1} ms ({} ops)  \
             versions in {} MiB: {fits}  wire {per_cell:.0} B/cell \
             (4 MiB frame holds {} changed cells)",
            bytes.len() as f64 / (1 << 20) as f64,
            match &report.op {
                Operation::Batch(ops) => ops.len(),
                _ => 1,
            },
            policy.max_bytes >> 20,
            (4.0 * (1 << 20) as f64 / per_cell) as u64,
        );
    }
}
