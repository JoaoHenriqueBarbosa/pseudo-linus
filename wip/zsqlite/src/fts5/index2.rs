//! `fts5_index.c` (parte 2): o multi-iterador, as funções de saída (`xSetOutputs`), o escritor de
//! segmentos, o merge (incremental, automerge e crisismerge), o secure-delete, o flush do hash
//! para um segmento de nível 0 e o optimize. Veja o cabeçalho de [`super::index`] para os desvios
//! do C comuns às três partes.

use crate::connection::Connection;
use crate::consts::{SQLITE_FULL, SQLITE_OK};
use crate::mem::StrDtor;
use crate::util::at;
use crate::vdbeapi::{bind_blob, bind_int, bind_int64, bind_null, reset, step};

use super::buffer::Fts5Buffer;
use super::index::*;
use super::int::{
    Fts5Colset, Fts5Config, FTS5INDEX_QUERY_NOOUTPUT, FTS5INDEX_QUERY_SKIPHASH, FTS5_CORRUPT,
    FTS5_DETAIL_FULL, FTS5_DETAIL_NONE, FTS5_MAX_SEGMENT,
};
use super::varint::fts5_put_varint;

// ---------------------------------------------------------------------------------------------
// Tombstones
// ---------------------------------------------------------------------------------------------

/// `TOMBSTONE_KEYSIZE`.
pub(crate) fn tombstone_keysize(pg: &Fts5Data) -> i32 {
    if at(&pg.p, 0) == 4 {
        4
    } else {
        8
    }
}

/// `TOMBSTONE_NSLOT`.
pub(crate) fn tombstone_nslot(pg: &Fts5Data) -> i32 {
    if pg.nn > 16 {
        (pg.nn - 8) / tombstone_keysize(pg)
    } else {
        1
    }
}

/// `fts5IndexTombstoneQuery`: verdadeiro se `i_rowid` está na tabela hash (`n_hash_table` páginas).
pub(crate) fn fts5_index_tombstone_query(p_hash: &Fts5Data, n_hash_table: i32, i_rowid: u64) -> i32 {
    let sz_key = tombstone_keysize(p_hash);
    let n_slot = tombstone_nslot(p_hash);
    let mut i_slot = ((i_rowid / n_hash_table.max(1) as u64) % n_slot as u64) as i32;
    let mut n_collide = n_slot;

    if i_rowid == 0 {
        return at(&p_hash.p, 1) as i32;
    }
    let slot_set = |i: i32| -> bool {
        let off = 8 + (i as usize) * sz_key as usize;
        (0..sz_key as usize).any(|k| at(&p_hash.p, off + k) != 0)
    };
    while slot_set(i_slot) {
        let off = 8 + (i_slot as usize) * sz_key as usize;
        let v = if sz_key == 4 {
            get_u32(&p_hash.p, off) as u64
        } else {
            get_u64(&p_hash.p, off)
        };
        if v == i_rowid {
            return 1;
        }
        let c = n_collide;
        n_collide -= 1;
        if c == 0 {
            break;
        }
        i_slot = (i_slot + 1) % n_slot;
    }
    0
}

