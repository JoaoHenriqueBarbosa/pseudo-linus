// Mesclado das partes traduzidas de build_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Estrutura usada apenas por `lock_table()` e `code_table_locks()`: uma trava
/// de tabela desejada em tempo de execução (cache compartilhado).
#[derive(Clone, Debug, Default)]
pub struct TableLock {
    /// O banco de dados contendo a tabela a ser travada
    pub i_db: i32,
    /// A página raiz da tabela a ser travada
    pub i_tab: Pgno,
    /// Verdadeiro para trava de escrita. Falso para trava de leitura
    pub is_write_lock: u8,
    /// Nome da tabela
    pub z_lock_name: Vec<u8>,
}

/// Registra o fato de que desejamos travar uma tabela em tempo de execução.
///
/// A tabela a ser travada tem página raiz `i_tab` e é encontrada no banco
/// `i_db`. Uma trava de leitura ou de escrita pode ser obtida conforme
/// `is_write_lock`. Esta rotina apenas registra o desejo: o código que faz a
/// trava acontecer é gerado depois por `code_table_locks()`, durante
/// `finish_coding()`.
fn lock_table(p_parse: &mut Parse, i_db: i32, i_tab: Pgno, is_write_lock: u8, z_name: &[u8]) {
    debug_assert!(i_db >= 0);

    // O Parse de nível mais alto guarda a lista de travas.
    let p_toplevel = p_parse.p_toplevel.clone();
    match p_toplevel {
        Some(top) => lock_table_on(&mut top.borrow_mut(), i_db, i_tab, is_write_lock, z_name),
        None => lock_table_on(p_parse, i_db, i_tab, is_write_lock, z_name),
    }
}

/// Corpo de `lock_table()` sobre o Parse de nível mais alto já resolvido
/// (`pToplevel` do C).
fn lock_table_on(p_toplevel: &mut Parse, i_db: i32, i_tab: Pgno, is_write_lock: u8, z_name: &[u8]) {
    for i in 0..p_toplevel.n_table_lock as usize {
        let p = &mut p_toplevel.a_table_lock[i];
        if p.i_db == i_db && p.i_tab == i_tab {
            p.is_write_lock = u8::from(p.is_write_lock != 0 || is_write_lock != 0);
            return;
        }
    }

    // Em Rust a realocação de `aTableLock` não falha: o ramo de OOM do C
    // (`nTableLock = 0` + `sqlite3OomFault`) não existe.
    p_toplevel.a_table_lock.truncate(p_toplevel.n_table_lock as usize);
    p_toplevel.a_table_lock.push(TableLock {
        i_db,
        i_tab,
        is_write_lock,
        z_lock_name: z_name.to_vec(),
    });
    p_toplevel.n_table_lock += 1;
}

/// Registra o fato de que desejamos travar uma tabela em tempo de execução
/// (só vale quando o banco é compartilhável e não é o `temp`).
pub fn table_lock(p_parse: &mut Parse, i_db: i32, i_tab: Pgno, is_write_lock: u8, z_name: &[u8]) {
    if i_db == 1 {
        return;
    }
    let db = p_parse.db.upgrade().unwrap();
    if !btree_sharable(db.borrow().a_db[i_db as usize].p_bt.as_ref()) {
        return;
    }
    lock_table(p_parse, i_db, i_tab, is_write_lock, z_name);
}

/// Codifica uma instrução OP_TableLock para cada tabela travada pela instrução
/// (configurada por chamadas a `table_lock()`).
fn code_table_locks(p_parse: &mut Parse) {
    let v = p_parse.p_vdbe.clone().unwrap();
    for i in 0..p_parse.n_table_lock as usize {
        let p = &p_parse.a_table_lock[i];
        let p1 = p.i_db;
        vdbe_add_op4(
            &v,
            OP_TABLELOCK,
            p1,
            p.i_tab as i32,
            p.is_write_lock as i32,
            P4Arg::Static(p.z_lock_name.clone()),
        );
    }
}

/// Esta rotina é chamada depois que uma única instrução SQL foi analisada e um
/// programa VDBE para executá-la foi preparado. Ela dá os toques finais no
/// programa VDBE e reinicia a estrutura `Parse` para a próxima análise.
///
/// Se um erro ocorreu, pode ser que nenhum código VDBE tenha sido gerado.
pub fn finish_coding(p_parse: &mut Parse) {
    debug_assert!(p_parse.p_toplevel.is_none());
    let db = p_parse.db.upgrade().unwrap();
    if p_parse.nested != 0 {
        return;
    }
    if p_parse.n_err != 0 {
        if db.borrow().malloc_failed != 0 {
            p_parse.rc = SQLITE_NOMEM;
        }
        return;
    }

    // Começa gerando algum código de término no final do programa VDBE
    let mut v = p_parse.p_vdbe.clone();
    if v.is_none() {
        if db.borrow().init.busy != 0 {
            p_parse.rc = SQLITE_DONE;
            return;
        }
        v = get_vdbe(p_parse);
        if v.is_none() {
            p_parse.rc = SQLITE_ERROR;
        }
    }
    if let Some(v) = &v {
        if p_parse.b_returning != 0 {
            let p_returning = p_parse.u1.p_returning.clone().unwrap();
            let (n_ret_col, i_ret_cur, i_ret_reg) = {
                let r = p_returning.borrow();
                (r.n_ret_col, r.i_ret_cur, r.i_ret_reg)
            };
            if n_ret_col != 0 {
                vdbe_add_op0(v, OP_FKCHECK);
                let addr_rewind = vdbe_add_op1(v, OP_REWIND, i_ret_cur);
                let reg = i_ret_reg;
                let mut i = 0;
                while i < n_ret_col {
                    vdbe_add_op3(v, OP_COLUMN, i_ret_cur, i, reg + i);
                    i += 1;
                }
                vdbe_add_op2(v, OP_RESULTROW, reg, i);
                vdbe_add_op2(v, OP_NEXT, i_ret_cur, addr_rewind + 1);
                vdbe_jump_here(v, addr_rewind);
            }
        }
        vdbe_add_op0(v, OP_HALT);

        // A máscara de cookies tem um bit para cada arquivo de banco aberto
        // (bit 0 é main, bit 1 é temp, e assim por diante). Os bits são
        // ligados para cada banco usado. Gera o código que inicia uma
        // transação em cada banco usado e verifica o cookie de esquema.
        vdbe_jump_here(v, 0);
        let mut i_db: i32 = 0;
        loop {
            if db_mask_test(&p_parse.cookie_mask, i_db) {
                vdbe_uses_btree(v, i_db);
                let (schema_cookie, i_generation) = {
                    let d = db.borrow();
                    let s = d.a_db[i_db as usize].p_schema.as_ref().unwrap().borrow();
                    (s.schema_cookie, s.i_generation)
                };
                vdbe_add_op4_int(
                    v,
                    OP_TRANSACTION,
                    i_db,
                    i32::from(db_mask_test(&p_parse.write_mask, i_db)),
                    schema_cookie,
                    i_generation,
                );
                if db.borrow().init.busy == 0 {
                    vdbe_change_p5(v, 1);
                }
            }
            i_db += 1;
            if i_db >= db.borrow().n_db {
                break;
            }
        }

        for i in 0..p_parse.n_vtab_lock as usize {
            let p_tab = p_parse.ap_vtab_lock[i].clone();
            let vtab = get_vtable(&db.borrow(), &p_tab);
            vdbe_add_op4(v, OP_VBEGIN, 0, 0, 0, P4Arg::Vtab(vtab));
        }
        p_parse.n_vtab_lock = 0;

        // Depois que todos os cookies foram verificados e as transações
        // abertas, obtém as travas de tabela necessárias. Sem efeito a menos
        // que o cache compartilhado esteja habilitado.
        if p_parse.n_table_lock != 0 {
            code_table_locks(p_parse);
        }

        // Inicializa qualquer estrutura AUTOINCREMENT necessária
        if p_parse.p_ainc.is_some() {
            autoincrement_begin(p_parse);
        }

        // Codifica as expressões constantes que foram fatoradas de laços
        // internos. A lista é retirada do Parse durante o laço (okConstFactor
        // fica em 0, então nada é acrescentado a ela) e devolvida em seguida.
        if let Some(p_el) = p_parse.p_const_expr.take() {
            p_parse.ok_const_factor = 0;
            for i in 0..p_el.n_expr as usize {
                debug_assert!(p_el.a[i].u_i_const_expr_reg > 0);
                expr_code(p_parse, p_el.a[i].p_expr.as_deref(), p_el.a[i].u_i_const_expr_reg);
            }
            p_parse.p_const_expr = Some(p_el);
        }

        if p_parse.b_returning != 0 {
            let p_ret = p_parse.u1.p_returning.clone().unwrap();
            let (n_ret_col, i_ret_cur) = {
                let r = p_ret.borrow();
                (r.n_ret_col, r.i_ret_cur)
            };
            if n_ret_col != 0 {
                vdbe_add_op2(v, OP_OPENEPHEMERAL, i_ret_cur, n_ret_col);
            }
        }

        // Por fim, salta de volta para o início do código executável.
        vdbe_goto(v, 1);
    }

    // Deixa o programa VDBE pronto para execução
    if p_parse.n_err == 0 {
        debug_assert!(p_parse.p_ainc.is_none() || p_parse.n_tab > 0);
        if let Some(v) = &v {
            vdbe_make_ready(v, p_parse);
        }
        p_parse.rc = SQLITE_DONE;
    } else {
        p_parse.rc = SQLITE_ERROR;
    }
}

/// Executa o analisador e o gerador de código recursivamente para gerar código
/// da instrução SQL formatada, anexada ao fim do contexto `Parse` em
/// construção. O `OP_Halt` final não é anexado e as outras etapas de
/// inicialização e finalização são omitidas (ficam com o analisador mais
/// externo). Funções SQL embutidas sempre têm precedência sobre as definidas
/// pela aplicação.
pub fn nested_parse(p_parse: &mut Parse, z_format: &[u8], args: &[PrintfArg]) {
    let db = p_parse.db.upgrade().unwrap();
    let saved_db_flags = db.borrow().m_db_flags;

    if p_parse.n_err != 0 {
        return;
    }
    if p_parse.e_parse_mode != 0 {
        return;
    }
    debug_assert!(p_parse.nested < 10); // A aninhagem tem profundidade limitada
    let z_sql = match vm_printf(&db.borrow(), z_format, args) {
        Some(z) => z,
        None => {
            // Pode vir de OOM ou de a string formatada exceder
            // SQLITE_LIMIT_LENGTH. No segundo caso precisamos marcar um erro.
            if db.borrow().malloc_failed == 0 {
                p_parse.rc = SQLITE_TOOBIG;
            }
            p_parse.n_err += 1;
            return;
        }
    };
    p_parse.nested += 1;
    // memcpy(saveBuf, PARSE_TAIL) + memset(PARSE_TAIL, 0): `take` deixa o valor padrão (zerado).
    let save_buf = std::mem::take(&mut p_parse.tail);
    db.borrow_mut().m_db_flags |= DBFLAG_PREFER_BUILTIN;
    run_parser(p_parse, &z_sql);
    db.borrow_mut().m_db_flags = saved_db_flags;
    p_parse.tail = save_buf;
    p_parse.nested -= 1;
}

/// Localiza a estrutura em memória que descreve uma tabela particular dado o
/// nome da tabela e (opcionalmente) o nome do banco que a contém. Devolve
/// `None` se não encontrada.
///
/// Se `z_database` é `None`, todos os bancos são pesquisados e a primeira
/// tabela que casar é devolvida (sem checar nomes duplicados). A ordem de busca
/// é TEMP primeiro, depois MAIN, depois os bancos auxiliares do ATTACH.
///
/// Veja também `locate_table()`.
pub fn find_table(db: &Sqlite3, z_name: &[u8], z_database: Option<&[u8]>) -> Option<TableRef> {
    // O código SQLITE_USER_AUTHENTICATION não é compilado no Debian.
    let tbl_find = |i: usize, name: &[u8]| -> Option<TableRef> {
        hash_find(&db.a_db[i].p_schema.as_ref().unwrap().borrow().tbl_hash, name)
    };
    let mut p: Option<TableRef> = None;

    if let Some(z_database) = z_database {
        let mut i: i32 = 0;
        while i < db.n_db {
            if str_i_cmp(z_database, db.a_db[i as usize].z_db_s_name.as_deref().unwrap_or(b"")) == 0 {
                break;
            }
            i += 1;
        }
        if i >= db.n_db {
            // Sem casamento com os nomes oficiais. Mas "main" sempre casa com o
            // esquema 0 como fallback legado.
            if str_i_cmp(z_database, b"main") == 0 {
                i = 0;
            } else {
                return None;
            }
        }
        let i = i as usize;
        p = tbl_find(i, z_name);
        if p.is_none() && api::strnicmp(z_name, b"sqlite_", 7) == 0 {
            if i == 1 {
                if str_i_cmp(&z_name[7..], &PREFERRED_TEMP_SCHEMA_TABLE[7..]) == 0
                    || str_i_cmp(&z_name[7..], &PREFERRED_SCHEMA_TABLE[7..]) == 0
                    || str_i_cmp(&z_name[7..], &LEGACY_SCHEMA_TABLE[7..]) == 0
                {
                    p = tbl_find(1, LEGACY_TEMP_SCHEMA_TABLE);
                }
            } else if str_i_cmp(&z_name[7..], &PREFERRED_SCHEMA_TABLE[7..]) == 0 {
                p = tbl_find(i, LEGACY_SCHEMA_TABLE);
            }
        }
    } else {
        // Casa com TEMP primeiro
        p = tbl_find(1, z_name);
        if p.is_some() {
            return p;
        }
        // O banco main é o segundo
        p = tbl_find(0, z_name);
        if p.is_some() {
            return p;
        }
        // Os bancos anexados vêm na ordem do ATTACH
        let mut i = 2;
        while i < db.n_db as usize {
            p = tbl_find(i, z_name);
            if p.is_some() {
                break;
            }
            i += 1;
        }
        if p.is_none() && api::strnicmp(z_name, b"sqlite_", 7) == 0 {
            if str_i_cmp(&z_name[7..], &PREFERRED_SCHEMA_TABLE[7..]) == 0 {
                p = tbl_find(0, LEGACY_SCHEMA_TABLE);
            } else if str_i_cmp(&z_name[7..], &PREFERRED_TEMP_SCHEMA_TABLE[7..]) == 0 {
                p = tbl_find(1, LEGACY_TEMP_SCHEMA_TABLE);
            }
        }
    }
    p
}


// ---- part_001.rs ----

/// Localiza a estrutura em memória que descreve uma tabela particular dado o
/// nome da tabela e (opcionalmente) o nome do banco que a contém. Devolve
/// `None` se não encontrada e deixa uma mensagem de erro em `p_parse.z_err_msg`.
///
/// A diferença para `find_table()` é que esta rotina deixa a mensagem de erro
/// em `p_parse` e a outra não.
pub fn locate_table(
    p_parse: &mut Parse,
    flags: u32,
    z_name: &[u8],
    z_dbase: Option<&[u8]>,
) -> Option<TableRef> {
    let db = p_parse.db.upgrade().unwrap();

    // Lê o esquema do banco. Se ocorrer um erro, deixa mensagem e código em
    // p_parse e devolve None.
    if (db.borrow().m_db_flags & DBFLAG_SCHEMA_KNOWN_OK) == 0 && SQLITE_OK != read_schema(p_parse) {
        return None;
    }

    let mut p = find_table(&db.borrow(), z_name, z_dbase);
    if p.is_none() {
        // Se z_name não é o nome de uma tabela criada com CREATE no esquema,
        // verifica se é o nome de uma tabela virtual que pode ser epônima.
        if (p_parse.prep_flags & SQLITE_PREPARE_NO_VTAB) == 0 && db.borrow().init.busy == 0 {
            let mut p_mod = hash_find(&db.borrow().a_module, z_name);
            if p_mod.is_none() && api::strnicmp(z_name, b"pragma_", 7) == 0 {
                p_mod = pragma_vtab_register(&db, z_name);
            }
            if let Some(p_mod) = p_mod {
                if vtab_eponymous_table_init(p_parse, &p_mod) {
                    return p_mod.borrow().p_epo_tab.clone();
                }
            }
        }
        if (flags & LOCATE_NOERR) != 0 {
            return None;
        }
        p_parse.check_schema = 1;
    } else if is_virtual(&p.as_ref().unwrap().borrow())
        && (p_parse.prep_flags & SQLITE_PREPARE_NO_VTAB) != 0
    {
        p = None;
    }

    match &p {
        None => {
            let z_msg: &[u8] = if (flags & LOCATE_VIEW) != 0 {
                b"no such view"
            } else {
                b"no such table"
            };
            if let Some(z_dbase) = z_dbase {
                error_msg(
                    p_parse,
                    b"%s: %s.%s",
                    &[PrintfArg::Str(z_msg), PrintfArg::Str(z_dbase), PrintfArg::Str(z_name)],
                );
            } else {
                error_msg(p_parse, b"%s: %s", &[PrintfArg::Str(z_msg), PrintfArg::Str(z_name)]);
            }
        }
        Some(t) => {
            let t = t.borrow();
            debug_assert!(has_rowid(&t) || t.i_p_key < 0);
        }
    }

    p
}

/// Localiza a tabela identificada por `p`.
///
/// É um invólucro de `locate_table()`. A diferença é que esta função restringe
/// a busca ao esquema `p.p_schema` quando ele não é `None`. `p.p_schema` pode
/// ser não nulo quando faz parte de uma definição de view ou de um programa de
/// trigger (veja `fix_src_list()`).
pub fn locate_table_item(p_parse: &mut Parse, flags: u32, p: &SrcItem) -> Option<TableRef> {
    debug_assert!(p.p_schema.is_none() || p.z_database.is_none());
    let z_db: Option<Vec<u8>> = if let Some(p_schema) = &p.p_schema {
        let db = p_parse.db.upgrade().unwrap();
        let db = db.borrow();
        let i_db = schema_to_index(&db, p_schema);
        db.a_db[i_db as usize].z_db_s_name.clone()
    } else {
        p.z_database.clone()
    };
    locate_table(p_parse, flags, &p.z_name, z_db.as_deref())
}

/// Devolve o nome de tabela preferido para tabelas de sistema. Traduz os nomes
/// legados nos novos nomes preferidos, quando apropriado.
pub fn preferred_table_name(z_name: &[u8]) -> &[u8] {
    if str_n_i_cmp(z_name, b"sqlite_", 7) == 0 {
        if str_i_cmp(&z_name[7..], &LEGACY_SCHEMA_TABLE[7..]) == 0 {
            return PREFERRED_SCHEMA_TABLE;
        }
        if str_i_cmp(&z_name[7..], &LEGACY_TEMP_SCHEMA_TABLE[7..]) == 0 {
            return PREFERRED_TEMP_SCHEMA_TABLE;
        }
    }
    z_name
}

/// Localiza a estrutura em memória que descreve um índice particular dado o
/// nome do índice e o nome do banco que o contém. Devolve `None` se não
/// encontrado.
///
/// Se `z_db` é `None`, todos os bancos são pesquisados e o primeiro índice que
/// casar é devolvido (sem checar nomes duplicados). A ordem de busca é TEMP
/// primeiro, depois MAIN, depois os bancos auxiliares do ATTACH.
pub fn find_index(db: &Sqlite3, z_name: &[u8], z_db: Option<&[u8]>) -> Option<IndexRef> {
    let mut p: Option<IndexRef> = None;
    let mut i = OMIT_TEMPDB;
    while i < db.n_db {
        let j = if i < 2 { i ^ 1 } else { i }; // Busca TEMP antes de MAIN
        let p_schema = db.a_db[j as usize].p_schema.as_ref().unwrap();
        if let Some(z_db) = z_db {
            if !db_is_named(db, j, z_db) {
                i += 1;
                continue;
            }
        }
        p = hash_find(&p_schema.borrow().idx_hash, z_name);
        if p.is_some() {
            break;
        }
        i += 1;
    }
    p
}

/// Recupera a memória usada por um índice
pub fn free_index(db: &Sqlite3, p_idx: &IndexRef) {
    let mut p = p_idx.borrow_mut();
    delete_index_samples(db, &mut p);
    expr_delete(db, p.p_part_idx_where.take());
    expr_list_delete(db, p.a_col_expr.take());
    p.z_col_aff = None;
    if p.is_resized != 0 {
        p.az_coll = Vec::new();
    }
}

/// Para o índice chamado `z_idx_name` encontrado no banco `i_db`, desvincula
/// esse índice de sua tabela, remove-o do hash de índices e libera todas as
/// estruturas de memória associadas.
pub fn unlink_and_delete_index(db: &mut Sqlite3, i_db: i32, z_idx_name: &[u8]) {
    let p_schema = db.a_db[i_db as usize].p_schema.clone().unwrap();
    let p_index = hash_insert(&mut p_schema.borrow_mut().idx_hash, z_idx_name, None);
    if let Some(p_index) = p_index {
        let p_table = p_index.borrow().p_table.upgrade().unwrap();
        let first = p_table.borrow().p_index.clone();
        let is_first = first.as_ref().map_or(false, |f| Rc::ptr_eq(f, &p_index));
        if is_first {
            let p_next = p_index.borrow().p_next.clone();
            p_table.borrow_mut().p_index = p_next;
        } else {
            // Justificativa do ALWAYS() do C: o índice precisa estar na lista de índices.
            let mut p = first;
            while let Some(cur) = p.clone() {
                let p_next = cur.borrow().p_next.clone();
                if p_next.as_ref().map_or(false, |n| Rc::ptr_eq(n, &p_index)) {
                    let after = p_index.borrow().p_next.clone();
                    cur.borrow_mut().p_next = after;
                    break;
                }
                p = p_next;
            }
        }
        free_index(db, &p_index);
    }
    db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
}

/// Procura na lista de arquivos de banco abertos em `db.a_db` e, se algum foi
/// fechado, remove-o da lista. Reduz a estrutura `db.a_db` se possível.
///
/// A entrada 0 (banco "main") e a entrada 1 (banco "temp") nunca são
/// candidatas a colapso.
pub fn collapse_database_array(db: &mut Sqlite3) {
    let mut i: i32 = 2;
    let mut j: i32 = 2;
    while i < db.n_db {
        if db.a_db[i as usize].p_bt.is_none() {
            db.a_db[i as usize].z_db_s_name = None;
            i += 1;
            continue;
        }
        if j < i {
            db.a_db[j as usize] = db.a_db[i as usize].clone();
        }
        j += 1;
        i += 1;
    }
    db.n_db = j;
    if db.n_db <= 2 && db.a_db.len() > 2 {
        // Equivale a copiar para `aDbStatic` e liberar o vetor dinâmico: o Vec
        // não distingue os dois, então só encolhe para as duas entradas fixas.
        db.a_db.truncate(2);
    }
}

/// Redefine o esquema do banco no índice `i_db`. Também redefine o esquema TEMP.
/// O reset é adiado se `db.n_schema_lock` não é zero. Resets adiados podem ser
/// executados chamando com `i_db < 0`.
pub fn reset_one_schema(db: &mut Sqlite3, i_db: i32) {
    debug_assert!(i_db < db.n_db);

    if i_db >= 0 {
        db_set_property(db, i_db, DB_RESET_WANTED);
        db_set_property(db, 1, DB_RESET_WANTED);
        db.m_db_flags &= !DBFLAG_SCHEMA_KNOWN_OK;
    }

    if db.n_schema_lock == 0 {
        for i in 0..db.n_db {
            if db_has_property(db, i, DB_RESET_WANTED) {
                let p_schema = db.a_db[i as usize].p_schema.clone();
                schema_clear(p_schema.as_ref());
            }
        }
    }
}

/// Apaga toda a informação de esquema de todos os bancos anexados (incluindo
/// "main" e "temp") de uma única conexão de banco.
pub fn reset_all_schemas_of_connection(db: &mut Sqlite3) {
    btree_enter_all(db);
    for i in 0..db.n_db {
        let p_schema = db.a_db[i as usize].p_schema.clone();
        if p_schema.is_some() {
            if db.n_schema_lock == 0 {
                schema_clear(p_schema.as_ref());
            } else {
                db_set_property(db, i, DB_RESET_WANTED);
            }
        }
    }
    db.m_db_flags &= !(DBFLAG_SCHEMA_CHANGE | DBFLAG_SCHEMA_KNOWN_OK);
    vtab_unlock_list(db);
    btree_leave_all(db);
    if db.n_schema_lock == 0 {
        collapse_database_array(db);
    }
}

/// Esta rotina é chamada quando ocorre um commit.
pub fn commit_internal_changes(db: &mut Sqlite3) {
    db.m_db_flags &= !DBFLAG_SCHEMA_CHANGE;
}

/// Define a expressão associada a uma coluna. Normalmente é o valor DEFAULT,
/// mas também pode ser a expressão que calcula o valor de uma coluna gerada.
/// A coluna é indicada por índice em `p_tab.a_col` (o C recebe o ponteiro).
pub fn column_set_expr(p_parse: &mut Parse, p_tab: &mut Table, i_col: usize, p_expr: Option<Box<Expr>>) {
    debug_assert!(is_ordinary_table(p_tab));
    let n_expr = p_tab.u_tab.p_dflt_list.as_ref().map_or(0, |l| l.n_expr);
    let i_dflt = p_tab.a_col[i_col].i_dflt as i32;
    if i_dflt == 0 || p_tab.u_tab.p_dflt_list.is_none() || n_expr < i_dflt {
        let p_list = p_tab.u_tab.p_dflt_list.take();
        p_tab.a_col[i_col].i_dflt = if p_list.is_none() { 1 } else { (n_expr + 1) as u16 };
        p_tab.u_tab.p_dflt_list = expr_list_append(p_parse, p_list, p_expr);
    } else {
        let db = p_parse.db.upgrade().unwrap();
        let p_list = p_tab.u_tab.p_dflt_list.as_mut().unwrap();
        let item = &mut p_list.a[(i_dflt - 1) as usize];
        expr_delete(&db.borrow(), item.p_expr.take());
        item.p_expr = p_expr;
    }
}

/// Devolve a expressão associada a uma coluna. Pode ser a cláusula DEFAULT ou
/// a cláusula AS de uma coluna gerada. Devolve `None` se a coluna não tem
/// expressão associada.
pub fn column_expr<'a>(p_tab: &'a Table, p_col: &Column) -> Option<&'a Expr> {
    if p_col.i_dflt == 0 {
        return None;
    }
    if !is_ordinary_table(p_tab) {
        return None;
    }
    let p_list = p_tab.u_tab.p_dflt_list.as_ref()?;
    if p_list.n_expr < p_col.i_dflt as i32 {
        return None;
    }
    p_list.a[p_col.i_dflt as usize - 1].p_expr.as_deref()
}

/// Define o nome da sequência de ordenação de uma coluna. O nome fica depois do
/// nome da coluna (e do tipo, quando há) em `z_cn_name`, separados por NUL.
pub fn column_set_coll(_db: &Sqlite3, p_col: &mut Column, z_coll: &[u8]) {
    let mut n = str_len30(&p_col.z_cn_name) as usize + 1;
    if (p_col.col_flags & COLFLAG_HASTYPE) != 0 {
        n += str_len30(&p_col.z_cn_name[n..]) as usize + 1;
    }
    p_col.z_cn_name.truncate(n);
    p_col.z_cn_name.extend_from_slice(z_coll);
    p_col.z_cn_name.push(0);
    p_col.col_flags |= COLFLAG_HASCOLL;
}

/// Devolve o nome da sequência de ordenação de uma coluna (sem o NUL final)
pub fn column_coll(p_col: &Column) -> Option<&[u8]> {
    if (p_col.col_flags & COLFLAG_HASCOLL) == 0 {
        return None;
    }
    let z = &p_col.z_cn_name;
    let mut i = 0;
    while z[i] != 0 {
        i += 1;
    }
    if (p_col.col_flags & COLFLAG_HASTYPE) != 0 {
        loop {
            i += 1;
            if z[i] == 0 {
                break;
            }
        }
    }
    let start = i + 1;
    let end = z[start..].iter().position(|&b| b == 0).map_or(z.len(), |k| start + k);
    Some(&z[start..end])
}

/// Libera a memória alocada para os nomes das colunas de uma tabela ou view
/// (o vetor `Table.a_col`).
pub fn delete_column_names(db: &Sqlite3, p_table: &mut Table) {
    if !p_table.a_col.is_empty() {
        for p_col in p_table.a_col.iter() {
            debug_assert!(p_col.z_cn_name.is_empty() || p_col.h_name == str_i_hash(&p_col.z_cn_name));
        }
        if is_ordinary_table(p_table) {
            expr_list_delete(db, p_table.u_tab.p_dflt_list.take());
        }
        if db.pn_bytes_freed.is_none() {
            p_table.a_col = Vec::new();
            p_table.n_col = 0;
        }
    }
}


// ---- part_002.rs ----

/// Remove as estruturas de dados em memória associadas à tabela dada. Nenhuma
/// mudança é feita em disco por esta rotina.
///
/// Só apaga a estrutura de dados. Não a desvincula da tabela hash, mas destrói
/// as estruturas de memória dos índices e das chaves estrangeiras associados.
///
/// O parâmetro `db` é opcional no C; aqui é sempre fornecido (ele é necessário
/// quando o objeto Table contém memória lookaside, ou para medir a memória via
/// `db.pn_bytes_freed`).
fn delete_table_body(db: &Sqlite3, p_table: &TableRef) {
    // Apaga todos os índices associados a esta tabela.
    let mut p_index = p_table.borrow().p_index.clone();
    while let Some(idx) = p_index {
        let p_next = idx.borrow().p_next.clone();
        debug_assert!(
            idx.borrow().p_schema.upgrade().map(|s| Rc::as_ptr(&s))
                == p_table.borrow().p_schema.upgrade().map(|s| Rc::as_ptr(&s))
                || (is_virtual(&p_table.borrow()) && idx.borrow().idx_type != SQLITE_IDXTYPE_APPDEF)
        );
        if db.pn_bytes_freed.is_none() && !is_virtual(&p_table.borrow()) {
            let z_name = idx.borrow().z_name.clone();
            let p_schema = idx.borrow().p_schema.upgrade().unwrap();
            let p_old = hash_insert(&mut p_schema.borrow_mut().idx_hash, &z_name, None);
            debug_assert!(p_old.as_ref().map_or(true, |o| Rc::ptr_eq(o, &idx)));
        }
        free_index(db, &idx);
        p_index = p_next;
    }

    let is_ordinary = is_ordinary_table(&p_table.borrow());
    if is_ordinary {
        fk_delete(db, p_table);
    } else if is_virtual(&p_table.borrow()) {
        vtab_clear(db, p_table);
    } else {
        debug_assert!(is_view(&p_table.borrow()));
        let p_select = p_table.borrow_mut().u_view.p_select.take();
        select_delete(db, p_select);
    }

    // Apaga a própria estrutura Table. Nome, afinidades e lista CHECK são
    // liberados junto com ela quando a última referência cai.
    delete_column_names(db, &mut p_table.borrow_mut());
    let mut t = p_table.borrow_mut();
    t.z_name = Vec::new();
    t.z_col_aff = None;
    expr_list_delete(db, t.p_check.take());
}

