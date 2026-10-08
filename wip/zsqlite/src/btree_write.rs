//! btree.c, chunks 018 a 026: a metade de escrita do b-tree. Rebalanceamento (`rebuildPage`,
//! `pageInsertArray`, `pageFreeArray`, `editPage`, `balance_quick`, `copyNodeContent`,
//! `balance_nonroot`, `balance_deeper`, `balance`), sobrescrita de célula, `sqlite3BtreeInsert`,
//! `sqlite3BtreeTransferRow`, `sqlite3BtreeDelete`, criação, limpeza e remoção de tabelas,
//! meta-dados, contagem, verificação de integridade e o resto até o fim do arquivo.
//!
//! Modelo (CONVENTIONS.md): páginas são `PgId` com referência contada no pager do `BtShared`; os
//! bytes saem de `bt.pager.page_data(pg)` e o `MemPage` de `bt.pager.page_extra(pg)`. Um ponteiro
//! de célula do C (`u8*` dentro de uma página ou de um buffer solto) é, aqui, um `CellPtr`
//! (origem mais offset): a origem é uma página (`CellSrc::Page`) ou um buffer próprio do
//! `CellArray` (`CellSrc::Buf`). As comparações de ponteiro do C (`SQLITE_WITHIN`,
//! `SQLITE_OVERFLOW`, `pCell < pEnd`) só têm sentido dentro do mesmo buffer: cada uma vira
//! "mesma origem e comparação de offsets", e vale falso para origens diferentes (o que o C
//! obtém, na prática, comparando endereços de objetos sem relação).
//!
//! Desvios do C:
//!
//! * `balance_nonroot` não recebe `aOvflSpace` e `balance` não tem `pSpace`/`pFree`: as células
//!   de overflow são donas dos próprios bytes (`MemPage.ap_ovfl`, `Vec<u8>`), então não existe
//!   buffer de página para guardá-las. `insert_cell` copia a célula quando ela vira overflow,
//!   o que dispensa o `pTemp` (a assinatura que este arquivo supõe não o tem).
//! * `apDiv[]` guarda cópia dos bytes do divisor (o C guarda ponteiro para a página do pai, que
//!   `dropCell` estraga nos 4 primeiros bytes, bytes que nunca são lidos depois).
//! * `balance_nonroot` separa a limpeza (liberar `apOld`/`apNew`) do corpo, que devolve o
//!   código no lugar dos `goto balance_cleanup`.
//! * `sqlite3BtreeEnter`/`Leave` somem. `sqlite3BtreeCreateTable` e `btreeCreateTable` são uma
//!   função só (`btree_create_table`); idem `btree_drop_table`. `sqlite3BtreeClearTableOfCursor`
//!   não existe: quem chama usa `btree_clear_table(p, cur.pgno_root as i32, None)`.
//!   `sqlite3BtreePager`, `GetFilename`, `GetJournalname`, `IsInBackup`, `CursorHasHint`,
//!   `IsReadonly`, `Sharable`, `ConnectionCount` e `HeaderSizeBtree` são leitura de campo
//!   (`p.bt.pager`, `pager.filename(true)`, `pager.z_journal`, `p.n_backup != 0`,
//!   `bt.bts_flags & BTS_READ_ONLY`, `p.sharable`, 1): não existem aqui.
//! * `p->hasIncrblobCur` mora no `Btree`, não no `BtShared`: `btree_insert` e `btree_delete`
//!   recebem `has_incrblob_cur: bool` por parâmetro.
//! * `sqlite3BtreeTransferRow` recebe `src_bt: Option<&mut BtShared>` (`None` quando origem e
//!   destino são o mesmo `BtShared`).
//! * `sqlite3BtreeCount` recebe a consulta de interrupção no lugar do `db`; a verificação de
//!   integridade recebe um `IntegrityDb` (interrupção, `xProgress`), o `IntegrityCk` do
//!   btree_types.rs não guarda `db`/`pBt`/`pPager` e o contexto `CheckCtx` os carrega.
//! * `sqlite3BtreeSchema` recebe o valor inicial (`Some` equivale a `nBytes != 0`).
//! * Falha de alocação não existe: os ramos que só a tratavam somem.
//! * A célula corrompida que o C leria fora do buffer (comportamento indefinido) devolve
//!   `SQLITE_CORRUPT_BKPT`.
//!
//! FUNÇÕES DE OUTROS ARQUIVOS (`crate::btree`, `crate::btree_cursor`) com a assinatura que este
//! arquivo supõe, todas `pub(crate)` (o integrador ajusta o que divergir):
//!   x_cell_size(bt: &BtShared, pg: PgId, cell: &[u8]) -> u16          (pPage->xCellSize)
//!   x_parse_cell(bt: &BtShared, pg: PgId, cell: &[u8], info: &mut CellInfo)  (xParseCell)
//!   btree_clear_cell(bt, pg, cell_off: usize, info: &mut CellInfo) -> i32   (BTREE_CLEAR_CELL)
//!   btree_get_page(bt, pgno: u32, flags: i32) -> Result<PgId, i32>
//!   get_and_init_page(bt, pgno: u32, b_read_only: i32) -> Result<PgId, i32>
//!   allocate_btree_page(bt, n_near: u32, e_mode: u8) -> Result<PgId, i32>
//!   free_page(bt, pg, rc: &mut i32); zero_page(bt, pg, flags: i32)
//!   btree_init_page(bt, pg) -> i32; btree_compute_free_space(bt, pg) -> i32
//!   defragment_page(bt, pg, n_max_frag: i32) -> i32; free_space(bt, pg, i_start: u16, i_size: u16) -> i32
//!   page_find_slot(bt, pg, n_byte: i32, rc: &mut i32) -> Option<usize>
//!   drop_cell(bt, pg, idx: i32, sz: i32, rc: &mut i32)
//!   insert_cell(bt, pg, i: i32, cell: &[u8], sz: i32, i_child: u32) -> i32
//!   insert_cell_fast(bt, pg, i: i32, cell: &[u8], sz: i32) -> i32
//!   fill_in_cell(bt, pg, cell: &mut [u8], x: &BtreePayload, sz: &mut i32) -> i32
//!   ptrmap_put(bt, key: u32, e_type: u8, parent: u32, rc: &mut i32)
//!   ptrmap_get(bt, key: u32, e_type: &mut u8, pgno: &mut u32) -> i32; ptrmap_pageno(bt: &BtShared, pgno: u32) -> u32
//!   ptrmap_put_ovfl_ptr(bt, pg, cell: &[u8], room: usize, rc: &mut i32): `room` são os bytes
//!     entre o início da célula e `pSrc->aDataEnd` quando a célula está na página de origem antes
//!     do fim dela, e 0 nos demais casos (o `SQLITE_OVERFLOW` do C é `0 < room < n_local`)
//!   set_child_ptrmaps(bt, pg) -> i32; relocate_page(bt, pg, e_type: u8, i_ptr_page: u32, i_free: u32, is_commit: i32) -> i32
//!   btree_payload_to_local(bt, pg, n_payload: i64) -> i32
//!   save_all_cursors(bt, pgno_root: u32, except: Option<CursorId>) -> i32
//!   invalidate_incrblob_cursors(bt, pgno_root: u32, i_row: i64, is_clear_table: i32); invalidate_all_overflow_cache(bt)
//!   move_to_root(cur, bt) -> i32; move_to_child(cur, bt, pgno: u32) -> i32; move_to_parent(cur, bt)
//!   get_cell_info(cur, bt); save_cursor_key(cur, bt) -> i32; btree_release_all_cursor_pages(cur, bt)
//!   btree_restore_cursor_position(cur, bt) -> i32; btree_previous(cur, bt, flags: i32) -> i32
//!   btree_table_moveto(cur, bt, int_key: i64, bias: i32, res: &mut i32) -> i32
//!   btree_index_moveto(cur, bt, idx_key: &UnpackedRecord, res: &mut i32) -> i32
//!   btree_moveto(cur, bt, key: Option<&[u8]>, n_key: i64, bias: i32, res: &mut i32) -> i32
//!   access_payload(cur, bt, offset: u32, amt: u32, buf: &mut [u8], e_op: i32) -> i32
//!   btree_begin_trans(p: &mut Btree, wrflag: i32, schema_version: Option<&mut i32>) -> i32
//!   query_shared_cache_table_lock(p: &mut Btree, i_tab: u32, e_lock: u8) -> i32
//!   set_shared_cache_table_lock(p: &mut Btree, i_table: u32, e_lock: u8) -> i32
//! Definidas aqui por não existirem nos chunks anteriores: `CellArray` e `populate_cell_cache`
//! (o trecho final do chunk 017 que o agente anterior não deve duplicar).

use std::any::Any;
use std::rc::Rc;

use crate::btree::*;
use crate::btree_cursor::*;
use crate::btree_cursor::BtDb;
use crate::btree_types::{BtCursor, BtShared, Btree, BtreePayload, CellInfo, IntegrityCk};
use crate::consts::{
    BTALLOC_EXACT, BTCF_INCRBLOB, BTCF_MULTIPLE, BTCF_VALID_NKEY, BTCF_VALID_OVFL,
    BTCF_WRITE_FLAG, BTREE_APPEND, BTREE_BULKLOAD, BTREE_DATA_VERSION, BTREE_INCR_VACUUM,
    BTREE_INTKEY, BTREE_LARGEST_ROOT_PAGE, BTREE_PREFORMAT, BTREE_SAVEPOSITION, BTREE_SINGLE,
    BTS_NO_WAL, CURSOR_INVALID, CURSOR_REQUIRESEEK, CURSOR_SKIPNEXT, CURSOR_VALID,
    LARGEST_INT64, PAGER_GET_READONLY, PTF_INTKEY, PTF_LEAF, PTF_LEAFDATA, PTF_ZERODATA,
    PTRMAP_BTREE, PTRMAP_FREEPAGE, PTRMAP_OVERFLOW1, PTRMAP_OVERFLOW2, PTRMAP_ROOTPAGE, READ_LOCK,
    SCHEMA_ROOT, SQLITE_ABORT, SQLITE_CELL_SIZE_CK, SQLITE_CORRUPT_BKPT, SQLITE_EMPTY,
    SQLITE_INTERRUPT, SQLITE_IOERR_NOMEM, SQLITE_LOCKED, SQLITE_MAX_LENGTH, SQLITE_NOMEM,
    SQLITE_OK, SQLITE_PRINTF_INTERNAL, SQLITE_READONLY, TRANS_NONE,
};
use crate::mem::{mem_set_int64, KeyInfo, Mem, UnpackedRecord};
use crate::pcache::PgId;
use crate::printf::{PrintfArg, StrAccum};
use crate::util::{get2byte, get4byte, put2byte, put4byte, put_varint};

/// `NB`: número de páginas irmãs de cada lado mais a própria (`NN*2+1`).
const NB: usize = 3;

// ---------------------------------------------------------------------------
// Auxiliares de leitura segura (o C lê além do buffer em página corrompida)
// ---------------------------------------------------------------------------

/// `get2byte(&data[pos])`, 0 fora do buffer.
#[inline]
fn g2(data: &[u8], pos: usize) -> u32 {
    match data.get(pos..pos + 2) {
        Some(s) => get2byte(s),
        None => 0,
    }
}

/// `get4byte(&data[pos])`, 0 fora do buffer.
#[inline]
fn g4(data: &[u8], pos: usize) -> u32 {
    match data.get(pos..pos + 4) {
        Some(s) => get4byte(s),
        None => 0,
    }
}

/// `get2byteNotZero(X)`: `(((X)-1)&0xffff)+1`.
#[inline]
fn get2byte_not_zero(v: u32) -> u32 {
    (v.wrapping_sub(1) & 0xffff) + 1
}

/// `memmove` dentro de `data`; falso se algum intervalo sai do buffer.
fn mv(data: &mut [u8], dst: usize, src: usize, n: usize) -> bool {
    let len = data.len();
    match (src.checked_add(n), dst.checked_add(n)) {
        (Some(se), Some(de)) if se <= len && de <= len => {
            data.copy_within(src..se, dst);
            true
        }
        _ => false,
    }
}

/// Grava os 16 bits baixos de `v` em `data[pos..pos+2]`; falso fora do buffer.
fn p2(data: &mut [u8], pos: usize, v: u32) -> bool {
    match data.get_mut(pos..pos + 2) {
        Some(s) => {
            put2byte(s, v);
            true
        }
        None => false,
    }
}

/// Grava `v` em `data[pos..pos+4]`; falso fora do buffer.
fn p4(data: &mut [u8], pos: usize, v: u32) -> bool {
    match data.get_mut(pos..pos + 4) {
        Some(s) => {
            put4byte(s, v);
            true
        }
        None => false,
    }
}

/// `findCell(P, I)`: offset da célula `i` da página (`maskPage & get2byteAligned(aCellIdx+2*i)`).
fn find_cell(bt: &BtShared, pg: PgId, i: usize) -> usize {
    let p = bt.pager.page_extra(pg);
    let data = bt.pager.page_data(pg);
    (p.mask_page as u32 & g2(data, p.a_cell_idx + 2 * i)) as usize
}

// ---------------------------------------------------------------------------
// CellArray
// ---------------------------------------------------------------------------

/// Origem de um ponteiro de célula.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellSrc {
    /// Os bytes de uma página do pager.
    Page(PgId),
    /// Um buffer próprio do `CellArray` (índice em `bufs`).
    Buf(usize),
}

/// Um `u8*` do C que aponta para uma célula (ou para o fim de uma página).
#[derive(Clone, Copy)]
struct CellPtr {
    src: CellSrc,
    off: usize,
}

impl CellPtr {
    /// O ponteiro nulo.
    const NONE: CellPtr = CellPtr { src: CellSrc::Buf(usize::MAX), off: 0 };
}

/// `SQLITE_OVERFLOW(end, cell, cell+sz)` com ponteiros: a célula atravessa o ponto `end`.
fn straddles(cell: CellPtr, sz: usize, end: CellPtr) -> bool {
    cell.src == end.src && cell.off < end.off && cell.off + sz > end.off
}

/// `struct CellArray` do btree.c (definido no fim do chunk 017).
struct CellArray {
    /// Número de células em `ap_cell`.
    n_cell: i32,
    /// Página de referência (para `xCellSize`).
    p_ref: PgId,
    /// Todas as células em balanceamento.
    ap_cell: Vec<CellPtr>,
    /// Tamanho local de cada célula (0 se ainda não calculado).
    sz_cell: Vec<u16>,
    /// Valores de `MemPage.aDataEnd`.
    ap_end: [CellPtr; NB * 2],
    /// Índice em que se passa para o próximo `ap_end`.
    ix_nx: [i32; NB * 2],
    /// Buffers próprios: `bufs[0]` é o `aSpace1`, os demais são células em overflow.
    bufs: Vec<Vec<u8>>,
}

impl CellArray {
    /// Os bytes a partir do ponteiro de célula `p` até o fim do buffer de origem.
    fn slice<'a>(&'a self, bt: &'a BtShared, p: CellPtr) -> &'a [u8] {
        let whole: &[u8] = match p.src {
            CellSrc::Page(pg) => bt.pager.page_data(pg),
            CellSrc::Buf(i) => match self.bufs.get(i) {
                Some(b) => b,
                None => return &[],
            },
        };
        whole.get(p.off..).unwrap_or(&[])
    }

    /// Copia `sz` bytes da célula `p` para `out`; falso se saem do buffer de origem.
    fn copy_cell(&self, bt: &BtShared, p: CellPtr, sz: usize, out: &mut Vec<u8>) -> bool {
        match self.slice(bt, p).get(..sz) {
            Some(s) => {
                out.clear();
                out.extend_from_slice(s);
                true
            }
            None => false,
        }
    }

    /// `cachedCellSize` (com `computeCellSize`): `None` se `n` está fora do vetor.
    fn cached_cell_size(&mut self, bt: &BtShared, n: i32) -> Option<u16> {
        if n < 0 || n >= self.n_cell || n as usize >= self.sz_cell.len() {
            return None;
        }
        let n = n as usize;
        if self.sz_cell[n] == 0 {
            let cp = self.ap_cell[n];
            let sz = {
                let s = self.slice(bt, cp);
                x_cell_size(bt.pager.page_extra(self.p_ref), s, 0)
            };
            self.sz_cell[n] = sz;
        }
        Some(self.sz_cell[n])
    }

    /// `populateCellCache`: garante o tamanho das células `idx..idx+n`; falso se saem do vetor.
    fn populate_cell_cache(&mut self, bt: &BtShared, idx: i32, n: i32) -> bool {
        if idx < 0 || n < 0 || idx + n > self.n_cell || (idx + n) as usize > self.sz_cell.len() {
            return false;
        }
        for i in idx..idx + n {
            // `assert( szCell[idx]==xCellSize )` no ramo já calculado é só de depuração.
            let _ = self.cached_cell_size(bt, i);
        }
        true
    }
}

/// Acrescenta uma célula ao `CellArray` em carga; falso se o vetor está cheio.
fn push_cell(b: &mut CellArray, cp: CellPtr) -> bool {
    let n = b.n_cell as usize;
    if n >= b.ap_cell.len() {
        return false;
    }
    b.ap_cell[n] = cp;
    b.n_cell += 1;
    true
}

// ---------------------------------------------------------------------------
// rebuildPage, pageInsertArray, pageFreeArray, editPage
// ---------------------------------------------------------------------------

