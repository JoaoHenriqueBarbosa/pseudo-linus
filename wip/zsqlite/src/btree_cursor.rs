//! btree.c, chunks 009 a 017: transações do Btree (BeginTrans, CommitPhaseOne/Two, Rollback,
//! BeginStmt, Savepoint), auto-vacuum (setChildPtrmaps, relocatePage, incrVacuumStep,
//! autoVacuumCommit), cursores (abrir, fechar, mover, ler payload), alocação e liberação de
//! páginas (allocateBtreePage, freePage2, clearCellOverflow), montagem de célula (fillInCell),
//! remoção e inserção de célula numa página (dropCell, insertCell) e o `CellArray` do balance.
//!
//! Modelo (CONVENTIONS.md, itens 2 a 4):
//!
//! * Funções de página: `fn(bt: &mut BtShared, pg: PgId, ...)`. Os bytes são
//!   `bt.pager.page_data(pg)` / `page_parts(pg)`; os metadados são o `MemPage` do `extra`.
//! * Funções de cursor: `fn(cur: &mut BtCursor, bt: &mut BtShared, ...)`. O cursor NÃO está em
//!   `bt.cursors` enquanto a função roda (a camada pública faz `take(id)` e `put(id, cur)`).
//! * Funções de transação recebem `p: &mut Btree` e, quando o C lê algo de `p->db`, um
//!   `BtDb` com só o que é lido (`nSavepoint`, `nVdbeRead`, busy-handler, `xAutovacPages`).
//!   `db->flags` já vem espelhado em `BtShared.db_flags`.
//! * Mutex (`sqlite3BtreeEnter/Leave`) some. Cache compartilhado não existe (`Btree == BtShared`),
//!   então `querySharedCacheTableLock`, `sqlite3ConnectionBlocked` e a busca de travas de
//!   outro Btree somem; `has_writer`, `lock_list` e `BTS_PENDING` seguem como no C.
//! * `SQLITE_CORRUPT_PAGE(p)` e `SQLITE_CORRUPT_PGNO(n)` só acrescentam um `sqlite3_log`: aqui
//!   são `SQLITE_CORRUPT_BKPT`.
//! * `sqlite3FaultSim`, `testcase`, `TRACE`, `btreeIntegrity`, `VVA_ONLY` e todo `assert` que só
//!   observa estado (e não protege contra banco corrompido) somem.
//!
//! Desvios de assinatura (todos por empréstimo ou por modelo):
//!
//! * `allocate_btree_page(bt, nearby, e_mode) -> Result<PgId, i32>`: o `ppPage` do C é o `Ok`;
//!   o `*pPgno` é `bt.pager.page_extra(pg).pgno`.
//! * `get_overflow_page(bt, ovfl, want_page) -> Result<(Option<PgId>, u32), i32>`.
//! * `btree_cursor(p, i_table, wr_flag, key_info) -> Result<CursorId, i32>`: o `BtCursor` nasce
//!   aqui e vai para o slab; `sqlite3BtreeCursorZero`, `sqlite3BtreeCursorSize` e os dois
//!   invólucros com mutex (`btreeCursorWithLock`) somem.
//! * `btree_close_cursor(p, id) -> bool` tira o cursor do slab (o `sqlite3_free` de `aOverflow`
//!   e `pKey` é o `Drop`). Devolve verdadeiro quando o C chamaria `sqlite3BtreeClose(pBtree)`
//!   ali (`BTREE_SINGLE` e último cursor): o `Btree` não pode se fechar por `&mut`, então o
//!   dono dele (o cursor do VDBE) chama `btree_close(p, db)` em seguida. O C devolve sempre
//!   `SQLITE_OK`.
//! * `insert_cell_fast` é `insert_cell` com `i_child == 0` (as duas do C são cópias uma da
//!   outra; `iChild` é sempre > 0 em `insertCell`). `apOvfl` possui uma cópia da célula, então
//!   o `pTemp` do C não existe.
//! * `btree_payload_fetch` devolve a fatia local do payload (o par `pAmt` e ponteiro).
//! * `btree_clear_cell` é a macro `BTREE_CLEAR_CELL` (devolve o `CellInfo` em `info`).
//! * `btree_table_moveto` e os cursores: `p_idx_key` é `&mut UnpackedRecord` (o comparador grava
//!   `err_code` e `eq_seen` nele).
//! * Nomes que colidem entre a função pública e a `static` de mesmo nome ganham o sufixo
//!   `_inner` na `static`: `btree_begin_trans_inner`, `btree_last_inner`, `btree_next_inner`,
//!   `btree_previous_inner`.
//! * `CellArray`, `populate_cell_cache`, `NN` e `NB` (fim do chunk 017) são definidos em
//!   `btree_write.rs`, que os usa; não são duplicados aqui.
//!
//! De `crate::btree` (chunks 000 a 008) este arquivo usa: `find_cell`, `find_cell_past_ptr`
//! (índice `i32`), `x_parse_cell`, `btree_parse_cell`, `btree_get_page`,
//! `btree_get_unused_page`, `get_and_init_page`, `btree_page_lookup`, `release_page*`,
//! `ptrmap_ispage`, `ptrmap_pageno`, `ptrmap_get` (com `Option<&mut u32>`), `ptrmap_put`,
//! `ptrmap_put_ovfl_ptr` (com `src_data_end`), `btree_init_page`, `allocate_space`,
//! `free_space`, `save_all_cursors`, `save_cursor_position`, `btree_release_all_cursor_pages`,
//! `btree_restore_cursor_position`, `restore_cursor_position`, `btree_clear_cursor`,
//! `invalidate_all_overflow_cache`, `btree_set_has_content`, `btree_get_has_content`,
//! `btree_clear_has_content`, `allocate_temp_space`, `lock_btree`, `unlock_btree_if_unused`,
//! `new_database`, `clear_all_shared_cache_table_locks` e
//! `downgrade_all_shared_cache_table_locks`. `btreePagecount` é `bt.n_page`.

use std::rc::Rc;

use crate::btree::*;
use crate::btree_types::{
    BtCursor, BtShared, Btree, BtreePayload, CellInfo, CursorId, MemPage,
};
use crate::consts::*;
use crate::mem::{KeyInfo, UnpackedRecord};
use crate::pager::file_of;
use crate::pcache::PgId;
use crate::record::{find_compare, record_compare, RecordCompare};
use crate::util::{
    abs_int32, at, get2byte, get4byte, get_varint, put2byte, put4byte, put_varint, put_varint32,
};

/// O que o C lê de `p->db` nas rotinas de transação (CONVENTIONS.md, item 4: o `Btree` não
/// aponta para a conexão; ela monta isto antes de chamar).
pub(crate) struct BtDb<'a> {
    /// `db->nSavepoint`.
    pub n_savepoint: i32,
    /// `db->nVdbeRead`.
    pub n_vdbe_read: i32,
    /// `sqlite3TempInMemory(db)`.
    pub temp_in_memory: bool,
    /// `sqlite3InvokeBusyHandler(&db->busyHandler)`: devolve verdadeiro para tentar de novo;
    /// `None` quando não há busy-handler.
    pub busy: Option<&'a mut dyn FnMut() -> bool>,
    /// `db->xAutovacPages` já com `pAutovacPagesArg` e o nome do banco capturados; recebe
    /// `(nOrig, nFree, pageSize)` e devolve `nVac`. `None` quando não há callback.
    pub autovac_pages: Option<&'a mut dyn FnMut(u32, u32, u32) -> u32>,
}

/// `BtDb` neutro (conexão sem savepoints, leitores, busy-handler nem callback de autovacuum).
impl Default for BtDb<'_> {
    fn default() -> Self {
        BtDb { n_savepoint: 0, n_vdbe_read: 0, temp_in_memory: false, busy: None, autovac_pages: None }
    }
}

/// Lê quatro bytes big-endian sem entrar em pânico: uma leitura além do fim da fatia enxerga
/// zeros (o C leria a memória vizinha; só acontece em banco corrompido).
#[inline]
fn rd4(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([at(data, off), at(data, off + 1), at(data, off + 2), at(data, off + 3)])
}

/// `pBt->pPage1` (o C supõe sempre presente dentro de uma transação).
#[inline]
fn page1(bt: &BtShared) -> PgId {
    bt.p_page1.expect("btree: pPage1 ausente (invariante do C)")
}

/// A página corrente do cursor (o `assert( pCur->pPage )` do C).
#[inline]
fn cur_page(cur: &BtCursor) -> PgId {
    cur.p_page.expect("btree: cursor sem página (invariante do C)")
}

/// Fatia `[start, start+n)` de `data` recortada ao tamanho da página (corrupção).
#[inline]
fn key_slice(data: &[u8], start: usize, n: usize) -> &[u8] {
    let s = start.min(data.len());
    let e = start.saturating_add(n).min(data.len());
    &data[s..e]
}

