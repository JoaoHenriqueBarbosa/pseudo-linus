//! Uma página carregada: busca a URL, monta o documento, roda os scripts e o ciclo de vida no motor JavaScript e
//! serializa o resultado.

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::node::NodeData;
use blitz_dom::{BaseDocument, NodeId};
use sysabi::sys;
use ul_net::net::client::RequestSpec;
use ul_net::net::url;
use zjsc::api::embed::{run_page, HostArg};
use zjsc::runtime::console_host::ConsoleHost;

use crate::bridge::{Bridge, USER_AGENT};
use crate::fetch::Fetcher;
use crate::{document, encoding};

/// O prelúdio JavaScript: o DOM e as APIs de janela escritos sobre a ponte. O valor do `let` de topo é o gancho de
/// ciclo de vida; não é propriedade do global, então a página não o enxerga por introspecção.
const PRELUDE_PARTS: [&str; 8] = [
    include_str!("../prelude/00-core.js"),
    include_str!("../prelude/10-events.js"),
    include_str!("../prelude/20-nodes.js"),
    include_str!("../prelude/30-elements.js"),
    include_str!("../prelude/40-html.js"),
    include_str!("../prelude/50-document.js"),
    include_str!("../prelude/55-xpath.js"),
    include_str!("../prelude/60-window.js"),
];
const PRELUDE_TAIL: &str = include_str!("../prelude/70-expose.js");

/// O que o Chromium faz com `console.*` no modo headless sem `--enable-logging`: nada aparece.
#[derive(Debug)]
struct SilentConsole;

impl ConsoleHost for SilentConsole {
    fn write_stdout(&self, _bytes: &[u8]) {}
    fn write_stderr(&self, _bytes: &[u8]) {}
    fn read_stdin_line(&self) -> Option<String> {
        None
    }
    fn read_stdin_rest(&self) -> Vec<u8> {
        Vec::new()
    }
}

/// Como um `<script>` do documento é executado.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScriptKind {
    /// Bloqueia o parser: roda na ordem do documento, quando o parser chega nele.
    Sync,
    /// `defer`: roda depois do parse, em ordem, antes de `DOMContentLoaded`.
    Defer,
    /// `async`: roda quando chega; aqui, depois de `DOMContentLoaded`.
    Async,
    /// `type=module`: adiado como `defer`, mas com a resolução de `import`.
    Module,
}

struct ScriptInfo {
    node: NodeId,
    kind: ScriptKind,
    src: Option<String>,
}

pub struct Page {
    pub bridge: Rc<Bridge>,
    /// Orçamento de tempo virtual para o laço de eventos depois do carregamento, em ms.
    pub budget_ms: f64,
    /// A semente do `Math.random` (o `--js-flags=--random-seed` do Chromium); `None` é entropia do sistema.
    pub random_seed: Option<u64>,
    /// Script de diagnóstico que roda antes do primeiro script da página (só a bancada usa, com `INSPECT=arquivo`).
    pub inspector: Option<String>,
    /// Para onde vai o `console.*` da página (mudo por padrão; os testes de diagnóstico trocam).
    pub console: Rc<dyn ConsoleHost>,
}

/// A ida e volta de rede de uma busca em tempo virtual: o que já estava na fila de tarefas roda antes da resposta.
/// Maior que o 1ms a que o `setTimeout(0)` é arredondado, para a resposta nunca empatar com uma tarefa já agendada.
const NETWORK_LATENCY_MS: f64 = 2.0;

impl Page {
    /// Busca `url` (`file:`, `http:` ou `https:`) e monta o documento.
    pub fn load(url: &str, fetcher: Fetcher) -> Result<Page, String> {
        let (bytes, final_url, content_type, header_csp) = fetch_document(url, &fetcher)?;
        let source = encoding::decode(&bytes, content_type.as_deref());
        let mut doc = document::parse_at(&source, &final_url);
        if header_csp {
            document::hide_nonces(&mut doc);
        }
        let bridge = Bridge {
            doc: RefCell::new(doc),
            doctype: document::sniff_doctype(&source),
            fetcher,
            url: RefCell::new(final_url),
            navigation: RefCell::new(None),
            unsynced_styles: RefCell::new(Vec::new()),
        };
        Ok(Page { bridge: Rc::new(bridge), budget_ms: 500.0, random_seed: None, inspector: None, console: Rc::new(SilentConsole) })
    }

