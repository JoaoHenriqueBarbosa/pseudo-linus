// Mesclado das partes traduzidas de attach_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Trecho de uma string C até o primeiro NUL (o `%s` e o `strcmp` do C param ali).
fn attach_cstr_prefix(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// Equivalente do `sqlite3_snprintf(sizeof(zErr), zErr, ...)` com `char zErr[128]`: a mensagem
/// montada é truncada em 127 bytes (o último byte do buffer é o NUL).
fn attach_snprintf_err128(parts: &[&[u8]]) -> Vec<u8> {
    let mut out: Vec<u8> = parts.concat();
    out.truncate(127);
    out
}

/// Igualdade de ponteiro entre dois `Schema` (o `==` do C), com `None` fazendo o papel de NULL.
fn attach_same_schema(a: &Option<SchemaRef>, b: &Option<SchemaRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Resolve uma expressão que fazia parte de um comando ATTACH ou DETACH. Isto é ligeiramente
/// diferente de resolver uma expressão SQL normal, porque identificadores simples são tratados
/// como strings, não possíveis nomes de colunas ou aliases.
///
/// Ou seja, se o analisador vê:
///
///     ATTACH DATABASE abc AS def
///
/// ele trata as duas expressões como strings literais 'abc' e 'def' em vez de procurar por
/// colunas de mesmo nome.
///
/// Isto só se aplica ao nó raiz de pExpr, então o comando:
///
///     ATTACH DATABASE abc||def AS 'db2'
///
/// falhará porque nem abc nem def podem ser resolvidos.
fn resolve_attach_expr(p_name: &mut NameContext, p_expr: &Option<ExprRef>) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(expr_ref) = p_expr {
        let op = expr_ref.borrow().op;
        if op != TK_ID {
            rc = resolve_expr_names(p_name, expr_ref);
        } else {
            expr_ref.borrow_mut().op = TK_STRING;
        }
    }
    rc
}

/// Retorna verdadeiro se zName aponta para um nome que pode ser usado para se referir ao banco
/// de dados iDb anexado ao handle db.
pub fn db_is_named(db: &Sqlite3, i_db: usize, z_name: &[u8]) -> bool {
    let z_db_sname: &[u8] = db.a_db[i_db].z_db_sname.as_deref().unwrap_or(b"");
    str_i_cmp(z_db_sname, z_name) == 0 || (i_db == 0 && str_i_cmp(b"main", z_name) == 0)
}

/// Uma função SQL de usuário registrada para fazer o trabalho de um comando ATTACH. Os três
/// argumentos da função vêm diretamente de um comando attach:
///
///     ATTACH DATABASE x AS y KEY z
///
///     SELECT sqlite_attach(x, y, z)
///
/// Se a sintaxe opcional "KEY z" for omitida, NULL SQL é passado como terceiro argumento.
///
/// Se a flag db->init.reopenMemdb está definida, então em vez de anexar um novo banco de dados,
/// fecha o banco de dados em db->init.iDb e reabre-o como um MemDB vazio.
fn attach_func(context: &mut Sqlite3Context, _not_used: i32, argv: &[Sqlite3ValueRef]) {
    let mut rc: i32 = 0;
    let db: SqliteRef = api::context_db_handle(context);
    let z_file_v: Vec<u8> = api::value_text(&argv[0]).unwrap_or_default();
    let z_name_v: Vec<u8> = api::value_text(&argv[1]).unwrap_or_default();
    let z_file: &[u8] = attach_cstr_prefix(&z_file_v);
    let z_name: &[u8] = attach_cstr_prefix(&z_name_v);
    let mut z_path: Option<Vec<u8>> = None;
    let mut z_err_dyn: Option<Vec<u8>> = None;
    // Índice do `Db` novo em db->aDb[] (o `pNew` do C).
    let mut p_new: usize = 0;

    // SQLITE_OMIT_DESERIALIZE não está definido no Debian: REOPEN_AS_MEMDB(db) é db->init.reopenMemdb.
    let reopen_as_memdb: bool = db.borrow().init.reopen_memdb != 0;

    'attach_error: {
        if reopen_as_memdb {
            // Isto não é um ATTACH real. Em vez disso, esta rotina está sendo chamada de
            // sqlite3_deserialize() para fechar o banco de dados db->init.iDb e reabri-lo
            // como um MemDB
            let p_vfs = match api::vfs_find(Some(b"memdb")) {
                Some(v) => v,
                None => return,
            };
            let mut p_new_bt: Option<BtreeRef> = None;
            rc = btree_open(&p_vfs, b"x\0", &db, &mut p_new_bt, 0, SQLITE_OPEN_MAIN_DB);
            if rc == SQLITE_OK {
                let new_bt = p_new_bt.clone().expect("btree_open OK sem Btree");
                let p_new_schema = schema_get(&db, &new_bt);
                if let Some(new_schema) = p_new_schema {
                    // Tanto a Btree como o novo Schema foram alocados com sucesso.
                    // Fecha o db antigo e atualiza o slot aDb[] com os novos valores de memdb.
                    p_new = db.borrow().init.i_db as usize;
                    let old_bt = db.borrow_mut().a_db[p_new].p_bt.take();
                    if let Some(old_bt) = old_bt {
                        btree_close(old_bt);
                    }
                    let mut db_mut = db.borrow_mut();
                    db_mut.a_db[p_new].p_bt = Some(new_bt);
                    db_mut.a_db[p_new].p_schema = Some(new_schema);
                } else {
                    btree_close(new_bt);
                    rc = SQLITE_NOMEM;
                }
            }
            if rc != 0 {
                break 'attach_error;
            }
        } else {
            // Isto é um ATTACH real
            //
            // Verifica os seguintes erros:
            //
            //     * Muitos bancos de dados anexados,
            //     * Transação atualmente aberta
            //     * Nome de banco de dados especificado já em uso.
            let (n_db, limit_attached) = {
                let db_ref = db.borrow();
                (db_ref.n_db, db_ref.a_limit[SQLITE_LIMIT_ATTACHED as usize])
            };
            if n_db >= limit_attached + 2 {
                z_err_dyn = Some(
                    [
                        b"too many attached databases - max ".as_slice(),
                        limit_attached.to_string().as_bytes(),
                    ]
                    .concat(),
                );
                break 'attach_error;
            }
            for i in 0..n_db as usize {
                let named = db_is_named(&db.borrow(), i, z_name);
                if named {
                    z_err_dyn = Some([b"database ".as_slice(), z_name, b" is already in use"].concat());
                    break 'attach_error;
                }
            }

            // Aloca a nova entrada no array db->aDb[] e inicializa as tabelas hash de esquema.
            // O Vec substitui o aDbStatic e o realloc; descartar o que passa de nDb equivale ao
            // memset(pNew, 0) do C sobre uma entrada deixada por um ATTACH que falhou.
            let (open_flags, default_vfs_name) = {
                let mut db_mut = db.borrow_mut();
                let n_db_now = db_mut.n_db as usize;
                db_mut.a_db.truncate(n_db_now);
                db_mut.a_db.push(Db::default());
                (db_mut.open_flags, db_mut.p_vfs.z_name().to_vec())
            };
            p_new = n_db as usize;

            // Abre o arquivo de banco de dados. Se a btree for aberta com sucesso, usa-a
            // para obter o esquema do banco de dados. Neste ponto o esquema pode ou não
            // estar inicializado.
            let mut flags: u32 = open_flags;
            let mut p_vfs_out: Option<VfsRef> = None;
            let mut z_err: Option<Vec<u8>> = None;
            rc = parse_uri(&default_vfs_name, z_file, &mut flags, &mut p_vfs_out, &mut z_path, &mut z_err);
            if rc != SQLITE_OK {
                if rc == SQLITE_NOMEM {
                    oom_fault(&db);
                }
                api::result_error(context, z_err.as_deref(), -1);
                return;
            }
            let p_vfs = p_vfs_out.expect("parse_uri OK sem VFS");
            flags |= SQLITE_OPEN_MAIN_DB as u32;
            let mut p_new_bt: Option<BtreeRef> = None;
            rc = btree_open(
                &p_vfs,
                attach_cstr_prefix(z_path.as_deref().unwrap_or(b"")),
                &db,
                &mut p_new_bt,
                0,
                flags as i32,
            );
            let z_db_sname = db_str_dup(&db, z_name);
            let mut db_mut = db.borrow_mut();
            db_mut.a_db[p_new].p_bt = p_new_bt;
            db_mut.n_db += 1;
            db_mut.a_db[p_new].z_db_sname = z_db_sname;
        }
        db.borrow_mut().no_shared_cache = 0;
        if rc == SQLITE_CONSTRAINT {
            rc = SQLITE_ERROR;
            z_err_dyn = Some(b"database is already attached".to_vec());
        } else if rc == SQLITE_OK {
            let p_bt: BtreeRef = db.borrow().a_db[p_new].p_bt.clone().expect("Btree aberta sem p_bt");
            let p_schema = schema_get(&db, &p_bt);
            db.borrow_mut().a_db[p_new].p_schema = p_schema.clone();
            match &p_schema {
                None => {
                    rc = SQLITE_NOMEM_BKPT;
                }
                Some(schema) => {
                    let (file_format, schema_enc) = {
                        let s = schema.borrow();
                        (s.file_format, s.enc)
                    };
                    if file_format != 0 && schema_enc != enc(&db.borrow()) {
                        z_err_dyn = Some(
                            b"attached databases must use the same text encoding as main database".to_vec(),
                        );
                        rc = SQLITE_ERROR;
                    }
                }
            }
            btree_enter(&p_bt);
            let p_pager = btree_pager(&p_bt);
            let dflt_lock_mode = db.borrow().dflt_lock_mode;
            pager_locking_mode(&p_pager, dflt_lock_mode as i32);
            let main_bt: BtreeRef = db.borrow().a_db[0].p_bt.clone().expect("banco main sem Btree");
            let main_secure_delete = btree_secure_delete(&main_bt, -1);
            btree_secure_delete(&p_bt, main_secure_delete);
            // SQLITE_OMIT_PAGER_PRAGMAS não está definido no Debian.
            let db_flags = db.borrow().flags;
            btree_set_pager_flags(
                &p_bt,
                PAGER_SYNCHRONOUS_FULL | ((db_flags & PAGER_FLAGS_MASK as u64) as u32),
            );
            btree_leave(&p_bt);
        }
        let name_missing = {
            let mut db_mut = db.borrow_mut();
            db_mut.a_db[p_new].safety_level = (SQLITE_DEFAULT_SYNCHRONOUS + 1) as u8;
            db_mut.a_db[p_new].z_db_sname.is_none()
        };
        if rc == SQLITE_OK && name_missing {
            rc = SQLITE_NOMEM_BKPT;
        }
        api::free_filename(z_path.take());

        // Se o arquivo foi aberto com sucesso, lê o esquema para o novo banco de dados.
        // Se isto falhar, ou se abrir o arquivo falhar, então fecha o arquivo e remove
        // a entrada do array db->aDb[]. Ou seja, coloca tudo de volta do jeito que estava.
        if rc == SQLITE_OK {
            btree_enter_all(&db);
            {
                let mut db_mut = db.borrow_mut();
                db_mut.init.i_db = 0;
                db_mut.m_db_flags &= !DBFLAG_SCHEMA_KNOWN_OK;
            }
            if !reopen_as_memdb {
                rc = init(&db, &mut z_err_dyn);
            }
            btree_leave_all(&db);
            debug_assert!(z_err_dyn.is_none() || rc != SQLITE_OK);
        }
        // SQLITE_USER_AUTHENTICATION não está definido no Debian: o ramo some.
        if rc != 0 {
            if !reopen_as_memdb {
                let i_db = db.borrow().n_db - 1;
                debug_assert!(i_db >= 2);
                let old_bt = {
                    let mut db_mut = db.borrow_mut();
                    let old_bt = db_mut.a_db[i_db as usize].p_bt.take();
                    if old_bt.is_some() {
                        db_mut.a_db[i_db as usize].p_schema = None;
                    }
                    old_bt
                };
                if let Some(old_bt) = old_bt {
                    btree_close(old_bt);
                }
                reset_all_schemas_of_connection(&db);
                db.borrow_mut().n_db = i_db;
                if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                    oom_fault(&db);
                    z_err_dyn = Some(b"out of memory".to_vec());
                } else if z_err_dyn.is_none() {
                    z_err_dyn = Some([b"unable to open database: ".as_slice(), z_file].concat());
                }
            }
            break 'attach_error;
        }

        return;
    }

    // attach_error: retorna um erro se chegamos aqui
    if let Some(err) = z_err_dyn {
        api::result_error(context, Some(&err), -1);
    }
    if rc != 0 {
        api::result_error_code(context, rc);
    }
}

