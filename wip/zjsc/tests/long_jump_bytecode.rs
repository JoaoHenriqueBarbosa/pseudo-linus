//! Saltos longos do bytecode: o alvo fora da faixa do operando vira deslocamento 0 no operando e o
//! alvo real fica em `UnlinkedCodeBlock::out_of_line_jump_targets` (`Label::setLocation`,
//! `addOutOfLineJumpTarget`). Cada programa abaixo tem um corpo com milhares de instruções entre o
//! salto e o alvo e confere uma contagem, para que um salto resolvido como "para o próprio pc" (laço
//! infinito) ou para o lugar errado apareça como falha de contagem, não como travamento silencioso.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// Os tamanhos de corpo: cabem no operando estreito, passam do 8 bits e passam do 16 bits.
const SIZES: [usize; 3] = [200, 2000, 40000];

/// Roda `source` (uma expressão) e devolve o resultado convertido para texto.
fn run(source: &str) -> String {
    match evaluate_script(source) {
        Ok(value) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Ok(_) => panic!("o programa não devolveu string"),
        Err(_) => panic!("o programa lançou exceção"),
    }
}

/// `x += 1;` repetido `count` vezes.
fn body(count: usize) -> String {
    "x += 1;".repeat(count)
}

/// Confere que `build(body)` devolve `expected(count)` para cada tamanho.
fn check(name: &str, build: impl Fn(&str) -> String, expected: impl Fn(usize) -> String) {
    for count in SIZES {
        let program = format!("(function () {{ var x = 0; {} }})() + \"\"", build(&body(count)));
        assert_eq!(run(&program), expected(count), "{name} com {count} repetições");
    }
}

#[test]
fn for_of_with_long_body() {
    check("for-of", |b| format!("for (var i of [1]) {{ {b} }} return x;"), |n| n.to_string());
}

#[test]
fn for_of_runs_body_each_iteration() {
    check("for-of 3x", |b| format!("for (var i of [1, 2, 3]) {{ {b} }} return x;"), |n| (3 * n).to_string());
}

#[test]
fn classic_loops_with_long_body() {
    check("for", |b| format!("for (var i = 0; i < 2; i++) {{ {b} }} return x;"), |n| (2 * n).to_string());
    check("while", |b| format!("var i = 0; while (i < 2) {{ i++; {b} }} return x;"), |n| (2 * n).to_string());
    check("do-while", |b| format!("var i = 0; do {{ i++; {b} }} while (i < 2); return x;"), |n| (2 * n).to_string());
    check("for-in", |b| format!("for (var k in {{ a: 1, b: 2 }}) {{ {b} }} return x;"), |n| (2 * n).to_string());
}

#[test]
fn if_else_with_long_branches() {
    for condition in ["true", "false"] {
        check(
            "if/else",
            |b| format!("if ({condition}) {{ {b} }} else {{ x = -1; {b} x = -2; }} return x;"),
            |n| if condition == "true" { n.to_string() } else { "-2".to_string() },
        );
    }
}

#[test]
fn if_without_else_skips_long_body() {
    check("if falso", |b| format!("if (x) {{ {b} }} return x + 7;"), |_| "7".to_string());
    check("if verdadeiro", |b| format!("x = 1; if (x) {{ {b} }} return x;"), |n| (n + 1).to_string());
}

#[test]
fn logical_and_conditional_jumps_over_long_code() {
    check("ternário", |b| format!("var y = x ? (function () {{ {b} return 1; }})() : 5; return y;"), |_| "5".to_string());
    check("curto-circuito", |b| format!("var y = (x && (x = 9)) || (function () {{ {b} return x; }})(); return y;"), |n| n.to_string());
}

#[test]
fn switch_with_long_cases() {
    for key in [1, 2, 3] {
        check(
            "switch imm",
            |b| format!("switch ({key}) {{ case 1: {b} break; case 2: {b} {b} break; default: x = -7; }} return x;"),
            |n| match key {
                1 => n.to_string(),
                2 => (2 * n).to_string(),
                _ => "-7".to_string(),
            },
        );
    }
    for key in ["a", "b", "z"] {
        check(
            "switch string",
            |b| format!("switch (\"{key}\") {{ case \"a\": {b} break; case \"b\": {b} {b} break; default: x = -7; }} return x;"),
            |n| match key {
                "a" => n.to_string(),
                "b" => (2 * n).to_string(),
                _ => "-7".to_string(),
            },
        );
    }
    check(
        "switch com fallthrough",
        |b| format!("var v = x + 1; switch (v) {{ case 1: {b} case 2: {b} break; case 3: x = -1; }} return x;"),
        |n| (2 * n).to_string(),
    );
}

#[test]
fn try_catch_finally_with_long_blocks() {
    check(
        "try/catch/finally",
        |b| format!("try {{ {b} throw 1; }} catch (e) {{ {b} }} finally {{ {b} }} return x;"),
        |n| (3 * n).to_string(),
    );
    check(
        "try sem exceção",
        |b| format!("try {{ {b} }} catch (e) {{ x = -1; }} finally {{ {b} }} return x;"),
        |n| (2 * n).to_string(),
    );
    check(
        "finally com return",
        |b| format!("try {{ {b} return x; }} finally {{ {b} }}"),
        |n| n.to_string(),
    );
    check(
        "exceção atravessa o corpo longo",
        |b| format!("try {{ {b} (function () {{ throw 1; }})(); {b} }} catch (e) {{ x += 1000000; }} return x;"),
        |n| (n + 1000000).to_string(),
    );
}

#[test]
fn labeled_break_and_continue_over_long_body() {
    check(
        "continue rotulado",
        |b| format!("outer: for (var j = 0; j < 3; j++) {{ for (var i = 0; i < 2; i++) {{ if (i == 1) continue outer; {b} if (j == 2) break outer; }} }} return x;"),
        |n| (3 * n).to_string(),
    );
    check(
        "break rotulado em bloco",
        |b| format!("blk: {{ {b} if (x) break blk; x = -1; {b} }} return x;"),
        |n| n.to_string(),
    );
    check(
        "continue com corpo longo e laço aninhado",
        |b| format!("for (var i = 0; i < 3; i++) {{ if (i == 1) continue; {b} }} return x;"),
        |n| (2 * n).to_string(),
    );
}

#[test]
fn generator_with_long_loop_body() {
    // Geradores passam pelo `BytecodeRewriter` (`adjustJumpTargets`), que reescreve a tabela de saltos fora de linha.
    check(
        "gerador",
        |b| format!("function* g() {{ for (var i of [1, 2]) {{ {b} yield x; }} }} var last; for (var v of g()) last = v; return last;"),
        |n| (2 * n).to_string(),
    );
}
