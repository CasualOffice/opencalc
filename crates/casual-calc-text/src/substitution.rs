//! Metric-compatible font substitution — a deterministic, WASM-safe port of
//! opendoc's font-management design (`40-FONT-MANAGEMENT-DESIGN.md`).
//!
//! A cell declares a font family the host may not have installed (every
//! deterministic / WASM build, and any machine missing the font). Shaping or
//! rendering a missing Arial/Times/Courier run with an arbitrary default gives it
//! the wrong advances, so column autofit and the PNG render diverge from
//! Excel/LibreOffice. This module maps a requested family to a **bundled** face
//! whose metrics match the substitute LibreOffice itself uses:
//!
//! - Arial / Helvetica → **Liberation Sans**
//! - Times New Roman / Times → **Liberation Serif**
//! - Courier New / Courier → **Liberation Mono**
//! - Calibri → **Carlito**, Cambria → **Caladea**
//!
//! An unknown missing family is classified by generic (serif / sans / mono) via a
//! known-name list plus a substring heuristic, keeping it on a face with
//! plausible Latin metrics rather than an arbitrary default.
//!
//! This is the **single source of truth** for whole-face substitution, shared by
//! the editor canvas (which builds a CSS font stack via [`css_stack`]) and the
//! `casual-calc-render` PNG backend (which maps [`BundledFamily::name`] to the
//! bundled face bytes). The function is pure over the requested name — host-
//! independent and identical on native and `wasm32-unknown-unknown`.

/// How faithfully a substitute matches the requested family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubstituteKind {
    /// The requested family *is* a bundled family; used directly.
    Bundled,
    /// A different bundled family whose advances match, so line breaking and
    /// autofit are preserved (only the glyph shapes differ).
    MetricCompatible,
    /// No metric-compatible partner; classified by generic family. Layout may
    /// shift relative to the true font.
    Generic,
}

/// A bundled family a requested name resolves to: its canonical name (as bundled
/// in `casual-calc-render`) and the CSS generic to end a font stack with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BundledFamily {
    /// The canonical family name (matches the face's `name` table and the
    /// `@font-face` family the webapp registers).
    pub name: &'static str,
    /// The CSS generic fallback (`sans-serif` / `serif` / `monospace`).
    pub generic: &'static str,
}

/// Roboto — the default family / ultimate fallback.
pub const ROBOTO: BundledFamily = BundledFamily {
    name: "Roboto",
    generic: "sans-serif",
};
/// Caladea — metric-compatible with Cambria.
pub const CALADEA: BundledFamily = BundledFamily {
    name: "Caladea",
    generic: "serif",
};
/// Carlito — metric-compatible with Calibri.
pub const CARLITO: BundledFamily = BundledFamily {
    name: "Carlito",
    generic: "sans-serif",
};
/// Liberation Sans — metric-compatible with Arial/Helvetica.
pub const LIBERATION_SANS: BundledFamily = BundledFamily {
    name: "Liberation Sans",
    generic: "sans-serif",
};
/// Liberation Serif — metric-compatible with Times New Roman.
pub const LIBERATION_SERIF: BundledFamily = BundledFamily {
    name: "Liberation Serif",
    generic: "serif",
};
/// Liberation Mono — metric-compatible with Courier New.
pub const LIBERATION_MONO: BundledFamily = BundledFamily {
    name: "Liberation Mono",
    generic: "monospace",
};

/// The bundled family a requested name resolves to, and the fidelity of the match.
#[derive(Clone, Copy, Debug)]
pub struct Substitute {
    /// The bundled family to shape and render the run with.
    pub family: &'static BundledFamily,
    /// The fidelity of the substitution.
    pub kind: SubstituteKind,
}

/// Resolve a requested font family to a bundled family. `None` for a blank name
/// (the caller uses its default family).
#[must_use]
pub fn substitute(family: &str) -> Option<Substitute> {
    let key = family.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    Some(known_family(&key).unwrap_or(Substitute {
        family: classify_generic(&key),
        kind: SubstituteKind::Generic,
    }))
}

