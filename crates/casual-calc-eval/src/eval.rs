//! The expression evaluator: an AST + a workbook context → a [`Value`].
//!
//! References are resolved by (memoized) recursive evaluation, so a formula that
//! reads another formula computes it on demand. A re-entrant cell is a circular
//! reference and yields an error.

use std::collections::{HashMap, HashSet};

use casual_calc_formula::{BinaryOp, CellReference, Expr, UnaryOp};
use casual_calc_model::{CellRef, ErrorValue, Workbook};

use crate::functions::call_function;
use crate::value::{Value, value_from_cell};

type CellKey = (usize, u32, u32);

/// Evaluates cells and expressions against a workbook, memoizing results.
#[derive(Debug)]
pub struct Evaluator<'a> {
    workbook: &'a Workbook,
    memo: HashMap<CellKey, Value>,
    in_progress: HashSet<CellKey>,
}

impl<'a> Evaluator<'a> {
    /// A new evaluator over `workbook`.
    pub fn new(workbook: &'a Workbook) -> Self {
        Self {
            workbook,
            memo: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    /// The workbook being evaluated.
    pub fn workbook(&self) -> &'a Workbook {
        self.workbook
    }

    /// Evaluate the cell at `(sheet_index, at)` to a value.
    pub fn eval_cell(&mut self, sheet_index: usize, at: CellRef) -> Value {
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
            return match self.workbook.formula(handle) {
                Some(expr) => self.eval_expr(sheet_index, expr),
                None => Value::Empty,
            };
        }
        value_from_cell(&cell.value, &self.workbook.strings)
    }

    /// Evaluate an expression in the context of `sheet_index`.
    pub fn eval_expr(&mut self, sheet_index: usize, expr: &Expr) -> Value {
        match expr {
            Expr::Number(n) => Value::Number(*n),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Text(s) => Value::Text(s.clone()),
            Expr::Error(token) => Value::Error(error_from_token(token)),
            Expr::Reference(reference) => self.eval_reference(sheet_index, reference),
            Expr::Range(..) => Value::Error(ErrorValue::Value),
            Expr::Name(name) => self.eval_name(sheet_index, name),
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
