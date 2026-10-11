//! `node:assert` e `assert/strict` contra o bun 1.4.2: `tests/golden/node_assert_main.js` roda como `/app/main.js` e o
//! stdout tem de ser o que o bun do oráculo deu (`node_assert_main.expected.txt`, gerada com `bun /app/main.js` na imagem
//! do oráculo; o aviso DEP0173 do `CallTracker` vai para o stderr e fica fora da comparação).
mod main_golden;

#[test]
fn node_assert_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_assert_main.js"), include_str!("golden/node_assert_main.expected.txt"));
}
