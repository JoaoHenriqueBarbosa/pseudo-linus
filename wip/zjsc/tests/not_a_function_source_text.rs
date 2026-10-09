//! O texto-fonte das mensagens de `TypeError` de chamada não função, em topo de script, dentro de função,
//! em `eval`, com expressões de membro e com espaços ou novas linhas antes (`(In '...')`, `(evaluating '...')`
//! e `(near '...')`). Cobre o defeito de `start_offset == end_offset == 0` (PLAN.md, item 6): o caso de topo
//! `g(); var g = function(){}` saía como "(near '... (f...')". Valores medidos no bun 1.4.2 com `(0, eval)(fonte)`,
//! um processo por caso; cada caso usa nomes próprios porque os `var` do eval indireto vazam para o global.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::runtime::error_instance::ErrorInstance;
use zjsc::runtime::js_value::JSValue;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// `(name, message)` do erro lançado por `source`; falha se o programa não lançar um `ErrorInstance`.
fn thrown_error(source: &str) -> (String, String) {
    let thrown: JSValue = match evaluate_indirect_eval(source) {
        Err(thrown) => thrown,
        Ok(_) => panic!("{source:?}: não lançou"),
    };
    assert!(thrown.is_cell(), "{source:?}: o valor lançado não é objeto");
    let error = ErrorInstance::from_cell_id(thrown.as_cell()).unwrap_or_else(|| panic!("{source:?}: o valor lançado não é um Error"));
    let message = String::from_utf8(error.message().utf8(ConversionMode::LenientConversion)).expect("mensagem em UTF-8");
    (error.name().to_string(), message)
}

fn assert_type_error(source: &str, expected: &str) {
    let (name, message) = thrown_error(source);
    assert_eq!(name, "TypeError", "{source:?}");
    assert_eq!(message, expected, "{source:?}");
}

// Topo de script.

#[test]
fn top_level_call_before_function_expression_assignment() {
    assert_type_error("g(); var g = function(){}", "g is not a function. (In 'g()', 'g' is undefined)");
}

#[test]
fn top_level_call_after_leading_blank_lines_and_spaces() {
    assert_type_error("\n\n   g1(); var g1 = function(){}", "g1 is not a function. (In 'g1()', 'g1' is undefined)");
}

#[test]
fn top_level_call_after_indented_statement_on_other_line() {
    assert_type_error("  \n\n  var a1;\n  a1()", "a1 is not a function. (In 'a1()', 'a1' is undefined)");
}

#[test]
fn top_level_call_on_second_line_with_trailing_newline() {
    assert_type_error("g14();\nvar g14;\n", "g14 is not a function. (In 'g14()', 'g14' is undefined)");
}

#[test]
fn top_level_call_after_block_comment() {
    assert_type_error("/* c */ g15(); var g15", "g15 is not a function. (In 'g15()', 'g15' is undefined)");
}

#[test]
fn top_level_call_of_implicit_global_number() {
    assert_type_error("x11 = 1; x11()", "x11 is not a function. (In 'x11()', 'x11' is 1)");
}

// Argumentos em várias linhas.

#[test]
fn call_with_arguments_on_several_lines() {
    assert_type_error("var a2 = 1; a2(\n1,\n2)", "a2 is not a function. (In 'a2(\n1,\n2)', 'a2' is 1)");
}

#[test]
fn call_with_empty_arguments_split_across_lines() {
    assert_type_error("var o13 = {}; o13.m(\n)", "o13.m is not a function. (In 'o13.m(\n)', 'o13.m' is undefined)");
}

// Dentro de função.

#[test]
fn call_inside_iife_before_function_expression_assignment() {
    assert_type_error("(function(){ g2(); var g2 = function(){} })()", "g2 is not a function. (In 'g2()', 'g2' is undefined)");
}

#[test]
fn call_inside_named_function_on_third_line() {
    assert_type_error("function f(){ \n  var x = 3;\n  x(); }\nf()", "x is not a function. (In 'x()', 'x' is 3)");
}

#[test]
fn call_inside_function_with_arguments_on_two_lines() {
    assert_type_error("function f3(){\n   g3(1,\n 2);\n var g3 = 5 }\nf3()", "g3 is not a function. (In 'g3(1,\n 2)', 'g3' is undefined)");
}

#[test]
fn call_inside_callback_function() {
    assert_type_error("[1].map(function(){ g12(); var g12; })", "g12 is not a function. (In 'g12()', 'g12' is undefined)");
}

