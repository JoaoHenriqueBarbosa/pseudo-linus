//! Tradução de `attach.c`: os comandos ATTACH e DETACH, e as rotinas `sqlite3Fix*` que fixam os
//! objetos de uma view, gatilho ou índice ao banco em que foram criados.
//!
//! Notas do modelo v2:
//! - `sqlite_attach()` e `sqlite_detach()` são `FuncDef` por `thread_local!` (o `Rc<FuncDef>` não é
//!   `Sync`), entregues a `add_function_call` como o `static const FuncDef` do C.
//! - `SQLITE_USER_AUTHENTICATION` está desligado.
//! - O `DbFixer` do C guarda `pParse` e o `Walker w`. Os callbacks do `Walker<C>` não recebem a
//!   conexão nem o `Parse`, então o contexto aqui é `FixCtx`: o `DbFixer` mais o que os callbacks
//!   consultariam na conexão (`db->init.busy` e o índice do banco) e o erro que eles detectam. O
//!   callback grava o erro em `FixCtx.err` e aborta o percurso; quem chamou o percurso (que tem
//!   `db` e `parse`) emite a mensagem. O efeito observável é o mesmo, porque o `WRC_ABORT`
//!   interrompe o percurso logo depois do `sqlite3ErrorMsg` do C.
//! - `sqlite3FindDbName(db, pItem->zDatabase)` comparado com o índice do banco fixado é, como os
//!   nomes de banco são únicos, o mesmo que perguntar se `zDatabase` nomeia aquele banco
//!   (`sqlite3DbIsNamed`); `name_matches` serve às duas.
//! - `Select.pSrc` possui a lista: `fix_src_list` move a lista para um `Select` temporário e a
//!   devolve ao chamador depois do percurso.

use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::btree::{
    btree_close, btree_open, btree_secure_delete, btree_set_mmap_limit, btree_set_pager_flags,
};
use crate::btree_cursor::BtDb;
use crate::btree_types::Btree;
use crate::btree_write::btree_txn_state;
use crate::auth::auth_check;
use crate::build::{collapse_database_array, reset_all_schemas_of_connection, token_arg};
use crate::callback::schema_get;
use crate::connection::{Connection, Context, DbSlot, FuncDef, Parse};
use crate::consts::{
    DBFLAG_SCHEMA_KNOWN_OK, EP_FROM_DDL, OP_EXPIRE, PAGER_FLAGS_MASK, PAGER_SYNCHRONOUS_FULL,
    SQLITE_ATTACH, SQLITE_CONSTRAINT, SQLITE_DEFAULT_SYNCHRONOUS, SQLITE_DETACH, SQLITE_ERROR,
    SQLITE_INTERRUPT, SQLITE_IOERR_NOMEM, SQLITE_LIMIT_ATTACHED, SQLITE_NOMEM, SQLITE_NO_CKPT_ON_CLOSE,
    SQLITE_OK, SQLITE_OPEN_MAIN_DB, SQLITE_TXN_NONE, SQLITE_UTF8, TK_ID, TK_NULL, TK_STRING,
    TK_VARIABLE, WRC_ABORT, WRC_CONTINUE,
};
use crate::expr_code2::{expr_code, get_temp_range};
use crate::hash::{hash_data_mut, hash_first, hash_next};
use crate::main::{close_btree, db_printf, install_busy_handlers, parse_uri, sync_btree_flags};
use crate::os::vfs_find;
use crate::pager_ext::PagerCloseDb;
use crate::prepare::init;
use crate::printf::{snprintf, PrintfArg};
use crate::resolve::{name_context_new, resolve_expr_names};
use crate::sqlite_int::{
    DbFixer, Expr, NameContext, Schema, SrcList, SrcU3, Select, Token, TriggerStep, Upsert, Walker,
};
use crate::util::{error_msg, oom_fault, str_icmp};
use crate::vdbeapi::{result_error, result_error_code, value_text};
use crate::vdbeaux::{add_function_call, add_op1, add_op2, vdbe_of_parse};
use crate::walker::{
    walk_expr, walk_expr_list, walk_select, walk_win_defn_dummy_callback, WALKER_FLAG_IN_RENAME,
};
use crate::mem::Mem;
use crate::prepare::read_schema;
use crate::select::get_vdbe;

