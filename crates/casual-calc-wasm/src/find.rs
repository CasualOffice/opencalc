//! Find and Replace — the one search every surface uses (XL-UX-024/025).
//!
//! Excel's Find and Replace dialog reads one query against one set of options:
//! where to look (the sheet, the selection, or the workbook), in what order
//! (by rows or by columns), what to read (formulas, values or notes), and how
//! to compare (case, entire cell). `?` and `*` are always wildcards, and `~`
//! escapes them — Excel has no switch for that, so the dialog has none.
//!
//! Before this module the bar had one matcher for Find and another, narrower
//! one for Replace (substring only, no whole-cell, no wildcards, one sheet), so
//! a cell Find had just found could be one Replace All would not touch. Here a
//! single [`Pattern`] decides both what matches and which span is rewritten,
//! and the older bindings are wrappers over it.

use super::*;

/// One token of a compiled query.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    /// A literal character.
    Lit(char),
    /// `?` — exactly one character.
    One,
    /// `*` — any run of characters, including none.
    Run,
}

/// A compiled Find query.
///
/// Matching is a position-set simulation over the tokens, O(text × pattern)
/// per cell whatever the query, so a pathological `*a*a*a*…` cannot go
/// exponential — the property [`casual_calc_model::wildcard_match`] holds too.
#[derive(Debug, Clone)]
pub(crate) struct Pattern {
    toks: Vec<Tok>,
    match_case: bool,
}

/// A text prepared for matching: its characters — lowercased one original
/// character at a time when case does not matter — and, for each, the index of
/// the original character it came from.
///
/// Folding per character with an origin map is what keeps `İ` (two characters
/// when lowercased) and `K` (the Kelvin sign) from desynchronising the folded
/// and original strings — the defect `ci_replace` documents — while still
/// letting `i̇stanbul` find `İstanbul`, as a whole-string lowercase does.
struct Folded {
    chars: Vec<char>,
    origin: Vec<usize>,
}

fn fold(text: &str, match_case: bool) -> Folded {
    let mut chars = Vec::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (i, ch) in text.chars().enumerate() {
        if match_case {
            chars.push(ch);
            origin.push(i);
        } else {
            for lc in ch.to_lowercase() {
                chars.push(lc);
                origin.push(i);
            }
        }
    }
    Folded { chars, origin }
}

impl Pattern {
    /// Compile `query`. With `wildcards`, `?` and `*` are pattern syntax and
    /// `~` makes the next character literal (`~*`, `~?`, `~~`); a trailing
    /// lone `~` is itself literal. Without, every character is literal.
    pub(crate) fn new(query: &str, wildcards: bool, match_case: bool) -> Self {
        let mut toks = Vec::with_capacity(query.len());
        let lit = |toks: &mut Vec<Tok>, ch: char| {
            if match_case {
                toks.push(Tok::Lit(ch));
            } else {
                toks.extend(ch.to_lowercase().map(Tok::Lit));
            }
        };
        let mut chars = query.chars().peekable();
        while let Some(ch) = chars.next() {
            if !wildcards {
                lit(&mut toks, ch);
                continue;
            }
            match ch {
                '?' => toks.push(Tok::One),
                '*' => {
                    // `**` is `*`; collapsing keeps the state count small.
                    if toks.last() != Some(&Tok::Run) {
                        toks.push(Tok::Run);
                    }
                }
                '~' => match chars.peek() {
                    Some(&next @ ('*' | '?' | '~')) => {
                        chars.next();
                        lit(&mut toks, next);
                    }
                    _ => lit(&mut toks, '~'),
                },
                other => lit(&mut toks, other),
            }
        }
        Self { toks, match_case }
    }

    /// Whether the query was empty (an empty query matches nothing).
    pub(crate) fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    /// Follow every `*` that can match the empty string.
    fn close(&self, states: &mut [Option<usize>]) {
        for k in 0..self.toks.len() {
            if self.toks[k] == Tok::Run
                && let Some(start) = states[k]
            {
                let next = &mut states[k + 1];
                if next.is_none_or(|s| start < s) {
                    *next = Some(start);
                }
            }
        }
    }

