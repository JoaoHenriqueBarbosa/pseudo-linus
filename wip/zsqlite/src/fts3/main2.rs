//! `fts3.c` (parte 2): as mesclagens de listas de posições e de doclists, a seleção de termos
//! (`fts3TermSelect`), os leitores de segmentos por termo, o cursor da tabela (`xOpen`, `xClose`,
//! `xFilter`, `xNext`, `xColumn`) e os iteradores de doclist. A avaliação da consulta (`fts3Eval*`)
//! está em [`super::main3`] e a tabela virtual em [`super::main`].
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **Ponteiros para dentro de buffers** são deslocamentos (`usize`); `pEnd` é o `len()` do
//!   buffer, que tem sempre o comprimento exato da lista (o zero de enchimento do C é o
//!   [`at`], que lê zero além do fim).
//! * **Mesclagem no lugar.** `fts3PoslistPhraseMerge`, `fts3PoslistMerge` e as mesclagens de
//!   doclist escrevem num `Vec` de saída (o C às vezes escreve por cima da lista de entrada da
//!   direita; as escritas nunca passam as leituras, então o resultado é o mesmo byte a byte). A
//!   posição de escrita `p` do C é o `len()` da saída, e o "nada foi escrito" (`*pp==p`) é
//!   `len()` igual ao do começo.
//! * **`xEof`.** O `fts3EofMethod` do C limpa o cursor no fim (`fts3ClearCursor`), mas o `xEof`
//!   deste crate não recebe a conexão, que a limpeza precisa para finalizar o comando. A limpeza
//!   acontece em `xClose` e no `xFilter` seguinte (os dois chamam [`fts3_clear_cursor`]); o que
//!   se observa é o mesmo.
//! * **Cursor por id.** O `Fts3Cursor` mora na tabela ([`super::main::Fts3FullTable`]) e o cursor
//!   do núcleo é só o id dele; o valor "ponteiro" da coluna oculta com o nome da tabela é um
//!   [`Fts3CursorRef`], que as funções `snippet()` e companhia resolvem pela conexão.

use crate::connection::{Connection, Context, StmtId};
use crate::consts::{
    LARGEST_INT64, SMALLEST_INT64, SQLITE_INTEGER, SQLITE_NOMEM, SQLITE_NULL, SQLITE_OK,
    SQLITE_PREPARE_PERSISTENT, SQLITE_ROW,
};
use crate::mem::{value_numeric_type, value_type, Mem};
use crate::mem2::mem_set_pointer;
use crate::prepare::prepare_v3;
use crate::printf::{mprintf, PrintfArg};
use crate::util::at;
use crate::vdbeapi::{
    bind_value, column_int64, column_value, data_count, finalize, reset, result_int, result_int64,
    result_value, step, text_of, value_int, value_int64,
};

use super::expr::{fts3_expr_free, fts3_expr_parse};
use super::int::{
    Fts3Cursor, Fts3MultiSegReader, Fts3PhraseToken, Fts3SegFilter, Fts3Table, FTS3_DOCID_SEARCH,
    FTS3_FULLSCAN_SEARCH, FTS3_FULLTEXT_SEARCH, FTS3_HAVE_DOCID_GE, FTS3_HAVE_DOCID_LE,
    FTS3_HAVE_LANGID, FTS3_SEGCURSOR_ALL, FTS3_SEGMENT_COLUMN_FILTER, FTS3_SEGMENT_FIRST,
    FTS3_SEGMENT_IGNORE_EMPTY, FTS3_SEGMENT_PREFIX, FTS3_SEGMENT_REQUIRE_POS, FTS_CORRUPT_VTAB,
    POS_COLUMN, POS_END,
};
use super::main3::{fts3_eval_next, fts3_eval_start};
use super::varint::{fts3_get_varint, fts3_get_varint32, fts3_get_varint_u, fts3_put_varint};
use super::write::{
    fts3_columnlist_copy, fts3_poslist_copy, fts3_free_deferred_tokens, fts3_seg_reader_cursor,
    fts3_seg_reader_cursor_fill, fts3_seg_reader_finish, fts3_seg_reader_start,
    fts3_seg_reader_step, fts3_segments_close, sl,
};

/// `POSITION_LIST_END`: o valor que `fts3ReadNextPos` devolve no fim de uma lista de coluna.
const POSITION_LIST_END: i64 = 0x7fff_ffff;

/// O valor "ponteiro" `"fts3cursor"` da coluna oculta com o nome da tabela (o `Fts3Cursor *` do
/// `sqlite3_result_pointer` do C): o id do cursor na tabela.
pub struct Fts3CursorRef {
    /// O id do cursor.
    pub id: i64,
}

// ---------------------------------------------------------------------------------------------
// Varints e listas de posições
// ---------------------------------------------------------------------------------------------

/// Acrescenta o varint de `v` a `out` e devolve o tamanho.
pub(super) fn push_varint(out: &mut Vec<u8>, v: i64) -> i32 {
    let mut tmp = [0u8; 10];
    let n = fts3_put_varint(&mut tmp, v);
    out.extend_from_slice(&tmp[..n as usize]);
    n
}

