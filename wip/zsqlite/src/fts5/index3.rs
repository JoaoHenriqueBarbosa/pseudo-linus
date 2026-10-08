//! `fts5_index.c` (parte 3): os iteradores de prefixo (mescla de doclists), a interface pública
//! (`Fts5Index::open`, `begin_write`, `write`, `query`, `sync`, `rollback`, ...), o tokendata
//! (`tokendata=1`), os tombstones do `contentless_delete=1` e a verificação de integridade. Veja o
//! cabeçalho de [`super::index`] para os desvios do C comuns às três partes.
//!
//! # A interface que `storage`, `main`, `expr` e `vocab` chamam
//!
//! Toda função recebe `db: &mut Connection` e a configuração; as que podem recarregar `%_config`
//! (as que chegam em `fts5_structure_read`) recebem `&mut Fts5Config`.
//!
//! * `Fts5Index::open(db, cfg, b_create, pz_err) -> Result<Fts5Index, i32>` e `close(db)`.
//! * `begin_write(db, cfg, b_delete, rowid)`, `write(cfg, i_col, i_pos, token)`, `sync`,
//!   `rollback`, `reinit`, `optimize`, `merge`, `integrity_check`, `reset`, `load_config`,
//!   `set_cookie`, `get_averages`, `set_averages`, `reads`, `get_origin`, `contentless_delete`.
//! * `query(db, cfg, token, flags, colset) -> Result<Fts5Iter, i32>` e os métodos do `Fts5Iter`:
//!   `eof`, `rowid`, `data`, `next`, `next_from`, `next_scan`, `term`, `token`, `close`,
//!   `clear_tokendata`, `write_tokendata`.
//! * `structure_ref`, `structure_release`, `structure_test` (identidade da estrutura).
//! * Livres: `fts5_index_entry_cksum`, `fts5_index_charlen_to_bytelen`, `fts5_index_init`.
//!
//! `Fts5Index::open` com `b_create` chama `super::storage::fts5_create_table(db, cfg, z_post,
//! z_defn, b_without, pz_err) -> i32` (o `sqlite3Fts5CreateTable`, que vive no `fts5_storage.c`).

use crate::connection::Connection;
use crate::consts::{SQLITE_ERROR, SQLITE_OK, SQLITE_ROW};
use crate::printf::PrintfArg;
use crate::util::at;
use crate::vdbeapi::{column_blob, column_int, finalize, step};

use super::buffer::{
    fts5_mprintf, fts5_poslist_next64, fts5_poslist_safe_append, fts5_pos2column, fts5_pos2offset,
    Fts5Buffer, Fts5PoslistReader,
};
use super::hash::Fts5Hash;
use super::index::*;
use super::int::{
    Fts5Colset, Fts5Config, FTS5INDEX_QUERY_DESC, FTS5INDEX_QUERY_NOOUTPUT,
    FTS5INDEX_QUERY_NOTOKENDATA, FTS5INDEX_QUERY_PREFIX, FTS5INDEX_QUERY_SCAN,
    FTS5INDEX_QUERY_SCANONETERM, FTS5INDEX_QUERY_SKIPEMPTY, FTS5_CORRUPT,
    FTS5_CURRENT_VERSION_SECUREDELETE, FTS5_DETAIL_FULL, FTS5_DETAIL_NONE,
};

/// `FTS5_MERGE_NLIST`.
const FTS5_MERGE_NLIST: usize = 16;

