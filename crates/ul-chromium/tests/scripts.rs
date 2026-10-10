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
    // Binding léxico de topo não é propriedade do global: só o `typeof` o enxerga.
    let (_, log) = run(&[("/", "<script>console.log(typeof $plHook, typeof hook, typeof op)</script>")]);
    assert_eq!(log, "undefined undefined undefined\n");
}

/// O estado interno do navegador não aparece como propriedade própria de nenhum objeto da página.
#[test]
fn no_internal_own_properties() {
    let script = "<div id=h></div><script>var el = document.getElementById('h'); el.attachShadow({mode:'open'}); el.classList.add('x'); \
        var x = new XMLHttpRequest(); var e = new Event('a'); document.doctype; \
        console.log([el, el.shadowRoot, document, x, e, document.doctype, history, location, window].map(o => Reflect.ownKeys(o).filter(k => String(k).startsWith('__') || String(k).startsWith('Symbol(__')).length).join(), \
        Object.keys(x).length, 'withCredentials' in XMLHttpRequest.prototype, el.shadowRoot.mode, document.doctype.name)</script>";
    let (_, log) = run(&[("/", &format!("<!DOCTYPE html>{script}"))]);
    assert_eq!(log, "0,0,0,0,0,0,0,0,0 0 true open html\n");
}

/// O innerHTML da raiz de sombra cria elementos HTML (o contexto é o host) e o slot atribui por nome.
#[test]
fn shadow_root_slots() {
    let script = "<div id=h><span slot=a>x</span><em>y</em></div><script>var r = document.getElementById('h').attachShadow({mode:'open'}); r.innerHTML = '<slot name=a></slot><slot></slot>'; var s = r.querySelector('slot'); console.log(s.constructor.name, s.namespaceURI,s.assignedNodes().map(n => n.textContent).join(), r.querySelector('slot:not([name])').assignedElements().map(n => n.localName).join())</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "HTMLSlotElement http://www.w3.org/1999/xhtml x em\n");
}

/// `overflow: scroll` reserva a barra clássica de 15px (o teste de barra visível do svelte.dev).
#[test]
fn scroll_container_reserves_scrollbar() {
    let script = "<body><script>var d = document.createElement('div'); Object.assign(d.style, { width: '100px', height: '100px', overflow: 'scroll', position: 'absolute', top: '-9999px' }); document.body.append(d); console.log(d.offsetWidth, d.clientWidth, d.clientHeight)</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "100 85 85\n");
}

#[test]
fn fractional_radix_strings_match_v8() {
    // Saída do chromium do oráculo; o id do CodeGroup do VitePress é `Math.random().toString(36).substring(7)`.
    let script = "<script>for (const x of [0.123456789, Math.PI / 10, 0.9876543210123, 1 / 3, 0.000123456, 123.456, 2 ** -30 + 0.1]) console.log(x.toString(36), x.toString(7), x.toString(16), x.toString(2))</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(
        log,
        "0.4fzzzxjylrx 0.0602263655133143611 0.1f9add3739635f 0.00011111100110101101110100110111001110010110001101011111\n\
         0.bb5exfivbcc 0.212520344346455155 0.506cbdda73c38 0.0101000001101100101111011101101001110011110000111\n\
         0.zk00001xj0a 0.662523354215303612 0.fcd6e9e0803be8 0.11111100110101101110100111100000100000000011101111101\n\
         0.c 0.2222222222222222222 0.55555555555554 0.010101010101010101010101010101010101010101010101010101\n\
         0.005rcwu8qgexl 0.00002034461564452542663 0.0008173f7b84ca6e8 0.00000000000010000001011100111111011110111000010011001010011011101\n\
         3f.gez4w97ry 234.312256641535441 7b.74bc6a7ef9dc 1111011.0111010010111100011010100111111011111001110111\n\
         0.3llllnmkyoi 0.04620462051066354516 0.1999999d99999a 0.0001100110011001100110011001110110011001100110011001101\n"
    );
}