    /// Advance every live state over `ch`. Each state carries the earliest
    /// start that reaches it, which is what makes the leftmost match findable
    /// in one pass.
    fn step(&self, states: &[Option<usize>], ch: char) -> Vec<Option<usize>> {
        let mut next = vec![None; states.len()];
        let mut put = |k: usize, start: usize| {
            let slot: &mut Option<usize> = &mut next[k];
            if slot.is_none_or(|s| start < s) {
                *slot = Some(start);
            }
        };
        for (k, tok) in self.toks.iter().enumerate() {
            let Some(start) = states[k] else { continue };
            match *tok {
                Tok::Lit(p) if p == ch => put(k + 1, start),
                Tok::Lit(_) => {}
                Tok::One => put(k + 1, start),
                Tok::Run => put(k, start),
            }
        }
        next
    }

    /// Whether the pattern matches the whole of `text` — Excel's "Match entire
    /// cell contents". An empty cell is never a hit, as in Excel.
    pub(crate) fn matches_whole(&self, text: &str) -> bool {
        if self.is_empty() || text.is_empty() {
            return false;
        }
        let mut states = vec![None; self.toks.len() + 1];
        states[0] = Some(0);
        self.close(&mut states);
        for ch in fold(text, self.match_case).chars {
            states = self.step(&states, ch);
            if states.iter().all(Option::is_none) {
                return false;
            }
            self.close(&mut states);
        }
        states[self.toks.len()].is_some()
    }

    /// The leftmost, then longest, span of `text[from..]` the pattern matches,
    /// as `(start, end)` indices into `text`.
    fn span(&self, text: &[char], from: usize) -> Option<(usize, usize)> {
        if self.is_empty() {
            return None;
        }
        let m = self.toks.len();
        let mut states: Vec<Option<usize>> = vec![None; m + 1];
        let mut best: Option<(usize, usize)> = None;
        let mut i = from;
        loop {
            // A new attempt starts here only while nothing has matched: once a
            // match exists, every later start is to its right and loses.
            if best.is_none() && states[0].is_none_or(|s| s > i) {
                states[0] = Some(i);
            }
            self.close(&mut states);
            if let Some(start) = states[m]
                && best.is_none_or(|(bs, be)| start < bs || (start == bs && i > be))
            {
                best = Some((start, i));
            }
            if let Some((bs, _)) = best {
                for s in &mut states {
                    if s.is_some_and(|start| start > bs) {
                        *s = None;
                    }
                }
                if states.iter().all(Option::is_none) {
                    break;
                }
            }
            if i >= text.len() {
                break;
            }
            states = self.step(&states, text[i]);
            i += 1;
        }
        best
    }

    /// Whether the pattern occurs anywhere in `text`.
    pub(crate) fn occurs_in(&self, text: &str) -> bool {
        !text.is_empty() && self.span(&fold(text, self.match_case).chars, 0).is_some()
    }

    /// Whether `text` is a hit under `whole_cell`.
    pub(crate) fn hits(&self, text: &str, whole_cell: bool) -> bool {
        if whole_cell {
            self.matches_whole(text)
        } else {
            self.occurs_in(text)
        }
    }

    /// `text` with every match replaced by `with` — or, under `whole_cell`, the
    /// whole of it. `None` when nothing matched, so a caller never writes a
    /// cell it did not change.
    pub(crate) fn replace(&self, text: &str, with: &str, whole_cell: bool) -> Option<String> {
        if whole_cell {
            return self.matches_whole(text).then(|| with.to_owned());
        }
        if text.is_empty() {
            return None;
        }
        let folded = fold(text, self.match_case);
        let orig: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        // `pos` walks the folded text; `emitted` is how much of the original
        // has been copied out. A match boundary inside one original
        // character's expansion widens to the whole character.
        let (mut pos, mut emitted, mut any) = (0usize, 0usize, false);
        while pos < folded.chars.len() {
            let Some((s, e)) = self.span(&folded.chars, pos) else {
                break;
            };
            any = true;
            let so = folded.origin.get(s).copied().unwrap_or(orig.len());
            if so > emitted {
                out.extend(&orig[emitted..so]);
            }
            out.push_str(with);
            if e == s {
                // Only an all-`*` pattern matches empty, and it matches longest,
                // so this is the end of the text.
                break;
            }
            emitted = emitted.max(folded.origin[e - 1] + 1);
            pos = e;
        }
        if !any {
            return None;
        }
        if emitted < orig.len() {
            out.extend(&orig[emitted..]);
        }
        Some(out)
    }
}

