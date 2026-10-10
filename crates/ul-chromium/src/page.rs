//! Uma página carregada: busca a URL, monta o documento, roda os scripts e o ciclo de vida no motor JavaScript e
//! serializa o resultado.

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::node::NodeData;
use blitz_dom::{BaseDocument, NodeId};
use sysabi::sys;
use ul_net::net::client::{RequestSpec, Transfer};
use ul_net::net::url;
use zjsc::api::embed::{run_page, HostArg, HostHandle, PageRunner};
use zjsc::runtime::console_host::ConsoleHost;

use crate::bridge::{Bridge, USER_AGENT};
use crate::fetch::Fetcher;
use crate::{document, encoding};

/// O prelúdio JavaScript: o DOM e as APIs de janela escritos sobre a ponte. O valor do `let` de topo é o gancho de
/// ciclo de vida; não é propriedade do global, então a página não o enxerga por introspecção.
const PRELUDE_PARTS: [&str; 36] = [
    include_str!("../prelude/00-core.js"),
    include_str!("../prelude/10-events.js"),
    include_str!("../prelude/20-nodes.js"),
    include_str!("../prelude/30-elements.js"),
    include_str!("../prelude/35-geometry.js"),
    include_str!("../prelude/40-html.js"),
    "const WEBGL_TABLE = ",
    include_str!("../data/webgl-swiftshader.json"),
    ";\nconst WEBGL_EXTENSIONS = ",
    include_str!("../data/webgl-extensions.json"),
    ";\n",
    include_str!("../prelude/45-webgl.js"),
    include_str!("../prelude/47-glsl.js"),
    include_str!("../prelude/48-webgl-raster.js"),
    include_str!("../prelude/49-webgl-texture.js"),
    "const CANVAS2D_SURFACE = ",
    include_str!("../data/canvas2d-surface.json"),
    ";\n",
    include_str!("../prelude/46-canvas2d.js"),
    include_str!("../prelude/50-document.js"),
    include_str!("../prelude/55-xpath.js"),
    include_str!("../prelude/60-window.js"),
    include_str!("../prelude/62-window-interfaces.js"),
    include_str!("../prelude/63-window-apis.js"),
    "const TRUSTED_TYPES_MAPPING = ",
    include_str!("../data/trusted-types-mapping.json"),
    ";\n",
    include_str!("../prelude/64-trusted-types.js"),
    include_str!("../prelude/66-trusted-types-csp.js"),
    include_str!("../prelude/67-navigation.js"),
    include_str!("../prelude/68-indexeddb.js"),
    include_str!("../prelude/69-persistence.js"),
    include_str!("../prelude/65-devtools.js"),
    "const WINDOW_KEYS = ",
    include_str!("../data/window-keys.json"),
    ";\n",
];
const PRELUDE_TAIL: &str = include_str!("../prelude/70-expose.js");

/// O segredo do `TimeClamper`: o `base::RandUint64()` do processo, do `getrandom` do sandbox.
fn clock_secret() -> u64 {
    let mut bytes = [0u8; 8];
    if let Some(sys) = sys::try_current() {
        let _ = sys.getrandom(&mut bytes);
    }
    u64::from_le_bytes(bytes)
}

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
    /// O tamanho do documento, cobrado como trabalho de parse.
    document_bytes: usize,
    /// As políticas CSP dos cabeçalhos, em JSON (`[[texto, soRelatorio], ...]`), para o prelúdio.
    csp_json: String,
    /// A troca de rede do documento, em JSON, para a entrada `navigation` do prelúdio.
    transfer_json: String,
    /// O início da navegação no relógio monotônico (`ul_common::time::monotonic_nanos`): a origem de tempo da página.
    navigation_start: i128,
}

