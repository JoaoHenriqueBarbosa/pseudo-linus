//! `delete x` dentro de `with (proxy)` (sloppy): `DeleteResolveNode` emite `resolve_scope` + `del_by_id`,
//! e a sequência de traps é a do `JSScope::resolve` (`has`, depois o `get` de `Symbol.unscopables`) seguida
//! do `deleteProperty` na base resolvida. Sequências medidas no bun 1.4.2 (sem `getOwnPropertyDescriptor`
//! nem `get` da própria chave em nenhum dos casos). O `delete` fica sozinho em funções auxiliares: qualquer
//! outro identificador usado dentro do `with` (como `r = ...`) também passaria pelo trap `has`.
use zjsc::api::eval::evaluate_script;

fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

/// `mk(target, options)`: um `Proxy` que registra os traps em `log`; `dx`, `dAbsent` e `dNowhere` fazem
/// `with (p) { return delete <nome> }`.
const PRELUDE: &str = "
var log = [];
function mk(target, options) {
  options = options || {};
  return new Proxy(target, {
    has(t, k) { log.push('has ' + String(k)); return options.has ? options.has(k) : Reflect.has(t, k); },
    get(t, k, r) { log.push('get ' + String(k)); return Reflect.get(t, k, r); },
    deleteProperty(t, k) { log.push('deleteProperty ' + String(k)); return options.del ? options.del(k) : Reflect.deleteProperty(t, k); },
    getOwnPropertyDescriptor(t, k) { log.push('gopd ' + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  });
}
function dx(p) { with (p) { return delete x; } }
function dAbsent(p) { with (p) { return delete wpdAbsent; } }
function dNowhere(p) { with (p) { return delete wpdNowhere; } }
";

fn check(body: &str) {
    let source = format!("{PRELUDE}\n(function () {{ {body} }})()");
    assert!(is_true(&source), "{body}");
}

const UNSCOPABLES: &str = "has x,get Symbol(Symbol.unscopables)";

#[test]
fn existing_property_is_deleted() {
    check(&format!(
        "var t = {{ x: 1 }}; var p = mk(t); var r = dx(p); \
         return r === true && !('x' in t) && log.join(',') === '{UNSCOPABLES},deleteProperty x';"
    ));
}

#[test]
fn absent_property_falls_to_global_after_only_has() {
    check(
        "globalThis.wpdAbsent = 5; var p = mk({}); var r = dAbsent(p); \
         return r === true && !('wpdAbsent' in globalThis) && log.join(',') === 'has wpdAbsent';",
    );
}

#[test]
fn absent_everywhere_is_true() {
    check("var p = mk({}); var r = dNowhere(p); return r === true && log.join(',') === 'has wpdNowhere';");
}

#[test]
fn delete_property_returning_false_gives_false() {
    check(&format!(
        "var t = {{ x: 1 }}; var p = mk(t, {{ del: function () {{ return false; }} }}); var r = dx(p); \
         return r === false && ('x' in t) && log.join(',') === '{UNSCOPABLES},deleteProperty x';"
    ));
}

#[test]
fn unscopables_blocks_and_delete_goes_to_the_outer_scope() {
    check(&format!(
        "globalThis.x = 7; var t = {{ x: 1 }}; t[Symbol.unscopables] = {{ x: true }}; var p = mk(t); var r = dx(p); \
         return r === true && ('x' in t) && !('x' in globalThis) && log.join(',') === '{UNSCOPABLES}';"
    ));
}

#[test]
fn unscopables_false_does_not_block() {
    check(&format!(
        "var t = {{ x: 1 }}; t[Symbol.unscopables] = {{ x: false }}; var p = mk(t); var r = dx(p); \
         return r === true && !('x' in t) && log.join(',') === '{UNSCOPABLES},deleteProperty x';"
    ));
}

#[test]
fn has_false_skips_the_proxy() {
    check(
        "globalThis.wpdAbsent = 1; var p = mk({ x: 1 }, { has: function () { return false; } }); var r = dAbsent(p); \
         return r === true && !('wpdAbsent' in globalThis) && log.join(',') === 'has wpdAbsent';",
    );
}

#[test]
fn strict_delete_returning_false_throws_type_error() {
    check(
        "'use strict'; var p = mk({ x: 1 }, { del: function () { return false; } }); \
         try { delete p.x; return false; } catch (e) { \
           return e instanceof TypeError && e.message === 'Unable to delete property.' && log.join(',') === 'deleteProperty x'; }",
    );
}

#[test]
fn non_configurable_property_gives_false_in_sloppy() {
    check(&format!(
        "var t = {{ x: 1 }}; Object.defineProperty(t, 'x', {{ configurable: false }}); var p = mk(t); var r = dx(p); \
         return r === false && ('x' in t) && log.join(',') === '{UNSCOPABLES},deleteProperty x';"
    ));
}

#[test]
fn delete_property_trap_exception_propagates() {
    check(&format!(
        "var p = mk({{ x: 1 }}, {{ del: function () {{ throw new Error('boom'); }} }}); \
         try {{ dx(p); return false; }} catch (e) {{ \
           return e.message === 'boom' && log.join(',') === '{UNSCOPABLES},deleteProperty x'; }}"
    ));
}

#[test]
fn unscopables_proxy_is_asked_for_the_key() {
    check(&format!(
        "var t = {{ x: 1 }}; t[Symbol.unscopables] = mk({{ x: true }}); var p = mk(t); log.length = 0; var r = dx(p); \
         return r === true && ('x' in t) && log.join(',') === 'has x,get Symbol(Symbol.unscopables),get x';"
    ));
}

#[test]
fn unscopables_exception_propagates_from_resolve() {
    check(
        "var t = { x: 1 }; t[Symbol.unscopables] = new Proxy({}, { get: function () { throw new RangeError('u'); } }); \
         var p = mk(t); try { dx(p); return false; } catch (e) { \
           return e instanceof RangeError && e.message === 'u' && ('x' in t) && log.join(',') === 'has x,get Symbol(Symbol.unscopables)'; }",
    );
}

#[test]
fn second_delete_asks_has_again_and_finds_nothing() {
    check(
        "var p = mk({ x: 1 }); var r = (function () { with (p) { return [delete x, delete x]; } })(); \
         return r[0] === true && r[1] === true && log.join(',') === \
           'has x,get Symbol(Symbol.unscopables),deleteProperty x,has x';",
    );
}

#[test]
fn parenthesized_identifier_is_the_same_delete() {
    check(&format!(
        "var t = {{ x: 1 }}; var p = mk(t); var r = (function () {{ with (p) {{ return delete (x); }} }})(); \
         return r === true && !('x' in t) && log.join(',') === '{UNSCOPABLES},deleteProperty x';"
    ));
}

#[test]
fn delete_member_of_the_resolved_value_reads_it_instead() {
    check(&format!(
        "var t = {{ x: {{ y: 1 }} }}; var p = mk(t); var r = (function () {{ with (p) {{ return delete x.y; }} }})(); \
         return r === true && !('y' in t.x) && log.join(',') === '{UNSCOPABLES},get x';"
    ));
}
