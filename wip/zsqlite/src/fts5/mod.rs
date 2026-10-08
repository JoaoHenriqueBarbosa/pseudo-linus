//! FTS5 (`ext/fts5` do SQLite 3.46.1) em Rust seguro, modelo v2 (ver `CONVENTIONS.md`).
//!
//! # Plano de módulos (uma fatia por grupo, em ordem)
//!
//! | Módulo | Origem | Fatia |
//! |---|---|---|
//! | `int` | `fts5Int.h`, `fts5.h`: constantes, `Fts5Config`, traits públicos | 1 |
//! | `buffer` | `fts5_buffer.c`: `Fts5Buffer`, poslists, `Fts5Termset`, `fts5_mprintf` | 1 |
//! | `varint` | `fts5_varint.c` (reexporta `util`) | 1 |
//! | `hash` | `fts5_hash.c`: `Fts5Hash` | 1 |
//! | `config` | `fts5_config.c`: `fts5_config_parse`, `Fts5Config::{load,set_value,...}` | 1 |
//! | `tokenize` | `fts5_tokenize.c`: ascii, unicode61, porter, trigram | 1 |
//! | `unicode2` | `fts5_unicode2.c`: dobra de caixa e categorias | 1 |
//! | `aux` | `fts5_aux.c`: `bm25`, `highlight`, `snippet` | 1 |
//! | `index` | `fts5_index.c`: `Fts5Index`, segmentos, iteradores | 2 |
//! | `expr` + `parse` | `fts5_expr.c` + `fts5parse.y` (lemon) | 3 |
//! | `storage` | `fts5_storage.c`: `%_content`, `%_docsize`, `%_config` | 4 |
//! | `main` | `fts5_main.c`: módulo virtual, `Fts5Global`, `Fts5Cursor`, `fts5_init` | 4 |
//! | `vocab` | `fts5_vocab.c`: módulo `fts5vocab` | 4 |
//!
//! `fts5_tcl.c`, `fts5_test_mi.c`, `fts5_test_tok.c` são de teste e não entram. Sem
//! `SQLITE_DEBUG` (o Debian não o liga): somem `bPrefixIndex` (e o comando `prefix-index`),
//! `sqlite3Fts5Corrupt()` (`FTS5_CORRUPT` é a constante `SQLITE_CORRUPT_VTAB`),
//! `sqlite3_fts5_may_be_corrupt` e `fts5HashCount`.
//!
//! # Decisões do modelo v2 que todas as fatias seguem
//!
//! **Nomes.** `sqlite3Fts5XxxYyy` vira `fts5_xxx_yyy` no módulo do arquivo C de origem. Quando
//! o primeiro parâmetro do C é um dos structs deste módulo, a função vira método com o resto do
//! nome (`sqlite3Fts5HashWrite` é `Fts5Hash::write`; `sqlite3Fts5BufferAppendBlob` é
//! `Fts5Buffer::append_blob`; `sqlite3Fts5ConfigLoad` é `Fts5Config::load`). Construtores são
//! `new`. O `Free`/`Close`/`Delete` do C é o `Drop`.
//!
//! **Posse.** Sem `Rc<RefCell>`/ponteiro de volta. A `Fts5Table` (fatia 5) possui, por valor,
//! `config: Fts5Config`, `index: Fts5Index` e `storage: Fts5Storage`. `Fts5Index` e `Fts5Storage`
//! NÃO guardam o `pConfig` do C: todo método deles recebe `&Fts5Config`/`&mut Fts5Config`
//! explicitamente. O `sqlite3 *db` também não é campo de ninguém: quem precisa dele recebe
//! `&mut Connection`. O `Fts5Hash` copia só o `eDetail`; o contador `*pnByte` (o `nPendingData`
//! do índice) é o campo `Fts5Hash::n_byte`, que o índice lê e zera no lugar de `nPendingData`.
//!
//! **Handles no lugar de ponteiros.** Listas encadeadas e árvores do C viram `Vec` com índices
//! (como o arena do `hash.rs`). Iteradores e estruturas do índice são tipos próprios com posse.
//! A árvore de expressão (`expr.rs`) é uma arena de nós e frases no `Fts5Expr` (`NodeId`,
//! `PhraseId`); o `Fts5Expr` NÃO guarda `pIndex` nem `pConfig`: `first`/`next` recebem
//! `db`, `idx` e `cfg`, e `Fts5Expr::free(db, idx)` fecha os iteradores de termo.
//!
//! **Sem `pRc` para falta de memória.** Alocação falha abortando em Rust, então as funções que
//! só falhavam com `SQLITE_NOMEM` perdem o `int *pRc` e a retornam o valor direto
//! (`Fts5Buffer::append_blob`, `Fts5Hash::new`). Onde o `*pRc` carrega um erro anterior que a
//! função deve preservar (`fts5_mprintf`, `Fts5Buffer::append_printf`, o `pRc` do índice), ele
//! continua como `&mut i32` ou campo `rc`.
//!
//! **Texto C.** "String C" é `&[u8]`/`Vec<u8>` (nunca `String`); o fim da fatia faz o papel do
//! NUL (`util::at`). `char **pzErr` é `&mut Option<Vec<u8>>`. Formatação é `printf::mprintf`
//! (`%Q`, `%q`, `%s`, `%d`) com `PrintfArg`.
//!
//! **Tokenizador por trait.** `Fts5Tokenizer` (a instância, imutável, `Rc<dyn Fts5Tokenizer>` em
//! `Fts5Config::p_tok`) e `Fts5TokenizerFactory` (o `xCreate`; o `pUserData` são os campos da
//! fábrica). O `xToken` do C é um fecho `&mut dyn FnMut(tflags, &[u8], i_start, i_end) -> i32`
//! (`Fts5TokenFn`), o que dispensa o `pCtx`. Quem precisa tokenizar enquanto muta a
//! configuração clona o `Rc` de `p_tok` antes (`config.p_tok.clone()`) e chama `tokenize` do
//! clone; `Fts5Config::tokenize` serve quando o fecho não toca a configuração.
//!
//! **`Fts5ExtensionApi` como trait.** O `Fts5Context*` do C vira o objeto que implementa
//! `Fts5ExtensionApi` (na fatia 4, o `CsrApi` de `main.rs`, que empresta a conexão, a tabela e o
//! cursor); as funções auxiliares recebem `&mut dyn Fts5ExtensionApi`. Textos devolvidos
//! (`xColumnText`, `xQueryToken`, `xInstToken`) são `Vec<u8>` próprios (o C devolvia ponteiro válido
//! até a linha mudar). `x_tokenize` devolve o objeto da API ao fecho (`Fts5ApiTokenFn`). O auxdata
//! é `Rc<dyn Any>` (o `xDelete` é o `Drop`). `Fts5PhraseIter` guarda uma cópia da poslist e dois
//! deslocamentos. A função auxiliar NÃO recebe o `sqlite3_context`: a API e o contexto precisariam
//! da mesma conexão ao mesmo tempo, então ela devolve um `Fts5AuxResult` (o que os
//! `sqlite3_result_*` gravariam) e quem a chamou o aplica ao contexto.
//!
//! **Registro.** `Fts5Api` (o `fts5_api`) e `Fts5GlobalApi` (o que o `config.rs` exige do
//! `Fts5Global`: `get_tokenizer`, o `sqlite3Fts5GetTokenizer`) são traits; o `Fts5Global` da
//! fatia 4 implementa os dois, guarda `Vec<(nome, Rc<dyn Fts5TokenizerFactory>)>` e
//! `Vec<(nome, Rc<dyn Fts5ExtensionFunction>)>`, e `fts5_init` chama `tokenize::fts5_tokenizer_init(&mut
//! global)` e `aux::fts5_aux_init(&mut global)` antes de registrar o módulo `fts5` e o `fts5vocab`.
//! `sqlite3Fts5TokenizerPattern` é o método `Fts5Tokenizer::pattern` (chame `p_tok.pattern()`
//! depois de criar o tokenizador).
//!
//! **Cursor na tabela (fatia 4).** O `Fts5Cursor` mora em `Fts5FullTable.cursors` e o cursor do
//! núcleo é só o id (a função SQL auxiliar acha o cursor pelo id percorrendo `db.vtabs`). O
//! `ORDER BY rank` não roda um comando aninhado (reentrância na mesma tabela virtual): veja o
//! cabeçalho de `main.rs`.
//!
//! **Util do crate reaproveitado.** `put_varint`/`get_varint`/`varint_len`
//! (reexportados em `varint`), `put4byte`/`get4byte` (no lugar de `sqlite3Fts5Put32`/`Get32`),
//! `utf::utf8_read`/`write_utf8`, `util::str_icmp`/`strnicmp`/`stricmp`, `printf::mprintf`.
//!
//! O módulo ainda não está registrado em `lib.rs` (`pub mod fts5;`): isso fecha ao fim das
//! fatias, quando o crate compila com tudo. `crate::fts5::fts5_init` é o ponto de entrada que o
//! `main.rs` do crate chama ao abrir uma conexão.

