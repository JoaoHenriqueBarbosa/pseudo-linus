// Mesclado das partes traduzidas de legacy_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Executa código SQL. Retorna um dos códigos de sucesso/falha SQLITE_. Também
/// escreve uma mensagem de erro em `pz_err_msg` (o `*pzErrMsg` do C).
///
/// Se o SQL for uma consulta, então para cada linha do resultado da consulta
/// a função `x_callback` é chamada. `p_arg` se torna o primeiro argumento do
/// callback. Se `x_callback` for `None` nenhum callback é invocado, mesmo para
/// consultas.
///
/// Este é o `sqlite3_exec`: o integrador deve reexportá-lo como `api::exec`.
/// O callback recebe `(p_arg, n_col, az_vals, az_cols)`, onde `az_vals` é
/// `None` quando o C passaria `azVals==0` (passo SQLITE_DONE com NullCallback).
pub fn exec(
    db: &Sqlite3Ref,
    z_sql: Option<&[u8]>,
    x_callback: Option<&Sqlite3Callback>,
    p_arg: &Option<Rc<dyn Any>>,
    pz_err_msg: Option<&mut Option<Vec<u8>>>,
) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let mut z_leftover: &[u8] = b"";
    let mut p_stmt: Option<VdbeRef> = None;
    let mut az_cols: Vec<Option<Vec<u8>>> = Vec::new();

    if !safety_check_ok(db) {
        return misuse_bkpt(line!());
    }
    let mut z_sql_cur: &[u8] = z_sql.unwrap_or(b"");

    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    error(db, SQLITE_OK);

    'exec_out: {
        while rc == SQLITE_OK && !z_sql_cur.is_empty() {
            let mut n_col: i32 = 0;

            p_stmt = None;
            rc = api::prepare_v2(db, z_sql_cur, -1, &mut p_stmt, &mut z_leftover);
            if rc != SQLITE_OK {
                continue;
            }
            if p_stmt.is_none() {
                // acontece para um comentário ou espaço em branco
                z_sql_cur = z_leftover;
                continue;
            }
            let mut callback_is_init = false;

            loop {
                let stmt = p_stmt.clone().unwrap();
                rc = api::step(&stmt);

                // Invoca a função de callback se necessário
                if x_callback.is_some()
                    && (rc == SQLITE_ROW
                        || (rc == SQLITE_DONE
                            && !callback_is_init
                            && (db.borrow().flags & SQLITE_NULL_CALLBACK) != 0))
                {
                    if !callback_is_init {
                        n_col = api::column_count(&stmt);
                        // o C aloca 2*nCol+1 ponteiros; aqui só os nCol nomes
                        // (a alocação em Rust não falha, some o ramo de OOM)
                        az_cols = Vec::with_capacity(n_col as usize);
                        for i in 0..n_col {
                            az_cols.push(api::column_name(&stmt, i));
                        }
                        callback_is_init = true;
                    }
                    let mut az_vals: Option<Vec<Option<Vec<u8>>>> = None;
                    if rc == SQLITE_ROW {
                        let mut vals: Vec<Option<Vec<u8>>> = Vec::with_capacity(n_col as usize);
                        for i in 0..n_col {
                            let v = api::column_text(&stmt, i);
                            if v.is_none() && api::column_type(&stmt, i) != SQLITE_NULL {
                                oom_fault(db);
                                break 'exec_out;
                            }
                            vals.push(v);
                        }
                        az_vals = Some(vals);
                    }
                    let cb = x_callback.unwrap();
                    if cb(p_arg, n_col, az_vals.as_deref(), &az_cols) != 0 {
                        // EVIDENCE-OF: R-38229-40159 Se o callback retornar
                        // não zero, sqlite3_exec() retorna SQLITE_ABORT.
                        rc = SQLITE_ABORT;
                        vdbe_finalize(&stmt);
                        p_stmt = None;
                        error(db, SQLITE_ABORT);
                        break 'exec_out;
                    }
                }

                if rc != SQLITE_ROW {
                    rc = vdbe_finalize(&stmt);
                    p_stmt = None;
                    z_sql_cur = z_leftover;
                    while !z_sql_cur.is_empty() && isspace(z_sql_cur[0]) {
                        z_sql_cur = &z_sql_cur[1..];
                    }
                    break;
                }
            }

            az_cols = Vec::new();
        }
    }

    // exec_out:
    if let Some(stmt) = p_stmt.take() {
        vdbe_finalize(&stmt);
    }
    drop(az_cols);

    rc = api_exit(db, rc);
    match pz_err_msg {
        Some(out) if rc != SQLITE_OK => {
            *out = db_str_dup(None, &api::errmsg(db));
            if out.is_none() {
                rc = nomem_bkpt(line!());
                error(db, SQLITE_NOMEM);
            }
        }
        Some(out) => {
            *out = None;
        }
        None => {}
    }

    debug_assert!((rc & db.borrow().err_mask) == rc);
    mutex_leave(mutex.as_ref());
    rc
}

