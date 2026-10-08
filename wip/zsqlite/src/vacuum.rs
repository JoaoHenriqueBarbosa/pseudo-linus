//! `vacuum.c`: o comando VACUUM (`sqlite3Vacuum`, `sqlite3RunVacuum`, `execSql`, `execSqlF`).
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - `pzErrMsg` é `&mut Option<Vec<u8>>` (o `Vdbe.z_err_msg`) e `sqlite3SetString` é uma atribuição;
//! - `sqlite3_value *pOut` é `Option<&mut Mem>`;
//! - `Btree *pMain` e `pTemp` são `db.dbs[i_db].bt` e `db.dbs[n_db].bt`: as rotinas que precisam
//!   dos dois ao mesmo tempo (`sqlite3BtreeCopyFile`) recebem o par por `split_at_mut`;
//! - `sqlite3BtreeBeginTrans` e `sqlite3BtreeCommit` leem da conexão o que o `BtDb` carrega, então
//!   rodam por `with_bt_db`;
//! - fechar o banco temporário é tirar o `Btree` do slot e entregá-lo a `close_btree`; o
//!   `sqlite3ResetAllSchemasOfConnection` que segue encolhe `db.dbs`;
//! - `SQLITE_BUG_COMPATIBLE_20160819` está desligado, e a falha de alocação não existe (somem os
//!   ramos `mallocFailed`).

use crate::btree::{
    btree_get_auto_vacuum, btree_get_page_size, btree_get_requested_reserve, btree_set_auto_vacuum,
    btree_set_cache_size, btree_set_page_size, btree_set_pager_flags, btree_set_spill_size,
};
use crate::btree_cursor::btree_begin_trans;
use crate::btree_types::Btree;
use crate::btree_cursor::btree_commit;
use crate::btree_write::{btree_get_meta, btree_update_meta};
use crate::build::{reset_all_schemas_of_connection, two_part_name};
use crate::connection::{Connection, Parse};
use crate::consts::{
    BTREE_APPLICATION_ID, BTREE_DEFAULT_CACHE_SIZE, BTREE_SCHEMA_VERSION, BTREE_TEXT_ENCODING,
    BTREE_USER_VERSION, DBFLAG_PREFER_BUILTIN, DBFLAG_VACUUM, DBFLAG_VACUUM_INTO, OP_VACUUM,
    PAGER_CACHESPILL, PAGER_FLAGS_MASK, PAGER_JOURNALMODE_WAL, PAGER_SYNCHRONOUS_OFF, SQLITE_COUNT_ROWS,
    SQLITE_DEFENSIVE, SQLITE_DONE, SQLITE_ERROR, SQLITE_FOREIGN_KEYS, SQLITE_IGNORE_CHECKS,
    SQLITE_NOMEM, SQLITE_NOMEM_BKPT, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READONLY,
    SQLITE_OPEN_READWRITE, SQLITE_REVERSE_ORDER, SQLITE_ROW, SQLITE_TEXT, SQLITE_WRITE_SCHEMA,
};
use crate::expr_code2::expr_code;
use crate::main::{close_btree, db_printf, errmsg};
use crate::mem::{value_type, Mem};
use crate::prepare::prepare_v2;
use crate::printf::PrintfArg;
use crate::resolve::resolve_self_reference;
use crate::select::get_vdbe;
use crate::sqlite_int::{Expr, Schema, Token};
use crate::vdbeapi::{column_text, finalize, step, value_text};
use crate::vdbeaux::{add_op2, vdbe_of_parse};
use crate::vdbeaux2::{uses_btree, with_bt_db};

/// Os metadados do btree que o VACUUM preserva: o número do metadado e o incremento aplicado
/// depois (o do cookie do esquema o faz subir, para outras conexões relerem o esquema).
const A_COPY: [(u32, u32); 5] = [
    (BTREE_SCHEMA_VERSION, 1),
    (BTREE_DEFAULT_CACHE_SIZE, 0),
    (BTREE_TEXT_ENCODING, 0),
    (BTREE_USER_VERSION, 0),
    (BTREE_APPLICATION_ID, 0),
];