/// A ida e volta de rede de uma busca em tempo virtual: o que já estava na fila de tarefas roda antes da resposta.
/// Maior que o 1ms a que o `setTimeout(0)` é arredondado, para a resposta nunca empatar com uma tarefa já agendada.
const NETWORK_LATENCY_MS: f64 = 2.0;
/// Um microssegundo além do fim do orçamento de tempo virtual (ver [`Page::run`]).
const BUDGET_DEADLINE_SLACK_MS: f64 = 0.001;
/// O custo virtual do trabalho da página (o `--virtual-time-budget` do Chromium anda com o renderer ocupado),
/// calibrado contra o oráculo: instrução de bytecode executada e byte de fonte (documento e scripts) compilado.
const NS_PER_INSTRUCTION: f64 = 0.0;
const NS_PER_SOURCE_BYTE: f64 = 200.0;

impl Page {
    /// Busca `url` (`file:`, `http:` ou `https:`) e monta o documento.
    pub fn load(url: &str, fetcher: Fetcher) -> Result<Page, String> {
        let response = fetch_document(url, &fetcher)?;
        Ok(Page::build(response, fetcher))
    }

    /// A página de erro de rede do Chromium (`chrome-error://chromewebdata/`) para a navegação a `url` que falhou com
    /// `error` (`net::ERR_...`): o modelo do `neterror.html` com o `loadTimeDataRaw` do código, medidos no oráculo.
    /// `None` para código sem texto medido.
    pub fn error_page(url: &str, error: &str, fetcher: Fetcher) -> Option<Page> {
        let html = neterror_html(url, error)?;
        let response = DocumentResponse { bytes: html.into_bytes(), final_url: "chrome-error://chromewebdata/".to_owned(), content_type: Some("text/html".to_owned()), header_csp: false, csp: Vec::new(), cross_origin_isolated: false, transfer: None, timing: Fetched::now() };
        Some(Page::build(response, fetcher))
    }

    fn build(response: DocumentResponse, fetcher: Fetcher) -> Page {
        let parse_start = ul_common::time::monotonic_nanos();
        let transfer_json = response.transfer_json(parse_start);
        let navigation_start = response.timing.navigation_start;
        let source = encoding::decode(&response.bytes, response.content_type.as_deref());
        let net = std::sync::Arc::new(document::QueuedNet::default());
        let mut doc = document::parse_page(&source, &response.final_url, net.clone());
        if response.header_csp {
            document::hide_nonces(&mut doc);
        }
        let bridge = Bridge {
            doc: RefCell::new(doc),
            doctype: document::sniff_doctype(&source),
            fetcher,
            url: RefCell::new(response.final_url),
            navigation: RefCell::new(None),
            net,
            unsynced_styles: RefCell::new(Vec::new()),
            images: RefCell::new(Vec::new()),
            cross_origin_isolated: response.cross_origin_isolated,
            canvases: RefCell::new(Vec::new()),
            tab: RefCell::new(Rc::new(crate::session::Tab::default())),
        };
        use crate::devtools::json::Json;
        let csp_json = Json::Array(response.csp.iter().map(|(text, report_only)| Json::Array(vec![Json::str(text), Json::Bool(*report_only)])).collect()).to_compact();
        Page { bridge: Rc::new(bridge), budget_ms: 500.0, random_seed: None, inspector: None, console: Rc::new(SilentConsole), document_bytes: response.bytes.len(), csp_json, transfer_json, navigation_start }
    }

    /// A navegação que a página pediu (`location.href = ...`) e ainda não aconteceu.
    pub fn pending_navigation(&self) -> Option<String> {
        self.bridge.navigation.borrow_mut().take()
    }

    /// Põe o documento na aba `tab` (antes de rodar): o storage dela e do perfil dela passam a ser os dele.
    pub fn enter_tab(&self, tab: Rc<crate::session::Tab>) {
        *self.bridge.tab.borrow_mut() = tab;
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
        let budget = self.budget_ms;
        // O fim do orçamento é inclusivo: a tarefa marcada para o instante exato dele ainda roda (medido no oráculo, uma
        // cadeia de 50 timers de 10 ms fecha com `--virtual-time-budget=500`). A folga absorve o erro de ponto
        // flutuante da soma dos atrasos sobre uma origem fracionária.
        self.run_with(|_, _| {}, |runner, _| runner.run_event_loop_for(budget + BUDGET_DEADLINE_SLACK_MS));
    }

