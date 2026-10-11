//! `util.styleText` com `validateStream` lê `internal/streams/utils` por `host.requireInternal` (o registro interno do bun):
//! `tests/golden/node_util_stream_utils_main.js` roda como `/app/main.js` e a saída tem de ser a que o bun do oráculo deu
//! (`node_util_stream_utils_main.expected.txt`, gerada com `bun /app/main.js` na imagem do oráculo). O objeto exportado
//! de `stream` segue com as mesmas chaves, sem nada do mecanismo.
mod main_golden;

#[test]
fn node_util_stream_utils_match_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_stream_utils_main.js"), include_str!("golden/node_util_stream_utils_main.expected.txt"));
}