/// `fts3GetDeltaVarint`: lê um varint de `buf` em `*pp` e o soma a `*p_val`.
fn get_delta_varint(buf: &[u8], pp: &mut usize, p_val: &mut i64) {
    let (n, v) = fts3_get_varint(sl(buf, *pp));
    *pp += n as usize;
    *p_val = p_val.wrapping_add(v);
}

/// `fts3PutDeltaVarint`: acrescenta a `out` o varint de `i_val - *pi_prev`.
fn put_delta_varint(out: &mut Vec<u8>, pi_prev: &mut i64, i_val: i64) {
    push_varint(out, i_val.wrapping_sub(*pi_prev));
    *pi_prev = i_val;
}

/// `fts3ReadNextPos`.
fn read_next_pos(buf: &[u8], pp: &mut usize, pi: &mut i64) {
    if (at(buf, *pp) & 0xFE) != 0 {
        let (n, i_val) = fts3_get_varint32(sl(buf, *pp));
        *pp += n as usize;
        *pi = pi.wrapping_add(i_val as i64);
        *pi -= 2;
    } else {
        *pi = POSITION_LIST_END;
    }
}

/// `fts3PutColNumber`: escreve `0x01` e o número da coluna, se não é zero. Devolve os bytes.
fn put_col_number(out: &mut Vec<u8>, i_col: i32) -> usize {
    if i_col != 0 {
        out.push(0x01);
        1 + push_varint(out, i_col as i64) as usize
    } else {
        0
    }
}

/// `fts3PoslistMerge`: mescla (união) duas listas de posições, `buf1` em `*pp1` e `buf2` em `*pp2`,
/// e acrescenta o resultado a `out`.
pub fn fts3_poslist_merge(
    out: &mut Vec<u8>,
    buf1: &[u8],
    pp1: &mut usize,
    buf2: &[u8],
    pp2: &mut usize,
) -> i32 {
    let mut p1 = *pp1;
    let mut p2 = *pp2;

    while at(buf1, p1) != 0 || at(buf2, p2) != 0 {
        let i_col1: i32;
        let i_col2: i32;

        if at(buf1, p1) == POS_COLUMN {
            i_col1 = fts3_get_varint32(sl(buf1, p1 + 1)).1;
            if i_col1 == 0 {
                return FTS_CORRUPT_VTAB;
            }
        } else if at(buf1, p1) == POS_END {
            i_col1 = 0x7fff_ffff;
        } else {
            i_col1 = 0;
        }

        if at(buf2, p2) == POS_COLUMN {
            i_col2 = fts3_get_varint32(sl(buf2, p2 + 1)).1;
            if i_col2 == 0 {
                return FTS_CORRUPT_VTAB;
            }
        } else if at(buf2, p2) == POS_END {
            i_col2 = 0x7fff_ffff;
        } else {
            i_col2 = 0;
        }

        if i_col1 == i_col2 {
            let mut i1: i64 = 0; /* última posição de pp1 */
            let mut i2: i64 = 0; /* última posição de pp2 */
            let mut i_prev: i64 = 0;
            let n = put_col_number(out, i_col1);
            p1 += n;
            p2 += n;

            /* Aqui p1 e p2 apontam o começo das listas da mesma coluna. Cada uma é uma lista de
            ** varints não negativos em delta, somados de 2, terminada por POS_END ou POS_COLUMN.
            ** O bloco as mescla em `out`, sem escrever o terminador. */
            get_delta_varint(buf1, &mut p1, &mut i1);
            get_delta_varint(buf2, &mut p2, &mut i2);
            if i1 < 2 || i2 < 2 {
                break;
            }
            loop {
                put_delta_varint(out, &mut i_prev, if i1 < i2 { i1 } else { i2 });
                i_prev -= 2;
                if i1 == i2 {
                    read_next_pos(buf1, &mut p1, &mut i1);
                    read_next_pos(buf2, &mut p2, &mut i2);
                } else if i1 < i2 {
                    read_next_pos(buf1, &mut p1, &mut i1);
                } else {
                    read_next_pos(buf2, &mut p2, &mut i2);
                }
                if !(i1 != POSITION_LIST_END || i2 != POSITION_LIST_END) {
                    break;
                }
            }
        } else if i_col1 < i_col2 {
            p1 += put_col_number(out, i_col1);
            fts3_columnlist_copy(Some(out), buf1, &mut p1);
        } else {
            p2 += put_col_number(out, i_col2);
            fts3_columnlist_copy(Some(out), buf2, &mut p2);
        }
    }

    out.push(POS_END);
    *pp1 = p1 + 1;
    *pp2 = p2 + 1;
    SQLITE_OK
}