    /// Como [`Page::run`], com `start` chamado quando o prelúdio fica pronto (antes do primeiro script da página) e
    /// `after` no lugar do orçamento de tempo virtual: o programa e o gancho do prelúdio seguem vivos enquanto `after`
    /// roda (o laço do DevTools). `None` quando o prelúdio falhou.
    pub fn run_with<T>(&self, start: impl FnOnce(&PageRunner, &HostHandle), after: impl FnOnce(&PageRunner, &HostHandle) -> T) -> Option<T> {
        let scripts = self.collect_scripts();
        let bridge = self.bridge.clone();
        let document_url = self.bridge.url.borrow().clone();
        let module_bridge = self.bridge.clone();
        let module_fetch: zjsc::api::browser_modules::ModuleFetch = Rc::new(move |address: &str| {
            let text = fetch_text(&module_bridge.fetcher, address, "*/*");
            if let Some(text) = &text {
                zjsc::runtime::cpu_clock::charge_source_bytes(text.len());
            }
            text
        });
        run_page(self.console.clone(), bridge.clone(), &document_url, module_fetch, |runner| {
            // `Number.prototype.toString(radix)` arredonda a cauda como o V8.
            zjsc::runtime::number_prototype::set_v8_radix_strings(true);
            // `Error.prototype.stack` e as mensagens de erro no formato do V8.
            zjsc::runtime::stack_frame::set_v8_stack_format(true);
            // Sob `--virtual-time-budget`, `Date` e `performance.now` andam com o tempo virtual dos timers, a partir
            // do relógio do sistema no início da página. A origem de tempo é o início da navegação: o primeiro script
            // vê o relógio no tempo real que a busca e o parse levaram (o `Date` segue na hora de parede).
            let now_ticks = ul_common::time::monotonic_nanos();
            let elapsed_ms = (now_ticks - self.navigation_start).max(0) as f64 / 1_000_000.0;
            let wall_ms = zjsc::wtf::date_math::current_time_in_nanoseconds() as f64 / 1_000_000.0;
            zjsc::wtf::date_math::set_virtual_clock(wall_ms - elapsed_ms);
            zjsc::runtime::waiter_list_manager::advance_virtual_clock_to(elapsed_ms);
            // O relógio da página arredonda como o `TimeClamper` do Blink: 100 µs, ou 5 µs com `crossOriginIsolated`, um
            // segredo aleatório por processo e a origem no relógio monotônico do sandbox.
            let now_ticks_us = (now_ticks / 1000) as i64;
            let unix_at_zero_us = (zjsc::wtf::date_math::current_time_in_nanoseconds() / 1000) as i64 - now_ticks_us;
            let origin_ticks_us = (self.navigation_start / 1000) as i64;
            let resolution = if self.bridge.cross_origin_isolated { zjsc::runtime::time_clamper::FINE_RESOLUTION_US } else { zjsc::runtime::time_clamper::COARSE_RESOLUTION_US };
            zjsc::runtime::time_clamper::set_time_clamper(clock_secret(), resolution, origin_ticks_us, unix_at_zero_us);
            // O valor do programa é a função gancho: o hospedeiro a guarda, sem binding global que a página veja.
            let prelude = format!("(function () {{ 'use strict';\nconst DOCUMENT_CSP = {};\nconst DOCUMENT_TRANSFER = {};\n{}{}\nreturn hook;\n}})();", self.csp_json, self.transfer_json, PRELUDE_PARTS.concat(), PRELUDE_TAIL);
            let hook = match runner.eval_hidden_value(&prelude) {
                Ok(hook) => hook,
                Err(error) => {
                    // O prelúdio é parte do navegador: falha dele é bug nosso e a página fica sem DOM de script.
                    eprintln!("chromium: prelúdio falhou: {error}");
                    return None;
                }
            };
            runner.set_rejection_hook(&hook);
            // O acesso nomeado da janela (`window.x` para o elemento de id `x`), pedido ao gancho.
            if let Ok(named) = runner.call_hidden(&hook, &[HostArg::Str("namedLookup")]) {
                runner.set_global_named_lookup(&named);
            }
            // Cada chamada de `console` vai também ao agente do DevTools, que a guarda para o `Runtime.consoleAPICalled`.
            if let Ok(console) = runner.call_hidden(&hook, &[HostArg::Str("consoleHook")]) {
                runner.set_console_hook(&console);
            }
            runner.set_network_latency_ms(NETWORK_LATENCY_MS);
            if let Some(seed) = self.random_seed {
                runner.set_random_seed(seed);
            }
            start(runner, &hook);
            if let Some(source) = &self.inspector {
                if let Err(error) = runner.eval(source, "inspector.js") {
                    eprintln!("inspector: {error}");
                }
            }
            // Daqui em diante o trabalho da página custa tempo virtual, como no renderer do Chromium: o parse do
            // documento, a compilação de cada script e as instruções que ela executa.
            zjsc::runtime::cpu_clock::set_cpu_cost(NS_PER_INSTRUCTION, NS_PER_SOURCE_BYTE);
            zjsc::runtime::cpu_clock::charge_source_bytes(self.document_bytes);
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
                            zjsc::runtime::cpu_clock::charge_source_bytes(text.len());
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
                zjsc::runtime::cpu_clock::charge_source_bytes(source.len());
                if let Err(message) = runner.eval(&source, &name) {
                    call(&[HostArg::Str("scriptError"), HostArg::Str(&message), HostArg::Str(&name)]);
                }
                call(&[HostArg::Str("currentScript"), HostArg::Null]);
                if info.src.is_some() {
                    call(&[HostArg::Str("fire"), node, HostArg::Str("load")]);
                }
            };
            // As imagens que o parser criou pedem a fonte já (as eager seguram o `load`; as lazy esperam o quadro).
            call(&[HostArg::Str("parsedImages")]);
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
                // Cada `async` é uma tarefa própria: o que venceu enquanto o anterior rodava (um quadro, um timer)
                // roda antes dele.
                runner.run_event_loop_for(0.0);
                run_script(info);
            }
            // As folhas de `<link rel=stylesheet>` chegam antes do `load` da janela; cada uma dispara `load` ou `error`.
            for (node, ok) in self.stylesheet_results() {
                call(&[HostArg::Str("fire"), HostArg::Num(node.as_u64() as f64), HostArg::Str(if ok { "load" } else { "error" })]);
            }
            call(&[HostArg::Str("subresourcesDone")]);
            Some(after(runner, &hook))
        })
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

