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
    let xml = document("https://calc.example/", &brand("OpenCalc"));

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
    assert!(document("https://calc.example", &Brand::default()).contains("external-https"));
    assert!(document("http://localhost:8090", &Brand::default()).contains("external-http"));
}

/// **The brand name reaches the document, and cannot break out of it.**
///
/// It is operator input going straight into an attribute. A `"` closes the
/// attribute and a `<` opens an element, so an unescaped name does not merely
/// look wrong — it produces XML the host cannot parse, and the editor vanishes
/// from the administrator's list with no error anywhere.
#[test]
fn a_brand_name_is_escaped_into_the_markup() {
    let xml = document("https://calc.example", &brand("Ada & Co \"Sheets\" <beta>"));

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
    assert!(!document("https://c.example", &Brand::default()).contains("favIconUrl"));

    let with = Brand {
        favicon: "https://cdn.example/logo.png".to_owned(),
        ..Brand::default()
    };
    assert!(
        document("https://c.example", &with)
            .contains(r#"favIconUrl="https://cdn.example/logo.png""#)
    );
}

/// **Nothing is advertised that the save leg would rewrite into another
/// format.**
///
/// A session saves an OOXML package. A host that handed us `.ods` and got that
/// back under the same name has lost the original, silently, with an
/// administrator's blessing — so the advertisement is the thing that has to
/// stay narrow until save preserves what it opened.
#[test]
fn only_formats_that_round_trip_are_advertised() {
    let xml = document("https://c.example", &Brand::default());
    for lossy in ["ods", "csv", "tsv", "xls", "fods"] {
        assert!(
            !xml.contains(&format!(r#"ext="{lossy}""#)),
            "{lossy} is advertised but a save would rewrite it as xlsx"
        );
    }
}