/// The bundled family for a name the table knows explicitly. `None` to classify
/// heuristically.
fn known_family(key: &str) -> Option<Substitute> {
    let bundled = |family| {
        Some(Substitute {
            family,
            kind: SubstituteKind::Bundled,
        })
    };
    let metric = |family| {
        Some(Substitute {
            family,
            kind: SubstituteKind::MetricCompatible,
        })
    };
    let generic = |family| {
        Some(Substitute {
            family,
            kind: SubstituteKind::Generic,
        })
    };
    match key {
        // The bundled families requested by their own name — used directly.
        "roboto" => bundled(&ROBOTO),
        "caladea" => bundled(&CALADEA),
        "carlito" => bundled(&CARLITO),
        "liberation sans" => bundled(&LIBERATION_SANS),
        "liberation serif" => bundled(&LIBERATION_SERIF),
        "liberation mono" => bundled(&LIBERATION_MONO),
        // Metric-compatible partners: matching advances preserve line breaks.
        "arial" | "arial narrow" | "arial black" | "helvetica" | "helvetica neue"
        | "nimbus sans" | "nimbus sans l" | "arimo" => metric(&LIBERATION_SANS),
        "times new roman" | "times" | "nimbus roman" | "nimbus roman no9 l" | "tinos" => {
            metric(&LIBERATION_SERIF)
        }
        "courier new" | "courier" | "nimbus mono" | "nimbus mono l" | "cousine" => {
            metric(&LIBERATION_MONO)
        }
        "calibri" => metric(&CARLITO),
        "cambria" => metric(&CALADEA),
        // Common families with no metric partner but a fixed generic class, so
        // they skip the substring heuristic (which would misread a name like
        // "Old Standard TT" as sans).
        "ubuntu" | "tahoma" | "verdana" | "segoe" | "segoe ui" | "open sans" | "trebuchet ms"
        | "century gothic" | "sans-serif" | "sans serif" => generic(&LIBERATION_SANS),
        "georgia" | "pt serif" | "old standard tt" | "garamond" | "adobe garamond" | "minion"
        | "minion pro" | "book antiqua" | "palatino" | "palatino linotype" | "constantia"
        | "serif" => generic(&LIBERATION_SERIF),
        "consolas" | "menlo" | "monaco" | "sf mono" | "cascadia code" | "cascadia mono"
        | "andale mono" | "lucida console" | "monospace" => generic(&LIBERATION_MONO),
        _ => None,
    }
}

/// Classify an unlisted, already-normalized family key by a generic-family
/// substring, defaulting to sans. `mono` wins first (a "… Mono" face is
/// monospaced regardless of "sans"/"serif" in the name); then `sans` before
/// `serif` so "sans serif" resolves to sans.
fn classify_generic(key: &str) -> &'static BundledFamily {
    if key.contains("mono") {
        &LIBERATION_MONO
    } else if key.contains("sans") {
        &LIBERATION_SANS
    } else if key.contains("serif") {
        &LIBERATION_SERIF
    } else {
        &LIBERATION_SANS
    }
}

/// A CSS font stack for a requested family: the deterministic bundled substitute
/// first (so every machine renders the same face once the webapp `@font-face`s
/// the bundled fonts), then the originally-requested name (honoring a real
/// installed face as a last-resort visual match), then the generic. A blank
/// request yields the default family.
#[must_use]
pub fn css_stack(requested: &str) -> String {
    let req = requested.trim();
    let Some(sub) = substitute(req) else {
        return format!("\"{}\", {}", ROBOTO.name, ROBOTO.generic);
    };
    if sub.family.name.eq_ignore_ascii_case(req) {
        format!("\"{}\", {}", sub.family.name, sub.family.generic)
    } else {
        format!(
            "\"{}\", \"{}\", {}",
            sub.family.name, req, sub.family.generic
        )
    }
}