    /// A navegação que a página pediu (`location.href = ...`) e ainda não aconteceu.
    pub fn pending_navigation(&self) -> Option<String> {
        self.bridge.navigation.borrow_mut().take()
    }

    /// O que `--dump-dom` imprime: o doctype, o `<html>` serializado e uma quebra de linha final.
    pub fn dump_dom(&self) -> String {
        self.bridge.flush_styles();
        let doc = self.bridge.doc.borrow();
        let mut out = String::new();
        if let Some(doctype) = &self.bridge.doctype {
            out.push_str(doctype);
            out.push('\n');
        }
        out.push_str(&document::outer_html(&doc, doc.root_element().id));
        out.push('\n');
        out
    }

    /// Roda os scripts na ordem do navegador, dispara `DOMContentLoaded` e `load` e deixa o laço de eventos andar até
    /// o orçamento de tempo virtual.
    pub fn run(&self) {
        let scripts = self.collect_scripts();
        let bridge = self.bridge.clone();
        let budget = self.budget_ms;
        let document_url = self.bridge.url.borrow().clone();
        let module_bridge = self.bridge.clone();
        let module_fetch: zjsc::api::browser_modules::ModuleFetch = Rc::new(move |address: &str| fetch_text(&module_bridge.fetcher, address, "*/*"));
        run_page(self.console.clone(), bridge.clone(), &document_url, module_fetch, |runner| {
            // `Number.prototype.toString(radix)` arredonda a cauda como o V8.
            zjsc::runtime::number_prototype::set_v8_radix_strings(true);
            // O valor do programa é a função gancho: o hospedeiro a guarda, sem binding global que a página veja.
            let prelude = format!("(function () {{ 'use strict';\n{}{}\nreturn hook;\n}})();", PRELUDE_PARTS.concat(), PRELUDE_TAIL);
            let hook = match runner.eval_hidden_value(&prelude) {
                Ok(hook) => hook,
                Err(error) => {
                    // O prelúdio é parte do navegador: falha dele é bug nosso e a página fica sem DOM de script.
                    eprintln!("chromium: prelúdio falhou: {error}");
                    return;
                }
            };
            runner.set_rejection_hook(&hook);
            runner.set_network_latency_ms(NETWORK_LATENCY_MS);
            if let Some(seed) = self.random_seed {
                runner.set_random_seed(seed);
            }
            if let Some(source) = &self.inspector {
                if let Err(error) = runner.eval(source, "inspector.js") {
                    eprintln!("inspector: {error}");
                }
            }
            let call = |args: &[HostArg]| {
                let _ = runner.call_hidden(&hook, args);
            };
            let run_script = |info: &ScriptInfo| {
                let node = HostArg::Num(info.node.as_u64() as f64);
                if info.kind == ScriptKind::Module {
                    // `document.currentScript` é `null` durante um módulo; a busca é do carregador de módulos.
                    let result = match &info.src {
                        Some(address) => runner.eval_module(None, Some(address)),
                        None => {
                            let text = self.bridge.doc.borrow().get_node(info.node).map(|n| n.text_content()).unwrap_or_default();
                            runner.eval_module(Some(&text), None)
                        }
                    };
                    if let Err(message) = result {
                        call(&[HostArg::Str("scriptError"), HostArg::Str(&message), HostArg::Str(info.src.as_deref().unwrap_or(&document_url))]);
                    }
                    return;
                }
                let Some((source, name)) = self.script_source(info) else {
                    call(&[HostArg::Str("fire"), node, HostArg::Str("error")]);
                    return;
                };
                call(&[HostArg::Str("currentScript"), HostArg::Num(info.node.as_u64() as f64)]);
                if let Err(message) = runner.eval(&source, &name) {
                    call(&[HostArg::Str("scriptError"), HostArg::Str(&message), HostArg::Str(&name)]);
                }
                call(&[HostArg::Str("currentScript"), HostArg::Null]);
                if info.src.is_some() {
                    call(&[HostArg::Str("fire"), node, HostArg::Str("load")]);
                }
            };
            for info in scripts.iter().filter(|s| s.kind == ScriptKind::Sync) {
                // O parser só viu o que vem antes do script: o resto do documento fica fora até ele terminar.
                let held = bridge.detach_following(info.node);
                run_script(info);
                bridge.restore_following(held);
            }
            let deferred: Vec<&ScriptInfo> = scripts.iter().filter(|s| matches!(s.kind, ScriptKind::Defer | ScriptKind::Module)).collect();
            if !deferred.is_empty() {
                // Scripts adiados e módulos esperam a rede (a busca deles e a do que já foi pedido): nesse meio tempo
                // as tarefas já na fila andam.
                runner.run_event_loop_for(2.0 * NETWORK_LATENCY_MS);
            }
            for info in deferred {
                run_script(info);
            }
            call(&[HostArg::Str("interactive")]);
            for info in scripts.iter().filter(|s| s.kind == ScriptKind::Async) {
                run_script(info);
            }
            // As folhas de `<link rel=stylesheet>` chegam antes do `load` da janela; cada uma dispara `load` ou `error`.
            for (node, ok) in self.stylesheet_results() {
                call(&[HostArg::Str("fire"), HostArg::Num(node.as_u64() as f64), HostArg::Str(if ok { "load" } else { "error" })]);
            }
            runner.run_event_loop_for(budget);
        });
    }

