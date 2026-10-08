//! `fts3Int.h`, `fts3.h`, `fts3_tokenizer.h` e `fts3_hash.h` (este último em `hash.rs`): as
//! constantes, as estruturas (`Fts3Table`, `Fts3Cursor`, `Fts3Expr`, `Fts3Phrase`, ...) e a
//! interface de tokenizadores (`sqlite3_tokenizer_module`, `sqlite3_tokenizer` e
//! `sqlite3_tokenizer_cursor`) do FTS3/FTS4.
//!
//! `fts3.h` só declara `sqlite3Fts3Init`, que é o `fts3_init` da fatia 2 (`main.rs`).
//!
//! Sem `SQLITE_DEBUG`, `SQLITE_TEST` e `SQLITE_COVERAGE_TEST` (o Debian não os liga) somem
//! `sqlite3_fts3_may_be_corrupt`, `sqlite3Fts3Corrupt` (`FTS_CORRUPT_VTAB` é a constante
//! `SQLITE_CORRUPT_VTAB`), `inTransaction`, `mxSavepoint`, `bNoIncrDoclist` e `nMergeCount`
//! (`MergeCount(P)` é [`FTS3_MERGE_COUNT`]). `SQLITE_DISABLE_FTS4_DEFERRED` e
//! `SQLITE_DISABLE_FTS3_UNICODE` não estão definidos: os tokens adiados e o tokenizador
//! `unicode61` existem.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **Tokenizador por traits.** [`Fts3TokenizerModule`] é o `sqlite3_tokenizer_module` (o
//!   `xCreate`; `iVersion` é [`Fts3TokenizerModule::i_version`]), [`Fts3Tokenizer`] a instância
//!   (`sqlite3_tokenizer`, com o `xOpen`) e [`Fts3TokenizerCursor`] o cursor (`xNext`,
//!   `xLanguageid`). `xDestroy` e `xClose` são o `Drop`. A instância é imutável depois de criada
//!   (os tokenizadores embutidos não têm estado mutável), então a tabela a guarda como
//!   `Rc<dyn Fts3Tokenizer>` e o analisador de expressões pode usá-la sem emprestar a tabela. O
//!   cursor copia o texto de entrada (o C guarda o ponteiro e exige que o chamador o mantenha
//!   vivo), e o token que `next` devolve é emprestado do cursor até a chamada seguinte.
//!   `SQLITE_DONE` é `Err(SQLITE_DONE)`.
//! * **Sem ponteiros de volta.** A `Fts3Table` NÃO guarda o `sqlite3 *db`: toda função que o
//!   precisa recebe `&mut Connection`. O `base.zErrMsg` é [`Fts3Table::z_err_msg`].
//! * **Texto C.** "String C" é `Vec<u8>`/`&[u8]` (nunca `String`), o comprimento é o da fatia
//!   (por isso somem `nToken`, `nColumn`, `nTerm`, `nDoclist`, `nAll` e `nBuffer`: são o `len()`);
//!   `char **pzErr` é `&mut Option<Vec<u8>>`. A formatação é `printf::mprintf`.
//! * **Ponteiros para dentro de buffers** (`pNextDocid`, `pList`, `pOrPoslist`, `pNextId`) são
//!   deslocamentos no buffer dono, `Option<usize>` quando o C testa contra `NULL`.
//! * **Árvore de expressão por arena.** Um `Fts3Expr` guarda `pParent`, `pLeft` e `pRight`, e o
//!   avaliador sobe pela árvore (`pParent`), então os nós ficam numa arena ([`Fts3ExprTree`]) e os
//!   ponteiros são [`ExprId`]. O dono da árvore é a `Fts3Cursor`; quem precisa do cursor e da
//!   árvore ao mesmo tempo retira a árvore do cursor com `Option::take` e a devolve depois.
//! * **Tipos definidos em outro arquivo C** e que este cabeçalho só declara: `Fts3SegReader`,
//!   `Fts3DeferredToken` e `PendingList` (de `fts3_write.c`, módulo `write`) e `MatchinfoBuffer`
//!   (de `fts3_snippet.c`, módulo `snippet`). Entram aqui por caminho: `write.rs` e `snippet.rs`
//!   são das fatias seguintes.
//! * **`pSegments`** (o `sqlite3_blob *` aberto em `%_segments`) é [`Fts3Table::p_segments`], um
//!   comando preparado parado na linha (o crate não tem `vdbeblob`; ver `write.rs`).
//! * **Auxiliares de `fts3.c`** que os arquivos desta fatia precisam antes da fatia 2:
//!   [`fts3_dequote`] (`sqlite3Fts3Dequote`) e [`fts3_read_int`] (`sqlite3Fts3ReadInt`) estão aqui
//!   e os varints (`sqlite3Fts3PutVarint`, `GetVarint`, ...) em `varint.rs`. A fatia 2 não os
//!   redefine. `sqlite3Fts3ErrMsg` não existe como função: é `*pz_err = mprintf(...)`.

