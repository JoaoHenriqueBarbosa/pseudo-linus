//! btree.c, chunks 000 a 008: travas de tabela, cursores salvos, mapa de ponteiros, análise de
//! células, espaço livre da página, inicialização de páginas, abertura e fechamento do `Btree`,
//! parâmetros do pager e do formato (tamanho de página, auto-vacuum, secure delete), `lockBtree`,
//! `unlockBtreeIfUnused` e `newDatabase`.
//!
//! Modelo (CONVENTIONS.md e btree_types.rs):
//!
//! * `MemPage` do C é um `PgId`. Onde o C lê ou escreve `pPage->aData[...]`, o Rust usa
//!   `bt.pager.page_parts(pg)` (metadados e bytes ao mesmo tempo). Os metadados que o C lia por
//!   `pPage->pBt` (`usableSize`, `maxLocal`...) estão em `MemPage.geom` (`PageGeom`), copiados do
//!   `BtShared` por `btree_init_page` e `zero_page`.
//! * Funções de célula não recebem `BtShared`: recebem `(page: &MemPage, buf: &[u8], cell: usize)`
//!   onde `buf` é a página inteira (ou o buffer solto da célula) e `cell` o offset do início da
//!   célula em `buf`. `CellInfo.p_payload` é offset em `buf`.
//! * Funções de cursor recebem `(cur: &mut BtCursor, bt: &mut BtShared, ...)`; o cursor não está no
//!   slab enquanto elas rodam.
//! * Páginas devolvidas por `btree_get_page` e afins levam uma referência contada no pager; quem
//!   as obtém as solta com `release_page*`.
//!
//! Sumiram (SQLITE_DEBUG ou mutex ou cache compartilhado): `hasSharedCacheTableLock`,
//! `hasReadConflicts`, `corruptPageError`, `cursorHoldsMutex`, `cursorOwnsBtShared`,
//! `countValidCursors`, `sqlite3BtreeSeekCount`, `sharedLockTrace`, `removeFromSharingList`,
//! `btreePagecount` (é `bt.n_page`), `btreeInvokeBusyHandler`, `setDefaultSyncFlag` (no Debian
//! `SQLITE_DEFAULT_SYNCHRONOUS == SQLITE_DEFAULT_WAL_SYNCHRONOUS`, então o C o define vazio).
//!
//! Leituras de bytes de página que dependem de conteúdo possivelmente corrompido usam `byte` e
//! `rd2`, que enxergam zero além do fim do buffer (o C leria memória vizinha).

use crate::bitvec::{bitvec_create, bitvec_set, bitvec_size, bitvec_test_not_null};
use crate::btree_types::{
    ptrmap_ptroffset, BtCursor, BtLock, BtShared, Btree, CellInfo, CursorId, MemPage, PageGeom,
    Slab,
};
use crate::consts::{
    BTCF_AT_LAST, BTCF_INCRBLOB, BTCF_MULTIPLE, BTCF_PINNED, BTCF_VALID_NKEY, BTCF_VALID_OVFL,
    BTREE_AUTOVACUUM_FULL, BTREE_AUTOVACUUM_INCR, BTREE_AUTOVACUUM_NONE, BTREE_MEMORY,
    BTS_EXCLUSIVE, BTS_FAST_SECURE, BTS_NO_WAL, BTS_PAGESIZE_FIXED, BTS_PENDING, BTS_READ_ONLY,
    BTS_SECURE_DELETE, CURSOR_FAULT, CURSOR_INVALID, CURSOR_REQUIRESEEK, CURSOR_SKIPNEXT,
    CURSOR_VALID, PTF_INTKEY, PTF_LEAF, PTF_LEAFDATA, PTF_ZERODATA, PTRMAP_OVERFLOW1,
    READ_LOCK, SQLITE_CONSTRAINT_PINNED, SQLITE_CORRUPT_BKPT, SQLITE_DEFAULT_CACHE_SIZE,
    SQLITE_ERROR, SQLITE_FILE_HEADER, SQLITE_LOCKED_SHAREDCACHE, SQLITE_MAX_PAGE_SIZE,
    SQLITE_NOMEM_BKPT, SQLITE_NOTADB, SQLITE_OK, SQLITE_OPEN_MAIN_DB, SQLITE_OPEN_MEMORY,
    SQLITE_OPEN_TEMP_DB, SQLITE_READONLY, SQLITE_RESET_DATABASE, SQLITE_WRITE_SCHEMA,
    SQLITE_DEFENSIVE, TRANS_NONE, WRITE_LOCK, PAGER_GET_READONLY,
};
use crate::mem::UnpackedRecord;
use crate::os::VfsRef;
use crate::pager_ext::{pager_open, PagerCloseDb};
use crate::pcache::PgId;
use crate::record::{alloc_unpacked_record, record_unpack};
use crate::util::{get4byte, get_varint, put2byte, put4byte};

// ---------------------------------------------------------------------------------------------
// Auxiliares de leitura segura
// ---------------------------------------------------------------------------------------------

/// Byte `i` de `buf`, ou zero além do fim.
#[inline]
fn byte(buf: &[u8], i: usize) -> u8 {
    buf.get(i).copied().unwrap_or(0)
}

/// Inteiro big-endian de dois bytes em `buf[i..i+2]`, zero além do fim.
#[inline]
fn rd2(buf: &[u8], i: usize) -> u32 {
    ((byte(buf, i) as u32) << 8) | byte(buf, i + 1) as u32
}

/// `get2byteNotZero(X)`: como `get2byte`, mas zero vale 65536.
#[inline]
fn get2byte_not_zero(buf: &[u8], i: usize) -> i32 {
    (((rd2(buf, i) as i32) - 1) & 0xffff) + 1
}