/// Apaga a tabela quando a contagem de referências chega a zero.
pub fn delete_table(db: &Sqlite3, p_table: Option<&TableRef>) {
    // Não apaga a tabela até a contagem de referências chegar a zero.
    let p_table = match p_table {
        Some(t) => t,
        None => return,
    };
    if db.pn_bytes_freed.is_none() {
        let mut t = p_table.borrow_mut();
        t.n_tab_ref -= 1;
        if t.n_tab_ref > 0 {
            return;
        }
    }
    delete_table_body(db, p_table);
}

/// Versão com ponteiro genérico de `delete_table()`, usada como callback
/// de destrutor (`sqlite3DeleteTableGeneric`).
pub fn delete_table_generic(db: &Sqlite3, p_table: Option<&TableRef>) {
    delete_table(db, p_table);
}

/// Desvincula a tabela dada das tabelas hash e apaga a estrutura da tabela
/// com todos os seus índices e chaves estrangeiras.
pub fn unlink_and_delete_table(db: &mut Sqlite3, i_db: i32, z_tab_name: &[u8]) {
    debug_assert!(i_db >= 0 && i_db < db.n_db);
    let p_schema = db.a_db[i_db as usize].p_schema.clone().unwrap();
    let p = hash_insert(&mut p_schema.borrow_mut().tbl_hash, z_tab_name, None);
    delete_table(db, p.as_ref());
    db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
}

/// Dado um token, devolve uma string com o texto desse token. Aspas
/// (`"nome"`, `'nome'`, `[nome]` ou `` `nome` ``) em volta do corpo do token
/// são removidas.
///
/// Os tokens costumam ser apontadores para o texto SQL original e não são
/// terminados em NUL nem persistentes; a string devolvida é uma cópia própria.
pub fn name_from_token(db: &Sqlite3, p_name: Option<&Token>) -> Option<Vec<u8>> {
    if let Some(p_name) = p_name {
        let mut z_name = db_str_n_dup(db, &p_name.z, p_name.n as usize);
        dequote(&mut z_name);
        Some(z_name)
    } else {
        None
    }
}

/// Abre para escrita a tabela sqlite_schema armazenada no banco `i_db`. A
/// tabela é aberta com o cursor 0.
pub fn open_schema_table(p_parse: &mut Parse, i_db: i32) {
    let v = get_vdbe(p_parse);
    table_lock(p_parse, i_db, SCHEMA_ROOT, 1, LEGACY_SCHEMA_TABLE);
    if let Some(v) = &v {
        vdbe_add_op4_int(v, OP_OPENWRITE, 0, SCHEMA_ROOT as i32, i_db, 5);
    }
    if p_parse.n_tab == 0 {
        p_parse.n_tab = 1;
    }
}

/// `z_name` contém o nome de um banco ("main", "temp" ou o nome de um banco
/// anexado). Devolve o índice do banco em `db.a_db`, ou -1 se o nome não for
/// encontrado.
pub fn find_db_name(db: &Sqlite3, z_name: Option<&[u8]>) -> i32 {
    let mut i: i32 = -1; // Número do banco
    if let Some(z_name) = z_name {
        i = db.n_db - 1;
        while i >= 0 {
            let p_db = &db.a_db[i as usize];
            if api::stricmp(p_db.z_db_s_name.as_deref().unwrap_or(b""), z_name) == 0 {
                break;
            }
            // "main" é sempre um apelido aceitável para o banco primário,
            // mesmo que ele tenha sido renomeado com SQLITE_DBCONFIG_MAINDBNAME.
            if i == 0 && api::stricmp(b"main", z_name) == 0 {
                break;
            }
            i -= 1;
        }
    }
    i
}

/// O token `p_name` contém o nome de um banco ("main", "temp" ou o nome de um
/// banco anexado). Devolve o índice do banco em `db.a_db`, ou -1 se não existe.
pub fn find_db(db: &Sqlite3, p_name: &Token) -> i32 {
    let z_name = name_from_token(db, Some(p_name));
    find_db_name(db, z_name.as_deref())
}

/// O nome da tabela, view ou trigger chega por dois tokens, `p_name1` e
/// `p_name2`. Se o nome é qualificado (`CREATE TABLE xxx.yyy (...)`), `p_name1`
/// é "xxx" e `p_name2` é "yyy". Se não é (`CREATE TABLE yyy(...)`), `p_name1` é
/// "yyy" e `p_name2` é vazio.
///
/// Esta rotina grava em `p_unqual` o token que guarda o nome não qualificado
/// (só quando não há erro) e devolve o índice do banco "xxx".
pub fn two_part_name(
    p_parse: &mut Parse,
    p_name1: &Token,
    p_name2: &Token,
    p_unqual: &mut Token,
) -> i32 {
    let db = p_parse.db.upgrade().unwrap();
    let i_db: i32;

    if p_name2.n > 0 {
        if db.borrow().init.busy != 0 {
            error_msg(p_parse, b"corrupt database", &[]);
            return -1;
        }
        *p_unqual = p_name2.clone();
        i_db = find_db(&db.borrow(), p_name1);
        if i_db < 0 {
            error_msg(p_parse, b"unknown database %T", &[PrintfArg::Tok(p_name1)]);
            return -1;
        }
    } else {
        let d = db.borrow();
        debug_assert!(
            d.init.i_db == 0
                || d.init.busy != 0
                || in_special_parse(p_parse)
                || (d.m_db_flags & DBFLAG_VACUUM) != 0
        );
        i_db = d.init.i_db;
        *p_unqual = p_name1.clone();
    }
    i_db
}

/// Verdadeiro se PRAGMA writable_schema está ON
pub fn writable_schema(db: &Sqlite3) -> bool {
    (db.flags & (SQLITE_WRITE_SCHEMA | SQLITE_DEFENSIVE)) == SQLITE_WRITE_SCHEMA
}

/// Verifica se a string UTF-8 `z_name` é um nome não qualificado legal para um
/// novo objeto de esquema (tabela, índice, view ou trigger). Todos os nomes são
/// legais, exceto os que começam com "sqlite_" (em maiúsculas, minúsculas ou
/// misto): essa parte do espaço de nomes é reservada para uso interno.
///
/// Ao analisar a tabela sqlite_schema, também confere se as colunas "type",
/// "name" e "tbl_name" são consistentes com o SQL.
pub fn check_object_name(
    p_parse: &mut Parse,
    z_name: &[u8],
    z_type: &[u8],
    z_tbl_name: &[u8],
) -> i32 {
    let db = p_parse.db.upgrade().unwrap();
    let (skip, busy) = {
        let d = db.borrow();
        (
            writable_schema(&d) || d.init.imposter_table != 0 || !global_config().b_extra_schema_checks,
            d.init.busy != 0,
        )
    };
    if skip {
        // Pula estas verificações de erro com writable_schema=ON
        return SQLITE_OK;
    }
    if busy {
        let mismatch = {
            let d = db.borrow();
            api::stricmp(z_type, &d.init.az_init[0]) != 0
                || api::stricmp(z_name, &d.init.az_init[1]) != 0
                || api::stricmp(z_tbl_name, &d.init.az_init[2]) != 0
        };
        if mismatch {
            error_msg(p_parse, b"", &[]); // corruptSchema() fornece o erro
            return SQLITE_ERROR;
        }
    } else {
        let reserved = (p_parse.nested == 0 && 0 == str_n_i_cmp(z_name, b"sqlite_", 7)) || {
            let d = db.borrow();
            read_only_shadow_tables(&d) && shadow_table_name(&d, z_name)
        };
        if reserved {
            error_msg(
                p_parse,
                b"object name reserved for internal use: %s",
                &[PrintfArg::Str(z_name)],
            );
            return SQLITE_ERROR;
        }
    }
    SQLITE_OK
}

/// Devolve o índice PRIMARY KEY de uma tabela
pub fn primary_key_index(p_tab: &Table) -> Option<IndexRef> {
    let mut p = p_tab.p_index.clone();
    while let Some(idx) = p.clone() {
        if is_primary_key_index(&idx.borrow()) {
            break;
        }
        p = idx.borrow().p_next.clone();
    }
    p
}

/// Converte um número de coluna da tabela em número de coluna do índice. Ou
/// seja, para a coluna `i_col` da tabela (como definida no CREATE TABLE),
/// acha o (primeiro) deslocamento dessa coluna no índice `p_idx`, ou -1 se a
/// coluna não é usada no índice.
pub fn table_column_to_index(p_idx: &Index, i_col: i16) -> i16 {
    for i in 0..p_idx.n_column as usize {
        if i_col == p_idx.ai_column[i] {
            return i as i16;
        }
    }
    -1
}

/// Converte um número de coluna de armazenamento em número de coluna da
/// tabela.
///
/// O número de armazenamento (0,1,2,...) é o índice do valor como ele aparece
/// no registro em disco. O número verdadeiro é o índice (0,1,2,...) da coluna
/// no CREATE TABLE. O de armazenamento é menor que o da tabela se, e somente
/// se, há colunas VIRTUAL à esquerda.
pub fn storage_column_to_table(p_tab: &Table, mut i_col: i16) -> i16 {
    if (p_tab.tab_flags & TF_HAS_VIRTUAL) != 0 {
        let mut i: i16 = 0;
        while i <= i_col {
            if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
                i_col += 1;
            }
            i += 1;
        }
    }
    i_col
}

/// Converte um número de coluna da tabela em número de coluna de
/// armazenamento.
///
/// O número de armazenamento é o índice do valor como ele aparece no registro
/// em disco. Se a coluna de entrada é a N-ésima coluna virtual (base zero), o
/// número de armazenamento é o número de colunas não virtuais da tabela mais N.
///
/// Se a coluna de entrada é VIRTUAL, ela não deve aparecer no armazenamento,
/// mas o valor às vezes fica em cache em registradores depois da faixa usada
/// para montar o registro. Por isso a coluna virtual vai depois de todas as
/// outras. Com a tabela `ex(N,S,V,N,S,V,N,S,V)` (N normal, S STORED, V
/// VIRTUAL), as entradas 0 1 2 3 4 5 6 7 8 dão as saídas 0 1 6 2 3 7 4 5 8.
///
/// Se a tabela não tem colunas virtuais, devolve `i_col`. Se `i_col` é
/// negativo (ROWID), devolve `i_col`.
pub fn table_column_to_storage(p_tab: &Table, i_col: i16) -> i16 {
    debug_assert!((i_col as i32) < p_tab.n_col as i32);
    if (p_tab.tab_flags & TF_HAS_VIRTUAL) == 0 || i_col < 0 {
        return i_col;
    }
    let mut n: i16 = 0;
    let mut i: i16 = 0;
    while i < i_col {
        if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) == 0 {
            n += 1;
        }
        i += 1;
    }
    if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
        // i_col é uma coluna virtual em si
        p_tab.n_nv_col + i - n
    } else {
        // i_col é uma coluna normal ou armazenada
        n
    }
}


// ---- part_003.rs ----

/// Insere um único opcode de consulta OP_JournalMode para forçar a instrução
/// preparada a devolver falso em `sqlite3_stmt_readonly()`. É usado por CREATE
/// TABLE IF NOT EXISTS e similares quando a tabela já existe, de modo que a
/// instrução preparada devolva falso mesmo sendo uma operação somente leitura
/// sem efeito.
fn force_not_readonly(p_parse: &mut Parse) {
    p_parse.n_mem += 1;
    let i_reg = p_parse.n_mem;
    let v = get_vdbe(p_parse);
    if let Some(v) = &v {
        vdbe_add_op3(v, OP_JOURNALMODE, 0, i_reg, PAGER_JOURNALMODE_QUERY);
        vdbe_uses_btree(v, 0);
    }
}

/// Começa a construir uma nova representação de tabela em memória. É a primeira
/// de várias rotinas de ação chamadas em resposta a um CREATE TABLE. Em
/// particular, é chamada depois de ver os tokens "CREATE" e "TABLE" e o nome da
/// tabela. O sinalizador `is_temp` é verdadeiro se a tabela deve ficar no
/// arquivo de banco auxiliar em vez do principal; normalmente é o caso quando
/// a palavra-chave "TEMP" ou "TEMPORARY" ocorre entre CREATE e TABLE.
///
/// O novo registro de tabela é inicializado e colocado em `p_parse.p_new_table`.
/// À medida que mais do CREATE TABLE é analisado, rotinas de ação adicionais
/// acrescentam informação a esse registro. No fim do CREATE TABLE, `end_table()`
/// é chamada para completar a construção.
pub fn start_table(
    p_parse: &mut Parse,
    p_name1: &Token,
    p_name2: &Token,
    mut is_temp: i32,
    is_view: i32,
    is_virtual: i32,
    no_err: i32,
) {
    let db = p_parse.db.upgrade().unwrap();
    let mut i_db: i32; // Número do banco onde criar a tabela
    let z_name: Option<Vec<u8>>; // O nome da nova tabela
    let p_name: Token; // Nome não qualificado da tabela a criar

    let (init_busy, init_new_tnum, init_i_db) = {
        let d = db.borrow();
        (d.init.busy != 0, d.init.new_tnum, d.init.i_db)
    };
    if init_busy && init_new_tnum == 1 {
        // Caso especial: analisando o esquema sqlite_schema ou sqlite_temp_schema
        i_db = init_i_db;
        z_name = Some(schema_table(i_db).to_vec());
        p_name = p_name1.clone();
    } else {
        // O caso comum
        let mut p_unqual = Token::default();
        i_db = two_part_name(p_parse, p_name1, p_name2, &mut p_unqual);
        if i_db < 0 {
            return;
        }
        if OMIT_TEMPDB == 0 && is_temp != 0 && p_name2.n > 0 && i_db != 1 {
            // Ao criar uma tabela temporária, o nome não pode ser qualificado,
            // a menos que o nome do banco seja "temp" de qualquer forma.
            error_msg(p_parse, b"temporary table name must be unqualified", &[]);
            return;
        }
        if OMIT_TEMPDB == 0 && is_temp != 0 {
            i_db = 1;
        }
        z_name = name_from_token(&db.borrow(), Some(&p_unqual));
        if in_rename_object(p_parse) {
            if let Some(z) = &z_name {
                rename_token_map(p_parse, z, &p_unqual);
            }
        }
        p_name = p_unqual;
    }
    p_parse.s_name_token = p_name.clone();
    let z_name = match z_name {
        Some(z) => z,
        None => return,
    };

    'begin_table_error: {
        if check_object_name(p_parse, &z_name, if is_view != 0 { b"view" } else { b"table" }, &z_name) != SQLITE_OK {
            break 'begin_table_error;
        }
        if db.borrow().init.i_db == 1 {
            is_temp = 1;
        }
        {
            debug_assert!(is_temp == 0 || is_temp == 1);
            debug_assert!(is_view == 0 || is_view == 1);
            const A_CODE: [i32; 4] = [
                SQLITE_CREATE_TABLE,
                SQLITE_CREATE_TEMP_TABLE,
                SQLITE_CREATE_VIEW,
                SQLITE_CREATE_TEMP_VIEW,
            ];
            let z_db = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
            if auth_check(p_parse, SQLITE_INSERT, Some(schema_table(is_temp)), None, z_db.as_deref()) != 0 {
                break 'begin_table_error;
            }
            if is_virtual == 0
                && auth_check(
                    p_parse,
                    A_CODE[(is_temp + 2 * is_view) as usize],
                    Some(&z_name),
                    None,
                    z_db.as_deref(),
                ) != 0
            {
                break 'begin_table_error;
            }
        }

        // Garante que o novo nome de tabela não colide com um índice ou tabela
        // existente no mesmo banco. Emite erro se colidir. A exceção é quando a
        // instrução analisada foi passada a `sqlite3_declare_vtab()`: nesse
        // caso só os nomes e tipos de colunas serão usados, então não há
        // necessidade de testar colisões de espaço de nomes.
        if !in_special_parse(p_parse) {
            let z_db = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
            if SQLITE_OK != read_schema(p_parse) {
                break 'begin_table_error;
            }
            let p_table = find_table(&db.borrow(), &z_name, z_db.as_deref());
            if let Some(p_table) = p_table {
                if no_err == 0 {
                    let kind: &[u8] = if is_view(&p_table.borrow()) { b"view" } else { b"table" };
                    error_msg(
                        p_parse,
                        b"%s %T already exists",
                        &[PrintfArg::Str(kind), PrintfArg::Tok(&p_name)],
                    );
                } else {
                    code_verify_schema(p_parse, i_db);
                    force_not_readonly(p_parse);
                }
                break 'begin_table_error;
            }
            if find_index(&db.borrow(), &z_name, z_db.as_deref()).is_some() {
                error_msg(
                    p_parse,
                    b"there is already an index named %s",
                    &[PrintfArg::Str(&z_name)],
                );
                break 'begin_table_error;
            }
        }

        // A falha de alocação do C (`sqlite3DbMallocZero` devolvendo NULL, com
        // SQLITE_NOMEM) não existe em Rust: este ramo não é traduzido.
        let mut p_table = Table::default();
        p_table.z_name = z_name;
        p_table.i_p_key = -1;
        p_table.p_schema = db.borrow().a_db[i_db as usize].p_schema.as_ref().map(Rc::downgrade);
        p_table.n_tab_ref = 1;
        // SQLITE_DEFAULT_ROWEST não definido: 200 == log_est(1048576)
        p_table.n_row_log_est = 200;
        debug_assert!(p_parse.p_new_table.is_none());
        p_parse.p_new_table = Some(Rc::new(RefCell::new(p_table)));

        // Começa a gerar o código que insere o registro da tabela na tabela de
        // esquema. Em particular, precisamos alocar já o número de registro da
        // entrada da tabela, antes de qualquer PRIMARY KEY ou UNIQUE ser
        // analisado. Essas palavras-chave criam índices e o registro da tabela
        // precisa vir antes deles.
        let v = if db.borrow().init.busy == 0 { get_vdbe(p_parse) } else { None };
        if let Some(v) = v {
            // null_row[] é uma codificação OP_Record de uma linha com 5 NULLs
            const NULL_ROW: [u8; 6] = [6, 0, 0, 0, 0, 0];
            begin_write_operation(p_parse, 1, i_db);

            if is_virtual != 0 {
                vdbe_add_op0(&v, OP_VBEGIN);
            }

            // Se o formato de arquivo e a codificação do banco ainda não foram
            // definidos, define-os agora.
            p_parse.n_mem += 1;
            p_parse.reg_rowid = p_parse.n_mem;
            let reg1 = p_parse.reg_rowid;
            p_parse.n_mem += 1;
            p_parse.reg_root = p_parse.n_mem;
            let reg2 = p_parse.reg_root;
            p_parse.n_mem += 1;
            let reg3 = p_parse.n_mem;
            vdbe_add_op3(&v, OP_READCOOKIE, i_db, reg3, BTREE_FILE_FORMAT);
            vdbe_uses_btree(&v, i_db);
            let addr1 = vdbe_add_op1(&v, OP_IF, reg3);
            let (flags, enc_db) = {
                let d = db.borrow();
                (d.flags, enc(&d))
            };
            let file_format = if (flags & SQLITE_LEGACY_FILE_FMT) != 0 { 1 } else { SQLITE_MAX_FILE_FORMAT };
            vdbe_add_op3(&v, OP_SETCOOKIE, i_db, BTREE_FILE_FORMAT, file_format);
            vdbe_add_op3(&v, OP_SETCOOKIE, i_db, BTREE_TEXT_ENCODING, enc_db as i32);
            vdbe_jump_here(&v, addr1);

            // Isto só cria um registro reservado na tabela sqlite_schema. O
            // registro criado ainda não contém nada. Será substituído pela
            // entrada real no código gerado em `end_table()`.
            //
            // O rowid da nova entrada fica no registrador `p_parse.reg_rowid`.
            // O número da página raiz da nova tabela fica em `p_parse.reg_root`.
            // Ambos são necessários pelo código que `end_table()` vai gerar.
            if is_view != 0 || is_virtual != 0 {
                vdbe_add_op2(&v, OP_INTEGER, 0, reg2);
            } else {
                debug_assert!(p_parse.b_returning == 0);
                p_parse.u1.addr_cr_tab = vdbe_add_op3(&v, OP_CREATEBTREE, i_db, reg2, BTREE_INTKEY);
            }
            open_schema_table(p_parse, i_db);
            vdbe_add_op2(&v, OP_NEWROWID, 0, reg1);
            vdbe_add_op4(&v, OP_BLOB, 6, reg3, 0, P4Arg::Static(NULL_ROW.to_vec()));
            vdbe_add_op3(&v, OP_INSERT, 0, reg3, reg1);
            vdbe_change_p5(&v, OPFLAG_APPEND);
            vdbe_add_op0(&v, OP_CLOSE);
        }

        // Retorno normal (sem erro).
        return;
    }

    // Se ocorre um erro, saltamos para cá (`begin_table_error`). O nome da
    // tabela é liberado ao sair do escopo.
    p_parse.check_schema = 1;
}

/// Limpa as estruturas de dados associadas à cláusula RETURNING.
fn delete_returning(db: &Sqlite3, p_ret: &Rc<RefCell<Returning>>) {
    let p_schema = db.a_db[1].p_schema.clone().unwrap();
    let z_name = p_ret.borrow().z_name.clone();
    hash_insert(&mut p_schema.borrow_mut().trig_hash, &z_name, None);
    // O C chama `sqlite3ExprListDelete(db, pRet->pReturnEL)`. A lista é
    // compartilhada (Rc) com o passo do trigger: soltar as referências a libera.
    let mut r = p_ret.borrow_mut();
    r.p_return_el = None;
    r.ret_t_step.borrow_mut().p_expr_list = None;
}

/// Adiciona a cláusula RETURNING à análise em andamento.
///
/// Esta rotina cria um trigger TEMP especial que dispara para cada linha da
/// instrução DML. Esse trigger TEMP contém um único SELECT cujo conjunto de
/// resultados é o argumento da cláusula RETURNING. O trigger tem o sinalizador
/// `Trigger.b_returning` e o opcode TK_RETURNING em vez de TK_SELECT, para que
/// o gerador de código de trigger o trate de modo especial. O trigger TEMP é
/// removido automaticamente no fim da análise.
///
/// Quando esta rotina é chamada, ainda não sabemos se o RETURNING está ligado a
/// um DELETE, INSERT ou UPDATE, então o construímos como um trigger RETURNING.
/// Ele será convertido no tipo apropriado na primeira chamada a
/// `triggers_exist()`.
pub fn add_returning(p_parse: &mut Parse, p_list: Option<Box<ExprList>>) {
    let db = p_parse.db.upgrade().unwrap();
    if p_parse.p_new_trigger.is_some() {
        error_msg(p_parse, b"cannot use RETURNING in a trigger", &[]);
    } else {
        debug_assert!(p_parse.b_returning == 0 || p_parse.if_not_exists != 0);
    }
    p_parse.b_returning = 1;
    // A falha de alocação de `pRet` (OOM) não existe em Rust.
    let p_list: Option<Rc<ExprList>> = p_list.map(Rc::from);
    let ret_trig = Rc::new(RefCell::new(Trigger::default()));
    let ret_t_step = Rc::new(RefCell::new(TriggerStep::default()));
    let p_ret = Rc::new(RefCell::new(Returning::default()));
    p_parse.u1.p_returning = Some(p_ret.clone());
    // `pRet->pParse = pParse` não é modelado: o Parse não é compartilhável.
    // Quem precisar dele recebe o `&mut Parse` pela chamada.
    {
        let mut r = p_ret.borrow_mut();
        r.p_return_el = p_list.clone();
        r.ret_trig = ret_trig.clone();
        r.ret_t_step = ret_t_step.clone();
    }
    let p_ret_cleanup = p_ret.clone();
    parser_add_cleanup(
        p_parse,
        Box::new(move |db: &Sqlite3| delete_returning(db, &p_ret_cleanup)),
    );
    if db.borrow().malloc_failed != 0 {
        return;
    }
    // sqlite3_snprintf(sizeof(pRet->zName)=40, "sqlite_returning_%p", pParse)
    let mut z_name = b"sqlite_returning_0x".to_vec();
    z_name.extend_from_slice(format!("{:x}", p_parse as *const Parse as usize).as_bytes());
    z_name.truncate(39);
    let p_schema1 = db.borrow().a_db[1].p_schema.clone().unwrap();
    {
        let mut t = ret_trig.borrow_mut();
        t.z_name = z_name.clone();
        t.op = TK_RETURNING;
        t.tr_tm = TRIGGER_AFTER;
        t.b_returning = 1;
        t.p_schema = Some(Rc::downgrade(&p_schema1));
        t.p_tab_schema = Some(Rc::downgrade(&p_schema1));
        t.step_list = Some(ret_t_step.clone());
    }
    {
        let mut s = ret_t_step.borrow_mut();
        s.op = TK_RETURNING;
        s.p_trig = Rc::downgrade(&ret_trig);
        s.p_expr_list = p_list;
    }
    p_ret.borrow_mut().z_name = z_name.clone();
    let mut p_schema = p_schema1.borrow_mut();
    debug_assert!(
        hash_find(&p_schema.trig_hash, &z_name).is_none() || p_parse.n_err != 0 || p_parse.if_not_exists != 0
    );
    if let Some(p_old) = hash_insert(&mut p_schema.trig_hash, &z_name, Some(ret_trig.clone())) {
        // O hash devolve o próprio dado quando não consegue alocar (OOM)
        if Rc::ptr_eq(&p_old, &ret_trig) {
            drop(p_schema);
            oom_fault(&mut db.borrow_mut());
        }
    }
}

/// Adiciona uma nova coluna à tabela em construção.
///
/// O analisador chama esta rotina uma vez para cada declaração de coluna em um
/// CREATE TABLE. `start_table()` é chamada primeiro para dar a partida; depois
/// esta rotina é chamada para cada coluna.
///
/// `column_properties_from_name()` não existe aqui: com
/// `SQLITE_ENABLE_HIDDEN_COLUMNS` desligado o C a define como macro vazia.
pub fn add_column(p_parse: &mut Parse, mut s_name: Token, mut s_type: Token) {
    let db = p_parse.db.upgrade().unwrap();
    let mut e_type: u8 = COLTYPE_CUSTOM;
    let mut sz_est: u8 = 1;
    let mut affinity: u8 = SQLITE_AFF_BLOB;

    let p = match p_parse.p_new_table.clone() {
        Some(p) => p,
        None => return,
    };
    if p.borrow().n_col as i32 + 1 > db.borrow().a_limit[SQLITE_LIMIT_COLUMN as usize] {
        let z = p.borrow().z_name.clone();
        error_msg(p_parse, b"too many columns on %s", &[PrintfArg::Str(&z)]);
        return;
    }
    if !in_rename_object(p_parse) {
        dequote_token(&mut s_name);
    }

    // Como as palavras-chave GENERATE ALWAYS podem ser convertidas em
    // identificadores pelo analisador, às vezes acabamos com um nome de tipo
    // terminando em "generated always". Verifica esse caso e omite o texto
    // excedente.
    if s_type.n >= 16 && api::strnicmp(&s_type.z[(s_type.n - 6) as usize..], b"always", 6) == 0 {
        s_type.n -= 6;
        while s_type.n > 0 && is_space(s_type.z[(s_type.n - 1) as usize]) {
            s_type.n -= 1;
        }
        if s_type.n >= 9 && api::strnicmp(&s_type.z[(s_type.n - 9) as usize..], b"generated", 9) == 0 {
            s_type.n -= 9;
            while s_type.n > 0 && is_space(s_type.z[(s_type.n - 1) as usize]) {
                s_type.n -= 1;
            }
        }
    }

    // Verifica nomes de tipo padrão. Para eles definimos o campo Column.e_type
    // em vez de guardar o nome do tipo depois do nome da coluna, para poupar
    // espaço.
    if s_type.n >= 3 {
        dequote_token(&mut s_type);
        for i in 0..SQLITE_N_STDTYPE {
            if s_type.n as usize == STD_TYPE_LEN[i] as usize
                && api::strnicmp(&s_type.z, STD_TYPE[i], s_type.n as usize) == 0
            {
                s_type.n = 0;
                e_type = (i + 1) as u8;
                affinity = STD_TYPE_AFFINITY[i];
                if affinity <= SQLITE_AFF_TEXT {
                    sz_est = 5;
                }
                break;
            }
        }
    }

    // A capacidade evita realocação: o ponteiro do buffer serve de chave ao
    // mapa de tokens da operação RENAME.
    let mut z: Vec<u8> = Vec::with_capacity(s_name.n as usize + 1 + s_type.n as usize + usize::from(s_type.n > 0));
    z.extend_from_slice(&s_name.z[..s_name.n as usize]);
    if in_rename_object(p_parse) {
        rename_token_map(p_parse, &z, &s_name);
    }
    dequote(&mut z);
    let h_name = str_i_hash(&z);
    let dup = {
        let pb = p.borrow();
        (0..pb.n_col as usize).any(|i| {
            let nm = &pb.a_col[i].z_cn_name;
            let nm = &nm[..nm.iter().position(|&b| b == 0).unwrap_or(nm.len())];
            pb.a_col[i].h_name == h_name && str_i_cmp(&z, nm) == 0
        })
    };
    if dup {
        error_msg(p_parse, b"duplicate column name: %s", &[PrintfArg::Str(&z)]);
        return;
    }
    let mut p_col = Column::default();
    p_col.h_name = h_name;

    if s_type.n == 0 {
        // Se não há tipo especificado, as colunas têm a afinidade padrão
        // 'BLOB' com tamanho padrão de 4 bytes.
        p_col.affinity = affinity;
        p_col.e_c_type = e_type;
        p_col.sz_est = sz_est;
        // SQLITE_ENABLE_SORTER_REFERENCES não está definido no Debian.
        z.push(0);
        p_col.z_cn_name = z;
    } else {
        let mut z_type = s_type.z[..s_type.n as usize].to_vec();
        dequote(&mut z_type);
        z.push(0);
        p_col.z_cn_name = z;
        p_col.affinity = affinity_type(&z_type, &mut p_col);
        p_col.z_cn_name.extend_from_slice(&z_type);
        p_col.z_cn_name.push(0);
        p_col.col_flags |= COLFLAG_HASTYPE;
    }
    let mut pb = p.borrow_mut();
    let n_col = pb.n_col as usize;
    pb.a_col.truncate(n_col);
    pb.a_col.push(p_col);
    pb.n_col += 1;
    pb.n_nv_col += 1;
    p_parse.constraint_name.n = 0;
}


// ---- part_004.rs ----

