// Mesclado das partes traduzidas de notify_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Arquivo de origem: notify.c (SQLITE_ENABLE_UNLOCK_NOTIFY está ligado no Debian 13).
//
// Notas para o integrador (tech lead):
//  - `sqlite3BlockedList` é a cabeça da lista de conexões bloqueadas, protegida pelo mutex
//    STATIC_MAIN. Como `Sqlite3Ref` é `Rc`, a cabeça vive num `thread_local!` e os elos são
//    `Weak`, o mesmo modelo de `p_next_blocked` em `Sqlite3`.
//  - O Debian compila com NDEBUG: `assertMutexHeld()`, `checkListProperties()` e os `assert()`
//    do arquivo somem.
//  - `mutex_alloc`, `mutex_enter` e `mutex_leave` seguem a assinatura de `main_c`:
//    `mutex_alloc(id) -> Option<Rc<Sqlite3Mutex>>`, `mutex_enter(Option<&Rc<Sqlite3Mutex>>)`.
//  - `xNotify(void **aArg, int nArg)` vira `Fn(&[CallbackArg])`; a identidade do callback (o
//    `xUnlockNotify!=xUnlockNotify` do C compara endereços) é a do `Rc`, via `Rc::ptr_eq`.

/// Tipo do callback de unlock-notify (o `void (*xNotify)(void **, int)` do C).
pub type UnlockNotifyFn = std::rc::Rc<dyn Fn(&[CallbackArg])>;

thread_local! {
    /// Cabeça da lista de todos os objetos `sqlite3` criados pelo processo para os quais
    /// `pBlockingConnection` ou `pUnlockConnection` não é NULL. Só pode ser acessada com o
    /// mutex STATIC_MAIN mantido.
    static BLOCKED_LIST: std::cell::RefCell<Option<std::rc::Weak<std::cell::RefCell<Sqlite3>>>> =
        std::cell::RefCell::new(None);
}

/// O `sqlite3 **pp` do C: ou a cabeça da lista, ou o campo `pNextBlocked` de uma conexão.
enum BlockedLink {
    Head,
    Next(Sqlite3Ref),
}

/// Promove um elo fraco (`Option<Weak>`) à conexão que ele nomeia.
fn blocked_upgrade(link: &Option<std::rc::Weak<std::cell::RefCell<Sqlite3>>>) -> Option<Sqlite3Ref> {
    link.as_ref().and_then(|w| w.upgrade())
}

/// Verdadeiro se o elo fraco aponta para `db` (a comparação de ponteiros do C).
fn blocked_is(link: &Option<std::rc::Weak<std::cell::RefCell<Sqlite3>>>, db: &Sqlite3Ref) -> bool {
    match link {
        Some(w) => std::ptr::eq(w.as_ptr(), std::rc::Rc::as_ptr(db)),
        None => false,
    }
}

/// Compara dois callbacks de unlock-notify como o C compara os ponteiros de função.
fn notify_same(a: &Option<UnlockNotifyFn>, b: &Option<UnlockNotifyFn>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => std::rc::Rc::ptr_eq(x, y),
        _ => false,
    }
}

impl BlockedLink {
    /// Lê `*pp`.
    fn get(&self) -> Option<Sqlite3Ref> {
        match self {
            BlockedLink::Head => BLOCKED_LIST.with(|h| blocked_upgrade(&h.borrow())),
            BlockedLink::Next(c) => blocked_upgrade(&c.borrow().p_next_blocked),
        }
    }

    /// Grava `*pp = v`.
    fn set(&self, v: Option<Sqlite3Ref>) {
        let weak = v.map(|r| std::rc::Rc::downgrade(&r));
        match self {
            BlockedLink::Head => BLOCKED_LIST.with(|h| *h.borrow_mut() = weak),
            BlockedLink::Next(c) => c.borrow_mut().p_next_blocked = weak,
        }
    }
}

/// Remove a conexão db da lista de conexões bloqueadas. Se db não faz parte da lista, não faz nada.
fn remove_from_blocked_list(db: &Sqlite3Ref) {
    let mut pp = BlockedLink::Head;
    while let Some(cur) = pp.get() {
        if std::rc::Rc::ptr_eq(&cur, db) {
            let next = blocked_upgrade(&cur.borrow().p_next_blocked);
            pp.set(next);
            break;
        }
        pp = BlockedLink::Next(cur);
    }
}