use std::ops::{Index, IndexMut};
use std::rc::Rc;

use crate::connection::StmtId;
use crate::consts::SQLITE_CORRUPT_VTAB;
use crate::util::{at, dequote, strlen30};

use super::hash::Fts3Hash;
use super::snippet::MatchinfoBuffer;
use super::write::{Fts3DeferredToken, Fts3SegReader, PendingList};

// ---------------------------------------------------------------------------------------------
// Constantes de fts3Int.h
// ---------------------------------------------------------------------------------------------

/// `SQLITE_FTS3_MAX_EXPR_DEPTH`: a profundidade máxima de uma árvore de expressão FTS.
pub const SQLITE_FTS3_MAX_EXPR_DEPTH: i32 = 12;

/// `FTS3_MERGE_COUNT`: depois de tantos segmentos de nível N, eles se fundem num de nível N+1.
pub const FTS3_MERGE_COUNT: i32 = 16;

/// `FTS3_MAX_PENDING_DATA`: o máximo de dados (em bytes) em `Fts3Table.pendingTerms`.
pub const FTS3_MAX_PENDING_DATA: i32 = 1024 * 1024;

/// `FTS3_VARINT_MAX`: o comprimento máximo de um varint (10, não 9 como no SQLite).
pub const FTS3_VARINT_MAX: usize = 10;

/// `FTS3_BUFFER_PADDING`.
pub const FTS3_BUFFER_PADDING: usize = 8;

/// `FTS3_SEGDIR_MAXLEVEL`: o índice a que uma árvore-b pertence é `level >> 10`.
pub const FTS3_SEGDIR_MAXLEVEL: i32 = 1024;

/// `FTS3_SEGDIR_MAXLEVEL_STR`.
pub const FTS3_SEGDIR_MAXLEVEL_STR: &[u8] = b"1024";

/// `POS_COLUMN`: terminador de lista de colunas.
pub const POS_COLUMN: u8 = 1;
/// `POS_END`: terminador de lista de posições.
pub const POS_END: u8 = 0;

/// `FTS_CORRUPT_VTAB` sem `SQLITE_DEBUG`.
pub const FTS_CORRUPT_VTAB: i32 = SQLITE_CORRUPT_VTAB;

/// O tamanho de `Fts3Table.aStmt`.
pub const FTS3_N_STMT: usize = 40;

/// `FTS3_EVAL_FILTER`, `FTS3_EVAL_NEXT`, `FTS3_EVAL_MATCHINFO`: `Fts3Cursor.eEvalmode`.
pub const FTS3_EVAL_FILTER: i32 = 0;
pub const FTS3_EVAL_NEXT: i32 = 1;
pub const FTS3_EVAL_MATCHINFO: i32 = 2;

/// `Fts3Cursor.eSearch`: varredura linear de `%_content`.
pub const FTS3_FULLSCAN_SEARCH: i32 = 0;
/// `Fts3Cursor.eSearch`: busca por rowid em `%_content`.
pub const FTS3_DOCID_SEARCH: i32 = 1;
/// `Fts3Cursor.eSearch`: busca no índice de texto completo. Valores maiores que este são
/// `FTS3_FULLTEXT_SEARCH + coluna`.
pub const FTS3_FULLTEXT_SEARCH: i32 = 2;