/// Esta rotina é chamada pelo analisador durante o meio de análise de uma
/// instrução CREATE TABLE. Uma restrição "NOT NULL" foi vista numa coluna. Esta
/// rotina define o sinalizador notNull na coluna em construção.
pub fn add_not_null(p_parse: &mut Parse, on_error: i32) {
    let p = match p_parse.p_new_table.clone() {
        Some(p) => p,
        None => return,
    };
    let (n_col, first_idx, is_unique) = {
        let mut t = p.borrow_mut();
        if t.n_col < 1 {
            return;
        }
        let i = (t.n_col - 1) as usize;
        t.a_col[i].not_null = on_error as u8;
        t.tab_flags |= TF_HASNOTNULL;
        (t.n_col, t.p_index.clone(), (t.a_col[i].col_flags & COLFLAG_UNIQUE) != 0)
    };

    // Define o sinalizador uniqNotNull em qualquer índice UNIQUE ou PK já criado nesta coluna.
    if is_unique {
        let mut cur = first_idx;
        while let Some(p_idx) = cur {
            let next = {
                let mut idx = p_idx.borrow_mut();
                if idx.ai_column[0] as i32 == n_col as i32 - 1 {
                    idx.uniq_not_null = 1;
                }
                idx.p_next.clone()
            };
            cur = next;
        }
    }
}

/// Examina o nome do tipo de coluna z_in e devolve o tipo de afinidade
/// associado.
///
/// Esta rotina faz uma busca sem considerar maiúsculas/minúsculas de z_in para as
/// substrings na tabela abaixo. Se uma das substrings for encontrada, a afinidade
/// correspondente é devolvida. Se z_in contiver mais de uma das substrings, as entradas
/// perto do topo da tabela têm prioridade. Por exemplo, se z_in é 'BLOBINT',
/// SQLITE_AFF_INTEGER é devolvido.
///
/// Substring     | Afinidade
/// --------------------------------
/// 'INT'         | SQLITE_AFF_INTEGER
/// 'CHAR'        | SQLITE_AFF_TEXT
/// 'CLOB'        | SQLITE_AFF_TEXT
/// 'TEXT'        | SQLITE_AFF_TEXT
/// 'BLOB'        | SQLITE_AFF_BLOB
/// 'REAL'        | SQLITE_AFF_REAL
/// 'FLOA'        | SQLITE_AFF_REAL
/// 'DOUB'        | SQLITE_AFF_REAL
///
/// Se nenhuma das substrings da tabela acima for encontrada, SQLITE_AFF_NUMERIC
/// é devolvido.
pub fn affinity_type(z_in: &[u8], p_col: Option<&mut Column>) -> u8 {
    const fn w(a: u8, b: u8, c: u8, d: u8) -> u32 {
        ((a as u32) << 24) + ((b as u32) << 16) + ((c as u32) << 8) + (d as u32)
    }
    let mut h: u32 = 0;
    let mut aff: u8 = SQLITE_AFF_NUMERIC;
    // Posição em z_in logo depois do último "char" (ou do "blob" seguido de "(").
    let mut z_char: Option<usize> = None;

    let mut i = 0usize;
    while i < z_in.len() && z_in[i] != 0 {
        let x = z_in[i];
        h = (h << 8).wrapping_add(upper_to_lower[x as usize] as u32);
        i += 1;
        if h == w(b'c', b'h', b'a', b'r') {
            // CHAR
            aff = SQLITE_AFF_TEXT;
            z_char = Some(i);
        } else if h == w(b'c', b'l', b'o', b'b') {
            // CLOB
            aff = SQLITE_AFF_TEXT;
        } else if h == w(b't', b'e', b'x', b't') {
            // TEXT
            aff = SQLITE_AFF_TEXT;
        } else if h == w(b'b', b'l', b'o', b'b')
            && (aff == SQLITE_AFF_NUMERIC || aff == SQLITE_AFF_REAL)
        {
            // BLOB
            aff = SQLITE_AFF_BLOB;
            if i < z_in.len() && z_in[i] == b'(' {
                z_char = Some(i);
            }
        } else if h == w(b'r', b'e', b'a', b'l') && aff == SQLITE_AFF_NUMERIC {
            // REAL
            aff = SQLITE_AFF_REAL;
        } else if h == w(b'f', b'l', b'o', b'a') && aff == SQLITE_AFF_NUMERIC {
            // FLOA
            aff = SQLITE_AFF_REAL;
        } else if h == w(b'd', b'o', b'u', b'b') && aff == SQLITE_AFF_NUMERIC {
            // DOUB
            aff = SQLITE_AFF_REAL;
        } else if (h & 0x00FF_FFFF) == (((b'i' as u32) << 16) + ((b'n' as u32) << 8) + (b't' as u32)) {
            // INT
            aff = SQLITE_AFF_INTEGER;
            break;
        }
    }

    // Se pCol não é NULL, armazena uma estimativa do tamanho do campo. A estimativa é
    // dimensionada de forma que o tamanho de um inteiro seja 1.
    if let Some(col) = p_col {
        let mut v: i32 = 0; // tamanho padrão é aproximadamente 4 bytes
        if aff < SQLITE_AFF_NUMERIC {
            if let Some(start) = z_char {
                let mut j = start;
                while j < z_in.len() && z_in[j] != 0 {
                    if isdigit(z_in[j]) {
                        // BLOB(k), VARCHAR(k), CHAR(k) -> r=(k/4+1)
                        get_int32(&z_in[j..], &mut v);
                        break;
                    }
                    j += 1;
                }
            } else {
                v = 16; // BLOB, TEXT, CLOB -> r=5  (aproximadamente 20 bytes)
            }
        }
        v = v / 4 + 1;
        if v > 255 {
            v = 255;
        }
        col.sz_est = v as u8;
    }
    aff
}

/// A expressão é o valor padrão da coluna adicionada mais recentemente da tabela
/// em construção.
///
/// As expressões de valor padrão devem ser constantes. Lança uma exceção se este
/// não for o caso.
///
/// Esta rotina é chamada pelo analisador durante o meio de análise de uma
/// instrução CREATE TABLE.
pub fn add_default_value(
    p_parse: &mut Parse,
    p_expr: Box<Expr>,
    z_start: &[u8],
    z_end: &[u8],
) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    if let Some(p) = p_parse.p_new_table.clone() {
        let (is_init, i_col) = {
            let d = db.borrow();
            let t = p.borrow();
            ((d.init.busy != 0 && d.init.i_db != 1) as i32, (t.n_col - 1) as usize)
        };
        let (flags, z_cn_name) = {
            let t = p.borrow();
            (t.a_col[i_col].col_flags, t.a_col[i_col].z_cn_name.clone())
        };
        if !expr_is_constant_or_function(&p_expr, is_init) {
            error_msg(
                p_parse,
                &[&b"default value of column ["[..], &z_cn_name[..], &b"] is not constant"[..]]
                    .concat(),
            );
        } else if (flags & COLFLAG_GENERATED) != 0 {
            error_msg(p_parse, b"cannot use DEFAULT on a generated column");
        } else {
            // Uma cópia de pExpr é usada em vez do original, já que pExpr contém
            // tokens que apontam para memória volátil.
            let mut x = Expr::default();
            x.op = TK_SPAN;
            x.u.z_token = db_span_dup(&db, z_start, z_end);
            x.p_left = Some(p_expr.clone());
            x.flags = EP_SKIP;
            let p_dflt_expr = expr_dup(&db, &x, EXPRDUP_REDUCE);
            let mut t = p.borrow_mut();
            column_set_expr(p_parse, &mut t, i_col, p_dflt_expr);
        }
    }
    if in_rename_object(p_parse) {
        rename_expr_unmap(p_parse, &p_expr);
    }
    expr_delete(&db, Some(p_expr));
}

/// Compatibilidade com versões anteriores, truque:
///
/// Versões históricas do SQLite aceitavam strings como nomes de coluna em
/// índices e restrições PRIMARY KEY e UNIQUE. Exemplo:
///
///     CREATE TABLE xyz(a,b,c,d,e,PRIMARY KEY('a'),UNIQUE('b','c' COLLATE trim)
///     CREATE INDEX abc ON xyz('c','d' DESC,'e' COLLATE nocase DESC);
///
/// Isto é bobo. Mas para preservar compatibilidade com versões anteriores continuamos a
/// aceitar isto. Esta rotina faz a conversão necessária. Ela converte
/// a expressão dada no seu argumento de um TK_STRING para um TK_ID
/// se a expressão é apenas um TK_STRING com uma cláusula COLLATE opcional.
/// Se a expressão for qualquer coisa diferente de TK_STRING, a expressão é
/// deixada inalterada.
fn string_to_id(p: &mut Expr) {
    if p.op == TK_STRING {
        p.op = TK_ID;
    } else if p.op == TK_COLLATE {
        if let Some(left) = p.p_left.as_mut() {
            if left.op == TK_STRING {
                left.op = TK_ID;
            }
        }
    }
}

/// Marca a coluna dada como sendo parte da CHAVE PRIMÁRIA.
fn make_column_part_of_primary_key(p_parse: &mut Parse, p_col: &mut Column) {
    p_col.col_flags |= COLFLAG_PRIMKEY;
    if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
        error_msg(p_parse, b"generated columns cannot be part of the PRIMARY KEY");
    }
}

/// Designa a CHAVE PRIMÁRIA para a tabela. pList é uma lista de nomes
/// de colunas que formam a chave primária. Se pList for NULL, então a
/// coluna adicionada mais recentemente da tabela é a chave primária.
///
/// Uma tabela pode ter no máximo uma chave primária. Se a tabela já tem
/// uma chave primária (e esta é a segunda chave primária) então cria um
/// erro.
///
/// Se a CHAVE PRIMÁRIA está numa única coluna cujo tipo de dados é INTEGER,
/// então tentaremos usar essa coluna como o rowid. Define o campo Table.iPKey
/// da tabela em construção para ser o índice da coluna INTEGER PRIMARY KEY.
/// Table.iPKey é definido como -1 se não há INTEGER PRIMARY KEY.
///
/// Se a chave não é uma INTEGER PRIMARY KEY, então cria um índice único
/// para a chave. Nenhum índice é criado para INTEGER PRIMARY KEYs.
pub fn add_primary_key(
    p_parse: &mut Parse,
    p_list: Option<Box<ExprList>>,
    on_error: i32,
    auto_inc: i32,
    sort_order: i32,
) {
    let mut p_list = p_list;
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    'primary_key_exit: {
        let p_tab = match p_parse.p_new_table.clone() {
            Some(t) => t,
            None => break 'primary_key_exit,
        };
        if (p_tab.borrow().tab_flags & TF_HASPRIMARYKEY) != 0 {
            let name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                &[&b"table \""[..], &name[..], &b"\" has more than one primary key"[..]].concat(),
            );
            break 'primary_key_exit;
        }
        p_tab.borrow_mut().tab_flags |= TF_HASPRIMARYKEY;

        // Índice da coluna escolhida (p_col do C) e valor de iCol do C.
        let mut p_col: Option<usize> = None;
        let mut i_col: i32 = -1;
        let n_term: i32;
        match p_list.as_mut() {
            None => {
                i_col = p_tab.borrow().n_col as i32 - 1;
                p_col = Some(i_col as usize);
                {
                    let mut t = p_tab.borrow_mut();
                    make_column_part_of_primary_key(p_parse, &mut t.a_col[i_col as usize]);
                }
                n_term = 1;
            }
            Some(list) => {
                n_term = list.n_expr;
                for i in 0..n_term as usize {
                    let p_c_expr = expr_skip_collate_mut(list.a[i].p_expr.as_mut().unwrap());
                    string_to_id(p_c_expr);
                    if p_c_expr.op == TK_ID {
                        let z_c_name = p_c_expr.u.z_token.clone();
                        let mut t = p_tab.borrow_mut();
                        i_col = 0;
                        while i_col < t.n_col as i32 {
                            if str_i_cmp(&z_c_name, &t.a_col[i_col as usize].z_cn_name) == 0 {
                                p_col = Some(i_col as usize);
                                make_column_part_of_primary_key(p_parse, &mut t.a_col[i_col as usize]);
                                break;
                            }
                            i_col += 1;
                        }
                    }
                }
            }
        }
        let is_int_col = match p_col {
            Some(c) => p_tab.borrow().a_col[c].e_c_type == COLTYPE_INTEGER,
            None => false,
        };
        if n_term == 1 && is_int_col && sort_order != SQLITE_SO_DESC {
            if in_rename_object(p_parse) {
                if let Some(list) = p_list.as_ref() {
                    let p_c_expr = expr_skip_collate(list.a[0].p_expr.as_ref().unwrap());
                    rename_token_remap_ipkey_from_expr(p_parse, &p_tab, p_c_expr);
                }
            }
            {
                let mut t = p_tab.borrow_mut();
                t.i_p_key = i_col as i16;
                t.key_conf = on_error as u8;
                debug_assert!(auto_inc == 0 || auto_inc == 1);
                t.tab_flags |= (auto_inc as u32) * TF_AUTOINCREMENT;
            }
            if let Some(list) = p_list.as_ref() {
                p_parse.i_pk_sort_order = list.a[0].fg.sort_flags;
            }
            let _ = has_explicit_nulls(p_parse, p_list.as_deref());
        } else if auto_inc != 0 {
            error_msg(p_parse, b"AUTOINCREMENT is only allowed on an INTEGER PRIMARY KEY");
        } else {
            create_index(
                p_parse,
                None,
                None,
                None,
                p_list.take(),
                on_error,
                None,
                None,
                sort_order,
                0,
                SQLITE_IDXTYPE_PRIMARYKEY,
            );
        }
    }
    expr_list_delete(&db, p_list);
}

/// Adiciona uma nova restrição CHECK à tabela em construção.
pub fn add_check_constraint(
    p_parse: &mut Parse,
    p_check_expr: Box<Expr>,
    z_start: &[u8],
    z_end: &[u8],
) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let p_tab = p_parse.p_new_table.clone();
    let readonly = {
        let d = db.borrow();
        let bt = d.a_db[d.init.i_db as usize].p_bt.clone();
        btree_is_readonly(bt.as_ref())
    };
    match p_tab {
        Some(p_tab) if !in_declare_vtab(p_parse) && !readonly => {
            let old = p_tab.borrow_mut().p_check.take();
            let new_list = expr_list_append(p_parse, old, Some(p_check_expr));
            p_tab.borrow_mut().p_check = new_list;
            if p_parse.constraint_name.n != 0 {
                let name = p_parse.constraint_name.clone();
                expr_list_set_name(p_parse, p_tab.borrow_mut().p_check.as_mut(), &name, 1);
            } else {
                // z_start e z_end são sufixos do texto de entrada: o trecho entre eles é o
                // prefixo de z_start que sobra tirando o comprimento de z_end.
                let span = &z_start[..z_start.len() - z_end.len()];
                // for(zStart++; sqlite3Isspace(zStart[0]); zStart++){}
                let mut s = 1usize;
                while s < span.len() && isspace(span[s]) {
                    s += 1;
                }
                // while( sqlite3Isspace(zEnd[-1]) ){ zEnd--; }
                let mut e = span.len();
                while e > s && isspace(span[e - 1]) {
                    e -= 1;
                }
                let t = Token { z: z_start[s..].to_vec(), n: (e - s) as u32 };
                expr_list_set_name(p_parse, p_tab.borrow_mut().p_check.as_mut(), &t, 1);
            }
        }
        _ => {
            expr_delete(&db, Some(p_check_expr));
        }
    }
}

/// Define a função de colação da coluna de tabela analisada mais recentemente
/// para a CollSeq dada.
pub fn add_collate_type(p_parse: &mut Parse, p_token: &Token) {
    let p = match p_parse.p_new_table.clone() {
        Some(p) => p,
        None => return,
    };
    if in_rename_object(p_parse) {
        return;
    }
    let i = (p.borrow().n_col - 1) as usize;
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let z_coll = match name_from_token(&db, p_token) {
        Some(z) => z,
        None => return,
    };
    if locate_coll_seq(p_parse, &z_coll).is_some() {
        column_set_coll(&db, &mut p.borrow_mut().a_col[i], &z_coll);

        // Se a coluna é declarada como "<name> PRIMARY KEY COLLATE <type>",
        // então um índice pode ter sido criado nesta coluna antes do tipo
        // de colação ser adicionado. Corrija isto se for o caso.
        let mut cur = p.borrow().p_index.clone();
        while let Some(p_idx) = cur {
            let next = {
                let mut idx = p_idx.borrow_mut();
                debug_assert!(idx.n_key_col == 1);
                if idx.ai_column[0] as usize == i {
                    idx.az_coll[0] = column_coll(&p.borrow().a_col[i]);
                }
                idx.p_next.clone()
            };
            cur = next;
        }
    }
}


// ---- part_005.rs ----

/// Devolve o prefixo de `z` até o primeiro byte 0x00 (os nomes de coluna e de índice vêm no
/// formato de string C do original).
#[inline]
fn nul_terminated(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// Muda a coluna analisada mais recentemente para uma coluna GENERATED ALWAYS AS.
///
/// `SQLITE_OMIT_GENERATED_COLUMNS` não está definida no Debian, então só o ramo completo existe. A expressão é consumida: o que sobra em `p_expr_in` no fim é apagado.
pub fn add_generated(p_parse: &mut Parse, p_expr_in: Option<Box<Expr>>, p_type: Option<&Token>) {
    let mut p_expr_in = p_expr_in;
    let mut e_type: u16 = COLFLAG_VIRTUAL;
    let mut generated_error = false;

    'generated_done: {
        let p_tab = match p_parse.p_new_table.clone() {
            Some(t) => t,
            // Coluna gerada num CREATE TABLE IF NOT EXISTS que já existe.
            None => break 'generated_done,
        };
        let mut tab = p_tab.borrow_mut();
        let idx = (tab.n_col - 1) as usize;
        if in_declare_vtab(p_parse) {
            error_msg(p_parse, b"virtual tables cannot use computed columns");
            break 'generated_done;
        }
        if tab.a_col[idx].i_dflt > 0 {
            generated_error = true;
            break 'generated_done;
        }
        if let Some(p_type) = p_type {
            if p_type.n == 7 && str_n_i_cmp(b"virtual", &p_type.z, 7) == 0 {
                // Sem operação.
            } else if p_type.n == 6 && str_n_i_cmp(b"stored", &p_type.z, 6) == 0 {
                e_type = COLFLAG_STORED;
            } else {
                generated_error = true;
                break 'generated_done;
            }
        }
        if e_type == COLFLAG_VIRTUAL {
            tab.n_nv_col -= 1;
        }
        tab.a_col[idx].col_flags |= e_type;
        debug_assert!(TF_HASVIRTUAL == COLFLAG_VIRTUAL as u32);
        debug_assert!(TF_HASSTORED == COLFLAG_STORED as u32);
        tab.tab_flags |= e_type as u32;
        if (tab.a_col[idx].col_flags & COLFLAG_PRIMKEY) != 0 {
            // Para a mensagem de erro.
            make_column_part_of_primary_key(p_parse, &mut tab.a_col[idx]);
        }
        if p_expr_in.as_ref().map_or(false, |e| e.op == TK_ID) {
            // O valor de uma coluna gerada precisa ser uma expressão de verdade, não só uma
            // referência a outra coluna, para as otimizações de índice cobridor funcionarem.
            // Se o valor não é uma expressão, vira uma com um "+" unário.
            p_expr_in = p_expr(p_parse, TK_UPLUS as i32, p_expr_in.take(), None);
        }
        if let Some(e) = p_expr_in.as_mut() {
            if e.op != TK_RAISE {
                e.aff_expr = tab.a_col[idx].affinity;
            }
        }
        // O C chama sqlite3ColumnSetExpr mesmo com pExpr nulo (falta de memória em PExpr).
        // A coluna é indicada pelo índice, porque a função precisa da tabela e da coluna.
        column_set_expr(p_parse, &mut tab, idx, p_expr_in.take());
    }

    if generated_error {
        // generated_error
        let name = match p_parse.p_new_table.as_ref() {
            Some(t) => {
                let tab = t.borrow();
                nul_terminated(&tab.a_col[(tab.n_col - 1) as usize].z_cn_name).to_vec()
            }
            None => Vec::new(),
        };
        let mut msg = b"error in generated column \"".to_vec();
        msg.extend_from_slice(&name);
        msg.push(b'"');
        error_msg(p_parse, &msg);
    }
    // generated_done
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    expr_delete(&db, p_expr_in);
}

/// Gera código que incrementa o schema cookie.
///
/// O schema cookie serve para saber quando o schema do banco mudou. A cada mudança de schema o
/// valor muda. Quando um processo lê o schema pela primeira vez, ele guarda o cookie; depois,
/// sempre que vai acessar o banco, confere se o cookie continua o mesmo.
///
/// O plano não é à prova de balas: o schema pode mudar várias vezes e o cookie voltar a um valor
/// anterior. Mas mudanças de schema são raras e a chance de repetir o valor é de 1 em 2^32.
pub fn change_cookie(p_parse: &mut Parse, i_db: i32) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let v = p_parse.p_vdbe.clone().expect("Vdbe do Parse");
    debug_assert!(schema_mutex_held(&db.borrow(), i_db, None) != 0);
    let cookie = {
        let db_ref = db.borrow();
        let schema = db_ref.a_db[i_db as usize].p_schema.as_ref().expect("schema do banco");
        let schema_cookie = schema.borrow().schema_cookie;
        1u32.wrapping_add(schema_cookie as u32)
    };
    vdbe_add_op3(
        &mut v.borrow_mut(),
        OP_SETCOOKIE as i32,
        i_db,
        BTREE_SCHEMA_VERSION as i32,
        cookie as i32,
    );
}

/// Mede quantos caracteres são necessários para escrever o identificador dado. O número inclui
/// as aspas, mas não o terminador nulo. A estimativa é conservadora: pode ser maior que o
/// necessário.
fn ident_length(z: &[u8]) -> i32 {
    let mut n: i32 = 0;
    for &c in nul_terminated(z) {
        if c == b'"' {
            n += 1;
        }
        n += 1;
    }
    n + 2
}

/// Acrescenta ao buffer de saída `z` o identificador `z_signed_ident`.
///
/// No C o buffer vem com um deslocamento `*pIdx`; aqui o deslocamento é o próprio tamanho do
/// `Vec`. Se o identificador só tem caracteres alfanuméricos, não começa com dígito e não é
/// palavra-chave SQL, é copiado como está. Caso contrário, é citado com aspas duplas.
fn ident_put(z: &mut Vec<u8>, z_signed_ident: &[u8]) {
    let z_ident = nul_terminated(z_signed_ident);
    let mut j = 0usize;
    while j < z_ident.len() {
        if !isalnum(z_ident[j]) && z_ident[j] != b'_' {
            break;
        }
        j += 1;
    }
    let need_quote = z_ident.first().map_or(false, |&c| isdigit(c))
        || keyword_code(z_ident, j as i32) != TK_ID as i32
        || j != z_ident.len()
        || j == 0;

    if need_quote {
        z.push(b'"');
    }
    for &c in z_ident {
        z.push(c);
        if c == b'"' {
            z.push(b'"');
        }
    }
    if need_quote {
        z.push(b'"');
    }
}

/// Gera um CREATE TABLE apropriado para a tabela dada. No C a memória vem de sqliteMalloc e é
/// liberada por quem chama; aqui o `Vec` devolvido é do chamador.
fn create_table_stmt(_db: &Sqlite3, p: &Table) -> Option<Vec<u8>> {
    const AZ_TYPE: [&[u8]; 6] = [
        b"",      // SQLITE_AFF_BLOB
        b" TEXT", // SQLITE_AFF_TEXT
        b" NUM",  // SQLITE_AFF_NUMERIC
        b" INT",  // SQLITE_AFF_INTEGER
        b" REAL", // SQLITE_AFF_REAL
        b" NUM",  // SQLITE_AFF_FLEXNUM
    ];

    let mut n: i32 = 0;
    for p_col in p.a_col.iter().take(p.n_col as usize) {
        n += ident_length(&p_col.z_cn_name) + 5;
    }
    n += ident_length(&p.z_name);
    let (mut z_sep, z_sep2, z_end): (&[u8], &[u8], &[u8]) = if n < 50 {
        (b"", b",", b")")
    } else {
        (b"\n  ", b",\n  ", b"\n)")
    };
    n += 35 + 6 * p.n_col as i32;

    let mut z_stmt: Vec<u8> = Vec::with_capacity(n as usize);
    z_stmt.extend_from_slice(b"CREATE TABLE ");
    ident_put(&mut z_stmt, &p.z_name);
    z_stmt.push(b'(');
    for p_col in p.a_col.iter().take(p.n_col as usize) {
        z_stmt.extend_from_slice(z_sep);
        z_sep = z_sep2;
        ident_put(&mut z_stmt, &p_col.z_cn_name);
        debug_assert!(p_col.affinity >= SQLITE_AFF_BLOB);
        debug_assert!(((p_col.affinity - SQLITE_AFF_BLOB) as usize) < AZ_TYPE.len());
        let z_type = AZ_TYPE[(p_col.affinity - SQLITE_AFF_BLOB) as usize];
        z_stmt.extend_from_slice(z_type);
        debug_assert!(z_stmt.len() as i32 <= n);
    }
    z_stmt.extend_from_slice(z_end);
    Some(z_stmt)
}

/// Redimensiona um objeto Index para ter N colunas no total. Devolve SQLITE_OK em sucesso.
///
/// No C as quatro tabelas (azColl, aiRowLogEst, aiColumn, aSortOrder) passam a viver numa só
/// alocação; aqui cada `Vec` cresce até N entradas com as novas zeradas.
fn resize_index_object(_db: &Sqlite3, p_idx: &mut Index, n: i32) -> i32 {
    if (p_idx.n_column as i32) >= n {
        return SQLITE_OK;
    }
    debug_assert!(!p_idx.is_resized);
    let n = n as usize;
    p_idx.az_coll.truncate(p_idx.n_column as usize);
    p_idx.az_coll.resize(n, Vec::new());
    p_idx.ai_row_log_est.truncate(p_idx.n_key_col as usize + 1);
    p_idx.ai_row_log_est.resize(n, 0);
    p_idx.ai_column.truncate(p_idx.n_column as usize);
    p_idx.ai_column.resize(n, 0);
    p_idx.a_sort_order.truncate(p_idx.n_column as usize);
    p_idx.a_sort_order.resize(n, 0);
    p_idx.n_column = n as u16;
    p_idx.is_resized = true;
    SQLITE_OK
}

/// Estima a largura total de uma linha da tabela.
fn estimate_table_width(p_tab: &mut Table) {
    let mut w_table: u32 = 0;
    for p_tab_col in p_tab.a_col.iter().take(p_tab.n_col as usize) {
        w_table = w_table.wrapping_add(p_tab_col.sz_est as u32);
    }
    if p_tab.i_p_key < 0 {
        w_table = w_table.wrapping_add(1);
    }
    p_tab.sz_tab_row = log_est(w_table.wrapping_mul(4) as u64);
}

/// Estima o tamanho médio de uma linha do índice.
fn estimate_index_width(p_idx: &mut Index) {
    let mut w_index: u32 = 0;
    {
        let p_table = p_idx.p_table.upgrade().expect("tabela do índice");
        let tab = p_table.borrow();
        for i in 0..p_idx.n_column as usize {
            let x = p_idx.ai_column[i];
            debug_assert!((x as i32) < tab.n_col as i32);
            w_index = w_index.wrapping_add(if x < 0 { 1 } else { tab.a_col[x as usize].sz_est as u32 });
        }
    }
    p_idx.sz_idx_row = log_est(w_index.wrapping_mul(4) as u64);
}

/// Verdadeiro se o número de coluna `x` é qualquer uma das primeiras `n_col` entradas de
/// `ai_col`. Serve para saber se a coluna `x` aparece nas primeiras `n_col` entradas de um
/// índice.
fn has_column(ai_col: &[i16], n_col: i32, x: i32) -> bool {
    for k in 0..n_col.max(0) as usize {
        if x == ai_col[k] as i32 {
            return true;
        }
    }
    false
}

/// Verdadeiro se alguma das primeiras `n_key` entradas do índice `p_idx` casa exatamente com a
/// entrada `i_col` de `p_pk`. `p_pk` é sempre o índice PRIMARY KEY de uma tabela WITHOUT ROWID;
/// `p_idx` é um índice da mesma tabela, que pode ou não ser o próprio `p_pk`.
///
/// As primeiras `n_key` entradas de `p_idx` são colunas comuns, nunca rowid nem expressão.
///
/// Difere de `has_column()` porque aqui a coluna e a sequência de collation precisam casar; lá
/// só o número da coluna.
fn is_dup_column(p_idx: &Index, n_key: i32, p_pk: &Index, i_col: i32) -> bool {
    debug_assert!(n_key <= p_idx.n_column as i32);
    debug_assert!(i_col < (p_pk.n_column as i32).max(p_pk.n_key_col as i32));
    debug_assert!(p_pk.idx_type == SQLITE_IDXTYPE_PRIMARYKEY);
    debug_assert!(p_pk.p_table.ptr_eq(&p_idx.p_table));
    let j = p_pk.ai_column[i_col as usize];
    debug_assert!(j != XN_ROWID && j != XN_EXPR);
    for i in 0..n_key as usize {
        debug_assert!(p_idx.ai_column[i] >= 0 || j >= 0);
        if p_idx.ai_column[i] == j
            && str_i_cmp(nul_terminated(&p_idx.az_coll[i]), nul_terminated(&p_pk.az_coll[i_col as usize])) == 0
        {
            return true;
        }
    }
    false
}

/// Recalcula o campo colNotIdxed do Index.
///
/// colNotIdxed é uma máscara com bit 0 para cada coluna indexada entre as 63 primeiras da
/// tabela e 1 para todos os outros bits (as colunas fora do índice). O bit mais alto é sempre 1.
/// Toda coluna não indexada da tabela tem 1.
///
/// 2019-10-24: para este cálculo, colunas virtuais não contam como cobertas pelo índice, mesmo
/// estando nele, porque não se confia que `whereIndexExprTrans()` ache todas as referências à
/// coluna da tabela e as converta em referências ao índice. Por isso sempre se quer a tabela de
/// verdade à mão para recalcular a coluna virtual, se preciso.
///
/// A máscara é combinada com `SrcList.a[].colUsed` por AND para decidir se o índice é cobridor.
fn recompute_columns_not_indexed(p_idx: &mut Index) {
    let mut m: Bitmask = 0;
    {
        let p_tab = p_idx.p_table.upgrade().expect("tabela do índice");
        let tab = p_tab.borrow();
        for j in (0..p_idx.n_column as i32).rev() {
            let x = p_idx.ai_column[j as usize] as i32;
            if x >= 0 && (tab.a_col[x as usize].col_flags & COLFLAG_VIRTUAL) == 0 {
                if x < BMS - 1 {
                    m |= maskbit(x as u32);
                }
            }
        }
    }
    p_idx.col_not_idxed = !m;
    debug_assert!((p_idx.col_not_idxed >> 63) == 1); // Veja note-20221022-a
}

