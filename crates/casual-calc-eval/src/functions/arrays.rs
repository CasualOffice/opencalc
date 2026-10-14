//! The 2022 array-shaping functions (`XL-FEAT-013`): `VSTACK`, `HSTACK`,
//! `TAKE`, `DROP`, `CHOOSECOLS`, `CHOOSEROWS`, `TOCOL`, `TOROW`, `WRAPROWS`,
//! `WRAPCOLS`, `EXPAND`, `RANDARRAY`, `TEXTSPLIT`, `TEXTBEFORE`, `TEXTAFTER`,
//! `ARRAYTOTEXT`, `VALUETOTEXT` and `TRIMRANGE`.
//!
//! Each takes a range or a computed array and answers with a new shape, which
//! the spill pass writes out. Three rules are shared and live here once:
//!
//! * A ragged result is padded with `#N/A` (or the caller's `pad_with`),
//!   never with blanks — a blank would read as data that was there.
//! * A result with no cells is `#CALC!`, Excel's error for an empty array.
//! * Every result is sized through [`checked_array_size`] before anything is
//!   allocated, so no argument can ask for more than the engine can hold.

use super::*;

/// A rectangular block being built: rows of values, all the same width.
type Block = Vec<Vec<Value>>;

/// An argument as a block — a range, a computed array, or a scalar (1×1).
fn block_arg(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Result<Block, ErrorValue> {
    let grid = eval_range_2d(ev, sheet, arg)?;
    Ok(grid_rows(&grid))
}

/// A finished block as a value: `#CALC!` when empty, a scalar when 1×1, and
/// refused by size before it is flattened.
fn block_value(block: Block) -> Value {
    let rows = block.len();
    let cols = block.first().map_or(0, Vec::len);
    if rows == 0 || cols == 0 {
        return Value::Error(ErrorValue::Calc);
    }
    if let Err(e) = checked_array_size(rows, cols) {
        return Value::Error(e);
    }
    rows_to_value(block)
}

/// An optional numeric argument; `None` when omitted or written empty.
fn optional_number(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    i: usize,
) -> Result<Option<f64>, ErrorValue> {
    match args.get(i) {
        None | Some(Expr::Empty) => Ok(None),
        Some(a) => ev.eval_expr(sheet, a).as_number().map(Some),
    }
}

/// The `pad_with` argument: `#N/A` unless given.
fn pad_arg(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], i: usize) -> Value {
    match args.get(i) {
        None | Some(Expr::Empty) => Value::Error(ErrorValue::Na),
        Some(a) => ev.eval_expr(sheet, a),
    }
}

/// Dispatch for this module.
pub(crate) fn eval_array_shape(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    let result = match name {
        "VSTACK" | "HSTACK" => stack(ev, sheet, args, name == "VSTACK"),
        "TAKE" | "DROP" => take_drop(ev, sheet, args, name == "TAKE"),
        "CHOOSECOLS" | "CHOOSEROWS" => choose_lines(ev, sheet, args, name == "CHOOSEROWS"),
        "TOCOL" | "TOROW" => to_line(ev, sheet, args, name == "TOCOL"),
        "WRAPROWS" | "WRAPCOLS" => wrap(ev, sheet, args, name == "WRAPROWS"),
        "EXPAND" => expand(ev, sheet, args),
        "RANDARRAY" => return randarray(ev, sheet, args),
        "TEXTSPLIT" => textsplit(ev, sheet, args),
        "TEXTBEFORE" | "TEXTAFTER" => return text_around(ev, sheet, args, name == "TEXTBEFORE"),
        "ARRAYTOTEXT" => return array_to_text(ev, sheet, args),
        "VALUETOTEXT" => return value_to_text(ev, sheet, args),
        "TRIMRANGE" => trim_range(ev, sheet, args),
        _ => return Value::Error(ErrorValue::Name),
    };
    match result {
        Ok(block) => block_value(block),
        Err(e) => Value::Error(e),
    }
}

/// `VSTACK(array1, …)` / `HSTACK(array1, …)` — the arrays one after another,
/// the narrower ones padded with `#N/A` to the widest.
fn stack(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    vertical: bool,
) -> Result<Block, ErrorValue> {
    if args.is_empty() {
        return Err(ErrorValue::Value);
    }
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        let block = block_arg(ev, sheet, arg)?;
        parts.push(if vertical { block } else { transpose(block) });
    }
    let width = parts.iter().map(|b| b[0].len()).max().unwrap_or(0);
    let height: usize = parts.iter().map(Vec::len).sum();
    checked_array_size(height, width)?;
    let mut out: Block = Vec::with_capacity(height);
    for block in parts {
        for mut row in block {
            row.resize(width, Value::Error(ErrorValue::Na));
            out.push(row);
        }
    }
    Ok(if vertical { out } else { transpose(out) })
}

