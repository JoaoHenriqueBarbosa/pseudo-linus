//! `fts3_write.c` (parte 2): fusão incremental (`sqlite3Fts3Incrmerge` e a escrita de segmentos
//! anexáveis), verificação de integridade, comandos especiais do `INSERT` (`optimize`, `rebuild`,
//! `integrity-check`, `merge=`, `automerge=`, `flush`), tokens adiados, o `xUpdate`
//! (`sqlite3Fts3UpdateMethod`) e `sqlite3Fts3Optimize`. A parte 1 é [`super::write`], que
//! reexporta este módulo.
//!
//! O fim do módulo tem as rotinas de `fts3.c` de que `fts3_write.c` depende e que moram aqui para
//! que `main.rs` as importe em vez de as redefinir (a segunda cópia de uma lógica não existe):
//! `fts3DbExec`, `sqlite3Fts3CreateStatTable`, `fts3PoslistCopy`, `fts3ColumnlistCopy`,
//! `fts3GetReverseVarint`, `fts3ReversePoslist`, `sqlite3Fts3DoclistPrev`,
//! `sqlite3Fts3FirstFilter`, `fts3ScanInteriorNode`, `fts3SelectLeaf`, `fts3SegReaderCursor`
//! (aqui [`fts3_seg_reader_cursor_fill`]) e `sqlite3Fts3SegReaderCursor`.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! Os de [`super::write`] e mais:
//!
//! * **`Blob`** é um `Vec<u8>` (`a`) com o comprimento lógico `n`; `blobGrowBuffer` é `grow`
//!   (cresce com zeros, sem falha). Um `Blob` sem `a` alocado é o `a==NULL` do C.
//! * **`NodeReader`** guarda o nó emprestado (`Option<&[u8]>`; `None` é o `aNode==NULL`, o fim) e a
//!   doclist como deslocamento no nó. Lê além do fim do nó como zero (o enchimento do C).
//! * **`sqlite3Fts3FirstFilter`** acrescenta a um `Vec<u8>` e devolve o número de bytes escritos
//!   (o C escreve num buffer que o chamador já dimensionou).
//! * **`fts3SqlStmt` com `pFindLevel` nulo.** Em `sqlite3Fts3Incrmerge`, uma falha ao preparar o
//!   comando `SQL_FIND_MERGE_LEVEL` deixa `pFindLevel==NULL`: `sqlite3_step` devolve `MISUSE`,
//!   `nSeg` vira -1 e `sqlite3_reset(NULL)` devolve `SQLITE_OK`, que apaga o erro. É o que o C
//!   faz, e é o que está aqui.

use crate::build::text_arg;
use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_CONSTRAINT, SQLITE_CORRUPT_VTAB, SQLITE_DONE, SQLITE_ERROR, SQLITE_NOMEM, SQLITE_NULL,
    SQLITE_OK, SQLITE_REPLACE, SQLITE_ROW,
};
use crate::legacy::exec;
use crate::mem::{value_type, Mem, StrDtor};
use crate::prepare::prepare_v2;
use crate::printf::{mprintf, PrintfArg};
use crate::util::{at, strnicmp};
use crate::vdbeapi::{
    bind_blob, bind_int, bind_int64, bind_null, column_blob, column_bytes, column_int,
    column_int64, column_text, finalize, reset, step, text_of, value_int, value_int64,
};
use crate::vtab::vtab_on_conflict;

use super::expr::fts3_open_tokenizer;
use super::int::{
    Fts3Cursor, Fts3MultiSegReader, Fts3PhraseToken, Fts3SegFilter, Fts3Table, FTS3_MERGE_COUNT,
    FTS3_SEGCURSOR_ALL, FTS3_SEGCURSOR_PENDING, FTS3_SEGDIR_MAXLEVEL, FTS3_SEGMENT_IGNORE_EMPTY,
    FTS3_SEGMENT_REQUIRE_POS, FTS3_SEGMENT_SCAN, FTS3_VARINT_MAX, FTS_CORRUPT_VTAB,
};
use super::varint::{
    fts3_get_varint, fts3_get_varint32, fts3_get_varint_u, fts3_put_varint, fts3_varint_len,
};
use super::write::{
    fts3_all_segdirs, fts3_c_str, fts3_delete_all, fts3_delete_segment, fts3_delete_terms,
    fts3_do_optimize, fts3_do_rebuild, fts3_insert_data, fts3_insert_docsize, fts3_insert_terms,
    fts3_is_empty, fts3_pending_terms_docid, fts3_pending_terms_flush, fts3_prefix_compress,
    fts3_promote_segments, fts3_read_block, fts3_read_end_block_field, fts3_seg_reader_finish,
    fts3_seg_reader_new, fts3_seg_reader_pending, fts3_seg_reader_start, fts3_seg_reader_step,
    fts3_segment_is_max_level, fts3_segments_close, fts3_sql_exec, fts3_sql_stmt,
    fts3_update_doc_totals, fts3_write_segdir, fts3_write_segment, fts3_writelock,
    get_absolute_level, langid_from_select, memcmp, pending_list_append_opt,
    pending_list_append_varint_opt, put_bytes_at, put_varint_at, sl, Fts3DeferredToken,
    FTS3_NODE_PADDING, FTS_MAX_APPENDABLE_HEIGHT, FTS_STAT_AUTOINCRMERGE, FTS_STAT_INCRMERGEHINT,
    SQL_CHOMP_SEGDIR, SQL_DELETE_CONTENT, SQL_DELETE_DOCSIZE, SQL_DELETE_SEGDIR_ENTRY,
    SQL_DELETE_SEGMENTS_RANGE, SQL_FIND_MERGE_LEVEL, SQL_MAX_LEAF_NODE_ESTIMATE,
    SQL_NEXT_SEGMENTS_ID, SQL_NEXT_SEGMENT_INDEX, SQL_REPLACE_STAT, SQL_SEGMENT_IS_APPENDABLE,
    SQL_SELECT_ALL_LANGID, SQL_SELECT_INDEXES, SQL_SELECT_LEVEL, SQL_SELECT_SEGDIR,
    SQL_SELECT_STAT, SQL_SHIFT_SEGDIR_ENTRY,
};

// ---------------------------------------------------------------------------------------------
// Blob, NodeWriter, IncrmergeWriter, NodeReader
// ---------------------------------------------------------------------------------------------

/// `Blob`: um buffer dinâmico em que se montam nós e outros blobs.
#[derive(Debug, Default)]
pub(super) struct Blob {
    /// `a`/`nAlloc`: a alocação (vazia é o `NULL`).
    pub(super) a: Vec<u8>,
    /// `n`: os bytes válidos de `a`.
    pub(super) n: i32,
}

impl Blob {
    /// `blobGrowBuffer`: garante pelo menos `n_min` bytes de alocação.
    fn grow(&mut self, n_min: usize) {
        if n_min > self.a.len() {
            self.a.resize(n_min, 0);
        }
    }

    /// Os `n` bytes válidos.
    fn bytes(&self) -> &[u8] {
        let n = (self.n.max(0) as usize).min(self.a.len());
        &self.a[..n]
    }

    /// Acrescenta um varint depois dos `n` bytes válidos.
    fn put_varint(&mut self, v: i64) {
        self.n += put_varint_at(&mut self.a, self.n.max(0) as usize, v);
    }

    /// Acrescenta bytes depois dos `n` bytes válidos.
    fn put_bytes(&mut self, src: &[u8]) {
        put_bytes_at(&mut self.a, self.n.max(0) as usize, src);
        self.n += src.len() as i32;
    }
}

/// `NodeWriter`: monta os blocos (nós) de uma camada da árvore de um segmento.
#[derive(Debug, Default)]
pub(super) struct NodeWriter {
    /// `iBlock`: o id do bloco corrente.
    i_block: i64,
    /// `key`: a última chave gravada no bloco corrente.
    key: Blob,
    /// `block`: a imagem do bloco corrente.
    block: Blob,
}

/// `IncrmergeWriter`: o estado para criar ou anexar a um segmento b-tree anexável.
#[derive(Debug, Default)]
pub(super) struct IncrmergeWriter {
    /// `nLeafEst`: o espaço reservado para folhas.
    n_leaf_est: i32,
    /// `nWork`: as folhas gravadas.
    n_work: i32,
    /// `iAbsLevel`: o nível absoluto dos segmentos de entrada.
    i_abs_level: i64,
    /// `iIdx`: o índice do segmento de *saída* em `i_abs_level+1`.
    i_idx: i32,
    /// `iStart`: o primeiro bloco alocado.
    i_start: i64,
    /// `iEnd`: o último bloco alocado.
    i_end: i64,
    /// `nLeafData`: os bytes de folha até agora.
    n_leaf_data: i64,
    /// `bNoLeafData`: verdadeiro grava 0 como tamanho do segmento.
    b_no_leaf_data: bool,
    /// `aNodeWriter`.
    a_node_writer: [NodeWriter; FTS_MAX_APPENDABLE_HEIGHT],
}

/// `NodeReader`: lê as entradas de um nó de segmento.
#[derive(Debug, Default)]
struct NodeReader<'a> {
    /// `aNode`: `None` é o fim.
    a_node: Option<&'a [u8]>,
    /// `nNode`.
    n_node: i32,
    /// `iOff`: o deslocamento corrente em `a_node`.
    i_off: i32,
    /// `iChild`: o ponteiro para o nó filho.
    i_child: i64,
    /// `term`: o termo corrente.
    term: Blob,
    /// `aDoclist`: o deslocamento da doclist em `a_node` (só nas folhas).
    a_doclist: Option<usize>,
    /// `nDoclist`.
    n_doclist: i32,
}

impl<'a> NodeReader<'a> {
    /// A doclist corrente como fatia (`None` sem doclist).
    fn doclist(&self) -> Option<&'a [u8]> {
        let off = self.a_doclist?;
        let d = sl(self.a_node.unwrap_or(&[]), off);
        Some(&d[..(self.n_doclist.max(0) as usize).min(d.len())])
    }
}

/// `nodeReaderNext`: avança para a próxima entrada do nó. Sem próxima, `a_node` vira `None`.
fn node_reader_next(p: &mut NodeReader<'_>) -> i32 {
    let b_first = p.term.n == 0; /* verdadeiro para o primeiro termo do nó */
    let mut n_prefix = 0i32;

    debug_assert!(p.a_node.is_some());
    let node: &[u8] = p.a_node.unwrap_or(&[]);
    if p.i_child != 0 && !b_first {
        p.i_child += 1;
    }
    if p.i_off >= p.n_node {
        /* Fim. */
        p.a_node = None;
    } else {
        if !b_first {
            let (n, v) = fts3_get_varint32(sl(node, p.i_off as usize));
            p.i_off += n;
            n_prefix = v;
        }
        let (n, n_suffix) = fts3_get_varint32(sl(node, p.i_off as usize));
        p.i_off += n;

        if n_prefix > p.term.n || n_suffix as i64 > p.n_node as i64 - p.i_off as i64 || n_suffix == 0
        {
            return FTS_CORRUPT_VTAB;
        }
        p.term.grow((n_prefix + n_suffix) as usize);
        for k in 0..n_suffix as usize {
            p.term.a[n_prefix as usize + k] = at(node, p.i_off as usize + k);
        }
        p.term.n = n_prefix + n_suffix;
        p.i_off += n_suffix;
        if p.i_child == 0 {
            let (n, v) = fts3_get_varint32(sl(node, p.i_off as usize));
            p.i_off += n;
            p.n_doclist = v;
            if (p.n_node as i64 - p.i_off as i64) < p.n_doclist as i64 {
                return FTS_CORRUPT_VTAB;
            }
            p.a_doclist = Some(p.i_off as usize);
            p.i_off += p.n_doclist;
        }
    }

    SQLITE_OK
}

/// `nodeReaderInit`: inicia um leitor sobre o nó `a_node` (`n_node` bytes) e o põe na primeira
/// entrada, se houver.
fn node_reader_init(a_node: Option<&[u8]>, n_node: i32) -> (NodeReader<'_>, i32) {
    let mut p = NodeReader { a_node, n_node, ..Default::default() };

    /* Se o nó é uma folha ou interno. */
    match a_node {
        Some(node) if at(node, 0) != 0 => {
            /* Nó interno. */
            let (n, v) = fts3_get_varint(sl(node, 1));
            p.i_off = 1 + n;
            p.i_child = v;
        }
        _ => p.i_off = 1,
    }

    let rc = if a_node.is_some() { node_reader_next(&mut p) } else { SQLITE_OK };
    (p, rc)
}

// ---------------------------------------------------------------------------------------------
// Fusão incremental
// ---------------------------------------------------------------------------------------------

