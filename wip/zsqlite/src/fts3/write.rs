//! `fts3_write.c` (parte 1): inserção, atualização e remoção de linhas nas tabelas FTS3/FTS4,
//! termos pendentes, leitores de segmentos, escrita de segmentos, fusão e `optimize`. A parte 2
//! ([`super::write2`]), reexportada no fim deste módulo, tem a fusão incremental, a verificação de
//! integridade, os comandos especiais do `INSERT`, os tokens adiados, o `xUpdate` e as rotinas de
//! `fts3.c` que esta camada precisa (`sqlite3Fts3SegReaderCursor`, `sqlite3Fts3DoclistPrev`,
//! `sqlite3Fts3FirstFilter`, `sqlite3Fts3CreateStatTable`) e que `main.rs` importa de lá em vez de
//! redefinir.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **Sem `sqlite3 *db` na tabela.** Toda função que toca o banco recebe `db: &mut Connection` na
//!   frente. O `Fts3Table` guarda os comandos pré-compilados (`a_stmt`) como `StmtId`.
//! * **`apVal` é `&[Mem]`** e `sqlite3_value_text`/`bytes` são `text_of` (o `Cow` tem o comprimento
//!   do texto UTF-8). "String C" é `&[u8]` e o comprimento é o da fatia; onde o C passa `-1` ao
//!   tokenizador o texto vale até o primeiro NUL ([`fts3_c_str`]).
//! * **`sqlite3_blob` de `%_segments`.** O crate não tem `vdbeblob.c`. O blob que `pSegments`
//!   guarda é o comando `SELECT block FROM %_segments WHERE blockid=?1` parado na linha lida (o
//!   mesmo caminho de `rtree.rs` e `fts5/index.rs`), em [`Fts3Table::p_segments`]. Os erros são os
//!   que o C converte em `SQLITE_CORRUPT_VTAB`: tabela ausente, linha ausente, coluna que não é
//!   BLOB nem TEXT (o marcador `NULL` dos segmentos anexáveis também). O comando é reiniciado a cada
//!   leitura.
//! * **Leitura incremental de nós (`nPopulate`, `pBlob`, `fts3SegReaderIncrRead`,
//!   `fts3SegReaderRequire`, `FTS3_NODE_CHUNKSIZE`) não existe.** No C o único chamador de
//!   `fts3SegReaderNext` passa `bIncr` constante zero (em `fts3SegReaderStart` e em
//!   `sqlite3Fts3SegReaderStep`), então `nPopulate` vale sempre zero, `pBlob` é sempre nulo e
//!   `fts3SegReaderRequire` nunca faz nada. O código morto some e, com ele, o `rc` de
//!   `fts3SegReaderFirstDocid` e `fts3SegReaderNextDocid`, que só vinha dali. Pelo mesmo motivo
//!   `sqlite3Fts3SegReaderFinish` recebe o `db` (o contrato de `aux.rs`) e não o usa.
//! * **Ponteiros para dentro de buffers** (`aDoclist`, `pOffsetList`, `pList`) são deslocamentos no
//!   `a_node` do leitor. O que o C devolve por ponteiro ao chamador (`Fts3MultiSegReader.aDoclist`,
//!   o `*paPoslist` de `sqlite3Fts3MsrIncrNext`) é uma cópia (`Vec<u8>`): a edição no lugar que
//!   `fts3EvalNearTrim` faz na lista devolvida passa a valer só para a cópia. A leitura além do fim
//!   de um buffer enxerga zero, como o `FTS3_NODE_PADDING` do C.
//! * **Ordenação de leitores.** `apSegment` é um `Vec<Fts3SegReader>` e `fts3SegReaderSort` troca os
//!   elementos de lugar (o C troca ponteiros).
//! * **`SegmentNode`** é um arena (`Vec<SegmentNode>` do `SegmentWriter`) e os ponteiros
//!   `pParent`/`pRight`/`pLeftmost` são índices. `zTerm`/`zMalloc`/`isCopyTerm` somem: o termo é
//!   sempre uma cópia (`fts3SegWriterAdd` só é chamada com `isCopyTerm` verdadeiro).
//! * **`PendingList`** guarda `aData` num `Vec` cujo comprimento é `nData` (o `'\0'` em
//!   `aData[nData]` é implícito) e `nSpace` some. O `Fts3Hash<PendingList>` guarda a lista por
//!   valor, então `fts3PendingListAppend` não precisa devolver "foi realocada". Os leitores de
//!   termos pendentes guardam os [`HashElemId`] dos elementos (o `ppNextElem` do C).
//! * **Sem falta de memória.** As funções que só falhavam com `SQLITE_NOMEM` perdem o código.
//! * **Sem `SQLITE_DEBUG`/`SQLITE_TEST`.** Somem `fts3LogMerge`, os comandos `nodesize=`,
//!   `maxpending=`, `mergecount=` e `test-no-incr-doclist=`, `MergeCount(P)` é
//!   [`FTS3_MERGE_COUNT`].

use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_BLOB, SQLITE_CONSTRAINT, SQLITE_DONE, SQLITE_ERROR, SQLITE_INTEGER, SQLITE_MISUSE,
    SQLITE_NOMEM, SQLITE_NULL, SQLITE_OK, SQLITE_PREPARE_NO_VTAB, SQLITE_PREPARE_PERSISTENT,
    SQLITE_ROW, SQLITE_TEXT,
};
use crate::build::text_arg;
use crate::mem::{value_type, Mem, StrDtor};
use crate::prepare::{prepare_v2, prepare_v3};
use crate::printf::{mprintf, PrintfArg};
use crate::util::at;
use crate::vdbeapi::{
    bind_blob, bind_int, bind_int64, bind_null, bind_parameter_count, bind_text, bind_value,
    column_blob, column_bytes, column_int, column_int64, column_text, column_type, finalize, reset,
    step, text_of, value_int, value_int64,
};

use super::expr::fts3_open_tokenizer;
use super::hash::HashElemId;
use super::int::{
    Fts3MultiSegReader, Fts3SegFilter, Fts3Table, FTS3_MERGE_COUNT, FTS3_N_STMT, FTS3_SEGCURSOR_ALL,
    FTS3_SEGCURSOR_PENDING, FTS3_SEGDIR_MAXLEVEL, FTS3_SEGMENT_COLUMN_FILTER, FTS3_SEGMENT_FIRST,
    FTS3_SEGMENT_IGNORE_EMPTY, FTS3_SEGMENT_PREFIX, FTS3_SEGMENT_REQUIRE_POS, FTS3_SEGMENT_SCAN,
    FTS3_VARINT_MAX, FTS_CORRUPT_VTAB,
};
use super::varint::{
    fts3_get_varint, fts3_get_varint32, fts3_get_varint_u, fts3_put_varint, fts3_varint_len,
};

pub use super::write2::*;

// ---------------------------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------------------------

/// `FTS_MAX_APPENDABLE_HEIGHT`: o número de camadas de uma árvore anexável.
pub(super) const FTS_MAX_APPENDABLE_HEIGHT: usize = 16;

/// `FTS3_NODE_PADDING`: os bytes de folga (zeros) depois de um nó lido do disco.
pub const FTS3_NODE_PADDING: usize = FTS3_VARINT_MAX * 2;

/// `FTS_STAT_DOCTOTAL`: a linha de `%_stat` com os totais de documentos.
pub(super) const FTS_STAT_DOCTOTAL: i32 = 0;
/// `FTS_STAT_INCRMERGEHINT`: a linha de `%_stat` com a dica da fusão incremental.
pub(super) const FTS_STAT_INCRMERGEHINT: i32 = 1;
/// `FTS_STAT_AUTOINCRMERGE`: a linha de `%_stat` com o valor de `automerge`.
pub(super) const FTS_STAT_AUTOINCRMERGE: i32 = 2;

/// `sizeof(Fts3HashElem)` no x86-64 (quatro ponteiros e um `int`, com enchimento). Entra na
/// estimativa `nPendingData`, que decide quando os termos pendentes descem para o disco.
const FTS3_HASH_ELEM_SIZE: i32 = 40;

/// Os valores do segundo argumento de `fts3SqlStmt`.
pub(super) const SQL_DELETE_CONTENT: usize = 0;
pub(super) const SQL_IS_EMPTY: usize = 1;
pub(super) const SQL_DELETE_ALL_CONTENT: usize = 2;
pub(super) const SQL_DELETE_ALL_SEGMENTS: usize = 3;
pub(super) const SQL_DELETE_ALL_SEGDIR: usize = 4;
pub(super) const SQL_DELETE_ALL_DOCSIZE: usize = 5;
pub(super) const SQL_DELETE_ALL_STAT: usize = 6;
pub(super) const SQL_SELECT_CONTENT_BY_ROWID: usize = 7;
pub(super) const SQL_NEXT_SEGMENT_INDEX: usize = 8;
pub(super) const SQL_INSERT_SEGMENTS: usize = 9;
pub(super) const SQL_NEXT_SEGMENTS_ID: usize = 10;
pub(super) const SQL_INSERT_SEGDIR: usize = 11;
pub(super) const SQL_SELECT_LEVEL: usize = 12;
pub(super) const SQL_SELECT_LEVEL_RANGE: usize = 13;
pub(super) const SQL_SELECT_SEGDIR_MAX_LEVEL: usize = 15;
pub(super) const SQL_DELETE_SEGDIR_LEVEL: usize = 16;
pub(super) const SQL_DELETE_SEGMENTS_RANGE: usize = 17;
pub(super) const SQL_CONTENT_INSERT: usize = 18;
pub(super) const SQL_DELETE_DOCSIZE: usize = 19;
pub(super) const SQL_REPLACE_DOCSIZE: usize = 20;
pub(super) const SQL_SELECT_DOCSIZE: usize = 21;
pub(super) const SQL_SELECT_STAT: usize = 22;
pub(super) const SQL_REPLACE_STAT: usize = 23;
pub(super) const SQL_DELETE_SEGDIR_RANGE: usize = 26;
pub(super) const SQL_SELECT_ALL_LANGID: usize = 27;
pub(super) const SQL_FIND_MERGE_LEVEL: usize = 28;
pub(super) const SQL_MAX_LEAF_NODE_ESTIMATE: usize = 29;
pub(super) const SQL_DELETE_SEGDIR_ENTRY: usize = 30;
pub(super) const SQL_SHIFT_SEGDIR_ENTRY: usize = 31;
pub(super) const SQL_SELECT_SEGDIR: usize = 32;
pub(super) const SQL_CHOMP_SEGDIR: usize = 33;
pub(super) const SQL_SEGMENT_IS_APPENDABLE: usize = 34;
pub(super) const SQL_SELECT_INDEXES: usize = 35;
pub(super) const SQL_SELECT_MXLEVEL: usize = 36;
pub(super) const SQL_SELECT_LEVEL_RANGE2: usize = 37;
pub(super) const SQL_UPDATE_LEVEL_IDX: usize = 38;
pub(super) const SQL_UPDATE_LEVEL: usize = 39;

/// `azSql[]` de `fts3SqlStmt`, byte a byte (inclusive a falta de espaço antes de `ORDER BY` no
/// índice 13 e de `WHERE` no 33, que vêm da concatenação de literais do C).
const AZ_SQL: [&str; FTS3_N_STMT] = [
    /* 0  */ "DELETE FROM %Q.'%q_content' WHERE rowid = ?",
    /* 1  */ "SELECT NOT EXISTS(SELECT docid FROM %Q.'%q_content' WHERE rowid!=?)",
    /* 2  */ "DELETE FROM %Q.'%q_content'",
    /* 3  */ "DELETE FROM %Q.'%q_segments'",
    /* 4  */ "DELETE FROM %Q.'%q_segdir'",
    /* 5  */ "DELETE FROM %Q.'%q_docsize'",
    /* 6  */ "DELETE FROM %Q.'%q_stat'",
    /* 7  */ "SELECT %s WHERE rowid=?",
    /* 8  */ "SELECT (SELECT max(idx) FROM %Q.'%q_segdir' WHERE level = ?) + 1",
    /* 9  */ "REPLACE INTO %Q.'%q_segments'(blockid, block) VALUES(?, ?)",
    /* 10 */ "SELECT coalesce((SELECT max(blockid) FROM %Q.'%q_segments') + 1, 1)",
    /* 11 */ "REPLACE INTO %Q.'%q_segdir' VALUES(?,?,?,?,?,?)",
    /* 12 */
    "SELECT idx, start_block, leaves_end_block, end_block, root FROM %Q.'%q_segdir' WHERE level = ? ORDER BY idx ASC",
    /* 13 */
    "SELECT idx, start_block, leaves_end_block, end_block, root FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?ORDER BY level DESC, idx ASC",
    /* 14 */ "SELECT count(*) FROM %Q.'%q_segdir' WHERE level = ?",
    /* 15 */ "SELECT max(level) FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?",
    /* 16 */ "DELETE FROM %Q.'%q_segdir' WHERE level = ?",
    /* 17 */ "DELETE FROM %Q.'%q_segments' WHERE blockid BETWEEN ? AND ?",
    /* 18 */ "INSERT INTO %Q.'%q_content' VALUES(%s)",
    /* 19 */ "DELETE FROM %Q.'%q_docsize' WHERE docid = ?",
    /* 20 */ "REPLACE INTO %Q.'%q_docsize' VALUES(?,?)",
    /* 21 */ "SELECT size FROM %Q.'%q_docsize' WHERE docid=?",
    /* 22 */ "SELECT value FROM %Q.'%q_stat' WHERE id=?",
    /* 23 */ "REPLACE INTO %Q.'%q_stat' VALUES(?,?)",
    /* 24 */ "",
    /* 25 */ "",
    /* 26 */ "DELETE FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ?",
    /* 27 */ "SELECT ? UNION SELECT level / (1024 * ?) FROM %Q.'%q_segdir'",
    /* 28 */
    "SELECT level, count(*) AS cnt FROM %Q.'%q_segdir'   GROUP BY level HAVING cnt>=?  ORDER BY (level %% 1024) ASC, 2 DESC LIMIT 1",
    /* 29 */
    "SELECT 2 * total(1 + leaves_end_block - start_block)   FROM (SELECT * FROM %Q.'%q_segdir'         WHERE level = ? ORDER BY idx ASC LIMIT ?  )",
    /* 30 */ "DELETE FROM %Q.'%q_segdir' WHERE level = ? AND idx = ?",
    /* 31 */ "UPDATE %Q.'%q_segdir' SET idx = ? WHERE level=? AND idx=?",
    /* 32 */
    "SELECT idx, start_block, leaves_end_block, end_block, root FROM %Q.'%q_segdir' WHERE level = ? AND idx = ?",
    /* 33 */ "UPDATE %Q.'%q_segdir' SET start_block = ?, root = ?WHERE level = ? AND idx = ?",
    /* 34 */ "SELECT 1 FROM %Q.'%q_segments' WHERE blockid=? AND block IS NULL",
    /* 35 */ "SELECT idx FROM %Q.'%q_segdir' WHERE level=? ORDER BY 1 ASC",
    /* 36 */ "SELECT max( level %% 1024 ) FROM %Q.'%q_segdir'",
    /* 37 */
    "SELECT level, idx, end_block FROM %Q.'%q_segdir' WHERE level BETWEEN ? AND ? ORDER BY level DESC, idx ASC",
    /* 38 */ "UPDATE OR FAIL %Q.'%q_segdir' SET level=-1,idx=? WHERE level=? AND idx=?",
    /* 39 */ "UPDATE OR FAIL %Q.'%q_segdir' SET level=? WHERE level=-1",
];

// ---------------------------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------------------------