/// The family names a host's font picker should offer, in the order to show
/// them (alphabetical, as Excel and Sheets list non-theme fonts).
///
/// Every entry is a name the table resolves explicitly, so a user can
/// only pick a family this build renders faithfully — either a bundled face or
/// a metric-compatible / fixed-generic substitute. Aliases that exist only to
/// catch what a `.xlsx` may *declare* (`Nimbus Sans`, `Arimo`, the bare CSS
/// generics, …) are deliberately absent: they resolve correctly on import but
/// are not something to offer as a choice. Arbitrary typed names still work —
/// [`substitute`] classifies them — the picker list is a convenience, not a
/// whitelist.
pub const PICKER_FAMILIES: &[&str] = &[
    "Andale Mono",
    "Arial",
    "Arial Black",
    "Arial Narrow",
    "Book Antiqua",
    "Caladea",
    "Calibri",
    "Cambria",
    "Carlito",
    "Cascadia Code",
    "Cascadia Mono",
    "Century Gothic",
    "Consolas",
    "Constantia",
    "Courier New",
    "Garamond",
    "Georgia",
    "Helvetica",
    "Helvetica Neue",
    "Liberation Mono",
    "Liberation Sans",
    "Liberation Serif",
    "Lucida Console",
    "Menlo",
    "Monaco",
    "Open Sans",
    "PT Serif",
    "Palatino Linotype",
    "Roboto",
    "SF Mono",
    "Segoe UI",
    "Tahoma",
    "Times New Roman",
    "Trebuchet MS",
    "Ubuntu",
    "Verdana",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_and_metric_partners() {
        assert_eq!(substitute("Carlito").unwrap().kind, SubstituteKind::Bundled);
        let calibri = substitute("Calibri").unwrap();
        assert_eq!(calibri.family.name, "Carlito");
        assert_eq!(calibri.kind, SubstituteKind::MetricCompatible);
        assert_eq!(substitute("Arial").unwrap().family.name, "Liberation Sans");
        assert_eq!(
            substitute("TIMES NEW ROMAN").unwrap().family.name,
            "Liberation Serif"
        );
        assert_eq!(
            substitute("Courier New").unwrap().family.name,
            "Liberation Mono"
        );
        assert_eq!(substitute("Cambria").unwrap().family.name, "Caladea");
    }

    #[test]
    fn picker_families_are_all_explicitly_known() {
        // A picker entry must resolve through the explicit table, never the
        // substring heuristic — otherwise the UI would offer a family whose
        // metrics are a guess.
        for name in PICKER_FAMILIES {
            let key = name.to_ascii_lowercase();
            assert!(
                known_family(&key).is_some(),
                "picker family {name:?} is not in the substitution table"
            );
        }
    }

    #[test]
    fn picker_families_are_sorted_and_unique() {
        let mut sorted = PICKER_FAMILIES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, PICKER_FAMILIES.to_vec());
    }

    #[test]
    fn generic_classification() {
        // Fixed-class known names.
        assert_eq!(
            substitute("Georgia").unwrap().family.name,
            "Liberation Serif"
        );
        assert_eq!(
            substitute("Consolas").unwrap().family.name,
            "Liberation Mono"
        );
        assert_eq!(
            substitute("Verdana").unwrap().family.name,
            "Liberation Sans"
        );
        // Substring heuristic for unlisted names: mono > sans > serif, default sans.
        assert_eq!(
            substitute("Foo Mono").unwrap().family.name,
            "Liberation Mono"
        );
        assert_eq!(
            substitute("Whatever Sans").unwrap().family.name,
            "Liberation Sans"
        );
        assert_eq!(
            substitute("Some Serif").unwrap().family.name,
            "Liberation Serif"
        );
        assert_eq!(
            substitute("Totally Unknown").unwrap().family.name,
            "Liberation Sans"
        );
        assert_eq!(
            substitute("Totally Unknown").unwrap().kind,
            SubstituteKind::Generic
        );
    }

    #[test]
    fn blank_is_none() {
        assert!(substitute("").is_none());
        assert!(substitute("   ").is_none());
    }

    #[test]
    fn css_stack_shape() {
        assert_eq!(css_stack("Calibri"), "\"Carlito\", \"Calibri\", sans-serif");
        assert_eq!(
            css_stack("Arial"),
            "\"Liberation Sans\", \"Arial\", sans-serif"
        );
        // A bundled family requested by its own name isn't duplicated.
        assert_eq!(css_stack("Carlito"), "\"Carlito\", sans-serif");
        // Blank → default family.
        assert_eq!(css_stack(""), "\"Roboto\", sans-serif");
    }
}
