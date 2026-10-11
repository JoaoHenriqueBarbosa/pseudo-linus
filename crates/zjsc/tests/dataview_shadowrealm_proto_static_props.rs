//! `dataViewTable` (22 funções e os `CustomAccessor` `buffer` e `byteOffset`) e `shadowRealmPrototypeTable`
//! (`evaluate`, `importValue`) são reificadas no primeiro acesso. Ordens medidas no bun 1.4.2: antes e depois de
//! acessar a lista é a mesma (nomes da tabela na frente de `byteLength`, `constructor` e do `@@toStringTag`); `delete`
//! de um nome da tabela reifica tudo e a ordem passa a ser a da `Structure`: o que não é da tabela, os já acessados e
//! o resto da tabela. Ler um `CustomAccessor` por descritor não o reifica.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const KEYS: &str = "var k = function (o) { return Reflect.ownKeys(o).map(String).join(','); };";
const TAG: &str = "Symbol(Symbol.toStringTag)";
const DATA_VIEW_TABLE: &str = "getInt8,getUint8,getInt16,getUint16,getInt32,getUint32,getFloat16,getFloat32,getFloat64,getBigInt64,\
getBigUint64,setInt8,setUint8,setInt16,setUint16,setInt32,setUint32,setFloat16,setFloat32,setFloat64,setBigInt64,setBigUint64,\
buffer,byteOffset";
const DATA_VIEW_TABLE_WITHOUT_GET_INT8: &str = "getUint8,getInt16,getUint16,getInt32,getUint32,getFloat16,getFloat32,getFloat64,\
getBigInt64,getBigUint64,setInt8,setUint8,setInt16,setUint16,setInt32,setUint32,setFloat16,setFloat32,setFloat64,setBigInt64,\
setBigUint64,buffer,byteOffset";
const DATA_VIEW_REST_AFTER_ACCESS: &str = "getInt8,getUint8,getInt16,getUint16,getInt32,getUint32,getFloat16,getFloat32,getBigInt64,\
getBigUint64,setUint8,setInt16,setUint16,setInt32,setUint32,setFloat16,setFloat32,setFloat64,setBigInt64,setBigUint64,byteOffset";
const SHADOW_REALM: &str = "evaluate,importValue,constructor,Symbol(Symbol.toStringTag)";

fn data_view_all() -> String {
    format!("{DATA_VIEW_TABLE},byteLength,constructor,{TAG}")
}

#[test]
fn own_keys_before_access() {
    assert_eq!(
        run(&format!("{KEYS} k(DataView.prototype) + '|' + k(ShadowRealm.prototype)")),
        format!("{}|{SHADOW_REALM}", data_view_all())
    );
}

#[test]
fn own_keys_after_access() {
    let program = format!(
        "{KEYS} var a = DataView.prototype.getInt8, b = DataView.prototype.setBigUint64, \
         c = ShadowRealm.prototype.importValue, d = ShadowRealm.prototype.evaluate; \
         k(DataView.prototype) + '|' + k(ShadowRealm.prototype)"
    );
    assert_eq!(run(&program), format!("{}|{SHADOW_REALM}", data_view_all()));
}

#[test]
fn data_view_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete DataView.prototype.getInt8; r + '|' + k(DataView.prototype)");
    assert_eq!(run(&program), format!("true|byteLength,constructor,{DATA_VIEW_TABLE_WITHOUT_GET_INT8},{TAG}"));
}

#[test]
fn data_view_delete_outside_table_does_not_reify() {
    let program = format!(
        "{KEYS} var a = delete DataView.prototype.byteLength; var ka = k(DataView.prototype); \
         var b = delete DataView.prototype.constructor; a + '|' + ka + '|' + b + '|' + k(DataView.prototype)"
    );
    assert_eq!(
        run(&program),
        format!("true|{DATA_VIEW_TABLE},constructor,{TAG}|true|{DATA_VIEW_TABLE},{TAG}")
    );
}