/// `PendingList`: uma doclist montada aos poucos (ver [`pending_list_append`]).
#[derive(Debug, Clone, Default)]
pub struct PendingList {
    /// `aData`/`nData`: os bytes da doclist; o `'\0'` que o C mantém em `aData[nData]` é implícito.
    pub a_data: Vec<u8>,
    /// `iLastDocid`.
    pub i_last_docid: i64,
    /// `iLastCol`.
    pub i_last_col: i64,
    /// `iLastPos`.
    pub i_last_pos: i64,
}

impl PendingList {
    /// `nData`: o número de bytes da lista.
    #[inline]
    pub fn n_data(&self) -> i32 {
        self.a_data.len() as i32
    }
}

/// `Fts3DeferredToken`: um token adiado de um cursor. O C guarda o `Fts3PhraseToken *` e o
/// `pNext` da lista (que é o `Vec` de [`super::int::Fts3Cursor::p_deferred`], cuja cabeça é o
/// último elemento). Aqui o token é uma cópia dos três campos que o código desta camada lê, e o
/// `Fts3PhraseToken::p_deferred` guarda o índice do elemento no `Vec`.
#[derive(Debug, Clone, Default)]
pub struct Fts3DeferredToken {
    /// `pToken->z`/`n`.
    pub z: Vec<u8>,
    /// `pToken->isPrefix`.
    pub is_prefix: bool,
    /// `pToken->bFirst`.
    pub b_first: bool,
    /// `iCol`: a coluna em que o token precisa ocorrer (ou `-1`).
    pub i_col: i32,
    /// `pList`: a doclist é montada aqui.
    pub p_list: Option<PendingList>,
}

/// O `ppNextElem` de um leitor de termos pendentes: os elementos do hash que ele percorre.
#[derive(Debug, Clone)]
pub struct PendingIter {
    /// O índice de `Fts3Table::a_index` cujo `h_pending` tem os elementos.
    pub i_index: usize,
    /// Os elementos, na ordem da visita (o C termina o vetor com `NULL`).
    pub elems: Vec<HashElemId>,
    /// A posição em `elems` do próximo elemento.
    pub i_next: usize,
}

/// `Fts3SegReader`: percorre os termos de um conjunto contíguo de folhas de um segmento (ou os
/// termos pendentes).
#[derive(Debug, Default)]
pub struct Fts3SegReader {
    /// `iIdx`: o índice dentro do nível, ou `0x7FFFFFFF` para os termos pendentes.
    pub i_idx: i32,
    /// `bLookup`: verdadeiro se só é uma busca.
    pub b_lookup: bool,
    /// `rootOnly`: verdadeiro se o segmento todo é o nó raiz.
    pub root_only: bool,
    /// `iStartBlock`.
    pub i_start_block: i64,
    /// `iLeafEndBlock`.
    pub i_leaf_end_block: i64,
    /// `iEndBlock`.
    pub i_end_block: i64,
    /// `iCurrentBlock`.
    pub i_current_block: i64,
    /// `aNode`: os dados do nó corrente (`None` é o `NULL`: fim). O buffer tem `n_node` bytes
    /// válidos, mais [`FTS3_NODE_PADDING`] de zeros quando vem do disco.
    pub a_node: Option<Vec<u8>>,
    /// `nNode`.
    pub n_node: i32,
    /// `ppNextElem`: o iterador dos termos pendentes (`None` se o leitor não é de termos pendentes).
    pub pending: Option<PendingIter>,
    /// `zTerm`/`nTerm`: o termo corrente.
    pub z_term: Vec<u8>,
    /// `aDoclist`: o deslocamento em `a_node` da doclist da entrada corrente (`None` é o `NULL`).
    pub a_doclist: Option<usize>,
    /// `nDoclist`.
    pub n_doclist: i32,
    /// `pOffsetList`: o deslocamento em `a_node` da lista de posições corrente.
    pub p_offset_list: Option<usize>,
    /// `nOffsetList`: só para leitores pendentes em ordem decrescente.
    pub n_offset_list: i32,
    /// `iDocid`.
    pub i_docid: i64,
}