/// `fts3PoslistPhraseMerge`: mescla (interseção com distância) duas listas de posições. Acrescenta
/// a `out` as posições de `buf2` (ou de `buf1`, com `is_save_left`) que casam; devolve verdadeiro
/// se escreveu algo (o terminador `0x00` incluído).
pub fn fts3_poslist_phrase_merge(
    out: &mut Vec<u8>,
    n_token: i32,
    is_save_left: bool,
    is_exact: bool,
    buf1: &[u8],
    pp1: &mut usize,
    buf2: &[u8],
    pp2: &mut usize,
) -> bool {
    let start = out.len();
    let mut p1 = *pp1;
    let mut p2 = *pp2;
    let mut i_col1: i32 = 0;
    let mut i_col2: i32 = 0;
    let n_token = n_token as i64;

    if at(buf1, p1) == POS_COLUMN {
        p1 += 1;
        let (n, v) = fts3_get_varint32(sl(buf1, p1));
        p1 += n as usize;
        i_col1 = v;
    }
    if at(buf2, p2) == POS_COLUMN {
        p2 += 1;
        let (n, v) = fts3_get_varint32(sl(buf2, p2));
        p2 += n as usize;
        i_col2 = v;
    }

    loop {
        if i_col1 == i_col2 {
            let mut p_save: Option<usize> = Some(out.len());
            let mut i_prev: i64 = 0;
            let mut i_pos1: i64 = 0;
            let mut i_pos2: i64 = 0;

            if i_col1 != 0 {
                out.push(POS_COLUMN);
                push_varint(out, i_col1 as i64);
            }

            get_delta_varint(buf1, &mut p1, &mut i_pos1);
            i_pos1 -= 2;
            get_delta_varint(buf2, &mut p2, &mut i_pos2);
            i_pos2 -= 2;
            if i_pos1 < 0 || i_pos2 < 0 {
                break;
            }

            loop {
                if i_pos2 == i_pos1.wrapping_add(n_token)
                    || (!is_exact && i_pos2 > i_pos1 && i_pos2 <= i_pos1.wrapping_add(n_token))
                {
                    let i_save = if is_save_left { i_pos1 } else { i_pos2 };
                    put_delta_varint(out, &mut i_prev, i_save + 2);
                    i_prev -= 2;
                    p_save = None;
                }
                if (!is_save_left && i_pos2 <= i_pos1.wrapping_add(n_token)) || i_pos2 <= i_pos1 {
                    if (at(buf2, p2) & 0xFE) == 0 {
                        break;
                    }
                    get_delta_varint(buf2, &mut p2, &mut i_pos2);
                    i_pos2 -= 2;
                } else {
                    if (at(buf1, p1) & 0xFE) == 0 {
                        break;
                    }
                    get_delta_varint(buf1, &mut p1, &mut i_pos1);
                    i_pos1 -= 2;
                }
            }

            if let Some(s) = p_save {
                out.truncate(s);
            }

            fts3_columnlist_copy(None, buf1, &mut p1);
            fts3_columnlist_copy(None, buf2, &mut p2);
            if at(buf1, p1) == 0 || at(buf2, p2) == 0 {
                break;
            }

            p1 += 1;
            let (n, v) = fts3_get_varint32(sl(buf1, p1));
            p1 += n as usize;
            i_col1 = v;
            p2 += 1;
            let (n, v) = fts3_get_varint32(sl(buf2, p2));
            p2 += n as usize;
            i_col2 = v;
        }
        /* Avança p1 ou p2 (a da menor coluna) até o 0x00 que fecha a lista de posições ou o 0x01
        ** que precede o número da próxima coluna. */
        else if i_col1 < i_col2 {
            fts3_columnlist_copy(None, buf1, &mut p1);
            if at(buf1, p1) == 0 {
                break;
            }
            p1 += 1;
            let (n, v) = fts3_get_varint32(sl(buf1, p1));
            p1 += n as usize;
            i_col1 = v;
        } else {
            fts3_columnlist_copy(None, buf2, &mut p2);
            if at(buf2, p2) == 0 {
                break;
            }
            p2 += 1;
            let (n, v) = fts3_get_varint32(sl(buf2, p2));
            p2 += n as usize;
            i_col2 = v;
        }
    }

    fts3_poslist_copy(None, buf2, &mut p2);
    fts3_poslist_copy(None, buf1, &mut p1);
    *pp1 = p1;
    *pp2 = p2;
    if out.len() == start {
        return false;
    }
    out.push(0x00);
    true
}

/// `fts3PoslistNearMerge`: o `xNEAR` entre duas listas de posições, `nRight` à direita e `nLeft`
/// à esquerda. `a_tmp` é o espaço de trabalho. Acrescenta o resultado a `out` e devolve verdadeiro
/// se há resultado.
pub fn fts3_poslist_near_merge(
    out: &mut Vec<u8>,
    a_tmp: &mut Vec<u8>,
    n_right: i32,
    n_left: i32,
    buf1: &[u8],
    pp1: &mut usize,
    buf2: &[u8],
    pp2: &mut usize,
) -> bool {
    let p1 = *pp1;
    let p2 = *pp2;
    let mut res = true;

    a_tmp.clear();
    fts3_poslist_phrase_merge(a_tmp, n_right, false, false, buf1, pp1, buf2, pp2);
    let a_tmp2 = a_tmp.len();
    *pp1 = p1;
    *pp2 = p2;
    fts3_poslist_phrase_merge(a_tmp, n_left, true, false, buf2, pp2, buf1, pp1);
    let first = a_tmp2 != 0;
    let second = a_tmp.len() != a_tmp2;
    let tmp: &[u8] = a_tmp;
    if first && second {
        let mut q1 = 0usize;
        let mut q2 = a_tmp2;
        fts3_poslist_merge(out, tmp, &mut q1, tmp, &mut q2);
    } else if first {
        let mut q = 0usize;
        fts3_poslist_copy(Some(out), tmp, &mut q);
    } else if second {
        let mut q = a_tmp2;
        fts3_poslist_copy(Some(out), tmp, &mut q);
    } else {
        res = false;
    }
    res
}