fn transpose(block: Block) -> Block {
    let cols = block.first().map_or(0, Vec::len);
    let mut out: Block = (0..cols).map(|_| Vec::with_capacity(block.len())).collect();
    for row in block {
        for (c, v) in row.into_iter().enumerate() {
            out[c].push(v);
        }
    }
    out
}

/// The span `TAKE`/`DROP` keep along one axis of length `len`: a positive
/// count from the start, a negative one from the end.
fn kept_span(len: usize, count: Option<f64>, take: bool) -> (usize, usize) {
    let Some(n) = count else {
        return (0, len);
    };
    let n = n.trunc();
    let k = (n.abs().min(len as f64)) as usize;
    match (take, n >= 0.0) {
        (true, true) => (0, k),
        (true, false) => (len - k, len),
        (false, true) => (k, len),
        (false, false) => (0, len - k),
    }
}

/// `TAKE(array, rows, [columns])` / `DROP(array, rows, [columns])`.
///
/// Counts beyond the array's size take (or drop) everything; a result with
/// nothing left is `#CALC!`.
fn take_drop(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    take: bool,
) -> Result<Block, ErrorValue> {
    if args.len() < 2 || args.len() > 3 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    let rows = optional_number(ev, sheet, args, 1)?;
    let cols = optional_number(ev, sheet, args, 2)?;
    let (r0, r1) = kept_span(block.len(), rows, take);
    let (c0, c1) = kept_span(block[0].len(), cols, take);
    Ok(block[r0..r1]
        .iter()
        .map(|row| row[c0..c1].to_vec())
        .filter(|row| !row.is_empty())
        .collect())
}

/// `CHOOSECOLS(array, col1, …)` / `CHOOSEROWS(array, row1, …)` — the lines
/// named, in the order named, repeats allowed; negative counts from the end.
/// A zero or an index past the array is `#VALUE!`.
fn choose_lines(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    rows: bool,
) -> Result<Block, ErrorValue> {
    if args.len() < 2 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    let block = if rows { block } else { transpose(block) };
    let len = block.len() as f64;
    let mut picked: Block = Vec::new();
    for arg in &args[1..] {
        let indices: Vec<Value> = match ev.eval_expr_array(sheet, arg) {
            Value::Array { cells, .. } => cells,
            v => vec![v],
        };
        for v in indices {
            let n = v.as_number()?.trunc();
            if n == 0.0 || n.abs() > len {
                return Err(ErrorValue::Value);
            }
            let i = if n > 0.0 { n - 1.0 } else { len + n } as usize;
            picked.push(block[i].clone());
            checked_array_size(picked.len(), block[0].len())?;
        }
    }
    Ok(if rows { picked } else { transpose(picked) })
}

/// `TOCOL(array, [ignore], [scan_by_column])` / `TOROW(…)` — the array as one
/// line, optionally without blanks (1), errors (2) or both (3).
fn to_line(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    column: bool,
) -> Result<Block, ErrorValue> {
    if args.is_empty() || args.len() > 3 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    let ignore = optional_number(ev, sheet, args, 1)?.unwrap_or(0.0).trunc();
    if !(0.0..=3.0).contains(&ignore) {
        return Err(ErrorValue::Value);
    }
    let by_column = match args.get(2) {
        None | Some(Expr::Empty) => false,
        Some(a) => ev.eval_expr(sheet, a).as_bool()?,
    };
    let block = if by_column { transpose(block) } else { block };
    let (skip_blank, skip_error) = (ignore == 1.0 || ignore == 3.0, ignore >= 2.0);
    let values: Vec<Value> = block
        .into_iter()
        .flatten()
        .filter(|v| !(skip_blank && matches!(v, Value::Empty)))
        .filter(|v| !(skip_error && matches!(v, Value::Error(_))))
        .collect();
    Ok(if column {
        values.into_iter().map(|v| vec![v]).collect()
    } else if values.is_empty() {
        Vec::new()
    } else {
        vec![values]
    })
}

