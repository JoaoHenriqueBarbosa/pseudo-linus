// Mesclado das partes traduzidas de alter_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Convenções deste arquivo (alter.c), para o integrador:
// - `ParseRef = Rc<RefCell<Parse>>` em toda função que recebe `Parse*`, porque o
//   Walker e o RenameCtx guardam o Parse e o devolvem a rotinas que o alteram.
// - `Parse.db` é `Weak<RefCell<Sqlite3>>` (`upgrade()` dá a conexão).
// - Funções variádicas do C (`sqlite3NestedParse`, `sqlite3ErrorMsg`, `sqlite3MPrintf`)
//   recebem o formato e um slice de `PrintfArg` (Text, Int, Null, Token).
// - Os nomes de função seguem o snake_case do crate `heck`: `sqlite3StrNICmp` vira
//   `str_ni_cmp`, `sqlite3GetVTable` vira `get_v_table`.
// - `sqlite3DbFree`/`sqlite3DbStrDup` somem: a liberação é o `drop` dos `Vec`.

/// Verifica se uma tabela pode ser alterada.
///
/// O parâmetro p_tab é a tabela que está prestes a ser alterada (seja com
/// ALTER TABLE ... RENAME TO ou ALTER TABLE ... ADD COLUMN). Se a tabela é uma
/// tabela do sistema, deixa uma mensagem de erro em p_parse.z_err_msg (tabelas
/// do sistema não podem ser alteradas) e devolve não-zero.
///
/// Ou, se a tabela não é uma tabela do sistema, devolve zero.
fn is_alterable_table(p_parse: &ParseRef, p_tab: &TableRef) -> i32 {
    let (z_name, tab_flags) = {
        let t = p_tab.borrow();
        (t.z_name.clone(), t.tab_flags)
    };
    let db = p_parse.borrow().db.upgrade();
    if 0 == str_ni_cmp(&z_name, b"sqlite_", 7)
        || (tab_flags & TF_EPONYMOUS) != 0
        || ((tab_flags & TF_SHADOW) != 0
            && db.as_ref().map_or(false, |d| read_only_shadow_tables(d)))
    {
        error_msg(
            p_parse,
            b"table %s may not be altered",
            &[PrintfArg::Text(&z_name)],
        );
        return 1;
    }
    0
}

/// Gera código para verificar que os esquemas do banco z_db e, se b_temp não é
/// verdadeiro, do banco "temp", ainda podem ser analisados. Isto é chamado ao fim
/// da geração de um comando ALTER TABLE ... RENAME ... para assegurar que a
/// operação não tornou nenhum objeto do esquema inutilizável.
fn rename_test_schema(
    p_parse: &ParseRef,
    z_db: &[u8],
    b_temp: i32,
    z_when: &[u8],
    b_no_dqs: i32,
) {
    p_parse.borrow_mut().col_names_set = 1;
    // LEGACY_SCHEMA_TABLE vale "sqlite_master".
    nested_parse(
        p_parse,
        concat!(
            "SELECT 1 ",
            "FROM \"%w\".sqlite_master ",
            "WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
            " AND sql NOT LIKE 'create virtual%%'",
            " AND sqlite_rename_test(%Q, sql, type, name, %d, %Q, %d)=NULL "
        )
        .as_bytes(),
        &[
            PrintfArg::Text(z_db),
            PrintfArg::Text(z_db),
            PrintfArg::Int(b_temp as i64),
            PrintfArg::Text(z_when),
            PrintfArg::Int(b_no_dqs as i64),
        ],
    );

    if b_temp == 0 {
        nested_parse(
            p_parse,
            concat!(
                "SELECT 1 ",
                "FROM temp.sqlite_master ",
                "WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
                " AND sql NOT LIKE 'create virtual%%'",
                " AND sqlite_rename_test(%Q, sql, type, name, 1, %Q, %d)=NULL "
            )
            .as_bytes(),
            &[
                PrintfArg::Text(z_db),
                PrintfArg::Text(z_when),
                PrintfArg::Int(b_no_dqs as i64),
            ],
        );
    }
}

/// Gera código de VM para substituir quaisquer strings entre aspas duplas (mas não
/// identificadores entre aspas duplas) dentro da coluna "sql" da tabela sqlite_schema
/// do banco z_db pelos equivalentes entre aspas simples. Se o argumento b_temp não é
/// verdadeiro, atualiza igualmente todas as instruções SQL da tabela sqlite_schema do
/// banco temp.
fn rename_fix_quotes(p_parse: &ParseRef, z_db: &[u8], b_temp: i32) {
    nested_parse(
        p_parse,
        concat!(
            "UPDATE \"%w\".sqlite_master",
            " SET sql = sqlite_rename_quotefix(%Q, sql)",
            "WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
            " AND sql NOT LIKE 'create virtual%%'"
        )
        .as_bytes(),
        &[PrintfArg::Text(z_db), PrintfArg::Text(z_db)],
    );
    if b_temp == 0 {
        nested_parse(
            p_parse,
            concat!(
                "UPDATE temp.sqlite_master",
                " SET sql = sqlite_rename_quotefix('temp', sql)",
                "WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
                " AND sql NOT LIKE 'create virtual%%'"
            )
            .as_bytes(),
            &[],
        );
    }
}

/// Gera código para recarregar o esquema do banco i_db. E, se i_db != 1, também o
/// do banco temp.
fn rename_reload_schema(p_parse: &ParseRef, i_db: i32, p5: u16) {
    let v = p_parse.borrow().p_vdbe.clone();
    if let Some(v) = v {
        change_cookie(p_parse, i_db);
        vdbe_add_parse_schema_op(&v, i_db, None, p5);
        if i_db != 1 {
            vdbe_add_parse_schema_op(&v, 1, None, p5);
        }
    }
}

/// Gera código para implementar o comando "ALTER TABLE xxx RENAME TO yyy".
pub fn alter_rename_table(p_parse: &ParseRef, p_src: Option<SrcListRef>, p_name: &Token) {
    let db = match p_parse.borrow().db.upgrade() {
        Some(db) => db,
        None => return,
    };
    let mut z_name: Option<Vec<u8>> = None; // versão terminada em NUL de p_name

    'exit_rename_table: {
        if never(db.borrow().malloc_failed != 0) {
            break 'exit_rename_table;
        }
        let p_src_ref = match &p_src {
            Some(s) => s.clone(),
            None => break 'exit_rename_table,
        };
        debug_assert!(p_src_ref.borrow().n_src == 1);
        debug_assert!(btree_holds_all_mutexes(&db));

        let p_tab = {
            let src = p_src_ref.borrow();
            locate_table_item(p_parse, 0, &src.a[0])
        };
        let p_tab = match p_tab {
            Some(t) => t,
            None => break 'exit_rename_table,
        };
        let i_db = schema_to_index(&db, &p_tab.borrow().p_schema);
        let z_db: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_sname.clone();

        // Obtém a versão terminada em NUL do novo nome da tabela.
        let name = match name_from_token(&db, p_name) {
            Some(n) => n,
            None => break 'exit_rename_table,
        };
        z_name = Some(name);
        let z_name_ref: &[u8] = z_name.as_ref().unwrap();

        // Verifica que não existe tabela ou índice chamado z_name no banco i_db.
        // Se existir, é um erro.
        if find_table(&db, z_name_ref, Some(&z_db)).is_some()
            || find_index(&db, z_name_ref, Some(&z_db)).is_some()
            || is_shadow_table_of(&db, &p_tab, z_name_ref)
        {
            error_msg(
                p_parse,
                b"there is already another table or index with this name: %s",
                &[PrintfArg::Text(z_name_ref)],
            );
            break 'exit_rename_table;
        }

        // Garante que não é uma tabela do sistema sendo alterada, nem um nome
        // reservado para o qual a tabela está sendo renomeada.
        if SQLITE_OK != is_alterable_table(p_parse, &p_tab) {
            break 'exit_rename_table;
        }
        if SQLITE_OK != check_object_name(p_parse, z_name_ref, b"table", z_name_ref) {
            break 'exit_rename_table;
        }

        if is_view(&p_tab.borrow()) {
            let tn = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"view %s may not be altered",
                &[PrintfArg::Text(&tn)],
            );
            break 'exit_rename_table;
        }

        // Chama o callback de autorização.
        {
            let tn = p_tab.borrow().z_name.clone();
            if auth_check(p_parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&tn), None) != 0 {
                break 'exit_rename_table;
            }
        }

        let mut p_v_tab: Option<VTableRef> = None; // não-nulo se for v-tab com xRename()
        if view_get_column_names(p_parse, &p_tab) != 0 {
            break 'exit_rename_table;
        }
        if is_virtual(&p_tab.borrow()) {
            let vt = get_v_table(&db, &p_tab);
            if vt.borrow().p_vtab.p_module.x_rename.is_some() {
                p_v_tab = Some(vt);
            }
        }

        // Inicia uma transação para o banco i_db. Depois modifica o cookie do
        // esquema (já que o ALTER TABLE modifica o esquema). Chama may_abort(),
        // pois as funções escalares (por exemplo sqlite_rename_table()) chamadas
        // pelo SQL aninhado podem levantar uma exceção.
        let v = match get_vdbe(p_parse) {
            Some(v) => v,
            None => break 'exit_rename_table,
        };
        may_abort(p_parse);

        // Descobre quantos caracteres UTF-8 há em z_name.
        let z_tab_name: Vec<u8> = p_tab.borrow().z_name.clone();
        let n_tab_name = utf8_char_len(&z_tab_name, -1);

        // Reescreve todas as instruções CREATE TABLE, INDEX, TRIGGER ou VIEW do
        // esquema para usar o novo nome da tabela.
        nested_parse(
            p_parse,
            concat!(
                "UPDATE \"%w\".sqlite_master SET ",
                "sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, %d) ",
                "WHERE (type!='index' OR tbl_name=%Q COLLATE nocase)",
                "AND   name NOT LIKE 'sqliteX_%%' ESCAPE 'X'"
            )
            .as_bytes(),
            &[
                PrintfArg::Text(&z_db),
                PrintfArg::Text(&z_db),
                PrintfArg::Text(&z_tab_name),
                PrintfArg::Text(z_name_ref),
                PrintfArg::Int((i_db == 1) as i64),
                PrintfArg::Text(&z_tab_name),
            ],
        );

        // Atualiza as colunas tbl_name e name da tabela sqlite_schema conforme o
        // necessário.
        nested_parse(
            p_parse,
            concat!(
                "UPDATE %Q.sqlite_master SET ",
                "tbl_name = %Q, ",
                "name = CASE ",
                "WHEN type='table' THEN %Q ",
                "WHEN name LIKE 'sqliteX_autoindex%%' ESCAPE 'X' ",
                "     AND type='index' THEN ",
                "'sqlite_autoindex_' || %Q || substr(name,%d+18) ",
                "ELSE name END ",
                "WHERE tbl_name=%Q COLLATE nocase AND ",
                "(type='table' OR type='index' OR type='trigger');"
            )
            .as_bytes(),
            &[
                PrintfArg::Text(&z_db),
                PrintfArg::Text(z_name_ref),
                PrintfArg::Text(z_name_ref),
                PrintfArg::Text(z_name_ref),
                PrintfArg::Int(n_tab_name as i64),
                PrintfArg::Text(&z_tab_name),
            ],
        );

        // Se a tabela sqlite_sequence existe neste banco, atualiza-a com o novo
        // nome da tabela.
        if find_table(&db, b"sqlite_sequence", Some(&z_db)).is_some() {
            let tn = p_tab.borrow().z_name.clone();
            nested_parse(
                p_parse,
                b"UPDATE \"%w\".sqlite_sequence set name = %Q WHERE name = %Q",
                &[
                    PrintfArg::Text(&z_db),
                    PrintfArg::Text(z_name_ref),
                    PrintfArg::Text(&tn),
                ],
            );
        }

        // Se a tabela renomeada não faz parte do banco temp, edita as definições
        // de views e triggers dentro do banco temp conforme o necessário.
        if i_db != 1 {
            nested_parse(
                p_parse,
                concat!(
                    "UPDATE sqlite_temp_schema SET ",
                    "sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, 1), ",
                    "tbl_name = ",
                    "CASE WHEN tbl_name=%Q COLLATE nocase AND ",
                    "  sqlite_rename_test(%Q, sql, type, name, 1, 'after rename', 0) ",
                    "THEN %Q ELSE tbl_name END ",
                    "WHERE type IN ('view', 'trigger')"
                )
                .as_bytes(),
                &[
                    PrintfArg::Text(&z_db),
                    PrintfArg::Text(&z_tab_name),
                    PrintfArg::Text(z_name_ref),
                    PrintfArg::Text(&z_tab_name),
                    PrintfArg::Text(&z_db),
                    PrintfArg::Text(z_name_ref),
                ],
            );
        }

        // Se for uma tabela virtual, chama xRename() se estiver definido. O
        // callback xRename() modifica os nomes de quaisquer recursos usados pela
        // implementação da v-table (inclusive outras tabelas do SQLite) que são
        // identificados pelo nome da tabela virtual.
        if let Some(vt) = p_v_tab {
            let i = {
                let mut p = p_parse.borrow_mut();
                p.n_mem += 1;
                p.n_mem
            };
            vdbe_load_string(&v, i, z_name_ref);
            vdbe_add_op4(&v, OP_VRENAME, i, 0, 0, P4Arg::VTab(vt), P4_VTAB);
        }

        rename_reload_schema(p_parse, i_db, INITFLAG_ALTERRENAME as u16);
        rename_test_schema(p_parse, &z_db, (i_db == 1) as i32, b"after rename", 0);
    }

    // exit_rename_table:
    src_list_delete(&db, p_src);
    drop(z_name);
}