/// A resposta do documento.
struct DocumentResponse {
    bytes: Vec<u8>,
    final_url: String,
    content_type: Option<String>,
    /// Veio CSP por cabeçalho (o que liga o nonce hiding).
    header_csp: bool,
    /// As políticas CSP dos cabeçalhos, cada uma com `true` se vem do `Content-Security-Policy-Report-Only`.
    csp: Vec<(String, bool)>,
    /// `crossOriginIsolated`: `Cross-Origin-Opener-Policy: same-origin` com `Cross-Origin-Embedder-Policy`
    /// `require-corp` ou `credentialless`, num contexto seguro (medido no oráculo, em `http://127.0.0.1`).
    cross_origin_isolated: bool,
    /// A troca de rede do documento, ou `None` para o que não veio pela rede (`file:`, `data:`, `about:blank`).
    transfer: Option<(u16, Transfer, Option<String>)>,
    /// Quando a navegação começou e a busca aconteceu.
    timing: Fetched,
}

/// Os instantes da busca do documento no relógio monotônico (`ul_common::time::monotonic_nanos`).
#[derive(Debug, Clone, Copy)]
struct Fetched {
    navigation_start: i128,
    /// A busca: do pedido ao fim do corpo (a troca de rede tem os instantes de dentro, em [`Transfer::timing`]).
    fetch_start: i128,
    fetch_end: i128,
}

