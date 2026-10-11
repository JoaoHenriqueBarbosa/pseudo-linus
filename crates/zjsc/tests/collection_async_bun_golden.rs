//! Golden de métodos de conjunto contra set-likes que registram acessos, Map/Set sob mutação entre awaits,
//! Map.groupBy/Object.groupBy, getOrInsert, Array.fromAsync e Iterator helpers contra o JavaScriptCore real:
//! `tests/golden/collection_async_bun.tsv` sai de `scripts/gen-collection-async-golden.js`, rodado no bun 1.4.2 com
//! `require('node:vm').runInThisContext(src)` (nunca como arquivo, para o transpilador do bun não tocar na fonte).
//! Os programas não usam API de host: só os auxiliares `L`, `tick`, `S`, `T`, `TA`, `KI` e `SL` do prelúdio abaixo
//! (o mesmo texto que o gerador embute). Cada linha é um programa e o JSON do log depois de esvaziar as
//! microtarefas, ou `error`, `name`, `message` (JSON) se a fonte lançou de forma síncrona. Cada programa roda num
//! realm novo.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_result;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("golden/collection_async_bun.tsv");

/// Mesmo texto de `HARNESS` em `scripts/gen-collection-async-golden.js`.
const HARNESS: &str = r##"globalThis.log = [];
globalThis.L = function (x) { log.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.S = function S(v, d) {
  d = d || 0; var t = typeof v;
  if (v === null) return "null";
  if (t === "undefined") return "undefined";
  if (t === "string") return JSON.stringify(v);
  if (t === "symbol") return String(v);
  if (t === "bigint") return v + "n";
  if (t === "number") return Object.is(v, -0) ? "-0" : String(v);
  if (t === "boolean") return String(v);
  if (t === "function") return "fn";
  if (d > 4) return "...";
  if (v instanceof Error) return v.name + ":" + v.message;
  if (Array.isArray(v)) return "[" + Array.from(v, function (x) { return S(x, d + 1); }).join(",") + "]";
  if (v instanceof Set) return "Set(" + S(Array.from(v), d + 1) + ")";
  if (v instanceof Map) return "Map(" + S(Array.from(v), d + 1) + ")";
  return "{" + Object.keys(v).map(function (k) { return k + ":" + S(v[k], d + 1); }).join(",") + "}";
};
globalThis.T = function (f) {
  try { return S(f()); } catch (e) { return "throw " + (e && e.name) + ": " + (e && e.message); }
};
globalThis.TA = function (label, f) {
  try {
    return Promise.resolve(f()).then(function (v) { L(label + "=" + S(v)); }, function (e) { L(label + "!" + S(e)); });
  } catch (e) { L(label + "!!" + S(e)); }
};
globalThis.KI = function (arr, o) {
  o = o || {}; var i = 0;
  var it = {
    next: function () {
      L("next");
      if (o.throwAt === i) throw new Error("boom");
      return i < arr.length ? { value: arr[i++], done: false } : { value: undefined, done: true };
    },
  };
  if (!o.noReturn) it.return = function (v) { L("return"); if (o.returnThrows) throw new Error("rt"); return o.returnPrim ? 1 : {}; };
  it[Symbol.iterator] = function () { L("@@iterator"); return it; };
  return it;
};
globalThis.SL = function (o) {
  var r = {};
  Object.defineProperty(r, "size", { enumerable: true, get: function () { L("get size"); return "size" in o ? (typeof o.size === "function" ? o.size() : o.size) : undefined; } });
  Object.defineProperty(r, "has", { enumerable: true, get: function () {
    L("get has"); var h = o.has;
    return typeof h === "function" ? function (x) { L("has(" + S(x) + ")"); return h.call(this, x); } : h;
  } });
  Object.defineProperty(r, "keys", { enumerable: true, get: function () {
    L("get keys"); var k = o.keys;
    return typeof k === "function" ? function () { L("keys()"); return k.call(this); } : k;
  } });
  return r;
};
globalThis.__err = null;
globalThis.__final = function () { return __err !== null ? __err : JSON.stringify(log); };
globalThis.__run = function (src) {
  try { (0, eval)(src); } catch (e) { __err = "error\t" + e.name + "\t" + JSON.stringify(String(e.message)); }
};"##;

/// `JSON.stringify(source)`: o que o prelúdio recebe em `__run`.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if (control as u32) < 0x20 => quoted.push_str(&format!("\\u{:04x}", control as u32)),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// Roda um programa no prelúdio, esvazia as microtarefas e devolve o resultado de `__final()`.
fn run(source: &str) -> Result<String, String> {
    let program = format!("{}\n__run({});", HARNESS, json_quote(source));
    let outcome =
        catch_unwind(AssertUnwindSafe(|| evaluate_named_script_result(&program, "collection_async_bun_golden.js", "__final()")));
    match outcome {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        Ok(Ok(_)) => Err("__final não devolveu string".to_string()),
        Ok(Err(_)) => Err("o prelúdio lançou exceção".to_string()),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

/// Pilha da thread do golden, igual à do golden de escopo.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

#[test]
fn collection_async_programs_match_bun() {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            collection_async_body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

fn collection_async_body() {
    let mut failures = Vec::new();
    let mut total = 0;
    for line in GOLDEN.lines().filter(|line| !line.is_empty()) {
        let (source, expected) = line.split_once('\t').expect("linha com fonte e resultado");
        total += 1;
        match run(source) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!("{source}\n    esperado {expected}\n    veio     {actual}")),
            Err(reason) => failures.push(format!("{source}\n    esperado {expected}\n    {reason}")),
        }
    }
    assert!(total >= 800, "o golden tem só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}
