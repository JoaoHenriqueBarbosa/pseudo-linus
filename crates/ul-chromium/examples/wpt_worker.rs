//! Roda UM arquivo do web-platform-tests no `ul-chromium` e imprime o resultado por subteste em JSON.
//!
//! Uso: `wpt_worker <raiz do wpt> <caminho da URL> [<arquivo html que substitui a página>]`.
//! A página é servida em `http://web-platform.test:8000<caminho>`, o resto do site sai da raiz do wpt (com os
//! cabeçalhos do `.headers` ao lado) e o `/resources/testharnessreport.js` é o relator daqui: ao fim do
//! `testharness.js` ele escreve uma linha `__WPT_RESULT__{...}` no console, que este programa lê e reimprime.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use ul_chromium::fetch::Fetcher;
use ul_chromium::page::Page;
use ul_net::net::client::{FetchError, Response, Transfer};
use ul_net::net::url;

const REPORTER: &str = r#"add_completion_callback(function (tests, harness) {
  console.log('__WPT_RESULT__' + JSON.stringify({
    harness: { status: harness.status, message: harness.message || null },
    tests: tests.map(function (t) { return { name: t.name, status: t.status, message: t.message || null }; })
  }));
});
"#;

const MARKER: &str = "__WPT_RESULT__";

/// O console da página, guardado para ler o resultado e, sem ele, mostrar o que a página escreveu.
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

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "xhtml" | "xht" => "application/xhtml+xml",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "xml" => "text/xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Os cabeçalhos do `<arquivo>.headers` do wpt (`Nome: valor` por linha).
fn sidecar_headers(file: &std::path::Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut name = file.as_os_str().to_owned();
    name.push(".headers");
    let Ok(text) = std::fs::read_to_string(PathBuf::from(name)) else { return Vec::new() };
    text.lines().filter_map(|line| line.split_once(':')).map(|(n, v)| (n.trim().to_ascii_lowercase().into_bytes(), v.trim().as_bytes().to_vec())).collect()
}

fn serve(root: PathBuf, page_path: String, page_override: Option<Vec<u8>>) -> Fetcher {
    Rc::new(move |target: &str, _spec: &ul_net::net::client::RequestSpec<'_>| {
        let parsed = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
        let full = parsed.target();
        let path = full.split(['?', '#']).next().unwrap_or("").to_owned();
        let file = root.join(path.trim_start_matches('/'));
        let mut headers = vec![(b"content-type".to_vec(), content_type(&path).as_bytes().to_vec())];
        let body = if path == "/resources/testharnessreport.js" {
            Some(REPORTER.as_bytes().to_vec())
        } else if path == page_path && page_override.is_some() {
            page_override.clone()
        } else if file.is_file() && !path.contains("..") {
            headers.extend(sidecar_headers(&file));
            std::fs::read(&file).ok()
        } else {
            None
        };
        let status = if body.is_some() { 200 } else { 404 };
        let body = body.unwrap_or_default();
        let transfer = Transfer { version: (1, 1), ..Transfer::identity(body.len()) };
        Ok(Response { status, reason: String::new(), headers, body, url: parsed, transfer, head: Vec::new(), sent: Vec::new(), hops: Vec::new() })
    })
}

fn failure(status: &str, message: &str) -> String {
    format!("{{\"harness\":{{\"status\":\"{status}\",\"message\":{message:?}}},\"tests\":[]}}")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: wpt_worker <raiz do wpt> <caminho da URL> [arquivo html]");
        std::process::exit(2);
    }
    let page_override = args.get(3).map(|file| std::fs::read(file).expect("arquivo da página"));
    let (root, page_path) = (PathBuf::from(&args[1]), args[2].clone());
    // Como o wptserve: os testes `.https.` rodam na origem segura, os outros na `http`.
    let origin = if args[2].contains(".https.") { "https://web-platform.test:8443" } else { "http://web-platform.test:8000" };
    let address = format!("{origin}{}", args[2]);
    // Pilha grande, como o worker do test262: o motor recorre fundo em páginas com muitos subtestes.
    let outcome = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let fetcher = serve(root, page_path, page_override);
            let mut page = match Page::load(&address, fetcher) {
                Ok(page) => page,
                Err(error) => return failure("LOAD_ERROR", &error),
            };
            // Tempo virtual: o `timeout=long` do testharness (60 s) cabe sem esperar o relógio de parede.
            page.budget_ms = 75_000.0;
            let console = Rc::new(Capture::default());
            page.console = console.clone();
            page.run();
            let text = console.0.borrow().clone();
            match text.lines().rev().find_map(|line| line.strip_prefix(MARKER)) {
                Some(json) => json.to_owned(),
                None => {
                    let tail: String = text.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
                    failure("NORESULT", &tail)
                }
            }
        })
        .expect("thread")
        .join();
    println!("{}", outcome.unwrap_or_else(|_| failure("CRASH", "panic")));
}