impl Fetched {
    /// Um documento que nasce sem busca (a página de erro).
    fn now() -> Fetched {
        let now = ul_common::time::monotonic_nanos();
        Fetched { navigation_start: now, fetch_start: now, fetch_end: now }
    }
}

impl DocumentResponse {
    /// O que a entrada `navigation` do Performance Timeline mostra do carregamento, em JSON para o prelúdio. Sem rede
    /// (medido no oráculo com `file:` e `data:`), o protocolo é vazio, o status 200 e os tamanhos são os do documento.
    /// Os marcos são o tempo real de cada etapa desde o início da navegação, em ms (o prelúdio os passa pelo
    /// `TimeClamper`); sem os instantes de dentro da troca, a conexão e o pedido ficam no começo da busca e a resposta
    /// no fim dela. `domLoading` (só no legado `performance.timing`) é o começo do parse.
    fn transfer_json(&self, parse_start: i128) -> String {
        use crate::devtools::json::Json;
        let (status, transfer, encoding) = match &self.transfer {
            Some((status, transfer, encoding)) => (*status, *transfer, encoding.clone()),
            None => (200, Transfer::identity(self.bytes.len()), None),
        };
        let essence = self.content_type.as_deref().map(|value| value.split(';').next().unwrap_or("").trim().to_ascii_lowercase()).unwrap_or_else(|| "text/html".to_owned());
        let Fetched { navigation_start, fetch_start, fetch_end } = self.timing;
        let at = |tick: i128| Json::Number((tick - navigation_start).max(0) as f64 / 1_000_000.0);
        let exchange = transfer.timing.unwrap_or(ul_net::net::client::Timing { connect_start: fetch_start, secure_start: None, connect_end: fetch_start, request_start: fetch_start, response_start: fetch_end, response_end: fetch_end });
        let secure = match exchange.secure_start {
            Some(tick) => at(tick),
            None => Json::Number(0.0),
        };
        let marks = vec![
            ("fetchStart".to_owned(), at(fetch_start)),
            ("domainLookupStart".to_owned(), at(exchange.connect_start)),
            ("domainLookupEnd".to_owned(), at(exchange.connect_start)),
            ("connectStart".to_owned(), at(exchange.connect_start)),
            ("secureConnectionStart".to_owned(), secure),
            ("connectEnd".to_owned(), at(exchange.connect_end)),
            ("requestStart".to_owned(), at(exchange.request_start)),
            ("responseStart".to_owned(), at(exchange.response_start)),
            ("responseEnd".to_owned(), at(exchange.response_end.max(fetch_end))),
            ("domLoading".to_owned(), at(parse_start)),
        ];
        Json::Object(vec![
            ("marks".to_owned(), Json::Object(marks)),
            ("network".to_owned(), Json::Bool(self.transfer.is_some())),
            ("protocol".to_owned(), Json::str(if self.transfer.is_some() { transfer.protocol() } else { String::new() })),
            ("status".to_owned(), Json::Number(f64::from(status))),
            ("encodedBodySize".to_owned(), Json::Number(transfer.encoded_size as f64)),
            ("decodedBodySize".to_owned(), Json::Number(self.bytes.len() as f64)),
            ("contentType".to_owned(), Json::str(&essence)),
            ("contentEncoding".to_owned(), Json::str(encoding.unwrap_or_default().trim().to_ascii_lowercase())),
            ("redirectCount".to_owned(), Json::Number(f64::from(transfer.redirects))),
        ])
        .to_compact()
    }
}

