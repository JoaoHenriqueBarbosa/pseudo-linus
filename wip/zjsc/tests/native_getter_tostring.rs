//! `name`, `length` e `Function.prototype.toString` dos getters e setters nativos, com as formas medidas no bun 1.4.2.
//!
//! Duas famílias, e a diferença está no `NativeExecutable`:
//! - `JSC_NATIVE_GETTER`/`JSC_NATIVE_INTRINSIC_GETTER`, `reifyStaticAccessor` (entrada `Accessor` da tabela), o
//!   `size` de `Map`/`Set` e o `__proto__`: `JSFunction::create(.., "get <nome>", ..)` põe o prefixo no próprio
//!   executável, então `name` e `toString()` dizem `get <nome>` (`function get size() { [native code] }`).
//! - `CustomAccessor` (`JSCustomGetterFunction`/`JSCustomSetterFunction`, `Function.prototype.caller`/`arguments`,
//!   `Symbol.prototype.description`, `DataView.prototype.buffer`/`byteOffset`, `Intl.*.compare`/`format`): o executável
//!   guarda o nome puro e a propriedade `name` é reificada à mão como `get <nome>`, então `toString()` diz só
//!   `function <nome>() { [native code] }`.
//!
//! O `bind` parte do `name` da propriedade para o nome e do executável para o `toString`.
mod common;

