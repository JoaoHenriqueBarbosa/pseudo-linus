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