/// Escreve código que vai levantar um erro se a tabela descrita por z_db e z_tab
/// não estiver vazia.
fn error_if_not_empty(p_parse: &ParseRef, z_db: &[u8], z_tab: &[u8], z_err: &[u8]) {
    nested_parse(
        p_parse,
        b"SELECT raise(ABORT,%Q) FROM \"%w\".\"%w\"",
        &[
            PrintfArg::Text(z_err),
            PrintfArg::Text(z_db),
            PrintfArg::Text(z_tab),
        ],
    );
}

/// Esta função é chamada depois que uma instrução "ALTER TABLE ... ADD" foi
/// analisada. O argumento p_col_def contém o texto da nova definição de coluna.
///
/// A estrutura Table p_parse.p_new_table foi estendida para incluir a nova coluna
/// durante a análise.
pub fn alter_finish_add_column(p_parse: &ParseRef, p_col_def: &Token) {
    let db = match p_parse.borrow().db.upgrade() {
        Some(d) => d,
        None => return,
    };
    debug_assert!(db
        .borrow()
        .p_parse
        .as_ref()
        .map_or(false, |p| Rc::ptr_eq(p, p_parse)));
    if p_parse.borrow().n_err != 0 {
        return;
    }
    debug_assert!(db.borrow().malloc_failed == 0);
    let p_new = p_parse.borrow().p_new_table.clone();
    let p_new = p_new.expect("p_new_table");

    debug_assert!(btree_holds_all_mutexes(&db));
    let i_db = schema_to_index(&db, &p_new.borrow().p_schema);
    let z_db: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_sname.clone();
    // Pula o prefixo "sqlite_altertab_" no nome.
    let z_tab: Vec<u8> = p_new.borrow().z_name[16..].to_vec();
    let (col_flags, col_not_null, mut p_dflt) = {
        let n = p_new.borrow();
        let p_col = &n.a_col[n.n_col as usize - 1];
        (p_col.col_flags, p_col.not_null, column_expr(&n, p_col))
    };
    let p_tab = find_table(&db, &z_tab, Some(&z_db));
    let p_tab = p_tab.expect("tabela alterada");

    // Chama o callback de autorização.
    {
        let tn = p_tab.borrow().z_name.clone();
        if auth_check(p_parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&tn), None) != 0 {
            return;
        }
    }

    // Verifica que a nova coluna não foi especificada como PRIMARY KEY ou UNIQUE.
    // Se há restrição NOT NULL, o valor padrão da coluna não pode ser NULL.
    if (col_flags & COLFLAG_PRIMKEY) != 0 {
        error_msg(p_parse, b"Cannot add a PRIMARY KEY column", &[]);
        return;
    }
    if p_new.borrow().p_index.is_some() {
        error_msg(p_parse, b"Cannot add a UNIQUE column", &[]);
        return;
    }
    if (col_flags & COLFLAG_GENERATED) == 0 {
        // Se o valor padrão da nova coluna foi especificado com um NULL literal,
        // faz p_dflt valer 0. Isso simplifica a verificação de padrão NULL do SQL
        // mais abaixo.
        debug_assert!(p_dflt.as_ref().map_or(true, |d| d.op == TK_SPAN));
        let dflt_is_null = p_dflt
            .as_ref()
            .map_or(false, |d| d.p_left.as_ref().map_or(false, |l| l.op == TK_NULL));
        if dflt_is_null {
            p_dflt = None;
        }
        debug_assert!(is_ordinary_table(&p_new.borrow()));
        if (db.borrow().flags & SQLITE_FOREIGNKEYS) != 0
            && p_new.borrow().u.tab.p_f_key.is_some()
            && p_dflt.is_some()
        {
            error_if_not_empty(
                p_parse,
                &z_db,
                &z_tab,
                b"Cannot add a REFERENCES column with non-NULL default value",
            );
        }
        if col_not_null != 0 && p_dflt.is_none() {
            error_if_not_empty(
                p_parse,
                &z_db,
                &z_tab,
                b"Cannot add a NOT NULL column with default value NULL",
            );
        }

        // Garante que a expressão padrão é algo que value_from_expr() sabe tratar
        // (isto é, não CURRENT_TIME etc.)
        if let Some(dflt) = &p_dflt {
            let (rc, p_val) = value_from_expr(&db, dflt, SQLITE_UTF8, SQLITE_AFF_BLOB);
            debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
            if rc != SQLITE_OK {
                debug_assert!(db.borrow().malloc_failed == 1);
                return;
            }
            if p_val.is_none() {
                error_if_not_empty(
                    p_parse,
                    &z_db,
                    &z_tab,
                    b"Cannot add a column with non-constant default",
                );
            }
            value_free(p_val);
        }
    } else if (col_flags & COLFLAG_STORED) != 0 {
        error_if_not_empty(p_parse, &z_db, &z_tab, b"cannot add a STORED column");
    }

    // Modifica a instrução CREATE TABLE.
    let mut z_col: Vec<u8> = p_col_def.z[..p_col_def.n as usize].to_vec();
    {
        // Em C, z_end aponta para o último byte e cada byte removido vira NUL; o
        // texto efetivo termina no primeiro NUL, daí o truncate.
        let mut z_end = (p_col_def.n as usize).wrapping_sub(1);
        while z_end > 0 && (z_col[z_end] == b';' || is_space(z_col[z_end])) {
            z_end -= 1;
        }
        z_col.truncate(z_end + 1);
        // substr() opera em caracteres, mas addColOffset é em bytes. Então é
        // preciso usar printf() para traduzir entre essas unidades:
        debug_assert!(is_ordinary_table(&p_tab.borrow()));
        debug_assert!(is_ordinary_table(&p_new.borrow()));
        let add_col_offset = p_new.borrow().u.tab.add_col_offset as i64;
        nested_parse(
            p_parse,
            concat!(
                "UPDATE \"%w\".sqlite_master SET ",
                "sql = printf('%%.%ds, ',sql) || %Q",
                " || substr(sql,1+length(printf('%%.%ds',sql))) ",
                "WHERE type = 'table' AND name = %Q"
            )
            .as_bytes(),
            &[
                PrintfArg::Text(&z_db),
                PrintfArg::Int(add_col_offset),
                PrintfArg::Text(&z_col),
                PrintfArg::Int(add_col_offset),
                PrintfArg::Text(&z_tab),
            ],
        );
    }

    if let Some(v) = get_vdbe(p_parse) {
        // Garante que a versão do esquema é pelo menos 3. Mas não promove de menos
        // que 3 para 4, pois isso corromperia qualquer índice DESC preexistente.
        let r1 = get_temp_reg(p_parse);
        vdbe_add_op3(&v, OP_READCOOKIE, i_db, r1, BTREE_FILE_FORMAT as i32);
        vdbe_uses_btree(&v, i_db);
        vdbe_add_op2(&v, OP_ADDIMM, r1, -2);
        let addr = vdbe_current_addr(&v) + 2;
        vdbe_add_op2(&v, OP_IFPOS, r1, addr);
        vdbe_add_op3(&v, OP_SETCOOKIE, i_db, BTREE_FILE_FORMAT as i32, 3);
        release_temp_reg(p_parse, r1);

        // Recarrega a definição da tabela
        rename_reload_schema(p_parse, i_db, INITFLAG_ALTERADD as u16);

        // Verifica que as restrições continuam satisfeitas
        if p_new.borrow().p_check.is_some()
            || (col_not_null != 0 && (col_flags & COLFLAG_GENERATED) != 0)
            || (p_tab.borrow().tab_flags & TF_STRICT) != 0
        {
            nested_parse(
                p_parse,
                concat!(
                    "SELECT CASE WHEN quick_check GLOB 'CHECK*'",
                    " THEN raise(ABORT,'CHECK constraint failed')",
                    " WHEN quick_check GLOB 'non-* value in*'",
                    " THEN raise(ABORT,'type mismatch on DEFAULT')",
                    " ELSE raise(ABORT,'NOT NULL constraint failed')",
                    " END",
                    "  FROM pragma_quick_check(%Q,%Q)",
                    " WHERE quick_check GLOB 'CHECK*'",
                    " OR quick_check GLOB 'NULL*'",
                    " OR quick_check GLOB 'non-* value in*'"
                )
                .as_bytes(),
                &[PrintfArg::Text(&z_tab), PrintfArg::Text(&z_db)],
            );
        }
    }
}


// ---- part_001.rs ----

/// Esta função é chamada pelo analisador depois que o nome da tabela em uma
/// instrução "ALTER TABLE <nome> ADD" é analisado. O argumento p_src é o nome
/// completo da tabela que está sendo alterada.
///
/// A rotina faz uma cópia (parcial) da estrutura Table da tabela alterada e faz
/// Parse.p_new_table apontar para ela. As rotinas chamadas pelo analisador enquanto
/// a definição da coluna é lida (isto é, add_column()) acrescentam os novos dados
/// de Column à cópia. A cópia da estrutura Table é apagada por tokenize.c depois que
/// a análise termina.
///
/// A rotina alter_finish_add_column() será chamada para completar a geração de
/// código da instrução "ALTER TABLE ... ADD".
pub fn alter_begin_add_column(p_parse: &ParseRef, p_src: Option<SrcListRef>) {
    let db = match p_parse.borrow().db.upgrade() {
        Some(d) => d,
        None => return,
    };

    'exit_begin_add_column: {
        // Procura a tabela que está sendo alterada.
        debug_assert!(p_parse.borrow().p_new_table.is_none());
        debug_assert!(btree_holds_all_mutexes(&db));
        if db.borrow().malloc_failed != 0 {
            break 'exit_begin_add_column;
        }
        let p_src_ref = match &p_src {
            Some(s) => s.clone(),
            None => break 'exit_begin_add_column,
        };
        let p_tab = {
            let src = p_src_ref.borrow();
            locate_table_item(p_parse, 0, &src.a[0])
        };
        let p_tab = match p_tab {
            Some(t) => t,
            None => break 'exit_begin_add_column,
        };

        if is_virtual(&p_tab.borrow()) {
            error_msg(p_parse, b"virtual tables may not be altered", &[]);
            break 'exit_begin_add_column;
        }

        // Garante que não é uma tentativa de ALTER em uma view.
        if is_view(&p_tab.borrow()) {
            error_msg(p_parse, b"Cannot add a column to a view", &[]);
            break 'exit_begin_add_column;
        }
        if SQLITE_OK != is_alterable_table(p_parse, &p_tab) {
            break 'exit_begin_add_column;
        }

        may_abort(p_parse);
        debug_assert!(is_ordinary_table(&p_tab.borrow()));
        debug_assert!(p_tab.borrow().u.tab.add_col_offset > 0);
        let i_db = schema_to_index(&db, &p_tab.borrow().p_schema);

        // Coloca uma cópia da struct Table em Parse.p_new_table para add_column() e
        // afins modificarem. Mas modifica o nome acrescentando o prefixo
        // "sqlite_altertab_". Com esse prefixo, o nome não colide com uma tabela
        // existente, pois tabelas de usuário não podem ter o prefixo "sqlite_".
        let mut p_new = Table::default();
        p_new.n_tab_ref = 1;
        {
            let t = p_tab.borrow();
            p_new.n_col = t.n_col;
            debug_assert!(p_new.n_col > 0);
            // O C aloca nAlloc = (((nCol-1)/8)*8)+8 colunas zeradas; o Vec cresce
            // sozinho em add_column(), então guarda só as n_col copiadas.
            p_new.z_name = m_printf(&db, b"sqlite_altertab_%s", &[PrintfArg::Text(&t.z_name)])
                .expect("sqlite_altertab_");
            p_new.a_col = t.a_col[..p_new.n_col as usize].to_vec();
            // O to_vec() acima já duplicou z_cn_name (sqlite3DbStrDup); falta o hash.
            for p_col in p_new.a_col.iter_mut() {
                p_col.h_name = str_i_hash(&p_col.z_cn_name);
            }
            debug_assert!(is_ordinary_table(&p_new));
            p_new.u.tab.p_dflt_list = expr_list_dup(&db, t.u.tab.p_dflt_list.as_deref(), 0);
            p_new.p_schema = db.borrow().a_db[i_db as usize].p_schema.clone();
            p_new.u.tab.add_col_offset = t.u.tab.add_col_offset;
        }
        debug_assert!(p_new.n_tab_ref == 1);
        p_parse.borrow_mut().p_new_table = Some(Rc::new(RefCell::new(p_new)));
    }

    // exit_begin_add_column:
    src_list_delete(&db, p_src);
}

