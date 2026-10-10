//! Comportamento de scripts em páginas pequenas servidas da memória: o DOM despejado e o que o console recebeu.
//! Cada caso fixa uma semântica do navegador que uma página real do conjunto da internet expôs.

use std::cell::RefCell;
use std::rc::Rc;

mod common;

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
    common::install_image_fonts();
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
fn unchanged_style_keeps_attribute_text() {
    // Medido no oráculo: a escrita via CSSOM que não muda o bloco deixa o atributo como veio.
    let html = r#"<body><div id=a style="--a:80;color:red"></div><div id=b style="--a:80;color:red"></div><div id=c style="color:red"></div><script>
const [a, b, c] = ['a', 'b', 'c'].map((id) => document.getElementById(id));
a.style.setProperty('--a', '80'); b.style.setProperty('--a', '81'); c.style.color = 'red'; c.style.removeProperty('width');
for (const el of [a, b, c]) console.log(el.getAttribute('style'));
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "--a:80;color:red\n--a: 81; color: red;\ncolor:red\n");
}

#[test]
fn window_named_access() {
    // Medido no oráculo: id e nome de form acham o elemento; homônimos viram HTMLCollection; declaração vence.
    let html = r#"<body><div id=h></div><form name=f></form><p id=two></p><p id=two></p><div id=v></div><script>
var v = 1;
console.log(h.localName, window.f.localName, two.length, String(two), v, 'h' in window, typeof missing, window.missing);
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "div form 2 [object HTMLCollection] 1 true undefined undefined\n");
}

#[test]
fn created_script_is_force_async() {
    // Medido no oráculo: script criado por script é async sem o atributo; escrever `async` limpa a flag.
    let html = r#"<body><script async src="data:,"></script><script>
const a = document.createElement('script'), b = document.createElement('script');
b.async = false;
const c = document.createElement('script'); c.setAttribute('async', '');
console.log(a.async, a.hasAttribute('async'), b.async, b.hasAttribute('async'), c.async, document.scripts[0].async, document.scripts[1].async);
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "true false false false true true false\n");
}

#[test]
fn input_value_modes() {
    // Medido no oráculo: radio e hidden guardam o valor no atributo, text guarda no valor sujo (wikipedia).
    let html = "<body><input type=radio id=r><input type=text id=t><input type=hidden id=h><script>
r.value = 'a'; t.value = 'b'; h.value = 'c';
console.log([r.getAttribute('value'), t.getAttribute('value'), h.getAttribute('value'), r.value, t.value].join(' '));
</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "a  c a b\n");
}

#[test]
fn webgl_state_matches_swiftshader() {
    // `tests/webgl/state.expected` é o `data-out` do oráculo (--disable-gpu, ANGLE sobre SwiftShader) para a mesma
    // página: contexto, parâmetros, extensões, shaders, link, uniforms, buffers, texturas, framebuffer e erros.
    let (_, log) = run(&[("/", include_str!("webgl/state.html"))]);
    assert_eq!(log, include_str!("webgl/state.expected"));
}

#[test]
fn request_resolves_against_document() {
    // Medido no oráculo (em file:///, aqui na origem do teste): a URL relativa resolve contra a página (Rive do vite.dev).
    let html = "<body><script>
const r = new Request('/a?b', { method: 'POST' });
console.log([r.url, r.method, Request.name, Request.length, String(Request), Object.prototype.toString.call(r), Object.getPrototypeOf(Request) === Function.prototype, new Request(r).url].join(' | '));
</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "http://t.test/a?b | POST | Request | 1 | function Request() { [native code] } | [object Request] | true | http://t.test/a?b\n");
}

