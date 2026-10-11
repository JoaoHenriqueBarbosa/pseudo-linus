//! Getter e setter de accessor que não são `JSFunction` comum: `InternalFunction` (`Array`), função nativa
//! (`Math.max`), função bound e Proxy de função. O `GetterSetter` do C++ guarda qualquer `JSObject` chamável e
//! chama por `call`. Os esperados saem do bun 1.4.2 via `node:vm`.
mod common;

use zjsc::api::eval::evaluate_named_script_result;

const CASES: &[(&str, &str)] = &[
    ("var o={};Object.defineProperty(o,'x',{get:Array});R=String(typeof o.x)+Array.isArray(o.x)+o.x.length", "objecttrue0"),
    ("var o={};Object.defineProperty(o,'x',{get:Math.max});R=String(o.x)", "-Infinity"),
    ("var o={};Object.defineProperty(o,'x',{get:(function(){return this+1}).bind(5)});R=String(o.x)", "6"),
    ("var o={};Object.defineProperty(o,'x',{get:(function(){return typeof this}).bind(5)});R=String(o.x)", "object"),
    ("var o={};Object.defineProperty(o,'x',{get:new Proxy(function(){return 7},{})});R=String(o.x)", "7"),
    ("var o={};Object.defineProperty(o,'x',{set:Array});o.x=1;R=String(o.x)", "undefined"),
    ("var o={};Object.defineProperty(o,'x',{set:Math.max});o.x=1;R=String(o.x)", "undefined"),
    ("var s;var o={};Object.defineProperty(o,'x',{set:(function(v){s=(this+1)+':'+v}).bind(5)});o.x=3;R=String(s)", "6:3"),
    ("var s;var o={};Object.defineProperty(o,'x',{set:new Proxy(function(v){s=v},{})});o.x=9;R=String(s)", "9"),
    ("var o={};Object.defineProperty(o,'x',{get:Array});R=String(Object.getOwnPropertyDescriptor(o,'x').get===Array)", "true"),
    ("var o={};Object.defineProperty(o,'x',{set:Array});R=String(Object.getOwnPropertyDescriptor(o,'x').set===Array)", "true"),
    ("var f=function(){return 7};var p=new Proxy(f,{});var o={};Object.defineProperty(o,'x',{get:p});R=String(Object.getOwnPropertyDescriptor(o,'x').get===p)", "true"),
    ("var o=[];Object.defineProperty(o,'1',{get:Array});R=String(typeof o[1])", "object"),
    ("var o={};Object.defineProperties(o,{x:{get:Array,set:Array,configurable:true}});Object.defineProperty(o,'x',{enumerable:true});var d=Object.getOwnPropertyDescriptor(o,'x');R=String(d.get===Array&&d.set===Array)", "true"),
    // Nomes de getter/setter nativos medidos no bun: `putDirectNativeIntrinsicGetter`/`reifyStaticAccessor` põem `get X` no
    // executável; `CustomAccessor` (Symbol.description, Intl.Locale, DataView.buffer, WebAssembly.Memory.buffer...) guarda
    // o nome puro no executável e `get X` só na propriedade `name`.
    ("var f=Object.getOwnPropertyDescriptor(RegExp.prototype,\"global\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get global\",\"function get global() { [native code] }\",\"bound get global\",\"function get global() { [native code] }\",\"function get global() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,\"byteLength\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get byteLength\",\"function get byteLength() { [native code] }\",\"bound get byteLength\",\"function get byteLength() { [native code] }\",\"function get byteLength() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),\"length\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get length\",\"function get length() { [native code] }\",\"bound get length\",\"function get length() { [native code] }\",\"function get length() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Map.prototype,\"size\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get size\",\"function get size() { [native code] }\",\"bound get size\",\"function get size() { [native code] }\",\"function get size() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Symbol.prototype,\"description\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get description\",\"function description() { [native code] }\",\"bound description\",\"function description() { [native code] }\",\"function description() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Intl.Locale.prototype,\"baseName\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get baseName\",\"function baseName() { [native code] }\",\"bound baseName\",\"function baseName() { [native code] }\",\"function baseName() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Intl.Collator.prototype,\"compare\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get compare\",\"function compare() { [native code] }\",\"bound compare\",\"function compare() { [native code] }\",\"function compare() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(WebAssembly.Memory.prototype,\"buffer\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get buffer\",\"function buffer() { [native code] }\",\"bound buffer\",\"function buffer() { [native code] }\",\"function buffer() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(WebAssembly.Table.prototype,\"length\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get length\",\"function length() { [native code] }\",\"bound length\",\"function length() { [native code] }\",\"function length() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(WebAssembly.Exception.prototype,\"stack\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get stack\",\"function stack() { [native code] }\",\"bound stack\",\"function stack() { [native code] }\",\"function stack() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype,\"value\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get value\",\"function get value() { [native code] }\",\"bound get value\",\"function get value() { [native code] }\",\"function get value() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(WebAssembly.Global.prototype,\"value\").set;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"set value\",\"function set value() { [native code] }\",\"bound set value\",\"function set value() { [native code] }\",\"function set value() { [native code] }\",1]"),
    ("var f=Object.getOwnPropertyDescriptor(DataView.prototype,\"buffer\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get buffer\",\"function buffer() { [native code] }\",\"bound buffer\",\"function buffer() { [native code] }\",\"function buffer() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(DataView.prototype,\"byteOffset\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get byteOffset\",\"function byteOffset() { [native code] }\",\"bound byteOffset\",\"function byteOffset() { [native code] }\",\"function byteOffset() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(DataView.prototype,\"byteLength\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get byteLength\",\"function get byteLength() { [native code] }\",\"bound get byteLength\",\"function get byteLength() { [native code] }\",\"function get byteLength() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Object.prototype,\"__proto__\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get __proto__\",\"function get __proto__() { [native code] }\",\"bound get __proto__\",\"function get __proto__() { [native code] }\",\"function get __proto__() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Object.prototype,\"__proto__\").set;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"set __proto__\",\"function set __proto__() { [native code] }\",\"bound set __proto__\",\"function set __proto__() { [native code] }\",\"function set __proto__() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Iterator.prototype,\"constructor\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get constructor\",\"function constructor() { [native code] }\",\"bound constructor\",\"function constructor() { [native code] }\",\"function constructor() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Iterator.prototype,\"constructor\").set;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"set constructor\",\"function constructor() { [native code] }\",\"bound constructor\",\"function constructor() { [native code] }\",\"function constructor() { [native code] }\",1]"),
    ("var f=Object.getOwnPropertyDescriptor(Set.prototype,\"size\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get size\",\"function get size() { [native code] }\",\"bound get size\",\"function get size() { [native code] }\",\"function get size() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(RegExp,Symbol.species).get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get [Symbol.species]\",\"function get [Symbol.species]() { [native code] }\",\"bound get [Symbol.species]\",\"function get [Symbol.species]() { [native code] }\",\"function get [Symbol.species]() { [native code] }\",0]"),
    // Temporal e os outros getters de classe B (CustomAccessor), medidos no bun: `name` é `get X`, `String`/`bind` partem de `X`.
    ("var f=Object.getOwnPropertyDescriptor(Temporal.Instant.prototype,\"epochMilliseconds\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get epochMilliseconds\",\"function epochMilliseconds() { [native code] }\",\"bound epochMilliseconds\",\"function epochMilliseconds() { [native code] }\",\"function epochMilliseconds() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Temporal.PlainDate.prototype,\"year\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get year\",\"function year() { [native code] }\",\"bound year\",\"function year() { [native code] }\",\"function year() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Temporal.PlainTime.prototype,\"hour\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get hour\",\"function hour() { [native code] }\",\"bound hour\",\"function hour() { [native code] }\",\"function hour() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Temporal.Duration.prototype,\"years\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get years\",\"function years() { [native code] }\",\"bound years\",\"function years() { [native code] }\",\"function years() { [native code] }\",0]"),
    // Classe A (executável com `get X`) nos que passam por `create_host_getter_function`: DisposableStack, RegExp, TypedArray.
    ("var f=Object.getOwnPropertyDescriptor(DisposableStack.prototype,\"disposed\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get disposed\",\"function get disposed() { [native code] }\",\"bound get disposed\",\"function get disposed() { [native code] }\",\"function get disposed() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(AsyncDisposableStack.prototype,\"disposed\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get disposed\",\"function get disposed() { [native code] }\",\"bound get disposed\",\"function get disposed() { [native code] }\",\"function get disposed() { [native code] }\",0]"),
    ("var f=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),\"buffer\").get;var b=f.bind();R=JSON.stringify([f.name,String(f),b.name,String(b),Function.prototype.toString.call(f),f.length])", "[\"get buffer\",\"function get buffer() { [native code] }\",\"bound get buffer\",\"function get buffer() { [native code] }\",\"function get buffer() { [native code] }\",0]"),
];

#[test]
fn getter_setter_callable_matches_bun() {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(|| {
            let mut failures = Vec::new();
            for (source, expected) in CASES {
                let got = common::guarded(|| evaluate_named_script_result(source, "getter_setter_callable.js", "R"));
                if got.as_deref() != Ok(*expected) {
                    failures.push(format!("{source}\n  esperado {expected:?}\n  obtido   {got:?}"));
                }
            }
            assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
        })
        .unwrap()
        .join()
        .unwrap();
}
