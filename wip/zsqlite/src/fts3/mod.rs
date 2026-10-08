//! FTS3/FTS4 (`ext/fts3` do SQLite 3.46.1) em Rust seguro, modelo v2 (ver `CONVENTIONS.md`).
//!
//! # Plano de módulos (uma fatia por grupo, em ordem)
//!
//! | Módulo | Origem | Fatia |
//! |---|---|---|
//! | `int` | `fts3Int.h`, `fts3.h`, `fts3_tokenizer.h`: constantes, `Fts3Table`, `Fts3Cursor`, `Fts3Expr`, traits de tokenizador | 1 |
//! | `hash` | `fts3_hash.c`, `fts3_hash.h`: `Fts3Hash<T>` | 1 |
//! | `varint` | a parte de varints de `fts3.c`: `PutVarint`, `GetVarint`, `GetVarint32`, `VarintLen` | 1 |
//! | `tokenizer` | `fts3_tokenizer.c`: registro, função `fts3_tokenizer()`, `NextToken`, `InitTokenizer` | 1 |
//! | `tokenizer1` | `fts3_tokenizer1.c`: o tokenizador `simple` | 1 |
//! | `porter` | `fts3_porter.c`: o tokenizador `porter` | 1 |
//! | `unicode` | `fts3_unicode.c`: o tokenizador `unicode61` | 1 |
//! | `unicode2` | `fts3_unicode2.c`: `IsAlnum` (a dobra e os diacríticos são os do FTS5) | 1 |
//! | `expr` | `fts3_expr.c`: o analisador da consulta MATCH | 1 |
//! | `tokenize_vtab` | `fts3_tokenize_vtab.c`: o módulo `fts3tokenize` | 1 |
//! | `aux` | `fts3_aux.c`: o módulo `fts4aux` | 1 |
//! | `write` | `fts3_write.c` (parte 1): termos pendentes, leitores de segmentos, escrita, fusão, `optimize`, `rebuild` | 2 |
//! | `write2` | `fts3_write.c` (parte 2): fusão incremental, integridade, comandos especiais, tokens adiados, `xUpdate`; mais as rotinas de `fts3.c` de que a escrita depende (`DoclistPrev`, `FirstFilter`, `SegReaderCursor`, `CreateStatTable`, `fts3DbExec`, helpers de lista de posições) | 2 |
//! | `main` | `fts3.c` (parte 1): módulo virtual `fts3`/`fts4`, criação e conexão, `xBestIndex`, cursor, as funções `snippet()`/`offsets()`/`matchinfo()`/`optimize()` ligadas por `xFindFunction`, `fts3_init` | 3 |
//! | `main2` | `fts3.c` (parte 2): mesclagens de listas de posições e de doclists, seleção de termos, `xFilter`/`xNext`/`xColumn` | 3 |
//! | `main3` | `fts3.c` (parte 3): a avaliação da consulta (`fts3Eval*`, NEAR, tokens adiados, estatísticas e listas de posições das frases) | 3 |
//! | `snippet` | `fts3_snippet.c`: `snippet()`, `offsets()`, `matchinfo()` | 3 |
//!
//! Fora do build do Debian (e portanto sem tradução): `fts3_icu.c` (`SQLITE_ENABLE_ICU` desligado),
//! `fts3_test.c` e o código `#ifdef SQLITE_TEST` (`sqlite3Fts3ExprInitTestInterface`,
//! `fts3_exprtest`, `fts3_tokenizer_test`) e `fts3_term.c`, que não faz parte do amalgamation. As
//! opções do oráculo que valem aqui: `ENABLE_FTS3`, `ENABLE_FTS4`, `ENABLE_FTS3_PARENTHESIS`
//! (a sintaxe nova de consulta, ver `expr.rs`) e `ENABLE_FTS3_TOKENIZER` (a função
//! `fts3_tokenizer()` de dois argumentos vem ligada). Sem `SQLITE_DEBUG`, `SQLITE_TEST` e
//! `SQLITE_COVERAGE_TEST`.
//!
//! # Decisões do modelo v2 que todas as fatias seguem
//!
//! **Nomes.** `sqlite3Fts3XxxYyy` vira `fts3_xxx_yyy` no módulo do arquivo C de origem. Quando o
//! primeiro parâmetro do C é um dos structs do módulo, a função pode virar método (`Fts3Hash::find`,
//! `Fts3Table::n_column`). Os tipos mantêm o nome do C (`Fts3Table`, `Fts3Expr`). `xDestroy`,
//! `xClose` e os `sqlite3_free` de estruturas são o `Drop`.
//!
//! **Posse.** Sem `Rc<RefCell>`/ponteiro de volta. A exceção, igual à do C, é o registro de
//! tokenizadores (`Fts3HashWrapper`, em `tokenizer.rs`), compartilhado entre os módulos `fts3`,
//! `fts4`, `fts3tokenize` e a função `fts3_tokenizer()` e alterado em tempo de execução por ela. A
//! `Fts3Table` não guarda o `sqlite3 *db`: toda função que precisa dele recebe `&mut Connection` na
//! frente. A `Fts3Table` guarda o tokenizador como `Rc<dyn Fts3Tokenizer>` (imutável depois de
//! criado), de modo que o analisador de expressões o usa sem emprestar a tabela.
//!
//! **Árvore de expressão por arena.** Os nós de `Fts3Expr` ficam em `Fts3ExprTree` e os ponteiros
//! `pParent`/`pLeft`/`pRight` são `ExprId` (a avaliação sobe pelos pais). O dono é o `Fts3Cursor`; quem
//! precisa do cursor e da árvore ao mesmo tempo retira a árvore com `Option::take` e a devolve.
//! Os campos de avaliação da árvore (doclists, leitores de segmentos, tokens adiados) são de
//! `int.rs` e as fatias seguintes podem refiná-los.
//!
//! **Ponteiros para dentro de buffers** (`pList`, `pNextDocid`, `pOrPoslist`, `pNextId`) são
//! deslocamentos no buffer dono. Os varints do FTS3 (formato diferente do SQLite, até 10 bytes)
//! ficam em `varint.rs`; ler além do fim da fatia dá zero (o C usa `FTS3_BUFFER_PADDING`).
//!
//! **Sem falta de memória.** Alocação falha abortando, então as funções que só falhavam com
//! `SQLITE_NOMEM` perdem o código de erro (`Fts3Hash::insert`, `Fts3ExprTree::alloc`).
//!
//! **Texto C.** "String C" é `&[u8]`/`Vec<u8>` (nunca `String`) e o comprimento é o da fatia;
//! `char **pzErr` é `&mut Option<Vec<u8>>`; `sqlite3Fts3ErrMsg` é `*pz_err = mprintf(...)`.
//! Formatação é `printf::mprintf` com `PrintfArg`.
//!
//! **Tokenizador por traits.** `Fts3TokenizerModule` (`sqlite3_tokenizer_module`: `xCreate`),
//! `Fts3Tokenizer` (a instância: `xOpen`) e `Fts3TokenizerCursor` (`xNext`, `xLanguageid`); o
//! token devolvido é emprestado do cursor. Os três tokenizadores embutidos têm um
//! `fts3_xxx_tokenizer_module()` que o `fts3_init` registra no `Fts3HashWrapper`.
//!
//! **Módulos virtuais.** Cada módulo é um `VtabModule`, a tabela um `Vtab` e o cursor um
//! `VtabCursor` (`aux.rs` e `tokenize_vtab.rs` têm o padrão). Módulos cujo `xCreate` e `xConnect`
//! são a mesma função respondem `create_is_connect() == true` (têm tabela epônima).
//!
//! **Util do crate reaproveitado.** `util::dequote`, `util::strnicmp`, `utf::utf8_read` e
//! `write_utf8`, `printf::mprintf`, as tabelas de dobra de caixa e diacríticos do
//! `fts5::unicode2` (idênticas às do FTS3, conferidas byte a byte).
//!
//! # Contrato das fatias seguintes (o que esta fatia já chama)
//!
//! De `write.rs`: os tipos `Fts3SegReader`, `Fts3DeferredToken` e `PendingList`, e as funções
//! `fts3_seg_reader_cursor`, `fts3_seg_reader_start`, `fts3_seg_reader_step`,
//! `fts3_seg_reader_finish` e `fts3_segments_close` (assinaturas no cabeçalho de `aux.rs`). De
//! `main.rs`: `eval_phrase_cleanup(db, &mut Fts3Phrase)`. De `snippet.rs`: o tipo
//! `MatchinfoBuffer`.
//!
//! O módulo ainda não está registrado em `lib.rs` (`pub mod fts3;`): isso fecha ao fim das
//! fatias, quando o crate compila com tudo. `crate::fts3::fts3_init` é o ponto de entrada que a
//! conexão chama.

