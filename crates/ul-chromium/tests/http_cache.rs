//! As entradas do cache de disco contra as que o Chromium 154 do oráculo gravou ao navegar `ss-none.html` servida pelo
//! `python3 -m http.server` (`measure-navigation.sh`): o corpo, os cabeçalhos e os tempos da resposta reproduzem os bytes
//! do arquivo (cabeçalho, chave, fluxos, CRC-32, SHA-256 da chave, registros de fim) e o nome do arquivo sai da chave.

use ul_chromium::http_cache::{entry_file, file_name, key, Stored};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}.cache", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn body_of(entry: &[u8], length: usize) -> Vec<u8> {
    let key_length = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
    entry[24 + key_length..24 + key_length + length].to_vec()
}

fn headers(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn the_page_entry_matches_the_oracle() {
    let oracle = fixture("page-entry");
    let key = key("http://127.0.0.1", "http://127.0.0.1:8000/ss-none.html");
    assert_eq!(file_name(&key), "2ed71a569c719abd_0");
    let body = body_of(&oracle, 87);
    let stored = Stored {
        key,
        status_line: "HTTP/1.0 200 OK".to_owned(),
        headers: headers(&[("Server", "SimpleHTTP/0.6 Python/3.13.5"), ("Date", "Sun, 11 Oct 2026 02:30:14 GMT"), ("Content-type", "text/html"), ("Content-Length", "87"), ("Last-Modified", "Sun, 11 Oct 2026 01:27:01 GMT")]),
        body: &body,
        request_time: 13_436_159_414_414_333,
        response_time: 13_436_159_414_416_987,
        remote_host: "127.0.0.1".to_owned(),
        remote_port: 8000,
    };
    assert_eq!(entry_file(&stored), oracle);
}

#[test]
fn the_favicon_entry_matches_the_oracle() {
    let oracle = fixture("favicon-entry");
    let key = key("http://127.0.0.1", "http://127.0.0.1:8000/favicon.ico");
    assert_eq!(file_name(&key), "cedba38e45ec0f4b_0");
    let body = body_of(&oracle, 335);
    let stored = Stored {
        key,
        status_line: "HTTP/1.0 404 File not found".to_owned(),
        headers: headers(&[("Server", "SimpleHTTP/0.6 Python/3.13.5"), ("Date", "Sun, 11 Oct 2026 02:30:14 GMT"), ("Content-Type", "text/html;charset=utf-8"), ("Content-Length", "335")]),
        body: &body,
        request_time: 13_436_159_414_438_277,
        response_time: 13_436_159_414_438_818,
        remote_host: "127.0.0.1".to_owned(),
        remote_port: 8000,
    };
    assert_eq!(entry_file(&stored), oracle);
}
