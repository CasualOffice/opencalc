//! Custom worksheet functions at run time (`MACRO-04`, docs/99).
//!
//! A workbook's custom-function *sources* are document content and live in
//! [`MacroLibrary::custom_functions`](crate::MacroLibrary). What lives here is
//! the other half — which names those sources registered, and the answers the
//! JavaScript gave — and it is **not** document content. It is never
//! serialised, never part of an operation, and never compared: two clients
//! that registered the same functions hold the same document whatever their
//! caches say. Each client computes its own answers, and a deterministic
//! function gives the same ones everywhere.
//!
//! # Why the engine holds a cache rather than calling out
//!
//! The JavaScript runs in a sandboxed worker (docs/99 §5) and answers
//! asynchronously; the evaluator is synchronous and must stay deterministic.
//! So the evaluator never calls a function. It looks the call up here by its
//! [`CustomCallKey`] — the function, the version of the source that defined
//! it, the calling cell, and the argument values and addresses — and either
//! finds an answer or records the call as wanted and evaluates to `#BUSY!`,
//! which is Excel's and ONLYOFFICE's value for a function still computing. The
//! host takes the wanted calls ([`CustomFunctionRuntime::take_pending`]),
//! runs them, hands the answers back ([`CustomFunctionRuntime::resolve`]),
//! and the cells that asked are recalculated, their dependents with them.
//!
//! The calling cell is part of the key because the function can read it
//! (`this.address`, `this.args[i].address`): two cells calling
//! `=WHERE()` with no arguments must not share an answer.
//!
//! # Volatility
//!
//! A function is volatile only when its JSDoc says so — Excel's `@volatile`,
//! or ONLYOFFICE's `@calculateCell`. A volatile call's key carries the
//! workbook's [`volatile_seed`](crate::Workbook::volatile_seed), the number the
//! host moves on every edit-driven recalculation to reroll `RAND()`. So a
//! volatile function is asked again exactly when `RAND()` rerolls, and *not*
//! when the recalculation is the one an answer arriving caused — which is what
//! keeps a volatile function from asking forever.
//!
//! # Bounds
//!
//! Every count here comes from a workbook and therefore from a file or a peer:
//! the registry, the queue and the cache are each capped. A call over the
//! queue's cap is not dropped — its cell is remembered and asked again after
//! the next answers arrive ([`CustomFunctionRuntime::take_deferred`]).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::value::ErrorValue;

/// The most custom functions one workbook may register.
pub const MAX_CUSTOM_FUNCTIONS: usize = 1_000;
/// The most calls waiting for the host at once.
pub const MAX_PENDING_CUSTOM_CALLS: usize = 10_000;
/// The most answers kept. Older answers are dropped first; a dropped answer is
/// only asked for again, never wrong.
pub const MAX_CUSTOM_RESULTS: usize = 100_000;
/// The most argument values one call may carry to the host (a range argument
/// counts each of its cells). A call over it is `#VALUE!`.
pub const MAX_CUSTOM_ARG_CELLS: usize = 100_000;
/// The most parameters one function may declare.
pub const MAX_CUSTOM_PARAMS: usize = 255;
/// The longest description or parameter text kept for the UI, in characters.
pub const MAX_CUSTOM_TEXT_CHARS: usize = 1_024;

/// One registered function, as the host's registration run reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFunctionSpec {
    /// The name formulas call it by, upper-case (`DOUBLE`).
    pub name: String,
    /// An opaque version of the source that defined it. Part of every call's
    /// key, so editing the source asks every cell again.
    pub version: String,
    /// The fewest arguments a call may pass.
    pub min_args: usize,
    /// The most.
    pub max_args: usize,
    /// `@volatile` / `@calculateCell`: asked again on every recalculation.
    pub volatile: bool,
    /// The JSDoc description, for Insert Function.
    pub description: String,
    /// The JSDoc parameters, for Insert Function and the argument hint.
    pub params: Vec<CustomParam>,
    /// The JSDoc `@returns` type (`number`, `number[][]`, …).
    pub returns: String,
}

/// One declared parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomParam {
    /// As written in the JSDoc.
    pub name: String,
    /// `number`, `string`, `boolean`, `any`, or one of their `[][]` forms.
    pub kind: String,
    /// Whether a call may leave it out (`[name]` in the JSDoc).
    pub optional: bool,
    /// What the JSDoc says about it.
    pub description: String,
}