// ---------------------------------------------------------------------------------------------
// chunk 009: transações
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeNewDb`: inicializa a primeira página do arquivo.
pub(crate) fn btree_new_db(p: &mut Btree) -> i32 {
    p.bt.n_page = 0;
    new_database(&mut p.bt)
}

/// `btreeBeginTrans`: abre uma transação de leitura (`wrflag == 0`) ou de escrita.
pub(crate) fn btree_begin_trans_inner(
    p: &mut Btree,
    wrflag: i32,
    schema_version: Option<&mut i32>,
    db: &mut BtDb<'_>,
) -> i32 {
    let mut rc = SQLITE_OK;

    'trans_begun: {
        // Já em escrita, ou já em leitura e leitura pedida: nada a fazer.
        if p.in_trans == TRANS_WRITE || (p.in_trans == TRANS_READ && wrflag == 0) {
            break 'trans_begun;
        }

        if (p.bt.db_flags & SQLITE_RESET_DATABASE) != 0 && !p.bt.pager.read_only {
            p.bt.bts_flags &= !BTS_READ_ONLY;
        }

        // Escrita num banco somente leitura é impossível.
        if (p.bt.bts_flags & BTS_READ_ONLY) != 0 && wrflag != 0 {
            rc = SQLITE_READONLY;
            break 'trans_begun;
        }

        // Cache compartilhado: outro escritor, ou escrita pendente.
        if (wrflag != 0 && p.bt.in_transaction == TRANS_WRITE)
            || (p.bt.bts_flags & BTS_PENDING) != 0
        {
            rc = SQLITE_LOCKED_SHAREDCACHE;
            break 'trans_begun;
        }

        p.bt.bts_flags &= !BTS_INITIALLY_EMPTY;
        if p.bt.n_page == 0 {
            p.bt.bts_flags |= BTS_INITIALLY_EMPTY;
        }
        loop {
            // Chama lockBtree() até pPage1 ficar preenchida ou ele devolver algo diferente de
            // SQLITE_OK. lockBtree() pode devolver SQLITE_OK e deixar pPage1 em 0 quando o
            // tamanho de página do arquivo difere de pBt->pageSize (ele atualiza pageSize).
            while p.bt.p_page1.is_none() {
                rc = lock_btree(&mut p.bt);
                if rc != SQLITE_OK {
                    break;
                }
            }

            if rc == SQLITE_OK && wrflag != 0 {
                if (p.bt.bts_flags & BTS_READ_ONLY) != 0 {
                    rc = SQLITE_READONLY;
                } else {
                    rc = p.bt.pager.begin(wrflag > 1, db.temp_in_memory);
                    if rc == SQLITE_OK {
                        rc = new_database(&mut p.bt);
                    } else if rc == SQLITE_BUSY_SNAPSHOT && p.bt.in_transaction == TRANS_NONE {
                        // Sem transação aberta ao entrar e SQLITE_BUSY_SNAPSHOT: vira BUSY.
                        rc = SQLITE_BUSY;
                    }
                }
            }

            if rc != SQLITE_OK {
                unlock_btree_if_unused(&mut p.bt);
            }
            let retry = (rc & 0xFF) == SQLITE_BUSY
                && p.bt.in_transaction == TRANS_NONE
                && db.busy.as_mut().is_some_and(|busy| busy());
            if !retry {
                break;
            }
        }

        if rc == SQLITE_OK {
            if p.in_trans == TRANS_NONE {
                p.bt.n_transaction += 1;
                if p.sharable {
                    p.lock.e_lock = READ_LOCK;
                    p.bt.lock_list.insert(0, p.lock);
                }
            }
            p.in_trans = if wrflag != 0 { TRANS_WRITE } else { TRANS_READ };
            if p.in_trans > p.bt.in_transaction {
                p.bt.in_transaction = p.in_trans;
            }
            if wrflag != 0 {
                let pg1 = page1(&p.bt);
                p.bt.has_writer = true;
                p.bt.bts_flags &= !BTS_EXCLUSIVE;
                if wrflag > 1 {
                    p.bt.bts_flags |= BTS_EXCLUSIVE;
                }

                // Se o campo de tamanho do banco na página 1 está errado (um cliente antigo
                // pode ter escrito o arquivo), corrige agora: assim o tamanho pode ser relido
                // da página 1 se um savepoint ou rollback ocorrer dentro da transação.
                if p.bt.n_page != get4byte(&p.bt.pager.page_data(pg1)[28..]) {
                    rc = p.bt.pager.write(pg1);
                    if rc == SQLITE_OK {
                        put4byte(&mut p.bt.pager.page_data_mut(pg1)[28..], p.bt.n_page);
                    }
                }
            }
        }
    }

    // trans_begun:
    if rc == SQLITE_OK {
        if let Some(sv) = schema_version {
            *sv = get4byte(&p.bt.pager.page_data(page1(&p.bt))[40..]) as i32;
        }
        if wrflag != 0 {
            // Garante que o pager tem o número certo de savepoints abertos; se o segundo
            // parâmetro for maior que 0 e o sub-journal não estiver aberto, abre aqui.
            rc = p.bt.pager.open_savepoint(db.n_savepoint);
        }
    }
    rc
}

/// `sqlite3BtreeBeginTrans`: caminho rápido quando já há transação suficiente.
pub(crate) fn btree_begin_trans(
    p: &mut Btree,
    wrflag: i32,
    schema_version: Option<&mut i32>,
    db: &mut BtDb<'_>,
) -> i32 {
    if p.sharable
        || p.in_trans == TRANS_NONE
        || (p.in_trans == TRANS_READ && wrflag != 0)
    {
        return btree_begin_trans_inner(p, wrflag, schema_version, db);
    }
    if let Some(sv) = schema_version {
        *sv = get4byte(&p.bt.pager.page_data(page1(&p.bt))[40..]) as i32;
    }
    if wrflag != 0 {
        p.bt.pager.open_savepoint(db.n_savepoint)
    } else {
        SQLITE_OK
    }
}

/// `setChildPtrmaps`: grava as entradas do mapa de ponteiros para todos os filhos da página
/// `pg` e, se houver células com páginas de overflow, também para elas.
pub(crate) fn set_child_ptrmaps(bt: &mut BtShared, pg: PgId) -> i32 {
    let pgno = bt.pager.page_extra(pg).pgno;
    let mut rc = if bt.pager.page_extra(pg).is_init { SQLITE_OK } else { btree_init_page(bt, pg) };
    if rc != SQLITE_OK {
        return rc;
    }
    let n_cell = bt.pager.page_extra(pg).n_cell as usize;
    let leaf = bt.pager.page_extra(pg).leaf;
    let hdr = bt.pager.page_extra(pg).hdr_offset as usize;
    let max_local = bt.pager.page_extra(pg).max_local as usize;

    for i in 0..n_cell {
        // A célula é copiada porque ptrmap_put_ovfl_ptr precisa do `BtShared` por inteiro.
        let (cell, child, room) = {
            let (page, data) = bt.pager.page_parts(pg);
            let off = find_cell(page, data, i as i32);
            let end = off.saturating_add(max_local + 32).min(data.len());
            (
                data[off.min(end)..end].to_vec(),
                rd4(data, off),
                page.a_data_end.saturating_sub(off),
            )
        };
        ptrmap_put_ovfl_ptr(bt, pg, Some(room), &cell, &mut rc);
        if !leaf {
            ptrmap_put(bt, child, PTRMAP_BTREE, pgno, &mut rc);
        }
    }

    if !leaf {
        let child = rd4(bt.pager.page_data(pg), hdr + 8);
        ptrmap_put(bt, child, PTRMAP_BTREE, pgno, &mut rc);
    }
    rc
}

/// `modifyPagePointer`: em algum lugar da página `pg` há um ponteiro para `i_from`; troca-o
/// por `i_to`. `e_type` diz que ponteiro é (`PTRMAP_BTREE`, `PTRMAP_OVERFLOW1` ou
/// `PTRMAP_OVERFLOW2`).
pub(crate) fn modify_page_pointer(
    bt: &mut BtShared,
    pg: PgId,
    i_from: u32,
    i_to: u32,
    e_type: u8,
) -> i32 {
    if e_type == PTRMAP_OVERFLOW2 {
        // O ponteiro é sempre os 4 primeiros bytes da página.
        let data = bt.pager.page_data_mut(pg);
        if get4byte(data) != i_from {
            return SQLITE_CORRUPT_BKPT;
        }
        put4byte(data, i_to);
    } else {
        let rc = if bt.pager.page_extra(pg).is_init { SQLITE_OK } else { btree_init_page(bt, pg) };
        if rc != SQLITE_OK {
            return rc;
        }
        let usable = bt.usable_size as usize;
        let (page, data) = bt.pager.page_parts(pg);
        let n_cell = page.n_cell as usize;
        let hdr = page.hdr_offset as usize;

        let mut i = 0;
        while i < n_cell {
            let cell = find_cell(page, data, i as i32);
            if e_type == PTRMAP_OVERFLOW1 {
                let mut info = CellInfo::default();
                x_parse_cell(page, data, cell, &mut info);
                if (info.n_local as u32) < info.n_payload {
                    if cell + info.n_size as usize > usable {
                        return SQLITE_CORRUPT_BKPT;
                    }
                    let at_end = cell + info.n_size as usize - 4;
                    if i_from == rd4(data, at_end) {
                        put4byte(&mut data[at_end..], i_to);
                        break;
                    }
                }
            } else {
                if cell + 4 > usable {
                    return SQLITE_CORRUPT_BKPT;
                }
                if rd4(data, cell) == i_from {
                    put4byte(&mut data[cell..], i_to);
                    break;
                }
            }
            i += 1;
        }

        if i == n_cell {
            if e_type != PTRMAP_BTREE || rd4(data, hdr + 8) != i_from {
                return SQLITE_CORRUPT_BKPT;
            }
            put4byte(&mut data[hdr + 8..], i_to);
        }
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// chunk 010: auto-vacuum e commit
// ---------------------------------------------------------------------------------------------

/// `relocatePage`: move a página aberta `pg` para o local `i_free_page` do banco. A referência
/// a `pg` continua válida.
pub(crate) fn relocate_page(
    bt: &mut BtShared,
    pg: PgId,
    e_type: u8,
    i_ptr_page: u32,
    i_free_page: u32,
    is_commit: bool,
) -> i32 {
    let i_db_page = bt.pager.page_extra(pg).pgno;
    if i_db_page < 3 {
        return SQLITE_CORRUPT_BKPT;
    }

    // Move a página iDbPage do local atual para o número iFreePage.
    let mut rc = bt.pager.movepage(pg, i_free_page, is_commit);
    if rc != SQLITE_OK {
        return rc;
    }
    bt.pager.page_extra_mut(pg).pgno = i_free_page;

    // Se pg era página de árvore, as entradas do mapa de ponteiros dos filhos e das páginas
    // de overflow mudam. Se era overflow, os 4 primeiros bytes podem apontar para a próxima.
    if e_type == PTRMAP_BTREE || e_type == PTRMAP_ROOTPAGE {
        rc = set_child_ptrmaps(bt, pg);
        if rc != SQLITE_OK {
            return rc;
        }
    } else {
        let next_ovfl = rd4(bt.pager.page_data(pg), 0);
        if next_ovfl != 0 {
            ptrmap_put(bt, next_ovfl, PTRMAP_OVERFLOW2, i_free_page, &mut rc);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    // Corrige o ponteiro da página iPtrPage que apontava para iDbPage e a entrada do mapa.
    if e_type != PTRMAP_ROOTPAGE {
        let p_ptr_page = match btree_get_page(bt, i_ptr_page, 0) {
            Ok(p) => p,
            Err(e) => return e,
        };
        rc = bt.pager.write(p_ptr_page);
        if rc != SQLITE_OK {
            release_page(bt, Some(p_ptr_page));
            return rc;
        }
        rc = modify_page_pointer(bt, p_ptr_page, i_db_page, i_free_page, e_type);
        release_page(bt, Some(p_ptr_page));
        if rc == SQLITE_OK {
            ptrmap_put(bt, i_free_page, e_type, i_ptr_page, &mut rc);
        }
    }
    rc
}

/// `incrVacuumStep`: um passo de um vacuum incremental. `SQLITE_OK` se houve progresso,
/// `SQLITE_DONE` se não há mais o que fazer, outro código em erro.
pub(crate) fn incr_vacuum_step(
    bt: &mut BtShared,
    n_fin: u32,
    i_last_pg: u32,
    b_commit: bool,
) -> i32 {
    let mut i_last_pg = i_last_pg;

    if !ptrmap_ispage(bt, i_last_pg) && i_last_pg != bt.pending_byte_page() {
        let n_free_list = rd4(bt.pager.page_data(page1(bt)), 36);
        if n_free_list == 0 {
            return SQLITE_DONE;
        }

        let mut e_type: u8 = 0;
        let mut i_ptr_page: u32 = 0;
        let rc = ptrmap_get(bt, i_last_pg, &mut e_type, Some(&mut i_ptr_page));
        if rc != SQLITE_OK {
            return rc;
        }
        if e_type == PTRMAP_ROOTPAGE {
            return SQLITE_CORRUPT_BKPT;
        }

        if e_type == PTRMAP_FREEPAGE {
            if !b_commit {
                // Tira a página da lista livre do arquivo. Não é preciso se b_commit: a lista
                // será truncada a zero depois, então lixo nela não importa.
                match allocate_btree_page(bt, i_last_pg, BTALLOC_EXACT) {
                    Err(e) => return e,
                    Ok(p_free_pg) => release_page(bt, Some(p_free_pg)),
                }
            }
        } else {
            let mut e_mode = BTALLOC_ANY;
            let mut i_near: u32 = 0;

            let p_last_pg = match btree_get_page(bt, i_last_pg, 0) {
                Ok(p) => p,
                Err(e) => return e,
            };

            // Sem b_commit o laço roda uma vez e a página é trocada com a primeira livre.
            // Com b_commit repete até achar uma página livre dentro das primeiras n_fin.
            if !b_commit {
                e_mode = BTALLOC_LE;
                i_near = n_fin;
            }
            let mut i_free_pg;
            loop {
                let db_size = bt.n_page;
                match allocate_btree_page(bt, i_near, e_mode) {
                    Err(e) => {
                        release_page(bt, Some(p_last_pg));
                        return e;
                    }
                    Ok(p_free_pg) => {
                        i_free_pg = bt.pager.page_extra(p_free_pg).pgno;
                        release_page(bt, Some(p_free_pg));
                        if i_free_pg > db_size {
                            release_page(bt, Some(p_last_pg));
                            return SQLITE_CORRUPT_BKPT;
                        }
                    }
                }
                if !(b_commit && i_free_pg > n_fin) {
                    break;
                }
            }

            let rc = relocate_page(bt, p_last_pg, e_type, i_ptr_page, i_free_pg, b_commit);
            release_page(bt, Some(p_last_pg));
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }

    if !b_commit {
        loop {
            i_last_pg = i_last_pg.wrapping_sub(1);
            if !(i_last_pg == bt.pending_byte_page() || ptrmap_ispage(bt, i_last_pg)) {
                break;
            }
        }
        bt.do_truncate = 1;
        bt.n_page = i_last_pg;
    }
    SQLITE_OK
}

/// `finalDbSize`: tamanho esperado do banco, em páginas, depois de um auto-vacuum de um banco
/// de `n_orig` páginas com `n_free` livres.
pub(crate) fn final_db_size(bt: &BtShared, n_orig: u32, n_free: u32) -> u32 {
    let n_entry = bt.usable_size / 5;
    let n_ptrmap = n_free
        .wrapping_sub(n_orig)
        .wrapping_add(ptrmap_pageno(bt, n_orig))
        .wrapping_add(n_entry)
        / n_entry;
    let mut n_fin = n_orig.wrapping_sub(n_free).wrapping_sub(n_ptrmap);
    if n_orig > bt.pending_byte_page() && n_fin < bt.pending_byte_page() {
        n_fin = n_fin.wrapping_sub(1);
    }
    while ptrmap_ispage(bt, n_fin) || n_fin == bt.pending_byte_page() {
        n_fin = n_fin.wrapping_sub(1);
    }
    n_fin
}

/// `sqlite3BtreeIncrVacuum`: uma unidade de trabalho de um vacuum incremental. Exige
/// transação de escrita aberta. `SQLITE_DONE` quando terminou.
pub(crate) fn btree_incr_vacuum(p: &mut Btree) -> i32 {
    let bt = &mut p.bt;
    let rc;
    if bt.auto_vacuum == 0 {
        rc = SQLITE_DONE;
    } else {
        let n_orig = bt.n_page;
        let n_free = rd4(bt.pager.page_data(page1(bt)), 36);
        let n_fin = final_db_size(bt, n_orig, n_free);

        if n_orig < n_fin || n_free >= n_orig {
            rc = SQLITE_CORRUPT_BKPT;
        } else if n_free > 0 {
            let mut r = save_all_cursors(bt, 0, None);
            if r == SQLITE_OK {
                invalidate_all_overflow_cache(bt);
                r = incr_vacuum_step(bt, n_fin, n_orig, false);
            }
            if r == SQLITE_OK {
                let pg1 = page1(bt);
                r = bt.pager.write(pg1);
                put4byte(&mut bt.pager.page_data_mut(pg1)[28..], bt.n_page);
            }
            rc = r;
        } else {
            rc = SQLITE_DONE;
        }
    }
    rc
}

/// `autoVacuumCommit`: chamada antes de `sqlite3PagerCommit` ao confirmar um banco com
/// auto-vacuum.
fn auto_vacuum_commit(p: &mut Btree, db: &mut BtDb<'_>) -> i32 {
    let bt = &mut p.bt;
    let mut rc = SQLITE_OK;

    invalidate_all_overflow_cache(bt);
    if bt.incr_vacuum == 0 {
        let n_orig = bt.n_page;
        if ptrmap_ispage(bt, n_orig) || n_orig == bt.pending_byte_page() {
            // Não é possível criar um banco cuja última página seja de mapa de ponteiros ou
            // a do byte pendente; se acontece, o arquivo está corrompido.
            return SQLITE_CORRUPT_BKPT;
        }

        let n_free = rd4(bt.pager.page_data(page1(bt)), 36);
        let mut n_vac;
        if let Some(autovac) = db.autovac_pages.as_mut() {
            n_vac = autovac(n_orig, n_free, bt.page_size);
            if n_vac > n_free {
                n_vac = n_free;
            }
            if n_vac == 0 {
                return SQLITE_OK;
            }
        } else {
            n_vac = n_free;
        }
        let n_fin = final_db_size(bt, n_orig, n_vac);
        if n_fin > n_orig {
            return SQLITE_CORRUPT_BKPT;
        }
        if n_fin < n_orig {
            rc = save_all_cursors(bt, 0, None);
        }
        let mut i_free = n_orig;
        while i_free > n_fin && rc == SQLITE_OK {
            rc = incr_vacuum_step(bt, n_fin, i_free, n_vac == n_free);
            i_free -= 1;
        }
        if (rc == SQLITE_DONE || rc == SQLITE_OK) && n_free > 0 {
            let pg1 = page1(bt);
            rc = bt.pager.write(pg1);
            let data = bt.pager.page_data_mut(pg1);
            if n_vac == n_free {
                put4byte(&mut data[32..], 0);
                put4byte(&mut data[36..], 0);
            }
            put4byte(&mut data[28..], n_fin);
            bt.do_truncate = 1;
            bt.n_page = n_fin;
        }
        if rc != SQLITE_OK {
            bt.pager.rollback();
        }
    }
    rc
}

/// `sqlite3BtreeCommitPhaseOne`: primeira fase do commit em duas fases. Sem efeito se não há
/// transação de escrita.
pub(crate) fn btree_commit_phase_one(
    p: &mut Btree,
    z_super_jrnl: Option<&[u8]>,
    db: &mut BtDb<'_>,
) -> i32 {
    let mut rc = SQLITE_OK;
    if p.in_trans == TRANS_WRITE {
        if p.bt.auto_vacuum != 0 {
            rc = auto_vacuum_commit(p, db);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        if p.bt.do_truncate != 0 {
            let n_page = p.bt.n_page;
            p.bt.pager.truncate_image(n_page);
        }
        rc = p.bt.pager.commit_phase_one(z_super_jrnl, false);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 011: fim de transação, rollback, savepoints, abrir cursor
// ---------------------------------------------------------------------------------------------

/// `btreeEndTransaction`: chamada por CommitPhaseTwo e por Rollback ao fim da transação.
fn btree_end_transaction(p: &mut Btree, db: &BtDb<'_>) {
    p.bt.do_truncate = 0;
    if p.in_trans > TRANS_NONE && db.n_vdbe_read > 1 {
        // Há outras instruções ativas desta conexão: rebaixa para leitura, elas ainda podem
        // estar lendo o banco.
        downgrade_all_shared_cache_table_locks(p);
        p.in_trans = TRANS_READ;
    } else {
        // Se havia transação, decrementa o contador do btree; em zero o estado vira NONE e o
        // unlockBtreeIfUnused() abaixo solta o pager.
        if p.in_trans != TRANS_NONE {
            clear_all_shared_cache_table_locks(p);
            p.bt.n_transaction -= 1;
            if p.bt.n_transaction == 0 {
                p.bt.in_transaction = TRANS_NONE;
            }
        }
        p.in_trans = TRANS_NONE;
        unlock_btree_if_unused(&mut p.bt);
    }
}

/// `sqlite3BtreeCommitPhaseTwo`: segunda fase do commit; apaga ou trunca o journal e solta as
/// travas. Com `b_cleanup`, um erro do pager não impede o fechamento da transação.
pub(crate) fn btree_commit_phase_two(p: &mut Btree, b_cleanup: bool, db: &BtDb<'_>) -> i32 {
    if p.in_trans == TRANS_NONE {
        return SQLITE_OK;
    }

    // Com transação de escrita, confirma e passa o estado compartilhado para TRANS_READ.
    if p.in_trans == TRANS_WRITE {
        let rc = p.bt.pager.commit_phase_two();
        if rc != SQLITE_OK && !b_cleanup {
            return rc;
        }
        p.i_b_data_version = p.i_b_data_version.wrapping_sub(1); // compensa iDataVersion++
        p.bt.in_transaction = TRANS_READ;
        btree_clear_has_content(&mut p.bt);
    }

    btree_end_transaction(p, db);
    SQLITE_OK
}

/// `sqlite3BtreeCommit`: as duas fases.
pub(crate) fn btree_commit(p: &mut Btree, db: &mut BtDb<'_>) -> i32 {
    let mut rc = btree_commit_phase_one(p, None, db);
    if rc == SQLITE_OK {
        rc = btree_commit_phase_two(p, false, db);
    }
    rc
}

/// `sqlite3BtreeTripAllCursors`: põe todo cursor em `CURSOR_FAULT` com `err_code` (ou só os
/// de escrita, se `write_only`).
pub(crate) fn btree_trip_all_cursors(p: &mut Btree, err_code: i32, write_only: bool) -> i32 {
    let mut rc = SQLITE_OK;
    let ids: Vec<CursorId> = p.bt.cursors.iter().map(|(id, _)| id).collect();
    for id in ids {
        let Some(mut cur) = p.bt.cursors.take(id) else {
            continue;
        };
        if write_only && (cur.cur_flags & BTCF_WRITE_FLAG) == 0 {
            if cur.e_state == CURSOR_VALID || cur.e_state == CURSOR_SKIPNEXT {
                rc = save_cursor_position(&mut cur, &mut p.bt);
                if rc != SQLITE_OK {
                    // Não deu para salvar a posição: derruba todos, inclusive este.
                    p.bt.cursors.put(id, cur);
                    let _ = btree_trip_all_cursors(p, rc, false);
                    break;
                }
            }
        } else {
            btree_clear_cursor(&mut cur);
            cur.e_state = CURSOR_FAULT;
            cur.skip_next = err_code;
        }
        btree_release_all_cursor_pages(&mut cur, &mut p.bt);
        p.bt.cursors.put(id, cur);
    }
    rc
}

/// `btreeSetNPage`: acerta `bt.n_page` conforme o estado do banco; supõe a página 1 válida.
fn btree_set_n_page(bt: &mut BtShared, pg1: PgId) {
    let mut n_page = rd4(bt.pager.page_data(pg1), 28);
    if n_page == 0 {
        n_page = bt.pager.pagecount() as u32;
    }
    bt.n_page = n_page;
}

/// `sqlite3BtreeRollback`: desfaz a transação em curso. Com `trip_code != SQLITE_OK` os
/// cursores são derrubados (só os de escrita se `write_only`).
pub(crate) fn btree_rollback(
    p: &mut Btree,
    trip_code: i32,
    write_only: bool,
    db: &BtDb<'_>,
) -> i32 {
    let mut rc;
    let mut trip_code = trip_code;
    let mut write_only = write_only;

    if trip_code == SQLITE_OK {
        rc = save_all_cursors(&mut p.bt, 0, None);
        trip_code = rc;
        if rc != SQLITE_OK {
            write_only = false;
        }
    } else {
        rc = SQLITE_OK;
    }
    if trip_code != SQLITE_OK {
        let rc2 = btree_trip_all_cursors(p, trip_code, write_only);
        if rc2 != SQLITE_OK {
            rc = rc2;
        }
    }

    if p.in_trans == TRANS_WRITE {
        let rc2 = p.bt.pager.rollback();
        if rc2 != SQLITE_OK {
            rc = rc2;
        }

        // O rollback pode ter destruído os bytes da página 1: busca de novo para ter certeza.
        if let Ok(pg1) = btree_get_page(&mut p.bt, 1, 0) {
            btree_set_n_page(&mut p.bt, pg1);
            release_page_one(&mut p.bt, pg1);
        }
        p.bt.in_transaction = TRANS_READ;
        btree_clear_has_content(&mut p.bt);
    }

    btree_end_transaction(p, db);
    rc
}

/// `sqlite3BtreeBeginStmt`: abre uma subtransação de instrução (um savepoint anônimo no
/// pager). `i_statement` é o total de savepoints abertos, contando o novo.
pub(crate) fn btree_begin_stmt(p: &mut Btree, i_statement: i32) -> i32 {
    p.bt.pager.open_savepoint(i_statement)
}

/// `sqlite3BtreeSavepoint`: libera ou desfaz (`op`) o savepoint `i_savepoint`; com
/// `SAVEPOINT_ROLLBACK`, `i_savepoint` pode ser -1 (desfaz a transação inteira sem soltar
/// travas).
pub(crate) fn btree_savepoint(p: &mut Btree, op: i32, i_savepoint: i32) -> i32 {
    let mut rc = SQLITE_OK;
    if p.in_trans == TRANS_WRITE {
        let bt = &mut p.bt;
        if op == SAVEPOINT_ROLLBACK {
            rc = save_all_cursors(bt, 0, None);
        }
        if rc == SQLITE_OK {
            rc = bt.pager.savepoint(op, i_savepoint);
        }
        if rc == SQLITE_OK {
            if i_savepoint < 0 && (bt.bts_flags & BTS_INITIALLY_EMPTY) != 0 {
                bt.n_page = 0;
            }
            rc = new_database(bt);
            let pg1 = page1(bt);
            btree_set_n_page(bt, pg1);
        }
    }
    rc
}

/// `btreeCursor` (e `sqlite3BtreeCursor`, que só acrescentava o mutex): cria um cursor na
/// árvore de raiz `i_table`. `wr_flag` é 0, `BTREE_WRCSR` ou `BTREE_WRCSR | BTREE_FORDELETE`.
/// O cursor entra no slab do `BtShared` e o handle é devolvido.
pub(crate) fn btree_cursor(
    p: &mut Btree,
    i_table: u32,
    wr_flag: u32,
    key_info: Option<Rc<KeyInfo>>,
) -> Result<CursorId, i32> {
    let bt = &mut p.bt;
    let mut i_table = i_table;

    if i_table <= 1 {
        if i_table < 1 {
            return Err(SQLITE_CORRUPT_BKPT);
        } else if bt.n_page == 0 {
            i_table = 0;
        }
    }

    // Terminado de conferir: preenche o cursor e o liga ao BtShared.
    let mut cur = BtCursor {
        pgno_root: i_table,
        i_page: -1,
        p_key_info: key_info,
        cur_flags: 0,
        ..BtCursor::default()
    };
    // Dois ou mais cursores na mesma árvore devem ter BTCF_MULTIPLE.
    for (_, px) in bt.cursors.iter_mut() {
        if px.pgno_root == i_table {
            px.cur_flags |= BTCF_MULTIPLE;
            cur.cur_flags = BTCF_MULTIPLE;
        }
    }
    cur.e_state = CURSOR_INVALID;
    if wr_flag != 0 {
        cur.cur_flags |= BTCF_WRITE_FLAG;
        cur.cur_pager_flags = 0;
    } else {
        cur.cur_pager_flags = PAGER_GET_READONLY as u8;
    }
    let id = bt.cursors.insert(cur);
    if wr_flag != 0 && bt.p_tmp_space.is_empty() {
        let rc = allocate_temp_space(bt);
        if rc != SQLITE_OK {
            bt.cursors.remove(id);
            return Err(rc);
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------------------------
// chunk 012: fechar cursor, informação da célula, payload
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeCloseCursor`: fecha o cursor; o lock de leitura sai quando o último fecha.
/// Devolve verdadeiro quando o C chamaria `sqlite3BtreeClose(pBtree)` aqui (o arquivo é
/// `BTREE_SINGLE` e este era o último cursor): um `&mut Btree` não pode se fechar, então o dono
/// do `Btree` (o cursor do VDBE) chama `btree_close(p, db)` em seguida. O C devolve sempre
/// `SQLITE_OK`.
pub(crate) fn btree_close_cursor(p: &mut Btree, id: CursorId) -> bool {
    let Some(mut cur) = p.bt.cursors.remove(id) else {
        return false;
    };
    btree_release_all_cursor_pages(&mut cur, &mut p.bt);
    unlock_btree_if_unused(&mut p.bt);
    // aOverflow e pKey são liberados pelo Drop de `cur`.
    drop(cur);
    (p.bt.open_flags & BTREE_SINGLE as u8) != 0 && p.bt.cursors.is_empty()
}

