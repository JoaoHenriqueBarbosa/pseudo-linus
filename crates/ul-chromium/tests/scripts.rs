//! Comportamento de scripts em páginas pequenas servidas da memória: o DOM despejado e o que o console recebeu.
//! Cada caso fixa uma semântica do navegador que uma página real do conjunto da internet expôs.

use std::cell::RefCell;
use std::rc::Rc;

use ul_chromium::fetch::Fetcher;
use ul_chromium::page::Page;
use ul_net::net::client::{FetchError, Response};
use ul_net::net::url;

#[derive(Debug, Default)]
struct Capture(RefCell<String>);

impl zjsc::runtime::console_host::ConsoleHost for Capture {
    fn write_stdout(&self, bytes: &[u8]) {
        self.0.borrow_mut().push_str(&String::from_utf8_lossy(bytes));
    }
    fn write_stderr(&self, bytes: &[u8]) {
        self.0.borrow_mut().push_str(&String::from_utf8_lossy(bytes));
    }
    fn read_stdin_line(&self) -> Option<String> {
        None
    }
    fn read_stdin_rest(&self) -> Vec<u8> {
        Vec::new()
    }
}

/// Serve `files` (caminho, corpo) em `http://t.test/`; o resto é 404.
fn serve(files: &[(&str, &str)]) -> Fetcher {
    let files: Vec<(String, String)> = files.iter().map(|(p, b)| ((*p).to_owned(), (*b).to_owned())).collect();
    Rc::new(move |target: &str, _spec: &ul_net::net::client::RequestSpec<'_>| {
        let parsed = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
        let found = files.iter().find(|(p, _)| *p == parsed.target());
        let (status, body) = match found {
            Some((p, b)) => (200, (p.clone(), b.clone())),
            None => (404, (String::new(), String::new())),
        };
        let kind = if body.0.ends_with(".js") { "text/javascript" } else { "text/html; charset=utf-8" };
        Ok(Response { status, reason: String::new(), headers: vec![(b"content-type".to_vec(), kind.as_bytes().to_vec())], body: body.1.into_bytes(), url: parsed })
    })
}

fn run(files: &[(&str, &str)]) -> (String, String) {
    let mut page = Page::load("http://t.test/", serve(files)).unwrap();
    let console = Rc::new(Capture::default());
    page.console = console.clone();
    page.run();
    let dom = page.dump_dom();
    let log = console.0.borrow().clone();
    (dom, log)
}

#[test]
fn console_reaches_host() {
    let (_, log) = run(&[("/", "<script>console.log('oi')</script>")]);
    assert_eq!(log, "oi\n");
}

#[test]
fn timer_exception_does_not_stop_the_page() {
    let (dom, log) = run(&[("/", "<body><script>setTimeout(() => { throw new Error('x') }, 0); setTimeout(() => document.body.append('depois'), 5)</script>")]);
    assert!(dom.contains("depois"), "{dom}");
    assert!(log.contains("Uncaught"), "{log}");
}

#[test]
fn style_property_writes_attribute() {
    let (dom, _) = run(&[("/", "<body><main id=m></main><script>document.getElementById('m').style.display = 'none'</script>")]);
    assert!(dom.contains(r#"<main id="m" style="display: none;"></main>"#), "{dom}");
}

#[test]
fn jquery_ready_runs() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testbench/corpus/internet/todomvc-jquery/bodies");
    let jquery = std::fs::read_to_string(dir.join("a0fe8723dcf560fa17b2d6c4b25f76fd88c3b09f4ffcd5bb43db8a46d13e0adb")).unwrap_or_default();
    let jquery = if jquery.is_empty() {
        let name = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).find(|n| n.starts_with("a0fe8723dcf5")).unwrap();
        std::fs::read_to_string(dir.join(name)).unwrap()
    } else {
        jquery
    };
    let (_, log) = run(&[("/", "<body><script src=/jq.js></script><script>console.log(typeof jQuery); jQuery(function () { console.log('ready') })</script>"), ("/jq.js", &jquery)]);
    assert_eq!(log, "function\nready\n");
}

/// Nenhum global de runtime de servidor (node, bun) nem nome interno da ponte aparece para a página.
#[test]
fn no_server_runtime_globals() {
    let (_, log) = run(&[("/", "<script>console.log(Object.getOwnPropertyNames(globalThis).sort().join(' '))</script>")]);
    if let Ok(path) = std::env::var("GLOBALS_OUT") {
        std::fs::write(path, log.split(' ').collect::<Vec<_>>().join("\n")).unwrap();
    }
    for name in ["process", "Bun", "require", "module", "exports", "global", "Buffer", "__dirname", "__filename", "setImmediate", "$plHook", "__pl_dom", "__pl_eval"] {
        assert!(!log.split_whitespace().any(|g| g == name), "global {name} visível");
    }
}

#[test]
fn push_apply_in_timer() {
    let script = "<script>setTimeout(function () { for (var n of [10, 1000, 70000, 200000]) { var a = []; var b = new Array(n).fill(1); a.push.apply(a, b); console.log(n, a.length) } }, 0)</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "10 10\n1000 1000\n70000 70000\n200000 200000\n");
}

#[test]
fn hidden_is_on_html_element_prototype() {
    let (_, log) = run(&[("/", "<script>console.log(Object.getOwnPropertyNames(HTMLElement.prototype).includes('hidden'), 'hidden' in Element.prototype)</script>")]);
    assert_eq!(log, "true false\n");
}
