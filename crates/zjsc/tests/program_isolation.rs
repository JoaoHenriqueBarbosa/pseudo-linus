//! Isolamento entre programas: o `run_program` do `cell_registry` faz de cada programa um "processo". Em uma
//! única thread, vários programas rodam em sequência pelo avaliador público e nada pode vazar de um para o
//! seguinte (registro global de símbolos, literais de RegExp, strings vazias, tabelas ordenadas, for-in, módulos,
//! WebAssembly, `Atomics.waitAsync` pendente, `Error.captureStackTrace` / `prepareStackTrace`). Os esperados saem
//! de `bun arquivo.js` com `console.log(R)` no fim de cada programa.
use zjsc::api::eval::evaluate_named_script_result;
use zjsc::api::module::evaluate_module_map;
use zjsc::runtime::array_buffer::live_buffer_bytes;
use zjsc::runtime::cell_registry::{live_butterfly_bytes, live_cell_count};
use zjsc::runtime::vm::live_vm_count;

mod common;

/// Programa e a saída do bun (o texto da variável global `R`).
const CASES: &[(&str, &str)] = &[
    (
        "var R=[Symbol.for('k')===Symbol.for('k'),Symbol.keyFor(Symbol.for('k')),Symbol.keyFor(Symbol('x')),Symbol.for('k').toString()].join()",
        "true,k,,Symbol(k)",
    ),
    (
        "var R=[...Array(3)].map(function(){return /a(b+)c/g}).map(function(r){return r.test('xabbc')+':'+r.lastIndex+':'+r.source}).join()+'|'+[1,2].map(function(){return 'xabbc'.replace(/b+/,'-')}).join()",
        "true:5:a(b+)c,true:5:a(b+)c,true:5:a(b+)c|xa-c,xa-c",
    ),
    (
        "var R=['',\"\",''+'', [].join(), 'a'.slice(1), ''.length, ('x'+'').slice(1)==='', JSON.stringify(['',''])].join('|')",
        "|||||0|true|[\"\",\"\"]",
    ),
    (
        "var m=new Map(),s=new Set();for(var i=0;i<50000;i++){m.set(i,i*2);s.add('k'+i)}for(var i=0;i<50000;i+=2){m.delete(i);s.delete('k'+i)}var R=[m.size,s.size,m.get(49999),m.has(4),s.has('k3'),[...m.keys()].slice(0,3).join()].join()",
        "25000,25000,99998,false,true,1,3,5",
    ),
    (
        "var o={a:1,b:2,c:3};var p=Object.create(o);p.d=4;var ks=[];for(var k in p)ks.push(k);var arr=[10,20];arr.x=1;for(var k in arr)ks.push(k);for(var k in 'ab')ks.push(k);var R=ks.join()",
        "d,a,b,c,0,1,x,0,1",
    ),
    (
        "var m=new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]));var i=new WebAssembly.Instance(m,{});var R=[WebAssembly.Module.exports(m).map(function(e){return e.name+':'+e.kind}).join(),i.exports.add(2,40),i.exports.add(-1,1),m instanceof WebAssembly.Module].join()",
        "add:function,42,0,true",
    ),
    (
        "var a=new Int32Array(new SharedArrayBuffer(16));var w=Atomics.waitAsync(a,0,0,10000);var R=[w.async,typeof w.value.then,Atomics.notify(a,0,1)].join()",
        "true,function,1",
    ),
    ("var e={};Error.captureStackTrace(e);var R=[typeof e.stack,e.stack.indexOf('Error')===0].join()", "string,true"),
    (
        "Error.prepareStackTrace=function(err,frames){return 'custom:'+Array.isArray(frames)};var e=new Error('boom');var R=e.stack;Error.prepareStackTrace=undefined",
        "custom:true",
    ),
    // O `prepareStackTrace` do programa anterior não pode sobreviver: o programa abaixo precisa do `stack` padrão.
    ("var R=new Error('x').stack.split('\\n')[0]", "Error: x"),
];