/// `getCellInfo`: garante que `cur.info` é válido (cache da célula corrente); senão analisa a
/// célula.
pub(crate) fn get_cell_info(cur: &mut BtCursor, bt: &mut BtShared) {
    if cur.info.n_size == 0 {
        cur.cur_flags |= BTCF_VALID_NKEY;
        let pg = cur_page(cur);
        let (page, data) = bt.pager.page_parts(pg);
        btree_parse_cell(page, data, cur.ix as i32, &mut cur.info);
    }
}

/// `sqlite3BtreeCursorIsValidNN`.
pub(crate) fn btree_cursor_is_valid_nn(cur: &BtCursor) -> bool {
    cur.e_state == CURSOR_VALID
}

/// `sqlite3BtreeIntegerKey`: o rowid da entrada corrente de uma tabela.
pub(crate) fn btree_integer_key(cur: &mut BtCursor, bt: &mut BtShared) -> i64 {
    get_cell_info(cur, bt);
    cur.info.n_key
}

/// `sqlite3BtreeCursorPin`.
pub(crate) fn btree_cursor_pin(cur: &mut BtCursor) {
    cur.cur_flags |= BTCF_PINNED;
}

/// `sqlite3BtreeCursorUnpin`.
pub(crate) fn btree_cursor_unpin(cur: &mut BtCursor) {
    cur.cur_flags &= !BTCF_PINNED;
}

/// `sqlite3BtreeOffset`: deslocamento no arquivo do início do payload da entrada corrente.
pub(crate) fn btree_offset(cur: &mut BtCursor, bt: &mut BtShared) -> i64 {
    get_cell_info(cur, bt);
    let pgno = bt.pager.page_extra(cur_page(cur)).pgno;
    bt.page_size as i64 * (pgno as i64 - 1) + cur.info.p_payload as i64
}

/// `sqlite3BtreePayloadSize`: bytes de payload da entrada corrente (dados em tabela, chave
/// em índice).
pub(crate) fn btree_payload_size(cur: &mut BtCursor, bt: &mut BtShared) -> u32 {
    get_cell_info(cur, bt);
    cur.info.n_payload
}

/// `sqlite3BtreeMaxRecordSize`: limite superior do tamanho de qualquer registro da tabela
/// (o tamanho do arquivo do banco).
pub(crate) fn btree_max_record_size(bt: &BtShared) -> i64 {
    bt.page_size as i64 * bt.n_page as i64
}