/// Bits do `idxNum` do `xBestIndex` (os 16 bits altos): `languageid=?`.
pub const FTS3_HAVE_LANGID: i32 = 0x0001_0000;
/// `docid>=?`.
pub const FTS3_HAVE_DOCID_GE: i32 = 0x0002_0000;
/// `docid<=?`.
pub const FTS3_HAVE_DOCID_LE: i32 = 0x0004_0000;

/// `Fts3Expr.eType`: os quatro primeiros valores estão na ordem de precedência do analisador.
pub const FTSQUERY_NEAR: i32 = 1;
pub const FTSQUERY_NOT: i32 = 2;
pub const FTSQUERY_AND: i32 = 3;
pub const FTSQUERY_OR: i32 = 4;
pub const FTSQUERY_PHRASE: i32 = 5;

/// `FTS3_SEGCURSOR_PENDING`: valor especial de `sqlite3Fts3SegReaderCursor`.
pub const FTS3_SEGCURSOR_PENDING: i32 = -1;
/// `FTS3_SEGCURSOR_ALL`.
pub const FTS3_SEGCURSOR_ALL: i32 = -2;

/// Flags de `Fts3SegFilter.flags` (o 4o argumento de `SegmentReaderIterate`).
pub const FTS3_SEGMENT_REQUIRE_POS: i32 = 0x0000_0001;
pub const FTS3_SEGMENT_IGNORE_EMPTY: i32 = 0x0000_0002;
pub const FTS3_SEGMENT_COLUMN_FILTER: i32 = 0x0000_0004;
pub const FTS3_SEGMENT_PREFIX: i32 = 0x0000_0008;
pub const FTS3_SEGMENT_SCAN: i32 = 0x0000_0010;
pub const FTS3_SEGMENT_FIRST: i32 = 0x0000_0020;

// ---------------------------------------------------------------------------------------------
// fts3_tokenizer.h
// ---------------------------------------------------------------------------------------------

/// O que `xNext` entrega: o texto normalizado do token (depois de dobra de caixa e radical) e a
/// posição dele na entrada. O texto é do cursor e vale até a chamada seguinte de `next`.
#[derive(Debug, Clone, Copy)]
pub struct Fts3Token<'a> {
    /// `*ppToken`/`*pnBytes`.
    pub z: &'a [u8],
    /// `*piStartOffset`: o índice do primeiro byte do token na entrada.
    pub i_start_offset: i32,
    /// `*piEndOffset`: o índice do primeiro byte depois do fim do token na entrada.
    pub i_end_offset: i32,
    /// `*piPosition`: quantos tokens vieram antes deste.
    pub i_position: i32,
}

/// `sqlite3_tokenizer_cursor`: tokeniza uma entrada específica.
pub trait Fts3TokenizerCursor {
    /// `xNext`: o token seguinte; `Err(SQLITE_DONE)` no fim do texto ou outro código de erro.
    fn next(&mut self) -> Result<Fts3Token<'_>, i32>;

    /// `xLanguageid` (só existe se `iVersion>=1`): configura o id de idioma do cursor. Os
    /// tokenizadores embutidos têm `iVersion` 0 e nunca a recebem.
    fn language_id(&mut self, _i_langid: i32) -> i32 {
        crate::consts::SQLITE_OK
    }
}

/// `sqlite3_tokenizer`: uma instância criada para uma tabela.
pub trait Fts3Tokenizer {
    /// `pModule->iVersion` (0 ou 1): com 1 ou mais, `xLanguageid` é chamado depois de abrir.
    fn i_version(&self) -> i32 {
        0
    }

    /// `xOpen`: um cursor sobre `input` (o texto inteiro; o C passa `pInput` e `nBytes`).
    fn open(&self, input: &[u8]) -> Result<Box<dyn Fts3TokenizerCursor>, i32>;
}

/// `sqlite3_tokenizer_module`: a implementação de um tokenizador, registrada pelo nome.
pub trait Fts3TokenizerModule {
    /// `iVersion`.
    fn i_version(&self) -> i32 {
        0
    }

