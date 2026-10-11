//! `Bun.deepEquals` contra o bun 1.4.2: `tests/golden/bun_deep_equals_main.js` roda como `/app/main.js` e a saída tem
//! de ser a que o bun do oráculo deu (`bun_deep_equals_main.expected.txt`, gerada com `bun /app/main.js` na imagem do
//! oráculo): a forma da função e os três modos (comum, estrito e estrito sem identidade de protótipo) em primitivos,
//! arrays, objetos, coleções, typed arrays, buffers, erros, invólucros, funções, objetos do WebCore, ciclos e `Proxy`.
mod main_golden;

#[test]
fn bun_deep_equals_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/bun_deep_equals_main.js"), include_str!("golden/bun_deep_equals_main.expected.txt"));
}