/// `getOverflowPage`: dado o número `ovfl` de uma página de overflow, acha a próxima da lista
/// encadeada (pelo mapa de ponteiros do auto-vacuum, se possível). Devolve `(página, próxima)`;
/// a página só vem se `want_page` (e se foi preciso lê-la), e então quem chama deve soltá-la
/// com `release_page`. `próxima == 0` quando `ovfl` é a última.
pub(crate) fn get_overflow_page(
    bt: &mut BtShared,
    ovfl: u32,
    want_page: bool,
) -> Result<(Option<PgId>, u32), i32> {
    let mut next: u32 = 0;
    let mut page: Option<PgId> = None;
    let mut rc = SQLITE_OK;

    // Tenta achar a próxima página da lista pelas páginas do mapa de ponteiros. Chuta que é
    // ovfl+1; se errar, lê os dados da página ovfl.
    if bt.auto_vacuum != 0 {
        let mut i_guess = ovfl.wrapping_add(1);
        while ptrmap_ispage(bt, i_guess) || i_guess == bt.pending_byte_page() {
            i_guess = i_guess.wrapping_add(1);
        }
        if i_guess <= bt.n_page {
            let mut e_type: u8 = 0;
            let mut pgno: u32 = 0;
            rc = ptrmap_get(bt, i_guess, &mut e_type, Some(&mut pgno));
            if rc == SQLITE_OK && e_type == PTRMAP_OVERFLOW2 && pgno == ovfl {
                next = i_guess;
                rc = SQLITE_DONE;
            }
        }
    }

    if rc == SQLITE_OK {
        match btree_get_page(bt, ovfl, if want_page { 0 } else { PAGER_GET_READONLY }) {
            Ok(pg) => {
                next = rd4(bt.pager.page_data(pg), 0);
                page = Some(pg);
            }
            Err(e) => rc = e,
        }
    }

    if !want_page {
        release_page(bt, page);
        page = None;
    }
    if rc == SQLITE_DONE {
        rc = SQLITE_OK;
    }
    if rc == SQLITE_OK {
        Ok((page, next))
    } else {
        Err(rc)
    }
}

/// `copyPayload`: copia entre o buffer e o payload na página `pg` (a partir de `off` nos bytes
/// da página). Com `e_op` falso lê da página para `buf`; com `e_op` verdadeiro chama
/// `sqlite3PagerWrite` e grava `buf` na página.
fn copy_payload(bt: &mut BtShared, pg: PgId, off: usize, buf: &mut [u8], e_op: i32) -> i32 {
    if e_op != 0 {
        let rc = bt.pager.write(pg);
        if rc != SQLITE_OK {
            return rc;
        }
        let Some(dst) = bt.pager.page_data_mut(pg).get_mut(off..off + buf.len()) else {
            return SQLITE_CORRUPT_BKPT;
        };
        dst.copy_from_slice(buf);
    } else {
        let Some(src) = bt.pager.page_data(pg).get(off..off + buf.len()) else {
            return SQLITE_CORRUPT_BKPT;
        };
        buf.copy_from_slice(src);
    }
    SQLITE_OK
}