/// `rebuildPage`: substitui o conteúdo da página `pg` pelas células `iFirst..iFirst+nCell` do
/// array. O `nFree` fica inválido: quem chama acerta.
fn rebuild_page(bt: &mut BtShared, c: &CellArray, i_first: i32, n_cell: i32, pg: PgId) -> i32 {
    if i_first < 0 || n_cell <= 0 || i_first + n_cell > c.n_cell {
        return SQLITE_CORRUPT_BKPT;
    }
    let usable = bt.usable_size as usize;
    let (hdr, cell_idx) = {
        let p = bt.pager.page_extra(pg);
        (p.hdr_offset as usize, p.a_cell_idx)
    };
    let mut i = i_first as usize;
    let i_end = i + n_cell as usize;
    let mut j = g2(bt.pager.page_data(pg), hdr + 5) as usize;
    if j > usable {
        j = 0;
    }
    // O `pTmp` do C: cópia do conteúdo da página, lido no mesmo offset.
    let tmp: Vec<u8> = match bt.pager.page_data(pg).get(j..usable) {
        Some(s) => s.to_vec(),
        None => return SQLITE_CORRUPT_BKPT,
    };

    let mut k = 0usize;
    while k < NB * 2 && c.ix_nx[k] <= i as i32 {
        k += 1;
    }
    let mut p_src_end = c.ap_end[k.min(NB * 2 - 1)];

    let mut p_data = usable;
    let mut p_cellptr = cell_idx;
    let mut scratch: Vec<u8> = Vec::new();
    loop {
        let cp = c.ap_cell[i];
        let sz = c.sz_cell[i] as usize;
        let within = cp.src == CellSrc::Page(pg) && cp.off >= j && cp.off < usable;
        if within {
            if cp.off + sz > usable {
                return SQLITE_CORRUPT_BKPT;
            }
            scratch.clear();
            scratch.extend_from_slice(&tmp[cp.off - j..cp.off - j + sz]);
        } else {
            if straddles(cp, sz, p_src_end) {
                return SQLITE_CORRUPT_BKPT;
            }
            if !c.copy_cell(bt, cp, sz, &mut scratch) {
                return SQLITE_CORRUPT_BKPT;
            }
        }

        if sz > p_data {
            return SQLITE_CORRUPT_BKPT;
        }
        p_data -= sz;
        {
            let data = bt.pager.page_data_mut(pg);
            if !p2(data, p_cellptr, p_data as u32) {
                return SQLITE_CORRUPT_BKPT;
            }
            p_cellptr += 2;
            if p_data < p_cellptr {
                return SQLITE_CORRUPT_BKPT;
            }
            data[p_data..p_data + sz].copy_from_slice(&scratch);
        }
        i += 1;
        if i >= i_end {
            break;
        }
        if c.ix_nx[k] <= i as i32 {
            k += 1;
            p_src_end = c.ap_end[k.min(NB * 2 - 1)];
        }
    }

    // O `nFree` está errado agora; quem chama acerta.
    let (page, data) = bt.pager.page_parts(pg);
    page.n_cell = n_cell as u16;
    page.n_overflow = 0;
    if hdr + 8 > data.len() {
        return SQLITE_CORRUPT_BKPT;
    }
    put2byte(&mut data[hdr + 1..], 0);
    put2byte(&mut data[hdr + 3..], page.n_cell as u32);
    put2byte(&mut data[hdr + 5..], p_data as u32);
    data[hdr + 7] = 0x00;
    SQLITE_OK
}

/// `pageInsertArray`: tenta acrescentar à página as células do array; devolve 1 se a página
/// precisa ser desfragmentada antes de caberem, 0 se deu certo. `p_data` aponta o início da
/// área de conteúdo (offset) e é atualizado.
#[allow(clippy::too_many_arguments)]
fn page_insert_array(
    bt: &mut BtShared,
    c: &CellArray,
    pg: PgId,
    p_begin: usize,
    p_data: &mut usize,
    mut p_cellptr: usize,
    i_first: i32,
    n_cell: i32,
) -> i32 {
    let mut i = i_first;
    let i_end = i_first + n_cell;
    if i_end <= i_first {
        return 0;
    }
    if i_first < 0 || i_end > c.n_cell || i_end as usize > c.sz_cell.len() {
        return 1;
    }
    let mut pd = *p_data;
    let mut k = 0usize;
    while k < NB * 2 && c.ix_nx[k] <= i {
        k += 1;
    }
    let mut p_end = c.ap_end[k.min(NB * 2 - 1)];
    let mut scratch: Vec<u8> = Vec::new();
    loop {
        let sz = c.sz_cell[i as usize] as usize;
        let free_empty = {
            let d = bt.pager.page_data(pg);
            d.get(1).copied() == Some(0) && d.get(2).copied() == Some(0)
        };
        let mut rc2 = SQLITE_OK;
        let found = if free_empty {
            None
        } else {
            let (page, data) = bt.pager.page_parts(pg);
            page_find_slot(&*page, data, sz as i32, &mut rc2)
        };
        let slot = match found {
            Some(s) => s,
            None => {
                if (pd as isize - p_begin as isize) < sz as isize {
                    return 1;
                }
                pd -= sz;
                pd
            }
        };
        let cp = c.ap_cell[i as usize];
        if straddles(cp, sz, p_end) {
            return 1;
        }
        if !c.copy_cell(bt, cp, sz, &mut scratch) {
            return 1;
        }
        {
            let data = bt.pager.page_data_mut(pg);
            match data.get_mut(slot..slot + sz) {
                Some(d) => d.copy_from_slice(&scratch),
                None => return 1,
            }
            if !p2(data, p_cellptr, slot as u32) {
                return 1;
            }
        }
        p_cellptr += 2;
        i += 1;
        if i >= i_end {
            break;
        }
        if c.ix_nx[k] <= i {
            k += 1;
            p_end = c.ap_end[k.min(NB * 2 - 1)];
        }
    }
    *p_data = pd;
    0
}

/// `pageFreeArray`: devolve à lista livre da página o espaço das células do array que estão
/// no corpo dela. Devolve quantas células foram liberadas.
fn page_free_array(bt: &mut BtShared, c: &CellArray, pg: PgId, i_first: i32, n_cell: i32) -> i32 {
    let usable = bt.usable_size as usize;
    let p_start = {
        let p = bt.pager.page_extra(pg);
        p.hdr_offset as usize + 8 + p.child_ptr_size as usize
    };
    let p_end = usable;
    let mut n_ret = 0;
    let i_end = i_first + n_cell;
    let mut n_free = 0usize;
    let mut a_ofst = [0i32; 10];
    let mut a_after = [0i32; 10];
    if i_first < 0 || i_end > c.n_cell {
        return 0;
    }
    for i in i_first..i_end {
        let cp = c.ap_cell[i as usize];
        if cp.src == CellSrc::Page(pg) && cp.off >= p_start && cp.off < p_end {
            let sz = c.sz_cell[i as usize] as i32;
            let i_ofst = (cp.off as u16) as i32;
            let i_after = i_ofst + sz;
            let mut j = 0usize;
            while j < n_free {
                if a_ofst[j] == i_after {
                    a_ofst[j] = i_ofst;
                    break;
                } else if a_after[j] == i_ofst {
                    a_after[j] = i_after;
                    break;
                }
                j += 1;
            }
            if j >= n_free {
                if n_free >= a_ofst.len() {
                    for j in 0..n_free {
                        let _ = free_space(bt, pg, a_ofst[j] as u16, (a_after[j] - a_ofst[j]) as u16);
                    }
                    n_free = 0;
                }
                a_ofst[n_free] = i_ofst;
                a_after[n_free] = i_after;
                if i_after as usize > p_end {
                    return 0;
                }
                n_free += 1;
            }
            n_ret += 1;
        }
    }
    for j in 0..n_free {
        let _ = free_space(bt, pg, a_ofst[j] as u16, (a_after[j] - a_ofst[j]) as u16);
    }
    n_ret
}

/// `editPage`: ajusta a página para conter as células `iNew..iNew+nNew` do array (ela tem hoje
/// as que começam em `iOld`). O `nFree` fica inválido.
fn edit_page(
    bt: &mut BtShared,
    c: &mut CellArray,
    pg: PgId,
    i_old: i32,
    i_new: i32,
    n_new: i32,
) -> i32 {
    let (hdr, cell_idx, mut n_cell, n_overflow, data_end) = {
        let p = bt.pager.page_extra(pg);
        (p.hdr_offset as usize, p.a_cell_idx, p.n_cell as i32, p.n_overflow as i32, p.a_data_end)
    };
    let p_begin = cell_idx + (n_new.max(0) as usize) * 2;
    let i_old_end = i_old + n_cell + n_overflow;
    let i_new_end = i_new + n_new;
    let mut p_data: usize;

    'fail: {
        // Retira células do começo e do fim da página.
        if i_old < i_new {
            let n_shift = page_free_array(bt, c, pg, i_old, i_new - i_old);
            if n_shift > n_cell {
                return SQLITE_CORRUPT_BKPT;
            }
            let data = bt.pager.page_data_mut(pg);
            if !mv(data, cell_idx, cell_idx + n_shift as usize * 2, n_cell as usize * 2) {
                return SQLITE_CORRUPT_BKPT;
            }
            n_cell -= n_shift;
        }
        if i_new_end < i_old_end {
            let n_tail = page_free_array(bt, c, pg, i_new_end, i_old_end - i_new_end);
            n_cell -= n_tail;
            if n_cell < 0 {
                return SQLITE_CORRUPT_BKPT;
            }
        }

        p_data = g2(bt.pager.page_data(pg), hdr + 5) as usize;
        if p_data < p_begin {
            break 'fail;
        }
        if p_data > data_end {
            break 'fail;
        }

        // Acrescenta células ao começo da página.
        if i_new < i_old {
            let n_add = n_new.min(i_old - i_new);
            let cellptr = cell_idx;
            {
                let data = bt.pager.page_data_mut(pg);
                if !mv(data, cellptr + n_add as usize * 2, cellptr, n_cell as usize * 2) {
                    break 'fail;
                }
            }
            if page_insert_array(bt, c, pg, p_begin, &mut p_data, cellptr, i_new, n_add) != 0 {
                break 'fail;
            }
            n_cell += n_add;
        }

        // Acrescenta as células em overflow.
        for i in 0..n_overflow as usize {
            let ai = bt.pager.page_extra(pg).ai_ovfl[i] as i32;
            let i_cell = (i_old + ai) - i_new;
            if i_cell >= 0 && i_cell < n_new {
                let cellptr = cell_idx + i_cell as usize * 2;
                if n_cell > i_cell {
                    let data = bt.pager.page_data_mut(pg);
                    if !mv(data, cellptr + 2, cellptr, (n_cell - i_cell) as usize * 2) {
                        break 'fail;
                    }
                }
                n_cell += 1;
                if c.cached_cell_size(bt, i_cell + i_new).is_none() {
                    break 'fail;
                }
                if page_insert_array(bt, c, pg, p_begin, &mut p_data, cellptr, i_cell + i_new, 1) != 0 {
                    break 'fail;
                }
            }
        }

        // Acrescenta células ao fim da página.
        let cellptr = cell_idx + n_cell as usize * 2;
        if page_insert_array(bt, c, pg, p_begin, &mut p_data, cellptr, i_new + n_cell, n_new - n_cell)
            != 0
        {
            break 'fail;
        }

        let (page, data) = bt.pager.page_parts(pg);
        page.n_cell = n_new as u16;
        page.n_overflow = 0;
        if hdr + 6 > data.len() {
            return SQLITE_CORRUPT_BKPT;
        }
        put2byte(&mut data[hdr + 3..], page.n_cell as u32);
        put2byte(&mut data[hdr + 5..], p_data as u32);
        return SQLITE_OK;
    }

    // Não deu para editar a página: reconstrói do zero.
    if n_new < 1 {
        return SQLITE_CORRUPT_BKPT;
    }
    if !c.populate_cell_cache(bt, i_new, n_new) {
        return SQLITE_CORRUPT_BKPT;
    }
    rebuild_page(bt, c, i_new, n_new, pg)
}

// ---------------------------------------------------------------------------
// balance_quick, copyNodeContent
// ---------------------------------------------------------------------------

/// `balance_quick`: o caso comum de inserção na extremidade direita da árvore. `pg` é a folha
/// mais à direita e tem exatamente uma célula em overflow, também a mais à direita.
fn balance_quick(bt: &mut BtShared, parent: PgId, pg: PgId) -> i32 {
    if bt.pager.page_extra(pg).n_cell == 0 {
        return SQLITE_CORRUPT_BKPT; // dbfuzz001.test
    }
    let p_new = match allocate_btree_page(bt, 0, 0) {
        Ok(p) => p,
        Err(rc) => return rc,
    };
    let pgno_new = bt.pager.page_pgno(p_new);

    let cell: Vec<u8> = bt.pager.page_extra(pg).ap_ovfl[0].clone().unwrap_or_default();
    let sz_cell = x_cell_size(bt.pager.page_extra(pg), &cell, 0);
    zero_page(bt, p_new, PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF);
    let data_end = bt.pager.page_extra(pg).a_data_end;
    let mut b = CellArray {
        n_cell: 1,
        p_ref: pg,
        ap_cell: vec![CellPtr { src: CellSrc::Buf(0), off: 0 }],
        sz_cell: vec![sz_cell],
        ap_end: [CellPtr::NONE; NB * 2],
        ix_nx: [0; NB * 2],
        bufs: vec![cell],
    };
    b.ap_end[0] = CellPtr { src: CellSrc::Page(pg), off: data_end };
    b.ix_nx[0] = 2;
    let mut rc = rebuild_page(bt, &b, 0, 1, p_new);
    if rc != SQLITE_OK {
        bt.pager.unref_not_null(p_new);
        return rc;
    }
    {
        let usable = bt.usable_size as i32;
        let np = bt.pager.page_extra_mut(p_new);
        np.n_free = usable - np.cell_offset as i32 - 2 - sz_cell as i32;
    }

    // Em banco auto-vacuum, atualiza o mapa de ponteiros para a página nova e para o ponteiro
    // de overflow da célula.
    if bt.auto_vacuum != 0 {
        let parent_pgno = bt.pager.page_extra(parent).pgno;
        ptrmap_put(bt, pgno_new, PTRMAP_BTREE, parent_pgno, &mut rc);
        let min_local = bt.pager.page_extra(p_new).min_local;
        if sz_cell > min_local {
            ptrmap_put_ovfl_ptr(bt, p_new, None, &b.bufs[0], &mut rc);
        }
    }

    // Cria a célula divisora: o número da página `pg` mais a maior chave dela.
    let mut space = [0u8; 13];
    let mut p_out = 4usize;
    {
        let off = {
            let n = bt.pager.page_extra(pg).n_cell as usize;
            find_cell(bt, pg, n - 1)
        };
        let data = bt.pager.page_data(pg);
        let rd = |p: usize| data.get(p).copied().unwrap_or(0);
        let mut p = off;
        let mut stop = p + 9;
        loop {
            let v = rd(p);
            p += 1;
            if !((v & 0x80) != 0 && p < stop) {
                break;
            }
        }
        stop = p + 9;
        loop {
            let v = rd(p);
            space[p_out] = v;
            p_out += 1;
            p += 1;
            if !((v & 0x80) != 0 && p < stop) {
                break;
            }
        }
    }

    // Insere o divisor no pai.
    if rc == SQLITE_OK {
        let n_cell_parent = bt.pager.page_extra(parent).n_cell as i32;
        let pgno_page = bt.pager.page_extra(pg).pgno;
        rc = insert_cell(bt, parent, n_cell_parent, &space[..p_out], p_out as i32, pgno_page);
    }

    // O ponteiro direito do pai passa a apontar para a página nova.
    let hdr_p = bt.pager.page_extra(parent).hdr_offset as usize;
    if !p4(bt.pager.page_data_mut(parent), hdr_p + 8, pgno_new) && rc == SQLITE_OK {
        rc = SQLITE_CORRUPT_BKPT;
    }

    bt.pager.unref_not_null(p_new);
    rc
}

/// `copyNodeContent`: copia o conteúdo do nó `from` para `to` e reinicializa `to`.
fn copy_node_content(bt: &mut BtShared, from: PgId, to: PgId, rc: &mut i32) {
    if *rc != SQLITE_OK {
        return;
    }
    let usable = bt.usable_size as usize;
    let (i_from_hdr, cell_offset, n_cell) = {
        let p = bt.pager.page_extra(from);
        (p.hdr_offset as usize, p.cell_offset as usize, p.n_cell as usize)
    };
    let i_to_hdr: usize = if bt.pager.page_extra(to).pgno == 1 { 100 } else { 0 };

    // Copia o conteúdo do nó `from` para `to`.
    let (tail, head) = {
        let a_from = bt.pager.page_data(from);
        let i_data = g2(a_from, i_from_hdr + 5) as usize;
        let tail = match a_from.get(i_data..usable) {
            Some(s) => (i_data, s.to_vec()),
            None => {
                *rc = SQLITE_CORRUPT_BKPT;
                return;
            }
        };
        let head = match a_from.get(i_from_hdr..i_from_hdr + cell_offset + 2 * n_cell) {
            Some(s) => s.to_vec(),
            None => {
                *rc = SQLITE_CORRUPT_BKPT;
                return;
            }
        };
        (tail, head)
    };
    {
        let a_to = bt.pager.page_data_mut(to);
        match a_to.get_mut(tail.0..tail.0 + tail.1.len()) {
            Some(d) => d.copy_from_slice(&tail.1),
            None => {
                *rc = SQLITE_CORRUPT_BKPT;
                return;
            }
        }
        match a_to.get_mut(i_to_hdr..i_to_hdr + head.len()) {
            Some(d) => d.copy_from_slice(&head),
            None => {
                *rc = SQLITE_CORRUPT_BKPT;
                return;
            }
        }
    }

    // Reinicializa `to` para que o MemPage case com o conteúdo novo.
    bt.pager.page_extra_mut(to).is_init = false;
    let mut r = btree_init_page(bt, to);
    if r == SQLITE_OK {
        r = btree_compute_free_space(bt, to);
    }
    if r != SQLITE_OK {
        *rc = r;
        return;
    }

    // Em banco auto-vacuum, atualiza as entradas do mapa de ponteiros dos filhos.
    if bt.auto_vacuum != 0 {
        *rc = set_child_ptrmaps(bt, to);
    }
}

// ---------------------------------------------------------------------------
// balance_nonroot
// ---------------------------------------------------------------------------

/// Páginas seguradas por `balance_nonroot` (a limpeza as solta em qualquer saída).
#[derive(Default)]
struct BalanceState {
    ap_old: [Option<PgId>; NB],
    ap_new: [Option<PgId>; NB + 2],
    n_old: usize,
    n_new: usize,
}

