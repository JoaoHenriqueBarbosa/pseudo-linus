//! A pilha do código de `eval` e o `CallSite` do `Error.prepareStackTrace`, medidos no bun 1.4.2: o `SyntaxError`
//! da análise do `eval` direto passa pelo frame nativo `eval`, o `//# sourceURL=` nomeia o frame, a estrita se
//! propaga para os frames mais antigos, o frame que fez tail call não casa com `captureStackTrace` e o
//! `getScriptId` do primeiro script de um programa é 2.
use zjsc::api::eval::evaluate_script;

/// Roda um programa e devolve o valor de conclusão como texto.
fn run(source: &str) -> String {
    let value = evaluate_script(source)
        .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
    text(value)
}

fn text(value: zjsc::runtime::js_value::JSValue) -> String {
    String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()
}

/// Roda um programa no wrapper CJS (`/case.cjs`, como um `.cjs` do bun) e devolve o texto de `R`.
fn run_cjs(source: &str) -> String {
    text(zjsc::api::eval::evaluate_cjs_program(source, "/case.cjs", "R", &[]).unwrap_or_else(|_| panic!("lançou: {source}")))
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

/// O arquivo que o bun roda passou pelo transpilador, que tira os comentários: o `//# sourceURL=` dele não nomeia
/// nada, e o do `eval` lá dentro continua valendo.
#[test]
fn source_url_directive_of_the_main_file_is_stripped() {
    let source = "var R = [new Error(1).stack, eval('new Error(2).stack\\n//# sourceURL=inner.js')].map(s => s.split('\\n')[1].trim()).join('|');\n//# sourceURL=main.js\n";
    let value = zjsc::api::eval::evaluate_named_script_result(source, "case.js", "R").expect("R");
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned();
    let urls: Vec<&str> = text.split('|').map(|frame| frame.split(':').next().unwrap_or_default()).collect();
    // Medido no bun 1.4.2: `at <arquivo>:1:14|at <anonymous> (inner.js:1:10)`.
    assert_eq!(urls, ["at case.js", "at <anonymous> (inner.js"], "{text}");
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
fn bound_caller_matches_its_target_frame_once() {
    let source = "function a() { var o = {}; Error.captureStackTrace(o, a.bind(null)); return o.stack.split('\\n').length; } \
                  function d() { var o = {}; Error.captureStackTrace(o, d.bind(null).bind(1)); return o.stack; } [a(), d()].join('|')";
    assert_eq!(run(source), "2|Error");
}

#[test]
fn result_is_read_through_the_global_this_binding() {
    let read = |source: &str| {
        let value = zjsc::api::eval::evaluate_named_script_result(source, "eval_case.js", "R").expect("leitura de R");
        if value.is_undefined() { "<undefined>".to_owned() } else { String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned() }
    };
    assert_eq!(read("globalThis.R = 'a';"), "a");
    // O gerador do bun lê `globalThis.R`: depois de `globalThis = 1` a leitura é `(1).R`.
    assert_eq!(read("globalThis.R = 'a'; globalThis = 1;"), "<undefined>");
}

#[test]
fn script_ids_start_at_two_in_each_program() {
    for _ in 0..2 {
        let ids = run("Error.prepareStackTrace = (e, cs) => cs.map(c => c.getScriptId()).join(); eval('new Error(\"x\").stack')");
        assert_eq!(ids, "2,0,3");
    }
}

/// Em código sloppy, o frame de host em C++ (`JSON.parse`, `sort`) é sempre toplevel e o builtin em JS (`reduce`) segue
/// o receptor (medido no bun 1.4.2 com um `.cjs`).
#[test]
fn host_frames_are_toplevel_and_js_builtins_follow_the_receiver() {
    let source = "var r = []; Error.prepareStackTrace = (e, s) => s.map(x => x.getFunctionName() + ':' + x.isToplevel()).join(); \
         JSON.parse('[1]', function (k, v) { if (k === '0') r.push(new Error('x').stack); return v; }); \
         [3, 1].sort(function (a, b) { r.push(new Error('w').stack); return a - b; }); \
         [1].reduce(function (a) { r.push(new Error('y').stack); return a; }, 0); R = r.join('|');";
    assert_eq!(run_cjs(source), ":false,parse:true,:false|:false,sort:true,:false|:false,reduce:false,:false");
}

/// O `SyntaxError` do `module._compile` nomeia o frame de parse com o nome dado e a linha no fonte embrulhado.
#[test]
fn compile_syntax_error_parse_frame_has_the_file_name() {
    let source = "var r = []; for (const t of ['(', ';;)']) { try { module._compile(t, 'y.js'); } catch (e) { r.push(e.stack.split('\\n')[1].trim()); } } R = r.join('|');";
    assert_eq!(run_cjs(source), "at <parse> (y.js:2)|at <parse> (y.js:2)");
}

/// Medido no bun 1.4.2: o erro cujo frame de cima é um builtin em JS (`native:1:11`) só ganha `line`, `column` e
/// `stack`; o criado em código do usuário ganha também `originalLine`, `originalColumn` e `sourceURL`.
#[test]
fn error_from_a_js_builtin_frame_has_no_original_position() {
    let keys = "e => Object.getOwnPropertyNames(e).join()";
    let source = format!(
        "const k = {keys}; const r = []; \
         const s = new DisposableStack(); s.defer(() => {{ throw 1 }}); s.defer(() => {{ throw 2 }}); \
         try {{ s.dispose() }} catch (e) {{ r.push(k(e)) }} \
         try {{ [].reduce((a, b) => a) }} catch (e) {{ r.push(k(e)) }} \
         r.push(k(new SuppressedError(1, 2))); r.join('|')"
    );
    assert_eq!(
        run(&source),
        "error,suppressed,line,column,stack|message,line,column,stack|error,suppressed,originalLine,originalColumn,line,column,sourceURL,stack"
    );
}