/// O parâmetro p_tab é o objeto de um comando ALTER TABLE ... RENAME COLUMN. Esta
/// função verifica se a tabela é uma view ou tabela virtual (colunas de views ou
/// de tabelas virtuais não podem ser renomeadas). Se for, carrega uma mensagem de
/// erro em p_parse e devolve não-zero.
///
/// Ou, se p_tab não é view nem tabela virtual, devolve zero.
fn is_real_table(p_parse: &ParseRef, p_tab: &TableRef, b_drop: i32) -> i32 {
    let mut z_type: Option<&[u8]> = None;
    if is_view(&p_tab.borrow()) {
        z_type = Some(b"view");
    }
    if is_virtual(&p_tab.borrow()) {
        z_type = Some(b"virtual table");
    }
    if let Some(z_type) = z_type {
        let tn = p_tab.borrow().z_name.clone();
        error_msg(
            p_parse,
            b"cannot %s %s \"%s\"",
            &[
                PrintfArg::Text(if b_drop != 0 {
                    b"drop column from"
                } else {
                    b"rename columns of"
                }),
                PrintfArg::Text(z_type),
                PrintfArg::Text(&tn),
            ],
        );
        return 1;
    }
    0
}

/// Trata a seguinte redução do analisador:
///
///  cmd ::= ALTER TABLE pSrc RENAME COLUMN pOld TO pNew
pub fn alter_rename_column(
    p_parse: &ParseRef,
    p_src: Option<SrcListRef>, // Tabela alterada. p_src.n_src==1
    p_old: &Token,             // Nome da coluna que muda
    p_new: &Token,             // Novo nome da coluna
) {
    let db = match p_parse.borrow().db.upgrade() {
        Some(d) => d,
        None => return,
    };

    'exit_rename_column: {
        // Localiza a tabela a ser alterada
        let p_src_ref = match &p_src {
            Some(s) => s.clone(),
            None => break 'exit_rename_column,
        };
        let p_tab = {
            let src = p_src_ref.borrow();
            locate_table_item(p_parse, 0, &src.a[0])
        };
        let p_tab = match p_tab {
            Some(t) => t,
            None => break 'exit_rename_column,
        };

        // Não se pode alterar uma tabela do sistema
        if SQLITE_OK != is_alterable_table(p_parse, &p_tab) {
            break 'exit_rename_column;
        }
        if SQLITE_OK != is_real_table(p_parse, &p_tab, 0) {
            break 'exit_rename_column;
        }

        // Qual esquema guarda a tabela alterada
        let i_schema = schema_to_index(&db, &p_tab.borrow().p_schema);
        debug_assert!(i_schema >= 0);
        let z_db: Vec<u8> = db.borrow().a_db[i_schema as usize].z_db_sname.clone();
        let z_tab_name: Vec<u8> = p_tab.borrow().z_name.clone();

        // Chama o callback de autorização.
        if auth_check(p_parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&z_tab_name), None) != 0 {
            break 'exit_rename_column;
        }

        // Garante que o nome antigo é mesmo o nome de uma coluna da tabela
        // alterada. Faz i_col ser o índice da coluna renomeada
        let z_old = match name_from_token(&db, p_old) {
            Some(z) => z,
            None => break 'exit_rename_column,
        };
        let n_col = p_tab.borrow().n_col as usize;
        let mut i_col = 0usize;
        while i_col < n_col {
            if 0 == str_i_cmp(&p_tab.borrow().a_col[i_col].z_cn_name, &z_old) {
                break;
            }
            i_col += 1;
        }
        if i_col == n_col {
            error_msg(
                p_parse,
                b"no such column: \"%T\"",
                &[PrintfArg::Token(p_old)],
            );
            break 'exit_rename_column;
        }

        // Garante que o esquema não contém strings entre aspas duplas
        rename_test_schema(p_parse, &z_db, (i_schema == 1) as i32, b"", 0);
        rename_fix_quotes(p_parse, &z_db, (i_schema == 1) as i32);

        // Faz a renomeação usando um UPDATE recursivo que usa a função SQL
        // sqlite_rename_column() para calcular o novo texto do CREATE da tabela
        // sqlite_schema.
        may_abort(p_parse);
        let z_new = match name_from_token(&db, p_new) {
            Some(z) => z,
            None => break 'exit_rename_column,
        };
        debug_assert!(p_new.n > 0);
        let b_quote = is_quote(p_new.z[0]) as i64;
        nested_parse(
            p_parse,
            concat!(
                "UPDATE \"%w\".sqlite_master SET ",
                "sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, %d) ",
                "WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' ",
                " AND (type != 'index' OR tbl_name = %Q)"
            )
            .as_bytes(),
            &[
                PrintfArg::Text(&z_db),
                PrintfArg::Text(&z_db),
                PrintfArg::Text(&z_tab_name),
                PrintfArg::Int(i_col as i64),
                PrintfArg::Text(&z_new),
                PrintfArg::Int(b_quote),
                PrintfArg::Int((i_schema == 1) as i64),
                PrintfArg::Text(&z_tab_name),
            ],
        );

        nested_parse(
            p_parse,
            concat!(
                "UPDATE temp.sqlite_master SET ",
                "sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, 1) ",
                "WHERE type IN ('trigger', 'view')"
            )
            .as_bytes(),
            &[
                PrintfArg::Text(&z_db),
                PrintfArg::Text(&z_tab_name),
                PrintfArg::Int(i_col as i64),
                PrintfArg::Text(&z_new),
                PrintfArg::Int(b_quote),
            ],
        );

        // Apaga e recarrega o esquema do banco de dados.
        rename_reload_schema(p_parse, i_schema, INITFLAG_ALTERRENAME as u16);
        rename_test_schema(p_parse, &z_db, (i_schema == 1) as i32, b"after rename", 1);
    }

    // exit_rename_column:
    src_list_delete(&db, p_src);
}

/// Chave de identidade de um elemento da árvore de análise. No C, RenameToken.p é
/// o endereço do elemento (Expr, Column.zCnName, IdList item etc.) e só é usado
/// para comparar igualdade. Aqui o endereço vira `usize` (conversão segura, sem
/// desreferenciar). O chamador precisa passar uma referência estável ao mesmo
/// elemento na marcação e na busca (para texto, `&v[..]` do `Vec<u8>` dono). O valor
/// 0 representa o ponteiro nulo.
pub fn rename_ptr_key<T: ?Sized>(r: &T) -> usize {
    r as *const T as *const u8 as usize
}

/// Posição do token no SQL de entrada (equivale a `pBest->t.z - zSql` do C).
#[inline]
pub fn rename_token_offset(p_token: &RenameToken) -> usize {
    p_token.t.off
}

/// Cada objeto RenameToken mapeia um elemento da árvore de análise para o token que
/// gerou esse elemento. O elemento pode ser:
///
///     *  Um ponteiro para um Expr que representa um ID
///     *  O nome de uma coluna de tabela em Column.zName
///
/// Uma lista de objetos RenameToken pode ser construída durante a análise. Cada
/// objeto novo é criado por rename_token_map(). Conforme a árvore é transformada,
/// rename_token_remap() mantém o mapeamento atualizado.
///
/// Depois que a análise termina, rename_token_find() pode ser usada para achar o
/// valor real do token que criou algum elemento da árvore.
#[derive(Clone, Default)]
pub struct RenameToken {
    /// Chave do elemento da árvore criado pelo token t (0 é nulo)
    pub p: usize,
    /// O token que criou o elemento p
    pub t: Token,
    /// Próximo da lista de todos os objetos RenameToken
    pub p_next: Option<Box<RenameToken>>,
}

/// O contexto de uma operação ALTER TABLE RENAME COLUMN que desce para o Walker.
#[derive(Default)]
pub struct RenameCtx {
    /// Lista de tokens a sobrescrever
    pub p_list: Option<Box<RenameToken>>,
    /// Número de tokens em p_list
    pub n_list: i32,
    /// Índice da coluna renomeada
    pub i_col: i32,
    /// Tabela sendo alterada
    pub p_tab: Option<TableRef>,
    /// Nome antigo da coluna
    pub z_old: Option<Vec<u8>>,
}

/// Lembra que o elemento p_ptr da árvore de análise foi criado usando o token
/// p_token.
///
/// Em outras palavras, constrói um novo objeto RenameToken e o acrescenta à lista
/// de objetos RenameToken que está sendo montada em p_parse.p_rename.
///
/// O argumento p_ptr é devolvido para que esta rotina possa ser usada com recursão
/// de cauda em token_expr(), para um pequeno ganho de desempenho.
pub fn rename_token_map(p_parse: &ParseRef, p_ptr: usize, p_token: &Token) -> usize {
    // renameTokenCheckAll só existe sob SQLITE_DEBUG.
    let mut parse = p_parse.borrow_mut();
    if always(parse.e_parse_mode != PARSE_MODE_UNMAP) {
        let p_new = RenameToken {
            p: p_ptr,
            t: p_token.clone(),
            p_next: parse.p_rename.take(),
        };
        parse.p_rename = Some(Box::new(p_new));
    }
    p_ptr
}

/// Presume-se que já existe um objeto RenameToken associado ao elemento p_from da
/// árvore de análise. Esta função remapeia o token associado para o elemento p_to.
pub fn rename_token_remap(p_parse: &ParseRef, p_to: usize, p_from: usize) {
    let mut parse = p_parse.borrow_mut();
    let mut p = parse.p_rename.as_deref_mut();
    while let Some(tok) = p {
        if tok.p == p_from {
            tok.p = p_to;
            break;
        }
        p = tok.p_next.as_deref_mut();
    }
}

/// Callback do Walker usado por rename_expr_unmap().
fn rename_unmap_expr_cb(p_walker: &mut Walker, p_expr: &Expr) -> i32 {
    let p_parse = p_walker.p_parse.clone().expect("Walker.p_parse");
    rename_token_remap(&p_parse, 0, rename_ptr_key(p_expr));
    if expr_use_y_tab(p_expr) {
        rename_token_remap(&p_parse, 0, rename_ptr_key(&p_expr.y.p_tab));
    }
    WRC_CONTINUE
}


// ---- part_002.rs ----

/// Itera pelos objetos Select que fazem parte de cláusulas WITH anexadas à
/// instrução Select p_select.
fn rename_walk_with(p_walker: &mut Walker, p_select: &SelectRef) {
    let p_with = p_select.borrow().p_with.clone();
    if let Some(p_with) = p_with {
        let p_parse = p_walker.p_parse.clone().expect("Walker.p_parse");
        let db = p_parse.borrow().db.upgrade().expect("Parse.db");
        let mut p_copy: Option<WithRef> = None;
        let n_cte = p_with.borrow().n_cte as usize;
        debug_assert!(n_cte > 0);
        let first_flags = {
            let w = p_with.borrow();
            let first = w.a[0].p_select.as_ref().expect("p_select");
            let f = first.borrow().sel_flags;
            f
        };
        if (first_flags & SF_EXPANDED) == 0 {
            // Empurra uma cópia do objeto With na pilha with. Usamos uma cópia aqui,
            // pois o original será expandido e resolvido (flags SF_Expanded e
            // SF_Resolved) abaixo. E o código do analisador que usa a pilha with
            // falha se os objetos Select nela já foram expandidos e resolvidos.
            p_copy = with_dup(&db, &p_with);
            p_copy = with_push(&p_parse, p_copy, 1);
        }
        for i in 0..n_cte {
            let p = p_with.borrow().a[i].p_select.clone().expect("p_select");
            let mut s_nc = NameContext::default();
            s_nc.p_parse = Some(p_parse.clone());
            if p_copy.is_some() {
                select_prep(&p_parse, &p, Some(&mut s_nc));
            }
            if db.borrow().malloc_failed != 0 {
                return;
            }
            walk_select(p_walker, Some(&p));
            let w = p_with.borrow();
            rename_exprlist_unmap(&p_parse, w.a[i].p_cols.as_deref());
        }
        if let Some(copy) = &p_copy {
            let same = p_parse
                .borrow()
                .p_with
                .as_ref()
                .map_or(false, |w| Rc::ptr_eq(w, copy));
            if same {
                let outer = copy.borrow().p_outer.clone();
                p_parse.borrow_mut().p_with = outer;
            }
        }
    }
}

