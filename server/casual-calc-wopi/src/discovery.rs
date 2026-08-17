//! The discovery document, and the branding that appears in it.
//!
//! `/hosting/discovery` is how an editor becomes *installable*. An
//! administrator pastes one URL into Nextcloud, ownCloud, SharePoint, Moodle or
//! Alfresco, and the host reads this XML to learn which file extensions the
//! editor handles and where to send a browser for each. Without it there is no
//! way to point any of them at OpenCalc at all.
//!
//! # Why the extension list is short
//!
//! Only `xlsx`. The save leg of a session emits an OOXML package, so
//! advertising a format we can *read* but not write back — `.ods`, `.csv` —
//! would mean a host handing us one file and getting a different one back under
//! the same name. That is silent data loss with an administrator's blessing,
//! and the list stays honest until save can preserve the format it opened
//! (`WOPI-05`).
//!
//! # The brand is operator input, and goes into markup
//!
//! A name with a `"` in it closes an attribute; a name with a `<` opens an
//! element. Everything interpolated here is escaped, and it is escaped in one
//! place so a new field cannot quietly skip it.

/// What this deployment calls itself.
///
/// White-labelling is table stakes in the market this competes in — an
/// integrator reselling a spreadsheet editor cannot ship one with somebody
/// else's name on the tab. Every field has a working default, so an operator
/// who wants none of this configures none of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brand {
    /// Shown in the host's editor list, the browser tab, and the editor chrome.
    pub name: String,
    /// An absolute URL to a favicon, or empty for the bundled one.
    pub favicon: String,
    /// A CSS colour for the editor's accent.
    pub accent: String,
}

impl Default for Brand {
    fn default() -> Self {
        Self {
            name: "OpenCalc".to_owned(),
            favicon: String::new(),
            accent: "#1f6f4a".to_owned(),
        }
    }
}

impl Brand {
    /// Read the brand from the environment, falling back field by field.
    #[must_use]
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            name: non_empty("OPENCALC_BRAND_NAME").unwrap_or(default.name),
            favicon: non_empty("OPENCALC_BRAND_FAVICON_URL").unwrap_or(default.favicon),
            accent: non_empty("OPENCALC_BRAND_ACCENT").unwrap_or(default.accent),
        }
    }
}

/// An environment variable that is set *and* not blank.
///
/// `FOO=` in a compose file is how an operator spells "leave this alone", and
/// reading it as an empty brand name gives an editor with no name at all.
fn non_empty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The formats advertised for editing. See the module note before adding one.
const EDITABLE: &[&str] = &["xlsx"];

/// Render `/hosting/discovery` for a service published at `public_url`.
///
/// `public_url` is the address a *browser* will be sent to, which is not
/// necessarily the one this process is bound to — behind a proxy it never is.
#[must_use]
pub fn document(public_url: &str, brand: &Brand) -> String {
    let base = public_url.trim_end_matches('/');
    // WOPI distinguishes the zones so a host can pick the one matching its own
    // scheme; publishing an `http` action to an `https` host gives a browser a
    // mixed-content block rather than an editor.
    let zone = if base.starts_with("https://") {
        "external-https"
    } else {
        "external-http"
    };

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<wopi-discovery>\n");
    out.push_str(&format!("  <net-zone name=\"{zone}\">\n"));
    out.push_str(&format!(
        "    <app name=\"{}\"{}>\n",
        escape(&brand.name),
        if brand.favicon.is_empty() {
            String::new()
        } else {
            format!(" favIconUrl=\"{}\"", escape(&brand.favicon))
        }
    ));
    for ext in EDITABLE {
        // The trailing `?` matters: the host appends `WOPISrc=` and
        // `access_token=` to whatever this string is, and without it the first
        // parameter lands on the path.
        out.push_str(&format!(
            "      <action name=\"edit\" ext=\"{ext}\" default=\"true\" urlsrc=\"{base}/wopi/edit?\"/>\n"
        ));
        out.push_str(&format!(
            "      <action name=\"view\" ext=\"{ext}\" urlsrc=\"{base}/wopi/view?\"/>\n"
        ));
    }
    out.push_str("    </app>\n  </net-zone>\n</wopi-discovery>\n");
    out
}

/// Escape text for an XML attribute or element body.
fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
