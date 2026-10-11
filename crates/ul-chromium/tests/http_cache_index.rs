//! O `the-real-index` do cache contra o do oráculo, com as cinco entradas da navegação de `ss-none.html` (a página, o
//! favicon e três do próprio Chromium): o cabeçalho, o CRC-32, a soma dos tamanhos e o campo de tamanho de cada entrada
//! pequena saem do mesmo cálculo.

use ul_chromium::http_cache::{index_file, index_size_field, key, key_id, IndexEntry};

#[test]
fn the_index_matches_the_oracle() {
    let oracle = std::fs::read(format!("{}/tests/fixtures/the-real-index.bin", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let id = |name: &str| u64::from_str_radix(name, 16).unwrap();
    let second = 13_436_159_414_000_000;
    let entry = |id: u64, size_field: u64| IndexEntry { id, last_used: second, size_field };
    let entries = [
        entry(key_id(&key("http://127.0.0.1", "http://127.0.0.1:8000/ss-none.html")), index_size_field(519, 2)),
        entry(id("6e1b34e0304e8638"), index_size_field(3580, 1)),
        entry(id("5761d302839d26b6"), index_size_field(4796, 1)),
        entry(key_id(&key("http://127.0.0.1", "http://127.0.0.1:8000/favicon.ico")), index_size_field(746, 1)),
        // O dicionário baixado do servidor do Google não é pequeno: o oráculo conta 1784 pedaços.
        entry(id("5f01ffa8bf6cc67e"), 1784 << 8),
    ];
    assert_eq!(index_file(&entries, 13_436_159_414_435_560), oracle);
}