/// `fts5MultiIterIsDeleted`: verdadeiro se o iterador aponta uma entrada com tombstone.
pub(crate) fn fts5_multi_iter_is_deleted(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &Fts5Iter,
) -> i32 {
    let i_first = it.a_first[1].i_first as usize;
    let seg = &it.a_seg[i_first];

    if seg.p_leaf.is_some() {
        if let Some(arr) = seg.p_tomb_array.as_ref() {
            let mut arr = arr.borrow_mut();
            /* Descobre em que página o rowid pode estar. */
            let n = arr.n_tombstone.max(1);
            let i_pg = ((seg.i_rowid as u64) % n as u64) as usize;

            /* Se a página `i_pg` da tabela hash ainda não foi lida do banco, lê agora. */
            if arr.ap_tombstone[i_pg].is_none() {
                let segid = seg.p_seg.as_ref().map_or(0, |s| s.i_segid);
                arr.ap_tombstone[i_pg] =
                    fts5_data_read(p, db, cfg, fts5_tombstone_rowid(segid, i_pg as i32));
                if arr.ap_tombstone[i_pg].is_none() {
                    return 0;
                }
            }

            let n_tomb = arr.n_tombstone;
            return match arr.ap_tombstone[i_pg].as_ref() {
                Some(pg) => fts5_index_tombstone_query(pg, n_tomb, seg.i_rowid as u64),
                None => 0,
            };
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------
// Multi-iterador
// ---------------------------------------------------------------------------------------------

/// `fts5MultiIterDoCompare`: popula `a_first[i_out]`. Se o valor devolvido não é zero, é o índice
/// de um iterador que aponta uma chave duplicada de outro de maior prioridade.
pub(crate) fn fts5_multi_iter_do_compare(it: &mut Fts5Iter, i_out: i32) -> i32 {
    let n_seg = it.n_seg;
    let (i1, i2) = if i_out >= n_seg / 2 {
        let i1 = (i_out - n_seg / 2) * 2;
        (i1, i1 + 1)
    } else {
        (
            it.a_first[(i_out * 2) as usize].i_first as i32,
            it.a_first[(i_out * 2 + 1) as usize].i_first as i32,
        )
    };

    it.a_first[i_out as usize].b_term_eq = 0;
    let i_res;
    let (leaf1, leaf2) = (
        it.a_seg[i1 as usize].p_leaf.is_some(),
        it.a_seg[i2 as usize].p_leaf.is_some(),
    );
    if !leaf1 {
        i_res = i2;
    } else if !leaf2 {
        i_res = i1;
    } else {
        let mut res = fts5_buffer_compare(&it.a_seg[i1 as usize].term, &it.a_seg[i2 as usize].term);
        if res == 0 {
            it.a_first[i_out as usize].b_term_eq = 1;
            if it.a_seg[i1 as usize].i_rowid == it.a_seg[i2 as usize].i_rowid {
                return i2;
            }
            res = if ((it.a_seg[i1 as usize].i_rowid > it.a_seg[i2 as usize].i_rowid) as i32)
                == it.b_rev
            {
                -1
            } else {
                1
            };
        }
        i_res = if res < 0 { i1 } else { i2 };
    }

    it.a_first[i_out as usize].i_first = i_res as u16;
    0
}

/// `fts5MultiIterAdvanced`.
pub(crate) fn fts5_multi_iter_advanced(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    i_changed: i32,
    i_minset: i32,
) {
    let mut i = (it.n_seg + i_changed) / 2;
    while i >= i_minset && p.rc == SQLITE_OK {
        let i_eq = fts5_multi_iter_do_compare(it, i);
        if i_eq != 0 {
            fts5_seg_iter_x_next(p, db, cfg, &mut it.a_seg[i_eq as usize], None);
            i = it.n_seg + i_eq;
        }
        i /= 2;
    }
}

/// `fts5MultiIterAdvanceRowid`: o sub-iterador `i_changed` avançou mas continua no mesmo termo.
/// Devolve 1 se o chamador deve usar `fts5_multi_iter_advanced`; senão 0 e `*pp_first` é o índice do
/// iterador mais adiantado.
pub(crate) fn fts5_multi_iter_advance_rowid(
    it: &mut Fts5Iter,
    i_changed: i32,
    pp_first: &mut usize,
) -> i32 {
    let mut p_new = i_changed as usize;

    if it.a_seg[p_new].i_rowid == it.i_switch_rowid
        || ((it.a_seg[p_new].i_rowid < it.i_switch_rowid) as i32) == it.b_rev
    {
        let mut p_other = (i_changed ^ 0x0001) as usize;
        it.i_switch_rowid = if it.b_rev != 0 { i64::MIN } else { i64::MAX };
        let mut i = ((it.n_seg + i_changed) / 2) as usize;
        loop {
            if it.a_first[i].b_term_eq != 0 {
                if it.a_seg[p_new].i_rowid == it.a_seg[p_other].i_rowid {
                    return 1;
                } else if ((it.a_seg[p_other].i_rowid > it.a_seg[p_new].i_rowid) as i32) == it.b_rev {
                    it.i_switch_rowid = it.a_seg[p_other].i_rowid;
                    p_new = p_other;
                } else if ((it.a_seg[p_other].i_rowid > it.i_switch_rowid) as i32) == it.b_rev {
                    it.i_switch_rowid = it.a_seg[p_other].i_rowid;
                }
            }
            it.a_first[i].i_first = p_new as u16;
            if i == 1 {
                break;
            }

            p_other = it.a_first[i ^ 0x0001].i_first as usize;
            i /= 2;
        }
    }

    *pp_first = p_new;
    0
}

/// `fts5MultiIterSetEof`.
pub(crate) fn fts5_multi_iter_set_eof(it: &mut Fts5Iter) {
    let i = it.a_first[1].i_first as usize;
    it.base.b_eof = it.a_seg[i].p_leaf.is_none() as u8;
    it.i_switch_rowid = it.a_seg[i].i_rowid;
}

/// `fts5MultiIterIsEmpty`: verdadeiro se o iterador aponta um marcador de delete.
pub(crate) fn fts5_multi_iter_is_empty(p: &Fts5Index, it: &Fts5Iter) -> bool {
    let seg = &it.a_seg[it.a_first[1].i_first as usize];
    p.rc == SQLITE_OK && seg.p_leaf.is_some() && seg.n_pos == 0
}

/// `fts5MultiIterNext`: move o iterador para a próxima entrada (com `b_from`, para a primeira com
/// rowid `i_from` ou além).
pub(crate) fn fts5_multi_iter_next(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    b_from: bool,
    i_from: i64,
) {
    let mut b_use_from = b_from;
    while p.rc == SQLITE_OK {
        let i_first = it.a_first[1].i_first as usize;
        let mut b_new_term: i32 = 0;
        let mut seg_idx = i_first;
        if b_use_from && it.a_seg[i_first].p_dlidx.is_some() {
            fts5_seg_iter_next_from(p, db, cfg, &mut it.a_seg[i_first], i_from);
        } else {
            fts5_seg_iter_x_next(p, db, cfg, &mut it.a_seg[i_first], Some(&mut b_new_term));
        }

        if it.a_seg[seg_idx].p_leaf.is_none()
            || b_new_term != 0
            || fts5_multi_iter_advance_rowid(it, i_first as i32, &mut seg_idx) != 0
        {
            fts5_multi_iter_advanced(p, db, cfg, it, i_first as i32, 1);
            fts5_multi_iter_set_eof(it);
            seg_idx = it.a_first[1].i_first as usize;
            if it.a_seg[seg_idx].p_leaf.is_none() {
                return;
            }
        }

        if (it.b_skip_empty == 0 || it.a_seg[seg_idx].n_pos != 0)
            && 0 == fts5_multi_iter_is_deleted(p, db, cfg, it)
        {
            fts5_iter_set_outputs(p, db, cfg, it, seg_idx);
            return;
        }
        b_use_from = false;
    }
}

/// `fts5MultiIterNext2`.
pub(crate) fn fts5_multi_iter_next2(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    pb_new_term: &mut i32,
) {
    if p.rc == SQLITE_OK {
        *pb_new_term = 0;
        loop {
            let i_first = it.a_first[1].i_first as usize;
            let mut seg_idx = i_first;
            let mut b_new_term: i32 = 0;

            fts5_seg_iter_x_next(p, db, cfg, &mut it.a_seg[i_first], Some(&mut b_new_term));
            if it.a_seg[seg_idx].p_leaf.is_none()
                || b_new_term != 0
                || fts5_multi_iter_advance_rowid(it, i_first as i32, &mut seg_idx) != 0
            {
                fts5_multi_iter_advanced(p, db, cfg, it, i_first as i32, 1);
                fts5_multi_iter_set_eof(it);
                *pb_new_term = 1;
            }

            let again = (fts5_multi_iter_is_empty(p, it) || fts5_multi_iter_is_deleted(p, db, cfg, it) != 0)
                && p.rc == SQLITE_OK;
            if !again {
                break;
            }
        }
    }
}

/// `fts5MultiIterAlloc`: aloca o multi-iterador com espaço para `n_seg` iteradores de segmento
/// (arredondado para uma potência de dois). `None` se há um erro pendente.
pub(crate) fn fts5_multi_iter_alloc(p: &Fts5Index, n_seg: i32) -> Option<Fts5Iter> {
    if p.rc != SQLITE_OK {
        return None;
    }
    let mut n_slot: i64 = 2;
    while n_slot < n_seg as i64 {
        n_slot *= 2;
    }
    let mut new = Fts5Iter::default();
    new.n_seg = n_slot as i32;
    new.a_first = vec![Fts5CResult::default(); n_slot as usize];
    new.a_seg = (0..n_slot).map(|_| Fts5SegIter::default()).collect();
    new.x_set_outputs = SetOutputsKind::Noop;
    Some(new)
}

/// `fts5IndexColsetTest`: verdadeiro se `i_col` está em `colset`.
fn fts5_index_colset_test(colset: &Fts5Colset, i_col: i32) -> bool {
    colset.ai_col.iter().any(|c| *c == i_col)
}

/// `fts5IterSetOutputCb`: escolhe o `xSetOutputs` conforme a configuração.
pub(crate) fn fts5_iter_set_output_cb(p: &Fts5Index, cfg: &Fts5Config, it: &mut Fts5Iter) {
    if p.rc == SQLITE_OK {
        if cfg.e_detail == FTS5_DETAIL_NONE {
            it.x_set_outputs = SetOutputsKind::NoDetail;
        } else if it.p_colset.is_none() {
            it.x_set_outputs = SetOutputsKind::Nocolset;
        } else if it.p_colset.as_ref().map_or(false, |c| c.ai_col.is_empty()) {
            it.x_set_outputs = SetOutputsKind::ZeroColset;
        } else if cfg.e_detail == FTS5_DETAIL_FULL {
            it.x_set_outputs = SetOutputsKind::Full;
        } else if cfg.n_col() <= 100 {
            it.x_set_outputs = SetOutputsKind::Col100;
        } else {
            it.x_set_outputs = SetOutputsKind::Col;
        }
    }
}

/// `fts5MultiIterFinishSetup`: termina de preparar o multi-iterador depois de os iteradores de
/// segmento estarem prontos.
pub(crate) fn fts5_multi_iter_finish_setup(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
) {
    let mut i_iter = it.n_seg - 1;
    while i_iter > 0 {
        let i_eq = fts5_multi_iter_do_compare(it, i_iter);
        if i_eq != 0 {
            if p.rc == SQLITE_OK {
                fts5_seg_iter_x_next(p, db, cfg, &mut it.a_seg[i_eq as usize], None);
            }
            fts5_multi_iter_advanced(p, db, cfg, it, i_eq, i_iter);
        }
        i_iter -= 1;
    }
    fts5_multi_iter_set_eof(it);

    if (it.b_skip_empty != 0 && fts5_multi_iter_is_empty(p, it))
        || fts5_multi_iter_is_deleted(p, db, cfg, it) != 0
    {
        fts5_multi_iter_next(p, db, cfg, it, false, 0);
    } else if it.base.b_eof == 0 {
        let i = it.a_first[1].i_first as usize;
        fts5_iter_set_outputs(p, db, cfg, it, i);
    }
}

/// `fts5MultiIterNew`: abre um multi-iterador sobre `p_struct`. Com `i_level < 0` mescla todos os
/// segmentos (e o hash); senão os primeiros `n_segment` do nível `i_level`. Aponta o primeiro
/// termo/rowid. Devolve `None` em erro (deixado em `p.rc`).
pub(crate) fn fts5_multi_iter_new(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &Fts5Structure,
    flags: i32,
    p_colset: Option<&Fts5Colset>,
    p_term: Option<&[u8]>,
    i_level: i32,
    n_segment: i32,
) -> Option<Fts5Iter> {
    let mut n_seg = 0;
    let mut i_iter = 0usize;

    /* Aloca espaço para o novo multi-iterador de segmentos. */
    if p.rc == SQLITE_OK {
        if i_level < 0 {
            n_seg = p_struct.n_segment;
            n_seg += (p.p_hash.is_some() && 0 == (flags & FTS5INDEX_QUERY_SKIPHASH)) as i32;
        } else {
            n_seg = p_struct.a_level[i_level as usize].n_seg.min(n_segment);
        }
    }
    let mut p_new = fts5_multi_iter_alloc(p, n_seg)?;
    p_new.b_rev = ((flags & super::int::FTS5INDEX_QUERY_DESC) != 0) as i32;
    p_new.b_skip_empty = ((flags & super::int::FTS5INDEX_QUERY_SKIPEMPTY) != 0) as u8;
    p_new.p_colset = p_colset.cloned();
    if (flags & FTS5INDEX_QUERY_NOOUTPUT) == 0 {
        fts5_iter_set_output_cb(p, cfg, &mut p_new);
    }

    /* Inicia cada um dos iteradores de segmento componentes. */
    if p.rc == SQLITE_OK {
        if i_level < 0 {
            if p.p_hash.is_some() && 0 == (flags & FTS5INDEX_QUERY_SKIPHASH) {
                /* Acrescenta um iterador para o conteúdo corrente da tabela hash. */
                fts5_seg_iter_hash_init(p, cfg, p_term, flags, &mut p_new.a_seg[i_iter]);
                i_iter += 1;
            }
            for lvl in p_struct.a_level.iter() {
                let mut i_seg = lvl.n_seg - 1;
                while i_seg >= 0 {
                    let seg = &lvl.a_seg[i_seg as usize];
                    if i_iter < p_new.a_seg.len() {
                        match p_term {
                            None => fts5_seg_iter_init(p, db, cfg, seg, &mut p_new.a_seg[i_iter]),
                            Some(t) => fts5_seg_iter_seek_init(
                                p,
                                db,
                                cfg,
                                t,
                                flags,
                                seg,
                                &mut p_new.a_seg[i_iter],
                            ),
                        }
                    }
                    i_iter += 1;
                    i_seg -= 1;
                }
            }
        } else {
            let lvl = &p_struct.a_level[i_level as usize];
            let mut i_seg = n_seg - 1;
            while i_seg >= 0 {
                fts5_seg_iter_init(p, db, cfg, &lvl.a_seg[i_seg as usize], &mut p_new.a_seg[i_iter]);
                i_iter += 1;
                i_seg -= 1;
            }
        }
    }

    /* Se deu certo, cada iterador componente aponta a primeira entrada do seu segmento: inicia
    ** `a_first`. Se houve erro, descarta o iterador e devolve `None`. */
    if p.rc == SQLITE_OK {
        fts5_multi_iter_finish_setup(p, db, cfg, &mut p_new);
        Some(p_new)
    } else {
        None
    }
}

/// `fts5MultiIterNew2`: cria um iterador sobre a doclist `p_data`.
pub(crate) fn fts5_multi_iter_new2(
    p: &mut Fts5Index,
    cfg: &Fts5Config,
    p_data: Fts5Data,
    b_desc: bool,
) -> Option<Fts5Iter> {
    let mut p_new = fts5_multi_iter_alloc(p, 2)?;
    {
        let it = &mut p_new.a_seg[1];
        it.flags = FTS5_SEGITER_ONETERM;
        if p_data.sz_leaf > 0 {
            let (n, v) = gv64(&p_data.p, 0);
            it.i_leaf_offset = n as i64;
            it.i_rowid = v as i64;
            it.i_endof_doclist = p_data.nn;
            it.p_leaf = Some(p_data);
            p_new.a_first[1].i_first = 1;
            if b_desc {
                p_new.b_rev = 1;
                it.flags |= FTS5_SEGITER_REVERSE;
                fts5_seg_iter_reverse_init_page(p, cfg, it);
            } else {
                fts5_seg_iter_load_npos(p, cfg, it);
            }
        } else {
            p_new.base.b_eof = 1;
        }
        fts5_seg_iter_set_next(cfg, it);
    }
    Some(p_new)
}

/// `fts5MultiIterEof`: verdadeiro no fim do iterador ou com um erro pendente.
pub(crate) fn fts5_multi_iter_eof(p: &Fts5Index, it: &Option<Fts5Iter>) -> bool {
    p.rc != SQLITE_OK || it.as_ref().map_or(true, |i| i.base.b_eof != 0)
}

/// `fts5MultiIterRowid`.
pub(crate) fn fts5_multi_iter_rowid(it: &Fts5Iter) -> i64 {
    it.a_seg[it.a_first[1].i_first as usize].i_rowid
}

/// `fts5MultiIterNextFrom`: move para a próxima entrada em `i_match` ou depois.
pub(crate) fn fts5_multi_iter_next_from(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    i_match: i64,
) {
    loop {
        fts5_multi_iter_next(p, db, cfg, it, true, i_match);
        if p.rc != SQLITE_OK || it.base.b_eof != 0 {
            break;
        }
        let i_rowid = fts5_multi_iter_rowid(it);
        if it.b_rev == 0 && i_rowid >= i_match {
            break;
        }
        if it.b_rev != 0 && i_rowid <= i_match {
            break;
        }
    }
}

/// `fts5MultiIterTerm`: o termo (com o byte do índice na frente).
pub(crate) fn fts5_multi_iter_term(it: &Fts5Iter) -> &[u8] {
    &it.a_seg[it.a_first[1].i_first as usize].term.p
}

// ---------------------------------------------------------------------------------------------
// Poslists e saídas do iterador
// ---------------------------------------------------------------------------------------------

/// `fts5PoslistOffsetsCallback`.
fn fts5_poslist_offsets_callback(
    buf: &mut Fts5Buffer,
    colset: &Fts5Colset,
    i_read: &mut i32,
    i_write: &mut i32,
    chunk: &[u8],
) {
    let n_chunk = chunk.len() as i32;
    let mut i = 0;
    while i < n_chunk {
        let (nb, mut i_val) = gv32(chunk, i);
        i += nb;
        i_val = i_val.wrapping_add(*i_read).wrapping_sub(2);
        *i_read = i_val;
        if fts5_index_colset_test(colset, i_val) {
            buf.append_varint(i_val.wrapping_add(2).wrapping_sub(*i_write) as i64);
            *i_write = i_val;
        }
    }
}

/// `fts5PoslistFilterCallback`.
fn fts5_poslist_filter_callback(
    buf: &mut Fts5Buffer,
    colset: &Fts5Colset,
    e_state: &mut i32,
    chunk: &[u8],
) {
    let n_chunk = chunk.len() as i32;
    if n_chunk > 0 {
        /* Procura o primeiro varint de valor 1: é o começo das ocorrências da próxima coluna. */
        let mut i: i32 = 0;
        let mut i_start: i32 = 0;

        if *e_state == 2 {
            let i_col = fast32(chunk, &mut i);
            if fts5_index_colset_test(colset, i_col) {
                *e_state = 1;
                buf.append_varint(1);
            } else {
                *e_state = 0;
            }
        }

        loop {
            while i < n_chunk && at(chunk, ux(i)) != 0x01 {
                while at(chunk, ux(i)) & 0x80 != 0 {
                    i += 1;
                }
                i += 1;
            }
            if *e_state != 0 {
                buf.append_blob(sub(chunk, i_start, i - i_start));
            }
            if i < n_chunk {
                i_start = i;
                i += 1;
                if i >= n_chunk {
                    *e_state = 2;
                } else {
                    let i_col = fast32(chunk, &mut i);
                    *e_state = fts5_index_colset_test(colset, i_col) as i32;
                    if *e_state != 0 {
                        buf.append_blob(sub(chunk, i_start, i - i_start));
                        i_start = i;
                    }
                }
            }
            if i >= n_chunk {
                break;
            }
        }
    }
}

/// O `xChunk` de `fts5ChunkIterate`: recebe o índice, a conexão, a configuração e o pedaço.
pub(crate) type ChunkFn<'a> = &'a mut dyn FnMut(&mut Fts5Index, &mut Connection, &Fts5Config, &[u8]);

/// `fts5ChunkIterate`: chama `x_chunk` para cada pedaço (um por página) da poslist corrente.
pub(crate) fn fts5_chunk_iterate(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &mut Fts5SegIter,
    x_chunk: ChunkFn<'_>,
) {
    let mut n_rem = seg.n_pos;
    let mut chunk: Vec<u8>;
    let mut n_chunk: i32;
    {
        let leaf = match seg.p_leaf.as_ref() {
            Some(l) => l,
            None => return,
        };
        n_chunk = n_rem.min(leaf.sz_leaf - seg.i_leaf_offset as i32);
        chunk = sub(&leaf.p, seg.i_leaf_offset as i32, n_chunk).to_vec();
    }
    let mut pgno = seg.i_leaf_pgno;
    let mut pgno_save = 0;

    /* Esta função não serve para bancos com detail=none. */
    if (seg.flags & FTS5_SEGITER_REVERSE) == 0 {
        pgno_save = pgno + 1;
    }

    loop {
        x_chunk(p, db, cfg, &chunk);
        n_rem -= n_chunk;
        if n_rem <= 0 {
            break;
        } else if seg.p_seg.is_none() {
            p.rc = FTS5_CORRUPT;
            return;
        } else {
            pgno += 1;
            let segid = seg.p_seg.as_ref().map_or(0, |s| s.i_segid);
            let p_data = fts5_leaf_read(p, db, cfg, fts5_segment_rowid(segid, pgno));
            let data = match p_data {
                Some(d) => d,
                None => break,
            };
            n_chunk = n_rem.min(data.sz_leaf - 4);
            chunk = sub(&data.p, 4, n_chunk).to_vec();
            if pgno == pgno_save {
                seg.p_next_leaf = Some(data);
            }
        }
    }
}

/// `fts5SegiterPoslist`: acrescenta a poslist da entrada corrente a `buf` (sem o campo de
/// tamanho), filtrada por `colset` se houver.
pub(crate) fn fts5_segiter_poslist(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &mut Fts5SegIter,
    colset: Option<&Fts5Colset>,
    buf: &mut Fts5Buffer,
) {
    match colset {
        None => {
            fts5_chunk_iterate(p, db, cfg, seg, &mut |_p, _db, _cfg, chunk| {
                if !chunk.is_empty() {
                    buf.append_blob(chunk);
                }
            });
        }
        Some(cs) => {
            if cfg.e_detail == FTS5_DETAIL_FULL {
                let mut e_state = fts5_index_colset_test(cs, 0) as i32;
                fts5_chunk_iterate(p, db, cfg, seg, &mut |_p, _db, _cfg, chunk| {
                    fts5_poslist_filter_callback(buf, cs, &mut e_state, chunk);
                });
            } else {
                let mut i_read = 0;
                let mut i_write = 0;
                fts5_chunk_iterate(p, db, cfg, seg, &mut |_p, _db, _cfg, chunk| {
                    fts5_poslist_offsets_callback(buf, cs, &mut i_read, &mut i_write, chunk);
                });
            }
        }
    }
}

/// `fts5IndexExtractColset`: filtra a poslist `pos` por `colset` e deixa o resultado em
/// `base.p_data`/`n_data` (usando `poslist` se for preciso montar um buffer).
fn fts5_index_extract_colset(
    rc: &mut i32,
    colset: &Fts5Colset,
    pos: &[u8],
    poslist: &mut Fts5Buffer,
    base: &mut Fts5IterBase,
) {
    if *rc != SQLITE_OK {
        return;
    }
    let n_col = colset.ai_col.len() as i32;
    let n_pos = pos.len() as i32;
    let mut p = 0i32;
    let mut a_copy = 0i32;
    let p_end = n_pos;
    let mut i = 0usize;
    let mut i_current = 0i32;

    loop {
        while colset.ai_col[i] < i_current {
            i += 1;
            if i as i32 == n_col {
                base.p_data = poslist.p.clone();
                base.n_data = poslist.n();
                return;
            }
        }

        /* Avança `p` até o fim ou até um byte 0x01 que não faz parte de um varint */
        while p < p_end && at(pos, ux(p)) != 0x01 {
            loop {
                let b = at(pos, ux(p));
                p += 1;
                if b & 0x80 == 0 {
                    break;
                }
            }
        }

        if colset.ai_col[i] == i_current {
            if n_col == 1 {
                base.p_data = sub(pos, a_copy, p - a_copy).to_vec();
                base.n_data = p - a_copy;
                return;
            }
            poslist.append_blob(sub(pos, a_copy, p - a_copy));
        }
        if p >= p_end {
            base.p_data = poslist.p.clone();
            base.n_data = poslist.n();
            return;
        }
        a_copy = p;
        p += 1;
        i_current = at(pos, ux(p)) as i32;
        p += 1;
        if i_current & 0x80 != 0 {
            p -= 1;
            let (nb, v) = gv32(pos, p);
            p += nb;
            i_current = v;
        }
    }
}

/// O `pIter->xSetOutputs(pIter, pSeg)` do C: `i_seg` é o índice do iterador de segmento.
pub(crate) fn fts5_iter_set_outputs(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    i_seg: usize,
) {
    match it.x_set_outputs {
        SetOutputsKind::Noop => {}
        SetOutputsKind::NoDetail => {
            it.base.i_rowid = it.a_seg[i_seg].i_rowid;
            it.base.n_data = it.a_seg[i_seg].n_pos;
        }
        SetOutputsKind::Nocolset => {
            it.base.i_rowid = it.a_seg[i_seg].i_rowid;
            it.base.n_data = it.a_seg[i_seg].n_pos;

            let all_on_page = {
                let seg = &it.a_seg[i_seg];
                seg.p_leaf
                    .as_ref()
                    .map_or(false, |l| seg.i_leaf_offset + seg.n_pos as i64 <= l.sz_leaf as i64)
            };
            if all_on_page {
                /* Todos os dados estão na página corrente. */
                let seg = &it.a_seg[i_seg];
                if let Some(leaf) = seg.p_leaf.as_ref() {
                    it.base.p_data = sub(&leaf.p, seg.i_leaf_offset as i32, seg.n_pos).to_vec();
                }
            } else {
                /* Os dados se espalham por duas ou mais páginas: copia para o buffer
                ** `poslist` do iterador. */
                it.poslist.zero();
                fts5_segiter_poslist(p, db, cfg, &mut it.a_seg[i_seg], None, &mut it.poslist);
                it.base.p_data = it.poslist.p.clone();
            }
        }
        SetOutputsKind::ZeroColset => {
            it.base.n_data = 0;
        }
        SetOutputsKind::Col => fts5_iter_set_outputs_col(p, db, cfg, it, i_seg),
        SetOutputsKind::Col100 => {
            let all_on_page = {
                let seg = &it.a_seg[i_seg];
                seg.p_leaf
                    .as_ref()
                    .map_or(false, |l| seg.i_leaf_offset + seg.n_pos as i64 <= l.sz_leaf as i64)
            };
            if !all_on_page {
                fts5_iter_set_outputs_col(p, db, cfg, it, i_seg);
            } else {
                let a: Vec<u8> = {
                    let seg = &it.a_seg[i_seg];
                    match seg.p_leaf.as_ref() {
                        Some(l) => sub(&l.p, seg.i_leaf_offset as i32, seg.n_pos).to_vec(),
                        None => Vec::new(),
                    }
                };
                let ai_col: Vec<i32> = it
                    .p_colset
                    .as_ref()
                    .map(|c| c.ai_col.clone())
                    .unwrap_or_default();
                let mut i_prev: i32 = 0;
                let mut i_col_idx = 0usize;
                let mut a_out: Vec<u8> = Vec::new();
                let mut i_prev_out: i32 = 0;

                it.base.i_rowid = it.a_seg[i_seg].i_rowid;

                let mut ia = 0usize;
                'scan: while ia < a.len() {
                    i_prev += a[ia] as i32 - 2;
                    ia += 1;
                    while ai_col[i_col_idx] < i_prev {
                        i_col_idx += 1;
                        if i_col_idx == ai_col.len() {
                            break 'scan;
                        }
                    }
                    if ai_col[i_col_idx] == i_prev {
                        a_out.push(((i_prev - i_prev_out) + 2) as u8);
                        i_prev_out = i_prev;
                    }
                }

                it.poslist.p = a_out;
                it.base.p_data = it.poslist.p.clone();
                it.base.n_data = it.poslist.n();
            }
        }
        SetOutputsKind::Full => {
            it.base.i_rowid = it.a_seg[i_seg].i_rowid;
            let all_on_page = {
                let seg = &it.a_seg[i_seg];
                seg.p_leaf
                    .as_ref()
                    .map_or(false, |l| seg.i_leaf_offset + seg.n_pos as i64 <= l.sz_leaf as i64)
            };
            if all_on_page {
                /* Todos os dados estão na página corrente. */
                let a: Vec<u8> = {
                    let seg = &it.a_seg[i_seg];
                    match seg.p_leaf.as_ref() {
                        Some(l) => sub(&l.p, seg.i_leaf_offset as i32, seg.n_pos).to_vec(),
                        None => Vec::new(),
                    }
                };
                it.poslist.zero();
                if let Some(cs) = it.p_colset.as_ref() {
                    fts5_index_extract_colset(&mut p.rc, cs, &a, &mut it.poslist, &mut it.base);
                }
            } else {
                it.poslist.zero();
                if let Some(cs) = it.p_colset.as_ref() {
                    fts5_segiter_poslist(p, db, cfg, &mut it.a_seg[i_seg], Some(cs), &mut it.poslist);
                }
                it.base.p_data = it.poslist.p.clone();
                it.base.n_data = it.poslist.n();
            }
        }
    }
}

/// `fts5IterSetOutputs_Col`.
fn fts5_iter_set_outputs_col(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    i_seg: usize,
) {
    it.poslist.zero();
    fts5_segiter_poslist(
        p,
        db,
        cfg,
        &mut it.a_seg[i_seg],
        it.p_colset.as_ref(),
        &mut it.poslist,
    );
    it.base.i_rowid = it.a_seg[i_seg].i_rowid;
    it.base.p_data = it.poslist.p.clone();
    it.base.n_data = it.poslist.n();
}

// ---------------------------------------------------------------------------------------------
// Segmentos novos: ids, hash e escritor
// ---------------------------------------------------------------------------------------------

/// `fts5AllocateSegid`: um id de segmento (1 a 65535) que ninguém usa; `SQLITE_FULL` se não houver.
pub(crate) fn fts5_allocate_segid(p: &mut Fts5Index, p_struct: &Fts5Structure) -> i32 {
    let mut i_segid = 0;

    if p.rc == SQLITE_OK {
        if p_struct.n_segment >= FTS5_MAX_SEGMENT {
            p.rc = SQLITE_FULL;
        } else {
            let mut a_used = [0u32; ((FTS5_MAX_SEGMENT + 31) / 32) as usize];
            for lvl in p_struct.a_level.iter() {
                for i_seg in 0..lvl.n_seg.max(0) as usize {
                    let i_id = lvl.a_seg[i_seg].i_segid;
                    if i_id <= FTS5_MAX_SEGMENT && i_id > 0 {
                        a_used[((i_id - 1) / 32) as usize] |= 1u32 << ((i_id - 1) % 32);
                    }
                }
            }

            let mut i = 0usize;
            while i + 1 < a_used.len() && a_used[i] == 0xFFFF_FFFF {
                i += 1;
            }
            let mask = a_used[i];
            while mask & (1u32 << i_segid) != 0 {
                i_segid += 1;
            }
            i_segid += 1 + (i as i32) * 32;
        }
    }

    i_segid
}

/// `fts5IndexDiscardData`: descarta o que está nos hashes em memória.
pub(crate) fn fts5_index_discard_data(p: &mut Fts5Index) {
    if let Some(h) = p.p_hash.as_mut() {
        h.clear();
        h.n_byte = 0;
        p.n_pending_row = 0;
        p.flush_rc = SQLITE_OK;
    }
    p.n_contentless_delete = 0;
}

/// `fts5PrefixCompress`: tamanho do prefixo que `p_new` divide com `p_old[..n_old]`.
fn fts5_prefix_compress(n_old: i32, p_old: &[u8], p_new: &[u8]) -> i32 {
    let mut i = 0;
    while i < n_old {
        if at(p_old, i as usize) != at(p_new, i as usize) {
            break;
        }
        i += 1;
    }
    i
}

/// `fts5WriteDlidxClear`.
fn fts5_write_dlidx_clear(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    b_flush: bool,
) {
    for i in 0..writer.a_dlidx.len() {
        if writer.a_dlidx[i].buf.n() == 0 {
            break;
        }
        if b_flush {
            let rowid = fts5_dlidx_rowid(writer.i_segid, i as i32, writer.a_dlidx[i].pgno);
            fts5_data_write(p, db, cfg, rowid, &writer.a_dlidx[i].buf.p);
        }
        writer.a_dlidx[i].buf.zero();
        writer.a_dlidx[i].b_prev_valid = 0;
    }
}

/// `fts5WriteDlidxGrow`: garante pelo menos `n_lvl` escritores de dlidx.
fn fts5_write_dlidx_grow(p: &Fts5Index, writer: &mut Fts5SegWriter, n_lvl: usize) -> i32 {
    if p.rc == SQLITE_OK && n_lvl >= writer.a_dlidx.len() {
        writer.a_dlidx.resize_with(n_lvl, Fts5DlidxWriter::default);
    }
    p.rc
}

/// `fts5WriteFlushDlidx`: grava o dlidx se for grande o bastante; senão o descarta.
fn fts5_write_flush_dlidx(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
) -> i32 {
    let mut b_flag = 0;

    /* Se foram gravadas `FTS5_MIN_DLIDX_SIZE` ou mais folhas vazias, grava também o índice de
    ** doclist. */
    if writer.a_dlidx[0].buf.n() > 0 && writer.n_empty >= FTS5_MIN_DLIDX_SIZE {
        b_flag = 1;
    }
    fts5_write_dlidx_clear(p, db, cfg, writer, b_flag != 0);
    writer.n_empty = 0;
    b_flag
}

/// `fts5WriteFlushBtree`.
fn fts5_write_flush_btree(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
) {
    if writer.i_bt_page == 0 {
        return;
    }
    let b_flag = fts5_write_flush_dlidx(p, db, cfg, writer);

    if p.rc == SQLITE_OK {
        if let Some(id) = p.p_idx_writer {
            bind_blob(
                db,
                id,
                2,
                Some(&writer.btterm.p),
                writer.btterm.n(),
                StrDtor::Transient,
            );
            bind_int64(db, id, 3, b_flag as i64 + ((writer.i_bt_page as i64) << 1));
            step(db, id);
            p.rc = reset(db, id);
            bind_null(db, id, 2);
        }
    }
    writer.i_bt_page = 0;
}

/// `fts5WriteBtreeTerm`: chamada uma vez para cada folha (menos a primeira) com um termo.
fn fts5_write_btree_term(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    term: &[u8],
) {
    fts5_write_flush_btree(p, db, cfg, writer);
    if p.rc == SQLITE_OK {
        writer.btterm.set(term);
        writer.i_bt_page = writer.writer.pgno;
    }
}

/// `fts5WriteBtreeNoTerm`: chamada ao gravar uma folha sem termos.
fn fts5_write_btree_no_term(writer: &mut Fts5SegWriter) {
    /* Se a folha também não tem rowids e o índice de doclist já começou, acrescenta-lhe um byte
    ** 0x00. */
    if writer.b_first_rowid_in_page != 0 && writer.a_dlidx[0].buf.n() > 0 {
        writer.a_dlidx[0].buf.append_varint(0);
    }

    /* Incrementa o contador de folhas seguidas sem termo. */
    writer.n_empty += 1;
}

/// `fts5DlidxExtractFirstRowid`.
fn fts5_dlidx_extract_first_rowid(buf: &Fts5Buffer) -> i64 {
    let (n, _) = gv64(&buf.p, 1);
    let i_off = 1 + n;
    gv64(&buf.p, i_off).1 as i64
}

/// `fts5WriteDlidxAppend`: `i_rowid` é o primeiro rowid da folha corrente.
fn fts5_write_dlidx_append(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    i_rowid: i64,
) {
    let mut b_done = false;
    let mut i = 0usize;

    while p.rc == SQLITE_OK && !b_done {
        if writer.a_dlidx.len() <= i {
            break;
        }

        if writer.a_dlidx[i].buf.n() >= cfg.pgsz {
            /* A página corrente do índice de doclist está cheia: grava-a e sobe uma cópia de
            ** `i_rowid` (que será o primeiro rowid da próxima folha do índice) para o nível
            ** seguinte da b-tree. Se o nó gravado é a raiz, sobe também o primeiro rowid dela. */
            if let Some(b0) = writer.a_dlidx[i].buf.p.first_mut() {
                *b0 = 0x01; /* Not the root node */
            }
            let rowid = fts5_dlidx_rowid(writer.i_segid, i as i32, writer.a_dlidx[i].pgno);
            fts5_data_write(p, db, cfg, rowid, &writer.a_dlidx[i].buf.p);
            fts5_write_dlidx_grow(p, writer, i + 2);
            if p.rc == SQLITE_OK && writer.a_dlidx[i + 1].buf.n() == 0 {
                let i_first = fts5_dlidx_extract_first_rowid(&writer.a_dlidx[i].buf);
                let pgno = writer.a_dlidx[i].pgno;

                /* Era a raiz: sobe o primeiro rowid dela para a raiz nova. */
                let up = &mut writer.a_dlidx[i + 1];
                up.pgno = pgno;
                up.buf.append_varint(0);
                up.buf.append_varint(pgno as i64);
                up.buf.append_varint(i_first);
                up.b_prev_valid = 1;
                up.i_prev = i_first;
            }

            writer.a_dlidx[i].buf.zero();
            writer.a_dlidx[i].b_prev_valid = 0;
            writer.a_dlidx[i].pgno += 1;
        } else {
            b_done = true;
        }

        let i_val: i64;
        if writer.a_dlidx[i].b_prev_valid != 0 {
            i_val = (i_rowid as u64).wrapping_sub(writer.a_dlidx[i].i_prev as u64) as i64;
        } else {
            let i_pgno = if i == 0 {
                writer.writer.pgno
            } else {
                writer.a_dlidx[i - 1].pgno
            };
            let dl = &mut writer.a_dlidx[i];
            dl.buf.append_varint(!b_done as i64);
            dl.buf.append_varint(i_pgno as i64);
            i_val = i_rowid;
        }

        let dl = &mut writer.a_dlidx[i];
        dl.buf.append_varint(i_val);
        dl.b_prev_valid = 1;
        dl.i_prev = i_rowid;
        i += 1;
    }
}

/// `fts5WriteFlushLeaf`.
fn fts5_write_flush_leaf(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
) {
    /* Grava o campo `sz_leaf` do cabeçalho. */
    let n = writer.writer.buf.n();
    put_u16(&mut writer.writer.buf.p, 2, n as u16);

    if writer.b_first_term_in_page != 0 {
        /* Nenhum termo foi gravado nesta página. */
        fts5_write_btree_no_term(writer);
    } else {
        /* Acrescenta o pgidx ao buffer da página. */
        let pg = std::mem::take(&mut writer.writer.pgidx.p);
        writer.writer.buf.append_blob(&pg);
    }

    /* Grava a página no banco */
    let i_rowid = fts5_segment_rowid(writer.i_segid, writer.writer.pgno);
    fts5_data_write(p, db, cfg, i_rowid, &writer.writer.buf.p);

    /* Prepara a próxima página. */
    writer.writer.buf.zero();
    writer.writer.pgidx.zero();
    writer.writer.buf.append_blob(&[0x00, 0x00, 0x00, 0x00]);
    writer.writer.i_prev_pgidx = 0;
    writer.writer.pgno += 1;

    /* Incrementa o contador de folhas gravadas */
    writer.n_leaf_written += 1;

    /* A folha nova não tem termos nem rowids */
    writer.b_first_term_in_page = 1;
    writer.b_first_rowid_in_page = 1;
}

/// `fts5WriteAppendTerm`: acrescenta o termo ao segmento escrito.
pub(crate) fn fts5_write_append_term(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    term: &[u8],
) {
    let n_term = term.len() as i32;
    let n_min = writer.writer.term.n().min(n_term);

    /* Se a folha corrente está cheia, grava-a. */
    if (writer.writer.buf.n() + writer.writer.pgidx.n() + n_term + 2) >= cfg.pgsz
        && writer.writer.buf.n() > 4
    {
        fts5_write_flush_leaf(p, db, cfg, writer);
        if p.rc != SQLITE_OK {
            return;
        }
    }

    let delta = writer.writer.buf.n() - writer.writer.i_prev_pgidx;
    writer.writer.pgidx.append_varint(delta as i64);
    writer.writer.i_prev_pgidx = writer.writer.buf.n();

    let n_prefix;
    if writer.b_first_term_in_page != 0 {
        n_prefix = 0;
        if writer.writer.pgno != 1 {
            /* É o primeiro termo de uma folha que não é a mais à esquerda do segmento. Então a
            ** hierarquia da b-tree precisa de um termo (a) maior que o maior já gravado no
            ** segmento e (b) menor ou igual a este. Ou seja, um prefixo de `term` um byte mais
            ** longo que o maior prefixo que `term` divide com o termo anterior. */
            let mut n = n_term;
            if writer.writer.term.n() != 0 {
                n = 1 + fts5_prefix_compress(n_min, &writer.writer.term.p, term);
            }
            fts5_write_btree_term(p, db, cfg, writer, sub(term, 0, n));
            if p.rc != SQLITE_OK {
                return;
            }
        }
    } else {
        n_prefix = fts5_prefix_compress(n_min, &writer.writer.term.p, term);
        writer.writer.buf.append_varint(n_prefix as i64);
    }

    /* Acrescenta à página o número de bytes novos e depois os bytes do termo. */
    writer.writer.buf.append_varint((n_term - n_prefix) as i64);
    writer.writer.buf.append_blob(sub(term, n_prefix, n_term - n_prefix));

    /* Atualiza o campo `term` do escritor de página. */
    writer.writer.term.set(term);
    writer.b_first_term_in_page = 0;

    writer.b_first_rowid_in_page = 0;
    writer.b_first_rowid_in_doclist = 1;

    let pgno = writer.writer.pgno;
    if let Some(d) = writer.a_dlidx.get_mut(0) {
        d.pgno = pgno;
    }
}

/// `fts5WriteAppendRowid`: acrescenta um rowid (e o campo de tamanho da poslist, que o chamador
/// escreve depois).
pub(crate) fn fts5_write_append_rowid(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    i_rowid: i64,
) {
    if p.rc == SQLITE_OK {
        if (writer.writer.buf.n() + writer.writer.pgidx.n()) >= cfg.pgsz {
            fts5_write_flush_leaf(p, db, cfg, writer);
        }

        /* Se é o primeiro rowid da página, grava o ponteiro de rowid no cabeçalho dela e
        ** acrescenta um valor ao buffer do dlidx, caso um índice de doclist seja preciso. */
        if writer.b_first_rowid_in_page != 0 {
            let n = writer.writer.buf.n();
            put_u16(&mut writer.writer.buf.p, 0, n as u16);
            fts5_write_dlidx_append(p, db, cfg, writer, i_rowid);
        }

        /* Grava o rowid. */
        if writer.b_first_rowid_in_doclist != 0 || writer.b_first_rowid_in_page != 0 {
            writer.writer.buf.append_varint(i_rowid);
        } else {
            let d = (i_rowid as u64).wrapping_sub(writer.i_prev_rowid as u64);
            writer.writer.buf.append_varint(d as i64);
        }
        writer.i_prev_rowid = i_rowid;
        writer.b_first_rowid_in_doclist = 0;
        writer.b_first_rowid_in_page = 0;
    }
}

/// `fts5WriteAppendPoslistData`.
pub(crate) fn fts5_write_append_poslist_data(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    data: &[u8],
) {
    let mut a = 0i32;
    let mut n = data.len() as i32;

    while p.rc == SQLITE_OK
        && (writer.writer.buf.n() + writer.writer.pgidx.n() + n) >= cfg.pgsz
    {
        let n_req = cfg.pgsz - writer.writer.buf.n() - writer.writer.pgidx.n();
        let mut n_copy = 0;
        while n_copy < n_req {
            n_copy += gv64(data, a + n_copy).0;
        }
        writer.writer.buf.append_blob(sub(data, a, n_copy));
        a += n_copy;
        n -= n_copy;
        fts5_write_flush_leaf(p, db, cfg, writer);
    }
    if n > 0 {
        writer.writer.buf.append_blob(sub(data, a, n));
    }
}

/// `fts5WriteFinish`: grava o que o escritor ainda guarda. `*pn_leaf` recebe o número de folhas
/// (só se não há erro).
pub(crate) fn fts5_write_finish(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    writer: &mut Fts5SegWriter,
    pn_leaf: &mut i32,
) {
    if p.rc == SQLITE_OK {
        if writer.writer.buf.n() > 4 {
            fts5_write_flush_leaf(p, db, cfg, writer);
        }
        *pn_leaf = writer.writer.pgno - 1;
        if writer.writer.pgno > 1 {
            fts5_write_flush_btree(p, db, cfg, writer);
        }
    }
    writer.writer.term.free();
    writer.writer.buf.free();
    writer.writer.pgidx.free();
    writer.btterm.free();

    for d in writer.a_dlidx.iter_mut() {
        d.buf.free();
    }
    writer.a_dlidx.clear();
}

/// `fts5WriteInit`.
pub(crate) fn fts5_write_init(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_segid: i32,
) -> Fts5SegWriter {
    let mut writer = Fts5SegWriter::default();
    writer.i_segid = i_segid;

    fts5_write_dlidx_grow(p, &mut writer, 1);
    writer.writer.pgno = 1;
    writer.b_first_term_in_page = 1;
    writer.i_bt_page = 1;

    if p.p_idx_writer.is_none() {
        let z_sql = crate::printf::mprintf(
            b"INSERT INTO '%q'.'%q_idx'(segid,term,pgno) VALUES(?,?,?)",
            &db_name_args(cfg),
        );
        p.p_idx_writer = fts5_index_prepare_stmt(p, db, z_sql);
    }

    if p.rc == SQLITE_OK {
        /* Zera o cabeçalho de 4 bytes da folha. */
        writer.writer.buf.p = vec![0u8; 4];

        /* Liga o id do segmento de saída ao comando do `%_idx`. Isso poupa ligar o mesmo valor a
        ** cada linha que o escritor insere. */
        if let Some(id) = p.p_idx_writer {
            bind_int(db, id, 1, writer.i_segid);
        }
    }
    writer
}

// ---------------------------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------------------------

/// `fts5TrimSegments`: o iterador `it` leu os segmentos de entrada de um merge incremental que
/// terminou sem esgotá-los. Apara as folhas já consumidas. `lvl` é o nível de entrada; o iterador
/// `i` leu o segmento `n_input-1-i` dele.
fn fts5_trim_segments(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &Fts5Iter,
    lvl: &mut Fts5StructureLevel,
    n_input: i32,
) {
    let mut buf = Fts5Buffer::new();
    let mut i = 0;
    while i < it.n_seg && p.rc == SQLITE_OK {
        let seg = &it.a_seg[i as usize];
        let seg_idx = n_input - 1 - i;
        if seg.p_seg.is_none() || seg_idx < 0 {
            /* nada a fazer */
        } else if seg.p_leaf.is_none() {
            /* Todas as chaves deste segmento de entrada foram para a saída: põe as páginas
            ** primeira e última em 0 para indicar que o segmento está vazio. */
            lvl.a_seg[seg_idx as usize].pgno_last = 0;
            lvl.a_seg[seg_idx as usize].pgno_first = 0;
        } else {
            let i_off = seg.i_term_leaf_offset; /* Offset on new first leaf page */
            let i_id = seg.p_seg.as_ref().map_or(0, |s| s.i_segid);

            let i_leaf_rowid = fts5_segment_rowid(i_id, seg.i_term_leaf_pgno);
            let p_data = fts5_leaf_read(p, db, cfg, i_leaf_rowid);
            if let Some(data) = p_data {
                if i_off > data.sz_leaf {
                    /* Acontece se as páginas de segmentos diferentes se sobrepõem (uma página
                    ** atribuída a mais de um segmento): uma volta anterior deste laço pode ter
                    ** corrompido o segmento que está sendo aparado. */
                    p.rc = FTS5_CORRUPT;
                } else {
                    buf.zero();
                    buf.append_blob(&[0x00, 0x00, 0x00, 0x00]);
                    buf.append_varint(seg.term.n() as i64);
                    buf.append_blob(&seg.term.p);
                    buf.append_blob(sub(&data.p, i_off, data.sz_leaf - i_off));
                    if p.rc == SQLITE_OK {
                        /* Grava o campo `sz_leaf` */
                        let n = buf.n();
                        put_u16(&mut buf.p, 2, n as u16);
                    }

                    /* Monta o índice de página novo */
                    buf.append_varint(4);
                    if seg.i_leaf_pgno == seg.i_term_leaf_pgno
                        && seg.i_endof_doclist < data.sz_leaf
                        && seg.i_pgidx_off <= data.nn
                    {
                        let n_diff = data.sz_leaf - seg.i_endof_doclist;
                        let v = buf.n() - 1 - n_diff - 4;
                        buf.append_varint(v as i64);
                        buf.append_blob(sub(&data.p, seg.i_pgidx_off, data.nn - seg.i_pgidx_off));
                    }

                    lvl.a_seg[seg_idx as usize].pgno_first = seg.i_term_leaf_pgno;
                    fts5_data_delete(p, db, cfg, fts5_segment_rowid(i_id, 1), i_leaf_rowid);
                    fts5_data_write(p, db, cfg, i_leaf_rowid, &buf.p);
                }
            }
        }
        i += 1;
    }
}

/// `fts5IndexMergeLevel`: mescla o nível `i_lvl` no seguinte. `pn_rem` limita as folhas escritas.
pub(crate) fn fts5_index_merge_level(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &mut Fts5Structure,
    i_lvl: usize,
    mut pn_rem: Option<&mut i32>,
) {
    let n_rem: i32 = pn_rem.as_deref().copied().unwrap_or(0);
    let has_rem = pn_rem.is_some();
    let n_input: i32;
    let seg_out_idx: usize;
    let mut writer: Fts5SegWriter;
    let mut term = Fts5Buffer::new();
    let e_detail = cfg.e_detail;
    let flags = FTS5INDEX_QUERY_NOOUTPUT;
    let mut b_term_written = false; /* True if current term already output */

    if p_struct.a_level[i_lvl].n_merge != 0 {
        n_input = p_struct.a_level[i_lvl].n_merge;
        seg_out_idx = (p_struct.a_level[i_lvl + 1].n_seg - 1).max(0) as usize;
        let seg = p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx].clone();

        writer = fts5_write_init(p, db, cfg, seg.i_segid);
        writer.writer.pgno = seg.pgno_last + 1;
        writer.i_bt_page = 0;
    } else {
        let i_segid = fts5_allocate_segid(p, p_struct);

        /* Garante que o segmento de saída exista na estrutura. */
        if p.rc == SQLITE_OK && i_lvl == p_struct.a_level.len() - 1 {
            fts5_structure_add_level(p_struct);
        }
        if p.rc == SQLITE_OK {
            fts5_structure_extend_level(p_struct, i_lvl + 1, 1, false);
        }
        if p.rc != SQLITE_OK {
            return;
        }

        writer = fts5_write_init(p, db, cfg, i_segid);

        /* Acrescenta o segmento novo ao nível de saída */
        seg_out_idx = p_struct.a_level[i_lvl + 1].n_seg as usize;
        p_struct.a_level[i_lvl + 1].n_seg += 1;
        {
            let seg = &mut p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx];
            seg.pgno_first = 1;
            seg.i_segid = i_segid;
        }
        p_struct.n_segment += 1;

        /* Lê a entrada de todos os segmentos do nível de entrada */
        n_input = p_struct.a_level[i_lvl].n_seg;

        /* Define a faixa de origens que irá para o segmento de saída. */
        if p_struct.n_origin_cntr > 0 {
            let o1 = p_struct.a_level[i_lvl].a_seg[0].i_origin1;
            let o2 = p_struct.a_level[i_lvl].a_seg[(n_input - 1).max(0) as usize].i_origin2;
            let seg = &mut p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx];
            seg.i_origin1 = o1;
            seg.i_origin2 = o2;
        }
    }
    let b_oldest = p_struct.a_level[i_lvl + 1].n_seg == 1 && p_struct.a_level.len() == i_lvl + 2;

    let mut p_iter = fts5_multi_iter_new(p, db, cfg, p_struct, flags, None, None, i_lvl as i32, n_input);
    'outer: loop {
        if fts5_multi_iter_eof(p, &p_iter) {
            break;
        }
        'body: {
            let it = match p_iter.as_mut() {
                Some(i) => i,
                None => break 'outer,
            };
            let first = it.a_first[1].i_first as usize;
            let p_term: Vec<u8> = it.a_seg[first].term.p.clone();
            let n_term = p_term.len();

            if n_term != term.p.len() || p_term != term.p {
                if has_rem && writer.n_leaf_written > n_rem {
                    break 'outer;
                }
                term.set(&p_term);
                b_term_written = false;
            }

            /* Anulação de chave. */
            let (n_pos, b_del, i_rowid) = {
                let s = &it.a_seg[first];
                (s.n_pos, s.b_del, s.i_rowid)
            };
            if n_pos == 0 && (b_oldest || b_del == 0) {
                break 'body;
            }

            if p.rc == SQLITE_OK && !b_term_written {
                /* É um termo novo: acrescenta-o ao segmento de saída. */
                fts5_write_append_term(p, db, cfg, &mut writer, &p_term);
                b_term_written = true;
            }

            /* Acrescenta o rowid à saída */
            fts5_write_append_rowid(p, db, cfg, &mut writer, i_rowid);

            if e_detail == FTS5_DETAIL_NONE {
                if b_del != 0 {
                    writer.writer.buf.append_varint(0);
                    if n_pos > 0 {
                        writer.writer.buf.append_varint(0);
                    }
                }
            } else {
                /* Acrescenta os dados da poslist à saída */
                let n = n_pos * 2 + b_del as i32;
                writer.writer.buf.append_varint(n as i64);
                fts5_chunk_iterate(
                    p,
                    db,
                    cfg,
                    &mut it.a_seg[first],
                    &mut |p2, db2, cfg2, chunk| {
                        fts5_write_append_poslist_data(p2, db2, cfg2, &mut writer, chunk);
                    },
                );
            }
        }
        match p_iter.as_mut() {
            Some(it) => fts5_multi_iter_next(p, db, cfg, it, false, 0),
            None => break,
        }
    }

    /* Grava a última folha. Define ao mesmo tempo o último número de folha do segmento de
    ** saída. */
    let mut pgno_last = p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx].pgno_last;
    fts5_write_finish(p, db, cfg, &mut writer, &mut pgno_last);
    p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx].pgno_last = pgno_last;

    if fts5_multi_iter_eof(p, &p_iter) {
        /* Remove os segmentos redundantes de `%_data` */
        for i in 0..n_input.max(0) as usize {
            let old = p_struct.a_level[i_lvl].a_seg[i].clone();
            let add = old.n_entry.wrapping_sub(old.n_entry_tombstone);
            let seg = &mut p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx];
            seg.n_entry = seg.n_entry.wrapping_add(add);
            fts5_data_remove_segment(p, db, cfg, &old);
        }

        /* Remove os segmentos redundantes do nível de entrada */
        if p_struct.a_level[i_lvl].n_seg != n_input {
            let n = (n_input.max(0) as usize).min(p_struct.a_level[i_lvl].a_seg.len());
            p_struct.a_level[i_lvl].a_seg.drain(0..n);
        }
        p_struct.n_segment -= n_input;
        p_struct.a_level[i_lvl].n_seg -= n_input;
        p_struct.a_level[i_lvl].n_merge = 0;
        if p_struct.a_level[i_lvl + 1].a_seg[seg_out_idx].pgno_last == 0 {
            p_struct.a_level[i_lvl + 1].n_seg -= 1;
            p_struct.n_segment -= 1;
        }
    } else {
        if let Some(it) = p_iter.as_ref() {
            fts5_trim_segments(p, db, cfg, it, &mut p_struct.a_level[i_lvl], n_input);
        }
        p_struct.a_level[i_lvl].n_merge = n_input;
    }

    if let Some(r) = pn_rem.as_deref_mut() {
        *r -= writer.n_leaf_written;
    }
}

