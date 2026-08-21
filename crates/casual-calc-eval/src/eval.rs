//! The expression evaluator: an AST + a workbook context → a [`Value`].
//!
//! References are resolved by (memoized) recursive evaluation, so a formula that
//! reads another formula computes it on demand. A re-entrant cell is a circular
//! reference and yields an error.

use std::collections::{HashMap, HashSet};

use casual_calc_formula::stored::{ABSOLUTE, Origin, StoredRef};
use casual_calc_formula::{BinaryOp, Expr, UnaryOp};
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
    /// What `<calcPr>` asks for when a formula depends on itself.
    iteration: casual_calc_model::Iteration,
    /// The cell whose formula is currently being evaluated — what `ROW()` /
    /// `COLUMN()` with no argument report. Saved and restored around each
    /// formula so a referenced cell's own formula sees its own address.
    current: Option<(usize, CellRef)>,
    /// Names bound by `LET` and by `LAMBDA` parameters, innermost last.
    ///
    /// A stack rather than a map because shadowing is legal and ordinary:
    /// `LET(x, 1, LET(x, x+1, x))` is 2, and a lambda parameter may share a
    /// name with an outer binding. Lookup walks from the end.
    scope: Vec<(String, Value)>,
    /// Nested lambda applications, to stop a recursive one without a base case
    /// from taking the stack down with it.
    depth: u32,
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
            iteration: workbook.settings.iteration(),
            dirty: None,
            current: None,
            scope: Vec::new(),
            depth: 0,
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
            iteration: workbook.settings.iteration(),
            dirty: Some(dirty),
            current: None,
            scope: Vec::new(),
            depth: 0,
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
            // A cell that depends on itself.
            //
            // With iteration off this is a mistake, and `#REF!` says so. With
            // it on the author *meant* the loop — a balance that depends on the
            // interest it accrues — and the answer is the value from the
            // previous pass, which is what makes the loop a sequence that can
            // converge instead of an error.
            if self.iteration.enabled {
                return self.cached_value(sheet_index, at);
            }
            return Value::Error(ErrorValue::Ref);
        }
        let value = self.compute_cell(sheet_index, at);
        self.in_progress.remove(&key);
        self.memo.insert(key, value.clone());
        value
    }

    /// A cell's last written value, without evaluating it.
    ///
    /// The seed for an iterative pass: on the first one it is whatever the file
    /// carried (or empty, which reads as zero), and on each pass after it is
    /// what the previous pass computed.
    fn cached_value(&self, sheet_index: usize, at: CellRef) -> Value {
        self.workbook
            .sheets
            .get(sheet_index)
            .and_then(|sheet| sheet.cells.get(at))
            .map_or(Value::Empty, |cell| {
                value_from_cell(&cell.value, &self.workbook.strings)
            })
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
        self.eval_expr_inner(sheet_index, expr).scalar()
    }

    /// Evaluate an expression in an **array** context.
    ///
    /// A bare range is the difference between the two contexts: on its own it
    /// is `#VALUE!`, because `=A1:B2` in one cell has no single answer — but
    /// inside `B1:B4>4` it is the four values, and comparing it to a number
    /// gives four answers. Excel draws the line in the same place.
    pub fn eval_expr_array(&mut self, sheet_index: usize, expr: &Expr) -> Value {
        if let Expr::Range(a, b) = expr {
            return self.range_as_array(sheet_index, a, b);
        }
        self.eval_expr_inner(sheet_index, expr)
    }

    /// The cells of a range as an array value.
    fn range_as_array(&mut self, sheet_index: usize, a: &StoredRef, b: &StoredRef) -> Value {
        let Some(target) = self.resolve_sheet(&a.sheet, sheet_index) else {
            return Value::Error(ErrorValue::Ref);
        };
        let (r0, c0, r1, c1) = self.range_bounds(target, a, b);
        let (rows, cols) = ((r1 - r0 + 1) as usize, (c1 - c0 + 1) as usize);
        // The same cap the aggregates use: a whole-column range in an array
        // context would otherwise materialise a million values.
        if rows.saturating_mul(cols) > 2_000_000 {
            return Value::Error(ErrorValue::Num);
        }
        let mut cells = Vec::with_capacity(rows * cols);
        for r in r0..=r1 {
            for c in c0..=c1 {
                cells.push(self.eval_cell(target, CellRef::new(r, c)));
            }
        }
        Value::Array { rows, cols, cells }
    }

    fn eval_expr_inner(&mut self, sheet_index: usize, expr: &Expr) -> Value {
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
            // An omitted argument is blank, which is what a function that
            // defaults it will read.
            Expr::Empty => Value::Empty,
            Expr::Name(name) => self.eval_name(sheet_index, name),
            // A structured reference resolves to a range, so on its own it is
            // as much a #VALUE! as `A1:B2` is; it is the aggregate around it
            // that consumes the range.
            Expr::StructuredRef { .. } => Value::Error(ErrorValue::Value),
            Expr::Unary { op, operand } => self.eval_unary(sheet_index, *op, operand),
            Expr::Binary { op, left, right } => self.eval_binary(sheet_index, *op, left, right),
            Expr::Function { name, args } if name == "LET" => self.eval_let(sheet_index, args),
            // A LAMBDA evaluates to a function *value*, capturing whatever is
            // in scope where it was written. Put in a cell it shows #CALC!,
            // which happens at the point the value is written rather than here
            // — a lambda passed to MAP must survive being a value first.
            Expr::Function { name, args } if name == "LAMBDA" => self.make_lambda(args),
            Expr::Function { name, args } => {
                // A defined name bound to a LAMBDA is called like a builtin,
                // which is the whole point of naming one.
                if !crate::functions::is_builtin(name)
                    && let Some(body) = self.lambda_named(name)
                {
                    return self.apply_lambda(sheet_index, &body, args);
                }
                call_function(self, sheet_index, name, args)
            }
            Expr::Call { callee, args } => self.eval_call(sheet_index, callee, args),
        }
    }

    /// The cell whose formula is being evaluated, as an origin.
    ///
    /// `current` has tracked this since `ROW()` and `COLUMN()` needed it — a
    /// formula's own address, saved and restored around each one. Since
    /// `PERF-11` it is also what its references measure from, so the two
    /// notions of "where this formula lives" are one value that cannot
    /// disagree with itself.
    ///
    /// [`ABSOLUTE`] when there is no holding cell: a tree evaluated on its own
    /// is the absolute form, which is what it was parsed as.
    pub(crate) fn origin(&self) -> Origin {
        self.current
            .map_or(ABSOLUTE, |(_, at)| Origin::at(at.row, at.col))
    }

    fn eval_reference(&mut self, sheet_index: usize, reference: &StoredRef) -> Value {
        let target_sheet = match &reference.sheet {
            Some(name) => match self.sheet_index_by_name(name) {
                Some(i) => i,
                None => return Value::Error(ErrorValue::Ref),
            },
            None => sheet_index,
        };
        // Off the sheet is `#REF!` — Excel's answer, and why `resolve` returns
        // an option rather than wrapping to the far edge.
        let Some(at) = reference.resolve(self.origin()) else {
            return Value::Error(ErrorValue::Ref);
        };
        self.eval_cell(target_sheet, CellRef::new(at.row, at.col))
    }

    /// `LET(name1, value1, …, calculation)`.
    ///
    /// Bindings take effect in order, so a later value may use an earlier name
    /// — that is what makes LET worth having over repeating a subexpression.
    fn eval_let(&mut self, sheet_index: usize, args: &[Expr]) -> Value {
        // Pairs then a final calculation, so the count is odd and at least 3.
        if args.len() < 3 || args.len().is_multiple_of(2) {
            return Value::Error(ErrorValue::Value);
        }
        let bindings = (args.len() - 1) / 2;
        let mut pushed = 0;
        for i in 0..bindings {
            let Expr::Name(name) = &args[i * 2] else {
                // A binding position that is not a name is a mistake, not a
                // value to evaluate.
                self.scope.truncate(self.scope.len() - pushed);
                return Value::Error(ErrorValue::Value);
            };
            let value = self.eval_expr_array(sheet_index, &args[i * 2 + 1]);
            self.scope.push((name.clone(), value));
            pushed += 1;
        }
        let result = self.eval_expr_array(sheet_index, &args[args.len() - 1]);
        self.scope.truncate(self.scope.len() - pushed);
        result
    }

    /// The LAMBDA a defined name is bound to, if it is bound to one.
    fn lambda_named(&self, name: &str) -> Option<Expr> {
        let defined = self
            .workbook
            .defined_names
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))?;
        match &defined.formula {
            Expr::Function { name, .. } if name == "LAMBDA" => Some(defined.formula.clone()),
            _ => None,
        }
    }

    /// Build a function value from a `LAMBDA` expression.
    fn make_lambda(&mut self, parts: &[Expr]) -> Value {
        if parts.is_empty() {
            return Value::Error(ErrorValue::Value);
        }
        let mut params = Vec::with_capacity(parts.len() - 1);
        for p in &parts[..parts.len() - 1] {
            let Expr::Name(n) = p else {
                return Value::Error(ErrorValue::Value);
            };
            params.push(n.clone());
        }
        Value::Lambda(std::rc::Rc::new(crate::value::LambdaValue {
            params,
            body: parts[parts.len() - 1].clone(),
            captured: self.scope.clone(),
        }))
    }

    /// Call whatever `callee` denotes.
    ///
    /// Evaluating the callee first is what makes currying work: the result of
    /// `LAMBDA(x, LAMBDA(y, x+y))(3)` is a function value that still knows `x`,
    /// and the second call applies it.
    fn eval_call(&mut self, sheet_index: usize, callee: &Expr, args: &[Expr]) -> Value {
        // A bare name is resolved as a lambda first, so `MYFN(1)` works whether
        // written as a call or as a function.
        if let Expr::Name(name) = callee
            && let Some(body) = self.lambda_named(name)
        {
            return self.apply_lambda(sheet_index, &body, args);
        }
        match self.eval_expr_array(sheet_index, callee) {
            Value::Lambda(f) => self.apply_value_lambda(sheet_index, &f, args),
            Value::Error(e) => Value::Error(e),
            _ => Value::Error(ErrorValue::Value),
        }
    }

    /// Apply an already-built function value.
    pub(crate) fn apply_value_lambda(
        &mut self,
        sheet_index: usize,
        f: &crate::value::LambdaValue,
        args: &[Expr],
    ) -> Value {
        let values: Vec<Value> = args
            .iter()
            .map(|a| self.eval_expr_array(sheet_index, a))
            .collect();
        self.apply_lambda_values(sheet_index, f, values)
    }

    /// Apply a function value to values that are already computed — what the
    /// LAMBDA helpers need, since they synthesise arguments rather than
    /// evaluating expressions.
    pub(crate) fn apply_lambda_values(
        &mut self,
        sheet_index: usize,
        f: &crate::value::LambdaValue,
        values: Vec<Value>,
    ) -> Value {
        const MAX_DEPTH: u32 = 256;
        if values.len() != f.params.len() {
            return Value::Error(ErrorValue::Value);
        }
        if self.depth >= MAX_DEPTH {
            return Value::Error(ErrorValue::Num);
        }
        // The captured scope replaces the caller's for the duration: a lambda
        // sees where it was *written*, not where it was called from.
        let saved = std::mem::replace(&mut self.scope, f.captured.clone());
        for (param, value) in f.params.iter().zip(values) {
            self.scope.push((param.clone(), value));
        }
        self.depth += 1;
        let result = self.eval_expr_array(sheet_index, &f.body);
        self.depth -= 1;
        self.scope = saved;
        result
    }

    /// Bind arguments to a LAMBDA's parameters and evaluate its body.
    ///
    /// The last argument of `LAMBDA` is the body; everything before it names a
    /// parameter. Arguments are evaluated in the *caller's* scope before the
    /// parameters are bound, or a parameter would shadow the value being
    /// passed into it.
    fn apply_lambda(&mut self, sheet_index: usize, lambda: &Expr, args: &[Expr]) -> Value {
        const MAX_DEPTH: u32 = 256;
        let Expr::Function { args: parts, .. } = lambda else {
            return Value::Error(ErrorValue::Value);
        };
        if parts.is_empty() {
            return Value::Error(ErrorValue::Value);
        }
        let params = &parts[..parts.len() - 1];
        let body = &parts[parts.len() - 1];
        if args.len() != params.len() {
            return Value::Error(ErrorValue::Value);
        }
        if self.depth >= MAX_DEPTH {
            // A recursive LAMBDA with no base case; Excel reports #NUM! rather
            // than taking the process down.
            return Value::Error(ErrorValue::Num);
        }

        let values: Vec<Value> = args
            .iter()
            .map(|a| self.eval_expr_array(sheet_index, a))
            .collect();
        let mut pushed = 0;
        for (param, value) in params.iter().zip(values) {
            let Expr::Name(name) = param else {
                self.scope.truncate(self.scope.len() - pushed);
                return Value::Error(ErrorValue::Value);
            };
            self.scope.push((name.clone(), value));
            pushed += 1;
        }
        self.depth += 1;
        let result = self.eval_expr_array(sheet_index, body);
        self.depth -= 1;
        self.scope.truncate(self.scope.len() - pushed);
        result
    }

    fn eval_name(&mut self, sheet_index: usize, name: &str) -> Value {
        // A LET binding or a lambda parameter shadows a defined name of the
        // same name — the inner one is the one the author just wrote, and it is
        // what every language with scope does.
        if let Some((_, value)) = self
            .scope
            .iter()
            .rev()
            .find(|(bound, _)| bound.eq_ignore_ascii_case(name))
        {
            return value.clone();
        }
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

    /// Apply a binary operator element-wise across arrays.
    ///
    /// A scalar operand pairs with every element; two arrays pair positionally
    /// and must agree in shape, because guessing how to line up a 3×1 against a
    /// 1×4 produces a plausible answer to a question nobody asked.
    fn broadcast_binary(&mut self, op: BinaryOp, lv: &Value, rv: &Value) -> Value {
        let shape = |v: &Value| match v {
            Value::Array { rows, cols, .. } => (*rows, *cols),
            _ => (1, 1),
        };
        let (lr, lc) = shape(lv);
        let (rr, rc) = shape(rv);
        let (rows, cols) = (lr.max(rr), lc.max(rc));
        let both_arrays = lr * lc > 1 && rr * rc > 1;
        if both_arrays && (lr, lc) != (rr, rc) {
            return Value::Error(ErrorValue::Value);
        }
        let pick = |v: &Value, r: usize, c: usize| -> Value {
            match v {
                Value::Array { rows, cols, cells } => cells
                    .get((r % *rows) * *cols + (c % *cols))
                    .cloned()
                    .unwrap_or(Value::Empty),
                other => other.clone(),
            }
        };
        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                let (a, b) = (pick(lv, r, c), pick(rv, r, c));
                cells.push(self.apply_scalar_binary(op, &a, &b));
            }
        }
        Value::Array { rows, cols, cells }
    }

    /// One operator on two scalars — the body of `eval_binary` once arrays are
    /// out of the way, shared so the element-wise path cannot drift from it.
    fn apply_scalar_binary(&mut self, op: BinaryOp, lv: &Value, rv: &Value) -> Value {
        if let Some(e) = lv.as_error().or_else(|| rv.as_error()) {
            return Value::Error(e);
        }
        match op {
            BinaryOp::Add
            | BinaryOp::Subtract
            | BinaryOp::Multiply
            | BinaryOp::Divide
            | BinaryOp::Power => arithmetic(op, lv, rv),
            BinaryOp::Concat => match (lv.as_text(), rv.as_text()) {
                (Ok(a), Ok(b)) => Value::Text(a + &b),
                (Err(e), _) | (_, Err(e)) => Value::Error(e),
            },
            _ => comparison(op, lv, rv),
        }
    }

    fn eval_binary(
        &mut self,
        sheet_index: usize,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
    ) -> Value {
        // Arrays stay whole here. `B1:B4>4` has to compare element by element
        // — that is how `FILTER(data, B1:B4>4)` is written, and collapsing to
        // the corner would silently test one cell and filter on the answer.
        let lv = self.eval_expr_array(sheet_index, left);
        let rv = self.eval_expr_array(sheet_index, right);
        if let Some(e) = lv.as_error().or_else(|| rv.as_error()) {
            return Value::Error(e);
        }
        if matches!(lv, Value::Array { .. }) || matches!(rv, Value::Array { .. }) {
            return self.broadcast_binary(op, &lv, &rv);
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
        a: &StoredRef,
        b: &StoredRef,
    ) -> (u32, u32, u32, u32) {
        match self.workbook.sheets.get(target) {
            Some(sheet) => crate::ranges::range_bounds(sheet, a, b, self.origin()),
            // No such sheet, so no data to narrow an unnamed axis against. The
            // literal bounds are the best answer — resolved against this cell,
            // and nothing at all if an endpoint falls off the sheet.
            None => match (a.resolve(self.origin()), b.resolve(self.origin())) {
                (Some(a), Some(b)) => (
                    a.row.min(b.row),
                    a.col.min(b.col),
                    a.row.max(b.row),
                    a.col.max(b.col),
                ),
                _ => (0, 0, 0, 0),
            },
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

/// Where a value sits in Excel's cross-type order: `number < text < logical`.
///
/// Only meaningful once `Empty` has been resolved against the other operand —
/// see [`compare_values`] — because empty has no fixed position: it behaves as
/// `0` beside a number and as `""` beside text, and no single rank delivers
/// both.
fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        // Resolved before ranking; reachable only if a new variant is added
        // without deciding where it belongs, and 3 sorts it after everything
        // rather than silently equal to something.
        _ => 3,
    }
}

/// Compare two values by Excel's rules. `None` when they are not comparable.
///
/// The rules, and why each is here, are in `docs/70-COMPARISON-SEMANTICS.md`.
/// In short: type before value, text case-insensitively, and no coercion
/// across types — a number and a piece of text that looks like one are
/// different values.
fn compare_values(lv: &Value, rv: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;

    // An array in a scalar position is its first cell, which is what the
    // previous implementation did by way of `as_number`/`as_text`.
    let first = |v: &Value| -> Value {
        match v {
            Value::Array { cells, .. } => cells.first().cloned().unwrap_or(Value::Empty),
            other => other.clone(),
        }
    };
    let (l, r) = (first(lv), first(rv));

    // Empty takes the shape of whatever it is compared against, before ranking.
    // `=A1=0` and `=A1=""` are both TRUE for an empty A1, which is not a thing
    // any single position in a total order can express.
    let resolve = |a: &Value, other: &Value| -> Value {
        match (a, other) {
            (Value::Empty, Value::Number(_)) => Value::Number(0.0),
            (Value::Empty, Value::Text(_)) => Value::Text(String::new()),
            (Value::Empty, Value::Bool(_)) => Value::Bool(false),
            (Value::Empty, _) => Value::Empty,
            (other_val, _) => other_val.clone(),
        }
    };
    let l = resolve(&l, &r);
    let r = resolve(&r, &l);

    if matches!((&l, &r), (Value::Empty, Value::Empty)) {
        return Some(Ordering::Equal);
    }

    match type_rank(&l).cmp(&type_rank(&r)) {
        Ordering::Equal => match (&l, &r) {
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
            // Case-insensitive, which is Excel's rule and already what
            // `loose_cmp` and the criteria matcher do. ASCII folding, not
            // locale collation — named as a limit in docs/70 rather than left
            // to be discovered.
            (Value::Text(a), Value::Text(b)) => Some(a.to_uppercase().cmp(&b.to_uppercase())),
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            _ => Some(Ordering::Equal),
        },
        other => Some(other),
    }
}

fn comparison(op: BinaryOp, lv: &Value, rv: &Value) -> Value {
    // Before anything else: comparing against an error is not a question with
    // an answer, and the previous implementation reached `as_text` on one,
    // which turned `#REF!` into a string that could compare *equal* to another
    // error's string.
    if let Value::Error(e) = lv {
        return Value::Error(*e);
    }
    if let Value::Error(e) = rv {
        return Value::Error(*e);
    }
    let Some(ordering) = compare_values(lv, rv) else {
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