/// Remove o mapeamento de todos os tokens do objeto IdList passado como segundo
/// argumento.
fn unmap_column_idlist_names(p_parse: &ParseRef, p_id_list: &IdList) {
    for ii in 0..p_id_list.n_id as usize {
        rename_token_remap(p_parse, 0, rename_ptr_key(&p_id_list.a[ii].z_name[..]));
    }
}

/// Callback do Walker usado por rename_expr_unmap().
fn rename_unmap_select_cb(p_walker: &mut Walker, p: &SelectRef) -> i32 {
    let p_parse = p_walker.p_parse.clone().expect("Walker.p_parse");
    if p_parse.borrow().n_err != 0 {
        return WRC_ABORT;
    }
    // testcase( p->selFlags & SF_View ) e SF_CopyCte: só cobertura, some.
    if (p.borrow().sel_flags & (SF_VIEW | SF_COPYCTE)) != 0 {
        return WRC_PRUNE;
    }
    {
        let s = p.borrow();
        if always(s.p_e_list.is_some()) {
            let p_list = s.p_e_list.as_ref().unwrap();
            for i in 0..p_list.n_expr as usize {
                if let Some(z) = p_list.a[i].z_e_name.as_deref() {
                    if p_list.a[i].fg.e_e_name == ENAME_NAME {
                        rename_token_remap(&p_parse, 0, rename_ptr_key(z));
                    }
                }
            }
        }
    }
    {
        let s = p.borrow();
        // Todo Select tem um SrcList, mesmo que vazio
        if always(s.p_src.is_some()) {
            let p_src = s.p_src.as_ref().unwrap();
            for i in 0..p_src.n_src as usize {
                let item = &p_src.a[i];
                let key = item.z_name.as_deref().map_or(0, |z| rename_ptr_key(z));
                rename_token_remap(&p_parse, 0, key);
                if item.fg.is_using == 0 {
                    if let SrcItemU3::On(p_on) = &item.u3 {
                        walk_expr(p_walker, p_on.as_deref());
                    }
                } else if let SrcItemU3::Using(p_using) = &item.u3 {
                    if let Some(p_using) = p_using.as_deref() {
                        unmap_column_idlist_names(&p_parse, p_using);
                    }
                }
            }
        }
    }

    rename_walk_with(p_walker, p);
    WRC_CONTINUE
}

/// Remove todos os nós que fazem parte da expressão p_expr da lista de renomeação.
pub fn rename_expr_unmap(p_parse: &ParseRef, p_expr: Option<&Expr>) {
    let e_mode = p_parse.borrow().e_parse_mode;
    let mut s_walker = Walker::default();
    s_walker.p_parse = Some(p_parse.clone());
    s_walker.x_expr_callback = Some(rename_unmap_expr_cb);
    s_walker.x_select_callback = Some(rename_unmap_select_cb);
    p_parse.borrow_mut().e_parse_mode = PARSE_MODE_UNMAP;
    walk_expr(&mut s_walker, p_expr);
    p_parse.borrow_mut().e_parse_mode = e_mode;
}

/// Remove todos os nós que fazem parte da lista de expressões p_e_list da lista de
/// renomeação.
pub fn rename_exprlist_unmap(p_parse: &ParseRef, p_e_list: Option<&ExprList>) {
    if let Some(p_list) = p_e_list {
        let mut s_walker = Walker::default();
        s_walker.p_parse = Some(p_parse.clone());
        s_walker.x_expr_callback = Some(rename_unmap_expr_cb);
        walk_expr_list(&mut s_walker, Some(p_list));
        for i in 0..p_list.n_expr as usize {
            if always(p_list.a[i].fg.e_e_name == ENAME_NAME) {
                let key = p_list.a[i].z_e_name.as_deref().map_or(0, |z| rename_ptr_key(z));
                rename_token_remap(p_parse, 0, key);
            }
        }
    }
}

/// Libera a lista de objetos RenameToken dada no segundo argumento.
fn rename_token_free(_db: &SqliteRef, p_token: Option<Box<RenameToken>>) {
    // Percorre soltando um a um, para a lista longa não estourar a pilha no Drop.
    let mut p = p_token;
    while let Some(mut t) = p {
        p = t.p_next.take();
    }
}

/// Procura no objeto Parse passado como primeiro argumento um objeto RenameToken
/// associado ao elemento p_ptr da árvore de análise. Se achar, devolve true (o C
/// devolve o ponteiro; nenhum chamador de alter.c usa o valor). Senão, devolve false.
///
/// Se o segundo argumento não é None e um RenameToken correspondente é achado,
/// ele é removido do objeto Parse e acrescentado à lista mantida pelo RenameCtx.
fn rename_token_find(p_parse: &ParseRef, p_ctx: Option<&mut RenameCtx>, p_ptr: usize) -> bool {
    if never(p_ptr == 0) {
        return false;
    }
    let mut parse = p_parse.borrow_mut();
    let mut pp = &mut parse.p_rename;
    while pp.as_ref().map_or(false, |t| t.p != p_ptr) {
        pp = &mut pp.as_mut().unwrap().p_next;
    }
    if pp.is_none() {
        return false;
    }
    if let Some(ctx) = p_ctx {
        let mut p_token = pp.take().unwrap();
        *pp = p_token.p_next.take();
        p_token.p_next = ctx.p_list.take();
        ctx.p_list = Some(p_token);
        ctx.n_list += 1;
    }
    true
}

/// Este é um callback Select do Walker. Não faz nada de útil. Só é necessário
/// porque, sem um callback fictício, walk_expr() e afins não descem em
/// sub-instruções select.
fn rename_column_select_cb(p_walker: &mut Walker, p: &SelectRef) -> i32 {
    if (p.borrow().sel_flags & (SF_VIEW | SF_COPYCTE)) != 0 {
        // testcase( p->selFlags & SF_View ) e SF_CopyCte: só cobertura, some.
        return WRC_PRUNE;
    }
    rename_walk_with(p_walker, p);
    WRC_CONTINUE
}

/// Este é um callback de expressão do Walker.
///
/// Para cada nó TK_COLUMN da árvore de expressão, procura se a coluna referenciada
/// é a coluna renomeada por um ALTER TABLE. Se for, anexa o RenameToken associado
/// à lista de RenameToken que está sendo construída no RenameCtx em
/// p_walker.u.p_rename.
fn rename_column_expr_cb(p_walker: &mut Walker, p_expr: &Expr) -> i32 {
    let ctx_rc = match &p_walker.u {
        WalkerU::Rename(c) => c.clone(),
        _ => return WRC_CONTINUE,
    };
    let p_parse = p_walker.p_parse.clone().expect("Walker.p_parse");
    let (i_col, p_tab) = {
        let c = ctx_rc.borrow();
        (c.i_col, c.p_tab.clone().expect("RenameCtx.p_tab"))
    };
    if p_expr.op == TK_TRIGGER
        && p_expr.i_column as i32 == i_col
        && p_parse
            .borrow()
            .p_trigger_tab
            .as_ref()
            .map_or(false, |t| Rc::ptr_eq(t, &p_tab))
    {
        rename_token_find(&p_parse, Some(&mut ctx_rc.borrow_mut()), rename_ptr_key(p_expr));
    } else if p_expr.op == TK_COLUMN
        && p_expr.i_column as i32 == i_col
        && always(expr_use_y_tab(p_expr))
        && p_expr
            .y
            .p_tab
            .as_ref()
            .map_or(false, |t| Rc::ptr_eq(t, &p_tab))
    {
        rename_token_find(&p_parse, Some(&mut ctx_rc.borrow_mut()), rename_ptr_key(p_expr));
    }
    WRC_CONTINUE
}

/// O RenameCtx contém uma lista de tokens que referenciam uma coluna que está sendo
/// renomeada por um ALTER TABLE. Devolve o "último" RenameToken do RenameCtx e o
/// remove do RenameCtx. "Último" é o último RenameToken encontrado quando o SQL de
/// entrada é lido da esquerda para a direita. Chamadas repetidas devolvem todos os
/// tokens de nome de coluna na ordem em que aparecem na instrução SQL.
fn rename_column_token_next(p_ctx: &mut RenameCtx) -> Box<RenameToken> {
    // Acha a posição do token de maior offset (empate fica com o primeiro, como o
    // `>` estrito do C).
    let mut best_idx = 0usize;
    {
        let first = p_ctx.p_list.as_deref().expect("RenameCtx.p_list");
        let mut best_off = rename_token_offset(first);
        let mut p_token = first.p_next.as_deref();
        let mut idx = 1usize;
        while let Some(t) = p_token {
            if rename_token_offset(t) > best_off {
                best_off = rename_token_offset(t);
                best_idx = idx;
            }
            idx += 1;
            p_token = t.p_next.as_deref();
        }
    }
    let mut pp = &mut p_ctx.p_list;
    for _ in 0..best_idx {
        pp = &mut pp.as_mut().unwrap().p_next;
    }
    let mut p_best = pp.take().unwrap();
    *pp = p_best.p_next.take();
    p_best
}

/// Ocorreu um erro ao analisar ou processar de outra forma um objeto do banco
/// (p_parse.p_new_table, p_new_index ou p_new_trigger) como parte de um programa
/// ALTER TABLE RENAME COLUMN. A mensagem de erro emitida pela sub-rotina está agora
/// em p_parse.z_err_msg. Esta função acrescenta contexto à mensagem de erro e a
/// guarda em p_ctx.
fn rename_column_parse_error(
    p_ctx: &sqlite3_context,
    z_when: &[u8],
    p_type: &MemRef,
    p_object: &MemRef,
    p_parse: &ParseRef,
) {
    let z_t = api::value_text(p_type).unwrap_or_default();
    let z_n = api::value_text(p_object).unwrap_or_default();
    let db = p_parse.borrow().db.upgrade().expect("Parse.db");
    let z_err_msg = p_parse.borrow().z_err_msg.clone().unwrap_or_default();

    let z_err = m_printf(
        &db,
        b"error in %s %s%s%s: %s",
        &[
            PrintfArg::Text(&z_t),
            PrintfArg::Text(&z_n),
            PrintfArg::Text(if !z_when.is_empty() { b" " } else { b"" }),
            PrintfArg::Text(z_when),
            PrintfArg::Text(&z_err_msg),
        ],
    )
    .unwrap_or_default();
    api::result_error(p_ctx, &z_err, -1);
}

/// Para cada nome da lista de expressões p_e_list (isto é, cada p_e_list.a[i].zName)
/// que casa com a string z_old, extrai o rename-token correspondente do objeto Parse
/// p_parse e o acrescenta ao RenameCtx p_ctx.
fn rename_column_elist_names(
    p_parse: &ParseRef,
    p_ctx: &mut RenameCtx,
    p_e_list: Option<&ExprList>,
    z_old: &[u8],
) {
    if let Some(p_list) = p_e_list {
        for i in 0..p_list.n_expr as usize {
            let z_name = p_list.a[i].z_e_name.as_deref();
            if always(p_list.a[i].fg.e_e_name == ENAME_NAME)
                && always(z_name.is_some())
                && 0 == api::stricmp(z_name.unwrap(), z_old)
            {
                rename_token_find(p_parse, Some(&mut *p_ctx), rename_ptr_key(z_name.unwrap()));
            }
        }
    }
}

/// Para cada nome da lista de ids p_id_list (isto é, cada p_id_list.a[i].zName) que
/// casa com a string z_old, extrai o rename-token correspondente do objeto Parse
/// p_parse e o acrescenta ao RenameCtx p_ctx.
fn rename_column_idlist_names(
    p_parse: &ParseRef,
    p_ctx: &mut RenameCtx,
    p_id_list: Option<&IdList>,
    z_old: &[u8],
) {
    if let Some(p_list) = p_id_list {
        for i in 0..p_list.n_id as usize {
            let z_name = &p_list.a[i].z_name[..];
            if 0 == api::stricmp(z_name, z_old) {
                rename_token_find(p_parse, Some(&mut *p_ctx), rename_ptr_key(z_name));
            }
        }
    }
}

/// Analisa a instrução SQL z_sql usando o objeto Parse p. O objeto Parse é
/// inicializado por esta função antes de ser usado.
fn rename_parse_sql(
    p: &ParseRef,           // Memória para o objeto Parse
    z_db: Option<&[u8]>,    // Nome do esquema a que o SQL pertence
    db: &SqliteRef,         // Handle do banco
    z_sql: Option<&[u8]>,   // SQL a analisar
    b_temp: i32,            // Verdadeiro se o SQL vem do esquema temp
) -> i32 {
    parse_object_init(p, db);
    let z_sql = match z_sql {
        Some(z) => z,
        None => return SQLITE_NOMEM,
    };
    if str_ni_cmp(z_sql, b"CREATE ", 7) != 0 {
        return corrupt_bkpt();
    }
    db.borrow_mut().init.i_db = if b_temp != 0 { 1 } else { find_db_name(db, z_db) };
    {
        let mut parse = p.borrow_mut();
        parse.e_parse_mode = PARSE_MODE_RENAME;
        parse.db = Rc::downgrade(db);
        parse.n_query_loop = 1;
    }
    let mut rc = run_parser(p, z_sql);
    if db.borrow().malloc_failed != 0 {
        rc = SQLITE_NOMEM;
    }
    if rc == SQLITE_OK && never({
        let parse = p.borrow();
        parse.p_new_table.is_none() && parse.p_new_index.is_none() && parse.p_new_trigger.is_none()
    }) {
        rc = corrupt_bkpt();
    }

    // A verificação sob SQLITE_DEBUG dos mapeamentos de Parse.p_rename some.

    db.borrow_mut().init.i_db = 0;
    rc
}