/// Uma função SQL de usuário registrada para fazer o trabalho de um comando DETACH. Os três
/// argumentos da função vêm diretamente de um comando detach:
///
///     DETACH DATABASE x
///
///     SELECT sqlite_detach(x)
fn detach_func(context: &mut Sqlite3Context, _not_used: i32, argv: &[Sqlite3ValueRef]) {
    let z_name_v: Vec<u8> = api::value_text(&argv[0]).unwrap_or_default();
    let z_name: &[u8] = attach_cstr_prefix(&z_name_v);
    let db: SqliteRef = api::context_db_handle(context);

    let (n_db, found) = {
        let db_ref = db.borrow();
        let n_db = db_ref.n_db as usize;
        let mut found: Option<usize> = None;
        for i in 0..n_db {
            if db_ref.a_db[i].p_bt.is_none() {
                continue;
            }
            if db_is_named(&db_ref, i, z_name) {
                found = Some(i);
                break;
            }
        }
        (n_db, found)
    };

    // `detach_error` do C: o buffer zErr tem 128 bytes.
    let z_err: Vec<u8> = 'detach_error: {
        let i = match found {
            Some(i) if i < n_db => i,
            _ => break 'detach_error attach_snprintf_err128(&[b"no such database: ", z_name]),
        };
        if i < 2 {
            break 'detach_error attach_snprintf_err128(&[b"cannot detach database ", z_name]);
        }
        let p_bt: BtreeRef = db.borrow().a_db[i].p_bt.clone().expect("banco anexado sem Btree");
        if btree_txn_state(&p_bt) != SQLITE_TXN_NONE || btree_is_in_backup(&p_bt) {
            break 'detach_error attach_snprintf_err128(&[b"database ", z_name, b" is locked"]);
        }

        // Se algum trigger TEMP referencia o esquema sendo desanexado, move esses triggers
        // para referenciar o próprio esquema TEMP.
        let temp_schema: SchemaRef = db.borrow().a_db[1].p_schema.clone().expect("banco temp sem Schema");
        let p_db_schema: Option<SchemaRef> = db.borrow().a_db[i].p_schema.clone();
        let mut p_entry = hash_first(&temp_schema.borrow().trig_hash);
        while let Some(entry) = p_entry {
            let p_trig: TriggerRef = hash_data(&temp_schema.borrow().trig_hash, &entry);
            {
                let mut trig = p_trig.borrow_mut();
                if attach_same_schema(&trig.p_tab_schema, &p_db_schema) {
                    trig.p_tab_schema = trig.p_schema.clone();
                }
            }
            p_entry = hash_next(&temp_schema.borrow().trig_hash, &entry);
        }

        let old_bt = {
            let mut db_mut = db.borrow_mut();
            let old_bt = db_mut.a_db[i].p_bt.take();
            db_mut.a_db[i].p_schema = None;
            old_bt
        };
        if let Some(old_bt) = old_bt {
            btree_close(old_bt);
        }
        collapse_database_array(&db);
        return;
    };
    api::result_error(context, Some(&z_err), -1);
}

