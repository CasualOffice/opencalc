//! Matrix arithmetic and the dynamic-array functions, including `LAMBDA`'s
//! helpers.
//!
//! Split out of the single `functions.rs` under `MNT-002`; the section
//! headings that file already carried are the seams.

use super::*;

/// The matrix functions that answer with a shape: TRANSPOSE, MMULT, MINVERSE,
/// and FREQUENCY.
///
/// Each returns a [`Value::Array`], which the recalculation pass spills into
/// the cells below and to the right. They could not exist before that pass did
/// — a function that can only return one number cannot return a matrix.
pub(crate) fn grid_numbers(grid: &Grid) -> Result<Vec<f64>, ErrorValue> {
    grid.cells.iter().map(Value::as_number).collect()
}

pub(crate) fn eval_matrix(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    match name {
        "TRANSPOSE" => {
            if args.len() != 1 {
                return Value::Error(ErrorValue::Value);
            }
            let g = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let mut cells = Vec::with_capacity(g.rows * g.cols);
            for c in 0..g.cols {
                for r in 0..g.rows {
                    cells.push(g.get(r, c).clone());
                }
            }
            Value::Array {
                rows: g.cols,
                cols: g.rows,
                cells,
            }
        }
        "MMULT" => {
            if args.len() != 2 {
                return Value::Error(ErrorValue::Value);
            }
            let (a, b) = match (
                eval_range_2d(ev, sheet, &args[0]),
                eval_range_2d(ev, sheet, &args[1]),
            ) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return Value::Error(e),
            };
            // The inner dimensions must agree; Excel answers #VALUE! rather
            // than padding, and padding would invent data.
            if a.cols != b.rows || a.rows == 0 || b.cols == 0 {
                return Value::Error(ErrorValue::Value);
            }
            let (av, bv) = match (grid_numbers(&a), grid_numbers(&b)) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(e), _) | (_, Err(e)) => return Value::Error(e),
            };
            let mut cells = Vec::with_capacity(a.rows * b.cols);
            for r in 0..a.rows {
                for c in 0..b.cols {
                    let mut sum = 0.0;
                    for k in 0..a.cols {
                        sum += av[r * a.cols + k] * bv[k * b.cols + c];
                    }
                    cells.push(Value::Number(sum));
                }
            }
            Value::Array {
                rows: a.rows,
                cols: b.cols,
                cells,
            }
        }
        "MINVERSE" => {
            if args.len() != 1 {
                return Value::Error(ErrorValue::Value);
            }
            let g = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let n = g.rows;
            if n == 0 || n != g.cols {
                return Value::Error(ErrorValue::Value);
            }
            let mut m = match grid_numbers(&g) {
                Ok(v) => v,
                Err(e) => return Value::Error(e),
            };
            // Gauss–Jordan on [M | I], with partial pivoting for the same
            // reason MDETERM uses it: a small leading entry amplifies error.
            let mut inv = vec![0.0; n * n];
            for i in 0..n {
                inv[i * n + i] = 1.0;
            }
            for col in 0..n {
                let mut pivot = col;
                for r in (col + 1)..n {
                    if m[r * n + col].abs() > m[pivot * n + col].abs() {
                        pivot = r;
                    }
                }
                if m[pivot * n + col] == 0.0 {
                    // Singular: there is no inverse, and #NUM! says so rather
                    // than returning a matrix of infinities.
                    return Value::Error(ErrorValue::Num);
                }
                if pivot != col {
                    for c in 0..n {
                        m.swap(col * n + c, pivot * n + c);
                        inv.swap(col * n + c, pivot * n + c);
                    }
                }
                let d = m[col * n + col];
                for c in 0..n {
                    m[col * n + c] /= d;
                    inv[col * n + c] /= d;
                }
                for r in 0..n {
                    if r == col {
                        continue;
                    }
                    let factor = m[r * n + col];
                    if factor == 0.0 {
                        continue;
                    }
                    for c in 0..n {
                        m[r * n + c] -= factor * m[col * n + c];
                        inv[r * n + c] -= factor * inv[col * n + c];
                    }
                }
            }
            Value::Array {
                rows: n,
                cols: n,
                cells: inv.into_iter().map(Value::Number).collect(),
            }
        }
        "FREQUENCY" => {
            if args.len() != 2 {
                return Value::Error(ErrorValue::Value);
            }
            let data = match flatten_numbers(ev, sheet, &args[..1]) {
                Ok(v) => v,
                Err(e) => return Value::Error(e),
            };
            let mut bins = match flatten_numbers(ev, sheet, &args[1..2]) {
                Ok(v) => v,
                Err(e) => return Value::Error(e),
            };
            bins.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            // One more bucket than there are bins: everything above the last
            // bin still has to land somewhere, and dropping it would make the
            // counts not sum to the data.
            let mut counts = vec![0.0; bins.len() + 1];
            for v in data {
                let idx = bins.iter().position(|b| v <= *b).unwrap_or(bins.len());
                counts[idx] += 1.0;
            }
            Value::Array {
                rows: counts.len(),
                cols: 1,
                cells: counts.into_iter().map(Value::Number).collect(),
            }
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// A fitted least-squares model, and the statistics `LINEST` reports about it.
pub(crate) struct Regression {
    /// `[intercept, m1, m2, …]`. Excel reports the slopes in *reverse* order
    /// with the intercept last, which is a presentation choice rather than a
    /// mathematical one, so the reversal happens where the array is built.
    coeffs: Vec<f64>,
    /// Standard error of each coefficient, in the same order.
    se: Vec<f64>,
    r2: f64,
    se_y: f64,
    f: f64,
    df: f64,
    ss_reg: f64,
    ss_resid: f64,
}

/// Ordinary least squares of `y` on the predictor columns in `x`.
///
/// Solved through the normal equations with Gaussian elimination. That is less
/// numerically careful than a QR decomposition, and adequate here: a
/// spreadsheet regression is a handful of predictors over a few hundred rows,
/// where the conditioning that would justify QR does not arise.
///
/// `intercept = false` forces the line through the origin, which changes the
/// degrees of freedom as well as the fit — a detail that silently corrupts R²
/// if it is missed.
pub(crate) fn least_squares(y: &[f64], x: &[Vec<f64>], intercept: bool) -> Option<Regression> {
    let n = y.len();
    let k = x.len();
    if n == 0 || x.iter().any(|col| col.len() != n) {
        return None;
    }
    let terms = k + usize::from(intercept);
    if terms == 0 || n < terms {
        return None;
    }

    // The design matrix, constant column first when there is an intercept.
    let design = |row: usize, col: usize| -> f64 {
        if intercept {
            if col == 0 { 1.0 } else { x[col - 1][row] }
        } else {
            x[col][row]
        }
    };

    // Normal equations: (XᵀX) b = Xᵀy.
    let mut a = vec![0.0; terms * terms];
    let mut rhs = vec![0.0; terms];
    for i in 0..terms {
        for j in 0..terms {
            a[i * terms + j] = (0..n).map(|r| design(r, i) * design(r, j)).sum();
        }
        rhs[i] = (0..n).map(|r| design(r, i) * y[r]).sum();
    }

    // Gauss–Jordan on [A | I | rhs], keeping the inverse because the standard
    // errors need its diagonal.
    let mut inv = vec![0.0; terms * terms];
    for i in 0..terms {
        inv[i * terms + i] = 1.0;
    }
    for col in 0..terms {
        let mut pivot = col;
        for r in (col + 1)..terms {
            if a[r * terms + col].abs() > a[pivot * terms + col].abs() {
                pivot = r;
            }
        }
        if a[pivot * terms + col].abs() < 1e-300 {
            return None; // collinear predictors: no unique fit
        }
        if pivot != col {
            for c in 0..terms {
                a.swap(col * terms + c, pivot * terms + c);
                inv.swap(col * terms + c, pivot * terms + c);
            }
            rhs.swap(col, pivot);
        }
        let d = a[col * terms + col];
        for c in 0..terms {
            a[col * terms + c] /= d;
            inv[col * terms + c] /= d;
        }
        rhs[col] /= d;
        for r in 0..terms {
            if r == col {
                continue;
            }
            let factor = a[r * terms + col];
            if factor == 0.0 {
                continue;
            }
            for c in 0..terms {
                a[r * terms + c] -= factor * a[col * terms + c];
                inv[r * terms + c] -= factor * inv[col * terms + c];
            }
            rhs[r] -= factor * rhs[col];
        }
    }
    let beta = rhs;

    let predict = |row: usize| -> f64 { (0..terms).map(|c| beta[c] * design(row, c)).sum() };
    let ss_resid: f64 = (0..n).map(|r| (y[r] - predict(r)).powi(2)).sum();
    // Without an intercept the total sum of squares is measured about zero,
    // not about the mean — using the mean there reports an R² that can be
    // negative or above one.
    let mean = y.iter().sum::<f64>() / n as f64;
    let ss_total: f64 = if intercept {
        y.iter().map(|v| (v - mean).powi(2)).sum()
    } else {
        y.iter().map(|v| v * v).sum()
    };
    let ss_reg = (ss_total - ss_resid).max(0.0);
    let df = (n - terms) as f64;
    let se_y = if df > 0.0 {
        (ss_resid / df).sqrt()
    } else {
        0.0
    };
    let r2 = if ss_total > 0.0 {
        ss_reg / ss_total
    } else {
        1.0
    };
    let predictors = (terms - usize::from(intercept)).max(1) as f64;
    let f = if df > 0.0 && ss_resid > 0.0 {
        (ss_reg / predictors) / (ss_resid / df)
    } else {
        f64::INFINITY
    };
    let se: Vec<f64> = (0..terms)
        .map(|i| (inv[i * terms + i].max(0.0) * se_y * se_y).sqrt())
        .collect();

    // Internally `[intercept, slopes…]` when there is one; callers that want
    // Excel's order reverse at the boundary.
    let coeffs = if intercept {
        beta
    } else {
        std::iter::once(0.0).chain(beta).collect()
    };
    let se = if intercept {
        se
    } else {
        std::iter::once(0.0).chain(se).collect()
    };
    Some(Regression {
        coeffs,
        se,
        r2,
        se_y,
        f,
        df,
        ss_reg,
        ss_resid,
    })
}

/// Gather `known_y` and `known_x` as columns.
///
/// A missing `known_x` is `{1, 2, 3, …}`, which is what makes `TREND(y)` mean
/// "fit against position". Multiple predictors come from a range whose *other*
/// axis is the shorter one — Excel decides orientation by shape, and so does
/// this, because a file written elsewhere relies on it.
pub(crate) fn regression_inputs(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    y_arg: &Expr,
    x_arg: Option<&Expr>,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), ErrorValue> {
    let ys = flatten_numbers(ev, sheet, std::slice::from_ref(y_arg))?;
    if ys.is_empty() {
        return Err(ErrorValue::Value);
    }
    let Some(x_arg) = x_arg else {
        return Ok((ys.clone(), vec![(1..=ys.len()).map(|i| i as f64).collect()]));
    };
    let grid = eval_range_2d(ev, sheet, x_arg)?;
    let values: Vec<f64> = grid
        .cells
        .iter()
        .map(Value::as_number)
        .collect::<Result<_, _>>()?;
    if values.len() == ys.len() {
        return Ok((ys, vec![values]));
    }
    // More x values than y: several predictors, laid out along whichever axis
    // matches the y count.
    if grid.rows == ys.len() && grid.cols > 0 {
        let cols = (0..grid.cols)
            .map(|c| (0..grid.rows).map(|r| values[r * grid.cols + c]).collect())
            .collect();
        return Ok((ys, cols));
    }
    if grid.cols == ys.len() && grid.rows > 0 {
        let cols = (0..grid.rows)
            .map(|r| (0..grid.cols).map(|c| values[r * grid.cols + c]).collect())
            .collect();
        return Ok((ys, cols));
    }
    Err(ErrorValue::Ref)
}

/// `LINEST`, `LOGEST`, `TREND` and `GROWTH`.
///
/// The exponential pair are the linear pair fitted to `ln(y)`: `y = b·m^x`
/// becomes `ln y = ln b + x·ln m`. That is why a non-positive `y` is `#NUM!`
/// rather than being skipped — its logarithm does not exist, and dropping the
/// point would fit a different dataset than the one given.
pub(crate) fn eval_regression(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    if args.is_empty() || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let logarithmic = matches!(name, "LOGEST" | "GROWTH");
    let estimating = matches!(name, "LINEST" | "LOGEST");

    let x_arg = args.get(1);
    let (mut ys, xs) = match regression_inputs(ev, sheet, &args[0], x_arg) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if logarithmic {
        if ys.iter().any(|v| *v <= 0.0) {
            return Value::Error(ErrorValue::Num);
        }
        ys = ys.into_iter().map(f64::ln).collect();
    }

    // Third argument: LINEST/LOGEST take `const`, TREND/GROWTH take `new_x`.
    let const_arg = if estimating { args.get(2) } else { args.get(3) };
    let intercept = match const_arg {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(v) => v,
            Err(e) => return Value::Error(e),
        },
        None => true,
    };

    let Some(fit) = least_squares(&ys, &xs, intercept) else {
        return Value::Error(ErrorValue::Num);
    };

    if estimating {
        let stats = match args.get(3) {
            Some(a) => ev.eval_expr(sheet, a).as_bool().unwrap_or(false),
            None => false,
        };
        // Excel's order: slopes reversed, intercept last.
        let mut row: Vec<f64> = fit.coeffs[1..].iter().rev().copied().collect();
        row.push(fit.coeffs[0]);
        if logarithmic {
            // The fit is on ln(y), so the coefficients come back through exp.
            row = row.into_iter().map(f64::exp).collect();
        }
        let width = row.len();
        if !stats {
            return Value::Array {
                rows: 1,
                cols: width,
                cells: row.into_iter().map(Value::Number).collect(),
            };
        }
        let mut se: Vec<f64> = fit.se[1..].iter().rev().copied().collect();
        se.push(fit.se[0]);
        // The 5×n block. Cells with no meaning in a given column are #N/A,
        // which is what Excel puts there — a zero would read as a measurement.
        let mut cells: Vec<Value> = Vec::with_capacity(5 * width);
        cells.extend(row.iter().map(|v| Value::Number(*v)));
        cells.extend(se.iter().map(|v| Value::Number(*v)));
        let mut push_pair = |a: f64, b: f64| {
            cells.push(Value::Number(a));
            cells.push(Value::Number(b));
            for _ in 2..width {
                cells.push(Value::Error(ErrorValue::Na));
            }
        };
        push_pair(fit.r2, fit.se_y);
        push_pair(fit.f, fit.df);
        push_pair(fit.ss_reg, fit.ss_resid);
        // A single-column fit has no room for the second statistic of each
        // pair, so the block is trimmed to what fits.
        cells.truncate(5 * width);
        return Value::Array {
            rows: 5,
            cols: width,
            cells,
        };
    }

    // TREND / GROWTH: predict at `new_x`, defaulting to the fitted points.
    let new_x: Vec<Vec<f64>> = match args.get(2) {
        Some(a) => match regression_inputs(ev, sheet, &args[0], Some(a)) {
            Ok((_, cols)) => cols,
            Err(_) => {
                // A new_x of a different length than known_y is normal — it is
                // the whole point of predicting — so gather it on its own.
                match flatten_numbers(ev, sheet, std::slice::from_ref(a)) {
                    Ok(v) => vec![v],
                    Err(e) => return Value::Error(e),
                }
            }
        },
        None => xs.clone(),
    };
    let points = new_x.first().map_or(0, Vec::len);
    if points == 0 || new_x.len() != xs.len() {
        return Value::Error(ErrorValue::Ref);
    }
    let cells: Vec<Value> = (0..points)
        .map(|i| {
            let mut v = fit.coeffs[0];
            for (j, col) in new_x.iter().enumerate() {
                v += fit.coeffs[j + 1] * col[i];
            }
            Value::Number(if logarithmic { v.exp() } else { v })
        })
        .collect();
    Value::Array {
        rows: points,
        cols: 1,
        cells,
    }
}