/// `fts5IndexFindDeleteMerge`: o nível com mais tombstones, ou -1.
pub(crate) fn fts5_index_find_delete_merge(cfg: &Fts5Config, p_struct: &Fts5Structure) -> i32 {
    let mut i_ret = -1;
    if cfg.b_contentless_delete != 0 && cfg.n_delete_merge > 0 {
        let mut n_best = 0;

        for (ii, lvl) in p_struct.a_level.iter().enumerate() {
            let mut n_entry: i64 = 0;
            let mut n_tomb: i64 = 0;
            for i_seg in 0..lvl.n_seg.max(0) as usize {
                n_entry = n_entry.wrapping_add(lvl.a_seg[i_seg].n_entry as i64);
                n_tomb = n_tomb.wrapping_add(lvl.a_seg[i_seg].n_entry_tombstone as i64);
            }
            if n_entry > 0 {
                let n_percent = ((n_tomb.wrapping_mul(100)) / n_entry) as i32;
                if n_percent >= cfg.n_delete_merge && n_percent > n_best {
                    i_ret = ii as i32;
                    n_best = n_percent;
                }
            }
        }
    }
    i_ret
}

/// `fts5IndexMerge`: até `n_pg` páginas de trabalho de merge. Verdadeiro se algo mudou.
pub(crate) fn fts5_index_merge(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &mut Fts5Structure,
    n_pg: i32,
    n_min: i32,
) -> bool {
    let mut n_rem = n_pg;
    let mut b_ret = false;
    let mut n_min = n_min;
    while n_rem > 0 && p.rc == SQLITE_OK {
        let mut i_best_lvl: i32 = 0; /* Level offering the most input segments */
        let mut n_best = 0; /* Number of input segments on best level */

        /* Define `i_best_lvl`, o nível de onde ler segmentos de entrada, ou -1 se nenhum nível
        ** serve para merge. */
        for (i_lvl, lvl) in p_struct.a_level.iter().enumerate() {
            if lvl.n_merge != 0 {
                if lvl.n_merge > n_best {
                    i_best_lvl = i_lvl as i32;
                    n_best = n_min;
                }
                break;
            }
            if lvl.n_seg > n_best {
                n_best = lvl.n_seg;
                i_best_lvl = i_lvl as i32;
            }
        }
        if n_best < n_min {
            i_best_lvl = fts5_index_find_delete_merge(cfg, p_struct);
        }

        if i_best_lvl < 0 {
            break;
        }
        b_ret = true;
        fts5_index_merge_level(p, db, cfg, p_struct, i_best_lvl as usize, Some(&mut n_rem));
        if p.rc == SQLITE_OK && p_struct.a_level[i_best_lvl as usize].n_merge == 0 {
            fts5_structure_promote(p, i_best_lvl as usize + 1, p_struct);
        }

        if n_min == 1 {
            n_min = 2;
        }
    }
    b_ret
}

