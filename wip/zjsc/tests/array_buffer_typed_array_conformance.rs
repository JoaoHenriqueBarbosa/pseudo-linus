//! `ArrayBuffer`, `SharedArrayBuffer`, `%TypedArray%`, `DataView` e `Atomics` ponta a ponta pelo `eval`
//! indireto: a propagação das exceções das conversões (`valueOf` que lança, o `RETURN_IF_EXCEPTION` de
//! `JSArrayBufferPrototype.cpp`, `JSArrayBufferConstructor.cpp`, `JSDataViewPrototype.cpp`) e os resultados
//! de `resize`, `transfer`, `fill` com `BigInt`, `with`, `toSorted`, `findLast`, `set` e `subarray` que
//! `JSGenericTypedArrayViewPrototypeFunctions.h` fixa.
use zjsc::api::eval::evaluate_indirect_eval;
use zjsc::wtf::text::conversion_mode::ConversionMode;

/// O valor de conclusão de `source`, que tem de ser string.
fn eval_string(source: &str) -> String {
    let value = match evaluate_indirect_eval(source) {
        Ok(value) => value,
        Err(_) => panic!("{source}: lançou"),
    };
    assert!(value.is_string(), "{source}: o valor não é string");
    String::from_utf8(value.as_js_string().value().utf8(ConversionMode::LenientConversion)).expect("UTF-8")
}

/// `source` tem de lançar um objeto com `message` igual a `expected` (o `valueOf` do teste lança `Error`).
fn assert_propagates(source: &str, expected: &str) {
    let wrapped = format!("(function () {{ try {{ {source}; return 'no throw'; }} catch (e) {{ return String(e.message); }} }})()");
    assert_eq!(eval_string(&wrapped), expected, "{source}");
}

const THROWER: &str = "{ valueOf() { throw new Error('boom'); } }";

#[test]
fn conversion_exceptions_propagate_from_array_buffer() {
    assert_propagates(&format!("new ArrayBuffer({THROWER})"), "boom");
    assert_propagates(&format!("new ArrayBuffer(4, {{ maxByteLength: {THROWER} }})"), "boom");
    assert_propagates(&format!("new ArrayBuffer(8).slice({THROWER})"), "boom");
    assert_propagates(&format!("new ArrayBuffer(8).slice(0, {THROWER})"), "boom");
    assert_propagates(&format!("new ArrayBuffer(4, {{ maxByteLength: 8 }}).resize({THROWER})"), "boom");
    assert_propagates(&format!("new SharedArrayBuffer(4, {{ maxByteLength: 8 }}).grow({THROWER})"), "boom");
    assert_propagates(&format!("new ArrayBuffer(8).transfer({THROWER})"), "boom");
}

#[test]
fn conversion_exceptions_propagate_from_data_view_and_views() {
    assert_propagates(&format!("new DataView(new ArrayBuffer(8)).setInt8(0, {THROWER})"), "boom");
    assert_propagates(&format!("new DataView(new ArrayBuffer(8)).setFloat64({THROWER}, 1)"), "boom");
    assert_propagates(&format!("new DataView(new ArrayBuffer(8), {THROWER})"), "boom");
    assert_propagates(&format!("new Int8Array(new ArrayBuffer(8), {THROWER})"), "boom");
    assert_propagates(&format!("new Int8Array(1).fill({THROWER})"), "boom");
}

#[test]
fn resizable_array_buffer_transfer_keeps_or_drops_resizability() {
    assert_eq!(
        eval_string("var b = new ArrayBuffer(4, { maxByteLength: 8 }); var c = b.transfer(6); [b.detached, c.byteLength, c.resizable, c.maxByteLength].join()"),
        "true,6,true,8"
    );
    assert_eq!(
        eval_string("var b = new ArrayBuffer(4, { maxByteLength: 8 }); var c = b.transferToFixedLength(); [b.detached, c.byteLength, c.resizable].join()"),
        "true,4,false"
    );
}

#[test]
fn length_tracking_view_follows_resize_and_growable_shared_grows() {
    assert_eq!(eval_string("var b = new ArrayBuffer(4, { maxByteLength: 8 }); var t = new Uint8Array(b); b.resize(8); String(t.length)"), "8");
    assert_eq!(
        eval_string("var s = new SharedArrayBuffer(2, { maxByteLength: 4 }); s.grow(4); [s.byteLength, s.growable, s.maxByteLength].join()"),
        "4,true,4"
    );
}

#[test]
fn big_int_typed_arrays_fill_with_big_int_only() {
    assert_eq!(eval_string("new BigInt64Array(3).fill(7n, 1).join()"), "0,7,7");
    assert_eq!(eval_string("try { new BigInt64Array(1).fill(1); 'no throw' } catch (e) { e.name }"), "TypeError");
    assert_eq!(eval_string("new BigUint64Array(1).fill(-1n).join()"), "18446744073709551615");
}

#[test]
fn typed_array_prototype_copying_methods() {
    assert_eq!(eval_string("new Uint8Array([1, 2, 3]).with(-1, 9).join()"), "1,2,9");
    assert_eq!(eval_string("new Float64Array([3, -0, 0, NaN, -1]).toSorted().join()"), "-1,0,0,3,NaN");
    assert_eq!(eval_string("String(new Int8Array([1, 2, 3, 4]).findLast(function (x) { return x % 2 === 1; }))"), "3");
    assert_eq!(eval_string("var a = new Uint8Array(4); a.set([1, 2], 2); a.join()"), "0,0,1,2");
    assert_eq!(eval_string("new Int16Array([1, 2, 3, 4]).subarray(1, -1).join()"), "2,3");
}
