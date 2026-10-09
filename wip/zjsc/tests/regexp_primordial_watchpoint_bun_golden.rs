//! `regExpExecWatchpointIsValid` (RegExpPrototypeInlines.h) em `RegExp.prototype.test` e `[Symbol.search]`: com as
//! propriedades primordiais de `RegExp.prototype` intactas, um objeto não-RegExp que herda dele lança "Builtin
//! RegExp exec can only be called on a RegExp object" antes de ler `lastIndex`; depois de `exec`, `sticky` etc.
//! serem escritos, redefinidos ou apagados (mesmo com o mesmo valor) o `search` genérico lê `lastIndex` primeiro.
//! Esperado: bun 1.4.2 (`/tmp/w2.js`, `/tmp/w3.js`).
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const PRELUDE: &str = r#"
function run(call){var L=[];var o=Object.create(RegExp.prototype);
  Object.defineProperty(o,'lastIndex',{get(){L.push('get');return 0},set(v){L.push('set')}});
  var r;try{r=String(call(o))}catch(e){r=e.message}return call.name+":"+r+"|"+L.join(',')}
function test(o){return o.test('a')}
function search(o){return RegExp.prototype[Symbol.search].call(o,'a')}
"#;

const MSG: &str = "Builtin RegExp exec can only be called on a RegExp object";

fn eval(body: &str) -> String {
    let source = format!("{PRELUDE}globalThis.R = JSON.stringify((()=>{{{body}}})());");
    common::guarded(|| evaluate_named_script_result(&source, "regexp_primordial_watchpoint.js", "R")).expect("avaliação")
}

fn expected(test_log: &str, search_log: &str) -> String {
    format!("[\"test:{MSG}|{test_log}\",\"search:{MSG}|{search_log}\"]")
}

#[test]
fn intact_primordials_throw_before_reading_last_index() {
    assert_eq!(eval("return [run(test),run(search)]"), expected("", ""));
}

#[test]
fn same_value_write_of_exec_invalidates_the_watchpoint() {
    assert_eq!(eval("RegExp.prototype.exec=RegExp.prototype.exec;return [run(test),run(search)]"), expected("", "get"));
}

#[test]
fn redefining_a_watched_property_invalidates_the_watchpoint() {
    assert_eq!(
        eval("Object.defineProperty(RegExp.prototype,'sticky',{get(){return false},configurable:true});return [run(test),run(search)]"),
        expected("", "get")
    );
}

#[test]
fn deleting_a_watched_property_invalidates_the_watchpoint() {
    assert_eq!(eval("delete RegExp.prototype.flags;return [run(test),run(search)]"), expected("", "get"));
}