// ---------------------------------------------------------------------------------------------
// Mesclagem de doclists
// ---------------------------------------------------------------------------------------------

/// `DOCID_CMP`.
#[inline]
pub(super) fn docid_cmp(b_desc: bool, i1: i64, i2: i64) -> i64 {
    let c: i64 = if i1 > i2 {
        1
    } else if i1 == i2 {
        0
    } else {
        -1
    };
    if b_desc {
        -c
    } else {
        c
    }
}

/// `fts3GetDeltaVarint3`: lê o próximo docid em delta de `buf` em `*pp`; `*pp` vira `None` no fim.
fn get_delta_varint3(buf: &[u8], pp: &mut Option<usize>, b_desc_idx: bool, p_val: &mut i64) {
    let Some(p) = *pp else { return };
    if p >= buf.len() {
        *pp = None;
    } else {
        let (n, i_val) = fts3_get_varint_u(sl(buf, p));
        *pp = Some(p + n as usize);
        *p_val = if b_desc_idx {
            (*p_val as u64).wrapping_sub(i_val) as i64
        } else {
            (*p_val as u64).wrapping_add(i_val) as i64
        };
    }
}

/// `fts3PutDeltaVarint3`.
fn put_delta_varint3(
    out: &mut Vec<u8>,
    b_desc_idx: bool,
    pi_prev: &mut i64,
    pb_first: &mut bool,
    i_val: i64,
) {
    let i_write: u64 = if !b_desc_idx || !*pb_first {
        (i_val as u64).wrapping_sub(*pi_prev as u64)
    } else {
        (*pi_prev as u64).wrapping_sub(i_val as u64)
    };
    push_varint(out, i_write as i64);
    *pi_prev = i_val;
    *pb_first = true;
}

/// `fts3DoclistOrMerge`: a união de duas doclists.
pub fn fts3_doclist_or_merge(b_desc: bool, a1: &[u8], a2: &[u8]) -> Result<Vec<u8>, i32> {
    let mut i1: i64 = 0;
    let mut i2: i64 = 0;
    let mut i_prev: i64 = 0;
    let mut p1: Option<usize> = Some(0);
    let mut p2: Option<usize> = Some(0);
    let mut out: Vec<u8> = Vec::with_capacity(a1.len() + a2.len());
    let mut b_first_out = false;
    let mut rc = SQLITE_OK;

    get_delta_varint3(a1, &mut p1, false, &mut i1);
    get_delta_varint3(a2, &mut p2, false, &mut i2);
    while p1.is_some() || p2.is_some() {
        let i_diff = docid_cmp(b_desc, i1, i2);

        if p1.is_some() && p2.is_some() && i_diff == 0 {
            put_delta_varint3(&mut out, b_desc, &mut i_prev, &mut b_first_out, i1);
            let (mut q1, mut q2) = (p1.unwrap_or(0), p2.unwrap_or(0));
            rc = fts3_poslist_merge(&mut out, a1, &mut q1, a2, &mut q2);
            p1 = Some(q1);
            p2 = Some(q2);
            if rc != SQLITE_OK {
                break;
            }
            get_delta_varint3(a1, &mut p1, b_desc, &mut i1);
            get_delta_varint3(a2, &mut p2, b_desc, &mut i2);
        } else if p2.is_none() || (p1.is_some() && i_diff < 0) {
            put_delta_varint3(&mut out, b_desc, &mut i_prev, &mut b_first_out, i1);
            let mut q1 = p1.unwrap_or(0);
            fts3_poslist_copy(Some(&mut out), a1, &mut q1);
            p1 = Some(q1);
            get_delta_varint3(a1, &mut p1, b_desc, &mut i1);
        } else {
            put_delta_varint3(&mut out, b_desc, &mut i_prev, &mut b_first_out, i2);
            let mut q2 = p2.unwrap_or(0);
            fts3_poslist_copy(Some(&mut out), a2, &mut q2);
            p2 = Some(q2);
            get_delta_varint3(a2, &mut p2, b_desc, &mut i2);
        }
    }

    if rc != SQLITE_OK {
        return Err(rc);
    }
    Ok(out)
}