// ---- part_003.rs ----

/// Edita a instrução SQL z_sql, substituindo cada token identificado pela lista
/// ligada de p_rename pelo texto de z_new. Se b_quote é verdadeiro, z_new sempre
/// é citado antes. Se não há erro, o resultado vai para o contexto p_ctx.
///
/// Ou, se há erro (isto é, falta de memória), o erro fica em p_ctx e um código de
/// erro do SQLite é devolvido.
fn rename_edit_sql(
    p_ctx: &sqlite3_context,
    p_rename: &mut RenameCtx,
    z_sql: &[u8],
    z_new: Option<&[u8]>,
    b_quote: i32,
) -> i32 {
    let n_new: i64 = z_new.map(|z| strlen_30(Some(z)) as i64).unwrap_or(0);
    let n_sql: i64 = strlen_30(Some(z_sql)) as i64;
    let db = api::context_db_handle(p_ctx);
    let mut z_quot: Vec<u8> = Vec::new();
    let mut n_quot: i64 = 0;
    let mut z_out: Vec<u8>;

    if let Some(z_new) = z_new {
        // z_quot recebe uma cópia citada do identificador z_new. Se o identificador
        // correspondente no ALTER TABLE original estava citado (b_quote==1), todas as
        // substituições usam a versão citada do novo nome de coluna.
        match m_printf(&db, b"\"%w\" ", &[PrintfArg::Text(z_new)]) {
            None => return SQLITE_NOMEM,
            Some(z) => {
                n_quot = strlen_30(Some(&z)) as i64 - 1;
                z_quot = z;
            }
        }

        debug_assert!(n_quot >= n_new);
        z_out = vec![0u8; (n_sql + p_rename.n_list as i64 * n_quot + 1) as usize];
    } else {
        z_out = vec![0u8; ((n_sql * 2 + 1) * 3) as usize];
    }

    // Neste ponto p_rename.p_list contém a lista de RenameToken de todos os tokens do
    // SQL de entrada que precisam ser trocados pelo novo nome de coluna ou por versões
    // entre aspas simples deles mesmos. Resta montar e devolver o SQL editado.
    let mut n_out: i64 = n_sql;
    z_out[..n_sql as usize].copy_from_slice(&z_sql[..n_sql as usize]);
    while p_rename.p_list.is_some() {
        let p_best = match rename_column_token_next(p_rename) {
            Some(p) => p,
            None => break,
        };
        let i_off: i64 = rename_token_offset(&p_best, z_sql) as i64;
        let n_tok: i64 = p_best.t.n as i64;
        let z_after: u8 = z_sql.get((i_off + n_tok) as usize).copied().unwrap_or(0);
        let n_replace: i64;
        let z_replace: Vec<u8>;

        if let Some(z_new) = z_new {
            if b_quote == 0 && is_id_char(p_best.t.z[0]) {
                n_replace = n_new;
                z_replace = z_new.to_vec();
            } else {
                let mut n = n_quot;
                if z_after == b'"' {
                    n += 1;
                }
                n_replace = n;
                z_replace = z_quot.clone();
            }
        } else {
            // Remove as aspas duplas do token e o cita de novo, agora com aspas simples.
            // Se o caractere logo depois do token original era uma aspa simples ('),
            // acrescenta outro espaço depois da versão nova, para que
            // (SELECT "string"'alias') vire (SELECT 'string' 'alias') e não
            // (SELECT 'string''alias').
            let mut z_buf1: Vec<u8> = p_best.t.z[..p_best.t.n as usize].to_vec();
            dequote(&mut z_buf1);
            let z_buf2 = api::snprintf(
                (n_sql * 2) as i32,
                b"%Q%s",
                &[
                    PrintfArg::Text(&z_buf1),
                    PrintfArg::Text(if z_after == b'\'' { b" " } else { b"" }),
                ],
            );
            n_replace = strlen_30(Some(&z_buf2)) as i64;
            z_replace = z_buf2;
        }

        if n_tok != n_replace {
            z_out.copy_within(
                (i_off + n_tok) as usize..n_out as usize,
                (i_off + n_replace) as usize,
            );
            n_out += n_replace - n_tok;
            z_out[n_out as usize] = 0;
        }
        z_out[i_off as usize..(i_off + n_replace) as usize]
            .copy_from_slice(&z_replace[..n_replace as usize]);
    }

    // O texto do resultado vai até o primeiro NUL (tamanho -1 no C).
    let n_text = z_out.iter().position(|&b| b == 0).unwrap_or(z_out.len());
    api::result_text(p_ctx, &z_out[..n_text], -1, SQLITE_TRANSIENT);

    // A falha de alocação (SQLITE_NOMEM) do C não existe com Vec.
    SQLITE_OK
}

/// Define todos os campos pEList->a[].fg.eEName da lista de expressões como val.
fn rename_set_e_names(p_e_list: Option<&mut ExprList>, val: i32) {
    if let Some(p_e_list) = p_e_list {
        for i in 0..p_e_list.n_expr as usize {
            debug_assert!(val == ENAME_NAME || p_e_list.a[i].fg.e_e_name == ENAME_NAME);
            p_e_list.a[i].fg.e_e_name = val as u8;
        }
    }
}

/// Resolve todos os símbolos do trigger em p_parse.p_new_trigger, supondo que ele
/// foi lido do schema do banco z_db. Devolve SQLITE_OK se der certo. Senão devolve
/// um código de erro do SQLite e deixa uma mensagem de erro no objeto Parse.
fn rename_resolve_trigger(p_parse: &ParseRef) -> i32 {
    let db = p_parse.borrow().db.upgrade().expect("Parse.db deve apontar para uma conexão viva");
    let p_new = p_parse.borrow().p_new_trigger.clone().expect("p_new_trigger");
    let mut s_nc = NameContext::default();
    let mut rc = SQLITE_OK;

    s_nc.p_parse = Some(p_parse.clone());
    debug_assert!(p_new.borrow().p_tab_schema.is_some());
    {
        let i_db = schema_to_index(&db, p_new.borrow().p_tab_schema.as_ref().unwrap());
        let z_db_sname = db.borrow().a_db[i_db as usize].z_db_sname.clone();
        let p_trigger_tab = find_table(&db, &p_new.borrow().table, &z_db_sname);
        let mut parse = p_parse.borrow_mut();
        parse.p_trigger_tab = p_trigger_tab;
        parse.e_trigger_op = p_new.borrow().op;
    }
    // ALWAYS() porque, se a tabela do trigger não existe, o erro já teria
    // aparecido antes deste ponto
    let p_trigger_tab = p_parse.borrow().p_trigger_tab.clone();
    if let Some(p_trigger_tab) = p_trigger_tab {
        rc = (view_get_column_names(p_parse, &p_trigger_tab) != 0) as i32;
    }

    // Resolve símbolos na cláusula WHEN
    if rc == SQLITE_OK && p_new.borrow().p_when.is_some() {
        rc = resolve_expr_names(&mut s_nc, p_new.borrow_mut().p_when.as_deref_mut());
    }

    let mut p_step = p_new.borrow().step_list.clone();
    while rc == SQLITE_OK && p_step.is_some() {
        let step = p_step.clone().unwrap();
        if step.borrow().p_select.is_some() {
            let p_step_select = step.borrow().p_select.clone().unwrap();
            select_prep(p_parse, &p_step_select, Some(&mut s_nc));
            if p_parse.borrow().n_err != 0 {
                rc = p_parse.borrow().rc;
            }
        }
        if rc == SQLITE_OK && step.borrow().z_target.is_some() {
            let mut p_src = trigger_step_src(p_parse, &step);
            if p_src.is_some() {
                let p_sel = select_new(
                    p_parse,
                    step.borrow_mut().p_expr_list.take(),
                    p_src.take(),
                    None,
                    None,
                    None,
                    None,
                    0,
                    None,
                );
                match p_sel {
                    None => {
                        step.borrow_mut().p_expr_list = None;
                        p_src = None;
                        rc = SQLITE_NOMEM;
                    }
                    Some(p_sel) => {
                        // p_step.p_expr_list contém a lista de expressões de um UPDATE,
                        // então os valores a[].zEName são o lado direito das cláusulas
                        // "<col> = <expr>". Antes de rodar select_prep(), troca todos os
                        // e_e_name de p_step.p_expr_list para ENAME_SPAN (do valor atual
                        // ENAME_NAME). Isso evita que ids em cláusulas ON() de p_src sejam
                        // resolvidos por engano contra os a[].zEName como se fossem
                        // apelidos de coluna.
                        rename_set_e_names(p_sel.borrow_mut().p_e_list.as_deref_mut(), ENAME_SPAN as i32);
                        select_prep(p_parse, &p_sel, None);
                        rename_set_e_names(p_sel.borrow_mut().p_e_list.as_deref_mut(), ENAME_NAME as i32);
                        rc = if p_parse.borrow().n_err != 0 { SQLITE_ERROR } else { SQLITE_OK };
                        // O select devolve a lista de expressões e a lista de origem ao
                        // passo e ao local, em vez de liberá-las junto com o Select
                        // (no C: pStep->pExprList = pSel->pEList = 0 e pSel->pSrc = 0).
                        step.borrow_mut().p_expr_list = p_sel.borrow_mut().p_e_list.take();
                        p_src = p_sel.borrow_mut().p_src.take();
                        select_delete(&db, Some(p_sel));
                    }
                }
                if let Some(p_from) = step.borrow().p_from.as_ref() {
                    let mut i = 0;
                    while i < p_from.n_src && rc == SQLITE_OK {
                        if let Some(p_select) = p_from.a[i as usize].p_select.as_ref() {
                            select_prep(p_parse, p_select, None);
                        }
                        i += 1;
                    }
                }

                if db.borrow().malloc_failed != 0 {
                    rc = SQLITE_NOMEM;
                }
                s_nc.p_src_list = p_src.clone();
                if rc == SQLITE_OK && step.borrow().p_where.is_some() {
                    rc = resolve_expr_names(&mut s_nc, step.borrow_mut().p_where.as_deref_mut());
                }
                if rc == SQLITE_OK {
                    rc = resolve_expr_list_names(&mut s_nc, step.borrow_mut().p_expr_list.as_deref_mut());
                }
                debug_assert!(
                    step.borrow().p_upsert.is_none()
                        || (step.borrow().p_where.is_none() && step.borrow().p_expr_list.is_none())
                );
                if step.borrow().p_upsert.is_some() && rc == SQLITE_OK {
                    let mut step_mut = step.borrow_mut();
                    let p_upsert = step_mut.p_upsert.as_deref_mut().unwrap();
                    p_upsert.p_upsert_src = p_src.clone();
                    s_nc.u_nc = NameContextUNC::Upsert;
                    s_nc.nc_flags = NC_UUPSERT;
                    rc = resolve_expr_list_names(&mut s_nc, p_upsert.p_upsert_target.as_deref_mut());
                    if rc == SQLITE_OK {
                        let p_upsert_set = p_upsert.p_upsert_set.as_deref_mut();
                        rc = resolve_expr_list_names(&mut s_nc, p_upsert_set);
                    }
                    if rc == SQLITE_OK {
                        rc = resolve_expr_names(&mut s_nc, p_upsert.p_upsert_where.as_deref_mut());
                    }
                    if rc == SQLITE_OK {
                        rc = resolve_expr_names(&mut s_nc, p_upsert.p_upsert_target_where.as_deref_mut());
                    }
                    s_nc.nc_flags = 0;
                }
                s_nc.p_src_list = None;
                src_list_delete(&db, p_src);
            } else {
                rc = SQLITE_NOMEM;
            }
        }
        p_step = step.borrow().p_next.clone();
    }
    rc
}