#[test]
fn images_decode_svg_and_blob() {
    // Medido no oráculo: tamanho intrínseco de SVG (atributos, viewBox, 300x150), erro em lixo, PNG de blob: URL.
    let html = r#"<body><script>
const cases = [
  '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"></svg>',
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 50 25"></svg>',
  '<svg xmlns="http://www.w3.org/2000/svg" width="40" viewBox="0 0 50 25"></svg>',
  '<svg xmlns="http://www.w3.org/2000/svg"></svg>',
  '<svg xmlns="http://www.w3.org/2000/svg" width="50%" height="2em"></svg>',
  'garbage',
];
const out = [];
cases.forEach((c, i) => {
  const img = new Image();
  img.onload = () => { out[i] = 'load ' + img.naturalWidth + 'x' + img.naturalHeight + ' ' + img.width + 'x' + img.height + ' ' + img.complete; };
  img.onerror = () => { out[i] = 'error ' + img.naturalWidth + ' ' + img.complete; };
  img.src = 'data:image/svg+xml,' + encodeURIComponent(c);
});
const b = new Blob([new Uint8Array([137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,137,0,0,0,13,73,68,65,84,120,156,99,248,207,192,240,31,0,5,0,1,255,137,153,61,29,0,0,0,0,73,69,78,68,174,66,96,130])], { type: 'image/png' });
const u = URL.createObjectURL(b);
const bi = new Image(); bi.onload = () => { out[6] = 'blob ' + bi.naturalWidth + 'x' + bi.naturalHeight + ' ' + /^blob:http:\/\/t\.test\/[0-9a-f-]{36}$/.test(u); }; bi.onerror = () => { out[6] = 'blob error'; }; bi.src = u;
const before = new Image(); out[7] = 'before ' + before.complete + ' ' + before.naturalWidth;
setTimeout(() => console.log(out.join('\n')), 200);
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "load 40x20 40x20 true\nload 300x150 300x150 true\nload 40x20 40x20 true\nload 300x150 300x150 true\nload 300x32 300x32 true\nerror 0 true\nblob 1x1 true\nbefore true 0\n");
}

#[test]
fn font_families_resolve_like_debian_chromium() {
    // Medido no oráculo (`testbench/oracle/measured/font-families.html`): sans-serif e Arial são a Liberation Sans,
    // família desconhecida cai na padrão (Liberation Serif), monospace é a DejaVu Sans Mono.
    let html = "<body><script>
const width = (family) => { const s = document.createElement('span'); s.style.font = '16px ' + family; s.style.display = 'inline-block'; s.textContent = 'Hamburgefonstiv WQ 0123'; document.body.append(s); const w = s.getBoundingClientRect().width; s.remove(); return w; };
const named = (f) => width('\"' + f + '\"');
const ref = { sans: named('Liberation Sans'), serif: named('Liberation Serif'), mono: named('DejaVu Sans Mono'), dvs: named('DejaVu Sans') };
const pick = (w) => Object.keys(ref).find((k) => ref[k] === w) || w;
console.log(ref.sans, ['sans-serif', 'Arial', 'Helvetica', 'serif', 'Foo', '\"Times New Roman\"', 'monospace', 'system-ui'].map((f) => f + '=' + pick(width(f))).join(' '));
</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "192.984375 sans-serif=sans Arial=sans Helvetica=sans serif=serif Foo=serif \"Times New Roman\"=serif monospace=mono system-ui=dvs\n");
}

#[test]
fn blink_only_properties_survive_cssom() {
    // Medido no oráculo: `transform-box` (que o stylo do Servo não tem) fica no bloco, na ordem de escrita, e o
    // getComputedStyle dá o inicial `view-box` sem declaração (o motion do nextjs.org escreve `fill-box`).
    let html = "<body><div id=d></div><p id=p></p><script>
const s = d.style;
s.transformOrigin = '50% 50% 0px'; const a = s.cssText;
s.transformBox = 'fill-box'; const b = s.cssText;
s.transformOrigin = '10px 0px 0px'; const c = s.cssText;
s.transformBox = 'bogus'; const e = s.cssText;
console.log([a, b, c, e, getComputedStyle(d).transformBox, getComputedStyle(p).transformBox, getComputedStyle(p).writingMode].join(' | '));
</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "transform-origin: 50% 50% 0px; | transform-origin: 50% 50% 0px; transform-box: fill-box; | transform-origin: 10px 0px 0px; transform-box: fill-box; | transform-origin: 10px 0px 0px; transform-box: fill-box; | fill-box | view-box | horizontal-tb\n");
}

#[test]
fn switch_with_many_numeric_cases() {
    // O getBumpedLaneForHydrationByLane do React 19.3 (nextjs.org): com o switch errado a hidratação nunca agenda.
    let script = "<script>function e$(e){switch(e){case 2:e=1;break;case 8:e=4;break;case 32:e=16;break;case 256:case 512:case 1024:case 2048:case 4096:case 8192:case 16384:case 32768:case 65536:case 131072:case 262144:case 524288:case 1048576:case 2097152:case 4194304:case 8388608:case 0x1000000:case 0x2000000:e=128;break;case 0x10000000:e=0x8000000;break;default:e=0}return e}
console.log([2, 8, 32, 256, 0x2000000, 0x10000000, 3].map(e$).join(' '))</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "1 4 16 128 128 134217728 0\n");
}

