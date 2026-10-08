//! `notify.c`: `sqlite3_unlock_notify()` e a lista de conexões bloqueadas (`SQLITE_ENABLE_UNLOCK_NOTIFY`
//! está ligada no Debian 13).
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - As conexões não se apontam: `pBlockingConnection` e `pUnlockConnection` guardam a identidade
//!   da conexão (o endereço do `Connection`, que vive dentro de um `Box` e nunca se move), e os
//!   quatro campos que o C pendura no `sqlite3` (`pBlockingConnection`, `pUnlockConnection`,
//!   `xUnlockNotify`, `pUnlockArg`) mais o `pNextBlocked` moram juntos numa entrada da lista
//!   `BLOCKED_LIST`. Uma conexão fora da lista tem os quatro campos nulos, como no C.
//! - `sqlite3BlockedList` é um `thread_local!`: um `Connection` usa `Rc` e nunca sai da thread
//!   que o abriu, então a relação "esta conexão espera aquela" só existe dentro de uma thread.
//!   O mutex `STATIC_MAIN` some.
//! - O callback `xNotify(void **apArg, int nArg)` é um `Rc<dyn Fn(&[UnlockArg])>` e o `pArg` um
//!   `Option<Rc<dyn Any>>`. A agrupação de callbacks iguais compara o `Rc` por identidade.
//! - Os callbacks de `sqlite3ConnectionUnlocked` rodam depois do laço, com a lista já solta (o
//!   C os chama no meio do laço, com o mutex tomado; um callback não pode usar a API do SQLite,
//!   então a ordem e os argumentos são os mesmos).
//! - Sem cache compartilhado (CONVENTIONS, item 4) ninguém chama `sqlite3ConnectionBlocked`, que
//!   só o `btree` com `sqlite3_enable_shared_cache` aciona; a lista fica vazia e
//!   `sqlite3_unlock_notify` com callback não nulo o invoca na hora, como o C faz sem bloqueio.
//! - Sem `sqlite3SafetyCheckOk` (`SQLITE_ENABLE_API_ARMOR` está desligado) e sem a falta de
//!   memória do vetor `aArg`.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::connection::Connection;
use crate::consts::{SQLITE_LOCKED, SQLITE_OK};
use crate::main::{error, error_with_msg};

/// O `void *pArg` do `sqlite3_unlock_notify`.
pub type UnlockArg = Option<Rc<dyn Any>>;

/// O `void (*xNotify)(void **apArg, int nArg)`: recebe os `pArg` das conexões desbloqueadas.
pub type UnlockNotifyFn = Rc<dyn Fn(&[UnlockArg])>;

/// Uma conexão da lista `sqlite3BlockedList`, com os campos que o C guarda no próprio `sqlite3`.
struct BlockedEntry {
    /// A identidade da conexão (o endereço do `Connection`).
    key: usize,
    /// `pBlockingConnection`.
    blocking: Option<usize>,
    /// `pUnlockConnection`.
    unlock: Option<usize>,
    /// `xUnlockNotify`.
    notify: Option<UnlockNotifyFn>,
    /// `pUnlockArg`.
    arg: UnlockArg,
}

thread_local! {
    /// `sqlite3BlockedList`: toda conexão com `pBlockingConnection` ou `pUnlockConnection` não
    /// nulo. As entradas que compartilham o mesmo `xUnlockNotify` ficam juntas.
    static BLOCKED_LIST: RefCell<Vec<BlockedEntry>> = const { RefCell::new(Vec::new()) };
}

/// A identidade de uma conexão para a lista (o `sqlite3*` do C).
fn connection_key(db: &Connection) -> usize {
    db as *const Connection as usize
}