/// `fts5IndexAutomerge`: `n_leaf` folhas acabaram de ir para um segmento de nível 0; atualiza o
/// contador e faz merge incremental se preciso.
pub(crate) fn fts5_index_automerge(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &mut Fts5Structure,
    n_leaf: i32,
) {
    if p.rc == SQLITE_OK && cfg.n_automerge > 0 {
        /* Atualiza o contador de escrita e calcula `n_work`. */
        let n_write = p_struct.n_write_counter;
        let wu = p.n_work_unit.max(1) as u64;
        let n_work = (((n_write.wrapping_add(n_leaf as u64)) / wu).wrapping_sub(n_write / wu)) as i32;
        p_struct.n_write_counter = p_struct.n_write_counter.wrapping_add(n_leaf as u64);
        let n_rem = p.n_work_unit.wrapping_mul(n_work).wrapping_mul(p_struct.n_level());

        fts5_index_merge(p, db, cfg, p_struct, n_rem, cfg.n_automerge);
    }
}

/// `fts5IndexCrisismerge`.
pub(crate) fn fts5_index_crisismerge(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &mut Fts5Structure,
) {
    let n_crisis = cfg.n_crisis_merge;
    if p_struct.n_level() > 0 {
        let mut i_lvl = 0usize;
        while p.rc == SQLITE_OK
            && i_lvl < p_struct.a_level.len()
            && p_struct.a_level[i_lvl].n_seg >= n_crisis
        {
            fts5_index_merge_level(p, db, cfg, p_struct, i_lvl, None);
            fts5_structure_promote(p, i_lvl + 1, p_struct);
            i_lvl += 1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Secure-delete
// ---------------------------------------------------------------------------------------------

/// `fts5PoslistPrefix`: o maior prefixo de `a_buf` (uma lista de varints de 32 bits) com no máximo
/// `n_max` bytes.
pub(crate) fn fts5_poslist_prefix(a_buf: &[u8], n_max: i32) -> i32 {
    let (mut ret, _dummy) = gv32(a_buf, 0);
    if ret < n_max {
        loop {
            let (i, _d) = gv32(a_buf, ret);
            if (ret + i) > n_max {
                break;
            }
            ret += i;
        }
    }
    ret
}

/// `fts5SecureDeleteIdxEntry`: `DELETE FROM %_idx WHERE (segid, (pgno/2)) = (?1, ?2)`.
fn fts5_secure_delete_idx_entry(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_segid: i32,
    i_pgno: i32,
) {
    if i_pgno != 1 {
        if p.p_delete_from_idx.is_none() {
            let z_sql = crate::printf::mprintf(
                b"DELETE FROM '%q'.'%q_idx' WHERE (segid, (pgno/2)) = (?1, ?2)",
                &db_name_args(cfg),
            );
            p.p_delete_from_idx = fts5_index_prepare_stmt(p, db, z_sql);
        }
        if p.rc == SQLITE_OK {
            if let Some(id) = p.p_delete_from_idx {
                bind_int(db, id, 1, i_segid);
                bind_int(db, id, 2, i_pgno);
                step(db, id);
                p.rc = reset(db, id);
            }
        }
    }
}

/// Copia `n` bytes de `a[src..]` para `a[dst..]` (o `memmove`), ampliando `a` se preciso.
fn fts5_memmove(a: &mut Vec<u8>, dst: i32, src: i32, n: i32) {
    if n <= 0 || dst < 0 || src < 0 {
        return;
    }
    let (dst, src, n) = (dst as usize, src as usize, n as usize);
    let need = dst.max(src) + n;
    if a.len() < need {
        a.resize(need, 0);
    }
    a.copy_within(src..src + n, dst);
}

/// `sqlite3Fts5PutVarint(&a[off], v)` sobre um `Vec` com folga.
fn fts5_put_varint_at(a: &mut Vec<u8>, off: i32, v: u64) -> i32 {
    let off = ux(off);
    if a.len() < off + 9 {
        a.resize(off + 9, 0);
    }
    fts5_put_varint(&mut a[off..], v)
}

/// `fts5SecureDeleteOverflow`: a poslist removida transborda para a página `i_pgno` do segmento;
/// reescreve essa página (e talvez as vizinhas). `*pb_last_in_doclist` fica verdadeiro se depois
/// vem um termo novo ou o fim do segmento.
fn fts5_secure_delete_overflow(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
    i_pgno: i32,
    pb_last_in_doclist: &mut i32,
) {
    let b_detail_none = cfg.e_detail == FTS5_DETAIL_NONE;

    *pb_last_in_doclist = 1;
    let mut pgno = i_pgno;
    while p.rc == SQLITE_OK && pgno <= seg.pgno_last {
        let i_rowid = fts5_segment_rowid(seg.i_segid, pgno);

        let leaf = match fts5_data_read(p, db, cfg, i_rowid) {
            Some(l) => l,
            None => break,
        };
        let mut a_pg = leaf.p.clone();

        let mut i_next = get_u16(&a_pg, 0) as i32;
        if i_next != 0 {
            *pb_last_in_doclist = 0;
        }
        if i_next == 0 && leaf.sz_leaf != leaf.nn {
            i_next = gv32(&a_pg, leaf.sz_leaf).1;
        }

        if i_next == 0 {
            /* A página não tem termos nem rowids: troca-a por uma página vazia e segue para a
            ** vizinha da direita. */
            let a_empty: [u8; 4] = [0x00, 0x00, 0x00, 0x04];
            if !b_detail_none {
                fts5_data_write(p, db, cfg, i_rowid, &a_empty);
            }
        } else if b_detail_none {
            break;
        } else if i_next >= leaf.sz_leaf || leaf.nn < leaf.sz_leaf || i_next < 4 {
            p.rc = FTS5_CORRUPT;
            break;
        } else {
            let n_shift = i_next - 4;
            let mut n_idx: i32 = 0;
            let mut a_idx: Vec<u8> = Vec::new();

            /* A menos que o rodapé da página seja de 0 bytes (e então o novo também será), monta
            ** um buffer com o rodapé novo e define `a_idx` e `n_idx`. */
            if leaf.nn > leaf.sz_leaf {
                let mut i1 = leaf.sz_leaf;
                let (nb, i_first) = gv32(&a_pg, i1);
                i1 += nb;
                if i_first < i_next {
                    p.rc = FTS5_CORRUPT;
                    break;
                }
                a_idx = vec![0u8; ux(leaf.nn - leaf.sz_leaf) + 2 + 9];
                let mut i2 = fts5_put_varint_at(&mut a_idx, 0, (i_first - n_shift) as u64);
                if i1 < leaf.nn {
                    let tailb = sub(&a_pg, i1, leaf.nn - i1).to_vec();
                    for (k, b) in tailb.iter().enumerate() {
                        a_idx[ux(i2) + k] = *b;
                    }
                    i2 += leaf.nn - i1;
                }
                n_idx = i2;
            }

            /* Altera o conteúdo de `a_pg` e define `n_pg`, o tamanho novo (sempre menor que o
            ** antigo). */
            let mut n_pg = leaf.sz_leaf - n_shift;
            fts5_memmove(&mut a_pg, 4, 4 + n_shift, n_pg - 4);
            put_u16(&mut a_pg, 2, n_pg as u16);
            if get_u16(&a_pg, 0) != 0 {
                put_u16(&mut a_pg, 0, 4);
            }
            if n_idx > 0 {
                let need = ux(n_pg) + ux(n_idx);
                if a_pg.len() < need {
                    a_pg.resize(need, 0);
                }
                for k in 0..ux(n_idx) {
                    a_pg[ux(n_pg) + k] = a_idx[k];
                }
                n_pg += n_idx;
            }

            /* Grava a página nova e sai do laço */
            fts5_data_write(p, db, cfg, i_rowid, sub(&a_pg, 0, n_pg));
            break;
        }
        pgno += 1;
    }
}

/// `fts5DoSecureDelete`: remove do banco a entrada para a qual `seg` aponta.
fn fts5_do_secure_delete(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &mut Fts5SegIter,
) {
    let b_detail_none = cfg.e_detail == FTS5_DETAIL_NONE;
    let p_seg = match seg.p_seg.clone() {
        Some(s) => s,
        None => return,
    };
    let i_segid = p_seg.i_segid;
    let leaf = match seg.p_leaf.as_ref() {
        Some(l) => l,
        None => return,
    };
    let mut a_pg: Vec<u8> = leaf.p.clone();
    a_pg.resize(a_pg.len() + 32, 0);
    let mut n_pg = leaf.nn;
    let mut i_pg_idx = leaf.sz_leaf;
    let leaf_sz = leaf.sz_leaf;

    let mut i_delta: u64 = 0;
    let mut i_next_off: i32;
    let mut i_off: i32;
    let n_idx: i32 = n_pg - i_pg_idx;
    let mut a_idx: Vec<u8> = sub(&a_pg, i_pg_idx, n_idx).to_vec();
    a_idx.resize(ux(n_idx) + 16, 0);
    let mut b_last_in_doclist: i32 = 0;
    let mut i_start: i32;
    let mut i_del_key_off: i32 = 0; /* Offset of deleted key, if any */
    let _ = leaf_sz;

    /* Aqui `seg` aponta a entrada que esta função deve remover do segmento. */
    {
        let mut i_sop: i32; /* Start-Of-Position-list */
        if seg.i_leaf_pgno == seg.i_term_leaf_pgno {
            i_start = seg.i_term_leaf_offset;
        } else {
            i_start = get_u16(&a_pg, 0) as i32;
        }

        let (nb, d) = gv64(&a_pg, i_start);
        i_delta = d;
        i_sop = i_start + nb;

        if b_detail_none {
            while (i_sop as i64) < seg.i_leaf_offset {
                if at(&a_pg, ux(i_sop)) == 0x00 {
                    i_sop += 1;
                }
                if at(&a_pg, ux(i_sop)) == 0x00 {
                    i_sop += 1;
                }
                i_start = i_sop;
                let (nb, d) = gv64(&a_pg, i_start);
                i_delta = d;
                i_sop = i_start + nb;
            }

            i_next_off = i_sop;
            if i_next_off < seg.i_endof_doclist && at(&a_pg, ux(i_next_off)) == 0x00 {
                i_next_off += 1;
            }
            if i_next_off < seg.i_endof_doclist && at(&a_pg, ux(i_next_off)) == 0x00 {
                i_next_off += 1;
            }
        } else {
            let (nb, mut n_pos) = gv32(&a_pg, i_sop);
            i_sop += nb;
            while (i_sop as i64) < seg.i_leaf_offset {
                i_start = i_sop + (n_pos / 2);
                let (nb, d) = gv64(&a_pg, i_start);
                i_delta = d;
                i_sop = i_start + nb;
                let (nb, np) = gv32(&a_pg, i_sop);
                i_sop += nb;
                n_pos = np;
            }
            i_next_off = (seg.i_leaf_offset as i32) + seg.n_pos;
        }
    }

    i_off = i_start;

    /* Se a poslist da entrada removida passa do fim desta página, apaga a parte dela na página
    ** seguinte e além. `b_last_in_doclist` fica verdadeiro se a entrada é o último rowid da
    ** doclist do seu termo. */
    if i_next_off >= i_pg_idx {
        let pgno = seg.i_leaf_pgno + 1;
        fts5_secure_delete_overflow(p, db, cfg, &p_seg, pgno, &mut b_last_in_doclist);
        i_next_off = i_pg_idx;
    }

    if seg.b_del == 0 {
        if i_next_off != i_pg_idx {
            /* Percorre o rodapé da página. Se `i_next_off` (deslocamento da entrada que segue a
            ** removida) é igual ao deslocamento de uma chave desta página, a entrada é a última
            ** da sua doclist. */
            let mut i_key_off: i32 = 0;
            let mut i_idx = 0;
            while i_idx < n_idx {
                let (nb, i_val) = gv32(&a_idx, i_idx);
                i_idx += nb;
                i_key_off = i_key_off.wrapping_add(i_val);
                if i_key_off == i_next_off {
                    b_last_in_doclist = 1;
                }
            }
        }

        /* Se é (a) o primeiro rowid da página e (b) não é seguido de outra poslist na mesma
        ** página, zera o campo "primeiro rowid" do cabeçalho. */
        if get_u16(&a_pg, 0) as i32 == i_start && (b_last_in_doclist != 0 || i_next_off == i_pg_idx) {
            put_u16(&mut a_pg, 0, 0);
        }
    }

    if seg.b_del != 0 {
        i_off += fts5_put_varint_at(&mut a_pg, i_off, i_delta);
        let o = ux(i_off);
        if a_pg.len() <= o {
            a_pg.resize(o + 1, 0);
        }
        a_pg[o] = 0x01;
        i_off += 1;
    } else if b_last_in_doclist == 0 {
        if i_next_off != i_pg_idx {
            let (nb, i_next_delta) = gv64(&a_pg, i_next_off);
            i_next_off += nb;
            i_off += fts5_put_varint_at(&mut a_pg, i_off, i_delta.wrapping_add(i_next_delta));
        }
    } else if seg.i_leaf_pgno == seg.i_term_leaf_pgno && i_start == seg.i_term_leaf_offset {
        /* A entrada removida era a única poslist da sua doclist: o termo também precisa sair. */
        let mut i_key: i32 = 0;
        let mut i_key_off: i32 = 0;

        /* `i_key_off` recebe o deslocamento do termo que sai: o último deslocamento do rodapé
        ** que não passa de `i_start`. */
        let mut i_idx = 0;
        while i_idx < n_idx {
            let (nb, i_val) = gv32(&a_idx, i_idx);
            i_idx += nb;
            if (i_key_off as u32).wrapping_add(i_val as u32) > i_start as u32 {
                break;
            }
            i_key_off = i_key_off.wrapping_add(i_val);
            i_key += 1;
        }

        /* `i_del_key_off` recebe o valor da entrada do rodapé a remover da página. */
        i_off = i_key_off;
        i_del_key_off = i_key_off;

        if i_next_off != i_pg_idx {
            /* É a única poslist do termo e há outro termo depois dele nesta página: o termo
            ** seguinte precisa ocupar o lugar do termo da entrada removida. */
            let mut n_prefix: i32 = 0;

            i_del_key_off = i_next_off;
            let (nb, n_prefix2) = gv32(&a_pg, i_next_off);
            i_next_off += nb;
            let (nb, n_suffix2) = gv32(&a_pg, i_next_off);
            i_next_off += nb;

            if i_key != 1 {
                let (nb, np) = gv32(&a_pg, i_key_off);
                i_key_off += nb;
                n_prefix = np;
            }
            let (nb, _n_suffix) = gv32(&a_pg, i_key_off);
            i_key_off += nb;

            n_prefix = n_prefix.min(n_prefix2);
            let n_suffix = (n_prefix2 + n_suffix2) - n_prefix;

            if (i_key_off + n_suffix) > i_pg_idx || (i_next_off + n_suffix2) > i_pg_idx {
                p.rc = FTS5_CORRUPT;
            } else {
                if i_key != 1 {
                    i_off += fts5_put_varint_at(&mut a_pg, i_off, n_prefix as u64);
                }
                i_off += fts5_put_varint_at(&mut a_pg, i_off, n_suffix as u64);
                if n_prefix2 > seg.term.n() {
                    p.rc = FTS5_CORRUPT;
                } else if n_prefix2 > n_prefix {
                    let part = sub(&seg.term.p, n_prefix, n_prefix2 - n_prefix).to_vec();
                    let need = ux(i_off) + part.len();
                    if a_pg.len() < need {
                        a_pg.resize(need, 0);
                    }
                    for (k, b) in part.iter().enumerate() {
                        a_pg[ux(i_off) + k] = *b;
                    }
                    i_off += n_prefix2 - n_prefix;
                }
                fts5_memmove(&mut a_pg, i_off, i_next_off, n_suffix2);
                i_off += n_suffix2;
                i_next_off += n_suffix2;
            }
        }
    } else if i_start == 4 {
        /* A entrada removida pode ser a única poslist da sua doclist. */
        let mut i_pgno = seg.i_leaf_pgno - 1;
        while i_pgno > seg.i_term_leaf_pgno {
            let p_pg = fts5_data_read(p, db, cfg, fts5_segment_rowid(i_segid, i_pgno));
            let b_empty = p_pg.as_ref().map_or(false, |d| d.nn == 4);
            if !b_empty {
                break;
            }
            i_pgno -= 1;
        }

        if i_pgno == seg.i_term_leaf_pgno {
            let i_id = fts5_segment_rowid(i_segid, seg.i_term_leaf_pgno);
            if let Some(mut p_term) = fts5_data_read(p, db, cfg, i_id) {
                if p_term.sz_leaf == seg.i_term_leaf_offset {
                    let a_term_idx: Vec<u8> = sub(&p_term.p, p_term.sz_leaf, p_term.nn - p_term.sz_leaf).to_vec();
                    let mut n_term_idx = p_term.nn - p_term.sz_leaf;
                    let mut i_term_idx = 0;
                    let mut i_term_off: i32 = 0;

                    loop {
                        let (n_byte, i_val) = gv32(&a_term_idx, i_term_idx);
                        i_term_off = i_term_off.wrapping_add(i_val);
                        if (i_term_idx + n_byte) >= n_term_idx {
                            break;
                        }
                        i_term_idx += n_byte;
                    }
                    n_term_idx = i_term_idx;

                    let sz = p_term.sz_leaf;
                    fts5_memmove(&mut p_term.p, i_term_off, sz, n_term_idx);
                    put_u16(&mut p_term.p, 2, i_term_off as u16);

                    fts5_data_write(p, db, cfg, i_id, sub(&p_term.p, 0, i_term_off + n_term_idx));
                    if n_term_idx == 0 {
                        fts5_secure_delete_idx_entry(p, db, cfg, i_segid, seg.i_term_leaf_pgno);
                    }
                }
            }
        }
    }

    /* Se não houve erro, este bloco faz os ajustes finais na folha antes de gravá-la. Entradas:
    **
    **   n_pg: tamanho inicial da folha.
    **   i_pg_idx: deslocamento inicial do rodapé.
    **
    **   i_off: deslocamento para onde mover os dados
    **   i_next_off: deslocamento de onde mover os dados
    */
    if p.rc == SQLITE_OK {
        let n_move = n_pg - i_next_off; /* Number of bytes to move */
        let n_shift = i_next_off - i_off; /* Distance to move them */

        let mut i_prev_key_out: i32 = 0;
        let mut i_key_in: i32 = 0;

        fts5_memmove(&mut a_pg, i_off, i_next_off, n_move);
        i_pg_idx -= n_shift;
        n_pg = i_pg_idx;
        put_u16(&mut a_pg, 2, i_pg_idx as u16);

        let mut i_idx = 0;
        while i_idx < n_idx {
            let (nb, i_val) = gv32(&a_idx, i_idx);
            i_idx += nb;
            i_key_in = i_key_in.wrapping_add(i_val);
            if i_key_in != i_del_key_off {
                let i_key_out = i_key_in - (if i_key_in > i_off { n_shift } else { 0 });
                n_pg += fts5_put_varint_at(&mut a_pg, n_pg, (i_key_out - i_prev_key_out) as u64);
                i_prev_key_out = i_key_out;
            }
        }

        if i_pg_idx == n_pg && n_idx > 0 && seg.i_leaf_pgno != 1 {
            fts5_secure_delete_idx_entry(p, db, cfg, i_segid, seg.i_leaf_pgno);
        }

        fts5_data_write(
            p,
            db,
            cfg,
            fts5_segment_rowid(i_segid, seg.i_leaf_pgno),
            sub(&a_pg, 0, n_pg),
        );
    }
}

/// `fts5FlushSecureDelete`: edita os segmentos do banco para remover a entrada `(term, rowid)`.
fn fts5_flush_secure_delete(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &Fts5Structure,
    term: &[u8],
    i_rowid: i64,
) {
    let f = FTS5INDEX_QUERY_SKIPHASH;
    let mut p_iter = fts5_multi_iter_new(p, db, cfg, p_struct, f, None, Some(term), -1, 0);
    if !fts5_multi_iter_eof(p, &p_iter) {
        if let Some(it) = p_iter.as_mut() {
            let i_this = fts5_multi_iter_rowid(it);
            if i_this < i_rowid {
                fts5_multi_iter_next_from(p, db, cfg, it, i_rowid);
            }

            if p.rc == SQLITE_OK && it.base.b_eof == 0 && i_rowid == fts5_multi_iter_rowid(it) {
                let first = it.a_first[1].i_first as usize;
                fts5_do_secure_delete(p, db, cfg, &mut it.a_seg[first]);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Flush
// ---------------------------------------------------------------------------------------------

/// `fts5FlushOneHash`: grava o conteúdo do hash num segmento novo de nível 0 e atualiza o registro
/// de estrutura.
fn fts5_flush_one_hash(p: &mut Fts5Index, db: &mut Connection, cfg: &mut Fts5Config) {
    let mut pgno_last = 0; /* Last leaf page number in segment */

    /* Lê a estrutura do índice e aloca um id de segmento para o novo segmento de nível 0. */
    let p_struct_opt = fts5_structure_read(p, db, cfg);
    fts5_structure_invalidate(p);
    let mut p_struct = match p_struct_opt {
        Some(s) => s,
        None => return,
    };
    let cfg: &Fts5Config = cfg;

    let hash_empty = p.p_hash.as_ref().map_or(true, |h| h.is_empty());
    if !hash_empty {
        let i_segid = fts5_allocate_segid(p, &p_struct);
        if i_segid != 0 {
            let pgsz = cfg.pgsz;
            let e_detail = cfg.e_detail;
            let b_secure_delete = cfg.b_secure_delete != 0;

            let mut writer = fts5_write_init(p, db, cfg, i_segid);

            /* Percorre as entradas do hash. O laço roda uma vez por termo/doclist guardado nele. */
            if p.rc == SQLITE_OK {
                p.rc = p.p_hash.as_mut().map_or(SQLITE_OK, |h| h.scan_init(&[]));
            }
            while p.rc == SQLITE_OK && !p.p_hash.as_ref().map_or(true, |h| h.scan_eof()) {
                /* Pega o termo e a doclist desta entrada. */
                let entry = p
                    .p_hash
                    .as_mut()
                    .and_then(|h| h.scan_entry().map(|(z, d)| (z.to_vec(), d.to_vec())));
                let (z_term, pdoclist) = match entry {
                    Some(e) => e,
                    None => break,
                };
                let mut n_doclist = pdoclist.len() as i32;
                if !b_secure_delete {
                    fts5_write_append_term(p, db, cfg, &mut writer, &z_term);
                    if p.rc != SQLITE_OK {
                        break;
                    }
                }

                if !b_secure_delete
                    && pgsz >= (writer.writer.buf.n() + writer.writer.pgidx.n() + n_doclist + 1)
                {
                    /* A doclist inteira cabe na folha corrente. */
                    writer.writer.buf.append_blob(&pdoclist);
                } else {
                    let mut b_term_written = !b_secure_delete;
                    let mut i_rowid: i64 = 0;
                    let mut i_prev: i64 = 0;
                    let mut i_off: i32 = 0;

                    /* A doclist inteira não cabe nesta folha: o laço seguinte percorre as poslists
                    ** que a compõem. */
                    while p.rc == SQLITE_OK && i_off < n_doclist {
                        let (nb, i_delta) = gv64(&pdoclist, i_off);
                        i_off += nb;
                        i_rowid = i_rowid.wrapping_add(i_delta as i64);

                        /* No modo secure-delete, se esta entrada da poslist é de fato um delete,
                        ** edita os segmentos existentes direto com `fts5_flush_secure_delete`. */
                        if b_secure_delete {
                            if e_detail == FTS5_DETAIL_NONE {
                                if i_off < n_doclist && at(&pdoclist, ux(i_off)) == 0x00 {
                                    fts5_flush_secure_delete(p, db, cfg, &p_struct, &z_term, i_rowid);
                                    i_off += 1;
                                    if i_off < n_doclist && at(&pdoclist, ux(i_off)) == 0x00 {
                                        i_off += 1;
                                        n_doclist = 0;
                                    } else {
                                        continue;
                                    }
                                }
                            } else if (at(&pdoclist, ux(i_off)) & 0x01) != 0 {
                                fts5_flush_secure_delete(p, db, cfg, &p_struct, &z_term, i_rowid);
                                if p.rc != SQLITE_OK || at(&pdoclist, ux(i_off)) == 0x01 {
                                    i_off += 1;
                                    continue;
                                }
                            }
                        }

                        if p.rc == SQLITE_OK && !b_term_written {
                            fts5_write_append_term(p, db, cfg, &mut writer, &z_term);
                            b_term_written = true;
                        }

                        if writer.b_first_rowid_in_page != 0 {
                            let n = writer.writer.buf.n();
                            put_u16(&mut writer.writer.buf.p, 0, n as u16); /* first rowid on page */
                            writer.writer.buf.append_varint(i_rowid);
                            writer.b_first_rowid_in_page = 0;
                            fts5_write_dlidx_append(p, db, cfg, &mut writer, i_rowid);
                        } else {
                            let i_rowid_delta = (i_rowid as u64).wrapping_sub(i_prev as u64);
                            writer.writer.buf.append_varint(i_rowid_delta as i64);
                        }
                        if p.rc != SQLITE_OK {
                            break;
                        }
                        i_prev = i_rowid;

                        if e_detail == FTS5_DETAIL_NONE {
                            if i_off < n_doclist && at(&pdoclist, ux(i_off)) == 0 {
                                writer.writer.buf.p.push(0);
                                i_off += 1;
                                if i_off < n_doclist && at(&pdoclist, ux(i_off)) == 0 {
                                    writer.writer.buf.p.push(0);
                                    i_off += 1;
                                }
                            }
                            if (writer.writer.buf.n() + writer.writer.pgidx.n()) >= pgsz {
                                fts5_write_flush_leaf(p, db, cfg, &mut writer);
                            }
                        } else {
                            let (nb0, n_pos, b_del) = fts5_get_poslist_size(&pdoclist, i_off);
                            let mut n_copy = nb0;
                            if b_del != 0 && b_secure_delete {
                                writer.writer.buf.append_varint((n_pos * 2) as i64);
                                i_off += n_copy;
                                n_copy = n_pos;
                            } else {
                                n_copy += n_pos;
                            }
                            if (writer.writer.buf.n() + writer.writer.pgidx.n() + n_copy) <= pgsz {
                                /* A poslist inteira cabe na folha corrente: copia de uma vez. */
                                writer.writer.buf.append_blob(sub(&pdoclist, i_off, n_copy));
                            } else {
                                /* A poslist inteira não cabe nesta folha: é partida em seções,
                                ** com a única restrição de que cada varint fique contíguo. */
                                let p_poslist = sub(&pdoclist, i_off, n_copy.max(0)).to_vec();
                                let mut i_pos = 0;
                                while p.rc == SQLITE_OK {
                                    let n_space = pgsz - writer.writer.buf.n() - writer.writer.pgidx.n();
                                    let n;
                                    if (n_copy - i_pos) <= n_space {
                                        n = n_copy - i_pos;
                                    } else {
                                        n = fts5_poslist_prefix(tail(&p_poslist, ux(i_pos)), n_space);
                                    }
                                    writer.writer.buf.append_blob(sub(&p_poslist, i_pos, n));
                                    i_pos += n;
                                    if (writer.writer.buf.n() + writer.writer.pgidx.n()) >= pgsz {
                                        fts5_write_flush_leaf(p, db, cfg, &mut writer);
                                    }
                                    if i_pos >= n_copy || n <= 0 {
                                        break;
                                    }
                                }
                            }
                            i_off += n_copy;
                        }
                    }
                }

                /* O terminador da doclist (TODO2 do C) não é escrito: o formato não o tem. */
                if p.rc == SQLITE_OK {
                    if let Some(h) = p.p_hash.as_mut() {
                        h.scan_next();
                    }
                }
            }
            fts5_write_finish(p, db, cfg, &mut writer, &mut pgno_last);

            if pgno_last > 0 {
                /* Atualiza a estrutura; ela é gravada pelo `fts5_structure_write` mais abaixo. */
                if p_struct.n_level() == 0 && p.rc == SQLITE_OK {
                    fts5_structure_add_level(&mut p_struct);
                }
                if p.rc == SQLITE_OK {
                    fts5_structure_extend_level(&mut p_struct, 0, 1, false);
                }
                if p.rc == SQLITE_OK {
                    let n_origin_cntr = p_struct.n_origin_cntr;
                    let n_pending_row = p.n_pending_row;
                    let idx = p_struct.a_level[0].n_seg as usize;
                    p_struct.a_level[0].n_seg += 1;
                    {
                        let seg = &mut p_struct.a_level[0].a_seg[idx];
                        seg.i_segid = i_segid;
                        seg.pgno_first = 1;
                        seg.pgno_last = pgno_last;
                        if n_origin_cntr > 0 {
                            seg.i_origin1 = n_origin_cntr;
                            seg.i_origin2 = n_origin_cntr;
                            seg.n_entry = n_pending_row as u64;
                        }
                    }
                    if n_origin_cntr > 0 {
                        p_struct.n_origin_cntr += 1;
                    }
                    p_struct.n_segment += 1;
                }
                fts5_structure_promote(p, 0, &mut p_struct);
            }
        }
    }

    let n_cd = p.n_contentless_delete;
    fts5_index_automerge(p, db, cfg, &mut p_struct, pgno_last + n_cd);
    fts5_index_crisismerge(p, db, cfg, &mut p_struct);
    fts5_structure_write(p, db, cfg, &p_struct);
}

/// `fts5IndexFlush`: grava o conteúdo do hash em memória.
pub(crate) fn fts5_index_flush(p: &mut Fts5Index, db: &mut Connection, cfg: &mut Fts5Config) {
    /* Se o hash não está vazio, grava-o */
    if p.flush_rc != SQLITE_OK {
        p.rc = p.flush_rc;
        return;
    }
    if p.n_pending_data() != 0 || p.n_contentless_delete != 0 {
        fts5_flush_one_hash(p, db, cfg);
        if p.rc == SQLITE_OK {
            if let Some(h) = p.p_hash.as_mut() {
                h.clear();
            }
            p.clear_pending_data();
            p.n_pending_row = 0;
            p.n_contentless_delete = 0;
        } else if p.n_pending_data() != 0 || p.n_contentless_delete != 0 {
            p.flush_rc = p.rc;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Optimize e merge do usuário
// ---------------------------------------------------------------------------------------------

/// `fts5IndexOptimizeStruct`: uma estrutura com todos os segmentos no último nível, ou `None` se
/// não há o que otimizar. Se já está otimizada devolve uma cópia dela.
pub(crate) fn fts5_index_optimize_struct(
    _p: &mut Fts5Index,
    p_struct: &Fts5Structure,
) -> Option<Fts5Structure> {
    let n_seg = p_struct.n_segment;

    /* Descobre se esta estrutura precisa de otimização. Não precisa se:
    **
    **  1. tem menos de dois segmentos, ou
    **  2. todos os segmentos estão no mesmo nível, ou
    **  3. todos os segmentos menos um são entrada de um merge em andamento.
    */
    if n_seg == 0 {
        return None;
    }
    for lvl in p_struct.a_level.iter() {
        let n_this = lvl.n_seg;
        let n_merge = lvl.n_merge;
        if n_this > 0 && (n_this == n_seg || (n_this == n_seg - 1 && n_merge == n_this)) {
            if n_seg == 1 && n_this == 1 && lvl.a_seg[0].n_pg_tombstone == 0 {
                return None;
            }
            return Some(p_struct.clone());
        }
    }

    let n_level = (p_struct.n_level() + 1).min(FTS5_MAX_LEVEL) as usize;
    let mut p_new = Fts5Structure::default();
    p_new.a_level = vec![Fts5StructureLevel::default(); n_level];
    p_new.n_write_counter = p_struct.n_write_counter;
    p_new.n_origin_cntr = p_struct.n_origin_cntr;
    let mut a_seg: Vec<Fts5StructureSegment> = Vec::with_capacity(n_seg.max(0) as usize);

    /* Percorre todos os segmentos, do mais antigo ao mais novo, e os põe no nível novo de modo
    ** que `a_seg[0]` seja o segmento mais antigo. */
    for lvl in p_struct.a_level.iter().rev() {
        for i_seg in 0..lvl.n_seg.max(0) as usize {
            a_seg.push(lvl.a_seg[i_seg].clone());
        }
    }
    p_new.n_segment = n_seg;
    let last = &mut p_new.a_level[n_level - 1];
    last.a_seg = a_seg;
    last.n_seg = n_seg;
    Some(p_new)
}

impl Fts5Index {
    /// `sqlite3Fts5IndexOptimize`: funde todos os segmentos num só.
    pub fn optimize(&mut self, db: &mut Connection, cfg: &mut Fts5Config) -> i32 {
        fts5_index_flush(self, db, cfg);
        let p_struct = fts5_structure_read(self, db, cfg);
        fts5_structure_invalidate(self);

        let mut p_new = None;
        if let Some(s) = p_struct.as_ref() {
            p_new = fts5_index_optimize_struct(self, s);
        }

        if let Some(mut new) = p_new {
            let mut i_lvl = 0usize;
            while i_lvl < new.a_level.len() && new.a_level[i_lvl].n_seg == 0 {
                i_lvl += 1;
            }
            while self.rc == SQLITE_OK && i_lvl < new.a_level.len() && new.a_level[i_lvl].n_seg > 0 {
                let mut n_rem = FTS5_OPT_WORK_UNIT;
                fts5_index_merge_level(self, db, cfg, &mut new, i_lvl, Some(&mut n_rem));
            }

            fts5_structure_write(self, db, cfg, &new);
        }

        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexMerge`: o comando especial `INSERT INTO t(t, rank) VALUES('merge', n)`.
    pub fn merge(&mut self, db: &mut Connection, cfg: &mut Fts5Config, n_merge: i32) -> i32 {
        let mut n_merge = n_merge;

        fts5_index_flush(self, db, cfg);
        let p_struct = fts5_structure_read(self, db, cfg);
        if let Some(s) = p_struct {
            let mut n_min = cfg.n_usermerge;
            let mut cur: Option<Fts5Structure> = Some(s);
            fts5_structure_invalidate(self);
            if n_merge < 0 {
                let p_new = match cur.as_ref() {
                    Some(c) => fts5_index_optimize_struct(self, c),
                    None => None,
                };
                cur = p_new;
                n_min = 1;
                n_merge = n_merge.wrapping_mul(-1);
            }
            if let Some(st) = cur.as_mut() {
                if st.n_level() > 0 && fts5_index_merge(self, db, cfg, st, n_merge, n_min) {
                    fts5_structure_write(self, db, cfg, st);
                }
            }
        }
        fts5_index_return(self)
    }
}