/// `execSql`: executa `z_sql` no banco `db`. Se devolve linhas, cada uma tem exatamente uma coluna
/// (só acontece quando o SQL começa com SELECT) e é executada de novo, recursivamente, se for um
/// CREATE ou INSERT. Historicamente houve ataques que corrompiam `sqlite_schema.sql` com outros
/// comandos e rodavam o VACUUM para executá-los em hora imprópria; por isso só esses dois passam.
fn exec_sql(db: &mut Connection, pz_err_msg: &mut Option<Vec<u8>>, z_sql: &[u8]) -> i32 {
    let (mut rc, p_stmt, _tail) = prepare_v2(db, z_sql, -1);
    if rc != SQLITE_OK {
        return rc;
    }
    let Some(id) = p_stmt else {
        return SQLITE_OK;
    };
    loop {
        rc = step(db, id);
        if rc != SQLITE_ROW {
            break;
        }
        let z_sub_sql = column_text(db, id, 0).map(<[u8]>::to_vec);
        if let Some(z) = z_sub_sql {
            if z.starts_with(b"CRE") || z.starts_with(b"INS") {
                rc = exec_sql(db, pz_err_msg, &z);
                if rc != SQLITE_OK {
                    break;
                }
            }
        }
    }
    debug_assert!(rc != SQLITE_ROW);
    if rc == SQLITE_DONE {
        rc = SQLITE_OK;
    }
    if rc != SQLITE_OK {
        *pz_err_msg = Some(errmsg(db));
    }
    finalize(db, id);
    rc
}

/// `execSqlF`: como [`exec_sql`], com um formato de `printf` interno e os argumentos dele.
fn exec_sql_f(
    db: &mut Connection,
    pz_err_msg: &mut Option<Vec<u8>>,
    z_sql: &[u8],
    args: &[PrintfArg],
) -> i32 {
    match db_printf(db, z_sql, args) {
        Some(z) => exec_sql(db, pz_err_msg, &z),
        None => SQLITE_NOMEM,
    }
}

/// O `Btree` do slot `i`. Os chamadores só pedem o banco vacuumado (que o VDBE já abriu) e o
/// `vacuum_db` recém-anexado, ambos com `Btree`; o C os desreferencia sem conferir.
fn bt_at(db: &mut Connection, i: usize) -> &mut Btree {
    match db.dbs[i].bt.as_mut() {
        Some(bt) => bt,
        None => unreachable!("o banco {i} do VACUUM sempre tem Btree"),
    }
}

/// Os `Btree` de `dbs[i_main]` e de `dbs[i_temp]` (com `i_main < i_temp`) ao mesmo tempo.
fn bt_pair(db: &mut Connection, i_main: usize, i_temp: usize) -> (&mut Btree, &mut Btree) {
    debug_assert!(i_main < i_temp);
    let (lo, hi) = db.dbs.split_at_mut(i_temp);
    match (lo[i_main].bt.as_mut(), hi[0].bt.as_mut()) {
        (Some(main), Some(temp)) => (main, temp),
        _ => unreachable!("o banco {i_main} e o vacuum_db sempre têm Btree"),
    }
}