/// Roda no fim da análise de um CREATE TABLE com cláusula WITHOUT ROWID. Converte as estruturas
/// de schema em memória e o código VDBE gerado para os de uma tabela WITHOUT ROWID em vez de uma
/// tabela com rowid. As mudanças são:
///
///   (1) Marcar todas as colunas do PRIMARY KEY como NOT NULL.
///   (2) Converter o P3 do OP_CreateBtree de BTREE_INTKEY para BTREE_BLOBKEY.
///   (3) Pular a criação da entrada do sqlite_schema para o PRIMARY KEY, já que o índice da
///       chave primária passa a ser identificado pela entrada da própria tabela.
///   (4) Pôr no Index.tnum do índice PRIMARY KEY a rootpage da tabela.
///   (5) Acrescentar todas as colunas da tabela ao índice PRIMARY KEY, para ele ser cobridor. As
///       colunas extras fazem parte de KeyInfo.nAllField e não entram em ordenação, busca nem
///       checagem de unicidade.
///   (6) Trocar o rabo de rowid de todos os índices UNIQUE gerados automaticamente pelas colunas
///       do PRIMARY KEY.
///
/// Para tabelas virtuais só o item (1) vale.
///
/// A tabela vem como `TableRef` (e não como `&mut Table`) porque `create_index` e
/// `recompute_columns_not_indexed` precisam emprestar a mesma tabela; os empréstimos aqui são
/// todos curtos.
fn convert_to_without_rowid_table(p_parse: &mut Parse, p_tab: &TableRef) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let v = p_parse.p_vdbe.clone();
    let imposter_table = db.borrow().init.imposter_table != 0;

    // Marca toda coluna do PRIMARY KEY como NOT NULL (menos nas tabelas impostoras).
    if !imposter_table {
        let mut tab = p_tab.borrow_mut();
        for i in 0..tab.n_col as usize {
            if (tab.a_col[i].col_flags & COLFLAG_PRIMKEY) != 0 && tab.a_col[i].not_null == OE_NONE {
                tab.a_col[i].not_null = OE_ABORT;
            }
        }
        tab.tab_flags |= TF_HASNOTNULL;
    }

    // Converte o operando P3 do OP_CreateBtree de BTREE_INTKEY para BTREE_BLOBKEY.
    debug_assert!(p_parse.b_returning == 0);
    if p_parse.u1_addr_cr_tab != 0 {
        let v = v.as_ref().expect("Vdbe do Parse");
        vdbe_change_p3(&mut v.borrow_mut(), p_parse.u1_addr_cr_tab, BTREE_BLOBKEY as i32);
    }

    // Localiza o índice PRIMARY KEY. Ou, se a tabela era originalmente INTEGER PRIMARY KEY,
    // cria um índice PRIMARY KEY novo.
    let i_p_key = p_tab.borrow().i_p_key;
    let p_pk: IndexRef;
    if i_p_key >= 0 {
        let name = nul_terminated(&p_tab.borrow().a_col[i_p_key as usize].z_cn_name).to_vec();
        let mut ipk_token = Token { z: Vec::new(), n: 0 };
        token_init(&mut ipk_token, &name);
        let p_id = expr_alloc(&mut db.borrow_mut(), TK_ID as i32, Some(&ipk_token), false);
        let mut p_list = match expr_list_append(p_parse, None, p_id) {
            Some(l) => l,
            None => {
                p_tab.borrow_mut().tab_flags &= !TF_WITHOUTROWID;
                return;
            }
        };
        if in_rename_object(p_parse) {
            // Desvia o token do nome da coluna INTEGER PRIMARY KEY para a nova expressão.
            rename_token_remap_expr_from_ipkey(p_parse, p_list.a[0].p_expr.as_deref(), p_tab);
        }
        p_list.a[0].fg.sort_flags = p_parse.i_pk_sort_order;
        debug_assert!(p_parse.p_new_table.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_tab)));
        p_tab.borrow_mut().i_p_key = -1;
        let key_conf = p_tab.borrow().key_conf;
        create_index(
            p_parse,
            None,
            None,
            None,
            Some(p_list), // create_index toma a posse da lista, como no C
            key_conf as i32,
            None,
            None,
            0,
            0,
            SQLITE_IDXTYPE_PRIMARYKEY,
        );
        if p_parse.n_err > 0 {
            p_tab.borrow_mut().tab_flags &= !TF_WITHOUTROWID;
            return;
        }
        debug_assert!(db.borrow().malloc_failed == 0);
        p_pk = primary_key_index(Some(&p_tab.borrow())).expect("índice PRIMARY KEY");
        debug_assert!(p_pk.borrow().n_key_col == 1);
    } else {
        p_pk = primary_key_index(Some(&p_tab.borrow())).expect("índice PRIMARY KEY");

        // Remove do PRIMARY KEY todas as colunas redundantes. Por exemplo, troca
        // "PRIMARY KEY(a,b,a,b,c,b,c,d)" por só "PRIMARY KEY(a,b,c,d)". O código adiante supõe
        // que o PRIMARY KEY não repete colunas.
        let mut pk = p_pk.borrow_mut();
        let mut i: usize = 1;
        let mut j: usize = 1;
        while i < pk.n_key_col as usize {
            if is_dup_column(&pk, j as i32, &pk, i as i32) {
                pk.n_column -= 1;
            } else {
                let coll = pk.az_coll[i].clone();
                pk.az_coll[j] = coll;
                pk.a_sort_order[j] = pk.a_sort_order[i];
                pk.ai_column[j] = pk.ai_column[i];
                j += 1;
            }
            i += 1;
        }
        pk.n_key_col = j as u16;
    }

    let n_pk: i32;
    {
        let mut pk = p_pk.borrow_mut();
        pk.is_covering = true;
        if !imposter_table {
            pk.uniq_not_null = true;
        }
        pk.n_column = pk.n_key_col;
        n_pk = pk.n_column as i32;

        // Pula a criação da btree do PRIMARY KEY e da entrada do sqlite_schema. Só se faz
        // quando se gera código VDBE para um CREATE TABLE (não ao ler o schema de um banco).
        if let Some(v) = v.as_ref() {
            if pk.tnum > 0 {
                debug_assert!(db.borrow().init.busy == 0);
                vdbe_change_opcode(&mut v.borrow_mut(), pk.tnum as i32, OP_GOTO);
            }
        }

        // A página raiz do PRIMARY KEY é a página raiz da tabela.
        pk.tnum = p_tab.borrow().tnum;
    }

    // Atualiza a representação em memória de todos os índices UNIQUE, convertendo a última
    // coluna de rowid em uma ou mais colunas do PRIMARY KEY.
    let mut p_idx_cur = p_tab.borrow().p_index.clone();
    while let Some(p_idx) = p_idx_cur {
        p_idx_cur = p_idx.borrow().p_next.clone();
        let mut idx = p_idx.borrow_mut();
        if is_primary_key_index(&idx) {
            continue;
        }
        let n_key_col = idx.n_key_col as i32;
        let mut n: i32 = 0;
        {
            let pk = p_pk.borrow();
            for i in 0..n_pk {
                if !is_dup_column(&idx, n_key_col, &pk, i) {
                    n += 1;
                }
            }
        }
        if n == 0 {
            // Este índice é um superconjunto do PRIMARY KEY.
            idx.n_column = idx.n_key_col;
            continue;
        }
        if resize_index_object(&db.borrow(), &mut idx, n_key_col + n) != SQLITE_OK {
            return;
        }
        let pk = p_pk.borrow();
        let mut j = idx.n_key_col as usize;
        for i in 0..n_pk {
            if !is_dup_column(&idx, n_key_col, &pk, i) {
                idx.ai_column[j] = pk.ai_column[i as usize];
                idx.az_coll[j] = pk.az_coll[i as usize].clone();
                if pk.a_sort_order[i as usize] != 0 {
                    // Veja o ticket https://www.sqlite.org/src/info/bba7b69f9849b5bf
                    idx.b_asc_key_bug = true;
                }
                j += 1;
            }
        }
        debug_assert!(idx.n_column as i32 >= n_key_col + n);
        debug_assert!(idx.n_column as usize >= j);
    }

    // Acrescenta todas as colunas da tabela ao índice PRIMARY KEY.
    let n_col = p_tab.borrow().n_col as i32;
    let n_extra = {
        let tab = p_tab.borrow();
        let pk = p_pk.borrow();
        let mut n_extra: i32 = 0;
        for i in 0..n_col {
            if !has_column(&pk.ai_column, n_pk, i)
                && (tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) == 0
            {
                n_extra += 1;
            }
        }
        n_extra
    };
    if resize_index_object(&db.borrow(), &mut p_pk.borrow_mut(), n_pk + n_extra) != SQLITE_OK {
        return;
    }
    {
        let tab = p_tab.borrow();
        let mut pk = p_pk.borrow_mut();
        let mut j = n_pk;
        for i in 0..n_col {
            if !has_column(&pk.ai_column, j, i)
                && (tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) == 0
            {
                debug_assert!(j < pk.n_column as i32);
                pk.ai_column[j as usize] = i as i16;
                pk.az_coll[j as usize] = SQLITE_STR_BINARY.to_vec();
                j += 1;
            }
        }
        debug_assert!(pk.n_column as i32 == j);
        debug_assert!(tab.n_nv_col as i32 <= j);
    }
    recompute_columns_not_indexed(&mut p_pk.borrow_mut());
}


// ---- part_006.rs ----

/// Verifica se p_tab é uma tabela virtual e z_name é um nome de tabela de sombra
/// para essa tabela virtual.
pub fn is_shadow_table_of(db: &Sqlite3, p_tab: &Table, z_name: &[u8]) -> bool {
    if !is_virtual(p_tab) {
        return false;
    }
    let n_name = p_tab.z_name.len();
    if str_n_i_cmp(z_name, &p_tab.z_name, n_name as i32) != 0 {
        return false;
    }
    if z_name.get(n_name) != Some(&b'_') {
        return false;
    }
    let p_mod = match hash_find(&db.a_module, &p_tab.u.vtab.az_arg[0]) {
        Some(m) => m,
        None => return false,
    };
    let p_mod = p_mod.borrow();
    let p_module = match p_mod.p_module.as_ref() {
        Some(m) => m,
        None => return false,
    };
    if p_module.i_version < 3 {
        return false;
    }
    match p_module.x_shadow_name {
        None => false,
        Some(x_shadow_name) => x_shadow_name(&z_name[n_name + 1..]),
    }
}

/// A tabela p_tab é uma tabela virtual. Se a implementação da tabela virtual
/// existe e tem um método xShadowName, percorre todas as outras tabelas comuns
/// do mesmo schema procurando tabelas de sombra de p_tab, e marca com a flag
/// TF_SHADOW cada uma que encontrar.
pub fn mark_all_shadow_tables_of(db: &Sqlite3, p_tab: &Table) {
    debug_assert!(is_virtual(p_tab));
    let p_mod = match hash_find(&db.a_module, &p_tab.u.vtab.az_arg[0]) {
        Some(m) => m,
        None => return,
    };
    let x_shadow_name = {
        let p_mod = p_mod.borrow();
        let p_module = match p_mod.p_module.as_ref() {
            Some(m) => m,
            None => return,
        };
        if p_module.i_version < 3 {
            return;
        }
        match p_module.x_shadow_name {
            Some(f) => f,
            None => return,
        }
    };
    debug_assert!(!p_tab.z_name.is_empty());
    let n_name = p_tab.z_name.len();
    let p_schema = p_tab.p_schema.clone();
    let tbl_hash = &p_schema.borrow().tbl_hash;
    let mut k = sqlite_hash_first(tbl_hash);
    while let Some(elem) = k {
        let p_other = sqlite_hash_data(&elem);
        k = sqlite_hash_next(tbl_hash, &elem);
        let mut other = p_other.borrow_mut();
        debug_assert!(!other.z_name.is_empty());
        if !is_ordinary_table(&other) {
            continue;
        }
        if (other.tab_flags & TF_SHADOW) != 0 {
            continue;
        }
        if str_n_i_cmp(&other.z_name, &p_tab.z_name, n_name as i32) == 0
            && other.z_name.get(n_name) == Some(&b'_')
            && x_shadow_name(&other.z_name[n_name + 1..])
        {
            other.tab_flags |= TF_SHADOW;
        }
    }
}

/// Verifica se z_name é um nome de tabela de sombra na conexão de banco de dados
/// atual. O C troca temporariamente o último "_" por NUL; aqui se procura a tabela
/// pelo prefixo e o nome original fica intacto.
pub fn shadow_table_name(db: &Sqlite3, z_name: &[u8]) -> bool {
    let z_tail = match z_name.iter().rposition(|&b| b == b'_') {
        Some(t) => t,
        None => return false,
    };
    let p_tab = match find_table(db, &z_name[..z_tail], None) {
        Some(t) => t,
        None => return false,
    };
    let p_tab = p_tab.borrow();
    if !is_virtual(&p_tab) {
        return false;
    }
    is_shadow_table_of(db, &p_tab, z_name)
}

/// Esta rotina é chamada para relatar o ")" final que termina uma instrução
/// CREATE TABLE.
///
/// A estrutura da tabela que as outras rotinas de ação vêm montando é
/// acrescentada às tabelas hash internas, desde que não tenha havido erro.
///
/// Uma entrada para a tabela é gravada na tabela de schema em disco, a menos que
/// seja uma tabela temporária ou db.init.busy==1. Com db.init.busy==1 estamos
/// lendo a tabela sqlite_schema (acabamos de conectar ou ela mudou), então a
/// entrada desta tabela já existe lá e não deve ser criada de novo.
///
/// Se p_select não é None, a rotina foi chamada para criar uma tabela de um
/// "CREATE TABLE ... AS SELECT ...". Os nomes de coluna da nova tabela casam com
/// o conjunto de resultados do SELECT.
///
/// Os campos `z` dos Token são sufixos do texto SQL de entrada a partir do token,
/// então a diferença de dois ponteiros do C é a diferença dos comprimentos.
pub fn end_table(
    p_parse: &mut Parse,
    p_cons: Option<&Token>,
    p_end: Option<&Token>,
    tab_opts: u32,
    mut p_select: Option<&mut Select>,
) {
    if p_end.is_none() && p_select.is_none() {
        return;
    }
    let p = match p_parse.p_new_table.clone() {
        Some(p) => p,
        None => return,
    };
    let db_ref = p_parse.db.upgrade().expect("conexão do Parse");

    if p_select.is_none() && shadow_table_name(&db_ref.borrow(), &p.borrow().z_name) {
        p.borrow_mut().tab_flags |= TF_SHADOW;
    }

    // Se db.init.busy é 1, estamos lendo o SQL da tabela "sqlite_schema" ou
    // "sqlite_temp_schema" em disco, então não se grava de novo. O número da página
    // raiz vem de db.init.new_tnum (posto lá pela rotina sqliteOpenCb).
    //
    // Se a página raiz é 1, esta é a própria tabela sqlite_schema: marca somente leitura.
    let (init_busy, new_tnum) = {
        let d = db_ref.borrow();
        (d.init.busy != 0, d.init.new_tnum)
    };
    if init_busy {
        if p_select.is_some() || (!is_ordinary_table(&p.borrow()) && new_tnum != 0) {
            error_msg(p_parse, b"");
            return;
        }
        let mut t = p.borrow_mut();
        t.tnum = new_tnum;
        if t.tnum == 1 {
            t.tab_flags |= TF_READONLY;
        }
    }

    // Tratamento especial das tabelas com a palavra-chave STRICT:
    //
    //   * Não permite tipos de dado personalizados. Toda coluna precisa ter um tipo
    //     entre INT, INTEGER, REAL, TEXT ou BLOB.
    //
    //   * Se há PRIMARY KEY que não seja o INTEGER PRIMARY KEY, todas as colunas dela
    //     precisam ter restrição NOT NULL.
    if (tab_opts & TF_STRICT) != 0 {
        p.borrow_mut().tab_flags |= TF_STRICT;
        let n_col = p.borrow().n_col as usize;
        for ii in 0..n_col {
            let (e_c_type, col_flags, z_name, z_cn_name, ty) = {
                let t = p.borrow();
                let c = &t.a_col[ii];
                (c.e_c_type, c.col_flags, t.z_name.clone(), c.z_cn_name.clone(), column_type(c, b""))
            };
            if e_c_type == COLTYPE_CUSTOM {
                if (col_flags & COLFLAG_HASTYPE) != 0 {
                    error_msg(
                        p_parse,
                        &[
                            &b"unknown datatype for "[..],
                            &z_name[..],
                            &b"."[..],
                            &z_cn_name[..],
                            &b": \""[..],
                            &ty[..],
                            &b"\""[..],
                        ]
                        .concat(),
                    );
                } else {
                    error_msg(
                        p_parse,
                        &[&b"missing datatype for "[..], &z_name[..], &b"."[..], &z_cn_name[..]].concat(),
                    );
                }
                return;
            } else if e_c_type == COLTYPE_ANY {
                p.borrow_mut().a_col[ii].affinity = SQLITE_AFF_BLOB;
            }
            let mut t = p.borrow_mut();
            if (col_flags & COLFLAG_PRIMKEY) != 0
                && t.i_p_key as i32 != ii as i32
                && t.a_col[ii].not_null == OE_NONE
            {
                t.a_col[ii].not_null = OE_ABORT;
                t.tab_flags |= TF_HASNOTNULL;
            }
        }
    }

    {
        let t = p.borrow();
        debug_assert!(
            (t.tab_flags & TF_HASPRIMARYKEY) == 0 || t.i_p_key >= 0 || primary_key_index(&t).is_some()
        );
        debug_assert!(
            (t.tab_flags & TF_HASPRIMARYKEY) != 0 || (t.i_p_key < 0 && primary_key_index(&t).is_none())
        );
    }

    // Tratamento especial das tabelas WITHOUT ROWID
    if (tab_opts & TF_WITHOUTROWID) != 0 {
        let flags = p.borrow().tab_flags;
        if (flags & TF_AUTOINCREMENT) != 0 {
            error_msg(p_parse, b"AUTOINCREMENT not allowed on WITHOUT ROWID tables");
            return;
        }
        if (flags & TF_HASPRIMARYKEY) == 0 {
            let name = p.borrow().z_name.clone();
            error_msg(p_parse, &[&b"PRIMARY KEY missing on table "[..], &name[..]].concat());
            return;
        }
        p.borrow_mut().tab_flags |= TF_WITHOUTROWID | TF_NOVISIBLEROWID;
        convert_to_without_rowid_table(p_parse, &p);
    }
    let i_db = schema_to_index(&db_ref.borrow(), &p.borrow().p_schema);

    // Resolve os nomes em todas as expressões de restrição CHECK. A lista sai da
    // tabela durante a resolução, porque resolve_self_reference empresta a tabela.
    let mut p_check = p.borrow_mut().p_check.take();
    if p_check.is_some() {
        resolve_self_reference(p_parse, &p, NC_ISCHECK, None, p_check.as_deref_mut());
        if p_parse.n_err != 0 {
            // Se houve erro, apaga as restrições CHECK agora, senão elas poderiam ser
            // usadas de fato com PRAGMA writable_schema=ON.
            expr_list_delete(&db_ref, p_check.take());
        }
        // A marcação EP_Immutable (markExprListImmutable) só existe com SQLITE_DEBUG.
        p.borrow_mut().p_check = p_check;
    }

    if (p.borrow().tab_flags & TF_HASGENERATED) != 0 {
        let mut n_ng = 0;
        let n_col = p.borrow().n_col as usize;
        for ii in 0..n_col {
            let col_flags = p.borrow().a_col[ii].col_flags;
            if (col_flags & COLFLAG_GENERATED) != 0 {
                let mut p_x = column_take_expr(&mut p.borrow_mut(), ii);
                let rc = resolve_self_reference(p_parse, &p, NC_GENCOL, p_x.as_deref_mut(), None);
                let mut t = p.borrow_mut();
                if rc != 0 {
                    // Se há erro ao resolver a expressão, troca por NULL. Isso evita que
                    // geradores de código que operam na expressão insiram nela partes
                    // alocadas da lookaside, o que é ilegal num schema e leva a erros ou
                    // corrupção de heap quando a conexão fecha.
                    let p_null = expr_alloc(&db_ref, TK_NULL, None, 0);
                    column_set_expr(p_parse, &mut t, ii, p_null);
                } else {
                    column_put_expr(&mut t, ii, p_x);
                }
            } else {
                n_ng += 1;
            }
        }
        if n_ng == 0 {
            error_msg(p_parse, b"must have at least one non-generated column");
            return;
        }
    }

    // Estima o tamanho médio da linha da tabela e de todos os índices implícitos
    estimate_table_width(&mut p.borrow_mut());
    let mut p_idx = p.borrow().p_index.clone();
    while let Some(idx) = p_idx {
        estimate_index_width(&mut idx.borrow_mut());
        p_idx = idx.borrow().p_next.clone();
    }

    // Se não está inicializando, cria o registro da nova tabela na tabela de schema
    // do banco. Se a tabela é TEMPORARY, a entrada vai para o arquivo auxiliar.
    if !init_busy {
        let v = match get_vdbe(p_parse) {
            Some(v) => v,
            None => return,
        };
        vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE, 0);

        // Inicializa z_type da nova view ou tabela.
        let (z_type, z_type2): (&[u8], &[u8]) = if is_ordinary_table(&p.borrow()) {
            (b"table", b"TABLE")
        } else {
            (b"view", b"VIEW")
        };

        // Se é um CREATE TABLE xx AS SELECT ..., executa o SELECT para popular a nova
        // tabela. O número da página raiz da nova tabela está no registrador
        // p_parse.reg_root.
        //
        // Depois que o SELECT foi codificado por select(), ele está em estado adequado
        // para consultar os nomes e tipos de coluna da nova tabela.
        //
        // Não é preciso trava de escrita de cache compartilhado para escrever na nova
        // tabela: a trava de schema já foi obtida para criá-la e exclui os demais usuários.
        if let Some(sel) = p_select.as_deref_mut() {
            if in_special_parse(p_parse) {
                p_parse.rc = SQLITE_ERROR;
                p_parse.n_err += 1;
                return;
            }
            let i_csr = p_parse.n_tab;
            p_parse.n_tab += 1;
            p_parse.n_mem += 1;
            let reg_yield = p_parse.n_mem;
            p_parse.n_mem += 1;
            let reg_rec = p_parse.n_mem;
            p_parse.n_mem += 1;
            let reg_rowid = p_parse.n_mem;
            may_abort(p_parse);
            vdbe_add_op3(&mut v.borrow_mut(), OP_OPENWRITE, i_csr, p_parse.reg_root, i_db);
            vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_P2ISREG);
            let addr_top = vdbe_current_addr(&v.borrow()) + 1;
            vdbe_add_op3(&mut v.borrow_mut(), OP_INITCOROUTINE, reg_yield, 0, addr_top);
            if p_parse.n_err != 0 {
                return;
            }
            let p_sel_tab = match result_set_of_select(p_parse, sel, SQLITE_AFF_BLOB) {
                Some(t) => t,
                None => return,
            };
            {
                let mut t = p.borrow_mut();
                debug_assert!(t.a_col.is_empty());
                let mut st = p_sel_tab.borrow_mut();
                t.n_col = st.n_col;
                t.n_nv_col = st.n_col;
                t.a_col = std::mem::take(&mut st.a_col);
                st.n_col = 0;
            }
            delete_table(&db_ref, Some(p_sel_tab));
            let mut dest = SelectDest::default();
            select_dest_init(&mut dest, SRT_COROUTINE, reg_yield);
            select(p_parse, sel, &mut dest);
            if p_parse.n_err != 0 {
                return;
            }
            vdbe_end_coroutine(&mut v.borrow_mut(), reg_yield);
            vdbe_jump_here(&mut v.borrow_mut(), addr_top - 1);
            let addr_ins_loop = vdbe_add_op1(&mut v.borrow_mut(), OP_YIELD, dest.i_sd_parm);
            vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD, dest.i_sdst, dest.n_sdst, reg_rec);
            table_affinity(&mut v.borrow_mut(), &p.borrow(), 0);
            vdbe_add_op2(&mut v.borrow_mut(), OP_NEWROWID, i_csr, reg_rowid);
            vdbe_add_op3(&mut v.borrow_mut(), OP_INSERT, i_csr, reg_rec, reg_rowid);
            vdbe_goto(&mut v.borrow_mut(), addr_ins_loop);
            vdbe_jump_here(&mut v.borrow_mut(), addr_ins_loop);
            vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE, i_csr);
        }

        // Calcula o texto completo do comando CREATE
        let z_stmt: Option<Vec<u8>> = if p_select.is_some() {
            create_table_stmt(&db_ref.borrow(), &p.borrow())
        } else {
            let p_end2 = if tab_opts != 0 { &p_parse.s_last_token } else { p_end.unwrap() };
            let mut n = p_parse.s_name_token.z.len() - p_end2.z.len();
            if p_end2.z.first() != Some(&b';') {
                n += p_end2.n as usize;
            }
            // "CREATE %s %.*s"
            Some([&b"CREATE "[..], z_type2, &b" "[..], &p_parse.s_name_token.z[..n]].concat())
        };

        // Um espaço para o registro já foi alocado na tabela de schema. Só falta
        // atualizar esse espaço com tudo o que foi coletado.
        let z_db_s_name = db_ref.borrow().a_db[i_db as usize].z_db_s_name.clone();
        let z_name = p.borrow().z_name.clone();
        let reg_root = p_parse.reg_root;
        let reg_rowid = p_parse.reg_rowid;
        nested_parse(
            p_parse,
            b"UPDATE %Q.sqlite_master SET type='%s', name=%Q, tbl_name=%Q, rootpage=#%d, sql=%Q WHERE rowid=#%d",
            &[
                PrintfArg::Str(&z_db_s_name),
                PrintfArg::Str(z_type),
                PrintfArg::Str(&z_name),
                PrintfArg::Str(&z_name),
                PrintfArg::Int(reg_root),
                match z_stmt.as_deref() {
                    Some(s) => PrintfArg::Str(s),
                    None => PrintfArg::Null,
                },
                PrintfArg::Int(reg_rowid),
            ],
        );
        change_cookie(p_parse, i_db);

        // Vê se é preciso criar a tabela sqlite_sequence para guardar as chaves
        // autoincrement.
        if (p.borrow().tab_flags & TF_AUTOINCREMENT) != 0 && !in_special_parse(p_parse) {
            debug_assert!(schema_mutex_held(&db_ref.borrow(), i_db, None));
            let sem_seq = {
                let d = db_ref.borrow();
                let schema = d.a_db[i_db as usize].p_schema.clone();
                let s = schema.borrow();
                s.p_seq_tab.is_none()
            };
            if sem_seq {
                nested_parse(
                    p_parse,
                    b"CREATE TABLE %Q.sqlite_sequence(name,seq)",
                    &[PrintfArg::Str(&z_db_s_name)],
                );
            }
        }

        // Relê tudo para atualizar as estruturas internas
        let z_where = mprintf(&db_ref, b"tbl_name='%q' AND type!='trigger'", &[PrintfArg::Str(&z_name)]);
        vdbe_add_parse_schema_op(&mut v.borrow_mut(), i_db, z_where, 0);

        // Testa ciclos em colunas geradas e expressões ilegais em restrições CHECK e
        // em cláusulas DEFAULT.
        if (p.borrow().tab_flags & TF_HASGENERATED) != 0 {
            let z_sql = mprintf(
                &db_ref,
                b"SELECT*FROM\"%w\".\"%w\"",
                &[PrintfArg::Str(&z_db_s_name), PrintfArg::Str(&z_name)],
            );
            vdbe_add_op4(&mut v.borrow_mut(), OP_SQLEXEC, 0x0001, 0, 0, P4::Dynamic(z_sql));
        }
    }

    // Acrescenta a tabela à representação em memória do banco de dados.
    if init_busy {
        let p_schema = p.borrow().p_schema.clone();
        debug_assert!(schema_mutex_held(&db_ref.borrow(), i_db, None));
        debug_assert!(has_rowid(&p.borrow()) || p.borrow().i_p_key < 0);
        let z_name = p.borrow().z_name.clone();
        let p_old = hash_insert(&mut p_schema.borrow_mut().tbl_hash, &z_name, Some(p.clone()));
        if p_old.is_some() {
            // O malloc deve ter falhado dentro de hash_insert()
            debug_assert!(p_old.as_ref().map_or(false, |o| Rc::ptr_eq(o, &p)));
            oom_fault(&mut db_ref.borrow_mut());
            return;
        }
        p_parse.p_new_table = None;
        db_ref.borrow_mut().m_db_flags |= DBFLAG_SCHEMACHANGE;

        // Se é a tabela mágica sqlite_sequence usada por autoincrement, guarda uma
        // referência a ela na estrutura principal para o INSERT achá-la fácil.
        debug_assert!(!p_parse.nested);
        if z_name == b"sqlite_sequence" {
            debug_assert!(schema_mutex_held(&db_ref.borrow(), i_db, None));
            p_schema.borrow_mut().p_seq_tab = Some(Rc::downgrade(&p));
        }
    }

    if p_select.is_none() && is_ordinary_table(&p.borrow()) {
        debug_assert!(p_cons.is_some() && p_end.is_some());
        let mut p_cons = p_cons.unwrap();
        if p_cons.z.is_empty() {
            p_cons = p_end.unwrap();
        }
        p.borrow_mut().u.tab.add_col_offset = 13 + (p_parse.s_name_token.z.len() - p_cons.z.len()) as i32;
    }
}


// ---- part_007.rs ----