/// (expressão do objeto, chave, "get" ou "set", resultado de `name|length|toString`).
const CASES: &[(&str, &str, &str, &str)] = &[
    ("Map.prototype", "size", "get", "get size|0|function get size() { [native code] }"),
    ("Set.prototype", "size", "get", "get size|0|function get size() { [native code] }"),
    ("ArrayBuffer.prototype", "byteLength", "get", "get byteLength|0|function get byteLength() { [native code] }"),
    ("ArrayBuffer.prototype", "maxByteLength", "get", "get maxByteLength|0|function get maxByteLength() { [native code] }"),
    ("ArrayBuffer.prototype", "resizable", "get", "get resizable|0|function get resizable() { [native code] }"),
    ("ArrayBuffer.prototype", "detached", "get", "get detached|0|function get detached() { [native code] }"),
    ("SharedArrayBuffer.prototype", "byteLength", "get", "get byteLength|0|function get byteLength() { [native code] }"),
    ("SharedArrayBuffer.prototype", "growable", "get", "get growable|0|function get growable() { [native code] }"),
    ("SharedArrayBuffer.prototype", "maxByteLength", "get", "get maxByteLength|0|function get maxByteLength() { [native code] }"),
    ("RegExp.prototype", "flags", "get", "get flags|0|function get flags() { [native code] }"),
    ("RegExp.prototype", "source", "get", "get source|0|function get source() { [native code] }"),
    ("RegExp.prototype", "global", "get", "get global|0|function get global() { [native code] }"),
    ("RegExp.prototype", "dotAll", "get", "get dotAll|0|function get dotAll() { [native code] }"),
    ("RegExp.prototype", "hasIndices", "get", "get hasIndices|0|function get hasIndices() { [native code] }"),
    ("RegExp.prototype", "ignoreCase", "get", "get ignoreCase|0|function get ignoreCase() { [native code] }"),
    ("RegExp.prototype", "multiline", "get", "get multiline|0|function get multiline() { [native code] }"),
    ("RegExp.prototype", "sticky", "get", "get sticky|0|function get sticky() { [native code] }"),
    ("RegExp.prototype", "unicode", "get", "get unicode|0|function get unicode() { [native code] }"),
    ("RegExp.prototype", "unicodeSets", "get", "get unicodeSets|0|function get unicodeSets() { [native code] }"),
    ("DataView.prototype", "byteLength", "get", "get byteLength|0|function get byteLength() { [native code] }"),
    ("Object.getPrototypeOf(Int8Array).prototype", "buffer", "get", "get buffer|0|function get buffer() { [native code] }"),
    ("Object.getPrototypeOf(Int8Array).prototype", "byteLength", "get", "get byteLength|0|function get byteLength() { [native code] }"),
    ("Object.getPrototypeOf(Int8Array).prototype", "byteOffset", "get", "get byteOffset|0|function get byteOffset() { [native code] }"),
    ("Object.getPrototypeOf(Int8Array).prototype", "length", "get", "get length|0|function get length() { [native code] }"),
    ("Object.prototype", "__proto__", "get", "get __proto__|0|function get __proto__() { [native code] }"),
    ("Object.prototype", "__proto__", "set", "set __proto__|0|function set __proto__() { [native code] }"),
    ("Function.prototype", "caller", "get", "get caller|0|function caller() { [native code] }"),
    ("Function.prototype", "caller", "set", "set caller|1|function caller() { [native code] }"),
    ("Function.prototype", "arguments", "get", "get arguments|0|function arguments() { [native code] }"),
    ("Function.prototype", "arguments", "set", "set arguments|1|function arguments() { [native code] }"),
    ("Symbol.prototype", "description", "get", "get description|0|function description() { [native code] }"),
    ("DataView.prototype", "buffer", "get", "get buffer|0|function buffer() { [native code] }"),
    ("DataView.prototype", "byteOffset", "get", "get byteOffset|0|function byteOffset() { [native code] }"),
    ("Intl.Collator.prototype", "compare", "get", "get compare|0|function compare() { [native code] }"),
    ("Intl.NumberFormat.prototype", "format", "get", "get format|0|function format() { [native code] }"),
    ("Intl.DateTimeFormat.prototype", "format", "get", "get format|0|function format() { [native code] }"),
    ("Intl.Locale.prototype", "baseName", "get", "get baseName|0|function baseName() { [native code] }"),
    // Os statics legados do `RegExp` (`CustomAccessor`): sem setter os de leitura, com setter `input`/`$_`/`multiline`/`$*`.
    ("RegExp", "$1", "get", "get $1|0|function $1() { [native code] }"),
    ("RegExp", "$2", "get", "get $2|0|function $2() { [native code] }"),
    ("RegExp", "$3", "get", "get $3|0|function $3() { [native code] }"),
    ("RegExp", "$4", "get", "get $4|0|function $4() { [native code] }"),
    ("RegExp", "$5", "get", "get $5|0|function $5() { [native code] }"),
    ("RegExp", "$6", "get", "get $6|0|function $6() { [native code] }"),
    ("RegExp", "$7", "get", "get $7|0|function $7() { [native code] }"),
    ("RegExp", "$8", "get", "get $8|0|function $8() { [native code] }"),
    ("RegExp", "$9", "get", "get $9|0|function $9() { [native code] }"),
    ("RegExp", "input", "get", "get input|0|function input() { [native code] }"),
    ("RegExp", "input", "set", "set input|1|function input() { [native code] }"),
    ("RegExp", "$_", "get", "get $_|0|function $_() { [native code] }"),
    ("RegExp", "$_", "set", "set $_|1|function $_() { [native code] }"),
    ("RegExp", "multiline", "get", "get multiline|0|function multiline() { [native code] }"),
    ("RegExp", "multiline", "set", "set multiline|1|function multiline() { [native code] }"),
    ("RegExp", "$*", "get", "get $*|0|function $*() { [native code] }"),
    ("RegExp", "$*", "set", "set $*|1|function $*() { [native code] }"),
    ("RegExp", "lastMatch", "get", "get lastMatch|0|function lastMatch() { [native code] }"),
    ("RegExp", "$&", "get", "get $&|0|function $&() { [native code] }"),
    ("RegExp", "lastParen", "get", "get lastParen|0|function lastParen() { [native code] }"),
    ("RegExp", "$+", "get", "get $+|0|function $+() { [native code] }"),
    ("RegExp", "leftContext", "get", "get leftContext|0|function leftContext() { [native code] }"),
    ("RegExp", "$`", "get", "get $`|0|function $`() { [native code] }"),
    ("RegExp", "rightContext", "get", "get rightContext|0|function rightContext() { [native code] }"),
    ("RegExp", "$'", "get", "get $'|0|function $'() { [native code] }"),
    // `Iterator.prototype`: `constructor` e `@@toStringTag` são `CustomGetterSetter`; a chave símbolo dá nome vazio.
    ("Iterator.prototype", "constructor", "get", "get constructor|0|function constructor() { [native code] }"),
    ("Iterator.prototype", "constructor", "set", "set constructor|1|function constructor() { [native code] }"),
    ("Iterator.prototype", "Symbol.toStringTag", "get", "get |0|function () { [native code] }"),
    ("Iterator.prototype", "Symbol.toStringTag", "set", "set |1|function () { [native code] }"),
    // `Temporal`: todos os getters são `CustomAccessor`, sem setter.
    ("Temporal.Duration.prototype", "years", "get", "get years|0|function years() { [native code] }"),
    ("Temporal.PlainDate.prototype", "year", "get", "get year|0|function year() { [native code] }"),
    ("Temporal.PlainDate.prototype", "calendarId", "get", "get calendarId|0|function calendarId() { [native code] }"),
    ("Temporal.PlainTime.prototype", "hour", "get", "get hour|0|function hour() { [native code] }"),
    ("Temporal.ZonedDateTime.prototype", "timeZoneId", "get", "get timeZoneId|0|function timeZoneId() { [native code] }"),
    ("Temporal.ZonedDateTime.prototype", "epochMilliseconds", "get", "get epochMilliseconds|0|function epochMilliseconds() { [native code] }"),
];