/// Adiciona a conexão db à lista de conexões bloqueadas. Presume que ela ainda não faz parte
/// da lista.
fn add_to_blocked_list(db: &Sqlite3Ref) {
    let mut pp = BlockedLink::Head;
    loop {
        match pp.get() {
            Some(cur)
                if !notify_same(&cur.borrow().x_unlock_notify, &db.borrow().x_unlock_notify) =>
            {
                pp = BlockedLink::Next(cur);
            }
            _ => break,
        }
    }
    let next = pp.get();
    db.borrow_mut().p_next_blocked = next.map(|r| std::rc::Rc::downgrade(&r));
    pp.set(Some(db.clone()));
}

/// Obtém o mutex STATIC_MAIN.
fn enter_mutex() {
    let m = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(m.as_ref());
}

/// Libera o mutex STATIC_MAIN.
fn leave_mutex() {
    let m = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_leave(m.as_ref());
}

/// Registra um callback de unlock-notify (`sqlite3_unlock_notify`).
///
/// Chamado depois que a conexão db tentou uma operação mas recebeu SQLITE_LOCKED porque outra
/// conexão do mesmo processo (pOther) estava usando o mesmo cache compartilhado. pOther é
/// achada em `db.p_blocking_connection`.
///
/// Sem conexão bloqueadora, o callback é invocado na hora, antes de a rotina retornar. Se pOther
/// já está bloqueada em db, devolve SQLITE_LOCKED (impasse). Senão, arranja para invocar
/// x_notify quando pOther soltar seus locks. Cada chamada substitui callbacks registrados antes
/// na mesma db; com x_notify nulo, o callback anterior é cancelado na hora.
pub fn unlock_notify(db: &Sqlite3Ref, x_notify: Option<UnlockNotifyFn>, p_arg: CallbackArg) -> i32 {
    let mut rc = SQLITE_OK;

    let db_mutex = db.borrow().mutex.clone();
    mutex_enter(db_mutex.as_ref());
    enter_mutex();

    match x_notify {
        None => {
            remove_from_blocked_list(db);
            let mut d = db.borrow_mut();
            d.p_blocking_connection = None;
            d.p_unlock_connection = None;
            d.x_unlock_notify = None;
            d.p_unlock_arg = None;
        }
        Some(x_notify) => {
            let blocking = db.borrow().p_blocking_connection.clone();
            if blocking.is_none() {
                // A transação bloqueadora terminou, ou nunca houve uma. Em qualquer caso,
                // invoca o callback de notificação imediatamente.
                x_notify(&[p_arg]);
            } else {
                let mut p = blocked_upgrade(&blocking);
                while let Some(cur) = p.clone() {
                    if std::rc::Rc::ptr_eq(&cur, db) {
                        break;
                    }
                    p = blocked_upgrade(&cur.borrow().p_unlock_connection);
                }
                if p.is_some() {
                    rc = SQLITE_LOCKED; // Impasse detectado.
                } else {
                    {
                        let mut d = db.borrow_mut();
                        d.p_unlock_connection = d.p_blocking_connection.clone();
                        d.x_unlock_notify = Some(x_notify);
                        d.p_unlock_arg = p_arg;
                    }
                    remove_from_blocked_list(db);
                    add_to_blocked_list(db);
                }
            }
        }
    }

    leave_mutex();
    error_with_msg(
        &mut db.borrow_mut(),
        rc,
        if rc != 0 { Some(&b"database is deadlocked"[..]) } else { None },
    );
    mutex_leave(db_mutex.as_ref());
    rc
}

/// Chamada ao andar ou preparar um comando associado à conexão db. A operação devolverá
/// SQLITE_LOCKED ao usuário porque exige um lock que só estará disponível quando a conexão
/// p_blocker concluir a transação atual.
pub fn connection_blocked(db: &Sqlite3Ref, p_blocker: &Sqlite3Ref) {
    enter_mutex();
    let free = {
        let d = db.borrow();
        d.p_blocking_connection.is_none() && d.p_unlock_connection.is_none()
    };
    if free {
        add_to_blocked_list(db);
    }
    db.borrow_mut().p_blocking_connection = Some(std::rc::Rc::downgrade(p_blocker));
    leave_mutex();
}

