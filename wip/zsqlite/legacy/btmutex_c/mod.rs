// Mesclado das partes traduzidas de btmutex_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Compara duas referências fracas opcionais por identidade (o `==` de ponteiros do C).
fn weak_opt_eq<T>(a: &Option<std::rc::Weak<T>>, b: &Option<std::rc::Weak<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => std::rc::Weak::ptr_eq(x, y),
        _ => false,
    }
}

/// Obtém o mutex BtShared associado ao manipulador B-Tree p. Também
/// define BtShared.db para o manipulador de banco de dados associado a p e
/// p->locked para verdadeiro.
fn lock_btree_mutex(p: &mut Btree) {
    debug_assert_eq!(p.locked, 0);
    if let Some(ref p_bt_rc) = p.p_bt {
        let p_bt = p_bt_rc.borrow();
        if let Some(ref mutex) = p_bt.mutex {
            debug_assert_eq!(sqlite3_mutex_notheld(Some(mutex)), 1);
        }
    }
    if let Some(ref db_weak) = p.db {
        if let Some(db_rc) = db_weak.upgrade() {
            let db = db_rc.borrow();
            if let Some(ref mutex) = db.mutex {
                debug_assert_eq!(sqlite3_mutex_held(Some(mutex)), 1);
            }
        }
    }

    if let Some(ref p_bt_rc) = p.p_bt {
        let mut p_bt = p_bt_rc.borrow_mut();
        if let Some(ref mutex) = p_bt.mutex {
            sqlite3_mutex_enter(Some(mutex));
        }
        p_bt.db = p.db.clone();
    }
    p.locked = 1;
}

/// Libera o mutex BtShared associado ao manipulador B-Tree p e
/// limpa o booleano p->locked.
fn unlock_btree_mutex(p: &mut Btree) {
    let p_bt = p.p_bt.clone();
    debug_assert_eq!(p.locked, 1);

    if let Some(ref p_bt_rc) = p_bt {
        let p_bt_borrow = p_bt_rc.borrow();
        if let Some(ref mutex) = p_bt_borrow.mutex {
            debug_assert_eq!(sqlite3_mutex_held(Some(mutex)), 1);
        }
        if let Some(ref db_weak) = p.db {
            if let Some(db_rc) = db_weak.upgrade() {
                let db = db_rc.borrow();
                if let Some(ref mutex) = db.mutex {
                    debug_assert_eq!(sqlite3_mutex_held(Some(mutex)), 1);
                }
            }
        }
        debug_assert!(weak_opt_eq(&p.db, &p_bt_borrow.db));
    }

    if let Some(ref p_bt_rc) = p_bt {
        let p_bt_borrow = p_bt_rc.borrow();
        if let Some(ref mutex) = p_bt_borrow.mutex {
            sqlite3_mutex_leave(Some(mutex));
        }
    }
    p.locked = 0;
}