/// `sqlite3Vacuum`: gera o código do comando VACUUM. `p_nm` é o nome do banco (ou `None`) e
/// `p_into` a expressão do `INTO` (ou `None`).
pub fn vacuum(
    db: &mut Connection,
    parse: &mut Parse,
    p_nm: Option<&Token>,
    p_into: Option<Box<Expr>>,
) {
    let mut p_into = p_into;
    let mut i_db: i32 = 0;
    'build_vacuum_end: {
        get_vdbe(db, parse);
        if parse.n_err != 0 {
            break 'build_vacuum_end;
        }
        if let Some(nm) = p_nm {
            // Comportamento padrão: erro se o argumento do VACUUM não é reconhecido.
            match two_part_name(db, parse, nm, nm) {
                Some((i, _)) => i_db = i,
                None => break 'build_vacuum_end,
            }
        }
        if i_db != 1 {
            let mut i_into_reg = 0;
            if let Some(into) = p_into.as_deref_mut() {
                if resolve_self_reference(db, parse, None, 0, Some(&mut *into), None) == 0 {
                    parse.n_mem += 1;
                    i_into_reg = parse.n_mem;
                    expr_code(db, parse, into, i_into_reg, None);
                }
            }
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_VACUUM as i32, i_db, i_into_reg);
            uses_btree(v, i_db);
        }
    }
    // `sqlite3ExprDelete(pParse->db, pInto)` é o `Drop` de `p_into`.
}

