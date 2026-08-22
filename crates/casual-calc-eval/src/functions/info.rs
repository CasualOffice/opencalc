//! Logical operators and the information functions that report on a cell or
//! the host rather than computing from it.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// The IS-family: evaluate the single argument and test the resulting value.
pub(crate) fn is_predicate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    test: fn(&Value) -> bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    Value::Bool(test(&ev.eval_expr(sheet, arg)))
}

/// IFS(test1, value1, test2, value2, …): first TRUE test's value, else #N/A.
pub(crate) fn eval_ifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    for pair in args.chunks(2) {
        match ev.eval_expr(sheet, &pair[0]).as_bool() {
            Ok(true) => return ev.eval_expr(sheet, &pair[1]),
            Ok(false) => {}
            Err(e) => return Value::Error(e),
        }
    }
    Value::Error(ErrorValue::Na)
}

/// SWITCH(expr, v1, r1, …, [default]).
pub(crate) fn eval_switch(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let subject = ev.eval_expr(sheet, &args[0]);
    let rest = &args[1..];
    let mut i = 0;
    while i + 1 < rest.len() {
        let candidate = ev.eval_expr(sheet, &rest[i]);
        if values_equal(&subject, &candidate) {
            return ev.eval_expr(sheet, &rest[i + 1]);
        }
        i += 2;
    }
    // A trailing odd argument is the default.
    if rest.len() % 2 == 1 {
        return ev.eval_expr(sheet, &rest[rest.len() - 1]);
    }
    Value::Error(ErrorValue::Na)
}

/// Excel equality for SWITCH: numeric when both numeric, else case-insensitive text.
pub(crate) fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        _ => a
            .as_text()
            .unwrap_or_default()
            .eq_ignore_ascii_case(&b.as_text().unwrap_or_default()),
    }
}

/// IFNA(value, value_if_na): substitute only on #N/A.
pub(crate) fn eval_ifna(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]) {
        Value::Error(ErrorValue::Na) => ev.eval_expr(sheet, &args[1]),
        v => v,
    }
}

/// Which aggregate a `*IFS` call computes over the matched positions.
pub(crate) enum IfsKind {
    Sum,
    Average,
    Max,
    Min,
}

/// A function taking no arguments at all.
pub(crate) fn nullary(args: &[Expr], value: Value) -> Value {
    if args.is_empty() {
        value
    } else {
        Value::Error(ErrorValue::Value)
    }
}

/// `N(value)` — the numeric reading of a value.
///
/// Not the same as a coercion: text is `0` rather than an error, `TRUE` is 1,
/// and an error propagates. That asymmetry is the function's entire purpose, so
/// routing it through the ordinary `as_number` would defeat it.
pub(crate) fn eval_n(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, arg) {
        Value::Number(n) => Value::Number(n),
        Value::Bool(b) => Value::Number(if b { 1.0 } else { 0.0 }),
        Value::Error(e) => Value::Error(e),
        Value::Text(_) | Value::Empty => Value::Number(0.0),
        Value::Array { .. } | Value::Lambda(_) => Value::Error(ErrorValue::Value),
    }
}

/// `TYPE(value)` — 1 number, 2 text, 4 logical, 16 error, 64 array.
pub(crate) fn eval_type(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let code = match ev.eval_expr(sheet, arg) {
        // An empty cell reads as a number here, matching Excel: TYPE of a blank
        // is 1, not a distinct "empty" code.
        Value::Number(_) | Value::Empty => 1.0,
        Value::Text(_) => 2.0,
        Value::Bool(_) => 4.0,
        Value::Error(_) => 16.0,
        // Excel's own code for an array, which is why TYPE has one.
        Value::Array { .. } => 64.0,
        // 128 is Excel's code for a lambda.
        Value::Lambda(_) => 128.0,
    };
    Value::Number(code)
}

/// `ERROR.TYPE(error)` — the ordinal of an error value, or `#N/A` when the
/// argument is not an error at all.
pub(crate) fn eval_error_type(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg) {
        Value::Error(e) => Value::Number(match e {
            ErrorValue::Null => 1.0,
            ErrorValue::Div0 => 2.0,
            ErrorValue::Value => 3.0,
            ErrorValue::Ref => 4.0,
            ErrorValue::Name => 5.0,
            ErrorValue::Num => 6.0,
            ErrorValue::Na => 7.0,
            // #SPILL! post-dates the 5th edition, which stops at 7. Excel
            // numbers it 9 (8 being #GETTING_DATA), so that is what a workbook
            // round-tripped through Excel expects to see.
            ErrorValue::Spill => 9.0,
            // Excel numbers #CALC! 14, continuing past #SPILL!'s 9.
            ErrorValue::Calc => 14.0,
        }),
        // Not an error: the answer is itself #N/A, not a number.
        _ => Value::Error(ErrorValue::Na),
    }
}