// ---------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------

/// O mesmo teste de `sqlite3DbIsNamed` sobre o nome do banco `i_db`: `z_db_s_name` é o nome dele.
fn name_matches(z_db_s_name: &[u8], i_db: i32, z_name: &[u8]) -> bool {
    str_icmp(z_db_s_name, z_name) == 0 || (i_db == 0 && str_icmp(b"main", z_name) == 0)
}

/// `sqlite3DbIsNamed`: verdadeiro se `z_name` é um nome que pode ser usado para se referir ao
/// banco `i_db` da conexão `db`.
pub fn db_is_named(db: &Connection, i_db: usize, z_name: &[u8]) -> bool {
    name_matches(&db.dbs[i_db].z_db_s_name, i_db as i32, z_name)
}

/// `sqlite3_value_text` de um argumento imutável: converte uma cópia.
fn text_arg(p: &Mem) -> Option<Vec<u8>> {
    let mut c = p.clone();
    value_text(&mut c).map(|s| s.to_vec())
}


// ---------------------------------------------------------------------------------------------
// ATTACH e DETACH
// ---------------------------------------------------------------------------------------------

/// `resolveAttachExpr`: resolve uma expressão que fez parte de um ATTACH ou DETACH. Difere da
/// resolução normal porque um identificador simples vale como cadeia, não como possível nome de
/// coluna ou alias. Só vale para a raiz de `p_expr`: `ATTACH DATABASE abc||def AS 'db2'` falha,
/// porque nem abc nem def se resolvem.
fn resolve_attach_expr(
    parse: &mut Parse,
    db: &mut Connection,
    p_name: &mut NameContext<'_>,
    p_expr: Option<&mut Expr>,
) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(e) = p_expr {
        if e.op != TK_ID {
            rc = resolve_expr_names(db, parse, p_name, Some(e));
        } else {
            e.op = TK_STRING;
        }
    }
    rc
}

/// O que `attach_func` deixa para o chamador relatar ao `Context`.
enum AttachOutcome {
    /// Sucesso (ou o retorno silencioso de um `return` do C).
    Done,
    /// `sqlite3_result_error(context, zErr, -1)` e `return`.
    UriError(Option<Vec<u8>>),
    /// O `attach_error` do C: a mensagem `zErrDyn` e o código `rc`.
    Error { rc: i32, msg: Option<Vec<u8>> },
}

