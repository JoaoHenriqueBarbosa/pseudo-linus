//! Golden das propriedades próprias dos construtores e protótipos globais, contra o JavaScriptCore
//! real: `wip-notes/bun-builtin-props.json` é a medição do bun 1.4.2 (`Object.getOwnPropertyNames` de
//! cada construtor e protótipo, só strings, já ordenadas por unidade de código UTF-16). Por isso o
//! primeira checagem compara o CONJUNTO de nomes: a ordem de instalação não está nessa medição.
//!
//! A segunda checagem usa `tests/golden/own_keys_bun.json` (gerado por `scripts/gen-own-keys-golden.js`),
//! medido SEM ordenar: `Reflect.ownKeys` na ordem de instalação, com os Symbols (`@@sym:` mais a
//! descrição), os atributos `writable`/`enumerable`/`configurable` (ou `get`/`set` nos acessores) e
//! `length`/`name` de cada função. Ela só roda quando o conjunto já bate. `globalThis` é filtrado pela
//! lista `only` do golden, porque o bun acrescenta globais que não são do JavaScriptCore.
//!
//! O JSON entra no programa como literal e é desmontado em JavaScript (`JSON.parse`); cada caminho
//! (`Object.prototype`, `Array`...) é resolvido a partir de `globalThis` e o programa devolve uma
//! linha por objeto divergente, com os nomes que faltam (`-nome`) e os que sobram (`+nome`).
//!
//! `Error.appendStackTrace` e `Error.prepareStackTrace` existem só no bun (`ZigGlobalObject`), não no
//! JavaScriptCore; ficam fora da comparação.
use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

const GOLDEN: &str = include_str!("../wip-notes/bun-builtin-props.json");
const ORDERED_GOLDEN: &str = include_str!("golden/own_keys_bun.json");

/// Ordem, Symbols e atributos: uma linha por divergência, prefixada pelo caminho.
/// `bunOnly` (nos dois programas): `Error.appendStackTrace/prepareStackTrace`.
const ORDERED_PROGRAM: &str = r#"(function (spec) {
  var bunOnly = { "Error": ["appendStackTrace", "prepareStackTrace"] };
  var root = typeof globalThis !== "undefined" ? globalThis : (0, eval)("this");
  var gf = function () { return Object.getPrototypeOf(eval("(function* () {})")); };
  var agf = function () { return Object.getPrototypeOf(eval("(async function* () {})")); };
  var af = function () { return Object.getPrototypeOf(eval("(async function () {})")); };
  var specials = {
    "%ArrayIteratorPrototype%": function () { return Object.getPrototypeOf([][Symbol.iterator]()); },
    "%MapIteratorPrototype%": function () { return Object.getPrototypeOf(new Map()[Symbol.iterator]()); },
    "%SetIteratorPrototype%": function () { return Object.getPrototypeOf(new Set()[Symbol.iterator]()); },
    "%StringIteratorPrototype%": function () { return Object.getPrototypeOf(""[Symbol.iterator]()); },
    "%RegExpStringIteratorPrototype%": function () { return Object.getPrototypeOf(/a/[Symbol.matchAll]("")); },
    "%GeneratorFunction%": function () { return gf().constructor; },
    "%GeneratorFunctionPrototype%": gf,
    "%GeneratorPrototype%": function () { return gf().prototype; },
    "%AsyncGeneratorFunction%": function () { return agf().constructor; },
    "%AsyncGeneratorFunctionPrototype%": agf,
    "%AsyncGeneratorPrototype%": function () { return agf().prototype; },
    "%AsyncFunction%": function () { return af().constructor; },
    "%AsyncFunctionPrototype%": af,
    "%AsyncIteratorPrototype%": function () { return Object.getPrototypeOf(agf().prototype); },
    "%TypedArray%": function () { return Object.getPrototypeOf(Int8Array); },
    "%TypedArrayPrototype%": function () { return Object.getPrototypeOf(Int8Array.prototype); }
  };
  function encode(key) {
    return typeof key === "symbol" ? "@@sym:" + (key.description === undefined ? "" : key.description) : key;
  }
  var lines = [];
  var paths = Object.keys(spec);
  for (var i = 0; i < paths.length; i++) {
    var path = paths[i];
    var record = spec[path];
    var parts = path.split(".");
    var target = root;
    for (var j = 0; j < parts.length && target != null; j++) {
      target = j === 0 && specials[parts[j]] ? specials[parts[j]]() : target[parts[j]];
    }
    if (target == null) { lines.push(path + ": ausente"); continue; }
    var skip = bunOnly[path] || [];
    var expected = record.keys.filter(function (key) { return skip.indexOf(key) < 0; });
    var raw = Reflect.ownKeys(target);
    if (record.only) raw = raw.filter(function (key) { return typeof key === "string" && record.only.indexOf(key) >= 0; });
    var actual = raw.map(encode).filter(function (key) { return skip.indexOf(key) < 0; });
    var missing = expected.filter(function (key) { return actual.indexOf(key) < 0; });
    var extra = actual.filter(function (key) { return expected.indexOf(key) < 0; });
    if (missing.length || extra.length) {
      lines.push(path + " conjunto: " + missing.map(function (n) { return "-" + n; })
        .concat(extra.map(function (n) { return "+" + n; })).join(" "));
      continue;
    }
    var at = -1;
    for (var k = 0; k < expected.length; k++) if (expected[k] !== actual[k]) { at = k; break; }
    if (at >= 0) {
      lines.push(path + " ordem: na posição " + at + " esperado " + expected[at] + ", obtido " + actual[at]);
      lines.push("  ordem obtida:   " + actual.join(","));
      lines.push("  ordem esperada: " + expected.join(","));
      continue;
    }
    for (var m = 0; m < raw.length; m++) {
      var name = encode(raw[m]);
      var want = record.props[name];
      if (!want) continue;
      var d = Object.getOwnPropertyDescriptor(target, raw[m]);
      var got = {};
      if ("value" in d) {
        got.w = d.writable;
        if (typeof d.value === "function") { got.length = d.value.length; got.name = d.value.name; }
      } else {
        got.get = d.get !== undefined;
        got.set = d.set !== undefined;
      }
      got.e = d.enumerable;
      got.c = d.configurable;
      var fields = Object.keys(want).concat(Object.keys(got).filter(function (f) { return !(f in want); }));
      var diffs = fields.filter(function (f) { return want[f] !== got[f]; })
        .map(function (f) { return f + "=" + String(got[f]) + " (bun " + String(want[f]) + ")"; });
      if (diffs.length) lines.push(path + " atributos de " + name + ": " + diffs.join(", "));
    }
  }
  return lines.join("\n");
})"#;