/// Chama sqlite3WalkExpr() ou sqlite3WalkSelect() em todos os objetos Select ou
/// Expr que fazem parte do trigger passado como segundo argumento.
fn rename_walk_trigger(p_walker: &mut Walker, p_trigger: &TriggerRef) {
    // Acha os tokens a editar na cláusula WHEN
    walk_expr(p_walker, p_trigger.borrow().p_when.as_deref());

    // Acha os tokens a editar nos passos do trigger
    let mut p_step = p_trigger.borrow().step_list.clone();
    while let Some(step) = p_step {
        {
            let p_step_select = step.borrow().p_select.clone();
            walk_select(p_walker, p_step_select.as_ref());
        }
        walk_expr(p_walker, step.borrow().p_where.as_deref());
        walk_expr_list(p_walker, step.borrow().p_expr_list.as_deref());
        if let Some(p_upsert) = step.borrow().p_upsert.as_deref() {
            walk_expr_list(p_walker, p_upsert.p_upsert_target.as_deref());
            walk_expr_list(p_walker, p_upsert.p_upsert_set.as_deref());
            walk_expr(p_walker, p_upsert.p_upsert_where.as_deref());
            walk_expr(p_walker, p_upsert.p_upsert_target_where.as_deref());
        }
        if let Some(p_from) = step.borrow().p_from.as_ref() {
            for i in 0..p_from.n_src as usize {
                walk_select(p_walker, p_from.a[i].p_select.as_ref());
            }
        }
        p_step = step.borrow().p_next.clone();
    }
}

/// Libera o conteúdo do objeto Parse (*p_parse). Não libera a memória ocupada
/// pelo próprio objeto Parse.
fn rename_parse_cleanup(p_parse: &ParseRef) {
    let db = p_parse.borrow().db.upgrade().expect("Parse.db deve apontar para uma conexão viva");
    if let Some(p_vdbe) = p_parse.borrow_mut().p_vdbe.take() {
        vdbe_finalize(p_vdbe);
    }
    delete_table(&db, p_parse.borrow_mut().p_new_table.take());
    loop {
        let p_idx = p_parse.borrow_mut().p_new_index.take();
        let p_idx = match p_idx {
            Some(p) => p,
            None => break,
        };
        p_parse.borrow_mut().p_new_index = p_idx.borrow_mut().p_next.take();
        free_index(&db, p_idx);
    }
    delete_trigger(&db, p_parse.borrow_mut().p_new_trigger.take());
    db_free(&db, p_parse.borrow_mut().z_err_msg.take());
    rename_token_free(&db, p_parse.borrow_mut().p_rename.take());
    parse_object_reset(p_parse);
}

/// Função SQL:
///
///     sqlite_rename_column(SQL,TYPE,OBJ,DB,TABLE,COL,NEWNAME,QUOTE,TEMP)
///
///   0. z_sql:    instrução SQL a reescrever
///   1. type:     tipo do objeto ("table", "view" etc.)
///   2. object:   nome do objeto
///   3. Database: nome do banco (por exemplo "main")
///   4. Table:    nome da tabela
///   5. i_col:    índice da coluna a renomear
///   6. z_new:    novo nome da coluna
///   7. b_quote:  diferente de zero se o novo nome da coluna deve ser citado
///   8. b_temp:   verdadeiro se z_sql vem do schema temp
///
/// Faz a renomeação de coluna na instrução CREATE dada em z_sql. A coluna i_col
/// (a mais à esquerda é 0) da tabela z_table é renomeada de z_col para z_new. O
/// nome deve ser citado se b_quote é verdadeiro.
///
/// Esta função é usada internamente pelo ALTER TABLE RENAME COLUMN. Só é acessível
/// a SQL criado com sqlite3NestedParse(). Não é alcançável por SQL comum passado ao
/// sqlite3_prepare() a menos que SQLITE_TESTCTRL_INTERNAL_FUNCTIONS esteja ligado.
fn rename_column_func(context: &sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let z_sql = api::value_text(&argv[0]);
    let z_db = api::value_text(&argv[3]);
    let z_table = api::value_text(&argv[4]);
    let i_col = api::value_int(&argv[5]);
    let z_new = api::value_text(&argv[6]);
    let b_quote = api::value_int(&argv[7]);
    let b_temp = api::value_int(&argv[8]);
    let x_auth = db.borrow().x_auth.clone();

    let z_sql = match z_sql {
        Some(z) => z,
        None => return,
    };
    let z_table = match z_table {
        Some(z) => z,
        None => return,
    };
    let z_new = match z_new {
        Some(z) => z,
        None => return,
    };
    if i_col < 0 {
        return;
    }
    btree_enter_all(&db);
    let p_tab = find_table(&db, &z_table, z_db.as_deref());
    let p_tab = match p_tab {
        Some(t) if (i_col as i64) < t.borrow().n_col as i64 => t,
        _ => {
            btree_leave_all(&db);
            return;
        }
    };
    let z_old: Vec<u8> = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
    let s_ctx = Rc::new(RefCell::new(RenameCtx {
        p_list: None,
        n_list: 0,
        i_col: if i_col == p_tab.borrow().i_p_key as i32 { -1 } else { i_col },
        p_tab: Some(p_tab.clone()),
        z_old: None,
    }));

    db.borrow_mut().x_auth = None;
    let s_parse: ParseRef = Rc::new(RefCell::new(Parse::default()));
    let mut rc = rename_parse_sql(&s_parse, z_db.as_deref(), &db, Some(&z_sql), b_temp);

    // Acha os tokens que precisam ser trocados.
    let mut s_walker = Walker::default();
    s_walker.p_parse = Some(s_parse.clone());
    s_walker.x_expr_callback = Some(rename_column_expr_cb);
    s_walker.x_select_callback = Some(rename_column_select_cb);
    s_walker.u = WalkerU::Rename(s_ctx.clone());

    s_ctx.borrow_mut().p_tab = Some(p_tab.clone());
    'renameColumnFunc_done: {
        if rc != SQLITE_OK {
            break 'renameColumnFunc_done;
        }
        let p_new_table = s_parse.borrow().p_new_table.clone();
        let p_new_index = s_parse.borrow().p_new_index.clone();
        if let Some(p_new_table) = p_new_table {
            if is_view(&p_new_table.borrow()) {
                let p_select = p_new_table.borrow().u.view.p_select.clone().expect("view.p_select");
                p_select.borrow_mut().sel_flags &= !SF_VIEW;
                s_parse.borrow_mut().rc = SQLITE_OK;
                select_prep(&s_parse, &p_select, None);
                rc = if db.borrow().malloc_failed != 0 { SQLITE_NOMEM } else { s_parse.borrow().rc };
                if rc == SQLITE_OK {
                    walk_select(&mut s_walker, Some(&p_select));
                }
                if rc != SQLITE_OK {
                    break 'renameColumnFunc_done;
                }
            } else if is_ordinary_table(&p_new_table.borrow()) {
                // Uma tabela comum
                let b_fk_only = api::stricmp(&z_table, &p_new_table.borrow().z_name);
                s_ctx.borrow_mut().p_tab = Some(p_new_table.clone());
                if b_fk_only == 0 {
                    if i_col < p_new_table.borrow().n_col as i32 {
                        let tab = p_new_table.borrow();
                        let key = rename_ptr_key(&tab.a_col[i_col as usize].z_cn_name[..]);
                        rename_token_find(&s_parse, Some(&mut s_ctx.borrow_mut()), key);
                    }
                    if s_ctx.borrow().i_col < 0 {
                        let tab = p_new_table.borrow();
                        let key = rename_ptr_key(&tab.i_p_key);
                        rename_token_find(&s_parse, Some(&mut s_ctx.borrow_mut()), key);
                    }
                    walk_expr_list(&mut s_walker, p_new_table.borrow().p_check.as_deref());
                    let mut p_idx = p_new_table.borrow().p_index.clone();
                    while let Some(idx) = p_idx {
                        walk_expr_list(&mut s_walker, idx.borrow().a_col_expr.as_deref());
                        p_idx = idx.borrow().p_next.clone();
                    }
                    let mut p_idx = p_new_index.clone();
                    while let Some(idx) = p_idx {
                        walk_expr_list(&mut s_walker, idx.borrow().a_col_expr.as_deref());
                        p_idx = idx.borrow().p_next.clone();
                    }
                    for i in 0..p_new_table.borrow().n_col as usize {
                        let tab = p_new_table.borrow();
                        let p_expr = column_expr(&tab, &tab.a_col[i]);
                        walk_expr(&mut s_walker, p_expr.as_deref());
                    }
                }

                debug_assert!(is_ordinary_table(&p_new_table.borrow()));
                let mut p_f_key = p_new_table.borrow().u.tab.p_f_key.clone();
                while let Some(f_key) = p_f_key {
                    for i in 0..f_key.borrow().n_col as usize {
                        if b_fk_only == 0 && f_key.borrow().a_col[i].i_from == i_col {
                            let fk = f_key.borrow();
                            let key = rename_ptr_key(&fk.a_col[i]);
                            rename_token_find(&s_parse, Some(&mut s_ctx.borrow_mut()), key);
                        }
                        if 0 == api::stricmp(&f_key.borrow().z_to, &z_table)
                            && 0 == api::stricmp(&f_key.borrow().a_col[i].z_col, &z_old)
                        {
                            let fk = f_key.borrow();
                            let key = rename_ptr_key(&fk.a_col[i].z_col[..]);
                            rename_token_find(&s_parse, Some(&mut s_ctx.borrow_mut()), key);
                        }
                    }
                    p_f_key = f_key.borrow().p_next_from.clone();
                }
            }
        } else if let Some(p_new_index) = p_new_index {
            walk_expr_list(&mut s_walker, p_new_index.borrow().a_col_expr.as_deref());
            walk_expr(&mut s_walker, p_new_index.borrow().p_part_idx_where.as_deref());
        } else {
            // Um trigger
            rc = rename_resolve_trigger(&s_parse);
            if rc != SQLITE_OK {
                break 'renameColumnFunc_done;
            }

            let p_new_trigger = s_parse.borrow().p_new_trigger.clone().expect("p_new_trigger");
            let mut p_step = p_new_trigger.borrow().step_list.clone();
            while let Some(step) = p_step {
                if let Some(z_target) = step.borrow().z_target.clone() {
                    let p_target = locate_table(&s_parse, 0, &z_target, z_db.as_deref());
                    let eq = match &p_target {
                        Some(t) => Rc::ptr_eq(t, &p_tab),
                        None => false,
                    };
                    if eq {
                        if let Some(p_upsert) = step.borrow().p_upsert.as_deref() {
                            rename_column_elist_names(&s_parse, &mut s_ctx.borrow_mut(), p_upsert.p_upsert_set.as_deref(), &z_old);
                        }
                        rename_column_idlist_names(&s_parse, &mut s_ctx.borrow_mut(), step.borrow().p_id_list.as_deref(), &z_old);
                        rename_column_elist_names(&s_parse, &mut s_ctx.borrow_mut(), step.borrow().p_expr_list.as_deref(), &z_old);
                    }
                }
                p_step = step.borrow().p_next.clone();
            }

            // Acha os tokens a editar na cláusula UPDATE OF
            let p_trigger_tab = s_parse.borrow().p_trigger_tab.clone();
            if let Some(t) = p_trigger_tab {
                if Rc::ptr_eq(&t, &p_tab) {
                    rename_column_idlist_names(&s_parse, &mut s_ctx.borrow_mut(), p_new_trigger.borrow().p_columns.as_deref(), &z_old);
                }
            }

            // Acha os tokens a editar em várias expressões e selects
            rename_walk_trigger(&mut s_walker, &p_new_trigger);
        }

        debug_assert!(rc == SQLITE_OK);
        rc = rename_edit_sql(context, &mut s_ctx.borrow_mut(), &z_sql, Some(&z_new), b_quote);
    }

    // renameColumnFunc_done:
    if rc != SQLITE_OK {
        if rc == SQLITE_ERROR && writable_schema(&db) {
            api::result_value(context, &argv[0]);
        } else if s_parse.borrow().z_err_msg.is_some() {
            rename_column_parse_error(context, b"", &argv[1], &argv[2], &s_parse);
        } else {
            api::result_error_code(context, rc);
        }
    }

    rename_parse_cleanup(&s_parse);
    rename_token_free(&db, s_ctx.borrow_mut().p_list.take());
    db.borrow_mut().x_auth = x_auth;
    btree_leave_all(&db);
}


// ---- part_004.rs ----

/// Callback de expressão do Walker usado por "RENAME TABLE".
fn rename_table_expr_cb(p_walker: &mut Walker, p_expr: &Expr) -> i32 {
    let p = p_walker.u.p_rename_mut();
    if p_expr.op == TK_COLUMN && always(expr_use_ytab(p_expr)) && p.p_tab_is(&p_expr.y.p_tab) {
        rename_token_find(p_walker.p_parse_mut(), Some(p), token_key_ytab(p_expr));
    }
    WRC_CONTINUE
}