/// Os callbacks são iguais quando são o mesmo `Rc` (ou os dois nulos).
fn notify_eq(a: &Option<UnlockNotifyFn>, b: &Option<UnlockNotifyFn>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `removeFromBlockedList`: tira a conexão `key` da lista; se não está nela, não faz nada.
fn remove_from_blocked_list(list: &mut Vec<BlockedEntry>, key: usize) -> Option<BlockedEntry> {
    let pos = list.iter().position(|e| e.key == key)?;
    Some(list.remove(pos))
}

/// `addToBlockedList`: põe a entrada na lista, na frente do primeiro grupo que tem o mesmo
/// `xUnlockNotify` (ou no fim, se não há). Supõe que a conexão ainda não está nela.
fn add_to_blocked_list(list: &mut Vec<BlockedEntry>, entry: BlockedEntry) {
    let pos = list
        .iter()
        .position(|e| notify_eq(&e.notify, &entry.notify))
        .unwrap_or(list.len());
    list.insert(pos, entry);
}

/// `sqlite3_unlock_notify`: registra um callback de unlock-notify.
///
/// Chamada depois que a conexão `db` tentou algo e recebeu `SQLITE_LOCKED` porque outra conexão
/// do processo usava o mesmo cache compartilhado (a outra é `db->pBlockingConnection`). Sem
/// conexão bloqueadora, o callback roda na hora. Se a outra já espera por `db`, devolve
/// `SQLITE_LOCKED` (deadlock). Senão, programa a chamada de `x_notify` para quando a outra solta
/// as travas. Cada chamada substitui o callback anterior de `db`; `x_notify` nulo cancela.
pub fn unlock_notify(db: &mut Connection, x_notify: Option<UnlockNotifyFn>, p_arg: UnlockArg) -> i32 {
    let key = connection_key(db);
    let mut rc = SQLITE_OK;
    let mut call_now: Option<(UnlockNotifyFn, UnlockArg)> = None;

    BLOCKED_LIST.with(|cell| {
        let mut list = cell.borrow_mut();
        match x_notify {
            None => {
                // Os quatro campos voltam a nulo: a entrada deixa de existir.
                remove_from_blocked_list(&mut list, key);
            }
            Some(notify) => {
                let blocking = list.iter().find(|e| e.key == key).and_then(|e| e.blocking);
                match blocking {
                    None => {
                        // A transação bloqueadora terminou, ou nunca houve. Em qualquer caso o
                        // callback roda agora.
                        call_now = Some((notify, p_arg));
                    }
                    Some(first) => {
                        // `for(p=db->pBlockingConnection; p && p!=db; p=p->pUnlockConnection)`.
                        let mut p = Some(first);
                        while let Some(cur) = p {
                            if cur == key {
                                break;
                            }
                            p = list.iter().find(|e| e.key == cur).and_then(|e| e.unlock);
                        }
                        if p.is_some() {
                            rc = SQLITE_LOCKED; // Deadlock detectado.
                        } else if let Some(mut entry) = remove_from_blocked_list(&mut list, key) {
                            entry.unlock = entry.blocking;
                            entry.notify = Some(notify);
                            entry.arg = p_arg;
                            add_to_blocked_list(&mut list, entry);
                        }
                    }
                }
            }
        }
    });

    // `xNotify(&pArg, 1)`: roda com a lista solta, como se a conexão não esperasse ninguém.
    if let Some((notify, arg)) = call_now {
        notify(&[arg]);
    }

    if rc != SQLITE_OK {
        error_with_msg(db, rc, b"database is deadlocked", &[]);
    } else {
        error(db, rc);
    }
    rc
}

/// `sqlite3ConnectionBlocked`: chamada ao executar ou preparar um comando da conexão `db` que vai
/// devolver `SQLITE_LOCKED` ao usuário porque precisa de uma trava que só fica livre quando a
/// conexão `blocker` concluir a transação.
pub fn connection_blocked(db: &Connection, blocker: &Connection) {
    let key = connection_key(db);
    let blocker_key = connection_key(blocker);
    BLOCKED_LIST.with(|cell| {
        let mut list = cell.borrow_mut();
        match list.iter_mut().find(|e| e.key == key) {
            Some(entry) => entry.blocking = Some(blocker_key),
            None => add_to_blocked_list(
                &mut list,
                BlockedEntry {
                    key,
                    blocking: Some(blocker_key),
                    unlock: None,
                    notify: None,
                    arg: None,
                },
            ),
        }
    });
}

/// `sqlite3ConnectionUnlocked`: chamada quando a transação aberta por `db` acabou e as travas
/// foram soltas. Percorre a lista e, para cada entrada:
///
/// 1. se `pBlockingConnection` é `db`, zera;
/// 2. se `pUnlockConnection` é `db`, junta o `pUnlockArg` ao lote do seu callback, zera
///    `pUnlockConnection` (e o callback e o argumento);
/// 3. se os dois ficaram nulos, tira a entrada da lista.
///
/// Os argumentos das entradas consecutivas que dividem o mesmo callback vão numa só chamada.
pub fn connection_unlocked(db: &Connection) {
    let key = connection_key(db);
    let mut calls: Vec<(UnlockNotifyFn, Vec<UnlockArg>)> = Vec::new();

    BLOCKED_LIST.with(|cell| {
        let mut list = cell.borrow_mut();
        // `xUnlockNotify` e `aArg[]` do C: o lote em formação.
        let mut batch: Option<(UnlockNotifyFn, Vec<UnlockArg>)> = None;
        let mut i = 0;
        while i < list.len() {
            let p = &mut list[i];

            // Passo 1.
            if p.blocking == Some(key) {
                p.blocking = None;
            }

            // Passo 2.
            if p.unlock == Some(key) {
                if let Some(notify) = p.notify.take() {
                    let same = batch.as_ref().is_some_and(|(cur, _)| Rc::ptr_eq(cur, &notify));
                    if !same {
                        if let Some(full) = batch.take() {
                            calls.push(full);
                        }
                        batch = Some((notify, Vec::new()));
                    }
                    if let Some((_, args)) = batch.as_mut() {
                        args.push(p.arg.take());
                    }
                }
                p.unlock = None;
                p.arg = None;
            }

            // Passo 3.
            if p.blocking.is_none() && p.unlock.is_none() {
                list.remove(i);
            } else {
                i += 1;
            }
        }
        if let Some(rest) = batch {
            calls.push(rest);
        }
    });

    for (notify, args) in calls {
        notify(&args);
    }
}

/// `sqlite3ConnectionClosed`: a conexão `db` está sendo fechada; sai da lista.
pub fn connection_closed(db: &Connection) {
    connection_unlocked(db);
    let key = connection_key(db);
    BLOCKED_LIST.with(|cell| {
        remove_from_blocked_list(&mut cell.borrow_mut(), key);
    });
}