/// O corpo de `attachFunc` sobre a conexão.
fn attach_impl(db: &mut Connection, z_file: &[u8], z_name: &[u8]) -> AttachOutcome {
    let mut rc: i32;
    let mut z_err_dyn: Option<Vec<u8>> = None;
    let reopen = db.init.reopen_memdb;
    let new_idx: usize; // o índice de `pNew` em `db.dbs`

    if reopen {
        // Isto não é um ATTACH de verdade: a rotina foi chamada por sqlite3_deserialize() para
        // fechar o banco `db.init.i_db` e reabri-lo como um MemDB vazio.
        let Some(vfs) = vfs_find(Some(b"memdb")) else {
            return AttachOutcome::Done;
        };
        new_idx = db.init.i_db as usize;
        rc = match btree_open(vfs, Some(b"x\0"), 0, SQLITE_OPEN_MAIN_DB) {
            Ok(mut bt) => {
                // `sqlite3SchemaGet` não falha por falta de memória neste modelo: o ramo
                // `SQLITE_NOMEM` do C não existe.
                let schema = schema_get(db, Some(&bt));
                bt.db_index = new_idx as i32;
                btree_set_mmap_limit(&mut bt, db.sz_mmap);
                if let Some(old) = db.dbs[new_idx].bt.take() {
                    close_btree(db, old);
                }
                db.dbs[new_idx].bt = Some(bt);
                db.dbs[new_idx].schema = schema;
                install_busy_handlers(db);
                sync_btree_flags(db);
                SQLITE_OK
            }
            Err(e) => e,
        };
        if rc != SQLITE_OK {
            return AttachOutcome::Error { rc, msg: z_err_dyn };
        }
    } else {
        // Um ATTACH de verdade. Confere os erros: bancos demais, transação aberta, nome já em
        // uso.
        if db.dbs.len() as i32 >= db.a_limit[SQLITE_LIMIT_ATTACHED as usize] + 2 {
            z_err_dyn = db_printf(
                db,
                b"too many attached databases - max %d",
                &[PrintfArg::Int(db.a_limit[SQLITE_LIMIT_ATTACHED as usize] as i64)],
            );
            return AttachOutcome::Error { rc: SQLITE_OK, msg: z_err_dyn };
        }
        for i in 0..db.dbs.len() {
            if db_is_named(db, i, z_name) {
                z_err_dyn = db_printf(
                    db,
                    b"database %s is already in use",
                    &[PrintfArg::Text(Some(z_name.to_vec()))],
                );
                return AttachOutcome::Error { rc: SQLITE_OK, msg: z_err_dyn };
            }
        }

        // Abre o arquivo do banco. Se a árvore abre, usa-a para obter o esquema do banco; nesse
        // ponto o esquema pode ou não estar inicializado.
        let mut flags = db.open_flags;
        let mut p_vfs = None;
        let mut z_path: Option<Vec<u8>> = None;
        let mut z_err: Option<Vec<u8>> = None;
        let rc_uri = parse_uri(
            db.p_vfs.as_ref().map(|v| v.name()),
            z_file,
            &mut flags,
            &mut p_vfs,
            &mut z_path,
            &mut z_err,
        );
        if rc_uri != SQLITE_OK {
            if rc_uri == SQLITE_NOMEM {
                oom_fault(db);
            }
            return AttachOutcome::UriError(z_err);
        }
        let Some(p_vfs) = p_vfs else {
            return AttachOutcome::UriError(z_err);
        };
        flags |= SQLITE_OPEN_MAIN_DB as u32;
        let opened = btree_open(p_vfs, z_path.as_deref(), 0, flags as i32);
        new_idx = db.dbs.len();
        let mut slot = DbSlot { z_db_s_name: z_name.to_vec(), ..DbSlot::default() };
        match opened {
            Ok(mut bt) => {
                bt.db_index = new_idx as i32;
                btree_set_mmap_limit(&mut bt, db.sz_mmap);
                slot.bt = Some(bt);
                rc = SQLITE_OK;
            }
            Err(e) => rc = e,
        }
        db.dbs.push(slot);
        if db.dbs[new_idx].bt.is_some() {
            install_busy_handlers(db);
            sync_btree_flags(db);
        }
    }
    db.no_shared_cache = 0;
    if rc == SQLITE_CONSTRAINT {
        rc = SQLITE_ERROR;
        z_err_dyn = db_printf(db, b"database is already attached", &[]);
    } else if rc == SQLITE_OK {
        let bt = db.dbs[new_idx].bt.take();
        let schema = schema_get(db, bt.as_ref());
        db.dbs[new_idx].bt = bt;
        if schema.file_format != 0 && schema.enc != db.enc {
            z_err_dyn = db_printf(
                db,
                b"attached databases must use the same text encoding as main database",
                &[],
            );
            rc = SQLITE_ERROR;
        }
        db.dbs[new_idx].schema = schema;
        let secure = db.dbs[0].bt.as_mut().map_or(0, |b| btree_secure_delete(b, -1));
        let pager_flags = PAGER_SYNCHRONOUS_FULL | (db.flags as u32 & PAGER_FLAGS_MASK);
        let lock_mode = db.dflt_lock_mode as i32;
        if let Some(bt) = db.dbs[new_idx].bt.as_mut() {
            bt.bt.pager.locking_mode(lock_mode);
            btree_secure_delete(bt, secure);
            btree_set_pager_flags(bt, pager_flags);
        }
    }
    db.dbs[new_idx].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;

    // Se o arquivo abriu, lê o esquema do banco novo. Se isto falha, ou se a abertura falhou,
    // fecha o arquivo e remove a entrada de `db.dbs`: devolve tudo como estava.
    if rc == SQLITE_OK {
        db.init.i_db = 0;
        db.m_db_flags &= !DBFLAG_SCHEMA_KNOWN_OK;
        if !reopen {
            rc = init(db, &mut z_err_dyn);
        }
        debug_assert!(z_err_dyn.is_none() || rc != SQLITE_OK);
    }
    if rc != SQLITE_OK {
        if !reopen {
            let i_db = db.dbs.len() - 1;
            debug_assert!(i_db >= 2);
            if let Some(bt) = db.dbs[i_db].bt.take() {
                close_btree(db, bt);
                db.dbs[i_db].schema = Schema::default();
            }
            reset_all_schemas_of_connection(db);
            db.dbs.truncate(i_db);
            if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                oom_fault(db);
                z_err_dyn = db_printf(db, b"out of memory", &[]);
            } else if z_err_dyn.is_none() {
                z_err_dyn = db_printf(
                    db,
                    b"unable to open database: %s",
                    &[PrintfArg::Text(Some(z_file.to_vec()))],
                );
            }
        }
        return AttachOutcome::Error { rc, msg: z_err_dyn };
    }
    AttachOutcome::Done
}