    /// `xCreate`: cria o tokenizador. `args` são os argumentos da cláusula `tokenize=` depois do
    /// nome, já sem aspas.
    fn create(&self, args: &[Vec<u8>]) -> Result<Rc<dyn Fts3Tokenizer>, i32>;
}

// ---------------------------------------------------------------------------------------------
// Tabela e cursor
// ---------------------------------------------------------------------------------------------

/// `struct Fts3Index`: um índice (o principal ou um de prefixo) e os termos pendentes dele.
pub struct Fts3Index {
    /// `nPrefix`: o comprimento do prefixo (0 para o índice principal).
    pub n_prefix: i32,
    /// `hPending`: a tabela de termos pendentes deste índice.
    pub h_pending: Fts3Hash<PendingList>,
}

/// `Fts3Table`: uma conexão a um índice de texto completo. Construa com [`Fts3Table::new`].
pub struct Fts3Table {
    /// `base.zErrMsg`.
    pub z_err_msg: Option<Vec<u8>>,
    /// `zDb`: o nome lógico do banco.
    pub z_db: Vec<u8>,
    /// `zName`: o nome da tabela virtual.
    pub z_name: Vec<u8>,
    /// `azColumn`: os nomes das colunas (`nColumn` é o `len()`, [`Fts3Table::n_column`]).
    pub az_column: Vec<Vec<u8>>,
    /// `abNotindexed`: verdadeiro (1) nas colunas `notindexed`.
    pub ab_notindexed: Vec<u8>,
    /// `pTokenizer`.
    pub p_tokenizer: Option<Rc<dyn Fts3Tokenizer>>,
    /// `zContentTbl`: a opção `content=xxx`.
    pub z_content_tbl: Option<Vec<u8>>,
    /// `zLanguageid`: a opção `languageid=xxx`.
    pub z_languageid: Option<Vec<u8>>,
    /// `nAutoincrmerge`: o valor de `automerge`.
    pub n_autoincrmerge: i32,
    /// `nLeafAdd`: blocos folha acrescentados nesta transação.
    pub n_leaf_add: u32,
    /// `bLock`: impede tabelas `content=` recursivas.
    pub b_lock: i32,
    /// `aStmt[40]`: os comandos pré-compilados (cada um roda e é resetado numa chamada da API).
    pub a_stmt: Vec<Option<StmtId>>,
    /// `pSeekStmt`: o cache de `fts3CursorSeekStmt`.
    pub p_seek_stmt: Option<StmtId>,
    /// `pSegments`: o `sqlite3_blob *` aberto em `%_segments`. Este porte não tem `vdbeblob.c`: o
    /// handle é um comando `SELECT block FROM %_segments WHERE blockid=?1` parado na linha lida
    /// (`write.rs`, `fts3_read_block`). Fechar o blob é finalizar o comando.
    pub p_segments: Option<StmtId>,
    /// `zReadExprlist`.
    pub z_read_exprlist: Option<Vec<u8>>,
    /// `zWriteExprlist`.
    pub z_write_exprlist: Option<Vec<u8>>,
    /// `nNodeSize`: o limite brando do tamanho de um nó.
    pub n_node_size: i32,
    /// `bFts4`: verdadeiro para FTS4.
    pub b_fts4: bool,
    /// `bHasStat`: `%_stat` existe (2 é "desconhecido").
    pub b_has_stat: u8,
    /// `bHasDocsize`: `%_docsize` existe.
    pub b_has_docsize: bool,
    /// `bDescIdx`: as doclists estão em ordem inversa.
    pub b_desc_idx: bool,
    /// `bIgnoreSavepoint`: ignora as chamadas de `xSavepoint`.
    pub b_ignore_savepoint: bool,
    /// `nPgsz`: o tamanho de página do banco hospedeiro.
    pub n_pgsz: i32,
    /// `zSegmentsTbl`: o nome da tabela `%_segments`.
    pub z_segments_tbl: Option<Vec<u8>>,
    /// `iSavepoint`.
    pub i_savepoint: i32,
    /// `nIndex`: o tamanho de `aIndex` (a tabela `fts4aux` declara 1 sem ter `aIndex`).
    pub n_index: i32,
    /// `aIndex`: o índice 0 é o de todos os termos; os seguintes, de prefixos.
    pub a_index: Vec<Fts3Index>,
    /// `nMaxPendingData`: o máximo antes de descarregar as tabelas para o disco.
    pub n_max_pending_data: i32,
    /// `nPendingData`: a estimativa corrente de bytes pendentes.
    pub n_pending_data: i32,
    /// `iPrevDocid`: o docid do último registro inserido.
    pub i_prev_docid: i64,
    /// `iPrevLangid`.
    pub i_prev_langid: i32,
    /// `bPrevDelete`: a última operação foi um DELETE.
    pub b_prev_delete: bool,
}

