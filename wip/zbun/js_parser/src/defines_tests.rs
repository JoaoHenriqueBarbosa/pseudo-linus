//! O `Define` com o `store` próprio (opção (B) do README): os índices de `DefineData.value` valem no `Store` de
//! cada arquivo, que nasce como cópia do `store` do `Define`.

use bun_ast::NodeStore as _;
use bun_ast::{E, ExprData, NodeStore};

use crate::defines::{Define, DefineData};

#[test]
fn values_keep_their_indices_in_each_file_store() {
    let mut define = Define::default();
    let text = define.store.alloc_str(b"production");
    let value = DefineData::init_static_string(&mut define.store, E::EString::init(text));
    define.insert(b"process.env.NODE_ENV", value).unwrap();
    define.insert(b"IS_BROWSER", DefineData::init_boolean(true)).unwrap();

    let dots = define.dots_for(b"NODE_ENV").expect("o define com ponto");
    let parts: Vec<&[u8]> = dots[0].parts.iter().map(|part| &part[..]).collect();
    assert_eq!(parts, [&b"process"[..], b"env", b"NODE_ENV"]);
    let ExprData::EString(node) = dots[0].data.value else { panic!("o valor não é string") };

    // Dois arquivos, cada um com a sua cópia: o mesmo índice lê o mesmo texto, e gravar num não mexe no outro.
    let mut first = define.store.clone();
    let second = define.store.clone();
    assert_eq!(first[node].slice8(&first), b"production");
    first[node].prefer_template = true;
    assert!(!second[node].prefer_template);
    assert_eq!(second[node].slice8(&second), b"production");

    assert!(define.dots_for(b"NODE_ENVX").is_none());
    assert!(matches!(define.for_identifier(b"IS_BROWSER").map(|d| d.value), Some(ExprData::EBoolean(E::Boolean { value: true }))));
}

#[test]
fn pure_globals_fall_back_to_the_table() {
    let define = Define::default();
    assert!(matches!(define.for_identifier(b"NaN").map(|d| d.value), Some(ExprData::ENumber(_))));
    let undefined = define.for_identifier(b"undefined").expect("undefined consta na tabela");
    assert!(matches!(undefined.value, ExprData::EUndefined(_)));
    assert!(undefined.can_be_removed_if_unused() && !undefined.valueless());
    let map = define.for_identifier(b"Map").expect("Map consta na tabela");
    assert!(map.valueless() && map.can_be_removed_if_unused());
    assert!(define.for_identifier(b"notAGlobal").is_none());
}

#[test]
fn dot_defines_merge_flags_like_upstream() {
    let mut define = Define::default();
    let mut first = DefineData::init_boolean(true);
    first.flags.set_method_call_must_be_replaced_with_undefined(true);
    define.insert(b"a.b", first).unwrap();
    define.insert(b"a.b", DefineData::init_boolean(false)).unwrap();
    let merged = &define.dots_for(b"b").unwrap()[0].data;
    assert!(merged.method_call_must_be_replaced_with_undefined() && merged.valueless());
    assert!(matches!(merged.value, ExprData::EBoolean(E::Boolean { value: false })));
}