/// Esta rotina gera código VDBE para uma única invocação de qualquer uma das funções SQL de
/// usuário sqlite_detach() ou sqlite_attach().
fn code_attach(
    p_parse: &ParseRef,
    type_: i32,
    p_func: &FuncDef,
    p_auth_arg: Option<&ExprRef>,
    p_filename: Option<ExprRef>,
    p_dbname: Option<ExprRef>,
    p_key: Option<ExprRef>,
) {
    let db: SqliteRef = p_parse.borrow().db.upgrade().expect("Parse sem conexão");

    'attach_end: {
        if SQLITE_OK != read_schema(p_parse) {
            break 'attach_end;
        }

        if p_parse.borrow().n_err != 0 {
            break 'attach_end;
        }
        let mut s_name = NameContext::default();
        s_name.p_parse = Some(p_parse.clone());

        if SQLITE_OK != resolve_attach_expr(&mut s_name, &p_filename)
            || SQLITE_OK != resolve_attach_expr(&mut s_name, &p_dbname)
            || SQLITE_OK != resolve_attach_expr(&mut s_name, &p_key)
        {
            break 'attach_end;
        }

        // SQLITE_OMIT_AUTHORIZATION não está definido no Debian. O ALWAYS(pAuthArg) vale.
        if let Some(auth_arg) = p_auth_arg {
            let z_auth_arg: Option<Vec<u8>> = {
                let expr = auth_arg.borrow();
                if expr.op == TK_STRING {
                    expr.u.z_token.clone()
                } else {
                    None
                }
            };
            let rc = auth_check(p_parse, type_, z_auth_arg.as_deref(), None, None);
            if rc != SQLITE_OK {
                break 'attach_end;
            }
        }

        let v = get_vdbe(p_parse);
        let reg_args = get_temp_range(p_parse, 4);
        expr_code(p_parse, p_filename.as_ref(), reg_args);
        expr_code(p_parse, p_dbname.as_ref(), reg_args + 1);
        expr_code(p_parse, p_key.as_ref(), reg_args + 2);

        debug_assert!(v.is_some() || db.borrow().malloc_failed != 0);
        if let Some(v) = v {
            vdbe_add_function_call(
                p_parse,
                0,
                reg_args + 3 - p_func.n_arg as i32,
                reg_args + 3,
                p_func.n_arg as i32,
                p_func,
                0,
            );
            // Código um OP_Expire. Para um comando ATTACH, define P1 como verdadeiro (expira
            // apenas esta declaração). Para DETACH, define como falso (expira todas as
            // declarações existentes).
            vdbe_add_op1(&v, OP_EXPIRE, (type_ == SQLITE_ATTACH) as i32);
        }
    }

    // attach_end:
    expr_delete(&db, p_filename);
    expr_delete(&db, p_dbname);
    expr_delete(&db, p_key);
}


