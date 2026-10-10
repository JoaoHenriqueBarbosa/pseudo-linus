//! `util.MIMEType` e `util.MIMEParams` contra o bun 1.4.2: `tests/golden/node_util_mime_main.js` roda como `/app/main.js`
//! e a saída tem de ser a que o bun do oráculo deu (`node_util_mime_main.expected.txt`, gerada com `bun /app/main.js` na
//! imagem do oráculo).
mod main_golden;

/// Passa com o `util` registrado no `BUILTINS` (conferido). O registro entra na fatia que fecha o `util`; ao registrar,
/// tira-se o `ignore`.
#[test]
#[ignore = "o util só entra no BUILTINS na fatia do registro"]
fn node_util_mime_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_mime_main.js"), include_str!("golden/node_util_mime_main.expected.txt"));
}