/// `attachFunc`: a função SQL que faz o trabalho de um ATTACH. Os três argumentos vêm direto do
/// comando:
///
/// ```text
///     ATTACH DATABASE x AS y KEY z
///     SELECT sqlite_attach(x, y, z)
/// ```
///
/// Se o "KEY z" é omitido, o terceiro argumento é um NULL SQL. Se `db.init.reopen_memdb` está
/// ligado, em vez de anexar um banco novo fecha o banco de `db.init.i_db` e o reabre como um
/// MemDB vazio.
fn attach_func(context: &mut Context<'_>, argv: &[Mem]) {
    let z_file = text_arg(&argv[0]).unwrap_or_default();
    let z_name = text_arg(&argv[1]).unwrap_or_default();
    match attach_impl(&mut *context.db, &z_file, &z_name) {
        AttachOutcome::Done => {}
        AttachOutcome::UriError(z_err) => {
            if let Some(msg) = z_err {
                result_error(context, &msg, msg.len() as i32);
            } else {
                context.is_error = SQLITE_ERROR;
            }
        }
        AttachOutcome::Error { rc, msg } => {
            // attach_error: devolve o erro.
            if let Some(msg) = msg {
                result_error(context, &msg, msg.len() as i32);
            }
            if rc != SQLITE_OK {
                result_error_code(context, rc);
            }
        }
    }
}

/// O corpo de `detachFunc`: a mensagem de erro, se houver.
fn detach_impl(db: &mut Connection, z_name: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    while i < db.dbs.len() {
        if db.dbs[i].bt.is_some() && db_is_named(db, i, z_name) {
            break;
        }
        i += 1;
    }
    let z_arg = [PrintfArg::Text(Some(z_name.to_vec()))];
    if i >= db.dbs.len() {
        return Some(snprintf(127, b"no such database: %s", &z_arg));
    }
    if i < 2 {
        return Some(snprintf(127, b"cannot detach database %s", &z_arg));
    }
    let p_bt = db.dbs[i].bt.as_ref();
    if btree_txn_state(p_bt) != SQLITE_TXN_NONE || p_bt.map_or(false, |b| b.n_backup != 0) {
        return Some(snprintf(127, b"database %s is locked", &z_arg));
    }

    // Se algum gatilho TEMP referencia o esquema que sai, passa esses gatilhos a referenciar o
    // próprio esquema TEMP. (O `Rc<Trigger>` é copiado na escrita: a cópia que a tabela guarda
    // pertence ao esquema que está saindo.)
    let tab_schema = db.dbs[i].schema.id;
    let h = &mut db.dbs[1].schema.trig_hash;
    let mut entry = hash_first(h);
    while let Some(e) = entry {
        let p_trig = hash_data_mut(h, e);
        if p_trig.p_tab_schema == tab_schema {
            let t = Rc::make_mut(p_trig);
            t.p_tab_schema = t.p_schema;
        }
        entry = hash_next(h, e);
    }

    if let Some(bt) = db.dbs[i].bt.take() {
        close_btree(db, bt);
    }
    db.dbs[i].schema = Schema::default();
    collapse_database_array(db);
    None
}