const PROGRAM: &str = r#"(function (spec) {
  var bunOnly = { "Error": ["appendStackTrace", "prepareStackTrace"] };
  var root = typeof globalThis !== "undefined" ? globalThis : (0, eval)("this");
  var lines = [];
  var paths = Object.keys(spec);
  for (var i = 0; i < paths.length; i++) {
    var path = paths[i];
    var parts = path.split(".");
    var target = root;
    for (var j = 0; j < parts.length && target != null; j++) target = target[parts[j]];
    if (target == null) { lines.push(path + ": ausente"); continue; }
    var skip = bunOnly[path] || [];
    var expected = spec[path].split(",").filter(function (name) { return skip.indexOf(name) < 0; });
    var actual = Object.getOwnPropertyNames(target).filter(function (name) { return skip.indexOf(name) < 0; });
    var missing = expected.filter(function (name) { return actual.indexOf(name) < 0; });
    var extra = actual.filter(function (name) { return expected.indexOf(name) < 0; });
    if (missing.length || extra.length) {
      lines.push(path + ": " + missing.map(function (n) { return "-" + n; })
        .concat(extra.map(function (n) { return "+" + n; })).join(" "));
    }
  }
  return lines.join("\n");
})"#;

/// `JSON.stringify(source)` para fonte ASCII.
fn json_quote(source: &str) -> String {
    let mut quoted = String::with_capacity(source.len() + 2);
    quoted.push('"');
    for character in source.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
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

/// Avalia `program(JSON.parse(golden))` e devolve a string do relatório.
fn run_report(program: &str, golden: &str) -> String {
    let program = format!("{}(JSON.parse({}))", program, json_quote(golden.trim()));
    match catch_unwind(AssertUnwindSafe(|| evaluate_script(&program))) {
        Ok(Ok(value)) if value.is_string() => {
            let bytes = value.as_js_string().value().utf8(ConversionMode::LenientConversion);
            String::from_utf8_lossy(&bytes).into_owned()
        }
        Ok(Ok(_)) => panic!("o programa não devolveu string"),
        Ok(Err(_)) => panic!("o programa lançou exceção"),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            panic!("pânico: {reason}")
        }
    }
}

#[test]
fn builtin_own_property_names_match_bun() {
    let set_report = run_report(PROGRAM, GOLDEN);
    assert!(set_report.is_empty(), "objetos com nomes próprios divergentes do bun (-falta, +sobra):\n{set_report}");
    let ordered_report = run_report(ORDERED_PROGRAM, ORDERED_GOLDEN);
    assert!(
        ordered_report.is_empty(),
        "conjunto, ordem, Symbols ou atributos divergentes do bun:\n{ordered_report}"
    );
}