/// A value crossing between the evaluator and the host: an argument going out
/// or an answer coming back.
///
/// Numbers compare and hash by their bits (with `-0` folded into `0`), so a
/// value can be part of a [`CustomCallKey`].
#[derive(Debug, Clone)]
pub enum CustomValue {
    /// A blank cell, or an argument left out.
    Empty,
    /// A number; always finite.
    Number(f64),
    /// Text.
    Text(String),
    /// A logical.
    Bool(bool),
    /// An error value.
    Error(ErrorValue),
    /// A block, row-major, never nested.
    Array {
        /// Row count, at least one.
        rows: usize,
        /// Column count, at least one.
        cols: usize,
        /// `rows × cols` scalars.
        cells: Vec<CustomValue>,
    },
}

impl CustomValue {
    fn bits(n: f64) -> u64 {
        if n == 0.0 { 0 } else { n.to_bits() }
    }

    /// How many scalars this carries.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        match self {
            CustomValue::Array { cells, .. } => cells.len(),
            _ => 1,
        }
    }
}

impl PartialEq for CustomValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (CustomValue::Empty, CustomValue::Empty) => true,
            (CustomValue::Number(a), CustomValue::Number(b)) => Self::bits(*a) == Self::bits(*b),
            (CustomValue::Text(a), CustomValue::Text(b)) => a == b,
            (CustomValue::Bool(a), CustomValue::Bool(b)) => a == b,
            (CustomValue::Error(a), CustomValue::Error(b)) => a == b,
            (
                CustomValue::Array {
                    rows: r1,
                    cols: c1,
                    cells: a,
                },
                CustomValue::Array {
                    rows: r2,
                    cols: c2,
                    cells: b,
                },
            ) => r1 == r2 && c1 == c2 && a == b,
            _ => false,
        }
    }
}

impl Eq for CustomValue {}

impl Hash for CustomValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            CustomValue::Empty => {}
            CustomValue::Number(n) => Self::bits(*n).hash(state),
            CustomValue::Text(s) => s.hash(state),
            CustomValue::Bool(b) => b.hash(state),
            CustomValue::Error(e) => e.hash(state),
            CustomValue::Array { rows, cols, cells } => {
                rows.hash(state);
                cols.hash(state);
                cells.hash(state);
            }
        }
    }
}

/// Everything an answer depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CustomCallKey {
    /// The function, upper-case.
    pub name: String,
    /// [`CustomFunctionSpec::version`] when the call was made.
    pub version: String,
    /// The calling cell: sheet index, row, column.
    pub cell: (usize, u32, u32),
    /// The calling cell as the function sees it (`this.address`).
    pub address: String,
    /// The argument values, in order; a left-out argument is
    /// [`CustomValue::Empty`].
    pub args: Vec<CustomValue>,
    /// Each argument's address when it was a reference (`this.args[i].address`).
    pub arg_addresses: Vec<Option<String>>,
    /// The volatile seed, for a volatile function; `None` otherwise.
    pub epoch: Option<u64>,
}

/// A wanted call the host has taken and not yet answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCustomCall {
    /// The ticket to answer it with.
    pub ticket: u64,
    /// What to call.
    pub key: CustomCallKey,
}

/// A fresh number per runtime, so a host can tell one workbook's runtime from
/// the next one's (an open, File ▸ New, a collaboration join).
static INSTANCES: AtomicU64 = AtomicU64::new(1);

/// The run-time state of a workbook's custom functions. See the module docs.
#[derive(Debug, Clone)]
pub struct CustomFunctionRuntime {
    instance: u64,
    library_generation: u64,
    functions: BTreeMap<String, CustomFunctionSpec>,
    results: HashMap<CustomCallKey, CustomValue>,
    order: VecDeque<CustomCallKey>,
    queue: VecDeque<CustomCallKey>,
    wanted: HashSet<CustomCallKey>,
    in_flight: BTreeMap<u64, CustomCallKey>,
    next_ticket: u64,
    deferred: BTreeSet<(usize, u32, u32)>,
}

impl Default for CustomFunctionRuntime {
    fn default() -> Self {
        Self {
            instance: INSTANCES.fetch_add(1, Ordering::Relaxed),
            library_generation: 0,
            functions: BTreeMap::new(),
            results: HashMap::new(),
            order: VecDeque::new(),
            queue: VecDeque::new(),
            wanted: HashSet::new(),
            in_flight: BTreeMap::new(),
            next_ticket: 1,
            deferred: BTreeSet::new(),
        }
    }
}