/// O analisador chama esta rotina para criar uma nova VIEW.
///
/// Os campos `z` dos Token são sufixos do texto SQL de entrada a partir do token,
/// então a diferença de dois ponteiros do C é a diferença dos comprimentos.
pub fn create_view(
    p_parse: &mut Parse,
    p_begin: &Token,
    p_name1: &Token,
    p_name2: &Token,
    p_c_names: Option<Box<ExprList>>,
    p_select: Option<Box<Select>>,
    is_temp: i32,
    no_err: i32,
) {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let mut p_c_names = p_c_names;
    let mut p_select = p_select;

    'create_view_fail: {
        if p_parse.n_var > 0 {
            error_msg(p_parse, b"parameters are not allowed in views");
            break 'create_view_fail;
        }
        start_table(p_parse, p_name1, p_name2, is_temp, 1, 0, no_err);
        let p = match p_parse.p_new_table.clone() {
            Some(p) if p_parse.n_err == 0 => p,
            _ => break 'create_view_fail,
        };

        // Versões antigas do SQLite permitiam usar a coluna mágica "rowid" numa view,
        // embora views não tenham rowid. O sinalizador abaixo corrige isso.
        // SQLITE_ALLOW_ROWID_IN_VIEW não é definida no Debian.
        p.borrow_mut().tab_flags |= TF_NOVISIBLEROWID; // Nunca permite rowid em view

        let p_name = two_part_name(p_parse, p_name1, p_name2);
        let i_db = schema_to_index(&db.borrow(), &p.borrow().p_schema);
        let mut s_fix = DbFixer::default();
        fix_init(&mut s_fix, p_parse, i_db, b"view", p_name.as_ref());
        if fix_select(&mut s_fix, p_select.as_deref_mut()) != 0 {
            break 'create_view_fail;
        }

        // Faz uma cópia do SELECT inteiro que define a view. Isso força todos os valores
        // Expr.token.z a serem alocados dinamicamente em vez de apontar para o texto de
        // entrada, o que os faz persistir depois que a chamada de sqlite3_exec() retorna.
        p_select.as_mut().expect("select da view").sel_flags |= SF_VIEW;
        if in_rename_object(p_parse) {
            p.borrow_mut().u.view.p_select = p_select.take();
        } else {
            let dup = select_dup(&db, p_select.as_deref(), EXPRDUP_REDUCE);
            p.borrow_mut().u.view.p_select = dup;
        }
        let dup_names = expr_list_dup(&db, p_c_names.as_deref(), EXPRDUP_REDUCE);
        {
            let mut t = p.borrow_mut();
            t.p_check = dup_names;
            t.e_tab_type = TABTYP_VIEW;
        }
        if db.borrow().malloc_failed != 0 {
            break 'create_view_fail;
        }

        // Localiza o fim do CREATE VIEW. Faz s_end apontar para o fim.
        let mut s_end = p_parse.s_last_token.clone();
        debug_assert!(s_end.z.first().map_or(false, |&c| c != 0) || s_end.n == 0);
        if s_end.z.first() != Some(&b';') {
            s_end.z = s_end.z[(s_end.n as usize).min(s_end.z.len())..].to_vec();
        }
        s_end.n = 0;
        let mut n = p_begin.z.len() - s_end.z.len();
        debug_assert!(n > 0);
        let z = &p_begin.z;
        while isspace(z[n - 1]) {
            n -= 1;
        }
        s_end.z = z[n - 1..].to_vec();
        s_end.n = 1;

        // Usa end_table() para acrescentar a view à tabela de schema
        end_table(p_parse, None, Some(&s_end), 0, None);
    }

    // create_view_fail
    select_delete(&db, p_select);
    if in_rename_object(p_parse) {
        rename_exprlist_unmap(p_parse, p_c_names.as_deref());
    }
    expr_list_delete(&db, p_c_names);
}

/// A estrutura Table p_table é na verdade uma VIEW. Preenche em p_table os nomes
/// das colunas da view. Devolve não zero se houver erros; nesse caso a mensagem
/// fica em p_parse.z_err_msg.
///
/// O nome estático do C (viewGetColumnNames) colide com o público
/// (sqlite3ViewGetColumnNames) depois da conversão; aqui o estático leva `_impl`.
fn view_get_column_names_impl(p_parse: &mut Parse, p_table: &TableRef) -> i32 {
    let db = p_parse.db.upgrade().expect("conexão do Parse");
    let mut n_err = 0;

    if is_virtual(&p_table.borrow()) {
        db.borrow_mut().n_schema_lock += 1;
        let rc = vtab_call_connect(p_parse, p_table);
        db.borrow_mut().n_schema_lock -= 1;
        return rc;
    }

    // Um n_col positivo significa que os nomes das colunas desta view já são
    // conhecidos. Esta rotina só é chamada se a tabela é virtual ou n_col é zero.
    debug_assert!(p_table.borrow().n_col <= 0);

    // Um n_col negativo é um marcador especial: estamos calculando os nomes das
    // colunas. Entrar aqui com n_col negativo significa que duas ou mais views formam
    // um laço, como:
    //
    //     CREATE VIEW one AS SELECT * FROM two;
    //     CREATE VIEW two AS SELECT * FROM one;
    //
    // Na verdade esse erro já é pego antes de chegar aqui. Mas o teste continua
    // importante porque aparece em:
    //
    //     CREATE TABLE main.ex1(a);
    //     CREATE TEMP VIEW ex1 AS SELECT a FROM ex1;
    //     SELECT * FROM temp.ex1;
    if p_table.borrow().n_col < 0 {
        let name = p_table.borrow().z_name.clone();
        error_msg(p_parse, &[&b"view "[..], &name[..], &b" is circularly defined"[..]].concat());
        return 1;
    }
    debug_assert!(p_table.borrow().n_col >= 0);

    // Se chegou aqui, é preciso calcular os nomes da tabela. A chamada a
    // result_set_of_select() expande os elementos "*" do conjunto de resultados da
    // view e atribui cursores aos elementos da cláusula FROM. Essas mudanças não devem
    // ser permanentes, então o cálculo é feito numa cópia do SELECT que define a view.
    debug_assert!(is_view(&p_table.borrow()));
    let p_sel_src = p_table.borrow().u.view.p_select.clone();
    let p_sel = select_dup(&db, p_sel_src.as_deref(), 0);
    if let Some(mut p_sel) = p_sel {
        let e_parse_mode = p_parse.e_parse_mode;
        let n_tab = p_parse.n_tab;
        let n_select = p_parse.n_select;
        p_parse.e_parse_mode = PARSE_MODE_NORMAL;
        src_list_assign_cursors(p_parse, p_sel.p_src.as_deref_mut());
        p_table.borrow_mut().n_col = -1;
        disable_lookaside(&db);
        let x_auth = db.borrow_mut().x_auth.take();
        let p_sel_tab = result_set_of_select(p_parse, &mut p_sel, SQLITE_AFF_NONE);
        db.borrow_mut().x_auth = x_auth;
        p_parse.n_tab = n_tab;
        p_parse.n_select = n_select;
        match p_sel_tab {
            None => {
                p_table.borrow_mut().n_col = 0;
                n_err += 1;
                let mut t = p_table.borrow_mut();
                t.n_nv_col = t.n_col;
            }
            Some(p_sel_tab) => {
                let p_check = p_table.borrow_mut().p_check.take();
                if let Some(p_check) = p_check {
                    // CREATE VIEW name(arglist) AS ...
                    // Os nomes das colunas da tabela vêm de arglist, guardada em
                    // p_table.p_check. O campo p_check normalmente guarda restrições CHECK
                    // de uma tabela comum, mas numa VIEW guarda a lista de nomes de coluna.
                    let mut n_col: i16 = 0;
                    let mut a_col: Vec<Column> = Vec::new();
                    columns_from_expr_list(p_parse, &p_check, &mut n_col, &mut a_col);
                    {
                        let mut t = p_table.borrow_mut();
                        t.n_col = n_col;
                        t.a_col = a_col;
                        t.p_check = Some(p_check);
                    }
                    let n_expr = p_sel.p_e_list.as_ref().map_or(0, |l| l.n_expr);
                    if p_parse.n_err == 0 && p_table.borrow().n_col as i32 == n_expr as i32 {
                        debug_assert!(db.borrow().malloc_failed == 0);
                        subquery_column_types(p_parse, p_table, &p_sel, SQLITE_AFF_NONE);
                    }
                } else {
                    // CREATE VIEW name AS...  sem lista de argumentos. Monta os nomes das
                    // colunas a partir do SELECT que define a view.
                    let mut t = p_table.borrow_mut();
                    let mut st = p_sel_tab.borrow_mut();
                    debug_assert!(t.a_col.is_empty());
                    t.n_col = st.n_col;
                    t.a_col = std::mem::take(&mut st.a_col);
                    t.tab_flags |= (st.tab_flags & (COLFLAG_NOINSERT as u32)) as u32;
                    st.n_col = 0;
                    debug_assert!(schema_mutex_held(&db.borrow(), 0, Some(&t.p_schema)));
                }
                {
                    let mut t = p_table.borrow_mut();
                    t.n_nv_col = t.n_col;
                }
                delete_table(&db, Some(p_sel_tab));
            }
        }
        select_delete(&db, Some(p_sel));
        enable_lookaside(&db);
        p_parse.e_parse_mode = e_parse_mode;
    } else {
        n_err += 1;
    }
    p_table.borrow().p_schema.borrow_mut().schema_flags |= DB_UNRESETVIEWS;
    if db.borrow().malloc_failed != 0 {
        delete_column_names(&db, p_table);
    }
    n_err + p_parse.n_err
}

pub fn view_get_column_names(p_parse: &mut Parse, p_table: &TableRef) -> i32 {
    if !is_virtual(&p_table.borrow()) && p_table.borrow().n_col > 0 {
        return 0;
    }
    view_get_column_names_impl(p_parse, p_table)
}

/// Limpa os nomes de coluna de toda VIEW do banco de dados idx.
fn view_reset_all(db: &Sqlite3, idx: i32) {
    debug_assert!(schema_mutex_held(db, idx, None));
    if !db_has_property(db, idx, DB_UNRESETVIEWS) {
        return;
    }
    let p_schema = db.a_db[idx as usize].p_schema.clone().expect("schema do banco");
    let tbl_hash = &p_schema.borrow().tbl_hash;
    let mut i = sqlite_hash_first(tbl_hash);
    while let Some(elem) = i {
        let p_tab = sqlite_hash_data(&elem);
        i = sqlite_hash_next(tbl_hash, &elem);
        if is_view(&p_tab.borrow()) {
            delete_column_names(db, &p_tab);
        }
    }
    db_clear_property(db, idx, DB_UNRESETVIEWS);
}

/// Esta função é chamada pelo VDBE para ajustar o schema interno usado pelo SQLite
/// quando a camada btree move a página raiz de uma tabela. A página raiz de uma
/// tabela ou índice do banco de dados i_db mudou de i_from para i_to.
///
/// Ticket #1728: a tabela de símbolos ainda pode ter informação sobre tabelas e/ou
/// índices em processo de remoção. Com azar, um deles pode ter o mesmo número de
/// página raiz da tabela ou índice real que está sendo movido. Por isso não se pode
/// parar de procurar no primeiro casamento: ele pode ser de um índice ou tabela
/// removido. É preciso continuar até converter todas as tabelas e índices com
/// rootpage==i_from para i_to, para ter certeza de pegar o certo.
pub fn root_page_moved(db: &Sqlite3, i_db: i32, i_from: u32, i_to: u32) {
    debug_assert!(schema_mutex_held(db, i_db, None));
    let p_schema = db.a_db[i_db as usize].p_schema.clone().expect("schema do banco");
    {
        let p_hash = &p_schema.borrow().tbl_hash;
        let mut p_elem = sqlite_hash_first(p_hash);
        while let Some(elem) = p_elem {
            let p_tab = sqlite_hash_data(&elem);
            p_elem = sqlite_hash_next(p_hash, &elem);
            let mut t = p_tab.borrow_mut();
            if t.tnum == i_from {
                t.tnum = i_to;
            }
        }
    }
    let p_hash = &p_schema.borrow().idx_hash;
    let mut p_elem = sqlite_hash_first(p_hash);
    while let Some(elem) = p_elem {
        let p_idx = sqlite_hash_data(&elem);
        p_elem = sqlite_hash_next(p_hash, &elem);
        let mut idx = p_idx.borrow_mut();
        if idx.tnum == i_from {
            idx.tnum = i_to;
        }
    }
}

/// Escreve código para apagar a tabela com página raiz i_table do banco i_db.
/// Escreve também código para modificar a tabela sqlite_schema e o schema interno
/// se a página raiz de outra tabela for movida pela camada btree enquanto i_table é
/// apagada (o que pode acontecer num banco auto-vacuum).
fn destroy_root_page(p_parse: &mut Parse, i_table: u32, i_db: i32) {
    let v = get_vdbe(p_parse).expect("Vdbe do Parse");
    let r1 = get_temp_reg(p_parse);
    if i_table < 2 {
        error_msg(p_parse, b"corrupt schema");
    }
    vdbe_add_op3(&mut v.borrow_mut(), OP_DESTROY, i_table as i32, r1, i_db);
    may_abort(p_parse);
    // OP_Destroy guarda um inteiro em r1. Se ele é diferente de zero, é o número da
    // página raiz de uma tabela movida para a posição i_table. O código abaixo modifica
    // a tabela sqlite_schema para refletir isso.
    //
    // O "#NNN" no SQL é uma constante especial que significa o valor que estiver no
    // registrador NNN. Veja as regras da gramática ligadas ao token TK_REGISTER.
    let z_db_s_name = {
        let db = p_parse.db.upgrade().expect("conexão do Parse");
        let d = db.borrow();
        d.a_db[i_db as usize].z_db_s_name.clone()
    };
    nested_parse(
        p_parse,
        b"UPDATE %Q.sqlite_master SET rootpage=%d WHERE #%d AND rootpage=#%d",
        &[
            PrintfArg::Str(&z_db_s_name),
            PrintfArg::Int(i_table as i32),
            PrintfArg::Int(r1),
            PrintfArg::Int(r1),
        ],
    );
    release_temp_reg(p_parse, r1);
}

/// Escreve código VDBE para apagar a tabela p_tab e todos os índices associados em
/// disco. Acrescenta também o código que atualiza as tabelas sqlite_schema e as
/// definições internas de schema caso a camada btree mova a página raiz de outra
/// tabela (o que pode acontecer num banco auto-vacuum).
fn destroy_table(p_parse: &mut Parse, p_tab: &TableRef) {
    // Se o banco pode ser auto-vacuum (SQLITE_OMIT_AUTOVACUUM não está definida), é
    // importante chamar OP_Destroy nas páginas raiz da tabela e dos índices em ordem,
    // começando pelo maior número de página raiz. Isso garante que nenhuma página raiz
    // a destruir seja realocada por um OP_Destroy anterior. Ou seja, se fosse gerado:
    //
    // OP_Destroy 4 0
    // ...
    // OP_Destroy 5 0
    //
    // e a página 5 fosse a de maior número do banco, o "OP_Destroy 4 0" moveria a
    // página 5 para a 4, e o "OP_Destroy 5 0" seguinte cairia numa página da freelist.
    let i_tab: u32 = p_tab.borrow().tnum;
    let mut i_destroyed: u32 = 0;

    loop {
        let mut i_largest: u32 = 0;

        if i_destroyed == 0 || i_tab < i_destroyed {
            i_largest = i_tab;
        }
        let mut p_idx = p_tab.borrow().p_index.clone();
        while let Some(idx) = p_idx {
            let i_idx = idx.borrow().tnum;
            if (i_destroyed == 0 || i_idx < i_destroyed) && i_idx > i_largest {
                i_largest = i_idx;
            }
            p_idx = idx.borrow().p_next.clone();
        }
        if i_largest == 0 {
            return;
        } else {
            let i_db = {
                let db = p_parse.db.upgrade().expect("conexão do Parse");
                let d = db.borrow();
                schema_to_index(&d, &p_tab.borrow().p_schema)
            };
            debug_assert!(i_db >= 0);
            destroy_root_page(p_parse, i_largest, i_db);
            i_destroyed = i_largest;
        }
    }
}


// ---- part_008.rs ----

// Convenções de formatação usadas nesta parte e nas vizinhas (`part_009` a `part_011`):
// `error_msg(p_parse, fmt: &[u8], args: &[PrintfArg])`, `nested_parse(p_parse, fmt: &[u8], args:
// &[PrintfArg])` e `m_printf(db: &Sqlite3, fmt: &[u8], args: &[PrintfArg]) -> Option<Vec<u8>>`.
// `%s`, `%q` e `%Q` recebem `PrintfArg::Text(Option<Vec<u8>>)`, `%d` recebe `PrintfArg::Int(i64)`,
// `%T` recebe `PrintfArg::Token(Token)` e `%S` recebe `PrintfArg::SrcItem(SrcItem)`.
// `SrcItem.z_database` vazio equivale ao `NULL` do C.

/// Nome de banco de um `SrcItem` como argumento opcional: vazio equivale ao `NULL` do C.
fn db_name_arg(z: &[u8]) -> Option<&[u8]> {
    if z.is_empty() {
        None
    } else {
        Some(z)
    }
}

/// Remove entradas das tabelas `sqlite_statN` (para N em 1..4) depois de um `DROP INDEX` ou
/// `DROP TABLE`. `z_type` é `"idx"` ou `"tbl"`; `z_name` é o nome do índice ou da tabela.
pub fn clear_stat_tables(p_parse: &mut Parse, i_db: i32, z_type: &[u8], z_name: &[u8]) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };
    let z_db_name: Vec<u8> = db_rc.borrow().a_db[i_db as usize].z_db_s_name.clone();
    for i in 1..=4 {
        let z_tab: Vec<u8> = format!("sqlite_stat{}", i).into_bytes();
        if find_table(&db_rc.borrow(), &z_tab, Some(z_db_name.as_slice())).is_some() {
            nested_parse(
                p_parse,
                b"DELETE FROM %Q.%s WHERE %s=%Q",
                &[
                    PrintfArg::Text(Some(z_db_name.clone())),
                    PrintfArg::Text(Some(z_tab)),
                    PrintfArg::Text(Some(z_type.to_vec())),
                    PrintfArg::Text(Some(z_name.to_vec())),
                ],
            );
        }
    }
}

/// Gera o código para remover uma tabela.
pub fn code_drop_table(p_parse: &mut Parse, p_tab: &TableRef, i_db: i32, is_view: i32) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };
    let z_db_s_name: Vec<u8> = db_rc.borrow().a_db[i_db as usize].z_db_s_name.clone();

    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => {
            debug_assert!(false);
            return;
        }
    };
    begin_write_operation(p_parse, 1, i_db);

    if is_virtual(&p_tab.borrow()) {
        vdbe_add_op0(&mut v.borrow_mut(), OP_VBEGIN as i32);
    }

    // Remove todos os gatilhos associados à tabela removida. O código gerado apaga as entradas
    // de sqlite_schema e/ou sqlite_temp_schema quando preciso.
    let mut p_trigger = trigger_list(p_parse, p_tab);
    while let Some(trigger) = p_trigger {
        debug_assert!({
            let t_schema = trigger.borrow().p_schema.clone();
            let tab_schema = p_tab.borrow().p_schema.clone();
            let temp_schema = db_rc.borrow().a_db[1].p_schema.clone();
            schema_opt_eq(&t_schema, &tab_schema) || schema_opt_eq(&t_schema, &temp_schema)
        });
        drop_trigger_ptr(p_parse, &trigger);
        p_trigger = trigger.borrow().p_next.clone();
    }

    let z_tab_name: Vec<u8> = p_tab.borrow().z_name.clone();

    // Apaga as entradas da tabela sqlite_sequence associadas à tabela removida. Isto é feito
    // antes de a tabela ser removida no nível do btree, caso sqlite_sequence precise se mover
    // por causa da remoção (pode acontecer no modo auto-vacuum).
    if (p_tab.borrow().tab_flags & TF_AUTOINCREMENT) != 0 {
        nested_parse(
            p_parse,
            b"DELETE FROM %Q.sqlite_sequence WHERE name=%Q",
            &[
                PrintfArg::Text(Some(z_db_s_name.clone())),
                PrintfArg::Text(Some(z_tab_name.clone())),
            ],
        );
    }

    // Apaga todas as entradas da tabela de schema que se referem à tabela. O programa percorre
    // a tabela de schema e apaga cada linha que se refere a uma tabela com o mesmo nome da que
    // está sendo removida. Gatilhos são tratados à parte porque um gatilho pode ser criado no
    // banco temp referindo-se a uma tabela de outro banco.
    nested_parse(
        p_parse,
        b"DELETE FROM %Q.sqlite_master WHERE tbl_name=%Q and type!='trigger'",
        &[
            PrintfArg::Text(Some(z_db_s_name.clone())),
            PrintfArg::Text(Some(z_tab_name.clone())),
        ],
    );
    let tab_is_virtual = is_virtual(&p_tab.borrow());
    if is_view == 0 && !tab_is_virtual {
        destroy_table(p_parse, p_tab);
    }

    // Remove a entrada da tabela do schema interno do SQLite e modifica o cookie de schema.
    if tab_is_virtual {
        vdbe_add_op4(
            &mut v.borrow_mut(),
            OP_VDESTROY as i32,
            i_db,
            0,
            0,
            P4Value::Text(z_tab_name.clone()),
            P4_DYNAMIC,
        );
        may_abort(p_parse);
    }
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_DROPTABLE as i32,
        i_db,
        0,
        0,
        P4Value::Text(z_tab_name),
        P4_DYNAMIC,
    );
    change_cookie(p_parse, i_db);
    sqlite_view_reset_all(&mut db_rc.borrow_mut(), i_db);
}

/// Compara dois `Option<SchemaRef>` por identidade (o `==` de ponteiros do C).
fn schema_opt_eq(a: &Option<SchemaRef>, b: &Option<SchemaRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Devolve verdadeiro se as tabelas sombra devem ser somente leitura no contexto atual.
pub fn read_only_shadow_tables(db: &Sqlite3) -> i32 {
    if (db.flags & SQLITE_DEFENSIVE as u64) != 0
        && db.p_vtab_ctx.is_none()
        && db.n_vdbe_exec == 0
        && !vtab_in_sync(db)
    {
        return 1;
    }
    0
}

/// Devolve verdadeiro se não é permitido remover a tabela dada.
fn table_may_not_be_dropped(db: &Sqlite3, p_tab: &Table) -> i32 {
    if str_n_i_cmp(&p_tab.z_name, b"sqlite_", 7) == 0 {
        if str_n_i_cmp(&p_tab.z_name[7..], b"stat", 4) == 0 {
            return 0;
        }
        if str_n_i_cmp(&p_tab.z_name[7..], b"parameters", 10) == 0 {
            return 0;
        }
        return 1;
    }
    if (p_tab.tab_flags & TF_SHADOW) != 0 && read_only_shadow_tables(db) != 0 {
        return 1;
    }
    if (p_tab.tab_flags & TF_EPONYMOUS) != 0 {
        return 1;
    }
    0
}

/// Faz o trabalho de um comando `DROP TABLE`. `p_name` é o nome da tabela a remover.
pub fn drop_table(p_parse: &mut Parse, p_name: Box<SrcList>, is_view: i32, no_err: i32) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };

    'exit_drop_table: {
        if db_rc.borrow().malloc_failed != 0 {
            break 'exit_drop_table;
        }
        debug_assert!(p_parse.n_err == 0);
        debug_assert!(p_name.n_src == 1);
        if read_schema(p_parse) != 0 {
            break 'exit_drop_table;
        }
        if no_err != 0 {
            db_rc.borrow_mut().suppress_err += 1;
        }
        debug_assert!(is_view == 0 || is_view as u32 == LOCATE_VIEW);
        // `locate_table_item` precisa do item mutável porque marca o item como resolvido.
        let mut p_name = p_name;
        let p_tab = locate_table_item(p_parse, is_view as u32, &mut p_name.a[0]);
        if no_err != 0 {
            db_rc.borrow_mut().suppress_err -= 1;
        }

        let p_tab = match p_tab {
            Some(t) => t,
            None => {
                if no_err != 0 {
                    code_verify_named_schema(p_parse, db_name_arg(&p_name.a[0].z_database));
                    force_not_read_only(p_parse);
                }
                break 'exit_drop_table;
            }
        };
        let i_db = {
            let p_schema = p_tab.borrow().p_schema.clone();
            schema_to_index(&db_rc.borrow(), p_schema.as_ref())
        };
        debug_assert!(i_db >= 0 && i_db < db_rc.borrow().n_db);

        // Se pTab é uma tabela virtual, chama view_get_column_names() para garantir que ela
        // está inicializada.
        if is_virtual(&p_tab.borrow()) && view_get_column_names(p_parse, &p_tab) != 0 {
            break 'exit_drop_table;
        }
        {
            let z_tab: Vec<u8> = schema_table(i_db).to_vec();
            let z_db: Vec<u8> = db_rc.borrow().a_db[i_db as usize].z_db_s_name.clone();
            let z_tab_name: Vec<u8> = p_tab.borrow().z_name.clone();
            let mut z_arg2: Option<Vec<u8>> = None;
            if auth_check(p_parse, SQLITE_DELETE, Some(&z_tab), None, Some(&z_db)) != 0 {
                break 'exit_drop_table;
            }
            let code: i32;
            if is_view != 0 {
                if OMIT_TEMPDB == 0 && i_db == 1 {
                    code = SQLITE_DROP_TEMP_VIEW;
                } else {
                    code = SQLITE_DROP_VIEW;
                }
            } else if is_virtual(&p_tab.borrow()) {
                code = SQLITE_DROP_VTABLE;
                z_arg2 = get_v_table(&db_rc.borrow(), &p_tab)
                    .map(|vt| vt.borrow().p_mod.borrow().z_name.clone());
            } else if OMIT_TEMPDB == 0 && i_db == 1 {
                code = SQLITE_DROP_TEMP_TABLE;
            } else {
                code = SQLITE_DROP_TABLE;
            }
            if auth_check(p_parse, code, Some(&z_tab_name), z_arg2.as_deref(), Some(&z_db)) != 0 {
                break 'exit_drop_table;
            }
            if auth_check(p_parse, SQLITE_DELETE, Some(&z_tab_name), None, Some(&z_db)) != 0 {
                break 'exit_drop_table;
            }
        }
        if table_may_not_be_dropped(&db_rc.borrow(), &p_tab.borrow()) != 0 {
            let z_tab_name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"table %s may not be dropped",
                &[PrintfArg::Text(Some(z_tab_name))],
            );
            break 'exit_drop_table;
        }

        // Garante que DROP TABLE não é usado numa visão e DROP VIEW não é usado numa tabela.
        // IsView(pTab): o parâmetro `is_view` esconde a função homônima, então o teste é direto.
        let tab_is_view = p_tab.borrow().e_tab_type == TABTYP_VIEW;
        if is_view != 0 && !tab_is_view {
            let z_tab_name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"use DROP TABLE to delete table %s",
                &[PrintfArg::Text(Some(z_tab_name))],
            );
            break 'exit_drop_table;
        }
        if is_view == 0 && tab_is_view {
            let z_tab_name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"use DROP VIEW to delete view %s",
                &[PrintfArg::Text(Some(z_tab_name))],
            );
            break 'exit_drop_table;
        }

        // Gera o código para remover a tabela da tabela de schema em disco.
        let v = get_vdbe(p_parse);
        if v.is_some() {
            begin_write_operation(p_parse, 1, i_db);
            if is_view == 0 {
                let z_tab_name = p_tab.borrow().z_name.clone();
                clear_stat_tables(p_parse, i_db, b"tbl", &z_tab_name);
                fk_drop_table(p_parse, &p_name, &p_tab);
            }
            code_drop_table(p_parse, &p_tab, i_db, is_view);
        }
    }
    // exit_drop_table: o sqlite3SrcListDelete(db, pName) do C é o drop de `p_name` ao fim do
    // bloco acima, tanto no caminho normal quanto nos `break`.
}

/// Cria uma nova chave estrangeira na tabela em construção. `p_from_col` diz quais colunas da
/// tabela atual apontam para a chave estrangeira. Se `p_from_col` é `None`, liga a chave à
/// última coluna inserida. `p_to` é o nome da tabela referida (a tabela "pai"). `p_to_col` é a
/// lista de colunas da tabela pai. `flags` contém as informações sobre os algoritmos de
/// resolução de conflito dados nas cláusulas ON DELETE, ON UPDATE e ON INSERT.
///
/// Uma estrutura `FKey` é criada e adicionada à tabela em construção em `p_parse.p_new_table`.
/// A chave estrangeira nasce com processamento IMMEDIATE. Uma chamada posterior a
/// `defer_foreign_key()` pode mudá-la para DEFERRED.
pub fn create_foreign_key(
    p_parse: &mut Parse,
    p_from_col: Option<Box<ExprList>>,
    p_to: &Token,
    p_to_col: Option<Box<ExprList>>,
    flags: i32,
) {
    foreign_key_body(p_parse, p_from_col.as_deref(), p_to, p_to_col.as_deref(), flags);
    // fk_end: as duas listas são do chamador e morrem aqui (sqlite3ExprListDelete).
    drop(p_from_col);
    drop(p_to_col);
}

/// Corpo de `create_foreign_key()`; cada `return` é um `goto fk_end` do C (o `pFKey` local é
/// solto sozinho ao sair).
fn foreign_key_body(
    p_parse: &mut Parse,
    p_from_col: Option<&ExprList>,
    p_to: &Token,
    p_to_col: Option<&ExprList>,
    flags: i32,
) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };
    let p = match &p_parse.p_new_table {
        Some(t) => t.clone(),
        None => return,
    };
    if in_declare_vtab(p_parse) {
        return;
    }
    let n_col: i32;
    match p_from_col {
        None => {
            let i_col = p.borrow().n_col as i32 - 1;
            if i_col < 0 {
                return;
            }
            if let Some(to_col) = p_to_col {
                if to_col.n_expr != 1 {
                    let z_cn_name = p.borrow().a_col[i_col as usize].z_cn_name.clone();
                    error_msg(
                        p_parse,
                        b"foreign key on %s should reference only one column of table %T",
                        &[
                            PrintfArg::Text(Some(z_cn_name)),
                            PrintfArg::Token(p_to.clone()),
                        ],
                    );
                    return;
                }
            }
            n_col = 1;
        }
        Some(from_col) => {
            if let Some(to_col) = p_to_col {
                if to_col.n_expr != from_col.n_expr {
                    error_msg(
                        p_parse,
                        b"number of columns in foreign key does not match the number of columns in the referenced table",
                        &[],
                    );
                    return;
                }
            }
            n_col = from_col.n_expr;
        }
    }

    let mut p_fkey = FKey::default();
    p_fkey.p_from = Rc::downgrade(&p);
    debug_assert!(is_ordinary_table(&p.borrow()));
    p_fkey.p_next_from = match &p.borrow().u {
        TableU::Tab(tab) => tab.p_fkey.clone(),
        _ => None,
    };
    p_fkey.a_col = vec![SColMap::default(); n_col as usize];

    // zTo: cópia do texto do token, sem aspas.
    let mut z_to: Vec<u8> = p_to.z[..p_to.n as usize].to_vec();
    dequote(&mut z_to);
    if in_rename_object(p_parse) {
        rename_token_map(p_parse, z_to.as_ptr() as usize, p_to);
    }
    p_fkey.z_to = z_to;
    p_fkey.n_col = n_col;

    match p_from_col {
        None => {
            p_fkey.a_col[0].i_from = p.borrow().n_col as i32 - 1;
        }
        Some(from_col) => {
            for i in 0..n_col as usize {
                let mut j: usize = 0;
                {
                    let tab = p.borrow();
                    while j < tab.n_col as usize {
                        if str_i_cmp(&tab.a_col[j].z_cn_name, &from_col.a[i].z_e_name) == 0 {
                            p_fkey.a_col[i].i_from = j as i32;
                            break;
                        }
                        j += 1;
                    }
                    if j >= tab.n_col as usize {
                        drop(tab);
                        error_msg(
                            p_parse,
                            b"unknown column \"%s\" in foreign key definition",
                            &[PrintfArg::Text(Some(from_col.a[i].z_e_name.clone()))],
                        );
                        return;
                    }
                }
                if in_rename_object(p_parse) {
                    rename_token_remap(
                        p_parse,
                        &p_fkey.a_col[i] as *const SColMap as usize,
                        from_col.a[i].z_e_name.as_ptr() as usize,
                    );
                }
            }
        }
    }
    if let Some(to_col) = p_to_col {
        for i in 0..n_col as usize {
            let z: Vec<u8> = to_col.a[i].z_e_name.clone();
            if in_rename_object(p_parse) {
                rename_token_remap(
                    p_parse,
                    z.as_ptr() as usize,
                    to_col.a[i].z_e_name.as_ptr() as usize,
                );
            }
            p_fkey.a_col[i].z_col = Some(z);
        }
    }
    p_fkey.is_deferred = 0;
    p_fkey.a_action[0] = (flags & 0xff) as u8; // ação de ON DELETE
    p_fkey.a_action[1] = ((flags >> 8) & 0xff) as u8; // ação de ON UPDATE

    let z_to_key: Vec<u8> = p_fkey.z_to.clone();
    let fkey: FKeyRef = Rc::new(RefCell::new(p_fkey));
    let p_schema = match p.borrow().p_schema.clone() {
        Some(s) => s,
        None => return,
    };
    let p_next_to = hash_insert(&mut p_schema.borrow_mut().fkey_hash, &z_to_key, fkey.clone());
    if let Some(next) = &p_next_to {
        if Rc::ptr_eq(next, &fkey) {
            oom_fault(&mut db_rc.borrow_mut());
            return;
        }
    }
    if let Some(next) = p_next_to {
        debug_assert!(next.borrow().p_prev_to.is_none());
        fkey.borrow_mut().p_next_to = Some(next.clone());
        next.borrow_mut().p_prev_to = Some(Rc::downgrade(&fkey));
    }

    // Liga a chave estrangeira à tabela como último passo.
    debug_assert!(is_ordinary_table(&p.borrow()));
    if let TableU::Tab(tab) = &mut p.borrow_mut().u {
        tab.p_fkey = Some(fkey);
    }
}


