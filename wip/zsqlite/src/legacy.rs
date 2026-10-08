//! `legacy.c`: `sqlite3_exec`, a interface de conveniência que prepara e executa uma sequência
//! de comandos SQL, chamando um gancho a cada linha de resultado (SQLite 3.46.1, modelo v2).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * O gancho `xCallback(pArg, nCol, azVals, azCols)` é um fechamento que também recebe a
//!   conexão (`init_callback`, de `prepare.rs`, prepara comandos nela): `FnMut(&mut Connection,
//!   &[Option<Vec<u8>>], &[Vec<u8>]) -> i32`, com os valores (`None` é NULL) e os nomes das
//!   colunas. Sem `pArg` (a captura do fechamento o substitui) e sem `nCol` (é o tamanho de
//!   `az_cols`). No caso `SQLITE_DONE` com `SQLITE_NullCallback`, em que o C passa `azVals` nulo,
//!   os valores chegam como fatia vazia.
//! * `pzErrMsg` é o parâmetro `pz_err_msg` de [`exec_with_errmsg`]; [`exec`] é a forma sem ele
//!   (o ponteiro nulo do C). O texto da mensagem é um `Vec<u8>` possuído.
//! * `zSql` é uma fatia: o fim é o fim da fatia ou o primeiro NUL; o resto do texto depois de
//!   cada comando é um deslocamento (ver `prepare.rs`).
//! * O mutex da conexão some e, como o Rust não falha ao alocar, somem os ramos de
//!   `sqlite3DbMallocRaw`/`sqlite3DbStrDup` com resultado nulo.

use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_ABORT, SQLITE_DONE, SQLITE_MISUSE, SQLITE_NULL, SQLITE_NULL_CALLBACK, SQLITE_OK,
    SQLITE_ROW,
};
use crate::ctype::is_space;
use crate::prepare::prepare_v2;
use crate::vdbeapi::{column_count, column_name, column_text, column_type, finalize, step};

/// O byte de `z` em `i`, ou 0 depois do fim (o terminador implícito do C).
#[inline]
fn byte_at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// `sqlite3_exec` sem `pzErrMsg`: executa o SQL e chama o gancho (se houver) em cada linha de
/// resultado. Devolve um dos códigos `SQLITE_*`.
pub fn exec(
    db: &mut Connection,
    z_sql: &[u8],
    x_callback: Option<
        &mut dyn FnMut(&mut Connection, &[Option<Vec<u8>>], &[Vec<u8>]) -> i32,
    >,
) -> i32 {
    exec_with_errmsg(db, z_sql, x_callback, None)
}

/// `sqlite3_exec`: executa o SQL, comando por comando. Se o SQL é uma consulta, chama
/// `x_callback` em cada linha de resultado; sem gancho nada é chamado, nem para consultas. Se o
/// gancho devolve diferente de zero a execução termina com `SQLITE_ABORT`. Com `pz_err_msg`, uma
/// falha grava nele a mensagem de erro (e um sucesso o deixa `None`).
pub fn exec_with_errmsg(
    db: &mut Connection,
    z_sql: &[u8],
    x_callback: Option<
        &mut dyn FnMut(&mut Connection, &[Option<Vec<u8>>], &[Vec<u8>]) -> i32,
    >,
    pz_err_msg: Option<&mut Option<Vec<u8>>>,
) -> i32 {
    let mut x_callback = x_callback;
    let mut rc = SQLITE_OK;
    let mut p_stmt: Option<StmtId> = None;
    let mut pos = 0usize;

    if !crate::main::safety_check_ok(db) {
        return SQLITE_MISUSE;
    }
    crate::main::error(db, SQLITE_OK);

    'exec_out: {
        while rc == SQLITE_OK && byte_at(z_sql, pos) != 0 {
            let (prepare_rc, stmt, tail) = prepare_v2(db, &z_sql[pos.min(z_sql.len())..], -1);
            rc = prepare_rc;
            p_stmt = stmt;
            let z_leftover = pos + tail;
            if rc != SQLITE_OK {
                continue;
            }
            let Some(id) = p_stmt else {
                // Acontece com um comentário ou espaço em branco.
                pos = z_leftover;
                continue;
            };
            let mut callback_is_init = false;
            let mut az_cols: Vec<Vec<u8>> = Vec::new();

            loop {
                rc = step(db, id);

                // Chama o gancho se for o caso.
                if x_callback.is_some()
                    && (rc == SQLITE_ROW
                        || (rc == SQLITE_DONE
                            && !callback_is_init
                            && (db.flags & SQLITE_NULL_CALLBACK) != 0))
                {
                    if !callback_is_init {
                        let n_col = column_count(db, id);
                        // `sqlite3VdbeSetColName()` grava os nomes em UTF-8, então
                        // `sqlite3_column_name()` não tem como falhar.
                        az_cols = (0..n_col)
                            .map(|i| column_name(db, id, i).unwrap_or_default())
                            .collect();
                        callback_is_init = true;
                    }
                    let mut az_vals: Vec<Option<Vec<u8>>> = Vec::new();
                    if rc == SQLITE_ROW {
                        for i in 0..az_cols.len() as i32 {
                            let val = column_text(db, id, i).map(|b| b.to_vec());
                            if val.is_none() && column_type(db, id, i) != SQLITE_NULL {
                                crate::util::oom_fault(db);
                                break 'exec_out;
                            }
                            az_vals.push(val);
                        }
                    }
                    let aborted = match x_callback.as_mut() {
                        Some(cb) => cb(db, &az_vals, &az_cols) != 0,
                        None => false,
                    };
                    if aborted {
                        // EVIDENCE-OF R-38229-40159: se o gancho de `sqlite3_exec()` devolve
                        // diferente de zero, a função devolve `SQLITE_ABORT`.
                        rc = SQLITE_ABORT;
                        finalize(db, id);
                        p_stmt = None;
                        crate::main::error(db, SQLITE_ABORT);
                        break 'exec_out;
                    }
                }

                if rc != SQLITE_ROW {
                    rc = finalize(db, id);
                    p_stmt = None;
                    pos = z_leftover;
                    while is_space(byte_at(z_sql, pos)) {
                        pos += 1;
                    }
                    break;
                }
            }
        }
    }

    if let Some(id) = p_stmt {
        finalize(db, id);
    }

    rc = crate::main::api_exit(db, rc);
    if let Some(pz) = pz_err_msg {
        if rc != SQLITE_OK {
            *pz = Some(crate::main::errmsg(db));
        } else {
            *pz = None;
        }
    }

    debug_assert!((rc & db.err_mask) == rc);
    rc
}