/// O valor de um cabeçalho de política, sem os parâmetros (`same-origin; report-to="x"`).
fn policy_value(response: &ul_net::net::client::Response, name: &str) -> Option<String> {
    response.header(name).map(|value| value.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
}

/// Contexto seguro de uma URL de documento: `https`, ou `http` num host que é sempre local.
fn potentially_trustworthy(address: &str) -> bool {
    let Ok(parsed) = url::parse(address, false) else { return false };
    if parsed.scheme == "https" {
        return true;
    }
    let host = parsed.host_for_url().to_ascii_lowercase();
    parsed.scheme == "http" && (host == "localhost" || host.ends_with(".localhost") || host.starts_with("127.") || host == "[::1]")
}

/// As portas que o `IsPortAllowedForScheme` do Chromium recusa com `net::ERR_UNSAFE_PORT` (`kRestrictedPorts` de
/// `net/base/port_util.cc`, a lista de portas ruins da especificação de Fetch).
const BAD_PORTS: [u16; 82] = [
    1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143,
    161, 179, 389, 427, 465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993, 995, 1719, 1720, 1723, 2049, 3659, 4045,
    4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668, 6669, 6679, 6697, 10080,
];

/// O modelo da página de erro e o `loadTimeDataRaw` de cada código, com `{{HOST}}` e `{{URL}}` no lugar do destino.
const NETERROR_TEMPLATE: &str = include_str!("../data/neterror/template.html");
const NETERROR_DATA: &str = include_str!("../data/neterror/load-time-data.json");

fn neterror_html(url: &str, error: &str) -> Option<String> {
    use crate::devtools::json::{self, Json};
    let code = error.strip_prefix("net::").unwrap_or(error);
    let table = json::parse(NETERROR_DATA).ok()?;
    let data = table.get(code)?.as_str()?;
    let host = ul_net::net::url::parse(url, false).map(|u| u.host_for_url()).unwrap_or_default();
    // Os valores entram já escapados como texto JSON (sem as aspas), no meio do literal do modelo.
    let inner = |text: &str| {
        let quoted = Json::str(text).to_compact();
        quoted[1..quoted.len() - 1].to_owned()
    };
    let data = data.replace("{{URL}}", &inner(url)).replace("{{HOST}}", &inner(&host));
    let title = json::parse(&data).ok()?.get("title")?.as_str()?.to_owned();
    let escaped = title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    Some(NETERROR_TEMPLATE.replacen("{{TITLE}}", &escaped, 1).replacen("{{DATA}}", &data, 1))
}

/// Os documentos WebUI que o navegador abre sozinho: os dois popups do omnibox de cada janela, com o HTML que o
/// oráculo serve (lido pelo DevTools depois de carregado).
pub const WEBUI_DOCUMENTS: [(&str, &str); 2] = [
    ("chrome://omnibox-popup.top-chrome/omnibox_popup_aim.html", include_str!("../data/omnibox/omnibox_popup_aim.html")),
    ("chrome://omnibox-popup.top-chrome/", include_str!("../data/omnibox/omnibox_popup.html")),
];

/// Cada política CSP dos cabeçalhos da resposta (um cabeçalho pode trazer várias, separadas por vírgula), com a marca
/// de só relatório.
fn csp_policies(response: &ul_net::net::client::Response) -> Vec<(String, bool)> {
    let mut policies = Vec::new();
    for (name, value) in &response.headers {
        let report_only = if name.eq_ignore_ascii_case(b"content-security-policy") {
            false
        } else if name.eq_ignore_ascii_case(b"content-security-policy-report-only") {
            true
        } else {
            continue;
        };
        for policy in String::from_utf8_lossy(value).split(',') {
            let policy = policy.trim();
            if !policy.is_empty() {
                policies.push((policy.to_owned(), report_only));
            }
        }
    }
    policies
}

fn fetch_document(address: &str, fetcher: &Fetcher) -> Result<DocumentResponse, String> {
    let now = ul_common::time::monotonic_nanos;
    let navigation_start = now();
    let local = |bytes: Vec<u8>, content_type: Option<String>| DocumentResponse {
        bytes,
        final_url: address.to_owned(),
        content_type,
        header_csp: false,
        csp: Vec::new(),
        cross_origin_isolated: false,
        transfer: None,
        timing: Fetched { navigation_start, fetch_start: navigation_start, fetch_end: now() },
    };
    // A aba nova (`about:blank`) é um documento vazio sem rede; `data:` sai pelo processador de data: URL do Fetch.
    if address == "about:blank" {
        return Ok(local(Vec::new(), None));
    }
    if address.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:")) {
        let parsed = data_url::DataUrl::process(address).map_err(|_| "net::ERR_INVALID_URL".to_owned())?;
        let (bytes, _) = parsed.decode_to_vec().map_err(|_| "net::ERR_INVALID_URL".to_owned())?;
        return Ok(local(bytes, Some(parsed.mime_type().to_string())));
    }
    if let Some(&(_, html)) = WEBUI_DOCUMENTS.iter().find(|(url, _)| *url == address) {
        return Ok(local(html.as_bytes().to_vec(), Some("text/html".to_owned())));
    }
    if let Some(path) = address.strip_prefix("file://") {
        let bytes = sys::read_file(path.as_bytes()).map_err(|_| "net::ERR_FILE_NOT_FOUND".to_owned())?;
        return Ok(local(bytes, None));
    }
    if url::parse(address, false).is_ok_and(|u| BAD_PORTS.contains(&u.port_or_default())) {
        return Err("net::ERR_UNSAFE_PORT".to_owned());
    }
    let headers = vec![("User-Agent".to_owned(), USER_AGENT.to_owned()), ("Accept".to_owned(), "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8".to_owned()), ("Accept-Language".to_owned(), "en-US,en;q=0.9".to_owned())];
    let spec = RequestSpec { headers: &headers, ..RequestSpec::default() };
    let fetch_start = now();
    let response = fetcher(address, &spec).map_err(|e| e.net_error().to_owned())?;
    let fetch_end = now();
    let final_url = response.url.to_string_with(true);
    let cross_origin_isolated = policy_value(&response, "Cross-Origin-Opener-Policy").as_deref() == Some("same-origin")
        && matches!(policy_value(&response, "Cross-Origin-Embedder-Policy").as_deref(), Some("require-corp" | "credentialless"))
        && potentially_trustworthy(&final_url);
    Ok(DocumentResponse {
        content_type: response.header("Content-Type"),
        header_csp: response.header("Content-Security-Policy").is_some(),
        csp: csp_policies(&response),
        transfer: Some((response.status, response.transfer, response.header("Content-Encoding"))),
        bytes: response.body,
        final_url,
        cross_origin_isolated,
        timing: Fetched { navigation_start, fetch_start, fetch_end },
    })
}

#[cfg(test)]
mod tests {
    /// O título e o `loadTimeDataRaw` da página de erro como o oráculo os mostrou para `http://127.0.0.1:8999/a?b=1`.
    #[test]
    fn neterror_page_as_the_oracle() {
        let html = super::neterror_html("http://127.0.0.1:8999/a?b=1", "net::ERR_CONNECTION_REFUSED").unwrap();
        assert!(html.contains("<title>127.0.0.1</title>"));
        assert!(html.contains(r#""reloadButton":{"msg":"Reload","reloadUrl":"http://127.0.0.1:8999/a?b=1"}"#));
        // O `<` sai como a sequência de escape do JSON do Chromium (barra invertida, `u003C`).
        let lt = concat!("\\", "u003C");
        assert!(html.contains(&format!(r#""summary":{{"msg":"{lt}strong>127.0.0.1{lt}/strong> refused to connect."}}"#)));
        assert!(super::neterror_html("http://x/", "net::ERR_NOT_MEASURED").is_none());
    }
}
