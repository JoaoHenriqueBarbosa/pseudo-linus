//! `Bun.stripANSI` contra o bun 1.4.2: `tests/golden/bun_strip_ansi_main.js` roda como `/app/main.js` e a saída tem de
//! ser a que o bun do oráculo deu (`bun_strip_ansi_main.expected.txt`, gerada com `bun /app/main.js` na imagem do
//! oráculo): cada tipo de sequência ANSI, os terminadores, o que aborta, o que não termina e o ST solto, em Latin-1 e
//! em UTF-16, e a conversão do argumento.
mod main_golden;

#[test]
fn bun_strip_ansi_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/bun_strip_ansi_main.js"), include_str!("golden/bun_strip_ansi_main.expected.txt"));
}
