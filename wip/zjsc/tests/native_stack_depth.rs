//! Mede o consumo de pilha NATIVA do Rust por nível de recursão JS para JS (`llint_execute` + `dispatch_loop`).
//!
//! Sem função nativa que exponha o ponteiro de pilha ao JS, a medição usa o próprio orçamento: com dois
//! orçamentos B1 < B2 (`VM::set_thread_stack_budget`) a recursão `f(n) { return 1 + f(n + 1) }` pára com
//! `RangeError` nas profundidades D1 < D2, e `(B2 - B1) / (D2 - D1)` são os bytes por nível, sem a constante
//! do prólogo. Rode com `--nocapture` para ver o número; use `--release` e o perfil de teste para os dois casos.
use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const THREAD_STACK_BYTES: usize = 512 * 1024 * 1024;
const FIRST_BUDGET: usize = 16 * 1024 * 1024;
const SECOND_BUDGET: usize = 64 * 1024 * 1024;

const PROGRAM: &str = "var depth = 0;\n\
    function f(n) { depth = n; return 1 + f(n + 1); }\n\
    try { f(1); } catch (e) { }\n\
    var R = String(depth);";

/// Profundidade JS alcançada até o `RangeError` com o orçamento nativo `budget`.
fn depth_for_budget(budget: usize) -> usize {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || {
            zjsc::runtime::vm::VM::set_thread_stack_budget(budget);
            let value = evaluate_named_script_result(PROGRAM, "native_stack_depth.js", "R").expect("o programa roda");
            let bytes = value.to_wtf_string().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).parse::<usize>().expect("profundidade numérica")
        })
        .expect("thread")
        .join()
        .expect("thread terminou sem pânico")
}

#[test]
fn native_stack_bytes_per_js_level() {
    let first = depth_for_budget(FIRST_BUDGET);
    let second = depth_for_budget(SECOND_BUDGET);
    println!("orçamento {} MiB: {} níveis; orçamento {} MiB: {} níveis", FIRST_BUDGET >> 20, first, SECOND_BUDGET >> 20, second);

    assert!(second > first, "o orçamento maior precisa render mais níveis ({first} contra {second})");
    // Abaixo do `CLoopStack` (5 MiB de registradores), o limite que mediu foi a pilha nativa.

    let per_level = (SECOND_BUDGET - FIRST_BUDGET) / (second - first);
    println!("pilha nativa por nível de JS: {per_level} bytes");
    // Piso: o frame de `dispatch_loop_from` não cabe em menos que isso; teto: acima, a thread do golden de limites
    // (1 GiB para 45609 níveis, ou seja 23 KiB por nível) deixa de bastar.
    assert!(per_level >= 256, "frame nativo implausivelmente pequeno: {per_level}");
    assert!(per_level <= 23 * 1024, "frame nativo grande demais para a thread do golden de limites: {per_level}");
}