pub(crate) fn eval_dynamic(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    match name {
        "SEQUENCE" => {
            if args.is_empty() || args.len() > 4 {
                return Value::Error(ErrorValue::Value);
            }
            let v = match opt_numbers(ev, sheet, args, 1, [1.0, 1.0, 1.0, 1.0]) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let (rows, cols, start, step) = (v[0] as i64, v[1] as i64, v[2], v[3]);
            if rows < 1 || cols < 1 {
                return Value::Error(ErrorValue::Value);
            }
            let cells = (0..rows * cols)
                .map(|i| Value::Number(start + step * i as f64))
                .collect();
            Value::Array {
                rows: rows as usize,
                cols: cols as usize,
                cells,
            }
        }
        "XMATCH" | "XLOOKUP" => {
            let min = if name == "XMATCH" { 2 } else { 3 };
            if args.len() < min || args.len() > min + 3 {
                return Value::Error(ErrorValue::Value);
            }
            let needle = ev.eval_expr(sheet, &args[0]);
            if let Value::Error(e) = needle {
                return Value::Error(e);
            }
            let haystack = match eval_range_2d(ev, sheet, &args[1]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let flat: Vec<Value> = haystack.cells.clone();
            let opt = |ev: &mut Evaluator<'_>, i: usize, dflt: f64| -> f64 {
                args.get(i)
                    .map(|a| ev_number_or(ev, sheet, a, dflt))
                    .unwrap_or(dflt)
            };
            // Argument positions differ: XMATCH has no return array or
            // not-found value, so its modes sit two places earlier.
            let (mode_at, search_at) = if name == "XMATCH" { (2, 3) } else { (4, 5) };
            let match_mode = opt(ev, mode_at, 0.0) as i64;
            let search_mode = opt(ev, search_at, 1.0) as i64;
            let found = lookup_index(&flat, &needle, match_mode, search_mode);

            if name == "XMATCH" {
                return match found {
                    Some(i) => Value::Number(i as f64 + 1.0),
                    None => Value::Error(ErrorValue::Na),
                };
            }
            let Some(i) = found else {
                // `if_not_found` is the fourth argument, and its whole purpose
                // is to replace the #N/A — so it is only consulted here.
                return match args.get(3) {
                    Some(a) => ev.eval_expr(sheet, a),
                    None => Value::Error(ErrorValue::Na),
                };
            };
            let ret = match eval_range_2d(ev, sheet, &args[2]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            // The return array may be wider than the lookup column, in which
            // case XLOOKUP answers with a whole row — that is what makes it a
            // replacement for INDEX/MATCH rather than for VLOOKUP alone.
            if ret.rows == flat.len() && ret.cols >= 1 {
                let row: Vec<Value> = (0..ret.cols).map(|c| ret.get(i, c).clone()).collect();
                return rows_to_value(vec![row]);
            }
            if ret.cols == flat.len() && ret.rows >= 1 {
                let col: Vec<Vec<Value>> =
                    (0..ret.rows).map(|r| vec![ret.get(r, i).clone()]).collect();
                return rows_to_value(col);
            }
            Value::Error(ErrorValue::Value)
        }
        "FILTER" => {
            if args.len() < 2 || args.len() > 3 {
                return Value::Error(ErrorValue::Value);
            }
            let data = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let mask = match eval_range_2d(ev, sheet, &args[1]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let keep: Vec<bool> = mask
                .cells
                .iter()
                .map(|v| v.as_bool().unwrap_or(false))
                .collect();
            // The mask runs along whichever axis it matches; a mask that
            // matches neither is a mistake worth reporting.
            let rows = grid_rows(&data);
            let picked: Vec<Vec<Value>> = if keep.len() == data.rows {
                rows.into_iter()
                    .enumerate()
                    .filter(|(i, _)| keep[*i])
                    .map(|(_, r)| r)
                    .collect()
            } else if keep.len() == data.cols {
                rows.into_iter()
                    .map(|r| {
                        r.into_iter()
                            .enumerate()
                            .filter(|(i, _)| keep[*i])
                            .map(|(_, v)| v)
                            .collect()
                    })
                    .collect()
            } else {
                return Value::Error(ErrorValue::Value);
            };
            if picked.is_empty() || picked[0].is_empty() {
                return match args.get(2) {
                    Some(a) => ev.eval_expr(sheet, a),
                    // Excel's own answer when nothing matches and no
                    // replacement was given.
                    None => Value::Error(ErrorValue::Calc),
                };
            }
            rows_to_value(picked)
        }
        "UNIQUE" => {
            if args.is_empty() || args.len() > 3 {
                return Value::Error(ErrorValue::Value);
            }
            let data = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let exactly_once = args
                .get(2)
                .map(|a| ev.eval_expr(sheet, a).as_bool().unwrap_or(false))
                .unwrap_or(false);
            let rows = grid_rows(&data);
            let key = |r: &Vec<Value>| -> String {
                r.iter()
                    .map(|v| v.as_text().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\u{1}")
            };
            let mut counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for r in &rows {
                *counts.entry(key(r)).or_default() += 1;
            }
            let mut seen: std::collections::HashSet<String> = Default::default();
            let picked: Vec<Vec<Value>> = rows
                .into_iter()
                .filter(|r| {
                    let k = key(r);
                    if exactly_once && counts.get(&k) != Some(&1) {
                        return false;
                    }
                    seen.insert(k)
                })
                .collect();
            if picked.is_empty() {
                return Value::Error(ErrorValue::Calc);
            }
            rows_to_value(picked)
        }
        "SORT" | "SORTBY" => {
            if args.is_empty() {
                return Value::Error(ErrorValue::Value);
            }
            let data = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let mut rows = grid_rows(&data);
            // SORT keys on one of its own columns; SORTBY on a parallel array.
            let (keys, descending) = if name == "SORT" {
                let index = args
                    .get(1)
                    .map(|a| ev_number_or(ev, sheet, a, 1.0))
                    .unwrap_or(1.0) as usize;
                if index < 1 || index > data.cols {
                    return Value::Error(ErrorValue::Value);
                }
                let order = args
                    .get(2)
                    .map(|a| ev_number_or(ev, sheet, a, 1.0))
                    .unwrap_or(1.0);
                (
                    rows.iter()
                        .map(|r| r[index - 1].clone())
                        .collect::<Vec<_>>(),
                    order < 0.0,
                )
            } else {
                let by = match eval_range_2d(ev, sheet, &args[1]) {
                    Ok(g) => g,
                    Err(e) => return Value::Error(e),
                };
                if by.cells.len() != rows.len() {
                    return Value::Error(ErrorValue::Value);
                }
                let order = args
                    .get(2)
                    .map(|a| ev_number_or(ev, sheet, a, 1.0))
                    .unwrap_or(1.0);
                (by.cells.clone(), order < 0.0)
            };
            // Sorted by index so the comparison can read the key list, and
            // stably — equal keys keep their original order, which is what
            // makes a second SORTBY pass meaningful.
            let mut order: Vec<usize> = (0..rows.len()).collect();
            order.sort_by(|a, b| {
                let c = lookup_compare(&keys[*a], &keys[*b]);
                if descending { c.reverse() } else { c }
            });
            let sorted: Vec<Vec<Value>> = order.into_iter().map(|i| rows[i].clone()).collect();
            rows.clear();
            rows_to_value(sorted)
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// An argument as a number, falling back when it is absent or unreadable.
pub(crate) fn ev_number_or(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr, dflt: f64) -> f64 {
    ev.eval_expr(sheet, arg).as_number().unwrap_or(dflt)
}

/// The LAMBDA helpers: MAP, REDUCE, SCAN, BYROW, BYCOL, MAKEARRAY, ISOMITTED.
///
/// These are why LAMBDA is a language feature rather than a curiosity — a
/// user-defined function you cannot hand to anything can only be called by
/// name. Each takes a function *value*, so they work with an inline LAMBDA, a
/// named one, or one returned by another lambda.
pub(crate) fn lambda_arg(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    arg: &Expr,
) -> Option<std::rc::Rc<crate::value::LambdaValue>> {
    match ev.eval_expr_array(sheet, arg) {
        Value::Lambda(f) => Some(f),
        _ => None,
    }
}

pub(crate) fn eval_lambda_helper(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    name: &str,
    args: &[Expr],
) -> Value {
    match name {
        "ISOMITTED" => {
            // True only for an argument that was left out, which is the one
            // question a lambda cannot otherwise ask about its own call.
            Value::Bool(matches!(args.first(), Some(Expr::Empty) | None))
        }
        "MAKEARRAY" => {
            if args.len() != 3 {
                return Value::Error(ErrorValue::Value);
            }
            let rows = ev.eval_expr(sheet, &args[0]).as_number().unwrap_or(0.0) as i64;
            let cols = ev.eval_expr(sheet, &args[1]).as_number().unwrap_or(0.0) as i64;
            if rows < 1 || cols < 1 || rows * cols > 1_000_000 {
                return Value::Error(ErrorValue::Value);
            }
            let Some(f) = lambda_arg(ev, sheet, &args[2]) else {
                return Value::Error(ErrorValue::Value);
            };
            let mut cells = Vec::with_capacity((rows * cols) as usize);
            for r in 1..=rows {
                for c in 1..=cols {
                    // One-based, because the lambda is written in spreadsheet
                    // terms and ROW()/COLUMN() are one-based too.
                    cells.push(ev.apply_lambda_values(
                        sheet,
                        &f,
                        vec![Value::Number(r as f64), Value::Number(c as f64)],
                    ));
                }
            }
            Value::Array {
                rows: rows as usize,
                cols: cols as usize,
                cells,
            }
        }
        "MAP" => {
            if args.len() < 2 {
                return Value::Error(ErrorValue::Value);
            }
            let grid = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let Some(f) = lambda_arg(ev, sheet, &args[args.len() - 1]) else {
                return Value::Error(ErrorValue::Value);
            };
            let cells: Vec<Value> = grid
                .cells
                .iter()
                .map(|v| ev.apply_lambda_values(sheet, &f, vec![v.clone()]))
                .collect();
            Value::Array {
                rows: grid.rows,
                cols: grid.cols,
                cells,
            }
        }
        "REDUCE" | "SCAN" => {
            if args.len() != 3 {
                return Value::Error(ErrorValue::Value);
            }
            let initial = ev.eval_expr(sheet, &args[0]);
            let grid = match eval_range_2d(ev, sheet, &args[1]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let Some(f) = lambda_arg(ev, sheet, &args[2]) else {
                return Value::Error(ErrorValue::Value);
            };
            let mut acc = initial;
            let mut steps = Vec::with_capacity(grid.cells.len());
            for v in &grid.cells {
                acc = ev.apply_lambda_values(sheet, &f, vec![acc.clone(), v.clone()]);
                steps.push(acc.clone());
            }
            // REDUCE answers with the final accumulator; SCAN with every one it
            // passed through, which is what makes a running total possible.
            if name == "REDUCE" {
                acc
            } else {
                Value::Array {
                    rows: grid.rows,
                    cols: grid.cols,
                    cells: steps,
                }
            }
        }
        "BYROW" | "BYCOL" => {
            if args.len() != 2 {
                return Value::Error(ErrorValue::Value);
            }
            let grid = match eval_range_2d(ev, sheet, &args[0]) {
                Ok(g) => g,
                Err(e) => return Value::Error(e),
            };
            let Some(f) = lambda_arg(ev, sheet, &args[1]) else {
                return Value::Error(ErrorValue::Value);
            };
            let by_row = name == "BYROW";
            let outer = if by_row { grid.rows } else { grid.cols };
            let inner = if by_row { grid.cols } else { grid.rows };
            let cells: Vec<Value> = (0..outer)
                .map(|i| {
                    // Each slice is handed over as an array, so the lambda can
                    // use SUM or any other aggregate on it.
                    let slice: Vec<Value> = (0..inner)
                        .map(|j| {
                            if by_row {
                                grid.get(i, j).clone()
                            } else {
                                grid.get(j, i).clone()
                            }
                        })
                        .collect();
                    let arg = Value::Array {
                        rows: if by_row { 1 } else { inner },
                        cols: if by_row { inner } else { 1 },
                        cells: slice,
                    };
                    ev.apply_lambda_values(sheet, &f, vec![arg])
                })
                .collect();
            // A row-wise result is a column of answers, and vice versa.
            if by_row {
                Value::Array {
                    rows: outer,
                    cols: 1,
                    cells,
                }
            } else {
                Value::Array {
                    rows: 1,
                    cols: outer,
                    cells,
                }
            }
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// `GETPIVOTDATA(data_field, pivot_table, [field, item], …)`.
///
/// `pivot_table` is a *reference* rather than a value — any cell inside the
/// report — so the argument is read from the AST, as `OFFSET` reads its base.
/// Evaluating it first would hand back whatever number happens to be in that
/// cell, which is exactly the figure the function is supposed to find by name.
///
/// Excel writes this formula for you when you click a pivot cell while building
/// one, and the reason is worth keeping in mind: `=D7` breaks the moment the
/// report grows a row, while this keeps pointing at the same *figure*.
///
/// With no field/item pairs the answer is the grand total — the same query with
/// every group left open.
pub(crate) fn eval_getpivotdata(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    // The pairs are pairs, so an odd tail means one is half-written.
    if args.len() < 2 || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    let measure = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(text) => text,
        Err(e) => return Value::Error(e),
    };
    let Expr::Reference(anchor) = &args[1] else {
        return Value::Error(ErrorValue::Ref);
    };
    let Some(at_sheet) = ev.resolve_sheet(&anchor.sheet, sheet) else {
        return Value::Error(ErrorValue::Ref);
    };
    let Some(anchor) = anchor.resolve(ev.origin()) else {
        return Value::Error(ErrorValue::Ref);
    };
    let workbook = ev.workbook();
    let Some(pivot) = workbook.sheets.get(at_sheet).and_then(|sh| {
        sh.pivots.iter().find(|p| {
            p.output.is_some_and(|r| {
                anchor.row >= r.start.row
                    && anchor.row <= r.end.row
                    && anchor.col >= r.start.col
                    && anchor.col <= r.end.col
            })
        })
    }) else {
        // Not a pivot cell. `#REF!` rather than the cell's own value: pointing
        // this at the wrong place is a mistake to report, not to paper over.
        return Value::Error(ErrorValue::Ref);
    };

    let mut criteria: Vec<(String, String)> = Vec::new();
    for pair in args[2..].chunks(2) {
        let field = match ev.eval_expr(sheet, &pair[0]).as_text() {
            Ok(text) => text,
            Err(e) => return Value::Error(e),
        };
        let item = match ev.eval_expr(sheet, &pair[1]).as_text() {
            Ok(text) => text,
            Err(e) => return Value::Error(e),
        };
        criteria.push((field, item));
    }

    match crate::pivot::lookup(workbook, pivot, &measure, &criteria) {
        Ok(value) => value,
        Err(e) => Value::Error(e),
    }
}
