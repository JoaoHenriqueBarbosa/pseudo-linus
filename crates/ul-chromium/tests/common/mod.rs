//! O que os testes de integração do navegador compartilham.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ul_chromium::fetch::Fetcher;
use ul_chromium::page::Page;
use ul_net::net::client::{FetchError, Response, Transfer};
use ul_net::net::url;

/// O console da página, guardado para o teste ler.
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

/// Serve `files` (caminho, corpo) em qualquer host; o resto é 404. `headers` entram em toda resposta. Responde em
/// HTTP/1.0, como o `http.server` do Python que serve as medições do oráculo.
fn serve(files: &[(&str, &str)], headers: &[(&str, &str)]) -> Fetcher {
    let files: Vec<(String, String)> = files.iter().map(|(p, b)| ((*p).to_owned(), (*b).to_owned())).collect();
    let extra: Vec<(Vec<u8>, Vec<u8>)> = headers.iter().map(|(n, v)| (n.as_bytes().to_vec(), v.as_bytes().to_vec())).collect();
    Rc::new(move |target: &str, _spec: &ul_net::net::client::RequestSpec<'_>| {
        let parsed = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
        let found = files.iter().find(|(p, _)| *p == parsed.target());
        let (status, body) = match found {
            Some((p, b)) => (200, (p.clone(), b.clone())),
            None => (404, (String::new(), String::new())),
        };
        let kind = if body.0.ends_with(".js") { "text/javascript" } else { "text/html; charset=utf-8" };
        let mut response_headers = vec![(b"content-type".to_vec(), kind.as_bytes().to_vec())];
        response_headers.extend(extra.iter().cloned());
        let body = body.1.into_bytes();
        let transfer = Transfer { version: (1, 0), ..Transfer::identity(body.len()) };
        Ok(Response { status, reason: String::new(), headers: response_headers, body, url: parsed, transfer })
    })
}

/// Roda a página servida de `files` (com as fontes da imagem) e devolve o DOM despejado e o console.
#[allow(dead_code)]
pub fn run(files: &[(&str, &str)]) -> (String, String) {
    run_at("http://t.test/", &[], files)
}

/// Como `run`, com o endereço da página e os cabeçalhos extras de toda resposta (COOP e COEP, por exemplo).
#[allow(dead_code)]
pub fn run_at(address: &str, headers: &[(&str, &str)], files: &[(&str, &str)]) -> (String, String) {
    run_at_for(address, headers, files, None)
}

/// `run_at` com o `--virtual-time-budget` em `budget_ms` (sem ele, o padrão do `Page`).
pub fn run_at_for(address: &str, headers: &[(&str, &str)], files: &[(&str, &str)], budget_ms: Option<f64>) -> (String, String) {
    install_image_fonts();
    let mut page = Page::load(address, serve(files, headers)).unwrap();
    if let Some(budget) = budget_ms {
        page.budget_ms = budget;
    }
    let console = Rc::new(Capture::default());
    page.console = console.clone();
    page.run();
    let dom = page.dump_dom();
    let log = console.0.borrow().clone();
    (dom, log)
}

/// Como `run_at`, seguindo as navegações que a página pede (`location.href = ...`) na aba `tab`, como o `chromium` do
/// pseudo-linus faz; devolve o console de todos os documentos.
#[allow(dead_code)]
pub fn run_in_tab(address: &str, files: &[(&str, &str)], tab: Rc<ul_chromium::session::Tab>) -> String {
    install_image_fonts();
    let console = Rc::new(Capture::default());
    let mut next = Some(address.to_owned());
    while let Some(address) = next.take() {
        let mut page = Page::load(&address, serve(files, &[])).unwrap();
        page.enter_tab(tab.clone());
        page.console = console.clone();
        page.run();
        next = page.pending_navigation().map(|target| url::resolve(&url::parse(&address, false).unwrap(), &target).unwrap().to_string_with(true));
    }
    let log = console.0.borrow().clone();
    log
}

/// Roda uma sonda servida em `/` e devolve o JSON que ela grava em `data-out` (sem ele, falha mostrando o console).
#[allow(dead_code)]
pub fn probe_output(probe: &str) -> serde_json::Value {
    let (dom, log) = run(&[("/", probe)]);
    let start = dom.find("data-out=\"").unwrap_or_else(|| panic!("sem data-out; console:\n{log}")) + 10;
    serde_json::from_str(&dom[start..start + dom[start..].find('"').unwrap()].replace("&quot;", "\"")).unwrap()
}

/// As fontes da imagem do kernel: (caminho na VFS, bytes), o que o pseudo-linus tem em `/usr/share/fonts`.
pub fn image_fonts() -> Vec<(String, Vec<u8>)> {
    let image = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../kernel/image");
    let mut paths = Vec::new();
    collect(&image.join("usr/share/fonts"), &mut paths);
    paths.sort();
    paths.into_iter().map(|p| (format!("/{}", p.strip_prefix(&image).unwrap().display()), std::fs::read(&p).unwrap())).collect()
}

/// Instala as fontes da imagem na thread do teste, para a página rodada fora de um pseudo-processo medir texto como
/// no sistema de verdade.
#[allow(dead_code)]
pub fn install_image_fonts() {
    ul_chromium::fonts::install(image_fonts().into_iter().map(|(_, bytes)| bytes).collect());
}

/// O TestKit com as fontes da imagem na VFS.
#[allow(dead_code)]
pub fn with_image_fonts(mut kit: sysabi::testkit::TestKit) -> sysabi::testkit::TestKit {
    for (path, bytes) in image_fonts() {
        kit = kit.file(&path, bytes, 0o644);
    }
    kit
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "ttf" || e == "otf") {
            out.push(path);
        }
    }
}