/// Entra em um mutex no objeto BTree dado.
///
/// Se o objeto não é compartilhável, então nenhum mutex é necessário
/// e essa rotina é uma operação vazia. O mutex subjacente é não-recursivo.
/// Mas mantemos uma contagem de referência em Btree.want_to_lock para que o comportamento
/// desta interface seja recursivo.
///
/// Para evitar deadlocks, múltiplas B-Trees são bloqueadas na mesma ordem
/// por todas as conexões de banco de dados. p->pNext é uma lista de outras
/// B-Trees pertencendo à mesma conexão de banco de dados que a B-Tree p
/// que precisam ser bloqueadas após p. Se não conseguirmos obter um bloqueio em
/// p, então primeiro desbloqueamos todas as outras em p->pNext, depois esperamos
/// que o bloqueio fique disponível em p, então reobstruímos todas as
/// B-Trees subsequentes que desejam um bloqueio.
pub fn btree_enter(p: &mut Btree) {
    // Alguma verificação de sanidade básica no B-Tree. A lista de B-Trees
    // conectadas por pNext e pPrev deve estar em ordem de classificação por
    // valor Btree.pBt. Todos os elementos da lista devem pertencer à
    // mesma conexão. Apenas B-Trees compartilháveis estão na lista.
    if let Some(ref p_next) = p.p_next {
        let p_next_borrow = p_next.borrow();
        if let (Some(ref p_next_bt), Some(ref p_bt)) = (p_next_borrow.p_bt.as_ref(), p.p_bt.as_ref()) {
            let next_addr = p_next_bt.as_ptr() as usize;
            let curr_addr = p_bt.as_ptr() as usize;
            debug_assert!(next_addr > curr_addr);
        }
    }
    if let Some(ref p_prev_weak) = p.p_prev {
        if let Some(p_prev_rc) = p_prev_weak.upgrade() {
            let p_prev_borrow = p_prev_rc.borrow();
            if let (Some(ref p_prev_bt), Some(ref p_bt)) = (p_prev_borrow.p_bt.as_ref(), p.p_bt.as_ref()) {
                let prev_addr = p_prev_bt.as_ptr() as usize;
                let curr_addr = p_bt.as_ptr() as usize;
                debug_assert!(prev_addr < curr_addr);
            }
        }
    }
    if let Some(ref p_next) = p.p_next {
        debug_assert!(weak_opt_eq(&p_next.borrow().db, &p.db));
    }
    if let Some(ref p_prev_weak) = p.p_prev {
        if let Some(p_prev_rc) = p_prev_weak.upgrade() {
            debug_assert!(weak_opt_eq(&p_prev_rc.borrow().db, &p.db));
        }
    }
    debug_assert!(p.sharable != 0 || (p.p_next.is_none() && p.p_prev.is_none()));

    // Verificar consistência de bloqueio
    debug_assert!(!((p.locked != 0) && p.want_to_lock <= 0));
    debug_assert!(p.sharable != 0 || p.want_to_lock == 0);

    // Já devemos manter um bloqueio na conexão de banco de dados
    if let Some(ref db_weak) = p.db {
        if let Some(db_rc) = db_weak.upgrade() {
            let db = db_rc.borrow();
            if let Some(ref mutex) = db.mutex {
                debug_assert_eq!(sqlite3_mutex_held(Some(mutex)), 1);
            }
        }
    }

    // A menos que o banco de dados seja compartilhável e desbloqueado, então BtShared.db
    // já deve estar configurado corretamente.
    debug_assert!((p.locked == 0 && p.sharable != 0) || {
        if let Some(ref p_bt) = p.p_bt {
            weak_opt_eq(&p_bt.borrow().db, &p.db)
        } else {
            false
        }
    });

    if p.sharable == 0 {
        return;
    }
    p.want_to_lock += 1;
    if p.locked != 0 {
        return;
    }
    btree_lock_carefully(p);
}

/// Esta é uma função auxiliar para sqlite3BtreeLock(). Ao mover
/// lógica complexa, mas raramente usada, fora de sqlite3BtreeLock() e
/// para esta rotina, evitamos mudanças desnecessárias de ponteiro de pilha
/// e assim ajudamos a rotina sqlite3BtreeLock() a funcionar muito mais rapidamente
/// no caso comum.
fn btree_lock_carefully(p: &mut Btree) {
    // Na maioria dos casos, devemos ser capazes de adquirir o bloqueio que
    // queremos sem ter que passar pelo procedimento de bloqueio crescente
    // que se segue. Apenas certifique-se de não bloquear.
    // Mutex ausente equivale ao mutex nulo do C: sqlite3_mutex_try devolve SQLITE_OK.
    let try_ok = match p.p_bt {
        Some(ref p_bt_rc) => {
            let p_bt_borrow = p_bt_rc.borrow();
            match p_bt_borrow.mutex {
                Some(ref mutex) => sqlite3_mutex_try(Some(mutex)) == SQLITE_OK,
                None => true,
            }
        }
        None => false,
    };
    if try_ok {
        if let Some(ref p_bt_rc) = p.p_bt {
            p_bt_rc.borrow_mut().db = p.db.clone();
        }
        p.locked = 1;
        return;
    }

    // Para evitar deadlock, primeiro libera todos os bloqueios com um endereço
    // BtShared maior. Depois adquire nosso bloqueio. Depois reacquire
    // os outros bloqueios BtShared que costumávamos manter em ordem crescente.
    let mut p_later = p.p_next.clone();
    while let Some(p_later_rc) = p_later {
        {
            let p_later_borrow = p_later_rc.borrow();
            debug_assert_eq!(p_later_borrow.sharable, 1);
            if let Some(ref p_next_next) = p_later_borrow.p_next {
                if let (Some(ref p_later_bt), Some(ref p_next_next_bt)) =
                    (p_later_borrow.p_bt.as_ref(), p_next_next.borrow().p_bt.as_ref())
                {
                    let next_addr = p_next_next_bt.as_ptr() as usize;
                    let later_addr = p_later_bt.as_ptr() as usize;
                    debug_assert!(next_addr > later_addr);
                }
            }
            debug_assert!(!((p_later_borrow.locked != 0) && p_later_borrow.want_to_lock <= 0));
        }
        if p_later_rc.borrow().locked != 0 {
            unlock_btree_mutex(&mut p_later_rc.borrow_mut());
        }
        p_later = p_later_rc.borrow().p_next.clone();
    }
    lock_btree_mutex(p);
    p_later = p.p_next.clone();
    while let Some(p_later_rc) = p_later {
        if p_later_rc.borrow().want_to_lock != 0 {
            lock_btree_mutex(&mut p_later_rc.borrow_mut());
        }
        p_later = p_later_rc.borrow().p_next.clone();
    }
}