impl Fts3SegReader {
    /// `fts3SegReaderIsPending`.
    #[inline]
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// `SegmentNode`: um nó interno da árvore de um segmento em construção.
#[derive(Debug, Default)]
pub(super) struct SegmentNode {
    /// `pParent`.
    p_parent: Option<usize>,
    /// `pRight`.
    p_right: Option<usize>,
    /// `pLeftmost`.
    p_leftmost: Option<usize>,
    /// `nEntry`: termos escritos no nó até agora.
    n_entry: i32,
    /// `zTerm`/`nTerm`: o termo anterior (`None` é o `NULL`: nenhum termo ainda).
    z_term: Option<Vec<u8>>,
    /// `nData`: os bytes válidos de `a_data`.
    n_data: i32,
    /// `aData`.
    a_data: Vec<u8>,
}

/// `SegmentWriter`: constrói a árvore de um segmento no banco.
#[derive(Debug, Default)]
pub(super) struct SegmentWriter {
    /// O arena de `SegmentNode`.
    nodes: Vec<SegmentNode>,
    /// `pTree`: o nó interno corrente da camada mais baixa.
    p_tree: Option<usize>,
    /// `iFirst`: o primeiro slot escrito em `%_segments`.
    i_first: i64,
    /// `iFree`: o próximo slot livre em `%_segments`.
    i_free: i64,
    /// `zTerm`/`nTerm`: o termo anterior.
    z_term: Vec<u8>,
    /// `nData`: os bytes de `a_data` com dados.
    n_data: i32,
    /// `aData`: a folha em construção.
    a_data: Vec<u8>,
    /// `nLeafData`: os bytes de folha escritos.
    n_leaf_data: i64,
}

// ---------------------------------------------------------------------------------------------
// Auxiliares pequenos
// ---------------------------------------------------------------------------------------------

/// A cauda de `z` a partir de `i` (vazia depois do fim).
#[inline]
pub(super) fn sl(z: &[u8], i: usize) -> &[u8] {
    if i >= z.len() {
        &[]
    } else {
        &z[i..]
    }
}

/// O texto C: vale até o primeiro NUL (o `-1` que o C passa como comprimento ao tokenizador).
#[inline]
pub(super) fn fts3_c_str(z: &[u8]) -> &[u8] {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    &z[..n]
}

/// O sinal de `memcmp` de duas fatias de mesmo comprimento.
#[inline]
pub(super) fn memcmp(a: &[u8], b: &[u8]) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Escreve o varint de `v` em `buf[off..]`, crescendo o buffer se preciso; devolve o tamanho.
pub(super) fn put_varint_at(buf: &mut Vec<u8>, off: usize, v: i64) -> i32 {
    if buf.len() < off + FTS3_VARINT_MAX {
        buf.resize(off + FTS3_VARINT_MAX, 0);
    }
    fts3_put_varint(&mut buf[off..], v)
}

/// Copia `src` em `buf[off..]`, crescendo o buffer se preciso.
pub(super) fn put_bytes_at(buf: &mut Vec<u8>, off: usize, src: &[u8]) {
    if buf.len() < off + src.len() {
        buf.resize(off + src.len(), 0);
    }
    buf[off..off + src.len()].copy_from_slice(src);
}

// ---------------------------------------------------------------------------------------------
// Comandos SQL
// ---------------------------------------------------------------------------------------------

/// `fts3SqlStmt`: o comando preparado de `e_stmt`, com os valores de `ap_val` ligados aos
/// parâmetros (um por parâmetro do comando). Em erro devolve o código (o comando, se preparado,
/// continua guardado em `a_stmt`).
pub(super) fn fts3_sql_stmt(
    db: &mut Connection,
    p: &mut Fts3Table,
    e_stmt: usize,
    ap_val: &[Mem],
) -> Result<StmtId, i32> {
    debug_assert!(e_stmt < FTS3_N_STMT);
    let mut rc = SQLITE_OK;
    let mut p_stmt = p.a_stmt[e_stmt];
    if p_stmt.is_none() {
        let mut f = SQLITE_PREPARE_PERSISTENT | SQLITE_PREPARE_NO_VTAB;
        let fmt = AZ_SQL[e_stmt].as_bytes();
        let z_sql = if e_stmt == SQL_CONTENT_INSERT {
            mprintf(
                fmt,
                &[
                    text_arg(&p.z_db),
                    text_arg(&p.z_name),
                    PrintfArg::Text(p.z_write_exprlist.clone()),
                ],
            )
        } else if e_stmt == SQL_SELECT_CONTENT_BY_ROWID {
            f &= !SQLITE_PREPARE_NO_VTAB;
            mprintf(fmt, &[PrintfArg::Text(p.z_read_exprlist.clone())])
        } else {
            mprintf(fmt, &[text_arg(&p.z_db), text_arg(&p.z_name)])
        };
        match z_sql {
            None => rc = SQLITE_NOMEM,
            Some(z_sql) => {
                let (rc2, stmt, _) = prepare_v3(db, &z_sql, -1, f);
                rc = rc2;
                p_stmt = stmt;
                p.a_stmt[e_stmt] = stmt;
            }
        }
    }
    if !ap_val.is_empty() {
        if let Some(st) = p_stmt {
            let n_param = bind_parameter_count(db, st);
            let mut i = 0;
            while rc == SQLITE_OK && i < n_param {
                rc = match ap_val.get(i as usize) {
                    Some(v) => bind_value(db, st, i + 1, v),
                    None => SQLITE_MISUSE,
                };
                i += 1;
            }
        }
    }
    match p_stmt {
        Some(st) if rc == SQLITE_OK => Ok(st),
        _ => Err(rc),
    }
}

/// `fts3SelectDocsize`/`sqlite3Fts3SelectDocsize`: o comando `SQL_SELECT_DOCSIZE` parado na linha
/// do docid (a coluna 0 é um BLOB). Quem recebe o comando o reinicia com `reset`.
pub fn fts3_select_docsize(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_docid: i64,
) -> Result<StmtId, i32> {
    let st = fts3_sql_stmt(db, p, SQL_SELECT_DOCSIZE, &[])?;
    bind_int64(db, st, 1, i_docid);
    let rc = step(db, st);
    if rc != SQLITE_ROW || column_type(db, st, 0) != SQLITE_BLOB {
        let mut rc = reset(db, st);
        if rc == SQLITE_OK {
            rc = FTS_CORRUPT_VTAB;
        }
        return Err(rc);
    }
    Ok(st)
}

/// `sqlite3Fts3SelectDoctotal`: o comando `SQL_SELECT_STAT` parado na linha dos totais.
pub fn fts3_select_doctotal(db: &mut Connection, p: &mut Fts3Table) -> Result<StmtId, i32> {
    let st = fts3_sql_stmt(db, p, SQL_SELECT_STAT, &[])?;
    bind_int(db, st, 1, FTS_STAT_DOCTOTAL);
    if step(db, st) != SQLITE_ROW || column_type(db, st, 0) != SQLITE_BLOB {
        let mut rc = reset(db, st);
        if rc == SQLITE_OK {
            rc = FTS_CORRUPT_VTAB;
        }
        return Err(rc);
    }
    Ok(st)
}

/// `fts3SqlExec`: como `fts3SqlStmt`, mas executa o comando. Não faz nada se `*rc` já é erro.
pub(super) fn fts3_sql_exec(
    rc: &mut i32,
    db: &mut Connection,
    p: &mut Fts3Table,
    e_stmt: usize,
    ap_val: &[Mem],
) {
    if *rc != SQLITE_OK {
        return;
    }
    match fts3_sql_stmt(db, p, e_stmt, ap_val) {
        Ok(st) => {
            step(db, st);
            *rc = reset(db, st);
        }
        Err(e) => *rc = e,
    }
}

/// `fts3Writelock`: garante o bloqueio de escrita de `%_segdir` antes de gravar.
pub(super) fn fts3_writelock(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut rc = SQLITE_OK;
    if p.n_pending_data == 0 {
        match fts3_sql_stmt(db, p, SQL_DELETE_SEGDIR_LEVEL, &[]) {
            Ok(st) => {
                bind_null(db, st, 1);
                step(db, st);
                rc = reset(db, st);
            }
            Err(e) => rc = e,
        }
    }
    rc
}

/// `getAbsoluteLevel`: o nível absoluto de `i_level` no índice `i_index` do idioma `i_langid`.
pub(super) fn get_absolute_level(p: &Fts3Table, i_langid: i32, i_index: i32, i_level: i32) -> i64 {
    debug_assert!(i_langid >= 0);
    debug_assert!(p.n_index > 0);
    debug_assert!(i_index >= 0 && i_index < p.n_index);
    let i_base = (i_langid as i64 * p.n_index as i64 + i_index as i64) * FTS3_SEGDIR_MAXLEVEL as i64;
    i_base + i_level as i64
}

/// `sqlite3Fts3AllSegdirs`: o comando que percorre as linhas de `%_segdir` (de um nível relativo,
/// ou de todos se `i_level<0`), da mais velha para a mais nova. Colunas: `idx`, `start_block`,
/// `leaves_end_block`, `end_block`, `root`.
pub fn fts3_all_segdirs(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
) -> Result<StmtId, i32> {
    debug_assert!(i_level == FTS3_SEGCURSOR_ALL || i_level >= 0);
    debug_assert!(i_level < FTS3_SEGDIR_MAXLEVEL);
    debug_assert!(i_index >= 0 && i_index < p.n_index);

    if i_level < 0 {
        /* "SELECT * FROM %_segdir WHERE level BETWEEN ? AND ? ORDER BY ..." */
        let st = fts3_sql_stmt(db, p, SQL_SELECT_LEVEL_RANGE, &[])?;
        bind_int64(db, st, 1, get_absolute_level(p, i_langid, i_index, 0));
        bind_int64(
            db,
            st,
            2,
            get_absolute_level(p, i_langid, i_index, FTS3_SEGDIR_MAXLEVEL - 1),
        );
        Ok(st)
    } else {
        /* "SELECT * FROM %_segdir WHERE level = ? ORDER BY ..." */
        let st = fts3_sql_stmt(db, p, SQL_SELECT_LEVEL, &[])?;
        bind_int64(db, st, 1, get_absolute_level(p, i_langid, i_index, i_level));
        Ok(st)
    }
}

// ---------------------------------------------------------------------------------------------
// Doclists pendentes
// ---------------------------------------------------------------------------------------------

/// `fts3PendingListAppendVarint`: acrescenta um varint a uma `PendingList`.
fn pending_list_append_varint(p: &mut PendingList, i: i64) {
    let mut buf = [0u8; FTS3_VARINT_MAX];
    let n = fts3_put_varint(&mut buf, i) as usize;
    p.a_data.extend_from_slice(&buf[..n]);
}

/// `fts3PendingListAppend`: acrescenta uma entrada docid/coluna/posição. `is_new` é o `*pp==NULL`
/// do C: `p` é a lista recém-criada (zerada).
fn pending_list_append(p: &mut PendingList, is_new: bool, i_docid: i64, i_col: i64, i_pos: i64) {
    debug_assert!(is_new || p.i_last_docid <= i_docid);

    if is_new || p.i_last_docid != i_docid {
        let i_delta = (i_docid as u64).wrapping_sub(p.i_last_docid as u64);
        if !is_new {
            /* O `'\0'` de `aData[nData]` passa a fazer parte da lista (POS_END do docid anterior). */
            p.a_data.push(0);
        }
        pending_list_append_varint(p, i_delta as i64);
        p.i_last_col = -1;
        p.i_last_pos = 0;
        p.i_last_docid = i_docid;
    }
    if i_col > 0 && p.i_last_col != i_col {
        pending_list_append_varint(p, 1);
        pending_list_append_varint(p, i_col);
        p.i_last_col = i_col;
        p.i_last_pos = 0;
    }
    if i_col >= 0 {
        debug_assert!(i_pos > p.i_last_pos || (i_pos == 0 && p.i_last_pos == 0));
        pending_list_append_varint(p, 2i64.wrapping_add(i_pos).wrapping_sub(p.i_last_pos));
        p.i_last_pos = i_pos;
    }
}

/// `fts3PendingListAppend` sobre um `Option<PendingList>` (a lista de um token adiado).
pub(super) fn pending_list_append_opt(
    pp: &mut Option<PendingList>,
    i_docid: i64,
    i_col: i64,
    i_pos: i64,
) {
    let is_new = pp.is_none();
    let p = pp.get_or_insert_with(PendingList::default);
    pending_list_append(p, is_new, i_docid, i_col, i_pos);
}

/// `fts3PendingListAppendVarint` sobre um `Option<PendingList>` (cria a lista se não existe).
pub(super) fn pending_list_append_varint_opt(pp: &mut Option<PendingList>, i: i64) {
    let p = pp.get_or_insert_with(PendingList::default);
    pending_list_append_varint(p, i);
}

/// `fts3PendingTermsAddOne`: acrescenta uma entrada a uma das tabelas de termos pendentes.
fn fts3_pending_terms_add_one(
    p: &mut Fts3Table,
    i_col: i32,
    i_pos: i32,
    i_index: usize,
    z_token: &[u8],
) {
    let n_token = z_token.len() as i32;
    let i_docid = p.i_prev_docid;
    let h_pending = &mut p.a_index[i_index].h_pending;

    if let Some(p_list) = h_pending.find_mut(z_token) {
        p.n_pending_data -= p_list.n_data() + n_token + FTS3_HASH_ELEM_SIZE;
        pending_list_append(p_list, false, i_docid, i_col as i64, i_pos as i64);
        p.n_pending_data += p_list.n_data() + n_token + FTS3_HASH_ELEM_SIZE;
    } else {
        let mut p_list = PendingList::default();
        pending_list_append(&mut p_list, true, i_docid, i_col as i64, i_pos as i64);
        p.n_pending_data += p_list.n_data() + n_token + FTS3_HASH_ELEM_SIZE;
        h_pending.insert(z_token, Some(p_list));
    }
}

/// `fts3PendingTermsAdd`: tokeniza `z_text` e acrescenta os tokens às tabelas de termos pendentes.
/// O docid é o de `p.i_prev_docid` e a coluna é `i_col`.
pub(super) fn fts3_pending_terms_add(
    p: &mut Fts3Table,
    i_langid: i32,
    z_text: Option<&[u8]>,
    i_col: i32,
    pn_word: &mut u32,
) -> i32 {
    /* Se o usuário inseriu NULL a função é chamada sem texto: nenhum termo entra. */
    let Some(z_text) = z_text else {
        *pn_word = 0;
        return SQLITE_OK;
    };
    let Some(tokenizer) = p.p_tokenizer.clone() else {
        return SQLITE_ERROR;
    };

    let mut p_csr = match fts3_open_tokenizer(&*tokenizer, i_langid, fts3_c_str(z_text)) {
        Ok(c) => c,
        Err(rc) => return rc,
    };

    let mut n_word: i32 = 0;
    let mut rc = SQLITE_OK;
    loop {
        let tok = match p_csr.next() {
            Ok(t) => t,
            Err(e) => {
                rc = e;
                break;
            }
        };
        if tok.i_position >= n_word {
            n_word = tok.i_position + 1;
        }

        /* As posições não são negativas (o -1 é um terminador interno) e os tokens têm comprimento
        ** diferente de zero. */
        if tok.i_position < 0 || tok.z.is_empty() {
            rc = SQLITE_ERROR;
            break;
        }

        /* Acrescenta o termo ao índice de termos. */
        fts3_pending_terms_add_one(p, i_col, tok.i_position, 0, tok.z);

        /* Acrescenta o termo a cada índice de prefixo para o qual ele não é curto demais. */
        let mut i = 1;
        while i < p.n_index {
            let n_prefix = p.a_index[i as usize].n_prefix;
            if (tok.z.len() as i32) >= n_prefix {
                fts3_pending_terms_add_one(
                    p,
                    i_col,
                    tok.i_position,
                    i as usize,
                    &tok.z[..n_prefix as usize],
                );
            }
            i += 1;
        }
    }

    *pn_word = pn_word.wrapping_add(n_word as u32);
    if rc == SQLITE_DONE {
        SQLITE_OK
    } else {
        rc
    }
}

/// `fts3PendingTermsDocid`: as chamadas seguintes de `fts3_pending_terms_add` acrescentam os
/// pares termo/lista de posições do documento `i_docid`.
pub(super) fn fts3_pending_terms_docid(
    db: &mut Connection,
    p: &mut Fts3Table,
    b_delete: bool,
    i_langid: i32,
    i_docid: i64,
) -> i32 {
    debug_assert!(i_langid >= 0);

    if i_docid < p.i_prev_docid
        || (i_docid == p.i_prev_docid && !p.b_prev_delete)
        || p.i_prev_langid != i_langid
        || p.n_pending_data > p.n_max_pending_data
    {
        let rc = fts3_pending_terms_flush(db, p);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    p.i_prev_docid = i_docid;
    p.i_prev_langid = i_langid;
    p.b_prev_delete = b_delete;
    SQLITE_OK
}

/// `sqlite3Fts3PendingTermsClear`: descarta as tabelas de termos pendentes.
pub fn fts3_pending_terms_clear(p: &mut Fts3Table) {
    for index in p.a_index.iter_mut() {
        index.h_pending.clear();
    }
    p.n_pending_data = 0;
}

/// `fts3InsertTerms`: acrescenta aos termos pendentes os termos do novo registro. `a_sz` recebe o
/// tamanho de cada coluna (e o total em `a_sz[nColumn]`).
pub(super) fn fts3_insert_terms(
    p: &mut Fts3Table,
    i_langid: i32,
    ap_val: &[Mem],
    a_sz: &mut [u32],
) -> i32 {
    let n_column = p.n_column() as usize;
    for i in 2..n_column + 2 {
        let i_col = i - 2;
        if at(&p.ab_notindexed, i_col) == 0 {
            let z_text = text_of(&ap_val[i]);
            let rc = fts3_pending_terms_add(
                p,
                i_langid,
                z_text.as_deref(),
                i_col as i32,
                &mut a_sz[i_col],
            );
            if rc != SQLITE_OK {
                return rc;
            }
            let n_bytes = z_text.as_ref().map_or(0, |z| z.len()) as u32;
            a_sz[n_column] = a_sz[n_column].wrapping_add(n_bytes);
        }
    }
    SQLITE_OK
}

/// `fts3InsertData`: insere o registro em `%_content`. `ap_val` é o do `xUpdate` (`apVal[0]` não
/// serve para `INSERT`, `apVal[1]` é o rowid, `apVal[2..]` as colunas, depois a coluna oculta com
/// o nome da tabela, `docid` e `languageid`). `*pi_docid` recebe o docid da linha.
pub(super) fn fts3_insert_data(
    db: &mut Connection,
    p: &mut Fts3Table,
    ap_val: &[Mem],
    pi_docid: &mut i64,
) -> i32 {
    let n_column = p.n_column() as usize;

    if p.z_content_tbl.is_some() {
        let mut p_rowid = &ap_val[n_column + 3];
        if value_type(p_rowid) == SQLITE_NULL {
            p_rowid = &ap_val[1];
        }
        if value_type(p_rowid) != SQLITE_INTEGER {
            return SQLITE_CONSTRAINT;
        }
        *pi_docid = value_int64(p_rowid);
        return SQLITE_OK;
    }

    /* O comando é `INSERT INTO %_content VALUES(?, ?, ?, ...)`, com um `?` por coluna do usuário
    ** mais um para o docid. */
    let p_content_insert = match fts3_sql_stmt(db, p, SQL_CONTENT_INSERT, &ap_val[1..]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };
    if p.z_languageid.is_some() {
        let rc = bind_int(db, p_content_insert, n_column as i32 + 2, value_int(&ap_val[n_column + 4]));
        if rc != SQLITE_OK {
            return rc;
        }
    }

    /* O INSERT do usuário pode ter dado um valor para "rowid", para "docid" ou para os dois, e
    ** eles são apelidos do mesmo valor. No FTS3 é erro dar valores não NULL aos dois. */
    if value_type(&ap_val[3 + n_column]) != SQLITE_NULL {
        if value_type(&ap_val[0]) == SQLITE_NULL && value_type(&ap_val[1]) != SQLITE_NULL {
            /* Conflito entre rowid e docid. */
            return SQLITE_ERROR;
        }
        let rc = bind_value(db, p_content_insert, 1, &ap_val[3 + n_column]);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    step(db, p_content_insert);
    let rc = reset(db, p_content_insert);

    *pi_docid = db.last_rowid;
    rc
}

/// `fts3DeleteAll`: apaga todos os dados da tabela. `b_content` falso deixa `%_content` como está.
pub(super) fn fts3_delete_all(db: &mut Connection, p: &mut Fts3Table, b_content: bool) -> i32 {
    let mut rc = SQLITE_OK;

    /* Descarta o conteúdo da tabela de termos pendentes. */
    fts3_pending_terms_clear(p);

    debug_assert!(p.z_content_tbl.is_none() || !b_content);
    if b_content {
        fts3_sql_exec(&mut rc, db, p, SQL_DELETE_ALL_CONTENT, &[]);
    }
    fts3_sql_exec(&mut rc, db, p, SQL_DELETE_ALL_SEGMENTS, &[]);
    fts3_sql_exec(&mut rc, db, p, SQL_DELETE_ALL_SEGDIR, &[]);
    if p.b_has_docsize {
        fts3_sql_exec(&mut rc, db, p, SQL_DELETE_ALL_DOCSIZE, &[]);
    }
    if p.b_has_stat != 0 {
        fts3_sql_exec(&mut rc, db, p, SQL_DELETE_ALL_STAT, &[]);
    }
    rc
}

/// `langidFromSelect`: o `languageid` da linha corrente de `p_select`.
pub(super) fn langid_from_select(db: &mut Connection, p: &Fts3Table, p_select: StmtId) -> i32 {
    if p.z_languageid.is_some() {
        column_int(db, p_select, p.n_column() + 1)
    } else {
        0
    }
}

/// `fts3DeleteTerms`: o docid de `p_rowid` é de uma linha prestes a ser apagada; tira os termos
/// dela do índice. `*pb_found` fica verdadeiro se a linha existe.
pub(super) fn fts3_delete_terms(
    p_rc: &mut i32,
    db: &mut Connection,
    p: &mut Fts3Table,
    p_rowid: &Mem,
    a_sz: &mut [u32],
    pb_found: &mut bool,
) {
    debug_assert!(!*pb_found);
    if *p_rc != SQLITE_OK {
        return;
    }
    match fts3_sql_stmt(db, p, SQL_SELECT_CONTENT_BY_ROWID, std::slice::from_ref(p_rowid)) {
        Ok(p_select) => {
            if step(db, p_select) == SQLITE_ROW {
                let n_column = p.n_column();
                let i_langid = langid_from_select(db, p, p_select);
                let i_docid = column_int64(db, p_select, 0);
                let mut rc = fts3_pending_terms_docid(db, p, true, i_langid, i_docid);
                let mut i = 1;
                while rc == SQLITE_OK && i <= n_column {
                    let i_col = (i - 1) as usize;
                    if at(&p.ab_notindexed, i_col) == 0 {
                        let z_text = column_text(db, p_select, i).map(|s| s.to_vec());
                        rc = fts3_pending_terms_add(
                            p,
                            i_langid,
                            z_text.as_deref(),
                            -1,
                            &mut a_sz[i_col],
                        );
                        let n_bytes = column_bytes(db, p_select, i) as u32;
                        a_sz[n_column as usize] = a_sz[n_column as usize].wrapping_add(n_bytes);
                    }
                    i += 1;
                }
                if rc != SQLITE_OK {
                    reset(db, p_select);
                    *p_rc = rc;
                    return;
                }
                *pb_found = true;
            }
            *p_rc = reset(db, p_select);
        }
        Err(rc) => {
            *p_rc = rc;
        }
    }
}

/// `fts3AllocateSegdirIdx`: aloca um índice novo no nível `i_level` de `%_segdir`. Se o nível já
/// tem `FTS3_MERGE_COUNT` segmentos, eles se fundem num segmento do nível seguinte e o índice
/// alocado é 0.
fn fts3_allocate_segdir_idx(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
    pi_idx: &mut i32,
) -> i32 {
    debug_assert!(i_langid >= 0);
    debug_assert!(p.n_index >= 1);

    let mut i_next = 0;
    let mut rc;

    /* `i_next` é o próximo índice livre do nível `i_level`. */
    match fts3_sql_stmt(db, p, SQL_NEXT_SEGMENT_INDEX, &[]) {
        Ok(p_next_idx) => {
            bind_int64(db, p_next_idx, 1, get_absolute_level(p, i_langid, i_index, i_level));
            if step(db, p_next_idx) == SQLITE_ROW {
                i_next = column_int(db, p_next_idx, 0);
            }
            rc = reset(db, p_next_idx);
        }
        Err(e) => rc = e,
    }

    if rc == SQLITE_OK {
        /* Com `FTS3_MERGE_COUNT` segmentos no nível, fundem-se todos num do nível seguinte e o
        ** índice 0 do nível fica livre. */
        if i_next >= FTS3_MERGE_COUNT {
            rc = fts3_segment_merge(db, p, i_langid, i_index, i_level);
            *pi_idx = 0;
        } else {
            *pi_idx = i_next;
        }
    }

    rc
}

// ---------------------------------------------------------------------------------------------
// Leitura de blocos de %_segments
// ---------------------------------------------------------------------------------------------

/// O trabalho de `sqlite3_blob_open`/`sqlite3_blob_reopen`: posiciona o comando de `%_segments` na
/// linha `i_blockid`. `SQLITE_ERROR` é o que o C recebe quando a linha não existe ou a coluna não
/// é BLOB nem TEXT.
fn fts3_blob_seek(db: &mut Connection, st: StmtId, i_blockid: i64) -> i32 {
    reset(db, st);
    bind_int64(db, st, 1, i_blockid);
    let rc = step(db, st);
    if rc == SQLITE_ROW {
        let e_type = column_type(db, st, 0);
        if e_type == SQLITE_BLOB || e_type == SQLITE_TEXT {
            SQLITE_OK
        } else {
            SQLITE_ERROR
        }
    } else if rc == SQLITE_DONE {
        SQLITE_ERROR
    } else {
        reset(db, st)
    }
}

/// `sqlite3Fts3ReadBlock`: lê a linha `i_blockid` de `%_segments`. Devolve os bytes do bloco
/// (com [`FTS3_NODE_PADDING`] de zeros depois, só se `b_blob`) e o tamanho do bloco. O blob
/// (comando) aberto fica em `p.p_segments` para a chamada seguinte; [`fts3_segments_close`] o fecha
/// e todo método da tabela virtual que pode chegar aqui o chama antes de voltar.
pub fn fts3_read_block(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_blockid: i64,
    b_blob: bool,
) -> Result<(Option<Vec<u8>>, i32), i32> {
    let mut rc;

    if let Some(st) = p.p_segments {
        rc = fts3_blob_seek(db, st, i_blockid);
    } else {
        if p.z_segments_tbl.is_none() {
            let mut z = p.z_name.clone();
            z.extend_from_slice(b"_segments");
            p.z_segments_tbl = Some(z);
        }
        let z_sql = mprintf(
            b"SELECT block FROM \"%w\".\"%w\" WHERE blockid=?1",
            &[
                text_arg(&p.z_db),
                PrintfArg::Text(p.z_segments_tbl.clone()),
            ],
        );
        match z_sql {
            None => return Err(SQLITE_NOMEM),
            Some(z_sql) => {
                let (rc2, stmt, _) =
                    prepare_v3(db, &z_sql, -1, SQLITE_PREPARE_PERSISTENT | SQLITE_PREPARE_NO_VTAB);
                rc = rc2;
                if rc == SQLITE_OK {
                    if let Some(st) = stmt {
                        rc = fts3_blob_seek(db, st, i_blockid);
                        p.p_segments = Some(st);
                    }
                }
            }
        }
    }

    if rc != SQLITE_OK {
        /* O blob que falhou ao abrir ou reabrir se fecha. */
        if let Some(st) = p.p_segments.take() {
            finalize(db, st);
        }
        if rc == SQLITE_ERROR {
            rc = FTS_CORRUPT_VTAB;
        }
        return Err(rc);
    }

    let st = p.p_segments.unwrap_or_default();
    let n_byte = column_bytes(db, st, 0);
    if !b_blob {
        return Ok((None, n_byte));
    }
    let mut a_byte = vec![0u8; n_byte.max(0) as usize + FTS3_NODE_PADDING];
    if let Some(b) = column_blob(db, st, 0) {
        let n = b.len().min(n_byte.max(0) as usize);
        a_byte[..n].copy_from_slice(&b[..n]);
    }
    Ok((Some(a_byte), n_byte))
}

/// `sqlite3Fts3SegmentsClose`: fecha o blob de `p.p_segments`, se aberto.
pub fn fts3_segments_close(db: &mut Connection, p: &mut Fts3Table) {
    if let Some(st) = p.p_segments.take() {
        finalize(db, st);
    }
}

// ---------------------------------------------------------------------------------------------
// Leitores de segmentos
// ---------------------------------------------------------------------------------------------

/// `fts3SegReaderSetEof`: põe o leitor no fim.
fn fts3_seg_reader_set_eof(p_seg: &mut Fts3SegReader) {
    p_seg.a_node = None;
}

/// `fts3SegReaderNext`: avança o leitor para o termo seguinte do segmento. `SQLITE_OK` se há um
/// próximo termo (ou se o leitor chegou ao fim: `a_node` fica `None`), ou um código de erro.
fn fts3_seg_reader_next(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_reader: &mut Fts3SegReader,
) -> i32 {
    /* `pNext`: o deslocamento do próximo termo em `a_node` e se já se passou do fim do nó. */
    let n_node = p_reader.n_node as i64;
    let (mut p_next, at_end) = match (&p_reader.a_node, p_reader.a_doclist) {
        (None, _) => (0usize, true),
        (Some(_), None) => (0usize, n_node <= 0),
        (Some(_), Some(d)) => {
            let n = d + p_reader.n_doclist.max(0) as usize;
            (n, n as i64 >= n_node)
        }
    };

    if at_end {
        if p_reader.pending.is_some() {
            p_reader.a_node = None;
            let (i_index, p_elem) = match p_reader.pending.as_ref() {
                Some(it) => (it.i_index, it.elems.get(it.i_next).copied()),
                None => (0, None),
            };
            if let Some(p_elem) = p_elem {
                let h_pending = &p.a_index[i_index].h_pending;
                p_reader.z_term = h_pending.key(p_elem).to_vec();
                let mut a_copy = h_pending.data(p_elem).a_data.clone();
                a_copy.push(0);
                p_reader.n_node = a_copy.len() as i32;
                p_reader.n_doclist = p_reader.n_node;
                p_reader.a_node = Some(a_copy);
                p_reader.a_doclist = Some(0);
                if let Some(it) = p_reader.pending.as_mut() {
                    it.i_next += 1;
                }
            }
            return SQLITE_OK;
        }

        fts3_seg_reader_set_eof(p_reader);

        /* Com `iCurrentBlock>=iLeafEndBlock` é o fim: todas as folhas já foram percorridas. */
        if p_reader.i_current_block >= p_reader.i_leaf_end_block {
            return SQLITE_OK;
        }

        p_reader.i_current_block += 1;
        match fts3_read_block(db, p, p_reader.i_current_block, true) {
            Ok((a_node, n_node)) => {
                p_reader.a_node = a_node;
                p_reader.n_node = n_node;
            }
            Err(rc) => return rc,
        }
        p_next = 0;
    }

    debug_assert!(!p_reader.is_pending());

    let n_node = p_reader.n_node as i64;
    let a_node = p_reader.a_node.as_deref().unwrap_or(&[]);

    /* Por causa do enchimento de `FTS3_NODE_PADDING` os dois varints se leem sem risco de passar
    ** do fim, mesmo com o nó corrompido. */
    let (n, n_prefix) = fts3_get_varint32(sl(a_node, p_next));
    p_next += n as usize;
    let (n, n_suffix) = fts3_get_varint32(sl(a_node, p_next));
    p_next += n as usize;
    if n_suffix <= 0
        || (n_node - p_next as i64) < n_suffix as i64
        || n_prefix as usize > p_reader.z_term.len()
    {
        return FTS_CORRUPT_VTAB;
    }

    p_reader.z_term.truncate(n_prefix as usize);
    p_reader
        .z_term
        .extend_from_slice(&sl(a_node, p_next)[..(n_suffix as usize).min(sl(a_node, p_next).len())]);
    p_next += n_suffix as usize;
    let (n, n_doclist) = fts3_get_varint32(sl(a_node, p_next));
    p_next += n as usize;
    p_reader.a_doclist = Some(p_next);
    p_reader.n_doclist = n_doclist;
    p_reader.p_offset_list = None;

    /* A doclist não pode passar do fim do nó e o último byte dela é 0x00; senão os dados estão
    ** corrompidos. */
    if n_doclist as i64 > n_node - p_next as i64
        || n_doclist == 0
        || at(a_node, p_next + n_doclist as usize - 1) != 0
    {
        return FTS_CORRUPT_VTAB;
    }
    SQLITE_OK
}

/// `fts3SegReaderFirstDocid`: põe o leitor no primeiro docid da doclist do termo corrente.
fn fts3_seg_reader_first_docid(p_tab: &Fts3Table, p_reader: &mut Fts3SegReader) {
    let d = p_reader.a_doclist.unwrap_or(0);
    debug_assert!(p_reader.a_doclist.is_some());
    debug_assert!(p_reader.p_offset_list.is_none());
    if p_tab.b_desc_idx && p_reader.is_pending() {
        let mut b_eof = false;
        let mut i_docid = 0i64;
        let mut n_offset_list = 0i32;
        let mut p_iter: Option<usize> = None;
        {
            let a_node = p_reader.a_node.as_deref().unwrap_or(&[]);
            let end = (d + p_reader.n_doclist.max(0) as usize).min(a_node.len());
            let a_doclist = &a_node[d.min(end)..end];
            fts3_doclist_prev(
                false,
                a_doclist,
                &mut p_iter,
                &mut i_docid,
                &mut n_offset_list,
                &mut b_eof,
            );
        }
        p_reader.i_docid = i_docid;
        p_reader.n_offset_list = n_offset_list;
        p_reader.p_offset_list = p_iter.map(|i| i + d);
    } else {
        let a_node = p_reader.a_node.as_deref().unwrap_or(&[]);
        let (n, v) = fts3_get_varint(sl(a_node, d));
        p_reader.i_docid = v;
        p_reader.p_offset_list = Some(d + n as usize);
    }
}

/// `fts3SegReaderNextDocid`: avança o leitor para o docid seguinte da doclist do termo corrente.
/// Devolve o deslocamento (em `a_node`) e o tamanho, sem o terminador, da lista de posições da
/// entrada que acabou de sair.
fn fts3_seg_reader_next_docid(p_tab: &Fts3Table, p_reader: &mut Fts3SegReader) -> (usize, i32) {
    let p0 = p_reader.p_offset_list.unwrap_or(0);
    debug_assert!(p_reader.p_offset_list.is_some());

    if p_tab.b_desc_idx && p_reader.is_pending() {
        /* Um leitor de termos pendentes de uma tabela FTS4 com `order=desc`. As doclists pendentes
        ** são sempre montadas em ordem crescente, então aqui se percorrem de trás para a frente. */
        let d = p_reader.a_doclist.unwrap_or(0);
        let out = (p0, p_reader.n_offset_list - 1);
        let mut b_eof = false;
        let mut i_docid = p_reader.i_docid;
        let mut n_offset_list = p_reader.n_offset_list;
        let mut p_iter = Some(p0 - d);
        {
            let a_node = p_reader.a_node.as_deref().unwrap_or(&[]);
            let end = (d + p_reader.n_doclist.max(0) as usize).min(a_node.len());
            let a_doclist = &a_node[d.min(end)..end];
            fts3_doclist_prev(
                false,
                a_doclist,
                &mut p_iter,
                &mut i_docid,
                &mut n_offset_list,
                &mut b_eof,
            );
        }
        p_reader.i_docid = i_docid;
        p_reader.n_offset_list = n_offset_list;
        p_reader.p_offset_list = if b_eof { None } else { p_iter.map(|i| i + d) };
        out
    } else {
        let d = p_reader.a_doclist.unwrap_or(0);
        let p_end = d + p_reader.n_doclist.max(0) as usize;
        let a_node = p_reader.a_node.as_deref().unwrap_or(&[]);
        let mut p = p0;
        let mut c: u8 = 0;

        /* `p` aponta o primeiro byte de uma lista de offsets; avança até um byte depois do fim da
        ** mesma lista. */
        while (at(a_node, p) | c) != 0 {
            c = at(a_node, p) & 0x80;
            p += 1;
        }
        p += 1;

        let out = (p0, (p - p0) as i32 - 1);

        /* A lista pode ter sido editada no lugar por `fts3EvalNearTrim()`. */
        while p < p_end && at(a_node, p) == 0 {
            p += 1;
        }

        /* Sem mais entradas na doclist, `pOffsetList` é nulo; senão aponta a lista seguinte e
        ** `iDocid` é o docid seguinte. */
        if p >= p_end {
            p_reader.p_offset_list = None;
        } else {
            let (n, i_delta) = fts3_get_varint_u(sl(a_node, p));
            p_reader.p_offset_list = Some(p + n as usize);
            if p_tab.b_desc_idx {
                p_reader.i_docid = (p_reader.i_docid as u64).wrapping_sub(i_delta) as i64;
            } else {
                p_reader.i_docid = (p_reader.i_docid as u64).wrapping_add(i_delta) as i64;
            }
        }
        out
    }
}

/// `sqlite3Fts3MsrOvfl`: soma ao `Ok` o número de páginas de estouro que as folhas dos segmentos de
/// `p_msr` ocupariam no arquivo.
pub fn fts3_msr_ovfl(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_msr: &Fts3MultiSegReader,
) -> Result<i32, i32> {
    let mut n_ovfl: i32 = 0;
    let pgsz = p.n_pgsz;

    debug_assert!(p.b_fts4);
    debug_assert!(pgsz > 0);

    for p_reader in p_msr.ap_segment.iter() {
        if !p_reader.is_pending() && !p_reader.root_only {
            let mut jj = p_reader.i_start_block;
            while jj <= p_reader.i_leaf_end_block {
                let (_, n_blob) = fts3_read_block(db, p, jj, false)?;
                if (n_blob + 35) > pgsz {
                    n_ovfl += (n_blob + 34).checked_div(pgsz).unwrap_or(0);
                }
                jj += 1;
            }
        }
    }
    Ok(n_ovfl)
}

/// `sqlite3Fts3SegReaderFree`: libera o leitor (sem o `db` o `Drop` basta: não há blob de nó).
pub fn fts3_seg_reader_free(_db: &mut Connection, p_reader: Fts3SegReader) {
    drop(p_reader);
}

/// `sqlite3Fts3SegReaderNew`: um leitor novo. `z_root` é o nó raiz (copiado se o segmento cabe nele).
pub fn fts3_seg_reader_new(
    i_age: i32,
    b_lookup: bool,
    i_start_leaf: i64,
    i_end_leaf: i64,
    i_end_block: i64,
    z_root: Option<&[u8]>,
) -> Result<Fts3SegReader, i32> {
    let mut p_reader = Fts3SegReader {
        i_idx: i_age,
        b_lookup,
        i_start_block: i_start_leaf,
        i_leaf_end_block: i_end_leaf,
        i_end_block,
        ..Default::default()
    };

    if i_start_leaf == 0 {
        if i_end_leaf != 0 {
            return Err(FTS_CORRUPT_VTAB);
        }
        /* O segmento inteiro está no nó raiz. */
        let z_root = z_root.unwrap_or(&[]);
        let mut a_node = vec![0u8; z_root.len() + FTS3_NODE_PADDING];
        a_node[..z_root.len()].copy_from_slice(z_root);
        p_reader.a_node = Some(a_node);
        p_reader.root_only = true;
        p_reader.n_node = z_root.len() as i32;
    } else {
        p_reader.i_current_block = i_start_leaf - 1;
    }
    Ok(p_reader)
}

/// `sqlite3Fts3SegReaderPending`: um leitor que percorre um subconjunto dos termos pendentes do
/// índice `i_index`. Sem `b_prefix` visita os termos que casam exatamente com `z_term`; com ele,
/// todos os termos que começam com `z_term` (todos, se `z_term` é vazio), em ordem de termo.
pub fn fts3_seg_reader_pending(
    p: &Fts3Table,
    i_index: usize,
    z_term: &[u8],
    b_prefix: bool,
) -> Result<Option<Fts3SegReader>, i32> {
    let p_hash = &p.a_index[i_index].h_pending;
    let mut a_elem: Vec<HashElemId> = Vec::new();

    if b_prefix {
        let mut p_e = p_hash.first();
        while let Some(e) = p_e {
            let z_key = p_hash.key(e);
            if z_term.is_empty() || (z_key.len() >= z_term.len() && z_key[..z_term.len()] == *z_term)
            {
                a_elem.push(e);
            }
            p_e = p_hash.next(e);
        }

        /* Com mais de um termo casando, ordena por termo (a mesma comparação que o `flush`). */
        if a_elem.len() > 1 {
            a_elem.sort_by(|l, r| p_hash.key(*l).cmp(p_hash.key(*r)));
        }
    } else {
        /* A busca de um termo casa no máximo um termo do índice: basta consultar o hash. */
        if let Some(e) = p_hash.find_elem(z_term) {
            a_elem.push(e);
        }
    }

    if a_elem.is_empty() {
        return Ok(None);
    }
    Ok(Some(Fts3SegReader {
        i_idx: 0x7FFF_FFFF,
        pending: Some(PendingIter { i_index, elems: a_elem, i_next: 0 }),
        ..Default::default()
    }))
}

/// `fts3SegReaderCmp`: o fim é maior que o não fim; depois comparam-se os termos (o mais longo de
/// dois com o mesmo prefixo é o maior); depois a idade (o segmento mais velho é o maior).
fn fts3_seg_reader_cmp(p_lhs: &Fts3SegReader, p_rhs: &Fts3SegReader) -> i32 {
    let mut rc;
    if p_lhs.a_node.is_some() && p_rhs.a_node.is_some() {
        let n_l = p_lhs.z_term.len();
        let n_r = p_rhs.z_term.len();
        let rc2 = n_l as i32 - n_r as i32;
        if rc2 < 0 {
            rc = memcmp(&p_lhs.z_term[..n_l], &p_rhs.z_term[..n_l]);
        } else {
            rc = memcmp(&p_lhs.z_term[..n_r], &p_rhs.z_term[..n_r]);
        }
        if rc == 0 {
            rc = rc2;
        }
    } else {
        rc = (p_lhs.a_node.is_none() as i32) - (p_rhs.a_node.is_none() as i32);
    }
    if rc == 0 {
        rc = p_rhs.i_idx.wrapping_sub(p_lhs.i_idx);
    }
    rc
}

/// `fts3SegReaderDoclistCmp`: cada leitor aponta para uma entrada de uma doclist de termos iguais.
/// O fim é maior; depois o docid corrente; depois a idade (o mais velho é o maior).
fn fts3_seg_reader_doclist_cmp(p_lhs: &Fts3SegReader, p_rhs: &Fts3SegReader) -> i32 {
    let mut rc = (p_lhs.p_offset_list.is_none() as i32) - (p_rhs.p_offset_list.is_none() as i32);
    if rc == 0 {
        if p_lhs.i_docid == p_rhs.i_docid {
            rc = p_rhs.i_idx.wrapping_sub(p_lhs.i_idx);
        } else {
            rc = if p_lhs.i_docid > p_rhs.i_docid { 1 } else { -1 };
        }
    }
    rc
}

/// `fts3SegReaderDoclistCmpRev`: como a anterior, com os docids em ordem decrescente.
fn fts3_seg_reader_doclist_cmp_rev(p_lhs: &Fts3SegReader, p_rhs: &Fts3SegReader) -> i32 {
    let mut rc = (p_lhs.p_offset_list.is_none() as i32) - (p_rhs.p_offset_list.is_none() as i32);
    if rc == 0 {
        if p_lhs.i_docid == p_rhs.i_docid {
            rc = p_rhs.i_idx.wrapping_sub(p_lhs.i_idx);
        } else {
            rc = if p_lhs.i_docid < p_rhs.i_docid { 1 } else { -1 };
        }
    }
    rc
}

/// `fts3SegReaderTermCmp`: compara o termo do leitor com `z_term`. No fim devolve 0.
fn fts3_seg_reader_term_cmp(p_seg: &Fts3SegReader, z_term: &[u8]) -> i32 {
    let mut res = 0;
    if p_seg.a_node.is_some() {
        let n_seg = p_seg.z_term.len();
        if n_seg > z_term.len() {
            res = memcmp(&p_seg.z_term[..z_term.len()], z_term);
        } else {
            res = memcmp(&p_seg.z_term[..n_seg], &z_term[..n_seg]);
        }
        if res == 0 {
            res = n_seg as i32 - z_term.len() as i32;
        }
    }
    res
}

/// `fts3SegReaderSort`: os últimos `n_segment - n_suspect` elementos de `ap_segment` (os `n_segment`
/// primeiros) já estão em ordem; embaralha o vetor até tudo ficar ordenado.
fn fts3_seg_reader_sort(
    ap_segment: &mut [Fts3SegReader],
    n_segment: usize,
    n_suspect: usize,
    x_cmp: fn(&Fts3SegReader, &Fts3SegReader) -> i32,
) {
    let ap_segment = &mut ap_segment[..n_segment];
    let mut n_suspect = n_suspect;
    if n_suspect == n_segment {
        n_suspect = n_suspect.saturating_sub(1);
    }
    let mut i = n_suspect as isize - 1;
    while i >= 0 {
        let mut j = i as usize;
        while j + 1 < n_segment {
            if x_cmp(&ap_segment[j], &ap_segment[j + 1]) < 0 {
                break;
            }
            ap_segment.swap(j, j + 1);
            j += 1;
        }
        i -= 1;
    }
}

// ---------------------------------------------------------------------------------------------
// Escrita de segmentos
// ---------------------------------------------------------------------------------------------

/// `fts3WriteSegment`: insere um registro em `%_segments` (`z` `None` grava NULL).
pub(super) fn fts3_write_segment(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_block: i64,
    z: Option<&[u8]>,
) -> i32 {
    match fts3_sql_stmt(db, p, SQL_INSERT_SEGMENTS, &[]) {
        Ok(st) => {
            bind_int64(db, st, 1, i_block);
            bind_blob(db, st, 2, z, z.map_or(0, |z| z.len() as i32), StrDtor::Transient);
            step(db, st);
            let rc = reset(db, st);
            bind_null(db, st, 2);
            rc
        }
        Err(rc) => rc,
    }
}

/// `sqlite3Fts3MaxLevel`: `*pn_max` recebe o maior nível relativo da tabela (0 em erro).
pub fn fts3_max_level(db: &mut Connection, p: &mut Fts3Table, pn_max: &mut i32) -> i32 {
    let mut mx_level = 0;
    let rc = match fts3_sql_stmt(db, p, SQL_SELECT_MXLEVEL, &[]) {
        Ok(st) => {
            if step(db, st) == SQLITE_ROW {
                mx_level = column_int(db, st, 0);
            }
            reset(db, st)
        }
        Err(e) => e,
    };
    *pn_max = mx_level;
    rc
}

/// `fts3WriteSegdir`: insere um registro em `%_segdir`.
pub(super) fn fts3_write_segdir(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_level: i64,
    i_idx: i32,
    i_start_block: i64,
    i_leaf_end_block: i64,
    i_end_block: i64,
    n_leaf_data: i64,
    z_root: &[u8],
) -> i32 {
    match fts3_sql_stmt(db, p, SQL_INSERT_SEGDIR, &[]) {
        Ok(st) => {
            bind_int64(db, st, 1, i_level);
            bind_int(db, st, 2, i_idx);
            bind_int64(db, st, 3, i_start_block);
            bind_int64(db, st, 4, i_leaf_end_block);
            if n_leaf_data == 0 {
                bind_int64(db, st, 5, i_end_block);
            } else {
                let Some(z_end) = mprintf(
                    b"%lld %lld",
                    &[PrintfArg::Int(i_end_block), PrintfArg::Int(n_leaf_data)],
                ) else {
                    return SQLITE_NOMEM;
                };
                bind_text(db, st, 5, Some(&z_end), z_end.len() as i32, StrDtor::Transient);
            }
            bind_blob(db, st, 6, Some(z_root), z_root.len() as i32, StrDtor::Transient);
            step(db, st);
            let rc = reset(db, st);
            bind_null(db, st, 6);
            rc
        }
        Err(rc) => rc,
    }
}

/// `fts3PrefixCompress`: o tamanho do prefixo comum (se houver) de `z_prev` e `z_next`.
pub(super) fn fts3_prefix_compress(z_prev: &[u8], z_next: &[u8]) -> i32 {
    let mut n = 0;
    while n < z_prev.len() && n < z_next.len() && z_prev[n] == z_next[n] {
        n += 1;
    }
    // O `assert_fts3_nc(n<nNext)` do C não existe fora do `SQLITE_DEBUG`: dado corrompido chega
    // aqui legitimamente e quem chama devolve `FTS_CORRUPT_VTAB`.
    n as i32
}

/// `fts3NodeAddTerm`: acrescenta `z_term` ao `SegmentNode`; é garantido que ele é maior (por
/// `memcmp`) que o termo anterior. `pp_tree` é o nó corrente da camada e, na volta, o nó em que o
/// termo entrou (ou o novo nó vazio, irmão à direita, se o corrente estava cheio).
fn fts3_node_add_term(
    n_node_size: i32,
    nodes: &mut Vec<SegmentNode>,
    pp_tree: &mut Option<usize>,
    z_term: &[u8],
) -> i32 {
    let n_term = z_term.len() as i32;

    /* Primeiro tenta acrescentar o termo ao nó corrente. */
    if let Some(t) = *pp_tree {
        let mut n_data = nodes[t].n_data;
        let mut n_req = n_data;

        let n_prefix = fts3_prefix_compress(nodes[t].z_term.as_deref().unwrap_or(&[]), z_term);
        let n_suffix = n_term - n_prefix;

        /* Sem sufixo, `z_term` é prefixo do termo anterior, isto é, é menor ou igual a ele por
        ** `BINARY`: os dados estão corrompidos. */
        if n_suffix <= 0 {
            return FTS_CORRUPT_VTAB;
        }

        n_req += fts3_varint_len(n_prefix as u64) + fts3_varint_len(n_suffix as u64) + n_suffix;
        if n_req <= n_node_size || nodes[t].z_term.is_none() {
            if n_req > n_node_size {
                /* Caso incomum: é o primeiro termo do nó e o buffer estático (`nNodeSize` bytes)
                ** não basta (dois termos com um prefixo de quase 2KB em comum). */
                nodes[t].a_data = vec![0u8; n_req as usize];
            }

            if nodes[t].z_term.is_some() {
                /* O primeiro termo de um nó não tem o campo do tamanho do prefixo. */
                n_data += put_varint_at(&mut nodes[t].a_data, n_data as usize, n_prefix as i64);
            }

            n_data += put_varint_at(&mut nodes[t].a_data, n_data as usize, n_suffix as i64);
            put_bytes_at(&mut nodes[t].a_data, n_data as usize, &z_term[n_prefix as usize..]);
            nodes[t].n_data = n_data + n_suffix;
            nodes[t].n_entry += 1;
            nodes[t].z_term = Some(z_term.to_vec());
            return SQLITE_OK;
        }
    }

    /* Não foi possível acrescentar `z_term` ao nó corrente: cria um irmão à direita. Sendo o
    ** primeiro nó da árvore, o termo vai nele. Senão o termo fica fora do nó novo, que fica
    ** vazio, e vai para o pai do corrente (criado aqui se não existe). */
    let p_new = nodes.len();
    nodes.push(SegmentNode {
        n_data: 1 + FTS3_VARINT_MAX as i32,
        a_data: vec![0u8; n_node_size.max(0) as usize],
        ..Default::default()
    });

    let rc;
    if let Some(t) = *pp_tree {
        let mut p_parent = nodes[t].p_parent;
        rc = fts3_node_add_term(n_node_size, nodes, &mut p_parent, z_term);
        if nodes[t].p_parent.is_none() {
            nodes[t].p_parent = p_parent;
        }
        nodes[t].p_right = Some(p_new);
        nodes[p_new].p_leftmost = nodes[t].p_leftmost;
        nodes[p_new].p_parent = p_parent;
    } else {
        nodes[p_new].p_leftmost = Some(p_new);
        let mut p_tree_new = Some(p_new);
        rc = fts3_node_add_term(n_node_size, nodes, &mut p_tree_new, z_term);
    }

    *pp_tree = Some(p_new);
    rc
}

/// `fts3TreeFinishNode`: escreve a altura e o filho esquerdo no começo dos dados do nó (nos
/// `FTS3_VARINT_MAX+1` bytes que a construção reservou) e devolve onde o nó começa.
fn fts3_tree_finish_node(p_tree: &mut SegmentNode, i_height: i32, i_left_child: i64) -> usize {
    debug_assert!((1..128).contains(&i_height));
    let n_start = FTS3_VARINT_MAX - fts3_varint_len(i_left_child as u64) as usize;
    put_bytes_at(&mut p_tree.a_data, n_start, &[i_height as u8]);
    put_varint_at(&mut p_tree.a_data, n_start + 1, i_left_child);
    n_start
}

/// `fts3NodeWrite`: grava no banco os dados do nó `p_tree` e dos irmãos dele, e chama a si mesma
/// para o pai. Se `p_tree` é a raiz, não grava: devolve o nó raiz. O `Ok` tem o maior blockid
/// gravado (ou zero) e os dados da raiz.
fn fts3_node_write(
    db: &mut Connection,
    p: &mut Fts3Table,
    nodes: &mut Vec<SegmentNode>,
    p_tree: usize,
    i_height: i32,
    i_leaf: i64,
    i_free: i64,
) -> Result<(i64, Vec<u8>), i32> {
    let Some(p_parent) = nodes[p_tree].p_parent else {
        /* Nó raiz da árvore. */
        let n_start = fts3_tree_finish_node(&mut nodes[p_tree], i_height, i_leaf);
        let n_data = nodes[p_tree].n_data as usize;
        let a_root = nodes[p_tree].a_data[n_start..n_data.max(n_start)].to_vec();
        return Ok((i_free - 1, a_root));
    };

    let mut rc = SQLITE_OK;
    let mut i_next_free = i_free;
    let mut i_next_leaf = i_leaf;
    let mut p_iter = nodes[p_tree].p_leftmost;
    while let Some(it) = p_iter {
        if rc != SQLITE_OK {
            break;
        }
        let n_start = fts3_tree_finish_node(&mut nodes[it], i_height, i_next_leaf);
        let n_write = nodes[it].n_data as usize - n_start;

        rc = fts3_write_segment(
            db,
            p,
            i_next_free,
            Some(&nodes[it].a_data[n_start..n_start + n_write]),
        );
        i_next_free += 1;
        i_next_leaf += (nodes[it].n_entry + 1) as i64;
        p_iter = nodes[it].p_right;
    }
    if rc != SQLITE_OK {
        return Err(rc);
    }
    debug_assert!(i_next_leaf == i_free);
    fts3_node_write(db, p, nodes, p_parent, i_height + 1, i_free, i_next_free)
}

/// `fts3SegWriterAdd`: acrescenta um termo ao segmento em construção em `*pp_writer` (que é
/// `None` na primeira chamada: o escritor é criado aqui).
fn fts3_seg_writer_add(
    db: &mut Connection,
    p: &mut Fts3Table,
    pp_writer: &mut Option<SegmentWriter>,
    z_term: &[u8],
    a_doclist: &[u8],
) -> i32 {
    let n_term = z_term.len() as i32;
    let n_doclist = a_doclist.len() as i32;

    if pp_writer.is_none() {
        /* Aloca o escritor e o buffer onde os dados se acumulam. */
        let mut w = SegmentWriter::default();
        w.a_data = vec![0u8; p.n_node_size.max(0) as usize];
        *pp_writer = Some(w);

        /* Procura o próximo blockid livre em `%_segments`. */
        let st = match fts3_sql_stmt(db, p, SQL_NEXT_SEGMENTS_ID, &[]) {
            Ok(st) => st,
            Err(rc) => return rc,
        };
        if step(db, st) == SQLITE_ROW {
            let i_free = column_int64(db, st, 0);
            if let Some(w) = pp_writer.as_mut() {
                w.i_free = i_free;
                w.i_first = i_free;
            }
        }
        let rc = reset(db, st);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    let Some(w) = pp_writer.as_mut() else {
        return SQLITE_ERROR;
    };
    let mut n_data = w.n_data;

    let mut n_prefix = fts3_prefix_compress(&w.z_term, z_term);
    let mut n_suffix = n_term - n_prefix;

    /* Sem sufixo, `z_term` é prefixo do termo anterior: dados corrompidos. */
    if n_suffix <= 0 {
        return FTS_CORRUPT_VTAB;
    }

    /* Quantos bytes a entrada nova pede. */
    let mut n_req: i64 = fts3_varint_len(n_prefix as u64) as i64
        + fts3_varint_len(n_suffix as u64) as i64
        + n_suffix as i64
        + fts3_varint_len(n_doclist as u64) as i64
        + n_doclist as i64;

    if n_data > 0 && n_data as i64 + n_req > p.n_node_size as i64 {
        /* A folha corrente está cheia: grava no banco. */
        if w.i_free == i64::MAX {
            return FTS_CORRUPT_VTAB;
        }
        let i_block = w.i_free;
        w.i_free += 1;
        let rc = fts3_write_segment(db, p, i_block, Some(&w.a_data[..n_data as usize]));
        if rc != SQLITE_OK {
            return rc;
        }
        p.n_leaf_add += 1;

        /* Acrescenta o termo à árvore interna. O termo da árvore interna precisa ser maior que o
        ** maior termo da folha recém-gravada (ainda em `w.z_term`) e menor ou igual ao termo da
        ** folha nova (`z_term`): é o prefixo de `z_term` com um byte a mais que o prefixo comum
        ** dos dois. */
        debug_assert!(n_prefix < n_term);
        let rc = fts3_node_add_term(
            p.n_node_size,
            &mut w.nodes,
            &mut w.p_tree,
            &z_term[..(n_prefix + 1) as usize],
        );
        if rc != SQLITE_OK {
            return rc;
        }

        n_data = 0;
        w.z_term.clear();

        n_prefix = 0;
        n_suffix = n_term;
        n_req = 1
            + fts3_varint_len(n_term as u64) as i64
            + n_term as i64
            + fts3_varint_len(n_doclist as u64) as i64
            + n_doclist as i64;
    }

    /* Soma os bytes da entrada nova ao total gravado. */
    w.n_leaf_data += n_req;

    /* Acrescenta o termo (prefixo comprimido) e a doclist ao buffer. */
    let mut off = n_data as usize;
    off += put_varint_at(&mut w.a_data, off, n_prefix as i64) as usize;
    off += put_varint_at(&mut w.a_data, off, n_suffix as i64) as usize;
    debug_assert!(n_suffix > 0);
    put_bytes_at(&mut w.a_data, off, &z_term[n_prefix as usize..]);
    off += n_suffix as usize;
    off += put_varint_at(&mut w.a_data, off, n_doclist as i64) as usize;
    debug_assert!(n_doclist > 0);
    put_bytes_at(&mut w.a_data, off, a_doclist);
    w.n_data = off as i32 + n_doclist;

    /* Guarda o termo para comprimir o prefixo do próximo. */
    w.z_term = z_term.to_vec();

    SQLITE_OK
}

/// `fts3SegWriterFlush`: grava no banco tudo o que o escritor tem. Chamada depois de todos os
/// termos terem entrado por `fts3_seg_writer_add`.
fn fts3_seg_writer_flush(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_writer: &mut SegmentWriter,
    i_level: i64,
    i_idx: i32,
) -> i32 {
    let rc;
    if let Some(p_tree) = p_writer.p_tree {
        let i_last_leaf = p_writer.i_free;
        let i_block = p_writer.i_free;
        p_writer.i_free += 1;
        let mut r = fts3_write_segment(
            db,
            p,
            i_block,
            Some(&p_writer.a_data[..p_writer.n_data as usize]),
        );
        let mut i_last = 0i64;
        let mut z_root: Vec<u8> = Vec::new();
        if r == SQLITE_OK {
            match fts3_node_write(
                db,
                p,
                &mut p_writer.nodes,
                p_tree,
                1,
                p_writer.i_first,
                p_writer.i_free,
            ) {
                Ok((l, root)) => {
                    i_last = l;
                    z_root = root;
                }
                Err(e) => r = e,
            }
        }
        if r == SQLITE_OK {
            r = fts3_write_segdir(
                db,
                p,
                i_level,
                i_idx,
                p_writer.i_first,
                i_last_leaf,
                i_last,
                p_writer.n_leaf_data,
                &z_root,
            );
        }
        rc = r;
    } else {
        /* A árvore toda cabe no nó raiz: grava em `%_segdir`. */
        rc = fts3_write_segdir(
            db,
            p,
            i_level,
            i_idx,
            0,
            0,
            0,
            p_writer.n_leaf_data,
            &p_writer.a_data[..p_writer.n_data as usize],
        );
    }
    p.n_leaf_add += 1;
    rc
}

/// `fts3IsEmpty`: verdadeiro se, apagado o documento `p_rowid`, a tabela ficaria vazia (há algum
/// documento com docid diferente?). Com `content=xxx` assume-se que nunca fica.
pub(super) fn fts3_is_empty(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_rowid: &Mem,
    pis_empty: &mut i32,
) -> i32 {
    if p.z_content_tbl.is_some() {
        /* Com a opção `content=xxx` assume-se que a tabela nunca está vazia. */
        *pis_empty = 0;
        SQLITE_OK
    } else {
        match fts3_sql_stmt(db, p, SQL_IS_EMPTY, std::slice::from_ref(p_rowid)) {
            Ok(st) => {
                if step(db, st) == SQLITE_ROW {
                    *pis_empty = column_int(db, st, 0);
                }
                reset(db, st)
            }
            Err(rc) => rc,
        }
    }
}

/// `fts3SegmentMaxLevel`: o maior nível de segmento do índice `i_index` do idioma `i_langid`.
fn fts3_segment_max_level(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    pn_max: &mut i64,
) -> i32 {
    debug_assert!(i_index >= 0 && i_index < p.n_index);

    /* `SELECT max(level) FROM %_segdir WHERE level BETWEEN ? AND ?` */
    let st = match fts3_sql_stmt(db, p, SQL_SELECT_SEGDIR_MAX_LEVEL, &[]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };
    bind_int64(db, st, 1, get_absolute_level(p, i_langid, i_index, 0));
    bind_int64(
        db,
        st,
        2,
        get_absolute_level(p, i_langid, i_index, FTS3_SEGDIR_MAXLEVEL - 1),
    );
    if step(db, st) == SQLITE_ROW {
        *pn_max = column_int64(db, st, 0);
    }
    reset(db, st)
}

/// `fts3SegmentIsMaxLevel`: `*pb_max` fica verdadeiro se `i_abs_level` (que existe) é o maior nível
/// do índice dele.
pub(super) fn fts3_segment_is_max_level(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    pb_max: &mut bool,
) -> i32 {
    /* `SELECT max(level) FROM %_segdir WHERE level BETWEEN ? AND ?` */
    let st = match fts3_sql_stmt(db, p, SQL_SELECT_SEGDIR_MAX_LEVEL, &[]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };
    bind_int64(db, st, 1, i_abs_level.wrapping_add(1));
    bind_int64(
        db,
        st,
        2,
        (((i_abs_level as u64) / FTS3_SEGDIR_MAXLEVEL as u64) + 1).wrapping_mul(FTS3_SEGDIR_MAXLEVEL as u64)
            as i64,
    );

    *pb_max = false;
    if step(db, st) == SQLITE_ROW {
        *pb_max = column_type(db, st, 0) == SQLITE_NULL;
    }
    reset(db, st)
}

/// `fts3DeleteSegment`: apaga de `%_segments` as linhas do segmento do leitor `p_seg` (não mexe em
/// `%_segdir`).
pub(super) fn fts3_delete_segment(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_seg: &Fts3SegReader,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p_seg.i_start_block != 0 {
        match fts3_sql_stmt(db, p, SQL_DELETE_SEGMENTS_RANGE, &[]) {
            Ok(p_delete) => {
                bind_int64(db, p_delete, 1, p_seg.i_start_block);
                bind_int64(db, p_delete, 2, p_seg.i_end_block);
                step(db, p_delete);
                rc = reset(db, p_delete);
            }
            Err(e) => rc = e,
        }
    }
    rc
}

/// `fts3DeleteSegdir`: depois de fundir vários segmentos num só, apaga os b-trees antigos: as
/// linhas de `%_segments` de cada leitor de `ap_segment` e as de `%_segdir` do nível `i_level` (ou
/// de todos, com `FTS3_SEGCURSOR_ALL`).
fn fts3_delete_segdir(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
    ap_segment: &[Fts3SegReader],
) -> i32 {
    for seg in ap_segment.iter() {
        let rc = fts3_delete_segment(db, p, seg);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    debug_assert!(i_level >= 0 || i_level == FTS3_SEGCURSOR_ALL);
    let p_delete;
    if i_level == FTS3_SEGCURSOR_ALL {
        match fts3_sql_stmt(db, p, SQL_DELETE_SEGDIR_RANGE, &[]) {
            Ok(st) => {
                bind_int64(db, st, 1, get_absolute_level(p, i_langid, i_index, 0));
                bind_int64(
                    db,
                    st,
                    2,
                    get_absolute_level(p, i_langid, i_index, FTS3_SEGDIR_MAXLEVEL - 1),
                );
                p_delete = st;
            }
            Err(rc) => return rc,
        }
    } else {
        match fts3_sql_stmt(db, p, SQL_DELETE_SEGDIR_LEVEL, &[]) {
            Ok(st) => {
                bind_int64(db, st, 1, get_absolute_level(p, i_langid, i_index, i_level));
                p_delete = st;
            }
            Err(rc) => return rc,
        }
    }

    step(db, p_delete);
    reset(db, p_delete)
}

/// `fts3ColumnFilter`: `buf[p_list..p_list+n_list]` é uma lista de posições que pode ter várias
/// colunas. Devolve o começo e o tamanho do subconjunto da coluna `i_col` (tamanho zero se não há
/// entradas dela). Com `b_zero`, o que vem depois do fim da lista de saída é zerado.
pub(super) fn fts3_column_filter(
    i_col: i32,
    b_zero: bool,
    buf: &mut [u8],
    p_list: usize,
    n_list: i32,
) -> (usize, i32) {
    let mut p_list = p_list;
    let mut n_list = n_list;
    let p_end = p_list + n_list.max(0) as usize;
    let mut i_current = 0i32;
    let mut p = p_list;

    debug_assert!(i_col >= 0);
    loop {
        let mut c: u8 = 0;
        while p < p_end && ((c | at(buf, p)) & 0xFE) != 0 {
            c = at(buf, p) & 0x80;
            p += 1;
        }

        if i_col == i_current {
            n_list = (p - p_list) as i32;
            break;
        }

        n_list -= (p - p_list) as i32;
        p_list = p;
        if n_list <= 0 {
            break;
        }
        p = p_list + 1;
        let (n, v) = fts3_get_varint32(sl(buf, p));
        p += n as usize;
        i_current = v;
    }

    if b_zero {
        let start = (p_list as i64 + n_list as i64).max(0) as usize;
        if start < p_end {
            let end = p_end.min(buf.len());
            if start < end {
                buf[start..end].fill(0);
            }
        }
    }
    (p_list, n_list)
}

/// `sqlite3Fts3MsrIncrNext`: o docid e a lista de posições seguintes do termo que `sqlite3Fts3MsrIncrStart`
/// escolheu, ou `None` no fim. A lista é uma cópia, sem o terminador (zero depois do fim).
pub fn fts3_msr_incr_next(
    p: &Fts3Table,
    p_msr: &mut Fts3MultiSegReader,
) -> Result<Option<(i64, Vec<u8>)>, i32> {
    let n_merge = p_msr.n_advance.max(0) as usize;
    let x_cmp: fn(&Fts3SegReader, &Fts3SegReader) -> i32 = if p.b_desc_idx {
        fts3_seg_reader_doclist_cmp_rev
    } else {
        fts3_seg_reader_doclist_cmp
    };

    if n_merge == 0 {
        return Ok(None);
    }

    loop {
        if p_msr.ap_segment[0].p_offset_list.is_none() {
            return Ok(None);
        }

        let i_docid = p_msr.ap_segment[0].i_docid;

        let (off, n) = fts3_seg_reader_next_docid(p, &mut p_msr.ap_segment[0]);
        let mut p_list: Vec<u8> = {
            let a_node = p_msr.ap_segment[0].a_node.as_deref().unwrap_or(&[]);
            (off..off + n.max(0) as usize).map(|i| at(a_node, i)).collect()
        };
        let mut n_list = n;

        let mut j = 1;
        while j < n_merge
            && p_msr.ap_segment[j].p_offset_list.is_some()
            && p_msr.ap_segment[j].i_docid == i_docid
        {
            fts3_seg_reader_next_docid(p, &mut p_msr.ap_segment[j]);
            j += 1;
        }
        fts3_seg_reader_sort(&mut p_msr.ap_segment, n_merge, j, x_cmp);

        if p_msr.i_col_filter >= 0 {
            let (o, l) = fts3_column_filter(p_msr.i_col_filter, true, &mut p_list, 0, n_list);
            p_list.drain(..o.min(p_list.len()));
            n_list = l;
        }
        p_list.truncate(n_list.max(0) as usize);

        if n_list > 0 {
            return Ok(Some((i_docid, p_list)));
        }
    }
}

/// `fts3SegReaderStart`: avança cada leitor até um termo igual ou maior que `z_term` (se dado), o
/// que poupa muitas fusões e ordenações quando uma folha tem mais de um termo.
fn fts3_seg_reader_start_at(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3MultiSegReader,
    z_term: Option<&[u8]>,
) -> i32 {
    let n_seg = p_csr.ap_segment.len();

    let mut i = 0;
    while !p_csr.b_restart && i < n_seg {
        let mut res = 0;
        loop {
            let rc = fts3_seg_reader_next(db, p, &mut p_csr.ap_segment[i]);
            if rc != SQLITE_OK {
                return rc;
            }
            match z_term {
                Some(zt) => {
                    res = fts3_seg_reader_term_cmp(&p_csr.ap_segment[i], zt);
                    if res < 0 {
                        continue;
                    }
                    break;
                }
                None => break,
            }
        }

        if p_csr.ap_segment[i].b_lookup && res != 0 {
            fts3_seg_reader_set_eof(&mut p_csr.ap_segment[i]);
        }
        i += 1;
    }
    fts3_seg_reader_sort(&mut p_csr.ap_segment, n_seg, n_seg, fts3_seg_reader_cmp);

    SQLITE_OK
}

/// `sqlite3Fts3SegReaderStart`: guarda uma cópia do filtro (o C guarda o ponteiro para o do
/// chamador) e posiciona os leitores.
pub fn fts3_seg_reader_start(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3MultiSegReader,
    p_filter: &Fts3SegFilter,
) -> i32 {
    p_csr.p_filter = Some(p_filter.clone());
    fts3_seg_reader_start_at(db, p, p_csr, p_filter.z_term.as_deref())
}

/// `sqlite3Fts3MsrIncrStart`: prepara `p_csr` para percorrer a doclist do termo `z_term`
/// documento a documento. `i_col` é a coluna a casar (ou negativa).
pub fn fts3_msr_incr_start(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3MultiSegReader,
    i_col: i32,
    z_term: &[u8],
) -> i32 {
    let n_segment = p_csr.ap_segment.len();
    let x_cmp: fn(&Fts3SegReader, &Fts3SegReader) -> i32 = if p.b_desc_idx {
        fts3_seg_reader_doclist_cmp_rev
    } else {
        fts3_seg_reader_doclist_cmp
    };

    debug_assert!(p_csr.p_filter.is_none());
    debug_assert!(!z_term.is_empty());

    /* Avança cada leitor até o termo `z_term`. */
    let rc = fts3_seg_reader_start_at(db, p, p_csr, Some(z_term));
    if rc != SQLITE_OK {
        return rc;
    }

    /* Quantos leitores apontam de fato para `z_term`. */
    let mut i = 0;
    while i < n_segment {
        let p_seg = &p_csr.ap_segment[i];
        if p_seg.a_node.is_none() || fts3_seg_reader_term_cmp(p_seg, z_term) != 0 {
            break;
        }
        i += 1;
    }
    p_csr.n_advance = i as i32;

    /* Põe cada um no primeiro docid. */
    for j in 0..i {
        fts3_seg_reader_first_docid(p, &mut p_csr.ap_segment[j]);
    }
    fts3_seg_reader_sort(&mut p_csr.ap_segment, i, i, x_cmp);

    debug_assert!(i_col < 0 || i_col < p.n_column());
    p_csr.i_col_filter = i_col;

    SQLITE_OK
}

/// `sqlite3Fts3MsrIncrRestart`: deixa o leitor tal que, se as duas chamadas seguintes forem
/// `sqlite3Fts3SegReaderStart` e `sqlite3Fts3SegReaderStep`, a doclist inteira do termo fica em
/// `a_doclist`.
pub fn fts3_msr_incr_restart(p_csr: &mut Fts3MultiSegReader) -> i32 {
    debug_assert!(p_csr.z_term.is_empty());
    debug_assert!(p_csr.a_doclist.is_empty());

    p_csr.n_advance = 0;
    p_csr.b_restart = true;
    for seg in p_csr.ap_segment.iter_mut() {
        seg.p_offset_list = None;
        seg.n_offset_list = 0;
        seg.i_docid = 0;
    }
    SQLITE_OK
}

/// `sqlite3Fts3SegReaderStep`: leva o leitor múltiplo ao termo seguinte. `SQLITE_ROW` com o termo
/// e a doclist em `z_term` e `a_doclist`; `SQLITE_OK` no fim; ou um código de erro.
pub fn fts3_seg_reader_step(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3MultiSegReader,
) -> i32 {
    debug_assert!(p_csr.p_filter.is_some());
    let p_filter = p_csr.p_filter.take().unwrap_or_default();
    let rc = fts3_seg_reader_step_filtered(db, p, p_csr, &p_filter);
    p_csr.p_filter = Some(p_filter);
    rc
}

/// O corpo de `sqlite3Fts3SegReaderStep`, com o filtro (`pCsr->pFilter`) à parte.
fn fts3_seg_reader_step_filtered(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3MultiSegReader,
    p_filter: &Fts3SegFilter,
) -> i32 {
    let is_ignore_empty = (p_filter.flags & FTS3_SEGMENT_IGNORE_EMPTY) != 0;
    let is_require_pos = (p_filter.flags & FTS3_SEGMENT_REQUIRE_POS) != 0;
    let is_col_filter = (p_filter.flags & FTS3_SEGMENT_COLUMN_FILTER) != 0;
    let is_prefix = (p_filter.flags & FTS3_SEGMENT_PREFIX) != 0;
    let is_scan = (p_filter.flags & FTS3_SEGMENT_SCAN) != 0;
    let is_first = (p_filter.flags & FTS3_SEGMENT_FIRST) != 0;

    let n_segment = p_csr.ap_segment.len();
    let x_cmp: fn(&Fts3SegReader, &Fts3SegReader) -> i32 = if p.b_desc_idx {
        fts3_seg_reader_doclist_cmp_rev
    } else {
        fts3_seg_reader_doclist_cmp
    };

    if n_segment == 0 {
        return SQLITE_OK;
    }

    let mut rc = SQLITE_OK;
    loop {
        /* Avança os primeiros `nAdvance` leitores e ordena de novo por termo. */
        for i in 0..p_csr.n_advance.max(0) as usize {
            let p_seg = &mut p_csr.ap_segment[i];
            if p_seg.b_lookup {
                fts3_seg_reader_set_eof(p_seg);
            } else {
                rc = fts3_seg_reader_next(db, p, p_seg);
            }
            if rc != SQLITE_OK {
                return rc;
            }
        }
        fts3_seg_reader_sort(
            &mut p_csr.ap_segment,
            n_segment,
            p_csr.n_advance.max(0) as usize,
            fts3_seg_reader_cmp,
        );
        p_csr.n_advance = 0;

        /* Com todos os leitores no fim, acabou. */
        debug_assert!(rc == SQLITE_OK);
        if p_csr.ap_segment[0].a_node.is_none() {
            break;
        }

        p_csr.z_term = p_csr.ap_segment[0].z_term.clone();

        /* Numa busca por prefixo, se o termo de `apSegment[0]` não começa com o do filtro, todas as
        ** chamadas pedidas já foram feitas: sai. Numa busca exata, se o primeiro termo do segmento
        ** não casa, também. */
        if let Some(ft) = p_filter.z_term.as_deref() {
            if !is_scan
                && (p_csr.z_term.len() < ft.len()
                    || (!is_prefix && p_csr.z_term.len() > ft.len())
                    || p_csr.z_term[..ft.len()] != *ft)
            {
                break;
            }
        }

        let mut n_merge = 1;
        while n_merge < n_segment
            && p_csr.ap_segment[n_merge].a_node.is_some()
            && p_csr.ap_segment[n_merge].z_term == p_csr.z_term
        {
            n_merge += 1;
        }

        debug_assert!(is_ignore_empty || (is_require_pos && !is_col_filter));
        if n_merge == 1
            && !is_ignore_empty
            && !is_first
            && (!p.b_desc_idx || !p_csr.ap_segment[0].is_pending())
        {
            let p_seg = &p_csr.ap_segment[0];
            let d = p_seg.a_doclist.unwrap_or(0);
            let a_node = p_seg.a_node.as_deref().unwrap_or(&[]);
            p_csr.a_doclist = (d..d + p_seg.n_doclist.max(0) as usize).map(|i| at(a_node, i)).collect();
            rc = SQLITE_ROW;
        } else {
            let mut n_doclist: i32 = 0; /* tamanho da doclist */
            let mut i_prev: i64 = 0; /* o docid anterior gravado na doclist */
            let mut a_buffer: Vec<u8> = Vec::new();

            /* O termo corrente dos `nMerge` primeiros leitores é o mesmo: as doclists se fundem e
            ** um único termo sai, com a doclist fundida. */
            for i in 0..n_merge {
                fts3_seg_reader_first_docid(p, &mut p_csr.ap_segment[i]);
            }
            fts3_seg_reader_sort(&mut p_csr.ap_segment, n_merge, n_merge, x_cmp);
            while p_csr.ap_segment[0].p_offset_list.is_some() {
                let i_docid = p_csr.ap_segment[0].i_docid;
                let (off, n) = fts3_seg_reader_next_docid(p, &mut p_csr.ap_segment[0]);
                let mut p_list: Vec<u8> = {
                    let a_node = p_csr.ap_segment[0].a_node.as_deref().unwrap_or(&[]);
                    (off..off + n.max(0) as usize).map(|i| at(a_node, i)).collect()
                };
                let mut n_list = n;
                let mut j = 1;
                while j < n_merge
                    && p_csr.ap_segment[j].p_offset_list.is_some()
                    && p_csr.ap_segment[j].i_docid == i_docid
                {
                    fts3_seg_reader_next_docid(p, &mut p_csr.ap_segment[j]);
                    j += 1;
                }

                if is_col_filter {
                    let (o, l) = fts3_column_filter(p_filter.i_col, false, &mut p_list, 0, n_list);
                    p_list.drain(..o.min(p_list.len()));
                    n_list = l;
                }
                p_list.truncate(n_list.max(0) as usize);

                if !is_ignore_empty || n_list > 0 {
                    /* O delta de docid que a doclist fundida grava. */
                    let i_delta: i64;
                    if p.b_desc_idx && n_doclist > 0 {
                        if i_prev <= i_docid {
                            return FTS_CORRUPT_VTAB;
                        }
                        i_delta = (i_prev as u64).wrapping_sub(i_docid as u64) as i64;
                    } else {
                        if n_doclist > 0 && i_prev >= i_docid {
                            return FTS_CORRUPT_VTAB;
                        }
                        i_delta = (i_docid as u64).wrapping_sub(i_prev as u64) as i64;
                    }

                    if is_first {
                        let n_write = fts3_first_filter(i_delta, &p_list, &mut a_buffer);
                        if n_write > 0 {
                            i_prev = i_docid;
                            n_doclist += n_write;
                        }
                    } else {
                        let mut tmp = [0u8; FTS3_VARINT_MAX];
                        let nv = fts3_put_varint(&mut tmp, i_delta) as usize;
                        a_buffer.extend_from_slice(&tmp[..nv]);
                        n_doclist += nv as i32;
                        i_prev = i_docid;
                        if is_require_pos {
                            a_buffer.extend_from_slice(&p_list);
                            n_doclist += n_list;
                            a_buffer.push(0);
                            n_doclist += 1;
                        }
                    }
                }

                fts3_seg_reader_sort(&mut p_csr.ap_segment, n_merge, j, x_cmp);
            }
            if n_doclist > 0 {
                debug_assert!(a_buffer.len() == n_doclist as usize);
                p_csr.a_doclist = a_buffer;
                rc = SQLITE_ROW;
            }
        }
        p_csr.n_advance = n_merge as i32;

        if rc != SQLITE_OK {
            break;
        }
    }

    rc
}

/// `sqlite3Fts3SegReaderFinish`: solta os leitores e o buffer do leitor múltiplo.
pub fn fts3_seg_reader_finish(_db: &mut Connection, p_csr: &mut Fts3MultiSegReader) {
    p_csr.ap_segment = Vec::new();
    p_csr.a_buffer = Vec::new();
}

/// `fts3ReadEndBlockField`: decodifica o campo `end_block`, a coluna `i_col` da linha corrente de
/// `p_stmt`: um inteiro, ou o texto de dois inteiros não negativos separados por espaços. No
/// primeiro caso `*pi_end_block` recebe o inteiro (e `*pn_byte` fica como estava); no segundo, o
/// primeiro valor em `*pi_end_block` e o segundo em `*pn_byte`.
pub(super) fn fts3_read_end_block_field(
    db: &mut Connection,
    p_stmt: StmtId,
    i_col: i32,
    pi_end_block: &mut i64,
    pn_byte: &mut i64,
) {
    let Some(z_text) = column_text(db, p_stmt, i_col).map(|s| s.to_vec()) else {
        return;
    };
    let mut i = 0usize;
    let mut i_mul: i64 = 1;
    let mut i_val: u64 = 0;
    while at(&z_text, i).is_ascii_digit() {
        i_val = i_val.wrapping_mul(10).wrapping_add((at(&z_text, i) - b'0') as u64);
        i += 1;
    }
    *pi_end_block = i_val as i64;
    while at(&z_text, i) == b' ' {
        i += 1;
    }
    i_val = 0;
    if at(&z_text, i) == b'-' {
        i += 1;
        i_mul = -1;
    }
    while at(&z_text, i).is_ascii_digit() {
        i_val = i_val.wrapping_mul(10).wrapping_add((at(&z_text, i) - b'0') as u64);
        i += 1;
    }
    *pn_byte = (i_val as i64).wrapping_mul(i_mul);
}

/// `fts3PromoteSegments`: um segmento de `n_byte` bytes acabou de ser gravado no nível absoluto
/// `i_abs_level`; promove os segmentos que devem subir por causa disso.
pub(super) fn fts3_promote_segments(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    n_byte: i64,
) -> i32 {
    let p_range = match fts3_sql_stmt(db, p, SQL_SELECT_LEVEL_RANGE2, &[]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };

    let mut b_ok = false;
    let i_last = (i_abs_level / FTS3_SEGDIR_MAXLEVEL as i64 + 1) * FTS3_SEGDIR_MAXLEVEL as i64 - 1;
    let n_limit = n_byte.wrapping_mul(3) / 2;

    /* Percorre as entradas de `%_segdir` dos segmentos deste índice nos níveis maiores que
    ** `i_abs_level`. Havendo pelo menos uma e sendo possível saber que todas têm menos de
    ** `n_limit` bytes, elas sobem para `i_abs_level`. */
    bind_int64(db, p_range, 1, i_abs_level.wrapping_add(1));
    bind_int64(db, p_range, 2, i_last);
    while step(db, p_range) == SQLITE_ROW {
        let mut n_size: i64 = 0;
        let mut dummy: i64 = 0;
        fts3_read_end_block_field(db, p_range, 2, &mut dummy, &mut n_size);
        if n_size <= 0 || n_size > n_limit {
            /* Com `n_size==0` o campo `end_block` não traz o tamanho (é de uma versão antiga do
            ** FTS) e não há como saber o tamanho do segmento: não se promove. */
            b_ok = false;
            break;
        }
        b_ok = true;
    }
    let mut rc = reset(db, p_range);

    if b_ok {
        let mut i_idx = 0;
        let mut p_update1: Option<StmtId> = None;
        let mut p_update2: Option<StmtId> = None;

        if rc == SQLITE_OK {
            match fts3_sql_stmt(db, p, SQL_UPDATE_LEVEL_IDX, &[]) {
                Ok(st) => p_update1 = Some(st),
                Err(e) => rc = e,
            }
        }
        if rc == SQLITE_OK {
            match fts3_sql_stmt(db, p, SQL_UPDATE_LEVEL, &[]) {
                Ok(st) => p_update2 = Some(st),
                Err(e) => rc = e,
            }
        }

        if let (SQLITE_OK, Some(p_update1)) = (rc, p_update1) {
            /* Para cada entrada de `%_segdir` deste índice com nível igual ou maior que
            ** `i_abs_level`: nível -1 e `idx` = N (0 para a mais velha da faixa, 1 para a seguinte,
            ** e assim por diante). Isto é, move os segmentos promovidos para o nível -1 (que nunca
            ** se usa, só aqui, de passagem) mantendo a ordem. */
            bind_int64(db, p_range, 1, i_abs_level);
            while step(db, p_range) == SQLITE_ROW {
                bind_int(db, p_update1, 1, i_idx);
                i_idx += 1;
                let v2 = column_int(db, p_range, 0);
                bind_int(db, p_update1, 2, v2);
                let v3 = column_int(db, p_range, 1);
                bind_int(db, p_update1, 3, v3);
                step(db, p_update1);
                rc = reset(db, p_update1);
                if rc != SQLITE_OK {
                    reset(db, p_range);
                    break;
                }
            }
        }
        if rc == SQLITE_OK {
            rc = reset(db, p_range);
        }

        /* Passa o nível -1 para `i_abs_level`. */
        if rc == SQLITE_OK {
            if let Some(p_update2) = p_update2 {
                bind_int64(db, p_update2, 1, i_abs_level);
                step(db, p_update2);
                rc = reset(db, p_update2);
            }
        }
    }

    rc
}

/// `fts3SegmentMerge`: funde num só segmento de nível `i_level+1` todos os segmentos de nível
/// `i_level`. Com `i_level<0` (`FTS3_SEGCURSOR_ALL`) funde todos os segmentos num só, do maior
/// nível presente. Devolve `SQLITE_DONE` se `i_level<0` e só há um segmento.
fn fts3_segment_merge(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
) -> i32 {
    let mut csr = Fts3MultiSegReader::default();
    let mut p_writer: Option<SegmentWriter> = None;
    let rc = fts3_segment_merge_body(db, p, i_langid, i_index, i_level, &mut csr, &mut p_writer);
    drop(p_writer);
    fts3_seg_reader_finish(db, &mut csr);
    rc
}

/// O corpo de `fts3SegmentMerge`; o `finished:` do C (soltar o escritor e o leitor) é do chamador.
fn fts3_segment_merge_body(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
    csr: &mut Fts3MultiSegReader,
    p_writer: &mut Option<SegmentWriter>,
) -> i32 {
    let mut i_idx = 0; /* o índice do novo segmento */
    let i_new_level: i64; /* o nível/índice do novo segmento */
    let b_ignore_empty: bool;
    let mut i_max_level: i64 = 0;

    debug_assert!(
        i_level == FTS3_SEGCURSOR_ALL || i_level == FTS3_SEGCURSOR_PENDING || i_level >= 0
    );
    debug_assert!(i_level < FTS3_SEGDIR_MAXLEVEL);
    debug_assert!(i_index >= 0 && i_index < p.n_index);

    let mut rc = fts3_seg_reader_cursor(db, p, i_langid, i_index, i_level, None, true, false, csr);
    if rc != SQLITE_OK || csr.ap_segment.is_empty() {
        return rc;
    }

    if i_level != FTS3_SEGCURSOR_PENDING {
        rc = fts3_segment_max_level(db, p, i_langid, i_index, &mut i_max_level);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    if i_level == FTS3_SEGCURSOR_ALL {
        /* Funde todos os segmentos do banco num só. O nível do novo segmento é o maior nível
        ** presente no índice e o `idx` é sempre 0. */
        if csr.ap_segment.len() == 1 && !csr.ap_segment[0].is_pending() {
            return SQLITE_DONE;
        }
        i_new_level = i_max_level;
        b_ignore_empty = true;
    } else {
        /* Funde todos os segmentos do nível `i_level`: acha o próximo `idx` livre no nível
        ** `i_level+1` (`fts3AllocateSegdirIdx` funde o nível `i_level+1` num do `i_level+2` se
        ** preciso). */
        debug_assert!(FTS3_SEGCURSOR_PENDING == -1);
        i_new_level = get_absolute_level(p, i_langid, i_index, i_level + 1);
        rc = fts3_allocate_segdir_idx(db, p, i_langid, i_index, i_level + 1, &mut i_idx);
        b_ignore_empty = (i_level != FTS3_SEGCURSOR_PENDING) && (i_new_level > i_max_level);
    }
    if rc != SQLITE_OK {
        return rc;
    }

    debug_assert!(!csr.ap_segment.is_empty());

    let mut filter = Fts3SegFilter::default();
    filter.flags = FTS3_SEGMENT_REQUIRE_POS;
    filter.flags |= if b_ignore_empty { FTS3_SEGMENT_IGNORE_EMPTY } else { 0 };

    rc = fts3_seg_reader_start(db, p, csr, &filter);
    while rc == SQLITE_OK {
        rc = fts3_seg_reader_step(db, p, csr);
        if rc != SQLITE_ROW {
            break;
        }
        rc = fts3_seg_writer_add(db, p, p_writer, &csr.z_term, &csr.a_doclist);
    }
    if rc != SQLITE_OK {
        return rc;
    }
    debug_assert!(p_writer.is_some() || b_ignore_empty);

    if i_level != FTS3_SEGCURSOR_PENDING {
        rc = fts3_delete_segdir(db, p, i_langid, i_index, i_level, &csr.ap_segment);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    if let Some(w) = p_writer.as_mut() {
        rc = fts3_seg_writer_flush(db, p, w, i_new_level, i_idx);
        if rc == SQLITE_OK && (i_level == FTS3_SEGCURSOR_PENDING || i_new_level < i_max_level) {
            rc = fts3_promote_segments(db, p, i_new_level, w.n_leaf_data);
        }
    }

    rc
}

/// `sqlite3Fts3PendingTermsFlush`: grava os termos pendentes em segmentos de nível 0.
pub fn fts3_pending_terms_flush(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut rc = SQLITE_OK;
    let i_prev_langid = p.i_prev_langid;

    let mut i = 0;
    while rc == SQLITE_OK && i < p.n_index {
        rc = fts3_segment_merge(db, p, i_prev_langid, i, FTS3_SEGCURSOR_PENDING);
        if rc == SQLITE_DONE {
            rc = SQLITE_OK;
        }
        i += 1;
    }

    /* Se o valor de `automerge` é desconhecido, determina-o; ligado, estima os blocos folha de
    ** conteúdo a gravar. */
    if rc == SQLITE_OK && p.b_has_stat != 0 && p.n_autoincrmerge == 0xff && p.n_leaf_add > 0 {
        match fts3_sql_stmt(db, p, SQL_SELECT_STAT, &[]) {
            Ok(st) => {
                bind_int(db, st, 1, FTS_STAT_AUTOINCRMERGE);
                rc = step(db, st);
                if rc == SQLITE_ROW {
                    p.n_autoincrmerge = column_int(db, st, 0);
                    if p.n_autoincrmerge == 1 {
                        p.n_autoincrmerge = 8;
                    }
                } else if rc == SQLITE_DONE {
                    p.n_autoincrmerge = 0;
                }
                rc = reset(db, st);
            }
            Err(e) => rc = e,
        }
    }

    if rc == SQLITE_OK {
        fts3_pending_terms_clear(p);
    }
    rc
}

/// `fts3EncodeIntArray`: codifica os inteiros de `a` como varints.
fn fts3_encode_int_array(a: &[u32]) -> Vec<u8> {
    let mut z_buf = Vec::with_capacity(a.len() * FTS3_VARINT_MAX);
    let mut tmp = [0u8; FTS3_VARINT_MAX];
    for &v in a {
        let n = fts3_put_varint(&mut tmp, v as i64) as usize;
        z_buf.extend_from_slice(&tmp[..n]);
    }
    z_buf
}

/// `fts3DecodeIntArray`: decodifica um blob de varints em `a.len()` inteiros.
fn fts3_decode_int_array(a: &mut [u32], z_buf: &[u8]) {
    let n = a.len();
    let mut i = 0;
    if !z_buf.is_empty() && (z_buf[z_buf.len() - 1] & 0x80) == 0 {
        let mut j = 0usize;
        while i < n && j < z_buf.len() {
            let (nb, x) = fts3_get_varint(sl(z_buf, j));
            j += nb as usize;
            a[i] = (x & 0xffff_ffff) as u32;
            i += 1;
        }
    }
    while i < n {
        a[i] = 0;
        i += 1;
    }
}

/// `fts3InsertDocsize`: insere os tamanhos (em tokens) de cada coluna do documento com docid
/// `p.i_prev_docid`, codificados como um blob de varints.
pub(super) fn fts3_insert_docsize(
    p_rc: &mut i32,
    db: &mut Connection,
    p: &mut Fts3Table,
    a_sz: &[u32],
) {
    if *p_rc != SQLITE_OK {
        return;
    }
    let n_column = p.n_column() as usize;
    let p_blob = fts3_encode_int_array(&a_sz[..n_column]);
    match fts3_sql_stmt(db, p, SQL_REPLACE_DOCSIZE, &[]) {
        Ok(st) => {
            bind_int64(db, st, 1, p.i_prev_docid);
            bind_blob(db, st, 2, Some(&p_blob), p_blob.len() as i32, StrDtor::Transient);
            step(db, st);
            *p_rc = reset(db, st);
        }
        Err(rc) => *p_rc = rc,
    }
}

/// `fts3UpdateDocTotals`: o registro 0 de `%_stat` é um blob de `nCol+2` varints: o total de
/// linhas, o total de tokens de cada coluna e o tamanho em bytes de todos os textos de todas as
/// colunas de todas as linhas. Aplica a mudança de tamanhos (`a_sz_ins` e `a_sz_del`) e de número
/// de documentos (`n_chng`).
pub(super) fn fts3_update_doc_totals(
    p_rc: &mut i32,
    db: &mut Connection,
    p: &mut Fts3Table,
    a_sz_ins: &[u32],
    a_sz_del: &[u32],
    n_chng: i32,
) {
    let n_column = p.n_column() as usize;
    let n_stat = n_column + 2;

    if *p_rc != SQLITE_OK {
        return;
    }
    let mut a = vec![0u32; n_stat];
    let st = match fts3_sql_stmt(db, p, SQL_SELECT_STAT, &[]) {
        Ok(st) => st,
        Err(rc) => {
            *p_rc = rc;
            return;
        }
    };
    bind_int(db, st, 1, FTS_STAT_DOCTOTAL);
    if step(db, st) == SQLITE_ROW {
        let blob: Vec<u8> = column_blob(db, st, 0).map(|b| b.to_vec()).unwrap_or_default();
        fts3_decode_int_array(&mut a, &blob);
    } else {
        a.fill(0);
    }
    let rc = reset(db, st);
    if rc != SQLITE_OK {
        *p_rc = rc;
        return;
    }
    if n_chng < 0 && a[0] < n_chng.unsigned_abs() {
        a[0] = 0;
    } else {
        a[0] = a[0].wrapping_add(n_chng as u32);
    }
    for i in 0..n_column + 1 {
        let mut x = a[i + 1];
        if x.wrapping_add(a_sz_ins[i]) < a_sz_del[i] {
            x = 0;
        } else {
            x = x.wrapping_add(a_sz_ins[i]).wrapping_sub(a_sz_del[i]);
        }
        a[i + 1] = x;
    }
    let p_blob = fts3_encode_int_array(&a);
    match fts3_sql_stmt(db, p, SQL_REPLACE_STAT, &[]) {
        Ok(st) => {
            bind_int(db, st, 1, FTS_STAT_DOCTOTAL);
            bind_blob(db, st, 2, Some(&p_blob), p_blob.len() as i32, StrDtor::Transient);
            step(db, st);
            *p_rc = reset(db, st);
            bind_null(db, st, 2);
        }
        Err(rc) => *p_rc = rc,
    }
}

/// `fts3DoOptimize`: funde o banco todo, de modo que haja um segmento por combinação de índice e
/// idioma.
pub(super) fn fts3_do_optimize(db: &mut Connection, p: &mut Fts3Table, b_return_done: bool) -> i32 {
    let mut b_seen_done = false;

    let mut rc = fts3_pending_terms_flush(db, p);
    let mut p_all_langid: Option<StmtId> = None;
    if rc == SQLITE_OK {
        match fts3_sql_stmt(db, p, SQL_SELECT_ALL_LANGID, &[]) {
            Ok(st) => p_all_langid = Some(st),
            Err(e) => rc = e,
        }
    }
    if let (SQLITE_OK, Some(p_all_langid)) = (rc, p_all_langid) {
        bind_int(db, p_all_langid, 1, p.i_prev_langid);
        bind_int(db, p_all_langid, 2, p.n_index);
        while step(db, p_all_langid) == SQLITE_ROW {
            let i_langid = column_int(db, p_all_langid, 0);
            let mut i = 0;
            while rc == SQLITE_OK && i < p.n_index {
                rc = fts3_segment_merge(db, p, i_langid, i, FTS3_SEGCURSOR_ALL);
                if rc == SQLITE_DONE {
                    b_seen_done = true;
                    rc = SQLITE_OK;
                }
                i += 1;
            }
        }
        let rc2 = reset(db, p_all_langid);
        if rc == SQLITE_OK {
            rc = rc2;
        }
    }

    fts3_segments_close(db, p);

    if rc == SQLITE_OK && b_return_done && b_seen_done {
        SQLITE_DONE
    } else {
        rc
    }
}

/// `fts3DoRebuild`: `INSERT INTO <tbl>(<tbl>) VALUES('rebuild')`. Descarta o índice todo e o refaz
/// a partir do conteúdo atual de `%_content` (ou, com `content=xxx`, da tabela `xxx`).
pub(super) fn fts3_do_rebuild(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut rc = fts3_delete_all(db, p, false);
    if rc == SQLITE_OK {
        let n_column = p.n_column() as usize;
        let mut p_stmt: Option<StmtId> = None;
        let mut n_entry = 0;

        /* Monta e prepara o SQL que percorre a tabela de conteúdo. */
        match mprintf(b"SELECT %s", &[PrintfArg::Text(p.z_read_exprlist.clone())]) {
            None => rc = SQLITE_NOMEM,
            Some(z_sql) => {
                let (rc2, stmt, _) = prepare_v2(db, &z_sql, -1);
                rc = rc2;
                p_stmt = stmt;
            }
        }

        /* `aSz`, `aSzIns` e `aSzDel` contíguos no C; aqui três vetores. */
        let mut a_sz = vec![0u32; n_column + 1];
        let mut a_sz_ins = vec![0u32; n_column + 1];
        let a_sz_del = vec![0u32; n_column + 1];

        while rc == SQLITE_OK {
            let Some(st) = p_stmt else { break };
            if step(db, st) != SQLITE_ROW {
                break;
            }
            let i_langid = langid_from_select(db, p, st);
            let i_docid = column_int64(db, st, 0);
            rc = fts3_pending_terms_docid(db, p, false, i_langid, i_docid);
            a_sz.fill(0);
            let mut i_col = 0;
            while rc == SQLITE_OK && i_col < n_column {
                if at(&p.ab_notindexed, i_col) == 0 {
                    let z = column_text(db, st, i_col as i32 + 1).map(|s| s.to_vec());
                    rc = fts3_pending_terms_add(
                        p,
                        i_langid,
                        z.as_deref(),
                        i_col as i32,
                        &mut a_sz[i_col],
                    );
                    let n_bytes = column_bytes(db, st, i_col as i32 + 1) as u32;
                    a_sz[n_column] = a_sz[n_column].wrapping_add(n_bytes);
                }
                i_col += 1;
            }
            if p.b_has_docsize {
                fts3_insert_docsize(&mut rc, db, p, &a_sz);
            }
            if rc != SQLITE_OK {
                finalize(db, st);
                p_stmt = None;
            } else {
                n_entry += 1;
                for i_col in 0..=n_column {
                    a_sz_ins[i_col] = a_sz_ins[i_col].wrapping_add(a_sz[i_col]);
                }
            }
        }
        if p.b_fts4 {
            fts3_update_doc_totals(&mut rc, db, p, &a_sz_ins, &a_sz_del, n_entry);
        }

        if let Some(st) = p_stmt {
            let rc2 = finalize(db, st);
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
    }

    rc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_list_builds_the_doclist() {
        /* Docid 5, coluna 0, posições 0 e 3; docid 9, coluna 1, posição 1. */
        let mut l = PendingList::default();
        pending_list_append(&mut l, true, 5, 0, 0);
        pending_list_append(&mut l, false, 5, 0, 3);
        pending_list_append(&mut l, false, 9, 1, 1);
        assert_eq!(l.a_data, vec![5, 2, 5, 0, 4, 1, 1, 3]);
        assert_eq!(l.n_data(), 8);
        let mut o = None;
        pending_list_append_opt(&mut o, 7, 0, 0);
        pending_list_append_varint_opt(&mut o, 0);
        assert_eq!(o.map(|l| l.a_data), Some(vec![7, 2, 0]));
    }

    #[test]
    fn prefix_compress_counts_common_bytes() {
        assert_eq!(fts3_prefix_compress(b"abc", b"abcdef"), 3);
        assert_eq!(fts3_prefix_compress(b"abX", b"abcdef"), 2);
        assert_eq!(fts3_prefix_compress(b"abX", b"Xbcdef"), 0);
        assert_eq!(fts3_prefix_compress(b"", b"x"), 0);
    }

    #[test]
    fn column_filter_selects_one_column() {
        /* Coluna 0: posições 2 e 4; coluna 1: posição 3; coluna 2: posição 5. */
        let mut list = vec![2u8, 4, 1, 1, 5, 1, 2, 7];
        let n = list.len() as i32;
        assert_eq!(fts3_column_filter(0, false, &mut list, 0, n), (0, 2));
        assert_eq!(fts3_column_filter(1, false, &mut list, 0, n), (2, 3));
        assert_eq!(fts3_column_filter(2, false, &mut list, 0, n), (5, 3));
        assert_eq!(fts3_column_filter(3, false, &mut list, 0, n).1, 0);
        assert_eq!(fts3_column_filter(1, true, &mut list, 0, n), (2, 3));
        assert_eq!(list, vec![2, 4, 1, 1, 5, 0, 0, 0]);
    }

    #[test]
    fn node_tree_splits_when_full() {
        let mut nodes: Vec<SegmentNode> = Vec::new();
        let mut tree: Option<usize> = None;
        for t in [&b"aaa"[..], b"aab", b"abc", b"b", b"c"] {
            assert_eq!(fts3_node_add_term(16, &mut nodes, &mut tree, t), SQLITE_OK);
        }
        assert!(tree.is_some());
        assert!(nodes.len() >= 2);
        /* Um termo que é prefixo do anterior é dado corrompido. */
        assert_eq!(fts3_node_add_term(16, &mut nodes, &mut tree, b"c"), FTS_CORRUPT_VTAB);
    }

    #[test]
    fn end_block_decoding_helpers() {
        assert_eq!(fts3_c_str(b"ab\0cd"), b"ab");
        assert_eq!(memcmp(b"ab", b"ac"), -1);
        assert_eq!(sl(b"abc", 5), b"");
        let mut v = Vec::new();
        assert_eq!(put_varint_at(&mut v, 3, 300), 2);
        assert_eq!(&v[3..5], &[0xAC, 0x02]);
    }
}
