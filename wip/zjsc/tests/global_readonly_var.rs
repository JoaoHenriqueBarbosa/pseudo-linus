//! `var` e atribuição sobre as propriedades globais não graváveis (`NaN`, `Infinity`, `undefined`).
//! Medido no bun 1.4.2 com `vm.runInThisContext` (script de verdade) e `(0, eval)`:
//! - script sloppy: `var NaN = 1; NaN` dá `NaN`, `var undefined = 5; undefined` dá `undefined`,
//!   `var Infinity = 2; Infinity` dá `Infinity` (a declaração não lança e a escrita é ignorada);
//! - script sloppy: `NaN = 3; NaN` dá `NaN`; `Infinity = 3; Infinity` dá `Infinity`;
//! - script strict: `var NaN = 1` e `NaN = 3` lançam `TypeError: Attempted to assign to readonly property.`,
//!   o mesmo para `undefined = 3`;
//! - `(0, eval)('var NaN=1; NaN')` dá `NaN`, `(0, eval)('NaN=2; NaN')` dá `NaN`;
//! - `(0, eval)('"use strict"; var NaN=1; NaN')` dá `1` (o `var` do eval strict é local).
use zjsc::api::eval::{describe_exception, evaluate_script};

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", describe_exception(&thrown)));
    value.is_true()
}

fn thrown_message(source: &str) -> String {
    match evaluate_script(source) {
        Ok(_) => panic!("não lançou: {source}"),
        Err(thrown) => describe_exception(&thrown),
    }
}

#[test]
fn sloppy_script_var_keeps_the_global_value() {
    assert!(is_true("var NaN = 1; Number.isNaN(NaN)"));
    assert!(is_true("var undefined = 5; undefined === void 0"));
    assert!(is_true("var Infinity = 2; Infinity === 1 / 0"));
}

#[test]
fn sloppy_script_assignment_is_ignored() {
    assert!(is_true("NaN = 3; Number.isNaN(NaN)"));
    assert!(is_true("Infinity = 3; Infinity === 1 / 0"));
    assert!(is_true("undefined = 3; undefined === void 0"));
}

#[test]
fn strict_script_var_and_assignment_throw() {
    for source in ["'use strict'; var NaN = 1;", "'use strict'; NaN = 3;", "'use strict'; undefined = 3;"] {
        let message = thrown_message(source);
        assert!(message.contains("Attempted to assign to readonly property."), "{source}: {message}");
    }
}

#[test]
fn indirect_eval_matches_script() {
    assert!(is_true("Number.isNaN((0, eval)('var NaN = 1; NaN'))"));
    assert!(is_true("Number.isNaN((0, eval)('NaN = 2; NaN'))"));
    assert!(is_true("(0, eval)('var undefined = 5; undefined') === void 0"));
    assert!(is_true("(0, eval)('var Infinity = 2; Infinity') === 1 / 0"));
    assert!(is_true("(0, eval)('\"use strict\"; var NaN = 1; NaN') === 1"));
}