/// What Find reads in each cell — Excel's **Look in**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LookIn {
    /// What you would type: `=B2*C2`, `12.5`, `widget`.
    Formulas,
    /// What the cell shows, with its number format applied.
    Values,
    /// The text of the cell's note.
    Notes,
}

/// The dialog's options, read from the JSON a binding receives.
#[derive(Debug, Clone)]
pub(crate) struct FindOptions {
    pub(crate) match_case: bool,
    pub(crate) whole_cell: bool,
    pub(crate) look_in: LookIn,
    pub(crate) all_sheets: bool,
    pub(crate) by_columns: bool,
    pub(crate) wildcards: bool,
    /// `[r0, c0, r1, c1]` — Excel searches only the selection when more than
    /// one cell is selected. Applies to the active sheet only.
    pub(crate) range: Option<[u32; 4]>,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            whole_cell: false,
            look_in: LookIn::Formulas,
            all_sheets: false,
            by_columns: false,
            wildcards: true,
            range: None,
        }
    }
}

impl FindOptions {
    /// Parse `{"matchCase","wholeCell","lookIn","allSheets","byColumns",
    /// "wildcards","range"}`; anything missing or malformed takes Excel's
    /// default, so a caller can pass `{}`.
    pub(crate) fn from_json(json: &str) -> Self {
        let mut o = Self::default();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
            return o;
        };
        let flag = |k: &str, d: bool| v.get(k).and_then(serde_json::Value::as_bool).unwrap_or(d);
        o.match_case = flag("matchCase", false);
        o.whole_cell = flag("wholeCell", false);
        o.all_sheets = flag("allSheets", false);
        o.by_columns = flag("byColumns", false);
        o.wildcards = flag("wildcards", true);
        o.look_in = match v.get("lookIn").and_then(serde_json::Value::as_str) {
            Some("values") => LookIn::Values,
            Some("notes") => LookIn::Notes,
            _ => LookIn::Formulas,
        };
        o.range = v.get("range").and_then(|r| {
            let a = r.as_array()?;
            if a.len() != 4 {
                return None;
            }
            let n = |i: usize| a[i].as_u64().and_then(|x| u32::try_from(x).ok());
            let (r0, c0, r1, c1) = (n(0)?, n(1)?, n(2)?, n(3)?);
            Some([r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)])
        });
        o
    }
}

/// One hit: the sheet and cell, and what Find All lists for it.
#[derive(Debug, Clone)]
pub(crate) struct Hit {
    pub(crate) sheet: usize,
    pub(crate) at: CellRef,
}

