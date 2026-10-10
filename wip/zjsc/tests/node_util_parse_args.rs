//! `util.parseArgs` contra o bun 1.4.2: `tests/golden/node_util_parse_args_main.js` roda como `/app/main.js` e a saída
//! tem de ser a que o bun do oráculo deu (`node_util_parse_args_main.expected.txt`, gerada com `bun /app/main.js` na
//! imagem do oráculo): valores, posicionais, `tokens`, `default`, `allowNegative`, cada erro com o `code` e a mensagem do
//! bun, a validação da configuração e a ordem de leitura das propriedades.
mod main_golden;

/// Passa com o `util` registrado no `BUILTINS` (conferido). O registro só entra com o módulo completo: um `util` sem
/// `MIMEType` e `isDeepStrictEqual` seria pior do que o módulo ausente. Ao registrar, tira-se o `ignore`.
#[test]
#[ignore = "o util só entra no BUILTINS com MIMEType, MIMEParams e isDeepStrictEqual"]
fn node_util_parse_args_match_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_parse_args_main.js"), include_str!("golden/node_util_parse_args_main.expected.txt"));
}