// ---- part_001.rs ----

/// Chamada pelo analisador para compilar uma instrução DETACH.
///
///     DETACH pDbname
pub fn detach(p_parse: &ParseRef, p_dbname: Option<ExprRef>) {
    // O `static const FuncDef detach_func` do C é montado a cada chamada.
    let detach_def = FuncDef {
        n_arg: 1,
        func_flags: SQLITE_UTF8 as u32,
        p_user_data: None,
        p_next: None,
        x_s_func: Some(Rc::new(|ctx: &mut Sqlite3Context, argv: &[Sqlite3ValueRef]| {
            detach_func(ctx, argv.len() as i32, argv)
        })),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: b"sqlite_detach".to_vec(),
        u: FuncDefU::PHash(None),
    };
    code_attach(p_parse, SQLITE_DETACH, &detach_def, p_dbname.as_ref(), None, None, p_dbname.clone());
}

/// Chamada pelo analisador para compilar uma instrução ATTACH.
///
///     ATTACH p AS pDbname KEY pKey
pub fn attach(p_parse: &ParseRef, p: Option<ExprRef>, p_dbname: Option<ExprRef>, p_key: Option<ExprRef>) {
    // O `static const FuncDef attach_func` do C é montado a cada chamada.
    let attach_def = FuncDef {
        n_arg: 3,
        func_flags: SQLITE_UTF8 as u32,
        p_user_data: None,
        p_next: None,
        x_s_func: Some(Rc::new(|ctx: &mut Sqlite3Context, argv: &[Sqlite3ValueRef]| {
            attach_func(ctx, argv.len() as i32, argv)
        })),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: b"sqlite_attach".to_vec(),
        u: FuncDefU::PHash(None),
    };
    code_attach(p_parse, SQLITE_ATTACH, &attach_def, p.as_ref(), p.clone(), p_dbname, p_key);
}

