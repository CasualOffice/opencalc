//! Custom worksheet functions (`MACRO-04`): the engine's half of the async
//! model. The JavaScript runs in the host's sandbox; here the session asks for
//! calls, shows `#BUSY!` until answered, and recalculates what the answer
//! reaches.

use casual_calc_model::{
    CellRef, CellValue, CustomFunctionSpec, CustomParam, CustomValue, ErrorValue, Id, Sheet,
    SheetId,
};

use crate::WorkbookSession;

fn spec(name: &str, params: usize, volatile: bool) -> CustomFunctionSpec {
    CustomFunctionSpec {
        name: name.to_owned(),
        version: "v1".to_owned(),
        min_args: params,
        max_args: params,
        volatile,
        description: format!("{name} does a thing"),
        params: (0..params)
            .map(|i| CustomParam {
                name: format!("x{i}"),
                kind: "number".to_owned(),
                optional: false,
                description: String::new(),
            })
            .collect(),
        returns: "number".to_owned(),
    }
}

fn session() -> WorkbookSession {
    let mut s = WorkbookSession::blank();
    s.workbook_mut()
        .sheets
        .push(Sheet::new(SheetId(Id::from_parts(9, 1)), "Sheet1"));
    s
}

fn type_in(s: &mut WorkbookSession, row: u32, col: u32, input: &str) {
    let op = s.input_edit(0, CellRef::new(row, col), input);
    s.edit(op).unwrap();
}

fn value(s: &WorkbookSession, row: u32, col: u32) -> CellValue {
    s.workbook().sheets[0]
        .cells
        .get(CellRef::new(row, col))
        .map(|c| c.value.clone())
        .unwrap_or(CellValue::Empty)
}

const BUSY: CellValue = CellValue::Error(ErrorValue::Busy);

#[test]
fn a_registered_function_is_busy_until_answered_and_then_its_dependents_follow() {
    let mut s = session();
    s.set_custom_functions(vec![spec("MYFN", 1, false)])
        .unwrap();
    type_in(&mut s, 0, 0, "=MYFN(1)");
    type_in(&mut s, 0, 1, "=A1+1");
    assert_eq!(value(&s, 0, 0), BUSY, "a call with no answer is #BUSY!");
    assert_eq!(value(&s, 0, 1), BUSY, "and so is what reads it");

    let calls = s.take_custom_calls();
    assert_eq!(calls.len(), 1, "exactly one call is wanted: {calls:?}");
    let call = &calls[0];
    assert_eq!(call.key.name, "MYFN");
    assert_eq!(call.key.args, vec![CustomValue::Number(1.0)]);
    assert_eq!(
        call.key.address, "Sheet1!A1",
        "this.address, ONLYOFFICE's form"
    );
    assert!(s.take_custom_calls().is_empty(), "taken once");

    s.set_custom_results(vec![(call.ticket, CustomValue::Number(41.0))]);
    assert_eq!(value(&s, 0, 0), CellValue::Number(41.0));
    assert_eq!(
        value(&s, 0, 1),
        CellValue::Number(42.0),
        "the dependent recalculated"
    );
    assert!(
        s.take_custom_calls().is_empty(),
        "an answer does not ask again"
    );
}

#[test]
fn changed_arguments_ask_again_and_the_old_answer_is_not_reused() {
    let mut s = session();
    s.set_custom_functions(vec![spec("MYFN", 1, false)])
        .unwrap();
    type_in(&mut s, 1, 0, "5");
    type_in(&mut s, 0, 0, "=MYFN(A2)");
    let first = s.take_custom_calls();
    assert_eq!(first[0].key.args, vec![CustomValue::Number(5.0)]);
    assert_eq!(
        first[0].key.arg_addresses,
        vec![Some("Sheet1!A2".to_owned())]
    );
    s.set_custom_results(vec![(first[0].ticket, CustomValue::Number(10.0))]);
    assert_eq!(value(&s, 0, 0), CellValue::Number(10.0));

    type_in(&mut s, 1, 0, "6");
    assert_eq!(value(&s, 0, 0), BUSY, "a new argument has no answer yet");
    let second = s.take_custom_calls();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].key.args, vec![CustomValue::Number(6.0)]);

    // Back to the first argument: the cached answer serves without asking.
    type_in(&mut s, 1, 0, "5");
    assert_eq!(value(&s, 0, 0), CellValue::Number(10.0));
    assert!(s.take_custom_calls().is_empty());
}