/// `detachFunc`: a função SQL que faz o trabalho de um DETACH:
///
/// ```text
///     DETACH DATABASE x
///     SELECT sqlite_detach(x)
/// ```
fn detach_func(context: &mut Context<'_>, argv: &[Mem]) {
    let z_name = text_arg(&argv[0]).unwrap_or_default();
    if let Some(z_err) = detach_impl(&mut *context.db, &z_name) {
        result_error(context, &z_err, z_err.len() as i32);
    }
}

thread_local! {
    /// O `static const FuncDef detach_func` de `sqlite3Detach`.
    static DETACH_FUNC: Rc<FuncDef> = Rc::new(FuncDef {
        n_arg: 1,
        func_flags: SQLITE_UTF8 as u32,
        x_s_func: Some(detach_func),
        z_name: b"sqlite_detach".to_vec(),
        ..FuncDef::default()
    });
    /// O `static const FuncDef attach_func` de `sqlite3Attach`.
    static ATTACH_FUNC: Rc<FuncDef> = Rc::new(FuncDef {
        n_arg: 3,
        func_flags: SQLITE_UTF8 as u32,
        x_s_func: Some(attach_func),
        z_name: b"sqlite_attach".to_vec(),
        ..FuncDef::default()
    });
}

/// `sqlite3ExprCode` de uma expressão que pode ser nula (o C gera `OP_Null` para o ponteiro
/// nulo).
fn code_nullable(db: &mut Connection, parse: &mut Parse, p_expr: Option<&mut Expr>, target: i32) {
    match p_expr {
        Some(e) => expr_code(db, parse, e, target, None),
        None => {
            if parse.p_vdbe.is_some() {
                add_op2(vdbe_of_parse(parse), crate::consts::OP_NULL as i32, 0, target);
            }
        }
    }
}