#[test]
fn frames_stop_after_load() {
    // Ordem medida no chromium do oráculo: o quadro que segue o `load` roda o rAF e entrega os observadores, o idle
    // callback vem depois dele, e depois disso não há quadro (nem rAF nem entrega nova).
    let html = "<body><div id=a style='height:50px'>a</div><div id=h style='display:none'>h</div><script>
const log = (s) => console.log(s);
const watch = (label, id = 'a') => { new IntersectionObserver((es) => log(label + ' io ' + es[0].isIntersecting)).observe(document.getElementById(id)); };
requestAnimationFrame(() => log('raf sync'));
watch('hidden', 'h');
document.addEventListener('DOMContentLoaded', () => { log('dcl'); requestIdleCallback(() => { log('ric dcl'); watch('ric dcl'); }); });
window.addEventListener('load', () => { log('load'); watch('load'); requestIdleCallback(() => { log('ric load'); watch('ric load'); }); requestAnimationFrame(() => log('raf load')); });
setTimeout(() => { log('t10'); }, 10);
setTimeout(() => { log('t30'); watch('t30'); requestAnimationFrame(() => log('raf t30')); }, 30);
</script><script type=module>log('module'); watch('module'); requestIdleCallback(() => { log('ric module'); watch('ric module'); });</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "module\ndcl\nload\nt10\nraf sync\nraf load\nhidden io false\nmodule io true\nload io true\nric module\nric dcl\nric load\nt30\n");
}

#[test]
fn push_apply_in_timer() {
    let script = "<script>setTimeout(function () { for (var n of [10, 1000, 70000, 200000]) { var a = []; var b = new Array(n).fill(1); a.push.apply(a, b); console.log(n, a.length) } }, 0)</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "10 10\n1000 1000\n70000 70000\n200000 200000\n");
}

#[test]
fn xpath_attribute_prefix_query() {
    let html = r#"<body><div id=a hx-on:click="x"></div><p id=b data-hx-on:load="y"></p><span id=c class=z></span><script>
        var r = document.evaluate('.//*[@*[ starts-with(name(), "hx-on:")] or @*[ starts-with(name(), "data-hx-on:")]]', document, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
        var ids = []; for (var i = 0; i < r.snapshotLength; i++) ids.push(r.snapshotItem(i).id);
        console.log(ids.join(','), document.evaluate('count(//span[@class="z"])', document, null, 0, null).numberValue, document.evaluate('string(//p/@id)', document, null, 2, null).stringValue);
    </script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "a,b 1 b\n");
}

#[test]
fn cssom_write_serializes_canonically() {
    let script = "<body><p id=p></p><script>var p = document.getElementById('p'); p.style.cssText = 'border: 0; clip: rect(0 0 0 0); word-wrap: normal; margin: -1px'; console.log(p.getAttribute('style'))</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "border: 0px; clip: rect(0px, 0px, 0px, 0px); overflow-wrap: normal; margin: -1px;\n");
}

#[test]
fn module_scripts_with_dynamic_import_and_top_level_await() {
    let html = "<body><script type=module>import { a } from './m.js'; const { b } = await import('/n.js'); console.log(a, b, document.currentScript, import.meta.url)</script><script type=module>console.log(typeof C)</script><script>var C = 1</script>";
    let (_, log) = run(&[("/", html), ("/m.js", "export const a = 'a'"), ("/n.js", "export const b = 'b'")]);
    // O clássico roda primeiro (bloqueia o parser); os módulos são adiados, e o segundo roda enquanto o primeiro espera.
    assert_eq!(log, "number\na b null http://t.test/\n");
}

#[test]
fn hidden_is_on_html_element_prototype() {
    let (_, log) = run(&[("/", "<script>console.log(Object.getOwnPropertyNames(HTMLElement.prototype).includes('hidden'), 'hidden' in Element.prototype)</script>")]);
    assert_eq!(log, "true false\n");
}
