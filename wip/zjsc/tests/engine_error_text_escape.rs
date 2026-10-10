//! O texto das mensagens de erro do motor com identificadores fora do ASCII contra o bun 1.4.2: o `(In '...')` e o
//! `(evaluating '...')` citam o fonte da expressão, e o JSC os cita crus. O escape (`\u{65e5}`, `日`, `\xE9`) que
//! se vê num arquivo `.js` do bun é do impressor do transpilador (`wip/zbun`), que reimprime o arquivo antes de ele
//! chegar ao motor; por isso a sonda vai por `eval`, que o transpilador não toca.
//! `tests/golden/engine_error_text_escape_main.js` roda como `/app/main.js` e a saída tem de ser a do oráculo.
mod main_golden;

#[test]
fn engine_error_text_escapes_non_ascii_like_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/engine_error_text_escape_main.js"), include_str!("golden/engine_error_text_escape_main.expected.txt"));
}
