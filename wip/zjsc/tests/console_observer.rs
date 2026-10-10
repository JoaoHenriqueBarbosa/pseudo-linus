//! O observador de `console` (`console_client::set_console_observer`), que o navegador usa para o
//! `Runtime.consoleAPICalled` do DevTools: cada chamada chega com o tipo no vocabulário do protocolo e os argumentos, e a
//! saída no console do host segue igual.

use std::cell::RefCell;
use std::rc::Rc;

use zjsc::api::eval::evaluate_named_script_with_console;
use zjsc::runtime::console_client::set_console_observer;
use zjsc::runtime::console_host::MemoryConsole;

#[test]
fn observer_sees_each_console_call() {
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let log = seen.clone();
    set_console_observer(Some(Rc::new(move |_global, kind, args| {
        let rendered: Vec<String> = args.iter().map(|value| String::from_utf16_lossy(&value.to_wtf_string().characters_without_null_termination().unwrap_or_default())).collect();
        log.borrow_mut().push(format!("{kind}:{}", rendered.join(",")));
    })));
    let console = Rc::new(MemoryConsole::new(b""));
    let source = "console.log('a', 1); console.warn('w'); console.error('e'); console.info('i'); console.debug('d'); \
                  console.count('c'); console.count('c'); console.table([1]); console.group('g'); console.groupEnd(); \
                  console.assert(false, 'x'); console.assert(true, 'y'); console.dir({}); var R = 1;";
    let _ = evaluate_named_script_with_console(source, "observer.js", "R", Some(console.clone()));
    set_console_observer(None);
    let seen = seen.borrow();
    assert_eq!(
        seen.iter().map(String::as_str).collect::<Vec<_>>(),
        ["log:a,1", "warning:w", "error:e", "info:i", "debug:d", "count:c,1", "count:c,2", "table:1", "startGroup:g", "endGroup:", "assert:x", "dir:[object Object]"]
    );
    // A saída do host não muda com o observador.
    assert!(console.stdout().starts_with("a 1\n"));
}