impl Fts3Table {
    /// Uma tabela com tudo zerado (o `memset(0)` do C) e as vagas de `aStmt` vazias.
    pub fn new(z_db: Vec<u8>, z_name: Vec<u8>) -> Self {
        Fts3Table {
            z_err_msg: None,
            z_db,
            z_name,
            az_column: Vec::new(),
            ab_notindexed: Vec::new(),
            p_tokenizer: None,
            z_content_tbl: None,
            z_languageid: None,
            n_autoincrmerge: 0,
            n_leaf_add: 0,
            b_lock: 0,
            a_stmt: vec![None; FTS3_N_STMT],
            p_seek_stmt: None,
            p_segments: None,
            z_read_exprlist: None,
            z_write_exprlist: None,
            n_node_size: 0,
            b_fts4: false,
            b_has_stat: 0,
            b_has_docsize: false,
            b_desc_idx: false,
            b_ignore_savepoint: false,
            n_pgsz: 0,
            z_segments_tbl: None,
            i_savepoint: 0,
            n_index: 0,
            a_index: Vec::new(),
            n_max_pending_data: 0,
            n_pending_data: 0,
            i_prev_docid: 0,
            i_prev_langid: 0,
            b_prev_delete: false,
        }
    }

    /// `nColumn`: o número de colunas nomeadas.
    #[inline]
    pub fn n_column(&self) -> i32 {
        self.az_column.len() as i32
    }
}

/// `Fts3Cursor`: o cursor de uma tabela virtual FTS3/FTS4 (o `sqlite3_vtab_cursor` é o do
/// núcleo, no `VtabCursor` que a fatia 2 implementa sobre esta estrutura).
#[derive(Default)]
pub struct Fts3Cursor {
    /// `eSearch`: a estratégia de busca (`FTS3_*_SEARCH`, ou `FTS3_FULLTEXT_SEARCH + coluna`).
    pub e_search: i16,
    /// `isEof`.
    pub is_eof: bool,
    /// `isRequireSeek`: o `pStmt` precisa buscar a linha de `%_content`.
    pub is_require_seek: bool,
    /// `bSeekStmt`: o `pStmt` é um comando de busca.
    pub b_seek_stmt: bool,
    /// `pStmt`.
    pub p_stmt: Option<StmtId>,
    /// `pExpr`: a consulta MATCH analisada.
    pub p_expr: Option<Fts3ExprTree>,
    /// `iLangid`.
    pub i_langid: i32,
    /// `nPhrase`: o número de frases da consulta.
    pub n_phrase: i32,
    /// `pDeferred`: os tokens adiados. A lista do C insere na frente; o último elemento do `Vec`
    /// é a cabeça dela.
    pub p_deferred: Vec<Fts3DeferredToken>,
    /// `iPrevId`.
    pub i_prev_id: i64,
    /// `pNextId`: o deslocamento em `a_doclist` do próximo docid.
    pub p_next_id: usize,
    /// `aDoclist`/`nDoclist`: a lista de docids das consultas de texto completo.
    pub a_doclist: Vec<u8>,
    /// `bDesc`: ordem decrescente.
    pub b_desc: bool,
    /// `eEvalmode`: `FTS3_EVAL_*`.
    pub e_evalmode: i32,
    /// `nRowAvg`: o tamanho médio das linhas, em páginas.
    pub n_row_avg: i32,
    /// `nDoc`: documentos na tabela.
    pub n_doc: i64,
    /// `iMinDocid`.
    pub i_min_docid: i64,
    /// `iMaxDocid`.
    pub i_max_docid: i64,
    /// `isMatchinfoNeeded`.
    pub is_matchinfo_needed: bool,
    /// `pMIBuffer`.
    pub p_mi_buffer: Option<MatchinfoBuffer>,
}