/// `fts3IncrmergeCsr`: abre o cursor que lê a entrada de uma fusão incremental: os `n_seg`
/// segmentos mais velhos (`idx` 0 a `n_seg-1`) do nível absoluto `i_abs_level`.
fn fts3_incrmerge_csr(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    n_seg: i32,
    p_csr: &mut Fts3MultiSegReader,
) -> i32 {
    *p_csr = Fts3MultiSegReader::default();
    let p_stmt = match fts3_sql_stmt(db, p, SQL_SELECT_LEVEL, &[]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };

    let mut rc = SQLITE_OK;
    bind_int64(db, p_stmt, 1, i_abs_level);
    let mut i = 0;
    while rc == SQLITE_OK && step(db, p_stmt) == SQLITE_ROW && i < n_seg {
        let i_start = column_int64(db, p_stmt, 1); /* segdir.start_block */
        let i_leaf_end = column_int64(db, p_stmt, 2); /* segdir.leaves_end_block */
        let i_end = column_int64(db, p_stmt, 3); /* segdir.end_block */
        let z_root = column_blob(db, p_stmt, 4).map(|b| b.to_vec()); /* segdir.root */
        match fts3_seg_reader_new(i, false, i_start, i_leaf_end, i_end, z_root.as_deref()) {
            Ok(seg) => p_csr.ap_segment.push(seg),
            Err(e) => rc = e,
        }
        i += 1;
    }
    let rc2 = reset(db, p_stmt);
    if rc == SQLITE_OK {
        rc = rc2;
    }

    rc
}

/// `fts3IncrmergePush`: chamada a cada folha (ou nó) gravada. A chave `z_term` é maior que a maior
/// chave do nó que acabou de ir para o disco e menor ou igual à primeira que irá para o próximo.
/// O id do bloco da folha gravada está em `a_node_writer[0].i_block`.
fn fts3_incrmerge_push(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_writer: &mut IncrmergeWriter,
    z_term: &[u8],
) -> i32 {
    let n_term = z_term.len() as i32;
    let mut i_ptr = p_writer.a_node_writer[0].i_block;

    debug_assert!(n_term > 0);
    for i_layer in 1..FTS_MAX_APPENDABLE_HEIGHT {
        let mut i_next_ptr: i64 = 0;
        let mut rc = SQLITE_OK;

        /* O espaço que a chave pede se for gravada no nó corrente da camada `i_layer`. Por causa da
        ** compressão de prefixo, ele depende do nó em que a chave entra. */
        let p_node = &mut p_writer.a_node_writer[i_layer];
        let n_prefix = fts3_prefix_compress(p_node.key.bytes(), z_term);
        let n_suffix = n_term - n_prefix;
        if n_suffix <= 0 {
            return FTS_CORRUPT_VTAB;
        }
        let n_space = fts3_varint_len(n_prefix as u64) + fts3_varint_len(n_suffix as u64) + n_suffix;

        if p_node.key.n == 0 || (p_node.block.n + n_space) <= p.n_node_size {
            /* O nó corrente da camada não tem chaves, ou a chave nova não o faz passar de
            ** `n_node_size` bytes: grava a chave aqui. */
            let p_blk = &mut p_node.block;
            if p_blk.n == 0 {
                p_blk.grow(p.n_node_size.max(0) as usize);
                p_blk.grow(1 + FTS3_VARINT_MAX);
                p_blk.a[0] = i_layer as u8;
                p_blk.n = 1 + put_varint_at(&mut p_blk.a, 1, i_ptr);
            }
            p_blk.grow((p_blk.n + n_space) as usize);
            p_node.key.grow(z_term.len());

            if p_node.key.n != 0 {
                p_blk.put_varint(n_prefix as i64);
            }
            p_blk.put_varint(n_suffix as i64);
            p_blk.put_bytes(&z_term[n_prefix as usize..]);

            p_node.key.a[..z_term.len()].copy_from_slice(z_term);
            p_node.key.n = n_term;
        } else {
            /* Senão grava o nó corrente da camada no disco e aloca um irmão novo, vazio. A chave
            ** vai para o pai. */
            rc = fts3_write_segment(db, p, p_node.i_block, Some(p_node.block.bytes()));

            p_node.block.grow(1 + FTS3_VARINT_MAX);
            p_node.block.a[0] = i_layer as u8;
            p_node.block.n = 1 + put_varint_at(&mut p_node.block.a, 1, i_ptr + 1);

            i_next_ptr = p_node.i_block;
            p_node.i_block += 1;
            p_node.key.n = 0;
        }

        if rc != SQLITE_OK || i_next_ptr == 0 {
            return rc;
        }
        i_ptr = i_next_ptr;
    }

    SQLITE_OK
}

/// `fts3AppendToNode`: acrescenta um termo e (numa folha) uma doclist ao nó em `p_node`. O cabeçalho
/// do nó precisa já estar escrito. `p_prev` tem o termo anterior (vazio no primeiro termo do nó) e
/// sai com a cópia de `z_term`.
fn fts3_append_to_node(
    p_node: &mut Blob,
    p_prev: &mut Blob,
    z_term: &[u8],
    a_doclist: Option<&[u8]>,
) -> i32 {
    let b_first = p_prev.n == 0; /* verdadeiro se é o primeiro termo gravado */

    /* O nó já foi iniciado. */
    debug_assert!(p_node.n > 0);

    p_prev.grow(z_term.len());

    let n_prefix = fts3_prefix_compress(p_prev.bytes(), z_term);
    let n_suffix = z_term.len() as i32 - n_prefix;
    if n_suffix <= 0 {
        return FTS_CORRUPT_VTAB;
    }
    p_prev.a[..z_term.len()].copy_from_slice(z_term);
    p_prev.n = z_term.len() as i32;

    if !b_first {
        p_node.put_varint(n_prefix as i64);
    }
    p_node.put_varint(n_suffix as i64);
    p_node.put_bytes(&z_term[n_prefix as usize..]);

    if let Some(d) = a_doclist {
        p_node.put_varint(d.len() as i64);
        p_node.put_bytes(d);
    }

    SQLITE_OK
}

/// `fts3IncrmergeAppend`: acrescenta o termo e a doclist correntes do cursor ao segmento anexável.
fn fts3_incrmerge_append(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_writer: &mut IncrmergeWriter,
    p_csr: &Fts3MultiSegReader,
) -> i32 {
    let z_term: &[u8] = &p_csr.z_term;
    let a_doclist: &[u8] = &p_csr.a_doclist;
    let n_term = z_term.len() as i32;
    let n_doclist = a_doclist.len() as i32;
    let mut rc = SQLITE_OK;

    let n_prefix = fts3_prefix_compress(p_writer.a_node_writer[0].key.bytes(), z_term);
    let mut n_suffix = n_term - n_prefix;
    if n_suffix <= 0 {
        return FTS_CORRUPT_VTAB;
    }

    /* O espaço total pedido na folha. */
    let mut n_space = fts3_varint_len(n_prefix as u64);
    n_space += fts3_varint_len(n_suffix as u64) + n_suffix;
    n_space += fts3_varint_len(n_doclist as u64) + n_doclist;

    /* Se o bloco corrente não está vazio, o termo e a doclist o fariam passar de `n_node_size` e
    ** ainda há espaço para outra folha, grava este bloco no banco. */
    if p_writer.a_node_writer[0].block.n > 0
        && (p_writer.a_node_writer[0].block.n + n_space) > p.n_node_size
        && p_writer.a_node_writer[0].i_block < (p_writer.i_start + p_writer.n_leaf_est as i64)
    {
        rc = fts3_write_segment(
            db,
            p,
            p_writer.a_node_writer[0].i_block,
            Some(p_writer.a_node_writer[0].block.bytes()),
        );
        p_writer.n_work += 1;

        /* O termo que vai para o pai precisa ser maior que o maior termo da folha recém-gravada
        ** (ainda na chave da folha) e menor ou igual ao termo que vai para a folha nova: o prefixo
        ** de `z_term` um byte mais longo que o prefixo comum dos dois. */
        if rc == SQLITE_OK {
            rc = fts3_incrmerge_push(db, p, p_writer, &z_term[..(n_prefix + 1) as usize]);
        }

        /* Passa ao bloco de saída seguinte. */
        let p_leaf = &mut p_writer.a_node_writer[0];
        p_leaf.i_block += 1;
        p_leaf.key.n = 0;
        p_leaf.block.n = 0;

        n_suffix = n_term;
        n_space = 1;
        n_space += fts3_varint_len(n_suffix as u64) + n_suffix;
        n_space += fts3_varint_len(n_doclist as u64) + n_doclist;
    }

    p_writer.n_leaf_data += n_space as i64;
    let p_leaf = &mut p_writer.a_node_writer[0];
    p_leaf.block.grow((p_leaf.block.n + n_space).max(0) as usize);
    if rc == SQLITE_OK {
        if p_leaf.block.n == 0 {
            p_leaf.block.grow(1);
            p_leaf.block.n = 1;
            p_leaf.block.a[0] = 0;
        }
        rc = fts3_append_to_node(&mut p_leaf.block, &mut p_leaf.key, z_term, Some(a_doclist));
    }

    rc
}

/// `fts3IncrmergeRelease`: grava no disco os buffers de nó pendentes (se `*p_rc` é `SQLITE_OK`) e
/// o registro de `%_segdir`. Com `*p_rc` já um erro, não grava nada.
fn fts3_incrmerge_release(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_writer: &mut IncrmergeWriter,
    p_rc: &mut i32,
) {
    let mut rc = *p_rc;

    /* `i_root` é o índice em `a_node_writer` do nó raiz do segmento de saída: 0 se o segmento cabe
    ** numa folha; 1 se a raiz é o pai das folhas; e assim por diante. */
    let mut i_root = FTS_MAX_APPENDABLE_HEIGHT as isize - 1;
    while i_root >= 0 {
        if p_writer.a_node_writer[i_root as usize].block.n > 0 {
            break;
        }
        i_root -= 1;
    }

    /* Segmento de saída vazio: nada a fazer. */
    if i_root < 0 {
        return;
    }
    let mut i_root = i_root as usize;

    /* O segmento todo cabe num nó. Normalmente ele iria como blob na coluna "root" de `%_segdir`,
    ** mas aqui não pode: o espaço já foi reservado em `%_segments` e os campos `start_block` e
    ** `end_block` de `%_segdir` têm de ser preenchidos, e as versões publicadas do FTS não lidam
    ** com segmentos que cabem na raiz com `start_block!=0`. Cria uma raiz sintética que só tem um
    ** ponteiro para o único nó de conteúdo. */
    if i_root == 0 {
        let i_leaf_block = p_writer.a_node_writer[0].i_block;
        let p_block = &mut p_writer.a_node_writer[1].block;
        p_block.grow(1 + FTS3_VARINT_MAX);
        p_block.a[0] = 0x01;
        p_block.n = 1 + put_varint_at(&mut p_block.a, 1, i_leaf_block);
        i_root = 1;
    }

    /* Grava todos os nós pendentes. */
    for i in 0..i_root {
        let p_node = &p_writer.a_node_writer[i];
        if p_node.block.n > 0 && rc == SQLITE_OK {
            rc = fts3_write_segment(db, p, p_node.i_block, Some(p_node.block.bytes()));
        }
    }

    /* Grava o registro de `%_segdir`. */
    if rc == SQLITE_OK {
        rc = fts3_write_segdir(
            db,
            p,
            p_writer.i_abs_level + 1,
            p_writer.i_idx,
            p_writer.i_start,
            p_writer.a_node_writer[0].i_block,
            p_writer.i_end,
            if !p_writer.b_no_leaf_data { p_writer.n_leaf_data } else { 0 },
            p_writer.a_node_writer[i_root].block.bytes(),
        );
    }

    *p_rc = rc;
}

/// `fts3TermCmp`: compara dois termos com `memcmp`; um prefixo do outro é o menor.
fn fts3_term_cmp(z_lhs: &[u8], z_rhs: &[u8]) -> i32 {
    let n_cmp = z_lhs.len().min(z_rhs.len());
    let res = if n_cmp > 0 { memcmp(&z_lhs[..n_cmp], &z_rhs[..n_cmp]) } else { 0 };
    if res == 0 {
        z_lhs.len() as i32 - z_rhs.len() as i32
    } else {
        res
    }
}

/// `fts3IsAppendable`: `*pb_res` fica verdadeiro se a linha `i_end` de `%_segments` é NULL (o
/// marcador de segmento anexável).
fn fts3_is_appendable(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_end: i64,
    pb_res: &mut bool,
) -> i32 {
    let mut b_res = false;
    let rc = match fts3_sql_stmt(db, p, SQL_SEGMENT_IS_APPENDABLE, &[]) {
        Ok(p_check) => {
            bind_int64(db, p_check, 1, i_end);
            if step(db, p_check) == SQLITE_ROW {
                b_res = true;
            }
            reset(db, p_check)
        }
        Err(rc) => rc,
    };

    *pb_res = b_res;
    rc
}

