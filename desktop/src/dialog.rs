//! The platform's own open and save panels: what goes in them.
//!
//! `File ▸ Open` used to raise the *webview's* file picker inside a native
//! window, which is the single most obviously-wrong thing about a desktop
//! build — a Chrome-shaped dialog inside an application that is not a browser.
//! The panel is now the operating system's, which means somebody has to say
//! which files it offers and what the save panel proposes as a name.
//!
//! **This module decides that and nothing else.** No file is opened here, no
//! dialog is raised here: the filters and names are values, so the part that is
//! wrong in a screenshot is the part `cargo test` can check without a window.
//!
//! The formats are the ones the engine reads and writes through
//! `format_for_extension` — `.xlsx` and the three delimited kinds. An extension
//! offered here that the engine refuses is a file the user is invited to open
//! and then told they cannot.

/// Every spreadsheet extension the panels offer, in the order a user meets them.
pub const SPREADSHEET_EXTENSIONS: &[&str] = &["xlsx", "csv", "tsv", "psv"];

/// One entry in a panel's format list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// What the panel calls this group.
    pub name: String,
    /// Extensions, lower-case and without a leading dot — which is the shape
    /// every platform's panel wants and the shape a caller is least likely to
    /// hand over.
    pub extensions: Vec<String>,
}

impl Filter {
    fn new(name: &str, extensions: &[&str]) -> Self {
        Self {
            name: name.to_owned(),
            extensions: extensions.iter().map(|e| normalise(e)).collect(),
        }
    }
}

/// `.CSV`, `csv`, and `.csv` are one extension.
///
/// A panel matches the string it is given literally: a filter built from
/// `".CSV"` shows an empty folder on macOS and nobody finds out until they try
/// to open a file with it.
fn normalise(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

/// What a human calls a format, for the panel's format list.
fn label(ext: &str) -> String {
    match normalise(ext).as_str() {
        "xlsx" => "Excel Workbook".to_owned(),
        "csv" => "Comma-separated values".to_owned(),
        "tsv" => "Tab-separated values".to_owned(),
        "psv" => "Pipe-separated values".to_owned(),
        // Not a format this build knows. Named rather than silently dropped:
        // an unknown extension reaching here is a bug upstream, and a panel
        // with no filter at all is indistinguishable from a panel that offers
        // everything.
        other => format!("{} file", other.to_uppercase()),
    }
}

/// The open panel's format list: everything first, then one entry per format.
///
/// "Spreadsheets" leads because it is what a user picking a file wants, and
/// "All Files" is last because a file with the wrong extension is a real
/// situation and a panel that cannot show it is a dead end.
///
/// **On macOS the grouping does not survive, and the escape hatch does not
/// work.** `rfd`'s Cocoa backend concatenates every filter's extensions into
/// one `setAllowedFileTypes:` list (`backend/macos/file_dialog/panel_ffi.rs`,
/// `add_filters`), so there is no format popup and `*` is matched as a literal
/// extension rather than as "anything". The list is still correct there — the
/// four spreadsheet extensions are selectable — but a `.csv` somebody named
/// `.txt` cannot be reached from the panel on that platform. Windows and the
/// GTK/portal backends build one entry per filter and behave as written. Left
/// as it is rather than trimmed to the platform's floor: the entries are right
/// on two of three platforms, and a filter list written down to macOS's
/// limitation would be wrong on the other two.
pub fn open_filters() -> Vec<Filter> {
    let mut out = vec![Filter::new("Spreadsheets", SPREADSHEET_EXTENSIONS)];
    out.extend(
        SPREADSHEET_EXTENSIONS
            .iter()
            .map(|ext| Filter::new(&label(ext), &[ext])),
    );
    out.push(Filter::new("All Files", &["*"]));
    out
}

/// The save panel's format list.
///
/// One entry, because the format was already chosen — `File ▸ Download ▸ CSV`
/// is the command that got here. A save panel offering four formats would let
/// the user pick a fifth answer nothing downstream honours.
pub fn save_filters(ext: &str) -> Vec<Filter> {
    vec![Filter::new(&label(ext), &[ext])]
}

/// The last path component, with no directories and nothing that walks upward.
///
/// A name reaches this from two directions: a path the platform's open panel
/// returned, and a string the webview sent. The second is why `..` and
/// separators are stripped rather than trusted — the result is proposed to a
/// *save* panel, and a proposed name that contains a separator is a save that
/// lands somewhere the user did not choose.
pub fn base_name(raw: &str) -> String {
    let last = raw
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty() && *part != "." && *part != "..")
        .unwrap_or("");
    last.to_owned()
}