/// Run-time state is not document content: two workbooks are equal whatever
/// their caches hold. (The derive on [`crate::Workbook`] is document equality.)
impl PartialEq for CustomFunctionRuntime {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Why a registration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomFunctionError(pub String);

impl core::fmt::Display for CustomFunctionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CustomFunctionError {}

/// Whether `name` can be a custom function's name: a letter or `_`, then
/// letters, digits, `_` and `.`, at most 255 characters — Excel's rule for a
/// function id (custom-functions JSON metadata), ASCII only.
#[must_use]
pub fn is_valid_custom_function_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 255
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

impl CustomFunctionRuntime {
    /// This runtime's number, distinct from every other runtime's in the
    /// process.
    #[must_use]
    pub fn instance(&self) -> u64 {
        self.instance
    }

    /// How many times the workbook's custom-function sources have changed.
    #[must_use]
    pub fn library_generation(&self) -> u64 {
        self.library_generation
    }

    /// The sources changed: the host must register again.
    pub fn note_library_changed(&mut self) {
        self.library_generation = self.library_generation.wrapping_add(1);
    }

    /// Replace the registry. Every answer, wanted call and ticket is dropped:
    /// they were computed by the functions being replaced.
    ///
    /// # Errors
    ///
    /// Over [`MAX_CUSTOM_FUNCTIONS`], a name that is not
    /// [valid](is_valid_custom_function_name), a repeated name, an arity
    /// that is not `min ≤ max ≤ MAX_CUSTOM_PARAMS`, or text over its bound.
    /// Nothing changes when refused.
    pub fn register(&mut self, specs: Vec<CustomFunctionSpec>) -> Result<(), CustomFunctionError> {
        if specs.len() > MAX_CUSTOM_FUNCTIONS {
            return Err(CustomFunctionError(format!(
                "more than {MAX_CUSTOM_FUNCTIONS} custom functions"
            )));
        }
        let mut functions = BTreeMap::new();
        for mut spec in specs {
            spec.name = spec.name.to_ascii_uppercase();
            if !is_valid_custom_function_name(&spec.name) {
                return Err(CustomFunctionError(format!(
                    "{:?} is not a function name",
                    spec.name
                )));
            }
            if spec.min_args > spec.max_args || spec.max_args > MAX_CUSTOM_PARAMS {
                return Err(CustomFunctionError(format!(
                    "{}: invalid argument count",
                    spec.name
                )));
            }
            let long = |s: &str| s.chars().count() > MAX_CUSTOM_TEXT_CHARS;
            if long(&spec.version)
                || long(&spec.description)
                || long(&spec.returns)
                || spec.params.len() > MAX_CUSTOM_PARAMS
                || spec
                    .params
                    .iter()
                    .any(|p| long(&p.name) || long(&p.kind) || long(&p.description))
            {
                return Err(CustomFunctionError(format!("{}: too large", spec.name)));
            }
            let name = spec.name.clone();
            if functions.insert(name.clone(), spec).is_some() {
                return Err(CustomFunctionError(format!("{name} is registered twice")));
            }
        }
        let instance = self.instance;
        let library_generation = self.library_generation;
        *self = Self {
            instance,
            library_generation,
            functions,
            ..Self::default()
        };
        self.instance = instance;
        Ok(())
    }

    /// The registered function called `name` (upper-case).
    #[must_use]
    pub fn spec(&self, name: &str) -> Option<&CustomFunctionSpec> {
        self.functions.get(name)
    }

    /// Every registered function, by name.
    pub fn functions(&self) -> impl Iterator<Item = &CustomFunctionSpec> {
        self.functions.values()
    }

    /// Whether any function is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// The answer for `key`, if one has arrived.
    #[must_use]
    pub fn lookup(&self, key: &CustomCallKey) -> Option<&CustomValue> {
        self.results.get(key)
    }

    /// Ask for `key`. `true` when it is wanted (newly or already); `false` when
    /// the queue is full, in which case the caller remembers the cell with
    /// [`defer`](Self::defer).
    pub fn request(&mut self, key: CustomCallKey) -> bool {
        if self.wanted.contains(&key) || self.results.contains_key(&key) {
            return true;
        }
        if self.wanted.len() >= MAX_PENDING_CUSTOM_CALLS {
            return false;
        }
        self.wanted.insert(key.clone());
        self.queue.push_back(key);
        true
    }

    /// Remember a cell whose call did not fit in the queue.
    pub fn defer(&mut self, cell: (usize, u32, u32)) {
        self.deferred.insert(cell);
    }

    /// How many calls are waiting to be taken.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.queue.len()
    }

