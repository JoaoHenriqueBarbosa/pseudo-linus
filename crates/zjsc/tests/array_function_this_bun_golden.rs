//! `JSFunction` como `this` de `Array.prototype.*`: o C++ só usa `JSObject*`, e o `length` preguiçoso da função
//! (aqui 2) tem de ser visto pelo `getOwnPropertySlot` virtual. Valores medidos no bun 1.4.2.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como booleano.
fn is_true(source: &str) -> bool {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    value.is_true()
}

#[test]
fn read_only_methods_see_lazy_length() {
    assert!(is_true(
        "var f = function (a, b) {}; \
         Array.prototype.join.call(f, '-') === '-' && Array.prototype.indexOf.call(f, 2) === -1 \
         && JSON.stringify(Array.prototype.slice.call(f)) === '[null,null]' \
         && Array.prototype.concat.call(f).length === 1 && Array.prototype.concat.call(f)[0] === f"
    ));
}

#[test]
fn array_from_with_function_constructor() {
    assert!(is_true("var f = function (a, b) {}; Array.from.call(f, [1, 2]).length === 2 && Array.from.call(f, [1, 2])[1] === 2"));
}

#[test]
fn push_and_pop_hit_readonly_length() {
    assert!(is_true(
        "var f = function (a, b) {}; var m1, m2; \
         try { Array.prototype.push.call(f, 1); } catch (e) { m1 = e.constructor === TypeError && e.message; } \
         try { Array.prototype.pop.call(function () {}); } catch (e) { m2 = e.constructor === TypeError && e.message; } \
         m1 === 'Attempted to assign to readonly property.' && m2 === 'Attempted to assign to readonly property.'"
    ));
}

#[test]
fn function_with_own_elements_and_redefined_length() {
    assert!(is_true(
        "var f = function (a, b) {}; f.x = 5; f[0] = 'a'; Object.defineProperty(f, 'length', { value: 1 }); \
         Array.prototype.join.call(f, '+') === 'a' && Array.prototype.map.call(f, x => x + '!').join() === 'a!'"
    ));
}
