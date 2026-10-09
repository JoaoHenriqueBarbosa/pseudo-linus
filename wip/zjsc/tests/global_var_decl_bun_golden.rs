//! Declarações (`var`, `function`, `let`, `const`, `class`) de nomes que já são propriedades do objeto global
//! (`undefined`, `NaN`, `Infinity`, `globalThis`, `Math`, `Array`, `eval`, `Object`, `navigator`, `self`, `global`,
//! `atob`), por script, eval direto no topo e eval indireto: a GlobalDeclarationInstantiation vista de fora.
//! `tests/golden/global_var_decl_bun.tsv` (modo, nome, fonte, resultado, separados por `\t`) sai de
//! `scripts/gen-global-var-decl-golden.js`, rodado no bun 1.4.2 com `node:vm` `runInThisContext`.
//! O resultado é `out.join('|')` (erro do eval como `ERR nome: mensagem`, depois `typeof NOME` e o descritor).
//! No modo `script` uma instanciação que lança aborta o script inteiro e o golden guarda `SCRIPTERR nome: mensagem`;
//! `evaluate_named_script_reporting_uncaught` devolve `Uncaught nome: mensagem` do valor lançado, e o runner confere
//! o `nome: mensagem` exato contra o golden.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_reporting_uncaught;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/global_var_decl_bun.tsv");

/// Mesmo texto de `PRELUDE` em `scripts/gen-global-var-decl-golden.js`.
const PRELUDE: &str = "globalThis.out = [];\nglobalThis.__g = globalThis;\nglobalThis.__gopd = Object.getOwnPropertyDescriptor;\nglobalThis.__final = function () { return out.join('|'); };\n";

/// Mesmo texto de `PROBES` (com `NAME` no lugar do nome sondado).
const PROBES: &str = r#"out.push(typeof NAME);
out.push((function () { var d = __gopd(__g, "NAME"); return d ? ('value' in d ? typeof d.value : 'accessor') + ',' + d.writable + ',' + d.enumerable + ',' + d.configurable : 'none'; })());"#;

/// Mesmo texto de `CATCH`.
const CATCH: &str = "catch (e) { out.push('ERR ' + e.name + ': ' + e.message); }";

/// `JSON.stringify(source)`: o que o eval recebe.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// O programa de `build` do gerador: prelúdio, a declaração (direta, por eval ou por eval indireto) e as sondas.
fn build(mode: &str, name: &str, source: &str) -> String {
    let probes = PROBES.replace("NAME", name);
    match mode {
        "script" => format!("{PRELUDE}{source}\n{probes}"),
        "eval" => format!("{PRELUDE}try {{ eval({}); }} {CATCH}\n{probes}", json_quote(source)),
        "indirect" => format!("{PRELUDE}try {{ (0, eval)({}); }} {CATCH}\n{probes}", json_quote(source)),
        other => panic!("modo desconhecido {other}"),
    }
}

/// Roda um caso num realm novo e devolve `out.join('|')`, ou `SCRIPTERR nome: mensagem` se o script lançou.
fn run(mode: &str, name: &str, source: &str) -> Result<String, String> {
    let program = build(mode, name, source);
    let outcome = catch_unwind(AssertUnwindSafe(|| evaluate_named_script_reporting_uncaught(&program, "global_var_decl_bun_golden.js", "__final()")));
    match outcome {
        Ok(Ok(Ok(value))) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("o resultado não é string".to_string()),
        Ok(Err(uncaught)) => Ok(format!("SCRIPTERR {}", uncaught.strip_prefix("Uncaught ").unwrap_or(&uncaught))),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

/// Pilha da thread do golden, igual à dos outros goldens.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn global_var_decl_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            global_var_decl_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn global_var_decl_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let mut columns = line.splitn(4, '\t');
        let mode = columns.next().expect("modo");
        let name = columns.next().expect("nome");
        let source = columns.next().expect("fonte");
        let expected = columns.next().expect("resultado");
        total += 1;
        let label = format!("{mode} {name}: {source}");
        match run(mode, name, source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{label}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{label}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 50, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
