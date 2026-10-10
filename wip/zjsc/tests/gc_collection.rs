//! O coletor: o lixo de um laço é varrido pelo gatilho automático de alocação (`Heap::collectIfNecessaryOrDefer`)
//! e o que segue alcançável sobrevive às coletas com o valor intacto.
use zjsc::api::eval::evaluate_script;
use zjsc::runtime::gc;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

/// `for (let i=0;i<N;i++){const a=[i]}`: antes do coletor cada array vivia até o fim da thread.
#[test]
fn garbage_of_a_loop_stays_bounded() {
    assert_eq!(run("for (let i = 0; i < 1e6; i++) { const a = [i]; } 'ok'"), "ok");
    let statistics = gc::statistics();
    assert!(statistics.collections >= 5, "{statistics:?}");
    assert!(statistics.freed_cells >= 900_000, "{statistics:?}");
    assert!(statistics.peak_live_cells < 300_000, "{statistics:?}");
}

/// O que segue alcançável (global, closure, protótipo, Map, propriedade, elemento) passa pelas coletas.
#[test]
fn reachable_values_survive_collections() {
    let program = "
        var keep = { list: [] };
        function make(n) { var o = { n: n, inner: [n, 'x' + n] }; return function () { return o.inner[1]; }; }
        var closures = [];
        var map = new Map();
        class Point { constructor(x) { this.x = x; } sum() { return this.x + 1; } }
        for (let i = 0; i < 400000; i++) {
            const garbage = { i: i, s: 'g' + i };
            if (i % 1000 === 0) {
                keep.list.push(new Point(i));
                closures.push(make(i));
                map.set('k' + i, [i]);
            }
        }
        var total = 0;
        for (const p of keep.list) total += p.sum();
        [total, closures[7](), map.get('k5000')[0], keep.list.length].join(',');
    ";
    assert_eq!(run(program), "79800400,x7000,5000,400");
    assert!(gc::statistics().collections >= 1);
}

/// Medida, não asserção: o pico de memória residente (`VmHWM`) do laço de 1e7. Com `ZJSC_GC_STRESS` enorme o
/// gatilho nunca dispara, que é o comportamento de antes do coletor.
#[test]
#[ignore]
fn measure_loop_peak_memory() {
    assert_eq!(run("for (let i = 0; i < 1e7; i++) { const a = [i]; } 'ok'"), "ok");
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak = status.lines().find(|line| line.starts_with("VmHWM")).unwrap_or("VmHWM: ?");
    println!("{peak} {:?}", gc::statistics());
}

/// Fracos, medido no bun 1.4.2 com coleta automática: o `WeakRef` segura o alvo até o fim do trabalho síncrono e
/// das microtarefas e o solta na tarefa seguinte; o `FinalizationRegistry` chama o callback numa tarefa posterior
/// para cada alvo morto; o valor de `WeakMap` vive enquanto a chave vive (efêmero) e morre com ela; o item de
/// `WeakSet` não segura nada.
#[test]
fn weak_references_follow_the_collector() {
    let program = "
        var log = [];
        var r = new WeakRef({});
        var fr = new FinalizationRegistry(h => log.push('fin ' + h));
        (function () { fr.register({}, 'a'); fr.register({}, 'b'); })();
        var key = {};
        var wm = new WeakMap();
        wm.set(key, { v: 7 });
        (function () { var k2 = {}, v2 = {}; fr.register(v2, 'wmvalue'); wm.set(k2, v2); })();
        var ws = new WeakSet();
        (function () { var o = {}; ws.add(o); fr.register(o, 'wsitem'); })();
        (function () { var t = {}; fr.register({}, 'cancelled', t); fr.unregister(t); })();
        for (let i = 0; i < 300000; i++) { const a = [i]; }
        log.push('sync ' + typeof r.deref());
        queueMicrotask(() => log.push('micro ' + typeof r.deref()));
        setTimeout(() => {
            for (let i = 0; i < 300000; i++) { const a = [i]; }
            log.push('t1 ' + typeof r.deref() + ' ' + wm.get(key).v);
            setTimeout(() => { R = log.sort().join(','); }, 5);
        }, 0);
    ";
    let result = zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught(program, "/app/main.js", "R")
        .expect("sem exceção não capturada")
        .unwrap_or_else(|_| panic!("o programa lançou exceção"));
    let text = String::from_utf8_lossy(&result.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "fin a,fin b,fin wmvalue,fin wsitem,micro object,sync object,t1 undefined 7");
}