/// Every hit for `pattern` under `opts`, in Excel's order: sheet by sheet,
/// and within a sheet row-major (**By Rows**) or column-major (**By
/// Columns**).
pub(crate) fn scan(wb: &Workbook, sheet: usize, pattern: &Pattern, opts: &FindOptions) -> Vec<Hit> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let sheets: Vec<usize> = if opts.all_sheets {
        (0..wb.sheets.len()).collect()
    } else {
        vec![sheet]
    };
    let mut hits = Vec::new();
    for idx in sheets {
        let Some(sh) = wb.sheets.get(idx) else {
            continue;
        };
        let inside = |at: CellRef| match (opts.all_sheets, opts.range) {
            (false, Some([r0, c0, r1, c1])) => {
                at.row >= r0 && at.row <= r1 && at.col >= c0 && at.col <= c1
            }
            _ => true,
        };
        let start = hits.len();
        if opts.look_in == LookIn::Notes {
            for note in &sh.comments {
                if inside(note.at) && pattern.hits(&note.text, opts.whole_cell) {
                    hits.push(Hit {
                        sheet: idx,
                        at: note.at,
                    });
                }
            }
        } else {
            for (at, cell) in sh.cells.iter() {
                if !inside(at) {
                    continue;
                }
                let haystack = if opts.look_in == LookIn::Values {
                    display_text(wb, cell)
                } else {
                    cell_input_text(wb, cell, at)
                };
                if pattern.hits(&haystack, opts.whole_cell) {
                    hits.push(Hit { sheet: idx, at });
                }
            }
        }
        let by_columns = opts.by_columns;
        hits[start..].sort_unstable_by_key(|h| {
            if by_columns {
                (h.at.col, h.at.row)
            } else {
                (h.at.row, h.at.col)
            }
        });
    }
    hits
}

/// The rewrites Replace All would make on the hits of `scan`: `(sheet, cell,
/// new input)`. Replace reads formulas only, as Excel's Replace tab does.
pub(crate) fn plan_replace(
    wb: &Workbook,
    sheet: usize,
    pattern: &Pattern,
    with: &str,
    opts: &FindOptions,
) -> Vec<(usize, CellRef, String)> {
    let mut opts = opts.clone();
    opts.look_in = LookIn::Formulas;
    let mut edits = Vec::new();
    for hit in scan(wb, sheet, pattern, &opts) {
        let Some(cell) = wb.sheets.get(hit.sheet).and_then(|s| s.cells.get(hit.at)) else {
            continue;
        };
        let input = cell_input_text(wb, cell, hit.at);
        if let Some(out) = pattern.replace(&input, with, opts.whole_cell) {
            edits.push((hit.sheet, hit.at, out));
        }
    }
    edits
}