/// Dobra os `%` de um argumento de formato. O `sqlite3ErrorMsg` do C recebe os argumentos
/// separados do formato; aqui a mensagem chega já montada e é lida de novo como formato,
/// então um `%` vindo de nome de objeto precisa sair como `%%` para continuar literal.
fn fix_escape_format_arg(z: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(z.len());
    for &c in z {
        if c == b'%' {
            out.push(b'%');
        }
        out.push(c);
    }
    out
}

/// Callback de expressão usado pelas rotinas sqlite3FixAAAA().
///
/// O `DbFixer` do C é lido por `p->u.pFix`; aqui `p.u` carrega uma cópia dos campos de leitura
/// do fixer (ver `fix_init`).
fn fix_expr_cb(p: &mut Walker, p_expr: &mut Expr) -> i32 {
    let (b_temp, p_parse, z_type) = match &p.u {
        WalkerU::Fix(p_fix) => (p_fix.b_temp, p_fix.p_parse.clone(), p_fix.z_type.clone()),
        _ => unreachable!(),
    };
    if b_temp == 0 {
        expr_set_property(p_expr, EP_FROM_DDL);
    }
    if p_expr.op == TK_VARIABLE {
        let p_parse = p_parse.expect("DbFixer sem Parse");
        let db = p_parse.borrow().db.upgrade().expect("Parse sem conexão");
        let busy = db.borrow().init.busy;
        if busy != 0 {
            p_expr.op = TK_NULL;
        } else {
            let mut z_msg = fix_escape_format_arg(&z_type);
            z_msg.extend_from_slice(b" cannot use variables");
            error_msg(&mut p_parse.borrow_mut(), Some(&z_msg));
            return WRC_ABORT;
        }
    }
    WRC_CONTINUE
}

