// Mesclado das partes traduzidas de table_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Estrutura usada para passar dados de `sqlite3_get_table()` através da função
/// callback que ela usa para construir o resultado.
pub struct TabResult {
    /// Saída acumulada (o slot 0 é o reservado do C, onde ficava `n_data`).
    pub az_result: Vec<Option<Vec<u8>>>,
    /// Texto da mensagem de erro, se um erro ocorrer.
    pub z_err_msg: Option<Vec<u8>>,
    /// Slots alocados para `az_result`.
    pub n_alloc: u32,
    /// Número de linhas no resultado.
    pub n_row: u32,
    /// Número de colunas no resultado.
    pub n_column: u32,
    /// Slots usados em `az_result`. Será `(n_row+1)*n_column`.
    pub n_data: u32,
    /// Código de retorno de `sqlite3_exec()`.
    pub rc: i32,
}

/// Essa rotina é chamada uma vez para cada linha na tabela de resultado. Seu
/// trabalho é preencher a estrutura `TabResult` adequadamente, alocando nova
/// memória conforme necessário. `argv` é `None` quando o C passa `argv==0`.
fn sqlite3_get_table_cb(
    p: &mut TabResult,
    argv: Option<&[Option<Vec<u8>>]>,
    colv: &[Vec<u8>],
) -> i32 {
    let n_col = colv.len() as i32;

    // Garante que há espaço suficiente em `az_result` para tudo o que
    // precisamos guardar dessa invocação do callback.
    let need: u32 = if p.n_row == 0 && argv.is_some() {
        (n_col * 2) as u32
    } else {
        n_col as u32
    };
    if p.n_data.wrapping_add(need) > p.n_alloc {
        p.n_alloc = p.n_alloc.wrapping_mul(2).wrapping_add(need);
        p.az_result.resize(p.n_alloc as usize, None);
    }

    // Se essa for a primeira linha, gera uma linha extra contendo os nomes
    // de todas as colunas.
    if p.n_row == 0 {
        p.n_column = n_col as u32;
        for i in 0..n_col as usize {
            p.az_result[p.n_data as usize] = Some(colv[i].clone());
            p.n_data += 1;
        }
    } else if p.n_column as i32 != n_col {
        p.z_err_msg = Some(
            b"sqlite3_get_table() called with two or more incompatible queries".to_vec(),
        );
        p.rc = SQLITE_ERROR;
        return 1;
    }

    // Copia os dados da linha.
    if let Some(argv) = argv {
        for i in 0..n_col as usize {
            p.az_result[p.n_data as usize] = argv[i].clone();
            p.n_data += 1;
        }
        p.n_row += 1;
    }
    0
}

/// Consulta o banco de dados. Mas em vez de invocar um callback para cada linha,
/// aloca memória para guardar o resultado e devolve todos os resultados ao fim
/// da chamada.
///
/// O resultado escrito em `paz_result` já não inclui o slot 0 reservado do C
/// (`&res.azResult[1]`), então `free_table` é só o `Drop` do `Vec`.
/// Registrada como `api::get_table` pelo módulo `api` do integrador. Espera
/// `api::exec(db, sql, Option<&mut dyn FnMut(Option<&[Option<Vec<u8>>]>, &[Vec<u8>]) -> i32>,
/// Option<&mut Option<Vec<u8>>>) -> i32`, em que o callback devolvendo diferente de zero aborta.
pub fn get_table(
    db: Sqlite3Ref,
    z_sql: &[u8],
    paz_result: &mut Vec<Option<Vec<u8>>>,
    pn_row: Option<&mut i32>,
    pn_column: Option<&mut i32>,
    mut pz_err_msg: Option<&mut Option<Vec<u8>>>,
) -> i32 {
    *paz_result = Vec::new();
    let mut pn_row = pn_row;
    let mut pn_column = pn_column;
    if let Some(c) = pn_column.as_mut() {
        **c = 0;
    }
    if let Some(r) = pn_row.as_mut() {
        **r = 0;
    }
    if let Some(e) = pz_err_msg.as_mut() {
        **e = None;
    }
    let mut res = TabResult {
        z_err_msg: None,
        n_row: 0,
        n_column: 0,
        n_data: 1,
        n_alloc: 20,
        rc: SQLITE_OK,
        az_result: Vec::new(),
    };
    res.az_result = vec![None; res.n_alloc as usize];
    res.az_result[0] = None;

    let rc = {
        let mut cb = |argv: Option<&[Option<Vec<u8>>]>, colv: &[Vec<u8>]| -> i32 {
            sqlite3_get_table_cb(&mut res, argv, colv)
        };
        api::exec(db.clone(), z_sql, Some(&mut cb), pz_err_msg.as_deref_mut())
    };
    // No C o slot 0 guardava `n_data` (SQLITE_INT_TO_PTR); aqui o slot é descartado ao devolver.
    if (rc & 0xff) == SQLITE_ABORT {
        // `free_table` do C: o Drop libera `res.az_result`.
        if let Some(msg) = res.z_err_msg.take() {
            if let Some(e) = pz_err_msg.as_mut() {
                **e = Some(msg);
            }
        }
        db.borrow_mut().err_code = res.rc;
        return res.rc;
    }
    res.z_err_msg = None;
    if rc != SQLITE_OK {
        return rc;
    }
    if res.n_alloc > res.n_data {
        res.az_result.truncate(res.n_data as usize);
    }
    res.az_result.remove(0);
    *paz_result = res.az_result;
    if let Some(c) = pn_column.as_mut() {
        **c = res.n_column as i32;
    }
    if let Some(r) = pn_row.as_mut() {
        **r = res.n_row as i32;
    }
    rc
}

/// Essa rotina libera o espaço alocado por `sqlite3_get_table()`. No C percorre
/// os slots 1..n liberando cada um; aqui o `Drop` de `Vec<Option<Vec<u8>>>` faz isso.
/// Registrada como `api::free_table`.
pub fn free_table(az_result: Vec<Option<Vec<u8>>>) {
    drop(az_result);
}