// ---------------------------------------------------------------------------------------------
// Doclists, frases e a árvore de expressão
// ---------------------------------------------------------------------------------------------

/// `Fts3Doclist`.
#[derive(Default)]
pub struct Fts3Doclist {
    /// `aAll`/`nAll`: a doclist inteira (ou vazia).
    pub a_all: Vec<u8>,
    /// `pNextDocid`: o deslocamento em `a_all` do próximo docid (`None` é o `NULL`).
    pub p_next_docid: Option<usize>,
    /// `iDocid`: o docid corrente (se `p_list` não é `None`).
    pub i_docid: i64,
    /// `bFreeList`: verdadeiro se a lista de posições mora em `a_list` e não em `a_all`.
    pub b_free_list: bool,
    /// `pList`: o deslocamento da lista de posições que segue `i_docid`, em `a_all` (ou em
    /// `a_list` se `b_free_list`); `None` é o `NULL`.
    pub p_list: Option<usize>,
    /// O buffer próprio da lista quando ela não é parte de `a_all` (o que o C aloca e libera
    /// com `bFreeList`, ou o que devolve `sqlite3Fts3MsrIncrNext`).
    pub a_list: Vec<u8>,
    /// `nList`: o tamanho da lista de posições.
    pub n_list: i32,
}

/// `Fts3PhraseToken`: um token de uma frase.
#[derive(Default)]
pub struct Fts3PhraseToken {
    /// `z`/`n`: o texto do token.
    pub z: Vec<u8>,
    /// `isPrefix`: o token termina em `*`.
    pub is_prefix: bool,
    /// `bFirst`: o token tem de aparecer na posição 0.
    pub b_first: bool,
    /* Acima: preenchido na análise da expressão (`expr.rs`). Abaixo: usado na avaliação. */
    /// `pDeferred`: o índice em `Fts3Cursor.p_deferred` do token adiado deste token.
    pub p_deferred: Option<usize>,
    /// `pSegcsr`: o leitor de segmentos deste token.
    pub p_segcsr: Option<Box<Fts3MultiSegReader>>,
}

/// `Fts3Phrase`: uma sequência de um ou mais tokens que precisam casar em sequência.
#[derive(Default)]
pub struct Fts3Phrase {
    /// `doclist`: o cache da doclist da frase.
    pub doclist: Fts3Doclist,
    /// `bIncr`: a doclist é carregada incrementalmente.
    pub b_incr: bool,
    /// `iDoclistToken`.
    pub i_doclist_token: i32,
    /// `pOrPoslist`: usado por `sqlite3Fts3EvalPhrasePoslist` se a frase descende de um OR; o
    /// deslocamento em `doclist.a_all`.
    pub p_or_poslist: Option<usize>,
    /// `iOrDocid`.
    pub i_or_docid: i64,
    /// `iColumn`: o índice da coluna que a frase precisa casar (negativo: qualquer).
    pub i_column: i32,
    /// `aToken` (`nToken` é o `len()`, [`Fts3Phrase::n_token`]).
    pub a_token: Vec<Fts3PhraseToken>,
}

impl Fts3Phrase {
    /// `nToken`: o número de tokens da frase.
    #[inline]
    pub fn n_token(&self) -> i32 {
        self.a_token.len() as i32
    }
}

/// O `Fts3Expr *` do C: o índice de um nó em [`Fts3ExprTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