/// Callback de SELECT usado pelas rotinas sqlite3FixAAAA().
fn fix_select_cb(p: &mut Walker, p_select: &mut Select) -> i32 {
    let (b_temp, p_parse, p_schema, z_db, z_type, p_name) = match &p.u {
        WalkerU::Fix(p_fix) => (
            p_fix.b_temp,
            p_fix.p_parse.clone(),
            p_fix.p_schema.clone(),
            p_fix.z_db.clone(),
            p_fix.z_type.clone(),
            p_fix.p_name.clone(),
        ),
        _ => unreachable!(),
    };
    let p_parse = p_parse.expect("DbFixer sem Parse");
    let db = p_parse.borrow().db.upgrade().expect("Parse sem conexão");
    let i_db = find_db_name(&db.borrow(), &z_db);

    // NEVER(pList==0)
    let p_list = match p_select.p_src.as_mut() {
        Some(l) => l,
        None => return WRC_CONTINUE,
    };
    for i in 0..p_list.n_src as usize {
        if b_temp == 0 {
            if !p_list.a[i].z_database.is_empty() {
                if i_db != find_db_name(&db.borrow(), &p_list.a[i].z_database) {
                    let mut z_msg = fix_escape_format_arg(&z_type);
                    z_msg.push(b' ');
                    if let Some(tok) = &p_name {
                        let n = (tok.n as usize).min(tok.z.len());
                        z_msg.extend_from_slice(&fix_escape_format_arg(&tok.z[..n]));
                    }
                    z_msg.extend_from_slice(b" cannot reference objects in database ");
                    z_msg.extend_from_slice(&fix_escape_format_arg(&p_list.a[i].z_database));
                    error_msg(&mut p_parse.borrow_mut(), Some(&z_msg));
                    return WRC_ABORT;
                }
                p_list.a[i].z_database = Vec::new();
                p_list.a[i].fg.not_cte = 1;
            }
            p_list.a[i].p_schema = p_schema.clone();
            p_list.a[i].fg.from_ddl = 1;
        }
        // #if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_TRIGGER): vale no Debian.
        // O C chama sqlite3WalkExpr(&pFix->w, ...); como toda caminhada do fixer parte de
        // &pFix->w, esse walker é o próprio `p`.
        if p_list.a[i].fg.is_using == 0 {
            if let SrcItemU3::On(p_on) = &p_list.a[i].u3 {
                if walk_expr(p, p_on) != 0 {
                    return WRC_ABORT;
                }
            }
        }
    }
    if let Some(p_with) = &p_select.p_with {
        let p_with = p_with.borrow();
        for i in 0..p_with.n_cte as usize {
            if let Some(p_cte_select) = &p_with.a[i].p_select {
                if walk_select(p, p_cte_select) != 0 {
                    return WRC_ABORT;
                }
            }
        }
    }
    WRC_CONTINUE
}