/// `fts3IncrmergeLoad`: ao iniciar uma fusão incremental, vê se o segmento de índice `i_idx` do
/// nível `i_abs_level+1` aceita anexos e, se sim, inicia `p_writer` para anexar a ele. Aceita se foi
/// criado anexável e se a primeira chave da entrada (`z_key`) é maior que a maior chave dele.
fn fts3_incrmerge_load(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    i_idx: i32,
    z_key: &[u8],
    p_writer: &mut IncrmergeWriter,
) -> i32 {
    let p_select = match fts3_sql_stmt(db, p, SQL_SELECT_SEGDIR, &[]) {
        Ok(st) => st,
        Err(rc) => return rc,
    };

    let mut i_start: i64 = 0; /* segdir.start_block */
    let mut i_leaf_end: i64 = 0; /* segdir.leaves_end_block */
    let mut i_end: i64 = 0; /* segdir.end_block */
    let a_root: Vec<u8>; /* segdir.root */
    let n_root: i32;

    /* Lê a entrada de `%_segdir` do índice `i_idx` do nível absoluto `i_abs_level+1`. */
    bind_int64(db, p_select, 1, i_abs_level + 1);
    bind_int(db, p_select, 2, i_idx);
    if step(db, p_select) == SQLITE_ROW {
        i_start = column_int64(db, p_select, 1);
        i_leaf_end = column_int64(db, p_select, 2);
        fts3_read_end_block_field(db, p_select, 3, &mut i_end, &mut p_writer.n_leaf_data);
        if p_writer.n_leaf_data < 0 {
            p_writer.n_leaf_data = p_writer.n_leaf_data.wrapping_mul(-1);
        }
        p_writer.b_no_leaf_data = p_writer.n_leaf_data == 0;
        n_root = column_bytes(db, p_select, 4);
        match column_blob(db, p_select, 4).map(|b| b.to_vec()) {
            Some(a) => a_root = a,
            None => {
                reset(db, p_select);
                return if n_root != 0 { SQLITE_NOMEM } else { FTS_CORRUPT_VTAB };
            }
        }
    } else {
        return reset(db, p_select);
    }

    /* Confere o marcador de comprimento zero em `%_segments`. */
    let mut b_appendable = false;
    let mut rc = fts3_is_appendable(db, p, i_end, &mut b_appendable);

    /* Confere se `z_key` é maior que a maior chave do candidato. */
    if rc == SQLITE_OK && b_appendable {
        match fts3_read_block(db, p, i_leaf_end, true) {
            Err(e) => rc = e,
            Ok((a_leaf, n_leaf)) => {
                let a_leaf = a_leaf.unwrap_or_default();
                let node = &a_leaf[..(n_leaf.max(0) as usize).min(a_leaf.len())];
                let (mut reader, mut r) = node_reader_init(Some(node), n_leaf);
                while r == SQLITE_OK && reader.a_node.is_some() {
                    r = node_reader_next(&mut reader);
                }
                rc = r;
                if fts3_term_cmp(z_key, reader.term.bytes()) <= 0 {
                    b_appendable = false;
                }
            }
        }
    }

    if rc == SQLITE_OK && b_appendable {
        /* É possível anexar a este segmento: inicia o `IncrmergeWriter`. */
        let n_height = at(&a_root, 0) as i8 as i32;
        if !(1..FTS_MAX_APPENDABLE_HEIGHT as i32).contains(&n_height) {
            reset(db, p_select);
            return FTS_CORRUPT_VTAB;
        }

        p_writer.n_leaf_est = ((i_end - i_start) + 1) as i32 / FTS_MAX_APPENDABLE_HEIGHT as i32;
        p_writer.i_start = i_start;
        p_writer.i_end = i_end;
        p_writer.i_abs_level = i_abs_level;
        p_writer.i_idx = i_idx;

        for i in (n_height + 1)..FTS_MAX_APPENDABLE_HEIGHT as i32 {
            p_writer.a_node_writer[i as usize].i_block =
                p_writer.i_start + i.wrapping_mul(p_writer.n_leaf_est) as i64;
        }

        let nh = n_height as usize;
        p_writer.a_node_writer[nh].i_block =
            p_writer.i_start + p_writer.n_leaf_est.wrapping_mul(n_height) as i64;
        {
            let p_node = &mut p_writer.a_node_writer[nh];
            p_node.block.grow(n_root.max(p.n_node_size).max(0) as usize + FTS3_NODE_PADDING);
            p_node.block.a[..a_root.len()].copy_from_slice(&a_root);
            p_node.block.n = n_root;
            p_node.block.a[a_root.len()..a_root.len() + FTS3_NODE_PADDING].fill(0);
        }

        let mut i = nh as isize;
        while i >= 0 && rc == SQLITE_OK {
            let iu = i as usize;
            if !p_writer.a_node_writer[iu].block.a.is_empty() {
                let (term, i_child, r) = {
                    let blk = &p_writer.a_node_writer[iu].block;
                    let (mut reader, mut r) = node_reader_init(Some(blk.bytes()), blk.n);
                    while reader.a_node.is_some() && r == SQLITE_OK {
                        r = node_reader_next(&mut reader);
                    }
                    (reader.term.bytes().to_vec(), reader.i_child, r)
                };
                rc = r;
                if rc == SQLITE_OK {
                    let p_node = &mut p_writer.a_node_writer[iu];
                    p_node.key.grow(term.len());
                    p_node.key.a[..term.len()].copy_from_slice(&term);
                    p_node.key.n = term.len() as i32;
                    if iu > 0 {
                        p_writer.a_node_writer[iu - 1].i_block = i_child;
                        match fts3_read_block(db, p, i_child, true) {
                            Err(e) => rc = e,
                            Ok((a_block, n_block)) => {
                                let a_block = a_block.unwrap_or_default();
                                let p_lower = &mut p_writer.a_node_writer[iu - 1];
                                let n = (n_block.max(0) as usize).min(a_block.len());
                                p_lower.block.grow(
                                    n_block.max(p.n_node_size).max(0) as usize + FTS3_NODE_PADDING,
                                );
                                p_lower.block.a[..n].copy_from_slice(&a_block[..n]);
                                p_lower.block.n = n_block;
                                p_lower.block.a[n..n + FTS3_NODE_PADDING].fill(0);
                            }
                        }
                    }
                }
            }
            i -= 1;
        }
    }

    let rc2 = reset(db, p_select);
    if rc == SQLITE_OK {
        rc = rc2;
    }
    rc
}

/// `fts3IncrmergeOutputIdx`: o maior `idx` mais um existente no nível `i_abs_level+1` (zero se não
/// há segmentos nele).
fn fts3_incrmerge_output_idx(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    pi_idx: &mut i32,
) -> i32 {
    match fts3_sql_stmt(db, p, SQL_NEXT_SEGMENT_INDEX, &[]) {
        Ok(p_output_idx) => {
            bind_int64(db, p_output_idx, 1, i_abs_level + 1);
            step(db, p_output_idx);
            *pi_idx = column_int(db, p_output_idx, 0);
            reset(db, p_output_idx)
        }
        Err(rc) => rc,
    }
}

/// `fts3IncrmergeWriter`: aloca um segmento de saída anexável no nível `i_abs_level+1` com o
/// `idx` `i_idx`. Estima o máximo de folhas (as folhas dos segmentos de entrada mais dois por
/// segmento) e reserva `16*nLeafEst` blocos: as folhas ficam no começo, os pais delas em seguida, e
/// assim por diante.
fn fts3_incrmerge_writer(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    i_idx: i32,
    p_csr: &Fts3MultiSegReader,
    p_writer: &mut IncrmergeWriter,
) -> i32 {
    let mut n_leaf_est: i32 = 0; /* blocos reservados para folhas */

    /* Calcula `n_leaf_est`. */
    let mut rc = match fts3_sql_stmt(db, p, SQL_MAX_LEAF_NODE_ESTIMATE, &[]) {
        Ok(p_leaf_est) => {
            bind_int64(db, p_leaf_est, 1, i_abs_level);
            bind_int64(db, p_leaf_est, 2, p_csr.ap_segment.len() as i64);
            if step(db, p_leaf_est) == SQLITE_ROW {
                n_leaf_est = column_int(db, p_leaf_est, 0);
            }
            reset(db, p_leaf_est)
        }
        Err(rc) => rc,
    };
    if rc != SQLITE_OK {
        return rc;
    }

    /* Calcula o primeiro bloco do segmento de saída. */
    rc = match fts3_sql_stmt(db, p, SQL_NEXT_SEGMENTS_ID, &[]) {
        Ok(p_first_block) => {
            if step(db, p_first_block) == SQLITE_ROW {
                p_writer.i_start = column_int64(db, p_first_block, 0);
                p_writer.i_end = p_writer.i_start - 1;
                p_writer.i_end +=
                    n_leaf_est.wrapping_mul(FTS_MAX_APPENDABLE_HEIGHT as i32) as i64;
            }
            reset(db, p_first_block)
        }
        Err(rc) => rc,
    };
    if rc != SQLITE_OK {
        return rc;
    }

    /* O marcador em `%_segments` impede que alguém roube o espaço recém-alocado e identifica os
    ** segmentos anexáveis. */
    rc = fts3_write_segment(db, p, p_writer.i_end, None);
    if rc != SQLITE_OK {
        return rc;
    }

    p_writer.i_abs_level = i_abs_level;
    p_writer.n_leaf_est = n_leaf_est;
    p_writer.i_idx = i_idx;

    /* Inicia o vetor de `NodeWriter`. */
    for i in 0..FTS_MAX_APPENDABLE_HEIGHT {
        p_writer.a_node_writer[i].i_block =
            p_writer.i_start + (i as i32).wrapping_mul(p_writer.n_leaf_est) as i64;
    }
    SQLITE_OK
}

/// `fts3RemoveSegdirEntry`: remove a entrada de `idx` `i_idx` do nível `i_abs_level` de `%_segdir`.
fn fts3_remove_segdir_entry(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    i_idx: i32,
) -> i32 {
    match fts3_sql_stmt(db, p, SQL_DELETE_SEGDIR_ENTRY, &[]) {
        Ok(p_delete) => {
            bind_int64(db, p_delete, 1, i_abs_level);
            bind_int(db, p_delete, 2, i_idx);
            step(db, p_delete);
            reset(db, p_delete)
        }
        Err(rc) => rc,
    }
}

/// `fts3RepackSegdirLevel`: segmentos acabaram de sair do nível `i_abs_level`; renumera os
/// restantes para que os `idx` sejam uma sequência contígua a partir de 0.
fn fts3_repack_segdir_level(db: &mut Connection, p: &mut Fts3Table, i_abs_level: i64) -> i32 {
    let mut a_idx: Vec<i32> = Vec::new(); /* os `idx` restantes */

    let mut rc = match fts3_sql_stmt(db, p, SQL_SELECT_INDEXES, &[]) {
        Ok(p_select) => {
            bind_int64(db, p_select, 1, i_abs_level);
            while step(db, p_select) == SQLITE_ROW {
                a_idx.push(column_int(db, p_select, 0));
            }
            reset(db, p_select)
        }
        Err(rc) => rc,
    };

    let mut p_update: Option<StmtId> = None;
    if rc == SQLITE_OK {
        match fts3_sql_stmt(db, p, SQL_SHIFT_SEGDIR_ENTRY, &[]) {
            Ok(st) => p_update = Some(st),
            Err(e) => rc = e,
        }
    }
    if let (SQLITE_OK, Some(p_update)) = (rc, p_update) {
        bind_int64(db, p_update, 2, i_abs_level);

        debug_assert!(!p.b_ignore_savepoint);
        p.b_ignore_savepoint = true;
        let mut i = 0;
        while rc == SQLITE_OK && i < a_idx.len() {
            if a_idx[i] != i as i32 {
                bind_int(db, p_update, 3, a_idx[i]);
                bind_int(db, p_update, 1, i as i32);
                step(db, p_update);
                rc = reset(db, p_update);
            }
            i += 1;
        }
        p.b_ignore_savepoint = false;
    }

    rc
}

/// `fts3StartNode`: o cabeçalho de um nó: a altura e, se não é zero, o filho esquerdo.
fn fts3_start_node(p_node: &mut Blob, i_height: i32, i_child: i64) {
    p_node.grow(1 + FTS3_VARINT_MAX);
    p_node.a[0] = i_height as u8;
    if i_child != 0 {
        p_node.n = 1 + put_varint_at(&mut p_node.a, 1, i_child);
    } else {
        p_node.n = 1;
    }
}

/// `fts3TruncateNode`: cria em `p_new` a imagem de um nó copiando de `a_node` os termos maiores ou
/// iguais a `z_term` (folha) ou maiores que `z_term` (nó interno). `*pi_block` recebe o bloco da
/// camada de baixo.
fn fts3_truncate_node(
    a_node: &[u8],
    p_new: &mut Blob,
    z_term: &[u8],
    pi_block: &mut i64,
) -> i32 {
    let mut prev = Blob::default(); /* o termo anterior gravado no nó novo */

    if a_node.is_empty() {
        return FTS_CORRUPT_VTAB;
    }
    let b_leaf = a_node[0] == 0; /* verdadeiro para folha */

    /* Aloca o espaço de saída. */
    p_new.grow(a_node.len());
    p_new.n = 0;

    /* Monta o buffer do nó novo. */
    let (mut reader, mut rc) = node_reader_init(Some(a_node), a_node.len() as i32);
    while rc == SQLITE_OK && reader.a_node.is_some() {
        if p_new.n == 0 {
            let res = fts3_term_cmp(reader.term.bytes(), z_term);
            if res < 0 || (!b_leaf && res == 0) {
                rc = node_reader_next(&mut reader);
                continue;
            }
            fts3_start_node(p_new, a_node[0] as i32, reader.i_child);
            *pi_block = reader.i_child;
        }
        rc = fts3_append_to_node(p_new, &mut prev, reader.term.bytes(), reader.doclist());
        if rc != SQLITE_OK {
            break;
        }
        rc = node_reader_next(&mut reader);
    }
    if p_new.n == 0 {
        fts3_start_node(p_new, a_node[0] as i32, reader.i_child);
        *pi_block = reader.i_child;
    }
    debug_assert!(p_new.n as usize <= p_new.a.len());

    rc
}