#[test]
fn media_range_syntax_applies() {
    // O Tailwind 4 escreve `@media (width>=48rem)`; a janela do headless tem 780px.
    let html = "<style>body{margin:0}@media (width>=48rem){.g{display:grid;grid-template-columns:repeat(2,minmax(0,1fr))}}</style><div class=g><div id=a>a</div><div>b</div></div><script>console.log(document.getElementById('a').offsetWidth, matchMedia('(width>=48rem)').matches)</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "390 true\n");
}

#[test]
fn computed_style_and_media_queries() {
    // Saída do chromium do oráculo para a mesma página.
    let html = r#"<body style="margin:0"><style>.b{box-sizing:border-box}</style><div id=a style="width:100px;padding:10px;border:1px solid red;color:rgb(1,2,3);--x: 5px;margin:4px"></div><div id=b class=b style="width:100px;padding:10px"></div><span id=c>s</span><div id=d style="display:none;width:50%"></div><script>
const g = (id) => getComputedStyle(document.getElementById(id));
const a = g('a'), b = g('b'), c = g('c'), d = g('d');
console.log([a.width, a.height, a.color, a.getPropertyValue('--x'), a.margin, a.marginTop, a.display, b.width, c.display, c.width, d.width, d.display, a.position, a.opacity, matchMedia('(width>=48rem)').matches, matchMedia('(max-width: 700px)').matches, matchMedia('(prefers-color-scheme: dark)').matches].join('|'));
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "100px|0px|rgb(1, 2, 3)|5px|4px|4px|block|100px|inline|auto|50%|none|static|1|true|false|false\n");
}