/// Os bytes de `z` até o primeiro NUL.
fn until_nul(z: &[u8]) -> &[u8] {
    match z.iter().position(|&b| b == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// `PageGeom` com os valores vigentes do `BtShared`.
pub(crate) fn page_geom_of(bt: &BtShared) -> PageGeom {
    PageGeom {
        page_size: bt.page_size,
        usable_size: bt.usable_size,
        max_local: bt.max_local,
        min_local: bt.min_local,
        max_leaf: bt.max_leaf,
        min_leaf: bt.min_leaf,
        max1byte_payload: bt.max1byte_payload,
        cell_size_ck: bt.db_flags & crate::consts::SQLITE_CELL_SIZE_CK != 0,
    }
}

// ---------------------------------------------------------------------------------------------
// Travas de tabela do cache compartilhado (chunks 000 e 001)
// ---------------------------------------------------------------------------------------------

/// `querySharedCacheTableLock`: `p` pode obter a trava `e_lock` na tabela de raiz `i_table`?
/// Com um dono só por `BtShared`, nenhuma trava de outra conexão existe: o laço sobre
/// `pIter->pBtree != p` do C nunca casa e some; resta o teste do escritor exclusivo.
pub(crate) fn query_shared_cache_table_lock(p: &mut Btree, i_table: u32, e_lock: u8) -> i32 {
    debug_assert!(e_lock == READ_LOCK || e_lock == WRITE_LOCK);
    let _ = (i_table, e_lock);
    if !p.sharable {
        return SQLITE_OK;
    }
    // `pBt->pWriter != p`: com um dono, o escritor, se existe, é o próprio `p`.
    if !p.bt.has_writer && (p.bt.bts_flags & BTS_EXCLUSIVE) != 0 {
        return SQLITE_LOCKED_SHAREDCACHE;
    }
    SQLITE_OK
}

/// `setSharedCacheTableLock`: acrescenta (ou sobe) a trava `e_lock` em `i_table`.
pub(crate) fn set_shared_cache_table_lock(p: &mut Btree, i_table: u32, e_lock: u8) -> i32 {
    debug_assert!(e_lock == READ_LOCK || e_lock == WRITE_LOCK);
    debug_assert!(p.sharable);
    let bt = &mut p.bt;
    let idx = match bt.lock_list.iter().position(|l| l.i_table == i_table) {
        Some(i) => i,
        None => {
            // O C liga a trava nova no início da lista.
            bt.lock_list.insert(0, BtLock { i_table, e_lock: 0 });
            0
        }
    };
    if e_lock > bt.lock_list[idx].e_lock {
        bt.lock_list[idx].e_lock = e_lock;
    }
    SQLITE_OK
}

/// `clearAllSharedCacheTableLocks`: solta todas as travas de tabela de `p`.
pub(crate) fn clear_all_shared_cache_table_locks(p: &mut Btree) {
    let bt = &mut p.bt;
    bt.lock_list.clear();
    if bt.has_writer {
        bt.has_writer = false;
        bt.bts_flags &= !(BTS_EXCLUSIVE | BTS_PENDING);
    } else if bt.n_transaction == 2 {
        bt.bts_flags &= !BTS_PENDING;
    }
}

/// `downgradeAllSharedCacheTableLocks`: troca as travas de escrita de `p` por de leitura.
pub(crate) fn downgrade_all_shared_cache_table_locks(p: &mut Btree) {
    let bt = &mut p.bt;
    if bt.has_writer {
        bt.has_writer = false;
        bt.bts_flags &= !(BTS_EXCLUSIVE | BTS_PENDING);
        for l in bt.lock_list.iter_mut() {
            l.e_lock = READ_LOCK;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Cache de overflow, incrblob, pHasContent (chunk 001)
// ---------------------------------------------------------------------------------------------

/// `invalidateOverflowCache(pCur)`.
#[inline]
pub(crate) fn invalidate_overflow_cache(cur: &mut BtCursor) {
    cur.cur_flags &= !BTCF_VALID_OVFL;
}

/// `invalidateAllOverflowCache`: invalida o cache de overflow de todos os cursores do slab.
pub(crate) fn invalidate_all_overflow_cache(bt: &mut BtShared) {
    for (_, c) in bt.cursors.iter_mut() {
        invalidate_overflow_cache(c);
    }
}

/// `invalidateIncrblobCursors`: invalida os cursores incrblob sobre a linha (ou a tabela).
pub(crate) fn invalidate_incrblob_cursors(
    p: &mut Btree,
    pgno_root: u32,
    i_row: i64,
    is_clear_table: bool,
) {
    p.has_incrblob_cur = false;
    let mut has = false;
    for (_, c) in p.bt.cursors.iter_mut() {
        if (c.cur_flags & BTCF_INCRBLOB) != 0 {
            has = true;
            if c.pgno_root == pgno_root && (is_clear_table || c.info.n_key == i_row) {
                c.e_state = CURSOR_INVALID;
            }
        }
    }
    p.has_incrblob_cur = has;
}

/// `btreeSetHasContent`: liga o bit `pgno` de `pHasContent`.
pub(crate) fn btree_set_has_content(bt: &mut BtShared, pgno: u32) -> i32 {
    if bt.p_has_content.is_none() {
        debug_assert!(pgno <= bt.n_page);
        bt.p_has_content = Some(bitvec_create(bt.n_page));
    }
    let size = bt.p_has_content.as_deref().map_or(0, bitvec_size);
    if pgno <= size {
        return bitvec_set(bt.p_has_content.as_deref_mut(), pgno);
    }
    SQLITE_OK
}

/// `btreeGetHasContent`: falso se é seguro buscar a página com `NOCONTENT`.
pub(crate) fn btree_get_has_content(bt: &BtShared, pgno: u32) -> bool {
    match bt.p_has_content.as_deref() {
        Some(p) => pgno > bitvec_size(p) || bitvec_test_not_null(p, pgno),
        None => false,
    }
}

/// `btreeClearHasContent`.
pub(crate) fn btree_clear_has_content(bt: &mut BtShared) {
    bt.p_has_content = None;
}

/// `btreeReleaseAllCursorPages`: solta as páginas de `apPage[]` e `pPage`.
pub(crate) fn btree_release_all_cursor_pages(cur: &mut BtCursor, bt: &mut BtShared) {
    if cur.i_page >= 0 {
        for i in 0..cur.i_page as usize {
            if let Some(pg) = cur.ap_page[i].take() {
                release_page_not_null(bt, pg);
            }
        }
        if let Some(pg) = cur.p_page.take() {
            release_page_not_null(bt, pg);
        }
        cur.i_page = -1;
    }
}

/// `saveCursorKey`: guarda a chave corrente em `n_key` e `bt_key`.
pub(crate) fn save_cursor_key(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    debug_assert!(cur.e_state == CURSOR_VALID);
    debug_assert!(cur.bt_key.is_none());
    let mut rc = SQLITE_OK;
    if cur.cur_int_key != 0 {
        // Em tabela só o rowid é necessário.
        cur.n_key = crate::btree_cursor::btree_integer_key(cur, bt);
    } else {
        // Em índice guarda a chave inteira, mais 9+8 bytes zerados: a chave pode estar
        // corrompida e o desempacotamento ler além do fim.
        cur.n_key = crate::btree_cursor::btree_payload_size(cur, bt) as i64;
        let n = cur.n_key as usize;
        let mut key: Vec<u8> = Vec::new();
        if n + 9 + 8 > 0x7fffff00 || key.try_reserve_exact(n + 9 + 8).is_err() {
            rc = SQLITE_NOMEM_BKPT;
        } else {
            key.resize(n + 9 + 8, 0);
            rc = crate::btree_cursor::btree_payload(cur, bt, 0, n as u32, &mut key[..n]);
            if rc == SQLITE_OK {
                cur.bt_key = Some(key);
            }
        }
    }
    debug_assert!(cur.cur_int_key == 0 || cur.bt_key.is_none());
    rc
}

// ---------------------------------------------------------------------------------------------
// Cursores salvos (chunk 002)
// ---------------------------------------------------------------------------------------------

/// `saveCursorPosition`: guarda a posição do cursor e muda o estado para `CURSOR_REQUIRESEEK`.
pub(crate) fn save_cursor_position(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    debug_assert!(cur.e_state == CURSOR_VALID || cur.e_state == CURSOR_SKIPNEXT);
    debug_assert!(cur.bt_key.is_none());
    if (cur.cur_flags & BTCF_PINNED) != 0 {
        return SQLITE_CONSTRAINT_PINNED;
    }
    if cur.e_state == CURSOR_SKIPNEXT {
        cur.e_state = CURSOR_VALID;
    } else {
        cur.skip_next = 0;
    }
    let rc = save_cursor_key(cur, bt);
    if rc == SQLITE_OK {
        btree_release_all_cursor_pages(cur, bt);
        cur.e_state = CURSOR_REQUIRESEEK;
    }
    cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL | BTCF_AT_LAST);
    rc
}

/// Os cursores do slab que `saveAllCursors` deve tratar.
fn cursors_to_save(bt: &BtShared, i_root: u32, except: Option<CursorId>) -> Vec<CursorId> {
    bt.cursors
        .iter()
        .filter(|(id, c)| Some(*id) != except && (i_root == 0 || c.pgno_root == i_root))
        .map(|(id, _)| id)
        .collect()
}

/// `saveCursorsOnList`: o trabalho de `saveAllCursors` sobre os cursores escolhidos.
fn save_cursors_on_list(bt: &mut BtShared, ids: Vec<CursorId>) -> i32 {
    for id in ids {
        let mut c = match bt.cursors.take(id) {
            Some(c) => c,
            None => continue,
        };
        let rc = if c.e_state == CURSOR_VALID || c.e_state == CURSOR_SKIPNEXT {
            save_cursor_position(&mut c, bt)
        } else {
            btree_release_all_cursor_pages(&mut c, bt);
            SQLITE_OK
        };
        bt.cursors.put(id, c);
        if rc != SQLITE_OK {
            return rc;
        }
    }
    SQLITE_OK
}

/// `saveAllCursors`: salva a posição de todos os cursores (menos `except`) sobre a raiz
/// `i_root` (todos se zero). O cursor corrente de quem chama está fora do slab e portanto
/// nunca é visto; use `save_all_cursors_cur` para também limpar `BTCF_Multiple` dele.
pub(crate) fn save_all_cursors(bt: &mut BtShared, i_root: u32, except: Option<CursorId>) -> i32 {
    let ids = cursors_to_save(bt, i_root, except);
    if ids.is_empty() {
        return SQLITE_OK;
    }
    save_cursors_on_list(bt, ids)
}

/// `saveAllCursors(pBt, iRoot, pCur)` para um cursor `cur` que está fora do slab: igual a
/// `save_all_cursors`, e se nenhum outro cursor foi achado limpa `BTCF_Multiple` em `cur`.
pub(crate) fn save_all_cursors_cur(cur: &mut BtCursor, bt: &mut BtShared, i_root: u32) -> i32 {
    let ids = cursors_to_save(bt, i_root, None);
    if ids.is_empty() {
        cur.cur_flags &= !BTCF_MULTIPLE;
        return SQLITE_OK;
    }
    save_cursors_on_list(bt, ids)
}

/// `sqlite3BtreeClearCursor`.
pub(crate) fn btree_clear_cursor(cur: &mut BtCursor) {
    cur.bt_key = None;
    cur.e_state = CURSOR_INVALID;
}

/// `btreeMoveto`: move o cursor a uma chave (registro empacotado em índices, rowid em tabelas).
pub(crate) fn btree_moveto(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    p_key: Option<&[u8]>,
    n_key: i64,
    bias: i32,
    p_res: &mut i32,
) -> i32 {
    match p_key {
        Some(key) => {
            let ki = match cur.p_key_info.clone() {
                Some(k) => k,
                None => return SQLITE_CORRUPT_BKPT,
            };
            let n = (n_key.max(0) as usize).min(key.len());
            let mut idx: UnpackedRecord = alloc_unpacked_record(ki.clone());
            record_unpack(&ki, &key[..n], &mut idx);
            if idx.n_field == 0 || idx.n_field > ki.n_all_field {
                SQLITE_CORRUPT_BKPT
            } else {
                crate::btree_cursor::btree_index_moveto(cur, bt, &mut idx, p_res)
            }
        }
        None => crate::btree_cursor::btree_table_moveto(cur, bt, n_key, bias, p_res),
    }
}

/// `btreeRestoreCursorPosition`.
pub(crate) fn btree_restore_cursor_position(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    let mut skip_next = 0i32;
    debug_assert!(cur.e_state >= CURSOR_REQUIRESEEK);
    if cur.e_state == CURSOR_FAULT {
        return cur.skip_next;
    }
    cur.e_state = CURSOR_INVALID;
    let key = cur.bt_key.take();
    let n_key = cur.n_key;
    let rc = btree_moveto(cur, bt, key.as_deref(), n_key, 0, &mut skip_next);
    if rc == SQLITE_OK {
        debug_assert!(cur.e_state == CURSOR_VALID || cur.e_state == CURSOR_INVALID);
        if skip_next != 0 {
            cur.skip_next = skip_next;
        }
        if cur.skip_next != 0 && cur.e_state == CURSOR_VALID {
            cur.e_state = CURSOR_SKIPNEXT;
        }
    } else {
        cur.bt_key = key;
    }
    rc
}

/// `restoreCursorPosition(p)`.
#[inline]
pub(crate) fn restore_cursor_position(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    if cur.e_state >= CURSOR_REQUIRESEEK {
        btree_restore_cursor_position(cur, bt)
    } else {
        SQLITE_OK
    }
}

/// `sqlite3BtreeCursorHasMoved`: o cursor saiu de onde foi posto (ou foi invalidado)?
#[inline]
pub(crate) fn btree_cursor_has_moved(cur: &BtCursor) -> bool {
    cur.e_state != CURSOR_VALID
}

/// `sqlite3BtreeFakeValidCursor`: um cursor que sempre responde "não se moveu" a
/// `btree_cursor_has_moved`. Não pode ser usado em nenhuma outra função do btree.
pub(crate) fn btree_fake_valid_cursor() -> BtCursor {
    BtCursor { e_state: CURSOR_VALID, ..BtCursor::default() }
}

/// `sqlite3BtreeCursorRestore`.
pub(crate) fn btree_cursor_restore(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    p_different_row: &mut i32,
) -> i32 {
    debug_assert!(cur.e_state != CURSOR_VALID);
    let rc = restore_cursor_position(cur, bt);
    if rc != SQLITE_OK {
        *p_different_row = 1;
        return rc;
    }
    *p_different_row = if cur.e_state != CURSOR_VALID { 1 } else { 0 };
    SQLITE_OK
}

/// `sqlite3BtreeCursorHintFlags`.
pub(crate) fn btree_cursor_hint_flags(cur: &mut BtCursor, x: u32) {
    debug_assert!(
        x == crate::consts::BTREE_SEEK_EQ || x == crate::consts::BTREE_BULKLOAD || x == 0
    );
    cur.hints = x as u8;
}

// ---------------------------------------------------------------------------------------------
// Mapa de ponteiros (chunks 002 e 003)
// ---------------------------------------------------------------------------------------------

/// `ptrmapPageno` (e `PTRMAP_PAGENO`): página do mapa de ponteiros que cobre `pgno`.
pub(crate) fn ptrmap_pageno(bt: &BtShared, pgno: u32) -> u32 {
    if pgno < 2 {
        return 0;
    }
    let n_pages_per_map_page = bt.usable_size / 5 + 1;
    let i_ptr_map = (pgno - 2) / n_pages_per_map_page;
    let mut ret = i_ptr_map.wrapping_mul(n_pages_per_map_page).wrapping_add(2);
    if ret == bt.pending_byte_page() {
        ret += 1;
    }
    ret
}

/// `PTRMAP_ISPAGE(pBt, pgno)`.
#[inline]
pub(crate) fn ptrmap_ispage(bt: &BtShared, pgno: u32) -> bool {
    ptrmap_pageno(bt, pgno) == pgno
}

/// `ptrmapPut`: grava a entrada `(e_type, parent)` de `key`. Se `*rc != 0` não faz nada.
pub(crate) fn ptrmap_put(bt: &mut BtShared, key: u32, e_type: u8, parent: u32, rc: &mut i32) {
    if *rc != SQLITE_OK {
        return;
    }
    debug_assert!(bt.auto_vacuum != 0);
    if key == 0 {
        *rc = SQLITE_CORRUPT_BKPT;
        return;
    }
    let i_ptrmap = ptrmap_pageno(bt, key);
    let pg = match bt.pager.get(i_ptrmap, 0) {
        Ok(p) => p,
        Err(e) => {
            *rc = e;
            return;
        }
    };
    'exit: {
        // O primeiro byte do extra é `isInit`: ligado, a página também é de árvore-b.
        if bt.pager.page_extra(pg).is_init {
            *rc = SQLITE_CORRUPT_BKPT;
            break 'exit;
        }
        let offset = ptrmap_ptroffset(i_ptrmap, key) as i32;
        if offset < 0 || offset as usize + 5 > bt.pager.page_data(pg).len() {
            *rc = SQLITE_CORRUPT_BKPT;
            break 'exit;
        }
        let off = offset as usize;
        let differs = {
            let data = bt.pager.page_data(pg);
            e_type != data[off] || get4byte(&data[off + 1..]) != parent
        };
        if differs {
            let r = bt.pager.write(pg);
            *rc = r;
            if r == SQLITE_OK {
                let data = bt.pager.page_data_mut(pg);
                data[off] = e_type;
                put4byte(&mut data[off + 1..], parent);
            }
        }
    }
    bt.pager.unref_not_null(pg);
}

/// `ptrmapGet`: lê a entrada de `key`.
pub(crate) fn ptrmap_get(
    bt: &mut BtShared,
    key: u32,
    p_e_type: &mut u8,
    p_pgno: Option<&mut u32>,
) -> i32 {
    let i_ptrmap = ptrmap_pageno(bt, key);
    let pg = match bt.pager.get(i_ptrmap, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let offset = ptrmap_ptroffset(i_ptrmap, key) as i32;
    if offset < 0 || offset as usize + 5 > bt.pager.page_data(pg).len() {
        bt.pager.unref_not_null(pg);
        return SQLITE_CORRUPT_BKPT;
    }
    let off = offset as usize;
    {
        let data = bt.pager.page_data(pg);
        *p_e_type = data[off];
        if let Some(p) = p_pgno {
            *p = get4byte(&data[off + 1..]);
        }
    }
    bt.pager.unref_not_null(pg);
    if *p_e_type < 1 || *p_e_type > 5 {
        return SQLITE_CORRUPT_BKPT;
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Células (chunks 003 e 004)
// ---------------------------------------------------------------------------------------------

/// `findCell(P, I)`: offset da célula `i` na página (só para páginas sem células em overflow).
#[inline]
pub(crate) fn find_cell(page: &MemPage, data: &[u8], i: i32) -> usize {
    (page.mask_page as u32 & rd2(data, page.a_cell_idx + 2 * i as usize)) as usize
}

/// `findCellPastPtr(P, I)`: como `find_cell`, já depois do ponteiro de filho de 4 bytes.
#[inline]
pub(crate) fn find_cell_past_ptr(page: &MemPage, data: &[u8], i: i32) -> usize {
    page.a_data_ofst + (page.mask_page as u32 & rd2(data, page.a_cell_idx + 2 * i as usize)) as usize
}

/// Divisor `usableSize - 4` das contas de overflow.
#[inline]
fn ovfl_divisor(page: &MemPage) -> u32 {
    page.geom.usable_size.wrapping_sub(4).max(1)
}

/// Lê o varint de 32 bits do tamanho do payload (laço do C, até 9 bytes) em `buf[pos..]`.
/// Devolve o valor e a posição seguinte.
fn payload_varint(buf: &[u8], pos: usize) -> (u32, usize) {
    let mut it = pos;
    let mut n = byte(buf, it) as u32;
    if n >= 0x80 {
        let end = it + 8;
        n &= 0x7f;
        loop {
            it += 1;
            n = (n << 7) | (byte(buf, it) as u32 & 0x7f);
            if !(byte(buf, it) >= 0x80 && it < end) {
                break;
            }
        }
    }
    (n, it + 1)
}

/// Parte comum de `btreeParseCellPtr`, `btreeParseCellPtrIndex` e `cellSizePtr*`: dado o
/// tamanho do payload e `hdr = pIter - pCell`, devolve `(nLocal, nSize)`. Cobre também
/// `btreeParseCellAdjustSizeForOverflow`.
fn payload_local_and_size(page: &MemPage, n_payload: u32, hdr: usize) -> (u16, u16) {
    if n_payload <= page.max_local as u32 {
        let mut n_size = n_payload.wrapping_add(hdr as u16 as u32) as u16;
        if n_size < 4 {
            n_size = 4;
        }
        (n_payload as u16, n_size)
    } else {
        let min_local = page.min_local as u32;
        let surplus = min_local + n_payload.wrapping_sub(min_local) % ovfl_divisor(page);
        let n_local = if surplus <= page.max_local as u32 { surplus as u16 } else { min_local as u16 };
        let n_size = ((hdr + n_local as usize) as u16).wrapping_add(4);
        (n_local, n_size)
    }
}

/// `btreePayloadToLocal`: bytes do payload guardados na própria página.
pub(crate) fn btree_payload_to_local(page: &MemPage, n_payload: i64) -> i32 {
    let max_local = page.max_local as i64;
    if n_payload <= max_local {
        n_payload as i32
    } else {
        let min_local = page.min_local as i64;
        let surplus = min_local + (n_payload - min_local) % ovfl_divisor(page) as i64;
        (if surplus <= max_local { surplus } else { min_local }) as i32
    }
}

/// `btreeParseCellPtrNoPayload`: nó interno de tabela.
pub(crate) fn btree_parse_cell_ptr_no_payload(
    page: &MemPage,
    buf: &[u8],
    cell: usize,
    info: &mut CellInfo,
) {
    debug_assert!(!page.leaf);
    debug_assert!(page.child_ptr_size == 4);
    let _ = page;
    let tail = buf.get(cell + 4..).unwrap_or(&[]);
    let (n, v) = get_varint(tail);
    info.n_key = v as i64;
    info.n_size = 4 + n as u16;
    info.n_payload = 0;
    info.n_local = 0;
    info.p_payload = 0;
}

/// `btreeParseCellPtr`: folha de tabela.
pub(crate) fn btree_parse_cell_ptr(page: &MemPage, buf: &[u8], cell: usize, info: &mut CellInfo) {
    debug_assert!(page.int_key_leaf);
    debug_assert!(page.child_ptr_size == 0);
    let (n_payload, mut it) = payload_varint(buf, cell);

    // Varint de 64 bits da chave, com o laço desenrolado do C.
    let mut i_key = byte(buf, it) as u64;
    'key: {
        if i_key < 0x80 {
            break 'key;
        }
        it += 1;
        let mut x = byte(buf, it);
        i_key = (i_key << 7) ^ x as u64;
        if x < 0x80 {
            i_key ^= 0x4000;
            break 'key;
        }
        it += 1;
        x = byte(buf, it);
        i_key = (i_key << 7) ^ x as u64;
        if x < 0x80 {
            i_key ^= 0x204000;
            break 'key;
        }
        it += 1;
        x = byte(buf, it);
        i_key = (i_key << 7) ^ 0x10204000 ^ x as u64;
        if x < 0x80 {
            break 'key;
        }
        for _ in 0..4 {
            it += 1;
            x = byte(buf, it);
            i_key = (i_key << 7) ^ 0x4000 ^ x as u64;
            if x < 0x80 {
                break 'key;
            }
        }
        it += 1;
        i_key = (i_key << 8) ^ 0x8000 ^ byte(buf, it) as u64;
    }
    it += 1;

    info.n_key = i_key as i64;
    info.n_payload = n_payload;
    info.p_payload = it;
    let (n_local, n_size) = payload_local_and_size(page, n_payload, it - cell);
    info.n_local = n_local;
    info.n_size = n_size;
}

/// `btreeParseCellPtrIndex`: nós de índice.
pub(crate) fn btree_parse_cell_ptr_index(
    page: &MemPage,
    buf: &[u8],
    cell: usize,
    info: &mut CellInfo,
) {
    debug_assert!(!page.int_key_leaf);
    let (n_payload, it) = payload_varint(buf, cell + page.child_ptr_size as usize);
    info.n_key = n_payload as i64;
    info.n_payload = n_payload;
    info.p_payload = it;
    let (n_local, n_size) = payload_local_and_size(page, n_payload, it - cell);
    info.n_local = n_local;
    info.n_size = n_size;
}

/// `pPage->xParseCell(pPage, pCell, pInfo)`: despacha pelo tipo da página (o `decodeFlags`
/// escolhia a função; aqui a escolha sai de `int_key` e `leaf`).
pub(crate) fn x_parse_cell(page: &MemPage, buf: &[u8], cell: usize, info: &mut CellInfo) {
    if page.int_key {
        if page.leaf {
            btree_parse_cell_ptr(page, buf, cell, info)
        } else {
            btree_parse_cell_ptr_no_payload(page, buf, cell, info)
        }
    } else {
        btree_parse_cell_ptr_index(page, buf, cell, info)
    }
}

/// `btreeParseCell`: como `x_parse_cell`, com a célula referida pelo índice.
pub(crate) fn btree_parse_cell(page: &MemPage, data: &[u8], i_cell: i32, info: &mut CellInfo) {
    x_parse_cell(page, data, find_cell(page, data, i_cell), info);
}

/// `cellSizePtr`: nó interno de índice.
pub(crate) fn cell_size_ptr(page: &MemPage, buf: &[u8], cell: usize) -> u16 {
    debug_assert!(page.child_ptr_size == 4);
    let (n, it) = payload_varint(buf, cell + 4);
    payload_local_and_size(page, n, it - cell).1
}

/// `cellSizePtrIdxLeaf`: folha de índice.
pub(crate) fn cell_size_ptr_idx_leaf(page: &MemPage, buf: &[u8], cell: usize) -> u16 {
    debug_assert!(page.child_ptr_size == 0);
    let (n, it) = payload_varint(buf, cell);
    payload_local_and_size(page, n, it - cell).1
}

/// `cellSizePtrNoPayload`: nó interno de tabela.
pub(crate) fn cell_size_ptr_no_payload(page: &MemPage, buf: &[u8], cell: usize) -> u16 {
    debug_assert!(page.child_ptr_size == 4);
    let _ = page;
    let mut it = cell + 4;
    let end = it + 9;
    loop {
        let c = byte(buf, it);
        it += 1;
        if !(c & 0x80 != 0 && it < end) {
            break;
        }
    }
    (it - cell) as u16
}

/// `cellSizePtrTableLeaf`: folha de tabela.
pub(crate) fn cell_size_ptr_table_leaf(page: &MemPage, buf: &[u8], cell: usize) -> u16 {
    let (n, mut it) = payload_varint(buf, cell);
    // Pula o varint da chave (até 9 bytes).
    let mut all = true;
    for _ in 0..8 {
        let c = byte(buf, it);
        it += 1;
        if c & 0x80 == 0 {
            all = false;
            break;
        }
    }
    if all {
        it += 1;
    }
    payload_local_and_size(page, n, it - cell).1
}

/// `pPage->xCellSize(pPage, pCell)`: despacha pelo tipo da página.
pub(crate) fn x_cell_size(page: &MemPage, buf: &[u8], cell: usize) -> u16 {
    if page.int_key {
        if page.leaf {
            cell_size_ptr_table_leaf(page, buf, cell)
        } else {
            cell_size_ptr_no_payload(page, buf, cell)
        }
    } else if page.leaf {
        cell_size_ptr_idx_leaf(page, buf, cell)
    } else {
        cell_size_ptr(page, buf, cell)
    }
}

/// `ptrmapPutOvflPtr`: se a célula (começando em `cell[0]`) tem página de overflow, grava no
/// mapa a entrada dela como filha de `pg`. `src_data_end` é o offset de `aDataEnd` da página de
/// origem relativo ao início de `cell` (ou `None` se a célula está num buffer solto); o `cell`
/// deve ser uma cópia (o `bt.pager` fica livre para o `ptrmap_put`).
pub(crate) fn ptrmap_put_ovfl_ptr(
    bt: &mut BtShared,
    pg: PgId,
    src_data_end: Option<usize>,
    cell: &[u8],
    rc: &mut i32,
) {
    if *rc != SQLITE_OK {
        return;
    }
    let mut info = CellInfo::default();
    let pgno = {
        let page = bt.pager.page_extra(pg);
        x_parse_cell(page, cell, 0, &mut info);
        page.pgno
    };
    if (info.n_local as u32) < info.n_payload {
        // SQLITE_OVERFLOW(aDataEnd, pCell, pCell + nLocal): a célula atravessa o fim da página.
        if let Some(end) = src_data_end {
            if 0 < end && (info.n_local as usize) > end {
                *rc = SQLITE_CORRUPT_BKPT;
                return;
            }
        }
        let at = (info.n_size as usize).wrapping_sub(4);
        if at.wrapping_add(4) > cell.len() {
            *rc = SQLITE_CORRUPT_BKPT;
            return;
        }
        let ovfl = get4byte(&cell[at..]);
        ptrmap_put(bt, ovfl, PTRMAP_OVERFLOW1, pgno, rc);
    }
}

// ---------------------------------------------------------------------------------------------
// Espaço livre da página (chunks 004 e 005)
// ---------------------------------------------------------------------------------------------

/// Núcleo de `defragmentPage`. `tmp` é o `sqlite3PagerTempSpace` retirado do pager.
fn defragment_inner(
    page: &mut MemPage,
    data: &mut [u8],
    tmp: &mut Vec<u8>,
    n_max_frag: i32,
) -> i32 {
    let hdr = page.hdr_offset as usize;
    let cell_offset = page.cell_offset as usize;
    let n_cell = page.n_cell as usize;
    let usable = page.geom.usable_size as i32;
    if usable as usize > data.len() || cell_offset + 2 * n_cell > data.len() || hdr + 8 > data.len() {
        return SQLITE_CORRUPT_BKPT;
    }
    let i_cell_first = (cell_offset + 2 * n_cell) as i32;
    let mut cbrk: i32 = 0;

    // Páginas com no máximo dois blocos livres e poucos fragmentos: move os blocos de células.
    let fast = 'fast: {
        if (data[hdr + 7] as i32) <= n_max_frag {
            let i_free = rd2(data, hdr + 1) as i32;
            if i_free > usable - 4 {
                return SQLITE_CORRUPT_BKPT;
            }
            if i_free != 0 {
                let i_free2 = rd2(data, i_free as usize) as i32;
                if i_free2 > usable - 4 {
                    return SQLITE_CORRUPT_BKPT;
                }
                if 0 == i_free2 || (data[i_free2 as usize] == 0 && data[i_free2 as usize + 1] == 0) {
                    let p_end = cell_offset + n_cell * 2;
                    let mut sz2 = 0i32;
                    let mut sz = rd2(data, i_free as usize + 2) as i32;
                    let top = rd2(data, hdr + 5) as i32;
                    if top >= i_free {
                        return SQLITE_CORRUPT_BKPT;
                    }
                    if i_free2 != 0 {
                        if i_free + sz > i_free2 {
                            return SQLITE_CORRUPT_BKPT;
                        }
                        sz2 = rd2(data, i_free2 as usize + 2) as i32;
                        if i_free2 + sz2 > usable {
                            return SQLITE_CORRUPT_BKPT;
                        }
                        data.copy_within(
                            (i_free + sz) as usize..i_free2 as usize,
                            (i_free + sz + sz2) as usize,
                        );
                        sz += sz2;
                    } else if i_free + sz > usable {
                        return SQLITE_CORRUPT_BKPT;
                    }
                    cbrk = top + sz;
                    data.copy_within(top as usize..i_free as usize, cbrk as usize);
                    let mut p_addr = cell_offset;
                    while p_addr < p_end {
                        let pc = rd2(data, p_addr) as i32;
                        if pc < i_free {
                            put2byte(&mut data[p_addr..], (pc + sz) as u32);
                        } else if pc < i_free2 {
                            put2byte(&mut data[p_addr..], (pc + sz2) as u32);
                        }
                        p_addr += 2;
                    }
                    break 'fast true;
                }
            }
        }
        false
    };

    if !fast {
        cbrk = usable;
        let i_cell_last = usable - 4;
        let i_cell_start = rd2(data, hdr + 5) as i32;
        if n_cell > 0 {
            if tmp.len() < usable as usize {
                tmp.resize(usable as usize, 0);
            }
            tmp[..usable as usize].copy_from_slice(&data[..usable as usize]);
            for i in 0..n_cell {
                let p_addr = cell_offset + i * 2;
                let pc = rd2(data, p_addr) as i32;
                if pc > i_cell_last {
                    return SQLITE_CORRUPT_BKPT;
                }
                let size = x_cell_size(page, &tmp[..], pc as usize) as i32;
                cbrk -= size;
                if cbrk < i_cell_start || pc + size > usable {
                    return SQLITE_CORRUPT_BKPT;
                }
                put2byte(&mut data[p_addr..], cbrk as u32);
                data[cbrk as usize..(cbrk + size) as usize]
                    .copy_from_slice(&tmp[pc as usize..(pc + size) as usize]);
            }
        }
        data[hdr + 7] = 0;
    }

    // defragment_out
    if data[hdr + 7] as i32 + cbrk - i_cell_first != page.n_free {
        return SQLITE_CORRUPT_BKPT;
    }
    if cbrk < i_cell_first || cbrk as usize > data.len() {
        return SQLITE_CORRUPT_BKPT;
    }
    put2byte(&mut data[hdr + 5..], cbrk as u32);
    data[hdr + 1] = 0;
    data[hdr + 2] = 0;
    data[i_cell_first as usize..cbrk as usize].fill(0);
    SQLITE_OK
}

/// `defragmentPage`: reorganiza as células para não sobrar bloco livre; deixa no máximo
/// `n_max_frag` bytes fragmentados.
pub(crate) fn defragment_page(bt: &mut BtShared, pg: PgId, n_max_frag: i32) -> i32 {
    let mut tmp = std::mem::take(&mut bt.pager.tmp_space);
    let rc = {
        let (page, data) = bt.pager.page_parts(pg);
        defragment_inner(page, data, &mut tmp, n_max_frag)
    };
    bt.pager.tmp_space = tmp;
    rc
}

/// `pageFindSlot`: procura na lista livre um espaço de `n_byte` bytes. Devolve o offset, ou
/// `None` (com `*rc` ligado se achou corrupção).
pub(crate) fn page_find_slot(
    page: &MemPage,
    data: &mut [u8],
    n_byte: i32,
    rc: &mut i32,
) -> Option<usize> {
    let hdr = page.hdr_offset as usize;
    let usable = page.geom.usable_size as i32;
    let mut i_addr = hdr + 1;
    let mut pc = rd2(data, i_addr) as i32;
    let max_pc = usable - n_byte;
    while pc <= max_pc {
        let size = rd2(data, pc as usize + 2) as i32;
        let x = size - n_byte;
        if x >= 0 {
            if pc as usize + 4 > data.len() {
                *rc = SQLITE_CORRUPT_BKPT;
                return None;
            }
            if x < 4 {
                // Em página bem formada os fragmentos somam no máximo 60.
                if data[hdr + 7] > 57 {
                    return None;
                }
                data.copy_within(pc as usize..pc as usize + 2, i_addr);
                data[hdr + 7] = data[hdr + 7].wrapping_add(x as u8);
                return Some(pc as usize);
            } else if x + pc > max_pc {
                *rc = SQLITE_CORRUPT_BKPT;
                return None;
            } else {
                put2byte(&mut data[pc as usize + 2..], x as u32);
            }
            return Some((pc + x) as usize);
        }
        i_addr = pc as usize;
        pc = rd2(data, i_addr) as i32;
        if pc <= i_addr as i32 {
            if pc != 0 {
                *rc = SQLITE_CORRUPT_BKPT;
            }
            return None;
        }
    }
    if pc > max_pc + n_byte - 4 {
        *rc = SQLITE_CORRUPT_BKPT;
    }
    None
}

/// `allocateSpace`: reserva `n_byte` bytes na página; `*p_idx` recebe o offset.
pub(crate) fn allocate_space(bt: &mut BtShared, pg: PgId, n_byte: i32, p_idx: &mut i32) -> i32 {
    let mut tmp = std::mem::take(&mut bt.pager.tmp_space);
    let rc = {
        let (page, data) = bt.pager.page_parts(pg);
        allocate_inner(page, data, &mut tmp, n_byte, p_idx)
    };
    bt.pager.tmp_space = tmp;
    rc
}

fn allocate_inner(
    page: &mut MemPage,
    data: &mut [u8],
    tmp: &mut Vec<u8>,
    n_byte: i32,
    p_idx: &mut i32,
) -> i32 {
    let hdr = page.hdr_offset as usize;
    let usable = page.geom.usable_size as i32;
    if usable as usize > data.len() || hdr + 8 > data.len() {
        return SQLITE_CORRUPT_BKPT;
    }
    let gap = page.cell_offset as i32 + 2 * page.n_cell as i32;
    let mut top = rd2(data, hdr + 5) as i32;
    if gap > top {
        if top == 0 && usable == 65536 {
            top = 65536;
        } else {
            return SQLITE_CORRUPT_BKPT;
        }
    } else if top > usable {
        return SQLITE_CORRUPT_BKPT;
    }

    // Se cabe mais um ponteiro de célula e a lista livre não está vazia, procura nela.
    let mut rc = SQLITE_OK;
    if (data[hdr + 2] != 0 || data[hdr + 1] != 0) && gap + 2 <= top {
        if let Some(sp) = page_find_slot(page, data, n_byte, &mut rc) {
            let g2 = sp as i32;
            *p_idx = g2;
            return if g2 <= gap { SQLITE_CORRUPT_BKPT } else { SQLITE_OK };
        } else if rc != SQLITE_OK {
            return rc;
        }
    }

    // Sem espaço na lista livre: desfragmenta se preciso.
    if gap + 2 + n_byte > top {
        rc = defragment_inner(page, data, tmp, 4.min(page.n_free - (2 + n_byte)));
        if rc != SQLITE_OK {
            return rc;
        }
        top = get2byte_not_zero(data, hdr + 5);
    }

    // Aloca da folga entre o índice de células e a área de conteúdo.
    top -= n_byte;
    if top < 0 {
        return SQLITE_CORRUPT_BKPT;
    }
    put2byte(&mut data[hdr + 5..], top as u32);
    *p_idx = top;
    SQLITE_OK
}

/// `freeSpace`: devolve `[i_start, i_start + i_size)` à lista livre, juntando vizinhos.
pub(crate) fn free_space(bt: &mut BtShared, pg: PgId, i_start: u16, i_size: u16) -> i32 {
    let fast_secure = bt.bts_flags & BTS_FAST_SECURE != 0;
    let (page, data) = bt.pager.page_parts(pg);
    let usable = page.geom.usable_size;
    let hdr = page.hdr_offset as usize;
    let mut i_start = i_start;
    let mut i_size = i_size;
    let i_orig_size = i_size;
    let mut n_frag: u8 = 0;
    let mut i_end: u32 = i_start as u32 + i_size as u32;
    if i_end as usize > data.len() || i_start as usize + 4 > data.len() || hdr + 8 > data.len() {
        return SQLITE_CORRUPT_BKPT;
    }

    // A lista livre é ascendente: acha onde `i_start` entra.
    let mut i_ptr: u16 = (hdr + 1) as u16;
    let mut i_free_blk: u16;
    if data[i_ptr as usize + 1] == 0 && data[i_ptr as usize] == 0 {
        i_free_blk = 0; // lista vazia
    } else {
        loop {
            i_free_blk = rd2(data, i_ptr as usize) as u16;
            if i_free_blk >= i_start {
                break;
            }
            if i_free_blk <= i_ptr {
                if i_free_blk == 0 {
                    break;
                }
                return SQLITE_CORRUPT_BKPT;
            }
            i_ptr = i_free_blk;
        }
        if i_free_blk as u32 > usable.wrapping_sub(4) {
            return SQLITE_CORRUPT_BKPT;
        }

        // Junta `i_free_blk` ao fim de `i_start`?
        if i_free_blk != 0 && i_end + 3 >= i_free_blk as u32 {
            n_frag = (i_free_blk as u32).wrapping_sub(i_end) as u8;
            if i_end > i_free_blk as u32 {
                return SQLITE_CORRUPT_BKPT;
            }
            i_end = i_free_blk as u32 + rd2(data, i_free_blk as usize + 2);
            if i_end > usable {
                return SQLITE_CORRUPT_BKPT;
            }
            i_size = (i_end - i_start as u32) as u16;
            i_free_blk = rd2(data, i_free_blk as usize) as u16;
        }

        // Se `i_ptr` é outro bloco livre, junta `i_start` ao fim dele?
        if i_ptr as usize > hdr + 1 {
            let i_ptr_end = i_ptr as i32 + rd2(data, i_ptr as usize + 2) as i32;
            if i_ptr_end + 3 >= i_start as i32 {
                if i_ptr_end > i_start as i32 {
                    return SQLITE_CORRUPT_BKPT;
                }
                n_frag = n_frag.wrapping_add((i_start as i32 - i_ptr_end) as u8);
                i_size = (i_end - i_ptr as u32) as u16;
                i_start = i_ptr;
            }
        }
        if n_frag > data[hdr + 7] {
            return SQLITE_CORRUPT_BKPT;
        }
        data[hdr + 7] -= n_frag;
    }
    let x = rd2(data, hdr + 5) as u16;
    if fast_secure {
        // Com secure_delete sobrescreve o conteúdo apagado com zeros.
        let s = i_start as usize;
        let e = s + i_size as usize;
        if e > data.len() {
            return SQLITE_CORRUPT_BKPT;
        }
        data[s..e].fill(0);
    }
    if i_start <= x {
        // O bloco novo está no começo da área de conteúdo: só a estende.
        if i_start < x {
            return SQLITE_CORRUPT_BKPT;
        }
        if i_ptr as usize != hdr + 1 {
            return SQLITE_CORRUPT_BKPT;
        }
        put2byte(&mut data[hdr + 1..], i_free_blk as u32);
        put2byte(&mut data[hdr + 5..], i_end);
    } else {
        // Insere o bloco novo na lista livre.
        put2byte(&mut data[i_ptr as usize..], i_start as u32);
        put2byte(&mut data[i_start as usize..], i_free_blk as u32);
        put2byte(&mut data[i_start as usize + 2..], i_size as u32);
    }
    page.n_free += i_orig_size as i32;
    SQLITE_OK
}

/// `decodeFlags`: decodifica o byte de flags da página (usa `page.geom`).
pub(crate) fn decode_flags(page: &mut MemPage, flag_byte: u8) -> i32 {
    let g = page.geom;
    page.max1byte_payload = g.max1byte_payload;
    if flag_byte >= (PTF_ZERODATA | PTF_LEAF) {
        page.child_ptr_size = 0;
        page.leaf = true;
        if flag_byte == (PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF) {
            page.int_key_leaf = true;
            page.int_key = true;
            page.max_local = g.max_leaf;
            page.min_local = g.min_leaf;
        } else if flag_byte == (PTF_ZERODATA | PTF_LEAF) {
            page.int_key = false;
            page.int_key_leaf = false;
            page.max_local = g.max_local;
            page.min_local = g.min_local;
        } else {
            page.int_key = false;
            page.int_key_leaf = false;
            return SQLITE_CORRUPT_BKPT;
        }
    } else {
        page.child_ptr_size = 4;
        page.leaf = false;
        if flag_byte == PTF_ZERODATA {
            page.int_key = false;
            page.int_key_leaf = false;
            page.max_local = g.max_local;
            page.min_local = g.min_local;
        } else if flag_byte == (PTF_LEAFDATA | PTF_INTKEY) {
            page.int_key_leaf = false;
            page.int_key = true;
            page.max_local = g.max_leaf;
            page.min_local = g.min_leaf;
        } else {
            page.int_key = false;
            page.int_key_leaf = false;
            return SQLITE_CORRUPT_BKPT;
        }
    }
    SQLITE_OK
}

/// `btreeComputeFreeSpace`: preenche `n_free` da página.
pub(crate) fn btree_compute_free_space(bt: &mut BtShared, pg: PgId) -> i32 {
    let (page, data) = bt.pager.page_parts(pg);
    debug_assert!(page.is_init);
    let usable = page.geom.usable_size as i32;
    let hdr = page.hdr_offset as usize;
    let top = get2byte_not_zero(data, hdr + 5);
    let i_cell_first = hdr as i32 + 8 + page.child_ptr_size as i32 + 2 * page.n_cell as i32;
    let i_cell_last = usable - 4;

    let mut pc = rd2(data, hdr + 1) as i32;
    let mut n_free = byte(data, hdr + 7) as i32 + top;
    if pc > 0 {
        let mut next: u32;
        let mut size: u32;
        if pc < top {
            // Numa página bem formada há ao menos uma célula antes do primeiro bloco livre.
            return SQLITE_CORRUPT_BKPT;
        }
        loop {
            if pc > i_cell_last {
                return SQLITE_CORRUPT_BKPT;
            }
            next = rd2(data, pc as usize);
            size = rd2(data, pc as usize + 2);
            n_free = n_free.wrapping_add(size as i32);
            if next <= (pc as u32).wrapping_add(size).wrapping_add(3) {
                break;
            }
            pc = next as i32;
        }
        if next > 0 {
            return SQLITE_CORRUPT_BKPT; // bloco livre fora de ordem
        }
        if (pc as u32).wrapping_add(size) > usable as u32 {
            return SQLITE_CORRUPT_BKPT;
        }
    }
    if n_free > usable || n_free < i_cell_first {
        return SQLITE_CORRUPT_BKPT;
    }
    page.n_free = (n_free - i_cell_first) as u16 as i32;
    SQLITE_OK
}

/// `btreeCellSizeCheck`: conferência extra de `PRAGMA cell_size_check=ON`.
fn btree_cell_size_check(page: &MemPage, data: &[u8]) -> i32 {
    let i_cell_first = page.cell_offset as i32 + 2 * page.n_cell as i32;
    let usable = page.geom.usable_size as i32;
    let mut i_cell_last = usable - 4;
    let cell_offset = page.cell_offset as usize;
    if !page.leaf {
        i_cell_last -= 1;
    }
    for i in 0..page.n_cell as usize {
        let pc = rd2(data, cell_offset + i * 2) as i32;
        if pc < i_cell_first || pc > i_cell_last {
            return SQLITE_CORRUPT_BKPT;
        }
        let sz = x_cell_size(page, data, pc as usize) as i32;
        if pc + sz > usable {
            return SQLITE_CORRUPT_BKPT;
        }
    }
    SQLITE_OK
}

/// Núcleo de `btreeInitPage`, também usado por `page_reinit` (que só tem a página e os bytes).
fn init_page_inner(page: &mut MemPage, data: &[u8]) -> i32 {
    let hdr = page.hdr_offset as usize;
    let g = page.geom;
    if decode_flags(page, byte(data, hdr)) != SQLITE_OK {
        return SQLITE_CORRUPT_BKPT;
    }
    debug_assert!(g.page_size >= 512 && g.page_size <= 65536);
    page.mask_page = (g.page_size - 1) as u16;
    page.n_overflow = 0;
    page.cell_offset = (hdr + 8 + page.child_ptr_size as usize) as u16;
    page.a_cell_idx = hdr + page.child_ptr_size as usize + 8;
    page.a_data_end = g.page_size as usize;
    page.a_data_ofst = page.child_ptr_size as usize;
    page.n_cell = rd2(data, hdr + 3) as u16;
    if page.n_cell as u32 > (g.page_size - 8) / 6 {
        // Células demais para uma página: corrompida.
        return SQLITE_CORRUPT_BKPT;
    }
    page.n_free = -1; // ainda não calculado
    page.is_init = true;
    if g.cell_size_ck {
        return btree_cell_size_check(page, data);
    }
    SQLITE_OK
}

/// `btreeInitPage`: inicializa os dados auxiliares de uma página de disco.
pub(crate) fn btree_init_page(bt: &mut BtShared, pg: PgId) -> i32 {
    let geom = page_geom_of(bt);
    let (page, data) = bt.pager.page_parts(pg);
    debug_assert!(!page.is_init);
    page.geom = geom;
    init_page_inner(page, data)
}

// ---------------------------------------------------------------------------------------------
// Páginas (chunk 006)
// ---------------------------------------------------------------------------------------------

/// `zeroPage`: deixa a página com cara de página de banco sem entradas.
pub(crate) fn zero_page(bt: &mut BtShared, pg: PgId, flags: u8) {
    let geom = page_geom_of(bt);
    let fast_secure = bt.bts_flags & BTS_FAST_SECURE != 0;
    let (page, data) = bt.pager.page_parts(pg);
    page.geom = geom;
    let hdr = page.hdr_offset as usize;
    let usable = geom.usable_size as usize;
    if fast_secure {
        data[hdr..usable].fill(0);
    }
    data[hdr] = flags;
    let first = hdr + if (flags & PTF_LEAF) == 0 { 12 } else { 8 };
    data[hdr + 1..hdr + 5].fill(0);
    data[hdr + 7] = 0;
    put2byte(&mut data[hdr + 5..], geom.usable_size);
    page.n_free = (geom.usable_size as usize).wrapping_sub(first) as u16 as i32;
    decode_flags(page, flags);
    page.cell_offset = first as u16;
    page.a_data_end = geom.page_size as usize;
    page.a_cell_idx = first;
    page.a_data_ofst = page.child_ptr_size as usize;
    page.n_overflow = 0;
    page.mask_page = (geom.page_size - 1) as u16;
    page.n_cell = 0;
    page.is_init = true;
}

/// `btreePageFromDbPage`: acerta `pgno` e `hdr_offset` do `MemPage` da página `pg`.
pub(crate) fn btree_page_from_db_page(bt: &mut BtShared, pg: PgId, pgno: u32) {
    let page = bt.pager.page_extra_mut(pg);
    if pgno != page.pgno {
        page.pgno = pgno;
        page.hdr_offset = if pgno == 1 { 100 } else { 0 };
    }
}

/// `btreeGetPage`: obtém a página do pager (com referência contada).
pub(crate) fn btree_get_page(bt: &mut BtShared, pgno: u32, flags: i32) -> Result<PgId, i32> {
    let pg = bt.pager.get(pgno, flags)?;
    btree_page_from_db_page(bt, pg, pgno);
    Ok(pg)
}

/// `btreePageLookup`: a página se já está no cache, sem ler do disco.
pub(crate) fn btree_page_lookup(bt: &mut BtShared, pgno: u32) -> Option<PgId> {
    let pg = bt.pager.lookup(pgno)?;
    btree_page_from_db_page(bt, pg, pgno);
    Some(pg)
}

/// `sqlite3BtreeLastPage`.
pub(crate) fn btree_last_page(p: &Btree) -> u32 {
    p.bt.n_page
}

/// `getAndInitPage`: obtém a página e a inicializa. `flags` é `0` ou `PAGER_GET_READONLY`.
pub(crate) fn get_and_init_page(bt: &mut BtShared, pgno: u32, flags: i32) -> Result<PgId, i32> {
    debug_assert!(flags == 0 || flags == PAGER_GET_READONLY);
    if pgno > bt.n_page {
        return Err(SQLITE_CORRUPT_BKPT);
    }
    let pg = bt.pager.get(pgno, flags)?;
    if !bt.pager.page_extra(pg).is_init {
        btree_page_from_db_page(bt, pg, pgno);
        let rc = btree_init_page(bt, pg);
        if rc != SQLITE_OK {
            release_page(bt, Some(pg));
            return Err(rc);
        }
    }
    Ok(pg)
}

/// `releasePageNotNull`.
#[inline]
pub(crate) fn release_page_not_null(bt: &mut BtShared, pg: PgId) {
    bt.pager.unref_not_null(pg);
}

/// `releasePage`.
#[inline]
pub(crate) fn release_page(bt: &mut BtShared, pg: Option<PgId>) {
    bt.pager.unref(pg);
}

/// `releasePageOne`: solta a referência à página 1.
#[inline]
pub(crate) fn release_page_one(bt: &mut BtShared, pg: PgId) {
    bt.pager.unref_page_one(pg);
}

/// `btreeGetUnusedPage`: como `btree_get_page`, mas `SQLITE_CORRUPT` se a página já está em
/// uso, e deixa `is_init` desligado.
pub(crate) fn btree_get_unused_page(bt: &mut BtShared, pgno: u32, flags: i32) -> Result<PgId, i32> {
    let pg = btree_get_page(bt, pgno, flags)?;
    if bt.pager.page_ref_count(pg) > 1 {
        release_page(bt, Some(pg));
        return Err(SQLITE_CORRUPT_BKPT);
    }
    bt.pager.page_extra_mut(pg).is_init = false;
    Ok(pg)
}

/// `pageReinit`: o `xReiniter` do pager. Na reversão o pager recarrega a página; aqui os
/// metadados se acertam com os bytes restaurados. `n_ref` é `sqlite3PagerPageRefcount`.
pub(crate) fn page_reinit(page: &mut MemPage, data: &mut [u8], n_ref: i64) {
    debug_assert!(n_ref > 0);
    if page.is_init {
        page.is_init = false;
        if n_ref > 1 {
            // Pode não ser página de árvore-b (overflow, mapa, livre): então o init
            // provavelmente dá `SQLITE_CORRUPT`, sem dano. Importa chamar em toda página.
            let _ = init_page_inner(page, data);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Abertura e fechamento (chunks 006 e 007)
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeOpen`. `filename` é o `sqlite3_filename` (caminho, NUL, pares de URI) ou
/// `None`/vazio para banco temporário. Diferenças do C, por não haver `db` aqui: quem chama
/// passa `BTREE_MEMORY` em `flags` quando `sqlite3TempInMemory(db)` vale; `db_index` sai zero e
/// `szMmap` não é aplicado (use `btree_set_mmap_limit`); o busy handler do pager é instalado
/// pela conexão (`bt.pager.set_busy_handler`). O cache compartilhado não existe: `sharable`
/// fica falso.
pub(crate) fn btree_open(
    vfs: VfsRef,
    filename: Option<&[u8]>,
    flags: i32,
    vfs_flags: i32,
) -> Result<Btree, i32> {
    let name = filename.map(until_nul);
    let is_temp_db = name.map_or(true, |z| z.is_empty());
    let is_memdb = name == Some(&b":memory:"[..])
        || (flags as u32 & BTREE_MEMORY) != 0
        || (vfs_flags & SQLITE_OPEN_MEMORY) != 0;
    let mut flags = flags;
    let mut vfs_flags = vfs_flags;
    if is_memdb {
        flags |= BTREE_MEMORY as i32;
    }
    if (vfs_flags & SQLITE_OPEN_MAIN_DB) != 0 && (is_memdb || is_temp_db) {
        vfs_flags = (vfs_flags & !SQLITE_OPEN_MAIN_DB) | SQLITE_OPEN_TEMP_DB;
    }

    let mut pager = pager_open::<MemPage>(
        vfs,
        filename,
        std::mem::size_of::<MemPage>() as i32,
        flags,
        vfs_flags,
        Some(page_reinit),
    )?;

    let mut db_header = [0u8; 100];
    let rc = pager.read_fileheader(&mut db_header);
    if rc != SQLITE_OK {
        pager.close(None);
        return Err(rc);
    }

    let mut bts_flags: u16 = 0;
    let mut auto_vacuum = 0u8;
    let mut incr_vacuum = 0u8;
    if pager.read_only {
        bts_flags |= BTS_READ_ONLY;
    }
    bts_flags |= BTS_SECURE_DELETE; // SQLITE_SECURE_DELETE do Debian

    let mut page_size: u32 = ((db_header[16] as u32) << 8) | ((db_header[17] as u32) << 16);
    let n_reserve: u8;
    if page_size < 512 || page_size > SQLITE_MAX_PAGE_SIZE as u32 || ((page_size - 1) & page_size) != 0 {
        page_size = 0;
        // SQLITE_DEFAULT_AUTOVACUUM é 0: nada a ligar em arquivo novo.
        n_reserve = 0;
    } else {
        n_reserve = db_header[20];
        bts_flags |= BTS_PAGESIZE_FIXED;
        auto_vacuum = (get4byte(&db_header[36 + 4 * 4..]) != 0) as u8;
        incr_vacuum = (get4byte(&db_header[36 + 7 * 4..]) != 0) as u8;
    }
    let rc = pager.set_pagesize(&mut page_size, n_reserve as i32);
    if rc != SQLITE_OK {
        pager.close(None);
        return Err(rc);
    }
    debug_assert!(page_size & 7 == 0);

    let mut p = Btree {
        db_index: 0,
        in_trans: TRANS_NONE,
        sharable: false,
        locked: false,
        has_incrblob_cur: false,
        want_to_lock: 0,
        n_backup: 0,
        i_b_data_version: 0,
        lock: BtLock { i_table: 1, e_lock: 0 },
        bt: BtShared {
            pager,
            db_flags: 0,
            cursors: Slab::default(),
            p_page1: None,
            open_flags: flags as u8,
            auto_vacuum,
            incr_vacuum,
            do_truncate: 0,
            in_transaction: TRANS_NONE,
            max1byte_payload: 0,
            n_reserve_wanted: 0,
            bts_flags,
            max_local: 0,
            min_local: 0,
            max_leaf: 0,
            min_leaf: 0,
            page_size,
            usable_size: page_size - n_reserve as u32,
            n_transaction: 0,
            n_page: 0,
            p_schema: None,
            p_has_content: None,
            lock_list: Vec::new(),
            has_writer: false,
            p_tmp_space: Vec::new(),
            n_preformat_size: 0,
        },
    };

    // Tamanho de cache padrão, salvo em cache compartilhado já existente (esquema alocado).
    if p.bt.p_schema.is_none() {
        btree_set_cache_size(&mut p, SQLITE_DEFAULT_CACHE_SIZE);
    }
    // `SQLITE_FCNTL_PDB` só entrega ao VFS o ponteiro do `db`; sem `db` aqui, não se envia.
    Ok(p)
}

/// `allocateTempSpace`: `p_tmp_space` com `page_size` bytes; os 4 primeiros são o prefixo para o
/// ponteiro de filho (o `pTmpSpace` do C é `p_tmp_space[4..]`). Os 8 primeiros bytes zerados.
pub(crate) fn allocate_temp_space(bt: &mut BtShared) -> i32 {
    debug_assert!(bt.p_tmp_space.is_empty());
    bt.p_tmp_space = vec![0u8; bt.page_size as usize];
    SQLITE_OK
}

/// `freeTempSpace`.
pub(crate) fn free_temp_space(bt: &mut BtShared) {
    bt.p_tmp_space = Vec::new();
}

/// `sqlite3BtreeClose`: desfaz a transação ativa e fecha o pager. `db` é o que o
/// `sqlite3PagerClose(pPager, p->db)` lê da conexão.
pub(crate) fn btree_close(
    mut p: Btree,
    bt_db: &crate::btree_cursor::BtDb<'_>,
    db: Option<PagerCloseDb<'_>>,
) -> i32 {
    debug_assert!(p.bt.cursors.is_empty());
    crate::btree_cursor::btree_rollback(&mut p, SQLITE_OK, false, bt_db);
    debug_assert!(p.want_to_lock == 0 && !p.locked);
    let Btree { bt, .. } = p;
    let BtShared { pager, .. } = bt;
    pager.close(db);
    SQLITE_OK
}

/// `sqlite3BtreeSetCacheSize`.
pub(crate) fn btree_set_cache_size(p: &mut Btree, mx_page: i32) -> i32 {
    p.bt.pager.pcache.set_cache_size(mx_page);
    SQLITE_OK
}

/// `sqlite3BtreeSetSpillSize`.
pub(crate) fn btree_set_spill_size(p: &mut Btree, mx_page: i32) -> i32 {
    p.bt.pager.pcache.set_spill_size(mx_page)
}

/// `sqlite3BtreeIsReadonly`: verdadeiro se o arquivo do banco foi aberto somente para leitura.
pub(crate) fn btree_is_readonly(p: &Btree) -> bool {
    (p.bt.bts_flags & BTS_READ_ONLY) != 0
}

/// `sqlite3BtreeSetMmapLimit`.
pub(crate) fn btree_set_mmap_limit(p: &mut Btree, sz_mmap: i64) -> i32 {
    p.bt.pager.set_mmap_limit(sz_mmap);
    SQLITE_OK
}

/// `sqlite3BtreeSetPagerFlags`.
pub(crate) fn btree_set_pager_flags(p: &mut Btree, pg_flags: u32) -> i32 {
    p.bt.pager.set_flags(pg_flags);
    SQLITE_OK
}

/// `sqlite3BtreeSetPageSize`: muda o tamanho de página padrão e os bytes reservados; se o
/// tamanho já está fixado devolve `SQLITE_READONLY`. `n_reserve < 0` mantém o atual.
pub(crate) fn btree_set_page_size(
    p: &mut Btree,
    page_size: i32,
    n_reserve: i32,
    i_fix: i32,
) -> i32 {
    let bt = &mut p.bt;
    let mut page_size = page_size;
    let mut n_reserve = n_reserve;
    debug_assert!(n_reserve >= 0 && n_reserve <= 255);
    bt.n_reserve_wanted = n_reserve as u8;
    let x = bt.page_size as i32 - bt.usable_size as i32;
    if n_reserve < x {
        n_reserve = x;
    }
    if bt.bts_flags & BTS_PAGESIZE_FIXED != 0 {
        return SQLITE_READONLY;
    }
    if page_size >= 512 && page_size <= SQLITE_MAX_PAGE_SIZE && ((page_size - 1) & page_size) == 0 {
        debug_assert!(page_size & 7 == 0);
        debug_assert!(bt.cursors.is_empty());
        if n_reserve > 32 && page_size == 512 {
            page_size = 1024;
        }
        bt.page_size = page_size as u32;
        free_temp_space(bt);
    }
    let rc = bt.pager.set_pagesize(&mut bt.page_size, n_reserve);
    bt.usable_size = bt.page_size.wrapping_sub(n_reserve as u16 as u32);
    if i_fix != 0 {
        bt.bts_flags |= BTS_PAGESIZE_FIXED;
    }
    rc
}

/// `sqlite3BtreeGetPageSize`.
pub(crate) fn btree_get_page_size(p: &Btree) -> i32 {
    p.bt.page_size as i32
}

/// `sqlite3BtreeGetReserveNoMutex`.
pub(crate) fn btree_get_reserve_no_mutex(p: &Btree) -> i32 {
    p.bt.page_size as i32 - p.bt.usable_size as i32
}

/// `sqlite3BtreeGetRequestedReserve`: o maior entre o reservado e o pedido.
pub(crate) fn btree_get_requested_reserve(p: &Btree) -> i32 {
    let n1 = p.bt.n_reserve_wanted as i32;
    let n2 = btree_get_reserve_no_mutex(p);
    if n1 > n2 {
        n1
    } else {
        n2
    }
}

/// `sqlite3BtreeMaxPageCount`.
pub(crate) fn btree_max_page_count(p: &mut Btree, mx_page: u32) -> u32 {
    p.bt.pager.max_page_count(mx_page)
}

/// `sqlite3BtreeSecureDelete`: `new_flag` 0, 1 ou 2 muda; negativo só consulta.
pub(crate) fn btree_secure_delete(p: &mut Btree, new_flag: i32) -> i32 {
    let bt = &mut p.bt;
    if new_flag >= 0 {
        bt.bts_flags &= !BTS_FAST_SECURE;
        bt.bts_flags |= BTS_SECURE_DELETE * new_flag as u16;
    }
    ((bt.bts_flags & BTS_FAST_SECURE) / BTS_SECURE_DELETE) as i32
}

// ---------------------------------------------------------------------------------------------
// Auto-vacuum, lockBtree, newDatabase (chunk 008)
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeSetAutoVacuum`.
pub(crate) fn btree_set_auto_vacuum(p: &mut Btree, auto_vacuum: i32) -> i32 {
    let bt = &mut p.bt;
    let av = auto_vacuum as u8;
    if (bt.bts_flags & BTS_PAGESIZE_FIXED) != 0 && (av != 0) as u8 != bt.auto_vacuum {
        SQLITE_READONLY
    } else {
        bt.auto_vacuum = (av != 0) as u8;
        bt.incr_vacuum = (av == 2) as u8;
        SQLITE_OK
    }
}

/// `sqlite3BtreeGetAutoVacuum`: `BTREE_AUTOVACUUM_NONE`, `_FULL` ou `_INCR`.
pub(crate) fn btree_get_auto_vacuum(p: &Btree) -> i32 {
    (if p.bt.auto_vacuum == 0 {
        BTREE_AUTOVACUUM_NONE
    } else if p.bt.incr_vacuum == 0 {
        BTREE_AUTOVACUUM_FULL
    } else {
        BTREE_AUTOVACUUM_INCR
    }) as i32
}

/// `lockBtree`: obtém a página 1 (e com ela a trava de leitura). Em sucesso `p_page1` fica
/// preenchido, salvo quando precisa ser chamada de novo (WAL recém-aberto ou tamanho de página
/// descoberto diferente).
pub(crate) fn lock_btree(bt: &mut BtShared) -> i32 {
    debug_assert!(bt.p_page1.is_none());
    let rc = bt.pager.shared_lock();
    if rc != SQLITE_OK {
        return rc;
    }
    let page1 = match btree_get_page(bt, 1, 0) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Confere que o arquivo é mesmo um banco (copia o cabeçalho para soltar o empréstimo).
    let mut hdr = [0u8; 100];
    hdr.copy_from_slice(&bt.pager.page_data(page1)[..100]);
    let mut n_page = get4byte(&hdr[28..]);
    let n_page_file = bt.pager.pagecount() as u32;
    if n_page == 0 || hdr[24..28] != hdr[92..96] {
        n_page = n_page_file;
    }
    if (bt.db_flags & SQLITE_RESET_DATABASE) != 0 {
        n_page = 0;
    }

    let failed: Option<i32> = 'failed: {
        if n_page > 0 {
            let mut rc = SQLITE_NOTADB;
            if hdr[..16] != SQLITE_FILE_HEADER[..] {
                break 'failed Some(rc);
            }
            if hdr[18] > 2 {
                bt.bts_flags |= BTS_READ_ONLY;
            }
            if hdr[19] > 2 {
                break 'failed Some(rc);
            }

            // Versão de leitura 2: o banco é WAL; abre o log se não está aberto e pede ao
            // chamador para repetir (a página 1 lida pode não ser a mais recente).
            if hdr[19] == 2 && (bt.bts_flags & BTS_NO_WAL) == 0 {
                let mut is_open = 0i32;
                rc = bt.pager.open_wal(Some(&mut is_open));
                if rc != SQLITE_OK {
                    break 'failed Some(rc);
                }
                if is_open == 0 {
                    release_page_one(bt, page1);
                    return SQLITE_OK;
                }
                rc = SQLITE_NOTADB;
            }

            // As frações de payload são fixas em 64, 32 e 32.
            if hdr[21..24] != [64u8, 32, 32] {
                break 'failed Some(rc);
            }
            let page_size: u32 = ((hdr[16] as u32) << 8) | ((hdr[17] as u32) << 16);
            if (page_size.wrapping_sub(1) & page_size) != 0
                || page_size > SQLITE_MAX_PAGE_SIZE as u32
                || page_size <= 256
            {
                break 'failed Some(rc);
            }
            debug_assert!(page_size & 7 == 0);
            let usable_size = page_size - hdr[20] as u32;
            if page_size != bt.page_size {
                // O tamanho real é outro: solta a página 1, ajusta e deixa o chamador repetir.
                release_page_one(bt, page1);
                bt.usable_size = usable_size;
                bt.page_size = page_size;
                bt.bts_flags |= BTS_PAGESIZE_FIXED;
                free_temp_space(bt);
                return bt.pager.set_pagesize(&mut bt.page_size, (page_size - usable_size) as i32);
            }
            if n_page > n_page_file {
                // sqlite3WritableSchema(db)
                if (bt.db_flags & (SQLITE_WRITE_SCHEMA | SQLITE_DEFENSIVE)) != SQLITE_WRITE_SCHEMA {
                    break 'failed Some(SQLITE_CORRUPT_BKPT);
                } else {
                    n_page = n_page_file;
                }
            }
            // O tamanho utilizável não pode ser menor que 480.
            if usable_size < 480 {
                break 'failed Some(rc);
            }
            bt.bts_flags |= BTS_PAGESIZE_FIXED;
            bt.page_size = page_size;
            bt.usable_size = usable_size;
            bt.auto_vacuum = (get4byte(&hdr[36 + 4 * 4..]) != 0) as u8;
            bt.incr_vacuum = (get4byte(&hdr[36 + 7 * 4..]) != 0) as u8;
        }
        None
    };
    if let Some(rc) = failed {
        release_page_one(bt, page1);
        bt.p_page1 = None;
        return rc;
    }

    // Limites de payload local por célula (ver o comentário longo do C: ao menos `minFanout`
    // células por página; célula = ponteiro 2 + cabeçalho até 17 + payload + overflow 4).
    let us = bt.usable_size;
    bt.max_local = ((us - 12) * 64 / 255).wrapping_sub(23) as u16;
    bt.min_local = ((us - 12) * 32 / 255).wrapping_sub(23) as u16;
    bt.max_leaf = (us - 35) as u16;
    bt.min_leaf = ((us - 12) * 32 / 255).wrapping_sub(23) as u16;
    bt.max1byte_payload = if bt.max_local > 127 { 127 } else { bt.max_local as u8 };
    debug_assert!(bt.max_leaf as i32 + 23 <= bt.mx_cell_size());
    bt.p_page1 = Some(page1);
    bt.n_page = n_page;
    SQLITE_OK
}

/// `unlockBtreeIfUnused`: sem cursores nem transação, solta a página 1 (e a trava de leitura).
pub(crate) fn unlock_btree_if_unused(bt: &mut BtShared) {
    if bt.in_transaction == TRANS_NONE {
        if let Some(pg) = bt.p_page1.take() {
            debug_assert!(bt.pager.pcache.ref_count() == 1);
            release_page_one(bt, pg);
        }
    }
}

/// `newDatabase`: se o arquivo está vazio, inicializa a página 1 como banco novo.
pub(crate) fn new_database(bt: &mut BtShared) -> i32 {
    if bt.n_page > 0 {
        return SQLITE_OK;
    }
    let pg = match bt.p_page1 {
        Some(p) => p,
        None => return SQLITE_ERROR,
    };
    let rc = bt.pager.write(pg);
    if rc != SQLITE_OK {
        return rc;
    }
    let page_size = bt.page_size;
    let usable_size = bt.usable_size;
    {
        let data = bt.pager.page_data_mut(pg);
        data[..16].copy_from_slice(&SQLITE_FILE_HEADER[..]);
        data[16] = ((page_size >> 8) & 0xff) as u8;
        data[17] = ((page_size >> 16) & 0xff) as u8;
        data[18] = 1;
        data[19] = 1;
        debug_assert!(usable_size <= page_size && usable_size + 255 >= page_size);
        data[20] = (page_size - usable_size) as u8;
        data[21] = 64;
        data[22] = 32;
        data[23] = 32;
        data[24..100].fill(0);
    }
    zero_page(bt, pg, PTF_INTKEY | PTF_LEAF | PTF_LEAFDATA);
    bt.bts_flags |= BTS_PAGESIZE_FIXED;
    debug_assert!(bt.auto_vacuum <= 1 && bt.incr_vacuum <= 1);
    let (av, iv) = (bt.auto_vacuum as u32, bt.incr_vacuum as u32);
    let data = bt.pager.page_data_mut(pg);
    put4byte(&mut data[36 + 4 * 4..], av);
    put4byte(&mut data[36 + 7 * 4..], iv);
    bt.n_page = 1;
    data[31] = 1;
    SQLITE_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_leaf_page() -> MemPage {
        let mut p = MemPage::default();
        p.geom = PageGeom {
            page_size: 4096,
            usable_size: 4096,
            max_local: 4061,
            min_local: 489,
            max_leaf: 4061,
            min_leaf: 489,
            max1byte_payload: 127,
            cell_size_ck: false,
        };
        assert_eq!(decode_flags(&mut p, PTF_LEAFDATA | PTF_INTKEY | PTF_LEAF), SQLITE_OK);
        p
    }

    #[test]
    fn table_leaf_cell_in_page() {
        let page = table_leaf_page();
        // payload de 3 bytes, rowid 300 (varint 0x82 0x2c)
        let cell = [3u8, 0x82, 0x2c, b'a', b'b', b'c'];
        let mut info = CellInfo::default();
        x_parse_cell(&page, &cell, 0, &mut info);
        assert_eq!(info.n_key, 300);
        assert_eq!(info.n_payload, 3);
        assert_eq!(info.n_local, 3);
        assert_eq!(info.n_size, 6);
        assert_eq!(info.p_payload, 3);
        assert_eq!(x_cell_size(&page, &cell, 0), 6);
    }

    #[test]
    fn table_leaf_cell_with_overflow() {
        let page = table_leaf_page();
        // payload 5000 (varint 0xa7 0x08), rowid 1; fica local 489 + (5000-489)%4092 = 908
        let mut cell = vec![0u8; 920];
        cell[0] = 0xa7;
        cell[1] = 0x08;
        cell[2] = 1;
        let mut info = CellInfo::default();
        x_parse_cell(&page, &cell, 0, &mut info);
        assert_eq!(info.n_payload, 5000);
        assert_eq!(info.n_local, 908);
        assert_eq!(info.n_size, 3 + 908 + 4);
        assert_eq!(x_cell_size(&page, &cell, 0), info.n_size);
        assert_eq!(btree_payload_to_local(&page, 5000), 908);
    }

    #[test]
    fn corrupt_flags_are_rejected() {
        let mut p = MemPage::default();
        assert_eq!(decode_flags(&mut p, 0x07), SQLITE_CORRUPT_BKPT);
        assert_eq!(decode_flags(&mut p, 0x0b), SQLITE_CORRUPT_BKPT);
    }
}
