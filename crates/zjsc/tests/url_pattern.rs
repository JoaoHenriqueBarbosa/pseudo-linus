//! `URLPattern` contra o bun 1.4.2: `tests/golden/url_pattern_main.js` (a superfície da classe e os 349 casos do WPT
//! `urlpatterntestdata.json`, gerado por `scripts/gen-url-pattern-golden.js`) roda como `/app/main.js` e o stdout tem de
//! ser o que o bun do oráculo deu (`url_pattern_main.expected.txt`, gerada com `bun /app/main.js` na imagem do oráculo).
mod main_golden;

#[test]
fn url_pattern_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/url_pattern_main.js"), include_str!("golden/url_pattern_main.expected.txt"));
}