#[test]
fn thin_scrollbar_width() {
    // Medido no oráculo: barra fina de 10px (com ou sem scrollbar-color, em border-box ou content-box), a normal de 15.
    let html = "<style>html{box-sizing:border-box}*{box-sizing:inherit;scrollbar-width:thin;scrollbar-color:#888 transparent;margin:0;padding:0}</style><body><script>
const out = [];
for (const style of [{}, { boxSizing: 'content-box' }, { scrollbarColor: 'auto' }, { scrollbarWidth: 'auto' }]) {
  const d = document.createElement('div'); Object.assign(d.style, { width: '100px', height: '100px', overflow: 'scroll', position: 'absolute', top: '-9999px' }, style); document.body.append(d);
  out.push(d.offsetWidth + '/' + d.clientWidth + '/' + d.clientHeight); d.remove();
}
console.log(out.join(' '));
</script>";
    let (_, log) = run(&[("/", html)]);
    assert_eq!(log, "100/90/90 100/90/90 100/90/90 100/85/85\n");
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
fn two_value_transform_origin_keeps_no_z() {
    // Medido no oráculo: o Blink não acrescenta o `0px` do eixo z que o stylo completa.
    let script = "<body><p id=p></p><script>var p = document.getElementById('p'); p.style.transformOrigin = '50% 50%'; console.log(p.getAttribute('style'))</script>";
    let (_, log) = run(&[("/", script)]);
    assert_eq!(log, "transform-origin: 50% 50%;\n");
}

#[test]
fn parsed_images_load_and_pick_their_source() {
    // Medido no oráculo (testbench/oracle, headless sob --virtual-time-budget, imagens que respondem 404): toda imagem
    // do documento busca a fonte, lazy inclusive (a qualquer distância e com display: none, no primeiro quadro), e a
    // lazy desconectada busca como a eager; a fonte vem do srcset pela densidade 1 e do primeiro <source> do <picture>
    // que serve; o error não borbulha, e tudo termina antes do load da janela.
    let html = r#"<body><img id=a src="a.png"><img id=b loading=lazy src="b.png">
<img id=c srcset="c1.png 1x, c2.png 2x" src="c0.png">
<img id=d srcset="d300.png 300w, d900.png 900w, d2000.png 2000w" sizes="(max-width: 500px) 100px, 50vw">
<img id=e srcset="e2.png 2x" src="e1.png">
<picture><source media="(min-width: 5000px)" srcset="f-big.png"><source type="image/x-unknown" srcset="f-unk.png"><source srcset="f-ok.png"><img id=f src="f-img.png"></picture>
<img id=g><img id=h src=""><div style="height: 30000px"></div><img id=i loading=lazy src="i.png" style="display:none">
<script>
var ids = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i'], seen = [];
for (var id of ids) document.getElementById(id).addEventListener('error', function (ev) { seen.push(ev.target.id + (ev.bubbles ? '!' : '')); });
var dyn = document.createElement('img'); dyn.loading = 'lazy'; dyn.onerror = function () { seen.push('dyn'); }; dyn.src = 'dyn.png';
document.getElementById('h').decode().then(function () { console.log('h decodificou'); }, function (e) { console.log('h', e.name, e.message); });
window.addEventListener('load', function () {
  console.log(seen.slice().sort().join(','));
  console.log(ids.map(function (id) { var el = document.getElementById(id); return id + '=' + el.complete + ':' + el.currentSrc.split('/').pop(); }).join(' '));
});
</script>"#;
    let (_, log) = run(&[("/", html)]);
    assert_eq!(
        log,
        concat!(
            "h EncodingError The source image cannot be decoded.\n",
            "a,b,c,d,dyn,e,f,h,i\n",
            "a=true:a.png b=true:b.png c=true:c1.png d=true:d900.png e=true:e1.png f=true:f-ok.png g=true: h=true: i=true:i.png\n",
        )
    );
}

#[test]
fn transform_functions_keep_written_arity() {
    // Medido no oráculo: o Blink guarda o segundo argumento de translate, scale e skew mesmo quando ele é o padrão.
    let cases = ["translate(0px, 0px)", "translate(10px,0)", "translate(5px)", "translate(0px, 0%)", "scale(1,1)", "scale(2)", "scale(2, 2)", "skew(10deg, 0)", "skew(10deg)", "translate(0, 0) scale(1, 1)", "translate(-50%, -50%) rotate(45deg)"];
    let list = cases.iter().map(|c| format!("'{c}'")).collect::<Vec<_>>().join(",");
    let script = format!("<body><script>for (var c of [{list}]) {{ var p = document.createElement('p'); p.style.transform = c; var q = document.createElement('p'); q.setAttribute('style', 'transform: ' + c); console.log(p.getAttribute('style') + ' | ' + q.style.transform) }}</script>");
    let (_, log) = run(&[("/", &script)]);
    assert_eq!(
        log,
        concat!(
            "transform: translate(0px, 0px); | translate(0px, 0px)\n",
            "transform: translate(10px, 0px); | translate(10px, 0px)\n",
            "transform: translate(5px); | translate(5px)\n",
            "transform: translate(0px, 0%); | translate(0px, 0%)\n",
            "transform: scale(1, 1); | scale(1, 1)\n",
            "transform: scale(2); | scale(2)\n",
            "transform: scale(2, 2); | scale(2, 2)\n",
            "transform: skew(10deg, 0deg); | skew(10deg, 0deg)\n",
            "transform: skew(10deg); | skew(10deg)\n",
            "transform: translate(0px, 0px) scale(1, 1); | translate(0px, 0px) scale(1, 1)\n",
            "transform: translate(-50%, -50%) rotate(45deg); | translate(-50%, -50%) rotate(45deg)\n",
        )
    );
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

#[test]
fn clock_has_the_blink_grain_and_frames_the_vsync_grid() {
    // Medido no oráculo (testbench/oracle/measured/clock.html): sem crossOriginIsolated, performance.now, timeOrigin e
    // Event.timeStamp caem em múltiplos de 100 µs, com o ruído da subtração de dois instantes grandes; o carimbo do
    // rAF sai redondo e os quadros andam 16,6 ou 16,7 ms (a grade de 16666 µs clampada).
    let script = r#"<script>
var grain = function (v) { return Math.abs(v * 10 - Math.round(v * 10)) < 1e-6; };
var nows = []; for (var i = 0; i < 50; i++) { for (var j = 0, s = 0; j < 3000 * i; j++) s += j; nows.push(performance.now()); }
console.log('now', nows.every(grain), nows.some(function (v) { return !/^\d+(\.\d)?$/.test(String(v)); }));
console.log('origin', grain(performance.timeOrigin), 'event', grain(new Event('x').timeStamp), crossOriginIsolated);
var frames = [];
var step = function (t) { frames.push(t); if (frames.length < 3) requestAnimationFrame(step); };
requestAnimationFrame(step);
setTimeout(function () {
  var onGrid = function (t) { return Math.abs(t / 16.666 - Math.round(t / 16.666)) * 16.666 <= 0.1 + 1e-9; };
  console.log('frames', frames.length >= 1, frames.every(function (t) { return /^\d+(\.\d)?$/.test(String(t)) && onGrid(t); }));
  console.log('deltas', frames.slice(1).map(function (t, i) { return (t - frames[i]).toFixed(1); }).join(','));
}, 200);
</script>"#;
    let (_, log) = run(&[("/", script)]);
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines[0], "now true true", "{log}");
    assert_eq!(lines[1], "origin true event true false", "{log}");
    assert_eq!(lines[2], "frames true true", "{log}");
    let deltas = lines[3].strip_prefix("deltas").unwrap().trim();
    assert!(deltas.is_empty() || deltas.split(',').all(|d| d == "16.6" || d == "16.7"), "{log}");
}
