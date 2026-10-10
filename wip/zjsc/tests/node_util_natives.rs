//! Os nativos de `require("util")` contra o bun 1.4.2: `tests/golden/node_util_natives_main.js` roda como `/app/main.js`
//! e a saída tem de ser a que o bun do oráculo deu (`node_util_natives_main.expected.txt`, gerada com `bun /app/main.js`
//! na imagem do oráculo).
mod main_golden;

/// Passa com o `util` registrado no `BUILTINS` (conferido). O registro só entra com os que ainda faltam (`parseArgs`,
/// `MIMEType`, `MIMEParams` e `isDeepStrictEqual`): um `util` que lança `host.parseArgs is not a function` seria pior do
/// que o módulo ausente. Ao registrar, tira-se o `ignore`.
#[test]
#[ignore = "o util só entra no BUILTINS com parseArgs, MIMEType, MIMEParams e isDeepStrictEqual"]
fn node_util_natives_match_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_natives_main.js"), include_str!("golden/node_util_natives_main.expected.txt"));
}