/// `fts3DoclistPhraseMerge`: a doclist de uma frase de dois tokens `n_dist` posições distantes.
/// `a_right` é a doclist da direita e recebe o resultado.
pub fn fts3_doclist_phrase_merge(
    b_desc: bool,
    n_dist: i32,
    a_left: &[u8],
    a_right: &mut Vec<u8>,
) -> i32 {
    let mut i1: i64 = 0;
    let mut i2: i64 = 0;
    let mut i_prev: i64 = 0;
    let mut p1: Option<usize> = Some(0);
    let mut p2: Option<usize> = Some(0);
    let mut b_first_out = false;
    let mut out: Vec<u8> = Vec::with_capacity(a_right.len() + 10);

    get_delta_varint3(a_left, &mut p1, false, &mut i1);
    get_delta_varint3(a_right, &mut p2, false, &mut i2);

    while p1.is_some() && p2.is_some() {
        let i_diff = docid_cmp(b_desc, i1, i2);
        let (mut q1, mut q2) = (p1.unwrap_or(0), p2.unwrap_or(0));
        if i_diff == 0 {
            let save_len = out.len();
            let i_prev_save = i_prev;
            let b_first_out_save = b_first_out;

            put_delta_varint3(&mut out, b_desc, &mut i_prev, &mut b_first_out, i1);
            if !fts3_poslist_phrase_merge(&mut out, n_dist, false, true, a_left, &mut q1, a_right, &mut q2) {
                out.truncate(save_len);
                i_prev = i_prev_save;
                b_first_out = b_first_out_save;
            }
            p1 = Some(q1);
            p2 = Some(q2);
            get_delta_varint3(a_left, &mut p1, b_desc, &mut i1);
            get_delta_varint3(a_right, &mut p2, b_desc, &mut i2);
        } else if i_diff < 0 {
            fts3_poslist_copy(None, a_left, &mut q1);
            p1 = Some(q1);
            get_delta_varint3(a_left, &mut p1, b_desc, &mut i1);
        } else {
            fts3_poslist_copy(None, a_right, &mut q2);
            p2 = Some(q2);
            get_delta_varint3(a_right, &mut p2, b_desc, &mut i2);
        }
    }

    *a_right = out;
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Seleção de termos
// ---------------------------------------------------------------------------------------------

/// `TermSelect`: as 16 doclists parciais da mesclagem aos pares. O comprimento de cada `Vec` é o
/// `anOutput`.
#[derive(Default)]
struct TermSelect {
    aa_output: [Option<Vec<u8>>; 16],
}

/// `fts3TermSelectFinishMerge`: mescla todas as doclists de `aa_output` em uma, que fica em `[0]`.
fn fts3_term_select_finish_merge(p: &Fts3Table, ts: &mut TermSelect) -> i32 {
    let mut a_out: Option<Vec<u8>> = None;

    for i in 0..ts.aa_output.len() {
        if let Some(cur) = ts.aa_output[i].take() {
            match a_out.take() {
                None => a_out = Some(cur),
                Some(prev) => match fts3_doclist_or_merge(p.b_desc_idx, &cur, &prev) {
                    Ok(merged) => a_out = Some(merged),
                    Err(rc) => return rc,
                },
            }
        }
    }

    ts.aa_output[0] = a_out;
    SQLITE_OK
}

/// `fts3TermSelectMerge`: acrescenta a doclist `a_doclist` ao conjunto.
fn fts3_term_select_merge(p: &Fts3Table, ts: &mut TermSelect, a_doclist: &[u8]) -> i32 {
    if ts.aa_output[0].is_none() {
        /* O primeiro termo selecionado: copia a doclist para a saída. */
        ts.aa_output[0] = Some(a_doclist.to_vec());
    } else {
        let mut a_merge: Vec<u8> = a_doclist.to_vec();
        let n = ts.aa_output.len();
        for i_out in 0..n {
            match ts.aa_output[i_out].take() {
                None => {
                    ts.aa_output[i_out] = Some(a_merge);
                    break;
                }
                Some(cur) => {
                    match fts3_doclist_or_merge(p.b_desc_idx, &a_merge, &cur) {
                        Ok(merged) => a_merge = merged,
                        Err(rc) => return rc,
                    }
                    if (i_out + 1) == n {
                        ts.aa_output[i_out] = Some(std::mem::take(&mut a_merge));
                    }
                }
            }
        }
    }
    SQLITE_OK
}

/// `fts3SegReaderCursorFree`.
pub(super) fn fts3_seg_reader_cursor_free(db: &mut Connection, mut p_segcsr: Box<Fts3MultiSegReader>) {
    fts3_seg_reader_finish(db, &mut p_segcsr);
}

/// `fts3TermSelect`: a doclist de um token em todas as colunas (`i_column` negativo) ou em uma.
/// Consome o leitor de segmentos do token (`pSegcsr`). `Ok(None)` é o `NULL` do C.
pub fn fts3_term_select(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_tok: &mut Fts3PhraseToken,
    i_column: i32,
) -> Result<Option<Vec<u8>>, i32> {
    let Some(mut p_segcsr) = p_tok.p_segcsr.take() else {
        return Ok(None);
    };
    let mut tsc = TermSelect::default();

    let filter = Fts3SegFilter {
        flags: FTS3_SEGMENT_IGNORE_EMPTY
            | FTS3_SEGMENT_REQUIRE_POS
            | (if p_tok.is_prefix { FTS3_SEGMENT_PREFIX } else { 0 })
            | (if p_tok.b_first { FTS3_SEGMENT_FIRST } else { 0 })
            | (if i_column < p.n_column() { FTS3_SEGMENT_COLUMN_FILTER } else { 0 }),
        i_col: i_column,
        z_term: Some(p_tok.z.clone()),
    };

    let mut rc = fts3_seg_reader_start(db, p, &mut p_segcsr, &filter);
    while rc == SQLITE_OK {
        rc = fts3_seg_reader_step(db, p, &mut p_segcsr);
        if rc != SQLITE_ROW {
            break;
        }
        rc = fts3_term_select_merge(p, &mut tsc, &p_segcsr.a_doclist);
    }

    if rc == SQLITE_OK {
        rc = fts3_term_select_finish_merge(p, &mut tsc);
    }
    let out = if rc == SQLITE_OK { Ok(tsc.aa_output[0].take()) } else { Err(rc) };

    fts3_seg_reader_cursor_free(db, p_segcsr);
    out
}

/// `fts3SegReaderCursorAddZero`.
fn fts3_seg_reader_cursor_add_zero(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    z_term: &[u8],
    p_csr: &mut Fts3MultiSegReader,
) -> i32 {
    fts3_seg_reader_cursor_fill(db, p, i_langid, 0, FTS3_SEGCURSOR_ALL, Some(z_term), false, false, p_csr)
}

/// `fts3TermSegReaderCursor`: o leitor de segmentos de um termo. Devolve o leitor (que o chamador
/// guarda no token mesmo em erro, como o C) e o código de retorno.
pub fn fts3_term_seg_reader_cursor(
    db: &mut Connection,
    p: &mut Fts3Table,
    i_langid: i32,
    z_term: &[u8],
    is_prefix: bool,
) -> (Box<Fts3MultiSegReader>, i32) {
    let mut p_segcsr = Box::new(Fts3MultiSegReader::default());
    let mut rc = SQLITE_NOMEM;
    let mut b_found = false; /* verdadeiro depois de achar um índice */
    let n_term = z_term.len() as i32;

    if is_prefix {
        let mut i = 1;
        while !b_found && i < p.n_index {
            if p.a_index[i as usize].n_prefix == n_term {
                b_found = true;
                rc = fts3_seg_reader_cursor(
                    db, p, i_langid, i, FTS3_SEGCURSOR_ALL, Some(z_term), false, false, &mut p_segcsr,
                );
                p_segcsr.b_lookup = true;
            }
            i += 1;
        }

        let mut i = 1;
        while !b_found && i < p.n_index {
            if p.a_index[i as usize].n_prefix == n_term + 1 {
                b_found = true;
                rc = fts3_seg_reader_cursor(
                    db, p, i_langid, i, FTS3_SEGCURSOR_ALL, Some(z_term), true, false, &mut p_segcsr,
                );
                if rc == SQLITE_OK {
                    rc = fts3_seg_reader_cursor_add_zero(db, p, i_langid, z_term, &mut p_segcsr);
                }
            }
            i += 1;
        }
    }

    if !b_found {
        rc = fts3_seg_reader_cursor(
            db, p, i_langid, 0, FTS3_SEGCURSOR_ALL, Some(z_term), is_prefix, false, &mut p_segcsr,
        );
        p_segcsr.b_lookup = !is_prefix;
    }

    (p_segcsr, rc)
}

/// `fts3DoclistCountDocids`: o número de docids de uma doclist.
pub fn fts3_doclist_count_docids(a_list: &[u8]) -> i32 {
    let mut n_doc = 0;
    let mut p = 0usize;
    while p < a_list.len() {
        n_doc += 1;
        loop {
            let c = at(a_list, p);
            p += 1;
            if (c & 0x80) == 0 {
                break;
            }
        }
        fts3_poslist_copy(None, a_list, &mut p);
    }
    n_doc
}

/// `sqlite3Fts3DoclistNext`: avança `*pp_iter` para o próximo docid de `a_doclist`.
pub fn fts3_doclist_next(
    b_desc_idx: bool,
    a_doclist: &[u8],
    pp_iter: &mut Option<usize>,
    pi_docid: &mut i64,
    pb_eof: &mut bool,
) {
    match *pp_iter {
        None => {
            let (n, v) = fts3_get_varint(sl(a_doclist, 0));
            *pi_docid = v;
            *pp_iter = Some(n as usize);
        }
        Some(mut p) => {
            fts3_poslist_copy(None, a_doclist, &mut p);
            while p < a_doclist.len() && at(a_doclist, p) == 0 {
                p += 1;
            }
            if p >= a_doclist.len() {
                *pb_eof = true;
            } else {
                let (n, i_var) = fts3_get_varint(sl(a_doclist, p));
                p += n as usize;
                let m: i64 = if b_desc_idx { -1 } else { 1 };
                *pi_docid = pi_docid.wrapping_add(m.wrapping_mul(i_var));
            }
            *pp_iter = Some(p);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// O cursor
// ---------------------------------------------------------------------------------------------

/// `sqlite3_step` de um comando que pode ser nulo (o `MISUSE` do C).
fn step_opt(db: &mut Connection, st: Option<StmtId>) -> i32 {
    match st {
        Some(s) => step(db, s),
        None => crate::consts::SQLITE_MISUSE,
    }
}

/// `sqlite3_reset` de um comando que pode ser nulo (`SQLITE_OK`).
pub(super) fn reset_opt(db: &mut Connection, st: Option<StmtId>) -> i32 {
    match st {
        Some(s) => reset(db, s),
        None => SQLITE_OK,
    }
}

/// `fts3CursorFinalizeStmt`.
fn fts3_cursor_finalize_stmt(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) {
    if p_csr.b_seek_stmt {
        if p.p_seek_stmt.is_none() {
            p.p_seek_stmt = p_csr.p_stmt.take();
            reset_opt(db, p.p_seek_stmt);
        }
        p_csr.b_seek_stmt = false;
    }
    if let Some(st) = p_csr.p_stmt.take() {
        finalize(db, st);
    }
}

/// `fts3ClearCursor`: solta tudo o que o cursor guarda e o devolve ao estado de recém-aberto.
pub fn fts3_clear_cursor(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) {
    fts3_cursor_finalize_stmt(db, p, p_csr);
    fts3_free_deferred_tokens(p_csr);
    p_csr.a_doclist = Vec::new();
    p_csr.p_mi_buffer = None;
    if let Some(tree) = p_csr.p_expr.take() {
        fts3_expr_free(db, tree);
    }
    *p_csr = Fts3Cursor::default();
}

/// `fts3CursorSeekStmt`: prepara (ou toma o guardado) o comando que busca uma linha de `%_content`.
fn fts3_cursor_seek_stmt(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> i32 {
    let mut rc = SQLITE_OK;
    if p_csr.p_stmt.is_none() {
        if p.p_seek_stmt.is_some() {
            p_csr.p_stmt = p.p_seek_stmt.take();
        } else {
            let Some(z_sql) = mprintf(
                b"SELECT %s WHERE rowid = ?",
                &[PrintfArg::Text(p.z_read_exprlist.clone())],
            ) else {
                return SQLITE_NOMEM;
            };
            p.b_lock += 1;
            let (rc2, st, _) = prepare_v3(db, &z_sql, -1, SQLITE_PREPARE_PERSISTENT);
            rc = rc2;
            p_csr.p_stmt = st;
            p.b_lock -= 1;
        }
        if rc == SQLITE_OK {
            p_csr.b_seek_stmt = true;
        }
    }
    rc
}

/// `fts3CursorSeek` (sem o `pContext`: o chamador aplica o `sqlite3_result_error_code`): se o
/// cursor precisa de busca, posiciona o comando na linha `iPrevId` de `%_content`.
pub fn fts3_cursor_seek(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> i32 {
    let mut rc = SQLITE_OK;
    if p_csr.is_require_seek {
        rc = fts3_cursor_seek_stmt(db, p, p_csr);
        if rc == SQLITE_OK {
            p.b_lock += 1;
            if let Some(st) = p_csr.p_stmt {
                crate::vdbeapi::bind_int64(db, st, 1, p_csr.i_prev_id);
            }
            p_csr.is_require_seek = false;
            if SQLITE_ROW == step_opt(db, p_csr.p_stmt) {
                p.b_lock -= 1;
                return SQLITE_OK;
            } else {
                p.b_lock -= 1;
                rc = reset_opt(db, p_csr.p_stmt);
                if rc == SQLITE_OK && p.z_content_tbl.is_none() {
                    /* Nenhuma linha achada e nenhum erro: `%_content` não tem uma linha que está
                    ** no índice de texto completo. As estruturas estão corrompidas. */
                    rc = FTS_CORRUPT_VTAB;
                    p_csr.is_eof = true;
                }
            }
        }
    }
    rc
}

/// `fts3NextMethod`.
pub fn fts3_next(db: &mut Connection, p: &mut Fts3Table, p_csr: &mut Fts3Cursor) -> i32 {
    let rc;
    if p_csr.e_search as i32 == FTS3_DOCID_SEARCH || p_csr.e_search as i32 == FTS3_FULLSCAN_SEARCH {
        p.b_lock += 1;
        if SQLITE_ROW != step_opt(db, p_csr.p_stmt) {
            p_csr.is_eof = true;
            rc = reset_opt(db, p_csr.p_stmt);
        } else {
            p_csr.i_prev_id = match p_csr.p_stmt {
                Some(st) => column_int64(db, st, 0),
                None => 0,
            };
            rc = SQLITE_OK;
        }
        p.b_lock -= 1;
    } else {
        rc = fts3_eval_next(db, p, p_csr);
    }
    rc
}

/// `fts3DocidRange`.
fn fts3_docid_range(p_val: Option<&Mem>, i_default: i64) -> i64 {
    if let Some(v) = p_val {
        let mut c = v.clone();
        if value_numeric_type(&mut c) == SQLITE_INTEGER {
            return value_int64(v);
        }
    }
    i_default
}

/// `fts3FilterMethod`.
pub fn fts3_filter(
    db: &mut Connection,
    p: &mut Fts3Table,
    p_csr: &mut Fts3Cursor,
    idx_num: i32,
    idx_str: Option<&[u8]>,
    ap_val: &[Mem],
) -> i32 {
    let mut rc = SQLITE_OK;

    if p.b_lock != 0 {
        return crate::consts::SQLITE_ERROR;
    }

    let e_search = idx_num & 0x0000_FFFF;

    /* Junta os argumentos em variáveis locais. */
    let mut i_idx = 0usize;
    let mut p_cons: Option<&Mem> = None;
    let mut p_langid: Option<&Mem> = None;
    let mut p_docid_ge: Option<&Mem> = None;
    let mut p_docid_le: Option<&Mem> = None;
    if e_search != FTS3_FULLSCAN_SEARCH {
        p_cons = ap_val.get(i_idx);
        i_idx += 1;
    }
    if (idx_num & FTS3_HAVE_LANGID) != 0 {
        p_langid = ap_val.get(i_idx);
        i_idx += 1;
    }
    if (idx_num & FTS3_HAVE_DOCID_GE) != 0 {
        p_docid_ge = ap_val.get(i_idx);
        i_idx += 1;
    }
    if (idx_num & FTS3_HAVE_DOCID_LE) != 0 {
        p_docid_le = ap_val.get(i_idx);
    }

    /* Se o cursor já foi usado, limpa. */
    fts3_clear_cursor(db, p, p_csr);

    /* Os limites inferior e superior dos docids devolvidos. */
    p_csr.i_min_docid = fts3_docid_range(p_docid_ge, SMALLEST_INT64);
    p_csr.i_max_docid = fts3_docid_range(p_docid_le, LARGEST_INT64);

    p_csr.b_desc = match idx_str {
        Some(s) => at(s, 0) == b'D',
        None => p.b_desc_idx,
    };
    p_csr.e_search = e_search as i16;

    if e_search != FTS3_DOCID_SEARCH && e_search != FTS3_FULLSCAN_SEARCH {
        let i_col = e_search - FTS3_FULLTEXT_SEARCH;
        let cons = p_cons.unwrap_or(&Mem::default()).clone();
        let z_query = text_of(&cons).map(|c| c.into_owned());

        if z_query.is_none() && value_type(&cons) != SQLITE_NULL {
            return SQLITE_NOMEM;
        }

        p_csr.i_langid = 0;
        if let Some(l) = p_langid {
            p_csr.i_langid = value_int(l);
        }

        let Some(tokenizer) = p.p_tokenizer.clone() else {
            return crate::consts::SQLITE_ERROR;
        };
        match fts3_expr_parse(
            &*tokenizer,
            p_csr.i_langid,
            &p.az_column,
            p.b_fts4,
            i_col,
            z_query.as_deref(),
            &mut p.z_err_msg,
        ) {
            Ok(tree) => p_csr.p_expr = tree,
            Err(rc) => return rc,
        }

        rc = fts3_eval_start(db, p, p_csr);
        fts3_segments_close(db, p);
        if rc != SQLITE_OK {
            return rc;
        }
        p_csr.p_next_id = 0;
        p_csr.i_prev_id = 0;
    }

    /* Compila um SELECT para este cursor. Na varredura total ele percorre `%_content`; na busca
    ** de texto completo ou de docid ele busca uma linha por docid. */
    if e_search == FTS3_FULLSCAN_SEARCH {
        let z_dir: &[u8] = if p_csr.b_desc { b"DESC" } else { b"ASC" };
        let z_sql = if p_docid_ge.is_some() || p_docid_le.is_some() {
            mprintf(
                b"SELECT %s WHERE rowid BETWEEN %lld AND %lld ORDER BY rowid %s",
                &[
                    PrintfArg::Text(p.z_read_exprlist.clone()),
                    PrintfArg::Int(p_csr.i_min_docid),
                    PrintfArg::Int(p_csr.i_max_docid),
                    PrintfArg::Text(Some(z_dir.to_vec())),
                ],
            )
        } else {
            mprintf(
                b"SELECT %s ORDER BY rowid %s",
                &[
                    PrintfArg::Text(p.z_read_exprlist.clone()),
                    PrintfArg::Text(Some(z_dir.to_vec())),
                ],
            )
        };
        match z_sql {
            Some(z_sql) => {
                p.b_lock += 1;
                let (rc2, st, _) = prepare_v3(db, &z_sql, -1, SQLITE_PREPARE_PERSISTENT);
                rc = rc2;
                p_csr.p_stmt = st;
                p.b_lock -= 1;
            }
            None => rc = SQLITE_NOMEM,
        }
    } else if e_search == FTS3_DOCID_SEARCH {
        rc = fts3_cursor_seek_stmt(db, p, p_csr);
        if rc == SQLITE_OK {
            if let (Some(st), Some(c)) = (p_csr.p_stmt, p_cons) {
                rc = bind_value(db, st, 1, c);
            }
        }
    }
    if rc != SQLITE_OK {
        return rc;
    }

    fts3_next(db, p, p_csr)
}

/// `fts3ColumnMethod`: `id` é o id do cursor na tabela (o valor da coluna com o nome da tabela).
pub fn fts3_column(
    p: &mut Fts3Table,
    id: i64,
    p_csr: &mut Fts3Cursor,
    ctx: &mut Context<'_>,
    i_col: i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut i_col = i_col;

    let mut which = i_col - p.n_column();
    if which == 2 {
        if p_csr.p_expr.is_some() {
            result_int64(ctx, p_csr.i_langid as i64);
            return rc;
        } else if p.z_languageid.is_none() {
            result_int(ctx, 0);
            return rc;
        } else {
            i_col = p.n_column();
            which = -1; /* cai no caso `default` do C */
        }
    }

    match which {
        0 => {
            /* A coluna especial com o nome da tabela. */
            mem_set_pointer(&mut ctx.out, Box::new(Fts3CursorRef { id }), b"fts3cursor");
        }
        1 => {
            /* A coluna docid. */
            result_int64(ctx, p_csr.i_prev_id);
        }
        _ => {
            /* Uma coluna do usuário. Ou, numa varredura total, talvez a coluna do id de idioma.
            ** Posiciona o cursor. */
            rc = fts3_cursor_seek(&mut *ctx.db, p, p_csr);
            if rc == SQLITE_OK {
                if let Some(st) = p_csr.p_stmt {
                    if data_count(&*ctx.db, st) - 1 > i_col {
                        let v = column_value(&mut *ctx.db, st, i_col + 1);
                        result_value(ctx, &v);
                    }
                }
            }
        }
    }
    rc
}