// ---- part_009.rs ----

/// Chamada quando uma cláusula `INITIALLY IMMEDIATE` ou `INITIALLY DEFERRED` aparece como parte
/// da definição de uma chave estrangeira. O parâmetro `is_deferred` é 1 para `INITIALLY
/// DEFERRED` e 0 para `INITIALLY IMMEDIATE`. O comportamento da chave estrangeira criada mais
/// recentemente é ajustado de acordo.
pub fn defer_foreign_key(p_parse: &mut Parse, is_deferred: i32) {
    let p_tab = match &p_parse.p_new_table {
        Some(t) => t.clone(),
        None => return,
    };
    if !is_ordinary_table(&p_tab.borrow()) {
        return;
    }
    let p_fkey = match &p_tab.borrow().u {
        TableU::Tab(tab) => tab.p_fkey.clone(),
        _ => None,
    };
    let Some(fkey_ref) = p_fkey else {
        return;
    };
    debug_assert!(is_deferred == 0 || is_deferred == 1); // EV: R-30323-21917
    fkey_ref.borrow_mut().is_deferred = is_deferred as u8;
}

/// Gera código que apaga e recarrega o índice `p_index`. Serve para inicializar um índice
/// recém-criado ou para recalcular seu conteúdo em resposta a um comando `REINDEX`.
///
/// Se `mem_root_page` não for negativo, o índice foi recém-criado e o registrador dado por
/// `mem_root_page` contém o número da página raiz do índice. Se for negativo, o índice já
/// existe e precisa ser limpo antes de ser recarregado, e a página raiz vem de `p_index.tnum`.
pub fn refill_index(p_parse: &mut Parse, p_index: &IndexRef, mem_root_page: i32) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };
    let p_tab = match p_index.borrow().p_table.upgrade() {
        Some(t) => t,
        None => return,
    };
    let i_tab = p_parse.n_tab; // cursor do btree de pTab
    p_parse.n_tab += 1;
    let i_idx = p_parse.n_tab; // cursor do btree de pIndex
    p_parse.n_tab += 1;
    let i_db = {
        let p_schema = p_index.borrow().p_schema.as_ref().and_then(|w| w.upgrade());
        schema_to_index(&db_rc.borrow(), p_schema.as_ref())
    };

    {
        let z_name = p_index.borrow().z_name.clone();
        let z_db = db_rc.borrow().a_db[i_db as usize].z_db_s_name.clone();
        if auth_check(p_parse, SQLITE_REINDEX, Some(&z_name), None, Some(&z_db)) != 0 {
            return;
        }
    }

    // Exige um bloqueio de escrita na tabela para executar esta operação.
    {
        let (tab_tnum, z_tab_name) = {
            let tab = p_tab.borrow();
            (tab.tnum, tab.z_name.clone())
        };
        table_lock(p_parse, i_db, tab_tnum, 1, &z_tab_name);
    }

    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => return,
    };
    let tnum: Pgno = if mem_root_page >= 0 {
        mem_root_page as Pgno
    } else {
        p_index.borrow().tnum
    };
    // Só falta o KeyInfo em caso de OOM; o C segue adiante com ele nulo, aqui o erro já está em
    // `p_parse` e não há o que gerar.
    let p_key = match key_info_of_index(p_parse, p_index) {
        Some(k) => k,
        None => return,
    };

    // Abre o cursor do sorter, se for usar um.
    let i_sorter = p_parse.n_tab; // cursor aberto por OpenSorter
    p_parse.n_tab += 1;
    let n_key_col = p_index.borrow().n_key_col as i32;
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_SORTEROPEN as i32,
        i_sorter,
        0,
        n_key_col,
        P4Value::KeyInfo(key_info_ref(&p_key)),
        P4_KEYINFO,
    );

    // Abre a tabela. Percorre todas as linhas da tabela, inserindo registros de índice no sorter.
    open_table(p_parse, i_tab, i_db, &p_tab, OP_OPENREAD);
    let mut addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_tab, 0); // topo do laço
    let reg_record = get_temp_reg(p_parse);
    multi_write(p_parse);

    let mut i_part_idx_label: i32 = 0;
    generate_index_key(
        p_parse,
        p_index,
        i_tab,
        reg_record,
        0,
        &mut i_part_idx_label,
        None,
        0,
    );
    vdbe_add_op2(&mut v.borrow_mut(), OP_SORTERINSERT as i32, i_sorter, reg_record);
    resolve_part_idx_label(p_parse, i_part_idx_label);
    vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_tab, addr1 + 1);
    vdbe_jump_here(&mut v.borrow_mut(), addr1);
    if mem_root_page < 0 {
        vdbe_add_op2(&mut v.borrow_mut(), OP_CLEAR as i32, tnum as i32, i_db);
    }
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_OPENWRITE as i32,
        i_idx,
        tnum as i32,
        i_db,
        P4Value::KeyInfo(p_key),
        P4_KEYINFO,
    );
    vdbe_change_p5(
        &mut v.borrow_mut(),
        (OPFLAG_BULKCSR | (if mem_root_page >= 0 { OPFLAG_P2ISREG } else { 0 })) as u16,
    );

    addr1 = vdbe_add_op2(&mut v.borrow_mut(), OP_SORTERSORT as i32, i_sorter, 0);
    let addr2: i32; // endereço para onde saltar na próxima iteração
    if is_unique_index(&p_index.borrow()) {
        let j2 = vdbe_goto(&mut v.borrow_mut(), 1);
        addr2 = vdbe_current_addr(&v.borrow());
        vdbe_add_op4_int(
            &mut v.borrow_mut(),
            OP_SORTERCOMPARE as i32,
            i_sorter,
            j2,
            reg_record,
            n_key_col,
        );
        unique_constraint(p_parse, OE_ABORT, p_index);
        vdbe_jump_here(&mut v.borrow_mut(), j2);
    } else {
        // A maioria dos CREATE INDEX e REINDEX que não são UNIQUE não pode abortar. A exceção
        // é se uma das expressões indexadas contém uma função de usuário que lança exceção ao
        // ser avaliada. Mas o custo de acrescentar um diário de instrução a um CREATE INDEX é
        // muito pequeno (a maioria das páginas escritas não tem conteúdo que precise ser
        // restaurado se a instrução abortar), então may_abort() é chamada em todo CREATE INDEX.
        may_abort(p_parse);
        addr2 = vdbe_current_addr(&v.borrow());
    }
    vdbe_add_op3(
        &mut v.borrow_mut(),
        OP_SORTERDATA as i32,
        i_sorter,
        reg_record,
        i_idx,
    );
    if !p_index.borrow().b_asc_key_bug {
        // Este OP_SeekEnd torna a inserção de índice de um REINDEX muito mais rápida ao evitar
        // buscas desnecessárias. Mas a otimização não funciona para índices de restrição UNIQUE
        // em tabelas WITHOUT ROWID com chaves primárias DESC, pois esses índices têm as chaves
        // numa ordem diferente da tabela principal. Ver o ticket bba7b69f9849b5bf do SQLite.
        vdbe_add_op1(&mut v.borrow_mut(), OP_SEEKEND as i32, i_idx);
    }
    vdbe_add_op2(&mut v.borrow_mut(), OP_IDXINSERT as i32, i_idx, reg_record);
    vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_USESEEKRESULT as u16);
    release_temp_reg(p_parse, reg_record);
    vdbe_add_op2(&mut v.borrow_mut(), OP_SORTERNEXT as i32, i_sorter, addr2);
    vdbe_jump_here(&mut v.borrow_mut(), addr1);

    vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE as i32, i_tab);
    vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE as i32, i_idx);
    vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE as i32, i_sorter);
}

/// Aloca um objeto `Index` com `n_col` colunas. O C aloca num só bloco o objeto, os vetores
/// `azColl`, `aiRowLogEst`, `aiColumn`, `aSortOrder` e um espaço extra para os nomes; aqui cada
/// vetor é dono dos próprios dados e o espaço extra some (`nExtra`/`ppExtra`). Em OOM o
/// chamador vê `db.malloc_failed` ligado.
pub fn allocate_index_object(db: &Sqlite3, n_col: i16) -> Index {
    let _ = db;
    let n = n_col.max(0) as usize;
    let mut p = Index::default();
    p.az_coll = vec![Vec::new(); n];
    p.ai_row_log_est = vec![0; n + 1];
    p.ai_column = vec![0; n];
    p.a_sort_order = vec![0; n];
    p.n_column = n_col as u16;
    p.n_key_col = (n_col - 1) as u16;
    p
}

/// Se a lista `p_list` contém uma expressão analisada com uma cláusula explícita `NULLS FIRST`
/// ou `NULLS LAST`, deixa um erro em `p_parse` e devolve um valor não nulo. Caso contrário,
/// devolve zero.
pub fn has_explicit_nulls(p_parse: &mut Parse, p_list: Option<&ExprList>) -> i32 {
    if let Some(list) = p_list {
        for item in list.a.iter().take(list.n_expr as usize) {
            if item.fg.b_nulls != 0 {
                let sf = item.fg.sort_flags;
                let z_which: &[u8] = if sf == 0 || sf == 3 { b"FIRST" } else { b"LAST" };
                error_msg(
                    p_parse,
                    b"unsupported use of NULLS %s",
                    &[PrintfArg::Text(Some(z_which.to_vec()))],
                );
                return 1;
            }
        }
    }
    0
}

/// Compara dois `Option<SchemaRef>` por identidade (o `==` de ponteiros do C).
fn same_schema(a: &Option<SchemaRef>, b: &Option<SchemaRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Cria um novo índice para uma tabela SQL. `p_name1.p_name2` é o nome do índice e
/// `p_tbl_name` é o nome da tabela a indexar. Ambos são `None` para uma chave primária ou um
/// índice criado para satisfazer uma restrição UNIQUE. Se `p_tbl_name` é `None`, usa
/// `p_parse.p_new_table` como a tabela a indexar: a tabela em construção por um CREATE TABLE.
///
/// `p_list` é a lista de colunas a indexar. Será `None` se for uma chave primária ou restrição
/// única da coluna mais recente adicionada à tabela em construção.
///
/// A função do C é uma só; aqui a primeira metade (até achar a tabela e fixar o nome do índice)
/// é `create_index_front()` e o resto está em `create_index_rest()` na `part_010`, ligadas pelo
/// estado `CreateIndexCtx`. Todo `goto exit_create_index` vira `create_index_exit()`.
pub fn create_index(
    p_parse: &mut Parse,
    p_name1: Option<&Token>,
    p_name2: Option<&Token>,
    p_tbl_name: Option<Box<SrcList>>,
    p_list: Option<Box<ExprList>>,
    on_error: i32,
    p_start: Option<Token>,
    p_pi_where: Option<Box<Expr>>,
    sort_order: i32,
    if_not_exist: i32,
    idx_type: u8,
) {
    let mut ctx = CreateIndexCtx {
        p_tbl_name,
        p_list,
        on_error,
        p_start,
        p_pi_where,
        sort_order,
        if_not_exist: if_not_exist != 0,
        idx_type,
        ..Default::default()
    };
    if create_index_front(p_parse, p_name1, p_name2, &mut ctx) {
        create_index_rest(p_parse, &mut ctx);
    } else {
        create_index_exit(p_parse, &mut ctx);
    }
}

/// Primeira metade de `sqlite3CreateIndex()`. Devolve `false` quando o C faria `goto
/// exit_create_index` (o chamador então chama `create_index_exit()`, que é idempotente).
fn create_index_front(
    p_parse: &mut Parse,
    p_name1: Option<&Token>,
    p_name2: Option<&Token>,
    ctx: &mut CreateIndexCtx,
) -> bool {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return false,
    };
    if p_parse.n_err != 0 {
        return false;
    }
    debug_assert!(db_rc.borrow().malloc_failed == 0);
    if in_declare_vtab(p_parse) && ctx.idx_type != SQLITE_IDXTYPE_PRIMARYKEY {
        return false;
    }
    if read_schema(p_parse) != SQLITE_OK {
        return false;
    }
    if has_explicit_nulls(p_parse, ctx.p_list.as_deref()) != 0 {
        return false;
    }

    // Acha a tabela a indexar. Sai cedo se não achar.
    if ctx.p_tbl_name.is_some() {
        // Usa o nome do índice em duas partes para determinar o banco em que procurar a
        // tabela. "Fixa" o nome da tabela a esse banco antes de procurá-la.
        debug_assert!(p_name1.is_some() && p_name2.is_some());
        let (i_db0, p_name) = two_part_name(p_parse, p_name1.unwrap(), p_name2.unwrap());
        if i_db0 < 0 {
            return false;
        }
        ctx.i_db = i_db0;
        ctx.p_name = p_name;
        let name = match ctx.p_name.clone() {
            Some(n) => n,
            None => return false,
        };
        debug_assert!(!name.z.is_empty());

        // Se o nome do índice não foi qualificado, verifica se a tabela é temporária. Se for,
        // usa o banco 1. Não faz isso ao inicializar o schema de um banco.
        if OMIT_TEMPDB == 0 && db_rc.borrow().init.busy == 0 {
            let p_tab = src_list_lookup(p_parse, ctx.p_tbl_name.as_deref().unwrap());
            if p_name2.unwrap().n == 0 {
                if let Some(tab) = &p_tab {
                    let tab_schema = tab.borrow().p_schema.clone();
                    let temp_schema = db_rc.borrow().a_db[1].p_schema.clone();
                    if same_schema(&tab_schema, &temp_schema) {
                        ctx.i_db = 1;
                    }
                }
            }
        }

        let mut s_fix = DbFixer::default();
        fix_init(&mut s_fix, p_parse, ctx.i_db, b"index", &name);
        if fix_src_list(&mut s_fix, ctx.p_tbl_name.as_deref_mut().unwrap()) != 0 {
            // Como o parser monta pTblName de um único identificador, fix_src_list nunca falha.
            debug_assert!(false);
        }
        let p_tab = locate_table_item(p_parse, 0, &mut ctx.p_tbl_name.as_mut().unwrap().a[0]);
        debug_assert!(db_rc.borrow().malloc_failed == 0 || p_tab.is_none());
        ctx.p_tab = p_tab;
        let p_tab = match &ctx.p_tab {
            Some(t) => t.clone(),
            None => return false,
        };
        let tab_schema = p_tab.borrow().p_schema.clone();
        let db_schema = db_rc.borrow().a_db[ctx.i_db as usize].p_schema.clone();
        if ctx.i_db == 1 && !same_schema(&db_schema, &tab_schema) {
            let z_tab_name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"cannot create a TEMP index on non-TEMP table \"%s\"",
                &[PrintfArg::Text(Some(z_tab_name))],
            );
            return false;
        }
        if !has_rowid(&p_tab.borrow()) {
            ctx.p_pk = primary_key_index(&p_tab.borrow());
        }
    } else {
        debug_assert!(ctx.p_name.is_none());
        debug_assert!(ctx.p_start.is_none());
        let p_tab = match &p_parse.p_new_table {
            Some(t) => t.clone(),
            None => return false,
        };
        let tab_schema = p_tab.borrow().p_schema.clone();
        ctx.i_db = schema_to_index(&db_rc.borrow(), tab_schema.as_ref());
        ctx.p_tab = Some(p_tab);
    }
    let p_tab = ctx.p_tab.clone().unwrap();
    let z_db_s_name: Vec<u8> = db_rc.borrow().a_db[ctx.i_db as usize].z_db_s_name.clone();

    let z_tab_name = p_tab.borrow().z_name.clone();
    if str_n_i_cmp(&z_tab_name, b"sqlite_", 7) == 0
        && db_rc.borrow().init.busy == 0
        && ctx.p_tbl_name.is_some()
    {
        error_msg(
            p_parse,
            b"table %s may not be indexed",
            &[PrintfArg::Text(Some(z_tab_name))],
        );
        return false;
    }
    if is_view(&p_tab.borrow()) {
        error_msg(p_parse, b"views may not be indexed", &[]);
        return false;
    }
    if is_virtual(&p_tab.borrow()) {
        error_msg(p_parse, b"virtual tables may not be indexed", &[]);
        return false;
    }

    // Acha o nome do índice. Garante que não existe outro índice ou tabela com o mesmo nome.
    //
    // Exceção: se estamos lendo os nomes de índices permanentes da tabela sqlite_schema (porque
    // outro processo mudou o schema) e um dos nomes colide com o de uma tabela ou índice
    // temporário, continuamos a processar este índice.
    //
    // Se pName==0 estamos tratando uma chave primária ou restrição UNIQUE e é preciso inventar
    // um nome.
    if let Some(p_name) = ctx.p_name.clone() {
        let z_name = match name_from_token(&db_rc.borrow(), &p_name) {
            Some(z) => z,
            None => return false,
        };
        ctx.z_name = Some(z_name.clone());
        debug_assert!(!p_name.z.is_empty());
        let z_tab_name = p_tab.borrow().z_name.clone();
        if check_object_name(p_parse, &z_name, b"index", &z_tab_name) != SQLITE_OK {
            return false;
        }
        if !in_rename_object(p_parse) {
            if db_rc.borrow().init.busy == 0
                && find_table(&db_rc.borrow(), &z_name, Some(z_db_s_name.as_slice())).is_some()
            {
                error_msg(
                    p_parse,
                    b"there is already a table named %s",
                    &[PrintfArg::Text(Some(z_name))],
                );
                return false;
            }
            if find_index(&db_rc.borrow(), &z_name, Some(z_db_s_name.as_slice())).is_some() {
                if !ctx.if_not_exist {
                    error_msg(
                        p_parse,
                        b"index %s already exists",
                        &[PrintfArg::Text(Some(z_name))],
                    );
                    return false;
                }
                // O ramo `ifNotExist` termina na `part_010`, já com a saída feita.
                create_index_exists_quiet(p_parse, ctx);
                return false;
            }
        }
        true
    } else {
        create_index_autoname(p_parse, ctx)
    }
}


// ---- part_010.rs ----

// Este trecho é a segunda metade de `sqlite3CreateIndex()` (a primeira metade está em
// `part_009.rs`). No C a função é uma só, com variáveis locais compartilhadas e `goto
// exit_create_index`. Como o fatiamento a cortou no meio de um `if`, as variáveis locais que
// continuam vivas depois do corte viram os campos de `CreateIndexCtx`, e cada `goto
// exit_create_index` vira uma chamada a `create_index_exit()` (ou um `return` dentro de
// `create_index_body()`, que `create_index_rest()` sempre faz seguir de `create_index_exit()`).
//
// Memória do `Index`: no C o objeto, os vetores `aiColumn`/`azColl`/`aSortOrder` e o texto dos
// nomes de collation vivem numa só alocação (`zExtra`). Aqui cada vetor é dono dos próprios
// dados, então a contabilidade de `nExtra`/`zExtra`/`nName` some; só o efeito observável fica.

/// Estado de `sqlite3CreateIndex()` que atravessa o corte entre `part_009` e `part_010`: os
/// parâmetros ainda usados depois do corte e as variáveis locais vivas nele.
#[derive(Default)]
pub struct CreateIndexCtx {
    /// Parâmetro `pTblName`: tabela a indexar; `None` quando a tabela é `pParse->pNewTable`.
    pub p_tbl_name: Option<Box<SrcList>>,
    /// Parâmetro `pList`: lista de colunas do índice. Vira `None` quando é movida para
    /// `Index.aColExpr`.
    pub p_list: Option<Box<ExprList>>,
    /// Parâmetro `onError`: OE_ABORT, OE_IGNORE, OE_REPLACE ou OE_NONE.
    pub on_error: i32,
    /// Parâmetro `pStart`: o token CREATE que começa a instrução.
    pub p_start: Option<Token>,
    /// Parâmetro `pPIWhere`: cláusula WHERE de índices parciais. Vira `None` quando é movida
    /// para `Index.pPartIdxWhere`.
    pub p_pi_where: Option<Box<Expr>>,
    /// Parâmetro `sortOrder`: ordem da chave primária quando `pList==NULL`.
    pub sort_order: i32,
    /// Parâmetro `ifNotExist`: omite o erro se o índice já existe.
    pub if_not_exist: bool,
    /// Parâmetro `idxType`: o tipo do índice.
    pub idx_type: u8,
    /// Local `pTab`: tabela a ser indexada.
    pub p_tab: Option<TableRef>,
    /// Local `pIndex`: o índice a ser criado. Vira `None` quando é entregue à tabela ou ao
    /// `Parse`; o que sobra no fim é liberado por `create_index_exit()`.
    pub p_index: Option<IndexRef>,
    /// Local `zName`: nome do índice.
    pub z_name: Option<Vec<u8>>,
    /// Local `iDb`: índice do banco em que o índice está sendo criado.
    pub i_db: i32,
    /// Local `pName`: nome não qualificado do índice a criar.
    pub p_name: Option<Token>,
    /// Local `pPk`: índice da PRIMARY KEY de tabelas WITHOUT ROWID.
    pub p_pk: Option<IndexRef>,
}

/// Fim do ramo "o índice já existe" de `sqlite3CreateIndex()` quando `ifNotExist` está ligado:
/// em vez de erro, só verifica o schema e marca a instrução como não somente leitura. Termina
/// no `goto exit_create_index` que, no C, vem logo depois do `if`/`else` (por isso chama
/// `create_index_exit()`); o ramo `!ifNotExist` da `part_009` também precisa terminar com
/// `create_index_exit()`.
pub fn create_index_exists_quiet(p_parse: &mut Parse, ctx: &mut CreateIndexCtx) {
    if let Some(db_rc) = p_parse.db.upgrade() {
        debug_assert!(db_rc.borrow().init.busy == 0);
    }
    code_verify_schema(p_parse, ctx.i_db);
    force_not_read_only(p_parse);
    create_index_exit(p_parse, ctx);
}

/// Ramo `else` de `if( pName )`: o índice é automático (PRIMARY KEY ou UNIQUE) e o nome precisa
/// ser inventado. Devolve `false` se o código C faria `goto exit_create_index`; nesse caso a
/// limpeza já foi feita.
pub fn create_index_autoname(p_parse: &mut Parse, ctx: &mut CreateIndexCtx) -> bool {
    let p_tab = match ctx.p_tab.clone() {
        Some(t) => t,
        None => {
            create_index_exit(p_parse, ctx);
            return false;
        }
    };
    let mut n: i64 = 1;
    let mut p_loop = p_tab.borrow().p_index.clone();
    while let Some(l) = p_loop {
        p_loop = l.borrow().p_next.clone();
        n += 1;
    }
    let tab_name = p_tab.borrow().z_name.clone();
    let z_name = match p_parse.db.upgrade() {
        Some(db_rc) => m_printf(
            &db_rc.borrow(),
            b"sqlite_autoindex_%s_%d",
            &[PrintfArg::Text(Some(tab_name)), PrintfArg::Int(n)],
        ),
        None => None,
    };
    let mut z_name = match z_name {
        Some(z) => z,
        None => {
            create_index_exit(p_parse, ctx);
            return false;
        }
    };

    // Nomes de índice automáticos gerados dentro de sqlite3_declare_vtab() precisam ser
    // distintos dos nomes automáticos normais. O comando abaixo converte "sqlite3_autoindex..."
    // em "sqlite3_butoindex..." para torná-los distintos. O teste "vtab_err.test" mostra por
    // que esta linha é necessária.
    if in_special_parse(p_parse) {
        z_name[7] = z_name[7].wrapping_add(1);
    }
    ctx.z_name = Some(z_name);
    true
}

/// Todo o resto de `sqlite3CreateIndex()`, da verificação de autorização até o rótulo
/// `exit_create_index`.
pub fn create_index_rest(p_parse: &mut Parse, ctx: &mut CreateIndexCtx) {
    create_index_body(p_parse, ctx);
    create_index_exit(p_parse, ctx);
}