/// `codeAttach`: gera o código do VDBE de uma única chamada de `sqlite_detach()` ou
/// `sqlite_attach()`. `auth_on_key` diz qual expressão vai ao gancho de autorização: o C passa
/// `pAuthArg` aliasado com `pFilename` (ATTACH) ou com `pKey` (DETACH).
fn code_attach(
    db: &mut Connection,
    parse: &mut Parse,
    type_: i32,
    p_func: &Rc<FuncDef>,
    auth_on_key: bool,
    mut p_filename: Option<Box<Expr>>,
    mut p_dbname: Option<Box<Expr>>,
    mut p_key: Option<Box<Expr>>,
) {
    'attach_end: {
        if SQLITE_OK != read_schema(db, parse) {
            break 'attach_end;
        }
        if parse.n_err != 0 {
            break 'attach_end;
        }
        let mut s_name = name_context_new();
        if SQLITE_OK != resolve_attach_expr(parse, db, &mut s_name, p_filename.as_deref_mut())
            || SQLITE_OK != resolve_attach_expr(parse, db, &mut s_name, p_dbname.as_deref_mut())
            || SQLITE_OK != resolve_attach_expr(parse, db, &mut s_name, p_key.as_deref_mut())
        {
            break 'attach_end;
        }

        {
            let p_auth_arg = if auth_on_key { p_key.as_deref() } else { p_filename.as_deref() };
            debug_assert!(p_auth_arg.is_some());
            let z_auth_arg: Option<Vec<u8>> = match p_auth_arg {
                Some(a) if a.op == TK_STRING => match &a.u {
                    crate::sqlite_int::ExprU::Token(t) => t.clone(),
                    crate::sqlite_int::ExprU::IValue(_) => None,
                },
                _ => None,
            };
            if p_auth_arg.is_some() {
                let rc = auth_check(db, parse, type_, z_auth_arg.as_deref(), None, None);
                if rc != SQLITE_OK {
                    break 'attach_end;
                }
            }
        }

        get_vdbe(db, parse);
        let reg_args = get_temp_range(parse, 4);
        code_nullable(db, parse, p_filename.as_deref_mut(), reg_args);
        code_nullable(db, parse, p_dbname.as_deref_mut(), reg_args + 1);
        code_nullable(db, parse, p_key.as_deref_mut(), reg_args + 2);

        let n_arg = p_func.n_arg as i32;
        add_function_call(parse, 0, reg_args + 3 - n_arg, reg_args + 3, n_arg, p_func, 0);
        // Codifica um OP_Expire. Num ATTACH, P1 é verdadeiro (expira só este comando); num
        // DETACH é falso (expira todos os comandos existentes).
        add_op1(vdbe_of_parse(parse), OP_EXPIRE as i32, (type_ == SQLITE_ATTACH) as i32);
    }
    // attach_end: `p_filename`, `p_dbname` e `p_key` são liberados ao sair do escopo.
}

/// `sqlite3Detach`: chamada pelo analisador para compilar um DETACH.
///
/// ```text
///     DETACH pDbname
/// ```
pub fn detach(db: &mut Connection, parse: &mut Parse, p_dbname: Option<Box<Expr>>) {
    DETACH_FUNC.with(|f| code_attach(db, parse, SQLITE_DETACH, f, true, None, None, p_dbname));
}

/// `sqlite3Attach`: chamada pelo analisador para compilar um ATTACH.
///
/// ```text
///     ATTACH p AS pDbname KEY pKey
/// ```
pub fn attach(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<Box<Expr>>,
    p_dbname: Option<Box<Expr>>,
    p_key: Option<Box<Expr>>,
) {
    ATTACH_FUNC.with(|f| code_attach(db, parse, SQLITE_ATTACH, f, false, p, p_dbname, p_key));
}

// ---------------------------------------------------------------------------------------------
// sqlite3Fix*
// ---------------------------------------------------------------------------------------------

/// O erro que um callback do fixador detectou; a mensagem sai depois do percurso.
#[derive(Clone)]
pub enum FixError {
    /// `"%s cannot use variables"`.
    Variable,
    /// `"%s %T cannot reference objects in database %s"`, com o `zDatabase` do item.
    Database(Vec<u8>),
}

/// O contexto do `Walker` do fixador (ver o comentário do módulo).
#[derive(Clone)]
pub struct FixCtx {
    /// O `DbFixer` do C.
    pub fix: DbFixer,
    /// O índice do banco a que os objetos ficam fixados.
    pub i_db: i32,
    /// `db->init.busy` no momento de `fix_init`.
    pub init_busy: bool,
    /// O erro detectado por um callback, se houve.
    pub err: Option<FixError>,
}

/// `fixExprCb`: o callback de expressão das rotinas `sqlite3FixAAAA()`.
fn fix_expr_cb(p: &mut Walker<FixCtx>, p_expr: &mut Expr) -> i32 {
    if !p.u.fix.b_temp {
        p_expr.set_property(EP_FROM_DDL);
    }
    if p_expr.op == TK_VARIABLE {
        if p.u.init_busy {
            p_expr.op = TK_NULL;
        } else {
            p.u.err = Some(FixError::Variable);
            return WRC_ABORT;
        }
    }
    WRC_CONTINUE
}

