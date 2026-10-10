//! A pilha do código de `eval` e o `CallSite` do `Error.prepareStackTrace`, medidos no bun 1.4.2: o `SyntaxError`
//! da análise do `eval` direto passa pelo frame nativo `eval`, o `//# sourceURL=` nomeia o frame, a estrita se
//! propaga para os frames mais antigos, o frame que fez tail call não casa com `captureStackTrace` e o
//! `getScriptId` do primeiro script de um programa é 2.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como texto.
fn run(source: &str) -> String {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
}

/// As linhas de `stack` sem a posição: `at nome (url` até o primeiro `:` da URL.
fn frames(stack: &str) -> Vec<String> {
    stack.lines().skip(1).map(|line| line.trim().split(':').next().unwrap_or_default().to_owned()).collect()
}

#[test]
fn eval_syntax_error_passes_through_the_native_eval_frame() {
    for program in ["var", "1 +", "let a; let a"] {
        let stack = run(&format!("'use strict'; try {{ eval({program:?}); }} catch (e) {{ e.stack }}"));
        assert_eq!(frames(&stack)[..2], ["at <parse> (", "at eval (unknown)"], "{program}: {stack}");
    }
}

#[test]
fn source_url_directive_names_the_eval_frame() {
    let stack = run("try { eval('throw new Error(1)\\n//# sourceURL=named_eval.js'); } catch (e) { e.stack }");
    assert_eq!(stack.lines().nth(1).map(str::trim), Some("at <anonymous> (named_eval.js:1:16)"), "{stack}");
    let stack = run("try { Function('//# sourceURL=fn.js\\nthrow new Error(1)')(); } catch (e) { e.stack }");
    assert!(stack.lines().nth(1).is_some_and(|line| line.trim().starts_with("at anonymous (fn.js:")), "{stack}");
}

#[test]
fn strictness_spreads_to_older_frames_and_native_frames_follow_the_receiver() {
    let hook = "Error.prepareStackTrace = (e, cs) => cs.map(c => c.getFunctionName() + ':' + c.isToplevel() + ':' + typeof c.getFunction()).join(' ');";
    // O builtin `reduce` (estrito no JSC) no topo não é estrito por si: a função aparece e o receptor `[]` não é global.
    let reduce = run(&format!("{hook} try {{ [].reduce((a, b) => a); }} catch (e) {{ e.stack }}"));
    assert!(reduce.starts_with("reduce:false:function"), "{reduce}");
    // Abaixo de um frame estrito, o `Function` sloppy e o `eval` nativo ficam como estritos.
    let below = run(&format!(
        "{hook} function mk() {{ 'use strict'; return Error('x').stack; }} [Function('mk', 'return mk()')(mk), eval('mk()')].join('|')"
    ));
    assert_eq!(below, "mk:true:undefined anonymous:true:undefined :true:undefined|mk:true:undefined :true:undefined eval:true:undefined :true:undefined");
}

#[test]
fn tail_calling_frame_is_not_the_capture_stack_trace_caller() {
    let source = "'use strict'; var o = {}; var f = () => Error.captureStackTrace(o, f); f(); \
                  var p = {}; var g = () => { Error.captureStackTrace(p, g); }; g(); [o.stack, p.stack.split('\\n').length].join('|')";
    assert_eq!(run(source), "Error|2");
}

#[test]
fn script_ids_start_at_two_in_each_program() {
    for _ in 0..2 {
        let ids = run("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getScriptId()).join(); eval('new Error(\"x\").stack')");
        assert_eq!(ids, "2,0,3");
    }
}