/// Callback de Select do Walker usado por "RENAME TABLE".
fn rename_table_select_cb(p_walker: &mut Walker, p_select: &Select) -> i32 {
    let p = p_walker.u.p_rename_mut();
    if p_select.sel_flags & (SF_VIEW | SF_COPY_CTE) != 0 {
        return WRC_PRUNE;
    }
    let p_src = match p_select.p_src.as_ref() {
        Some(s) => s,
        None => {
            debug_assert!(p_walker.p_parse().db_malloc_failed());
            return WRC_ABORT;
        }
    };
    for p_item in p_src.a.iter().take(p_src.n_src as usize) {
        if p.p_tab_is(&p_item.p_tab) {
            rename_token_find(p_walker.p_parse_mut(), Some(p), token_key_name(&p_item.z_name));
        }
    }
    rename_walk_with(p_walker, p_select);
    WRC_CONTINUE
}

/// Implementa a função SQL usada pelo código gerado por ALTER TABLE ... RENAME
/// para alterar a definição de chaves estrangeiras que usam a tabela renomeada
/// como tabela pai. Recebe:
///
///   0: o banco que contém a tabela renomeada
///   1: tipo do objeto ("table", "view" etc.)
///   2: nome do objeto
///   3: o texto completo da instrução de esquema a modificar
///   4: o nome antigo da tabela
///   5: o novo nome da tabela
///   6: verdadeiro se a instrução vem do banco temp
///
/// Devolve a nova instrução de esquema.
fn rename_table_func(context: &mut sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let z_db = api::value_text(&argv[0]);
    let z_input = api::value_text(&argv[3]);
    let z_old = api::value_text(&argv[4]);
    let z_new = api::value_text(&argv[5]);
    let b_temp = api::value_int(&argv[6]);

    if let (Some(z_db), Some(z_input), Some(z_old), Some(z_new)) = (z_db, z_input, z_old, z_new) {
        let mut s_parse = Parse::default();
        let b_quote = 1;
        let mut s_ctx = RenameCtx::default();
        let mut s_walker = Walker::default();

        let x_auth = db.borrow_mut().x_auth.take();
        btree_enter_all(&db);

        s_ctx.p_tab = find_table(&db.borrow(), &z_old, Some(&z_db));
        s_walker.set_parse(&mut s_parse);
        s_walker.x_expr_callback = Some(rename_table_expr_cb);
        s_walker.x_select_callback = Some(rename_table_select_cb);
        s_walker.set_rename(&mut s_ctx);

        let mut rc = rename_parse_sql(&mut s_parse, &z_db, &db.borrow(), Some(&z_input), b_temp);

        if rc == SQLITE_OK {
            let is_legacy = db.borrow().flags & SQLITE_LEGACY_ALTER != 0;
            if let Some(p_tab) = s_parse.p_new_table.clone() {
                if is_view(&p_tab.borrow()) {
                    if !is_legacy {
                        let p_select = p_tab.borrow().u.view.p_select_ref();
                        let mut s_nc = NameContext::default();
                        s_nc.p_parse = Some(&mut s_parse as *mut Parse as usize);
                        debug_assert!(p_select.borrow().sel_flags & SF_VIEW != 0);
                        p_select.borrow_mut().sel_flags &= !SF_VIEW;
                        select_prep(&mut s_parse, &p_select, Some(&mut s_nc));
                        if s_parse.n_err != 0 {
                            rc = s_parse.rc;
                        } else {
                            walk_select(&mut s_walker, &p_select);
                        }
                    }
                } else {
                    // Modifica as definições de FK para apontarem para a nova tabela
                    if (!is_legacy || db.borrow().flags & SQLITE_FOREIGN_KEYS != 0) && !is_virtual(&p_tab.borrow()) {
                        debug_assert!(is_ordinary_table(&p_tab.borrow()));
                        let mut p_fkey = p_tab.borrow().u.tab.p_fkey.clone();
                        while let Some(fk) = p_fkey {
                            if stricmp(&fk.borrow().z_to, &z_old) == 0 {
                                rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(&fk.borrow().z_to));
                            }
                            p_fkey = fk.borrow().p_next_from.clone();
                        }
                    }
                    // Se é a tabela alterada, corrige as referências nas expressões CHECK
                    // e atualiza o nome logo depois de "CREATE [VIRTUAL] TABLE".
                    if stricmp(&z_old, &p_tab.borrow().z_name) == 0 {
                        s_ctx.p_tab = Some(p_tab.clone());
                        if !is_legacy {
                            walk_expr_list(&mut s_walker, p_tab.borrow().p_check.as_ref());
                        }
                        rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(&p_tab.borrow().z_name));
                    }
                }
            } else if let Some(p_idx) = s_parse.p_new_index.clone() {
                rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(&p_idx.borrow().z_name));
                if !is_legacy {
                    walk_expr(&mut s_walker, p_idx.borrow().p_part_idx_where.as_ref());
                }
            } else {
                let p_trigger = s_parse.p_new_trigger.clone().expect("gatilho novo");
                if stricmp(&p_trigger.borrow().table, &z_old) == 0
                    && s_ctx.p_tab_schema_is(&p_trigger.borrow().p_tab_schema)
                {
                    rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(&p_trigger.borrow().table));
                }
                if !is_legacy {
                    rc = rename_resolve_trigger(&mut s_parse);
                    if rc == SQLITE_OK {
                        rename_walk_trigger(&mut s_walker, &p_trigger);
                        let mut p_step = p_trigger.borrow().step_list.clone();
                        while let Some(step) = p_step {
                            if let Some(z_target) = step.borrow().z_target.as_ref() {
                                if stricmp(z_target, &z_old) == 0 {
                                    rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(z_target));
                                }
                            }
                            if let Some(p_from) = step.borrow().p_from.as_ref() {
                                for p_item in p_from.a.iter().take(p_from.n_src as usize) {
                                    if stricmp(&p_item.z_name, &z_old) == 0 {
                                        rename_token_find(&mut s_parse, Some(&mut s_ctx), token_key_name(&p_item.z_name));
                                    }
                                }
                            }
                            p_step = step.borrow().p_next.clone();
                        }
                    }
                }
            }
        }

        if rc == SQLITE_OK {
            rc = rename_edit_sql(context, &mut s_ctx, &z_input, Some(&z_new), b_quote);
        }
        if rc != SQLITE_OK {
            if rc == SQLITE_ERROR && writable_schema(&db.borrow()) {
                api::result_value(context, &argv[3]);
            } else if s_parse.z_err_msg.is_some() {
                rename_column_parse_error(context, b"", &argv[1], &argv[2], &mut s_parse);
            } else {
                api::result_error_code(context, rc);
            }
        }

        rename_parse_cleanup(&mut s_parse);
        rename_token_free(&db.borrow(), s_ctx.p_list.take());
        btree_leave_all(&db);
        db.borrow_mut().x_auth = x_auth;
    }
}

fn rename_quotefix_expr_cb(p_walker: &mut Walker, p_expr: &Expr) -> i32 {
    if p_expr.op == TK_STRING && p_expr.flags & EP_DBL_QUOTED != 0 {
        rename_token_find(p_walker.p_parse_mut(), Some(p_walker.u.p_rename_mut()), token_key_expr(p_expr));
    }
    WRC_CONTINUE
}

/// Função SQL: sqlite_rename_quotefix(DB,SQL)
///
/// Reescreve a instrução DDL "SQL" para que literais de texto entre aspas duplas
/// passem a usar aspas simples. Argumentos:
///
///   0: nome do banco ("main", "temp" etc.)
///   1: instrução SQL a editar
///
/// O valor devolvido é a instrução modificada. Se há erro no SQL de entrada, levanta
/// o erro, exceto com PRAGMA writable_schema=ON, quando devolve a entrada intacta.
fn rename_quotefix_func(context: &mut sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let z_db = api::value_text(&argv[0]);
    let z_input = api::value_text(&argv[1]);

    let x_auth = db.borrow_mut().x_auth.take();
    btree_enter_all(&db);

    if let (Some(z_db), Some(z_input)) = (z_db, z_input) {
        let mut s_parse = Parse::default();
        let mut rc = rename_parse_sql(&mut s_parse, &z_db, &db.borrow(), Some(&z_input), 0);

        if rc == SQLITE_OK {
            let mut s_ctx = RenameCtx::default();
            let mut s_walker = Walker::default();

            // Walker que acha os tokens a substituir
            s_walker.set_parse(&mut s_parse);
            s_walker.x_expr_callback = Some(rename_quotefix_expr_cb);
            s_walker.x_select_callback = Some(rename_column_select_cb);
            s_walker.set_rename(&mut s_ctx);

            if let Some(p_new_table) = s_parse.p_new_table.clone() {
                if is_view(&p_new_table.borrow()) {
                    let p_select = p_new_table.borrow().u.view.p_select_ref();
                    p_select.borrow_mut().sel_flags &= !SF_VIEW;
                    s_parse.rc = SQLITE_OK;
                    select_prep(&mut s_parse, &p_select, None);
                    rc = if db.borrow().malloc_failed != 0 { SQLITE_NOMEM } else { s_parse.rc };
                    if rc == SQLITE_OK {
                        walk_select(&mut s_walker, &p_select);
                    }
                } else {
                    walk_expr_list(&mut s_walker, p_new_table.borrow().p_check.as_ref());
                    for i in 0..p_new_table.borrow().n_col as usize {
                        let p_e = column_expr(&p_new_table.borrow(), i);
                        walk_expr(&mut s_walker, p_e.as_ref());
                    }
                }
            } else if let Some(p_new_index) = s_parse.p_new_index.clone() {
                walk_expr_list(&mut s_walker, p_new_index.borrow().a_col_expr.as_ref());
                walk_expr(&mut s_walker, p_new_index.borrow().p_part_idx_where.as_ref());
            } else {
                rc = rename_resolve_trigger(&mut s_parse);
                if rc == SQLITE_OK {
                    let p_trigger = s_parse.p_new_trigger.clone().expect("gatilho novo");
                    rename_walk_trigger(&mut s_walker, &p_trigger);
                }
            }

            if rc == SQLITE_OK {
                rc = rename_edit_sql(context, &mut s_ctx, &z_input, None, 0);
            }
            rename_token_free(&db.borrow(), s_ctx.p_list.take());
        }
        if rc != SQLITE_OK {
            if writable_schema(&db.borrow()) && rc == SQLITE_ERROR {
                api::result_value(context, &argv[1]);
            } else {
                api::result_error_code(context, rc);
            }
        }
        rename_parse_cleanup(&mut s_parse);
    }

    db.borrow_mut().x_auth = x_auth;
    btree_leave_all(&db);
}

/// Função: sqlite_rename_test(DB,SQL,TYPE,NAME,ISTEMP,WHEN,DQS)
///
/// Verifica que não há problemas de análise ou de resolução de símbolos numa instrução
/// CREATE TRIGGER|TABLE|VIEW|INDEX. Argumentos:
///
///   0: nome do banco ("main", "temp" etc.)
///   1: instrução SQL
///   2: tipo do objeto ("view", "table", "trigger" ou "index")
///   3: nome do objeto
///   4: verdadeiro se o objeto vem do esquema temp
///   5: parte "when" da mensagem de erro
///   6: verdadeiro para desligar a peculiaridade DQS ao analisar o SQL
///
/// Valor devolvido:
///
///   A. Se há erro e não está em PRAGMA writable_schema=ON, levanta o erro.
///   B. Senão, se um gatilho é criado e a tabela dele está no banco zDb, devolve 1.
///   C. Senão, devolve NULL.
fn rename_table_test(context: &mut sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let z_db = api::value_text(&argv[0]);
    let z_input = api::value_text(&argv[1]);
    let b_temp = api::value_int(&argv[4]);
    let is_legacy = db.borrow().flags & SQLITE_LEGACY_ALTER != 0;
    let z_when = api::value_text(&argv[5]);
    let b_no_dqs = api::value_int(&argv[6]);

    let x_auth = db.borrow_mut().x_auth.take();

    if let (Some(z_db), Some(z_input)) = (z_db, z_input) {
        let mut s_parse = Parse::default();
        let flags = db.borrow().flags;
        if b_no_dqs != 0 {
            db.borrow_mut().flags &= !(SQLITE_DQS_DML | SQLITE_DQS_DDL);
        }
        let mut rc = rename_parse_sql(&mut s_parse, &z_db, &db.borrow(), Some(&z_input), b_temp);
        db.borrow_mut().flags |= flags & (SQLITE_DQS_DML | SQLITE_DQS_DDL);
        if rc == SQLITE_OK {
            let is_view_tab = s_parse.p_new_table.as_ref().map_or(false, |t| is_view(&t.borrow()));
            if !is_legacy && is_view_tab {
                let mut s_nc = NameContext::default();
                s_nc.p_parse = Some(&mut s_parse as *mut Parse as usize);
                let p_select = s_parse.p_new_table.as_ref().unwrap().borrow().u.view.p_select_ref();
                select_prep(&mut s_parse, &p_select, Some(&mut s_nc));
                if s_parse.n_err != 0 {
                    rc = s_parse.rc;
                }
            } else if let Some(p_trigger) = s_parse.p_new_trigger.clone() {
                if !is_legacy {
                    rc = rename_resolve_trigger(&mut s_parse);
                }
                if rc == SQLITE_OK {
                    let i1 = schema_to_index(&db.borrow(), &p_trigger.borrow().p_tab_schema);
                    let i2 = find_db_name(&db.borrow(), &z_db);
                    if i1 == i2 {
                        // Caso de saída B
                        api::result_int(context, 1);
                    }
                }
            }
        }

        if rc != SQLITE_OK && z_when.is_some() && !writable_schema(&db.borrow()) {
            // Caso de saída A
            rename_column_parse_error(context, z_when.as_deref().unwrap(), &argv[2], &argv[3], &mut s_parse);
        }
        rename_parse_cleanup(&mut s_parse);
    }

    db.borrow_mut().x_auth = x_auth;
}