/// Compara duas fatias como o `memcmp` seguido da diferença de tamanho (só o sinal conta).
fn fts5_bytes_compare(a: &[u8], b: &[u8]) -> i32 {
    let n = a.len().min(b.len());
    match a[..n].cmp(&b[..n]) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => {
            if a.len() < b.len() {
                -1
            } else if a.len() > b.len() {
                1
            } else {
                0
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mescla de doclists (prefixos)
// ---------------------------------------------------------------------------------------------

/// `Fts5DoclistIter`: percorre uma doclist guardada num buffer (a cópia dele).
#[derive(Default)]
struct DoclistIter {
    /// A doclist.
    data: Vec<u8>,
    /// Deslocamento da poslist corrente (`None` no fim; o `aPoslist==0` do C).
    a_poslist: Option<usize>,
    /// Bytes do campo de tamanho da poslist.
    n_size: i32,
    /// Bytes da poslist.
    n_poslist: i32,
    /// Rowid corrente.
    i_rowid: i64,
}

/// `fts5DoclistIterNext`.
fn fts5_doclist_iter_next(it: &mut DoclistIter) {
    let a = match it.a_poslist {
        Some(a) => a,
        None => return,
    };
    let mut p = a as i32 + it.n_size + it.n_poslist;
    let a_eof = it.data.len() as i32;

    if p >= a_eof {
        it.a_poslist = None;
    } else {
        let (nb, i_delta) = gv64(&it.data, p);
        p += nb;
        it.i_rowid = it.i_rowid.wrapping_add(i_delta as i64);

        /* Lê o tamanho da poslist */
        if at(&it.data, ux(p)) & 0x80 != 0 {
            let (n, n_pos) = gv32(&it.data, p);
            it.n_size = n;
            it.n_poslist = n_pos >> 1;
        } else {
            it.n_poslist = (at(&it.data, ux(p)) as i32) >> 1;
            it.n_size = 1;
        }

        it.a_poslist = Some(p as usize);
        if p + it.n_poslist > a_eof {
            it.a_poslist = None;
        }
    }
}

/// `fts5DoclistIterInit`.
fn fts5_doclist_iter_init(buf: &Fts5Buffer) -> DoclistIter {
    let mut it = DoclistIter::default();
    if buf.n() > 0 {
        it.data = buf.p.clone();
        it.a_poslist = Some(0);
        fts5_doclist_iter_next(&mut it);
    }
    it
}

/// `fts5NextRowid`.
fn fts5_next_rowid(buf: &Fts5Buffer, pi_off: &mut i32, pi_rowid: &mut i64) {
    let i = *pi_off;
    if i >= buf.n() {
        *pi_off = -1;
    } else {
        let (nb, i_val) = gv64(&buf.p, i);
        *pi_off = i + nb;
        *pi_rowid = pi_rowid.wrapping_add(i_val as i64);
    }
}

/// `fts5MergeRowidLists`: o equivalente de `fts5_merge_prefix_lists` para `detail=none` (as listas
/// só têm rowids). `a_buf` tem exatamente uma lista.
fn fts5_merge_rowid_lists(_p: &mut Fts5Index, p1: &mut Fts5Buffer, a_buf: &mut [Fts5Buffer]) {
    let mut i1: i32 = 0;
    let mut i2: i32 = 0;
    let mut i_rowid1: i64 = 0;
    let mut i_rowid2: i64 = 0;
    let mut i_out: i64 = 0;
    let mut out = Fts5Buffer::new();
    let p2 = &a_buf[0];

    fts5_next_rowid(p1, &mut i1, &mut i_rowid1);
    fts5_next_rowid(p2, &mut i2, &mut i_rowid2);
    while i1 >= 0 || i2 >= 0 {
        if i1 >= 0 && (i2 < 0 || i_rowid1 < i_rowid2) {
            out.append_varint(i_rowid1.wrapping_sub(i_out));
            i_out = i_rowid1;
            fts5_next_rowid(p1, &mut i1, &mut i_rowid1);
        } else {
            out.append_varint(i_rowid2.wrapping_sub(i_out));
            i_out = i_rowid2;
            if i1 >= 0 && i_rowid1 == i_rowid2 {
                fts5_next_rowid(p1, &mut i1, &mut i_rowid1);
            }
            fts5_next_rowid(p2, &mut i2, &mut i_rowid2);
        }
    }

    std::mem::swap(&mut out, p1);
}

/// `PrefixMerger`.
#[derive(Default)]
struct PrefixMerger {
    /// O iterador da doclist.
    iter: DoclistIter,
    /// Posição corrente na poslist.
    i_pos: i64,
    /// Deslocamento na poslist.
    i_off: i32,
    /// Deslocamento (em `iter.data`) do começo da poslist, sem o campo de tamanho.
    a_pos: usize,
    /// Próximo, na ordem de rowid ou de posição.
    p_next: Option<usize>,
}

/// `fts5PrefixMergerInsertByRowid`.
fn fts5_prefix_merger_insert_by_rowid(a: &mut [PrefixMerger], head: &mut Option<usize>, idx: usize) {
    if a[idx].iter.a_poslist.is_some() {
        let mut prev: Option<usize> = None;
        let mut cur = *head;
        while let Some(c) = cur {
            if a[idx].iter.i_rowid > a[c].iter.i_rowid {
                prev = Some(c);
                cur = a[c].p_next;
            } else {
                break;
            }
        }
        a[idx].p_next = cur;
        match prev {
            None => *head = Some(idx),
            Some(pv) => a[pv].p_next = Some(idx),
        }
    }
}

/// `fts5PrefixMergerInsertByPosition`.
fn fts5_prefix_merger_insert_by_position(
    a: &mut [PrefixMerger],
    head: &mut Option<usize>,
    idx: usize,
) {
    if a[idx].i_pos >= 0 {
        let mut prev: Option<usize> = None;
        let mut cur = *head;
        while let Some(c) = cur {
            if a[idx].i_pos > a[c].i_pos {
                prev = Some(c);
                cur = a[c].p_next;
            } else {
                break;
            }
        }
        a[idx].p_next = cur;
        match prev {
            None => *head = Some(idx),
            Some(pv) => a[pv].p_next = Some(idx),
        }
    }
}

/// `fts5PrefixMergerNextPosition`.
fn fts5_prefix_merger_next_position(m: &mut PrefixMerger) {
    let list = sub(&m.iter.data, m.a_pos as i32, m.iter.n_poslist);
    fts5_poslist_next64(list, &mut m.i_off, &mut m.i_pos);
}

/// `fts5MergePrefixLists`: junta as doclists de `a_buf` à doclist `p1`.
fn fts5_merge_prefix_lists(p: &mut Fts5Index, p1: &mut Fts5Buffer, a_buf: &mut [Fts5Buffer]) {
    let n_buf = a_buf.len();
    debug_assert!(n_buf + 1 <= FTS5_MERGE_NLIST);

    /* Inicia um iterador de doclist para cada buffer de entrada e os encadeia a partir de
    ** `p_head` em ordem crescente de rowid, sem encadear os que já estão no fim. */
    let mut a_merger: Vec<PrefixMerger> = (0..=n_buf).map(|_| PrefixMerger::default()).collect();
    let mut p_head: Option<usize> = Some(n_buf);
    a_merger[n_buf].iter = fts5_doclist_iter_init(p1);
    let mut n_out: i32 = 0;
    for i in 0..n_buf {
        a_merger[i].iter = fts5_doclist_iter_init(&a_buf[i]);
        fts5_prefix_merger_insert_by_rowid(&mut a_merger, &mut p_head, i);
        n_out += a_buf[i].n();
    }
    if n_out == 0 {
        return;
    }

    let mut out = Fts5Buffer::new();
    let mut tmp = Fts5Buffer::new();
    let mut i_last_rowid: i64 = 0;

    while let Some(h) = p_head {
        /* fts5MergeAppendDocid */
        let rowid = a_merger[h].iter.i_rowid;
        out.append_varint((rowid as u64).wrapping_sub(i_last_rowid as u64) as i64);
        i_last_rowid = rowid;

        let next = a_merger[h].p_next;
        let merge_here = match next {
            Some(nx) => i_last_rowid == a_merger[nx].iter.i_rowid,
            None => false,
        };
        if merge_here {
            /* Mescla os dados de duas ou mais poslists */
            let mut i_prev: i64 = 0;
            let mut n_tmp: i32 = FTS5_DATA_ZERO_PADDING as i32;
            let mut p_save = p_head;
            p_head = None;
            while let Some(s) = p_save {
                if a_merger[s].iter.i_rowid != i_last_rowid {
                    break;
                }
                let nx = a_merger[s].p_next;
                a_merger[s].i_off = 0;
                a_merger[s].i_pos = 0;
                a_merger[s].a_pos = a_merger[s].iter.a_poslist.unwrap_or(0) + a_merger[s].iter.n_size as usize;
                fts5_prefix_merger_next_position(&mut a_merger[s]);
                n_tmp += a_merger[s].iter.n_poslist + 10;
                fts5_prefix_merger_insert_by_position(&mut a_merger, &mut p_head, s);
                p_save = nx;
            }

            let first = match p_head {
                Some(f) if a_merger[f].p_next.is_some() => f,
                _ => {
                    p.rc = FTS5_CORRUPT;
                    break;
                }
            };

            tmp.zero();

            let mut p_this = first;
            p_head = a_merger[p_this].p_next;
            fts5_poslist_safe_append(&mut tmp, &mut i_prev, a_merger[p_this].i_pos);
            fts5_prefix_merger_next_position(&mut a_merger[p_this]);
            fts5_prefix_merger_insert_by_position(&mut a_merger, &mut p_head, p_this);

            while let Some(hd) = p_head {
                if a_merger[hd].p_next.is_none() {
                    break;
                }
                p_this = hd;
                if a_merger[p_this].i_pos != i_prev {
                    fts5_poslist_safe_append(&mut tmp, &mut i_prev, a_merger[p_this].i_pos);
                }
                fts5_prefix_merger_next_position(&mut a_merger[p_this]);
                p_head = a_merger[p_this].p_next;
                fts5_prefix_merger_insert_by_position(&mut a_merger, &mut p_head, p_this);
            }

            let last = match p_head {
                Some(l) => l,
                None => {
                    p.rc = FTS5_CORRUPT;
                    break;
                }
            };
            if a_merger[last].i_pos != i_prev {
                fts5_poslist_safe_append(&mut tmp, &mut i_prev, a_merger[last].i_pos);
            }
            let n_tail = a_merger[last].iter.n_poslist - a_merger[last].i_off;

            /* Escreve o tamanho da poslist */
            if tmp.n() + n_tail > n_tmp - FTS5_DATA_ZERO_PADDING as i32 {
                if p.rc == SQLITE_OK {
                    p.rc = FTS5_CORRUPT;
                }
                break;
            }
            out.append_varint(((tmp.n() + n_tail) * 2) as i64);
            out.append_blob(&tmp.p);
            if n_tail > 0 {
                let m = &a_merger[last];
                out.append_blob(sub(&m.iter.data, m.a_pos as i32 + m.i_off, n_tail));
            }

            p_head = p_save;
            for i in 0..n_buf + 1 {
                if a_merger[i].iter.a_poslist.is_some() && a_merger[i].iter.i_rowid == i_last_rowid {
                    fts5_doclist_iter_next(&mut a_merger[i].iter);
                    fts5_prefix_merger_insert_by_rowid(&mut a_merger, &mut p_head, i);
                }
            }
        } else {
            /* Copia a poslist de `p_head` para a saída */
            let p_this = h;
            {
                let it = &a_merger[p_this].iter;
                if let Some(a) = it.a_poslist {
                    out.append_blob(sub(&it.data, a as i32, it.n_poslist + it.n_size));
                }
            }
            fts5_doclist_iter_next(&mut a_merger[p_this].iter);
            p_head = a_merger[p_this].p_next;
            fts5_prefix_merger_insert_by_rowid(&mut a_merger, &mut p_head, p_this);
        }
    }

    *p1 = out;
}

/// `fts5AppendRowid` (`detail=none`).
fn fts5_append_rowid(p: &mut Fts5Index, i_delta: u64, _it: &Fts5Iter, buf: &mut Fts5Buffer) {
    let _ = p;
    buf.append_varint(i_delta as i64);
}

/// `fts5AppendPoslist`.
fn fts5_append_poslist(p: &mut Fts5Index, i_delta: u64, multi: &Fts5Iter, buf: &mut Fts5Buffer) {
    let n_data = multi.base.n_data;
    if p.rc == SQLITE_OK {
        buf.append_varint(i_delta as i64);
        buf.append_varint((n_data * 2) as i64);
        buf.append_blob(sub(&multi.base.p_data, 0, n_data));
    }
}

/// `fts5IndexCharlen`: caracteres UTF-8 em `p_in`.
fn fts5_index_charlen(p_in: &[u8]) -> i32 {
    let n = p_in.len();
    let mut n_char = 0;
    let mut i = 0;
    while i < n {
        let c = p_in[i];
        i += 1;
        if c >= 0xc0 {
            while i < n && (p_in[i] & 0xc0) == 0x80 {
                i += 1;
            }
        }
        n_char += 1;
    }
    n_char
}

/// `fts5SetupPrefixIter`: iterador para uma consulta de prefixo (sem índice de prefixo, ou com o de
/// prefixos um caractere mais longos em `i_idx`). `p_token[0]` recebe o byte do índice.
fn fts5_setup_prefix_iter(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &mut Fts5Config,
    b_desc: bool,
    i_idx: i32,
    p_token: &mut Vec<u8>,
    p_colset: Option<&Fts5Colset>,
) -> Option<Fts5Iter> {
    let n_token = p_token.len();
    let b_none = cfg.e_detail == FTS5_DETAIL_NONE;
    let (n_buf, n_merge): (usize, usize) = if b_none {
        (32, 1)
    } else {
        let n_merge = FTS5_MERGE_NLIST - 1;
        (n_merge * 8, n_merge) /* Sufficient to merge (16^8)==(2^32) lists */
    };

    let mut a_buf: Vec<Fts5Buffer> = (0..n_buf).map(|_| Fts5Buffer::new()).collect();
    let p_struct = fts5_structure_read(p, db, cfg);
    let mut p_ret: Option<Fts5Iter> = None;

    if p.rc == SQLITE_OK {
        if let Some(st) = p_struct.as_ref() {
            let flags = FTS5INDEX_QUERY_SCAN | FTS5INDEX_QUERY_SKIPEMPTY | FTS5INDEX_QUERY_NOOUTPUT;
            let mut i_last_rowid: i64 = 0;
            let mut doclist = Fts5Buffer::new();
            let mut b_new_term: i32 = 1;

            /* Se `i_idx` não é zero, é o número de um índice de prefixo para prefixos um
            ** caractere mais longos que o consultado. Esse índice tem todas as doclists
            ** necessárias, menos a do próprio prefixo, que é extraída aqui do índice principal. */
            if i_idx != 0 {
                let mut dummy: i32 = 0;
                let f2 = FTS5INDEX_QUERY_SKIPEMPTY | FTS5INDEX_QUERY_NOOUTPUT;
                p_token[0] = FTS5_MAIN_PREFIX;
                let mut p1 =
                    fts5_multi_iter_new(p, db, cfg, st, f2, p_colset, Some(&p_token[..]), -1, 0);
                if let Some(it) = p1.as_mut() {
                    fts5_iter_set_output_cb(p, cfg, it);
                }
                loop {
                    if fts5_multi_iter_eof(p, &p1) {
                        break;
                    }
                    let it = match p1.as_mut() {
                        Some(i) => i,
                        None => break,
                    };
                    let first = it.a_first[1].i_first as usize;
                    fts5_iter_set_outputs(p, db, cfg, it, first);
                    if it.base.n_data != 0 {
                        let delta = (it.base.i_rowid as u64).wrapping_sub(i_last_rowid as u64);
                        if b_none {
                            fts5_append_rowid(p, delta, it, &mut doclist);
                        } else {
                            fts5_append_poslist(p, delta, it, &mut doclist);
                        }
                        i_last_rowid = it.base.i_rowid;
                    }
                    fts5_multi_iter_next2(p, db, cfg, it, &mut dummy);
                }
            }

            p_token[0] = FTS5_MAIN_PREFIX + i_idx as u8;
            let mut p1 = fts5_multi_iter_new(p, db, cfg, st, flags, p_colset, Some(&p_token[..]), -1, 0);
            if let Some(it) = p1.as_mut() {
                fts5_iter_set_output_cb(p, cfg, it);
            }

            'outer: loop {
                if fts5_multi_iter_eof(p, &p1) {
                    break;
                }
                'body: {
                    let it = match p1.as_mut() {
                        Some(i) => i,
                        None => break 'outer,
                    };
                    let first = it.a_first[1].i_first as usize;
                    let p_term: Vec<u8> = it.a_seg[first].term.p.clone();
                    let n_term = p_term.len();
                    fts5_iter_set_outputs(p, db, cfg, it, first);

                    if b_new_term != 0 && (n_term < n_token || p_token[..n_token] != p_term[..n_token]) {
                        break 'outer;
                    }

                    if it.base.n_data == 0 {
                        break 'body;
                    }
                    if it.base.i_rowid <= i_last_rowid && doclist.n() > 0 {
                        let mut i = 0usize;
                        while p.rc == SQLITE_OK && doclist.n() != 0 {
                            let i1 = i * n_merge;
                            if i1 + n_merge > n_buf {
                                break;
                            }
                            let mut i_store = i1;
                            while i_store < i1 + n_merge {
                                if a_buf[i_store].n() == 0 {
                                    std::mem::swap(&mut doclist, &mut a_buf[i_store]);
                                    doclist.zero();
                                    break;
                                }
                                i_store += 1;
                            }
                            if i_store == i1 + n_merge {
                                if b_none {
                                    fts5_merge_rowid_lists(p, &mut doclist, &mut a_buf[i1..i1 + n_merge]);
                                } else {
                                    fts5_merge_prefix_lists(p, &mut doclist, &mut a_buf[i1..i1 + n_merge]);
                                }
                                for s in i1..i1 + n_merge {
                                    a_buf[s].zero();
                                }
                            }
                            i += 1;
                        }
                        i_last_rowid = 0;
                    }

                    let delta = (it.base.i_rowid as u64).wrapping_sub(i_last_rowid as u64);
                    if b_none {
                        fts5_append_rowid(p, delta, it, &mut doclist);
                    } else {
                        fts5_append_poslist(p, delta, it, &mut doclist);
                    }
                    i_last_rowid = it.base.i_rowid;
                }
                match p1.as_mut() {
                    Some(it) => fts5_multi_iter_next2(p, db, cfg, it, &mut b_new_term),
                    None => break,
                }
            }

            let mut i = 0;
            while i < n_buf {
                if p.rc == SQLITE_OK {
                    if b_none {
                        fts5_merge_rowid_lists(p, &mut doclist, &mut a_buf[i..i + n_merge]);
                    } else {
                        fts5_merge_prefix_lists(p, &mut doclist, &mut a_buf[i..i + n_merge]);
                    }
                }
                for s in i..i + n_merge {
                    a_buf[s].free();
                }
                i += n_merge;
            }

            if p.rc == SQLITE_OK {
                let data = Fts5Data::doclist(std::mem::take(&mut doclist.p));
                p_ret = fts5_multi_iter_new2(p, cfg, data, b_desc);
            }
        }
    }

    p_ret
}

// ---------------------------------------------------------------------------------------------
// Escrita
// ---------------------------------------------------------------------------------------------

/// `sqlite3Fts5IndexEntryCksum`: soma de verificação simples de uma entrada do índice. `i_idx` é o
/// índice de prefixo (negativo para nenhum).
pub fn fts5_index_entry_cksum(i_rowid: i64, i_col: i32, i_pos: i32, i_idx: i32, term: &[u8]) -> u64 {
    let mut ret: u64 = i_rowid as u64;
    ret = ret.wrapping_add((ret << 3).wrapping_add(i_col as i64 as u64));
    ret = ret.wrapping_add((ret << 3).wrapping_add(i_pos as i64 as u64));
    if i_idx >= 0 {
        ret = ret.wrapping_add((ret << 3).wrapping_add((FTS5_MAIN_PREFIX as i32 + i_idx) as i64 as u64));
    }
    for &b in term.iter() {
        /* O `char` do C tem sinal. */
        ret = ret.wrapping_add((ret << 3).wrapping_add(b as i8 as i64 as u64));
    }
    ret
}

/// `sqlite3Fts5IndexCharlenToBytelen`: bytes dos `n_char` primeiros caracteres UTF-8 de `p`, ou 0
/// se há menos de `n_char` caracteres.
pub fn fts5_index_charlen_to_bytelen(p: &[u8], n_byte: i32, n_char: i32) -> i32 {
    let mut n: i32 = 0;
    for i in 0..n_char {
        if n >= n_byte {
            return 0; /* Input contains fewer than nChar chars */
        }
        let c = at(p, ux(n));
        n += 1;
        if c >= 0xc0 {
            if n >= n_byte {
                return 0;
            }
            while (at(p, ux(n)) & 0xc0) == 0x80 {
                n += 1;
                if n >= n_byte {
                    if i + 1 == n_char {
                        break;
                    }
                    return 0;
                }
            }
        }
    }
    n
}

impl Fts5Index {
    /// `sqlite3Fts5IndexBeginWrite`: as chamadas seguintes a `write` são do documento `i_rowid`.
    pub fn begin_write(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        b_delete: bool,
        i_rowid: i64,
    ) -> i32 {
        /* Cria o hash se ele ainda não existe */
        if self.p_hash.is_none() {
            self.p_hash = Some(Fts5Hash::new(cfg));
        }

        /* Grava o hash no banco se preciso */
        if i_rowid < self.i_write_rowid
            || (i_rowid == self.i_write_rowid && self.b_delete == 0)
            || (self.n_pending_data() > cfg.n_hash_size)
        {
            fts5_index_flush(self, db, cfg);
        }

        self.i_write_rowid = i_rowid;
        self.b_delete = b_delete as i32;
        if !b_delete {
            self.n_pending_row += 1;
        }
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexWrite`: acrescenta ao (ou remove do) índice o token `token` na coluna
    /// `i_col` (negativa num delete), posição `i_pos`. Devolve o código de erro.
    pub fn write(&mut self, cfg: &Fts5Config, i_col: i32, i_pos: i32, token: &[u8]) -> i32 {
        let i_rowid = self.i_write_rowid;
        let n_prefix = cfg.n_prefix();
        let h = match self.p_hash.as_mut() {
            Some(h) => h,
            None => return SQLITE_ERROR,
        };

        /* Acrescenta a entrada ao índice principal de termos. */
        let mut rc = h.write(i_rowid, i_col, i_pos, FTS5_MAIN_PREFIX, token);

        let mut i = 0;
        while i < n_prefix && rc == SQLITE_OK {
            let n_char = cfg.a_prefix[i as usize];
            let n_byte = fts5_index_charlen_to_bytelen(token, token.len() as i32, n_char);
            if n_byte != 0 {
                rc = h.write(
                    i_rowid,
                    i_col,
                    i_pos,
                    (FTS5_MAIN_PREFIX as i32 + i + 1) as u8,
                    sub(token, 0, n_byte),
                );
            }
            i += 1;
        }

        rc
    }

    /// `sqlite3Fts5IndexSync`: grava os dados pendentes.
    pub fn sync(&mut self, db: &mut Connection, cfg: &mut Fts5Config) -> i32 {
        fts5_index_flush(self, db, cfg);
        self.close_reader(db);
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexRollback`: descarta os dados em memória e o cache de `%_data`.
    pub fn rollback(&mut self, db: &mut Connection) -> i32 {
        self.close_reader(db);
        fts5_index_discard_data(self);
        fts5_structure_invalidate(self);
        SQLITE_OK
    }

    /// `sqlite3Fts5IndexReinit`: `%_data` está vazia; grava as estruturas iniciais.
    pub fn reinit(&mut self, db: &mut Connection, cfg: &Fts5Config) -> i32 {
        fts5_structure_invalidate(self);
        fts5_index_discard_data(self);
        let mut s = Fts5Structure::default();
        if cfg.b_contentless_delete != 0 {
            s.n_origin_cntr = 1;
        }
        fts5_data_write(self, db, cfg, FTS5_AVERAGES_ROWID, &[]);
        fts5_structure_write(self, db, cfg, &s);
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexOpen`: abre o índice; com `b_create` cria as tabelas `%_data` e `%_idx`.
    pub fn open(
        db: &mut Connection,
        cfg: &mut Fts5Config,
        b_create: bool,
        pz_err: &mut Option<Vec<u8>>,
    ) -> Result<Fts5Index, i32> {
        let mut rc = SQLITE_OK;
        let mut p = Fts5Index::default();
        p.n_work_unit = FTS5_WORK_UNIT;
        let z = fts5_mprintf(&mut rc, b"%s_data", &[PrintfArg::Text(Some(cfg.z_name.clone()))]);
        p.z_data_tbl = z.unwrap_or_default();
        if rc == SQLITE_OK && b_create {
            rc = super::storage::fts5_create_table(
                db,
                cfg,
                b"data",
                b"id INTEGER PRIMARY KEY, block BLOB",
                false,
                pz_err,
            );
            if rc == SQLITE_OK {
                rc = super::storage::fts5_create_table(
                    db,
                    cfg,
                    b"idx",
                    b"segid, term, pgno, PRIMARY KEY(segid, term)",
                    true,
                    pz_err,
                );
            }
            if rc == SQLITE_OK {
                rc = p.reinit(db, cfg);
            }
        }

        if rc != SQLITE_OK {
            p.close(db);
            return Err(rc);
        }
        Ok(p)
    }

    /// `sqlite3Fts5IndexClose`: finaliza os comandos preparados. O `Drop` sozinho não pode fazê-lo
    /// (precisa da conexão), então quem tem a conexão chama `close` antes de largar o índice.
    pub fn close(&mut self, db: &mut Connection) -> i32 {
        fts5_structure_invalidate(self);
        self.close_reader(db);
        let slots = [
            self.p_writer.take(),
            self.p_deleter.take(),
            self.p_idx_writer.take(),
            self.p_idx_deleter.take(),
            self.p_idx_select.take(),
            self.p_idx_next_select.take(),
            self.p_data_version.take(),
            self.p_delete_from_idx.take(),
        ];
        for id in slots.into_iter().flatten() {
            finalize(db, id);
        }
        self.p_hash = None;
        SQLITE_OK
    }

    /// `sqlite3Fts5IndexReset`: invalida a estrutura em cache se o banco mudou por fora.
    pub fn reset(&mut self, db: &mut Connection, cfg: &Fts5Config) -> i32 {
        if fts5_index_data_version(self, db, cfg) != self.i_struct_version {
            fts5_structure_invalidate(self);
        }
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexGetAverages`: lê o registro de médias. `an_size` tem `nCol` elementos.
    pub fn get_averages(
        &mut self,
        db: &mut Connection,
        cfg: &Fts5Config,
        pn_row: &mut i64,
        an_size: &mut [i64],
    ) -> i32 {
        let n_col = cfg.n_col();

        *pn_row = 0;
        for x in an_size.iter_mut() {
            *x = 0;
        }
        let p_data = fts5_data_read(self, db, cfg, FTS5_AVERAGES_ROWID);
        if self.rc == SQLITE_OK {
            if let Some(d) = p_data.as_ref() {
                if d.nn != 0 {
                    let mut i = 0;
                    let (nb, v) = gv64(&d.p, i);
                    i += nb;
                    *pn_row = v as i64;
                    let mut i_col = 0;
                    while i < d.nn && i_col < n_col {
                        let (nb, v) = gv64(&d.p, i);
                        i += nb;
                        if (i_col as usize) < an_size.len() {
                            an_size[i_col as usize] = v as i64;
                        }
                        i_col += 1;
                    }
                }
            }
        }

        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexSetAverages`: substitui o registro de médias.
    pub fn set_averages(&mut self, db: &mut Connection, cfg: &Fts5Config, data: &[u8]) -> i32 {
        fts5_data_write(self, db, cfg, FTS5_AVERAGES_ROWID, data);
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexReads`: blocos lidos de `%_data` desde a criação do índice.
    pub fn reads(&self) -> i32 {
        self.n_read
    }

    /// `sqlite3Fts5IndexSetCookie`: grava o cookie de 32 bits que abre o registro de estrutura.
    pub fn set_cookie(&mut self, db: &mut Connection, cfg: &Fts5Config, i_new: i32) -> i32 {
        use crate::consts::{SQLITE_BLOB, SQLITE_DONE, SQLITE_TEXT};
        use crate::vdbeapi::column_type;

        let z_sql = crate::printf::mprintf(
            b"SELECT block FROM '%q'.'%q' WHERE id=?1",
            &[
                PrintfArg::Text(Some(cfg.z_db.clone())),
                PrintfArg::Text(Some(self.z_data_tbl.clone())),
            ],
        );
        let mut rc = SQLITE_OK;
        let mut data: Vec<u8> = Vec::new();
        match z_sql {
            None => rc = crate::consts::SQLITE_NOMEM,
            Some(z) => {
                let (rc2, stmt, _tail) = crate::prepare::prepare_v2(db, &z, -1);
                rc = rc2;
                if rc == SQLITE_OK {
                    if let Some(id) = stmt {
                        crate::vdbeapi::bind_int64(db, id, 1, FTS5_STRUCTURE_ROWID);
                        let r = step(db, id);
                        if r == SQLITE_ROW {
                            let t = column_type(db, id, 0);
                            if t == SQLITE_BLOB || t == SQLITE_TEXT {
                                data = column_blob(db, id, 0).map(|s| s.to_vec()).unwrap_or_default();
                            } else {
                                rc = SQLITE_ERROR;
                            }
                        } else if r == SQLITE_DONE {
                            rc = SQLITE_ERROR;
                        } else {
                            rc = r;
                        }
                        let rc3 = finalize(db, id);
                        if rc == SQLITE_OK {
                            rc = rc3;
                        }
                    }
                }
            }
        }

        if rc == SQLITE_OK {
            let a_cookie = (i_new as u32).to_be_bytes();
            if data.len() >= 4 {
                data[..4].copy_from_slice(&a_cookie);
                fts5_data_write(self, db, cfg, FTS5_STRUCTURE_ROWID, &data);
                rc = fts5_index_return(self);
            } else {
                rc = SQLITE_ERROR;
            }
        }
        rc
    }

    /// `sqlite3Fts5IndexLoadConfig`.
    pub fn load_config(&mut self, db: &mut Connection, cfg: &mut Fts5Config) -> i32 {
        let _ = fts5_structure_read(self, db, cfg);
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexGetOrigin`: a origem do segmento que está acumulando no hash.
    pub fn get_origin(&mut self, db: &mut Connection, cfg: &mut Fts5Config, pi_origin: &mut i64) -> i32 {
        if let Some(s) = fts5_structure_read(self, db, cfg) {
            *pi_origin = s.n_origin_cntr as i64;
        }
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexIntegrityCheck`: confere a consistência interna e que o XOR das somas de
    /// verificação das entradas é `cksum` (se `b_use_cksum`).
    pub fn integrity_check(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        cksum: u64,
        b_use_cksum: bool,
    ) -> i32 {
        let e_detail = cfg.e_detail;
        let mut cksum2: u64 = 0; /* Checksum based on contents of indexes */
        let mut poslist = Fts5Buffer::new(); /* Buffer used to hold a poslist */
        let flags = FTS5INDEX_QUERY_NOOUTPUT;

        /* Carrega a estrutura do índice FTS */
        let p_struct = match fts5_structure_read(self, db, cfg) {
            Some(s) => s,
            None => return fts5_index_return(self),
        };

        /* Confere que os nós internos de cada segmento batem com as folhas */
        for lvl in p_struct.a_level.iter() {
            for i_seg in 0..lvl.n_seg.max(0) as usize {
                fts5_index_integrity_check_segment(self, db, cfg, &lvl.a_seg[i_seg]);
            }
        }

        /* A soma de verificação do conteúdo real do índice FTS (varredura linear de cada índice,
        ** um por vez) deve ser igual à que o chamador passou. */
        let mut p_iter = fts5_multi_iter_new(self, db, cfg, &p_struct, flags, None, None, -1, 0);
        loop {
            if fts5_multi_iter_eof(self, &p_iter) {
                break;
            }
            let it = match p_iter.as_mut() {
                Some(i) => i,
                None => break,
            };
            let i_rowid = fts5_multi_iter_rowid(it);
            let z: Vec<u8> = fts5_multi_iter_term(it).to_vec();

            if self.rc != SQLITE_OK {
                break;
            }

            if e_detail == FTS5_DETAIL_NONE {
                if !fts5_multi_iter_is_empty(self, it) {
                    cksum2 ^= fts5_index_entry_cksum(i_rowid, 0, 0, -1, &z);
                }
            } else {
                poslist.zero();
                let first = it.a_first[1].i_first as usize;
                fts5_segiter_poslist(self, db, cfg, &mut it.a_seg[first], None, &mut poslist);
                poslist.append_blob(&[0, 0, 0, 0]);
                let mut i_off: i32 = 0;
                let mut i_pos: i64 = 0;
                while 0 == fts5_poslist_next64(&poslist.p, &mut i_off, &mut i_pos) {
                    let i_col = fts5_pos2column(i_pos);
                    let i_tok_off = fts5_pos2offset(i_pos);
                    cksum2 ^= fts5_index_entry_cksum(i_rowid, i_col, i_tok_off, -1, &z);
                }
            }

            fts5_multi_iter_next(self, db, cfg, it, false, 0);
        }

        if self.rc == SQLITE_OK && b_use_cksum && cksum != cksum2 {
            self.rc = FTS5_CORRUPT;
        }

        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexContentlessDelete`: acrescenta `i_rowid` à lista de tombstones dos
    /// segmentos que contêm linhas da origem `i_origin`.
    pub fn contentless_delete(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        i_origin: i64,
        i_rowid: i64,
    ) -> i32 {
        if fts5_structure_read(self, db, cfg).is_some() {
            let mut b_found = false; /* True after pSeg->nEntryTombstone incr. */
            let n_level = self.p_struct.as_ref().map_or(0, |s| s.a_level.len());
            let mut i_lvl = n_level as i32 - 1;
            while i_lvl >= 0 {
                let n_seg = self
                    .p_struct
                    .as_ref()
                    .map_or(0, |s| s.a_level[i_lvl as usize].n_seg);
                let mut i_seg = n_seg - 1;
                while i_seg >= 0 {
                    let seg = match self.p_struct.as_ref() {
                        Some(s) => s.a_level[i_lvl as usize].a_seg[i_seg as usize].clone(),
                        None => return fts5_index_return(self),
                    };
                    if seg.i_origin1 <= i_origin as u64 && seg.i_origin2 >= i_origin as u64 {
                        if !b_found {
                            if let Some(s) = self.p_struct.as_mut() {
                                s.a_level[i_lvl as usize].a_seg[i_seg as usize].n_entry_tombstone += 1;
                            }
                            b_found = true;
                        }
                        fts5_index_tombstone_add(
                            self,
                            db,
                            cfg,
                            i_lvl as usize,
                            i_seg as usize,
                            i_rowid as u64,
                        );
                    }
                    i_seg -= 1;
                }
                i_lvl -= 1;
            }
        }
        fts5_index_return(self)
    }

    /// `sqlite3Fts5IndexQuery`: abre um iterador sobre os rowids que casam com `token` (ou o
    /// prefixo `token`). Devolve o iterador ou o código de erro.
    pub fn query(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        token: &[u8],
        flags: i32,
        colset: Option<&Fts5Colset>,
    ) -> Result<Fts5Iter, i32> {
        let n_token = token.len();
        let mut buf: Vec<u8> = Vec::with_capacity(n_token + 1);
        buf.push(0);
        buf.extend_from_slice(token);

        let mut i_idx: i32 = 0; /* Index to search */
        let mut i_prefix_idx: i32 = 0; /* +1 prefix index */
        let mut b_tokendata = cfg.b_tokendata != 0;

        if flags & (FTS5INDEX_QUERY_NOTOKENDATA | FTS5INDEX_QUERY_SCAN) != 0 {
            b_tokendata = false;
        }

        /* Descobre qual índice pesquisar e define `i_idx`. Numa consulta de prefixo sem índice de
        ** prefixo, `i_idx` fica maior que `nPrefix` para indicar que a consulta se resolve
        ** varrendo vários termos do índice principal. */
        if flags & FTS5INDEX_QUERY_PREFIX != 0 {
            let n_char = fts5_index_charlen(token);
            i_idx = 1;
            while i_idx <= cfg.n_prefix() {
                let n_idx_char = cfg.a_prefix[(i_idx - 1) as usize];
                if n_idx_char == n_char {
                    break;
                }
                if n_idx_char == n_char + 1 {
                    i_prefix_idx = i_idx;
                }
                i_idx += 1;
            }
        }

        let mut p_ret: Option<Fts5Iter> = None;
        if b_tokendata && i_idx == 0 {
            buf[0] = b'0';
            p_ret = fts5_setup_tokendata_iter(self, db, cfg, &buf, colset);
        } else if i_idx <= cfg.n_prefix() {
            /* Consulta direta ao índice */
            let p_struct = fts5_structure_read(self, db, cfg);
            buf[0] = FTS5_MAIN_PREFIX + i_idx as u8;
            if let Some(st) = p_struct.as_ref() {
                p_ret = fts5_multi_iter_new(
                    self,
                    db,
                    cfg,
                    st,
                    flags | FTS5INDEX_QUERY_SKIPEMPTY,
                    colset,
                    Some(&buf[..]),
                    -1,
                    0,
                );
            }
        } else {
            /* Varre vários termos do índice principal */
            let b_desc = (flags & FTS5INDEX_QUERY_DESC) != 0;
            p_ret = fts5_setup_prefix_iter(self, db, cfg, b_desc, i_prefix_idx, &mut buf, colset);
            if let Some(it) = p_ret.as_mut() {
                fts5_iter_set_output_cb(self, cfg, it);
                if self.rc == SQLITE_OK {
                    let first = it.a_first[1].i_first as usize;
                    if it.a_seg[first].p_leaf.is_some() {
                        fts5_iter_set_outputs(self, db, cfg, it, first);
                    }
                }
            }
        }

        if self.rc != SQLITE_OK {
            p_ret = None;
            self.close_reader(db);
        }

        let rc = fts5_index_return(self);
        match p_ret {
            Some(it) if rc == SQLITE_OK => Ok(it),
            _ => Err(if rc != SQLITE_OK { rc } else { SQLITE_ERROR }),
        }
    }
}

/// `sqlite3Fts5IndexInit`: no Debian (sem `SQLITE_TEST` nem `SQLITE_FTS5_DEBUG`) não registra
/// nenhuma função.
pub fn fts5_index_init(_db: &mut Connection) -> i32 {
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Tokendata
// ---------------------------------------------------------------------------------------------

/// `fts5IsTokendataPrefix`: verdadeiro se o termo `buf` casa com o token da consulta.
fn fts5_is_tokendata_prefix(buf: &Fts5Buffer, token: &[u8]) -> bool {
    let n_token = token.len();
    buf.p.len() >= n_token
        && buf.p[..n_token] == *token
        && (buf.p.len() == n_token || buf.p[n_token] == 0x00)
}

/// `fts5TokendataIterAppendMap`.
fn fts5_tokendata_iter_append_map(
    p: &Fts5Index,
    pt: &mut Fts5TokenDataIter,
    i_iter: i32,
    i_rowid: i64,
    i_pos: i64,
) {
    if p.rc == SQLITE_OK {
        pt.a_map.push(Fts5TokenDataMap { i_rowid, i_pos, i_iter });
    }
}

/// `fts5IterSetOutputsTokendata`: ajusta as saídas do iterador tokendata conforme a linha corrente.
fn fts5_iter_set_outputs_tokendata(p: &mut Fts5Index, cfg: &Fts5Config, it: &mut Fts5Iter) {
    let mut pt = match it.p_token_data_iter.take() {
        Some(t) => t,
        None => return,
    };
    let mut n_hit = 0;
    let mut i_rowid = i64::MIN;
    let mut i_min = 0usize;

    it.base.n_data = 0;
    it.base.p_data.clear();

    for ii in 0..pt.ap_iter.len() {
        let q = &pt.ap_iter[ii];
        if q.base.b_eof == 0 {
            if n_hit == 0 || q.base.i_rowid < i_rowid {
                i_rowid = q.base.i_rowid;
                n_hit = 1;
                it.base.p_data = q.base.p_data.clone();
                it.base.n_data = q.base.n_data;
                i_min = ii;
            } else if q.base.i_rowid == i_rowid {
                n_hit += 1;
            }
        }
    }

    if n_hit == 0 {
        it.base.b_eof = 1;
    } else {
        let e_detail = cfg.e_detail;
        it.base.b_eof = 0;
        it.base.i_rowid = i_rowid;

        if n_hit == 1 && e_detail == FTS5_DETAIL_FULL {
            fts5_tokendata_iter_append_map(p, &mut pt, i_min as i32, i_rowid, -1);
        } else if n_hit > 1 && e_detail != FTS5_DETAIL_NONE {
            /* Prepara um leitor para cada poslist que será mesclada */
            let mut to_iter: Vec<i32> = Vec::new();
            let mut datas: Vec<Vec<u8>> = Vec::new();
            for ii in 0..pt.ap_iter.len() {
                let q = &pt.ap_iter[ii];
                if i_rowid == q.base.i_rowid {
                    to_iter.push(ii as i32);
                    datas.push(sub(&q.base.p_data, 0, q.base.n_data).to_vec());
                }
            }
            let mut readers: Vec<Fts5PoslistReader> =
                datas.iter().map(|d| Fts5PoslistReader::init(d)).collect();

            it.poslist.zero();
            let mut i_prev: i64 = 0;

            loop {
                let mut i_min_pos = i64::MAX;

                /* Acha a menor posição */
                let mut i_min_r = 0usize;
                for (ii, r) in readers.iter().enumerate() {
                    if r.b_eof == 0 && r.i_pos < i_min_pos {
                        i_min_pos = r.i_pos;
                        i_min_r = ii;
                    }
                }

                /* Se todos os leitores estão no fim, sai do laço. */
                if i_min_pos == i64::MAX {
                    break;
                }

                fts5_poslist_safe_append(&mut it.poslist, &mut i_prev, i_min_pos);
                readers[i_min_r].next();

                if e_detail == FTS5_DETAIL_FULL {
                    pt.a_map.push(Fts5TokenDataMap {
                        i_pos: i_min_pos,
                        i_iter: to_iter[i_min_r],
                        i_rowid,
                    });
                }
            }

            it.base.p_data = it.poslist.p.clone();
            it.base.n_data = it.poslist.n();
        }
    }

    it.p_token_data_iter = Some(pt);
}

/// `fts5TokendataIterNext`: avança o iterador tokendata (ou, com `b_from`, até `i_from`).
fn fts5_tokendata_iter_next(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5Iter,
    b_from: bool,
    i_from: i64,
) {
    let mut pt = match it.p_token_data_iter.take() {
        Some(t) => t,
        None => return,
    };
    for q in pt.ap_iter.iter_mut() {
        if q.base.b_eof == 0
            && (q.base.i_rowid == it.base.i_rowid || (b_from && q.base.i_rowid < i_from))
        {
            fts5_multi_iter_next(p, db, cfg, q, b_from, i_from);
            while b_from && q.base.b_eof == 0 && q.base.i_rowid < i_from && p.rc == SQLITE_OK {
                fts5_multi_iter_next(p, db, cfg, q, false, 0);
            }
        }
    }
    it.p_token_data_iter = Some(pt);

    if p.rc == SQLITE_OK {
        fts5_iter_set_outputs_tokendata(p, cfg, it);
    }
}

/// `fts5SetupTokendataIter`: iterador para uma consulta de não prefixo numa tabela `tokendata=1`.
/// `p_token` inclui o byte do índice na frente.
fn fts5_setup_tokendata_iter(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &mut Fts5Config,
    p_token: &[u8],
    p_colset: Option<&Fts5Colset>,
) -> Option<Fts5Iter> {
    let mut p_set: Option<Fts5TokenDataIter> = None;
    let flags = FTS5INDEX_QUERY_SCANONETERM | FTS5INDEX_QUERY_SCAN;
    let mut b_seek = Fts5Buffer::new();
    let mut p_small: Option<Vec<u8>> = None;

    fts5_index_flush(p, db, cfg);
    let p_struct = fts5_structure_read(p, db, cfg);
    let cfg: &Fts5Config = cfg;

    while p.rc == SQLITE_OK {
        let st = match p_struct.as_ref() {
            Some(s) => s,
            None => break,
        };
        let mut p_new = match fts5_multi_iter_alloc(p, st.n_segment) {
            Some(n) => n,
            None => break,
        };
        match p_small.as_ref() {
            Some(sm) => {
                b_seek.set(sm);
                b_seek.append_blob(&[0u8]);
            }
            None => b_seek.set(p_token),
        }

        let prev_idx: Option<usize> = p_set
            .as_ref()
            .and_then(|s| s.ap_iter.len().checked_sub(1));
        let mut i_new = 0usize;
        for lvl in st.a_level.iter() {
            let mut i_seg = lvl.n_seg - 1;
            while i_seg >= 0 {
                let seg = &lvl.a_seg[i_seg as usize];
                let mut b_done = false;

                if let (Some(pi), Some(set)) = (prev_idx, p_set.as_mut()) {
                    let prev_iter = &mut set.ap_iter[pi].a_seg[i_new];
                    let same_term = match p_small.as_ref() {
                        Some(sm) => fts5_bytes_compare(sm, &prev_iter.term.p) == 0,
                        None => false,
                    };
                    if !same_term {
                        p_new.a_seg[i_new] = std::mem::take(prev_iter);
                        b_done = true;
                    } else if prev_iter
                        .p_leaf
                        .as_ref()
                        .map_or(false, |l| prev_iter.i_endof_doclist > l.sz_leaf)
                    {
                        let n = b_seek.p.len().saturating_sub(1);
                        fts5_seg_iter_next_init(
                            p,
                            db,
                            cfg,
                            &b_seek.p[..n],
                            seg,
                            &mut p_new.a_seg[i_new],
                        );
                        b_done = true;
                    }
                }

                if !b_done {
                    fts5_seg_iter_seek_init(p, db, cfg, &b_seek.p, flags, seg, &mut p_new.a_seg[i_new]);
                }

                match (prev_idx, p_set.as_ref()) {
                    (Some(pi), Some(set)) => {
                        if let Some(arr) = set.ap_iter[pi].a_seg[i_new].p_tomb_array.clone() {
                            p_new.a_seg[i_new].p_tomb_array = Some(arr);
                        }
                    }
                    _ => fts5_seg_iter_alloc_tombstone(&mut p_new.a_seg[i_new]),
                }

                i_new += 1;
                i_seg -= 1;
                if p.rc != SQLITE_OK {
                    break;
                }
            }
        }

        /* fts5TokendataSetTermIfEof */
        if let (Some(pi), Some(set), Some(sm)) = (prev_idx, p_set.as_mut(), p_small.as_ref()) {
            if set.ap_iter[pi].a_seg[0].p_leaf.is_none() {
                set.ap_iter[pi].a_seg[0].term.set(sm);
            }
        }

        p_new.b_skip_empty = 1;
        p_new.p_colset = p_colset.cloned();
        fts5_iter_set_output_cb(p, cfg, &mut p_new);

        /* Percorre todos os segmentos do iterador novo e acha o menor termo para que algum deles
        ** aponta; o iterador novo serve a esse termo. Os que apontam um termo que não casa com
        ** o token da consulta vão para o fim. */
        p_small = None;
        for ii in 0..p_new.n_seg as usize {
            let p_ii = &mut p_new.a_seg[ii];
            if !fts5_is_tokendata_prefix(&p_ii.term, p_token) {
                fts5_seg_iter_set_eof(p_ii);
            }
            let smaller = match p_small.as_ref() {
                None => true,
                Some(sm) => fts5_bytes_compare(sm, &p_ii.term.p) > 0,
            };
            if p_ii.p_leaf.is_some() && smaller {
                p_small = Some(p_ii.term.p.clone());
            }
        }

        /* Se `p_small` continua vazio, o iterador novo não aponta nenhum termo da consulta: é
        ** descartado e o laço termina, pois todos os iteradores necessários já foram juntados. */
        if p_small.is_none() {
            break;
        }

        /* Acrescenta este iterador ao conjunto e continua. */
        p_set
            .get_or_insert_with(Fts5TokenDataIter::default)
            .ap_iter
            .push(p_new);
    }

    if p.rc == SQLITE_OK {
        if let Some(set) = p_set.as_mut() {
            for it in set.ap_iter.iter_mut() {
                for seg in it.a_seg.iter_mut() {
                    seg.flags |= FTS5_SEGITER_ONETERM;
                }
                fts5_multi_iter_finish_setup(p, db, cfg, it);
            }
        }
    }

    let mut p_ret: Option<Fts5Iter> = None;
    if p.rc == SQLITE_OK {
        p_ret = fts5_multi_iter_alloc(p, 0);
    }
    match p_ret {
        Some(mut r) => {
            r.p_token_data_iter = p_set.map(Box::new);
            if r.p_token_data_iter.is_some() {
                fts5_iter_set_outputs_tokendata(p, cfg, &mut r);
            } else {
                r.base.b_eof = 1;
            }
            Some(r)
        }
        None => None,
    }
}

impl Fts5Iter {
    /// `sqlite3Fts5IterEof`: verdadeiro no fim.
    #[inline]
    pub fn eof(&self) -> bool {
        self.base.b_eof != 0
    }

    /// O `pIter->iRowid` do `Fts5IndexIter`.
    #[inline]
    pub fn rowid(&self) -> i64 {
        self.base.i_rowid
    }

    /// A poslist corrente (`pData`/`nData`).
    #[inline]
    pub fn data(&self) -> &[u8] {
        sub(&self.base.p_data, 0, self.base.n_data)
    }

    /// `sqlite3Fts5IterNext`: move para a próxima linha que casa.
    pub fn next(&mut self, p: &mut Fts5Index, db: &mut Connection, cfg: &Fts5Config) -> i32 {
        if self.p_token_data_iter.is_some() {
            fts5_tokendata_iter_next(p, db, cfg, self, false, 0);
        } else {
            fts5_multi_iter_next(p, db, cfg, self, false, 0);
        }
        fts5_index_return(p)
    }

    /// `sqlite3Fts5IterNextScan`: move para o próximo termo/rowid (usado pelo fts5vocab).
    pub fn next_scan(&mut self, p: &mut Fts5Index, db: &mut Connection, cfg: &Fts5Config) -> i32 {
        fts5_multi_iter_next(p, db, cfg, self, false, 0);
        if p.rc == SQLITE_OK {
            let first = self.a_first[1].i_first as usize;
            let seg = &mut self.a_seg[first];
            if seg.p_leaf.is_some() && at(&seg.term.p, 0) != FTS5_MAIN_PREFIX {
                seg.p_leaf = None;
                self.base.b_eof = 1;
            }
        }
        fts5_index_return(p)
    }

    /// `sqlite3Fts5IterNextFrom`: move para o próximo rowid em `i_match` ou depois (na direção do
    /// iterador).
    pub fn next_from(
        &mut self,
        p: &mut Fts5Index,
        db: &mut Connection,
        cfg: &Fts5Config,
        i_match: i64,
    ) -> i32 {
        if self.p_token_data_iter.is_some() {
            fts5_tokendata_iter_next(p, db, cfg, self, true, i_match);
        } else {
            fts5_multi_iter_next_from(p, db, cfg, self, i_match);
        }
        fts5_index_return(p)
    }

    /// `sqlite3Fts5IterTerm`: o termo corrente (sem o byte do índice).
    pub fn term(&self) -> &[u8] {
        let z = fts5_multi_iter_term(self);
        if z.is_empty() {
            z
        } else {
            &z[1..]
        }
    }

    /// `sqlite3Fts5IterToken`: o token da ocorrência `(i_rowid, i_col, i_off)` (só em iteradores
    /// tokendata). `None` se não há mapeamento.
    pub fn token(&self, i_rowid: i64, i_col: i32, i_off: i32) -> Option<Vec<u8>> {
        let pt = self.p_token_data_iter.as_ref()?;
        let a_map = &pt.a_map;
        let i_pos: i64 = ((i_col as i64) << 32) + i_off as i64;

        let mut i1: i32 = 0;
        let mut i2: i32 = a_map.len() as i32;
        let mut i_test: i32 = 0;

        while i2 > i1 {
            i_test = (i1 + i2) / 2;
            let m = &a_map[i_test as usize];

            if m.i_rowid < i_rowid {
                i1 = i_test + 1;
            } else if m.i_rowid > i_rowid {
                i2 = i_test;
            } else if m.i_pos < i_pos {
                if m.i_pos < 0 {
                    break;
                }
                i1 = i_test + 1;
            } else if m.i_pos > i_pos {
                i2 = i_test;
            } else {
                break;
            }
        }

        if i2 > i1 {
            let p_map = pt.ap_iter.get(a_map[i_test as usize].i_iter as usize)?;
            let t = &p_map.a_seg[0].term.p;
            return Some(if t.is_empty() { Vec::new() } else { t[1..].to_vec() });
        }
        None
    }

    /// `sqlite3Fts5IndexIterClearTokendata`: esvazia o mapa de tokens.
    pub fn clear_tokendata(&mut self) {
        if let Some(pt) = self.p_token_data_iter.as_mut() {
            pt.a_map.clear();
        }
    }

    /// `sqlite3Fts5IndexIterWriteTokendata`: acrescenta um mapeamento (`detail=column` e
    /// `detail=none`, em que quem chama tokeniza a linha).
    pub fn write_tokendata(
        &mut self,
        p: &mut Fts5Index,
        token: &[u8],
        i_rowid: i64,
        i_col: i32,
        i_off: i32,
    ) -> i32 {
        if let Some(pt) = self.p_token_data_iter.as_mut() {
            let mut found: Option<usize> = None;
            for (ii, q) in pt.ap_iter.iter().enumerate() {
                let t = &q.a_seg[0].term.p;
                if t.len() >= 1 && token.len() == t.len() - 1 && token == &t[1..] {
                    found = Some(ii);
                    break;
                }
            }
            if let Some(ii) = found {
                fts5_tokendata_iter_append_map(
                    p,
                    pt,
                    ii as i32,
                    i_rowid,
                    ((i_col as i64) << 32) + i_off as i64,
                );
            }
        }
        fts5_index_return(p)
    }

    /// `sqlite3Fts5IterClose`: libera o iterador e fecha o leitor do índice.
    pub fn close(self, p: &mut Fts5Index, db: &mut Connection) {
        drop(self);
        p.close_reader(db);
    }
}

// ---------------------------------------------------------------------------------------------
// Tombstones do contentless_delete
// ---------------------------------------------------------------------------------------------

/// `fts5IndexTombstoneAddToPage`: acrescenta `i_rowid` à página `pg` da tabela hash (uma de `n_pg`).
/// Se `b_force` é falso e a tabela está cheia (mais da metade dos slots), devolve 1 sem inserir; 2
/// se o rowid não cabe em chaves de 4 bytes.
fn fts5_index_tombstone_add_to_page(pg: &mut Fts5Data, b_force: bool, n_pg: i32, i_rowid: u64) -> i32 {
    let sz_key = tombstone_keysize(pg);
    let n_slot = tombstone_nslot(pg);
    let n_elem = get_u32(&pg.p, 4) as i32;
    let mut i_slot = ((i_rowid / n_pg.max(1) as u64) % n_slot as u64) as i32;
    let mut n_collide = n_slot;

    if sz_key == 4 && i_rowid > 0xFFFF_FFFF {
        return 2;
    }
    if i_rowid == 0 {
        if pg.p.len() > 1 {
            pg.p[1] = 0x01;
        }
        return 0;
    }

    if !b_force && n_elem >= (n_slot / 2) {
        return 1;
    }

    put_u32(&mut pg.p, 4, (n_elem + 1) as u32);
    let slot_set = |pg: &Fts5Data, i: i32| -> bool {
        let off = 8 + (i as usize) * sz_key as usize;
        (0..sz_key as usize).any(|k| at(&pg.p, off + k) != 0)
    };
    while slot_set(pg, i_slot) {
        i_slot = (i_slot + 1) % n_slot;
        let c = n_collide;
        n_collide -= 1;
        if c == 0 {
            return 0;
        }
    }
    let off = 8 + (i_slot as usize) * sz_key as usize;
    if sz_key == 4 {
        put_u32(&mut pg.p, off, i_rowid as u32);
    } else {
        put_u64(&mut pg.p, off, i_rowid);
    }

    0
}

/// `fts5IndexTombstoneRehash`: reconstrói a tabela hash do segmento nas `n_out` páginas `ap_out`
/// com chaves de `sz_key` bytes. Devolve 0 se coube, ou não zero se uma página encheu.
fn fts5_index_tombstone_rehash(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
    p_data1: Option<&Fts5Data>,
    i_pg1: i32,
    sz_key: i32,
    n_out: i32,
    ap_out: &mut Vec<Fts5Data>,
) -> i32 {
    let mut res = 0;

    /* Zera os cabeçalhos de todas as páginas de saída */
    for ii in 0..n_out as usize {
        ap_out[ii].p[0] = sz_key as u8;
        put_u32(&mut ap_out[ii].p, 4, 0);
    }

    /* Percorre as páginas atuais da tabela hash. */
    let mut ii = 0;
    while res == 0 && ii < seg.n_pg_tombstone {
        let owned: Option<Fts5Data>;
        let p_data: Option<&Fts5Data> = if i_pg1 == ii {
            p_data1
        } else {
            owned = fts5_data_read(p, db, cfg, fts5_tombstone_rowid(seg.i_segid, ii));
            owned.as_ref()
        };

        if let Some(pd) = p_data {
            let sz_key_in = tombstone_keysize(pd);
            let n_slot_in = (pd.nn - 8) / sz_key_in;

            for i_in in 0..n_slot_in.max(0) {
                let mut i_val: u64 = 0;
                let off = 8 + (i_in as usize) * sz_key_in as usize;

                /* Lê o valor do slot `i_in` da página de entrada para `i_val`. */
                let nonzero = (0..sz_key_in as usize).any(|k| at(&pd.p, off + k) != 0);
                if nonzero {
                    i_val = if sz_key_in == 4 {
                        get_u32(&pd.p, off) as u64
                    } else {
                        get_u64(&pd.p, off)
                    };
                }

                /* Se `i_val` não é zero, insere-o na tabela hash nova */
                if i_val != 0 {
                    let idx = (i_val % n_out as u64) as usize;
                    res = fts5_index_tombstone_add_to_page(&mut ap_out[idx], false, n_out, i_val);
                    if res != 0 {
                        break;
                    }
                }
            }

            /* Se é a página 0 da tabela antiga, copia dela para a nova a flag do rowid 0. */
            if ii == 0 {
                let b = at(&pd.p, 1);
                if ap_out[0].p.len() > 1 {
                    ap_out[0].p[1] = b;
                }
            }
        }
        ii += 1;
    }

    res
}

/// `fts5IndexTombstoneRebuild`: reconstrói a tabela hash do segmento; devolve o número de páginas
/// novas e as próprias páginas (0 e vazio em erro).
fn fts5_index_tombstone_rebuild(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
    p_data1: Option<&Fts5Data>,
    i_pg1: i32,
    sz_key: i32,
) -> (i32, Vec<Fts5Data>) {
    const MINSLOT: i32 = 32;
    let n_slot_per_page = MINSLOT.max((cfg.pgsz - 8) / sz_key);
    let mut n_slot = 0;
    let mut n_out = 0;

    /* Define quantas páginas de saída (`n_out`) e quantos slots por página (`n_slot`). */
    if seg.n_pg_tombstone == 0 {
        /* Caso 1: a tabela ainda não existe. */
        n_out = 1;
        n_slot = MINSLOT;
    } else if seg.n_pg_tombstone == 1 {
        /* Caso 2: a tabela tem uma página; tenta fazê-la crescer. */
        let n_elem = p_data1.map_or(0, |d| get_u32(&d.p, 4) as i32);
        n_out = 1;
        n_slot = (n_elem * 4).max(MINSLOT);
        if n_slot > n_slot_per_page {
            n_out = 0;
        }
    }
    if n_out == 0 {
        /* Caso 3: mais de uma página, ou uma página que não cresce mais. */
        n_out = seg.n_pg_tombstone * 2 + 1;
        n_slot = n_slot_per_page;
    }

    /* Aloca o vetor e as páginas de saída */
    loop {
        let sz_page = 8 + n_slot * sz_key;
        let mut ap_out: Vec<Fts5Data> = (0..n_out)
            .map(|_| Fts5Data {
                p: vec![0u8; ux(sz_page) + FTS5_DATA_PADDING],
                nn: sz_page,
                sz_leaf: 0,
            })
            .collect();

        /* Reconstrói a tabela hash. */
        let mut res = 0;
        if p.rc == SQLITE_OK {
            res = fts5_index_tombstone_rehash(p, db, cfg, seg, p_data1, i_pg1, sz_key, n_out, &mut ap_out);
        }
        if res == 0 {
            if p.rc != SQLITE_OK {
                return (0, Vec::new());
            }
            return (n_out, ap_out);
        }

        /* Não foi possível reconstruir a tabela: descarta as páginas e tenta de novo com mais
        ** páginas. */
        n_slot = n_slot_per_page;
        n_out = n_out * 2 + 1;
    }
}

/// `fts5IndexTombstoneAdd`: acrescenta um tombstone para `i_rowid` ao segmento
/// `p.p_struct.a_level[i_lvl].a_seg[i_seg]`.
fn fts5_index_tombstone_add(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_lvl: usize,
    i_seg: usize,
    i_rowid: u64,
) {
    let seg = match p.p_struct.as_ref() {
        Some(s) => s.a_level[i_lvl].a_seg[i_seg].clone(),
        None => return,
    };
    let mut p_pg: Option<Fts5Data> = None;
    let mut i_pg: i32 = -1;

    p.n_contentless_delete += 1;

    if seg.n_pg_tombstone > 0 {
        i_pg = (i_rowid % seg.n_pg_tombstone as u64) as i32;
        p_pg = fts5_data_read(p, db, cfg, fts5_tombstone_rowid(seg.i_segid, i_pg));
        let pg = match p_pg.as_mut() {
            Some(pg) => pg,
            None => return,
        };

        if 0 == fts5_index_tombstone_add_to_page(pg, false, seg.n_pg_tombstone, i_rowid) {
            let nn = ux(pg.nn);
            fts5_data_write(
                p,
                db,
                cfg,
                fts5_tombstone_rowid(seg.i_segid, i_pg),
                &pg.p[..nn.min(pg.p.len())],
            );
            return;
        }
    }

    /* É preciso reconstruir a tabela hash. Primeiro descobre o tamanho da chave (4 ou 8). */
    let mut sz_key = match p_pg.as_ref() {
        Some(pg) => tombstone_keysize(pg),
        None => 4,
    };
    if i_rowid > 0xFFFF_FFFF {
        sz_key = 8;
    }

    /* Reconstrói a tabela hash */
    let (n_hash, mut ap_hash) = fts5_index_tombstone_rebuild(p, db, cfg, &seg, p_pg.as_ref(), i_pg, sz_key);

    /* Se tudo deu certo, grava o rowid novo numa das páginas novas e grava todas elas. */
    if n_hash > 0 {
        let idx = (i_rowid % n_hash as u64) as usize;
        fts5_index_tombstone_add_to_page(&mut ap_hash[idx], true, n_hash, i_rowid);
        for ii in 0..n_hash {
            let i_tombstone_rowid = fts5_tombstone_rowid(seg.i_segid, ii);
            let pg = &ap_hash[ii as usize];
            let nn = ux(pg.nn);
            fts5_data_write(p, db, cfg, i_tombstone_rowid, &pg.p[..nn.min(pg.p.len())]);
        }
        if let Some(s) = p.p_struct.as_mut() {
            s.a_level[i_lvl].a_seg[i_seg].n_pg_tombstone = n_hash;
        }
        if let Some(s) = p.p_struct.clone() {
            fts5_structure_write(p, db, cfg, &s);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Verificação de integridade
// ---------------------------------------------------------------------------------------------

/// `fts5IndexIntegrityCheckEmpty`: confere que as folhas de `i_first` a `i_last` existem e não têm
/// termos, e que as de `i_no_rowid` em diante também não têm rowids.
fn fts5_index_integrity_check_empty(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
    i_first: i32,
    i_no_rowid: i32,
    i_last: i32,
) {
    let mut i = i_first;
    while p.rc == SQLITE_OK && i <= i_last {
        if let Some(leaf) = fts5_data_read(p, db, cfg, fts5_segment_rowid(seg.i_segid, i)) {
            if !fts5_leaf_is_termless(&leaf) {
                p.rc = FTS5_CORRUPT;
            }
            if i >= i_no_rowid && 0 != fts5_leaf_first_rowid_off(&leaf) {
                p.rc = FTS5_CORRUPT;
            }
        }
        i += 1;
    }
}

/// `fts5IntegrityCheckPgidx`: os termos do índice de página da folha estão em ordem.
fn fts5_integrity_check_pgidx(p: &mut Fts5Index, leaf: &Fts5Data) {
    let mut i_term_off: i64 = 0;
    let mut buf1 = Fts5Buffer::new();
    let mut buf2 = Fts5Buffer::new();

    let mut ii = leaf.sz_leaf;
    while ii < leaf.nn && p.rc == SQLITE_OK {
        let (nb, n_incr) = gv32(&leaf.p, ii);
        ii += nb;
        i_term_off += n_incr as i64;
        let mut i_off: i64 = i_term_off;

        if i_off >= leaf.sz_leaf as i64 {
            p.rc = FTS5_CORRUPT;
        } else if i_term_off == n_incr as i64 {
            let (nb, n_byte) = gv32(&leaf.p, i_off as i32);
            i_off += nb as i64;
            if (i_off + n_byte as i64) > leaf.sz_leaf as i64 {
                p.rc = FTS5_CORRUPT;
            } else {
                buf1.set(sub(&leaf.p, i_off as i32, n_byte));
            }
        } else {
            let (nb, n_keep) = gv32(&leaf.p, i_off as i32);
            i_off += nb as i64;
            let (nb, n_byte) = gv32(&leaf.p, i_off as i32);
            i_off += nb as i64;
            if n_keep > buf1.n() || (i_off + n_byte as i64) > leaf.sz_leaf as i64 {
                p.rc = FTS5_CORRUPT;
            } else {
                buf1.p.truncate(n_keep as usize);
                buf1.append_blob(sub(&leaf.p, i_off as i32, n_byte));
            }

            if p.rc == SQLITE_OK {
                let res = fts5_buffer_compare(&buf1, &buf2);
                if res <= 0 {
                    p.rc = FTS5_CORRUPT;
                }
            }
        }
        buf2.set(&buf1.p);
    }
}

/// `fts5IndexIntegrityCheckSegment`: confere a b-tree de `%_idx` do segmento contra as folhas.
fn fts5_index_integrity_check_segment(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
) {
    let b_secure_delete = cfg.i_version == FTS5_CURRENT_VERSION_SECUREDELETE;
    let mut i_idx_prev_leaf = seg.pgno_first - 1;
    let mut i_dlidx_prev_leaf = seg.pgno_last;

    if seg.pgno_first == 0 {
        return;
    }

    let z_sql = crate::printf::mprintf(
        b"SELECT segid, term, (pgno>>1), (pgno&1) FROM %Q.'%q_idx' WHERE segid=%d ORDER BY 1, 2",
        &[
            PrintfArg::Text(Some(cfg.z_db.clone())),
            PrintfArg::Text(Some(cfg.z_name.clone())),
            PrintfArg::Int(seg.i_segid as i64),
        ],
    );
    let stmt = fts5_index_prepare_stmt(p, db, z_sql);

    /* Percorre a hierarquia da b-tree.  */
    while p.rc == SQLITE_OK {
        let id = match stmt {
            Some(id) => id,
            None => break,
        };
        if SQLITE_ROW != step(db, id) {
            break;
        }

        let z_idx_term: Vec<u8> = column_blob(db, id, 1).map(|s| s.to_vec()).unwrap_or_default();
        let n_idx_term = z_idx_term.len() as i32;
        let i_idx_leaf = column_int(db, id, 2);
        let b_idx_dlidx = column_int(db, id, 3);

        /* Se a folha já foi aparada do segmento, ignora esta entrada da b-tree. Senão a
        ** carrega em memória. */
        if i_idx_leaf < seg.pgno_first {
            continue;
        }
        let i_row = fts5_segment_rowid(seg.i_segid, i_idx_leaf);
        let leaf = match fts5_leaf_read(p, db, cfg, i_row) {
            Some(l) => l,
            None => break,
        };

        /* Confere que a folha tem pelo menos um termo e que ele é maior ou igual à chave de
        ** divisão `z_idx_term`. Confere também que o ponteiro de rowid do cabeçalho, se houver,
        ** aponta para antes do termo. */
        if leaf.nn <= leaf.sz_leaf {
            if n_idx_term == 0
                && cfg.i_version == FTS5_CURRENT_VERSION_SECUREDELETE
                && leaf.nn == leaf.sz_leaf
                && leaf.nn == 4
            {
                /* Caso especial: a primeira página de um segmento mantém a entrada em `%_idx`
                ** mesmo que as operações de secure-delete tirem todos os termos dela. */
            } else {
                p.rc = FTS5_CORRUPT;
            }
        } else {
            let mut i_off = fts5_leaf_first_term_off(&leaf); /* Offset of first term on leaf */
            let i_rowid_off = fts5_leaf_first_rowid_off(&leaf); /* Offset of first rowid on leaf */
            if i_rowid_off >= i_off || i_off >= leaf.sz_leaf {
                p.rc = FTS5_CORRUPT;
            } else {
                let (nb, n_term) = gv32(&leaf.p, i_off);
                i_off += nb;
                let n_cmp = n_term.min(n_idx_term);
                let mut res = fts5_bytes_compare(
                    sub(&leaf.p, i_off, n_cmp),
                    sub(&z_idx_term, 0, n_cmp),
                );
                if res == 0 {
                    res = n_term - n_idx_term;
                }
                if res < 0 {
                    p.rc = FTS5_CORRUPT;
                }
            }

            fts5_integrity_check_pgidx(p, &leaf);
        }
        drop(leaf);
        if p.rc != SQLITE_OK {
            break;
        }

        /* Confere que as folhas seguintes (até a próxima entrada de `%_idx`) existem e não têm
        ** termos. */
        fts5_index_integrity_check_empty(
            p,
            db,
            cfg,
            seg,
            i_idx_prev_leaf + 1,
            i_dlidx_prev_leaf + 1,
            i_idx_leaf - 1,
        );
        if p.rc != SQLITE_OK {
            break;
        }

        /* Se há índice de doclist, confere que ele está certo. */
        if b_idx_dlidx != 0 {
            let mut i_prev_leaf = i_idx_leaf;
            let i_segid = seg.i_segid;
            let mut i_pg = 0;

            if let Some(mut dl) = fts5_dlidx_iter_init(p, db, cfg, false, i_segid, i_idx_leaf) {
                while !fts5_dlidx_iter_eof(p, &dl) {
                    /* Confere as páginas sem rowid que vêm antes da folha corrente. */
                    i_pg = i_prev_leaf + 1;
                    while i_pg < fts5_dlidx_iter_pgno(&dl) {
                        let i_key = fts5_segment_rowid(i_segid, i_pg);
                        if let Some(l) = fts5_data_read(p, db, cfg, i_key) {
                            if fts5_leaf_first_rowid_off(&l) != 0 {
                                p.rc = FTS5_CORRUPT;
                            }
                        }
                        i_pg += 1;
                    }
                    i_prev_leaf = fts5_dlidx_iter_pgno(&dl);

                    /* Confere que a folha indicada pelo iterador contém mesmo o rowid que ele
                    ** sugere. */
                    let i_key = fts5_segment_rowid(i_segid, i_prev_leaf);
                    if let Some(l) = fts5_data_read(p, db, cfg, i_key) {
                        let i_rowid_off = fts5_leaf_first_rowid_off(&l);
                        if i_rowid_off >= l.sz_leaf {
                            p.rc = FTS5_CORRUPT;
                        } else if !b_secure_delete || i_rowid_off > 0 {
                            let i_dl_rowid = fts5_dlidx_iter_rowid(&dl);
                            let i_rowid = gv64(&l.p, i_rowid_off).1 as i64;
                            if i_rowid < i_dl_rowid || (!b_secure_delete && i_rowid != i_dl_rowid) {
                                p.rc = FTS5_CORRUPT;
                            }
                        }
                    }

                    fts5_dlidx_iter_next(p, db, cfg, &mut dl);
                }
            }

            i_dlidx_prev_leaf = i_pg;
        } else {
            i_dlidx_prev_leaf = seg.pgno_last;
            /* Falta conferir que não existe índice de doclist (o TODO do C). */
        }

        i_idx_prev_leaf = i_idx_leaf;
    }

    let rc2 = match stmt {
        Some(id) => finalize(db, id),
        None => SQLITE_OK,
    };
    if p.rc == SQLITE_OK {
        p.rc = rc2;
    }
}
