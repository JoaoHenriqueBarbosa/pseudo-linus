//! `Bun.hash` contra o bun 1.4.2: `tests/golden/bun_hash_main.js` roda como `/app/main.js` e a saída tem de ser a que o
//! bun do oráculo deu (`bun_hash_main.expected.txt`, gerada com `bun /app/main.js` na imagem do oráculo): a forma, os
//! doze algoritmos em tamanhos de 0 a 300 bytes com e sem semente, as conversões da semente e da entrada, e 100 mil
//! bytes de uma vez.
mod main_golden;

#[test]
fn bun_hash_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/bun_hash_main.js"), include_str!("golden/bun_hash_main.expected.txt"));
}