/// `fts3TruncateSegment`: tira do segmento `i_idx` do nível `i_abs_level` todos os termos menores
/// que `z_term`. Pode apagar linhas de `%_segments` e alterar linhas de `%_segments` e `%_segdir`.
fn fts3_truncate_segment(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    i_idx: i32,
    z_term: &[u8],
) -> i32 {
    let mut root = Blob::default(); /* a imagem nova da raiz */
    let mut block = Blob::default(); /* o buffer para qualquer outro bloco */
    let mut i_block: i64 = 0;
    let mut i_new_start: i64 = 0; /* o valor novo de `iStartBlock` */
    let mut i_old_start: i64 = 0; /* o valor antigo de `iStartBlock` */

    let mut rc = match fts3_sql_stmt(db, p, SQL_SELECT_SEGDIR, &[]) {
        Ok(p_fetch) => {
            bind_int64(db, p_fetch, 1, i_abs_level);
            bind_int(db, p_fetch, 2, i_idx);
            let mut rc = SQLITE_OK;
            if step(db, p_fetch) == SQLITE_ROW {
                let a_root = column_blob(db, p_fetch, 4).map(|b| b.to_vec()).unwrap_or_default();
                i_old_start = column_int64(db, p_fetch, 1);
                rc = fts3_truncate_node(&a_root, &mut root, z_term, &mut i_block);
            }
            let rc2 = reset(db, p_fetch);
            if rc == SQLITE_OK {
                rc = rc2;
            }
            rc
        }
        Err(rc) => rc,
    };

    while rc == SQLITE_OK && i_block != 0 {
        i_new_start = i_block;

        match fts3_read_block(db, p, i_block, true) {
            Err(e) => rc = e,
            Ok((a_block, n_block)) => {
                let a_block = a_block.unwrap_or_default();
                let n = (n_block.max(0) as usize).min(a_block.len());
                rc = fts3_truncate_node(&a_block[..n], &mut block, z_term, &mut i_block);
            }
        }
        if rc == SQLITE_OK {
            rc = fts3_write_segment(db, p, i_new_start, Some(block.bytes()));
        }
    }

    /* `i_new_start` é agora a primeira folha válida. */
    if rc == SQLITE_OK && i_new_start != 0 {
        match fts3_sql_stmt(db, p, SQL_DELETE_SEGMENTS_RANGE, &[]) {
            Ok(p_del) => {
                bind_int64(db, p_del, 1, i_old_start);
                bind_int64(db, p_del, 2, i_new_start - 1);
                step(db, p_del);
                rc = reset(db, p_del);
            }
            Err(e) => rc = e,
        }
    }

    if rc == SQLITE_OK {
        match fts3_sql_stmt(db, p, SQL_CHOMP_SEGDIR, &[]) {
            Ok(p_chomp) => {
                bind_int64(db, p_chomp, 1, i_new_start);
                let b = if root.a.is_empty() { None } else { Some(root.bytes()) };
                bind_blob(db, p_chomp, 2, b, root.n, StrDtor::Transient);
                bind_int64(db, p_chomp, 3, i_abs_level);
                bind_int(db, p_chomp, 4, i_idx);
                step(db, p_chomp);
                rc = reset(db, p_chomp);
                bind_null(db, p_chomp, 2);
            }
            Err(e) => rc = e,
        }
    }

    rc
}

/// `fts3IncrmergeChomp`: depois de uma fusão incremental de dois ou mais segmentos do nível
/// `i_abs_level`, cada segmento de entrada ou sai do banco (se todos os dados foram copiados para o
/// de saída) ou é alterado no lugar para não ter mais as entradas já duplicadas. `*pn_rem` recebe
/// o número de segmentos que não foram apagados.
fn fts3_incrmerge_chomp(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_abs_level: i64,
    p_csr: &Fts3MultiSegReader,
    pn_rem: &mut i32,
) -> i32 {
    let mut n_rem = 0;
    let mut rc = SQLITE_OK;
    let n_segment = p_csr.ap_segment.len() as i32;

    let mut i = n_segment - 1;
    while i >= 0 && rc == SQLITE_OK {
        /* Acha o leitor com `iIdx==i`: está escondido em algum lugar de `ap_segment`. */
        let p_seg = p_csr
            .ap_segment
            .iter()
            .find(|s| s.i_idx == i)
            .or_else(|| p_csr.ap_segment.last());
        let Some(p_seg) = p_seg else { break };
        debug_assert!(p_seg.i_idx == i);

        if p_seg.a_node.is_none() {
            /* O leitor está no fim: remove o segmento de entrada inteiro. */
            rc = fts3_delete_segment(db, p, p_seg);
            if rc == SQLITE_OK {
                rc = fts3_remove_segdir_entry(db, p, i_abs_level, p_seg.i_idx);
            }
            *pn_rem = 0;
        } else {
            /* A fusão incremental não copiou todos os dados deste segmento para o nível de cima:
            ** o segmento é alterado no lugar para não ter chaves menores que `z_term`. */
            rc = fts3_truncate_segment(db, p, i_abs_level, p_seg.i_idx, &p_seg.z_term);
            n_rem += 1;
        }
        i -= 1;
    }

    if rc == SQLITE_OK && n_rem != n_segment {
        rc = fts3_repack_segdir_level(db, p, i_abs_level);
    }

    *pn_rem = n_rem;
    rc
}

/// `fts3IncrmergeHintStore`: grava a dica de fusão incremental no banco.
fn fts3_incrmerge_hint_store(db: &mut Connection, p: &mut Fts3Table, p_hint: &Blob) -> i32 {
    match fts3_sql_stmt(db, p, SQL_REPLACE_STAT, &[]) {
        Ok(p_replace) => {
            bind_int(db, p_replace, 1, FTS_STAT_INCRMERGEHINT);
            let b = if p_hint.a.is_empty() { None } else { Some(p_hint.bytes()) };
            bind_blob(db, p_replace, 2, b, p_hint.n, StrDtor::Transient);
            step(db, p_replace);
            let rc = reset(db, p_replace);
            bind_null(db, p_replace, 2);
            rc
        }
        Err(rc) => rc,
    }
}

/// `fts3IncrmergeHintLoad`: lê a dica de fusão incremental, que fica na linha `rowid==1` de `%_stat`.
fn fts3_incrmerge_hint_load(db: &mut Connection, p: &mut Fts3Table, p_hint: &mut Blob) -> i32 {
    p_hint.n = 0;
    match fts3_sql_stmt(db, p, SQL_SELECT_STAT, &[]) {
        Ok(p_select) => {
            bind_int(db, p_select, 1, FTS_STAT_INCRMERGEHINT);
            if step(db, p_select) == SQLITE_ROW {
                let a_hint = column_blob(db, p_select, 0).map(|b| b.to_vec());
                let n_hint = column_bytes(db, p_select, 0);
                if let Some(a_hint) = a_hint {
                    p_hint.grow(n_hint.max(0) as usize);
                    let n = a_hint.len().min(n_hint.max(0) as usize);
                    p_hint.a[..n].copy_from_slice(&a_hint[..n]);
                    p_hint.n = n_hint;
                }
            }
            reset(db, p_select)
        }
        Err(rc) => rc,
    }
}

/// `fts3IncrmergeHintPush`: acrescenta uma entrada (o nível absoluto e o número de segmentos de
/// entrada, dois varints) à dica. Não faz nada se `*p_rc` não é `SQLITE_OK`.
fn fts3_incrmerge_hint_push(p_hint: &mut Blob, i_abs_level: i64, n_input: i32, p_rc: &mut i32) {
    if *p_rc == SQLITE_OK {
        p_hint.grow((p_hint.n + 2 * FTS3_VARINT_MAX as i32).max(0) as usize);
        p_hint.put_varint(i_abs_level);
        p_hint.put_varint(n_input as i64);
    }
}

/// `fts3IncrmergeHintPop`: lê e remove a última entrada (a mais recente) da dica.
fn fts3_incrmerge_hint_pop(p_hint: &mut Blob, pi_abs_level: &mut i64, pn_input: &mut i32) -> i32 {
    let n_hint = p_hint.n;
    let mut i = p_hint.n - 1;
    if (at(&p_hint.a, i.max(0) as usize) & 0x80) != 0 {
        return FTS_CORRUPT_VTAB;
    }
    while i > 0 && (at(&p_hint.a, (i - 1) as usize) & 0x80) != 0 {
        i -= 1;
    }
    if i == 0 {
        return FTS_CORRUPT_VTAB;
    }
    i -= 1;
    while i > 0 && (at(&p_hint.a, (i - 1) as usize) & 0x80) != 0 {
        i -= 1;
    }

    p_hint.n = i;
    let (n, v) = fts3_get_varint(sl(&p_hint.a, i as usize));
    i += n;
    *pi_abs_level = v;
    let (n, v) = fts3_get_varint32(sl(&p_hint.a, i as usize));
    i += n;
    *pn_input = v;
    debug_assert!(i <= n_hint);
    if i != n_hint {
        return FTS_CORRUPT_VTAB;
    }

    SQLITE_OK
}

/// `sqlite3Fts3Incrmerge`: tenta uma fusão incremental que grava `n_merge` blocos folha. As fusões
/// são de `n_min` segmentos por vez: os `n_min` mais velhos (os de menor `idx`) do nível mais alto
/// com pelo menos `n_min` segmentos. Podem ocorrer várias fusões até a cota de `n_merge` folhas.
pub fn fts3_incrmerge(db: &mut Connection, p: &mut Fts3Table, n_merge: i32, n_min: i32) -> i32 {
    let mut n_rem = n_merge; /* folhas ainda por gravar */
    let mut n_seg: i32 = 0; /* número de segmentos de entrada */
    let mut i_abs_level: i64 = 0; /* o nível absoluto em que se trabalha */
    let mut hint = Blob::default(); /* a dica lida de `%_stat` */
    let mut b_dirty_hint = false; /* verdadeiro se `hint` foi alterada */

    let mut rc = fts3_incrmerge_hint_load(db, p, &mut hint);
    while rc == SQLITE_OK && n_rem > 0 {
        let n_mod: i64 = FTS3_SEGDIR_MAXLEVEL as i64 * p.n_index as i64;
        let mut b_use_hint = false; /* verdadeiro se tenta anexar */
        let mut i_idx: i32 = 0; /* o maior `idx` do nível `i_abs_level+1` */

        /* Procura em `%_segdir` o nível absoluto de menor nível relativo com pelo menos `n_min`
        ** segmentos, se houver: `i_abs_level` é ele e `n_seg` é `n_min`. Se nenhum nível tem tantos
        ** segmentos, `n_seg` é -1. */
        match fts3_sql_stmt(db, p, SQL_FIND_MERGE_LEVEL, &[]) {
            Ok(p_find_level) => {
                bind_int(db, p_find_level, 1, n_min.max(2));
                if step(db, p_find_level) == SQLITE_ROW {
                    i_abs_level = column_int64(db, p_find_level, 0);
                    n_seg = column_int(db, p_find_level, 1);
                    debug_assert!(n_seg >= 2);
                } else {
                    n_seg = -1;
                }
                rc = reset(db, p_find_level);
            }
            Err(_) => {
                /* `pFindLevel==NULL`: ver o cabeçalho do módulo. */
                n_seg = -1;
                rc = SQLITE_OK;
            }
        }

        /* Se a dica lida de `%_stat` não está vazia, vê se a última entrada dela é de um nível
        ** relativo menor ou igual ao achado acima. Se for, esta volta funde no nível da dica. */
        if rc == SQLITE_OK && hint.n != 0 {
            let n_hint = hint.n;
            let mut i_hint_abs_level: i64 = 0; /* o nível da dica */
            let mut n_hint_seg: i32 = 0; /* o número de segmentos da dica */

            rc = fts3_incrmerge_hint_pop(&mut hint, &mut i_hint_abs_level, &mut n_hint_seg);
            if n_seg < 0 || (i_abs_level % n_mod) >= (i_hint_abs_level % n_mod) {
                /* Pela varredura acima, nenhum nível de nível relativo menor que o de
                ** `i_abs_level` tem mais de `n_seg` segmentos (ou, com `n_seg==-1`, mais de
                ** `n_min`). Isto limita `n_hint_seg` para evitar uma alocação grande se a dica
                ** está corrompida. */
                i_abs_level = i_hint_abs_level;
                n_seg = n_min.max(n_seg).min(n_hint_seg);
                b_use_hint = true;
                b_dirty_hint = true;
            } else {
                /* Desfaz o `HintPop()`: nenhuma entrada sai da dica. */
                hint.n = n_hint;
            }
        }

        /* Com `n_seg<=0` não há nível com `n_min` segmentos nem dica: nada a fazer. */
        if n_seg <= 0 {
            break;
        }

        debug_assert!(n_mod <= 0x7FFF_FFFF);
        if i_abs_level < 0 || i_abs_level > (n_mod << 32) {
            rc = FTS_CORRUPT_VTAB;
            break;
        }

        /* Abre um cursor sobre os `n_seg` índices mais velhos do nível `i_abs_level`. Aberto com os
        ** parâmetros da dica, pode haver menos de `n_seg` segmentos: nada se faz em
        ** `i_abs_level` e passa-se à volta seguinte. */
        let mut writer = IncrmergeWriter::default();
        let mut filter = Fts3SegFilter { flags: FTS3_SEGMENT_REQUIRE_POS, ..Default::default() };
        let mut csr = Fts3MultiSegReader::default();

        if rc == SQLITE_OK {
            rc = fts3_incrmerge_output_idx(db, p, i_abs_level, &mut i_idx);
            if i_idx == 0 || (b_use_hint && i_idx == 1) {
                let mut b_ignore = false;
                rc = fts3_segment_is_max_level(db, p, i_abs_level + 1, &mut b_ignore);
                if b_ignore {
                    filter.flags |= FTS3_SEGMENT_IGNORE_EMPTY;
                }
            }
        }

        if rc == SQLITE_OK {
            rc = fts3_incrmerge_csr(db, p, i_abs_level, n_seg, &mut csr);
        }
        let mut run = rc == SQLITE_OK && csr.ap_segment.len() as i32 == n_seg;
        if run {
            rc = fts3_seg_reader_start(db, p, &mut csr, &filter);
            run = rc == SQLITE_OK;
        }
        if run {
            let mut b_empty = false;
            rc = fts3_seg_reader_step(db, p, &mut csr);
            if rc == SQLITE_OK {
                b_empty = true;
            } else if rc != SQLITE_ROW {
                fts3_seg_reader_finish(db, &mut csr);
                break;
            }
            if b_use_hint && i_idx > 0 {
                let z_key = csr.z_term.clone();
                rc = fts3_incrmerge_load(db, p, i_abs_level, i_idx - 1, &z_key, &mut writer);
            } else {
                rc = fts3_incrmerge_writer(db, p, i_abs_level, i_idx, &csr, &mut writer);
            }

            if rc == SQLITE_OK && writer.n_leaf_est != 0 {
                if !b_empty {
                    loop {
                        rc = fts3_incrmerge_append(db, p, &mut writer, &csr);
                        if rc == SQLITE_OK {
                            rc = fts3_seg_reader_step(db, p, &mut csr);
                        }
                        if writer.n_work >= n_rem && rc == SQLITE_ROW {
                            rc = SQLITE_OK;
                        }
                        if rc != SQLITE_ROW {
                            break;
                        }
                    }
                }

                /* Atualiza ou apaga os segmentos de entrada. */
                if rc == SQLITE_OK {
                    n_rem -= 1 + writer.n_work;
                    rc = fts3_incrmerge_chomp(db, p, i_abs_level, &csr, &mut n_seg);
                    if n_seg != 0 {
                        b_dirty_hint = true;
                        fts3_incrmerge_hint_push(&mut hint, i_abs_level, n_seg, &mut rc);
                    }
                }
            }

            if n_seg != 0 {
                writer.n_leaf_data = writer.n_leaf_data.wrapping_mul(-1);
            }
            fts3_incrmerge_release(db, p, &mut writer, &mut rc);
            if n_seg == 0 && !writer.b_no_leaf_data {
                fts3_promote_segments(db, p, i_abs_level + 1, writer.n_leaf_data);
            }
        }

        fts3_seg_reader_finish(db, &mut csr);
    }

    /* Grava a dica em `%_stat` para a próxima fusão incremental. */
    if b_dirty_hint && rc == SQLITE_OK {
        rc = fts3_incrmerge_hint_store(db, p, &hint);
    }

    rc
}