    /// Busca cada `<link rel=stylesheet href>` do documento e diz se chegou (status de sucesso).
    fn stylesheet_results(&self) -> Vec<(NodeId, bool)> {
        let links: Vec<(NodeId, String)> = {
            let doc = self.bridge.doc.borrow();
            let base = self.bridge.url.borrow().clone();
            let found = doc.query_selector_all("link[href]").map(|f| f.to_vec()).unwrap_or_default();
            found
                .into_iter()
                .filter_map(|id| {
                    let node = doc.get_node(id)?;
                    let rel = node.attr(markup5ever::local_name!("rel"))?.to_ascii_lowercase();
                    if !rel.split_ascii_whitespace().any(|r| r == "stylesheet") || rel.split_ascii_whitespace().any(|r| r == "alternate") {
                        return None;
                    }
                    let href = node.attr(markup5ever::local_name!("href"))?;
                    let resolved = url::parse(&base, false).ok().and_then(|b| url::resolve(&b, href).ok()).map(|u| u.to_string_with(false))?;
                    Some((id, resolved))
                })
                .collect()
        };
        let headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), "text/css,*/*;q=0.1".to_owned())];
        let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
        links.into_iter().map(|(id, address)| (id, (self.bridge.fetcher)(&address, &spec).is_ok_and(|r| r.status < 400))).collect()
    }

    /// Os `<script>` do documento, em ordem de árvore, com o jeito de executar cada um.
    fn collect_scripts(&self) -> Vec<ScriptInfo> {
        let doc = self.bridge.doc.borrow();
        let base = self.bridge.url.borrow().clone();
        let mut found = Vec::new();
        collect(&doc, doc.root_node().id, &base, &mut found);
        found
    }

    /// O texto e o nome de um script: inline, ou buscado (`None` se a busca falhar).
    fn script_source(&self, info: &ScriptInfo) -> Option<(String, String)> {
        match &info.src {
            None => {
                let doc = self.bridge.doc.borrow();
                let text = doc.get_node(info.node)?.text_content();
                Some((text, self.bridge.url.borrow().clone()))
            }
            Some(address) => Some((fetch_text(&self.bridge.fetcher, address, "*/*")?, address.clone())),
        }
    }
}