/// `fixSelectCb`: o callback de SELECT das rotinas `sqlite3FixAAAA()`.
fn fix_select_cb(p: &mut Walker<FixCtx>, p_select: &mut Select) -> i32 {
    let Some(p_list) = p_select.p_src.as_deref_mut() else {
        return WRC_CONTINUE;
    };
    for i in 0..p_list.a.len() {
        if !p.u.fix.b_temp {
            let p_item = &mut p_list.a[i];
            if let Some(z_database) = &p_item.z_database {
                if !name_matches(&p.u.fix.z_db, p.u.i_db, z_database) {
                    p.u.err = Some(FixError::Database(z_database.clone()));
                    return WRC_ABORT;
                }
                p_item.z_database = None;
                p_item.fg.not_cte = true;
            }
            p_item.p_schema = Some(p.u.fix.p_schema);
            p_item.fg.from_ddl = true;
        }
        if !p_list.a[i].fg.is_using {
            if let SrcU3::On(p_on) = &mut p_list.a[i].u3 {
                if walk_expr(p, p_on.as_deref_mut()) != 0 {
                    return WRC_ABORT;
                }
            }
        }
    }
    if let Some(p_with) = p_select.p_with.as_deref_mut() {
        for cte in p_with.a.iter_mut() {
            if walk_select(p, cte.p_select.as_deref_mut()) != 0 {
                return WRC_ABORT;
            }
        }
    }
    WRC_CONTINUE
}

/// `sqlite3FixInit`: inicializa um `DbFixer`. Deve ser chamada antes de passar o fixador a uma
/// das rotinas `sqlite3FixAAAA()`. `z_type` é "view", "trigger" ou "index"; `p_name` é o nome da
/// view, gatilho ou índice.
pub fn fix_init(
    db: &Connection,
    i_db: i32,
    z_type: &'static str,
    p_name: &Token,
) -> Walker<FixCtx> {
    debug_assert!(db.dbs.len() as i32 > i_db);
    let slot = &db.dbs[i_db as usize];
    Walker {
        x_expr_callback: Some(fix_expr_cb),
        x_select_callback: Some(fix_select_cb),
        x_select_callback2: Some(walk_win_defn_dummy_callback::<FixCtx>),
        walker_depth: 0,
        e_code: 0,
        m_w_flags: 0,
        u: FixCtx {
            fix: DbFixer {
                p_schema: slot.schema.id,
                b_temp: i_db == 1,
                z_db: slot.z_db_s_name.clone(),
                z_type,
                p_name: p_name.clone(),
            },
            i_db,
            init_busy: db.init.busy != 0,
            err: None,
        },
    }
}

/// Prepara o `Walker` para um percurso (o `pFix->w.pParse` do C é o `Parse` corrente).
fn fix_begin(parse: &Parse, p_fix: &mut Walker<FixCtx>) {
    p_fix.m_w_flags = if parse.in_rename_object() { WALKER_FLAG_IN_RENAME } else { 0 };
    p_fix.u.err = None;
}

/// Emite a mensagem do erro que um callback deixou em `FixCtx.err` e devolve `rc`.
fn fix_report(db: &mut Connection, parse: &mut Parse, p_fix: &mut Walker<FixCtx>, rc: i32) -> i32 {
    let z_type = PrintfArg::Text(Some(p_fix.u.fix.z_type.as_bytes().to_vec()));
    match p_fix.u.err.take() {
        Some(FixError::Variable) => {
            error_msg(db, parse, b"%s cannot use variables", &[z_type]);
        }
        Some(FixError::Database(z_database)) => {
            error_msg(
                db,
                parse,
                b"%s %T cannot reference objects in database %s",
                &[z_type, token_arg(parse, &p_fix.u.fix.p_name), PrintfArg::Text(Some(z_database))],
            );
        }
        None => {}
    }
    rc
}

