//! Golden de chamadas e argumentos de borda contra o JavaScriptCore do bun: `tests/golden/call_edge_bun.tsv` sai de
//! `scripts/gen-call-edge-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global
//! `R` que ele grava, depois de esvaziadas as microtarefas. Cobre `arguments` (mapeado e não mapeado), rest e default
//! params (TDZ e escopo próprio), `apply` com array-like gigante, `call` com `this` primitivo, `new` com retorno
//! primitivo ou objeto, spread de iteráveis customizados, tail call, recursão profunda, `super` em getter e setter,
//! `Function.prototype.toString` de métodos computados e `bind` com `new`, `length` e `name`.
mod common;

use common::json_string;
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::runtime::options_list::Options;
use zjsc::runtime::process_time_zone::set_time_zone_spec_override;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/call_edge_bun.tsv");

/// Roda o programa e devolve o texto de `R` (`<undefined>` quando `R` não é string), ou o motivo de não ter devolvido.
fn run(source: &str) -> Result<String, String> {
    // O programa pode falhar na compilação: o bun então deixa `R` indefinido e o golden registra isso, por isso a
    // exceção do script não encerra a medição.
    match catch_unwind(AssertUnwindSafe(|| common::EvalMode::RunInThisContext.evaluate(source, "", "R"))) {
        Ok(Ok(value)) if value.is_undefined() => Ok("<undefined>".to_string()),
        Ok(Ok(value)) => {
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Err(_)) => Err("o programa lançou exceção".to_string()),
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

/// Pilha da thread do golden: a recursão de 10000 a 1000000 níveis e os `apply` gigantes precisam de pilha grande
/// (o orçamento padrão do VM é 1 MiB).
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn call_edge_matches_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            // Folga de 16 MiB para os protetores nativos; sem isto o orçamento seria o padrão de 1 MiB.
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            // O golden saiu de um bun em fuso America/Sao_Paulo (`new Date(2020).getFullYear()` dá 1969), e o motor
            // sozinho lê o fuso do processo; fixá-lo torna o resultado independente da máquina que roda o teste.
            set_time_zone_spec_override(Some("America/Sao_Paulo"));
            // A pilha lógica de registradores fica no padrão do bun (`maxPerThreadStackUsage` = 5 MiB, zona suave de
            // 128 KiB): os 19 casos que esperam "Maximum call stack size exceeded" (recursão de 100000 níveis ou mais)
            // só estouram com este limite. Um valor maior aqui (já foi 64 MiB) esconde o estouro.
            debug_assert_eq!(Options::max_per_thread_stack_usage(), 5 * 1024 * 1024);
            call_edge_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn call_edge_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    // `CALL_EDGE_LINES=inicio-fim` (linhas do tsv, a partir de 1) mede só uma faixa, para bisseccionar lentidão;
    // com ela o piso de 1000 programas não se aplica e cada caso imprime o tempo gasto em stderr.
    let range = std::env::var("CALL_EDGE_LINES").ok().and_then(|text| {
        let (start, end) = text.split_once('-')?;
        Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
    });
    for (index, line) in GOLDEN.lines().enumerate().filter(|(_, line)| !line.is_empty()) {
        if range.is_some_and(|(start, end)| index + 1 < start || index + 1 > end) {
            continue;
        }
        let started = std::time::Instant::now();
        let (source, expected) = line.split_once('\t').expect("fonte e resultado");
        let (source, expected) = (json_string(source), json_string(expected));
        total += 1;
        match run(&source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected:?}\n    veio     {actual:?}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected:?}\n    {reason}")),
        }
        if range.is_some() {
            eprintln!("linha {}: {:?}", index + 1, started.elapsed());
        }
    }
    assert!(range.is_some() || total >= 1000, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