#[test]
fn an_array_answer_spills_within_the_bound_and_one_over_it_is_refused() {
    let mut s = session();
    s.set_custom_functions(vec![spec("GRID", 0, false)])
        .unwrap();
    type_in(&mut s, 0, 0, "=GRID()");
    let calls = s.take_custom_calls();
    let block = CustomValue::Array {
        rows: 2,
        cols: 2,
        cells: [1.0, 2.0, 3.0, 4.0].map(CustomValue::Number).to_vec(),
    };
    s.set_custom_results(vec![(calls[0].ticket, block)]);
    assert_eq!(value(&s, 0, 0), CellValue::Number(1.0));
    assert_eq!(value(&s, 1, 1), CellValue::Number(4.0), "spilled to B2");

    // An answer whose shape breaks the array bound is #NUM!, as SEQUENCE's is.
    type_in(&mut s, 5, 0, "=GRID()+0");
    let calls = s.take_custom_calls();
    let huge = CustomValue::Array {
        rows: 1_048_577,
        cols: 1,
        cells: Vec::new(),
    };
    s.set_custom_results(vec![(calls[0].ticket, huge)]);
    assert_eq!(value(&s, 5, 0), CellValue::Error(ErrorValue::Num));
}

#[test]
fn an_unregistered_name_is_still_name_error() {
    let mut s = session();
    type_in(&mut s, 0, 0, "=MYFN(1)");
    assert_eq!(value(&s, 0, 0), CellValue::Error(ErrorValue::Name));
    assert!(s.take_custom_calls().is_empty());
}

#[test]
fn registering_a_function_computes_cells_that_already_call_it() {
    let mut s = session();
    type_in(&mut s, 0, 0, "=MYFN(1)");
    assert_eq!(value(&s, 0, 0), CellValue::Error(ErrorValue::Name));
    s.set_custom_functions(vec![spec("MYFN", 1, false)])
        .unwrap();
    assert_eq!(value(&s, 0, 0), BUSY);
    assert_eq!(s.take_custom_calls().len(), 1);
}

#[test]
fn the_wrong_argument_count_is_value_error_and_an_error_argument_propagates() {
    let mut s = session();
    s.set_custom_functions(vec![spec("MYFN", 1, false)])
        .unwrap();
    type_in(&mut s, 0, 0, "=MYFN(1,2)");
    type_in(&mut s, 1, 0, "=MYFN(1/0)");
    assert_eq!(value(&s, 0, 0), CellValue::Error(ErrorValue::Value));
    assert_eq!(value(&s, 1, 0), CellValue::Error(ErrorValue::Div0));
    assert!(
        s.take_custom_calls().is_empty(),
        "neither reaches the function"
    );
}

#[test]
fn a_volatile_function_asks_again_when_rand_would_reroll_and_not_when_an_answer_lands() {
    let mut s = session();
    s.set_custom_functions(vec![spec("ROLL", 0, true)]).unwrap();
    type_in(&mut s, 0, 0, "=ROLL()");
    let calls = s.take_custom_calls();
    assert_eq!(calls.len(), 1);
    s.set_custom_results(vec![(calls[0].ticket, CustomValue::Number(3.0))]);
    assert_eq!(value(&s, 0, 0), CellValue::Number(3.0));
    assert!(
        s.take_custom_calls().is_empty(),
        "the answer's own recalc does not reroll"
    );

    // An edit elsewhere with a new seed rerolls it.
    s.workbook_mut().volatile_seed += 1;
    type_in(&mut s, 3, 3, "x");
    assert_eq!(value(&s, 0, 0), BUSY);
    assert_eq!(s.take_custom_calls().len(), 1);
}

#[test]
fn answers_are_not_document_content_and_do_not_make_an_edit() {
    let mut s = session();
    s.set_custom_functions(vec![spec("MYFN", 0, false)])
        .unwrap();
    type_in(&mut s, 0, 0, "=MYFN()");
    let edits = s.edits_applied();
    let calls = s.take_custom_calls();
    s.set_custom_results(vec![(calls[0].ticket, CustomValue::Number(7.0))]);
    assert_eq!(s.edits_applied(), edits, "an answer is not an edit");
    assert!(!s.can_redo());
    // A reopened copy knows nothing of the registry: the formula is #NAME? to
    // a reader without the function, its saved value 7 notwithstanding.
    let bytes = s.save().unwrap();
    let reopened = WorkbookSession::open(bytes).unwrap();
    assert!(reopened.workbook().custom.is_empty());
    assert_eq!(reopened.cell_input(0, CellRef::new(0, 0)), "=MYFN()");
    assert_eq!(
        value(&reopened, 0, 0),
        CellValue::Number(7.0),
        "the file's cached value is shown"
    );
}

#[test]
fn a_change_to_the_function_sources_is_announced_for_re_registration() {
    let mut s = session();
    let before = s.workbook().custom.library_generation();
    let mut lib = s.workbook().macros.clone();
    lib.custom_functions
        .push(casual_calc_model::CustomFunctionSource {
            guid: "f1".to_owned(),
            name: "Function 1".to_owned(),
            source: "Api.AddCustomFunction(function F(){return 1;});".to_owned(),
        });
    s.edit(crate::EditOperation::SetMacros(lib)).unwrap();
    let after_edit = s.workbook().custom.library_generation();
    assert_ne!(
        after_edit, before,
        "an edit to the sources moves the generation"
    );
    s.undo().unwrap();
    assert_ne!(
        s.workbook().custom.library_generation(),
        after_edit,
        "and so does its undo"
    );
}