/// `Fts3Expr`: um nó da consulta MATCH.
#[derive(Default)]
pub struct Fts3Expr {
    /// `eType`: um `FTSQUERY_*`.
    pub e_type: i32,
    /// `nNear`: válido se `e_type==FTSQUERY_NEAR`.
    pub n_near: i32,
    /// `pParent`: o pai (`pParent->pLeft==this` ou `pParent->pRight==this`).
    pub p_parent: Option<ExprId>,
    /// `pLeft`: o operando esquerdo.
    pub p_left: Option<ExprId>,
    /// `pRight`: o operando direito.
    pub p_right: Option<ExprId>,
    /// `pPhrase`: válido se `e_type==FTSQUERY_PHRASE`.
    pub p_phrase: Option<Box<Fts3Phrase>>,
    /* Usados pela avaliação (`sqlite3Fts3EvalXxx`). */
    /// `iDocid`: o docid corrente.
    pub i_docid: i64,
    /// `bEof`: a expressão já chegou ao fim.
    pub b_eof: bool,
    /// `bStart`: `i_docid` é válido.
    pub b_start: bool,
    /// `bDeferred`: a expressão inteira é adiada.
    pub b_deferred: bool,
    /* Usados por `fts3_snippet.c`. */
    /// `iPhrase`: o índice desta frase nos resultados de `matchinfo()`.
    pub i_phrase: i32,
    /// `aMI`: os dados globais de matchinfo dos nós NEAR (`nCol*3` entradas: o `[3*i+1]` é o total
    /// de ocorrências e o `[3*i+2]` o de linhas com pelo menos uma). Vazio é o `NULL`.
    pub a_mi: Vec<u32>,
}

/// A árvore de expressão de uma consulta MATCH: a arena de nós e a raiz. `root` é `None` para a
/// consulta vazia.
#[derive(Default)]
pub struct Fts3ExprTree {
    nodes: Vec<Option<Fts3Expr>>,
    /// A raiz (o `Fts3Expr *` que `sqlite3Fts3ExprParse` devolve).
    pub root: Option<ExprId>,
}

impl Fts3ExprTree {
    /// Uma arena vazia.
    pub fn new() -> Self {
        Fts3ExprTree::default()
    }

    /// Aloca um nó e devolve o handle dele (o `sqlite3Fts3MallocZero` de um `Fts3Expr`).
    pub fn alloc(&mut self, e: Fts3Expr) -> ExprId {
        self.nodes.push(Some(e));
        ExprId((self.nodes.len() - 1) as u32)
    }

    /// Solta um único nó (o `sqlite3_free` de um `Fts3Expr` sem os filhos) e o devolve.
    pub fn release(&mut self, id: ExprId) -> Option<Fts3Expr> {
        self.nodes[id.0 as usize].take()
    }

    /// Solta a subárvore de `root` sem recursão (como o `sqlite3Fts3ExprFree`, que evita a pilha
    /// para consultas enormes). `cleanup` roda em cada nó antes de ele ser solto: é o
    /// `sqlite3Fts3EvalPhraseCleanup` do `fts3FreeExprNode`.
    pub fn free_subtree(&mut self, root: Option<ExprId>, cleanup: &mut dyn FnMut(&mut Fts3Expr)) {
        let mut stack: Vec<ExprId> = root.into_iter().collect();
        while let Some(id) = stack.pop() {
            if let Some(mut node) = self.nodes[id.0 as usize].take() {
                if let Some(r) = node.p_right {
                    stack.push(r);
                }
                if let Some(l) = node.p_left {
                    stack.push(l);
                }
                cleanup(&mut node);
            }
        }
    }
}

impl Index<ExprId> for Fts3ExprTree {
    type Output = Fts3Expr;

    #[inline]
    fn index(&self, id: ExprId) -> &Fts3Expr {
        self.nodes[id.0 as usize].as_ref().expect("fts3: nó de expressão solto")
    }
}

impl IndexMut<ExprId> for Fts3ExprTree {
    #[inline]
    fn index_mut(&mut self, id: ExprId) -> &mut Fts3Expr {
        self.nodes[id.0 as usize].as_mut().expect("fts3: nó de expressão solto")
    }
}

// ---------------------------------------------------------------------------------------------
// Leitores de segmentos
// ---------------------------------------------------------------------------------------------

/// `Fts3SegFilter`: o filtro do 4o argumento de `SegmentReaderIterate`.
#[derive(Debug, Clone, Default)]
pub struct Fts3SegFilter {
    /// `zTerm`/`nTerm`: o termo (`None` é o `NULL`).
    pub z_term: Option<Vec<u8>>,
    /// `iCol`.
    pub i_col: i32,
    /// `flags`: `FTS3_SEGMENT_*`.
    pub flags: i32,
}

