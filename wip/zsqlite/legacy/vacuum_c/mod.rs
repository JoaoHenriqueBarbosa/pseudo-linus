// Mesclado das partes traduzidas de vacuum_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Executa `z_sql` no banco `db`.
///
/// Se `z_sql` devolver linhas, cada linha tem exatamente uma coluna (isso só acontece quando
/// `z_sql` começa com "SELECT"). Cada linha do resultado é executada de novo, recursivamente,
/// por `exec_sql()`.
fn exec_sql(db: &Sqlite3Ref, pz_err_msg: &mut Option<Vec<u8>>, z_sql: &[u8]) -> i32 {
    let mut p_stmt: Option<VdbeRef> = None;
    let mut rc: i32;

    rc = api::prepare_v2(db, z_sql, -1, &mut p_stmt, None);
    if rc != SQLITE_OK {
        return rc;
    }
    let stmt = p_stmt.expect("prepare_v2 com SQLITE_OK entrega o statement");
    loop {
        rc = api::step(&stmt);
        if rc != SQLITE_ROW {
            break;
        }
        let z_sub_sql: Option<Vec<u8>> = api::column_text(&stmt, 0);
        debug_assert!(api::strnicmp(z_sql, b"SELECT", 6) == 0);
        // O SQL secundário precisa ser CREATE TABLE, CREATE INDEX ou INSERT. Historicamente
        // houve ataques que corrompiam o campo sqlite_schema.sql com outros tipos de comando e
        // depois rodavam VACUUM para executá-los em momentos inadequados.
        if let Some(z_sub_sql) = z_sub_sql {
            if z_sub_sql.starts_with(b"CRE") || z_sub_sql.starts_with(b"INS") {
                rc = exec_sql(db, pz_err_msg, &z_sub_sql);
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
    if rc != 0 {
        let z_msg = api::errmsg(db);
        set_string(pz_err_msg, db, &z_msg);
    }
    let _ = api::finalize(stmt);
    rc
}

/// Faz o mesmo que `exec_sql()`, mas o terceiro argumento é um formato (`%Q`, `%w`) e os
/// argumentos variádicos do C chegam como fatias de texto (`None` é o ponteiro nulo).
fn exec_sql_f(
    db: &Sqlite3Ref,
    pz_err_msg: &mut Option<Vec<u8>>,
    z_sql: &[u8],
    args: &[Option<&[u8]>],
) -> i32 {
    let z = match vm_printf(db, z_sql, args) {
        Some(z) => z,
        None => return SQLITE_NOMEM,
    };
    exec_sql(db, pz_err_msg, &z)
}

/// O comando VACUUM limpa o banco e compacta o espaço livre. É modelado a partir do VACUUM do
/// PostgreSQL e funciona em três passos:
///
///   (1) cria um arquivo de banco transitório novo;
///   (2) copia todo o conteúdo do banco sendo limpo para o arquivo transitório;
///   (3) copia o conteúdo do transitório de volta para o banco original.
///
/// O transitório precisa de espaço temporário próximo ao tamanho do original, e o passo (3)
/// precisa de outro tanto para o journal de rollback.
pub fn vacuum(p_parse: &mut Parse, p_nm: Option<&Token>, p_into: Option<Box<Expr>>) {
    let mut p_into = p_into;
    let v = get_vdbe(p_parse);
    let mut i_db: i32 = 0;
    'build_vacuum_end: {
        let v = match v {
            Some(v) => v,
            None => break 'build_vacuum_end,
        };
        if p_parse.n_err != 0 {
            break 'build_vacuum_end;
        }
        if let Some(nm) = p_nm {
            // Comportamento padrão (sem SQLITE_BUG_COMPATIBLE_20160819): relata erro se o
            // argumento do VACUUM não for reconhecido.
            let mut p_unqual: Option<Token> = None;
            i_db = two_part_name(p_parse, nm, nm, &mut p_unqual);
            if i_db < 0 {
                break 'build_vacuum_end;
            }
        }
        if i_db != 1 {
            let mut i_into_reg: i32 = 0;
            if let Some(into) = p_into.as_deref_mut() {
                if resolve_self_reference(p_parse, None, 0, Some(&mut *into), None) == 0 {
                    p_parse.n_mem += 1;
                    i_into_reg = p_parse.n_mem;
                    expr_code(p_parse, into, i_into_reg);
                }
            }
            vdbe_add_op2(&v, OP_VACUUM, i_db, i_into_reg);
            vdbe_uses_btree(&v, i_db);
        }
    }
    let db = p_parse.db.clone();
    expr_delete(&db, p_into);
}

/// Implementa o opcode OP_Vacuum do VDBE.
///
/// `pz_err_msg` recebe a mensagem de erro, `i_db` é o banco anexado a limpar e `p_out` é o nome
/// do arquivo de saída quando é um VACUUM INTO.
pub fn run_vacuum(
    pz_err_msg: &mut Option<Vec<u8>>,
    db: &Sqlite3Ref,
    i_db: i32,
    p_out: Option<&Mem>,
) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let z_out: Option<Vec<u8>>;
    let mut pgflags: u32 = PAGER_SYNCHRONOUS_OFF;

    let auto_commit = db.borrow().auto_commit;
    if auto_commit == 0 {
        set_string(pz_err_msg, db, b"cannot VACUUM from within a transaction");
        return SQLITE_ERROR; // IMP: R-12218-18073
    }
    let n_vdbe_active = db.borrow().n_vdbe_active;
    if n_vdbe_active > 1 {
        set_string(pz_err_msg, db, b"cannot VACUUM - SQL statements in progress");
        return SQLITE_ERROR; // IMP: R-15610-35227
    }
    let saved_open_flags: u32 = db.borrow().open_flags;
    if let Some(p_out) = p_out {
        if api::value_type(p_out) != SQLITE_TEXT {
            set_string(pz_err_msg, db, b"non-text filename");
            return SQLITE_ERROR;
        }
        z_out = api::value_text(p_out);
        let mut d = db.borrow_mut();
        d.open_flags &= !SQLITE_OPEN_READONLY;
        d.open_flags |= SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE;
    } else {
        z_out = Some(Vec::new());
    }

    // Guarda os flags atuais para restaurar antes de retornar. Depois liga o flag de esquema
    // gravável e desliga CHECK e chaves estrangeiras.
    let saved_flags: u64;
    let saved_m_db_flags: u32;
    let saved_n_change: i64;
    let saved_n_total_change: i64;
    let saved_m_trace: u8;
    let z_db_main: Option<Vec<u8>>;
    let p_main: BtreeRef;
    {
        let mut d = db.borrow_mut();
        saved_flags = d.flags;
        saved_m_db_flags = d.m_db_flags;
        saved_n_change = d.n_change;
        saved_n_total_change = d.n_total_change;
        saved_m_trace = d.m_trace;
        d.flags |= SQLITE_WRITESCHEMA | SQLITE_IGNORECHECKS;
        d.m_db_flags |= DBFLAG_PREFERBUILTIN | DBFLAG_VACUUM;
        d.flags &= !(SQLITE_FOREIGNKEYS | SQLITE_REVERSEORDER | SQLITE_DEFENSIVE | SQLITE_COUNTROWS);
        d.m_trace = 0;

        z_db_main = d.a_db[i_db as usize].z_db_s_name.clone();
        p_main = d.a_db[i_db as usize]
            .p_bt
            .clone()
            .expect("o banco a limpar sempre tem Btree aberto");
    }
    let is_mem_db: i32 = pager_is_memdb(&btree_pager(&p_main));
    let mut p_db: Option<usize> = None;

    'end_of_vacuum: {
        // Anexa o banco temporário como 'vacuum_db'. O pragma synchronous pode ficar 'off' para
        // esse arquivo, porque ele não é recuperado após uma queda de qualquer jeito. A
        // integridade é mantida por uma transação aberta no banco principal antes de
        // btree_copy_file().
        let n_db: i32 = db.borrow().n_db;
        rc = exec_sql_f(db, pz_err_msg, b"ATTACH %Q AS vacuum_db", &[z_out.as_deref()]);
        db.borrow_mut().open_flags = saved_open_flags;
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        debug_assert!(db.borrow().n_db - 1 == n_db);
        p_db = Some(n_db as usize);
        debug_assert!(
            db.borrow().a_db[n_db as usize].z_db_s_name.as_deref() == Some(&b"vacuum_db"[..])
        );
        let p_temp: BtreeRef = db.borrow().a_db[n_db as usize]
            .p_bt
            .clone()
            .expect("o ATTACH com SQLITE_OK deixa o Btree aberto");
        if p_out.is_some() {
            let id = pager_file(&btree_pager(&p_temp));
            let mut sz: i64 = 0;
            if id.borrow().p_methods.is_some()
                && (os_file_size(&id, &mut sz) != SQLITE_OK || sz > 0)
            {
                rc = SQLITE_ERROR;
                set_string(pz_err_msg, db, b"output file already exists");
                break 'end_of_vacuum;
            }
            db.borrow_mut().m_db_flags |= DBFLAG_VACUUMINTO;

            // No VACUUM INTO, os flags do pager são os mesmos do banco limpo, exceto que
            // PAGER_CACHESPILL fica sempre ligado.
            let d = db.borrow();
            pgflags = (d.a_db[i_db as usize].safety_level as u64
                | (d.flags & PAGER_FLAGS_MASK as u64)) as u32;
        }
        let mut n_res: i32 = btree_get_requested_reserve(&p_main);

        let cache_size: i32 = db.borrow().a_db[i_db as usize]
            .p_schema
            .as_ref()
            .expect("o banco a limpar sempre tem esquema")
            .borrow()
            .cache_size;
        btree_set_cache_size(&p_temp, cache_size);
        let spill_size = btree_set_spill_size(&p_main, 0);
        btree_set_spill_size(&p_temp, spill_size);
        btree_set_pager_flags(&p_temp, pgflags | PAGER_CACHESPILL);

        // Abre uma transação e toma lock exclusivo no arquivo principal. Isso vem antes de
        // btree_get_page_size(p_main) para não tentar mudar o tamanho de página de um banco WAL.
        rc = exec_sql(db, pz_err_msg, b"BEGIN");
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        rc = btree_begin_trans(&p_main, if p_out.is_none() { 2 } else { 0 }, None);
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }

        // Não tenta mudar o tamanho de página de um banco WAL.
        if pager_get_journal_mode(&btree_pager(&p_main)) == PAGER_JOURNALMODE_WAL
            && p_out.is_none()
        {
            db.borrow_mut().next_pagesize = 0;
        }

        let next_pagesize: i32 = db.borrow().next_pagesize;
        if btree_set_page_size(&p_temp, btree_get_page_size(&p_main), n_res, 0) != 0
            || (is_mem_db == 0 && btree_set_page_size(&p_temp, next_pagesize, n_res, 0) != 0)
            || db.borrow().malloc_failed != 0
        {
            rc = SQLITE_NOMEM;
            break 'end_of_vacuum;
        }

        let next_autovac: i32 = db.borrow().next_autovac as i32;
        let autovac = if next_autovac >= 0 {
            next_autovac
        } else {
            btree_get_auto_vacuum(&p_main)
        };
        btree_set_auto_vacuum(&p_temp, autovac);

        // Consulta o esquema do banco principal e cria um espelho no temporário.
        db.borrow_mut().init.i_db = n_db as u8; // força os CREATE novos para o vacuum_db
        rc = exec_sql_f(
            db,
            pz_err_msg,
            concat!(
                "SELECT sql FROM \"%w\".sqlite_schema",
                " WHERE type='table'AND name<>'sqlite_sequence'",
                " AND coalesce(rootpage,1)>0"
            )
            .as_bytes(),
            &[z_db_main.as_deref()],
        );
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        rc = exec_sql_f(
            db,
            pz_err_msg,
            concat!("SELECT sql FROM \"%w\".sqlite_schema", " WHERE type='index'").as_bytes(),
            &[z_db_main.as_deref()],
        );
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }
        db.borrow_mut().init.i_db = 0;

        // Percorre as tabelas do banco principal e, para cada uma, faz
        // "INSERT INTO vacuum_db.xxx SELECT * FROM main.xxx;" copiando o conteúdo.
        rc = exec_sql_f(
            db,
            pz_err_msg,
            concat!(
                "SELECT'INSERT INTO vacuum_db.'||quote(name)",
                "||' SELECT*FROM\"%w\".'||quote(name)",
                "FROM vacuum_db.sqlite_schema ",
                "WHERE type='table'AND coalesce(rootpage,1)>0"
            )
            .as_bytes(),
            &[z_db_main.as_deref()],
        );
        debug_assert!((db.borrow().m_db_flags & DBFLAG_VACUUM) != 0);
        db.borrow_mut().m_db_flags &= !DBFLAG_VACUUM;
        if rc != SQLITE_OK {
            break 'end_of_vacuum;
        }

        // Copia gatilhos, views e tabelas virtuais do principal para o temporário. Nenhum deles
        // tem armazenamento próprio, então basta copiar as entradas da tabela de esquema.
        rc = exec_sql_f(
            db,
            pz_err_msg,
            concat!(
                "INSERT INTO vacuum_db.sqlite_schema",
                " SELECT*FROM \"%w\".sqlite_schema",
                " WHERE type IN('view','trigger')",
                " OR(type='table'AND rootpage=0)"
            )
            .as_bytes(),
            &[z_db_main.as_deref()],
        );
        if rc != 0 {
            break 'end_of_vacuum;
        }

        // Neste ponto há transação de escrita aberta no banco temporário e no principal. Sem
        // erro, as duas fecham neste bloco: a do principal em btree_copy_file() e a outra na
        // chamada explícita a btree_commit().
        {
            // Define quais valores meta são preservados. Posições pares são o número do meta;
            // posições ímpares são o incremento aplicado depois, usado para aumentar o cookie
            // do esquema e fazer as outras conexões relerem o esquema.
            const A_COPY: [u8; 10] = [
                BTREE_SCHEMA_VERSION as u8,
                1, // soma um ao cookie de esquema antigo
                BTREE_DEFAULT_CACHE_SIZE as u8,
                0, // preserva o tamanho de cache padrão
                BTREE_TEXT_ENCODING as u8,
                0, // preserva a codificação de texto
                BTREE_USER_VERSION as u8,
                0, // preserva a versão do usuário
                BTREE_APPLICATION_ID as u8,
                0, // preserva o id da aplicação
            ];

            debug_assert!(SQLITE_TXN_WRITE == btree_txn_state(&p_temp));
            debug_assert!(p_out.is_some() || SQLITE_TXN_WRITE == btree_txn_state(&p_main));

            // Copia os valores meta do Btree. get_meta() e update_meta() não falham aqui porque
            // a página 1 já está no cache e marcada como suja.
            let mut i = 0;
            while i < A_COPY.len() {
                let mut meta: u32 = 0;
                btree_get_meta(&p_main, A_COPY[i] as i32, &mut meta);
                rc = btree_update_meta(
                    &p_temp,
                    A_COPY[i] as i32,
                    meta.wrapping_add(A_COPY[i + 1] as u32),
                );
                if rc != SQLITE_OK {
                    break 'end_of_vacuum;
                }
                i += 2;
            }

            if p_out.is_none() {
                rc = btree_copy_file(&p_main, &p_temp);
            }
            if rc != SQLITE_OK {
                break 'end_of_vacuum;
            }
            rc = btree_commit(&p_temp);
            if rc != SQLITE_OK {
                break 'end_of_vacuum;
            }
            if p_out.is_none() {
                btree_set_auto_vacuum(&p_main, btree_get_auto_vacuum(&p_temp));
            }
        }

        debug_assert!(rc == SQLITE_OK);
        if p_out.is_none() {
            n_res = btree_get_requested_reserve(&p_temp);
            rc = btree_set_page_size(&p_main, btree_get_page_size(&p_temp), n_res, 1);
        }
    }

    // end_of_vacuum: restaura os valores originais de db.flags.
    {
        let mut d = db.borrow_mut();
        d.init.i_db = 0;
        d.m_db_flags = saved_m_db_flags;
        d.flags = saved_flags;
        d.n_change = saved_n_change;
        d.n_total_change = saved_n_total_change;
        d.m_trace = saved_m_trace;
    }
    btree_set_page_size(&p_main, -1, 0, 1);

    // Há uma transação SQL aberta no banco de vácuo e nenhum lock em outro arquivo (o principal
    // já foi confirmado no nível do btree). Então é seguro encerrá-la ligando auto_commit à mão
    // e desanexando o banco de vácuo. O journal do vacuum_db é apagado quando o pager fecha.
    db.borrow_mut().auto_commit = 1;

    if let Some(idx) = p_db {
        let bt = {
            let mut d = db.borrow_mut();
            let bt = d.a_db[idx].p_bt.take();
            d.a_db[idx].p_schema = None;
            bt
        };
        if let Some(bt) = bt {
            btree_close(bt);
        }
    }

    // Isso limpa os esquemas e reduz o tamanho do vetor db.a_db.
    reset_all_schemas_of_connection(db);

    rc
}


// ---- part_001.rs ----

// O trecho C correspondente (chunks/vacuum_c.001.c) contém apenas o `#endif` que fecha
// `!defined(SQLITE_OMIT_VACUUM) && !defined(SQLITE_OMIT_ATTACH)`. Ambas as opções de omissão
// ficam indefinidas, então não há nenhum item a traduzir aqui.

