//! `require("timers")` e `require("timers/promises")` contra o bun 1.4.2: `tests/golden/node_timers_main.js` roda como
//! `/app/main.js` e a saída tem de ser a que o bun do oráculo deu (`node_timers_main.expected.txt`, gerada com
//! `bun /app/main.js` na imagem do oráculo): superfície, identidade com os globais, `active`/`enroll`/`unenroll`, a
//! ordem das promessas, o `scheduler`, o `setInterval` assíncrono, abort e validação.
mod main_golden;

#[test]
fn node_timers_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_timers_main.js"), include_str!("golden/node_timers_main.expected.txt"));
}