/// `WRAPROWS(vector, wrap_count, [pad_with])` / `WRAPCOLS(…)` — a single row
/// or column folded into lines of `wrap_count`, the last padded.
fn wrap(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    by_rows: bool,
) -> Result<Block, ErrorValue> {
    if args.len() < 2 || args.len() > 3 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    // Only a vector can be wrapped; a block has no single order to fold.
    if block.len() > 1 && block[0].len() > 1 {
        return Err(ErrorValue::Value);
    }
    let count = ev.eval_expr(sheet, &args[1]).as_number()?.trunc();
    if count < 1.0 {
        return Err(ErrorValue::Num);
    }
    let pad = pad_arg(ev, sheet, args, 2);
    let values: Vec<Value> = block.into_iter().flatten().collect();
    let width = (count as usize).min(values.len().max(1));
    let lines = values.len().div_ceil(width);
    checked_array_size(lines, width)?;
    let mut out: Block = values.chunks(width).map(<[Value]>::to_vec).collect();
    if let Some(last) = out.last_mut() {
        last.resize(width, pad);
    }
    Ok(if by_rows { out } else { transpose(out) })
}

/// `EXPAND(array, rows, [columns], [pad_with])` — grow to a size, padding;
/// a size smaller than the array is `#VALUE!`.
fn expand(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<Block, ErrorValue> {
    if args.len() < 2 || args.len() > 4 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    let (h, w) = (block.len(), block[0].len());
    let target = |n: Option<f64>, current: usize| match n {
        None => Ok(current),
        Some(n) if n.trunc() < current as f64 => Err(ErrorValue::Value),
        Some(n) => Ok(array_dimension(n)),
    };
    let rows = target(optional_number(ev, sheet, args, 1)?, h)?;
    let cols = target(optional_number(ev, sheet, args, 2)?, w)?;
    checked_array_size(rows, cols)?;
    let pad = pad_arg(ev, sheet, args, 3);
    let mut out = block;
    for row in &mut out {
        row.resize(cols, pad.clone());
    }
    out.resize(rows, vec![pad; cols]);
    Ok(out)
}

/// `RANDARRAY([rows], [columns], [min], [max], [whole_number])` — volatile,
/// like `RAND`, and drawing from the same host-seeded generator so a
/// recalculation is reproducible.
fn randarray(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let mut read = |i: usize, default: f64| -> Result<f64, ErrorValue> {
        Ok(optional_number(ev, sheet, args, i)?.unwrap_or(default))
    };
    let (rows, cols, min, max) = match (read(0, 1.0), read(1, 1.0), read(2, 0.0), read(3, 1.0)) {
        (Ok(r), Ok(c), Ok(lo), Ok(hi)) => (r.trunc(), c.trunc(), lo, hi),
        (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (.., Err(e)) => {
            return Value::Error(e);
        }
    };
    let whole = match args.get(4) {
        None | Some(Expr::Empty) => false,
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
    };
    if rows < 0.0 || cols < 0.0 || min > max {
        return Value::Error(ErrorValue::Value);
    }
    if rows == 0.0 || cols == 0.0 {
        return Value::Error(ErrorValue::Calc);
    }
    if whole && (min.fract() != 0.0 || max.fract() != 0.0) {
        return Value::Error(ErrorValue::Value);
    }
    let (rows, cols) = (array_dimension(rows), array_dimension(cols));
    let size = match checked_array_size(rows, cols) {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let cells: Vec<Value> = (0..size)
        .map(|_| {
            let r = ev.next_random();
            Value::Number(if whole {
                (min + (r * (max - min + 1.0)).floor()).min(max)
            } else {
                min + r * (max - min)
            })
        })
        .collect();
    if size == 1 {
        return cells.into_iter().next().unwrap_or(Value::Empty);
    }
    Value::Array { rows, cols, cells }
}

/// The delimiters of a `TEXTSPLIT`/`TEXTBEFORE`/`TEXTAFTER` argument: one
/// text, or an array of them, any of which matches.
fn delimiters(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Result<Vec<String>, ErrorValue> {
    match ev.eval_expr_array(sheet, arg) {
        Value::Array { cells, .. } => cells.iter().map(Value::as_text).collect(),
        v => Ok(vec![v.as_text()?]),
    }
}

/// Every match of any delimiter in `text`, as byte ranges, left to right and
/// non-overlapping — at each position the earliest-starting, then longest,
/// delimiter wins. Case-folding compares on lower-cased copies, which keep
/// byte offsets for the ASCII and the common cases.
fn find_all(text: &str, delims: &[String], fold: bool) -> Vec<(usize, usize)> {
    let hay = if fold {
        text.to_lowercase()
    } else {
        text.to_owned()
    };
    let needles: Vec<String> = delims
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| if fold { d.to_lowercase() } else { d.clone() })
        .collect();
    // Lower-casing can change a string's byte length (rare, outside ASCII);
    // matching then falls back to exact case rather than misplacing a cut.
    let (hay, needles) = if hay.len() == text.len() {
        (hay, needles)
    } else {
        (
            text.to_owned(),
            delims.iter().filter(|d| !d.is_empty()).cloned().collect(),
        )
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at < hay.len() {
        let best = needles
            .iter()
            .filter_map(|n| hay[at..].find(n.as_str()).map(|p| (at + p, n.len())))
            .min_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        match best {
            Some((start, len)) => {
                out.push((start, start + len));
                at = start + len;
            }
            None => break,
        }
    }
    out
}

/// `TEXTSPLIT(text, col_delimiter, [row_delimiter], [ignore_empty],
/// [match_mode], [pad_with])` — text cut into rows, and each row into
/// columns; short rows padded with `#N/A` (or `pad_with`).
fn textsplit(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<Block, ErrorValue> {
    if args.len() < 2 || args.len() > 6 {
        return Err(ErrorValue::Value);
    }
    let text = ev.eval_expr(sheet, &args[0]).as_text()?;
    let col_delims = match &args[1] {
        Expr::Empty => Vec::new(),
        a => delimiters(ev, sheet, a)?,
    };
    let row_delims = match args.get(2) {
        None | Some(Expr::Empty) => Vec::new(),
        Some(a) => delimiters(ev, sheet, a)?,
    };
    if col_delims.iter().all(String::is_empty) && row_delims.iter().all(String::is_empty) {
        return Err(ErrorValue::Value);
    }
    let ignore_empty = match args.get(3) {
        None | Some(Expr::Empty) => false,
        Some(a) => ev.eval_expr(sheet, a).as_bool()?,
    };
    let fold = optional_number(ev, sheet, args, 4)?.unwrap_or(0.0) != 0.0;
    let pad = pad_arg(ev, sheet, args, 5);
    let cut = |s: &str, delims: &[String]| -> Vec<String> {
        let mut pieces = Vec::new();
        let mut from = 0;
        for (start, end) in find_all(s, delims, fold) {
            pieces.push(s[from..start].to_owned());
            from = end;
        }
        pieces.push(s[from..].to_owned());
        if ignore_empty {
            pieces.retain(|p| !p.is_empty());
        }
        pieces
    };
    let mut out: Block = Vec::new();
    for line in cut(&text, &row_delims) {
        let cells: Vec<Value> = cut(&line, &col_delims)
            .into_iter()
            .map(Value::Text)
            .collect();
        if !cells.is_empty() {
            out.push(cells);
        }
    }
    let width = out.iter().map(Vec::len).max().unwrap_or(0);
    checked_array_size(out.len(), width)?;
    for row in &mut out {
        row.resize(width, pad.clone());
    }
    Ok(out)
}

/// `TEXTBEFORE` / `TEXTAFTER(text, delimiter, [instance_num], [match_mode],
/// [match_end], [if_not_found])`.
///
/// Microsoft's rules: instance 0, or one longer than the text, is `#VALUE!`;
/// a delimiter that is not there (or not that many times) is `#N/A`, or
/// `if_not_found`. An **empty** delimiter matches at once — at the start
/// searching forwards, at the end searching backwards — so TEXTBEFORE gives
/// `""` / the whole text and TEXTAFTER the reverse. `match_end` counts the
/// text's own edge as one more delimiter: its end for TEXTBEFORE, its start
/// for TEXTAFTER (as LibreOffice, the independent implementation, reads it).
fn text_around(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], before: bool) -> Value {
    if args.len() < 2 || args.len() > 6 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(t) => t,
        Err(e) => return Value::Error(e),
    };
    let delims = match delimiters(ev, sheet, &args[1]) {
        Ok(d) => d,
        Err(e) => return Value::Error(e),
    };
    let mut number = |i: usize, default: f64| -> Result<f64, ErrorValue> {
        Ok(optional_number(ev, sheet, args, i)?
            .unwrap_or(default)
            .trunc())
    };
    let (instance, fold, match_end) = match (number(2, 1.0), number(3, 0.0), number(4, 0.0)) {
        (Ok(i), Ok(m), Ok(e)) => (i, m != 0.0, e != 0.0),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return Value::Error(e),
    };
    let length = text.chars().count() as f64;
    if instance == 0.0 || instance.abs() > length.max(1.0) {
        return Value::Error(ErrorValue::Value);
    }
    let cut = |at: usize, end: usize| {
        Value::Text(if before {
            text[..at].to_owned()
        } else {
            text[end..].to_owned()
        })
    };
    if delims.iter().any(String::is_empty) {
        let edge = if instance > 0.0 { 0 } else { text.len() };
        return cut(edge, edge);
    }
    let mut found = find_all(&text, &delims, fold);
    if match_end {
        if before {
            found.push((text.len(), text.len()));
        } else {
            found.insert(0, (0, 0));
        }
    }
    let k = instance.abs() as usize;
    let hit = if instance > 0.0 {
        found.get(k - 1)
    } else {
        found.len().checked_sub(k).and_then(|i| found.get(i))
    };
    match hit {
        Some(&(start, end)) => cut(start, end),
        None => match args.get(5) {
            Some(a) if !matches!(a, Expr::Empty) => ev.eval_expr(sheet, a),
            _ => Value::Error(ErrorValue::Na),
        },
    }
}

/// One value as `VALUETOTEXT` / `ARRAYTOTEXT` write it: concise is what the
/// cell shows; strict quotes text (doubling its quotes) so it would read back
/// as the same value in a formula.
fn value_text(v: &Value, strict: bool) -> String {
    match v {
        Value::Text(s) if strict => format!("\"{}\"", s.replace('"', "\"\"")),
        Value::Text(s) => s.clone(),
        Value::Error(e) => e.to_string(),
        Value::Empty => String::new(),
        other => other.as_text().unwrap_or_default(),
    }
}

/// The `format` argument: 0 (concise, the default) or 1 (strict).
fn format_arg(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<bool, ErrorValue> {
    match optional_number(ev, sheet, args, 1)? {
        None => Ok(false),
        Some(f) if f.trunc() == 0.0 => Ok(false),
        Some(f) if f.trunc() == 1.0 => Ok(true),
        Some(_) => Err(ErrorValue::Value),
    }
}

/// `VALUETOTEXT(value, [format])`. An error is written as its text — that is
/// the function's point — rather than propagated.
fn value_to_text(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let strict = match format_arg(ev, sheet, args) {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let v = ev.eval_expr(sheet, &args[0]);
    Value::Text(value_text(&v, strict))
}

/// `ARRAYTOTEXT(array, [format])` — concise joins every value with `", "`;
/// strict writes an array constant, `{a,b;c,d}`, with text quoted.
fn array_to_text(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let strict = match format_arg(ev, sheet, args) {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let block = match block_arg(ev, sheet, &args[0]) {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    let text = if strict {
        let rows: Vec<String> = block
            .iter()
            .map(|r| {
                r.iter()
                    .map(|v| value_text(v, true))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect();
        format!("{{{}}}", rows.join(";"))
    } else {
        block
            .iter()
            .flatten()
            .map(|v| value_text(v, false))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return Value::Error(ErrorValue::Value);
    }
    Value::Text(text)
}

/// `TRIMRANGE(range, [trim_rows], [trim_cols])` — the range without its
/// blank outer rows and columns: 0 none, 1 leading, 2 trailing, 3 both (the
/// default) on each axis. Nothing left is `#CALC!`, Excel's empty array.
fn trim_range(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<Block, ErrorValue> {
    if args.is_empty() || args.len() > 3 {
        return Err(ErrorValue::Value);
    }
    let block = block_arg(ev, sheet, &args[0])?;
    let mut mode = |i: usize| -> Result<u8, ErrorValue> {
        let m = optional_number(ev, sheet, args, i)?.unwrap_or(3.0).trunc();
        if (0.0..=3.0).contains(&m) {
            Ok(m as u8)
        } else {
            Err(ErrorValue::Value)
        }
    };
    let (rows_mode, cols_mode) = (mode(1)?, mode(2)?);
    let blank = |v: &Value| matches!(v, Value::Empty);
    let row_blank: Vec<bool> = block.iter().map(|r| r.iter().all(blank)).collect();
    let col_blank: Vec<bool> = (0..block[0].len())
        .map(|c| block.iter().all(|r| blank(&r[c])))
        .collect();
    let span = |flags: &[bool], mode: u8| -> (usize, usize) {
        let mut start = 0;
        let mut end = flags.len();
        if mode & 1 != 0 {
            while start < end && flags[start] {
                start += 1;
            }
        }
        if mode & 2 != 0 {
            while end > start && flags[end - 1] {
                end -= 1;
            }
        }
        (start, end)
    };
    let (r0, r1) = span(&row_blank, rows_mode);
    let (c0, c1) = span(&col_blank, cols_mode);
    Ok(block[r0..r1]
        .iter()
        .map(|r| r[c0..c1].to_vec())
        .filter(|r| !r.is_empty())
        .collect())
}