// ---------------------------------------------------------------------------------------------
// Comandos especiais
// ---------------------------------------------------------------------------------------------

/// `fts3Getint`: o inteiro decimal que começa em `z[*pos]`; avança `*pos` para depois dele.
fn fts3_getint(z: &[u8], pos: &mut usize) -> i32 {
    let mut i: i32 = 0;
    while at(z, *pos).is_ascii_digit() && i < 214748363 {
        i = 10 * i + (at(z, *pos) - b'0') as i32;
        *pos += 1;
    }
    i
}

/// `fts3DoIncrmerge`: `INSERT INTO table(table) VALUES('merge=A,B')`: `A` é o número de folhas
/// gravadas pela fusão e `B` o mínimo de segmentos de um nível para ele ser escolhido.
fn fts3_do_incrmerge(db: &mut Connection, p: &mut Fts3Table, z_param: &[u8]) -> i32 {
    let mut n_min = FTS3_MERGE_COUNT / 2;
    let mut pos = 0usize;

    /* Lê o primeiro inteiro. */
    let n_merge = fts3_getint(z_param, &mut pos);

    /* Se o primeiro inteiro é seguido de ',', lê o segundo. */
    if at(z_param, pos) == b',' && at(z_param, pos + 1) != 0 {
        pos += 1;
        n_min = fts3_getint(z_param, &mut pos);
    }

    if at(z_param, pos) != 0 || n_min < 2 {
        SQLITE_ERROR
    } else {
        let mut rc = SQLITE_OK;
        if p.b_has_stat == 0 {
            debug_assert!(!p.b_fts4);
            fts3_create_stat_table(db, p, &mut rc);
        }
        if rc == SQLITE_OK {
            rc = fts3_incrmerge(db, p, n_merge, n_min);
        }
        fts3_segments_close(db, p);
        rc
    }
}

