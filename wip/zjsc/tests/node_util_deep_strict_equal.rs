//! `util.isDeepStrictEqual` contra o bun 1.4.2: `tests/golden/node_util_deep_strict_equal_main.js` roda como `/app/main.js`
//! e a saída tem de ser a que o bun do oráculo deu (`node_util_deep_strict_equal_main.expected.txt`, gerada com
//! `bun /app/main.js` na imagem do oráculo). A parte dos objetos comuns; os objetos especiais vêm no golden seguinte.
mod main_golden;

/// Passa com o `util` registrado no `BUILTINS` (conferido). O registro só entra com `MIMEType` e `MIMEParams`; ao
/// registrar, tira-se o `ignore`.
#[test]
#[ignore = "o util só entra no BUILTINS com MIMEType e MIMEParams"]
fn node_util_deep_strict_equal_matches_bun() {
    main_golden::assert_main_script_matches(
        include_str!("golden/node_util_deep_strict_equal_main.js"),
        include_str!("golden/node_util_deep_strict_equal_main.expected.txt"),
    );
}

/// A parte dos objetos especiais: `Set`, `Map`, buffers, `DataView`, datas, `RegExp`, typed arrays, invólucros, erros e
/// os que o Node nunca acha iguais (`WeakMap`, `WeakSet`, `Promise`).
#[test]
#[ignore = "o util só entra no BUILTINS com MIMEType e MIMEParams"]
fn node_util_deep_strict_equal_special_objects_match_bun() {
    main_golden::assert_main_script_matches(
        include_str!("golden/node_util_deep_strict_equal_special_main.js"),
        include_str!("golden/node_util_deep_strict_equal_special_main.expected.txt"),
    );
}