/// `accessPayload`: lê (`e_op == 0`) ou sobrescreve (`e_op == 1`) `amt` bytes do payload da
/// entrada do cursor a partir de `offset`, de/para `buf` (que tem pelo menos `amt` bytes). O
/// conteúdo pode estar na página ou espalhado em páginas de overflow; o cache de overflow
/// (`p_overflow`) é preenchido sob demanda.
pub(crate) fn access_payload(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    offset: u32,
    amt: u32,
    buf: &mut [u8],
    e_op: i32,
) -> i32 {
    let mut offset = offset;
    let mut amt = amt;
    let mut buf_pos: usize = 0; // pBuf - pBufStart
    let mut rc = SQLITE_OK;
    let mut i_idx: usize = 0;
    let pg = cur_page(cur);

    if cur.ix >= bt.pager.page_extra(pg).n_cell {
        return SQLITE_CORRUPT_BKPT;
    }

    get_cell_info(cur, bt);
    let a_payload = cur.info.p_payload;
    let n_local = cur.info.n_local as u32;

    if (a_payload as u64) > bt.usable_size.wrapping_sub(n_local) as u64 {
        // Ler ou escrever além do fim dos dados é erro. A conta é reescrita assim para evitar
        // estouro de inteiro (o teste real é aPayload+nLocal > aData+usableSize).
        return SQLITE_CORRUPT_BKPT;
    }

    // Verifica se os dados ficam na própria página da árvore.
    if offset < n_local {
        let mut a = amt;
        if a.wrapping_add(offset) > n_local {
            a = n_local - offset;
        }
        rc = copy_payload(
            bt,
            pg,
            a_payload + offset as usize,
            &mut buf[buf_pos..buf_pos + a as usize],
            e_op,
        );
        offset = 0;
        buf_pos += a as usize;
        amt -= a;
    } else {
        offset -= n_local;
    }

    if rc == SQLITE_OK && amt > 0 {
        let ovfl_size = bt.usable_size - 4; // bytes de conteúdo por página de overflow
        let mut next_page = rd4(bt.pager.page_data(pg), a_payload + n_local as usize);

        // Se aOverflow[] ainda não vale, preenche com zeros ("ainda desconhecido"): uma
        // entrada por página de overflow da cadeia, a primeira em [0].
        if (cur.cur_flags & BTCF_VALID_OVFL) == 0 {
            let n_ovfl = ((cur.info.n_payload as u64).saturating_sub(n_local as u64)
                + ovfl_size as u64
                - 1)
                / ovfl_size as u64;
            if n_ovfl > bt.n_page as u64 {
                // Uma cadeia de overflow não cabe em mais páginas que o banco tem.
                return SQLITE_CORRUPT_BKPT;
            }
            let n_ovfl = n_ovfl as usize;
            if cur.p_overflow.len() < n_ovfl {
                cur.p_overflow.resize(n_ovfl * 2, None);
            }
            for e in cur.p_overflow.iter_mut().take(n_ovfl) {
                *e = None;
            }
            cur.cur_flags |= BTCF_VALID_OVFL;
        } else {
            // Se a entrada da primeira página de overflow necessária é válida, pula direto.
            if cur.p_overflow.get((offset / ovfl_size) as usize).copied().flatten().is_some() {
                i_idx = (offset / ovfl_size) as usize;
                next_page = cur.p_overflow[i_idx].unwrap_or(0);
                offset %= ovfl_size;
            }
        }

        while next_page != 0 {
            // Se preciso, preenche o cache da lista de páginas de overflow.
            if next_page > bt.n_page {
                return SQLITE_CORRUPT_BKPT;
            }
            if i_idx >= cur.p_overflow.len() {
                return SQLITE_CORRUPT_BKPT;
            }
            cur.p_overflow[i_idx] = Some(next_page);

            if offset >= ovfl_size {
                // Só se quer o número da próxima página da cadeia, não os dados: consulta
                // primeiro o cache, depois getOverflowPage().
                if let Some(n) = cur.p_overflow.get(i_idx + 1).copied().flatten() {
                    next_page = n;
                } else {
                    match get_overflow_page(bt, next_page, false) {
                        Ok((_, n)) => next_page = n,
                        Err(e) => {
                            rc = e;
                            next_page = 0;
                        }
                    }
                }
                offset -= ovfl_size;
            } else {
                // Precisa ler esta página: ela contém parte do intervalo lido ou escrito.
                let mut a = amt;
                if a.wrapping_add(offset) > ovfl_size {
                    a = ovfl_size - offset;
                }

                // Leitura direta (SQLITE_DIRECT_OVERFLOW_READ): leitura, a partir do começo
                // da página, sem páginas sujas no cache, arquivo real, página fora do WAL e
                // pelo menos 4 bytes já lidos no buffer de saída. Lê do arquivo direto para o
                // buffer, sem passar pelo cache de páginas.
                if e_op == 0
                    && offset == 0
                    && bt.pager.direct_read_ok(next_page)
                    && buf_pos >= 4
                {
                    let a_write = buf_pos - 4;
                    let mut a_save = [0u8; 4];
                    a_save.copy_from_slice(&buf[a_write..a_write + 4]);
                    let file_off = bt.page_size as i64 * (next_page as i64 - 1);
                    rc = file_of!(bt.pager, fd)
                        .read(&mut buf[a_write..a_write + a as usize + 4], file_off);
                    next_page = rd4(buf, a_write);
                    buf[a_write..a_write + 4].copy_from_slice(&a_save);
                } else {
                    match bt.pager.get(next_page, if e_op == 0 { PAGER_GET_READONLY } else { 0 }) {
                        Ok(db_page) => {
                            next_page = rd4(bt.pager.page_data(db_page), 0);
                            rc = copy_payload(
                                bt,
                                db_page,
                                4 + offset as usize,
                                &mut buf[buf_pos..buf_pos + a as usize],
                                e_op,
                            );
                            bt.pager.unref(Some(db_page));
                            offset = 0;
                        }
                        Err(e) => rc = e,
                    }
                }
                amt -= a;
                if amt == 0 {
                    return rc;
                }
                buf_pos += a as usize;
            }
            if rc != SQLITE_OK {
                break;
            }
            i_idx += 1;
        }
    }

    if rc == SQLITE_OK && amt > 0 {
        // A cadeia de overflow terminou antes da hora.
        return SQLITE_CORRUPT_BKPT;
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 013: payload, mover o cursor
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreePayload`: lê `amt` bytes a partir de `offset` do payload da entrada corrente
/// (o cursor deve estar `CURSOR_VALID`).
pub(crate) fn btree_payload(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    offset: u32,
    amt: u32,
    buf: &mut [u8],
) -> i32 {
    access_payload(cur, bt, offset, amt, buf, 0)
}

/// `accessPayloadChecked`.
fn access_payload_checked(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    offset: u32,
    amt: u32,
    buf: &mut [u8],
) -> i32 {
    if cur.e_state == CURSOR_INVALID {
        return SQLITE_ABORT;
    }
    let rc = btree_restore_cursor_position(cur, bt);
    if rc != SQLITE_OK {
        rc
    } else {
        access_payload(cur, bt, offset, amt, buf, 0)
    }
}

/// `sqlite3BtreePayloadChecked`: como `btree_payload`, mas funciona com o cursor fora de
/// `CURSOR_VALID` (só usado por `sqlite3_blob_read`).
pub(crate) fn btree_payload_checked(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    offset: u32,
    amt: u32,
    buf: &mut [u8],
) -> i32 {
    if cur.e_state == CURSOR_VALID {
        access_payload(cur, bt, offset, amt, buf, 0)
    } else {
        access_payload_checked(cur, bt, offset, amt, buf)
    }
}

/// `sqlite3BtreePayloadFetch` (e `fetchPayload`): a parte do payload da entrada corrente que
/// cabe na página, como fatia dos bytes dela. Vazia se a página não tem espaço (banco
/// corrompido). A fatia olha direto para a página em cache: vale só até a próxima chamada ao
/// btree.
pub(crate) fn btree_payload_fetch<'a>(cur: &BtCursor, bt: &'a BtShared) -> &'a [u8] {
    let pg = cur_page(cur);
    let page = bt.pager.page_extra(pg);
    let mut amt = cur.info.n_local as i64;
    let room = page.a_data_end as i64 - cur.info.p_payload as i64;
    if amt > room {
        // Pouco espaço na página para o conteúdo local esperado: banco corrompido.
        amt = room.max(0);
    }
    let data = bt.pager.page_data(pg);
    data.get(cur.info.p_payload..cur.info.p_payload + amt as usize).unwrap_or(&[])
}

/// `moveToChild`: desce o cursor para a página filha `new_pgno`. Devolve `SQLITE_CORRUPT` se
/// os flags de cabeçalho da filha não combinam com os do pai.
pub(crate) fn move_to_child(cur: &mut BtCursor, bt: &mut BtShared, new_pgno: u32) -> i32 {
    if cur.i_page < 0 || cur.i_page as usize >= BTCURSOR_MAX_DEPTH - 1 {
        return SQLITE_CORRUPT_BKPT;
    }
    cur.info.n_size = 0;
    cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    cur.ai_idx[cur.i_page as usize] = cur.ix;
    cur.ap_page[cur.i_page as usize] = cur.p_page;
    cur.ix = 0;
    cur.i_page += 1;
    let mut rc;
    match get_and_init_page(bt, new_pgno, cur.cur_pager_flags as i32) {
        Ok(pg) => {
            cur.p_page = Some(pg);
            rc = SQLITE_OK;
        }
        Err(e) => {
            cur.p_page = None;
            rc = e;
        }
    }
    if let Some(pg) = cur.p_page {
        let page = bt.pager.page_extra(pg);
        if page.n_cell < 1 || (page.int_key as u8) != cur.cur_int_key {
            release_page(bt, Some(pg));
            rc = SQLITE_CORRUPT_BKPT;
        }
    }
    if rc != SQLITE_OK {
        cur.i_page -= 1;
        cur.p_page = cur.ap_page[cur.i_page as usize];
    }
    rc
}

/// `moveToParent`: sobe o cursor para a página pai; `ix` passa a ser o índice da célula que
/// aponta para a página de onde veio (ou `nCell` se veio do filho mais à direita).
pub(crate) fn move_to_parent(cur: &mut BtCursor, bt: &mut BtShared) {
    cur.info.n_size = 0;
    cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    cur.ix = cur.ai_idx[(cur.i_page - 1) as usize];
    let p_leaf = cur_page(cur);
    cur.i_page -= 1;
    cur.p_page = cur.ap_page[cur.i_page as usize];
    release_page_not_null(bt, p_leaf);
}

/// `moveToRoot`: leva o cursor à página raiz da árvore (ou à raiz virtual: raiz sem células e
/// um único filho, só possível na tabela da página 1). Árvore vazia deixa o cursor
/// `CURSOR_INVALID` e devolve `SQLITE_EMPTY`; senão aponta a primeira célula da raiz.
pub(crate) fn move_to_root(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    let mut rc = SQLITE_OK;
    let mut skip_init = false;

    if cur.i_page >= 0 {
        if cur.i_page != 0 {
            release_page_not_null(bt, cur_page(cur));
            loop {
                cur.i_page -= 1;
                if cur.i_page == 0 {
                    break;
                }
                if let Some(pg) = cur.ap_page[cur.i_page as usize] {
                    release_page_not_null(bt, pg);
                }
            }
            cur.p_page = cur.ap_page[0];
            skip_init = true;
        }
    } else if cur.pgno_root == 0 {
        cur.e_state = CURSOR_INVALID;
        return SQLITE_EMPTY;
    } else {
        if cur.e_state >= CURSOR_REQUIRESEEK {
            if cur.e_state == CURSOR_FAULT {
                return cur.skip_next;
            }
            btree_clear_cursor(cur);
        }
        match get_and_init_page(bt, cur.pgno_root, cur.cur_pager_flags as i32) {
            Ok(pg) => cur.p_page = Some(pg),
            Err(e) => {
                cur.p_page = None;
                cur.e_state = CURSOR_INVALID;
                return e;
            }
        }
        cur.i_page = 0;
        cur.cur_int_key = bt.pager.page_extra(cur_page(cur)).int_key as u8;
    }
    let p_root = cur_page(cur);

    if !skip_init {
        // Se pKeyInfo não é NULL, quem abriu esperava um índice; senão, uma tabela. Se não
        // bate, é corrupção. Isto vale mesmo com a raiz já carregada (iPage>=0): num banco
        // corrompido a raiz pode estar ligada a uma segunda árvore (ou à lista livre).
        let root = bt.pager.page_extra(p_root);
        if !root.is_init || cur.p_key_info.is_none() != root.int_key {
            return SQLITE_CORRUPT_BKPT;
        }
    }

    // skip_init:
    cur.ix = 0;
    cur.info.n_size = 0;
    cur.cur_flags &= !(BTCF_AT_LAST | BTCF_VALID_NKEY | BTCF_VALID_OVFL);

    let (n_cell, leaf, pgno, hdr) = {
        let root = bt.pager.page_extra(p_root);
        (root.n_cell, root.leaf, root.pgno, root.hdr_offset as usize)
    };
    if n_cell > 0 {
        cur.e_state = CURSOR_VALID;
    } else if !leaf {
        if pgno != 1 {
            return SQLITE_CORRUPT_BKPT;
        }
        let subpage = rd4(bt.pager.page_data(p_root), hdr + 8);
        cur.e_state = CURSOR_VALID;
        rc = move_to_child(cur, bt, subpage);
    } else {
        cur.e_state = CURSOR_INVALID;
        rc = SQLITE_EMPTY;
    }
    rc
}

/// `moveToLeftmost`: desce até a folha mais à esquerda abaixo da entrada corrente.
pub(crate) fn move_to_leftmost(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    let mut rc = SQLITE_OK;
    while rc == SQLITE_OK {
        let pg = cur_page(cur);
        let pgno = {
            let (page, data) = bt.pager.page_parts(pg);
            if page.leaf {
                break;
            }
            rd4(data, find_cell(page, data, cur.ix as i32))
        };
        rc = move_to_child(cur, bt, pgno);
    }
    rc
}

/// `moveToRightmost`: desce até a entrada mais à direita abaixo da página corrente.
pub(crate) fn move_to_rightmost(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    loop {
        let pg = cur_page(cur);
        let page = bt.pager.page_extra(pg);
        if page.leaf {
            cur.ix = page.n_cell.wrapping_sub(1);
            return SQLITE_OK;
        }
        let hdr = page.hdr_offset as usize;
        let n_cell = page.n_cell;
        let pgno = rd4(bt.pager.page_data(pg), hdr + 8);
        cur.ix = n_cell;
        let rc = move_to_child(cur, bt, pgno);
        if rc != SQLITE_OK {
            return rc;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 014: First, Last, Moveto
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeFirst`: move o cursor para a primeira entrada. `*res` fica 0 se o cursor
/// aponta para algo, 1 se a árvore está vazia.
pub(crate) fn btree_first(cur: &mut BtCursor, bt: &mut BtShared, res: &mut i32) -> i32 {
    let mut rc = move_to_root(cur, bt);
    if rc == SQLITE_OK {
        *res = 0;
        rc = move_to_leftmost(cur, bt);
    } else if rc == SQLITE_EMPTY {
        *res = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// `btreeLast`: move o cursor para a última entrada.
fn btree_last_inner(cur: &mut BtCursor, bt: &mut BtShared, res: &mut i32) -> i32 {
    let mut rc = move_to_root(cur, bt);
    if rc == SQLITE_OK {
        *res = 0;
        rc = move_to_rightmost(cur, bt);
        if rc == SQLITE_OK {
            cur.cur_flags |= BTCF_AT_LAST;
        } else {
            cur.cur_flags &= !BTCF_AT_LAST;
        }
    } else if rc == SQLITE_EMPTY {
        *res = 1;
        rc = SQLITE_OK;
    }
    rc
}

/// `sqlite3BtreeLast`: move o cursor para a última entrada (sem efeito se já está nela).
pub(crate) fn btree_last(cur: &mut BtCursor, bt: &mut BtShared, res: &mut i32) -> i32 {
    // Se o cursor já aponta para a última entrada, nada a fazer.
    if cur.e_state == CURSOR_VALID && (cur.cur_flags & BTCF_AT_LAST) != 0 {
        *res = 0;
        return SQLITE_OK;
    }
    btree_last_inner(cur, bt, res)
}

/// `sqlite3BtreeTableMoveto`: move o cursor para perto da chave `int_key` numa tabela. Se não
/// há igual, o cursor fica numa folha que conteria a entrada. `*res` < 0: o cursor está numa
/// entrada menor que `int_key` (ou a tabela está vazia); 0: igual; > 0: maior.
pub(crate) fn btree_table_moveto(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    int_key: i64,
    bias_right: i32,
    res: &mut i32,
) -> i32 {
    let mut rc;

    // Se o cursor já está onde queremos chegar, volta sem trabalho.
    if cur.e_state == CURSOR_VALID && (cur.cur_flags & BTCF_VALID_NKEY) != 0 {
        if cur.info.n_key == int_key {
            *res = 0;
            return SQLITE_OK;
        }
        if cur.info.n_key < int_key {
            if (cur.cur_flags & BTCF_AT_LAST) != 0 {
                *res = -1;
                return SQLITE_OK;
            }
            // Se a chave pedida é a seguinte à anterior, tenta chegar com Next() em vez de
            // uma busca binária completa. É só otimização; a resposta seria a mesma.
            if cur.info.n_key.wrapping_add(1) == int_key {
                *res = 0;
                rc = btree_next(cur, bt, 0);
                if rc == SQLITE_OK {
                    get_cell_info(cur, bt);
                    if cur.info.n_key == int_key {
                        return SQLITE_OK;
                    }
                } else if rc != SQLITE_DONE {
                    return rc;
                }
            }
        }
    }

    rc = move_to_root(cur, bt);
    if rc != SQLITE_OK {
        if rc == SQLITE_EMPTY {
            *res = -1;
            return SQLITE_OK;
        }
        return rc;
    }

    loop {
        let pg = cur_page(cur);
        let (n_cell, int_key_leaf, leaf, hdr, a_data_end) = {
            let page = bt.pager.page_extra(pg);
            (page.n_cell as i32, page.int_key_leaf, page.leaf, page.hdr_offset as usize, page.a_data_end)
        };

        // n_cell é maior que zero: se esta é a raiz o cursor teria ficado INVALID; senão
        // moveToChild() já detectou a corrupção.
        let mut lwr: i32 = 0;
        let mut upr: i32 = n_cell - 1;
        let mut idx: i32 = upr >> (1 - bias_right); // bias_right ? upr : (lwr+upr)/2
        let mut c: i32 = 0;
        let mut descend = false;
        loop {
            let (page, data) = bt.pager.page_parts(pg);
            let mut cell = find_cell_past_ptr(page, data, idx);
            if int_key_leaf {
                loop {
                    let b = at(data, cell);
                    cell += 1;
                    if b < 0x80 {
                        break;
                    }
                    if cell >= a_data_end {
                        return SQLITE_CORRUPT_BKPT;
                    }
                }
            }
            let n_cell_key = get_varint(data.get(cell..).unwrap_or(&[])).1 as i64;
            if n_cell_key < int_key {
                lwr = idx + 1;
                if lwr > upr {
                    c = -1;
                    break;
                }
            } else if n_cell_key > int_key {
                upr = idx - 1;
                if lwr > upr {
                    c = 1;
                    break;
                }
            } else {
                cur.ix = idx as u16;
                if !leaf {
                    lwr = idx;
                    descend = true;
                    break;
                } else {
                    cur.cur_flags |= BTCF_VALID_NKEY;
                    cur.info.n_key = n_cell_key;
                    cur.info.n_size = 0;
                    *res = 0;
                    return SQLITE_OK;
                }
            }
            idx = (lwr + upr) >> 1; // (lwr+upr)/2
        }
        if !descend && leaf {
            cur.ix = idx as u16;
            *res = c;
            rc = SQLITE_OK;
            break;
        }

        // moveto_table_next_layer:
        let chld_pg = {
            let (page, data) = bt.pager.page_parts(pg);
            if lwr >= n_cell {
                rd4(data, hdr + 8)
            } else {
                rd4(data, find_cell(page, data, lwr))
            }
        };
        cur.ix = lwr as u16;
        rc = move_to_child(cur, bt, chld_pg);
        if rc != SQLITE_OK {
            break;
        }
    }

    // moveto_table_finish:
    cur.info.n_size = 0;
    rc
}

/// `indexCellCompare`: compara a `idx`-ésima célula da página com `key` usando `x_compare`.
/// Devolve `Some(c)` com `c` negativo ou zero se a célula é menor ou igual à chave; `None`
/// quando a célula tem overflow (o `99` do C: "nada se sabe", sempre seguro devolver).
fn index_cell_compare(
    page: &MemPage,
    data: &[u8],
    idx: i32,
    key: &mut UnpackedRecord,
    x_compare: RecordCompare,
) -> Option<i32> {
    let cell = find_cell_past_ptr(page, data, idx);
    let n_cell = at(data, cell) as i32;
    if n_cell <= page.max1byte_payload as i32 {
        // O tamanho do registro é um varint de um byte e o registro cabe na página.
        Some(x_compare(key_slice(data, cell + 1, n_cell as usize), key))
    } else if (at(data, cell + 1) & 0x80) == 0 {
        let n = ((n_cell & 0x7f) << 7) + at(data, cell + 1) as i32;
        if n <= page.max_local as i32 {
            // O tamanho é um varint de dois bytes e o registro cabe na página.
            Some(x_compare(key_slice(data, cell + 2, n as usize), key))
        } else {
            None
        }
    } else {
        None
    }
}

/// `cursorOnLastPage`: verdadeiro se o cursor está na última página da tabela.
fn cursor_on_last_page(cur: &BtCursor, bt: &BtShared) -> bool {
    for i in 0..cur.i_page.max(0) as usize {
        if let Some(pg) = cur.ap_page[i] {
            if cur.ai_idx[i] < bt.pager.page_extra(pg).n_cell {
                return false;
            }
        }
    }
    true
}

/// `sqlite3BtreeIndexMoveto`: move o cursor para perto da chave `p_idx_key` num índice. Mesmos
/// significados de `*res` que `btree_table_moveto`. `p_idx_key.eq_seen` fica 1 se existe uma
/// entrada igual à chave.
pub(crate) fn btree_index_moveto(
    cur: &mut BtCursor,
    bt: &mut BtShared,
    p_idx_key: &mut UnpackedRecord,
    res: &mut i32,
) -> i32 {
    let mut rc;
    let x_record_compare = find_compare(p_idx_key);
    p_idx_key.err_code = 0;
    let mut bypass_moveto_root = false;

    // Pode-se pular muito trabalho em dois casos: (1) o cursor já aponta para a última
    // célula da tabela e a chave é maior ou igual a ela; (2) o cursor está na última página
    // e a primeira célula dela é menor ou igual à chave: a busca começa na página corrente.
    if cur.e_state == CURSOR_VALID && cursor_on_last_page(cur, bt) {
        let pg = cur_page(cur);
        let (leaf, n_cell, is_init) = {
            let page = bt.pager.page_extra(pg);
            (page.leaf, page.n_cell, page.is_init)
        };
        if leaf {
            if cur.ix as i32 == n_cell as i32 - 1 {
                let c = {
                    let (page, data) = bt.pager.page_parts(pg);
                    index_cell_compare(page, data, cur.ix as i32, p_idx_key, x_record_compare)
                };
                if let Some(c) = c {
                    if c <= 0 && p_idx_key.err_code == 0 {
                        *res = c;
                        return SQLITE_OK; // o cursor já aponta para o lugar certo
                    }
                }
            }
            if cur.i_page > 0 {
                let c = {
                    let (page, data) = bt.pager.page_parts(pg);
                    index_cell_compare(page, data, 0, p_idx_key, x_record_compare)
                };
                if c.is_some_and(|c| c <= 0) && p_idx_key.err_code == 0 {
                    cur.cur_flags &= !BTCF_VALID_OVFL;
                    if !is_init {
                        return SQLITE_CORRUPT_BKPT;
                    }
                    bypass_moveto_root = true; // começa a busca na página corrente
                }
            }
            if !bypass_moveto_root {
                p_idx_key.err_code = 0;
            }
        }
    }

    if !bypass_moveto_root {
        rc = move_to_root(cur, bt);
        if rc != SQLITE_OK {
            if rc == SQLITE_EMPTY {
                *res = -1;
                return SQLITE_OK;
            }
            return rc;
        }
    }

    // bypass_moveto_root:
    loop {
        let pg = cur_page(cur);
        let (n_cell, leaf, hdr, child_ptr_size) = {
            let page = bt.pager.page_extra(pg);
            (page.n_cell as i32, page.leaf, page.hdr_offset as usize, page.child_ptr_size as usize)
        };

        // n_cell é maior que zero (ver o comentário em btree_table_moveto).
        let mut lwr: i32 = 0;
        let mut upr: i32 = n_cell - 1;
        let mut idx: i32 = upr >> 1; // (lwr+upr)/2
        let mut c: i32;
        loop {
            // O tamanho máximo de página é 65536, então o registro de uma página de índice tem
            // menos de 16384 bytes e o tamanho é um varint de até 2 bytes: tenta comparar sem
            // analisar a célula inteira.
            let quick = {
                let (page, data) = bt.pager.page_parts(pg);
                index_cell_compare(page, data, idx, p_idx_key, x_record_compare)
            };
            c = match quick {
                Some(c) => c,
                None => {
                    // O registro vaza para páginas de overflow: analisa a célula inteira,
                    // junta o registro num buffer com accessPayload() e compara.
                    let n_cell_key = {
                        let (page, data) = bt.pager.page_parts(pg);
                        let cell = find_cell_past_ptr(page, data, idx);
                        x_parse_cell(
                            page,
                            data,
                            cell.saturating_sub(child_ptr_size),
                            &mut cur.info,
                        );
                        cur.info.n_key as i32
                    };
                    if n_cell_key < 2 || (n_cell_key as u32) / bt.usable_size > bt.n_page {
                        rc = SQLITE_CORRUPT_BKPT;
                        cur.info.n_size = 0;
                        return rc;
                    }
                    cur.ix = idx as u16;
                    // Os 18 bytes de folga do C são zeros além do fim; a comparação já lê
                    // zeros depois da fatia.
                    let mut cell_key = vec![0u8; n_cell_key as usize];
                    rc = access_payload(cur, bt, 0, n_cell_key as u32, &mut cell_key, 0);
                    cur.cur_flags &= !BTCF_VALID_OVFL;
                    if rc != SQLITE_OK {
                        cur.info.n_size = 0;
                        return rc;
                    }
                    record_compare(&cell_key, p_idx_key)
                }
            };
            if c < 0 {
                lwr = idx + 1;
            } else if c > 0 {
                upr = idx - 1;
            } else {
                *res = 0;
                rc = SQLITE_OK;
                cur.ix = idx as u16;
                if p_idx_key.err_code != 0 {
                    rc = SQLITE_CORRUPT_BKPT;
                }
                cur.info.n_size = 0;
                return rc;
            }
            if lwr > upr {
                break;
            }
            idx = (lwr + upr) >> 1; // (lwr+upr)/2
        }
        if leaf {
            cur.ix = idx as u16;
            *res = c;
            rc = SQLITE_OK;
            break;
        }
        let chld_pg = {
            let (page, data) = bt.pager.page_parts(pg);
            if lwr >= n_cell {
                rd4(data, hdr + 8)
            } else {
                rd4(data, find_cell(page, data, lwr))
            }
        };

        // Equivale ao moveToChild() embutido do C: ix = lwr; rc = moveToChild(pCur, chldPg).
        cur.ix = lwr as u16;
        rc = move_to_child(cur, bt, chld_pg);
        if rc != SQLITE_OK {
            break;
        }
    }

    // moveto_index_finish:
    cur.info.n_size = 0;
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 015: Eof, RowCountEst, Next, Previous, allocateBtreePage
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeEof`: verdadeiro se o cursor não aponta para uma entrada (passou do fim ou do
/// começo, ou a tabela está vazia).
pub(crate) fn btree_eof(cur: &BtCursor) -> bool {
    cur.e_state != CURSOR_VALID
}

/// `sqlite3BtreeRowCountEst`: estimativa do número de linhas da tabela; negativo se não há.
pub(crate) fn btree_row_count_est(cur: &BtCursor, bt: &BtShared) -> i64 {
    if cur.e_state != CURSOR_VALID {
        return 0;
    }
    let page = bt.pager.page_extra(cur_page(cur));
    if !page.leaf {
        return -1;
    }
    let mut n = page.n_cell as i64;
    for i in 0..cur.i_page.max(0) as usize {
        if let Some(pg) = cur.ap_page[i] {
            n = n.wrapping_mul(bt.pager.page_extra(pg).n_cell as i64);
        }
    }
    n
}

/// `btreeNext`: caminho lento de `btree_next` (muda de página ou restaura o cursor).
fn btree_next_inner(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    if cur.e_state != CURSOR_VALID {
        let rc = restore_cursor_position(cur, bt);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID == cur.e_state {
            return SQLITE_DONE;
        }
        if cur.e_state == CURSOR_SKIPNEXT {
            cur.e_state = CURSOR_VALID;
            if cur.skip_next > 0 {
                return SQLITE_OK;
            }
        }
    }

    let mut pg = cur_page(cur);
    cur.ix = cur.ix.wrapping_add(1);
    let idx = cur.ix;
    if !bt.pager.page_extra(pg).is_init {
        return SQLITE_CORRUPT_BKPT;
    }

    if idx >= bt.pager.page_extra(pg).n_cell {
        if !bt.pager.page_extra(pg).leaf {
            let hdr = bt.pager.page_extra(pg).hdr_offset as usize;
            let child = rd4(bt.pager.page_data(pg), hdr + 8);
            let rc = move_to_child(cur, bt, child);
            if rc != SQLITE_OK {
                return rc;
            }
            return move_to_leftmost(cur, bt);
        }
        loop {
            if cur.i_page == 0 {
                cur.e_state = CURSOR_INVALID;
                return SQLITE_DONE;
            }
            move_to_parent(cur, bt);
            pg = cur_page(cur);
            if cur.ix < bt.pager.page_extra(pg).n_cell {
                break;
            }
        }
        if bt.pager.page_extra(pg).int_key {
            return btree_next(cur, bt, 0);
        } else {
            return SQLITE_OK;
        }
    }
    if bt.pager.page_extra(pg).leaf {
        SQLITE_OK
    } else {
        move_to_leftmost(cur, bt)
    }
}

/// `sqlite3BtreeNext`: avança o cursor para a próxima entrada. `SQLITE_DONE` se já está na
/// última. O bit 0 de `flags` é só dica (o SQLite nativo a ignora).
pub(crate) fn btree_next(cur: &mut BtCursor, bt: &mut BtShared, _flags: i32) -> i32 {
    cur.info.n_size = 0;
    cur.cur_flags &= !(BTCF_VALID_NKEY | BTCF_VALID_OVFL);
    if cur.e_state != CURSOR_VALID {
        return btree_next_inner(cur, bt);
    }
    let pg = cur_page(cur);
    cur.ix = cur.ix.wrapping_add(1);
    if cur.ix >= bt.pager.page_extra(pg).n_cell {
        cur.ix = cur.ix.wrapping_sub(1);
        return btree_next_inner(cur, bt);
    }
    if bt.pager.page_extra(pg).leaf {
        SQLITE_OK
    } else {
        move_to_leftmost(cur, bt)
    }
}

/// `btreePrevious`: caminho lento de `btree_previous`.
fn btree_previous_inner(cur: &mut BtCursor, bt: &mut BtShared) -> i32 {
    if cur.e_state != CURSOR_VALID {
        let rc = restore_cursor_position(cur, bt);
        if rc != SQLITE_OK {
            return rc;
        }
        if CURSOR_INVALID == cur.e_state {
            return SQLITE_DONE;
        }
        if CURSOR_SKIPNEXT == cur.e_state {
            cur.e_state = CURSOR_VALID;
            if cur.skip_next < 0 {
                return SQLITE_OK;
            }
        }
    }

    let pg = cur_page(cur);
    if !bt.pager.page_extra(pg).is_init {
        return SQLITE_CORRUPT_BKPT;
    }
    let rc;
    if !bt.pager.page_extra(pg).leaf {
        let idx = cur.ix as i32;
        let child = {
            let (page, data) = bt.pager.page_parts(pg);
            rd4(data, find_cell(page, data, idx))
        };
        let r = move_to_child(cur, bt, child);
        if r != SQLITE_OK {
            return r;
        }
        rc = move_to_rightmost(cur, bt);
    } else {
        while cur.ix == 0 {
            if cur.i_page == 0 {
                cur.e_state = CURSOR_INVALID;
                return SQLITE_DONE;
            }
            move_to_parent(cur, bt);
        }
        cur.ix -= 1;
        let page = bt.pager.page_extra(cur_page(cur));
        if page.int_key && !page.leaf {
            rc = btree_previous(cur, bt, 0);
        } else {
            rc = SQLITE_OK;
        }
    }
    rc
}

/// `sqlite3BtreePrevious`: recua o cursor para a entrada anterior. `SQLITE_DONE` se já está
/// na primeira.
pub(crate) fn btree_previous(cur: &mut BtCursor, bt: &mut BtShared, _flags: i32) -> i32 {
    cur.cur_flags &= !(BTCF_AT_LAST | BTCF_VALID_OVFL | BTCF_VALID_NKEY);
    cur.info.n_size = 0;
    if cur.e_state != CURSOR_VALID
        || cur.ix == 0
        || !bt.pager.page_extra(cur_page(cur)).leaf
    {
        return btree_previous_inner(cur, bt);
    }
    cur.ix -= 1;
    SQLITE_OK
}

/// Copia 4 bytes de `src[src_off..]` para `dst[dst_off..]` (páginas possivelmente distintas).
fn copy4(bt: &mut BtShared, src: PgId, src_off: usize, dst: PgId, dst_off: usize) {
    let mut b = [0u8; 4];
    for (i, x) in b.iter_mut().enumerate() {
        *x = at(bt.pager.page_data(src), src_off + i);
    }
    bt.pager.page_data_mut(dst)[dst_off..dst_off + 4].copy_from_slice(&b);
}

/// `allocateBtreePage`: aloca uma página nova do arquivo (da lista livre ou do fim). A página
/// já foi tornada gravável (`sqlite3PagerWrite`) e tem uma referência que quem chama deve
/// soltar. O número da página é `bt.pager.page_extra(pg).pgno`. Com `nearby != 0` tenta achar
/// uma página perto dela; `BTALLOC_EXACT` garante a página `nearby` se estiver na lista livre;
/// `BTALLOC_LE` devolve uma `<= nearby` se houver; `BTALLOC_ANY` não restringe.
pub(crate) fn allocate_btree_page(
    bt: &mut BtShared,
    nearby: u32,
    e_mode: u8,
) -> Result<PgId, i32> {
    let pg1 = page1(bt);
    let mx_page = bt.n_page;
    // O inteiro de 4 bytes big-endian no offset 36 é o total de páginas da lista livre.
    let n = rd4(bt.pager.page_data(pg1), 36);
    if n >= mx_page {
        return Err(SQLITE_CORRUPT_BKPT);
    }
    let mut rc = SQLITE_OK;
    let mut p_trunk: Option<PgId> = None;
    let mut p_prev_trunk: Option<PgId> = None;
    let mut out_page: Option<PgId> = None;
    let mut out_pgno: u32 = 0;

    if n > 0 {
        // Há páginas na lista livre: reaproveita uma.
        let mut search_list = false; // se a lista livre deve ser pesquisada por `nearby`
        let mut n_search: u32 = 0; // tentativas de busca

        // Com BTALLOC_EXACT e o mapa de ponteiros dizendo que `nearby` está na lista livre,
        // a lista inteira é pesquisada atrás dela.
        if e_mode == BTALLOC_EXACT {
            if nearby <= mx_page {
                let mut e_type: u8 = 0;
                rc = ptrmap_get(bt, nearby, &mut e_type, None);
                if rc != SQLITE_OK {
                    return Err(rc);
                }
                if e_type == PTRMAP_FREEPAGE {
                    search_list = true;
                }
            }
        } else if e_mode == BTALLOC_LE {
            search_list = true;
        }

        // Decrementa o contador da lista livre em 1.
        rc = bt.pager.write(pg1);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        put4byte(&mut bt.pager.page_data_mut(pg1)[36..], n - 1);

        // O corpo roda uma vez se `search_list` é falso; senão, uma vez por página tronco da
        // lista livre até achar `nearby` (EXACT) ou uma página menor (LE).
        'alloc: loop {
            p_prev_trunk = p_trunk;
            let i_trunk = match p_prev_trunk {
                // O primeiro inteiro de um tronco é o número do próximo tronco (ou zero).
                Some(pt) => rd4(bt.pager.page_data(pt), 0),
                // O inteiro de 4 bytes no offset 32 é a primeira página da lista livre.
                None => rd4(bt.pager.page_data(pg1), 32),
            };
            if i_trunk > mx_page || {
                let over = n_search > n;
                n_search += 1;
                over
            } {
                rc = SQLITE_CORRUPT_BKPT;
            } else {
                match btree_get_unused_page(bt, i_trunk, 0) {
                    Ok(p) => {
                        p_trunk = Some(p);
                        rc = SQLITE_OK;
                    }
                    Err(e) => rc = e,
                }
            }
            if rc != SQLITE_OK {
                p_trunk = None;
                break 'alloc;
            }
            let Some(trunk) = p_trunk else {
                rc = SQLITE_CORRUPT_BKPT;
                break 'alloc;
            };
            // O segundo inteiro de um tronco é o número de ponteiros de folha que seguem.
            let k = rd4(bt.pager.page_data(trunk), 4);
            if k == 0 && !search_list {
                // O tronco não tem folhas e a lista não está sendo pesquisada: extrai o
                // próprio tronco e o usa como a página nova.
                rc = bt.pager.write(trunk);
                if rc != SQLITE_OK {
                    break 'alloc;
                }
                out_pgno = i_trunk;
                copy4(bt, trunk, 0, pg1, 32);
                out_page = Some(trunk);
                p_trunk = None;
            } else if k > bt.usable_size / 4 - 2 {
                // k fora do intervalo: banco corrompido.
                rc = SQLITE_CORRUPT_BKPT;
                break 'alloc;
            } else if search_list
                && (nearby == i_trunk || (i_trunk < nearby && e_mode == BTALLOC_LE))
            {
                // A lista está sendo pesquisada e este tronco é a página a alocar, tenha ou
                // não folhas.
                out_pgno = i_trunk;
                out_page = Some(trunk);
                search_list = false;
                rc = bt.pager.write(trunk);
                if rc != SQLITE_OK {
                    break 'alloc;
                }
                if k == 0 {
                    match p_prev_trunk {
                        None => copy4(bt, trunk, 0, pg1, 32),
                        Some(prev) => {
                            rc = bt.pager.write(prev);
                            if rc != SQLITE_OK {
                                break 'alloc;
                            }
                            copy4(bt, trunk, 0, prev, 0);
                        }
                    }
                } else {
                    // O tronco é pedido por quem chama mas tem ponteiros para folhas da
                    // lista livre: a primeira folha vira o tronco.
                    let i_new_trunk = rd4(bt.pager.page_data(trunk), 8);
                    if i_new_trunk > mx_page {
                        rc = SQLITE_CORRUPT_BKPT;
                        break 'alloc;
                    }
                    let new_trunk = match btree_get_unused_page(bt, i_new_trunk, 0) {
                        Ok(p) => p,
                        Err(e) => {
                            rc = e;
                            break 'alloc;
                        }
                    };
                    rc = bt.pager.write(new_trunk);
                    if rc != SQLITE_OK {
                        release_page(bt, Some(new_trunk));
                        break 'alloc;
                    }
                    copy4(bt, trunk, 0, new_trunk, 0);
                    put4byte(&mut bt.pager.page_data_mut(new_trunk)[4..], k - 1);
                    let tail: Vec<u8> = bt.pager.page_data(trunk)[12..12 + (k as usize - 1) * 4].to_vec();
                    bt.pager.page_data_mut(new_trunk)[8..8 + tail.len()].copy_from_slice(&tail);
                    release_page(bt, Some(new_trunk));
                    match p_prev_trunk {
                        None => put4byte(&mut bt.pager.page_data_mut(pg1)[32..], i_new_trunk),
                        Some(prev) => {
                            rc = bt.pager.write(prev);
                            if rc != SQLITE_OK {
                                break 'alloc;
                            }
                            put4byte(bt.pager.page_data_mut(prev), i_new_trunk);
                        }
                    }
                }
                p_trunk = None;
            } else if k > 0 {
                // Extrai uma folha do tronco.
                let mut closest: u32 = 0;
                if nearby > 0 {
                    let a_data = bt.pager.page_data(trunk);
                    if e_mode == BTALLOC_LE {
                        for i in 0..k {
                            let i_page = rd4(a_data, 8 + i as usize * 4);
                            if i_page <= nearby {
                                closest = i;
                                break;
                            }
                        }
                    } else {
                        let mut dist = abs_int32(rd4(a_data, 8).wrapping_sub(nearby) as i32);
                        for i in 1..k {
                            let d2 = abs_int32(rd4(a_data, 8 + i as usize * 4).wrapping_sub(nearby) as i32);
                            if d2 < dist {
                                closest = i;
                                dist = d2;
                            }
                        }
                    }
                }

                let i_page = rd4(bt.pager.page_data(trunk), 8 + closest as usize * 4);
                if i_page > mx_page || i_page < 2 {
                    rc = SQLITE_CORRUPT_BKPT;
                    break 'alloc;
                }
                if !search_list || (i_page == nearby || (i_page < nearby && e_mode == BTALLOC_LE)) {
                    out_pgno = i_page;
                    rc = bt.pager.write(trunk);
                    if rc != SQLITE_OK {
                        break 'alloc;
                    }
                    {
                        let a_data = bt.pager.page_data_mut(trunk);
                        if closest < k - 1 {
                            let from = 4 + k as usize * 4;
                            a_data.copy_within(from..from + 4, 8 + closest as usize * 4);
                        }
                        put4byte(&mut a_data[4..], k - 1);
                    }
                    let no_content =
                        if !btree_get_has_content(bt, out_pgno) { PAGER_GET_NOCONTENT } else { 0 };
                    match btree_get_unused_page(bt, out_pgno, no_content) {
                        Ok(p) => {
                            out_page = Some(p);
                            rc = bt.pager.write(p);
                            if rc != SQLITE_OK {
                                release_page(bt, Some(p));
                                out_page = None;
                            }
                        }
                        Err(e) => {
                            rc = e;
                            out_page = None;
                        }
                    }
                    search_list = false;
                }
            }
            release_page(bt, p_prev_trunk);
            p_prev_trunk = None;
            if !search_list {
                break;
            }
        }
    } else {
        // Não há páginas na lista livre: acrescenta uma página nova ao fim da imagem do banco.
        //
        // Em geral as páginas novas daqui podem ser pedidas ao pager com o flag sem-conteúdo,
        // que evita ler o conteúdo do disco. Mas se a transação já rodou um ou mais passos de
        // vacuum incremental, a página pode ter conteúdo exigido num rollback; aí o flag não é
        // usado e o pager carrega e grava no journal o conteúdo atual antes de sobrescrever.
        let b_no_content = if bt.do_truncate == 0 { PAGER_GET_NOCONTENT } else { 0 };

        rc = bt.pager.write(pg1);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        bt.n_page += 1;
        if bt.n_page == bt.pending_byte_page() {
            bt.n_page += 1;
        }

        if bt.auto_vacuum != 0 && ptrmap_ispage(bt, bt.n_page) {
            // Se a nova página é de mapa de ponteiros, aloca duas no fim do arquivo: a
            // primeira vira o novo mapa de ponteiros e a segunda é a de quem chama.
            let ptrmap_pgno = bt.n_page;
            match btree_get_unused_page(bt, ptrmap_pgno, b_no_content) {
                Ok(pp) => {
                    rc = bt.pager.write(pp);
                    release_page(bt, Some(pp));
                }
                Err(e) => rc = e,
            }
            if rc != SQLITE_OK {
                return Err(rc);
            }
            bt.n_page += 1;
            if bt.n_page == bt.pending_byte_page() {
                bt.n_page += 1;
            }
        }
        let n_page = bt.n_page;
        put4byte(&mut bt.pager.page_data_mut(pg1)[28..], n_page);
        out_pgno = n_page;

        match btree_get_unused_page(bt, out_pgno, b_no_content) {
            Err(e) => return Err(e),
            Ok(p) => {
                out_page = Some(p);
                rc = bt.pager.write(p);
                if rc != SQLITE_OK {
                    release_page(bt, Some(p));
                    out_page = None;
                }
            }
        }
    }

    // end_allocate_page:
    release_page(bt, p_trunk);
    release_page(bt, p_prev_trunk);
    if rc != SQLITE_OK {
        return Err(rc);
    }
    match out_page {
        Some(p) => Ok(p),
        None => Err(SQLITE_CORRUPT_BKPT),
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 016: liberar páginas, limpar overflow, montar célula
// ---------------------------------------------------------------------------------------------

/// `freePage2`: acrescenta a página `i_page` à lista livre do arquivo (supõe que ainda não
/// faz parte dela). `p_mem_page` é opcional; se dado, sua contagem de referências não muda.
pub(crate) fn free_page2(bt: &mut BtShared, p_mem_page: Option<PgId>, i_page: u32) -> i32 {
    let pg1 = page1(bt);
    let mut p_trunk: Option<PgId> = None; // página tronco da lista livre
    let mut i_trunk: u32 = 0; // número da página tronco
    let mut pg: Option<PgId>; // a página liberada (pode ser nula)
    let mut rc;

    if i_page < 2 || i_page > bt.n_page {
        return SQLITE_CORRUPT_BKPT;
    }
    if let Some(mp) = p_mem_page {
        pg = Some(mp);
        bt.pager.pcache.page_ref(mp);
    } else {
        pg = btree_page_lookup(bt, i_page);
    }

    'out: {
        // Incrementa o contador de páginas livres da página 1.
        rc = bt.pager.write(pg1);
        if rc != SQLITE_OK {
            break 'out;
        }
        let n_free = rd4(bt.pager.page_data(pg1), 36);
        put4byte(&mut bt.pager.page_data_mut(pg1)[36..], n_free.wrapping_add(1));

        if (bt.bts_flags & BTS_SECURE_DELETE) != 0 {
            // Com secure_delete, sempre sobrescreve a informação apagada com zeros.
            if pg.is_none() {
                match btree_get_page(bt, i_page, 0) {
                    Ok(p) => pg = Some(p),
                    Err(e) => {
                        rc = e;
                        break 'out;
                    }
                }
            }
            if let Some(p) = pg {
                rc = bt.pager.write(p);
                if rc != SQLITE_OK {
                    break 'out;
                }
                bt.pager.page_data_mut(p).fill(0);
            }
        }

        // Em banco com auto-vacuum, grava no mapa de ponteiros que a página é livre.
        if bt.auto_vacuum != 0 {
            ptrmap_put(bt, i_page, PTRMAP_FREEPAGE, 0, &mut rc);
            if rc != SQLITE_OK {
                break 'out;
            }
        }

        // Agora mexe na lista livre. Se está vazia, ou o primeiro tronco está cheio, esta
        // página vira o novo primeiro tronco; senão vira folha do primeiro tronco. Este
        // bloco testa se é possível acrescentá-la como folha.
        if n_free != 0 {
            i_trunk = rd4(bt.pager.page_data(pg1), 32);
            if i_trunk > bt.n_page {
                rc = SQLITE_CORRUPT_BKPT;
                break 'out;
            }
            let trunk = match btree_get_page(bt, i_trunk, 0) {
                Ok(p) => p,
                Err(e) => {
                    rc = e;
                    break 'out;
                }
            };
            p_trunk = Some(trunk);

            let n_leaf = rd4(bt.pager.page_data(trunk), 4);
            if n_leaf > bt.usable_size / 4 - 2 {
                rc = SQLITE_CORRUPT_BKPT;
                break 'out;
            }
            if n_leaf < bt.usable_size / 4 - 8 {
                // Há espaço no tronco para inserir a página liberada como folha nova. (O
                // tronco só está de fato cheio com usableSize/4 - 2 entradas, mas versões
                // anteriores à 3.6.0 acusam corrupção acima de usableSize/4 - 8; as últimas
                // seis entradas ficam sem uso para manter os arquivos legíveis por elas.)
                rc = bt.pager.write(trunk);
                if rc == SQLITE_OK {
                    let data = bt.pager.page_data_mut(trunk);
                    put4byte(&mut data[4..], n_leaf + 1);
                    put4byte(&mut data[8 + n_leaf as usize * 4..], i_page);
                    if let Some(p) = pg {
                        if (bt.bts_flags & BTS_SECURE_DELETE) == 0 {
                            bt.pager.dont_write(p);
                        }
                    }
                    rc = btree_set_has_content(bt, i_page);
                }
                break 'out;
            }
        }

        // Não deu para acrescentá-la como folha: ela vira o novo primeiro tronco.
        let p = match pg {
            Some(p) => p,
            None => match btree_get_page(bt, i_page, 0) {
                Ok(p) => {
                    pg = Some(p);
                    p
                }
                Err(e) => {
                    rc = e;
                    break 'out;
                }
            },
        };
        rc = bt.pager.write(p);
        if rc != SQLITE_OK {
            break 'out;
        }
        {
            let data = bt.pager.page_data_mut(p);
            put4byte(data, i_trunk);
            put4byte(&mut data[4..], 0);
        }
        put4byte(&mut bt.pager.page_data_mut(pg1)[32..], i_page);
    }

    // freepage_out:
    if let Some(p) = pg {
        bt.pager.page_extra_mut(p).is_init = false;
    }
    release_page(bt, pg);
    release_page(bt, p_trunk);
    rc
}

/// `freePage`: libera a página `pg` se `*rc` é `SQLITE_OK`; o erro vai para `*rc`.
pub(crate) fn free_page(bt: &mut BtShared, pg: PgId, rc: &mut i32) {
    if *rc == SQLITE_OK {
        let pgno = bt.pager.page_extra(pg).pgno;
        *rc = free_page2(bt, Some(pg), pgno);
    }
}

/// `clearCellOverflow`: libera as páginas de overflow da célula que começa em `cell_off` na
/// página `pg`; `info` é a análise dela.
pub(crate) fn clear_cell_overflow(
    bt: &mut BtShared,
    pg: PgId,
    cell_off: usize,
    info: &CellInfo,
) -> i32 {
    let a_data_end = bt.pager.page_extra(pg).a_data_end;
    if cell_off + info.n_size as usize > a_data_end {
        // A célula passa do fim da página.
        return SQLITE_CORRUPT_BKPT;
    }
    let mut ovfl_pgno = rd4(bt.pager.page_data(pg), cell_off + info.n_size as usize - 4);
    let ovfl_page_size = bt.usable_size - 4;
    let n_ovfl_u = info
        .n_payload
        .wrapping_sub(info.n_local as u32)
        .wrapping_add(ovfl_page_size - 1)
        / ovfl_page_size;
    let mut n_ovfl = n_ovfl_u as i32;
    while n_ovfl > 0 {
        n_ovfl -= 1;
        let mut i_next: u32 = 0;
        let mut p_ovfl: Option<PgId> = None;
        if ovfl_pgno < 2 || ovfl_pgno > bt.n_page {
            // 0 não é número de página legal e a página 1 não pode ser de overflow; portanto
            // ovflPgno < 2 ou além do fim do arquivo é corrupção.
            return SQLITE_CORRUPT_BKPT;
        }
        if n_ovfl != 0 {
            match get_overflow_page(bt, ovfl_pgno, true) {
                Err(e) => return e,
                Ok((p, next)) => {
                    p_ovfl = p;
                    i_next = next;
                }
            }
        }

        if p_ovfl.is_none() {
            p_ovfl = btree_page_lookup(bt, ovfl_pgno);
        }
        let rc;
        if p_ovfl.is_some_and(|p| bt.pager.page_ref_count(p) != 1) {
            // Nenhum cursor deveria ter referência a uma página de overflow de uma célula
            // sendo apagada ou atualizada. Se há mais de uma referência, a página não é
            // mesmo de overflow e o banco está corrompido. Detectar isto antes de freePage2()
            // ajuda: em secure_delete ele zeraria a página.
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            rc = free_page2(bt, p_ovfl, ovfl_pgno);
        }

        if let Some(p) = p_ovfl {
            bt.pager.unref(Some(p));
        }
        if rc != SQLITE_OK {
            return rc;
        }
        ovfl_pgno = i_next;
    }
    SQLITE_OK
}

/// Macro `BTREE_CLEAR_CELL`: analisa a célula em `cell_off` da página `pg` (o resultado fica
/// em `info`) e, se tem overflow, libera as páginas dele.
pub(crate) fn btree_clear_cell(
    bt: &mut BtShared,
    pg: PgId,
    cell_off: usize,
    info: &mut CellInfo,
) -> i32 {
    {
        let (page, data) = bt.pager.page_parts(pg);
        x_parse_cell(page, data, cell_off, info);
    }
    if info.n_local as u32 != info.n_payload {
        clear_cell_overflow(bt, pg, cell_off, info)
    } else {
        SQLITE_OK
    }
}

/// O buffer de destino da montagem da célula: o da própria célula (`None`) ou os bytes de
/// uma página de overflow.
fn dest_buf<'a>(bt: &'a mut BtShared, cell: &'a mut [u8], dest: Option<PgId>) -> &'a mut [u8] {
    match dest {
        Some(pg) => bt.pager.page_data_mut(pg),
        None => cell,
    }
}

/// Grava `n` bytes de `src` (ou zeros, se `src` é `None`) em `off` do destino.
fn write_dest(
    bt: &mut BtShared,
    cell: &mut [u8],
    dest: Option<PgId>,
    off: usize,
    src: Option<&[u8]>,
    n: usize,
) {
    let out = &mut dest_buf(bt, cell, dest)[off..off + n];
    match src {
        Some(s) => out.copy_from_slice(&s[..n]),
        None => out.fill(0),
    }
}

/// `fillInCell`: monta em `cell` a sequência de bytes que representa uma célula da página
/// `pg`, alocando e preenchendo as páginas de overflow se preciso. `cell` é um buffer à parte
/// (a célula é montada ali e depois copiada para a página) com espaço suficiente. O tamanho
/// da célula vai em `*size`.
pub(crate) fn fill_in_cell(
    bt: &mut BtShared,
    pg: PgId,
    cell: &mut [u8],
    x: &BtreePayload<'_>,
    size: &mut i32,
) -> i32 {
    let (child_ptr_size, int_key, max_local, min_local) = {
        let page = bt.pager.page_extra(pg);
        (page.child_ptr_size as usize, page.int_key, page.max_local as i32, page.min_local as i32)
    };

    // Preenche o cabeçalho.
    let mut n_header = child_ptr_size;
    let mut n_payload: i32;
    let src: &[u8];
    let mut n_src: i32;
    if int_key {
        n_payload = x.n_data + x.n_zero;
        src = x.p_data.unwrap_or(&[]);
        n_src = x.n_data;
        // Só se chama fillInCell() para folhas.
        n_header += put_varint32(&mut cell[n_header..], n_payload as u32) as usize;
        n_header += put_varint(&mut cell[n_header..], x.n_key as u64) as usize;
    } else {
        n_payload = x.n_key as i32;
        n_src = n_payload;
        src = x.p_key.unwrap_or(&[]);
        n_header += put_varint32(&mut cell[n_header..], n_payload as u32) as usize;
    }

    // Preenche o payload.
    let mut payload_in: Option<PgId> = None; // None: pPayload aponta para dentro de `cell`
    let mut payload_off = n_header;
    if n_payload <= max_local {
        // Caso comum: tudo cabe na página da árvore, sem overflow.
        let mut n = n_header as i32 + n_payload;
        if n < 4 {
            n = 4;
            cell[payload_off + n_payload as usize] = 0;
        }
        *size = n;
        write_dest(bt, cell, None, payload_off, Some(src), n_src as usize);
        write_dest(
            bt,
            cell,
            None,
            payload_off + n_src as usize,
            None,
            (n_payload - n_src) as usize,
        );
        return SQLITE_OK;
    }

    // Parte do conteúdo vai para páginas de overflow.
    let mn = min_local;
    let mut n = mn + (n_payload - mn) % (bt.usable_size - 4) as i32;
    if n > max_local {
        n = mn;
    }
    let mut space_left = n;
    *size = n + n_header as i32 + 4;
    let mut prior_in: Option<PgId> = None; // onde gravar o número da primeira página de overflow
    let mut prior_off = n_header + n as usize;
    let mut to_release: Option<PgId> = None;
    let mut pgno_ovfl: u32 = 0;
    let mut src_pos: usize = 0;

    // Grava o payload na célula local e o excedente em páginas de overflow.
    loop {
        n = n_payload;
        if n > space_left {
            n = space_left;
        }

        if n_src >= n {
            write_dest(bt, cell, payload_in, payload_off, Some(&src[src_pos..]), n as usize);
        } else if n_src > 0 {
            n = n_src;
            write_dest(bt, cell, payload_in, payload_off, Some(&src[src_pos..]), n as usize);
        } else {
            write_dest(bt, cell, payload_in, payload_off, None, n as usize);
        }
        n_payload -= n;
        if n_payload <= 0 {
            break;
        }
        payload_off += n as usize;
        src_pos += n as usize;
        n_src -= n;
        space_left -= n;
        if space_left == 0 {
            let pgno_ptrmap = pgno_ovfl; // entrada do mapa de ponteiros da página de overflow
            if bt.auto_vacuum != 0 {
                loop {
                    pgno_ovfl += 1;
                    if !(ptrmap_ispage(bt, pgno_ovfl) || pgno_ovfl == bt.pending_byte_page()) {
                        break;
                    }
                }
            }
            let mut rc;
            let p_ovfl: Option<PgId>;
            match allocate_btree_page(bt, pgno_ovfl, BTALLOC_ANY) {
                Ok(p) => {
                    p_ovfl = Some(p);
                    pgno_ovfl = bt.pager.page_extra(p).pgno;
                    rc = SQLITE_OK;
                }
                Err(e) => {
                    p_ovfl = None;
                    rc = e;
                }
            }
            // Em banco com auto-vacuum, a segunda página de overflow em diante ganha uma
            // entrada no mapa de ponteiros agora; a primeira ganha uma entrada parcial. Se
            // nada fosse gravado nesse slot, o processamento otimista da cadeia de overflow
            // em clearCell() poderia interpretar valores não inicializados e apagar páginas
            // erradas do banco.
            if bt.auto_vacuum != 0 && rc == SQLITE_OK {
                let e_type = if pgno_ptrmap != 0 { PTRMAP_OVERFLOW2 } else { PTRMAP_OVERFLOW1 };
                ptrmap_put(bt, pgno_ovfl, e_type, pgno_ptrmap, &mut rc);
                if rc != SQLITE_OK {
                    release_page(bt, p_ovfl);
                }
            }
            if rc != SQLITE_OK {
                release_page(bt, to_release);
                return rc;
            }

            put4byte(&mut dest_buf(bt, cell, prior_in)[prior_off..], pgno_ovfl);
            release_page(bt, to_release);
            to_release = p_ovfl;
            prior_in = p_ovfl;
            prior_off = 0;
            put4byte(&mut dest_buf(bt, cell, prior_in)[prior_off..], 0);
            payload_in = p_ovfl;
            payload_off = 4;
            space_left = bt.usable_size as i32 - 4;
        }
    }
    release_page(bt, to_release);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// chunk 017: dropCell, insertCell (CellArray fica em btree_write.rs)
// ---------------------------------------------------------------------------------------------

/// `dropCell`: remove a `idx`-ésima célula da página `pg`. Só mexe nesta página: o conteúdo
/// da célula não é liberado (supõe-se que já foi copiado para outro lugar). `sz` é o tamanho
/// da célula em bytes.
pub(crate) fn drop_cell(bt: &mut BtShared, pg: PgId, idx: i32, sz: i32, rc: &mut i32) {
    if *rc != SQLITE_OK {
        return;
    }
    if idx < 0 || sz < 0 {
        *rc = SQLITE_CORRUPT_BKPT;
        return;
    }
    let idx = idx as usize;
    let usable = bt.usable_size;
    let (ptr, pc, hdr) = {
        let (page, data) = bt.pager.page_parts(pg);
        let ptr = page.a_cell_idx + 2 * idx; // posição da entrada no índice de células
        if ptr + 2 > data.len() {
            *rc = SQLITE_CORRUPT_BKPT;
            return;
        }
        (ptr, get2byte(&data[ptr..]), page.hdr_offset as usize)
    };
    if pc as i64 + sz as i64 > usable as i64 {
        *rc = SQLITE_CORRUPT_BKPT;
        return;
    }
    let rc2 = free_space(bt, pg, pc as u16, sz as u16);
    if rc2 != SQLITE_OK {
        *rc = rc2;
        return;
    }
    let (page, data) = bt.pager.page_parts(pg);
    page.n_cell -= 1;
    if page.n_cell == 0 {
        data[hdr + 1..hdr + 5].fill(0);
        data[hdr + 7] = 0;
        put2byte(&mut data[hdr + 5..], usable);
        page.n_free = usable as i32 - page.hdr_offset as i32 - page.child_ptr_size as i32 - 8;
    } else {
        let n_move = 2 * (page.n_cell as usize).saturating_sub(idx);
        data.copy_within(ptr + 2..ptr + 2 + n_move, ptr);
        put2byte(&mut data[hdr + 3..], page.n_cell as u32);
        page.n_free += 2;
    }
}

/// `insertCell` (e `insertCellFast`, que é esta com `i_child == 0`): a célula nova passa a ser
/// a `i`-ésima da página `pg`; `cell` tem o conteúdo (pelo menos `sz` bytes). Se cabe, vai
/// para a página; senão uma cópia dela entra em `ap_ovfl` (e `n_overflow` aumenta). Com
/// `i_child != 0` (sempre > 0 no `insertCell` do C) os 4 primeiros bytes da cópia são
/// trocados por `i_child`.
pub(crate) fn insert_cell(
    bt: &mut BtShared,
    pg: PgId,
    i: i32,
    cell: &[u8],
    sz: i32,
    i_child: u32,
) -> i32 {
    if i < 0 || sz < 4 || cell.len() < sz as usize {
        return SQLITE_CORRUPT_BKPT;
    }
    let i = i as usize;
    let (n_overflow, n_free) = {
        let page = bt.pager.page_extra(pg);
        (page.n_overflow, page.n_free)
    };
    let sz_u = sz as usize;
    if n_overflow != 0 || sz + 2 > n_free {
        let mut owned = cell[..sz_u].to_vec();
        if i_child != 0 && owned.len() >= 4 {
            put4byte(&mut owned, i_child);
        }
        let page = bt.pager.page_extra_mut(pg);
        let j = page.n_overflow as usize;
        // Compara com ArraySize-1: uma vaga extra fica de reserva (nunca precisa de mais de
        // 3, mas são 4 por segurança).
        if j >= page.ap_ovfl.len() - 1 {
            return SQLITE_CORRUPT_BKPT;
        }
        page.n_overflow += 1;
        page.ap_ovfl[j] = Some(owned);
        page.ai_ovfl[j] = i as u16;
        // Vários overflows só ocorrem ao inserir células divisoras na página pai durante o
        // balance, e elas são adjacentes e ordenadas.
    } else {
        let rc = bt.pager.write(pg);
        if rc != SQLITE_OK {
            return rc;
        }
        let mut idx: i32 = 0; // onde gravar o conteúdo da célula em data[]
        let rc = allocate_space(bt, pg, sz, &mut idx);
        if rc != SQLITE_OK {
            return rc;
        }
        let idx = idx as usize;
        let (page, data) = bt.pager.page_parts(pg);
        page.n_free -= (2 + sz) as u16 as i32;
        let n_cell = page.n_cell as usize;
        let p_ins = page.a_cell_idx + i * 2; // onde, em aCellIdx[], entra a nova entrada
        if idx + sz_u > data.len() || i > n_cell || p_ins + 2 * (n_cell - i) + 2 > data.len() {
            return SQLITE_CORRUPT_BKPT;
        }
        if i_child != 0 {
            // Num banco corrompido onde uma entrada do índice de células vale 3 ou menos, o
            // pCell da página de origem pode apontar até 4 bytes antes do início de aData:
            // por isso os 4 primeiros bytes não são lidos.
            data[idx + 4..idx + sz_u].copy_from_slice(&cell[4..sz_u]);
            put4byte(&mut data[idx..], i_child);
        } else {
            data[idx..idx + sz_u].copy_from_slice(&cell[..sz_u]);
        }
        data.copy_within(p_ins..p_ins + 2 * (n_cell - i), p_ins + 2);
        put2byte(&mut data[p_ins..], idx as u32);
        page.n_cell += 1;
        // Incrementa a contagem de células do cabeçalho.
        let h = page.hdr_offset as usize;
        data[h + 4] = data[h + 4].wrapping_add(1);
        if data[h + 4] == 0 {
            data[h + 3] = data[h + 3].wrapping_add(1);
        }
        if bt.auto_vacuum != 0 {
            // A célula pode conter ponteiro para página de overflow: grava a entrada dela no
            // mapa de ponteiros. A célula está num buffer solto (`src_data_end` nulo).
            let mut rc2 = SQLITE_OK;
            ptrmap_put_ovfl_ptr(bt, pg, None, cell, &mut rc2);
            if rc2 != SQLITE_OK {
                return rc2;
            }
        }
    }
    SQLITE_OK
}

/// `insertCellFast`: `insert_cell` sem substituição do ponteiro de filho (`i_child == 0`).
pub(crate) fn insert_cell_fast(
    bt: &mut BtShared,
    pg: PgId,
    i: i32,
    cell: &[u8],
    sz: i32,
) -> i32 {
    insert_cell(bt, pg, i, cell, sz, 0)
}
