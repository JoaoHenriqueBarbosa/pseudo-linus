//! `Bun.stringWidth` contra o bun 1.4.2: `tests/golden/bun_string_width_main.js` roda como `/app/main.js` e a saída tem
//! de ser a que o bun do oráculo deu (`bun_string_width_main.expected.txt`, gerada com `bun /app/main.js` na imagem do
//! oráculo): grafemas, larguras, ANSI, ambíguos, o modo `perCodePoint`, as opções (protótipo, getter, `Proxy`) e 400
//! textos pseudoaleatórios nos seis modos.
mod main_golden;

#[test]
fn bun_string_width_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/bun_string_width_main.js"), include_str!("golden/bun_string_width_main.expected.txt"));
}