/// O texto decodificado de um subrecurso (script clássico ou módulo); `None` para falha de rede ou status de erro.
fn fetch_text(fetcher: &Fetcher, address: &str, accept: &str) -> Option<String> {
    let headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), accept.to_owned())];
    let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
    let response = fetcher(address, &spec).ok()?;
    if response.status >= 400 {
        return None;
    }
    let content_type = response.header("Content-Type");
    Some(encoding::decode(&response.body, content_type.as_deref()))
}

fn is_classic_type(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.is_empty()
        || matches!(
            value.as_str(),
            "text/javascript" | "application/javascript" | "text/ecmascript" | "application/ecmascript" | "application/x-javascript" | "application/x-ecmascript" | "text/x-javascript"
                | "text/x-ecmascript" | "text/jscript" | "text/livescript" | "text/javascript1.0" | "text/javascript1.1" | "text/javascript1.2" | "text/javascript1.3"
                | "text/javascript1.4" | "text/javascript1.5" | "text/jscript1.0"
        )
}

fn collect(doc: &BaseDocument, id: NodeId, base: &str, out: &mut Vec<ScriptInfo>) {
    let Some(node) = doc.get_node(id) else { return };
    if let NodeData::Element(element) = &node.data {
        let name: &str = &element.name.local;
        if name == "template" {
            return;
        }
        if name == "script" && &*element.name.ns == "http://www.w3.org/1999/xhtml" {
            let attr = |n: &str| element.attrs.iter().find(|a| &*a.name.local == n).map(|a| a.value.as_str());
            let kind_attr = attr("type").unwrap_or("");
            let language_ok = attr("language").is_none_or(|l| l.is_empty() || l.eq_ignore_ascii_case("javascript"));
            let is_module = kind_attr.eq_ignore_ascii_case("module");
            if (is_classic_type(kind_attr) && language_ok || is_module) && attr("nomodule").is_none() {
                let src = attr("src").filter(|s| !s.is_empty()).and_then(|s| url::parse(base, false).ok().and_then(|b| url::resolve(&b, s).ok()).map(|u| u.to_string_with(false)));
                // `src` presente mas inválido: o navegador dispara `error`; tratamos como busca que falha.
                let src = if attr("src").is_some_and(|s| !s.is_empty()) { Some(src.unwrap_or_default()) } else { None };
                let kind = if is_module {
                    ScriptKind::Module
                } else if attr("async").is_some() && src.is_some() {
                    ScriptKind::Async
                } else if attr("defer").is_some() && src.is_some() {
                    ScriptKind::Defer
                } else {
                    ScriptKind::Sync
                };
                out.push(ScriptInfo { node: id, kind, src });
            }
        }
    }
    for &child in &node.children {
        collect(doc, child, base, out);
    }
}

/// O documento: bytes, URL final, `Content-Type` e se veio CSP por cabeçalho (o que liga o nonce hiding).
fn fetch_document(address: &str, fetcher: &Fetcher) -> Result<(Vec<u8>, String, Option<String>, bool), String> {
    if let Some(path) = address.strip_prefix("file://") {
        let bytes = sys::read_file(path.as_bytes()).map_err(|_| "net::ERR_FILE_NOT_FOUND".to_owned())?;
        return Ok((bytes, address.to_owned(), None, false));
    }
    let headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8".to_owned()), ("Accept-Language".to_owned(), "en-US,en;q=0.9".to_owned())];
    let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
    let response = fetcher(address, &spec).map_err(|e| e.net_error().to_owned())?;
    let final_url = response.url.to_string_with(true);
    let content_type = response.header("Content-Type");
    let header_csp = response.header("Content-Security-Policy").is_some();
    Ok((response.body, final_url, content_type, header_csp))
}