/// Inicializa uma estrutura DbFixer. Esta rotina deve ser chamada antes de passar a estrutura
/// a uma das rotinas sqliteFixAAAA() abaixo.
pub fn fix_init(p_fix: &mut DbFixer, p_parse: &ParseRef, i_db: i32, z_type: &[u8], p_name: Option<&Token>) {
    let db = p_parse.borrow().db.upgrade().expect("Parse sem conexão");
    let db = db.borrow();
    debug_assert!(db.n_db > i_db);
    p_fix.p_parse = Some(p_parse.clone());
    p_fix.z_db = db.a_db[i_db as usize].z_db_sname.clone().unwrap_or_default();
    p_fix.p_schema = db.a_db[i_db as usize].p_schema.clone();
    p_fix.z_type = z_type.to_vec();
    p_fix.p_name = p_name.cloned();
    p_fix.b_temp = (i_db == 1) as u8;
    p_fix.w.p_parse = Some(p_parse.clone());
    p_fix.w.x_expr_callback = Some(fix_expr_cb);
    p_fix.w.x_select_callback = Some(fix_select_cb);
    p_fix.w.x_select_callback2 = Some(walk_win_defn_dummy_callback);
    p_fix.w.walker_depth = 0;
    p_fix.w.e_code = 0;
    // `pFix->w.u.pFix = pFix` do C é um ponteiro para si mesmo. Em Rust, `u` guarda uma cópia
    // dos campos de leitura do fixer (os callbacks só os leem).
    p_fix.w.u = WalkerU::Fix(Box::new(DbFixer {
        p_parse: p_fix.p_parse.clone(),
        w: Walker {
            p_parse: p_fix.p_parse.clone(),
            x_expr_callback: None,
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: WalkerU::None,
        },
        p_schema: p_fix.p_schema.clone(),
        b_temp: p_fix.b_temp,
        z_db: p_fix.z_db.clone(),
        z_type: p_fix.z_type.clone(),
        p_name: p_fix.p_name.clone(),
    }));
}

