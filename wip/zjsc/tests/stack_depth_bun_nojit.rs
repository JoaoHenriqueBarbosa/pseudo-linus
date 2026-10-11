//! Profundidade de recursão contra o bun 1.4.2 com `BUN_JSC_useJIT=0` (o LLInt; com o JIT a profundidade varia de
//! execução para execução). No bun os frames do LLInt e os do C++ dividem a mesma pilha nativa: cada reentrada de
//! uma função nativa no JS paga o frame nativo dela (`VM::enter_vm`, `stack_cost`). Cada caso roda dentro de
//! `(0,eval)(...)`, como o gerador mediu, e grava em `d` o último nível que chegou a rodar.
use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// (forma, profundidade do bun sem JIT).
const CASES: &[(&str, usize)] = &[
    ("let d=0;function f(n){d=n;f(n+1)}", 45594),
    ("let d=0;function f(n){let a=n,b=a+1,c=b*2,e=c-1;d=n;return f(n+1)+a+b+c+e}", 35462),
    ("let d=0;function f(n){d=n;'x'.replace('x',()=>f(n+1))}", 3470),
    ("let d=0;function f(n){d=n;[2,1].sort(()=>f(n+1))}", 1319),
    ("let d=0;function f(n){d=n;[n+1].forEach(f)}", 17732),
    ("let d=0;function f(n){d=n;[n+1].map(f)}", 16798),
    ("let d=0;function f(n){d=n;Array.from([n+1],f)}", 13877),
    ("let d=0;function f(n){d=n;JSON.parse('1',()=>f(n+1))}", 1200),
    ("let d=0;const o={get g(){return f(d+1)}};function f(n){d=n;return o.g}", 3990),
    ("let d=0;function f(n){d=n;return String({toString(){return f(n+1)}})}", 3325),
];

fn run(source: String) -> String {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            zjsc::runtime::vm::VM::set_thread_stack_budget(900 << 20);
            let value = evaluate_named_script_result(&source, "depth.js", "R").expect("o programa roda");
            String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
        })
        .expect("thread")
        .join()
        .expect("thread terminou sem pânico")
}

#[test]
fn reentry_depths_match_bun_without_jit() {
    let mut failures = Vec::new();
    for &(shape, expected) in CASES {
        let program = format!("var R = String((0,eval)(\"(function(){{{shape};try{{f(1)}}catch(e){{}};return d}})()\"));");
        let depth = run(program);
        if depth != expected.to_string() {
            failures.push(format!("{shape}: {depth} (bun {expected})"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A recursão comum no topo do programa, sem `eval`: só os frames nativos abaixo da primeira entrada.
#[test]
fn top_level_depth_matches_bun_without_jit() {
    assert_eq!(run("let d=0;function f(){d++;f()}try{f()}catch(e){};var R=String(d);".to_string()), "53213");
}

/// A cadeia mais longa de `bind` que se consegue chamar: cada nível é `boundFunctionCall` chamando a próxima função
/// ligada por `vmEntryToNative`. Medido no bun sem JIT, com alvo JS ou nativo (`Math.abs`), e com um argumento ligado
/// por nível (os argumentos crescem a cada nível).
#[test]
fn bound_chain_depths_match_bun_without_jit() {
    let program = r#"
function deepest(make, hi) {
  let lo = 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    let ok;
    try { make(mid)(); ok = true; } catch (e) { ok = false; }
    if (ok) lo = mid; else hi = mid - 1;
  }
  return lo;
}
const plain = n => { let f = function () { return 1 }; for (let i = 0; i < n; i++) f = f.bind(null); return f; };
const native = n => { let f = Math.abs; for (let i = 0; i < n; i++) f = f.bind(null); return f; };
const oneArg = n => { let f = function () { return arguments.length }; for (let i = 0; i < n; i++) f = f.bind(null, 1); return f; };
var R = [deepest(plain, 60000), deepest(native, 60000), deepest(oneArg, 3000)].join(" ");
"#;
    assert_eq!(run(program.to_string()), "13880 13880 1506");
}
