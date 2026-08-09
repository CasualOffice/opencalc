//! The expression evaluator: an AST + a workbook context → a [`Value`].
//!
//! References are resolved by (memoized) recursive evaluation, so a formula that
//! reads another formula computes it on demand. A re-entrant cell is a circular
//! reference and yields an error.

use std::collections::{HashMap, HashSet};

use casual_calc_formula::{BinaryOp, CellReference, Expr, UnaryOp};
use casual_calc_model::{CellRange, CellRef, ErrorValue, Workbook};

use crate::functions::call_function;
use crate::value::{Value, value_from_cell};

type CellKey = (usize, u32, u32);

/// Evaluates cells and expressions against a workbook, memoizing results.
///
/// When `dirty` is set (incremental recalc), a formula cell **outside** the
/// dirty set is read from its cached value rather than re-evaluated — its
/// inputs did not change, so its cache is authoritative. This is what makes an
/// incremental pass touch only the changed cell's transitive dependents while
/// producing values identical to a full recalc.
#[derive(Debug)]
pub struct Evaluator<'a> {
    workbook: &'a Workbook,
    memo: HashMap<CellKey, Value>,
    in_progress: HashSet<CellKey>,
    dirty: Option<&'a HashSet<CellKey>>,
    /// The cell whose formula is currently being evaluated — what `ROW()` /
    /// `COLUMN()` with no argument report. Saved and restored around each
    /// formula so a referenced cell's own formula sees its own address.
    current: Option<(usize, CellRef)>,
    /// How many random draws have been made this pass.
    ///
    /// Mixed into the seed so two `RAND()` calls in one formula differ, which a
    /// seed alone cannot do. Held here rather than on the workbook because the
    /// evaluator is the thing with `&mut self` during evaluation — and because
    /// a counter that resets per pass is what makes a recalculation
    /// reproducible from its seed.
    rand_counter: u64,
}

impl<'a> Evaluator<'a> {
    /// A new evaluator over `workbook` that evaluates every cell (full recalc).
    pub fn new(workbook: &'a Workbook) -> Self {
        Self {
            workbook,
            memo: HashMap::new(),
            in_progress: HashSet::new(),
            dirty: None,
            current: None,
            rand_counter: 0,
        }
    }

    /// A new evaluator that recomputes only formula cells in `dirty` and reads
    /// cached values for all others (incremental recalc).
    pub fn with_dirty(workbook: &'a Workbook, dirty: &'a HashSet<CellKey>) -> Self {
        Self {
            workbook,
            memo: HashMap::new(),
            in_progress: HashSet::new(),
            dirty: Some(dirty),
            current: None,
            rand_counter: 0,
        }
    }