/// O percurso de `sqlite3FixSrcList`, sem a mensagem de erro.
fn fix_src_list_walk(p_fix: &mut Walker<FixCtx>, p_list: &mut SrcList) -> i32 {
    let mut s = Select::default();
    s.p_src = Some(Box::new(std::mem::take(p_list)));
    let res = walk_select(p_fix, Some(&mut s));
    if let Some(l) = s.p_src.take() {
        *p_list = *l;
    }
    res
}

/// `sqlite3FixSrcList`: percorre a árvore de sintaxe e atribui um banco específico a toda
/// referência a tabela cujo nome de banco foi omitido no SQL original. `p_fix` deve ter sido
/// inicializado por `fix_init`.
///
/// Estas rotinas garantem que um índice, gatilho ou view de um banco não se refere a objetos de
/// outro banco (exceção: os do banco TEMP podem se referir a qualquer um). Se há referência
/// explícita a um objeto de outro banco, a mensagem vai para `parse` e a rotina devolve diferente
/// de zero; senão devolve 0.
pub fn fix_src_list(
    db: &mut Connection,
    parse: &mut Parse,
    p_fix: &mut Walker<FixCtx>,
    p_list: &mut SrcList,
) -> i32 {
    fix_begin(parse, p_fix);
    let res = fix_src_list_walk(p_fix, p_list);
    fix_report(db, parse, p_fix, res)
}

/// `sqlite3FixSelect`: como `fix_src_list`, para um SELECT.
pub fn fix_select(
    db: &mut Connection,
    parse: &mut Parse,
    p_fix: &mut Walker<FixCtx>,
    p_select: &mut Select,
) -> i32 {
    fix_begin(parse, p_fix);
    let res = walk_select(p_fix, Some(p_select));
    fix_report(db, parse, p_fix, res)
}

/// `sqlite3FixExpr`: como `fix_src_list`, para uma expressão.
pub fn fix_expr(
    db: &mut Connection,
    parse: &mut Parse,
    p_fix: &mut Walker<FixCtx>,
    p_expr: Option<&mut Expr>,
) -> i32 {
    fix_begin(parse, p_fix);
    let res = walk_expr(p_fix, p_expr);
    fix_report(db, parse, p_fix, res)
}

/// O percurso do `Upsert` de um passo do gatilho (o bloco `SQLITE_OMIT_UPSERT` do C).
fn fix_upsert_walk(p_fix: &mut Walker<FixCtx>, mut p_up: Option<&mut Upsert>) -> i32 {
    while let Some(up) = p_up {
        if walk_expr_list(p_fix, up.p_upsert_target.as_deref_mut()) != 0
            || walk_expr(p_fix, up.p_upsert_target_where.as_deref_mut()) != 0
            || walk_expr_list(p_fix, up.p_upsert_set.as_deref_mut()) != 0
            || walk_expr(p_fix, up.p_upsert_where.as_deref_mut()) != 0
        {
            return 1;
        }
        p_up = up.p_next_upsert.as_deref_mut();
    }
    0
}

/// `sqlite3FixTriggerStep`: como `fix_src_list`, para a lista encadeada de passos de um gatilho.
pub fn fix_trigger_step(
    db: &mut Connection,
    parse: &mut Parse,
    p_fix: &mut Walker<FixCtx>,
    mut p_step: Option<&mut TriggerStep>,
) -> i32 {
    fix_begin(parse, p_fix);
    let mut rc = 0;
    while let Some(step) = p_step {
        if walk_select(p_fix, step.p_select.as_deref_mut()) != 0
            || walk_expr(p_fix, step.p_where.as_deref_mut()) != 0
            || walk_expr_list(p_fix, step.p_expr_list.as_deref_mut()) != 0
            || match step.p_from.as_deref_mut() {
                Some(from) => fix_src_list_walk(p_fix, from) != 0,
                None => false,
            }
        {
            rc = 1;
            break;
        }
        if fix_upsert_walk(p_fix, step.p_upsert.as_deref_mut()) != 0 {
            rc = 1;
            break;
        }
        p_step = step.p_next.as_deref_mut();
    }
    fix_report(db, parse, p_fix, rc)
}
