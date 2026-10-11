//! Os membros pequenos e puros do global `Bun` contra o bun 1.4.2: `tests/golden/bun_small_main.js` roda como
//! `/app/main.js` e a saída tem de ser a que o bun do oráculo deu (`bun_small_main.expected.txt`, gerada com
//! `bun /app/main.js` na imagem do oráculo): descritores, nomes, aridades e `toString` de `version`, `revision`,
//! `argv`, `env`, `main`, `sleep`, `sleepSync`, `inspect` (com `custom` e `table`), `nanoseconds`, `escapeHTML`, `peek`
//! (com `status`), `pathToFileURL` e `fileURLToPath`, e o comportamento de cada um, erros incluídos.
mod main_golden;

#[test]
fn bun_small_members_match_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/bun_small_main.js"), include_str!("golden/bun_small_main.expected.txt"));
}