/// Excel's Find and Replace search, as JSON
/// `[{"s","r","c","v","f"}, …]` in the order Find Next walks: `v` is the cell
/// as displayed and `f` what you would type into it (Find All's **Value** and
/// **Formula** columns). `opts` is described at [`session_replace_all_ex`].
#[wasm_bindgen]
pub fn session_find_ex(sheet: usize, query: &str, opts: &str) -> String {
    let opts = FindOptions::from_json(opts);
    with_session(|s| {
        let wb = s.workbook();
        let pattern = Pattern::new(query, opts.wildcards, opts.match_case);
        let rows: Vec<String> = scan(wb, sheet, &pattern, &opts)
            .into_iter()
            .map(|h| {
                let cell = wb.sheets.get(h.sheet).and_then(|sh| sh.cells.get(h.at));
                let (v, f) = cell.map_or_else(
                    || (String::new(), String::new()),
                    |c| (display_text(wb, c), cell_input_text(wb, c, h.at)),
                );
                format!(
                    "{{\"s\":{},\"r\":{},\"c\":{},\"v\":{},\"f\":{}}}",
                    h.sheet,
                    h.at.row,
                    h.at.col,
                    json_string(&v),
                    json_string(&f)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    })
    .unwrap_or_else(|| "[]".to_owned())
}

/// Replace every match, workbook-wide if asked, as **one** undo step; returns
/// how many cells changed.
///
/// `opts` is JSON: `matchCase`, `wholeCell` (booleans, default false),
/// `allSheets` (Within: Workbook), `byColumns`, `wildcards` (default true —
/// Excel always reads `?` and `*` as wildcards) and `range` (`[r0,c0,r1,c1]`,
/// the selection to stay inside). `lookIn` is ignored: Replace reads formulas.
///
/// Refused as a whole if any sheet it would write is protected over the cells
/// it would change, so a replace never lands half done.
#[wasm_bindgen]
pub fn session_replace_all_ex(
    sheet: usize,
    find: &str,
    replace: &str,
    opts: &str,
) -> Result<usize, JsError> {
    let opts = FindOptions::from_json(opts);
    let pattern = Pattern::new(find, opts.wildcards, opts.match_case);
    if pattern.is_empty() {
        return Ok(0);
    }
    // Scan under one borrow, guard between, write under another: the guard
    // borrows the session itself (see `session_replace_all`'s history).
    let edits = with_session(|s| plan_replace(s.workbook(), sheet, &pattern, replace, &opts))
        .unwrap_or_default();
    if edits.is_empty() {
        return Ok(0);
    }
    let mut boxes: std::collections::BTreeMap<usize, (u32, u32, u32, u32)> =
        std::collections::BTreeMap::new();
    for (s, at, _) in &edits {
        let b = boxes.entry(*s).or_insert((u32::MAX, u32::MAX, 0, 0));
        b.0 = b.0.min(at.row);
        b.1 = b.1.min(at.col);
        b.2 = b.2.max(at.row);
        b.3 = b.3.max(at.col);
    }
    for (s, (r0, c0, r1, c1)) in boxes {
        guard_protected(s, r0, c0, r1, c1)?;
    }
    with_session_mut(|session| {
        let count = edits.len();
        // Each replaced input is re-parsed, so a number or formula stays one.
        let ops: Vec<EditOperation> = edits
            .into_iter()
            .map(|(s, at, input)| session.input_edit(s, at, &input))
            .collect();
        session.edit(EditOperation::Batch(ops)).map_err(js)?;
        Ok(count)
    })
}

/// Replace the matches in one cell under the same options as
/// [`session_replace_all_ex`]; returns whether the cell changed. The dialog's
/// **Replace** button.
#[wasm_bindgen]
pub fn session_replace_at_ex(
    sheet: usize,
    row: u32,
    col: u32,
    find: &str,
    replace: &str,
    opts: &str,
) -> Result<bool, JsError> {
    let opts = FindOptions::from_json(opts);
    let pattern = Pattern::new(find, opts.wildcards, opts.match_case);
    if pattern.is_empty() {
        return Ok(false);
    }
    guard_protected(sheet, row, col, row, col)?;
    with_session_mut(|session| {
        let at = CellRef::new(row, col);
        let replaced = {
            let wb = session.workbook();
            let Some(cell) = wb.sheets.get(sheet).and_then(|s| s.cells.get(at)) else {
                return Ok(false);
            };
            pattern.replace(&cell_input_text(wb, cell, at), replace, opts.whole_cell)
        };
        let Some(replaced) = replaced else {
            return Ok(false);
        };
        let op = session.input_edit(sheet, at, &replaced);
        session.edit(op).map_err(js)?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::{Pattern, session_find_ex, session_replace_all_ex};
    use crate::{
        session_can_undo, session_cell_input, session_new, session_set_cell, session_undo,
    };

    fn p(q: &str) -> Pattern {
        Pattern::new(q, true, false)
    }

    #[test]
    fn wildcards_match_and_tilde_escapes_them() {
        assert!(p("w?dget").occurs_in("a widget"));
        assert!(p("w*t").matches_whole("widget"));
        assert!(!p("w*t").matches_whole("widgets"));
        // `~*` is a literal star: it finds "5*3" and not "513".
        assert!(p("5~*3").occurs_in("=5*3"));
        assert!(!p("5~*3").occurs_in("513"));
        assert!(p("~~").occurs_in("a~b"));
        // Without wildcards every character is literal.
        assert!(!Pattern::new("w?dget", false, false).occurs_in("widget"));
        assert!(Pattern::new("w?dget", false, false).occurs_in("w?dget"));
    }

    #[test]
    fn case_follows_match_case_and_survives_lengthening_lowercase() {
        assert!(p("WIDGET").occurs_in("widget"));
        assert!(!Pattern::new("WIDGET", true, true).occurs_in("widget"));
        // `İ` lowercases to two characters; no index may drift, and the
        // two-character spelling still finds it, as a whole-string fold did.
        assert!(p("i\u{307}stanbul").occurs_in("İstanbul"));
        assert_eq!(
            p("i\u{307}stanbul")
                .replace("İstanbul", "X", false)
                .as_deref(),
            Some("X")
        );
        assert_eq!(
            p("smi").replace("Ürün İsmi", "X", false).as_deref(),
            Some("Ürün İX")
        );
        assert_eq!(
            p("sample")
                .replace("300\u{212A} sample", "X", false)
                .as_deref(),
            Some("300\u{212A} X")
        );
        assert_eq!(p("x").replace("İxİ", "y", false).as_deref(), Some("İyİ"));
    }

    #[test]
    fn replace_rewrites_the_leftmost_longest_span_of_each_match() {
        assert_eq!(
            p("a").replace("banana", "o", false).as_deref(),
            Some("bonono")
        );
        assert_eq!(
            p("a*a").replace("banana", "X", false).as_deref(),
            Some("bX")
        );
        assert_eq!(
            p("n?").replace("banana", "_", false).as_deref(),
            Some("ba__")
        );
        assert_eq!(p("*").replace("abc", "Z", false).as_deref(), Some("Z"));
        assert_eq!(p("q").replace("abc", "Z", false), None);
        // Entire cell: the whole cell becomes the replacement, or nothing does.
        assert_eq!(p("ab?").replace("abc", "Z", true).as_deref(), Some("Z"));
        assert_eq!(p("ab").replace("abc", "Z", true), None);
    }

    #[test]
    fn find_orders_by_rows_or_by_columns() {
        session_new();
        for (r, c) in [(0, 1), (1, 0), (0, 0), (1, 1)] {
            session_set_cell(0, r, c, "hit").unwrap();
        }
        let rows = session_find_ex(0, "hit", "{}");
        let cols = session_find_ex(0, "hit", r#"{"byColumns":true}"#);
        let order = |json: &str| -> Vec<(u64, u64)> {
            let v: serde_json::Value = serde_json::from_str(json).unwrap();
            v.as_array()
                .unwrap()
                .iter()
                .map(|h| (h["r"].as_u64().unwrap(), h["c"].as_u64().unwrap()))
                .collect()
        };
        assert_eq!(order(&rows), vec![(0, 0), (0, 1), (1, 0), (1, 1)]);
        assert_eq!(order(&cols), vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn replace_all_across_the_workbook_is_one_undo_step() {
        session_new();
        crate::session_add_sheet().unwrap();
        session_set_cell(0, 0, 0, "red apple").unwrap();
        session_set_cell(1, 2, 2, "Red").unwrap();
        session_set_cell(0, 3, 0, "=LEN(\"red\")").unwrap();
        let n = session_replace_all_ex(0, "red", "blue", r#"{"allSheets":true}"#).unwrap();
        assert_eq!(n, 3);
        assert_eq!(session_cell_input(0, 0, 0), "blue apple");
        assert_eq!(session_cell_input(1, 2, 2), "blue");
        assert_eq!(session_cell_input(0, 3, 0), "=LEN(\"blue\")");
        session_undo().unwrap();
        assert_eq!(session_cell_input(0, 0, 0), "red apple");
        assert_eq!(session_cell_input(1, 2, 2), "Red");
        assert_eq!(session_cell_input(0, 3, 0), "=LEN(\"red\")");
        let _ = session_can_undo();
    }

    #[test]
    fn whole_cell_and_selection_limit_what_replace_all_touches() {
        session_new();
        session_set_cell(0, 0, 0, "cat").unwrap();
        session_set_cell(0, 1, 0, "cats").unwrap();
        session_set_cell(0, 5, 5, "cat").unwrap();
        let n = session_replace_all_ex(0, "cat", "dog", r#"{"wholeCell":true,"range":[0,0,3,3]}"#)
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(session_cell_input(0, 0, 0), "dog");
        assert_eq!(session_cell_input(0, 1, 0), "cats");
        assert_eq!(session_cell_input(0, 5, 5), "cat");
    }
}