/// Corpo de `create_index_rest()`; cada `return` aqui é um `goto exit_create_index` do C.
fn create_index_body(p_parse: &mut Parse, ctx: &mut CreateIndexCtx) {
    let db_rc = match p_parse.db.upgrade() {
        Some(d) => d,
        None => return,
    };
    let p_tab = match ctx.p_tab.clone() {
        Some(t) => t,
        None => return,
    };
    let i_db = ctx.i_db as usize;
    let z_name = match ctx.z_name.clone() {
        Some(z) => z,
        None => return,
    };

    // Verifica a autorização para criar um índice.
    if !in_rename_object(p_parse) {
        let z_db = db_rc.borrow().a_db[i_db].z_db_s_name.clone();
        if auth_check(p_parse, SQLITE_INSERT, Some(schema_table(ctx.i_db)), None, Some(&z_db)) != 0 {
            return;
        }
        let mut code = SQLITE_CREATE_INDEX;
        if OMIT_TEMPDB == 0 && ctx.i_db == 1 {
            code = SQLITE_CREATE_TEMP_INDEX;
        }
        let tab_name = p_tab.borrow().z_name.clone();
        if auth_check(p_parse, code, Some(&z_name), Some(&tab_name), Some(&z_db)) != 0 {
            return;
        }
    }

    // Se pList==0, esta rotina foi chamada para fazer uma chave primária com a última coluna
    // adicionada à tabela em construção. Então cria uma lista falsa para simular isso.
    if ctx.p_list.is_none() {
        let mut prev_col = Token::default();
        {
            let mut tab = p_tab.borrow_mut();
            let last = (tab.n_col - 1) as usize;
            let p_col = &mut tab.a_col[last];
            p_col.col_flags |= COLFLAG_UNIQUE;
            token_init(&mut prev_col, &p_col.z_cn_name);
        }
        let new_expr = expr_alloc(&db_rc.borrow(), TK_ID as i32, Some(&prev_col), 0);
        ctx.p_list = expr_list_append(p_parse, None, new_expr);
        if ctx.p_list.is_none() {
            return;
        }
        debug_assert!(ctx.p_list.as_ref().map_or(false, |l| l.n_expr == 1));
        expr_list_set_sort_order(ctx.p_list.as_deref_mut(), ctx.sort_order, SQLITE_SO_UNDEFINED);
    } else {
        expr_list_check_length(p_parse, ctx.p_list.as_deref(), b"index");
        if p_parse.n_err != 0 {
            return;
        }
    }

    // Aloca a estrutura do índice. Os nomes explícitos de collation (que o C soma em `nExtra`
    // para dimensionar a alocação única) ficam em `az_coll`, cada um dono da própria cópia.
    let n_expr = ctx.p_list.as_ref().map_or(0, |l| l.n_expr);
    let n_extra_col: i32 = match &ctx.p_pk {
        Some(pk) => pk.borrow().n_key_col as i32,
        None => 1,
    };
    debug_assert!(n_expr + n_extra_col <= 32767); // Cabe em i16
    let new_index = allocate_index_object(&db_rc.borrow(), (n_expr + n_extra_col) as i16);
    if db_rc.borrow().malloc_failed != 0 {
        return;
    }
    let p_index: IndexRef = Rc::new(RefCell::new(new_index));
    ctx.p_index = Some(p_index.clone());
    {
        let schema = db_rc.borrow().a_db[i_db].p_schema.as_ref().map(Rc::downgrade);
        let mut idx = p_index.borrow_mut();
        idx.z_name = z_name.clone();
        idx.p_table = Rc::downgrade(&p_tab);
        idx.on_error = ctx.on_error as u8;
        idx.uniq_not_null = ctx.on_error != OE_NONE as i32;
        idx.idx_type = ctx.idx_type;
        idx.p_schema = schema;
        idx.n_key_col = n_expr as u16;
    }
    if let Some(mut p_pi_where) = ctx.p_pi_where.take() {
        resolve_self_reference(p_parse, Some(&p_tab), NC_PARTIDX, Some(&mut *p_pi_where), None);
        p_index.borrow_mut().p_part_idx_where = Some(p_pi_where);
    }

    // Verifica se devemos honrar pedidos DESC nas colunas do índice.
    let file_format = match &db_rc.borrow().a_db[i_db].p_schema {
        Some(s) => s.borrow().file_format,
        None => 0,
    };
    let sort_order_mask: i32 = if file_format >= 4 {
        -1 // Honra DESC
    } else {
        0 // Ignora DESC
    };

    // Analisa a lista de expressões que formam os termos do índice e reporta erros. No caso
    // comum em que a expressão é exatamente uma coluna da tabela, guarda a coluna em
    // aiColumn[]. Para expressões gerais, preenche pIndex->aColExpr e guarda XN_EXPR (-2) em
    // aiColumn[].
    //
    // TODO: emitir um aviso se duas ou mais colunas do índice forem idênticas.
    // TODO: emitir um aviso se a chave primária da tabela for usada como parte da chave do
    // índice.
    //
    // A lista sai de `ctx.p_list` durante o laço (o C guarda um ponteiro `pListItem` para o
    // arranjo, que continua válido depois que a lista passa para `aColExpr`); `moved` diz se
    // ela já pertence a `pIndex->aColExpr`.
    let mut list_box = match ctx.p_list.take() {
        Some(l) => l,
        None => return,
    };
    let mut moved = false;
    if in_rename_object(p_parse) {
        moved = true;
    }
    let n_key_col = n_expr as usize;
    let is_new_table = p_parse
        .p_new_table
        .as_ref()
        .map_or(false, |t| Rc::ptr_eq(t, &p_tab));
    let mut i: usize = 0;
    let loop_ok = 'items: {
        while i < n_key_col {
            // Expressão do i-ésimo termo do índice.
            string_to_id(list_box.a[i].p_expr.as_deref_mut().unwrap());
            resolve_self_reference(
                p_parse,
                Some(&p_tab),
                NC_IDXEXPR,
                list_box.a[i].p_expr.as_deref_mut(),
                None,
            );
            if p_parse.n_err != 0 {
                break 'items false;
            }
            let (c_op, c_column) = {
                let p_c_expr = expr_skip_collate(list_box.a[i].p_expr.as_deref().unwrap());
                (p_c_expr.op, p_c_expr.i_column)
            };
            let j: i32;
            if c_op != TK_COLUMN {
                if is_new_table {
                    error_msg(
                        p_parse,
                        b"expressions prohibited in PRIMARY KEY and UNIQUE constraints",
                        &[],
                    );
                    break 'items false;
                }
                if !moved {
                    // pIndex->aColExpr == 0
                    moved = true;
                    // A lista inteira passa a pertencer ao índice; o laço continua sobre ela.
                }
                j = XN_EXPR as i32;
                let mut idx = p_index.borrow_mut();
                idx.ai_column[i] = XN_EXPR;
                idx.uniq_not_null = false;
                idx.b_has_expr = true;
            } else {
                let mut jj = c_column;
                debug_assert!(jj <= 0x7fff);
                if jj < 0 {
                    jj = p_tab.borrow().i_p_key as i32;
                } else {
                    let (not_null, col_flags) = {
                        let tab = p_tab.borrow();
                        (tab.a_col[jj as usize].not_null, tab.a_col[jj as usize].col_flags)
                    };
                    let mut idx = p_index.borrow_mut();
                    if not_null == 0 {
                        idx.uniq_not_null = false;
                    }
                    if (col_flags & COLFLAG_VIRTUAL) != 0 {
                        idx.b_has_vcol = true;
                        idx.b_has_expr = true;
                    }
                }
                p_index.borrow_mut().ai_column[i] = jj as i16;
                j = jj;
            }
            let mut z_coll: Option<Vec<u8>> = None;
            if list_box.a[i].p_expr.as_ref().unwrap().op == TK_COLLATE {
                debug_assert!(!expr_has_property(list_box.a[i].p_expr.as_ref().unwrap(), EP_INT_VALUE));
                z_coll = list_box.a[i].p_expr.as_ref().unwrap().u.z_token.clone();
            } else if j >= 0 {
                let tab = p_tab.borrow();
                z_coll = column_coll(&tab.a_col[j as usize]).map(|z| z.to_vec());
            }
            let z_coll = match z_coll {
                Some(z) => z,
                None => STR_BINARY.to_vec(),
            };
            if db_rc.borrow().init.busy == 0 && locate_coll_seq(p_parse, &z_coll).is_none() {
                break 'items false;
            }
            let requested_sort_order = (list_box.a[i].fg.sort_flags as i32) & sort_order_mask;
            {
                let mut idx = p_index.borrow_mut();
                idx.az_coll[i] = z_coll;
                idx.a_sort_order[i] = requested_sort_order as u8;
            }
            i += 1;
        }
        true
    };
    if moved {
        p_index.borrow_mut().a_col_expr = Some(list_box);
    } else {
        ctx.p_list = Some(list_box);
    }
    if !loop_ok {
        return;
    }

    // Acrescenta a chave da tabela ao fim do índice. Para tabelas WITHOUT ROWID (quando
    // pPk!=0) é a PRIMARY KEY declarada. Para tabelas normais (quando pPk==0) é o rowid.
    if let Some(p_pk) = ctx.p_pk.clone() {
        let pk_key_col = p_pk.borrow().n_key_col as usize;
        for j in 0..pk_key_col {
            let x = p_pk.borrow().ai_column[j];
            debug_assert!(x >= 0);
            let dup = is_dup_column(&p_index.borrow(), n_key_col as i32, &p_pk.borrow(), j as i32);
            let mut idx = p_index.borrow_mut();
            if dup {
                idx.n_column -= 1;
            } else {
                let pk = p_pk.borrow();
                idx.ai_column[i] = x;
                idx.az_coll[i] = pk.az_coll[j].clone();
                idx.a_sort_order[i] = pk.a_sort_order[j];
                i += 1;
            }
        }
        debug_assert!(i == p_index.borrow().n_column as usize);
    } else {
        let mut idx = p_index.borrow_mut();
        idx.ai_column[i] = XN_ROWID;
        idx.az_coll[i] = STR_BINARY.to_vec();
    }
    default_row_est(&mut p_index.borrow_mut());
    if p_parse.p_new_table.is_none() {
        estimate_index_width(&p_index);
    }

    // Se este índice contém todas as colunas da tabela, marca-o como índice cobridor.
    debug_assert!({
        let tab = p_tab.borrow();
        has_rowid(&tab)
            || tab.i_p_key < 0
            || table_column_to_index(&p_index.borrow(), tab.i_p_key) >= 0
    });
    recompute_columns_not_indexed(&p_index);
    let (tab_n_col, tab_i_p_key) = {
        let tab = p_tab.borrow();
        (tab.n_col as i32, tab.i_p_key as i32)
    };
    if ctx.p_tbl_name.is_some() && p_index.borrow().n_column as i32 >= tab_n_col {
        let mut idx = p_index.borrow_mut();
        idx.is_covering = true;
        for j in 0..tab_n_col {
            if j == tab_i_p_key {
                continue;
            }
            if table_column_to_index(&idx, j as i16) >= 0 {
                continue;
            }
            idx.is_covering = false;
            break;
        }
    }

    if is_new_table {
        // Esta rotina foi chamada para criar um índice automático em consequência de uma
        // cláusula PRIMARY KEY ou UNIQUE numa definição de coluna, ou de uma cláusula PRIMARY
        // KEY ou UNIQUE depois das definições de coluna, isto é, uma destas:
        //
        // CREATE TABLE t(x PRIMARY KEY, y);
        // CREATE TABLE t(x, y, UNIQUE(x, y));
        //
        // De qualquer forma, verifica se a tabela já tem um índice assim. Se tiver, não se
        // dá ao trabalho de criar este. Isto só vale para índices criados automaticamente:
        // os usuários podem fazer o que quiserem com índices explícitos.
        //
        // Duas restrições UNIQUE ou PRIMARY KEY são consideradas equivalentes (e a segunda é
        // então suprimida) mesmo que tenham ordens de classificação diferentes.
        //
        // Se há sequências de collation diferentes, ou se as colunas da restrição aparecem em
        // ordens diferentes, as restrições são consideradas distintas e ambas resultam em
        // índices separados.
        let mut p_idx_opt = p_tab.borrow().p_index.clone();
        while let Some(p_idx) = p_idx_opt {
            p_idx_opt = p_idx.borrow().p_next.clone();
            debug_assert!(is_unique_index(&p_idx.borrow()));
            debug_assert!(p_idx.borrow().idx_type != SQLITE_IDXTYPE_APPDEF);
            debug_assert!(is_unique_index(&p_index.borrow()));

            let same = {
                let a = p_idx.borrow();
                let b = p_index.borrow();
                if a.n_key_col != b.n_key_col {
                    continue;
                }
                let mut k = 0usize;
                while k < a.n_key_col as usize {
                    debug_assert!(a.ai_column[k] >= 0);
                    if a.ai_column[k] != b.ai_column[k] {
                        break;
                    }
                    let z1 = &a.az_coll[k];
                    let z2 = &b.az_coll[k];
                    if str_i_cmp(z1, z2) != 0 {
                        break;
                    }
                    k += 1;
                }
                k == a.n_key_col as usize
            };
            if same {
                let (a_err, b_err) = (p_idx.borrow().on_error, p_index.borrow().on_error);
                if a_err != b_err {
                    // Esta restrição cria o mesmo índice que uma restrição anterior dada em
                    // algum ponto do CREATE TABLE. Porém as cláusulas ON CONFLICT são
                    // diferentes. Se esta restrição e a equivalente anterior têm cláusulas ON
                    // CONFLICT explícitas, é um erro. Caso contrário, usa o comportamento
                    // especificado explicitamente para o índice.
                    if !(a_err == OE_DEFAULT || b_err == OE_DEFAULT) {
                        error_msg(p_parse, b"conflicting ON CONFLICT clauses specified", &[]);
                    }
                    if a_err == OE_DEFAULT {
                        p_idx.borrow_mut().on_error = b_err;
                    }
                }
                if ctx.idx_type == SQLITE_IDXTYPE_PRIMARYKEY {
                    p_idx.borrow_mut().idx_type = ctx.idx_type;
                }
                if in_rename_object(p_parse) {
                    p_index.borrow_mut().p_next = p_parse.p_new_index.take();
                    p_parse.p_new_index = Some(p_index.clone());
                    ctx.p_index = None;
                }
                return;
            }
        }
    }

    if !in_rename_object(p_parse) {
        // Liga a nova estrutura Index à sua tabela e às outras estruturas de banco de dados
        // em memória.
        debug_assert!(p_parse.n_err == 0);
        if db_rc.borrow().init.busy != 0 {
            debug_assert!(!in_special_parse(p_parse));
            if ctx.p_tbl_name.is_some() {
                let new_tnum = db_rc.borrow().init.new_tnum;
                p_index.borrow_mut().tnum = new_tnum;
                if index_has_duplicate_root_page(&p_index.borrow()) {
                    error_msg(p_parse, b"invalid rootpage", &[]);
                    p_parse.rc = corrupt_bkpt();
                    return;
                }
            }
            let p_schema = db_rc.borrow().a_db[i_db].p_schema.clone();
            let p = match p_schema {
                Some(s) => hash_insert(&mut s.borrow_mut().idx_hash, &z_name, p_index.clone()),
                None => None,
            };
            if p.is_some() {
                // O malloc deve ter falhado.
                oom_fault(&mut db_rc.borrow_mut());
                return;
            }
            db_rc.borrow_mut().m_db_flags |= DBFLAG_SCHEMA_CHANGE;
        }
        // Se este é o CREATE INDEX inicial (ou o CREATE TABLE, se o índice é um índice
        // implícito de uma restrição UNIQUE ou PRIMARY KEY) então emite o código para alocar
        // a página raiz do índice em disco, fazer uma entrada para o índice na tabela
        // sqlite_schema e povoar o índice com conteúdo. Mas não faz isso se estamos apenas
        // lendo a tabela sqlite_schema para analisar o schema, ou se este índice é o índice
        // PRIMARY KEY de uma tabela WITHOUT ROWID.
        //
        // Se pTblName==0 significa que este índice é gerado como um índice implícito de
        // PRIMARY KEY ou UNIQUE num CREATE TABLE. Como a tabela acabou de ser criada, ela não
        // tem dados e a etapa de inicialização do índice pode ser pulada.
        else if has_rowid(&p_tab.borrow()) || ctx.p_tbl_name.is_some() {
            p_parse.n_mem += 1;
            let i_mem = p_parse.n_mem;

            let v = match get_vdbe(p_parse) {
                Some(v) => v,
                None => return,
            };

            begin_write_operation(p_parse, 1, ctx.i_db);

            // Cria a página raiz do índice com CreateIndex. Mas antes disso, codifica uma
            // instrução Noop e guarda seu endereço em Index.tnum. Isto é necessário caso este
            // índice seja na verdade uma PRIMARY KEY e a tabela seja na verdade uma tabela
            // WITHOUT ROWID. Nesse caso a rotina convertToWithoutRowidTable() troca o Noop por
            // um Goto que salta o código VDBE gerado abaixo.
            let tnum = vdbe_add_op0(&mut v.borrow_mut(), OP_NOOP as i32);
            p_index.borrow_mut().tnum = tnum as Pgno;
            vdbe_add_op3(&mut v.borrow_mut(), OP_CREATE_BTREE as i32, ctx.i_db, i_mem, BTREE_BLOBKEY as i32);

            // Junta o texto completo da instrução CREATE INDEX na variável zStmt.
            debug_assert!(ctx.p_name.is_some() || ctx.p_start.is_none());
            let z_stmt: Option<Vec<u8>> = if ctx.p_start.is_some() {
                let p_name = ctx.p_name.as_ref().unwrap();
                // A diferença de ponteiros `sLastToken.z - pName->z` do C: aqui os dois tokens
                // guardam o texto do início do token até o fim do SQL, então a distância é a
                // diferença dos comprimentos.
                let mut n: i64 = (p_name.z.len() as i64 - p_parse.s_last_token.z.len() as i64)
                    + p_parse.s_last_token.n as i64;
                if p_name.z[(n - 1) as usize] == b';' {
                    n -= 1;
                }
                // Um índice nomeado com uma instrução CREATE INDEX explícita.
                let unique: &[u8] = if ctx.on_error == OE_NONE as i32 { b"" } else { b" UNIQUE" };
                m_printf(
                    &db_rc.borrow(),
                    b"CREATE%s INDEX %.*s",
                    &[
                        PrintfArg::Text(Some(unique.to_vec())),
                        PrintfArg::Int(n),
                        PrintfArg::Text(Some(p_name.z.clone())),
                    ],
                )
            } else {
                // Um índice automático criado por uma restrição PRIMARY KEY ou UNIQUE.
                None
            };

            // Acrescenta uma entrada em sqlite_schema para este índice.
            let z_db_s_name = db_rc.borrow().a_db[i_db].z_db_s_name.clone();
            let tab_name = p_tab.borrow().z_name.clone();
            nested_parse(
                p_parse,
                b"INSERT INTO %Q.sqlite_master VALUES('index',%Q,%Q,#%d,%Q);",
                &[
                    PrintfArg::Text(Some(z_db_s_name)),
                    PrintfArg::Text(Some(z_name.clone())),
                    PrintfArg::Text(Some(tab_name)),
                    PrintfArg::Int(i_mem as i64),
                    PrintfArg::Text(z_stmt),
                ],
            );

            // Preenche o índice com dados e reanalisa o schema. Codifica um OP_Expire para
            // invalidar todas as instruções pré-compiladas.
            if ctx.p_tbl_name.is_some() {
                refill_index(p_parse, &p_index, i_mem);
                change_cookie(p_parse, ctx.i_db);
                let z_where = m_printf(
                    &db_rc.borrow(),
                    b"name='%q' AND type='index'",
                    &[PrintfArg::Text(Some(z_name.clone()))],
                );
                vdbe_add_parse_schema_op(&mut v.borrow_mut(), ctx.i_db, z_where, 0);
                vdbe_add_op2(&mut v.borrow_mut(), OP_EXPIRE as i32, 0, 1);
            }

            let tnum = p_index.borrow().tnum as i32;
            vdbe_jump_here(&mut v.borrow_mut(), tnum);
        }
    }
    if db_rc.borrow().init.busy != 0 || ctx.p_tbl_name.is_none() {
        let mut tab = p_tab.borrow_mut();
        p_index.borrow_mut().p_next = tab.p_index.take();
        tab.p_index = Some(p_index.clone());
        ctx.p_index = None;
    } else if in_rename_object(p_parse) {
        debug_assert!(p_parse.p_new_index.is_none());
        p_parse.p_new_index = Some(p_index.clone());
        ctx.p_index = None;
    }
}

/// Rótulo `exit_create_index`: limpeza antes de sair.
pub fn create_index_exit(p_parse: &mut Parse, ctx: &mut CreateIndexCtx) {
    let db_rc = p_parse.db.upgrade();
    if let Some(p_index) = ctx.p_index.take() {
        if let Some(db_rc) = &db_rc {
            free_index(&db_rc.borrow(), &mut p_index.borrow_mut());
        }
    }
    if let Some(p_tab) = ctx.p_tab.clone() {
        // Garante que todos os índices REPLACE de pTab ficam no fim da lista pIndex. A lista já
        // estava ordenada quando esta rotina foi chamada, então neste ponto no máximo um
        // índice (o recém-adicionado) está fora de ordem. Assim é preciso reordenar no máximo
        // um índice.
        //
        // `prev` é o nó cujo `p_next` é o `*ppFrom` do C; `None` quer dizer `&pTab->pIndex`.
        let mut prev: Option<IndexRef> = None;
        loop {
            let p_this = match &prev {
                None => p_tab.borrow().p_index.clone(),
                Some(p) => p.borrow().p_next.clone(),
            };
            let p_this = match p_this {
                Some(t) => t,
                None => break,
            };
            if p_this.borrow().on_error != OE_REPLACE {
                prev = Some(p_this);
                continue;
            }
            loop {
                let p_next = p_this.borrow().p_next.clone();
                let p_next = match p_next {
                    Some(n) if n.borrow().on_error != OE_REPLACE => n,
                    _ => break,
                };
                match &prev {
                    None => p_tab.borrow_mut().p_index = Some(p_next.clone()),
                    Some(p) => p.borrow_mut().p_next = Some(p_next.clone()),
                }
                let after = p_next.borrow().p_next.clone();
                p_this.borrow_mut().p_next = after;
                p_next.borrow_mut().p_next = Some(p_this.clone());
                prev = Some(p_next);
            }
            break;
        }
    }
    // pPIWhere, pList, pTblName e zName são donos únicos: soltá-los é o `*Delete`/`DbFree` do C.
    drop(ctx.p_pi_where.take());
    drop(ctx.p_list.take());
    drop(ctx.p_tbl_name.take());
    drop(ctx.z_name.take());
}


// ---- part_011.rs ----

/// Preenche o vetor `Index.ai_row_log_est[]` com informação padrão, a ser usada quando o comando
/// ANALYZE não foi executado.
///
/// `ai_row_log_est[0]` deveria conter o número de elementos do índice. Como não sabemos, chutamos
/// 1 milhão. `ai_row_log_est[1]` estima o número de linhas da tabela que casam com qualquer valor
/// da primeira coluna do índice. `ai_row_log_est[2]` estima o número de linhas que casam com
/// qualquer combinação das 2 primeiras colunas. E assim por diante. Sempre deve valer:
///
/// `ai_row_log_est[N] <= ai_row_log_est[N-1]` e `ai_row_log_est[N] >= 1`.
///
/// Fora isso, só temos a intuição de como inicializar o vetor. Os números gerados aqui se baseiam
/// em valores típicos de índices reais.
pub fn default_row_est(p_idx: &mut Index) {
    //                         10,  9,  8,  7,  6
    const A_VAL: [LogEst; 5] = [33, 32, 30, 28, 26];
    let n_copy = std::cmp::min(A_VAL.len(), p_idx.n_key_col as usize);

    // Índices com estimativas padrão não devem ter dados stat1
    debug_assert!(!p_idx.has_stat1);

    // Define a primeira entrada (número de linhas do índice) como o número estimado de linhas da
    // tabela, ou metade dele para um índice parcial.
    //
    // 2020-05-27: se parte dos dados vem da tabela sqlite_stat1 e o resto é chute, o número
    // estimado de linhas da tabela não pode ser menor que 1000 (LogEst 99). Sem isso, os índices
    // sem dados stat1 podem ser ignorados pelo planejador de consultas.
    let p_table = p_idx
        .p_table
        .upgrade()
        .expect("a tabela do índice precisa estar viva");
    let mut x: LogEst = p_table.borrow().n_row_log_est;
    debug_assert!(99 == log_est(1000));
    if x < 99 {
        x = 99;
        p_table.borrow_mut().n_row_log_est = x;
    }
    if p_idx.p_part_idx_where.is_some() {
        x -= 10;
        debug_assert!(10 == log_est(2));
    }
    p_idx.ai_row_log_est[0] = x;

    // Estima que a[1] é 10, a[2] é 9, a[3] é 8, a[4] é 7, a[5] é 6 e cada valor seguinte (se
    // houver) é 5.
    p_idx.ai_row_log_est[1..1 + n_copy].copy_from_slice(&A_VAL[..n_copy]);
    let mut i = n_copy + 1;
    while i <= p_idx.n_key_col as usize {
        p_idx.ai_row_log_est[i] = 23;
        debug_assert!(23 == log_est(5));
        i += 1;
    }

    debug_assert!(0 == log_est(1));
    if is_unique_index(p_idx) {
        let n_key_col = p_idx.n_key_col as usize;
        p_idx.ai_row_log_est[n_key_col] = 0;
    }
}

/// Esta rotina remove um índice nomeado existente. Implementa o comando DROP INDEX.
pub fn drop_index(p_parse: &mut Parse, p_name: Box<SrcList>, if_exists: i32) {
    let db = p_parse
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");

    'exit_drop_index: {
        if db.borrow().malloc_failed != 0 {
            break 'exit_drop_index;
        }
        debug_assert!(p_parse.n_err == 0); // Nunca chamada com erros anteriores além de OOM
        debug_assert!(p_name.n_src == 1);
        if SQLITE_OK != read_schema(p_parse) {
            break 'exit_drop_index;
        }
        let z_database: Option<&[u8]> = if p_name.a[0].z_database.is_empty() {
            None
        } else {
            Some(&p_name.a[0].z_database[..])
        };
        let p_index = find_index(&db.borrow(), &p_name.a[0].z_name, z_database);
        let p_index = match p_index {
            Some(p_index) => p_index,
            None => {
                if if_exists == 0 {
                    error_msg(
                        p_parse,
                        b"no such index: %S",
                        &[PrintfArg::SrcItem(p_name.a[0].clone())],
                    );
                } else {
                    code_verify_named_schema(p_parse, z_database);
                    force_not_read_only(p_parse);
                }
                p_parse.check_schema = 1;
                break 'exit_drop_index;
            }
        };
        if p_index.borrow().idx_type != SQLITE_IDXTYPE_APPDEF {
            error_msg(
                p_parse,
                b"index associated with UNIQUE or PRIMARY KEY constraint cannot be dropped",
                &[],
            );
            break 'exit_drop_index;
        }
        let i_db: i32 = {
            let p_schema = p_index.borrow().p_schema.as_ref().and_then(|w| w.upgrade());
            schema_to_index(&db.borrow(), p_schema.as_ref())
        };
        // SQLITE_OMIT_AUTHORIZATION não está ligado no Debian 13
        {
            let mut code = SQLITE_DROP_INDEX;
            let z_idx_name: Vec<u8> = p_index.borrow().z_name.clone();
            let z_tab_name: Vec<u8> = p_index
                .borrow()
                .p_table
                .upgrade()
                .expect("a tabela do índice precisa estar viva")
                .borrow()
                .z_name
                .clone();
            let z_db: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
            let z_tab: &[u8] = schema_table(i_db);
            if auth_check(p_parse, SQLITE_DELETE, Some(z_tab), None, Some(&z_db)) != 0 {
                break 'exit_drop_index;
            }
            if OMIT_TEMPDB == 0 && i_db == 1 {
                code = SQLITE_DROP_TEMP_INDEX;
            }
            if auth_check(
                p_parse,
                code,
                Some(&z_idx_name),
                Some(&z_tab_name),
                Some(&z_db),
            ) != 0
            {
                break 'exit_drop_index;
            }
        }

        // Gera o código que remove o índice e a entrada dele da tabela de schema
        let v = get_vdbe(p_parse);
        if let Some(v) = v {
            let z_db_s_name: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_s_name.clone();
            let z_idx_name: Vec<u8> = p_index.borrow().z_name.clone();
            begin_write_operation(p_parse, 1, i_db);
            nested_parse(
                p_parse,
                b"DELETE FROM %Q.sqlite_master WHERE name=%Q AND type='index'",
                &[
                    PrintfArg::Text(Some(z_db_s_name)),
                    PrintfArg::Text(Some(z_idx_name.clone())),
                ],
            );
            clear_stat_tables(p_parse, i_db, b"idx", &z_idx_name);
            change_cookie(p_parse, i_db);
            let tnum = p_index.borrow().tnum;
            destroy_root_page(p_parse, tnum as i32, i_db);
            vdbe_add_op4(
                &mut v.borrow_mut(),
                OP_DROPINDEX as i32,
                i_db,
                0,
                0,
                P4Value::Text(z_idx_name),
                P4_DYNAMIC,
            );
        }
    }
    // exit_drop_index:
    src_list_delete(&db.borrow(), Some(p_name));
}

/// `p_array` é um vetor de objetos. Esta rotina estende o vetor para haver espaço para um novo
/// objeto no fim (o C usa `sqlite3DbRealloc()`, aqui é o crescimento do `Vec`).
///
/// Quando a função é chamada, `*pn_entry` contém o tamanho atual do vetor (em entradas). Se a
/// realocação der certo (sem OOM), o novo objeto nasce zerado (`T::default()`), `*pn_entry` é
/// atualizado e `*p_idx` recebe o índice da nova entrada.
///
/// Caso contrário, se a realocação falhar, `*p_idx` vale -1, `*pn_entry` fica como estava e o
/// vetor segue inalterado.
pub fn array_allocate<T: Default>(
    db: &mut Sqlite3,
    p_array: &mut Vec<T>,
    pn_entry: &mut i32,
    p_idx: &mut i32,
) {
    let n: i64 = *pn_entry as i64;
    *p_idx = *pn_entry;
    if (n & (n - 1)) == 0 {
        let sz: i64 = if n == 0 { 1 } else { 2 * n };
        let falta = (sz as usize).saturating_sub(p_array.len());
        if p_array.try_reserve_exact(falta).is_err() {
            oom_fault(db);
            *p_idx = -1;
            return;
        }
    }
    // memset(&z[n * szEntry], 0, szEntry)
    let n = n as usize;
    if p_array.len() <= n {
        p_array.resize_with(n + 1, T::default);
    } else {
        p_array[n] = T::default();
    }
    *pn_entry += 1;
}

/// Acrescenta um novo elemento à `IdList` dada. Cria uma `IdList` nova se preciso.
///
/// Devolve a nova `IdList`, ou `None` se o malloc() falhar.
pub fn id_list_append(
    p_parse: &mut Parse,
    p_list: Option<Box<IdList>>,
    p_token: &Token,
) -> Option<Box<IdList>> {
    let db = p_parse
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");
    let mut p_list: Box<IdList> = match p_list {
        None => {
            // sqlite3DbMallocZero devolve 0 quando db->mallocFailed já está ligado
            if db.borrow().malloc_failed != 0 {
                return None;
            }
            Box::new(IdList {
                n_id: 0,
                e_u4: EU4_NONE,
                a: Vec::new(),
            })
        }
        Some(p_list) => p_list,
    };
    let i = p_list.n_id as usize;
    p_list.n_id += 1;
    let z_name = name_from_token(&db.borrow(), p_token);
    p_list.a.push(IdListItem {
        z_name: z_name.clone().unwrap_or_default(),
        u4_idx: 0,
    });
    debug_assert!(p_list.a.len() == i + 1);
    if in_rename_object(p_parse) && z_name.is_some() {
        // A chave do mapa de tokens é a identidade do buffer do nome (o ponteiro do C).
        rename_token_map(p_parse, p_list.a[i].z_name.as_ptr() as usize, p_token);
    }
    Some(p_list)
}

/// Apaga uma `IdList`.
pub fn id_list_delete(db: &Sqlite3, p_list: Option<Box<IdList>>) {
    let _ = db;
    let p_list = match p_list {
        None => return,
        Some(p_list) => p_list,
    };
    debug_assert!(p_list.e_u4 != EU4_EXPR); // O modo EU4_EXPR não é usado no momento
    // Os nomes (zName) e a própria lista são liberados pelo drop
    drop(p_list);
}

/// Devolve o índice em `p_list` do identificador chamado `z_name`, ou -1 se não achar.
pub fn id_list_index(p_list: &IdList, z_name: &[u8]) -> i32 {
    for i in 0..p_list.n_id {
        if str_i_cmp(&p_list.a[i as usize].z_name, z_name) == 0 {
            return i;
        }
    }
    -1
}

/// Tamanho máximo de um objeto `SrcList`.
/// O objeto `SrcList` representa a cláusula FROM de um SELECT, e o planejador de consultas não
/// lida com mais de 64 tabelas num join. Qualquer valor acima de 64 serve para a maioria dos usos.
/// Valores menores, como 10, servem para aplicações pequenas e com pouca memória.
pub const SQLITE_MAX_SRCLIST: i32 = 200;

/// Expande o espaço alocado para a `SrcList` criando `n_extra` slots novos a partir de `i_start`
/// (base zero). Os slots novos são zerados.
///
/// Por exemplo, se a lista tem duas entradas A,B, para acrescentar 3 no fim faça
/// `src_list_enlarge(p_parse, p_src, 3, 2)`: o resultado é A, B, nil, nil, nil. Com `i_start` 1 o
/// resultado seria A, nil, nil, nil, B. Com `i_start` 0 (inserir no começo): nil, nil, nil, A, B.
///
/// A lista cresce no lugar. Se a alocação falhar ou a lista ficar grande demais, a `SrcList`
/// original fica inalterada, a função devolve `false` e deixa uma mensagem de erro em `p_parse`.
pub fn src_list_enlarge(
    p_parse: &mut Parse,
    p_src: &mut SrcList,
    n_extra: i32,
    i_start: i32,
) -> bool {
    // Verificação de sanidade dos parâmetros
    debug_assert!(i_start >= 0);
    debug_assert!(n_extra >= 1);
    debug_assert!(i_start <= p_src.n_src);

    // Aloca mais espaço se preciso
    if (p_src.n_src as u32).wrapping_add(n_extra as u32) > p_src.n_alloc {
        let mut n_alloc: i64 = 2 * (p_src.n_src as i64) + n_extra as i64;

        if p_src.n_src + n_extra >= SQLITE_MAX_SRCLIST {
            error_msg(
                p_parse,
                b"too many FROM clause terms, max: %d",
                &[PrintfArg::Int(SQLITE_MAX_SRCLIST as i64)],
            );
            return false;
        }
        if n_alloc > SQLITE_MAX_SRCLIST as i64 {
            n_alloc = SQLITE_MAX_SRCLIST as i64;
        }
        let falta = (n_alloc as usize).saturating_sub(p_src.a.len());
        if p_src.a.try_reserve_exact(falta).is_err() {
            let db = p_parse
                .db
                .upgrade()
                .expect("a conexão precisa estar viva enquanto o Parse existe");
            oom_fault(&mut db.borrow_mut());
            return false;
        }
        p_src.n_alloc = n_alloc as u32;
    }

    // Move para longe os slots existentes que vêm depois dos novos, e zera os novos slots
    // (memset do C, com iCursor = -1)
    let mut cauda: Vec<SrcItem> = p_src.a.split_off(i_start as usize);
    for _ in 0..n_extra {
        let mut p_item = SrcItem::default();
        p_item.i_cursor = -1;
        p_src.a.push(p_item);
    }
    p_src.a.append(&mut cauda);
    p_src.n_src += n_extra;

    true
}