/// Coleta a cada alocação nesta thread enquanto roda `program` (com timers) e devolve `globalThis.R`. Cada caso
/// abaixo é uma aresta ou raiz que o `ZJSC_GC_STRESS` acusou: sem ela, a lápide do build de depuração dá pânico.
fn stressed(program: &str) -> String {
    let before = gc::statistics().collections;
    gc::set_stress_for_thread(Some(1));
    let result = zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught(program, "/app/main.js", "R");
    gc::set_stress_for_thread(None);
    assert!(gc::statistics().collections > before, "o programa rodou sem nenhuma coleta: o caso não testa raiz nenhuma");
    let value = result.expect("sem exceção não capturada").unwrap_or_else(|_| panic!("o programa lançou exceção"));
    String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
}

const CHURN: &str = "function churn() { for (let i = 0; i < 50; i++) { const g = [i]; } }";

#[test]
fn pending_promise_reactions_survive() {
    let program = format!("{CHURN} var R = ''; var resolve; var p = new Promise(r => resolve = r);
        p.then(v => {{ R += 'a' + v; }}); (function () {{ p.then(function (v) {{ R += 'b' + v; }}); }})();
        churn(); resolve(1); churn();");
    assert_eq!(stressed(&program), "a1b1");
}

#[test]
fn timer_callbacks_and_arguments_survive() {
    let program = format!("{CHURN} var R = ''; (function () {{ setTimeout((x, y) => {{ churn(); R += x.v + y[0]; }}, 1, {{ v: 'x' }}, ['y']); }})(); churn();");
    assert_eq!(stressed(&program), "xy");
}

#[test]
fn transform_stream_construction_locals_survive() {
    let program = format!("{CHURN} var R = '';
        (async () => {{
            var ts = new TransformStream({{ start() {{ return new Promise(r => setTimeout(r, 1)); }} }});
            var w = ts.writable.getWriter(); churn(); await w.ready; R += w.desiredSize;
            var ts2 = new TransformStream({{ transform(c, k) {{ churn(); k.enqueue(c.toUpperCase()); k.enqueue(c + '!'); }}, flush(k) {{ k.enqueue('end'); }} }});
            var w2 = ts2.writable.getWriter(); var r2 = ts2.readable.getReader();
            w2.write('a'); w2.write('b'); w2.close();
            for (;;) {{ var x = await r2.read(); if (x.done) break; churn(); R += x.value; }}
        }})();");
    assert_eq!(stressed(&program), "1Aa!Bb!end");
}

#[test]
fn stream_state_survives_collections() {
    let program = format!("{CHURN} var R = '';
        (async () => {{
            var rs = new ReadableStream({{ pull(c) {{ churn(); c.enqueue({{ v: 'x' }}); c.close(); }} }});
            var [a, b] = rs.tee();
            var ts = new TransformStream({{ transform(c, k) {{ churn(); k.enqueue(c.v + '1'); }} }});
            var w = ts.writable.getWriter(); var tr = ts.readable.getReader(); var ra = a.getReader();
            var chunk = await ra.read(); churn(); w.write(chunk.value); w.close();
            var out = await tr.read(); churn();
            var rb = await b.getReader().read();
            R = out.value + '|' + rb.value.v;
        }})();");
    assert_eq!(stressed(&program), "x1|x");
}

/// Roda `program` como `/app/main.js` coletando a cada alocação e devolve o que ele imprimiu. O programa principal
/// passa pelo ponto seguro do laço no topo (`stressed` avalia um programa sem esse caminho e não chega a coletar).
fn stressed_main(program: &str) -> String {
    let console = std::rc::Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
    gc::set_stress_for_thread(Some(1));
    let before = gc::statistics().collections;
    let (code, _, _) = zjsc::api::eval::evaluate_main_script_reporting_signal(program, "/app/main.js", console.clone());
    let collections = gc::statistics().collections - before;
    gc::set_stress_for_thread(None);
    let stderr = String::from_utf8_lossy(&console.stderr_bytes()).into_owned();
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(collections >= 1, "o programa não coletou");
    String::from_utf8_lossy(&console.stdout_bytes()).into_owned()
}

/// A fila de `process.nextTick` só segura o callback (uma closure sobre objeto que nada mais alcança) e os argumentos.
/// Ela não era raiz do coletor. Ignorado enquanto o corpo do `/app/main.js` não passa por ponto seguro: o
/// `safe_point` só coleta com `native_depth() == 1`, e o programa principal (CJS ou ESM) roda um nível abaixo, então
/// nenhuma coleta cai entre o agendamento e o tick e o teste não distingue a raiz da falta dela.
#[test]
fn next_tick_callbacks_and_arguments_survive() {
    let program = format!("{CHURN} (function () {{ var o = {{ v: 'c' }}; process.nextTick((x, y) => {{ console.log(o.v + x.v + y[0]); }}, {{ v: 'x' }}, ['y']); }})(); churn();");
    assert_eq!(stressed_main(&program), "cxy\n");
}

/// O contexto do `AsyncLocalStorage` capturado por promessa, `await`, `queueMicrotask`, `nextTick` e timer só é
/// alcançado pelo agendamento: a coleta no meio não pode levá-lo.
#[test]
fn async_local_storage_context_survives() {
    let program = format!("{CHURN} var R = ''; var als = new (require('async_hooks').AsyncLocalStorage)();
        als.run({{ k: [ 's' ] }}, () => {{
            Promise.resolve().then(() => {{ R += 'p' + als.getStore().k[0]; }});
            queueMicrotask(() => {{ R += 'q' + als.getStore().k[0]; }});
            process.nextTick(() => {{ R += 'n' + als.getStore().k[0]; }});
            setTimeout(() => {{ churn(); R += 't' + als.getStore().k[0]; }}, 1);
            (async () => {{ await null; R += 'a' + als.getStore().k[0]; }})();
        }}); churn(); setTimeout(() => console.log(R), 20);");
    assert_eq!(stressed_main(&program), "nspsqsasts\n");
}

#[test]
fn async_functions_and_microtask_arguments_survive() {
    let program = format!("{CHURN} var R = ''; async function f(o) {{ churn(); await null; churn(); await {{ then(r) {{ churn(); r(o.k); }} }}; R += o.k; }}
        (function () {{ f({{ k: 'z' }}); }})(); churn();");
    assert_eq!(stressed(&program), "z");
}

#[test]
fn bound_functions_and_arguments_objects_survive() {
    let program = format!("{CHURN} var R = '';
        var b = (function () {{ return function (a, c) {{ return this.t + a + c; }}.bind({{ t: 'T' }}, 'A'); }})();
        function args() {{ return arguments; }} var a = args({{ s: 'S' }}, ['Q']);
        function sloppy(x) {{ return arguments; }} var s = sloppy({{ s: 'P' }});
        churn(); R = b('C') + a[0].s + a[1][0] + s[0].s;");
    assert_eq!(stressed(&program), "TACSQP");
}

#[test]
fn wrappers_regexps_and_async_iterators_survive() {
    let program = format!("{CHURN} var R = ''; var w = Object(Symbol('w')); var re = /a/g; re.lastIndex = {{ valueOf() {{ return 0; }} }};
        async function go() {{ for await (const v of [Promise.resolve('i'), 'j']) {{ churn(); R += v; }} }}
        churn(); go().then(() => {{ R += typeof w.valueOf() + re.test('a'); }});");
    assert_eq!(stressed(&program), "ijsymboltrue");
}

#[test]
fn abort_signal_timeout_survives() {
    let program = format!("{CHURN} var R = ''; (function () {{ var s = AbortSignal.timeout(1); s.onabort = () => {{ R += s.reason.name; }}; }})(); setTimeout(churn, 10); churn();");
    assert_eq!(stressed(&program), "TimeoutError");
}

/// O estado do acessor `stack` do modo V8 é efêmero do objeto: o callee e o `this` dos frames guardados por
/// `Error.captureStackTrace` vivem enquanto o objeto vive.
#[test]
fn v8_stack_state_survives_with_its_object() {
    let program = format!("{CHURN} var o = {{}}; (function outer() {{ var self = {{ tag: 1 }}; (function inner() {{ Error.captureStackTrace(o); }}).call(self); }})();
        churn(); var e = (function thrower() {{ return new Error('m'); }})(); churn(); __out = o.stack.split('\\n').length + ':' + e.stack.split('\\n')[1].trim().split(' ')[1];");
    gc::set_stress_for_thread(Some(1));
    let result = zjsc::api::eval::evaluate_browser_script_result(&program, "file:///p/probe.js", "__out");
    gc::set_stress_for_thread(None);
    let value = result.unwrap_or_else(|_| panic!("o programa lançou exceção"));
    let text = String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    assert_eq!(text, "4:thrower");
}

/// `ErrorInstance::finalizeUnconditionally`: o frame de um callee que morre antes da primeira leitura de `stack`
/// leva o erro ao caminho de texto (cabeçalho `Error`, sem `originalLine`), e o que vive segue o caminho
/// normal (o cabeçalho do tipo).
#[test]
fn errors_with_dead_frames_take_the_text_path() {
    let program = format!("{CHURN} globalThis.maker = function () {{ return new TypeError('m'); }}; var dead = maker(); delete globalThis.maker;
        function alive() {{ return new TypeError('m'); }} var live = alive();
        for (let i = 0; i < 2000; i++) {{ const g = [i]; }}
        R = [dead.stack.split('\\n')[0], Object.getOwnPropertyNames(dead).join('+'), live.stack.split('\\n')[0], Object.getOwnPropertyNames(live).includes('originalLine')].join('|');");
    assert_eq!(stressed(&program), "Error|message+line+column+sourceURL+stack|TypeError: m|true");
}

/// Locais Rust de nativos que chamam JS (o `MarkedArgumentBuffer` e a pilha C++ do JSC): a coleção nova do
/// construtor de `Map` e `Set` e o iterador enquanto o gerador roda, os itens que o `toArray` já leu.
#[test]
fn native_locals_survive_calls_into_javascript() {
    let program = format!("{CHURN} var g = function* () {{ for (var i = 0; i < 4; i++) {{ churn(); yield [i, {{ v: i }}]; }} }};
        var m = new Map(g()); var s = new Set(g());
        var a = (function* () {{ for (var i = 0; i < 3; i++) {{ churn(); yield 'x' + i; }} }})().flatMap(x => new String(x)).toArray();
        R = m.size + ':' + m.get(3).v + ':' + s.size + ':' + a.join('');");
    assert_eq!(stressed(&program), "4:3:4:x0x1x2");
}

/// Os combinadores de `Promise` seguram a promessa de resultado, a capacidade e as funções do caminho rápido em
/// locais Rust enquanto o iterável roda JS.
#[test]
fn promise_combinators_survive_their_iterable() {
    let program = format!("{CHURN} var log = []; var g = function* () {{ for (let i = 0; i < 3; i++) {{ churn(); yield {{ then(f) {{ f(i); }} }}; }} }};
        Promise.race(g()).then(v => log.push('race ' + v)); Promise.all(g()).then(v => log.push('all ' + v.join('')));
        class P extends Promise {{}} P.allSettled(g()).then(v => log.push('settled ' + v.length));
        setTimeout(() => {{ R = log.join(','); }}, 1);");
    assert_eq!(stressed(&program), "race 0,all 012,settled 3");
}

/// O estado de `module` e `require` do CJS mora em Rust (`CjsState`) e é raiz.
#[test]
fn cjs_module_state_survives() {
    let program = format!("{CHURN} module.id = 'a' + 'b'; module.paths = ['p' + 1]; churn(); console.log(module.id + module.paths[0]);");
    assert_eq!(stressed_main(&program), "abp1\n");
}

/// A recursão de `yield*` não pode gastar mais pilha com o coletor ligado: a profundidade máxima com coleta a cada
/// alocação é a mesma que sem coleta.
#[test]
fn deep_yield_star_recursion_fits_under_collection() {
    let program = "var depth = 0; function* r() { depth++; yield* r(); } try { [...r()]; } catch (e) {} R = String(depth);";
    let plain = zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught(program, "/app/main.js", "R")
        .expect("sem exceção não capturada")
        .unwrap_or_else(|_| panic!("o programa lançou exceção"));
    let plain = String::from_utf8_lossy(&plain.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned();
    assert_eq!(stressed(program), plain);
}

/// Os três casos do golden de erros: a recursão de geradores estoura a pilha, o desenrolar mata os corpos (uma
/// função por gerador, como o `@generatorNext` do JSC) e uma coleta antes da leitura de `stack` leva o erro ao
/// caminho de texto. Medido no bun 1.4.2: cabeçalho `Error`, sem o tipo nem a mensagem.
#[test]
fn stack_overflow_in_generators_takes_the_text_path() {
    let program = "function describe(src) { try { (0, eval)(src); } catch (e) { var t = String(e); return e.stack.slice(0, t.length) === t; } }
        R = [describe('function* g() { yield* g() } g().next()'), describe('function* g() { yield* g() } [...g()]'),
             describe('var g = function* () { for (var x of g()); }; g().next()'), describe('function f() { f() } f()')].join();";
    assert_eq!(stressed(program), "false,false,false,true");
}

/// As tabelas laterais por célula (o estado que no C++ é campo do objeto) seguram o que o objeto segura e saem
/// com ele: `URL.searchParams`, `Request.headers` e `signal`, `Response.headers`, o `reason` e as origens de um
/// `AbortSignal.any`. Sem adiar a coleta: o laço solta células enquanto elas existem.
#[test]
fn side_tables_follow_their_objects() {
    let program = format!("{CHURN} var u = new URL('http://a/?x=1'); u.searchParams.append('y', '2');
        var c = new AbortController(); var q = new Request('http://a/', {{ headers: {{ k: 'v' }}, signal: c.signal }});
        var r = new Response('b', {{ headers: {{ z: 'w' }} }}); var any = AbortSignal.any([c.signal]); c.abort({{ why: 'w' }});
        for (var i = 0; i < 20; i++) {{ new URL('http://t/' + i); new Request('http://t/'); new Response('x'); }}
        churn();
        R = [u.search, q.headers.get('k'), q.signal.aborted, r.headers.get('z'), any.reason.why].join();");
    let freed_before = gc::statistics().freed_cells;
    assert_eq!(stressed(&program), "?x=1&y=2,v,true,w,w");
    assert!(gc::statistics().freed_cells > freed_before, "as tabelas laterais ainda adiam a coleta");
}

/// Segunda leva: o `detail` de marcas e medidas, o observador que observa (raiz) e o desconectado (com o objeto),
/// a lista entregue ao callback, a mensagem na fila do `BroadcastChannel` e o iterador de `Headers` e
/// `URLSearchParams` (segura a instância).
#[test]
fn performance_broadcast_and_iterators_survive() {
    let program = format!("{CHURN} var log = [];
        performance.mark('m', {{ detail: {{ d: 'mark' }} }});
        new PerformanceObserver((list) => {{ churn(); log.push(list.getEntries()[0].detail.d); }}).observe({{ type: 'mark' }});
        var a = new BroadcastChannel('c'), b = new BroadcastChannel('c'); b.onmessage = (e) => log.push(e.data.v); a.postMessage({{ v: 'bc' }});
        var it = new Headers({{ k: 'v' }}).entries(); var sp = new URLSearchParams('x=1').keys();
        churn(); performance.mark('n', {{ detail: {{ d: 'obs' }} }});
        setTimeout(() => {{ churn(); log.push(performance.getEntriesByName('m')[0].detail.d, it.next().value.join('='), sp.next().value); a.close(); b.close(); R = log.sort().join(); }}, 5);");
    assert_eq!(stressed(&program), "bc,k=v,mark,obs,x");
}

/// O lote de mensagens de uma porta sai da fila antes do primeiro ouvinte rodar: as que ainda não foram entregues
/// vivem só no laço de entrega.
#[test]
fn queued_port_messages_survive_the_batch() {
    let program = format!("{CHURN} var log = []; var c = new MessageChannel();
        c.port2.onmessage = (e) => {{ churn(); log.push(e.data instanceof Uint8Array ? e.data.join('') : e.data.v); }};
        c.port1.postMessage({{ v: 'a' }}); c.port1.postMessage(new Uint8Array([1, 2])); c.port1.postMessage({{ v: 'b' }});
        setTimeout(() => {{ c.port1.close(); R = log.join(); }}, 5);");
    assert_eq!(stressed(&program), "a,12,b");
}

/// Os erros de WebAssembly (a mensagem nasce no nativo) e o estado dos wrappers sobrevivem à coleta.
#[test]
fn webassembly_errors_and_wrappers_survive() {
    let program = format!("{CHURN} var T = (f) => {{ try {{ var v = f(); return typeof v === 'string' ? v : 'x'; }} catch (e) {{ return e.name + ': ' + e.message; }} }};
        var bytes = [0,97,115,109,1,0,0,0, 1,4,1,0x60,0,0, 13,3,1,0,0, 7,9,2, 1,97,0,0, 1,97,0,0];
        var a = T(() => {{ new WebAssembly.Module(new Uint8Array(bytes)); return 'ok'; }});
        churn();
        var m = new WebAssembly.Memory({{ initial: 1 }}); var b = m.buffer; churn();
        R = a.slice(0, 22) + '|' + (m.buffer === b) + '|' + m.buffer.byteLength;");
    assert_eq!(stressed(&program), "CompileError: WebAssem|true|65536");
}

/// `[...texto]` e `map` sobre as strings de um caractere: os valores que o spread cria e o `map` lê.
#[test]
fn string_spread_and_map_survive() {
    let program = "function leb(n) { var o = []; do { var b = n & 127; n >>= 7; if (n) b |= 128; o.push(b); } while (n); return o; }
        function str(s) { return [...leb(s.length), ...[...s].map(c => c.charCodeAt(0))]; }
        var out = []; for (var i = 0; i < 40; i++) out.push(str('run' + i).join('.'));
        R = out[0] + '|' + out[39];";
    assert_eq!(stressed(program), "4.114.117.110.48|5.114.117.110.51.57");
}

/// O callee do frame do `/app/main.js` (o wrapper CommonJS) nunca é dado como morto: o erro criado nele e lido com o
/// principal ainda executando, e o criado nele e lido num timer depois de o principal devolver, mantêm o caminho normal
/// de `stack` (cabeçalho com a mensagem e `originalLine`), com a coleta a cada alocação. No bun o wrapper do principal
/// segue vivo depois de devolver (medido com `Bun.gc(true)`); o caminho de texto (`Error` sem mensagem, sem
/// `originalLine`) é só de quem perdeu o callee. O `module.exports` do topo faz o principal ser CommonJS (sem ele o
/// bun o trata como ESM, cuja função, essa sim, morre depois de devolver).
#[test]
fn main_wrapper_callee_survives_collections() {
    let program = format!("module.exports.ready = true; {CHURN} var executing = new Error('executing'); churn();
        var running = executing.stack.split('\\n')[0] + ' ' + Object.getOwnPropertyNames(executing).includes('originalLine');
        var later = new Error('later');
        setTimeout(() => {{ churn(); console.log(running + '|' + later.stack.split('\\n')[0] + ' ' + Object.getOwnPropertyNames(later).includes('originalLine')); }}, 1);");
    assert_eq!(stressed_main(&program), "Error: executing true|Error: later true\n");
}

/// Cada tipo de dono sozinho, para a lápide apontar a aresta que falta.
fn survives(setup: &str, step: &str, check: &str, expected: &str) {
    let program = format!("{setup}; for (let i = 0; i < 250000; i++) {{ const g = {{ i: i }}; if (i % 1000 === 0) {{ {step}; }} }} String({check})");
    assert_eq!(run(&program), expected);
    assert!(gc::statistics().collections >= 1);
}

#[test]
fn closure_scope_survives() {
    survives("var fs = []; function mk(n) { var o = [n]; return () => o[0]; }", "fs.push(mk(i))", "fs[3]()", "3000");
}

#[test]
fn map_entries_survive() {
    survives("var m = new Map()", "m.set('k' + i, [i])", "m.get('k5000')[0]", "5000");
}

#[test]
fn set_entries_survive() {
    survives("var s = new Set()", "s.add([i])", "[...s][4][0]", "4000");
}

#[test]
fn nested_arrays_survive() {
    survives("var a = []", "a.push({ v: [i, 'x' + i] })", "a[9].v[1]", "x9000");
}

#[test]
fn class_instances_survive() {
    survives("class P { constructor(x) { this.x = x; } s() { return this.x + 1; } } var ps = []", "ps.push(new P(i))", "ps[2].s()", "2001");
}
