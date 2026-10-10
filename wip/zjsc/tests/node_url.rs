//! `require("url")` contra o bun 1.4.2: `tests/golden/node_url_main.js` roda como `/app/main.js` e a saída tem de ser a
//! que o bun do oráculo deu (`node_url_main.expected.txt`, gerada com `bun /app/main.js` na imagem do oráculo): o
//! `url.parse`, `format` e `resolve` legados, `domainToASCII`/`domainToUnicode` com IDNA, `pathToFileURL`,
//! `fileURLToPath` nos modos POSIX e Windows, `fileURLToPathBuffer`, `urlToHttpOptions` e os erros com código.
mod main_golden;

#[test]
fn node_url_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_url_main.js"), include_str!("golden/node_url_main.expected.txt"));
}