    /// How many calls have been taken and not answered.
    #[must_use]
    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    /// Take every waiting call, each with the ticket to answer it by.
    pub fn take_pending(&mut self) -> Vec<PendingCustomCall> {
        let mut out = Vec::with_capacity(self.queue.len());
        while let Some(key) = self.queue.pop_front() {
            let ticket = self.next_ticket;
            self.next_ticket += 1;
            self.in_flight.insert(ticket, key.clone());
            out.push(PendingCustomCall { ticket, key });
        }
        out
    }

    /// Store the answer for `ticket`, returning the cell that asked. `None`
    /// for a ticket this runtime did not issue or has since forgotten (a
    /// re-registration drops every ticket), in which case nothing changes.
    pub fn resolve(&mut self, ticket: u64, value: CustomValue) -> Option<(usize, u32, u32)> {
        let key = self.in_flight.remove(&ticket)?;
        self.wanted.remove(&key);
        let cell = key.cell;
        if self.results.len() >= MAX_CUSTOM_RESULTS
            && let Some(oldest) = self.order.pop_front()
        {
            self.results.remove(&oldest);
        }
        if self.results.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        Some(cell)
    }

    /// The cells deferred since the last call, now to be asked again.
    pub fn take_deferred(&mut self) -> Vec<(usize, u32, u32)> {
        std::mem::take(&mut self.deferred).into_iter().collect()
    }

    /// Drop every answer and every wanted call, keeping the registry — what a
    /// host does to compute everything afresh.
    pub fn clear_results(&mut self) {
        self.results.clear();
        self.order.clear();
        self.queue.clear();
        self.wanted.clear();
        self.in_flight.clear();
        self.deferred.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> CustomFunctionSpec {
        CustomFunctionSpec {
            name: name.to_owned(),
            version: "v1".to_owned(),
            min_args: 1,
            max_args: 1,
            volatile: false,
            description: String::new(),
            params: Vec::new(),
            returns: "number".to_owned(),
        }
    }

    fn key(n: f64) -> CustomCallKey {
        CustomCallKey {
            name: "DOUBLE".to_owned(),
            version: "v1".to_owned(),
            cell: (0, 0, 0),
            address: "Sheet1!A1".to_owned(),
            args: vec![CustomValue::Number(n)],
            arg_addresses: vec![None],
            epoch: None,
        }
    }

    #[test]
    fn names_are_upper_cased_and_checked() {
        let mut rt = CustomFunctionRuntime::default();
        rt.register(vec![spec("double")]).unwrap();
        assert!(rt.spec("DOUBLE").is_some());
        assert!(rt.register(vec![spec("1BAD")]).is_err());
        assert!(rt.register(vec![spec("A"), spec("a")]).is_err());
        // Refused means unchanged.
        assert!(rt.spec("DOUBLE").is_some());
    }

    #[test]
    fn a_call_is_wanted_once_and_answered_by_ticket() {
        let mut rt = CustomFunctionRuntime::default();
        assert!(rt.request(key(1.0)));
        assert!(rt.request(key(1.0)));
        let taken = rt.take_pending();
        assert_eq!(taken.len(), 1);
        assert!(rt.take_pending().is_empty());
        assert_eq!(
            rt.resolve(taken[0].ticket, CustomValue::Number(2.0)),
            Some((0, 0, 0))
        );
        assert_eq!(rt.lookup(&key(1.0)), Some(&CustomValue::Number(2.0)));
        assert_eq!(
            rt.lookup(&key(-0.0)),
            None,
            "a different argument is a different call"
        );
        assert_eq!(rt.lookup(&key(0.0)), rt.lookup(&key(-0.0)));
        assert_eq!(
            rt.resolve(taken[0].ticket, CustomValue::Number(3.0)),
            None,
            "a ticket answers once"
        );
    }

    #[test]
    fn the_queue_is_bounded() {
        let mut rt = CustomFunctionRuntime::default();
        for i in 0..MAX_PENDING_CUSTOM_CALLS {
            assert!(rt.request(key(i as f64)));
        }
        assert!(!rt.request(key(-1.0)));
    }

    #[test]
    fn re_registering_forgets_answers_and_tickets() {
        let mut rt = CustomFunctionRuntime::default();
        rt.register(vec![spec("DOUBLE")]).unwrap();
        rt.request(key(1.0));
        let t = rt.take_pending()[0].ticket;
        rt.register(vec![spec("DOUBLE")]).unwrap();
        assert_eq!(rt.resolve(t, CustomValue::Number(2.0)), None);
    }

    #[test]
    fn runtimes_are_told_apart_and_never_differ_as_document_content() {
        let a = CustomFunctionRuntime::default();
        let b = CustomFunctionRuntime::default();
        assert_ne!(a.instance(), b.instance());
        assert_eq!(a, b);
    }
}