pub mod aux;
pub mod expr;
pub mod hash;
pub mod int;
pub mod main;
pub mod main2;
pub mod main3;
pub mod porter;
pub mod snippet;
pub mod tokenize_vtab;
pub mod tokenizer;
pub mod tokenizer1;
pub mod unicode;
pub mod unicode2;
pub mod varint;
pub mod write;
pub mod write2;

pub use aux::fts3_init_aux;
pub use expr::{fts3_expr_free, fts3_expr_parse, fts3_open_tokenizer};
pub use hash::{Fts3Hash, HashElemId, FTS3_HASH_BINARY, FTS3_HASH_STRING};
pub use main::fts3_init;
pub use int::{
    fts3_dequote, fts3_read_int, ExprId, Fts3Cursor, Fts3Doclist, Fts3Expr, Fts3ExprTree,
    Fts3Index, Fts3MultiSegReader, Fts3Phrase, Fts3PhraseToken, Fts3SegFilter, Fts3Table,
    Fts3Token, Fts3Tokenizer, Fts3TokenizerCursor, Fts3TokenizerModule,
};
pub use porter::fts3_porter_tokenizer_module;
pub use tokenize_vtab::fts3_init_tok;
pub use tokenizer::{
    fts3_init_hash_table, fts3_init_tokenizer, fts3_is_id_char, fts3_next_token, Fts3HashWrapper,
};
pub use tokenizer1::fts3_simple_tokenizer_module;
pub use unicode::fts3_unicode_tokenizer_module;
pub use unicode2::{fts_unicode_fold, fts_unicode_isalnum, fts_unicode_isdiacritic};
pub use varint::{
    fts3_get_varint, fts3_get_varint32, fts3_get_varint_bounded, fts3_get_varint_u,
    fts3_put_varint, fts3_varint_len,
};
pub use write::{
    fts3_all_segdirs, fts3_cache_deferred_doclists, fts3_create_stat_table, fts3_db_exec,
    fts3_defer_token, fts3_deferred_token_list, fts3_doclist_prev, fts3_first_filter,
    fts3_free_deferred_doclists, fts3_free_deferred_tokens, fts3_incrmerge, fts3_integrity_check,
    fts3_max_level, fts3_msr_incr_next, fts3_msr_incr_restart, fts3_msr_incr_start, fts3_msr_ovfl,
    fts3_optimize, fts3_pending_terms_clear, fts3_pending_terms_flush, fts3_read_block,
    fts3_select_docsize, fts3_select_doctotal, fts3_seg_reader_cursor, fts3_seg_reader_finish,
    fts3_seg_reader_free, fts3_seg_reader_new, fts3_seg_reader_pending, fts3_seg_reader_start,
    fts3_seg_reader_step, fts3_segments_close, fts3_update_method, Fts3DeferredToken,
    Fts3SegReader, PendingList,
};
