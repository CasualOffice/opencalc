//! Lookup and reference: the `*LOOKUP` family, `INDEX`/`MATCH`, `OFFSET`,
//! `ROW`/`COLUMN` and `GETPIVOTDATA`.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// A materialized rectangular block of cell values (row-major).
pub(crate) struct Grid {
    pub(crate) rows: usize,
    pub(crate) cols: usize,
    pub(crate) cells: Vec<Value>,
}

impl Grid {
    pub(crate) fn get(&self, row: usize, col: usize) -> &Value {
        &self.cells[row * self.cols + col]
    }
}

/// Evaluate one argument into a [`Grid`]; a scalar becomes a 1x1 block.
pub(crate) fn eval_range_2d(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    arg: &Expr,
) -> Result<Grid, ErrorValue> {
    if let Expr::Range(a, b) = arg {
        let target = ev.resolve_sheet(&a.sheet, sheet).ok_or(ErrorValue::Ref)?;
        let (r0, c0, r1, c1) = ev.range_bounds(target, a, b);
        let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
        if area > MAX_RANGE_CELLS {
            return Err(ErrorValue::Num);
        }
        let rows = (r1 - r0 + 1) as usize;
        let cols = (c1 - c0 + 1) as usize;
        let mut cells = Vec::with_capacity(rows * cols);
        for row in r0..=r1 {
            for col in c0..=c1 {
                cells.push(ev.eval_cell(target, CellRef::new(row, col)));
            }
        }
        Ok(Grid { rows, cols, cells })
    } else {
        // Not a literal range — but it may still be a *shape*, now that
        // functions return arrays and `B1:B4>4` evaluates to one. Without this
        // a computed mask arrives as a single cell and matches nothing, which
        // is how `FILTER(data, B1:B4>4)` came back empty.
        match ev.eval_expr_array(sheet, arg) {
            Value::Array { rows, cols, cells } => Ok(Grid { rows, cols, cells }),
            other => Ok(Grid {
                rows: 1,
                cols: 1,
                cells: vec![other],
            }),
        }
    }
}

/// Order two values the way lookups compare: numerically when both are numeric,
/// otherwise by case-insensitive text (matches the engine's comparison rules).
pub(crate) fn loose_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (numeric_of(a), numeric_of(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => {
            let sa = a.as_text().unwrap_or_default().to_uppercase();
            let sb = b.as_text().unwrap_or_default().to_uppercase();
            Some(sa.cmp(&sb))
        }
    }
}

pub(crate) fn numeric_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `VLOOKUP` (`vertical` true) / `HLOOKUP` (false).
pub(crate) fn eval_vlookup(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    vertical: bool,
) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let index = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let approximate = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => true,
    };
    if index < 1 {
        return Value::Error(ErrorValue::Value);
    }
    let index = index as usize;
    // Length along the search axis, and bound for the return index.
    let (search_len, index_bound) = if vertical {
        (grid.rows, grid.cols)
    } else {
        (grid.cols, grid.rows)
    };
    if index > index_bound {
        return Value::Error(ErrorValue::Ref);
    }
    // Value in the search line at position `i`.
    let at = |g: &Grid, i: usize| -> Value {
        if vertical {
            g.get(i, 0).clone()
        } else {
            g.get(0, i).clone()
        }
    };
    let found = if approximate {
        // Largest entry <= key, assuming the line is sorted ascending.
        let mut best: Option<usize> = None;
        for i in 0..search_len {
            match loose_cmp(&at(&grid, i), &key) {
                Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                _ => break,
            }
        }
        best
    } else {
        (0..search_len).find(|&i| loose_cmp(&at(&grid, i), &key) == Some(Ordering::Equal))
    };
    match found {
        Some(i) if vertical => grid.get(i, index - 1).clone(),
        Some(i) => grid.get(index - 1, i).clone(),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `INDEX(range, row, [col])`. Row/col are 1-based; a single index selects
/// along the sole axis of a one-dimensional range.
pub(crate) fn eval_index(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let grid = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let first = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let second = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => Some(n as i64),
            Err(e) => return Value::Error(e),
        },
        None => None,
    };
    let (row, col) = match second {
        Some(c) => (first, c),
        None => {
            // One index: pick the axis that has more than one line.
            if grid.rows == 1 {
                (1, first)
            } else if grid.cols == 1 {
                (first, 1)
            } else {
                return Value::Error(ErrorValue::Ref);
            }
        }
    };
    if row < 1 || col < 1 || row as usize > grid.rows || col as usize > grid.cols {
        return Value::Error(ErrorValue::Ref);
    }
    grid.get(row as usize - 1, col as usize - 1).clone()
}