/// A expressão da chave: `Symbol.xxx` vai sem aspas, o resto é string.
fn key_expression(key: &str) -> String {
    if key.starts_with("Symbol.") { key.to_string() } else { format!("\"{key}\"") }
}

/// Os `@@species` (`globalFuncSpeciesGetter`): o nome inclui o símbolo e o `toString` também.
const SPECIES: &[&str] = &["Map", "Array", "Promise", "RegExp", "ArrayBuffer", "Object.getPrototypeOf(Int8Array)"];

fn json(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn line(program: &str, expected: &str) -> String {
    format!("{}\t{}\n", json(program), json(expected))
}

#[test]
fn native_getter_to_string_matches_bun() {
    let mut tsv = String::new();
    for (object, key, kind, expected) in CASES {
        let key_js = key_expression(key);
        let program = format!(
            "globalThis.R=(()=>{{var F=Object.getOwnPropertyDescriptor({object},{key_js}).{kind};return F.name+\"|\"+F.length+\"|\"+Function.prototype.toString.call(F)}})()"
        );
        tsv.push_str(&line(&program, expected));
        // `String(fn)` e `Function.prototype.toString` são o mesmo texto, e o `own` é só `length,name`.
        let own = format!(
            "globalThis.R=(()=>{{var F=Object.getOwnPropertyDescriptor({object},{key_js}).{kind};return String(F)+\"|\"+Object.getOwnPropertyNames(F).join()+\"|\"+(\"prototype\" in F)}})()"
        );
        let string_form = expected.split('|').nth(2).unwrap();
        tsv.push_str(&line(&own, &format!("{string_form}|length,name|false")));
    }
    for constructor in SPECIES {
        let program = format!(
            "globalThis.R=(()=>{{var F=Object.getOwnPropertyDescriptor({constructor},Symbol.species).get;return F.name+\"|\"+F.length+\"|\"+Function.prototype.toString.call(F)}})()"
        );
        tsv.push_str(&line(&program, "get [Symbol.species]|0|function get [Symbol.species]() { [native code] }"));
    }
    // `bind` parte do `name` da propriedade (com o prefixo), e o `toString` do executável.
    tsv.push_str(&line(
        "globalThis.R=(()=>{var B=Object.getOwnPropertyDescriptor(RegExp.prototype,\"unicode\").get.bind(null,1);return B.name+\"|\"+B.length+\"|\"+String(B)})()",
        "bound get unicode|0|function get unicode() { [native code] }",
    ));
    tsv.push_str(&line(
        "globalThis.R=(()=>{var B=Object.getOwnPropertyDescriptor(Function.prototype,\"caller\").set.bind(null,1);return B.name+\"|\"+B.length+\"|\"+String(B)})()",
        "bound caller|0|function caller() { [native code] }",
    ));
    let total = tsv.lines().count();
    common::run_golden(&tsv, total, |source| common::EvalMode::IndirectEval.evaluate(source, "native_getter_case.js", "R"));
}