/// Chamada quando a transação aberta pela conexão db acabou de terminar e os locks dela foram
/// liberados.
///
/// Percorre a lista de conexões bloqueadas e, para cada entrada:
///
///   1) se `p_blocking_connection` é db, zera `p_blocking_connection`;
///   2) se `p_unlock_connection` é db, invoca o callback de unlock-notify configurado e zera
///      `p_unlock_connection`;
///   3) se os dois passos deixam `p_blocking_connection` e `p_unlock_connection` nulos, tira a
///      entrada da lista.
pub fn connection_unlocked(db: &Sqlite3Ref) {
    /// Tamanho do espaço inicial `aStatic[16]`, que não exige malloc.
    const STATIC_ARGS: usize = 16;

    let mut x_unlock_notify: Option<UnlockNotifyFn> = None; // Callback a invocar
    let mut a_arg: Vec<CallbackArg> = Vec::with_capacity(STATIC_ARGS); // Argumentos do callback
    let mut capacity = STATIC_ARGS; // Capacidade de aArg[] em entradas

    enter_mutex(); // Entra no mutex STATIC_MAIN

    // O laço roda uma vez para cada entrada da lista de conexões bloqueadas.
    let mut pp = BlockedLink::Head;
    while let Some(p) = pp.get() {
        // Passo 1.
        if blocked_is(&p.borrow().p_blocking_connection, db) {
            p.borrow_mut().p_blocking_connection = None;
        }

        // Passo 2.
        if blocked_is(&p.borrow().p_unlock_connection, db) {
            let p_notify = p.borrow().x_unlock_notify.clone();
            if !notify_same(&p_notify, &x_unlock_notify) && !a_arg.is_empty() {
                if let Some(f) = &x_unlock_notify {
                    f(&a_arg);
                }
                a_arg.clear();
            }

            begin_benign_malloc();
            if a_arg.len() == capacity {
                // O array aArg[] precisa crescer.
                let n_arg = a_arg.len();
                if a_arg.try_reserve_exact(n_arg).is_ok() {
                    capacity = n_arg * 2;
                } else {
                    // Acontece quando o array de ponteiros de contexto a passar ao callback é
                    // maior que o espaço inicial e a tentativa de alocar um array maior falhou.
                    // Devolver erro ao chamador não resolve: a transação em db será fechada
                    // mesmo assim e os callbacks das conexões bloqueadas nunca seriam emitidos,
                    // o que poderia deixar a aplicação esperando para sempre. Em vez disso,
                    // invoca o callback com o array já acumulado, zera e recomeça a acumular
                    // sem alocação dinâmica. É subótimo (a aplicação recebe dois ou mais
                    // callbacks com arrays menores), mas é o melhor possível.
                    if let Some(f) = &x_unlock_notify {
                        f(&a_arg);
                    }
                    a_arg.clear();
                }
            }
            end_benign_malloc();

            {
                let mut pm = p.borrow_mut();
                a_arg.push(pm.p_unlock_arg.take());
                x_unlock_notify = pm.x_unlock_notify.take();
                pm.p_unlock_connection = None;
            }
        }

        // Passo 3.
        let gone = {
            let pm = p.borrow();
            pm.p_blocking_connection.is_none() && pm.p_unlock_connection.is_none()
        };
        if gone {
            // Remove a conexão p da lista de conexões bloqueadas.
            let next = blocked_upgrade(&p.borrow().p_next_blocked);
            pp.set(next);
            p.borrow_mut().p_next_blocked = None;
        } else {
            pp = BlockedLink::Next(p);
        }
    }

    if !a_arg.is_empty() {
        if let Some(f) = &x_unlock_notify {
            f(&a_arg);
        }
    }
    leave_mutex(); // Sai do mutex STATIC_MAIN
}

/// Chamada quando a conexão passada como argumento está sendo fechada. A conexão é removida da
/// lista de bloqueadas.
pub fn connection_closed(db: &Sqlite3Ref) {
    connection_unlocked(db);
    enter_mutex();
    remove_from_blocked_list(db);
    leave_mutex();
}