/// `Fts3MultiSegReader`: itera pela união de vários segmentos de um índice.
#[derive(Default)]
pub struct Fts3MultiSegReader {
    /* Usados internamente pelas chamadas `sqlite3Fts3SegReaderXxx`. */
    /// `apSegment` (`nSegment` é o `len()`).
    pub ap_segment: Vec<Fts3SegReader>,
    /// `nAdvance`: quantos leitores avançar.
    pub n_advance: i32,
    /// `pFilter`: uma cópia do filtro passado a `sqlite3Fts3SegReaderStart` (o C guarda um ponteiro
    /// para o filtro do chamador, que não muda durante a iteração).
    pub p_filter: Option<Fts3SegFilter>,
    /// `aBuffer`/`nBuffer`: o buffer onde fundir doclists.
    pub a_buffer: Vec<u8>,
    /// `iColFilter`: se não é negativo, filtra por esta coluna.
    pub i_col_filter: i32,
    /// `bRestart`.
    pub b_restart: bool,
    /* Usados só por `fts3.c`. */
    /// `nCost`: o custo de rodar o iterador.
    pub n_cost: i32,
    /// `bLookup`: a busca de uma única entrada.
    pub b_lookup: bool,
    /* Saídas, válidas depois de `sqlite3Fts3SegReaderStep` devolver `SQLITE_ROW`. */
    /// `zTerm`/`nTerm`.
    pub z_term: Vec<u8>,
    /// `aDoclist`/`nDoclist`.
    pub a_doclist: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------

/// A varredura que os tokenizadores `simple` e `porter` repetem no `xNext`: a partir de
/// `*i_offset`, pula os delimitadores e conta os caracteres que não o são. Devolve o começo e o
/// fim do token (e deixa `*i_offset` no fim), ou `None` quando a entrada acaba sem outro token.
pub fn scan_delimited_token(
    input: &[u8],
    i_offset: &mut usize,
    is_delim: impl Fn(u8) -> bool,
) -> Option<(usize, usize)> {
    let n_bytes = input.len();
    while *i_offset < n_bytes {
        /* Pula os delimitadores. */
        while *i_offset < n_bytes && is_delim(input[*i_offset]) {
            *i_offset += 1;
        }

        /* Conta os caracteres que não são delimitadores. */
        let i_start_offset = *i_offset;
        while *i_offset < n_bytes && !is_delim(input[*i_offset]) {
            *i_offset += 1;
        }

        if *i_offset > i_start_offset {
            return Some((i_start_offset, *i_offset));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Auxiliares de fts3.c usados antes da fatia 2
// ---------------------------------------------------------------------------------------------

/// `sqlite3Fts3Dequote`: converte, no próprio buffer, uma string SQL entre aspas em string normal,
/// sem as aspas. `"abc"`, `'xyz'`, `[pqr]` e `` `mno` `` viram `abc`, `xyz`, `pqr` e `mno`; fora
/// de aspas nada acontece. É o `sqlite3Dequote` do núcleo (mesmos quatro caracteres de aspas, o
/// `]` fecha o `[` e a aspa dobrada vale por uma) mais o corte do texto no NUL que ele grava.
pub fn fts3_dequote(z: &mut Vec<u8>) {
    dequote(z);
    let n = strlen30(z) as usize;
    z.truncate(n);
}

/// `sqlite3Fts3ReadInt`: `z` começa com um inteiro positivo em texto decimal. Grava o valor em
/// `pn_out` e devolve o número de bytes consumidos, ou -1 (sem gravar) se estoura 31 bits.
pub fn fts3_read_int(z: &[u8], pn_out: &mut i32) -> i32 {
    let mut i_val: u64 = 0;
    let mut i = 0usize;
    while at(z, i).is_ascii_digit() {
        i_val = i_val * 10 + (at(z, i) - b'0') as u64;
        if i_val > 0x7FFF_FFFF {
            return -1;
        }
        i += 1;
    }
    *pn_out = i_val as i32;
    i as i32
}