/// What the save panel proposes: the document's own name, with the extension of
/// the format being written.
///
/// Only the *last* extension is replaced, so `archive.tar.gz` saved as `.csv`
/// becomes `archive.tar.csv` rather than `archive.csv` — the second is a
/// different file name, and a save panel that proposes one silently renames the
/// user's document.
pub fn suggested_file_name(document: Option<&str>, ext: &str) -> String {
    let ext = normalise(ext);
    let stem = document
        .map(base_name)
        .filter(|name| !name.trim().is_empty())
        .map(|name| {
            let name = name.trim().to_owned();
            match name.rfind('.') {
                // A leading dot is not an extension, it is a hidden file:
                // `.gitignore` saved as `.csv` must not become `.csv`.
                Some(dot) if dot > 0 => name[..dot].to_owned(),
                _ => name,
            }
        })
        // The browser editor's own default, kept so that a user who has used
        // both meets one name rather than two.
        .unwrap_or_else(|| "opencalc".to_owned());
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_panel_offers_every_format_the_engine_reads() {
        let filters = open_filters();
        assert_eq!(filters[0].name, "Spreadsheets");
        assert_eq!(filters[0].extensions, ["xlsx", "csv", "tsv", "psv"]);
        // One entry per format, and an escape hatch. A user with a `.csv` that
        // somebody named `.txt` has to be able to reach it.
        let names: Vec<&str> = filters.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Spreadsheets",
                "Excel Workbook",
                "Comma-separated values",
                "Tab-separated values",
                "Pipe-separated values",
                "All Files",
            ]
        );
        assert_eq!(filters.last().unwrap().extensions, ["*"]);
    }

    #[test]
    fn extensions_reach_the_panel_bare_and_lower_case() {
        // A platform panel matches the string literally. `".CSV"` matches
        // nothing, and the symptom is an open panel where every file is greyed
        // out — which reads as a broken app, not as a bad filter.
        let filters = save_filters(".CSV");
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].name, "Comma-separated values");
        assert_eq!(filters[0].extensions, ["csv"]);
    }

    #[test]
    fn an_unknown_format_is_still_named() {
        let filters = save_filters("ods");
        assert_eq!(filters[0].name, "ODS file");
        assert_eq!(filters[0].extensions, ["ods"]);
    }

    #[test]
    fn the_save_panel_proposes_the_document_under_the_new_extension() {
        assert_eq!(
            suggested_file_name(Some("figures.xlsx"), "csv"),
            "figures.csv"
        );
        assert_eq!(suggested_file_name(Some("figures"), "xlsx"), "figures.xlsx");
        // Nothing open yet: the browser editor's own default name.
        assert_eq!(suggested_file_name(None, "xlsx"), "opencalc.xlsx");
        assert_eq!(suggested_file_name(Some("  "), "xlsx"), "opencalc.xlsx");
    }

    #[test]
    fn only_the_last_extension_is_replaced() {
        assert_eq!(
            suggested_file_name(Some("archive.tar.gz"), "csv"),
            "archive.tar.csv"
        );
        // A dotfile's dot is not an extension.
        assert_eq!(
            suggested_file_name(Some(".gitignore"), "csv"),
            ".gitignore.csv"
        );
    }

    #[test]
    fn a_proposed_name_can_never_carry_a_directory() {
        // The name arrives from the webview, and it is handed to a *save*
        // panel. A separator in it is a save that lands somewhere the user
        // did not pick.
        assert_eq!(
            suggested_file_name(Some("/etc/passwd"), "csv"),
            "passwd.csv"
        );
        assert_eq!(
            suggested_file_name(Some("..\\..\\windows\\system32\\hosts"), "csv"),
            "hosts.csv"
        );
        assert_eq!(suggested_file_name(Some("../../.."), "csv"), "opencalc.csv");
        assert_eq!(
            base_name("/Users/me/Documents/figures.xlsx"),
            "figures.xlsx"
        );
    }
}
