//! Regressão das ropes de `JSString` (concatenação sem cópia quadrática, substring rope, limite
//! `MaxLength`). Os resultados esperados foram medidos no bun.
mod common;

use zjsc::api::eval::evaluate_script_sequence_result;

/// Roda o programa numa thread de pilha grande e devolve o texto de `R` (ou o motivo da falha).
fn run(source: &'static str) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            common::guarded(|| {
                let (_errors, result) = evaluate_script_sequence_result(&[source], "rope_case.js", "R");
                result
            })
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|_| Err("pânico na thread".to_string()))
}

#[test]
fn append_in_loop_is_linear() {
    let result = run("var s = ''; for (var i = 0; i < 100000; i++) s += 'x'; var R = s.length + ':' + s.charAt(99999) + s.charCodeAt(0);");
    assert_eq!(result, Ok("100000:x120".to_string()));
}

#[test]
fn bound_function_chain_name_only_length() {
    let result = run("var g = function f() {}; for (var i = 0; i < 50000; i++) g = g.bind(null); var R = String(g.name.length === 6 * 50000 + 1) + ':' + g.name.slice(-8);");
    assert_eq!(result, Ok("true: bound f".to_string()));
}

#[test]
fn concat_over_max_length_throws_out_of_memory() {
    let result = run("var R; try { var a = 'a'.repeat(2 ** 29); var s = a + a + a + a; R = 'sem erro ' + s.length; } catch (e) { R = e.name + ':' + e.message; }");
    assert_eq!(result, Ok("RangeError:Out of memory".to_string()));
}

#[test]
fn substring_of_rope_and_of_substring() {
    let result = run(
        "var s = ''; for (var i = 0; i < 1000; i++) s += 'abcdef'; var t = s.substring(10, 500); var u = t.substring(3, 100); \
         var R = [t.length, t.slice(0, 6), u.length, u.slice(0, 6), u === s.substring(13, 110), (u + 'Z').slice(-4)].join('|');",
    );
    assert_eq!(result, Ok("490|efabcd|97|bcdefa|true|fabZ".to_string()));
}