fn run_case(source: &str) -> String {
    match common::guarded(|| evaluate_named_script_result(source, "isolation_case.js", "R")) {
        Ok(text) => text,
        Err(reason) => format!("<{reason}>"),
    }
}

const MODULE_FILES: &[(&str, &str)] = &[
    ("b.mjs", "export let n = 1; export function inc(){ n++ } export default 'd';"),
    ("main.mjs", "import d, {n, inc} from './b.mjs'; L(d + ' ' + n); inc(); L(n); L(await Promise.resolve('tla'));"),
];

fn run_module() -> String {
    let files: Vec<(String, String)> = MODULE_FILES.iter().map(|(name, text)| (name.to_string(), text.to_string())).collect();
    let outcome = evaluate_module_map(&files, "main.mjs");
    format!("{} {:?}", outcome.log_json, outcome.error)
}

/// Resident set size em KiB (campo `VmRSS` de `/proc/self/status`), ou `None` fora do Linux.
fn resident_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn isolation_body() {
    // Duas voltas pela lista inteira: o resultado de cada caso não pode depender do que rodou antes dele.
    for round in 0..2 {
        for (source, expected) in CASES {
            assert_eq!(run_case(source), *expected, "rodada {round}: {source}");
        }
    }
    // Ordem inversa: um programa que deixou rastro mudaria o vizinho do outro lado.
    for (source, expected) in CASES.iter().rev() {
        assert_eq!(run_case(source), *expected, "ordem inversa: {source}");
    }
    // Módulo: três vezes seguidas dão a mesma saída, e o mesmo caminho `b.mjs` não reaproveita registro algum.
    let first_module = run_module();
    assert_eq!(first_module, r#"["d 1","2","tla"] None"#, "módulo");
    assert_eq!(run_module(), first_module, "módulo, segunda vez");
    assert_eq!(run_module(), first_module, "módulo, terceira vez");
    // Intercalando programa e módulo.
    assert_eq!(run_case(CASES[0].0), CASES[0].1);
    assert_eq!(run_module(), first_module);
}

fn leak_body() {
    let program = "var a=new Array(131072).fill(1.5);var b=new Uint8Array(1048576);b[7]=3;var R=a.length+':'+b.length+':'+b[7]";
    let expected = "131072:1048576:3";
    // Aquecimento: o primeiro programa abre as tabelas de uma vez só (átomos, contadores) e o escopo do último
    // programa fica retido até o próximo começar, então a linha de base é medida com um programa já retido.
    for _ in 0..20 {
        assert_eq!(run_case(program), expected);
    }
    let baseline_cells = live_cell_count();
    let baseline_rss = resident_kib();
    for index in 0..2000 {
        assert_eq!(run_case(program), expected, "programa {index}");
        if index % 250 == 0 {
            let cells = live_cell_count();
            assert_eq!(cells, baseline_cells, "células vivas depois de {index} programas");
        }
    }
    assert_eq!(live_cell_count(), baseline_cells, "células vivas no fim");
    if let (Some(before), Some(after)) = (baseline_rss, resident_kib()) {
        // 2000 programas de 1 MiB somariam 2 GiB se vazassem; a folga cobre fragmentação do alocador.
        assert!(after < before + 256 * 1024, "RSS cresceu de {before} KiB para {after} KiB");
    }
}

#[test]
fn programs_do_not_leak_into_each_other() {
    common::run_with_stack(64 * 1024 * 1024, isolation_body);
}

#[test]
fn many_programs_do_not_grow_memory() {
    common::run_with_stack(64 * 1024 * 1024, leak_body);
}

fn buffers_body() {
    let program = "var a=new Array(131072).fill(1.5);var b=new Uint8Array(1048576);b[7]=3;var R=a.length+':'+b.length+':'+b[7]";
    let expected = "131072:1048576:3";
    // Aquecimento, como no `leak_body`: o escopo do último programa fica retido até o próximo começar.
    for _ in 0..20 {
        assert_eq!(run_case(program), expected);
    }
    let base_buffer = live_buffer_bytes();
    let base_butterfly = live_butterfly_bytes();
    let base_cells = live_cell_count();
    println!("base: buffer={base_buffer} butterfly={base_butterfly} células={base_cells} rss={:?} KiB", resident_kib());
    for index in 0..200 {
        assert_eq!(run_case(program), expected, "programa {index}");
        let (buffer, butterfly, cells) = (live_buffer_bytes(), live_butterfly_bytes(), live_cell_count());
        if index % 20 == 0 || buffer != base_buffer || butterfly != base_butterfly {
            println!("programa {index}: buffer={buffer} butterfly={butterfly} células={cells} rss={:?} KiB", resident_kib());
        }
        assert_eq!(buffer, base_buffer, "bytes de ArrayBuffer vivos depois do programa {index}");
        assert_eq!(butterfly, base_butterfly, "bytes de butterfly vivos depois do programa {index}");
    }
}

/// Separa vazamento real de fragmentação do alocador: se os bytes vivos de buffer e butterfly voltam à linha
/// de base a cada programa, o RSS que cresce é do `malloc`; se não voltam, algo retém o armazenamento.
#[test]
fn buffers_are_released_between_programs() {
    std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(buffers_body).expect("thread").join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// Programas variados que exercitam os ciclos que o `VM` forma com o global (exceção com pilha, closure,
/// classe, `eval`, WebAssembly, promessa pendente, TDZ capturado). `{n}` varia a cada volta.
const VARIED_PROGRAMS: &[&str] = &[
    "var R=(function(){try{f();}catch(e){return e.name+':'+(typeof e.stack)}function f(){g();let x=1}function g(){return (()=>y)();}let y=g})()+{n}",
    "function mk(k){return function(){return k+{n}}}var fs=[];for(var i=0;i<20;i++)fs.push(mk(i));var R=fs.map(function(f){return f()}).length",
    "class A{#p={n};static s=1;get v(){return this.#p}}class B extends A{constructor(){super();this.w=()=>this.v}}var R=new B().w()",
    "var R=eval('var q={n};(function(){return q})()')",
    "var m=new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11]));var R=new WebAssembly.Instance(m,{}).exports.add({n},1)",
    "var pend=new Promise(function(){});pend.then(function(){});var R=typeof pend",
    "var e=new Error('boom{n}');var R=e.stack.split('\\n')[0]",
    "var e={};Error.captureStackTrace(e);var R=typeof e.stack+(function(){return new Error('x').stack.length>0})()",
];

fn varied_body() {
    let files: Vec<(String, String)> = MODULE_FILES.iter().map(|(name, text)| (name.to_string(), text.to_string())).collect();
    // Aquecimento: abre as tabelas por thread de uma vez só antes de medir o RSS.
    for index in 0..30 {
        let source = VARIED_PROGRAMS[index % VARIED_PROGRAMS.len()].replace("{n}", &index.to_string());
        let _ = run_case(&source);
    }
    let baseline_rss = resident_kib();
    for index in 0..300usize {
        // Um a cada nove programas é o avaliador de módulo; os demais, os variados.
        if index % 9 == 8 {
            let outcome = evaluate_module_map(&files, "main.mjs");
            assert_eq!(outcome.error, None, "módulo no programa {index}");
        } else {
            let source = VARIED_PROGRAMS[index % VARIED_PROGRAMS.len()].replace("{n}", &index.to_string());
            let text = run_case(&source);
            assert!(!text.starts_with('<'), "programa {index} falhou: {text}");
        }
        // O escopo do último programa fica retido até o próximo começar, então no máximo um VM vivo.
        assert!(live_vm_count() <= 1, "VMs vivos depois do programa {index}: {}", live_vm_count());
    }
    if let (Some(before), Some(after)) = (baseline_rss, resident_kib()) {
        assert!(after < before + 128 * 1024, "RSS cresceu de {before} KiB para {after} KiB");
    }
}

#[test]
fn varied_programs_do_not_leak_vms() {
    std::thread::Builder::new().stack_size(64 * 1024 * 1024).spawn(varied_body).expect("thread").join().unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
