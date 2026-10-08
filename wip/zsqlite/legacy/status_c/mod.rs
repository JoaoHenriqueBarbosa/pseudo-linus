// Mesclado das partes traduzidas de status_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

use std::sync::Mutex;

/// Valor guardado em cada registro de status (SQLITE_PTRSIZE>4, logo 64 bits).
pub type StatValueType = i64;

/// Variáveis em que o status é registrado.
pub struct StatType {
    pub now_value: [StatValueType; 10], // Valor corrente
    pub mx_value: [StatValueType; 10],  // Valor máximo
}

/// O `sqlite3Stat` do C. Os elementos são protegidos pelo mutex do alocador de memória
/// ou pelo mutex do pcache1 (ver `STAT_MUTEX`); o `Mutex` do Rust só garante a
/// segurança de acesso à estática, sem unsafe.
static STAT: Mutex<StatType> = Mutex::new(StatType {
    now_value: [0; 10],
    mx_value: [0; 10],
});

/// Determina qual mutex protege cada elemento de `STAT`: 0 é o mutex do alocador,
/// 1 é o mutex do pcache1.
static STAT_MUTEX: [u8; 10] = [
    0, // SQLITE_STATUS_MEMORY_USED
    1, // SQLITE_STATUS_PAGECACHE_USED
    1, // SQLITE_STATUS_PAGECACHE_OVERFLOW
    0, // SQLITE_STATUS_SCRATCH_USED
    0, // SQLITE_STATUS_SCRATCH_OVERFLOW
    0, // SQLITE_STATUS_MALLOC_SIZE
    0, // SQLITE_STATUS_PARSER_STACK
    1, // SQLITE_STATUS_PAGECACHE_SIZE
    0, // SQLITE_STATUS_SCRATCH_SIZE
    0, // SQLITE_STATUS_MALLOC_COUNT
];

/// Retorna o valor corrente de um parâmetro de status. O chamador precisa estar
/// segurando o mutex apropriado.
pub fn status_value(op: i32) -> i64 {
    assert!(op >= 0 && (op as usize) < 10);
    STAT.lock().unwrap().now_value[op as usize]
}

/// Soma N ao valor de um registro de status. O chamador precisa estar segurando o
/// mutex apropriado. N pode ser positivo ou negativo; a marca d'água é ajustada
/// se necessário.
pub fn status_up(op: i32, n: i32) {
    assert!(op >= 0 && (op as usize) < 10);
    let mut st = STAT.lock().unwrap();
    let op = op as usize;
    st.now_value[op] = st.now_value[op].wrapping_add(n as i64);
    if st.now_value[op] > st.mx_value[op] {
        st.mx_value[op] = st.now_value[op];
    }
}

/// Reduz o valor corrente por N. A marca d'água não muda. N precisa ser não negativo.
pub fn status_down(op: i32, n: i32) {
    assert!(n >= 0);
    assert!(op >= 0 && (op as usize) < 10);
    let mut st = STAT.lock().unwrap();
    let op = op as usize;
    st.now_value[op] = st.now_value[op].wrapping_sub(n as i64);
}

/// Ajusta a marca d'água se necessário. O chamador precisa estar segurando o
/// mutex apropriado.
pub fn status_highwater(op: i32, x: i32) {
    assert!(x >= 0);
    let new_value: StatValueType = x as StatValueType;
    assert!(op >= 0 && (op as usize) < 10);
    assert!(
        op == SQLITE_STATUS_MALLOC_SIZE
            || op == SQLITE_STATUS_PAGECACHE_SIZE
            || op == SQLITE_STATUS_PARSER_STACK
    );
    let mut st = STAT.lock().unwrap();
    if new_value > st.mx_value[op as usize] {
        st.mx_value[op as usize] = new_value;
    }
}

/// Consulta informações de status (sqlite3_status64).
pub fn status64(op: i32, p_current: &mut i64, p_highwater: &mut i64, reset_flag: i32) -> i32 {
    if op < 0 || op >= 10 {
        return SQLITE_MISUSE_BKPT();
    }
    let p_mutex = if STAT_MUTEX[op as usize] != 0 {
        pcache1_mutex()
    } else {
        malloc_mutex()
    };
    mutex_enter(&p_mutex);
    {
        let mut st = STAT.lock().unwrap();
        let op = op as usize;
        *p_current = st.now_value[op];
        *p_highwater = st.mx_value[op];
        if reset_flag != 0 {
            st.mx_value[op] = st.now_value[op];
        }
    }
    mutex_leave(&p_mutex);
    SQLITE_OK
}

