//! Theme-linked colours, end to end.
//!
//! Lives here rather than in the importer because this crate already has the
//! importer as a dev-dependency: the round trip is the assertion, and it needs
//! both halves.

/// A theme-linked colour must survive a round trip as a *link*, not as the
/// colour it happened to resolve to.
///
/// This is the difference between a cell that re-colours when the workbook is
/// re-themed and one that stays put forever. Every piece of the chain existed
/// — the model carries `ThemeTint`, the exporter writes `theme="N"`, the
/// importer reads it back, and the editor's picker passes the slot — but
/// nothing asserted the whole path end to end, so the tracker went on recording
/// it as unfinished long after it worked.
#[test]
fn a_theme_linked_colour_survives_export_and_import_as_a_link() {
    use casual_calc_model::{
        Cell, CellRef, CellValue, Id, Sheet, SheetId, Style, ThemeTint, Workbook,
    };

    let mut workbook = Workbook::new(Id::from_parts(1, 1));
    let mut sheet = Sheet::new(SheetId(Id::from_parts(2, 1)), "S");
    let mut style = Style::default();
    // Slot 4 with a lightening tint, which is what picking from the theme row's
    // second ladder gives.
    style.set_fill_color(
        Some("8EAADB".to_owned()),
        Some(ThemeTint::from_tint(4, 0.6)),
    );
    let id = workbook.intern_style(style);
    let mut cell = Cell::value(CellValue::Number(1.0));
    cell.style = Some(id);
    sheet.cells.set(CellRef::new(0, 0), cell);
    workbook.sheets.push(sheet);

    let bytes = casual_calc_export::write_workbook(&workbook).expect("exports");
    let back = casual_calc_import::import_package(bytes).expect("imports");

    let cell = back.workbook.sheets[0]
        .cells
        .get(CellRef::new(0, 0))
        .expect("the cell survived");
    let style = cell
        .style
        .and_then(|id| back.workbook.styles.get(id))
        .expect("the style survived");
    let theme = style
        .fill_theme
        .expect("the fill is still linked to the theme, not flattened to RGB");
    assert_eq!(theme.slot, 4, "and to the slot it was picked from");
    assert!(
        theme.tint_micro > 0,
        "with its tint, which is part of the reference rather than a way out of it"
    );
}
