//! O arquivo principal roda no modo que o bun 1.4.2 escolhe para ele (`src/api/main_module_kind.rs`): sem marcador de
//! CommonJS é módulo ES (estrito, `var` do topo fora do global), com marcador é CJS no wrapper
//! `function(exports, require, module, __filename, __dirname)`.

fn run(source: &str) -> (i32, String, String) {
    let console = std::rc::Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let (code, _) = zjsc::api::eval::evaluate_main_script_reporting_hold(source, "/app/main.js", console.clone());
    (code, String::from_utf8_lossy(&console.stdout_bytes()).into_owned(), String::from_utf8_lossy(&console.stderr_bytes()).into_owned())
}

#[test]
fn file_without_commonjs_markers_runs_as_strict_module() {
    let (code, stdout, _) = run("var x = 1; function f() { return this; } console.log(f() === undefined, 'x' in globalThis);\n");
    assert_eq!((code, stdout.as_str()), (0, "true false\n"));
}

#[test]
fn file_with_require_runs_in_the_commonjs_wrapper() {
    let (code, stdout, _) = run("console.log(typeof require, typeof module, __filename, __dirname, this === module.exports);\n");
    assert_eq!((code, stdout.as_str()), (0, "function object /app/main.js /app true\n"));
}

#[test]
fn exit_in_module_body_skips_pending_ticks() {
    let (code, stdout, _) = run("process.nextTick(function () { console.log('tick'); }); process.exit();\n");
    assert_eq!((code, stdout.as_str()), (0, ""));
}

#[test]
fn module_top_level_frame_keeps_column_one() {
    let (code, _, stderr) = run("function a() { throw new Error('x'); }\na();\n");
    assert_eq!(code, 1);
    assert!(stderr.contains("      at /app/main.js:2:1\n"), "{stderr}");
}