/// `MATCH(lookup, range, [type])`. Type 1 (default) ascending, 0 exact,
/// -1 descending. Returns the 1-based position, or `#N/A`.
pub(crate) fn eval_match(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let match_type = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    // MATCH operates on a single row or column.
    let line: Vec<&Value> = grid.cells.iter().collect();
    let found = match match_type {
        0 => line
            .iter()
            .position(|v| loose_cmp(v, &key) == Some(Ordering::Equal)),
        1 => {
            // Largest value <= key (ascending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
        _ => {
            // -1: smallest value >= key (descending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Greater) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
    };
    match found {
        Some(i) => Value::Number(i as f64 + 1.0),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `CHOOSE(index, value1, value2, ...)`. Only the selected value is evaluated.
pub(crate) fn eval_choose(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let index = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let choices = &args[1..];
    if index < 1 || index as usize > choices.len() {
        return Value::Error(ErrorValue::Value);
    }
    ev.eval_expr(sheet, &choices[index as usize - 1])
}

// --- Extra text -----------------------------------------------------------

/// ROWS / COLUMNS: the row/column count of a range (a lone cell ref is 1×1).
pub(crate) fn eval_dim(_ev: &mut Evaluator<'_>, _sheet: usize, args: &[Expr], rows: bool) -> Value {
    match args.first() {
        Some(Expr::Range(a, b)) => {
            let n = if rows {
                a.row.max(b.row) - a.row.min(b.row) + 1
            } else {
                a.col.max(b.col) - a.col.min(b.col) + 1
            };
            Value::Number(n as f64)
        }
        Some(Expr::Reference(_)) => Value::Number(1.0),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// ROW / COLUMN: the 1-based row/column of a reference (top-left of a range),
/// or of the calling cell when no argument is given.
pub(crate) fn eval_row_col(ev: &mut Evaluator<'_>, args: &[Expr], row: bool) -> Value {
    let index = match args.first() {
        None => match ev.current_cell() {
            Some((_, at)) => {
                if row {
                    at.row
                } else {
                    at.col
                }
            }
            None => return Value::Error(ErrorValue::Value),
        },
        Some(Expr::Reference(r)) => {
            let Some(at) = r.resolve(ev.origin()) else {
                return Value::Error(ErrorValue::Ref);
            };
            if row { at.row } else { at.col }
        }
        Some(Expr::Range(a, _)) => {
            let Some(at) = a.resolve(ev.origin()) else {
                return Value::Error(ErrorValue::Ref);
            };
            if row { at.row } else { at.col }
        }
        Some(_) => return Value::Error(ErrorValue::Value),
    };
    Value::Number((index + 1) as f64)
}

// --- Maths helpers ---------------------------------------------------------

/// `ADDRESS(row, col, [abs], [a1], [sheet])` — build a reference *as text*.
///
/// It returns a string, not a reference: `ADDRESS(1,1)` is `"$A$1"`, and it is
/// `INDIRECT` that turns such a string back into something to read.
pub(crate) fn eval_address(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let mut number = |i: usize, default: f64| -> Result<f64, Value> {
        match args.get(i) {
            Some(a) => ev.eval_expr(sheet, a).as_number().map_err(Value::Error),
            None => Ok(default),
        }
    };
    let row = match number(0, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let col = match number(1, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let abs = match number(2, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    if row < 1 || col < 1 || !(1..=4).contains(&abs) {
        return Value::Error(ErrorValue::Value);
    }
    // 1 both absolute, 2 row absolute, 3 column absolute, 4 neither.
    let (row_abs, col_abs) = match abs {
        1 => (true, true),
        2 => (true, false),
        3 => (false, true),
        _ => (false, false),
    };
    let letters = casual_calc_formula::column_to_letters((col - 1) as u32);
    let mut out = format!(
        "{}{letters}{}{row}",
        if col_abs { "$" } else { "" },
        if row_abs { "$" } else { "" }
    );
    if let Some(arg) = args.get(4) {
        match ev.eval_expr(sheet, arg) {
            Value::Text(name) if !name.is_empty() => out = format!("{name}!{out}"),
            Value::Error(e) => return Value::Error(e),
            _ => {}
        }
    }
    Value::Text(out)
}

/// `AREAS(reference)` — how many areas a reference names.
///
/// Answered from the expression, since the evaluator resolves a reference to
/// its contents before a function sees it. Without union syntax in the parser
/// every reference is a single area.
pub(crate) fn eval_areas(args: &[Expr]) -> Value {
    match args {
        [Expr::Reference(_) | Expr::Range(..) | Expr::StructuredRef { .. }] => Value::Number(1.0),
        [_] => Value::Error(ErrorValue::Value),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// `LOOKUP(value, vector, [result])` — the vector form.
///
/// Always approximate: it assumes the lookup vector is sorted ascending and
/// returns the last entry not greater than the target. There is no exact-match
/// mode, which is exactly why MATCH/VLOOKUP exist.
pub(crate) fn eval_lookup(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let target = ev.eval_expr(sheet, &args[0]);
    if let Value::Error(e) = target {
        return Value::Error(e);
    }
    let Some(lookup) = range_cells(ev, sheet, &args[1]) else {
        return Value::Error(ErrorValue::Value);
    };
    let result = match args.get(2) {
        Some(a) => match range_cells(ev, sheet, a) {
            Some(cells) => Some(cells),
            None => return Value::Error(ErrorValue::Value),
        },
        None => None,
    };

    let mut best: Option<usize> = None;
    for (i, at) in lookup.1.iter().enumerate() {
        let value = ev.eval_cell(lookup.0, *at);
        if matches!(loose_cmp(&value, &target), Some(Ordering::Greater)) {
            break;
        }
        if loose_cmp(&value, &target).is_some() {
            best = Some(i);
        }
    }
    let Some(index) = best else {
        return Value::Error(ErrorValue::Na);
    };
    match result {
        Some((rs, cells)) => match cells.get(index) {
            Some(at) => ev.eval_cell(rs, *at),
            None => Value::Error(ErrorValue::Na),
        },
        None => ev.eval_cell(lookup.0, lookup.1[index]),
    }
}

/// The cells a range expression covers, in row-major order.
pub(crate) fn range_cells(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    expr: &Expr,
) -> Option<(usize, Vec<CellRef>)> {
    let (target, range) = match expr {
        Expr::Range(a, b) => {
            let target = ev.resolve_sheet(&a.sheet, sheet)?;
            (target, ev.range_bounds(target, a, b))
        }
        Expr::StructuredRef { table, spec } => {
            let (target, range) = ev.resolve_structured(sheet, table.as_deref(), spec)?;
            (
                target,
                (
                    range.start.row,
                    range.start.col,
                    range.end.row,
                    range.end.col,
                ),
            )
        }
        _ => return None,
    };
    let (r0, c0, r1, c1) = range;
    if (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64 > MAX_RANGE_CELLS {
        return None;
    }
    let mut out = Vec::new();
    for row in r0..=r1 {
        for col in c0..=c1 {
            out.push(CellRef::new(row, col));
        }
    }
    Some((target, out))
}

/// `INDIRECT(text)` — read the cell a *string* names.
///
/// The reason it is special: the dependency graph cannot see through it, since
/// the target is only known once the string is evaluated. `graph.rs` therefore
/// treats a formula containing INDIRECT as depending on everything, the same
/// treatment a defined name gets — conservative, and never stale.
pub(crate) fn eval_indirect(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    // The A1/R1C1 flag: only A1 is supported, and FALSE asks for R1C1, so it
    // is refused rather than silently answered in the wrong notation.
    if let Some(arg) = args.get(1) {
        match ev.eval_expr(sheet, arg).as_bool() {
            Ok(true) => {}
            Ok(false) => return Value::Error(ErrorValue::Value),
            Err(e) => return Value::Error(e),
        }
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => n.to_string(),
            Err(e) => return Value::Error(e),
        },
    };
    // A sheet-qualified target resolves through the same path a written
    // reference does, so `INDIRECT("Sheet2!A1")` behaves like `Sheet2!A1`.
    let (sheet_name, cell) = match text.rsplit_once('!') {
        Some((name, cell)) => (Some(name.trim_matches('\'').to_owned()), cell),
        None => (None, text.as_str()),
    };
    let Some(mut reference) = casual_calc_formula::parse_a1(cell) else {
        // A string that is not a reference is #REF!, which is what Excel shows
        // and is distinguishable from the cell simply being empty.
        return Value::Error(ErrorValue::Ref);
    };
    reference.sheet = sheet_name;
    // **Stored against the cell doing the asking.** `INDIRECT("A1")` names an
    // address, and the expression built here is evaluated at *this* cell — so
    // an absolute-form reference would be read as an offset and land somewhere
    // else. Storing it at the origin it will be resolved at is what makes the
    // round trip the identity.
    let reference = casual_calc_formula::stored::ResolvedRef::from(&reference).store(ev.origin());
    ev.eval_expr(sheet, &Expr::Reference(reference))
}

/// `OFFSET(reference, rows, cols, [height], [width])`.
///
/// Returns the single cell when the result is 1×1. A larger result is a range,
/// and a range on its own is `#VALUE!` here exactly as `A1:B2` is — it is the
/// aggregate around it that consumes one.
pub(crate) fn eval_offset(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let Expr::Reference(base) = &args[0] else {
        return Value::Error(ErrorValue::Value);
    };
    let number = |ev: &mut Evaluator<'_>, i: usize, default: f64| -> Result<f64, Value> {
        match args.get(i) {
            Some(a) => ev.eval_expr(sheet, a).as_number().map_err(Value::Error),
            None => Ok(default),
        }
    };
    let rows = match number(ev, 1, 0.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let cols = match number(ev, 2, 0.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let height = match number(ev, 3, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let width = match number(ev, 4, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    if height != 1 || width != 1 {
        return Value::Error(ErrorValue::Value);
    }
    // **Resolved before it is counted from.** `base.row` is an *offset* from
    // the holding cell, not an address (`PERF-11`), and adding `rows` to it
    // computes an offset-plus-a-count that is neither. It compiles, because
    // both are integers — which is exactly why the design says relativity has
    // to be in the type *and* why this one still had to be caught by a test.
    let Some(base_at) = base.resolve(ev.origin()) else {
        return Value::Error(ErrorValue::Ref);
    };
    let row = i64::from(base_at.row) + rows;
    let col = i64::from(base_at.col) + cols;
    if row < 0 || col < 0 {
        // Off the top or left edge of the grid.
        return Value::Error(ErrorValue::Ref);
    }
    // As `INDIRECT`: an address is computed here and read at this cell, so it
    // is stored against this cell rather than absolutely.
    let target = casual_calc_formula::stored::ResolvedRef {
        row: row as u32,
        col: col as u32,
        ..base_at
    }
    .store(ev.origin());
    ev.eval_expr(sheet, &Expr::Reference(target))
}

// --- Text helpers ----------------------------------------------------------
