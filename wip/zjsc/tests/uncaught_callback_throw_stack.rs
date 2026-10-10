//! O relato da exceção não capturada que sai de um callback (timer, microtarefa, ouvinte): o bun relata a
//! `JSC::Exception` e usa a pilha dela (a do `throw`), não a do erro (a da construção). Medido no bun 1.4.2.
mod common;

fn report(program: &str) -> String {
    let console = std::rc::Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    let _ = zjsc::api::eval::evaluate_main_script_reporting_hold(program, "/app/main.js", console.clone());
    String::from_utf8_lossy(&console.stderr_bytes()).into_owned()
}

/// O relato de uma linha só: o fonte, o `^` sob a coluna (depois do `1 | `), a mensagem e os frames.
fn expected(program: &str, column: usize, frames: &[&str]) -> String {
    let frames: String = frames.iter().map(|frame| format!("      at {frame}\n")).collect();
    format!("1 | {program}\n{}^\nerror: boom\n{frames}\nBun v1.4.2 (Linux x64)\n", " ".repeat(column + 3))
}

#[test]
fn callback_report_uses_the_throw_stack() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let rethrown = "function g(e) { throw e } setTimeout(function () { var e = new Error('boom');   g(e) }, 1)";
        assert_eq!(report(rethrown), expected(rethrown, 23, &["g (/app/main.js:1:23)", "<anonymous> (/app/main.js:1:81)"]));
        let microtask = "queueMicrotask(function () { var e = new Error('boom');   throw e })";
        assert_eq!(report(microtask), expected(microtask, 65, &["<anonymous> (/app/main.js:1:65)"]));
        // O frame da função de host (`dispatchEvent`), sem URL e sem posição, não sai.
        let dispatched = "setTimeout(function () { var t = new EventTarget(); t.addEventListener('x', function () { throw new Error('boom') }); t.dispatchEvent(new Event('x')) }, 1)";
        assert_eq!(
            report(dispatched),
            expected(dispatched, 113, &["<anonymous> (/app/main.js:1:113)", "<anonymous> (/app/main.js:1:121)"])
        );
    });
}