/// `sqlite3RunVacuum`: implementa o opcode `OP_Vacuum` do VDBE. `i_db` é o banco anexado a
/// vacuumar e `p_out`, se existe, o valor com o nome do arquivo de saída (`VACUUM INTO`).
pub fn run_vacuum(
    pz_err_msg: &mut Option<Vec<u8>>,
    db: &mut Connection,
    i_db: i32,
    p_out: Option<&mut Mem>,
) -> i32 {
    let i_db = i_db as usize;
    let mut rc: i32;
    let mut pgflags: u32 = PAGER_SYNCHRONOUS_OFF;

    if db.auto_commit == 0 {
        *pz_err_msg = Some(b"cannot VACUUM from within a transaction".to_vec());
        return SQLITE_ERROR; // IMP: R-12218-18073
    }
    if db.n_vdbe_active > 1 {
        *pz_err_msg = Some(b"cannot VACUUM - SQL statements in progress".to_vec());
        return SQLITE_ERROR; // IMP: R-15610-35227
    }
    let saved_open_flags = db.open_flags;
    let has_out = p_out.is_some();
    let z_out: Vec<u8> = match p_out {
        Some(out) => {
            if value_type(out) != SQLITE_TEXT {
                *pz_err_msg = Some(b"non-text filename".to_vec());
                return SQLITE_ERROR;
            }
            let z = value_text(out).map(<[u8]>::to_vec).unwrap_or_default();
            db.open_flags &= !(SQLITE_OPEN_READONLY as u32);
            db.open_flags |= (SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE) as u32;
            z
        }
        None => Vec::new(),
    };

    // Guarda os flags da conexão para restaurá-los antes de voltar. Liga o esquema gravável e
    // desliga as restrições CHECK e de chave estrangeira.
    let saved_flags = db.flags;
    let saved_m_db_flags = db.m_db_flags;
    let saved_n_change = db.n_change;
    let saved_n_total_change = db.n_total_change;
    let saved_m_trace = db.m_trace;
    db.flags |= SQLITE_WRITE_SCHEMA | SQLITE_IGNORE_CHECKS;
    db.m_db_flags |= DBFLAG_PREFER_BUILTIN | DBFLAG_VACUUM;
    db.flags &= !(SQLITE_FOREIGN_KEYS | SQLITE_REVERSE_ORDER | SQLITE_DEFENSIVE | SQLITE_COUNT_ROWS);
    db.m_trace = 0;

    let z_db_main = db.dbs[i_db].z_db_s_name.clone();
    let is_mem_db = bt_at(db, i_db).bt.pager.is_memdb();
    let n_db = db.dbs.len();
    let mut p_db_attached = false;

    'end_of_vacuum: {
        // Anexa o banco temporário como 'vacuum_db'. O `synchronous` pode ficar desligado para
        // esse arquivo: ele não é recuperado se houver uma queda. A integridade do banco fica a
        // cargo de uma transação (possivelmente síncrona) aberta no banco principal antes do
        // `sqlite3BtreeCopyFile()`.
        rc = exec_sql_f(
            db,
            pz_err_msg,
            b"ATTACH %Q AS vacuum_db",
            &[PrintfArg::Text(Some(z_out.clone()))],
        );
        db.open_flags = saved_open_flags;
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        debug_assert!(db.dbs.len() - 1 == n_db);
        p_db_attached = true;
        debug_assert!(db.dbs[n_db].z_db_s_name == b"vacuum_db");
        if has_out {
            let mut sz: i64 = 0;
            let temp = bt_at(db, n_db);
            if let Some(id) = temp.bt.pager.fd.as_mut() {
                if id.file_size(&mut sz) != SQLITE_OK || sz > 0 {
                    rc = SQLITE_ERROR;
                    *pz_err_msg = Some(b"output file already exists".to_vec());
                    break 'end_of_vacuum;
                }
            }
            db.m_db_flags |= DBFLAG_VACUUM_INTO;

            // Num VACUUM INTO os flags do pager são os do banco vacuumado, mas o
            // `PAGER_CACHESPILL` fica sempre ligado.
            pgflags = db.dbs[i_db].safety_level as u32 | (db.flags as u32 & PAGER_FLAGS_MASK);
        }
        let mut n_res = btree_get_requested_reserve(bt_at(db, i_db));

        let cache_size = db.dbs[i_db].schema.cache_size;
        let spill = btree_set_spill_size(bt_at(db, i_db), 0);
        {
            let temp = bt_at(db, n_db);
            btree_set_cache_size(temp, cache_size);
            btree_set_spill_size(temp, spill);
            btree_set_pager_flags(temp, pgflags | PAGER_CACHESPILL);
        }

        // Abre uma transação e toma um bloqueio exclusivo no arquivo principal. Isto vem antes de
        // `sqlite3BtreeGetPageSize(pMain)`, para não tentar mudar o tamanho de página de um banco
        // WAL.
        rc = exec_sql(db, pz_err_msg, b"BEGIN");
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        let wrflag = if has_out { 0 } else { 2 };
        rc = with_bt_db(db, i_db, |bt, bdb| btree_begin_trans(bt, wrflag, None, bdb))
            .unwrap_or(SQLITE_ERROR);
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }

        // Não tenta mudar o tamanho de página de um banco WAL.
        if bt_at(db, i_db).bt.pager.journal_mode == PAGER_JOURNALMODE_WAL && !has_out {
            db.next_pagesize = 0;
        }

        let main_page_size = btree_get_page_size(bt_at(db, i_db));
        let next_pagesize = db.next_pagesize;
        {
            let temp = bt_at(db, n_db);
            if btree_set_page_size(temp, main_page_size, n_res, 0) != 0
                || (!is_mem_db && btree_set_page_size(temp, next_pagesize, n_res, 0) != 0)
            {
                rc = SQLITE_NOMEM_BKPT;
                break 'end_of_vacuum;
            }
        }

        let autovac = if db.next_autovac >= 0 {
            db.next_autovac as i32
        } else {
            btree_get_auto_vacuum(bt_at(db, i_db))
        };
        btree_set_auto_vacuum(bt_at(db, n_db), autovac);

        // Consulta o esquema do banco principal e cria um espelho dele no banco temporário.
        db.init.i_db = n_db as u8; // força os CREATE novos para o vacuum_db
        let arg_main = [PrintfArg::Text(Some(z_db_main.clone()))];
        rc = exec_sql_f(
            db,
            pz_err_msg,
            b"SELECT sql FROM \"%w\".sqlite_schema WHERE type='table'AND name<>'sqlite_sequence' AND coalesce(rootpage,1)>0",
            &arg_main,
        );
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        rc = exec_sql_f(
            db,
            pz_err_msg,
            b"SELECT sql FROM \"%w\".sqlite_schema WHERE type='index'",
            &arg_main,
        );
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        db.init.i_db = 0;

        // Percorre as tabelas do banco principal e, para cada uma, faz um
        // "INSERT INTO vacuum_db.xxx SELECT * FROM main.xxx;" que copia o conteúdo para o banco
        // temporário.
        rc = exec_sql_f(
            db,
            pz_err_msg,
            b"SELECT'INSERT INTO vacuum_db.'||quote(name)||' SELECT*FROM\"%w\".'||quote(name)FROM vacuum_db.sqlite_schema WHERE type='table'AND coalesce(rootpage,1)>0",
            &arg_main,
        );
        debug_assert!((db.m_db_flags & DBFLAG_VACUUM) != 0);
        db.m_db_flags &= !DBFLAG_VACUUM;
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }

        // Copia os gatilhos, as views e as tabelas virtuais do banco principal para o temporário.
        // Nenhum desses objetos tem armazenamento: basta copiar as linhas da tabela de esquema.
        rc = exec_sql_f(
            db,
            pz_err_msg,
            b"INSERT INTO vacuum_db.sqlite_schema SELECT*FROM \"%w\".sqlite_schema WHERE type IN('view','trigger') OR(type='table'AND rootpage=0)",
            &arg_main,
        );
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }

        // Agora há uma transação de escrita aberta no banco temporário e no principal. Sem erro,
        // as duas fecham neste bloco: a do principal por `sqlite3BtreeCopyFile()` e a outra por
        // um `sqlite3BtreeCommit()` explícito.
        for (idx, incr) in A_COPY {
            // `GetMeta` e `UpdateMeta` não falham aqui: a página 1 já está no cache e suja.
            let meta = btree_get_meta(bt_at(db, i_db), idx as i32);
            rc = btree_update_meta(bt_at(db, n_db), idx as i32, meta.wrapping_add(incr));
            if rc != SQLITE_OK {
                break 'end_of_vacuum;
            }
        }

        if !has_out {
            let (main, temp) = bt_pair(db, i_db, n_db);
            rc = crate::backup::btree_copy_file(main, temp);
        }
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        rc = with_bt_db(db, n_db, |bt, bdb| btree_commit(bt, bdb)).unwrap_or(SQLITE_ERROR);
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        if !has_out {
            let temp_autovac = btree_get_auto_vacuum(bt_at(db, n_db));
            btree_set_auto_vacuum(bt_at(db, i_db), temp_autovac);
        }

        debug_assert!(rc == SQLITE_OK);
        if !has_out {
            n_res = btree_get_requested_reserve(bt_at(db, n_db));
            let temp_page_size = btree_get_page_size(bt_at(db, n_db));
            rc = btree_set_page_size(bt_at(db, i_db), temp_page_size, n_res, 1);
        }
    }

    // end_of_vacuum: restaura os valores originais dos flags da conexão.
    db.init.i_db = 0;
    db.m_db_flags = saved_m_db_flags;
    db.flags = saved_flags;
    db.n_change = saved_n_change;
    db.n_total_change = saved_n_total_change;
    db.m_trace = saved_m_trace;
    btree_set_page_size(bt_at(db, i_db), -1, 0, 1);

    // Há uma transação SQL aberta no banco temporário e nenhum bloqueio em outro arquivo (o
    // principal já foi confirmado no nível do btree). Então é seguro encerrar a transação ligando
    // `autoCommit` à mão e soltando o `vacuum_db`: o journal dele é apagado quando o pager fecha.
    db.auto_commit = 1;

    if p_db_attached {
        if let Some(bt) = db.dbs[n_db].bt.take() {
            close_btree(db, bt);
        }
        db.dbs[n_db].schema = Schema::default();
    }

    // Isto limpa os esquemas e também reduz o tamanho de `db.dbs`.
    reset_all_schemas_of_connection(db);

    rc
}