/// O conjunto de rotinas a seguir percorre a árvore de análise e atribui um banco específico a
/// todas as referências a tabela em que o nome do banco foi omitido no SQL original. A
/// estrutura pFix deve ter sido inicializada por uma chamada anterior a fix_init().
///
/// Estas rotinas garantem que um índice, trigger ou view de um banco não refira objetos de
/// outro banco (exceção: índices, triggers e views do banco TEMP podem referir qualquer coisa).
/// Se uma referência explícita é feita a um objeto de outro banco, uma mensagem de erro é
/// adicionada a pParse->zErrMsg e as rotinas retornam diferente de zero. Se tudo confere,
/// retornam 0.
pub fn fix_src_list(p_fix: &mut DbFixer, p_list: &mut Option<Box<SrcList>>) -> i32 {
    let mut res = 0;
    if p_list.is_some() {
        let mut s = Select {
            op: 0,
            n_select_row: 0,
            sel_flags: 0,
            i_limit: 0,
            i_offset: 0,
            sel_id: 0,
            addr_open_ephm: [0; 2],
            p_elist: None,
            p_src: p_list.take(),
            p_where: None,
            p_group_by: None,
            p_having: None,
            p_order_by: None,
            p_prior: None,
            p_next: None,
            p_limit: None,
            p_with: None,
            p_win: None,
            p_win_defn: None,
        };
        res = walk_select(&mut p_fix.w, &s);
        *p_list = s.p_src.take();
    }
    res
}

// #if !defined(SQLITE_OMIT_VIEW) || !defined(SQLITE_OMIT_TRIGGER): vale no Debian.
pub fn fix_select(p_fix: &mut DbFixer, p_select: &Select) -> i32 {
    walk_select(&mut p_fix.w, p_select)
}

pub fn fix_expr(p_fix: &mut DbFixer, p_expr: &Expr) -> i32 {
    walk_expr(&mut p_fix.w, p_expr)
}

// #ifndef SQLITE_OMIT_TRIGGER: vale no Debian.
pub fn fix_trigger_step(p_fix: &mut DbFixer, p_step: Option<&mut TriggerStep>) -> i32 {
    let mut cur = p_step;
    while let Some(step) = cur {
        if let Some(p_select) = step.p_select.as_deref() {
            if walk_select(&mut p_fix.w, p_select) != 0 {
                return 1;
            }
        }
        if let Some(p_where) = step.p_where.as_deref() {
            if walk_expr(&mut p_fix.w, p_where) != 0 {
                return 1;
            }
        }
        if let Some(p_expr_list) = step.p_expr_list.as_deref() {
            if walk_expr_list(&mut p_fix.w, p_expr_list) != 0 {
                return 1;
            }
        }
        if fix_src_list(p_fix, &mut step.p_from) != 0 {
            return 1;
        }
        // #ifndef SQLITE_OMIT_UPSERT: vale no Debian.
        {
            let mut p_up = step.p_upsert.as_deref();
            while let Some(up) = p_up {
                if let Some(l) = up.p_upsert_target.as_deref() {
                    if walk_expr_list(&mut p_fix.w, l) != 0 {
                        return 1;
                    }
                }
                if let Some(e) = up.p_upsert_target_where.as_deref() {
                    if walk_expr(&mut p_fix.w, e) != 0 {
                        return 1;
                    }
                }
                if let Some(l) = up.p_upsert_set.as_deref() {
                    if walk_expr_list(&mut p_fix.w, l) != 0 {
                        return 1;
                    }
                }
                if let Some(e) = up.p_upsert_where.as_deref() {
                    if walk_expr(&mut p_fix.w, e) != 0 {
                        return 1;
                    }
                }
                p_up = up.p_next_upsert.as_deref();
            }
        }
        cur = step.p_next.as_deref_mut();
    }
    0
}