#[test]
fn call_inside_strict_function() {
    assert_type_error("(function(){ 'use strict'; var a3; a3() })()", "a3 is not a function. (In 'a3()', 'a3' is undefined)");
}

// Em eval (aninhado no eval indireto).

#[test]
fn call_inside_nested_eval() {
    assert_type_error("var a4; eval('a4()')", "a4 is not a function. (In 'a4()', 'a4' is undefined)");
}

#[test]
fn call_inside_nested_eval_after_leading_spaces() {
    assert_type_error("eval('  g5(); var g5 = 1')", "g5 is not a function. (In 'g5()', 'g5' is undefined)");
}

#[test]
fn member_call_inside_nested_eval_over_two_lines() {
    assert_type_error("eval('var o6 = {}; o6\\n.m()')", "o6\n.m is not a function. (In 'o6\n.m()', 'o6\n.m' is undefined)");
}

#[test]
fn member_chain_inside_nested_eval_after_blank_lines() {
    assert_type_error("eval('\\n\\n  var o = {};\\n  o.p.q()')", "undefined is not an object (evaluating 'o.p.q')");
}

// Expressões de membro.

#[test]
fn member_call_of_missing_method() {
    assert_type_error("var o10 = {f: 1}; o10.f()", "o10.f is not a function. (In 'o10.f()', 'o10.f' is 1)");
}

#[test]
fn member_call_through_two_levels() {
    assert_type_error("var o17 = {a:{}}; o17.a.b()", "o17.a.b is not a function. (In 'o17.a.b()', 'o17.a.b' is undefined)");
}

#[test]
fn member_call_through_undefined_middle() {
    assert_type_error("var a5 = {}; a5.b.c()", "undefined is not an object (evaluating 'a5.b.c')");
}

#[test]
fn member_read_through_undefined_middle() {
    assert_type_error("var o9 = {}; o9.a.b", "undefined is not an object (evaluating 'o9.a.b')");
}

#[test]
fn bracket_call_with_string_key() {
    assert_type_error("var a6 = {}; a6['x']()", "a6['x'] is not a function. (In 'a6['x']()', 'a6['x']' is undefined)");
}

#[test]
fn bracket_call_with_key_on_next_line() {
    assert_type_error("var o18 = {}; o18[\n'k']()", "o18[\n'k'] is not a function. (In 'o18[\n'k']()', 'o18[\n'k']' is undefined)");
}

#[test]
fn dot_call_split_across_three_lines() {
    assert_type_error(
        "var a7 = {}; a7\n  .b\n  ()",
        "a7\n  .b\n   is not a function. (In 'a7\n  .b\n  ()', 'a7\n  .b\n  ' is undefined)",
    );
}

#[test]
fn spread_call_of_missing_method() {
    assert_type_error("var a9 = {}; a9.b(...[1])", "a9.b is not a function. (In 'a9.b(...[1])', 'a9.b' is undefined)");
}

#[test]
fn comma_expression_callee() {
    assert_type_error("var a10 = {}; (0, a10.b)()", "(0, a10.b) is not a function. (In '(0, a10.b)()', '(0, a10.b)' is undefined)");
}

#[test]
fn this_member_call() {
    assert_type_error("this.zz()", "this.zz is not a function. (In 'this.zz()', 'this.zz' is undefined)");
}

#[test]
fn computed_key_call() {
    assert_type_error("var a11x = {}; a11x[1+1]()", "a11x[1+1] is not a function. (In 'a11x[1+1]()', 'a11x[1+1]' is undefined)");
}

// Construção, literais e o caso "near".

#[test]
fn construct_of_missing_member() {
    assert_type_error("var o16 = {}; new o16.k()", "undefined is not a constructor (evaluating 'new o16.k()')");
}

#[test]
fn construct_of_number() {
    assert_type_error("var a12 = 1; new a12()", "1 is not a constructor (evaluating 'new a12()')");
}

#[test]
fn call_of_string_value_shows_quotes() {
    assert_type_error("var s1 = 'str'; s1()", "s1 is not a function. (In 's1()', 's1' is \"str\")");
}

#[test]
fn call_of_null_literal() {
    assert_type_error("null()", "null is not a function. (In 'null()', 'null' is null)");
}

#[test]
fn tagged_template_with_non_function_tag_uses_near() {
    assert_type_error("var a8 = 1; a8`x`", "1 is not a function (near '...a8`x`...')");
}

#[test]
fn optional_call_of_undefined_does_not_throw() {
    assert!(evaluate_indirect_eval("var a13; a13?.()").is_ok());
}