/// The zero-based index of a sheet by name, case-insensitively as Excel
/// compares sheet names.
pub(crate) fn sheet_index_by_name(ev: &Evaluator<'_>, name: &str) -> Option<usize> {
    ev.workbook()
        .sheets
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(name))
}

/// `ISREF(value)` — whether the argument *is* a reference.
///
/// Decided from the expression rather than its value, because by the time a
/// function receives an argument the evaluator has already resolved a reference
/// to its contents; asking the value would answer "no" for every reference.
pub(crate) fn eval_is_ref(args: &[Expr]) -> Value {
    match args {
        [expr] => Value::Bool(matches!(expr, Expr::Reference(_) | Expr::Range(_, _))),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// `ISFORMULA(reference)` — whether the referenced cell holds a formula.
pub(crate) fn eval_is_formula(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [Expr::Reference(reference)] = args else {
        // Anything that is not a plain reference cannot hold a formula, and
        // Excel answers #VALUE! rather than FALSE — the difference matters,
        // since FALSE would read as "that cell has no formula".
        return Value::Error(ErrorValue::Value);
    };
    let Some(resolved) = reference.resolve(ev.origin()) else {
        return Value::Error(ErrorValue::Ref);
    };
    let at = CellRef::new(resolved.row, resolved.col);
    let has = ev
        .workbook()
        .sheets
        .get(sheet)
        .and_then(|s| s.cells.get(at))
        .is_some_and(|c| c.formula.is_some());
    Value::Bool(has)
}

// --- Date and time helpers -------------------------------------------------

/// Whether a character occupies two bytes in a double-byte character set.
///
/// The `*B` text functions count bytes, not characters, and in a DBCS locale a
/// full-width character is two. Aliasing them to their character versions —
/// which is what they collapse to in a single-byte locale — would silently
/// halve every count on Japanese or Chinese text, which is precisely the data
/// they exist for.
///
/// The ranges are the full-width and CJK blocks: CJK ideographs, kana, Hangul,
/// and the full-width forms of the ASCII punctuation.
pub(crate) fn is_double_byte(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F      // Hangul Jamo
        | 0x2E80..=0x303E    // CJK radicals, kangxi, CJK symbols
        | 0x3041..=0x33FF    // kana, Hangul compat, CJK compat
        | 0x3400..=0x4DBF    // CJK extension A
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xA000..=0xA4CF    // Yi
        | 0xAC00..=0xD7A3    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK compatibility ideographs
        | 0xFE30..=0xFE6F    // CJK compatibility forms
        | 0xFF00..=0xFF60    // full-width forms
        | 0xFFE0..=0xFFE6    // full-width signs
        | 0x1F300..=0x1F64F  // emoji, which Excel also counts as wide
        | 0x20000..=0x2FA1F  // CJK extensions B..F
    )
}

/// The byte width of a string under DBCS rules.
pub(crate) fn dbcs_len(text: &str) -> usize {
    text.chars()
        .map(|c| if is_double_byte(c) { 2 } else { 1 })
        .sum()
}

/// Take characters until `bytes` byte-widths are used.
///
/// A cut that would land inside a double-byte character stops before it: Excel
/// pads with a space in that case, and half of a character is not a character.
pub(crate) fn dbcs_take(text: &str, bytes: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = if is_double_byte(c) { 2 } else { 1 };
        if used + w > bytes {
            // Landing mid-character: Excel emits a space for the half it can
            // not represent, so the result still has the requested width.
            if used < bytes {
                out.push(' ');
            }
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// The character index at or after a byte offset, for the `*B` functions that
/// take a start position.
pub(crate) fn dbcs_char_index(text: &str, byte_pos: usize) -> usize {
    let mut used = 0usize;
    for (i, c) in text.chars().enumerate() {
        if used >= byte_pos {
            return i;
        }
        used += if is_double_byte(c) { 2 } else { 1 };
    }
    text.chars().count()
}

/// The byte-oriented text functions.
///
/// Each is its character-counting twin measured in DBCS bytes. On text with no
/// double-byte characters they agree exactly, which is what makes them safe to
/// use in a single-byte locale — and why a test asserts both halves.
pub(crate) fn eval_text_bytes(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    let text_arg = |ev: &mut Evaluator<'_>, i: usize| -> Result<String, ErrorValue> {
        ev.eval_expr(sheet, &args[i]).as_text()
    };
    match name {
        "LENB" => {
            if args.len() != 1 {
                return Value::Error(ErrorValue::Value);
            }
            match text_arg(ev, 0) {
                Ok(t) => Value::Number(dbcs_len(&t) as f64),
                Err(e) => Value::Error(e),
            }
        }
        "LEFTB" | "RIGHTB" => match text_and_count(ev, sheet, args) {
            Ok((text, count)) => {
                let count = count as usize;
                if name == "LEFTB" {
                    Value::Text(dbcs_take(&text, count))
                } else {
                    // From the right: drop the leading bytes instead.
                    let total = dbcs_len(&text);
                    let skip = total.saturating_sub(count);
                    let at = dbcs_char_index(&text, skip);
                    Value::Text(text.chars().skip(at).collect())
                }
            }
            Err(e) => Value::Error(e),
        },
        "MIDB" => {
            if args.len() != 3 {
                return Value::Error(ErrorValue::Value);
            }
            let text = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let [start, count] = match pair_of_numbers(ev, sheet, &args[1..3]) {
                Ok(v) => v,
                Err(e) => return e,
            };
            if start < 1.0 || count < 0.0 {
                return Value::Error(ErrorValue::Value);
            }
            let at = dbcs_char_index(&text, start as usize - 1);
            let rest: String = text.chars().skip(at).collect();
            Value::Text(dbcs_take(&rest, count as usize))
        }
        "FINDB" | "SEARCHB" => {
            if args.len() < 2 || args.len() > 3 {
                return Value::Error(ErrorValue::Value);
            }
            let needle = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let hay = match text_arg(ev, 1) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let start = match args.get(2) {
                Some(a) => match ev.eval_expr(sheet, a).as_number() {
                    Ok(n) => n as usize,
                    Err(e) => return Value::Error(e),
                },
                None => 1,
            };
            if start < 1 {
                return Value::Error(ErrorValue::Value);
            }
            let from_char = dbcs_char_index(&hay, start - 1);
            let rest: String = hay.chars().skip(from_char).collect();
            // FINDB is case-sensitive; SEARCHB is not — the same split as
            // between FIND and SEARCH.
            let found = if name == "FINDB" {
                rest.find(&needle)
            } else {
                rest.to_lowercase().find(&needle.to_lowercase())
            };
            match found {
                Some(byte_off) => {
                    // `find` gives a UTF-8 offset; convert through characters to
                    // a DBCS byte position, which is a different measure again.
                    let chars_before = rest[..byte_off].chars().count();
                    let prefix: String = hay.chars().take(from_char + chars_before).collect();
                    Value::Number(dbcs_len(&prefix) as f64 + 1.0)
                }
                None => Value::Error(ErrorValue::Value),
            }
        }
        "REPLACEB" => {
            if args.len() != 4 {
                return Value::Error(ErrorValue::Value);
            }
            let text = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let [start, count] = match pair_of_numbers(ev, sheet, &args[1..3]) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let with = match text_arg(ev, 3) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            if start < 1.0 || count < 0.0 {
                return Value::Error(ErrorValue::Value);
            }
            let head_chars = dbcs_char_index(&text, start as usize - 1);
            let head: String = text.chars().take(head_chars).collect();
            let tail_at = dbcs_char_index(&text, start as usize - 1 + count as usize);
            let tail: String = text.chars().skip(tail_at).collect();
            Value::Text(format!("{head}{with}{tail}"))
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// `INFO(type)` — properties of the environment.
///
/// Only what can be answered truthfully is answered. There is no working
/// directory and no open-file count in a browser, and inventing them would be
/// worse than `#N/A`: a formula built on a fabricated path fails somewhere far
/// from here.
pub(crate) fn eval_info(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let kind = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(t) => t.trim().to_ascii_lowercase(),
        Err(e) => return Value::Error(e),
    };
    match kind.as_str() {
        // A1 versus R1C1; this engine speaks A1 and the origin is the
        // top-left of the sheet.
        "origin" => Value::Text("$A:$A$1".to_owned()),
        "recalc" => Value::Text("Automatic".to_owned()),
        "release" => Value::Text(env!("CARGO_PKG_VERSION").to_owned()),
        "system" => Value::Text("pcdos".to_owned()),
        "numfile" | "directory" | "osversion" | "memavail" | "memused" | "totmem" => {
            Value::Error(ErrorValue::Na)
        }
        _ => Value::Error(ErrorValue::Value),
    }
}

/// `CONVERT(number, from, to)` — unit conversion.
///
/// Every unit is expressed as a factor to one SI base per *category*, so a
/// conversion is a division rather than a lookup of every pair — a pairwise
/// table of eighty units is six thousand entries and every one a chance to be
/// wrong. Units from different categories are `#N/A`, which is Excel's answer
/// and the honest one: kilograms into metres is not a small error, it is a
/// question with no answer.
///
/// Temperature is the exception and has to be, because its scales have
/// different zeros: a factor alone turns 0 °C into 0 °F.
pub(crate) fn convert_factor(unit: &str) -> Option<(&'static str, f64)> {
    // (category, factor to the category's base unit)
    const UNITS: &[(&str, &str, f64)] = &[
        // Mass — base gram.
        ("g", "mass", 1.0),
        ("sg", "mass", 14593.9029372064),
        ("lbm", "mass", 453.59237),
        ("u", "mass", 1.66053886e-24),
        ("ozm", "mass", 28.349523125),
        ("grain", "mass", 0.06479891),
        ("cwt", "mass", 45359.237),
        ("shweight", "mass", 45359.237),
        ("uk_cwt", "mass", 50802.34544),
        ("lcwt", "mass", 50802.34544),
        ("hweight", "mass", 50802.34544),
        ("stone", "mass", 6350.29318),
        ("ton", "mass", 907184.74),
        ("uk_ton", "mass", 1016046.9088),
        ("LTON", "mass", 1016046.9088),
        ("brton", "mass", 1016046.9088),
        // Distance — base metre.
        ("m", "distance", 1.0),
        ("mi", "distance", 1609.344),
        ("Nmi", "distance", 1852.0),
        ("in", "distance", 0.0254),
        ("ft", "distance", 0.3048),
        ("yd", "distance", 0.9144),
        ("ang", "distance", 1e-10),
        ("ell", "distance", 1.143),
        ("ly", "distance", 9.46073047258080e15),
        ("parsec", "distance", 3.08567758128155e16),
        ("pc", "distance", 3.08567758128155e16),
        ("Picapt", "distance", 0.0254 / 72.0),
        ("Pica", "distance", 0.0254 / 6.0),
        ("survey_mi", "distance", 1609.347218694437),
        // Time — base second.
        ("yr", "time", 31557600.0),
        ("day", "time", 86400.0),
        ("d", "time", 86400.0),
        ("hr", "time", 3600.0),
        ("mn", "time", 60.0),
        ("min", "time", 60.0),
        ("sec", "time", 1.0),
        ("s", "time", 1.0),
        // Pressure — base pascal.
        ("Pa", "pressure", 1.0),
        ("p", "pressure", 1.0),
        ("atm", "pressure", 101325.0),
        ("at", "pressure", 101325.0),
        ("mmHg", "pressure", 133.322),
        ("psi", "pressure", 6894.75729316836),
        ("Torr", "pressure", 101325.0 / 760.0),
        // Force — base newton.
        ("N", "force", 1.0),
        ("dyn", "force", 1e-5),
        ("dy", "force", 1e-5),
        ("lbf", "force", 4.4482216152605),
        ("pond", "force", 9.80665e-3),
        // Energy — base joule.
        ("J", "energy", 1.0),
        ("e", "energy", 1e-7),
        ("c", "energy", 4.184),
        ("cal", "energy", 4.1868),
        ("eV", "energy", 1.60217653e-19),
        ("ev", "energy", 1.60217653e-19),
        ("HPh", "energy", 2684519.53769617),
        ("hh", "energy", 2684519.53769617),
        ("Wh", "energy", 3600.0),
        ("wh", "energy", 3600.0),
        ("flb", "energy", 1.3558179483314),
        ("BTU", "energy", 1055.05585262),
        ("btu", "energy", 1055.05585262),
        // Power — base watt.
        ("HP", "power", 745.69987158227),
        ("h", "power", 745.69987158227),
        ("W", "power", 1.0),
        ("w", "power", 1.0),
        ("PS", "power", 735.49875),
        // Magnetism — base tesla.
        ("T", "magnetism", 1.0),
        ("ga", "magnetism", 1e-4),
        // Volume — base litre.
        ("l", "volume", 1.0),
        ("L", "volume", 1.0),
        ("lt", "volume", 1.0),
        ("tsp", "volume", 0.00492892159375),
        ("tbs", "volume", 0.01478676478125),
        ("oz", "volume", 0.0295735295625),
        ("cup", "volume", 0.2365882365),
        ("pt", "volume", 0.473176473),
        ("us_pt", "volume", 0.473176473),
        ("uk_pt", "volume", 0.5682612544),
        ("qt", "volume", 0.946352946),
        ("uk_qt", "volume", 1.1365225088),
        ("gal", "volume", 3.785411784),
        ("uk_gal", "volume", 4.54609),
        ("ang3", "volume", 1e-27),
        ("barrel", "volume", 158.987294928),
        ("bushel", "volume", 35.23907016688),
        ("ft3", "volume", 28.316846592),
        ("in3", "volume", 0.016387064),
        ("m3", "volume", 1000.0),
        ("mi3", "volume", 4168181825440.58),
        ("yd3", "volume", 764.554857984),
        // Area — base square metre.
        ("m2", "area", 1.0),
        ("uk_acre", "area", 4046.8564224),
        ("us_acre", "area", 4046.87261),
        ("ang2", "area", 1e-20),
        ("ar", "area", 100.0),
        ("ft2", "area", 0.09290304),
        ("ha", "area", 10000.0),
        ("in2", "area", 0.00064516),
        ("mi2", "area", 2589988.110336),
        ("Nmi2", "area", 3429904.0),
        ("yd2", "area", 0.83612736),
        // Information — base bit.
        ("bit", "information", 1.0),
        ("byte", "information", 8.0),
        // Speed — base metres per second.
        ("m/s", "speed", 1.0),
        ("m/sec", "speed", 1.0),
        ("m/h", "speed", 1.0 / 3600.0),
        ("mph", "speed", 0.44704),
        ("kn", "speed", 1852.0 / 3600.0),
        ("admkn", "speed", 1853.184 / 3600.0),
    ];
    // Metric prefixes, which apply to the SI units only. Excel accepts them on
    // anything, so this does too rather than maintaining a second table of
    // which unit is metric.
    const PREFIXES: &[(&str, f64)] = &[
        ("Y", 1e24),
        ("Z", 1e21),
        ("E", 1e18),
        ("P", 1e15),
        ("T", 1e12),
        ("G", 1e9),
        ("M", 1e6),
        ("k", 1e3),
        ("h", 1e2),
        ("e", 1e1),
        ("d", 1e-1),
        ("c", 1e-2),
        ("m", 1e-3),
        ("u", 1e-6),
        ("n", 1e-9),
        ("p", 1e-12),
        ("f", 1e-15),
        ("a", 1e-18),
        ("z", 1e-21),
        ("y", 1e-24),
    ];
    // Exact match first: `m` is metres, not milli-anything, and `T` is tesla
    // rather than tera. Trying prefixes first would silently reinterpret the
    // commonest units in the table.
    if let Some((_, cat, f)) = UNITS.iter().find(|(u, _, _)| *u == unit) {
        return Some((cat, *f));
    }
    for (p, scale) in PREFIXES {
        if let Some(rest) = unit.strip_prefix(p)
            && !rest.is_empty()
            && let Some((_, cat, f)) = UNITS.iter().find(|(u, _, _)| u == &rest)
        {
            return Some((cat, f * scale));
        }
    }
    None
}

/// Temperature in kelvin, and back — the one family a factor cannot express,
/// because the scales do not share a zero.
pub(crate) fn temperature_to_kelvin(unit: &str, v: f64) -> Option<f64> {
    Some(match unit {
        "C" | "cel" => v + 273.15,
        "F" | "fah" => (v - 32.0) * 5.0 / 9.0 + 273.15,
        "K" | "kel" => v,
        "Rank" => v * 5.0 / 9.0,
        "Reau" => v * 1.25 + 273.15,
        _ => return None,
    })
}

pub(crate) fn temperature_from_kelvin(unit: &str, k: f64) -> Option<f64> {
    Some(match unit {
        "C" | "cel" => k - 273.15,
        "F" | "fah" => (k - 273.15) * 9.0 / 5.0 + 32.0,
        "K" | "kel" => k,
        "Rank" => k * 9.0 / 5.0,
        "Reau" => (k - 273.15) * 0.8,
        _ => return None,
    })
}
