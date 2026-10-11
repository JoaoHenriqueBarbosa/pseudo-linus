//! Construtores que no bun são `JSFunction` sobre `NativeExecutable` (`length` e `name` preguiçosos, primeiro
//! no `Reflect.ownKeys`), medidos no bun 1.4.2: `ArrayBuffer`, `DataView`, `Error` e os nativos.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

fn own_keys(name: &str) -> String {
    run(&format!("Reflect.ownKeys({name}).map(String).join(',')"))
}

#[test]
fn array_buffer_own_keys() {
    assert_eq!(own_keys("ArrayBuffer"), "length,name,prototype,isView,Symbol(Symbol.species)");
}

#[test]
fn data_view_own_keys() {
    assert_eq!(own_keys("DataView"), "length,name,prototype,BYTES_PER_ELEMENT");
}

#[test]
fn array_buffer_own_keys_after_access() {
    let program = "var a = ArrayBuffer.length + ArrayBuffer.name + typeof ArrayBuffer.isView; \
                   a + '|' + Reflect.ownKeys(ArrayBuffer).map(String).join(',')";
    assert_eq!(run(program), "1ArrayBufferfunction|length,name,prototype,isView,Symbol(Symbol.species)");
}

#[test]
fn array_buffer_after_delete_is_view() {
    let program = "var r = delete ArrayBuffer.isView; r + '|' + Reflect.ownKeys(ArrayBuffer).map(String).join(',')";
    assert_eq!(run(program), "true|length,name,prototype,Symbol(Symbol.species)");
}

#[test]
fn length_and_name_descriptors() {
    let program = "['ArrayBuffer', 'DataView'].map(function (n) { \
                     var c = globalThis[n]; \
                     var l = Object.getOwnPropertyDescriptor(c, 'length'); \
                     var m = Object.getOwnPropertyDescriptor(c, 'name'); \
                     return [n, l.value, l.writable, l.enumerable, l.configurable, m.value, m.writable, m.enumerable, m.configurable].join(','); \
                   }).join('|')";
    assert_eq!(
        run(program),
        "ArrayBuffer,1,false,false,true,ArrayBuffer,false,false,true|DataView,1,false,false,true,DataView,false,false,true"
    );
}

#[test]
fn function_shape() {
    let program = "[ArrayBuffer, DataView].map(function (c) { \
                     return [typeof c, Object.getPrototypeOf(c) === Function.prototype, Function.prototype.toString.call(c)].join(','); \
                   }).join('|')";
    assert_eq!(
        run(program),
        "function,true,function ArrayBuffer() { [native code] }|function,true,function DataView() { [native code] }"
    );
}

#[test]
fn call_without_new_and_construct() {
    let program = "var out = []; \
                   try { ArrayBuffer(1); } catch (e) { out.push(e.message); } \
                   try { DataView(1); } catch (e) { out.push(e.message); } \
                   out.push(new ArrayBuffer(4).byteLength, new DataView(new ArrayBuffer(4)).byteLength); \
                   out.join('|')";
    assert_eq!(
        run(program),
        "calling ArrayBuffer constructor without new is invalid|calling DataView constructor without new is invalid|4|4"
    );
}

#[test]
fn bytes_per_element_and_species() {
    let program = "var d = Object.getOwnPropertyDescriptor(DataView, 'BYTES_PER_ELEMENT'); \
                   [d.value, d.writable, d.enumerable, d.configurable, ArrayBuffer[Symbol.species] === ArrayBuffer].join(',')";
    assert_eq!(run(program), "1,false,false,false,true");
}

// Medido no bun 1.4.2.

#[test]
fn typed_array_super_constructor_own_keys() {
    let program = "var TA = Object.getPrototypeOf(Int8Array); \
                   [TA.name, TA.length, Reflect.ownKeys(TA).map(String).join(',')].join('|')";
    assert_eq!(run(program), "TypedArray|0|length,name,prototype,of,from,Symbol(Symbol.species)");
}

#[test]
fn typed_array_concrete_own_keys() {
    for name in ["Int8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array"] {
        assert_eq!(own_keys(name), "length,name,prototype,BYTES_PER_ELEMENT", "{name}");
    }
    assert_eq!(own_keys("Uint8Array"), "length,name,prototype,BYTES_PER_ELEMENT,fromBase64,fromHex");
}

#[test]
fn typed_array_shape() {
    let program = "var TA = Object.getPrototypeOf(Int8Array); \
                   [Int8Array.length, Float64Array.name, Object.getPrototypeOf(Int8Array) === Object.getPrototypeOf(Float64Array), \
                    Object.getPrototypeOf(TA) === Function.prototype, Function.prototype.toString.call(Int8Array), \
                    new Int8Array(2).length, Int8Array.BYTES_PER_ELEMENT].join('|')";
    assert_eq!(run(program), "3|Float64Array|true|true|function Int8Array() { [native code] }|2|1");
}

#[test]
fn iterator_own_keys() {
    assert_eq!(own_keys("Iterator"), "length,name,prototype,from,concat,zip,zipKeyed");
}

#[test]
fn collection_constructors_own_keys() {
    assert_eq!(own_keys("Map"), "length,name,prototype,groupBy,Symbol(Symbol.species)");
    assert_eq!(own_keys("Set"), "length,name,prototype,Symbol(Symbol.species)");
    for name in ["WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry"] {
        assert_eq!(own_keys(name), "length,name,prototype", "{name}");
    }
}

#[test]
fn promise_and_error_own_keys() {
    assert_eq!(
        own_keys("Promise"),
        "length,name,resolve,reject,race,all,allSettled,any,withResolvers,prototype,try,Symbol(Symbol.species)"
    );
    for name in ["EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "AggregateError"] {
        assert_eq!(own_keys(name), "length,name,prototype", "{name}");
    }
}

#[test]
fn reg_exp_own_keys() {
    assert_eq!(
        own_keys("RegExp"),
        "input,$_,multiline,$*,lastMatch,$&,lastParen,$+,leftContext,$`,rightContext,$',$1,$2,$3,$4,$5,$6,$7,$8,$9,\
         length,name,prototype,escape,Symbol(Symbol.species)"
    );
}

#[test]
fn error_constructor_own_keys() {
    assert_eq!(
        own_keys("Error"),
        "length,name,prototype,stackTraceLimit,captureStackTrace,isError,appendStackTrace,prepareStackTrace"
    );
}

#[test]
fn error_constructors_are_js_functions() {
    let program = "['Error', 'TypeError', 'RangeError', 'AggregateError'].map(function (n) { \
                     var c = globalThis[n]; \
                     var l = Object.getOwnPropertyDescriptor(c, 'length'); \
                     return [n, c.length, l.writable, l.configurable, Function.prototype.toString.call(c), \
                             Object.getPrototypeOf(c) === (n === 'Error' ? Function.prototype : Error)].join(','); \
                   }).join('|')";
    assert_eq!(
        run(program),
        "Error,1,false,true,function Error() { [native code] },true|TypeError,1,false,true,function TypeError() { [native code] },true\
         |RangeError,1,false,true,function RangeError() { [native code] },true|AggregateError,2,false,true,function AggregateError() { [native code] },true"
    );
}

#[test]
fn error_stack_trace_limit_put_and_delete_still_work() {
    let program = "Error.stackTraceLimit = 0; var a = 'stack' in new Error('x'); \
                   Error.stackTraceLimit = 5; var b = 'stack' in new Error('x'); \
                   delete Error.stackTraceLimit; var c = 'stack' in new Error('x'); \
                   a + ',' + b + ',' + c";
    assert_eq!(run(program), "false,true,false");
}
