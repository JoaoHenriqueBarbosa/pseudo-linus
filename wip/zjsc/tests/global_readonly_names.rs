//! `NaN`, `Infinity` e `undefined` no escopo global de um script. Medido no bun 1.4.2 com `vm.runInThisContext`
//! para cada um dos três nomes:
//! - `var X = 1; X` dá o valor original (a declaração não lança, a escrita é ignorada);
//! - `X = 1; X` em sloppy dá o valor original; em strict lança `TypeError: Attempted to assign to readonly property.`;
//! - `'use strict'; var X = 1` lança o mesmo `TypeError`;
//! - `function X(){}` lança `TypeError: Can't declare global function 'X': property must be either configurable or
//!   both writable and enumerable` (`canDeclareGlobalFunction` falha);
//! - `let X`, `const X` e `class X{}` lançam `SyntaxError: Can't create duplicate variable that shadows a global
//!   property: 'X'` (propriedade global não configurável).
use zjsc::api::eval::{describe_exception, evaluate_script};

const NAMES: [(&str, &str); 3] = [("NaN", "Number.isNaN(NaN)"), ("Infinity", "Infinity === 1 / 0"), ("undefined", "undefined === void 0")];

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
fn sloppy_var_and_assignment_keep_the_value() {
    for (name, check) in NAMES {
        assert!(is_true(&format!("var {name} = 1; {check}")), "var {name}");
        assert!(is_true(&format!("{name} = 1; {check}")), "{name} = 1");
        assert!(is_true(&format!("var {name}; {check}")), "var {name} sem inicializador");
    }
}

#[test]
fn strict_var_and_assignment_throw_type_error() {
    for (name, _) in NAMES {
        for source in [format!("'use strict'; var {name} = 1;"), format!("'use strict'; {name} = 1;")] {
            let message = thrown_message(&source);
            assert!(message.contains("TypeError"), "{source}: {message}");
            assert!(message.contains("Attempted to assign to readonly property."), "{source}: {message}");
        }
    }
}

#[test]
fn strict_assignment_inside_function_throws_type_error() {
    for (name, _) in NAMES {
        let source = format!("function f() {{ 'use strict'; {name} = 1; }} f();");
        let message = thrown_message(&source);
        assert!(message.contains("Attempted to assign to readonly property."), "{source}: {message}");
    }
}

#[test]
fn function_declaration_cannot_redeclare() {
    for (name, _) in NAMES {
        let source = format!("function {name}() {{}}");
        let message = thrown_message(&source);
        assert!(message.contains("TypeError"), "{source}: {message}");
        assert!(
            message.contains(&format!(
                "Can't declare global function '{name}': property must be either configurable or both writable and enumerable"
            )),
            "{source}: {message}"
        );
    }
}

#[test]
fn lexical_declarations_cannot_shadow() {
    for (name, _) in NAMES {
        for source in [format!("let {name} = 1;"), format!("const {name} = 1;"), format!("class {name} {{}}")] {
            let message = thrown_message(&source);
            assert!(message.contains("SyntaxError"), "{source}: {message}");
            assert!(
                message.contains(&format!("Can't create duplicate variable that shadows a global property: '{name}'")),
                "{source}: {message}"
            );
        }
    }
}