    /// The workbook being evaluated.
    pub fn workbook(&self) -> &'a Workbook {
        self.workbook
    }

    /// Evaluate the cell at `(sheet_index, at)` to a value.
    /// A formula cell's value in a scalar context; an array collapses to its
    /// corner, so a cell that *references* a spilling formula reads one value.
    pub fn eval_cell(&mut self, sheet_index: usize, at: CellRef) -> Value {
        self.eval_cell_array(sheet_index, at).scalar()
    }

    /// A formula cell's value with an array kept whole — what the spilling pass
    /// needs, and the only caller that wants it.
    pub fn eval_cell_array(&mut self, sheet_index: usize, at: CellRef) -> Value {
        let key = (sheet_index, at.row, at.col);
        if let Some(value) = self.memo.get(&key) {
            return value.clone();
        }
        if !self.in_progress.insert(key) {
            // Circular reference (iterative calc is not enabled).
            return Value::Error(ErrorValue::Ref);
        }
        let value = self.compute_cell(sheet_index, at);
        self.in_progress.remove(&key);
        self.memo.insert(key, value.clone());
        value
    }

    fn compute_cell(&mut self, sheet_index: usize, at: CellRef) -> Value {
        let Some(sheet) = self.workbook.sheets.get(sheet_index) else {
            return Value::Error(ErrorValue::Ref);
        };
        let Some(cell) = sheet.cells.get(at) else {
            return Value::Empty;
        };
        if let Some(handle) = cell.formula {
            // Incremental: a formula cell outside the dirty set keeps its cached
            // value (its precedents are unchanged, so its cache is correct).
            if let Some(dirty) = self.dirty
                && !dirty.contains(&(sheet_index, at.row, at.col))
            {
                return value_from_cell(&cell.value, &self.workbook.strings);
            }
            return match self.workbook.formula(handle) {
                Some(expr) => {
                    let previous = self.current;
                    self.current = Some((sheet_index, at));
                    let value = self.eval_expr_array(sheet_index, expr);
                    self.current = previous;
                    value
                }
                None => Value::Empty,
            };
        }
        value_from_cell(&cell.value, &self.workbook.strings)
    }

    /// The cell whose formula is currently being evaluated (for `ROW`/`COLUMN`
    /// with no argument), or `None` at the top level.
    pub fn current_cell(&self) -> Option<(usize, CellRef)> {
        self.current
    }

    /// Resolve a structured reference to the range it names.
    ///
    /// Returns `None` when no table of that name exists or the specifier names
    /// no column — the caller reports `#REF!`, which is what Excel shows for a
    /// reference to a table that has been deleted.
    pub fn resolve_structured(
        &self,
        sheet_index: usize,
        table: Option<&str>,
        spec: &str,
    ) -> Option<(usize, CellRange)> {
        // An unqualified `[Column]` means the table containing this formula.
        let (si, found) = match table {
            Some(name) => self.workbook.sheets.iter().enumerate().find_map(|(i, s)| {
                s.tables
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(name))
                    .map(|t| (i, t))
            })?,
            None => {
                let (_, at) = self.current_cell()?;
                let sheet = self.workbook.sheets.get(sheet_index)?;
                let table = sheet.tables.iter().find(|t| {
                    at.row >= t.range.start.row
                        && at.row <= t.range.end.row
                        && at.col >= t.range.start.col
                        && at.col <= t.range.end.col
                })?;
                (sheet_index, table)
            }
        };

        let spec = spec.trim();
        // The data body: everything but the header and totals rows. That is
        // what a bare `Table[Column]` means, and getting it wrong silently
        // includes the header text or the totals row in every aggregate.
        let first_data = found.range.start.row + found.header_row_count;
        let last_data = found.range.end.row.saturating_sub(found.totals_row_count);

        let (top, bottom) = match spec {
            "#All" => (found.range.start.row, found.range.end.row),
            "#Data" | "" => (first_data, last_data),
            "#Headers" => (found.range.start.row, first_data.saturating_sub(1)),
            "#Totals" => (last_data + 1, found.range.end.row),
            _ => (first_data, last_data),
        };

        // A column name narrows the span; a `#` keyword covers every column.
        let (left, right) = if spec.starts_with('#') || spec.is_empty() {
            (found.range.start.col, found.range.end.col)
        } else {
            let index = found
                .columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(spec))?;
            let col = found.range.start.col + index as u32;
            (col, col)
        };
        Some((
            si,
            CellRange {
                start: CellRef::new(top, left),
                end: CellRef::new(bottom, right),
            },
        ))
    }

    /// Evaluate an expression in the context of `sheet_index`.
    /// Evaluate an expression in a **scalar** context.
    ///
    /// An array result collapses to its top-left element — Excel's implicit
    /// intersection. Doing it here rather than at every call site is what keeps
    /// arrays from leaking into the hundred places that only ever wanted a
    /// number; only [`Self::eval_expr_array`] sees the whole block.
    pub fn eval_expr(&mut self, sheet_index: usize, expr: &Expr) -> Value {
        self.eval_expr_array(sheet_index, expr).scalar()
    }

    /// Evaluate an expression, keeping an array result whole. Used by the
    /// spilling pass and by the functions that consume a shape.
    pub fn eval_expr_array(&mut self, sheet_index: usize, expr: &Expr) -> Value {
        match expr {
            Expr::Number(n) => Value::Number(*n),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Text(s) => Value::Text(s.clone()),
            Expr::Error(token) => Value::Error(error_from_token(token)),
            Expr::Reference(reference) => self.eval_reference(sheet_index, reference),
            Expr::Range(..) => Value::Error(ErrorValue::Value),
            // Preserved text this parser cannot read. `#NAME?` is the honest
            // answer: the reference exists in the file but means nothing here,
            // and inventing a value would be worse than saying so.
            Expr::Raw(_) => Value::Error(ErrorValue::Name),
            Expr::Name(name) => self.eval_name(sheet_index, name),
            // A structured reference resolves to a range, so on its own it is
            // as much a #VALUE! as `A1:B2` is; it is the aggregate around it
            // that consumes the range.
            Expr::StructuredRef { .. } => Value::Error(ErrorValue::Value),
            Expr::Unary { op, operand } => self.eval_unary(sheet_index, *op, operand),
            Expr::Binary { op, left, right } => self.eval_binary(sheet_index, *op, left, right),
            Expr::Function { name, args } => call_function(self, sheet_index, name, args),
        }
    }

    fn eval_reference(&mut self, sheet_index: usize, reference: &CellReference) -> Value {
        let target_sheet = match &reference.sheet {
            Some(name) => match self.sheet_index_by_name(name) {
                Some(i) => i,
                None => return Value::Error(ErrorValue::Ref),
            },
            None => sheet_index,
        };
        self.eval_cell(target_sheet, CellRef::new(reference.row, reference.col))
    }

    fn eval_name(&mut self, sheet_index: usize, name: &str) -> Value {
        let defined = self.workbook.defined_names.iter().find(|d| d.name == name);
        match defined {
            Some(dn) => {
                let scope = dn
                    .sheet
                    .and_then(|id| self.workbook.sheets.iter().position(|s| s.id == id))
                    .unwrap_or(sheet_index);
                // Clone the AST so we no longer borrow the workbook while recursing.
                let expr = dn.formula.clone();
                self.eval_expr(scope, &expr)
            }
            None => Value::Error(ErrorValue::Name),
        }
    }

    fn eval_unary(&mut self, sheet_index: usize, op: UnaryOp, operand: &Expr) -> Value {
        let value = self.eval_expr(sheet_index, operand);
        let n = match value.as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        };
        match op {
            UnaryOp::Negate => Value::Number(-n),
            UnaryOp::Plus => Value::Number(n),
            UnaryOp::Percent => Value::Number(n / 100.0),
        }
    }

    fn eval_binary(
        &mut self,
        sheet_index: usize,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
    ) -> Value {
        let lv = self.eval_expr(sheet_index, left);
        let rv = self.eval_expr(sheet_index, right);
        if let Some(e) = lv.as_error().or_else(|| rv.as_error()) {
            return Value::Error(e);
        }
        match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Power => arithmetic(op, &lv, &rv),
            BinaryOp::Concat => match (lv.as_text(), rv.as_text()) {
                (Ok(a), Ok(b)) => Value::Text(a + &b),
                (Err(e), _) | (_, Err(e)) => Value::Error(e),
            },
            _ => comparison(op, &lv, &rv),
        }
    }

    fn sheet_index_by_name(&self, name: &str) -> Option<usize> {
        self.workbook
            .sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Resolve an optional sheet name to an index, defaulting to `default`.
    pub(crate) fn resolve_sheet(&self, sheet: &Option<String>, default: usize) -> Option<usize> {
        match sheet {
            Some(name) => self.sheet_index_by_name(name),
            None => Some(default),
        }
    }

    /// The moment the volatile date functions report, as the host set it.
    pub(crate) fn now_serial(&self) -> f64 {
        self.workbook.volatile_now
    }

    /// The next pseudo-random draw in `[0, 1)`.
    ///
    /// SplitMix64 over `(seed, counter)`: small, well-distributed, and — the
    /// point — a pure function of two numbers the host controls, so a
    /// recalculation can be reproduced exactly.
    pub(crate) fn next_random(&mut self) -> f64 {
        self.rand_counter = self.rand_counter.wrapping_add(1);
        let mut z = self
            .workbook
            .volatile_seed
            .wrapping_add(self.rand_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        // 53 bits is exactly what an f64 mantissa holds, so every value is
        // representable and the distribution stays uniform.
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The bounds of a range on `target`, with any unnamed axis narrowed to the
    /// data. Every site that walks a range goes through here — walking the
    /// literal bounds of `A:A` is a million-row loop.
    pub(crate) fn range_bounds(
        &self,
        target: usize,
        a: &CellReference,
        b: &CellReference,
    ) -> (u32, u32, u32, u32) {
        match self.workbook.sheets.get(target) {
            Some(sheet) => crate::ranges::range_bounds(sheet, a, b),
            None => (
                a.row.min(b.row),
                a.col.min(b.col),
                a.row.max(b.row),
                a.col.max(b.col),
            ),
        }
    }
}

fn arithmetic(op: BinaryOp, lv: &Value, rv: &Value) -> Value {
    let a = match lv.as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let b = match rv.as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let result = match op {
        BinaryOp::Add => a + b,
        BinaryOp::Subtract => a - b,
        BinaryOp::Multiply => a * b,
        BinaryOp::Divide => {
            if b == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            a / b
        }
        BinaryOp::Power => a.powf(b),
        _ => unreachable!("non-arithmetic op"),
    };
    // Overflow (e.g. 1e308*10) or an undefined result (e.g. (-1)^0.5) yields a
    // non-finite float, which Excel reports as #NUM! and which must never be
    // stored — `<v>inf</v>` is not a valid xlsx number.
    if !result.is_finite() {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(result)
}

fn comparison(op: BinaryOp, lv: &Value, rv: &Value) -> Value {
    let ordering = match (lv.as_number(), rv.as_number()) {
        (Ok(a), Ok(b)) => a.partial_cmp(&b),
        _ => {
            let a = lv.as_text().unwrap_or_default();
            let b = rv.as_text().unwrap_or_default();
            Some(a.cmp(&b))
        }
    };
    let Some(ordering) = ordering else {
        return Value::Error(ErrorValue::Value);
    };
    use std::cmp::Ordering;
    let result = match op {
        BinaryOp::Equal => ordering == Ordering::Equal,
        BinaryOp::NotEqual => ordering != Ordering::Equal,
        BinaryOp::Less => ordering == Ordering::Less,
        BinaryOp::LessEqual => ordering != Ordering::Greater,
        BinaryOp::Greater => ordering == Ordering::Greater,
        BinaryOp::GreaterEqual => ordering != Ordering::Less,
        _ => unreachable!("non-comparison op"),
    };
    Value::Bool(result)
}

fn error_from_token(token: &str) -> ErrorValue {
    match token {
        "#REF!" => ErrorValue::Ref,
        "#VALUE!" => ErrorValue::Value,
        "#DIV/0!" => ErrorValue::Div0,
        "#N/A" => ErrorValue::Na,
        "#NAME?" => ErrorValue::Name,
        "#NULL!" => ErrorValue::Null,
        "#NUM!" => ErrorValue::Num,
        "#SPILL!" => ErrorValue::Spill,
        _ => ErrorValue::Value,
    }
}
