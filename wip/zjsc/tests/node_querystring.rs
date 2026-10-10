//! `require("querystring")` contra o bun 1.4.2: `tests/golden/node_querystring_main.js` roda como `/app/main.js` e a
//! saída tem de ser a que o bun do oráculo deu (`node_querystring_main.expected.txt`, gerada com `bun /app/main.js` na
//! imagem do oráculo): superfície, parse com separadores, `maxKeys` e decodificador próprio, stringify, escape,
//! unescape, `unescapeBuffer` e o `ERR_INVALID_URI` do surrogate solto.
mod main_golden;

#[test]
fn node_querystring_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_querystring_main.js"), include_str!("golden/node_querystring_main.expected.txt"));
}