#[test]
fn data_view_accessed_then_delete_custom_accessor() {
    let program = format!(
        "{KEYS} var a = DataView.prototype.setInt8, b = DataView.prototype.getFloat64; \
         var r = delete DataView.prototype.buffer; r + '|' + k(DataView.prototype)"
    );
    assert_eq!(run(&program), format!("true|byteLength,constructor,setInt8,getFloat64,{DATA_VIEW_REST_AFTER_ACCESS},{TAG}"));
}

#[test]
fn shadow_realm_delete_in_table_reifies_all() {
    let program = format!("{KEYS} var r = delete ShadowRealm.prototype.evaluate; r + '|' + k(ShadowRealm.prototype)");
    assert_eq!(run(&program), format!("true|constructor,importValue,{TAG}"));
}

#[test]
fn shadow_realm_accessed_then_delete() {
    let program = format!(
        "{KEYS} var a = ShadowRealm.prototype.importValue; var r = delete ShadowRealm.prototype.evaluate; \
         r + '|' + k(ShadowRealm.prototype)"
    );
    assert_eq!(run(&program), format!("true|constructor,importValue,{TAG}"));
}

#[test]
fn shadow_realm_delete_constructor_does_not_reify() {
    let program = format!("{KEYS} var r = delete ShadowRealm.prototype.constructor; r + '|' + k(ShadowRealm.prototype)");
    assert_eq!(run(&program), format!("true|evaluate,importValue,{TAG}"));
}

#[test]
fn descriptors_match_table_attributes() {
    let program = "var P = DataView.prototype, S = ShadowRealm.prototype; \
                   var f = Object.getOwnPropertyDescriptor(P, 'getInt8'); \
                   var g = Object.getOwnPropertyDescriptor(P, 'setBigUint64'); \
                   var b = Object.getOwnPropertyDescriptor(P, 'buffer'); \
                   var o = Object.getOwnPropertyDescriptor(P, 'byteOffset'); \
                   var l = Object.getOwnPropertyDescriptor(P, 'byteLength'); \
                   var e = Object.getOwnPropertyDescriptor(S, 'evaluate'); \
                   var i = Object.getOwnPropertyDescriptor(S, 'importValue'); \
                   [typeof f.value, f.value.name, f.value.length, f.writable, f.enumerable, f.configurable, \
                    g.value.name, g.value.length, \
                    typeof b.get, b.get.name, b.get.length, b.set, b.enumerable, b.configurable, \
                    o.get.name, o.get.length, o.set, o.enumerable, o.configurable, \
                    l.get.name, l.get.length, l.set, l.enumerable, l.configurable, \
                    typeof e.value, e.value.name, e.value.length, e.writable, e.enumerable, e.configurable, \
                    i.value.name, i.value.length, i.writable, i.enumerable, i.configurable].join()";
    assert_eq!(
        run(program),
        "function,getInt8,1,true,false,true,setBigUint64,2,function,get buffer,0,,false,true,get byteOffset,0,,false,true,\
get byteLength,0,,false,true,function,evaluate,1,true,false,true,importValue,2,true,false,true"
    );
}

#[test]
fn custom_accessors_work_and_type_check_this() {
    let program = "var v = new DataView(new ArrayBuffer(8), 2); \
                   var m = function (name) { try { Object.getOwnPropertyDescriptor(DataView.prototype, name).get.call({}); return 'no'; } \
                   catch (e) { return e.message; } }; \
                   [v.byteOffset, v.byteLength, v.buffer.byteLength, m('buffer'), m('byteOffset'), m('byteLength')].join('|')";
    assert_eq!(
        run(program),
        "2|6|8|DataView.prototype.buffer expects |this| to be a DataView object|\
DataView.prototype.byteOffset expects |this| to be a DataView object|\
DataView.prototype.byteLength expects |this| to be a DataView object"
    );
}

#[test]
fn strict_assignment_to_read_only_custom_accessor_throws() {
    let program = "'use strict'; var r; try { DataView.prototype.buffer = 1; r = 'no'; } catch (e) { r = e.constructor.name; } r";
    assert_eq!(run(program), "TypeError");
}