/// `fts3DoAutoincrmerge`: `INSERT INTO table(table) VALUES('automerge=X')`: `X==0` desliga o
/// automerge; senão liga. O valor é persistente.
fn fts3_do_autoincrmerge(db: &mut Connection, p: &mut Fts3Table, z_param: &[u8]) -> i32 {
    let mut pos = 0usize;
    p.n_autoincrmerge = fts3_getint(z_param, &mut pos);
    if p.n_autoincrmerge == 1 || p.n_autoincrmerge > FTS3_MERGE_COUNT {
        p.n_autoincrmerge = 8;
    }
    if p.b_has_stat == 0 {
        let mut rc = SQLITE_OK;
        debug_assert!(!p.b_fts4);
        fts3_create_stat_table(db, p, &mut rc);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    match fts3_sql_stmt(db, p, SQL_REPLACE_STAT, &[]) {
        Ok(p_stmt) => {
            bind_int(db, p_stmt, 1, FTS_STAT_AUTOINCRMERGE);
            bind_int(db, p_stmt, 2, p.n_autoincrmerge);
            step(db, p_stmt);
            reset(db, p_stmt)
        }
        Err(rc) => rc,
    }
}

/// `fts3ChecksumEntry`: o checksum de 64 bits de uma entrada do índice FTS.
fn fts3_checksum_entry(
    z_term: &[u8],
    i_langid: i32,
    i_index: i32,
    i_docid: i64,
    i_col: i32,
    i_pos: i32,
) -> u64 {
    let mut ret = i_docid as u64;

    ret = ret.wrapping_add((ret << 3).wrapping_add(i_langid as i64 as u64));
    ret = ret.wrapping_add((ret << 3).wrapping_add(i_index as i64 as u64));
    ret = ret.wrapping_add((ret << 3).wrapping_add(i_col as i64 as u64));
    ret = ret.wrapping_add((ret << 3).wrapping_add(i_pos as i64 as u64));
    for &c in z_term {
        /* `zTerm[i]` é `char`, com sinal no x86-64. */
        ret = ret.wrapping_add((ret << 3).wrapping_add(c as i8 as i64 as u64));
    }

    ret
}

/// `fts3ChecksumIndex`: o checksum de todas as entradas do índice `i_index` do idioma `i_langid`
/// (o XOR dos checksums de cada entrada). Sem efeito (devolve 0) se `*p_rc` já é erro.
fn fts3_checksum_index(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    p_rc: &mut i32,
) -> u64 {
    let mut cksum: u64 = 0;

    if *p_rc != SQLITE_OK {
        return 0;
    }

    let filter = Fts3SegFilter {
        flags: FTS3_SEGMENT_REQUIRE_POS | FTS3_SEGMENT_IGNORE_EMPTY | FTS3_SEGMENT_SCAN,
        ..Default::default()
    };
    let mut csr = Fts3MultiSegReader::default();

    let mut rc =
        fts3_seg_reader_cursor(db, p, i_langid, i_index, FTS3_SEGCURSOR_ALL, None, false, true, &mut csr);
    if rc == SQLITE_OK {
        rc = fts3_seg_reader_start(db, p, &mut csr, &filter);
    }

    if rc == SQLITE_OK {
        loop {
            rc = fts3_seg_reader_step(db, p, &mut csr);
            if rc != SQLITE_ROW {
                break;
            }
            let a = &csr.a_doclist[..];
            let p_end = a.len();
            let mut pos = 0usize;

            let mut i_docid: i64 = 0;
            let mut i_col: i64 = 0;
            let mut i_pos: u64 = 0;

            let (n, v) = fts3_get_varint(sl(a, pos));
            pos += n as usize;
            i_docid = i_docid.wrapping_add(v);
            while pos < p_end {
                let (n, mut i_val) = fts3_get_varint_u(sl(a, pos));
                pos += n as usize;
                if pos < p_end {
                    if i_val == 0 || i_val == 1 {
                        i_col = 0;
                        i_pos = 0;
                        if i_val != 0 {
                            let (n, v) = fts3_get_varint(sl(a, pos));
                            pos += n as usize;
                            i_col = v;
                        } else {
                            let (n, v) = fts3_get_varint_u(sl(a, pos));
                            pos += n as usize;
                            i_val = v;
                            if p.b_desc_idx {
                                i_docid = (i_docid as u64).wrapping_sub(i_val) as i64;
                            } else {
                                i_docid = (i_docid as u64).wrapping_add(i_val) as i64;
                            }
                        }
                    } else {
                        i_pos = i_pos.wrapping_add(i_val.wrapping_sub(2));
                        cksum ^= fts3_checksum_entry(
                            &csr.z_term,
                            i_langid,
                            i_index,
                            i_docid,
                            i_col as i32,
                            i_pos as i32,
                        );
                    }
                }
            }
        }
    }
    fts3_seg_reader_finish(db, &mut csr);

    *p_rc = rc;
    cksum
}

/// `sqlite3Fts3IntegrityCheck`: confere se o índice FTS bate com o conteúdo atual da tabela de
/// conteúdo. `*pb_ok` é verdadeiro se bate. Devolve um erro (OOM, E/S) ou `SQLITE_OK`.
pub fn fts3_integrity_check(db: &mut Connection, p: &mut Fts3Table, pb_ok: &mut bool) -> i32 {
    let mut cksum1: u64 = 0; /* o checksum pelo índice FTS */
    let mut cksum2: u64 = 0; /* o checksum pelo conteúdo de `%_content` */

    /* Calcula o checksum pelo índice FTS. */
    let mut rc = SQLITE_OK;
    match fts3_sql_stmt(db, p, SQL_SELECT_ALL_LANGID, &[]) {
        Ok(p_all_langid) => {
            bind_int(db, p_all_langid, 1, p.i_prev_langid);
            bind_int(db, p_all_langid, 2, p.n_index);
            while rc == SQLITE_OK && step(db, p_all_langid) == SQLITE_ROW {
                let i_langid = column_int(db, p_all_langid, 0);
                for i in 0..p.n_index {
                    cksum1 ^= fts3_checksum_index(db, p, i_langid, i, &mut rc);
                }
            }
            let rc2 = reset(db, p_all_langid);
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
        Err(e) => rc = e,
    }

    /* Calcula o checksum pela tabela `%_content`. */
    if rc == SQLITE_OK {
        let mut p_stmt: Option<StmtId> = None;
        match mprintf(b"SELECT %s", &[PrintfArg::Text(p.z_read_exprlist.clone())]) {
            None => rc = SQLITE_NOMEM,
            Some(z_sql) => {
                let (rc2, stmt, _) = prepare_v2(db, &z_sql, -1);
                rc = rc2;
                p_stmt = stmt;
            }
        }
        let tokenizer = p.p_tokenizer.clone();

        while rc == SQLITE_OK {
            let Some(st) = p_stmt else { break };
            if step(db, st) != SQLITE_ROW {
                break;
            }
            let i_docid = column_int64(db, st, 0);
            let i_lang = langid_from_select(db, p, st);

            let mut i_col = 0;
            while rc == SQLITE_OK && i_col < p.n_column() {
                if at(&p.ab_notindexed, i_col as usize) == 0 {
                    let z_text = column_text(db, st, i_col + 1).map(|s| s.to_vec());
                    let Some(tok) = tokenizer.as_ref() else {
                        rc = SQLITE_ERROR;
                        break;
                    };
                    match fts3_open_tokenizer(
                        &**tok,
                        i_lang,
                        fts3_c_str(z_text.as_deref().unwrap_or(&[])),
                    ) {
                        Err(e) => rc = e,
                        Ok(mut p_t) => {
                            while rc == SQLITE_OK {
                                match p_t.next() {
                                    Err(e) => rc = e,
                                    Ok(t) => {
                                        cksum2 ^= fts3_checksum_entry(
                                            t.z, i_lang, 0, i_docid, i_col, t.i_position,
                                        );
                                        for i in 1..p.n_index {
                                            let n_prefix = p.a_index[i as usize].n_prefix;
                                            if n_prefix <= t.z.len() as i32 {
                                                cksum2 ^= fts3_checksum_entry(
                                                    &t.z[..n_prefix as usize],
                                                    i_lang,
                                                    i,
                                                    i_docid,
                                                    i_col,
                                                    t.i_position,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                    }
                }
                i_col += 1;
            }
        }

        if let Some(st) = p_stmt {
            finalize(db, st);
        }
    }

    if rc == SQLITE_CORRUPT_VTAB {
        rc = SQLITE_OK;
        *pb_ok = false;
    } else {
        *pb_ok = rc == SQLITE_OK && cksum1 == cksum2;
    }
    rc
}

/// `fts3DoIntegrityCheck`: roda a verificação; `SQLITE_CORRUPT_VTAB` se o índice não bate.
fn fts3_do_integrity_check(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut b_ok = false;
    let mut rc = fts3_integrity_check(db, p, &mut b_ok);
    if rc == SQLITE_OK && !b_ok {
        rc = FTS_CORRUPT_VTAB;
    }
    rc
}

/// `fts3SpecialInsert`: o `INSERT` especial `INSERT INTO tbl(tbl) VALUES(<expr>)`; `p_val` é o
/// resultado de `<expr>`.
fn fts3_special_insert(db: &mut Connection, p: &mut Fts3Table, p_val: &Mem) -> i32 {
    let mut rc = SQLITE_ERROR;
    let Some(z_val_cow) = text_of(p_val) else {
        return SQLITE_NOMEM;
    };
    let z_val: &[u8] = &z_val_cow;
    let n_val = z_val.len();

    if n_val == 8 && strnicmp(Some(z_val), Some(b"optimize"), 8) == 0 {
        rc = fts3_do_optimize(db, p, false);
    } else if n_val == 7 && strnicmp(Some(z_val), Some(b"rebuild"), 7) == 0 {
        rc = fts3_do_rebuild(db, p);
    } else if n_val == 15 && strnicmp(Some(z_val), Some(b"integrity-check"), 15) == 0 {
        rc = fts3_do_integrity_check(db, p);
    } else if n_val > 6 && strnicmp(Some(z_val), Some(b"merge="), 6) == 0 {
        rc = fts3_do_incrmerge(db, p, &z_val[6..]);
    } else if n_val > 10 && strnicmp(Some(z_val), Some(b"automerge="), 10) == 0 {
        rc = fts3_do_autoincrmerge(db, p, &z_val[10..]);
    } else if n_val == 5 && strnicmp(Some(z_val), Some(b"flush"), 5) == 0 {
        rc = fts3_pending_terms_flush(db, p);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Tokens adiados
// ---------------------------------------------------------------------------------------------

/// `sqlite3Fts3FreeDeferredDoclists`: apaga as doclists adiadas em cache (alocadas por
/// `fts3_cache_deferred_doclists`).
pub fn fts3_free_deferred_doclists(p_csr: &mut Fts3Cursor) {
    for p_def in p_csr.p_deferred.iter_mut() {
        p_def.p_list = None;
    }
}

/// `sqlite3Fts3FreeDeferredTokens`: apaga todos os tokens de `p_csr.p_deferred` (acrescentados por
/// `fts3_defer_token`).
pub fn fts3_free_deferred_tokens(p_csr: &mut Fts3Cursor) {
    p_csr.p_deferred.clear();
}

/// `sqlite3Fts3CacheDeferredDoclists`: gera as doclists adiadas de todos os tokens de
/// `p_csr.p_deferred` a partir da linha para a qual o cursor aponta. Uma doclist adiada é como as
/// outras com posições, mas só tem entradas de uma linha da tabela.
pub fn fts3_cache_deferred_doclists(
    db: &mut Connection,
    p: &Fts3Table,
    p_csr: &mut Fts3Cursor,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p_csr.p_deferred.is_empty() {
        return rc;
    }
    debug_assert!(!p_csr.is_require_seek);
    let Some(p_stmt) = p_csr.p_stmt else {
        return SQLITE_ERROR;
    };
    let Some(p_t) = p.p_tokenizer.clone() else {
        return SQLITE_ERROR;
    };
    let i_docid = column_int64(db, p_stmt, 0); /* o docid da linha do cursor */

    let mut i = 0;
    while i < p.n_column() && rc == SQLITE_OK {
        if at(&p.ab_notindexed, i as usize) == 0 {
            let z_text = column_text(db, p_stmt, i + 1).map(|s| s.to_vec());

            match fts3_open_tokenizer(&*p_t, p_csr.i_langid, fts3_c_str(z_text.as_deref().unwrap_or(&[]))) {
                Err(e) => rc = e,
                Ok(mut p_tc) => {
                    while rc == SQLITE_OK {
                        let tok = match p_tc.next() {
                            Ok(t) => t,
                            Err(e) => {
                                rc = e;
                                break;
                            }
                        };
                        /* A lista do C insere na frente: a cabeça é o último elemento. */
                        for p_def in p_csr.p_deferred.iter_mut().rev() {
                            let n = p_def.z.len();
                            if (p_def.i_col >= p.n_column() || p_def.i_col == i)
                                && (!p_def.b_first || tok.i_position == 0)
                                && (n == tok.z.len() || (p_def.is_prefix && n < tok.z.len()))
                                && tok.z[..n] == p_def.z[..]
                            {
                                pending_list_append_opt(
                                    &mut p_def.p_list,
                                    i_docid,
                                    i as i64,
                                    tok.i_position as i64,
                                );
                            }
                        }
                    }
                }
            }
            if rc == SQLITE_DONE {
                rc = SQLITE_OK;
            }
        }
        i += 1;
    }

    for p_def in p_csr.p_deferred.iter_mut().rev() {
        if rc != SQLITE_OK {
            break;
        }
        if p_def.p_list.is_some() {
            pending_list_append_varint_opt(&mut p_def.p_list, 0);
        }
    }

    rc
}

/// `sqlite3Fts3DeferredTokenList`: a lista de posições (sem o docid) de um token adiado, ou `None`
/// se ele não tem doclist.
pub fn fts3_deferred_token_list(p: &Fts3DeferredToken) -> Result<Option<Vec<u8>>, i32> {
    let Some(list) = p.p_list.as_ref() else {
        return Ok(None);
    };
    let (n_skip, _) = fts3_get_varint(&list.a_data);
    Ok(Some(sl(&list.a_data, n_skip as usize).to_vec()))
}

/// `sqlite3Fts3DeferToken`: acrescenta o token `p_token` à lista `p_csr.p_deferred`. `i_col` é a
/// coluna em que o token precisa ocorrer (ou -1). O token da frase guarda o índice do elemento.
pub fn fts3_defer_token(
    p_csr: &mut Fts3Cursor,
    p_token: &mut Fts3PhraseToken,
    i_col: i32,
) -> i32 {
    debug_assert!(p_token.p_deferred.is_none());
    p_csr.p_deferred.push(Fts3DeferredToken {
        z: p_token.z.clone(),
        is_prefix: p_token.is_prefix,
        b_first: p_token.b_first,
        i_col,
        p_list: None,
    });
    p_token.p_deferred = Some(p_csr.p_deferred.len() - 1);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// xUpdate e optimize
// ---------------------------------------------------------------------------------------------

/// `fts3DeleteByRowid`: o valor `p_rowid` é o rowid de uma linha que pode ou não estar na tabela.
/// Se está, apaga-a e ajusta as estruturas auxiliares. `a_sz` tem os tamanhos dos documentos
/// apagados (a primeira metade) e dos inseridos (a segunda), como o `aSzDel`/`aSzIns` contíguos do C.
fn fts3_delete_by_rowid(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_rowid: &Mem,
    pn_chng: &mut i32,
    a_sz: &mut [u32],
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut b_found = false; /* verdadeiro se `p_rowid` está de fato na tabela */
    let n_col1 = p.n_column() as usize + 1;

    fts3_delete_terms(&mut rc, db, p, p_rowid, &mut a_sz[..n_col1], &mut b_found);
    if b_found && rc == SQLITE_OK {
        let mut is_empty = 0; /* apagar `p_rowid` deixa a tabela vazia */
        rc = fts3_is_empty(db, p, p_rowid, &mut is_empty);
        if rc == SQLITE_OK {
            if is_empty != 0 {
                /* Apagar esta linha esvazia a tabela: apaga o conteúdo das três tabelas e
                ** descarta os termos pendentes. */
                rc = fts3_delete_all(db, p, true);
                *pn_chng = 0;
                a_sz.fill(0);
            } else {
                *pn_chng -= 1;
                if p.z_content_tbl.is_none() {
                    fts3_sql_exec(&mut rc, db, p, SQL_DELETE_CONTENT, std::slice::from_ref(p_rowid));
                }
                if p.b_has_docsize {
                    fts3_sql_exec(&mut rc, db, p, SQL_DELETE_DOCSIZE, std::slice::from_ref(p_rowid));
                }
            }
        }
    }

    rc
}

/// `sqlite3Fts3UpdateMethod`: o trabalho do `xUpdate` das tabelas FTS3. O esquema da tabela é
/// `<colunas>, <nome da tabela> HIDDEN, docid HIDDEN, <langid> HIDDEN`. `ap_val` é o do `xUpdate`
/// (um valor para `DELETE`; `2 + nColumn + 3` para `INSERT`/`UPDATE`) e `p_rowid` o rowid afetado.
pub fn fts3_update_method(
    db: &mut Connection,
    p: &mut Fts3Table,
    ap_val: &[Mem],
    p_rowid: &mut i64,
) -> i32 {
    let n_arg = ap_val.len();
    let n_column = p.n_column() as usize;
    let mut rc;
    let mut n_chng: i32 = 0; /* a mudança líquida do número de documentos */
    let mut b_insert_done = false;

    /* Aqui já se sabe se `%_stat` existe: `b_has_stat` não é 2. */
    debug_assert!(p.b_has_stat == 0 || p.b_has_stat == 1);

    debug_assert!(p.p_segments.is_none());
    debug_assert!(n_arg == 1 || n_arg == 2 + n_column + 3);

    'update_out: {
        /* Procura um `INSERT` "especial": `INSERT INTO xyz(xyz) VALUES('command')`. */
        if n_arg > 1
            && value_type(&ap_val[0]) == SQLITE_NULL
            && value_type(&ap_val[n_column + 2]) != SQLITE_NULL
        {
            rc = fts3_special_insert(db, p, &ap_val[n_column + 2]);
            break 'update_out;
        }

        if n_arg > 1 && value_int(&ap_val[2 + n_column + 2]) < 0 {
            rc = SQLITE_CONSTRAINT;
            break 'update_out;
        }

        /* O espaço para a mudança dos tamanhos dos documentos: `aSzDel` e `aSzIns` contíguos. */
        let n_col1 = n_column + 1;
        let mut a_sz = vec![0u32; n_col1 * 2];

        rc = fts3_writelock(db, p);
        if rc != SQLITE_OK {
            break 'update_out;
        }

        /* Um `INSERT`, ou um `UPDATE` que muda o rowid, pede tratamento de restrições. Com
        ** `ON CONFLICT REPLACE` apaga-se a linha existente antes de inserir a nova. Com outro
        ** modo, é preciso detectar o conflito e devolver `SQLITE_CONSTRAINT` antes de mexer no
        ** arquivo. */
        if n_arg > 1 && p.z_content_tbl.is_none() {
            /* Acha o valor do rowid novo. */
            let mut p_new_rowid = &ap_val[3 + n_column];
            if value_type(p_new_rowid) == SQLITE_NULL {
                p_new_rowid = &ap_val[1];
            }

            if value_type(p_new_rowid) != SQLITE_NULL
                && (value_type(&ap_val[0]) == SQLITE_NULL
                    || value_int64(&ap_val[0]) != value_int64(p_new_rowid))
            {
                /* O rowid novo não é NULL (senão seria atribuído automaticamente e não haveria
                ** conflito) e o comando é um `INSERT` ou um `UPDATE` que muda a coluna rowid. Com
                ** `REPLACE`, apaga a linha de rowid igual; senão insere o registro novo em
                ** `%_content` e, em caso de conflito (ou outro erro), volta já. */
                if vtab_on_conflict(db) == SQLITE_REPLACE {
                    rc = fts3_delete_by_rowid(db, p, p_new_rowid, &mut n_chng, &mut a_sz);
                } else {
                    rc = fts3_insert_data(db, p, ap_val, p_rowid);
                    b_insert_done = true;
                }
            }
        }
        if rc != SQLITE_OK {
            break 'update_out;
        }

        /* Um `DELETE` ou `UPDATE` remove o registro antigo. */
        if value_type(&ap_val[0]) != SQLITE_NULL {
            rc = fts3_delete_by_rowid(db, p, &ap_val[0], &mut n_chng, &mut a_sz);
        }

        /* Um `INSERT` ou `UPDATE` insere o registro novo. */
        if n_arg > 1 && rc == SQLITE_OK {
            let i_langid = value_int(&ap_val[2 + n_column + 2]);
            if !b_insert_done {
                rc = fts3_insert_data(db, p, ap_val, p_rowid);
                if rc == SQLITE_CONSTRAINT && p.z_content_tbl.is_none() {
                    rc = FTS_CORRUPT_VTAB;
                }
            }
            if rc == SQLITE_OK {
                rc = fts3_pending_terms_docid(db, p, false, i_langid, *p_rowid);
            }
            if rc == SQLITE_OK {
                debug_assert!(p.i_prev_docid == *p_rowid);
                rc = fts3_insert_terms(p, i_langid, ap_val, &mut a_sz[n_col1..]);
            }
            if p.b_has_docsize {
                fts3_insert_docsize(&mut rc, db, p, &a_sz[n_col1..]);
            }
            n_chng += 1;
        }

        if p.b_fts4 {
            fts3_update_doc_totals(&mut rc, db, p, &a_sz[n_col1..], &a_sz[..n_col1], n_chng);
        }
    }

    fts3_segments_close(db, p);
    rc
}

/// `sqlite3Fts3Optimize`: grava os termos pendentes e funde todos os segmentos do banco (inclusive o
/// novo, se houve o que gravar) num só.
pub fn fts3_optimize(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut rc = exec(db, b"SAVEPOINT fts3", None);
    if rc == SQLITE_OK {
        rc = fts3_do_optimize(db, p, true);
        if rc == SQLITE_OK || rc == SQLITE_DONE {
            let rc2 = exec(db, b"RELEASE fts3", None);
            if rc2 != SQLITE_OK {
                rc = rc2;
            }
        } else {
            exec(db, b"ROLLBACK TO fts3", None);
            exec(db, b"RELEASE fts3", None);
        }
    }
    fts3_segments_close(db, p);
    rc
}

// ---------------------------------------------------------------------------------------------
// Rotinas de fts3.c de que esta camada depende
// ---------------------------------------------------------------------------------------------

/// `fts3DbExec`: formata e executa SQL. Não faz nada se `*p_rc` já é erro.
pub fn fts3_db_exec(p_rc: &mut i32, db: &mut Connection, z_format: &[u8], args: &[PrintfArg]) {
    if *p_rc != SQLITE_OK {
        return;
    }
    match mprintf(z_format, args) {
        None => *p_rc = SQLITE_NOMEM,
        Some(z_sql) => *p_rc = exec(db, &z_sql, None),
    }
}

/// `sqlite3Fts3CreateStatTable`: cria `%_stat` se não existe.
pub fn fts3_create_stat_table(db: &mut Connection, p: &mut Fts3Table, p_rc: &mut i32) {
    fts3_db_exec(
        p_rc,
        db,
        b"CREATE TABLE IF NOT EXISTS %Q.'%q_stat'(id INTEGER PRIMARY KEY, value BLOB);",
        &[text_arg(&p.z_db), text_arg(&p.z_name)],
    );
    if *p_rc == SQLITE_OK {
        p.b_has_stat = 1;
    }
}

/// `fts3PoslistCopy`: `*pos` aponta para o começo de uma lista de posições em `buf`; avança até o
/// próximo docid da doclist (ou um byte depois do fim). Com `out`, copia a lista para ele.
pub fn fts3_poslist_copy(out: Option<&mut Vec<u8>>, buf: &[u8], pos: &mut usize) {
    let start = *pos;
    let mut p_end = start;
    let mut c: u8 = 0;

    /* O fim de uma lista de posições é um zero codificado como varint FTS3: um único byte POS_END
    ** (0), exceto se o 0 vem depois de um byte com o bit 0x80, caso em que é a cauda de outro valor
    ** de vários bytes. */
    while (at(buf, p_end) | c) != 0 {
        c = at(buf, p_end) & 0x80;
        p_end += 1;
    }
    p_end += 1; /* passa o terminador POS_END */

    if let Some(out) = out {
        out.extend((start..p_end).map(|i| at(buf, i)));
    }
    *pos = p_end;
}

/// `fts3ColumnlistCopy`: `*pos` aponta para o começo de uma lista de coluna; avança até o
/// terminador (POS_COLUMN ou POS_END) dela. Com `out`, copia a lista (sem o terminador).
pub fn fts3_columnlist_copy(out: Option<&mut Vec<u8>>, buf: &[u8], pos: &mut usize) {
    let start = *pos;
    let mut p_end = start;
    let mut c: u8 = 0;

    /* Uma lista de coluna termina com um byte 0x01 ou 0x00 que não é parte de um varint de vários
    ** bytes. */
    while (0xFE & (at(buf, p_end) | c)) != 0 {
        c = at(buf, p_end) & 0x80;
        p_end += 1;
    }
    if let Some(out) = out {
        out.extend((start..p_end).map(|i| at(buf, i)));
    }
    *pos = p_end;
}

/// `fts3GetReverseVarint`: `*pos` aponta para o primeiro byte depois de um varint de uma doclist;
/// move `*pos` para o começo desse varint e devolve o valor. `p_start` é o começo da doclist.
pub fn fts3_get_reverse_varint(pos: &mut usize, buf: &[u8], p_start: usize) -> i64 {
    /* `p` aponta o primeiro byte depois do varint; a menos que a doclist esteja corrompida, o bit
    ** 0x80 de `p[-1]` é zero. */
    let mut p = *pos as isize - 2;
    while p >= p_start as isize && (at(buf, p as usize) & 0x80) != 0 {
        p -= 1;
    }
    p += 1;
    *pos = p.max(0) as usize;

    fts3_get_varint(sl(buf, *pos)).1
}

/// `fts3ReversePoslist`: `*pos` aponta para o byte logo depois do fim de uma lista de posições
/// (`buf[*pos-1]` é POS_END); move-o para o primeiro byte da mesma lista.
pub fn fts3_reverse_poslist(buf: &[u8], p_start: usize, pos: &mut usize) {
    let ps = p_start as isize;
    let get = |i: isize| -> u8 {
        if i < 0 {
            0
        } else {
            at(buf, i as usize)
        }
    };
    let mut p = *pos as isize - 2;
    let mut c: u8 = 0;

    /* Anda para trás sobre os bytes 0x00 que `NearTrim()` deixou. */
    while p > ps {
        c = get(p);
        p -= 1;
        if c != 0 {
            break;
        }
    }

    /* Procura para trás um varint de valor zero (o fim da lista de posições anterior): um byte
    ** 0x00 precedido de um byte sem o bit 0x80. */
    while p > ps && ((get(p) & 0x80) | c) != 0 {
        c = get(p);
        p -= 1;
    }
    debug_assert!(p == ps || c == 0);

    /* `p` aponta para o byte precedente sem o bit 0x80: o começo da lista é dois bytes adiante e
    ** depois de um varint. Exceto quando `p==p_start` e a lista é a primeira da doclist: aí não se
    ** avançam os dois bytes (a segunda parte da condição, `c==0 && *pos>&p[2]`, é para o caso de
    ** um primeiro byte de doclist vazia, como `0x0A 0x00 <delta do próximo docid>`). */
    if p > ps || (c == 0 && *pos as isize > p + 2) {
        p += 2;
    }
    loop {
        let b = get(p);
        p += 1;
        if (b & 0x80) == 0 {
            break;
        }
    }
    *pos = p.max(0) as usize;
}

/// `sqlite3Fts3DoclistPrev`: percorre a doclist de trás para a frente (do fim ao começo,
/// qualquer que seja a ordem dos docids: `b_desc_idx` diz se ela é decrescente). `*pp_iter` é o
/// iterador (`None` na primeira chamada), `*pi_docid` o docid, `*pn_list` recebe o tamanho da lista
/// de posições e `*pb_eof` o fim.
pub fn fts3_doclist_prev(
    b_desc_idx: bool,
    a_doclist: &[u8],
    pp_iter: &mut Option<usize>,
    pi_docid: &mut i64,
    pn_list: &mut i32,
    pb_eof: &mut bool,
) {
    let n_doclist = a_doclist.len();
    debug_assert!(n_doclist > 0);
    debug_assert!(!*pb_eof);
    debug_assert!(match *pp_iter {
        Some(p) => p > 0 && p < n_doclist,
        None => true,
    });

    match *pp_iter {
        None => {
            let mut i_docid: i64 = 0;
            let mut p_next: usize = 0;
            let mut p_docid: usize = 0;
            let p_end = n_doclist;
            let mut i_mul: i64 = 1;

            while p_docid < p_end {
                let (n, i_delta) = fts3_get_varint(sl(a_doclist, p_docid));
                p_docid += n as usize;
                i_docid = i_docid.wrapping_add(i_mul.wrapping_mul(i_delta));
                p_next = p_docid;
                fts3_poslist_copy(None, a_doclist, &mut p_docid);
                while p_docid < p_end && a_doclist[p_docid] == 0 {
                    p_docid += 1;
                }
                i_mul = if b_desc_idx { -1 } else { 1 };
            }

            *pn_list = (p_end - p_next) as i32;
            *pp_iter = Some(p_next);
            *pi_docid = i_docid;
        }
        Some(p0) => {
            let mut p = p0;
            let i_mul: i64 = if b_desc_idx { -1 } else { 1 };
            let i_delta = fts3_get_reverse_varint(&mut p, a_doclist, 0);
            *pi_docid = pi_docid.wrapping_sub(i_mul.wrapping_mul(i_delta));

            if p == 0 {
                *pb_eof = true;
            } else {
                let p_save = p;
                fts3_reverse_poslist(a_doclist, 0, &mut p);
                *pn_list = (p_save - p) as i32;
            }
            *pp_iter = Some(p);
        }
    }
}

/// `sqlite3Fts3FirstFilter`: `p_list` é uma lista de posições (sem o terminador 0x00). Se tem
/// entradas na posição 0 (de qualquer coluna), acrescenta a `p_out` `i_delta`, as entradas da
/// posição 0 e um terminador 0x00. Devolve o número de bytes acrescentados.
pub fn fts3_first_filter(i_delta: i64, p_list: &[u8], p_out: &mut Vec<u8>) -> i32 {
    let mut n_out: i32 = 0;
    let mut b_written = false; /* verdadeiro depois de gravar `i_delta` */
    let mut p = 0usize;
    let p_end = p_list.len();
    let mut tmp = [0u8; FTS3_VARINT_MAX];

    if at(p_list, 0) != 0x01 {
        if at(p_list, 0) == 0x02 {
            let n = fts3_put_varint(&mut tmp, i_delta) as usize;
            p_out.extend_from_slice(&tmp[..n]);
            n_out += n as i32;
            p_out.push(0x02);
            n_out += 1;
            b_written = true;
        }
        fts3_columnlist_copy(None, p_list, &mut p);
    }

    while p < p_end {
        p += 1;
        let (n, i_col) = fts3_get_varint(sl(p_list, p));
        p += n as usize;
        if at(p_list, p) == 0x02 {
            if !b_written {
                let n = fts3_put_varint(&mut tmp, i_delta) as usize;
                p_out.extend_from_slice(&tmp[..n]);
                n_out += n as i32;
                b_written = true;
            }
            p_out.push(0x01);
            n_out += 1;
            let n = fts3_put_varint(&mut tmp, i_col) as usize;
            p_out.extend_from_slice(&tmp[..n]);
            n_out += n as i32;
            p_out.push(0x02);
            n_out += 1;
        }
        fts3_columnlist_copy(None, p_list, &mut p);
    }
    if b_written {
        p_out.push(0x00);
        n_out += 1;
    }

    n_out
}

/// `fts3ScanInteriorNode`: o nó interno `z_node` de um segmento b-tree é percorrido à procura de
/// `z_term`. `pi_first` recebe o blockid do filho que encabeça a sub-árvore que pode ter o termo;
/// `pi_last` o do filho mais à direita que encabeça uma sub-árvore que pode ter termos de que
/// `z_term` é prefixo.
fn fts3_scan_interior_node(
    z_term: &[u8],
    z_node: &[u8],
    pi_first: Option<&mut i64>,
    pi_last: Option<&mut i64>,
) -> i32 {
    let mut pi_first = pi_first;
    let mut pi_last = pi_last;
    let n_term = z_term.len() as i32;
    let z_end = z_node.len();
    let mut z_buffer: Vec<u8> = Vec::new(); /* o buffer onde os termos são carregados */
    let mut is_first_term = true; /* verdadeiro no primeiro termo da página */
    let mut n_buffer: i32 = 0; /* o tamanho total do termo */

    /* Pula o varint da altura (todo nó interno começa com ele) e lê o blockid do filho esquerdo. */
    let mut z_csr = 0usize;
    let (n, _) = fts3_get_varint_u(sl(z_node, z_csr));
    z_csr += n as usize;
    let (n, mut i_child) = fts3_get_varint_u(sl(z_node, z_csr)); /* o blockid do filho */
    z_csr += n as usize;
    if z_csr > z_end {
        return FTS_CORRUPT_VTAB;
    }

    while z_csr < z_end && (pi_first.is_some() || pi_last.is_some()) {
        let mut n_prefix: i32 = 0;

        /* Carrega o próximo termo do nó em `z_buffer`. */
        if !is_first_term {
            let (n, v) = fts3_get_varint32(sl(z_node, z_csr));
            z_csr += n as usize;
            n_prefix = v;
            if n_prefix > n_buffer {
                return FTS_CORRUPT_VTAB;
            }
        }
        is_first_term = false;
        let (n, n_suffix) = fts3_get_varint32(sl(z_node, z_csr));
        z_csr += n as usize;

        debug_assert!(n_prefix >= 0 && n_suffix >= 0);
        if n_prefix as i64 > z_csr as i64
            || n_suffix as i64 > z_end as i64 - z_csr as i64
            || n_suffix == 0
        {
            return FTS_CORRUPT_VTAB;
        }
        z_buffer.truncate(n_prefix as usize);
        z_buffer.extend_from_slice(&z_node[z_csr..z_csr + n_suffix as usize]);
        n_buffer = n_prefix + n_suffix;
        z_csr += n_suffix as usize;

        /* Compara o termo procurado com o carregado. Se o procurado é maior ou igual ao do nó
        ** interno, todos os termos da sub-árvore do filho são menores que ele: não é preciso
        ** procurar nela. Se o do nó interno é maior, a sub-árvore pode ter o termo. */
        let n_cmp = (n_buffer.min(n_term)) as usize;
        let cmp = memcmp(&z_term[..n_cmp], &z_buffer[..n_cmp]);
        if pi_first.is_some() && (cmp < 0 || (cmp == 0 && n_buffer > n_term)) {
            if let Some(f) = pi_first.take() {
                *f = i_child as i64;
            }
        }

        if pi_last.is_some() && cmp < 0 {
            if let Some(l) = pi_last.take() {
                *l = i_child as i64;
            }
        }

        i_child = i_child.wrapping_add(1);
    }

    if let Some(f) = pi_first {
        *f = i_child as i64;
    }
    if let Some(l) = pi_last {
        *l = i_child as i64;
    }

    SQLITE_OK
}

/// `fts3SelectLeaf`: o nó interno `z_node` e o termo `z_term`: procura na sub-árvore o intervalo de
/// folhas que pode ter o termo ou termos de que ele é prefixo. `pi_leaf` recebe a folha mais à
/// esquerda e `pi_leaf2` a mais à direita (as que não são `None`).
fn fts3_select_leaf(
    db: &mut Connection,
    p: &mut Fts3Table,
    z_term: &[u8],
    z_node: &[u8],
    pi_leaf: Option<&mut i64>,
    pi_leaf2: Option<&mut i64>,
) -> i32 {
    let mut pi_leaf = pi_leaf;
    let mut pi_leaf2 = pi_leaf2;

    debug_assert!(pi_leaf.is_some() || pi_leaf2.is_some());

    let (_, i_height) = fts3_get_varint32(z_node);
    let mut rc =
        fts3_scan_interior_node(z_term, z_node, pi_leaf.as_deref_mut(), pi_leaf2.as_deref_mut());

    if rc == SQLITE_OK && i_height > 1 {
        let mut z_blob: Vec<u8> = Vec::new(); /* o blob lido de `%_segments` */

        let both = match (pi_leaf.as_deref().copied(), pi_leaf2.as_deref().copied()) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        };
        if both == Some(true) {
            let i_blk = pi_leaf.as_deref().copied().unwrap_or(0);
            match fts3_read_block(db, p, i_blk, true) {
                Err(e) => rc = e,
                Ok((a, n)) => {
                    z_blob = a.unwrap_or_default();
                    z_blob.truncate(n.max(0) as usize);
                    rc = fts3_select_leaf(db, p, z_term, &z_blob, pi_leaf.as_deref_mut(), None);
                }
            }
            pi_leaf = None;
            z_blob = Vec::new();
        }

        if rc == SQLITE_OK {
            let i_blk = match pi_leaf.as_deref().copied() {
                Some(l) => l,
                None => pi_leaf2.as_deref().copied().unwrap_or(0),
            };
            match fts3_read_block(db, p, i_blk, true) {
                Err(e) => rc = e,
                Ok((a, n)) => {
                    z_blob = a.unwrap_or_default();
                    z_blob.truncate(n.max(0) as usize);
                }
            }
        }
        if rc == SQLITE_OK {
            let (_, i_new_height) = fts3_get_varint32(&z_blob);
            if i_new_height >= i_height {
                rc = FTS_CORRUPT_VTAB;
            } else {
                rc = fts3_select_leaf(
                    db,
                    p,
                    z_term,
                    &z_blob,
                    pi_leaf.as_deref_mut(),
                    pi_leaf2.as_deref_mut(),
                );
            }
        }
    }

    rc
}

/// `fts3SegReaderCursor`: acrescenta leitores de segmentos ao leitor múltiplo `p_csr`: o dos termos
/// pendentes (se `i_level<0` e a busca não é varredura de `fts4aux`) e um por linha de `%_segdir`
/// dos níveis pedidos.
pub fn fts3_seg_reader_cursor_fill(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
    z_term: Option<&[u8]>,
    is_prefix: bool,
    is_scan: bool,
    p_csr: &mut Fts3MultiSegReader,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_stmt: Option<StmtId> = None;

    /* Com `i_level<0` e sem ser varredura, inclui um leitor dos termos pendentes. Numa varredura a
    ** chamada vem de um módulo `fts4aux`, cujas estruturas não estão completas: filtra-se aqui. */
    if i_level < 0 && !p.a_index.is_empty() && p.i_prev_langid == i_langid {
        match fts3_seg_reader_pending(
            p,
            i_index as usize,
            z_term.unwrap_or(&[]),
            is_prefix || is_scan,
        ) {
            Ok(Some(p_seg)) => p_csr.ap_segment.push(p_seg),
            Ok(None) => {}
            Err(e) => rc = e,
        }
    }

    if i_level != FTS3_SEGCURSOR_PENDING {
        if rc == SQLITE_OK {
            match fts3_all_segdirs(db, p, i_langid, i_index, i_level) {
                Ok(st) => p_stmt = Some(st),
                Err(e) => rc = e,
            }
        }

        if let Some(st) = p_stmt {
            loop {
                if rc != SQLITE_OK {
                    break;
                }
                rc = step(db, st);
                if rc != SQLITE_ROW {
                    break;
                }

                /* Lê os valores do SELECT em variáveis locais. */
                let mut i_start_block = column_int64(db, st, 1);
                let mut i_leaves_end_block = column_int64(db, st, 2);
                let i_end_block = column_int64(db, st, 3);
                let z_root = column_blob(db, st, 4).map(|b| b.to_vec());

                /* Com `z_term` e o segmento não estando todo na raiz, dá para reduzir o intervalo
                ** de folhas varridas. */
                if let (true, Some(zt), Some(root)) = (i_start_block != 0, z_term, z_root.as_deref())
                {
                    let pi = if is_prefix { Some(&mut i_leaves_end_block) } else { None };
                    rc = fts3_select_leaf(db, p, zt, root, Some(&mut i_start_block), pi);
                    if rc != SQLITE_OK {
                        break;
                    }
                    if !is_prefix && !is_scan {
                        i_leaves_end_block = i_start_block;
                    }
                }

                match fts3_seg_reader_new(
                    p_csr.ap_segment.len() as i32 + 1,
                    !is_prefix && !is_scan,
                    i_start_block,
                    i_leaves_end_block,
                    i_end_block,
                    z_root.as_deref(),
                ) {
                    Ok(p_seg) => p_csr.ap_segment.push(p_seg),
                    Err(e) => {
                        rc = e;
                        break;
                    }
                }
            }
        }
    }

    let rc2 = match p_stmt {
        Some(st) => reset(db, st),
        None => SQLITE_OK,
    };
    if rc == SQLITE_DONE {
        rc = rc2;
    }

    rc
}

/// `sqlite3Fts3SegReaderCursor`: prepara um cursor para percorrer um índice de texto completo ou um
/// nível dele.
pub fn fts3_seg_reader_cursor(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    i_index: i32,
    i_level: i32,
    z_term: Option<&[u8]>,
    is_prefix: bool,
    is_scan: bool,
    p_csr: &mut Fts3MultiSegReader,
) -> i32 {
    debug_assert!(i_index >= 0 && i_index < p.n_index);
    debug_assert!(
        i_level == FTS3_SEGCURSOR_ALL || i_level == FTS3_SEGCURSOR_PENDING || i_level >= 0
    );
    debug_assert!(i_level < FTS3_SEGDIR_MAXLEVEL);
    debug_assert!(!is_prefix || !is_scan);

    *p_csr = Fts3MultiSegReader::default();
    fts3_seg_reader_cursor_fill(db, p, i_langid, i_index, i_level, z_term, is_prefix, is_scan, p_csr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_push_and_pop_roundtrip() {
        let mut hint = Blob::default();
        let mut rc = SQLITE_OK;
        fts3_incrmerge_hint_push(&mut hint, 1024, 3, &mut rc);
        fts3_incrmerge_hint_push(&mut hint, 5, 16, &mut rc);
        let n_all = hint.n;
        let (mut lvl, mut n) = (0i64, 0i32);
        assert_eq!(fts3_incrmerge_hint_pop(&mut hint, &mut lvl, &mut n), SQLITE_OK);
        assert_eq!((lvl, n), (5, 16));
        /* Desfazer o `pop` é restaurar `n`. */
        hint.n = n_all;
        assert_eq!(fts3_incrmerge_hint_pop(&mut hint, &mut lvl, &mut n), SQLITE_OK);
        assert_eq!(fts3_incrmerge_hint_pop(&mut hint, &mut lvl, &mut n), SQLITE_OK);
        assert_eq!((lvl, n), (1024, 3));
        assert_eq!(hint.n, 0);
    }

    #[test]
    fn node_reader_walks_a_leaf() {
        /* Folha: 0x00, depois ("abc", doclist [1,0]) e ("abd", doclist [2,0]). */
        let mut node = Blob::default();
        let mut prev = Blob::default();
        fts3_start_node(&mut node, 0, 0);
        assert_eq!(fts3_append_to_node(&mut node, &mut prev, b"abc", Some(&[1, 0])), SQLITE_OK);
        assert_eq!(fts3_append_to_node(&mut node, &mut prev, b"abd", Some(&[2, 0])), SQLITE_OK);
        assert_eq!(fts3_append_to_node(&mut node, &mut prev, b"abd", Some(&[2, 0])), FTS_CORRUPT_VTAB);

        let (mut r, mut rc) = node_reader_init(Some(node.bytes()), node.n);
        assert_eq!(rc, SQLITE_OK);
        assert_eq!(r.term.bytes(), b"abc");
        assert_eq!(r.doclist(), Some(&[1u8, 0][..]));
        rc = node_reader_next(&mut r);
        assert_eq!(rc, SQLITE_OK);
        assert_eq!(r.term.bytes(), b"abd");
        assert_eq!(r.doclist(), Some(&[2u8, 0][..]));
        assert_eq!(node_reader_next(&mut r), SQLITE_OK);
        assert!(r.a_node.is_none());
    }

    #[test]
    fn term_cmp_orders_prefix_first() {
        assert!(fts3_term_cmp(b"ab", b"abc") < 0);
        assert!(fts3_term_cmp(b"abd", b"abc") > 0);
        assert_eq!(fts3_term_cmp(b"abc", b"abc"), 0);
        assert_eq!(fts3_getint_for_test(b"12,5"), (12, 2));
    }

    fn fts3_getint_for_test(z: &[u8]) -> (i32, usize) {
        let mut pos = 0;
        let v = fts3_getint(z, &mut pos);
        (v, pos)
    }

    #[test]
    fn checksum_entry_is_stable() {
        let a = fts3_checksum_entry(b"term", 0, 0, 1, 0, 0);
        let b = fts3_checksum_entry(b"term", 0, 0, 1, 0, 1);
        assert_ne!(a, b);
        assert_eq!(a, fts3_checksum_entry(b"term", 0, 0, 1, 0, 0));
    }

    #[test]
    fn doclist_prev_walks_backwards() {
        /* Docid 1 com lista [2,0]; docid 4 (delta 3) com lista [5,0]. */
        let doclist = [1u8, 2, 0, 3, 5, 0];
        let mut it: Option<usize> = None;
        let (mut docid, mut n, mut eof) = (0i64, 0i32, false);
        fts3_doclist_prev(false, &doclist, &mut it, &mut docid, &mut n, &mut eof);
        assert_eq!((docid, n, it, eof), (4, 2, Some(4), false));
        fts3_doclist_prev(false, &doclist, &mut it, &mut docid, &mut n, &mut eof);
        assert_eq!((docid, n, it, eof), (1, 2, Some(1), false));
        fts3_doclist_prev(false, &doclist, &mut it, &mut docid, &mut n, &mut eof);
        assert!(eof);
    }

    #[test]
    fn first_filter_keeps_position_zero() {
        let mut out = Vec::new();
        /* Posição 0 na coluna 0 (valor 2): sai o delta, a lista e o terminador. */
        assert_eq!(fts3_first_filter(7, &[2, 4], &mut out), 3);
        assert_eq!(out, vec![7, 2, 0]);
        let mut out = Vec::new();
        assert_eq!(fts3_first_filter(7, &[3, 4], &mut out), 0);
        assert!(out.is_empty());
    }
}