/// Versão de 32 bits de `status64` (sqlite3_status).
pub fn status(op: i32, p_current: &mut i32, p_highwater: &mut i32, reset_flag: i32) -> i32 {
    let mut i_cur: i64 = 0;
    let mut i_hwtr: i64 = 0;
    let rc = status64(op, &mut i_cur, &mut i_hwtr, reset_flag);
    if rc == 0 {
        *p_current = i_cur as i32;
        *p_highwater = i_hwtr as i32;
    }
    rc
}

/// Retorna o número de elementos LookasideSlot na lista encadeada. Os slots vivem em
/// `lookaside.slots` e `p_next` é o índice do próximo (ver nota ao integrador).
fn count_lookaside_slots(l: &Lookaside, mut p: Option<usize>) -> u32 {
    let mut cnt: u32 = 0;
    while let Some(i) = p {
        p = l.slots[i].p_next;
        cnt += 1;
    }
    cnt
}

/// Conta o número de slots de lookaside que estão em uso.
pub fn lookaside_used(db: &Sqlite3, p_highwater: Option<&mut i32>) -> i32 {
    let l = &db.lookaside;
    let mut n_init = count_lookaside_slots(l, l.p_init);
    let mut n_free = count_lookaside_slots(l, l.p_free);
    n_init += count_lookaside_slots(l, l.p_small_init);
    n_free += count_lookaside_slots(l, l.p_small_free);
    if let Some(hw) = p_highwater {
        *hw = (l.n_slot as i32).wrapping_sub(n_init as i32);
    }
    (l.n_slot as i32).wrapping_sub(n_init.wrapping_add(n_free) as i32)
}

/// Move a lista `free` inteira para o fim da lista `init`, como o laço do C
/// (`while( p->pNext ) p = p->pNext; p->pNext = init; init = free; free = 0`).
/// Devolve (novo init, novo free).
fn lookaside_requeue(
    slots: &mut [LookasideSlot],
    init: Option<usize>,
    free: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    match free {
        Some(mut p) => {
            while let Some(n) = slots[p].p_next {
                p = n;
            }
            slots[p].p_next = init;
            (free, None)
        }
        None => (init, free),
    }
}