/// Sai do mutex recursivo em uma B-Tree.
pub fn btree_leave(p: &mut Btree) {
    if let Some(ref db_weak) = p.db {
        if let Some(db_rc) = db_weak.upgrade() {
            let db = db_rc.borrow();
            if let Some(ref mutex) = db.mutex {
                debug_assert_eq!(sqlite3_mutex_held(Some(mutex)), 1);
            }
        }
    }
    if p.sharable != 0 {
        debug_assert!(p.want_to_lock > 0);
        p.want_to_lock -= 1;
        if p.want_to_lock == 0 {
            unlock_btree_mutex(p);
        }
    }
}

#[cfg(debug_assertions)]
/// Retorna verdadeiro se o mutex BtShared é mantido no b-tree, ou se a
/// B-Tree não está marcada como compartilhável.
///
/// Esta rotina é usada apenas de dentro de instruções assert().
pub fn btree_holds_mutex(p: &Btree) -> i32 {
    debug_assert!(p.sharable == 0 || p.locked == 0 || p.want_to_lock > 0);
    debug_assert!(p.sharable == 0 || p.locked == 0 || {
        if let Some(ref p_bt) = p.p_bt {
            weak_opt_eq(&p.db, &p_bt.borrow().db)
        } else {
            false
        }
    });
    debug_assert!(p.sharable == 0 || p.locked == 0 || {
        if let Some(ref p_bt) = p.p_bt {
            if let Some(ref mutex) = p_bt.borrow().mutex {
                sqlite3_mutex_held(Some(mutex)) != 0
            } else {
                false
            }
        } else {
            false
        }
    });
    debug_assert!(p.sharable == 0 || p.locked == 0 || {
        if let Some(ref db_weak) = p.db {
            if let Some(db_rc) = db_weak.upgrade() {
                if let Some(ref mutex) = db_rc.borrow().mutex {
                    sqlite3_mutex_held(Some(mutex)) != 0
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    });

    if p.sharable == 0 || p.locked != 0 { 1 } else { 0 }
}

/// Entra no mutex em cada B-Tree associada com uma conexão de banco de dados.
/// Isto é necessário (por exemplo) antes de analisar uma instrução
/// já que compararemos nomes de tabela e coluna contra todos os esquemas
/// e não queremos que esses esquemas sejam redefinidos debaixo de nós.
///
/// Existe um procedimento de saída correspondente.
///
/// Entra nos mutexes em ordem crescente pelo endereço do ponteiro BtShared
/// para evitar a possibilidade de deadlock quando duas threads com
/// duas ou mais b-trees em comum tentam bloquear todas as suas b-trees
/// no mesmo instante.
fn btree_enter_all_impl(db: &mut Sqlite3) {
    let mut skip_ok = 1;
    debug_assert_eq!(sqlite3_mutex_held(db.mutex.as_ref()), 1);
    for i in 0..db.n_db as usize {
        if i < db.a_db.len() {
            if let Some(ref p_bt) = db.a_db[i].p_bt.clone() {
                if p_bt.borrow().sharable != 0 {
                    btree_enter(&mut p_bt.borrow_mut());
                    skip_ok = 0;
                }
            }
        }
    }
    db.no_shared_cache = skip_ok;
}

pub fn btree_enter_all(db: &mut Sqlite3) {
    if db.no_shared_cache == 0 {
        btree_enter_all_impl(db);
    }
}

fn btree_leave_all_impl(db: &mut Sqlite3) {
    debug_assert_eq!(sqlite3_mutex_held(db.mutex.as_ref()), 1);
    for i in 0..db.n_db as usize {
        if i < db.a_db.len() {
            if let Some(ref p_bt) = db.a_db[i].p_bt.clone() {
                btree_leave(&mut p_bt.borrow_mut());
            }
        }
    }
}

pub fn btree_leave_all(db: &mut Sqlite3) {
    if db.no_shared_cache == 0 {
        btree_leave_all_impl(db);
    }
}

#[cfg(debug_assertions)]
/// Retorna verdadeiro se a thread atual mantém o mutex de conexão de banco de dados
/// e todos os mutexes BtShared necessários.
///
/// Esta rotina é usada dentro de instruções assert() apenas.
pub fn btree_holds_all_mutexes(db: &Sqlite3) -> i32 {
    if sqlite3_mutex_held(db.mutex.as_ref()) == 0 {
        return 0;
    }
    for i in 0..db.n_db as usize {
        if i < db.a_db.len() {
            if let Some(ref p_bt) = db.a_db[i].p_bt {
                let p_bt_borrow = p_bt.borrow();
                if p_bt_borrow.sharable != 0
                    && (p_bt_borrow.want_to_lock == 0
                        || {
                            if let Some(ref p_bt_inner) = p_bt_borrow.p_bt {
                                if let Some(ref mutex) = p_bt_inner.borrow().mutex {
                                    sqlite3_mutex_held(Some(mutex)) == 0
                                } else {
                                    true
                                }
                            } else {
                                true
                            }
                        })
                {
                    return 0;
                }
            }
        }
    }
    1
}

#[cfg(debug_assertions)]
/// Retorna verdadeiro se os mutexes corretos são mantidos para acessar a
/// estrutura db->aDb[iDb].pSchema. Os mutexes necessários para o acesso
/// de esquema são:
///
///   (1) O mutex em db
///   (2) se iDb!=1, então o mutex em db->aDb[iDb].pBt.
///
/// Se pSchema não é NULL, então iDb é calculado a partir de pSchema e
/// db usando sqlite3SchemaToIndex().
pub fn schema_mutex_held(db: &Sqlite3, i_db: i32, p_schema: Option<&Schema>) -> i32 {
    if db.p_vfs.is_none() && db.n_db == 0 {
        return 1;
    }
    let i_db_resolved = if let Some(schema) = p_schema {
        schema_to_index(db, schema)
    } else {
        i_db
    };
    debug_assert!(i_db_resolved >= 0 && i_db_resolved < db.n_db);
    if sqlite3_mutex_held(db.mutex.as_ref()) == 0 {
        return 0;
    }
    if i_db_resolved == 1 {
        return 1;
    }
    let p = if (i_db_resolved as usize) < db.a_db.len() {
        db.a_db[i_db_resolved as usize].p_bt.clone()
    } else {
        None
    };
    debug_assert!(p.is_some());
    if let Some(p_bt) = p {
        let p_bt_borrow = p_bt.borrow();
        if p_bt_borrow.sharable == 0 || p_bt_borrow.locked == 1 { 1 } else { 0 }
    } else {
        0
    }
}

/// Entra no mutex de uma B-Tree dado um cursor pertencente a essa B-Tree.
///
/// Estes pontos de entrada são usados apenas por E/S incremental. Enter() é
/// necessário sempre que OMIT_SHARED_CACHE não está definido, seja o build
/// threadsafe ou não. Leave() só é necessário em builds threadsafe.
pub fn btree_enter_cursor(p_cur: &mut BtCursor) {
    if let Some(ref p_btree) = p_cur.p_btree {
        btree_enter(&mut p_btree.borrow_mut());
    }
}

pub fn btree_leave_cursor(p_cur: &mut BtCursor) {
    if let Some(ref p_btree) = p_cur.p_btree {
        btree_leave(&mut p_btree.borrow_mut());
    }
}

