//! The discovery document is parsed by other people's software, so these
//! assert on the bytes rather than on the structure that produced them.

use super::*;

fn brand(name: &str) -> Brand {
    Brand {
        name: name.to_owned(),
        ..Brand::default()
    }
}

/// **A host can find an edit action for a spreadsheet, and the URL it builds
/// from it is well formed.**
///
/// The host appends `WOPISrc=…&access_token=…` to `urlsrc` verbatim. Without
/// the trailing `?` the first parameter fuses onto the path and the action URL
/// 404s — the failure looks like a routing bug and is a string bug.
#[test]
fn an_edit_action_is_advertised_and_takes_parameters() {
    let xml = document("https://calc.example/", &brand("OpenCalc"), None);

    assert!(
        xml.contains(r#"<action name="edit" ext="xlsx" default="true""#),
        "{xml}"
    );
    assert!(
        xml.contains(r#"urlsrc="https://calc.example/wopi/edit?""#),
        "{xml}"
    );
    assert!(
        xml.contains(r#"urlsrc="https://calc.example/wopi/view?""#),
        "{xml}"
    );
    // The trailing slash of the public URL must not double.
    assert!(!xml.contains("//wopi/"), "{xml}");
}

/// **The net zone follows the scheme the browser will actually use.**
///
/// An `http` action advertised to an `https` host is a mixed-content block in
/// every current browser, which presents as the editor simply never appearing.
#[test]
fn the_net_zone_matches_the_scheme() {
    assert!(document("https://calc.example", &Brand::default(), None).contains("external-https"));
    assert!(document("http://localhost:8090", &Brand::default(), None).contains("external-http"));
}

/// **The brand name reaches the document, and cannot break out of it.**
///
/// It is operator input going straight into an attribute. A `"` closes the
/// attribute and a `<` opens an element, so an unescaped name does not merely
/// look wrong — it produces XML the host cannot parse, and the editor vanishes
/// from the administrator's list with no error anywhere.
#[test]
fn a_brand_name_is_escaped_into_the_markup() {
    let xml = document(
        "https://calc.example",
        &brand("Ada & Co \"Sheets\" <beta>"),
        None,
    );

    assert!(
        xml.contains(r#"<app name="Ada &amp; Co &quot;Sheets&quot; &lt;beta&gt;""#),
        "{xml}"
    );
    // Nothing survived that could still be read as markup: exactly one `<app`,
    // and no stray `<beta>` element.
    assert_eq!(xml.matches("<app ").count(), 1, "{xml}");
    assert!(!xml.contains("<beta>"), "{xml}");
}

/// **A favicon is advertised only when one is configured.**
///
/// An empty `favIconUrl=""` makes a host request the editor's own origin root
/// and cache the 404 as the icon.
#[test]
fn an_unset_favicon_is_absent_rather_than_empty() {
    assert!(!document("https://c.example", &Brand::default(), None).contains("favIconUrl"));

    let with = Brand {
        favicon: "https://cdn.example/logo.png".to_owned(),
        ..Brand::default()
    };
    assert!(
        document("https://c.example", &with, None)
            .contains(r#"favIconUrl="https://cdn.example/logo.png""#)
    );
}

/// **Nothing is advertised that the save leg would rewrite into another
/// format** (`WOPI-05`).
///
/// The list used to be `xlsx` alone, because a save emitted a package whatever
/// it opened: a host that handed us `.ods` got that package back under the same
/// name and had lost the original, silently, with an administrator's blessing.
///
/// Now the save leg converts, so the list is wider — and the guard has to
/// change shape with it, because a longer literal list of formats to *forbid*
/// stops meaning anything the moment somebody adds a format. It is asserted
/// against the engine's own table instead: an extension may be advertised only
/// if [`casual_calc_sdk::SessionFormat::for_extension`] recognises it, which is
/// the same table `save_as` writes from. Adding `.ods` here cannot pass this
/// until the engine can *write* one.
#[test]
fn only_formats_that_round_trip_are_advertised() {
    use casual_calc_sdk::SessionFormat;

    for ext in EDITABLE {
        let format = SessionFormat::for_extension(ext).unwrap_or_else(|| {
            panic!("`{ext}` is advertised, and a save would rewrite it as something else")
        });
        assert_eq!(
            format.extension(),
            *ext,
            "`{ext}` opens as a format that writes `.{}` back — a host would get \
             one kind of file under another kind's name",
            format.extension()
        );
    }

    // And the ones that must stay out, named so the reason survives: this
    // engine reads them and cannot write them.
    let xml = document("https://c.example", &Brand::default(), None);
    for unwritable in ["ods", "fods", "xls"] {
        assert!(
            !xml.contains(&format!(r#"ext="{unwritable}""#)),
            "{unwritable} is advertised but this engine cannot write one"
        );
        assert_eq!(SessionFormat::for_extension(unwritable), None);
    }
}

/// **The formats a save can now preserve are advertised, both actions each.**
///
/// The point of the row: an administrator installing this into Nextcloud gets
/// it offered for the spreadsheets their users actually have, and `.csv` is
/// most of them.
#[test]
fn the_delimited_formats_are_offered_for_editing() {
    let xml = document("https://calc.example", &Brand::default(), None);
    for ext in ["xlsx", "csv", "tsv", "psv"] {
        assert!(
            xml.contains(&format!(r#"<action name="edit" ext="{ext}""#)),
            "no edit action for {ext}: {xml}"
        );
        assert!(
            xml.contains(&format!(r#"<action name="view" ext="{ext}""#)),
            "no view action for {ext}: {xml}"
        );
    }
}