// ---- part_005.rs ----

/// Implementação da UDF interna sqlite_drop_column().
///
/// Argumentos:
///
///  argv[0]: um inteiro, o índice do esquema que contém a tabela
///  argv[1]: instrução CREATE TABLE a modificar
///  argv[2]: um inteiro, o índice da coluna a remover
///
/// O valor devolvido é um texto com a instrução CREATE TABLE sem a coluna argv[2].
///
/// Modelo de ponteiros: `RenameToken.t.z` é um deslocamento `usize` dentro de `z_sql`
/// (o C guarda `const char*` para dentro do mesmo buffer).
fn drop_column_func(context: &mut sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let i_schema = api::value_int(&argv[0]);
    let z_sql = api::value_text(&argv[1]);
    let i_col = api::value_int(&argv[2]);
    let z_db: Vec<u8> = db.borrow().a_db[i_schema as usize].z_db_sname.clone();
    let mut s_parse = Parse::default();
    let mut rc: i32;

    // SQLITE_OMIT_AUTHORIZATION não está definido: desliga o callback de autorização.
    let x_auth = db.borrow_mut().x_auth.take();

    'drop_column_done: {
        rc = rename_parse_sql(&mut s_parse, &z_db, &db.borrow(), z_sql.as_deref(), (i_schema == 1) as i32);
        if rc != SQLITE_OK {
            break 'drop_column_done;
        }
        let p_tab = match s_parse.p_new_table.clone() {
            Some(t) if t.borrow().n_col != 1 && i_col < t.borrow().n_col as i32 => t,
            _ => {
                // Pode acontecer se a tabela sqlite_schema estiver corrompida
                rc = SQLITE_CORRUPT_BKPT;
                break 'drop_column_done;
            }
        };
        let z_sql_bytes = z_sql.as_deref().unwrap_or(&[]);
        let n_col = p_tab.borrow().n_col as i32;

        let mut p_col = rename_token_find(&mut s_parse, None, p_tab.borrow().a_col[i_col as usize].z_cn_name_id());
        let z_end: usize;
        if i_col < n_col - 1 {
            let p_end = rename_token_find(&mut s_parse, None, p_tab.borrow().a_col[(i_col + 1) as usize].z_cn_name_id());
            z_end = p_end.expect("token da próxima coluna").t.z;
        } else {
            debug_assert!(is_ordinary_table(&p_tab.borrow()));
            z_end = p_tab.borrow().u.tab.add_col_offset as usize;
            if let Some(c) = p_col.as_mut() {
                while c.t.z < z_sql_bytes.len() && z_sql_bytes[c.t.z] != 0 && z_sql_bytes[c.t.z] != b',' {
                    if c.t.z == 0 {
                        break;
                    }
                    c.t.z -= 1;
                }
            }
        }
        let z_col_off = p_col.expect("token da coluna").t.z;

        // sqlite3MPrintf(db, "%.*s%s", pCol->t.z-zSql, zSql, zEnd)
        let mut z_new: Vec<u8> = z_sql_bytes[..z_col_off].to_vec();
        z_new.extend_from_slice(cstr_at(z_sql_bytes, z_end));
        api::result_text(context, &z_new, SQLITE_TRANSIENT);
    }

    rename_parse_cleanup(&mut s_parse);
    db.borrow_mut().x_auth = x_auth;
    if rc != SQLITE_OK {
        api::result_error_code(context, rc);
    }
}

/// Chamada pelo parser ao analisar
///
///     ALTER TABLE pSrc DROP COLUMN pName
///
/// pSrc tem o nome possivelmente qualificado da tabela e pName o nome da coluna a remover.
pub fn alter_drop_column(p_parse: &mut Parse, p_src: Box<SrcList>, p_name: &Token) {
    let db = p_parse.db.upgrade().expect("Parse.db deve apontar para uma conexão viva");
    let mut z_col: Option<Vec<u8>> = None;

    // Procura a tabela sendo alterada
    debug_assert!(p_parse.p_new_table.is_none());
    debug_assert!(btree_holds_all_mutexes(&db.borrow()));

    'exit_drop_column: {
        if never(db.borrow().malloc_failed != 0) {
            break 'exit_drop_column;
        }
        let p_tab = match locate_table_item(p_parse, 0, &p_src.a[0]) {
            Some(t) => t,
            None => break 'exit_drop_column,
        };

        // Não deixa alterar view, tabela virtual ou tabela de sistema
        if is_alterable_table(p_parse, &p_tab.borrow()) != SQLITE_OK {
            break 'exit_drop_column;
        }
        if is_real_table(p_parse, &p_tab.borrow(), 1) != SQLITE_OK {
            break 'exit_drop_column;
        }

        // Acha o índice da coluna a remover
        z_col = name_from_token(&db.borrow(), p_name);
        let z_col_s = match z_col.as_ref() {
            Some(c) => c.clone(),
            None => {
                debug_assert!(db.borrow().malloc_failed != 0);
                break 'exit_drop_column;
            }
        };
        let i_col = column_index(&p_tab.borrow(), &z_col_s);
        if i_col < 0 {
            // "no such column: \"%T\"" com o texto do token
            let mut msg = b"no such column: \"".to_vec();
            msg.extend_from_slice(token_text(p_name));
            msg.push(b'"');
            error_msg(p_parse, &msg);
            break 'exit_drop_column;
        }

        // Não deixa remover coluna PRIMARY KEY nem com restrição UNIQUE
        let col_flags = p_tab.borrow().a_col[i_col as usize].col_flags;
        if col_flags & (COLFLAG_PRIMKEY | COLFLAG_UNIQUE) != 0 {
            let mut msg = b"cannot drop ".to_vec();
            msg.extend_from_slice(if col_flags & COLFLAG_PRIMKEY != 0 { b"PRIMARY KEY" } else { b"UNIQUE" });
            msg.extend_from_slice(b" column: \"");
            msg.extend_from_slice(&z_col_s);
            msg.push(b'"');
            error_msg(p_parse, &msg);
            break 'exit_drop_column;
        }

        // Não deixa o número de colunas chegar a zero
        if p_tab.borrow().n_col <= 1 {
            let mut msg = b"cannot drop column \"".to_vec();
            msg.extend_from_slice(&z_col_s);
            msg.extend_from_slice(b"\": no other columns exist");
            error_msg(p_parse, &msg);
            break 'exit_drop_column;
        }

        // Edita a tabela sqlite_schema
        let i_db = schema_to_index(&db.borrow(), &p_tab.borrow().p_schema);
        debug_assert!(i_db >= 0);
        let z_db: Vec<u8> = db.borrow().a_db[i_db as usize].z_db_sname.clone();

        // Chama o callback de autorização
        if auth_check(p_parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&p_tab.borrow().z_name), Some(&z_col_s)) != 0 {
            break 'exit_drop_column;
        }
        rename_test_schema(p_parse, Some(&z_db), (i_db == 1) as i32, Some(b""), 0);
        rename_fix_quotes(p_parse, Some(&z_db), (i_db == 1) as i32);
        nested_parse(
            p_parse,
            b"UPDATE \"%w\".sqlite_master SET sql = sqlite_drop_column(%d, sql, %d) WHERE (type=='table' AND tbl_name=%Q COLLATE nocase)",
            &[
                FmtArg::Bytes(z_db.clone()),
                FmtArg::Int(i_db as i64),
                FmtArg::Int(i_col as i64),
                FmtArg::Bytes(p_tab.borrow().z_name.clone()),
            ],
        );

        // Descarta e recarrega o esquema
        rename_reload_schema(p_parse, i_db, INITFLAG_ALTER_DROP);
        rename_test_schema(p_parse, Some(&z_db), (i_db == 1) as i32, Some(b"after drop column"), 1);

        // Edita as linhas da tabela em disco
        if p_parse.n_err == 0 && p_tab.borrow().a_col[i_col as usize].col_flags & COLFLAG_VIRTUAL == 0 {
            let mut p_pk: Option<IndexRef> = None;
            let mut n_field: i32 = 0; // colunas não virtuais depois do drop
            let v = get_vdbe(p_parse);
            let i_cur = p_parse.n_tab;
            p_parse.n_tab += 1;
            open_table(p_parse, i_cur, i_db, &p_tab, OP_OPEN_WRITE);
            let addr = vdbe_add_op1(&v, OP_REWIND, i_cur);
            p_parse.n_mem += 1;
            let reg = p_parse.n_mem;
            let tab_n_col = p_tab.borrow().n_col as i32;
            if has_rowid(&p_tab.borrow()) {
                vdbe_add_op2(&v, OP_ROWID, i_cur, reg);
                p_parse.n_mem += tab_n_col;
            } else {
                let pk = primary_key_index(&p_tab.borrow()).expect("índice da chave primária");
                p_parse.n_mem += pk.borrow().n_column as i32;
                for i in 0..pk.borrow().n_key_col as i32 {
                    vdbe_add_op3(&v, OP_COLUMN, i_cur, i, reg + i + 1);
                }
                n_field = pk.borrow().n_key_col as i32;
                p_pk = Some(pk);
            }
            p_parse.n_mem += 1;
            let reg_rec = p_parse.n_mem;
            for i in 0..tab_n_col {
                let flags_i = p_tab.borrow().a_col[i as usize].col_flags;
                if i != i_col && flags_i & COLFLAG_VIRTUAL == 0 {
                    let reg_out: i32;
                    if let Some(pk) = &p_pk {
                        let i_pos = table_column_to_index(&pk.borrow(), i);
                        let i_col_pos = table_column_to_index(&pk.borrow(), i_col);
                        if i_pos < pk.borrow().n_key_col as i32 {
                            continue;
                        }
                        reg_out = reg + 1 + i_pos - (i_pos > i_col_pos) as i32;
                    } else {
                        reg_out = reg + 1 + n_field;
                    }
                    if i == p_tab.borrow().i_p_key as i32 {
                        vdbe_add_op2(&v, OP_NULL, 0, reg_out);
                    } else {
                        let aff = p_tab.borrow().a_col[i as usize].affinity;
                        if aff == SQLITE_AFF_REAL {
                            p_tab.borrow_mut().a_col[i as usize].affinity = SQLITE_AFF_NUMERIC;
                        }
                        expr_code_get_column_of_table(&v, &p_tab, i_cur, i, reg_out);
                        p_tab.borrow_mut().a_col[i as usize].affinity = aff;
                    }
                    n_field += 1;
                }
            }
            if n_field == 0 {
                // dbsqlfuzz 5f09e7bcc78b4954d06bf9f2400d7715f48d1fef
                p_parse.n_mem += 1;
                vdbe_add_op2(&v, OP_NULL, 0, reg + 1);
                n_field = 1;
            }
            vdbe_add_op3(&v, OP_MAKE_RECORD, reg + 1, n_field, reg_rec);
            if let Some(pk) = &p_pk {
                vdbe_add_op4_int(&v, OP_IDX_INSERT, i_cur, reg_rec, reg + 1, pk.borrow().n_key_col as i32);
            } else {
                vdbe_add_op3(&v, OP_INSERT, i_cur, reg_rec, reg);
            }
            vdbe_change_p5(&v, OPFLAG_SAVEPOSITION as u16);

            vdbe_add_op2(&v, OP_NEXT, i_cur, addr + 1);
            vdbe_jump_here(&v, addr);
        }
    }

    // exit_drop_column: z_col e p_src são liberados pelo Drop
    drop(z_col);
    src_list_delete(&db.borrow(), Some(p_src));
}

/// Registra as funções internas que ajudam a implementar ALTER TABLE.
pub fn alter_functions() {
    let a_alter_table_funcs: Vec<FuncDef> = vec![
        internal_function(b"sqlite_rename_column", 9, rename_column_func),
        internal_function(b"sqlite_rename_table", 7, rename_table_func),
        internal_function(b"sqlite_rename_test", 7, rename_table_test),
        internal_function(b"sqlite_drop_column", 3, drop_column_func),
        internal_function(b"sqlite_rename_quotefix", 2, rename_quotefix_func),
    ];
    insert_builtin_funcs(a_alter_table_funcs);
}