/// Acrescenta um novo nome de tabela à `SrcList` dada. Cria uma `SrcList` nova se preciso. Uma
/// entrada nova é criada mesmo que `p_table` seja `None`.
///
/// Devolve a `SrcList`, ou `None` se houver erro de OOM ou se a lista crescer demais. A lista
/// devolvida pode ser a mesma da entrada ou outra. Se ocorrer OOM, o valor anterior de `p_list` é
/// liberado automaticamente.
///
/// Se `p_database` não é nulo, a tabela tem um prefixo opcional de banco: "banco.tabela". Aí
/// `p_database` aponta para o nome da tabela e `p_table` para o nome do banco. `SrcList.a[].z_name`
/// recebe o nome da tabela, que vem de `p_table` (se `p_database` é nulo) ou de `p_database`.
/// `SrcList.a[].z_database` recebe o nome do banco vindo de `p_table`, ou fica nulo se nenhum
/// banco foi dado.
///
/// Ou seja, `src_list_append(D, A, B, None)` quer dizer que B é o nome da tabela e o banco não foi
/// especificado. `src_list_append(D, A, B, C)` quer dizer que C é a tabela e B é o banco. Se C
/// existe, B também existe: nunca ocorre `src_list_append(D, A, None, C)`.
///
/// `p_table` e `p_database` são considerados entre aspas; o `name_from_token` as remove antes de
/// entrarem na `SrcList`.
pub fn src_list_append(
    p_parse: &mut Parse,
    p_list: Option<Box<SrcList>>,
    p_table: Option<&Token>,
    p_database: Option<&Token>,
) -> Option<Box<SrcList>> {
    debug_assert!(p_database.is_none() || p_table.is_some()); // Não existe C sem B
    let db = p_parse
        .db
        .upgrade()
        .expect("a conexão precisa estar viva enquanto o Parse existe");
    let mut p_list: Box<SrcList> = match p_list {
        None => {
            // sqlite3DbMallocRawNN devolve 0 quando db->mallocFailed já está ligado
            if db.borrow().malloc_failed != 0 {
                return None;
            }
            let mut p_item = SrcItem::default();
            p_item.i_cursor = -1;
            Box::new(SrcList {
                n_alloc: 1,
                n_src: 1,
                a: vec![p_item],
            })
        }
        Some(mut p_list) => {
            let n_src = p_list.n_src;
            if !src_list_enlarge(p_parse, &mut p_list, 1, n_src) {
                src_list_delete(&db.borrow(), Some(p_list));
                return None;
            }
            p_list
        }
    };
    let i_last = (p_list.n_src - 1) as usize;
    let p_database = match p_database {
        Some(t) if t.z.is_empty() => None,
        other => other,
    };
    let (z_name, z_database) = if let Some(p_database) = p_database {
        (
            name_from_token(&db.borrow(), p_database),
            p_table.and_then(|t| name_from_token(&db.borrow(), t)),
        )
    } else {
        (p_table.and_then(|t| name_from_token(&db.borrow(), t)), None)
    };
    let p_item = &mut p_list.a[i_last];
    p_item.z_name = z_name.unwrap_or_default();
    p_item.z_database = z_database.unwrap_or_default();
    Some(p_list)
}


// ---- part_012.rs ----

/// Atribui números de cursor do VdbeCursor a todas as tabelas de um SrcList.
pub fn src_list_assign_cursors(p_parse: &mut Parse, p_list: Option<&mut SrcList>) {
    if let Some(p_list) = p_list {
        for p_item in p_list.a.iter_mut() {
            if p_item.i_cursor >= 0 {
                continue;
            }
            p_item.i_cursor = p_parse.n_tab;
            p_parse.n_tab += 1;
            if let Some(p_select) = p_item.p_select.as_mut() {
                src_list_assign_cursors(p_parse, p_select.p_src.as_deref_mut());
            }
        }
    }
}

/// Apaga um SrcList inteiro, incluindo toda a subestrutura.
pub fn src_list_delete(db: &Sqlite3Ref, p_list: Option<Box<SrcList>>) {
    let mut p_list = match p_list {
        Some(l) => l,
        None => return,
    };
    for p_item in p_list.a.iter_mut() {
        // z_database, z_name, z_alias, z_indexed_by, p_func_arg, p_select, p_on
        // e p_using são donos únicos: o Drop os libera. A tabela é contada por
        // referência e o delete_table faz o decremento.
        delete_table(db, p_item.p_tab.take());
        if let Some(p_select) = p_item.p_select.take() {
            select_delete(db, Some(p_select));
        }
        if p_item.fg.is_indexed_by {
            p_item.z_indexed_by = None;
        }
        if p_item.fg.is_tab_func {
            expr_list_delete(db, p_item.p_func_arg.take());
        }
        if p_item.fg.is_using {
            id_list_delete(db, p_item.p_using.take());
        } else if p_item.p_on.is_some() {
            expr_delete(db, p_item.p_on.take());
        }
    }
}

/// Chamada pelo parser para adicionar um termo novo ao fim de uma cláusula FROM
/// em construção. `p` é a parte já construída (None no primeiro termo).
/// `p_table` e `p_database` são nulos para subconsultas. Devolve o SrcList novo.
pub fn src_list_append_from_term(
    p_parse: &mut Parse,
    p: Option<Box<SrcList>>,
    p_table: Option<&Token>,
    p_database: Option<&Token>,
    p_alias: &Token,
    p_subquery: Option<Box<Select>>,
    mut p_on_using: Option<&mut OnOrUsing>,
) -> Option<Box<SrcList>> {
    let db = p_parse.db.clone();
    'append_from_error: {
        if p.is_none() {
            if let Some(ou) = p_on_using.as_deref() {
                if ou.p_on.is_some() || ou.p_using.is_some() {
                    let which: &[u8] = if ou.p_on.is_some() { b"ON" } else { b"USING" };
                    error_msg(
                        p_parse,
                        b"a JOIN clause is required before %s",
                        &[PrintfArg::Bytes(which)],
                    );
                    break 'append_from_error;
                }
            }
        }
        let mut p = match src_list_append(p_parse, p, p_table, p_database) {
            Some(p) => p,
            None => break 'append_from_error,
        };
        let last = p.a.len() - 1;
        let p_item = &mut p.a[last];
        if in_rename_object(p_parse) && p_item.z_name.is_some() {
            let p_token = match p_database {
                Some(d) if d.z.is_some() => d,
                _ => p_table.unwrap(),
            };
            rename_token_map(p_parse, p_item.z_name.as_deref().unwrap(), p_token);
        }
        if p_alias.n != 0 {
            p_item.z_alias = name_from_token(&db, p_alias);
        }
        if let Some(sub) = p_subquery {
            if sub.sel_flags & SF_NESTEDFROM != 0 {
                p_item.fg.is_nested_from = true;
            }
            p_item.p_select = Some(sub);
        }
        match p_on_using.as_deref_mut() {
            None => {
                p_item.p_on = None;
            }
            Some(ou) => {
                if ou.p_using.is_some() {
                    p_item.fg.is_using = true;
                    p_item.p_using = ou.p_using.take();
                } else {
                    p_item.p_on = ou.p_on.take();
                }
            }
        }
        return Some(p);
    }
    // append_from_error
    clear_on_or_using(&db, p_on_using);
    select_delete(&db, p_subquery);
    None
}

/// Adiciona uma cláusula INDEXED BY ou NOT INDEXED ao elemento mais recente
/// da lista de origem passada como segundo argumento.
pub fn src_list_indexed_by(p_parse: &mut Parse, p: Option<&mut SrcList>, p_indexed_by: &Token) {
    if let Some(p) = p {
        if p_indexed_by.n > 0 {
            let last = p.a.len() - 1;
            let p_item = &mut p.a[last];
            if p_indexed_by.n == 1 && p_indexed_by.z.is_none() {
                // Foi dada uma cláusula "NOT INDEXED". Ver o construto
                // "indexed_opt" do parse.y.
                p_item.fg.not_indexed = true;
            } else {
                let db = p_parse.db.clone();
                p_item.z_indexed_by = name_from_token(&db, p_indexed_by);
                p_item.fg.is_indexed_by = true;
            }
        }
    }
}

/// Anexa o conteúdo do SrcList `p2` ao SrcList `p1` e devolve o resultado, ou
/// None se der erro. Em todos os casos `p1` e `p2` são consumidos aqui.
pub fn src_list_append_list(
    p_parse: &mut Parse,
    p1: Box<SrcList>,
    p2: Option<Box<SrcList>>,
) -> Option<Box<SrcList>> {
    let mut p1 = p1;
    if let Some(mut p2) = p2 {
        let n_src = p2.a.len() as i32;
        match src_list_enlarge(p_parse, p1, n_src, 1) {
            None => {
                let db = p_parse.db.clone();
                src_list_delete(&db, Some(p2));
                return None;
            }
            Some(p_new) => {
                p1 = p_new;
                for (k, item) in p2.a.drain(..).enumerate() {
                    p1.a[1 + k] = item;
                }
                let j = JT_LTORJ & p1.a[1].fg.jointype;
                p1.a[0].fg.jointype |= j;
            }
        }
    }
    Some(p1)
}

/// Adiciona a lista de argumentos de função à entrada do SrcList de uma
/// função com valor de tabela.
pub fn src_list_func_args(p_parse: &mut Parse, p: Option<&mut SrcList>, p_list: Option<Box<ExprList>>) {
    if let Some(p) = p {
        let last = p.a.len() - 1;
        let p_item = &mut p.a[last];
        p_item.p_func_arg = p_list;
        p_item.fg.is_tab_func = true;
    } else {
        let db = p_parse.db.clone();
        expr_list_delete(&db, p_list);
    }
}

/// Ao montar a cláusula FROM no parser, o operador de junção é anexado
/// primeiro ao operando da esquerda, mas o gerador de código o espera no da
/// direita. Esta rotina desloca todos os operadores da esquerda para a direita
/// e marca com JT_LTORJ as tabelas à esquerda do RIGHT JOIN mais à direita.
pub fn src_list_shift_join_type(_p_parse: &mut Parse, p: Option<&mut SrcList>) {
    if let Some(p) = p {
        let n_src = p.a.len();
        if n_src > 1 {
            let mut i = n_src - 1;
            let mut all_flags: u8 = 0;
            loop {
                p.a[i].fg.jointype = p.a[i - 1].fg.jointype;
                all_flags |= p.a[i].fg.jointype;
                i -= 1;
                if i == 0 {
                    break;
                }
            }
            p.a[0].fg.jointype = 0;

            // Todos os termos à esquerda de um RIGHT JOIN recebem JT_LTORJ.
            if all_flags & JT_RIGHT != 0 {
                let mut i = (n_src - 1) as i32;
                while i > 0 && (p.a[i as usize].fg.jointype & JT_RIGHT) == 0 {
                    i -= 1;
                }
                i -= 1;
                loop {
                    p.a[i as usize].fg.jointype |= JT_LTORJ;
                    i -= 1;
                    if i < 0 {
                        break;
                    }
                }
            }
        }
    }
}

/// Gera código VDBE para uma instrução BEGIN.
pub fn begin_transaction(p_parse: &mut Parse, type_: i32) {
    let db = p_parse.db.clone();
    if auth_check(p_parse, SQLITE_TRANSACTION, Some(b"BEGIN"), None, None) != 0 {
        return;
    }
    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => return,
    };
    if type_ != TK_DEFERRED {
        let n_db = db.borrow().n_db;
        for i in 0..n_db {
            let p_bt = db.borrow().a_db[i as usize].p_bt.clone();
            let e_txn_type = if p_bt.as_ref().map_or(false, |bt| btree_is_readonly(bt)) {
                0 // transação de leitura
            } else if type_ == TK_EXCLUSIVE {
                2 // transação exclusiva
            } else {
                1 // transação de escrita
            };
            vdbe_add_op2(&v, OP_TRANSACTION, i, e_txn_type);
            vdbe_uses_btree(&v, i);
        }
    }
    vdbe_add_op0(&v, OP_AUTOCOMMIT);
}

/// Gera código VDBE para uma instrução COMMIT ou ROLLBACK. O código de
/// ROLLBACK sai se `e_type == TK_ROLLBACK`; senão sai o de COMMIT.
pub fn end_transaction(p_parse: &mut Parse, e_type: i32) {
    let is_rollback = e_type == TK_ROLLBACK;
    let z: &[u8] = if is_rollback { b"ROLLBACK" } else { b"COMMIT" };
    if auth_check(p_parse, SQLITE_TRANSACTION, Some(z), None, None) != 0 {
        return;
    }
    if let Some(v) = get_vdbe(p_parse) {
        vdbe_add_op2(&v, OP_AUTOCOMMIT, 1, is_rollback as i32);
    }
}

/// Chamada pelo parser ao interpretar um comando para criar, liberar ou
/// desfazer um savepoint de SQL.
pub fn savepoint(p_parse: &mut Parse, op: i32, p_name: &Token) {
    let db = p_parse.db.clone();
    let z_name = name_from_token(&db, p_name);
    if let Some(z_name) = z_name {
        let v = get_vdbe(p_parse);
        const AZ: [&[u8]; 3] = [b"BEGIN", b"RELEASE", b"ROLLBACK"];
        let v = match v {
            Some(v) => v,
            None => return,
        };
        if auth_check(p_parse, SQLITE_SAVEPOINT, Some(AZ[op as usize]), Some(&z_name), None) != 0 {
            return;
        }
        vdbe_add_op4(&v, OP_SAVEPOINT, op, 0, 0, Some(z_name), P4_DYNAMIC);
    }
}

/// Garante que o banco TEMP está aberto e disponível. Devolve o número de
/// erros e deixa as mensagens em `p_parse`.
pub fn open_temp_database(p_parse: &mut Parse) -> i32 {
    let db = p_parse.db.clone();
    let precisa = db.borrow().a_db[1].p_bt.is_none() && !p_parse.explain;
    if precisa {
        const FLAGS: i32 = SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE
            | SQLITE_OPEN_TEMP_DB;

        let (rc, p_bt) = btree_open(&db.borrow().p_vfs.clone(), None, &db, FLAGS);
        if rc != SQLITE_OK {
            error_msg(
                p_parse,
                b"unable to open a temporary database file for storing temporary tables",
                &[],
            );
            p_parse.rc = rc;
            return 1;
        }
        let p_bt = p_bt.unwrap();
        db.borrow_mut().a_db[1].p_bt = Some(p_bt.clone());
        let next_pagesize = db.borrow().next_pagesize;
        if SQLITE_NOMEM == btree_set_page_size(&p_bt, next_pagesize, 0, 0) {
            oom_fault(&db);
            return 1;
        }
    }
    0
}

/// Registra que o cookie do esquema precisará ser verificado para o banco
/// `i_db`. O código que o verifica sai no fim do VDBE de nível mais alto e é
/// gerado depois por `finish_coding()`.
pub fn code_verify_schema_at_toplevel(p_toplevel: &mut Parse, i_db: i32) {
    if db_mask_test(p_toplevel.cookie_mask, i_db) == 0 {
        db_mask_set(&mut p_toplevel.cookie_mask, i_db);
        if !OMIT_TEMPDB && i_db == 1 {
            open_temp_database(p_toplevel);
        }
    }
}


// ---- part_013.rs ----

pub fn code_verify_schema(p_parse: &mut Parse, i_db: i32) {
    code_verify_schema_at_toplevel(parse_toplevel(p_parse), i_db);
}

/// Se `z_db` for None, chama `code_verify_schema()` para cada banco anexado.
/// Senão, a invoca só para o banco chamado `z_db`.
pub fn code_verify_named_schema(p_parse: &mut Parse, z_db: Option<&[u8]>) {
    let db = p_parse.db.clone();
    let n_db = db.borrow().n_db;
    for i in 0..n_db {
        let faz = {
            let db_ref = db.borrow();
            let p_db = &db_ref.a_db[i as usize];
            p_db.p_bt.is_some()
                && match z_db {
                    None => true,
                    Some(z) => str_i_cmp(z, &p_db.z_db_sname) == 0,
                }
        };
        if faz {
            code_verify_schema(p_parse, i);
        }
    }
}

/// Gera código VDBE que se prepara para uma operação que pode mudar o banco.
///
/// Inicia uma transação se ainda não há uma. Se já há, um checkpoint é
/// definido quando `set_statement` é verdadeiro. O checkpoint serve a operações
/// que podem falhar no meio do caminho (por restrição) e precisam desfazer
/// algumas escritas sem reverter a transação inteira.
pub fn begin_write_operation(p_parse: &mut Parse, set_statement: i32, i_db: i32) {
    let p_toplevel = parse_toplevel(p_parse);
    code_verify_schema_at_toplevel(p_toplevel, i_db);
    db_mask_set(&mut p_toplevel.write_mask, i_db);
    p_toplevel.is_multi_write |= set_statement as u8;
}

/// Indica que a instrução em construção pode escrever mais de uma entrada.
/// Se um aborto ocorrer depois de algumas escritas, será preciso desfazê-las.
pub fn multi_write(p_parse: &mut Parse) {
    parse_toplevel(p_parse).is_multi_write = 1;
}

/// O gerador de código chama esta rotina ao descobrir que é possível abortar a
/// instrução antes do fim. Para abortar sem corromper o banco, a instrução
/// precisa estar protegida por uma transação de instrução.
///
/// Tecnicamente só seria preciso marcar `may_abort` se `is_multi_write` já
/// estivesse marcado, mas a otimização foi descartada pelo caminho seguro.
pub fn may_abort(p_parse: &mut Parse) {
    parse_toplevel(p_parse).may_abort = 1;
}

/// Codifica um `OP_Halt` que faz o VDBE devolver SQLITE_CONSTRAINT. O parâmetro
/// `on_error` determina qual parte (se houver) da instrução e da transação
/// atual sofre rollback.
pub fn halt_constraint(
    p_parse: &mut Parse,
    err_code: i32,
    on_error: i32,
    p4: Option<Vec<u8>>,
    p4_type: i8,
    p5_errmsg: u8,
) {
    let v = get_vdbe(p_parse).unwrap();
    if on_error == OE_ABORT {
        may_abort(p_parse);
    }
    vdbe_add_op4(&v, OP_HALT, err_code, on_error, 0, p4, p4_type);
    vdbe_change_p5(&v, p5_errmsg);
}

/// Codifica um `OP_Halt` por violação de UNIQUE ou PRIMARY KEY.
pub fn unique_constraint(p_parse: &mut Parse, on_error: i32, p_idx: &IndexRef) {
    let db = p_parse.db.clone();
    let p_idx_ref = p_idx.borrow();
    let p_tab = p_idx_ref.p_table.upgrade().unwrap();
    let p_tab_ref = p_tab.borrow();

    let mut err_msg = StrAccum::default();
    let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    str_accum_init(&mut err_msg, Some(&db), None, 0, limit);
    if p_idx_ref.a_col_expr.is_some() {
        api::str_appendf(&mut err_msg, b"index '%q'", &[PrintfArg::Bytes(&p_idx_ref.z_name)]);
    } else {
        for j in 0..p_idx_ref.n_key_col as usize {
            let z_col = &p_tab_ref.a_col[p_idx_ref.ai_column[j] as usize].z_cn_name;
            if j > 0 {
                api::str_append(&mut err_msg, b", ", 2);
            }
            api::str_appendall(&mut err_msg, &p_tab_ref.z_name);
            api::str_append(&mut err_msg, b".", 1);
            api::str_appendall(&mut err_msg, z_col);
        }
    }
    let z_err = str_accum_finish(&mut err_msg);
    let code = if is_primary_key_index(&p_idx_ref) {
        SQLITE_CONSTRAINT_PRIMARYKEY
    } else {
        SQLITE_CONSTRAINT_UNIQUE
    };
    drop(p_tab_ref);
    drop(p_idx_ref);
    halt_constraint(p_parse, code, on_error, z_err, P4_DYNAMIC, P5_CONSTRAINTUNIQUE);
}

/// Codifica um `OP_Halt` por rowid não único.
pub fn rowid_constraint(p_parse: &mut Parse, on_error: i32, p_tab: &TableRef) {
    let db = p_parse.db.clone();
    let (z_msg, rc) = {
        let t = p_tab.borrow();
        if t.i_p_key >= 0 {
            (
                m_printf(
                    &db,
                    b"%s.%s",
                    &[
                        PrintfArg::Bytes(&t.z_name),
                        PrintfArg::Bytes(&t.a_col[t.i_p_key as usize].z_cn_name),
                    ],
                ),
                SQLITE_CONSTRAINT_PRIMARYKEY,
            )
        } else {
            (
                m_printf(&db, b"%s.rowid", &[PrintfArg::Bytes(&t.z_name)]),
                SQLITE_CONSTRAINT_ROWID,
            )
        }
    };
    halt_constraint(p_parse, rc, on_error, z_msg, P4_DYNAMIC, P5_CONSTRAINTUNIQUE);
}

/// Verifica se `p_index` usa a sequência de comparação `z_coll`.
fn collation_match(z_coll: &[u8], p_index: &Index) -> bool {
    for i in 0..p_index.n_column as usize {
        let z = &p_index.az_coll[i];
        if p_index.ai_column[i] >= 0 && str_i_cmp(z, z_coll) == 0 {
            return true;
        }
    }
    false
}

/// Recomputa todos os índices de `p_tab` que usam a sequência `z_coll`. Se
/// `z_coll` for None, recomputa todos os índices de `p_tab`.
fn reindex_table(p_parse: &mut Parse, p_tab: &TableRef, z_coll: Option<&[u8]>) {
    if !is_virtual(&p_tab.borrow()) {
        let mut p_index = p_tab.borrow().p_index.clone();
        while let Some(idx) = p_index {
            let casa = match z_coll {
                None => true,
                Some(z) => collation_match(z, &idx.borrow()),
            };
            if casa {
                let db = p_parse.db.clone();
                let p_schema = p_tab.borrow().p_schema.clone();
                let i_db = schema_to_index(&db, p_schema.as_ref());
                begin_write_operation(p_parse, 0, i_db);
                refill_index(p_parse, &idx, -1);
            }
            p_index = idx.borrow().p_next.clone();
        }
    }
}

/// Recomputa todos os índices de todas as tabelas de todos os bancos cujos
/// índices usam a sequência `z_coll`. Se None, recomputa todos os índices.
fn reindex_databases(p_parse: &mut Parse, z_coll: Option<&[u8]>) {
    let db = p_parse.db.clone();
    let n_db = db.borrow().n_db;
    for i_db in 0..n_db {
        let p_schema = db.borrow().a_db[i_db as usize].p_schema.clone().unwrap();
        // As tabelas são coletadas na ordem da hash antes de reindexar: o
        // reindex não altera a tbl_hash.
        let mut tabs: Vec<TableRef> = Vec::new();
        {
            let schema_ref = p_schema.borrow();
            let mut k = hash_first(&schema_ref.tbl_hash);
            while let Some(e) = k {
                tabs.push(hash_data(&e));
                k = hash_next(&e);
            }
        }
        for p_tab in tabs {
            reindex_table(p_parse, &p_tab, z_coll);
        }
    }
}

/// Gera código para o comando REINDEX.
///
///        REINDEX                            -- 1
///        REINDEX  <collation>               -- 2
///        REINDEX  ?<database>.?<tablename>  -- 3
///        REINDEX  ?<database>.?<indexname>  -- 4
///
/// A forma 1 reconstrói todos os índices de todos os bancos anexados. A 2
/// reconstrói os que usam a função de comparação nomeada. As formas 3 e 4
/// reconstroem o índice nomeado ou todos os índices da tabela nomeada.
pub fn reindex(p_parse: &mut Parse, p_name1: Option<&Token>, p_name2: Option<&Token>) {
    let db = p_parse.db.clone();

    // Lê o esquema. Se der erro, deixa mensagem e código em p_parse e sai.
    if SQLITE_OK != read_schema(p_parse) {
        return;
    }

    let p_name1 = match p_name1 {
        None => {
            reindex_databases(p_parse, None);
            return;
        }
        Some(t) => t,
    };
    if p_name2.map_or(true, |t| t.z.is_none()) {
        let z_coll = match name_from_token(&db, p_name1) {
            Some(z) => z,
            None => return,
        };
        let enc = enc(&db.borrow());
        let p_coll = find_coll_seq(&db, enc, &z_coll, 0);
        if p_coll.is_some() {
            reindex_databases(p_parse, Some(&z_coll));
            return;
        }
    }
    let (i_db, p_obj_name) = two_part_name(p_parse, p_name1, p_name2);
    if i_db < 0 {
        return;
    }
    let z = match name_from_token(&db, p_obj_name.unwrap()) {
        Some(z) => z,
        None => return,
    };
    let z_db_owned: Option<Vec<u8>> = if p_name2.map_or(false, |t| t.n != 0) {
        Some(db.borrow().a_db[i_db as usize].z_db_sname.clone())
    } else {
        None
    };
    let z_db = z_db_owned.as_deref();
    if let Some(p_tab) = find_table(&db, &z, z_db) {
        reindex_table(p_parse, &p_tab, None);
        return;
    }
    if let Some(p_index) = find_index(&db, &z, z_db) {
        let p_schema = p_index.borrow().p_table.upgrade().unwrap().borrow().p_schema.clone();
        let i_db = schema_to_index(&db, p_schema.as_ref());
        begin_write_operation(p_parse, 0, i_db);
        refill_index(p_parse, &p_index, -1);
        return;
    }
    error_msg(p_parse, b"unable to identify the object to be reindexed", &[]);
}

/// Devolve um KeyInfo apropriado para o Index dado. O chamador deve invocar
/// `key_info_unref()` no objeto devolvido quando terminar de usá-lo.
pub fn key_info_of_index(p_parse: &mut Parse, p_idx: &IndexRef) -> Option<KeyInfoRef> {
    let (n_col, n_key, uniq_not_null) = {
        let i = p_idx.borrow();
        (i.n_column, i.n_key_col, i.uniq_not_null)
    };
    if p_parse.n_err != 0 {
        return None;
    }
    let db = p_parse.db.clone();
    let p_key = if uniq_not_null {
        key_info_alloc(&db, n_key, n_col - n_key)
    } else {
        key_info_alloc(&db, n_col, 0)
    };
    let p_key = match p_key {
        Some(k) => k,
        None => return None,
    };
    for i in 0..n_col as usize {
        let (z_coll, sort) = {
            let idx = p_idx.borrow();
            (idx.az_coll[i].clone(), idx.a_sort_order[i])
        };
        // No C a comparação é de ponteiro com sqlite3StrBINARY: aqui o
        // az_coll do índice sem COLLATE explícito guarda exatamente STR_BINARY.
        let coll = if z_coll == STR_BINARY {
            None
        } else {
            locate_coll_seq(p_parse, &z_coll)
        };
        let mut k = p_key.borrow_mut();
        k.a_coll[i] = coll;
        k.a_sort_flags[i] = sort;
    }
    if p_parse.n_err != 0 {
        let mut i = p_idx.borrow_mut();
        if !i.b_no_query {
            // Desativa o índice porque contém uma sequência de comparação
            // desconhecida. A única forma de reativá-lo é recarregar o esquema:
            // registrar a sequência faltante depois não o reativa. O aplicativo
            // já teve a chance de registrá-la pelo callback collation-needed e,
            // por simplicidade, o SQLite não dá uma segunda chance.
            i.b_no_query = true;
            p_parse.rc = SQLITE_ERROR_RETRY;
        }
        drop(i);
        key_info_unref(Some(p_key));
        return None;
    }
    Some(p_key)
}

/// Cria um novo objeto Cte.
pub fn cte_new(
    p_parse: &mut Parse,
    p_name: &Token,
    p_arglist: Option<Box<ExprList>>,
    p_query: Option<Box<Select>>,
    e_m10d: u8,
) -> Option<Box<Cte>> {
    let db = p_parse.db.clone();
    if db.borrow().malloc_failed {
        expr_list_delete(&db, p_arglist);
        select_delete(&db, p_query);
        None
    } else {
        let mut p_new = Box::new(Cte::default());
        p_new.p_select = p_query;
        p_new.p_cols = p_arglist;
        p_new.z_name = name_from_token(&db, p_name);
        p_new.e_m10d = e_m10d;
        Some(p_new)
    }
}


// ---- part_014.rs ----

/// Limpa as informações de um objeto Cte sem liberar o próprio objeto.
fn cte_clear(db: &Sqlite3Ref, p_cte: &mut Cte) {
    expr_list_delete(db, p_cte.p_cols.take());
    select_delete(db, p_cte.p_select.take());
    p_cte.z_name = None;
}

/// Libera o conteúdo do objeto Cte passado como segundo argumento.
pub fn cte_delete(db: &Sqlite3Ref, mut p_cte: Box<Cte>) {
    cte_clear(db, &mut p_cte);
}

/// Chamada uma vez por CTE pelo parser ao interpretar uma cláusula WITH. O CTE
/// do terceiro argumento é adicionado ao WITH do segundo. Se o segundo for
/// None, um novo WITH é criado.
pub fn with_add(
    p_parse: &mut Parse,
    p_with: Option<Box<With>>,
    p_cte: Option<Box<Cte>>,
) -> Option<Box<With>> {
    let db = p_parse.db.clone();

    let p_cte = match p_cte {
        None => return p_with,
        Some(c) => c,
    };

    // Confere que o nome do CTE é único dentro desta cláusula WITH. Se não for,
    // grava um erro no Parse.
    if let (Some(z_name), Some(w)) = (p_cte.z_name.as_deref(), p_with.as_deref()) {
        for c in w.a.iter() {
            if str_i_cmp(z_name, c.z_name.as_deref().unwrap_or(b"")) == 0 {
                error_msg(p_parse, b"duplicate WITH table name: %s", &[PrintfArg::Bytes(z_name)]);
            }
        }
    }

    if db.borrow().malloc_failed {
        cte_delete(&db, p_cte);
        p_with
    } else {
        let mut p_new = match p_with {
            Some(w) => w,
            None => Box::new(With::default()),
        };
        p_new.a.push(*p_cte);
        p_new.n_cte = p_new.a.len() as i32;
        Some(p_new)
    }
}

/// Libera o conteúdo do objeto With passado como segundo argumento.
pub fn with_delete(db: &Sqlite3Ref, p_with: Option<Box<With>>) {
    if let Some(mut w) = p_with {
        for c in w.a.iter_mut() {
            cte_clear(db, c);
        }
    }
}

// No C o with_delete_generic só converte o void* em With*: em Rust a conversão
// some e o nome vira reexportação.
pub use with_delete as with_delete_generic;

