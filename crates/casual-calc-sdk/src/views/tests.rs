use super::*;

/// **A view hides only the sheet it was applied to.**
#[test]
fn a_view_is_per_sheet() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::from([2, 4]));

    assert!(views.hides(0, 2));
    assert!(views.hides(0, 4));
    assert!(!views.hides(0, 3));
    // A different sheet is untouched, including the same row number.
    assert!(!views.hides(1, 2));
    assert!(!views.has_view(1));
}

/// **Setting a view replaces it**, rather than accumulating hidden rows.
///
/// A filter states what is shown. If the second call merged, narrowing a filter
/// would be impossible without clearing first, and rows would accrete until the
/// sheet was empty.
#[test]
fn setting_a_view_replaces_the_previous_one() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::from([1, 2, 3]));
    views.set(0, BTreeSet::from([9]));

    assert!(views.hides(0, 9));
    assert!(
        !views.hides(0, 1),
        "the previous view's rows are still hidden"
    );
    assert_eq!(views.hidden_rows(0), Some(&BTreeSet::from([9])));
}

/// **A view that hides nothing is still a view.**
///
/// The distinction is real: a filter whose predicate matches every row is not
/// the same as no filter, and the chrome that offers "clear your view" has to
/// know which it is looking at.
#[test]
fn a_view_hiding_nothing_is_not_the_same_as_no_view() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::new());

    assert!(
        views.has_view(0),
        "an empty view was indistinguishable from none"
    );
    assert!(!views.is_empty());
    views.clear(0);
    assert!(!views.has_view(0));
    assert!(views.is_empty());
}

/// **Clearing one sheet leaves the others.**
#[test]
fn clearing_is_per_sheet_and_clear_all_is_not() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::from([1]));
    views.set(2, BTreeSet::from([5]));

    views.clear(0);
    assert!(!views.has_view(0));
    assert!(
        views.has_view(2),
        "clearing one sheet took another's view with it"
    );

    views.clear_all();
    assert!(views.is_empty());
}

/// **A view follows its sheet when the sheets are renumbered.**
///
/// This is the defect that would never be reported as a filter bug. Delete
/// sheet 0 and, without this, the view keyed to index 1 goes on hiding rows —
/// but index 1 is now a different sheet. The participant sees rows vanish on a
/// sheet they never filtered, with nothing on the wire to explain it and
/// nothing in the history to undo.
#[test]
fn views_follow_their_sheets_when_sheets_move() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::from([7]));
    views.set(1, BTreeSet::from([8]));
    views.set(2, BTreeSet::from([9]));

    // Sheet 0 is deleted: 1 becomes 0, 2 becomes 1.
    views.resequence(|sheet| match sheet {
        0 => None,
        other => Some(other - 1),
    });

    assert!(!views.hides(0, 7), "the deleted sheet's view survived");
    assert_eq!(
        views.hidden_rows(0),
        Some(&BTreeSet::from([8])),
        "old sheet 1"
    );
    assert_eq!(
        views.hidden_rows(1),
        Some(&BTreeSet::from([9])),
        "old sheet 2"
    );
    assert!(!views.has_view(2));
}

/// **A reorder that swaps two sheets does not lose or duplicate a view.**
///
/// Written separately from the delete case because the naive implementation —
/// mutating the map in place — corrupts exactly here: moving 0→1 overwrites the
/// entry for 1 before it has been read.
#[test]
fn a_swap_keeps_both_views() {
    let mut views = PersonalViews::new();
    views.set(0, BTreeSet::from([1]));
    views.set(1, BTreeSet::from([2]));

    views.resequence(|sheet| Some(1 - sheet));

    assert_eq!(views.hidden_rows(0), Some(&BTreeSet::from([2])));
    assert_eq!(views.hidden_rows(1), Some(&BTreeSet::from([1])));
}