/// `balance_nonroot`: redistribui as células da `i_parent_idx`-ésima filha de `parent` e de até
/// duas irmãs. Em falha o banco pode ficar corrompido: quem chama faz rollback.
pub(crate) fn balance_nonroot(
    bt: &mut BtShared,
    parent: PgId,
    i_parent_idx: i32,
    is_root: bool,
    b_bulk: i32,
) -> i32 {
    let mut st = BalanceState::default();
    let rc = balance_nonroot_body(bt, parent, i_parent_idx, is_root, b_bulk, &mut st);
    // balance_cleanup
    for i in 0..st.n_old {
        bt.pager.unref(st.ap_old[i]);
    }
    for i in 0..st.n_new {
        bt.pager.unref(st.ap_new[i]);
    }
    rc
}

fn balance_nonroot_body(
    bt: &mut BtShared,
    parent: PgId,
    i_parent_idx: i32,
    is_root: bool,
    b_bulk: i32,
    st: &mut BalanceState,
) -> i32 {
    let mut n_max_cells: i32 = 0;
    let mut n_new: usize = 0;
    let mut sz_new = [0i32; NB + 3];
    let mut cnt_new = [0i32; NB + 3];
    let mut cnt_old = [0i32; NB + 3];
    let mut ab_done = [0u8; NB + 3];
    let mut a_pgno = [0u32; NB + 3];
    let mut ap_div: [Vec<u8>; NB - 1] = Default::default();
    let mut rc = SQLITE_OK;

    // No máximo uma célula em overflow no pai, e se existir é a de índice `iParentIdx`.
    let (p_n_overflow, p_n_cell, hdr_p) = {
        let p = bt.pager.page_extra(parent);
        (p.n_overflow as i32, p.n_cell as i32, p.hdr_offset as usize)
    };

    // Localiza as páginas irmãs e as células do pai que as dividem. O laço também retira os
    // divisores do pai.
    let mut i = p_n_overflow + p_n_cell;
    let nx_div: i32;
    if i < 2 {
        nx_div = 0;
    } else {
        nx_div = if i_parent_idx == 0 {
            0
        } else if i_parent_idx == i {
            i - 2 + b_bulk
        } else {
            i_parent_idx - 1
        };
        i = 2 - b_bulk;
    }
    let n_old = (i + 1) as usize;
    st.n_old = n_old;
    let p_right: usize = if (i + nx_div - p_n_overflow) == p_n_cell {
        hdr_p + 8
    } else {
        let idx = i + nx_div - p_n_overflow;
        if idx < 0 {
            return SQLITE_CORRUPT_BKPT;
        }
        find_cell(bt, parent, idx as usize)
    };
    let mut pgno: u32 = g4(bt.pager.page_data(parent), p_right);
    loop {
        if rc == SQLITE_OK {
            match get_and_init_page(bt, pgno, 0) {
                Ok(p) => st.ap_old[i as usize] = Some(p),
                Err(e) => rc = e,
            }
        }
        if rc != SQLITE_OK {
            return rc;
        }
        let pgi = match st.ap_old[i as usize] {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        if bt.pager.page_extra(pgi).n_free < 0 {
            rc = btree_compute_free_space(bt, pgi);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        n_max_cells += bt.pager.page_extra(pgi).n_cell as i32 + 4;
        let was = i;
        i -= 1;
        if was == 0 {
            break;
        }

        let iu = i as usize;
        let (p_nov, p_ai0) = {
            let p = bt.pager.page_extra(parent);
            (p.n_overflow as i32, p.ai_ovfl[0] as i32)
        };
        if p_nov != 0 && i + nx_div == p_ai0 {
            let cell: Vec<u8> = bt.pager.page_extra(parent).ap_ovfl[0].clone().unwrap_or_default();
            pgno = g4(&cell, 0);
            sz_new[iu] = x_cell_size(bt.pager.page_extra(parent), &cell, 0) as i32;
            ap_div[iu] = cell;
            bt.pager.page_extra_mut(parent).n_overflow = 0;
        } else {
            let idx = i + nx_div - p_nov;
            if idx < 0 {
                return SQLITE_CORRUPT_BKPT;
            }
            let off = find_cell(bt, parent, idx as usize);
            let (sz, copy, pg_div) = {
                let data = bt.pager.page_data(parent);
                let slice = data.get(off..).unwrap_or(&[]);
                let sz = x_cell_size(bt.pager.page_extra(parent), slice, 0);
                let copy = slice.get(..sz as usize).unwrap_or(slice).to_vec();
                (sz, copy, g4(slice, 0))
            };
            pgno = pg_div;
            sz_new[iu] = sz as i32;
            ap_div[iu] = copy;
            // `dropCell` só mexe nos 4 primeiros bytes (ou zera a célula em secure-delete);
            // o corpo do divisor já está copiado em `ap_div`.
            drop_cell(bt, parent, idx, sz_new[iu], &mut rc);
        }
    }

    // Múltiplo de 4 (alinhamento de 8 bytes no C).
    n_max_cells = (n_max_cells + 3) & !3;

    // Carrega os ponteiros de todas as células das irmãs e dos divisores. Copia os divisores
    // para `aSpace1` (`bufs[0]`).
    let n_max = n_max_cells as usize;
    let p_ref = match st.ap_old[0] {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    let mut b = CellArray {
        n_cell: 0,
        p_ref,
        ap_cell: vec![CellPtr::NONE; n_max],
        sz_cell: vec![0u16; n_max],
        ap_end: [CellPtr::NONE; NB * 2],
        ix_nx: [0; NB * 2],
        bufs: vec![vec![0u8; bt.page_size as usize]],
    };
    macro_rules! csz {
        ($n:expr) => {
            match b.cached_cell_size(bt, $n) {
                Some(v) => v as i32,
                None => return SQLITE_CORRUPT_BKPT,
            }
        };
    }
    let (ref_leaf, ref_int_key_leaf) = {
        let p = bt.pager.page_extra(p_ref);
        (p.leaf, p.int_key_leaf)
    };
    let leaf_correction: u16 = if ref_leaf { 4 } else { 0 };
    let leaf_data: bool = ref_int_key_leaf;
    let ref_byte0 = bt.pager.page_data(p_ref).first().copied().unwrap_or(0);
    let mut i_space1: usize = 0;
    for i in 0..n_old {
        let p_old = match st.ap_old[i] {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        let (mut limit, mask_page, cell_offset, n_ovfl, n_cell_p, old_leaf, ai0) = {
            let p = bt.pager.page_extra(p_old);
            (
                p.n_cell as i32,
                p.mask_page as u32,
                p.cell_offset as usize,
                p.n_overflow as usize,
                p.n_cell as usize,
                p.leaf,
                p.ai_ovfl[0] as i32,
            )
        };
        let mut pi_cell = cell_offset;

        // Todas as irmãs têm de ser do mesmo tipo.
        if bt.pager.page_data(p_old).first().copied().unwrap_or(0) != ref_byte0 {
            return SQLITE_CORRUPT_BKPT;
        }

        // Carrega as células de `pOld`, com as de overflow no lugar certo.
        if n_ovfl > 0 {
            if limit < ai0 {
                return SQLITE_CORRUPT_BKPT;
            }
            limit = ai0;
            for _ in 0..limit {
                let off = (mask_page & g2(bt.pager.page_data(p_old), pi_cell)) as usize;
                if !push_cell(&mut b, CellPtr { src: CellSrc::Page(p_old), off }) {
                    return SQLITE_CORRUPT_BKPT;
                }
                pi_cell += 2;
            }
            for k in 0..n_ovfl {
                let bytes: Vec<u8> =
                    bt.pager.page_extra(p_old).ap_ovfl[k].clone().unwrap_or_default();
                let bi = b.bufs.len();
                b.bufs.push(bytes);
                if !push_cell(&mut b, CellPtr { src: CellSrc::Buf(bi), off: 0 }) {
                    return SQLITE_CORRUPT_BKPT;
                }
            }
        }
        let pi_end = cell_offset + 2 * n_cell_p;
        while pi_cell < pi_end {
            let off = (mask_page & g2(bt.pager.page_data(p_old), pi_cell)) as usize;
            if !push_cell(&mut b, CellPtr { src: CellSrc::Page(p_old), off }) {
                return SQLITE_CORRUPT_BKPT;
            }
            pi_cell += 2;
        }

        cnt_old[i] = b.n_cell;
        if i < n_old - 1 && !leaf_data {
            let sz = sz_new[i] as u16;
            let nc = b.n_cell as usize;
            if nc >= n_max {
                return SQLITE_CORRUPT_BKPT;
            }
            b.sz_cell[nc] = sz;
            let pt = i_space1;
            i_space1 += sz as usize;
            if i_space1 > b.bufs[0].len() {
                return SQLITE_CORRUPT_BKPT;
            }
            let n_copy = ap_div[i].len().min(sz as usize);
            b.bufs[0][pt..pt + n_copy].copy_from_slice(&ap_div[i][..n_copy]);
            b.ap_cell[nc] = CellPtr { src: CellSrc::Buf(0), off: pt + leaf_correction as usize };
            b.sz_cell[nc] = b.sz_cell[nc].wrapping_sub(leaf_correction);
            if !old_leaf {
                // O ponteiro direito da filha `pOld` vira o ponteiro esquerdo do divisor.
                let right: [u8; 4] = {
                    let d = bt.pager.page_data(p_old);
                    match d.get(8..12) {
                        Some(s) => [s[0], s[1], s[2], s[3]],
                        None => return SQLITE_CORRUPT_BKPT,
                    }
                };
                let at = pt + leaf_correction as usize;
                match b.bufs[0].get_mut(at..at + 4) {
                    Some(d) => d.copy_from_slice(&right),
                    None => return SQLITE_CORRUPT_BKPT,
                }
            } else {
                while b.sz_cell[nc] < 4 {
                    // Nenhuma célula com menos de 4 bytes: completa com 0x00.
                    if i_space1 >= b.bufs[0].len() {
                        return SQLITE_CORRUPT_BKPT;
                    }
                    b.bufs[0][i_space1] = 0x00;
                    i_space1 += 1;
                    b.sz_cell[nc] += 1;
                }
            }
            b.n_cell += 1;
        }
    }

    // Quantas páginas são precisas para as `b.n_cell` células: `k`, e para cada página
    // `szNew[i]` (espaço usado) e `cntNew[i]` (índice da célula à direita da i-ésima página).
    let usable = bt.usable_size as i32;
    let usable_space: i32 = usable - 12 + leaf_correction as i32;
    let parent_end = CellPtr {
        src: CellSrc::Page(parent),
        off: bt.pager.page_extra(parent).a_data_end,
    };
    let mut k: usize = 0;
    for i in 0..n_old {
        let p = match st.ap_old[i] {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        b.ap_end[k] = CellPtr { src: CellSrc::Page(p), off: bt.pager.page_extra(p).a_data_end };
        b.ix_nx[k] = cnt_old[i];
        if k != 0 && b.ix_nx[k] == b.ix_nx[k - 1] {
            k -= 1; // omite a entrada de filha sem células
        }
        if !leaf_data {
            k += 1;
            if k >= NB * 2 {
                return SQLITE_CORRUPT_BKPT;
            }
            b.ap_end[k] = parent_end;
            b.ix_nx[k] = cnt_old[i] + 1;
        }
        let (n_free, n_ovfl) = {
            let pg = bt.pager.page_extra(p);
            (pg.n_free, pg.n_overflow as usize)
        };
        sz_new[i] = usable_space - n_free;
        for j in 0..n_ovfl {
            let cell: Vec<u8> = bt.pager.page_extra(p).ap_ovfl[j].clone().unwrap_or_default();
            sz_new[i] += 2 + x_cell_size(bt.pager.page_extra(p), &cell, 0) as i32;
        }
        cnt_new[i] = cnt_old[i];
        k += 1;
    }
    let mut k = n_old;
    let mut i = 0usize;
    while i < k {
        while sz_new[i] > usable_space {
            if i + 1 >= k {
                k = i + 2;
                if k > NB + 2 {
                    return SQLITE_CORRUPT_BKPT;
                }
                sz_new[k - 1] = 0;
                cnt_new[k - 1] = b.n_cell;
            }
            let mut sz = 2 + csz!(cnt_new[i] - 1);
            sz_new[i] -= sz;
            if !leaf_data {
                if cnt_new[i] < b.n_cell {
                    sz = 2 + csz!(cnt_new[i]);
                } else {
                    sz = 0;
                }
            }
            sz_new[i + 1] += sz;
            cnt_new[i] -= 1;
        }
        while cnt_new[i] < b.n_cell {
            let mut sz = 2 + csz!(cnt_new[i]);
            if sz_new[i] + sz > usable_space {
                break;
            }
            sz_new[i] += sz;
            cnt_new[i] += 1;
            if !leaf_data {
                if cnt_new[i] < b.n_cell {
                    sz = 2 + csz!(cnt_new[i]);
                } else {
                    sz = 0;
                }
            }
            sz_new[i + 1] -= sz;
        }
        if cnt_new[i] >= b.n_cell {
            k = i + 1;
        } else if cnt_new[i] <= (if i > 0 { cnt_new[i - 1] } else { 0 }) {
            return SQLITE_CORRUPT_BKPT;
        }
        i += 1;
    }

    // O empacotamento acima favorece as irmãs da esquerda; este bloco o corrige (não é só
    // otimização: a irmã mais à direita poderia ficar vazia).
    let mut i = k as i32 - 1;
    while i > 0 {
        let iu = i as usize;
        let mut sz_right = sz_new[iu];
        let mut sz_left = sz_new[iu - 1];
        let mut r = cnt_new[iu - 1] - 1;
        let mut d = r + 1 - leaf_data as i32;
        let _ = csz!(d);
        loop {
            let sz_r = csz!(r);
            let sz_d = csz!(d);
            if sz_right != 0
                && (b_bulk != 0
                    || sz_right + sz_d + 2 > sz_left - (sz_r + (if i as usize == k - 1 { 0 } else { 2 })))
            {
                break;
            }
            sz_right += sz_d + 2;
            sz_left -= sz_r + 2;
            cnt_new[iu - 1] = r;
            r -= 1;
            d -= 1;
            if r < 0 {
                break;
            }
        }
        sz_new[iu] = sz_right;
        sz_new[iu - 1] = sz_left;
        if cnt_new[iu - 1] <= (if i > 1 { cnt_new[iu - 2] } else { 0 }) {
            return SQLITE_CORRUPT_BKPT;
        }
        i -= 1;
    }

    // Aloca `k` páginas novas, reaproveitando as antigas quando possível.
    let page_flags: u8 = bt.pager.page_data(p_ref).first().copied().unwrap_or(0);
    for i in 0..k {
        if i < n_old {
            let p_new = match st.ap_old[i].take() {
                Some(p) => p,
                None => return SQLITE_CORRUPT_BKPT,
            };
            st.ap_new[i] = Some(p_new);
            rc = bt.pager.write(p_new);
            n_new += 1;
            st.n_new = n_new;
            let want: i64 = 1 + (i as i32 == (i_parent_idx - nx_div)) as i64;
            if bt.pager.page_ref_count(p_new) != want && rc == SQLITE_OK {
                rc = SQLITE_CORRUPT_BKPT;
            }
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            let near = if b_bulk != 0 { 1 } else { pgno };
            let p_new = match allocate_btree_page(bt, near, 0) {
                Ok(p) => p,
                Err(e) => return e,
            };
            pgno = bt.pager.page_pgno(p_new);
            zero_page(bt, p_new, page_flags);
            st.ap_new[i] = Some(p_new);
            n_new += 1;
            st.n_new = n_new;
            cnt_old[i] = b.n_cell;

            // Entrada do mapa de ponteiros para a nova irmã.
            if bt.auto_vacuum != 0 {
                let parent_pgno = bt.pager.page_extra(parent).pgno;
                let new_pgno = bt.pager.page_extra(p_new).pgno;
                ptrmap_put(bt, new_pgno, PTRMAP_BTREE, parent_pgno, &mut rc);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        }
    }

    // Reatribui os números de página para as novas ficarem em ordem crescente (O(N*N), com N
    // no máximo 5).
    macro_rules! apnew {
        ($i:expr) => {
            match st.ap_new[$i] {
                Some(p) => p,
                None => return SQLITE_CORRUPT_BKPT,
            }
        };
    }
    for i in 0..n_new {
        a_pgno[i] = bt.pager.page_extra(apnew!(i)).pgno;
    }
    for i in 0..n_new.saturating_sub(1) {
        let mut i_b = i;
        for j in i + 1..n_new {
            if bt.pager.page_extra(apnew!(j)).pgno < bt.pager.page_extra(apnew!(i_b)).pgno {
                i_b = j;
            }
        }
        // Se `apNew[i]` tem número maior que o de alguma seguinte, troca com a de menor número.
        if i_b != i {
            let pi = apnew!(i);
            let pb = apnew!(i_b);
            let pgno_a = bt.pager.page_extra(pi).pgno;
            let pgno_b = bt.pager.page_extra(pb).pgno;
            let pgno_temp = bt.pending_byte_page();
            let fg_a = bt.pager.page_flags(pi);
            let fg_b = bt.pager.page_flags(pb);
            bt.pager.rekey(pi, pgno_temp, fg_b);
            bt.pager.rekey(pb, pgno_a, fg_a);
            bt.pager.rekey(pi, pgno_b, fg_b);
            bt.pager.page_extra_mut(pi).pgno = pgno_b;
            bt.pager.page_extra_mut(pb).pgno = pgno_a;
        }
    }

    // O ponteiro do pai para a última irmã.
    {
        let last = bt.pager.page_extra(apnew!(n_new - 1)).pgno;
        if !p4(bt.pager.page_data_mut(parent), p_right, last) {
            return SQLITE_CORRUPT_BKPT;
        }
    }

    // Se as irmãs não são folhas, o ponteiro direito da última nova recebe o que estava na
    // última antiga.
    if (page_flags & PTF_LEAF) == 0 && n_old != n_new {
        let p_old_last = if n_new > n_old { st.ap_new[n_old - 1] } else { st.ap_old[n_old - 1] };
        let p_old_last = match p_old_last {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        let right: [u8; 4] = match bt.pager.page_data(p_old_last).get(8..12) {
            Some(s) => [s[0], s[1], s[2], s[3]],
            None => return SQLITE_CORRUPT_BKPT,
        };
        match bt.pager.page_data_mut(apnew!(n_new - 1)).get_mut(8..12) {
            Some(d) => d.copy_from_slice(&right),
            None => return SQLITE_CORRUPT_BKPT,
        }
    }

    // Atualiza o mapa de ponteiros das células guardadas nas irmãs depois do balanceamento
    // (as das células divisoras ficam por conta de `insert_cell`).
    if bt.auto_vacuum != 0 {
        let mut p_new = apnew!(0);
        let mut p_old = p_new;
        let mut cnt_old_next = {
            let p = bt.pager.page_extra(p_new);
            p.n_cell as i32 + p.n_overflow as i32
        };
        let mut i_new = 0usize;
        let mut i_old = 0usize;
        for i in 0..b.n_cell {
            let p_cell = b.ap_cell[i as usize];
            while i == cnt_old_next {
                i_old += 1;
                if i_old >= NB {
                    return SQLITE_CORRUPT_BKPT;
                }
                p_old = if i_old < n_new {
                    apnew!(i_old)
                } else {
                    match st.ap_old[i_old] {
                        Some(p) => p,
                        None => return SQLITE_CORRUPT_BKPT,
                    }
                };
                let pp = bt.pager.page_extra(p_old);
                cnt_old_next += pp.n_cell as i32 + pp.n_overflow as i32 + !leaf_data as i32;
            }
            if i == cnt_new[i_new] {
                i_new += 1;
                if i_new >= n_new {
                    return SQLITE_CORRUPT_BKPT;
                }
                p_new = apnew!(i_new);
                if !leaf_data {
                    continue;
                }
            }

            // A célula vai para a irmã nova `pNew`. Se a antiga `iOld` tinha o mesmo número de
            // página que `pNew` e a célula era mesmo dela (não divisor nem overflow), não há o
            // que atualizar.
            let in_old = p_cell.src == CellSrc::Page(p_old)
                && p_cell.off < bt.pager.page_extra(p_old).a_data_end;
            if i_old >= n_new || bt.pager.page_extra(p_new).pgno != a_pgno[i_old] || !in_old {
                let new_pgno = bt.pager.page_extra(p_new).pgno;
                if leaf_correction == 0 {
                    let child = g4(b.slice(bt, p_cell), 0);
                    ptrmap_put(bt, child, PTRMAP_BTREE, new_pgno, &mut rc);
                }
                let c_sz = csz!(i);
                if c_sz > bt.pager.page_extra(p_new).min_local as i32 {
                    let room = if p_cell.src == CellSrc::Page(p_old) {
                        let end = bt.pager.page_extra(p_old).a_data_end;
                        end.saturating_sub(p_cell.off)
                    } else {
                        0
                    };
                    let cell_bytes: Vec<u8> = b.slice(bt, p_cell).to_vec();
                    let end = if room > 0 { Some(room) } else { None };
                    ptrmap_put_ovfl_ptr(bt, p_new, end, &cell_bytes, &mut rc);
                }
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        }
    }

    // Insere os novos divisores no pai.
    for i in 0..n_new - 1 {
        let p_new = apnew!(i);
        let mut j = cnt_new[i];
        if j < 0 || j >= b.n_cell {
            return SQLITE_CORRUPT_BKPT;
        }
        let mut p_cell = b.ap_cell[j as usize];
        let mut sz = b.sz_cell[j as usize] as i32 + leaf_correction as i32;
        let new_leaf = bt.pager.page_extra(p_new).leaf;
        let mut cell_buf: Vec<u8> = Vec::new();
        let mut loose = false; // célula montada aqui, fora de qualquer origem
        if !new_leaf {
            let four: [u8; 4] = {
                let s = b.slice(bt, p_cell);
                match s.get(..4) {
                    Some(s) => [s[0], s[1], s[2], s[3]],
                    None => return SQLITE_CORRUPT_BKPT,
                }
            };
            match bt.pager.page_data_mut(p_new).get_mut(8..12) {
                Some(d) => d.copy_from_slice(&four),
                None => return SQLITE_CORRUPT_BKPT,
            }
        } else if leaf_data {
            // Árvore de tabela com folhas: o divisor é só a chave inteira da última célula da
            // irmã montada acima.
            j -= 1;
            let mut info = CellInfo::default();
            {
                let s = b.slice(bt, b.ap_cell[j as usize]);
                x_parse_cell(bt.pager.page_extra(p_new), s, 0, &mut info);
            }
            cell_buf = vec![0u8; 13];
            let n = put_varint(&mut cell_buf[4..], info.n_key as u64);
            sz = 4 + n;
            cell_buf.truncate(sz as usize);
            loose = true;
        } else {
            if p_cell.off < 4 {
                return SQLITE_CORRUPT_BKPT;
            }
            p_cell.off -= 4;
            // Caso obscuro: se a célula estava numa folha e o tamanho dela era 4, ela pode ser
            // menor de fato; reanalisa para passar o tamanho certo a `insertCell`.
            if b.sz_cell[j as usize] == 4 {
                let s = b.slice(bt, p_cell);
                sz = x_cell_size(bt.pager.page_extra(parent), s, 0) as i32;
            }
        }
        let mut k2 = 0usize;
        while k2 < NB * 2 && b.ix_nx[k2] <= j {
            k2 += 1;
        }
        let p_src_end = b.ap_end[k2.min(NB * 2 - 1)];
        if !loose && straddles(p_cell, sz as usize, p_src_end) {
            return SQLITE_CORRUPT_BKPT;
        }
        if !loose {
            if !b.copy_cell(bt, p_cell, sz as usize, &mut cell_buf) {
                return SQLITE_CORRUPT_BKPT;
            }
        }
        let new_pgno = bt.pager.page_extra(p_new).pgno;
        rc = insert_cell(bt, parent, nx_div + i as i32, &cell_buf, sz, new_pgno);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Atualiza as páginas irmãs. A ordem importa: não se pode estragar uma página de onde ainda
    // se vai ler célula. Duas passadas: iPg desce de nNew-1 a 0 e sobe de volta.
    let mut ii = 1 - n_new as i32;
    while ii < n_new as i32 {
        let i_pg = ii.unsigned_abs() as usize;
        if i_pg >= n_new {
            return SQLITE_CORRUPT_BKPT;
        }
        if ab_done[i_pg] != 0 {
            ii += 1;
            continue;
        }
        if ii >= 0 || cnt_old[i_pg - 1] >= cnt_new[i_pg - 1] {
            let (i_new, i_old, n_new_cell);
            if i_pg == 0 {
                i_new = 0;
                i_old = 0;
                n_new_cell = cnt_new[0];
            } else {
                i_old = if i_pg < n_old { cnt_old[i_pg - 1] + !leaf_data as i32 } else { b.n_cell };
                i_new = cnt_new[i_pg - 1] + !leaf_data as i32;
                n_new_cell = cnt_new[i_pg] - i_new;
            }
            rc = edit_page(bt, &mut b, apnew!(i_pg), i_old, i_new, n_new_cell);
            if rc != SQLITE_OK {
                return rc;
            }
            ab_done[i_pg] += 1;
            bt.pager.page_extra_mut(apnew!(i_pg)).n_free = usable_space - sz_new[i_pg];
        }
        ii += 1;
    }

    // Todas as páginas foram processadas exatamente uma vez.
    let (root_n_cell, root_hdr) = {
        let p = bt.pager.page_extra(parent);
        (p.n_cell, p.hdr_offset as i32)
    };
    if is_root && root_n_cell == 0 && root_hdr <= bt.pager.page_extra(apnew!(0)).n_free {
        // A raiz ficou sem células e a única irmã é a filha direita: copia o conteúdo da filha
        // para a raiz ("balance-shallower"). A filha tem de estar desfragmentada antes.
        rc = defragment_page(bt, apnew!(0), -1);
        copy_node_content(bt, apnew!(0), parent, &mut rc);
        free_page(bt, apnew!(0), &mut rc);
    } else if bt.auto_vacuum != 0 && leaf_correction == 0 {
        // Corrige as entradas do mapa de ponteiros do filho direito de cada irmã.
        for i in 0..n_new {
            let pg = apnew!(i);
            let key = g4(bt.pager.page_data(pg), 8);
            let pgno_i = bt.pager.page_extra(pg).pgno;
            ptrmap_put(bt, key, PTRMAP_BTREE, pgno_i, &mut rc);
        }
    }

    // Libera as páginas antigas que não foram reaproveitadas.
    for i in n_new..n_old {
        if let Some(p) = st.ap_old[i] {
            free_page(bt, p, &mut rc);
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// balance_deeper, anotherValidCursor, balance
// ---------------------------------------------------------------------------

/// `balance_deeper`: a raiz está cheia (tem células em overflow). Aloca uma filha, copia para
/// ela o conteúdo da raiz (com o overflow) e esvazia a raiz, que passa a apontar para a filha.
/// Quem chama solta a referência à filha exatamente uma vez.
fn balance_deeper(bt: &mut BtShared, root: PgId) -> Result<PgId, i32> {
    let mut rc = bt.pager.write(root);
    let mut p_child: Option<PgId> = None;
    let mut pgno_child = 0u32;
    if rc == SQLITE_OK {
        let near = bt.pager.page_extra(root).pgno;
        match allocate_btree_page(bt, near, 0) {
            Ok(c) => {
                p_child = Some(c);
                pgno_child = bt.pager.page_pgno(c);
            }
            Err(e) => rc = e,
        }
        if let Some(c) = p_child {
            copy_node_content(bt, root, c, &mut rc);
        }
        if bt.auto_vacuum != 0 {
            let root_pgno = bt.pager.page_extra(root).pgno;
            ptrmap_put(bt, pgno_child, PTRMAP_BTREE, root_pgno, &mut rc);
        }
    }
    if rc != SQLITE_OK {
        if let Some(c) = p_child {
            bt.pager.unref_not_null(c);
        }
        return Err(rc);
    }
    let child = match p_child {
        Some(c) => c,
        None => return Err(SQLITE_CORRUPT_BKPT),
    };

    // Copia as células em overflow da raiz para a filha.
    let (n_ovfl, ai_ovfl, ap_ovfl) = {
        let r = bt.pager.page_extra(root);
        (r.n_overflow, r.ai_ovfl, r.ap_ovfl.clone())
    };
    {
        let cp = bt.pager.page_extra_mut(child);
        for i in 0..n_ovfl as usize {
            cp.ai_ovfl[i] = ai_ovfl[i];
            cp.ap_ovfl[i] = ap_ovfl[i].clone();
        }
        cp.n_overflow = n_ovfl;
    }

    // Zera a raiz e instala a filha como filha direita.
    let flags0 = bt.pager.page_data(child).first().copied().unwrap_or(0);
    zero_page(bt, root, flags0 & !PTF_LEAF);
    let hdr_r = bt.pager.page_extra(root).hdr_offset as usize;
    if !p4(bt.pager.page_data_mut(root), hdr_r + 8, pgno_child) {
        bt.pager.unref_not_null(child);
        return Err(SQLITE_CORRUPT_BKPT);
    }
    Ok(child)
}

/// `anotherValidCursor`: `SQLITE_CORRUPT` se outro cursor válido está na mesma página que
/// `cur` (o cursor `cur` não está em `bt.cursors` enquanto a função roda).
fn another_valid_cursor(cur: &BtCursor, bt: &BtShared) -> i32 {
    for (_, other) in bt.cursors.iter() {
        if other.e_state == CURSOR_VALID && other.p_page == cur.p_page {
            return SQLITE_CORRUPT_BKPT;
        }
    }
    SQLITE_OK
}

/// `balance`: a página do cursor acabou de ser modificada; decide se a árvore precisa de
/// balanceamento e chama a rotina certa (`balance_quick`, `balance_deeper`, `balance_nonroot`).
fn balance(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    let mut rc = SQLITE_OK;
    loop {
        let pg = match cur.p_page {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        if bt.pager.page_extra(pg).n_free < 0 && btree_compute_free_space(bt, pg) != 0 {
            break;
        }
        let (n_overflow, n_free) = {
            let p = bt.pager.page_extra(pg);
            (p.n_overflow, p.n_free)
        };
        if n_overflow == 0 && n_free * 3 <= (bt.usable_size as i32) * 2 {
            // Sem balanceamento: nenhuma célula em overflow e menos de 2/3 da página livre.
            break;
        }
        let i_page = cur.i_page as i32;
        if i_page == 0 {
            if n_overflow != 0 && {
                rc = another_valid_cursor(cur, bt);
                rc == SQLITE_OK
            } {
                // A raiz está cheia: cria uma filha para ela; a próxima volta balanceia a filha.
                match balance_deeper(bt, pg) {
                    Ok(child) => {
                        cur.ap_page[1] = Some(child);
                        cur.i_page = 1;
                        cur.ix = 0;
                        cur.ai_idx[0] = 0;
                        cur.ap_page[0] = Some(pg);
                        cur.p_page = Some(child);
                    }
                    Err(e) => rc = e,
                }
            } else {
                break;
            }
        } else if bt.pager.page_ref_count(pg) > 1 {
            // Página que não é raiz com mais de uma referência: ela é ancestral dela mesma.
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            let parent = match cur.ap_page[(i_page - 1) as usize] {
                Some(p) => p,
                None => return SQLITE_CORRUPT_BKPT,
            };
            let i_idx = cur.ai_idx[(i_page - 1) as usize] as i32;

            rc = bt.pager.write(parent);
            if rc == SQLITE_OK && bt.pager.page_extra(parent).n_free < 0 {
                rc = btree_compute_free_space(bt, parent);
            }
            if rc == SQLITE_OK {
                let (int_key_leaf, n_ovfl, ai0, n_cell) = {
                    let p = bt.pager.page_extra(pg);
                    (p.int_key_leaf, p.n_overflow, p.ai_ovfl[0], p.n_cell)
                };
                let (parent_pgno, parent_n_cell) = {
                    let p = bt.pager.page_extra(parent);
                    (p.pgno, p.n_cell as i32)
                };
                if int_key_leaf
                    && n_ovfl == 1
                    && ai0 == n_cell
                    && parent_pgno != 1
                    && parent_n_cell == i_idx
                {
                    // `balance_quick` cria uma irmã nova para guardar a célula em overflow e
                    // insere uma célula no pai (que pode ficar cheio: a próxima volta resolve).
                    rc = balance_quick(bt, parent, pg);
                } else {
                    // Redistribui as células entre `pg` e até 2 irmãs; o pai pode ficar cheio
                    // ou vazio, e a próxima volta o balanceia.
                    rc = balance_nonroot(
                        bt,
                        parent,
                        i_idx,
                        i_page == 1,
                        (cur.hints as u32 & BTREE_BULKLOAD) as i32,
                    );
                }
            }

            bt.pager.page_extra_mut(pg).n_overflow = 0;

            // A próxima volta balanceia o pai.
            bt.pager.unref_not_null(pg);
            cur.i_page -= 1;
            cur.p_page = cur.ap_page[cur.i_page as usize];
        }
        if rc != SQLITE_OK {
            break;
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Sobrescrita de célula e inserção
// ---------------------------------------------------------------------------

/// `btreeOverwriteContent`: sobrescreve `i_amt` bytes a partir de `dest` (offset na página `pg`)
/// com o conteúdo de `x` a partir de `i_offset`, só se for diferente do que já está lá.
fn btree_overwrite_content(
    bt: &mut BtShared,
    pg: PgId,
    dest: usize,
    x: &BtreePayload,
    i_offset: i32,
    mut i_amt: i32,
) -> i32 {
    if i_amt < 0 {
        return SQLITE_CORRUPT_BKPT;
    }
    let n_data = x.n_data - i_offset;
    if n_data <= 0 {
        // Sobrescrita com zeros.
        let amt = i_amt as usize;
        let mut i = 0usize;
        {
            let d = bt.pager.page_data(pg);
            if dest + amt > d.len() {
                return SQLITE_CORRUPT_BKPT;
            }
            while i < amt && d[dest + i] == 0 {
                i += 1;
            }
        }
        if i < amt {
            let rc = bt.pager.write(pg);
            if rc != SQLITE_OK {
                return rc;
            }
            bt.pager.page_data_mut(pg)[dest + i..dest + amt].fill(0);
        }
    } else {
        if n_data < i_amt {
            // Dados reais e zeros no fim: escreve os zeros por recursão e segue com os dados.
            let rc = btree_overwrite_content(
                bt,
                pg,
                dest + n_data as usize,
                x,
                i_offset + n_data,
                i_amt - n_data,
            );
            if rc != SQLITE_OK {
                return rc;
            }
            i_amt = n_data;
        }
        let amt = i_amt as usize;
        let src = match x.p_data.and_then(|d| d.get(i_offset as usize..i_offset as usize + amt)) {
            Some(s) => s,
            None => return SQLITE_CORRUPT_BKPT,
        };
        let differs = match bt.pager.page_data(pg).get(dest..dest + amt) {
            Some(cur) => cur != src,
            None => return SQLITE_CORRUPT_BKPT,
        };
        if differs {
            let rc = bt.pager.write(pg);
            if rc != SQLITE_OK {
                return rc;
            }
            bt.pager.page_data_mut(pg)[dest..dest + amt].copy_from_slice(src);
        }
    }
    SQLITE_OK
}

/// `btreeOverwriteOverflowCell`: sobrescreve a célula do cursor quando ela tem overflow.
fn btree_overwrite_overflow_cell(cur: &mut BtCursor, bt: &mut BtShared, x: &BtreePayload) -> i32 {
    let n_total = x.n_data + x.n_zero;
    let pg = match cur.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };

    // Primeiro a parte local.
    let rc = btree_overwrite_content(bt, pg, cur.info.p_payload, x, 0, cur.info.n_local as i32);
    if rc != SQLITE_OK {
        return rc;
    }

    // Depois as páginas de overflow.
    let mut i_offset: u32 = cur.info.n_local as u32;
    let mut ovfl_pgno = g4(bt.pager.page_data(pg), cur.info.p_payload + i_offset as usize);
    let mut ovfl_page_size: u32 = bt.usable_size - 4;
    loop {
        let p = match btree_get_page(bt, ovfl_pgno, 0) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let rc;
        if bt.pager.page_ref_count(p) != 1 || bt.pager.page_extra(p).is_init {
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            if i_offset.wrapping_add(ovfl_page_size) < n_total as u32 {
                ovfl_pgno = g4(bt.pager.page_data(p), 0);
            } else {
                ovfl_page_size = (n_total as u32).wrapping_sub(i_offset);
            }
            rc = btree_overwrite_content(bt, p, 4, x, i_offset as i32, ovfl_page_size as i32);
        }
        bt.pager.unref_not_null(p);
        if rc != SQLITE_OK {
            return rc;
        }
        i_offset = i_offset.wrapping_add(ovfl_page_size);
        if i_offset >= n_total as u32 {
            break;
        }
    }
    SQLITE_OK
}

/// `btreeOverwriteCell`: sobrescreve a célula do cursor com o conteúdo de `x`.
fn btree_overwrite_cell(cur: &mut BtCursor, bt: &mut BtShared, x: &BtreePayload) -> i32 {
    let n_total = x.n_data + x.n_zero;
    let pg = match cur.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    let (data_end, cell_offset) = {
        let p = bt.pager.page_extra(pg);
        (p.a_data_end, p.cell_offset as usize)
    };
    if cur.info.p_payload + cur.info.n_local as usize > data_end || cur.info.p_payload < cell_offset {
        return SQLITE_CORRUPT_BKPT;
    }
    if cur.info.n_local as i32 == n_total {
        // A célula inteira é local.
        btree_overwrite_content(bt, pg, cur.info.p_payload, x, 0, cur.info.n_local as i32)
    } else {
        // A célula tem conteúdo em overflow.
        btree_overwrite_overflow_cell(cur, bt, x)
    }
}

/// `invalidateIncrblobCursors` (linha, não tabela) só com o `BtShared`: `btree_insert` e
/// `btree_delete` não recebem o `Btree`, que o `invalidate_incrblob_cursors` real exige. Não
/// atualiza `has_incrblob_cur` (é só otimização; ficar ligado a mais é inofensivo).
fn invalidate_incrblob_in_bt(bt: &mut BtShared, pgno_root: u32, i_row: i64) {
    for (_, c) in bt.cursors.iter_mut() {
        if (c.cur_flags & BTCF_INCRBLOB) != 0 && c.pgno_root == pgno_root && c.info.n_key == i_row {
            c.e_state = CURSOR_INVALID;
        }
    }
}

/// `sqlite3BtreeInsert`: insere um registro novo na árvore. O cursor só define a tabela e fica
/// numa posição qualquer. `flags` é `BTREE_SAVEPOSITION | BTREE_APPEND | BTREE_PREFORMAT`.
/// `seek_result` é o resultado de um `IndexMoveto` anterior (0 se não houve).
/// `has_incrblob_cur` é o `p->hasIncrblobCur` do `Btree` dono do cursor.
pub(crate) fn btree_insert(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    x: &BtreePayload,
    flags: i32,
    seek_result: i32,
    has_incrblob_cur: bool,
) -> i32 {
    // O `pTmpSpace` é retirado do `BtShared` durante a chamada: as rotinas que mexem em páginas
    // recebem o `bt` inteiro, e a célula nova não pode ficar dentro dele.
    let mut new_cell = std::mem::take(&mut bt.p_tmp_space);
    let rc = btree_insert_inner(cur, bt, x, flags, seek_result, has_incrblob_cur, &mut new_cell);
    bt.p_tmp_space = new_cell;
    rc
}

fn btree_insert_inner(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    x: &BtreePayload,
    flags: i32,
    seek_result: i32,
    has_incrblob_cur: bool,
    new_cell: &mut Vec<u8>,
) -> i32 {
    let mut rc: i32;
    let mut loc = seek_result; // -1: antes do local desejado, +1: depois
    let mut sz_new: i32 = 0;
    let fl = flags as u32;

    // Salva a posição dos outros cursores abertos nesta tabela.
    if (cur.cur_flags & BTCF_MULTIPLE) != 0 {
        rc = save_all_cursors(bt, cur.pgno_root, None);
        if rc != SQLITE_OK {
            return rc;
        }
        if loc != 0 && cur.i_page < 0 {
            // Só acontece com esquema corrompido em que mais de uma tabela ou índice usa a
            // mesma página raiz.
            return SQLITE_CORRUPT_BKPT;
        }
    }

    // O cursor não pode estar em CURSOR_FAULT e tem de apontar para uma célula válida.
    if cur.e_state >= CURSOR_REQUIRESEEK {
        rc = move_to_root(cur, bt);
        if rc != SQLITE_OK && rc != SQLITE_EMPTY {
            return rc;
        }
    }

    if cur.p_key_info.is_none() {
        // Inserção em tabela: invalida cursores incrblob na linha substituída.
        if has_incrblob_cur {
            invalidate_incrblob_in_bt(bt, cur.pgno_root, x.n_key);
        }

        if (cur.cur_flags & BTCF_VALID_NKEY) != 0 && x.n_key == cur.info.n_key {
            // O cursor aponta para a entrada a sobrescrever.
            if cur.info.n_size != 0 && cur.info.n_payload == (x.n_data + x.n_zero) as u32 {
                // Mesmo tamanho: sobrescreve.
                return btree_overwrite_cell(cur, bt, x);
            }
        } else if loc == 0 {
            // O cursor não aponta nem para a célula a sobrescrever nem para uma vizinha.
            rc = btree_table_moveto(
                cur,
                bt,
                x.n_key,
                ((fl & BTREE_APPEND) != 0) as i32,
                &mut loc,
            );
            if rc != SQLITE_OK {
                return rc;
            }
        }
    } else {
        // Índice ou tabela WITHOUT ROWID.
        if loc == 0 && (fl & BTREE_SAVEPOSITION) == 0 {
            if x.n_mem != 0 {
                let ki: Rc<KeyInfo> = match &cur.p_key_info {
                    Some(k) => Rc::clone(k),
                    None => return SQLITE_CORRUPT_BKPT,
                };
                let mut r = UnpackedRecord {
                    p_key_info: ki,
                    a_mem: x.a_mem.to_vec(),
                    u_i: 0,
                    n: 0,
                    n_field: x.n_mem,
                    default_rc: 0,
                    err_code: 0,
                    r1: 0,
                    r2: 0,
                    eq_seen: 0,
                };
                rc = btree_index_moveto(cur, bt, &mut r, &mut loc);
            } else {
                rc = btree_moveto(
                    cur,
                    bt,
                    x.p_key,
                    x.n_key,
                    ((fl & BTREE_APPEND) != 0) as i32,
                    &mut loc,
                );
            }
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Se o cursor aponta para uma entrada a sobrescrever com o mesmo conteúdo, usa a
        // otimização de sobrescrita.
        if loc == 0 {
            get_cell_info(cur, bt);
            if cur.info.n_key == x.n_key {
                let x2 = BtreePayload {
                    p_data: x.p_key,
                    n_data: x.n_key as i32,
                    n_zero: 0,
                    ..Default::default()
                };
                return btree_overwrite_cell(cur, bt, &x2);
            }
        }
    }

    let pg = match cur.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    if bt.pager.page_extra(pg).n_free < 0 {
        if cur.e_state > CURSOR_INVALID {
            rc = SQLITE_CORRUPT_BKPT; // por causa do `moveToRoot` acima
        } else {
            rc = btree_compute_free_space(bt, pg);
        }
        if rc != SQLITE_OK {
            return rc;
        }
    }

    if (fl & BTREE_PREFORMAT) != 0 {
        rc = SQLITE_OK;
        sz_new = bt.n_preformat_size;
        if sz_new < 4 {
            sz_new = 4;
            new_cell[3] = 0;
        }
        let (max_local, pgno_page) = {
            let p = bt.pager.page_extra(pg);
            (p.max_local as i32, p.pgno)
        };
        if bt.auto_vacuum != 0 && sz_new > max_local {
            let mut info = CellInfo::default();
            x_parse_cell(bt.pager.page_extra(pg), new_cell, 0, &mut info);
            if info.n_payload != info.n_local as u32 {
                let ovfl = g4(new_cell, sz_new as usize - 4);
                ptrmap_put(bt, ovfl, PTRMAP_OVERFLOW1, pgno_page, &mut rc);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        }
    } else {
        rc = fill_in_cell(bt, pg, new_cell, x, &mut sz_new);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    let mut idx = cur.ix as i32;
    cur.info.n_size = 0;
    if loc == 0 {
        if idx >= bt.pager.page_extra(pg).n_cell as i32 {
            return SQLITE_CORRUPT_BKPT;
        }
        rc = bt.pager.write(pg);
        if rc != SQLITE_OK {
            return rc;
        }
        let old_cell = find_cell(bt, pg, idx as usize);
        if !bt.pager.page_extra(pg).leaf {
            match bt.pager.page_data(pg).get(old_cell..old_cell + 4) {
                Some(s) => new_cell[..4].copy_from_slice(s),
                None => return SQLITE_CORRUPT_BKPT,
            }
        }
        let mut info = CellInfo::default();
        rc = btree_clear_cell(bt, pg, old_cell, &mut info);
        cur.cur_flags &= !BTCF_VALID_OVFL;
        let (min_local, hdr_offset, data_end) = {
            let p = bt.pager.page_extra(pg);
            (p.min_local as i32, p.hdr_offset as usize, p.a_data_end)
        };
        if info.n_size as i32 == sz_new
            && info.n_local as u32 == info.n_payload
            && (bt.auto_vacuum == 0 || sz_new < min_local)
        {
            // Mesmo tamanho: sobrescreve a célula velha com a nova. (Não vale em auto-vacuum
            // se a nova usa overflow: `insertCell` põe a entrada PTRMAP_OVERFLOW1.)
            if old_cell < hdr_offset + 10 {
                return SQLITE_CORRUPT_BKPT;
            }
            if old_cell + sz_new as usize > data_end {
                return SQLITE_CORRUPT_BKPT;
            }
            let data = bt.pager.page_data_mut(pg);
            return match data.get_mut(old_cell..old_cell + sz_new as usize) {
                Some(d) => {
                    d.copy_from_slice(&new_cell[..sz_new as usize]);
                    SQLITE_OK
                }
                None => SQLITE_CORRUPT_BKPT,
            };
        }
        drop_cell(bt, pg, idx, info.n_size as i32, &mut rc);
        if rc != SQLITE_OK {
            return rc;
        }
    } else if loc < 0 && bt.pager.page_extra(pg).n_cell > 0 {
        cur.ix += 1;
        idx = cur.ix as i32;
        cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    }
    rc = insert_cell_fast(bt, pg, idx, new_cell, sz_new);

    // Se não houve erro e a página tem célula em overflow, `balance` redistribui as células da
    // árvore. Como `balance` pode mover o cursor, zera `info.n_size` e BTCF_VALID_NKEY.
    if bt.pager.page_extra(pg).n_overflow != 0 {
        cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
        rc = balance(cur, bt);

        // `nOverflow` volta a zero mesmo se `balance` falhou, e o cursor fica inválido (isso
        // impede `saveCursorPosition` de tentar salvar a posição).
        if let Some(p) = cur.p_page {
            bt.pager.page_extra_mut(p).n_overflow = 0;
        }
        cur.e_state = CURSOR_INVALID;
        if (fl & BTREE_SAVEPOSITION) != 0 && rc == SQLITE_OK {
            btree_release_all_cursor_pages(cur, bt);
            if cur.p_key_info.is_some() {
                cur.bt_key = match x.p_key.and_then(|k| k.get(..x.n_key as usize)) {
                    Some(k) => Some(k.to_vec()),
                    None => None,
                };
            }
            cur.e_state = CURSOR_REQUIRESEEK;
            cur.n_key = x.n_key;
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// sqlite3BtreeTransferRow
// ---------------------------------------------------------------------------

/// Para onde `aOut` aponta em `btree_transfer_row`.
#[derive(Clone, Copy)]
enum Dst {
    /// A célula em montagem (`pTmpSpace`).
    Cell,
    /// Os bytes de uma página de overflow nova.
    Page(PgId),
}

/// De onde `aIn` lê em `btree_transfer_row`.
#[derive(Clone, Copy)]
enum SrcIn {
    /// A página do cursor de origem.
    Local,
    /// Uma página de overflow do `BtShared` de origem.
    Ovfl(PgId),
}

/// O `BtShared` de origem: o próprio `bt` quando `src_bt` é `None`.
fn src_of<'a>(bt: &'a mut BtShared, sb: &'a mut Option<&mut BtShared>) -> &'a mut BtShared {
    match sb {
        Some(s) => &mut **s,
        None => bt,
    }
}

/// `sqlite3BtreeTransferRow`: prepara no `pTmpSpace` de `bt` a célula que copia a linha atual de
/// `src` para `dest` (criando as páginas de overflow que precisar). O tamanho fica em
/// `bt.n_preformat_size`; quem chama termina com `btree_insert` e BTREE_PREFORMAT. `src_bt` é o
/// `BtShared` de `src` quando difere de `bt`.
pub(crate) fn btree_transfer_row(
    dest: &mut BtCursor,
    src: &mut BtCursor,
    bt: &mut BtShared,
    mut src_bt: Option<&mut BtShared>,
    i_key: i64,
) -> i32 {
    let mut cell = std::mem::take(&mut bt.p_tmp_space);
    let rc = transfer_row_inner(dest, src, bt, &mut src_bt, i_key, &mut cell);
    bt.p_tmp_space = cell;
    rc
}

fn transfer_row_inner(
    dest: &mut BtCursor,
    src: &mut BtCursor,
    bt: &mut BtShared,
    src_bt: &mut Option<&mut BtShared>,
    i_key: i64,
    cell: &mut Vec<u8>,
) -> i32 {
    get_cell_info(src, src_of(bt, src_bt));
    let n_payload = src.info.n_payload;
    let mut a_out: usize = 0;
    if cell.len() < 32 {
        return SQLITE_CORRUPT_BKPT;
    }
    if n_payload < 0x80 {
        cell[a_out] = n_payload as u8;
        a_out += 1;
    } else {
        a_out += put_varint(&mut cell[a_out..], n_payload as u64) as usize;
    }
    if dest.p_key_info.is_none() {
        a_out += put_varint(&mut cell[a_out..], i_key as u64) as usize;
    }
    let mut n_in: u32 = src.info.n_local as u32;
    let mut a_in: usize = src.info.p_payload;
    let src_page = match src.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    let src_end = src_of(bt, src_bt).pager.page_extra(src_page).a_data_end;
    if a_in + n_in as usize > src_end {
        return SQLITE_CORRUPT_BKPT;
    }
    let mut n_rem: u32 = n_payload;
    let dest_page = match dest.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    let max_local = bt.pager.page_extra(dest_page).max_local as u32;
    if n_in == n_rem && n_in < max_local {
        let bytes: Vec<u8> = {
            let sb = src_of(bt, src_bt);
            match sb.pager.page_data(src_page).get(a_in..a_in + n_in as usize) {
                Some(s) => s.to_vec(),
                None => return SQLITE_CORRUPT_BKPT,
            }
        };
        match cell.get_mut(a_out..a_out + n_in as usize) {
            Some(d) => d.copy_from_slice(&bytes),
            None => return SQLITE_CORRUPT_BKPT,
        }
        bt.n_preformat_size = (n_in as usize + a_out) as i32;
        return SQLITE_OK;
    }

    let mut rc = SQLITE_OK;
    let usable = bt.usable_size;
    let src_usable = src_of(bt, src_bt).usable_size;
    let mut page_in: Option<PgId> = None;
    let mut page_out: Option<PgId> = None;
    let mut ovfl_in: u32 = 0;
    let mut src_kind = SrcIn::Local;
    let mut dst = Dst::Cell;

    let mut n_out: u32 = btree_payload_to_local(bt.pager.page_extra(dest_page), n_payload as i64) as u32;
    bt.n_preformat_size = (n_out as usize + a_out) as i32;
    let mut pgno_out: Option<(Dst, usize)> = None;
    if n_out < n_payload {
        pgno_out = Some((Dst::Cell, a_out + n_out as usize));
        bt.n_preformat_size += 4;
    }

    if n_rem > n_in {
        if a_in + n_in as usize + 4 > src_end {
            return SQLITE_CORRUPT_BKPT;
        }
        ovfl_in = g4(src_of(bt, src_bt).pager.page_data(src_page), src.info.p_payload + n_in as usize);
    }

    'outer: loop {
        n_rem = n_rem.wrapping_sub(n_out);
        loop {
            if n_in > 0 {
                let n_copy = n_out.min(n_in) as usize;
                let bytes: Vec<u8> = {
                    let sb = src_of(bt, src_bt);
                    let whole = match src_kind {
                        SrcIn::Local => sb.pager.page_data(src_page),
                        SrcIn::Ovfl(p) => sb.pager.page_data(p),
                    };
                    match whole.get(a_in..a_in + n_copy) {
                        Some(s) => s.to_vec(),
                        None => {
                            rc = SQLITE_CORRUPT_BKPT;
                            break 'outer;
                        }
                    }
                };
                let ok = match dst {
                    Dst::Cell => match cell.get_mut(a_out..a_out + n_copy) {
                        Some(d) => {
                            d.copy_from_slice(&bytes);
                            true
                        }
                        None => false,
                    },
                    Dst::Page(p) => match bt.pager.page_data_mut(p).get_mut(a_out..a_out + n_copy) {
                        Some(d) => {
                            d.copy_from_slice(&bytes);
                            true
                        }
                        None => false,
                    },
                };
                if !ok {
                    rc = SQLITE_CORRUPT_BKPT;
                    break 'outer;
                }
                n_out -= n_copy as u32;
                n_in -= n_copy as u32;
                a_out += n_copy;
                a_in += n_copy;
            }
            if n_out > 0 {
                {
                    let sb = src_of(bt, src_bt);
                    sb.pager.unref(page_in.take());
                    match sb.pager.get(ovfl_in, PAGER_GET_READONLY) {
                        Ok(p) => {
                            page_in = Some(p);
                            ovfl_in = g4(sb.pager.page_data(p), 0);
                            a_in = 4;
                            src_kind = SrcIn::Ovfl(p);
                            n_in = src_usable - 4;
                        }
                        Err(e) => rc = e,
                    }
                }
            }
            if !(rc == SQLITE_OK && n_out > 0) {
                break;
            }
        }

        if rc == SQLITE_OK && n_rem > 0 && pgno_out.is_some() {
            let (where_, at) = match pgno_out {
                Some(t) => t,
                None => (Dst::Cell, 0),
            };
            let p_new = match allocate_btree_page(bt, 0, 0) {
                Ok(p) => Some(p),
                Err(e) => {
                    rc = e;
                    None
                }
            };
            if let Some(pn) = p_new {
                let pgno_new = bt.pager.page_pgno(pn);
                let wrote = match where_ {
                    Dst::Cell => p4(cell, at, pgno_new),
                    Dst::Page(p) => p4(bt.pager.page_data_mut(p), at, pgno_new),
                };
                if !wrote {
                    rc = SQLITE_CORRUPT_BKPT;
                }
                if bt.auto_vacuum != 0 {
                    if let Some(po) = page_out {
                        let po_pgno = bt.pager.page_extra(po).pgno;
                        ptrmap_put(bt, pgno_new, PTRMAP_OVERFLOW2, po_pgno, &mut rc);
                    }
                }
            }
            bt.pager.unref(page_out.take());
            page_out = p_new;
            if let Some(po) = page_out {
                pgno_out = Some((Dst::Page(po), 0));
                if !p4(bt.pager.page_data_mut(po), 0, 0) {
                    rc = SQLITE_CORRUPT_BKPT;
                    break 'outer;
                }
                dst = Dst::Page(po);
                a_out = 4;
                n_out = (usable - 4).min(n_rem);
            }
        }
        if !(n_rem > 0 && rc == SQLITE_OK) {
            break;
        }
    }

    bt.pager.unref(page_out.take());
    src_of(bt, src_bt).pager.unref(page_in.take());
    rc
}

// ---------------------------------------------------------------------------
// sqlite3BtreeDelete
// ---------------------------------------------------------------------------

/// `sqlite3BtreeDelete`: apaga a entrada para a qual o cursor aponta. Sem BTREE_SAVEPOSITION o
/// cursor fica numa posição qualquer; com ele, o próximo `Next`/`Previous` vai à mesma linha a
/// que iria se o delete não tivesse acontecido. `has_incrblob_cur` é o do `Btree` dono do cursor.
pub(crate) fn btree_delete(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    flags: u8,
    has_incrblob_cur: bool,
) -> i32 {
    let mut rc: i32;
    if cur.e_state != CURSOR_VALID {
        if cur.e_state >= CURSOR_REQUIRESEEK {
            rc = btree_restore_cursor_position(cur, bt);
            if rc != SQLITE_OK || cur.e_state != CURSOR_VALID {
                return rc;
            }
        } else {
            return SQLITE_CORRUPT_BKPT;
        }
    }

    let i_cell_depth = cur.i_page as i32;
    let i_cell_idx = cur.ix as i32;
    let pg = match cur.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    if bt.pager.page_extra(pg).n_cell as i32 <= i_cell_idx {
        return SQLITE_CORRUPT_BKPT;
    }
    let mut p_cell = find_cell(bt, pg, i_cell_idx as usize);
    if bt.pager.page_extra(pg).n_free < 0 && btree_compute_free_space(bt, pg) != 0 {
        return SQLITE_CORRUPT_BKPT;
    }
    {
        let p = bt.pager.page_extra(pg);
        if p_cell < p.a_cell_idx + 2 * p.n_cell as usize {
            return SQLITE_CORRUPT_BKPT;
        }
    }

    // Se BTREE_SAVEPOSITION está ligado, a posição do cursor tem de ser preservada. Se o delete
    // causa rebalanceamento, salva a chave e deixa o cursor em REQUIRESEEK; senão o deixa em
    // SKIPNEXT. `b_preserve`: 0 sem preservar, 1 REQUIRESEEK, 2 SKIPNEXT.
    let mut b_preserve: u8 = ((flags as u32 & BTREE_SAVEPOSITION) != 0) as u8;
    if b_preserve != 0 {
        let (leaf, n_free, n_cell) = {
            let p = bt.pager.page_extra(pg);
            (p.leaf, p.n_free, p.n_cell)
        };
        let cell_sz = {
            let s = bt.pager.page_data(pg).get(p_cell..).unwrap_or(&[]);
            x_cell_size(bt.pager.page_extra(pg), s, 0) as i32
        };
        if !leaf
            || (n_free + cell_sz + 2) > (bt.usable_size * 2 / 3) as i32
            || n_cell == 1 // ver dbfuzz001.test
        {
            // Vai precisar de rebalanceamento: salva a chave do cursor.
            rc = save_cursor_key(cur, bt);
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            b_preserve = 2;
        }
    }

    // Se a página não é folha, leva o cursor para a maior entrada menor que a apagada; essa
    // célula substituirá a apagada no nó interno.
    if !bt.pager.page_extra(pg).leaf {
        rc = btree_previous(cur, bt, 0);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Salva a posição dos outros cursores da tabela antes de modificar.
    if (cur.cur_flags & BTCF_MULTIPLE) != 0 {
        rc = save_all_cursors(bt, cur.pgno_root, None);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Em tabela, invalida os cursores incrblob na linha apagada.
    if cur.p_key_info.is_none() && has_incrblob_cur {
        invalidate_incrblob_in_bt(bt, cur.pgno_root, cur.info.n_key);
    }

    // Torna gravável a página da entrada, libera o overflow e remove a célula.
    rc = bt.pager.write(pg);
    if rc != SQLITE_OK {
        return rc;
    }
    let mut info = CellInfo::default();
    rc = btree_clear_cell(bt, pg, p_cell, &mut info);
    drop_cell(bt, pg, i_cell_idx, info.n_size as i32, &mut rc);
    if rc != SQLITE_OK {
        return rc;
    }

    // Se a célula apagada não estava numa folha, o cursor aponta para a maior entrada da
    // subárvore da filha dela: essa célula da folha vai para o nó interno.
    if !bt.pager.page_extra(pg).leaf {
        let leaf_pg = match cur.p_page {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        if bt.pager.page_extra(leaf_pg).n_free < 0 {
            rc = btree_compute_free_space(bt, leaf_pg);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        let n: u32 = if i_cell_depth < cur.i_page as i32 - 1 {
            match cur.ap_page[(i_cell_depth + 1) as usize] {
                Some(p) => bt.pager.page_extra(p).pgno,
                None => return SQLITE_CORRUPT_BKPT,
            }
        } else {
            bt.pager.page_extra(leaf_pg).pgno
        };
        let leaf_n_cell = bt.pager.page_extra(leaf_pg).n_cell as usize;
        if leaf_n_cell == 0 {
            return SQLITE_CORRUPT_BKPT;
        }
        p_cell = find_cell(bt, leaf_pg, leaf_n_cell - 1);
        if p_cell < 4 {
            return SQLITE_CORRUPT_BKPT;
        }
        let n_cell = {
            let s = bt.pager.page_data(leaf_pg).get(p_cell..).unwrap_or(&[]);
            x_cell_size(bt.pager.page_extra(leaf_pg), s, 0) as usize
        };
        rc = bt.pager.write(leaf_pg);
        if rc == SQLITE_OK {
            let cell: Vec<u8> = match bt.pager.page_data(leaf_pg).get(p_cell - 4..p_cell + n_cell) {
                Some(s) => s.to_vec(),
                None => return SQLITE_CORRUPT_BKPT,
            };
            rc = insert_cell(bt, pg, i_cell_idx, &cell, n_cell as i32 + 4, n);
        }
        drop_cell(bt, leaf_pg, leaf_n_cell as i32 - 1, n_cell as i32, &mut rc);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    // Balanceia a árvore. Se a entrada apagada estava numa folha, o cursor ainda aponta para ela
    // e o primeiro `balance` conserta tudo. Senão, balanceia primeiro a folha e depois, subindo
    // o cursor, o nó interno.
    let cur_pg = match cur.p_page {
        Some(p) => p,
        None => return SQLITE_CORRUPT_BKPT,
    };
    if bt.pager.page_extra(cur_pg).n_free * 3 <= (bt.usable_size as i32) * 2 {
        // Menos de 2/3 da página livre: `balance` não faria nada.
        rc = SQLITE_OK;
    } else {
        rc = balance(cur, bt);
    }
    if rc == SQLITE_OK && cur.i_page as i32 > i_cell_depth {
        if let Some(p) = cur.p_page {
            bt.pager.unref_not_null(p);
        }
        cur.i_page -= 1;
        while cur.i_page as i32 > i_cell_depth {
            let p = cur.ap_page[cur.i_page as usize];
            cur.i_page -= 1;
            bt.pager.unref(p);
        }
        cur.p_page = cur.ap_page[cur.i_page as usize];
        rc = balance(cur, bt);
    }

    if rc == SQLITE_OK {
        if b_preserve > 1 {
            cur.e_state = CURSOR_SKIPNEXT;
            let n_cell = bt.pager.page_extra(pg).n_cell as i32;
            if i_cell_idx >= n_cell {
                cur.skip_next = -1;
                cur.ix = (n_cell - 1) as u16;
            } else {
                cur.skip_next = 1;
            }
        } else {
            rc = move_to_root(cur, bt);
            if b_preserve != 0 {
                btree_release_all_cursor_pages(cur, bt);
                cur.e_state = CURSOR_REQUIRESEEK;
            }
            if rc == SQLITE_EMPTY {
                rc = SQLITE_OK;
            }
        }
    }
    rc
}

// ---------------------------------------------------------------------------
// Criação, limpeza e remoção de tabelas, meta-dados
// ---------------------------------------------------------------------------

/// `sqlite3BtreeGetMeta` (valor devolvido no lugar do `*pMeta`): lê o meta-valor `idx` do
/// cabeçalho. `BTREE_DATA_VERSION` não está no arquivo: vem do pager.
pub(crate) fn btree_get_meta(p: &Btree, idx: i32) -> u32 {
    let bt = &p.bt;
    if idx as u32 == BTREE_DATA_VERSION {
        bt.pager.data_version().wrapping_add(p.i_b_data_version)
    } else {
        match bt.p_page1 {
            Some(p1) => g4(bt.pager.page_data(p1), 36 + idx as usize * 4),
            None => 0,
        }
    }
}

/// `sqlite3BtreeUpdateMeta`: grava o meta-valor `idx` (1 a 15) no cabeçalho.
pub(crate) fn btree_update_meta(p: &mut Btree, idx: i32, i_meta: u32) -> i32 {
    let bt = &mut p.bt;
    let p1 = match bt.p_page1 {
        Some(p1) => p1,
        None => return SQLITE_CORRUPT_BKPT,
    };
    let rc = bt.pager.write(p1);
    if rc == SQLITE_OK {
        p4(bt.pager.page_data_mut(p1), 36 + idx as usize * 4, i_meta);
        if idx as u32 == BTREE_INCR_VACUUM {
            bt.incr_vacuum = i_meta as u8;
        }
    }
    rc
}

/// `sqlite3BtreeCreateTable` (com `btreeCreateTable`): cria uma árvore-b nova e devolve em
/// `pi_table` a página raiz. `create_tab_flags` é `BTREE_INTKEY` (tabela com rowid, a raiz é
/// uma folha INTKEY|LEAFDATA) ou `BTREE_BLOBKEY` (índice, raiz ZERODATA).
pub(crate) fn btree_create_table(p: &mut Btree, create_tab_flags: i32, pi_table: &mut u32) -> i32 {
    let mut rc: i32;
    let p_root: PgId;
    let pgno_root: u32;

    if p.bt.auto_vacuum != 0 {
        // Criar uma tabela pode exigir mover uma página para dar lugar à raiz nova. Se essa
        // página for de overflow, apaga os caches de overflow de todos os cursores.
        invalidate_all_overflow_cache(&mut p.bt);

        // O meta[3] é a maior raiz criada até agora; a raiz nova é meta[3]+1.
        let mut pgno = btree_get_meta(p, BTREE_LARGEST_ROOT_PAGE as i32);
        if pgno > p.bt.n_page {
            return SQLITE_CORRUPT_BKPT;
        }
        pgno += 1;

        // A raiz nova não pode ficar numa página do mapa de ponteiros nem na do PENDING_BYTE.
        while pgno == ptrmap_pageno(&p.bt, pgno) || pgno == p.bt.pending_byte_page() {
            pgno += 1;
        }

        // Aloca uma página; a que está em `pgno` vai para a alocada (se forem diferentes).
        let page_move = match allocate_btree_page(&mut p.bt, pgno, BTALLOC_EXACT) {
            Ok(pm) => pm,
            Err(e) => return e,
        };
        let pgno_move = p.bt.pager.page_pgno(page_move);

        if pgno_move != pgno {
            // Salva a posição dos cursores abertos (podem segurar uma referência à página).
            rc = save_all_cursors(&mut p.bt, 0, None);
            p.bt.pager.unref_not_null(page_move);
            if rc != SQLITE_OK {
                return rc;
            }

            // Move a página que está em `pgno` para `pgno_move`.
            let root = match btree_get_page(&mut p.bt, pgno, 0) {
                Ok(r) => r,
                Err(e) => return e,
            };
            let mut e_type: u8 = 0;
            let mut i_ptr_page: u32 = 0;
            rc = ptrmap_get(&mut p.bt, pgno, &mut e_type, Some(&mut i_ptr_page));
            if e_type == PTRMAP_ROOTPAGE || e_type == PTRMAP_FREEPAGE {
                rc = SQLITE_CORRUPT_BKPT;
            }
            if rc != SQLITE_OK {
                p.bt.pager.unref_not_null(root);
                return rc;
            }
            rc = relocate_page(&mut p.bt, root, e_type, i_ptr_page, pgno_move, false);
            p.bt.pager.unref_not_null(root);

            // Obtém a página em `pgno`.
            if rc != SQLITE_OK {
                return rc;
            }
            let root = match btree_get_page(&mut p.bt, pgno, 0) {
                Ok(r) => r,
                Err(e) => return e,
            };
            rc = p.bt.pager.write(root);
            if rc != SQLITE_OK {
                p.bt.pager.unref_not_null(root);
                return rc;
            }
            p_root = root;
        } else {
            p_root = page_move;
        }
        pgno_root = pgno;

        // Atualiza o mapa de ponteiros e o meta-dado com a raiz nova.
        rc = SQLITE_OK;
        ptrmap_put(&mut p.bt, pgno_root, PTRMAP_ROOTPAGE, 0, &mut rc);
        if rc != SQLITE_OK {
            p.bt.pager.unref_not_null(p_root);
            return rc;
        }

        // Ao alocar a raiz nova a página 1 virou gravável; o `UpdateMeta` não pode falhar.
        rc = btree_update_meta(p, 4, pgno_root);
        if rc != SQLITE_OK {
            p.bt.pager.unref_not_null(p_root);
            return rc;
        }
    } else {
        match allocate_btree_page(&mut p.bt, 1, 0) {
            Ok(r) => {
                p_root = r;
                pgno_root = p.bt.pager.page_pgno(r);
            }
            Err(e) => return e,
        }
    }
    let ptf_flags: u8 = if (create_tab_flags as u32 & BTREE_INTKEY) != 0 {
        PTF_INTKEY | PTF_LEAFDATA | PTF_LEAF
    } else {
        PTF_ZERODATA | PTF_LEAF
    };
    zero_page(&mut p.bt, p_root, ptf_flags);
    p.bt.pager.unref_not_null(p_root);
    *pi_table = pgno_root;
    SQLITE_OK
}

/// `clearDatabasePage`: apaga a página e todas as filhas e devolve a página à lista livre (se
/// `free_page_flag`). Soma o número de células apagadas a `pn_change`.
fn clear_database_page(
    bt: &mut BtShared,
    pgno: u32,
    free_page_flag: i32,
    mut pn_change: Option<&mut i64>,
) -> i32 {
    if pgno > bt.n_page {
        return SQLITE_CORRUPT_BKPT;
    }
    let pg = match get_and_init_page(bt, pgno, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut rc = SQLITE_OK;
    'out: {
        if (bt.open_flags as u32 & BTREE_SINGLE) == 0
            && bt.pager.page_ref_count(pg) != 1 + (pgno == 1) as i64
        {
            rc = SQLITE_CORRUPT_BKPT;
            break 'out;
        }
        let (hdr, n_cell, leaf, int_key) = {
            let p = bt.pager.page_extra(pg);
            (p.hdr_offset as usize, p.n_cell as usize, p.leaf, p.int_key)
        };
        for i in 0..n_cell {
            let p_cell = find_cell(bt, pg, i);
            if !leaf {
                let child = g4(bt.pager.page_data(pg), p_cell);
                rc = clear_database_page(bt, child, 1, pn_change.as_deref_mut());
                if rc != SQLITE_OK {
                    break 'out;
                }
            }
            let mut info = CellInfo::default();
            rc = btree_clear_cell(bt, pg, p_cell, &mut info);
            if rc != SQLITE_OK {
                break 'out;
            }
        }
        if !leaf {
            let child = g4(bt.pager.page_data(pg), hdr + 8);
            rc = clear_database_page(bt, child, 1, pn_change.as_deref_mut());
            if rc != SQLITE_OK {
                break 'out;
            }
            if int_key {
                pn_change = None;
            }
        }
        if let Some(c) = pn_change {
            *c += n_cell as i64;
        }
        if free_page_flag != 0 {
            free_page(bt, pg, &mut rc);
        } else {
            rc = bt.pager.write(pg);
            if rc == SQLITE_OK {
                let flags = bt.pager.page_data(pg).get(hdr).copied().unwrap_or(0) | PTF_LEAF;
                zero_page(bt, pg, flags);
            }
        }
    }
    bt.pager.unref_not_null(pg);
    rc
}

/// `sqlite3BtreeClearTable`: apaga todo o conteúdo da tabela de raiz `i_table`; a raiz continua
/// existindo, vazia. Soma a `pn_change` o número de entradas. (Falha com SQLITE_LOCKED se há
/// cursores de leitura abertos; os de escrita vão para a raiz.)
pub(crate) fn btree_clear_table(p: &mut Btree, i_table: i32, pn_change: Option<&mut i64>) -> i32 {
    let mut rc = save_all_cursors(&mut p.bt, i_table as u32, None);
    if rc == SQLITE_OK {
        // Invalida os cursores incrblob na tabela (sem efeito se `i_table` não é de tabela).
        if p.has_incrblob_cur {
            invalidate_incrblob_cursors(p, i_table as u32, 0, true);
        }
        rc = clear_database_page(&mut p.bt, i_table as u32, 0, pn_change);
    }
    rc
}

/// `sqlite3BtreeDropTable` (com `btreeDropTable`): apaga a tabela e põe a raiz na lista livre
/// (a raiz da tabela principal, na página 1, nunca vai para a lista). Em auto-vacuum, a última
/// raiz do arquivo ocupa o buraco; `pi_moved` recebe o número que ela tinha (0 se nada mudou).
pub(crate) fn btree_drop_table(p: &mut Btree, i_table: i32, pi_moved: &mut i32) -> i32 {
    if i_table as u32 > p.bt.n_page {
        return SQLITE_CORRUPT_BKPT;
    }
    let rc = btree_clear_table(p, i_table, None);
    if rc != SQLITE_OK {
        return rc;
    }
    let pg = match btree_get_page(&mut p.bt, i_table as u32, 0) {
        Ok(pg) => pg,
        Err(e) => return e,
    };
    let mut rc = SQLITE_OK;

    *pi_moved = 0;

    if p.bt.auto_vacuum != 0 {
        let mut max_root_pgno = btree_get_meta(p, BTREE_LARGEST_ROOT_PAGE as i32);

        if i_table as u32 == max_root_pgno {
            // A tabela apagada tem a maior raiz: a raiz vai para a lista livre.
            free_page(&mut p.bt, pg, &mut rc);
            p.bt.pager.unref_not_null(pg);
            if rc != SQLITE_OK {
                return rc;
            }
        } else {
            // Senão, a página de maior raiz ocupa o buraco da raiz apagada.
            p.bt.pager.unref_not_null(pg);
            let p_move = match btree_get_page(&mut p.bt, max_root_pgno, 0) {
                Ok(pm) => pm,
                Err(e) => return e,
            };
            rc = relocate_page(&mut p.bt, p_move, PTRMAP_ROOTPAGE, 0, i_table as u32, false);
            p.bt.pager.unref_not_null(p_move);
            if rc != SQLITE_OK {
                return rc;
            }
            match btree_get_page(&mut p.bt, max_root_pgno, 0) {
                Ok(pm) => {
                    free_page(&mut p.bt, pm, &mut rc);
                    p.bt.pager.unref_not_null(pm);
                }
                Err(e) => rc = e,
            }
            if rc != SQLITE_OK {
                return rc;
            }
            *pi_moved = max_root_pgno as i32;
        }

        // O novo maior número de raiz no cabeçalho é o antigo menos um, menos um se for página
        // do mapa de ponteiros, menos um se for a do PENDING_BYTE.
        max_root_pgno = max_root_pgno.wrapping_sub(1);
        while max_root_pgno == p.bt.pending_byte_page()
            || ptrmap_pageno(&p.bt, max_root_pgno) == max_root_pgno
        {
            max_root_pgno = max_root_pgno.wrapping_sub(1);
        }

        rc = btree_update_meta(p, 4, max_root_pgno);
    } else {
        free_page(&mut p.bt, pg, &mut rc);
        p.bt.pager.unref_not_null(pg);
    }
    rc
}

/// `sqlite3BtreeCount`: conta as entradas da árvore do cursor. `is_interrupted` é a consulta
/// de `db->u1.isInterrupted`.
pub(crate) fn btree_count(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    is_interrupted: &mut dyn FnMut() -> bool,
    pn_entry: &mut i64,
) -> i32 {
    let mut n_entry: i64 = 0;
    let mut rc = move_to_root(cur, bt);
    if rc == SQLITE_EMPTY {
        *pn_entry = 0;
        return SQLITE_OK;
    }

    // Uma volta por página da árvore (sem as de overflow), até um erro.
    while rc == SQLITE_OK && !is_interrupted() {
        let mut pg = match cur.p_page {
            Some(p) => p,
            None => return SQLITE_CORRUPT_BKPT,
        };
        let (leaf, int_key, n_cell) = {
            let p = bt.pager.page_extra(pg);
            (p.leaf, p.int_key, p.n_cell as i64)
        };

        // Folha, ou árvore que não é INTKEY: a página tem entradas contáveis.
        if leaf || !int_key {
            n_entry += n_cell;
        }

        // Numa folha, leva o cursor à primeira célula interna que aponta para o pai da próxima
        // página a visitar (ou ao número de células se a próxima é a filha direita). Se todas
        // as páginas foram visitadas, acabou.
        if leaf {
            loop {
                if cur.i_page == 0 {
                    *pn_entry = n_entry;
                    return move_to_root(cur, bt);
                }
                move_to_parent(cur, bt);
                let cp = match cur.p_page {
                    Some(p) => p,
                    None => return SQLITE_CORRUPT_BKPT,
                };
                if (cur.ix as i32) < bt.pager.page_extra(cp).n_cell as i32 {
                    break;
                }
            }
            cur.ix += 1;
            pg = match cur.p_page {
                Some(p) => p,
                None => return SQLITE_CORRUPT_BKPT,
            };
        }

        // Desce para a filha da célula do cursor (a direita se `i_idx == n_cell`).
        let i_idx = cur.ix as usize;
        let (n_cell_now, hdr) = {
            let p = bt.pager.page_extra(pg);
            (p.n_cell as usize, p.hdr_offset as usize)
        };
        if i_idx == n_cell_now {
            let child = g4(bt.pager.page_data(pg), hdr + 8);
            rc = move_to_child(cur, bt, child);
        } else {
            let off = find_cell(bt, pg, i_idx);
            let child = g4(bt.pager.page_data(pg), off);
            rc = move_to_child(cur, bt, child);
        }
    }

    // Ocorreu um erro.
    rc
}

// ---------------------------------------------------------------------------
// Verificação de integridade
// ---------------------------------------------------------------------------

/// O que a verificação de integridade lê do `db` do C: o sinal de interrupção e o
/// `xProgress` (`db->nProgressOps` é `n_progress_ops`, 0 se não há handler).
pub(crate) struct IntegrityDb<'a> {
    /// `AtomicLoad(&db->u1.isInterrupted)`.
    pub is_interrupted: &'a mut dyn FnMut() -> bool,
    /// `db->nProgressOps`; 0 se não há `xProgress`.
    pub n_progress_ops: u32,
    /// `db->xProgress(db->pProgressArg)`: diferente de zero interrompe.
    pub progress: &'a mut dyn FnMut() -> i32,
}

/// `IntegrityCk` mais o que o C guardava nele e o `IntegrityCk` do btree_types.rs não tem
/// (`pBt`, `pPager`, `db`).
struct CheckCtx<'a, 'b> {
    ck: IntegrityCk,
    bt: &'a mut BtShared,
    db: IntegrityDb<'b>,
}

/// `checkOom`.
fn check_oom(c: &mut CheckCtx) {
    c.ck.rc = SQLITE_NOMEM;
    c.ck.mx_err = 0; // faz o integrity_check parar
    if c.ck.n_err == 0 {
        c.ck.n_err += 1;
    }
}

/// `checkProgress`: chama o handler de progresso se for o caso e consulta a interrupção.
fn check_progress(c: &mut CheckCtx) {
    if (c.db.is_interrupted)() {
        c.ck.rc = SQLITE_INTERRUPT;
        c.ck.n_err += 1;
        c.ck.mx_err = 0;
    }
    if c.db.n_progress_ops != 0 {
        c.ck.n_step += 1;
        if c.ck.n_step % c.db.n_progress_ops == 0 && (c.db.progress)() != 0 {
            c.ck.rc = SQLITE_INTERRUPT;
            c.ck.n_err += 1;
            c.ck.mx_err = 0;
        }
    }
}

/// `checkAppendMsg`: acrescenta uma mensagem de erro (formato do `sqlite3_str_appendf`).
fn check_append_msg(c: &mut CheckCtx, fmt: &[u8], args: &[PrintfArg]) {
    check_progress(c);
    if c.ck.mx_err == 0 {
        return;
    }
    c.ck.mx_err -= 1;
    c.ck.n_err += 1;
    if c.ck.err_msg.n_char != 0 {
        c.ck.err_msg.append(b"\n");
    }
    if !c.ck.z_pfx.is_empty() {
        let v = [
            PrintfArg::Int(c.ck.v0 as i64),
            PrintfArg::Int(c.ck.v1 as i64),
            PrintfArg::Int(c.ck.v2 as i64),
        ];
        let pfx = c.ck.z_pfx;
        c.ck.err_msg.appendf(pfx.as_bytes(), &v);
    }
    c.ck.err_msg.appendf(fmt, args);
    if c.ck.err_msg.acc_error == SQLITE_NOMEM as u8 {
        check_oom(c);
    }
}

/// `getPageReferenced`.
fn get_page_referenced(c: &CheckCtx, i_pg: u32) -> bool {
    c.ck.a_pg_ref.get((i_pg / 8) as usize).is_some_and(|b| b & (1 << (i_pg & 0x07)) != 0)
}

/// `setPageReferenced`.
fn set_page_referenced(c: &mut CheckCtx, i_pg: u32) {
    if let Some(b) = c.ck.a_pg_ref.get_mut((i_pg / 8) as usize) {
        *b |= 1 << (i_pg & 0x07);
    }
}

/// `checkRef`: conta mais uma referência à página; devolve verdadeiro se já havia (ou se o
/// número é inválido).
fn check_ref(c: &mut CheckCtx, i_page: u32) -> bool {
    if i_page > c.ck.n_ck_page || i_page == 0 {
        check_append_msg(c, b"invalid page number %u", &[PrintfArg::Int(i_page as i64)]);
        return true;
    }
    if get_page_referenced(c, i_page) {
        check_append_msg(c, b"2nd reference to page %u", &[PrintfArg::Int(i_page as i64)]);
        return true;
    }
    set_page_referenced(c, i_page);
    false
}

/// `checkPtrmap`: confere que a entrada do mapa de ponteiros de `i_child` aponta para
/// `i_parent` com o tipo `e_type`.
fn check_ptrmap(c: &mut CheckCtx, i_child: u32, e_type: u8, i_parent: u32) {
    let mut e_ptrmap_type: u8 = 0;
    let mut i_ptrmap_parent: u32 = 0;
    let rc = ptrmap_get(c.bt, i_child, &mut e_ptrmap_type, Some(&mut i_ptrmap_parent));
    if rc != SQLITE_OK {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            check_oom(c);
        }
        check_append_msg(c, b"Failed to read ptrmap key=%u", &[PrintfArg::Int(i_child as i64)]);
        return;
    }

    if e_ptrmap_type != e_type || i_ptrmap_parent != i_parent {
        check_append_msg(
            c,
            b"Bad ptr map entry key=%u expected=(%u,%u) got=(%u,%u)",
            &[
                PrintfArg::Int(i_child as i64),
                PrintfArg::Int(e_type as i64),
                PrintfArg::Int(i_parent as i64),
                PrintfArg::Int(e_ptrmap_type as i64),
                PrintfArg::Int(i_ptrmap_parent as i64),
            ],
        );
    }
}

/// `checkList`: confere a lista livre ou uma lista de páginas de overflow, que deve ter `n`
/// páginas.
fn check_list(c: &mut CheckCtx, is_free_list: bool, i_page: u32, n: u32) {
    let expected = n;
    let mut n = n;
    let mut i_page = i_page;
    let n_err_at_start = c.ck.n_err;
    while i_page != 0 && c.ck.mx_err != 0 {
        if check_ref(c, i_page) {
            break;
        }
        n = n.wrapping_sub(1);
        let p_ovfl = match c.bt.pager.get(i_page, 0) {
            Ok(p) => p,
            Err(_) => {
                check_append_msg(c, b"failed to get page %u", &[PrintfArg::Int(i_page as i64)]);
                break;
            }
        };
        // Cópia dos bytes: as conferências abaixo precisam do `c` por inteiro.
        let data: Vec<u8> = c.bt.pager.page_data(p_ovfl).to_vec();
        if is_free_list {
            let n_leaf = g4(&data, 4);
            if c.bt.auto_vacuum != 0 {
                check_ptrmap(c, i_page, PTRMAP_FREEPAGE, 0);
            }
            if n_leaf > c.bt.usable_size / 4 - 2 {
                check_append_msg(
                    c,
                    b"freelist leaf count too big on page %u",
                    &[PrintfArg::Int(i_page as i64)],
                );
                n = n.wrapping_sub(1);
            } else {
                for i in 0..n_leaf as usize {
                    let i_free_page = g4(&data, 8 + i * 4);
                    if c.bt.auto_vacuum != 0 {
                        check_ptrmap(c, i_free_page, PTRMAP_FREEPAGE, 0);
                    }
                    check_ref(c, i_free_page);
                }
                n = n.wrapping_sub(n_leaf);
            }
        } else {
            // Em auto-vacuum, se `i_page` não é a última página da lista, a entrada do mapa de
            // ponteiros da seguinte tem de apontar para `i_page`.
            if c.bt.auto_vacuum != 0 && n > 0 {
                let i = g4(&data, 0);
                check_ptrmap(c, i, PTRMAP_OVERFLOW2, i_page);
            }
        }
        i_page = g4(&data, 0);
        c.bt.pager.unref_not_null(p_ovfl);
    }
    if n != 0 && n_err_at_start == c.ck.n_err {
        check_append_msg(
            c,
            b"%s is %u but should be %u",
            &[
                PrintfArg::Text(Some(
                    (if is_free_list { "size" } else { "overflow list length" }).as_bytes().to_vec(),
                )),
                PrintfArg::Int(expected.wrapping_sub(n) as i64),
                PrintfArg::Int(expected as i64),
            ],
        );
    }
}

/// `btreeHeapInsert`: mini-heap de `u32` em que `heap[0]` é o número de elementos.
fn btree_heap_insert(heap: &mut Vec<u32>, x: u32) {
    heap[0] += 1;
    let mut i = heap[0] as usize;
    if i >= heap.len() {
        heap.resize(i + 2, 0xffffffff);
    }
    heap[i] = x;
    loop {
        let j = i / 2;
        if j == 0 || heap[j] <= heap[i] {
            break;
        }
        heap.swap(j, i);
        i = j;
    }
}

/// `btreeHeapPull`: retira o menor elemento do heap; falso se está vazio.
fn btree_heap_pull(heap: &mut Vec<u32>, p_out: &mut u32) -> bool {
    let x = heap[0] as usize;
    if x == 0 {
        return false;
    }
    *p_out = heap[1];
    heap[1] = heap[x];
    heap[x] = 0xffffffff;
    heap[0] -= 1;
    let mut i = 1usize;
    loop {
        let mut j = i * 2;
        if j > heap[0] as usize {
            break;
        }
        if heap[j] > heap[j + 1] {
            j += 1;
        }
        if heap[i] < heap[j] {
            break;
        }
        heap.swap(i, j);
        i = j;
    }
    true
}

/// `checkTreePage`: confere uma página da árvore (sobreposição e cobertura de células e
/// blocos livres, ordem das chaves inteiras, listas de overflow, filhas recursivamente e
/// profundidade igual). Devolve a profundidade (raiz é 0).
fn check_tree_page(c: &mut CheckCtx, i_page: u32, pi_min_key: &mut i64, max_key: i64) -> i32 {
    let mut max_key = max_key;
    let mut depth: i32 = -1;
    let mut do_coverage_check = true; // verdadeiro se a cobertura de células deve ser conferida
    let mut key_can_be_equal = true; // IPK pode ser igual a maxKey (falso: estritamente menor)
    let saved_z_pfx = c.ck.z_pfx;
    let saved_v1 = c.ck.v1;
    let saved_v2 = c.ck.v2;
    let mut saved_is_init = false;
    let mut p_page: Option<PgId> = None;

    // Confere que a página existe.
    check_progress(c);
    'end_of_check: {
        if c.ck.mx_err == 0 {
            break 'end_of_check;
        }
        let usable: u32 = c.bt.usable_size;
        if i_page == 0 {
            return 0;
        }
        if check_ref(c, i_page) {
            return 0;
        }
        c.ck.z_pfx = "Tree %u page %u: ";
        c.ck.v1 = i_page;
        let pg = match btree_get_page(c.bt, i_page, 0) {
            Ok(p) => p,
            Err(rc) => {
                check_append_msg(
                    c,
                    b"unable to get the page. error code=%d",
                    &[PrintfArg::Int(rc as i64)],
                );
                if rc == SQLITE_IOERR_NOMEM {
                    c.ck.rc = SQLITE_NOMEM;
                }
                break 'end_of_check;
            }
        };
        p_page = Some(pg);

        // Zera `isInit` para que o código de detecção de corrupção de `btreeInitPage` rode.
        saved_is_init = c.bt.pager.page_extra(pg).is_init;
        c.bt.pager.page_extra_mut(pg).is_init = false;
        let rc = btree_init_page(c.bt, pg);
        if rc != 0 {
            check_append_msg(c, b"btreeInitPage() returns error code %d", &[PrintfArg::Int(rc as i64)]);
            break 'end_of_check;
        }
        let rc = btree_compute_free_space(c.bt, pg);
        if rc != 0 {
            check_append_msg(c, b"free space corruption", &[]);
            break 'end_of_check;
        }
        let data: Vec<u8> = c.bt.pager.page_data(pg).to_vec();
        let (hdr, leaf, int_key) = {
            let p = c.bt.pager.page_extra(pg);
            (p.hdr_offset as usize, p.leaf, p.int_key)
        };

        // Prepara a análise das células.
        c.ck.z_pfx = "Tree %u page %u cell %u: ";
        let content_offset: u32 = get2byte_not_zero(g2(&data, hdr + 5));

        let n_cell = g2(&data, hdr + 3) as i32;
        if leaf || !int_key {
            c.ck.n_row += n_cell as i64;
        }

        let cell_start: usize = hdr + 12 - 4 * leaf as usize;

        if !leaf {
            // Analisa a filha direita das páginas internas.
            let pgno = g4(&data, hdr + 8);
            if c.bt.auto_vacuum != 0 {
                c.ck.z_pfx = "Tree %u page %u right child: ";
                check_ptrmap(c, pgno, PTRMAP_BTREE, i_page);
            }
            let mk = max_key;
            depth = check_tree_page(c, pgno, &mut max_key, mk);
            key_can_be_equal = false;
        } else {
            // Nas folhas, a cobertura é conferida no mesmo laço das células: inicializa o heap.
            c.ck.heap[0] = 0;
        }

        // O índice de células tem K deslocamentos de 2 bytes para o conteúdo.
        let mut i: i32 = n_cell - 1;
        while i >= 0 && c.ck.mx_err != 0 {
            // Confere o tamanho da célula.
            c.ck.v2 = i;
            let pc: u32 = g2(&data, cell_start + i as usize * 2);
            if pc < content_offset || pc > usable - 4 {
                check_append_msg(
                    c,
                    b"Offset %u out of range %u..%u",
                    &[
                        PrintfArg::Int(pc as i64),
                        PrintfArg::Int(content_offset as i64),
                        PrintfArg::Int((usable - 4) as i64),
                    ],
                );
                do_coverage_check = false;
                i -= 1;
                continue;
            }
            let cell = data.get(pc as usize..).unwrap_or(&[]);
            let mut info = CellInfo::default();
            x_parse_cell(c.bt.pager.page_extra(pg), cell, 0, &mut info);
            if pc + info.n_size as u32 > usable {
                check_append_msg(c, b"Extends off end of page", &[]);
                do_coverage_check = false;
                i -= 1;
                continue;
            }

            // Chave inteira fora de ordem.
            if int_key {
                let bad = if key_can_be_equal { info.n_key > max_key } else { info.n_key >= max_key };
                if bad {
                    check_append_msg(c, b"Rowid %lld out of order", &[PrintfArg::Int(info.n_key)]);
                }
                max_key = info.n_key;
                key_can_be_equal = false; // só a primeira chave da página pode ser == maxKey
            }

            // Confere a lista de overflow do conteúdo.
            if info.n_payload > info.n_local as u32 {
                let n_page: u32 = info
                    .n_payload
                    .wrapping_sub(info.n_local as u32)
                    .wrapping_add(usable)
                    .wrapping_sub(5)
                    / (usable - 4);
                let pgno_ovfl = g4(&data, pc as usize + info.n_size as usize - 4);
                if c.bt.auto_vacuum != 0 {
                    check_ptrmap(c, pgno_ovfl, PTRMAP_OVERFLOW1, i_page);
                }
                check_list(c, false, pgno_ovfl, n_page);
            }

            if !leaf {
                // Confere a filha esquerda das páginas internas.
                let pgno = g4(&data, pc as usize);
                if c.bt.auto_vacuum != 0 {
                    check_ptrmap(c, pgno, PTRMAP_BTREE, i_page);
                }
                let mk = max_key;
                let d2 = check_tree_page(c, pgno, &mut max_key, mk);
                key_can_be_equal = false;
                if d2 != depth {
                    check_append_msg(c, b"Child page depth differs", &[]);
                    depth = d2;
                }
            } else {
                // Preenche o heap de cobertura nas folhas.
                btree_heap_insert(&mut c.ck.heap, (pc << 16) | (pc + info.n_size as u32 - 1));
            }
            i -= 1;
        }
        *pi_min_key = max_key;

        // Confere a cobertura completa da página.
        c.ck.z_pfx = "";
        if do_coverage_check && c.ck.mx_err > 0 {
            // Nas páginas internas o heap ainda não foi preenchido.
            if !leaf {
                c.ck.heap[0] = 0;
                let mut i = n_cell - 1;
                while i >= 0 {
                    let pc = g2(&data, cell_start + i as usize * 2);
                    let size = x_cell_size(
                        c.bt.pager.page_extra(pg),
                        data.get(pc as usize..).unwrap_or(&[]),
                        0,
                    ) as u32;
                    btree_heap_insert(&mut c.ck.heap, (pc << 16) | (pc + size - 1));
                    i -= 1;
                }
            }
            // Acrescenta os blocos livres ao heap. O segundo campo do cabeçalho é o offset do
            // primeiro bloco livre, ou zero.
            let mut i = g2(&data, hdr + 1) as i32;
            while i > 0 {
                let size = g2(&data, i as usize + 2) as i32;
                btree_heap_insert(&mut c.ck.heap, ((i as u32) << 16) | ((i + size - 1) as u32));
                // Os 2 primeiros bytes do bloco livre são o offset do próximo, ou zero.
                let j = g2(&data, i as usize) as i32;
                i = j;
            }
            // Analisa o heap atrás de sobreposição entre células e/ou blocos livres e conta os
            // bytes não rastreados em `n_frag`. Cada entrada é (início<<16)|fim; há uma entrada
            // implícita que cobre o cabeçalho, o índice de células e o espaço até o conteúdo.
            let mut n_frag: i32 = 0;
            let mut prev: u32 = content_offset - 1;
            let mut x: u32 = 0;
            while btree_heap_pull(&mut c.ck.heap, &mut x) {
                if (prev & 0xffff) >= (x >> 16) {
                    check_append_msg(
                        c,
                        b"Multiple uses for byte %u of page %u",
                        &[PrintfArg::Int((x >> 16) as i64), PrintfArg::Int(i_page as i64)],
                    );
                    break;
                } else {
                    n_frag += (x >> 16) as i32 - (prev & 0xffff) as i32 - 1;
                    prev = x;
                }
            }
            n_frag += usable as i32 - (prev & 0xffff) as i32 - 1;
            // O total de bytes fragmentados está no quinto campo do cabeçalho.
            let reported = data.get(hdr + 7).copied().unwrap_or(0) as i32;
            if c.ck.heap[0] == 0 && n_frag != reported {
                check_append_msg(
                    c,
                    b"Fragmentation of %u bytes reported as %u on page %u",
                    &[
                        PrintfArg::Int(n_frag as i64),
                        PrintfArg::Int(reported as i64),
                        PrintfArg::Int(i_page as i64),
                    ],
                );
            }
        }
    }

    // end_of_check
    if !do_coverage_check {
        if let Some(p) = p_page {
            c.bt.pager.page_extra_mut(p).is_init = saved_is_init;
        }
    }
    if let Some(p) = p_page {
        c.bt.pager.unref_not_null(p);
    }
    c.ck.z_pfx = saved_z_pfx;
    c.ck.v1 = saved_v1;
    c.ck.v2 = saved_v2;
    depth + 1
}

/// `sqlite3BtreeIntegrityCheck`: confere o arquivo todo. `a_root` são as raízes das tabelas; se
/// a primeira é 0 a lista é incompleta (verificação parcial: sem conferir a lista livre, salvo se
/// `a_root[1] == 1`, nem se toda página é referenciada). `a_cnt[i]` recebe a contagem de linhas
/// da i-ésima árvore. Devolve o código; `pn_err` recebe o número de erros e `pz_out` a mensagem
/// (`None` sem erros).
pub(crate) fn btree_integrity_check(
    db: IntegrityDb<'_>,
    p: &mut Btree,
    a_root: &[u32],
    a_cnt: &mut [Mem],
    mx_err: i32,
    pn_err: &mut i32,
    pz_out: &mut Option<Vec<u8>>,
) -> i32 {
    let n_root = a_root.len();
    let saved_db_flags = p.bt.db_flags;
    let mut b_partial = false; // verdadeiro se não confere todas as árvores
    let mut b_ck_freelist = true; // verdadeiro para varrer a lista livre

    // `aRoot[0]==0` quer dizer verificação parcial.
    if a_root.first().copied() == Some(0) {
        b_partial = true;
        if a_root.get(1).copied() != Some(1) {
            b_ck_freelist = false;
        }
    }

    let n_ck_page = p.bt.n_page;
    let mut err_msg = StrAccum::with_base(100, SQLITE_MAX_LENGTH as u32);
    err_msg.printf_flags = SQLITE_PRINTF_INTERNAL;
    let ck = IntegrityCk {
        a_pg_ref: Vec::new(),
        n_ck_page,
        mx_err,
        n_err: 0,
        rc: 0,
        n_step: 0,
        z_pfx: "",
        v0: 0,
        v1: 0,
        v2: 0,
        err_msg,
        heap: Vec::new(),
        n_row: 0,
    };
    let mut c = CheckCtx { ck, bt: &mut p.bt, db };

    'cleanup: {
        if c.ck.n_ck_page == 0 {
            break 'cleanup;
        }

        c.ck.a_pg_ref = vec![0u8; (n_ck_page / 8) as usize + 1];
        c.ck.heap = vec![0u32; (c.bt.page_size / 4) as usize + 2];

        let i = c.bt.pending_byte_page();
        if i <= c.ck.n_ck_page {
            set_page_referenced(&mut c, i);
        }

        let page1 = match c.bt.p_page1 {
            Some(p1) => p1,
            None => {
                c.ck.rc = SQLITE_CORRUPT_BKPT;
                break 'cleanup;
            }
        };

        // Confere a lista livre.
        if b_ck_freelist {
            c.ck.z_pfx = "Freelist: ";
            let (first, count) = {
                let d = c.bt.pager.page_data(page1);
                (g4(d, 32), g4(d, 36))
            };
            check_list(&mut c, true, first, count);
            c.ck.z_pfx = "";
        }

        // Confere todas as tabelas.
        if !b_partial {
            if c.bt.auto_vacuum != 0 {
                let mut mx: u32 = 0;
                for &r in a_root.iter() {
                    if mx < r {
                        mx = r;
                    }
                }
                let mx_in_hdr = g4(c.bt.pager.page_data(page1), 52);
                if mx != mx_in_hdr {
                    check_append_msg(
                        &mut c,
                        b"max rootpage (%u) disagrees with header (%u)",
                        &[PrintfArg::Int(mx as i64), PrintfArg::Int(mx_in_hdr as i64)],
                    );
                }
            } else if g4(c.bt.pager.page_data(page1), 64) != 0 {
                check_append_msg(
                    &mut c,
                    b"incremental_vacuum enabled with a max rootpage of zero",
                    &[],
                );
            }
        }
        c.bt.db_flags &= !SQLITE_CELL_SIZE_CK;
        let mut i = 0usize;
        while i < n_root && c.ck.mx_err != 0 {
            c.ck.n_row = 0;
            if a_root[i] != 0 {
                if c.bt.auto_vacuum != 0 && a_root[i] > 1 && !b_partial {
                    check_ptrmap(&mut c, a_root[i], PTRMAP_ROOTPAGE, 0);
                }
                c.ck.v0 = a_root[i];
                let mut not_used: i64 = 0;
                check_tree_page(&mut c, a_root[i], &mut not_used, LARGEST_INT64);
            }
            if let Some(m) = a_cnt.get_mut(i) {
                mem_set_int64(m, c.ck.n_row);
            }
            i += 1;
        }
        c.bt.db_flags = saved_db_flags;

        // Toda página do arquivo tem de ser referenciada.
        if !b_partial {
            let mut i = 1u32;
            while i <= c.ck.n_ck_page && c.ck.mx_err != 0 {
                // Em auto-vacuum, nenhuma tabela pode referenciar página do mapa de ponteiros.
                let is_ptrmap = ptrmap_pageno(c.bt, i) == i;
                if !get_page_referenced(&c, i) && (!is_ptrmap || c.bt.auto_vacuum == 0) {
                    check_append_msg(&mut c, b"Page %u: never used", &[PrintfArg::Int(i as i64)]);
                }
                if get_page_referenced(&c, i) && (is_ptrmap && c.bt.auto_vacuum != 0) {
                    check_append_msg(
                        &mut c,
                        b"Page %u: pointer map referenced",
                        &[PrintfArg::Int(i as i64)],
                    );
                }
                i += 1;
            }
        }
    }

    // integrity_ck_cleanup: devolve os erros.
    *pn_err = c.ck.n_err;
    if c.ck.n_err == 0 {
        c.ck.err_msg.reset();
        *pz_out = None;
    } else {
        *pz_out = c.ck.err_msg.finish();
    }
    c.ck.rc
}

// ---------------------------------------------------------------------------
// O resto do arquivo
// ---------------------------------------------------------------------------

/// `sqlite3BtreeTxnState`: `SQLITE_TXN_NONE`, `_READ` ou `_WRITE` (0 sem Btree).
pub(crate) fn btree_txn_state(p: Option<&Btree>) -> i32 {
    match p {
        Some(p) => p.in_trans as i32,
        None => 0,
    }
}

/// `sqlite3BtreeCheckpoint`: roda um checkpoint no Btree. `SQLITE_LOCKED` se há transação
/// aberta. `interrupt` é a consulta do sinal de interrupção da conexão.
pub(crate) fn btree_checkpoint(
    p: Option<&mut Btree>,
    interrupt: &mut dyn FnMut() -> i32,
    e_mode: i32,
    pn_log: Option<&mut i32>,
    pn_ckpt: Option<&mut i32>,
) -> i32 {
    match p {
        None => SQLITE_OK,
        Some(p) => {
            if p.bt.in_transaction != TRANS_NONE {
                SQLITE_LOCKED
            } else {
                p.bt.pager.checkpoint(interrupt, e_mode, pn_log, pn_ckpt)
            }
        }
    }
}

/// `sqlite3BtreeSchema`: o bloco de memória que o código de esquema associa ao Btree. Na
/// primeira chamada com `init` (`nBytes != 0` no C) o valor é guardado; as seguintes ignoram
/// `init` e devolvem o mesmo bloco. Sem `init` e sem bloco devolve `None`. O destrutor
/// (`xFree`) é o `Drop` do `Box`.
pub(crate) fn btree_schema(p: &mut Btree, init: Option<Box<dyn Any>>) -> Option<&mut Box<dyn Any>> {
    if p.bt.p_schema.is_none() {
        if let Some(v) = init {
            p.bt.p_schema = Some(v);
        }
    }
    p.bt.p_schema.as_mut()
}

/// `sqlite3BtreeSchemaLocked`: `SQLITE_LOCKED_SHAREDCACHE` se outro usuário do mesmo btree
/// tem trava exclusiva em sqlite_schema; senão `SQLITE_OK`.
pub(crate) fn btree_schema_locked(p: &mut Btree) -> i32 {
    query_shared_cache_table_lock(p, SCHEMA_ROOT, READ_LOCK)
}

/// `sqlite3BtreeLockTable`: trava a tabela de raiz `i_tab` (de escrita se `is_write_lock`).
pub(crate) fn btree_lock_table(p: &mut Btree, i_tab: i32, is_write_lock: u8) -> i32 {
    let mut rc = SQLITE_OK;
    if p.sharable {
        let lock_type: u8 = READ_LOCK + is_write_lock;
        rc = query_shared_cache_table_lock(p, i_tab as u32, lock_type);
        if rc == SQLITE_OK {
            rc = set_shared_cache_table_lock(p, i_tab as u32, lock_type);
        }
    }
    rc
}

/// `sqlite3BtreePutData`: modifica o conteúdo (sem mudar o tamanho) da entrada de uma tabela
/// INTKEY aberta para escrita e como incrblob. `SQLITE_CORRUPT` se `offset + amt` passa do fim.
pub(crate) fn btree_put_data(
    csr: &mut BtCursor,
    bt: &mut BtShared,
    offset: u32,
    amt: u32,
    z: &[u8],
) -> i32 {
    let rc = if csr.e_state >= CURSOR_REQUIRESEEK {
        btree_restore_cursor_position(csr, bt)
    } else {
        SQLITE_OK
    };
    if rc != SQLITE_OK {
        return rc;
    }
    if csr.e_state != CURSOR_VALID {
        return SQLITE_ABORT;
    }

    // Salva a posição dos outros cursores da tabela (podem segurar referência à página que
    // `accessPayload` vai modificar). Em tabela INTKEY isso não pode falhar.
    let _ = save_all_cursors(bt, csr.pgno_root, None);

    // O cursor tem de estar aberto para escrita.
    if (csr.cur_flags & BTCF_WRITE_FLAG) == 0 {
        return SQLITE_READONLY;
    }

    let mut buf = z.to_vec();
    access_payload(csr, bt, offset, amt, &mut buf, 1)
}

/// `sqlite3BtreeIncrblobCursor`: marca o cursor como cursor de blob incremental.
pub(crate) fn btree_incrblob_cursor(cur: &mut BtCursor, p: &mut Btree) {
    cur.cur_flags |= BTCF_INCRBLOB;
    p.has_incrblob_cur = true;
}

/// `sqlite3BtreeSetVersion`: grava `i_version` (1 ou 2) nos campos de versão de leitura
/// (byte 18) e de escrita (byte 19) do cabeçalho.
pub(crate) fn btree_set_version(p: &mut Btree, i_version: i32, db: &mut BtDb<'_>) -> i32 {
    // Com a versão 1 não abre a conexão WAL, mesmo se hoje está em 2.
    p.bt.bts_flags &= !BTS_NO_WAL;
    if i_version == 1 {
        p.bt.bts_flags |= BTS_NO_WAL;
    }

    let mut rc = btree_begin_trans(p, 0, None, db);
    if rc == SQLITE_OK {
        let p1 = match p.bt.p_page1 {
            Some(p1) => p1,
            None => return SQLITE_CORRUPT_BKPT,
        };
        let (a18, a19) = {
            let d = p.bt.pager.page_data(p1);
            (d.get(18).copied().unwrap_or(0), d.get(19).copied().unwrap_or(0))
        };
        if a18 != i_version as u8 || a19 != i_version as u8 {
            rc = btree_begin_trans(p, 2, None, db);
            if rc == SQLITE_OK {
                rc = p.bt.pager.write(p1);
                if rc == SQLITE_OK {
                    let d = p.bt.pager.page_data_mut(p1);
                    d[18] = i_version as u8;
                    d[19] = i_version as u8;
                }
            }
        }
    }

    p.bt.bts_flags &= !BTS_NO_WAL;
    rc
}

/// `sqlite3BtreeClearCache`: sem transação ativa (e fora de banco temporário), esvazia o
/// cache de páginas do pager.
pub(crate) fn btree_clear_cache(p: &mut Btree) {
    if p.bt.in_transaction == TRANS_NONE {
        p.bt.pager.clear_cache();
    }
}