/// Consulta informações de status de uma única conexão (sqlite3_db_status).
pub fn db_status(
    db: &mut Sqlite3,
    op: i32,
    p_current: &mut i32,
    p_highwater: &mut i32,
    reset_flag: i32,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut op = op;
    let mutex = db.mutex.clone();
    mutex_enter(&mutex);
    // O `match` do C com fall through de CACHE_SPILL vira esta seleção prévia.
    let mut in_cache_group = false;
    match op {
        SQLITE_DBSTATUS_LOOKASIDE_USED => {
            *p_current = lookaside_used(db, Some(p_highwater));
            if reset_flag != 0 {
                let (init, free) = lookaside_requeue(
                    &mut db.lookaside.slots,
                    db.lookaside.p_init,
                    db.lookaside.p_free,
                );
                db.lookaside.p_init = init;
                db.lookaside.p_free = free;
                let (init, free) = lookaside_requeue(
                    &mut db.lookaside.slots,
                    db.lookaside.p_small_init,
                    db.lookaside.p_small_free,
                );
                db.lookaside.p_small_init = init;
                db.lookaside.p_small_free = free;
            }
        }

        SQLITE_DBSTATUS_LOOKASIDE_HIT
        | SQLITE_DBSTATUS_LOOKASIDE_MISS_SIZE
        | SQLITE_DBSTATUS_LOOKASIDE_MISS_FULL => {
            assert!((op - SQLITE_DBSTATUS_LOOKASIDE_HIT) >= 0);
            assert!((op - SQLITE_DBSTATUS_LOOKASIDE_HIT) < 3);
            *p_current = 0;
            *p_highwater = db.lookaside.an_stat[(op - SQLITE_DBSTATUS_LOOKASIDE_HIT) as usize] as i32;
            if reset_flag != 0 {
                db.lookaside.an_stat[(op - SQLITE_DBSTATUS_LOOKASIDE_HIT) as usize] = 0;
            }
        }

        // Aproximação da memória usada por todos os pagers da conexão. A marca
        // d'água não tem significado e volta zero.
        SQLITE_DBSTATUS_CACHE_USED_SHARED | SQLITE_DBSTATUS_CACHE_USED => {
            let mut total_used: i32 = 0;
            btree_enter_all(db);
            for i in 0..db.n_db {
                if let Some(p_bt) = db.a_db[i as usize].p_bt.clone() {
                    let p_pager = btree_pager(&p_bt.borrow());
                    let mut n_byte = pager_mem_used(&p_pager);
                    if op == SQLITE_DBSTATUS_CACHE_USED_SHARED {
                        n_byte = n_byte / btree_connection_count(&p_bt.borrow());
                    }
                    total_used += n_byte;
                }
            }
            btree_leave_all(db);
            *p_current = total_used;
            *p_highwater = 0;
        }

        // Estimativa exata da memória usada para guardar o esquema de todos os
        // bancos (main, temp e anexados). *pHighwater vira zero.
        SQLITE_DBSTATUS_SCHEMA_USED => {
            let mut n_byte: i32 = 0;
            btree_enter_all(db);
            db.pn_bytes_freed = Some(Rc::new(Cell::new(0)));
            assert!(db.lookaside.p_end == db.lookaside.p_true_end);
            db.lookaside.p_end = db.lookaside.p_start;
            for i in 0..db.n_db {
                let p_schema = db.a_db[i as usize].p_schema.clone();
                if let Some(p_schema) = p_schema {
                    let sch = p_schema.borrow();
                    n_byte += (sqlite3_global_config().m.x_roundup)(HASH_ELEM_SIZE as i32)
                        * ((sch.tbl_hash.count
                            + sch.trig_hash.count
                            + sch.idx_hash.count
                            + sch.fkey_hash.count) as i32);
                    n_byte += hash_msize(&sch.tbl_hash);
                    n_byte += hash_msize(&sch.trig_hash);
                    n_byte += hash_msize(&sch.idx_hash);
                    n_byte += hash_msize(&sch.fkey_hash);

                    let mut p = hash_first(&sch.trig_hash);
                    while let Some(e) = p {
                        delete_trigger(db, hash_data_trigger(&e.borrow()));
                        p = hash_next(&e.borrow());
                    }
                    let mut p = hash_first(&sch.tbl_hash);
                    while let Some(e) = p {
                        delete_table(db, hash_data_table(&e.borrow()));
                        p = hash_next(&e.borrow());
                    }
                }
            }
            // Os bytes liberados por delete_trigger/delete_table acumulam em pn_bytes_freed.
            if let Some(c) = &db.pn_bytes_freed {
                n_byte += c.get();
            }
            db.pn_bytes_freed = None;
            db.lookaside.p_end = db.lookaside.p_true_end;
            btree_leave_all(db);
            *p_highwater = 0;
            *p_current = n_byte;
        }

        // Estimativa da memória usada para guardar todos os statements preparados.
        SQLITE_DBSTATUS_STMT_USED => {
            db.pn_bytes_freed = Some(Rc::new(Cell::new(0)));
            assert!(db.lookaside.p_end == db.lookaside.p_true_end);
            db.lookaside.p_end = db.lookaside.p_start;
            let mut p_vdbe = db.p_vdbe.clone();
            while let Some(v) = p_vdbe {
                // Lê o próximo antes de apagar, como o `pVdbe=pVdbe->pVNext` seguro do modelo.
                let next = v.borrow().p_v_next.clone();
                vdbe_delete(&v);
                p_vdbe = next;
            }
            db.lookaside.p_end = db.lookaside.p_true_end;
            let n_byte = db.pn_bytes_freed.as_ref().map(|c| c.get()).unwrap_or(0);
            db.pn_bytes_freed = None;
            *p_highwater = 0; // IMP: R-64479-57858
            *p_current = n_byte;
        }

        // Total de acertos, erros, escritas ou spills de cache de todos os pagers.
        // CACHE_SPILL cai (fall through) no grupo seguinte com op trocado.
        SQLITE_DBSTATUS_CACHE_SPILL => {
            op = SQLITE_DBSTATUS_CACHE_WRITE + 1;
            in_cache_group = true;
        }
        SQLITE_DBSTATUS_CACHE_HIT | SQLITE_DBSTATUS_CACHE_MISS | SQLITE_DBSTATUS_CACHE_WRITE => {
            in_cache_group = true;
        }

        // *pCurrent não zero se houver FKs adiadas não resolvidas.
        SQLITE_DBSTATUS_DEFERRED_FKS => {
            *p_highwater = 0; // IMP: R-11967-56545
            *p_current = (db.n_deferred_imm_cons > 0 || db.n_deferred_cons > 0) as i32;
        }

        _ => {
            rc = SQLITE_ERROR;
        }
    }

    if in_cache_group {
        let mut n_ret: u64 = 0;
        assert!(SQLITE_DBSTATUS_CACHE_MISS == SQLITE_DBSTATUS_CACHE_HIT + 1);
        assert!(SQLITE_DBSTATUS_CACHE_WRITE == SQLITE_DBSTATUS_CACHE_HIT + 2);
        for i in 0..db.n_db {
            if let Some(p_bt) = db.a_db[i as usize].p_bt.clone() {
                let p_pager = btree_pager(&p_bt.borrow());
                pager_cache_stat(&p_pager, op, reset_flag, &mut n_ret);
            }
        }
        *p_highwater = 0; // IMP: R-42420-56072, R-54100-20147, R-29431-39229
        *p_current = (n_ret as i32) & 0x7fffffff;
    }

    mutex_leave(&mutex);
    rc
}