pub mod aux;
pub mod buffer;
pub mod config;
pub mod expr;
pub mod hash;
pub mod index;
pub mod index2;
pub mod index3;
pub mod int;
pub mod main;
pub mod parse;
pub mod storage;
pub mod tokenize;
pub mod unicode2;
pub mod varint;
pub mod vocab;

pub use buffer::{Fts5Buffer, Fts5PoslistReader, Fts5PoslistWriter, Fts5Termset};
pub use expr::{
    fts5_expr_and, fts5_expr_init, fts5_expr_new, fts5_expr_pattern, Fts5Expr, Fts5ExprNearset,
    Fts5ExprNode, Fts5ExprPhrase, Fts5ExprTerm, Fts5Parse, Fts5PoslistPopulator, NodeId, PhraseId,
};
pub use hash::Fts5Hash;
pub use index::{Fts5Index, Fts5IndexIter, Fts5Iter};
pub use int::{
    Fts5Api, Fts5ApiTokenFn, Fts5AuxResult, Fts5Colset, Fts5Config, Fts5ExtensionApi,
    Fts5ExtensionFunction, Fts5GlobalApi, Fts5PhraseIter, Fts5TokenFn, Fts5Tokenizer,
    Fts5TokenizerFactory,
};
pub use main::{fts5_init, Fts5ApiSlot, Fts5AuxRef, Fts5Cursor, Fts5FullTable, Fts5Global};
pub use storage::{fts5_create_table, fts5_drop_all, Fts5Storage};
pub use vocab::fts5_vocab_init;

