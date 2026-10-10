//! Cadeia de `Proxy` sem trap: cada operação que recursa no alvo gasta pilha, e a cadeia longa demais dá
//! `RangeError: Maximum call stack size exceeded.`, como no bun 1.4.2. Os limites (o maior número de `Proxy` que
//! ainda passa) foram medidos no bun com a mesma busca binária deste teste, rodada no topo de um script: a pilha
//! do bun inclui os frames de JS, então o limite medido vale para essa profundidade. Roda numa thread de pilha
//! grande, como os goldens: quem decide é a pilha lógica, não a nativa.
use zjsc::api::eval::evaluate_script;

const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

fn run(source: &str) -> String {
    let source = source.to_owned();
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            let value = evaluate_script(&source)
                .unwrap_or_else(|thrown| panic!("lançou exceção ({}): {source}", zjsc::api::eval::describe_exception(&thrown)));
            String::from_utf8_lossy(&value.to_wtf_string().utf8(zjsc::wtf::text::conversion_mode::ConversionMode::LenientConversion))
                .into_owned()
        })
        .expect("thread")
        .join()
        .expect("thread terminou sem pânico")
}

#[test]
fn proxy_chains_overflow_where_bun_does() {
    let source = "function chain(n, base) { let p = base(); for (let i = 0; i < n; i++) p = new Proxy(p, {}); return p; } \
        function limit(base, op) { let lo = 0, hi = 60000; \
          const ok = n => { try { op(chain(n, base)); return true } catch (e) { if (!(e instanceof RangeError)) throw e; return false } }; \
          while (hi - lo > 1) { const m = (lo + hi) >> 1; if (ok(m)) lo = m; else hi = m; } return lo; } \
        const o = () => ({}), f = () => function () { return 1 }; \
        [limit(o, p => 'x' in p), limit(o, p => p.x), limit(o, p => Object.getOwnPropertyDescriptor(p, 'x')), limit(o, p => { p.x = 1 }), \
         limit(o, p => Object.keys(p)), limit(o, p => Object.getPrototypeOf(p)), limit(o, p => Object.setPrototypeOf(p, null)), \
         limit(o, p => Object.defineProperty(p, 'x', { value: 1 })), limit(o, p => delete p.x), limit(o, p => Object.isExtensible(p)), \
         limit(o, p => Object.preventExtensions(p)), limit(f, p => p()), limit(f, p => new p())].join()";
    assert_eq!(run(source), "12278,12278,12278,8185,11401,39907,31925,16802,18779,39907,39906,7786,5600");
}

/// O frame sai da conta na volta, inclusive no erro: a cadeia que passa continua passando depois de um estouro.
#[test]
fn proxy_frames_are_released() {
    let source = "let p = {}; for (let i = 0; i < 12000; i++) p = new Proxy(p, {}); \
                  let n = 0; for (let k = 0; k < 50; k++) { 'x' in p; n++; } \
                  let q = p; for (let i = 0; i < 1000; i++) q = new Proxy(q, {}); try { 'x' in q } catch (e) {} \
                  'x' in p; n";
    assert_eq!(run(source), "50");
}
