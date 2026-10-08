// Mesclado das partes traduzidas de delete_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Embora um SrcList possa, em geral, representar várias tabelas e subconsultas
/// (como na cláusula FROM de um SELECT), neste caso ele contém o nome de uma única
/// tabela, como se encontra numa instrução INSERT, DELETE ou UPDATE. Procura essa
/// tabela na tabela de símbolos e a devolve. Registra uma mensagem de erro e devolve
/// None se o nome da tabela não for encontrado ou se ocorrer qualquer outro erro.
///
/// Os seguintes campos são inicializados em p_src:
///
///    p_src.a[0].p_tab       A tabela (objeto Table)
///    p_src.a[0].p_index     O índice do INDEXED BY, se houver um
pub fn src_list_lookup(p_parse: &mut Parse, p_src: &mut SrcList) -> Option<TableRef> {
    assert!(!p_src.a.is_empty() && p_src.n_src >= 1);
    let p_item = &mut p_src.a[0];
    let mut p_tab: Option<TableRef> = locate_table_item(p_parse, 0, p_item);
    if let Some(old_tab) = p_item.p_tab.take() {
        let db = p_parse.db.upgrade().expect("banco deve estar ativo");
        delete_table(&db, old_tab);
    }
    p_item.p_tab = p_tab.clone();
    p_item.fg.not_cte = 1;
    if let Some(tab) = p_tab.clone() {
        tab.borrow_mut().n_tab_ref += 1;
        if p_item.fg.is_indexed_by != 0 && indexed_by_lookup(p_parse, p_item) != 0 {
            p_tab = None;
        }
    }
    p_tab
}

/// Gera bytecode que informa o número de linhas modificadas por uma instrução
/// DELETE, INSERT ou UPDATE.
pub fn code_change_count(v: &mut Vdbe, reg_counter: i32, z_col_name: &[u8]) {
    vdbe_add_op0(v, OP_FKCHECK as i32);
    vdbe_add_op2(v, OP_RESULTROW as i32, reg_counter, 1);
    vdbe_set_num_cols(v, 1);
    vdbe_set_col_name(v, 0, COLNAME_NAME, Some(z_col_name), SQLITE_STATIC);
}

/// Devolve verdadeiro se a tabela p_tab é somente leitura.
///
/// Uma tabela é somente leitura se uma das condições abaixo for verdadeira:
///
///   1) É uma tabela virtual e nenhuma implementação do método xUpdate foi fornecida
///
///   2) Um trigger está sendo codificado e a tabela é virtual e SQLITE_VTAB_DIRECTONLY,
///      ou PRAGMA trusted_schema=OFF e a tabela não é SQLITE_VTAB_INNOCUOUS.
///
///   3) É uma tabela de sistema (isto é, sqlite_schema), a chamada não faz parte de
///      uma análise aninhada e a pragma writable_schema não foi especificada
///
///   4) A tabela é uma tabela sombra, a conexão está em modo defensivo e o
///      sqlite3_prepare() atual é de uma instrução SQL de nível superior.
fn vtab_is_read_only(p_parse: &mut Parse, p_tab: &TableRef) -> i32 {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let p_v_table = get_v_table(&db, &p_tab.borrow()).expect("tabela virtual sem VTable");
    let has_x_update = p_v_table
        .borrow()
        .p_mod
        .borrow()
        .p_module
        .x_update
        .is_some();
    if !has_x_update {
        return 1;
    }

    // Dentro de triggers:
    //   *  Não permite DELETE, INSERT ou UPDATE de tabelas virtuais SQLITE_VTAB_DIRECTONLY
    //   *  Só permite DELETE, INSERT ou UPDATE de tabelas virtuais não SQLITE_VTAB_INNOCUOUS
    //      se PRAGMA trusted_schema=ON.
    let e_vtab_risk = p_tab.borrow().u.vtab.p.borrow().e_vtab_risk as i32;
    let trusted = ((db.borrow().flags & SQLITE_TRUSTED_SCHEMA) != 0) as i32;
    if p_parse.p_toplevel.is_some() && e_vtab_risk > trusted {
        let z_name = p_tab.borrow().z_name.clone();
        error_msg(
            p_parse,
            b"unsafe use of virtual table \"%s\"",
            &[Value::Text(z_name)],
        );
    }
    0
}

fn tab_is_read_only(p_parse: &mut Parse, p_tab: &TableRef) -> i32 {
    if is_virtual(&p_tab.borrow()) {
        return vtab_is_read_only(p_parse, p_tab);
    }
    let tab_flags = p_tab.borrow().tab_flags;
    if (tab_flags & (TF_READONLY | TF_SHADOW)) == 0 {
        return 0;
    }
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    if (tab_flags & TF_READONLY) != 0 {
        return (writable_schema(&db) == 0 && p_parse.nested == 0) as i32;
    }
    assert!((tab_flags & TF_SHADOW) != 0);
    read_only_shadow_tables(&db) as i32
}

/// Verifica se a tabela dada é gravável.
///
/// Se p_tab não é gravável  ->  gera uma mensagem de erro e devolve 1.
/// Se p_tab é gravável mas houve outros erros -> devolve 1.
/// Se p_tab é gravável e não houve erros anteriores -> devolve 0;
pub fn is_read_only(p_parse: &mut Parse, p_tab: &TableRef, p_trigger: Option<&TriggerRef>) -> i32 {
    if tab_is_read_only(p_parse, p_tab) != 0 {
        let z_name = p_tab.borrow().z_name.clone();
        error_msg(
            p_parse,
            b"table %s may not be modified",
            &[Value::Text(z_name)],
        );
        return 1;
    }
    if is_view(&p_tab.borrow()) {
        let permitted = match p_trigger {
            None => true,
            Some(trig) => {
                let t = trig.borrow();
                t.b_returning != 0 && t.p_next.is_none()
            }
        };
        if permitted {
            let z_name = p_tab.borrow().z_name.clone();
            error_msg(
                p_parse,
                b"cannot modify %s because it is a view",
                &[Value::Text(z_name)],
            );
            return 1;
        }
    }
    0
}

/// Avalia uma view e guarda o resultado numa tabela efêmera. O argumento p_where
/// é uma cláusula WHERE opcional que restringe o conjunto de linhas da view a
/// serem adicionadas à tabela efêmera. Os argumentos p_order_by e p_limit passam
/// para o novo SELECT (o chamador não os libera depois).
pub fn materialize_view(
    p_parse: &mut Parse,
    p_view: &TableRef,
    p_where: Option<&Expr>,
    p_order_by: Option<Box<ExprList>>,
    p_limit: Option<Box<Expr>>,
    i_cur: i32,
) {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let i_db = schema_to_index(&db, p_view.borrow().p_schema.as_ref());
    let p_where = expr_dup(&db, p_where, 0);
    let mut p_from = src_list_append(p_parse, None, None, None);
    if let Some(from) = p_from.as_mut() {
        assert!(from.n_src == 1);
        from.a[0].z_name = db_str_dup(&db, &p_view.borrow().z_name);
        from.a[0].z_database = db_str_dup(
            &db,
            db.borrow().a_db[i_db as usize]
                .z_db_sname
                .as_ref()
                .expect("nome do schema ausente"),
        );
        assert!(from.a[0].fg.is_using == 0);
        // O C também afirma u3.pOn==0: o item recém criado ainda não tem ON.
    }
    let p_sel = select_new(
        p_parse,
        None,
        p_from,
        p_where,
        None,
        None,
        p_order_by,
        SF_INCLUDEHIDDEN,
        p_limit,
    );
    let mut dest = SelectDest::default();
    select_dest_init(&mut dest, SRT_EPHEMTAB, i_cur);
    select(p_parse, p_sel.as_deref(), &mut dest);
    select_delete(&db, p_sel);
}

/// Gera uma árvore de expressão que implementa as partes WHERE, ORDER BY e
/// LIMIT/OFFSET das instruções DELETE e UPDATE.
///
///     DELETE FROM table_wxyz WHERE a<5 ORDER BY a LIMIT 1;
///                            \__________________________/
///                               p_limit_where (p_in_clause)
///
/// Esta função assume a posse de p_where, p_order_by e p_limit.
pub fn limit_where(
    p_parse: &mut Parse,
    p_src: &mut SrcList,
    p_where: Option<Box<Expr>>,
    p_order_by: Option<Box<ExprList>>,
    p_limit: Option<Box<Expr>>,
    z_stmt_type: &[u8],
) -> Option<Box<Expr>> {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let mut p_lhs: Option<Box<Expr>>;
    let mut p_e_list: Option<Box<ExprList>> = None;

    // Verifica que não há um ORDER BY sem cláusula LIMIT.
    if p_order_by.is_some() && p_limit.is_none() {
        error_msg(
            p_parse,
            b"ORDER BY without LIMIT on %s",
            &[Value::Text(z_stmt_type.to_vec())],
        );
        expr_delete(&db, p_where);
        expr_list_delete(&db, p_order_by);
        return None;
    }

    // Só é preciso gerar uma expressão select se houver um termo limit/offset.
    if p_limit.is_none() {
        return p_where;
    }

    // Gera uma árvore de expressão select para impor o termo limit/offset do
    // DELETE ou UPDATE. Por exemplo:
    //   DELETE FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
    // vira:
    //   DELETE FROM table_a WHERE rowid IN (
    //     SELECT rowid FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
    //   );
    let p_tab: TableRef = p_src.a[0].p_tab.clone().expect("tabela do SrcList ausente");
    if has_rowid(&p_tab.borrow()) {
        p_lhs = p_expr(p_parse, TK_ROW as i32, None, None);
        let e = p_expr(p_parse, TK_ROW as i32, None, None);
        p_e_list = expr_list_append(p_parse, None, e);
    } else {
        let p_pk: IndexRef = primary_key_index(&p_tab).expect("chave primária ausente");
        let n_key_col = p_pk.borrow().n_key_col as i32;
        assert!(n_key_col >= 1);
        if n_key_col == 1 {
            let i_col = p_pk.borrow().ai_column[0];
            assert!(i_col >= 0 && (i_col as i32) < p_tab.borrow().n_col as i32);
            let z_name = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
            p_lhs = expr(&db, TK_ID as i32, Some(&z_name));
            let e = expr(&db, TK_ID as i32, Some(&z_name));
            p_e_list = expr_list_append(p_parse, None, e);
        } else {
            for i in 0..n_key_col {
                let i_col = p_pk.borrow().ai_column[i as usize];
                assert!(i_col >= 0 && (i_col as i32) < p_tab.borrow().n_col as i32);
                let z_name = p_tab.borrow().a_col[i_col as usize].z_cn_name.clone();
                let p = expr(&db, TK_ID as i32, Some(&z_name));
                p_e_list = expr_list_append(p_parse, p_e_list, p);
            }
            p_lhs = p_expr(p_parse, TK_VECTOR as i32, None, None);
            if let Some(lhs) = p_lhs.as_mut() {
                lhs.x.p_list = expr_list_dup(&db, p_e_list.as_deref(), 0);
            }
        }
    }

    // Duplica a cláusula FROM, pois ela é necessária tanto na árvore DELETE/UPDATE
    // quanto na subárvore SELECT.
    p_src.a[0].p_tab = None;
    let p_select_src = src_list_dup(&db, p_src, 0);
    p_src.a[0].p_tab = Some(p_tab.clone());
    if p_src.a[0].fg.is_indexed_by != 0 {
        assert!(p_src.a[0].fg.is_cte == 0);
        p_src.a[0].fg.is_indexed_by = 0;
        // O nome do INDEXED BY (u1) é liberado junto com a troca do variante.
        p_src.a[0].u1 = SrcItemU1::NRow(0);
        p_src.a[0].u2 = SrcItemU2::IBIndex(IndexRef::default());
    } else if p_src.a[0].fg.is_cte != 0 {
        if let SrcItemU2::CteUse(cte_use) = &p_src.a[0].u2 {
            cte_use.borrow_mut().n_use += 1;
        }
    }

    // Gera a árvore de expressão SELECT.
    let p_select = select_new(
        p_parse,
        p_e_list,
        p_select_src,
        p_where,
        None,
        None,
        p_order_by,
        0,
        p_limit,
    );

    // Gera agora a nova cláusula WHERE rowid IN do DELETE/UPDATE.
    let mut p_in_clause = p_expr(p_parse, TK_IN as i32, p_lhs, None);
    p_expr_add_select(p_parse, p_in_clause.as_deref_mut(), p_select);
    p_in_clause
}

/// Gera código para uma instrução DELETE FROM.
///
///     DELETE FROM table_wxyz WHERE a<5 AND b NOT NULL;
///                 \________/       \________________/
///                  p_tab_list           p_where
///
/// A função assume a posse de p_tab_list, p_where, p_order_by e p_limit e os
/// libera no fim (rótulo delete_from_cleanup do C).
pub fn delete_from(
    p_parse: &mut Parse,
    mut p_tab_list: Box<SrcList>,
    mut p_where: Option<Box<Expr>>,
    mut p_order_by: Option<Box<ExprList>>,
    mut p_limit: Option<Box<Expr>>,
) {
    let mut i_data_cur: i32 = 0;
    let mut i_idx_cur: i32 = 0;
    let mut s_context = AuthContext::default();
    let mut mem_cnt: i32 = 0;
    let mut i_pk: i32 = 0;
    let mut n_pk: i16 = 1;
    let mut i_eph_cur: i32 = 0;
    let mut i_row_set: i32 = 0;
    let mut addr_bypass: i32 = 0;
    let mut addr_loop: i32 = 0;
    let mut addr_eph_open: i32 = 0;
    let mut ai_cur_one_pass: [i32; 2] = [0; 2];
    let mut a_to_open: Option<Vec<u8>> = None;
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");

    'delete_from_cleanup: {
        if p_parse.n_err != 0 {
            break 'delete_from_cleanup;
        }
        assert!(db.borrow().malloc_failed == 0);
        assert!(p_tab_list.n_src == 1);

        // Localiza a tabela da qual queremos apagar. Ela precisa ficar numa estrutura
        // SrcList porque algumas sub-rotinas que chamaremos foram feitas para várias
        // tabelas e esperam um SrcList* em vez de um Table*.
        let p_tab: TableRef = match src_list_lookup(p_parse, &mut p_tab_list) {
            Some(t) => t,
            None => break 'delete_from_cleanup,
        };

        // Descobre se há triggers e se a tabela de onde se apaga é uma view
        let p_trigger: Option<TriggerRef> =
            triggers_exist(p_parse, &p_tab, TK_DELETE, None, None);
        let b_is_view = is_view(&p_tab.borrow());
        let mut b_complex = p_trigger.is_some() || fk_required(p_parse, &p_tab, None, 0) != 0;

        // O LIMIT/ORDER BY vira um IN (SELECT ...) quando não é view
        if !b_is_view {
            p_where = limit_where(
                p_parse,
                &mut p_tab_list,
                p_where,
                p_order_by.take(),
                p_limit.take(),
                b"DELETE",
            );
            p_order_by = None;
            p_limit = None;
        }

        // Se p_tab é de fato uma view, garante que ela foi inicializada.
        if view_get_column_names(p_parse, &p_tab) != 0 {
            break 'delete_from_cleanup;
        }

        if is_read_only(p_parse, &p_tab, p_trigger.as_ref()) != 0 {
            break 'delete_from_cleanup;
        }
        let i_db = schema_to_index(&db, p_tab.borrow().p_schema.as_ref());
        assert!(i_db < db.borrow().n_db);
        let z_tab_name = p_tab.borrow().z_name.clone();
        let z_db_name = db.borrow().a_db[i_db as usize].z_db_sname.clone();
        let rc_auth = auth_check(
            p_parse,
            SQLITE_DELETE,
            Some(&z_tab_name),
            None,
            z_db_name.as_deref(),
        );
        assert!(rc_auth == SQLITE_OK || rc_auth == SQLITE_DENY || rc_auth == SQLITE_IGNORE);
        if rc_auth == SQLITE_DENY {
            break 'delete_from_cleanup;
        }
        assert!(!b_is_view || p_trigger.is_some());

        // Atribui números de cursor à tabela e a todos os seus índices.
        assert!(p_tab_list.n_src == 1);
        let i_tab_cur = p_parse.n_tab;
        p_tab_list.a[0].i_cursor = i_tab_cur;
        p_parse.n_tab += 1;
        let mut n_idx: i32 = 0;
        let mut p_idx: Option<IndexRef> = p_tab.borrow().p_index.clone();
        while let Some(idx) = p_idx {
            p_parse.n_tab += 1;
            n_idx += 1;
            p_idx = idx.borrow().p_next.clone();
        }

        // Inicia o contexto da view
        if b_is_view {
            auth_context_push(p_parse, &mut s_context, Some(z_tab_name.clone()));
        }

        // Começa a gerar código.
        let v: VdbeRef = match get_vdbe(p_parse) {
            Some(v) => v,
            None => break 'delete_from_cleanup,
        };
        if p_parse.nested == 0 {
            vdbe_count_changes(&mut v.borrow_mut());
        }
        begin_write_operation(p_parse, b_complex as i32, i_db);

        // Se vamos apagar de uma view, materializa essa view numa tabela efêmera.
        if b_is_view {
            materialize_view(
                p_parse,
                &p_tab,
                p_where.as_deref(),
                p_order_by.take(),
                p_limit.take(),
                i_tab_cur,
            );
            i_data_cur = i_tab_cur;
            i_idx_cur = i_tab_cur;
            p_order_by = None;
            p_limit = None;
        }

        // Resolve os nomes de coluna na cláusula WHERE. O SrcList fica emprestado ao
        // NameContext durante a resolução e volta para cá em seguida.
        let mut s_nc = NameContext::default();
        s_nc.p_src_list = Some(p_tab_list);
        let rc_resolve = resolve_expr_names(&mut s_nc, p_where.as_deref_mut());
        p_tab_list = s_nc.p_src_list.take().expect("SrcList devolvido pelo NameContext");
        if rc_resolve != 0 {
            break 'delete_from_cleanup;
        }

        // Inicializa o contador de linhas apagadas, se estamos contando linhas.
        if (db.borrow().flags & SQLITE_COUNT_ROWS) != 0
            && p_parse.nested == 0
            && p_parse.p_trigger_tab.is_none()
            && p_parse.b_returning == 0
        {
            p_parse.n_mem += 1;
            mem_cnt = p_parse.n_mem;
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, mem_cnt);
        }

        // Caso especial: um DELETE sem cláusula WHERE apaga tudo. É mais simples
        // apagar a tabela inteira. Antes da versão 3.6.5 essa otimização fazia o
        // contador de linhas alteradas (devolvido pela API sqlite3_count_changes)
        // ficar incorreto.
        //
        // O termo "rc_auth==SQLITE_OK" é o
        // IMPLEMENTATION-OF: R-17228-37124 Se o código de ação é SQLITE_DELETE e o
        // callback devolve SQLITE_IGNORE, a operação DELETE prossegue mas a
        // otimização de truncamento é desabilitada e todas as linhas são apagadas
        // individualmente.
        if rc_auth == SQLITE_OK
            && p_where.is_none()
            && !b_complex
            && !is_virtual(&p_tab.borrow())
            && db.borrow().x_pre_update_callback.is_none()
        {
            assert!(!b_is_view);
            let tnum = p_tab.borrow().tnum;
            table_lock(p_parse, i_db, tnum, 1, Some(z_tab_name.clone()));
            if has_rowid(&p_tab.borrow()) {
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_CLEAR as i32,
                    tnum as i32,
                    i_db,
                    if mem_cnt != 0 { mem_cnt } else { -1 },
                    P4Value::Static(z_tab_name.clone()),
                    P4_STATIC,
                );
            }
            let mut p_idx: Option<IndexRef> = p_tab.borrow().p_index.clone();
            while let Some(idx) = p_idx {
                let ib = idx.borrow();
                assert!(match (&ib.p_schema, &p_tab.borrow().p_schema) {
                    (Some(a), Some(b)) => Weak::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                });
                if is_primary_key_index(&ib) && !has_rowid(&p_tab.borrow()) {
                    vdbe_add_op3(
                        &mut v.borrow_mut(),
                        OP_CLEAR as i32,
                        ib.tnum as i32,
                        i_db,
                        if mem_cnt != 0 { mem_cnt } else { -1 },
                    );
                } else {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_CLEAR as i32, ib.tnum as i32, i_db);
                }
                let next = ib.p_next.clone();
                drop(ib);
                p_idx = next;
            }
        } else {
            let mut wcf: u32 = WHERE_ONEPASS_DESIRED | WHERE_DUPLICATES_OK;
            if (s_nc.nc_flags & NC_SUBQUERY) != 0 {
                b_complex = true;
            }
            wcf |= if b_complex { 0 } else { WHERE_ONEPASS_MULTIROW };
            let p_pk: Option<IndexRef>;
            if has_rowid(&p_tab.borrow()) {
                // Numa tabela rowid, inicializa o RowSet como conjunto vazio
                p_pk = None;
                assert!(n_pk == 1);
                p_parse.n_mem += 1;
                i_row_set = p_parse.n_mem;
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, i_row_set);
            } else {
                // Numa tabela WITHOUT ROWID, cria uma tabela efêmera para guardar
                // todas as chaves primárias das linhas a apagar.
                let pk = primary_key_index(&p_tab).expect("chave primária ausente");
                n_pk = pk.borrow().n_key_col as i16;
                i_pk = p_parse.n_mem + 1;
                p_parse.n_mem += n_pk as i32;
                i_eph_cur = p_parse.n_tab;
                p_parse.n_tab += 1;
                addr_eph_open =
                    vdbe_add_op2(&mut v.borrow_mut(), OP_OPENEPHEMERAL as i32, i_eph_cur, n_pk as i32);
                vdbe_set_p4_key_info(p_parse, &pk);
                p_pk = Some(pk);
            }

            // Monta uma consulta que acha o rowid ou a chave primária de cada linha a
            // apagar, a partir da cláusula WHERE. A variável e_one_pass indica a
            // estratégia usada:
            //
            //  ONEPASS_OFF:    Duas passagens, com uma FIFO de rowids/chaves.
            //  ONEPASS_SINGLE: Uma passagem, no máximo uma linha apagada.
            //  ONEPASS_MULTI:  Uma passagem, qualquer número de linhas apagadas.
            let p_w_info = match where_begin(
                p_parse,
                &mut p_tab_list,
                p_where.as_deref(),
                None,
                None,
                None,
                wcf,
                i_tab_cur + 1,
            ) {
                Some(w) => w,
                None => break 'delete_from_cleanup,
            };
            let e_one_pass = where_ok_one_pass(&p_w_info, &mut ai_cur_one_pass);
            assert!(
                !is_virtual(&p_tab.borrow())
                    || e_one_pass != ONEPASS_MULTI
            );
            assert!(
                is_virtual(&p_tab.borrow())
                    || b_complex
                    || e_one_pass != ONEPASS_OFF
                    || optimization_disabled(&db.borrow(), SQLITE_ONEPASS)
            );
            if e_one_pass != ONEPASS_SINGLE {
                multi_write(p_parse);
            }
            if where_uses_deferred_seek(&p_w_info) {
                vdbe_add_op1(&mut v.borrow_mut(), OP_FINISHSEEK as i32, i_tab_cur);
            }

            // Acompanha o número de linhas a apagar
            if mem_cnt != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, mem_cnt, 1);
            }

            // Extrai o rowid ou a chave primária da linha corrente
            let mut i_key: i32;
            if let Some(pk) = p_pk.as_ref() {
                for i in 0..n_pk as i32 {
                    let i_col = pk.borrow().ai_column[i as usize];
                    assert!(i_col >= 0);
                    expr_code_get_column_of_table(
                        &mut v.borrow_mut(),
                        &p_tab,
                        i_tab_cur,
                        i_col as i32,
                        i_pk + i,
                    );
                }
                i_key = i_pk;
            } else {
                p_parse.n_mem += 1;
                i_key = p_parse.n_mem;
                expr_code_get_column_of_table(&mut v.borrow_mut(), &p_tab, i_tab_cur, -1, i_key);
            }

            let n_key: i16;
            if e_one_pass != ONEPASS_OFF {
                // No ONEPASS não é preciso guardar o rowid/chave primária. Há só
                // uma, então ela fica nos seus registros e cai direto no código de
                // exclusão.
                n_key = n_pk; // OP_Found usará uma chave desempacotada
                // (a falha de alocação de a_to_open não existe em Rust)
                let mut to_open = vec![1u8; (n_idx + 1) as usize];
                to_open.push(0);
                if ai_cur_one_pass[0] >= 0 {
                    to_open[(ai_cur_one_pass[0] - i_tab_cur) as usize] = 0;
                }
                if ai_cur_one_pass[1] >= 0 {
                    to_open[(ai_cur_one_pass[1] - i_tab_cur) as usize] = 0;
                }
                a_to_open = Some(to_open);
                if addr_eph_open != 0 {
                    vdbe_change_to_noop(&mut v.borrow_mut(), addr_eph_open);
                }
                addr_bypass = vdbe_make_label(p_parse);
            } else {
                if let Some(pk) = p_pk.as_ref() {
                    // Acrescenta a chave primária desta linha à tabela temporária
                    p_parse.n_mem += 1;
                    i_key = p_parse.n_mem;
                    n_key = 0; // Zero diz ao OP_Found para usar uma chave composta
                    let aff = index_affinity_str(&db, pk);
                    vdbe_add_op4(
                        &mut v.borrow_mut(),
                        OP_MAKERECORD as i32,
                        i_pk,
                        n_pk as i32,
                        i_key,
                        P4Value::Static(aff),
                        n_pk as i8,
                    );
                    vdbe_add_op4_int(
                        &mut v.borrow_mut(),
                        OP_IDXINSERT as i32,
                        i_eph_cur,
                        i_key,
                        i_pk,
                        n_pk as i32,
                    );
                } else {
                    // Acrescenta o rowid da linha a apagar ao RowSet
                    n_key = 1; // OP_DeferredSeek sempre usa um único rowid
                    vdbe_add_op2(&mut v.borrow_mut(), OP_ROWSETADD as i32, i_row_set, i_key);
                }
                where_end(&p_w_info);
            }

            // A menos que seja uma view, abre cursores para a tabela de onde se apaga
            // e para todos os seus índices. Numa view, o único efeito da instrução é
            // disparar os triggers INSTEAD OF.
            if !b_is_view {
                let mut i_addr_once: i32 = 0;
                if e_one_pass == ONEPASS_MULTI {
                    i_addr_once = vdbe_add_op0(&mut v.borrow_mut(), OP_ONCE as i32);
                }
                open_table_and_indices(
                    p_parse,
                    &p_tab,
                    OP_OPENWRITE as i32,
                    OPFLAG_FORDELETE,
                    i_tab_cur,
                    a_to_open.as_deref(),
                    &mut i_data_cur,
                    &mut i_idx_cur,
                );
                assert!(p_pk.is_some() || is_virtual(&p_tab.borrow()) || i_data_cur == i_tab_cur);
                assert!(
                    p_pk.is_some() || is_virtual(&p_tab.borrow()) || i_idx_cur == i_data_cur + 1
                );
                if e_one_pass == ONEPASS_MULTI {
                    vdbe_jump_here_or_pop_inst(&mut v.borrow_mut(), i_addr_once);
                }
            }

            // Monta o laço sobre os rowids/chaves primárias achados no laço da
            // cláusula WHERE acima.
            if e_one_pass != ONEPASS_OFF {
                assert!(n_key == n_pk); // OP_Found usará uma chave desempacotada
                if !is_virtual(&p_tab.borrow())
                    && a_to_open.as_ref().expect("a_to_open")[(i_data_cur - i_tab_cur) as usize]
                        != 0
                {
                    assert!(p_pk.is_some() || is_view(&p_tab.borrow()));
                    vdbe_add_op4_int(
                        &mut v.borrow_mut(),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        addr_bypass,
                        i_key,
                        n_key as i32,
                    );
                }
            } else if p_pk.is_some() {
                addr_loop = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, i_eph_cur);
                if is_virtual(&p_tab.borrow()) {
                    vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_eph_cur, 0, i_key);
                } else {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_ROWDATA as i32, i_eph_cur, i_key);
                }
                assert!(n_key == 0); // OP_Found usará uma chave composta
            } else {
                addr_loop = vdbe_add_op3(&mut v.borrow_mut(), OP_ROWSETREAD as i32, i_row_set, 0, i_key);
                assert!(n_key == 1);
            }

            // Apaga a linha
            if is_virtual(&p_tab.borrow()) {
                let p_v_tab = get_v_table(&db, &p_tab.borrow());
                vtab_make_writable(p_parse, &p_tab);
                assert!(e_one_pass == ONEPASS_OFF || e_one_pass == ONEPASS_SINGLE);
                may_abort(p_parse);
                if e_one_pass == ONEPASS_SINGLE {
                    vdbe_add_op1(&mut v.borrow_mut(), OP_CLOSE as i32, i_tab_cur);
                    if is_toplevel(p_parse) {
                        p_parse.is_multi_write = 0;
                    }
                }
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_VUPDATE as i32,
                    0,
                    1,
                    i_key,
                    P4Value::VTab(p_v_tab),
                    P4_VTAB,
                );
                vdbe_change_p5(&mut v.borrow_mut(), OE_ABORT as u16);
            } else {
                let count = (p_parse.nested == 0) as u8; // Verdadeiro para contar as mudanças
                generate_row_delete(
                    p_parse,
                    &p_tab,
                    p_trigger.as_ref(),
                    i_data_cur,
                    i_idx_cur,
                    i_key,
                    n_key,
                    count,
                    OE_DEFAULT,
                    e_one_pass as u8,
                    ai_cur_one_pass[1],
                );
            }

            // Fim do laço sobre todos os rowids/chaves primárias.
            if e_one_pass != ONEPASS_OFF {
                vdbe_resolve_label(&mut v.borrow_mut(), addr_bypass);
                where_end(&p_w_info);
            } else if p_pk.is_some() {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_eph_cur, addr_loop + 1);
                vdbe_jump_here(&mut v.borrow_mut(), addr_loop);
            } else {
                vdbe_goto(&mut v.borrow_mut(), addr_loop);
                vdbe_jump_here(&mut v.borrow_mut(), addr_loop);
            }
        } // Fim do caminho sem truncamento

        // Atualiza a tabela sqlite_sequence gravando o conteúdo dos contadores de
        // rowid máximo registrados nas inserções em tabelas autoincrement.
        if p_parse.nested == 0 && p_parse.p_trigger_tab.is_none() {
            autoincrement_end(p_parse);
        }

        // Devolve o número de linhas apagadas. Se esta rotina está gerando código
        // por causa de uma chamada a sqlite3NestedParse(), não invoca o callback.
        if mem_cnt != 0 {
            code_change_count(&mut v.borrow_mut(), mem_cnt, b"rows deleted");
        }
    }

    // delete_from_cleanup:
    auth_context_pop(&mut s_context);
    src_list_delete(&db, Some(p_tab_list));
    expr_delete(&db, p_where);
    expr_list_delete(&db, p_order_by);
    expr_delete(&db, p_limit);
    drop(a_to_open);
}


// ---- part_001.rs ----

/// Gera código VDBE que apaga uma única linha de uma única tabela. Tanto a
/// entrada original da tabela quanto todos os índices são removidos.
///
/// Pré-condições:
///
///   1.  `i_data_cur` é um cursor aberto na b-tree que é o armazenamento canônico
///       de dados da tabela (a própria tabela numa tabela rowid, ou o índice
///       PRIMARY KEY numa tabela WITHOUT ROWID).
///
///   2.  Cursores de leitura/escrita para todos os índices de `p_tab` devem estar
///       abertos como cursor `i_idx_cur+i` para o i-ésimo índice.
///
///   3.  A chave primária da linha a apagar deve estar numa sequência de `n_pk`
///       células de memória a partir de `i_pk`. Se `n_pk==0`, um registro de busca
///       formado por OP_MakeRecord está na única célula `i_pk`.
///
/// `e_mode`:
///   Pode ser ONEPASS_OFF (0), ONEPASS_SINGLE ou ONEPASS_MULTI. Se não for
///   ONEPASS_OFF, o cursor `i_data_cur` já aponta para a linha a apagar. Se for
///   ONEPASS_OFF, esta função precisa posicionar `i_data_cur` na entrada
///   identificada por `i_pk` e `n_pk` antes de ler dela.
///
///   Se for ONEPASS_MULTI, a chamada faz parte de um DELETE ONEPASS que afeta
///   várias linhas. Nesse caso, se `i_idx_no_seek` é um número de cursor válido
///   (>=0) e diferente de `i_data_cur`, sua posição deve ser preservada depois da
///   exclusão. Se `i_idx_no_seek` não é válido, preserva-se a posição de
///   `i_data_cur`.
///
/// `i_idx_no_seek`:
///   Se é um número de cursor válido (>=0) diferente de `i_data_cur`, identifica
///   um cursor de índice (dentre os que começam em `i_idx_cur`) que já aponta para
///   a entrada de índice a apagar. Esta otimização é desabilitada se há triggers
///   BEFORE, pois o corpo do trigger pode ter movido o cursor.
pub fn generate_row_delete(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    p_trigger: Option<&TriggerRef>,
    i_data_cur: i32,
    i_idx_cur: i32,
    i_pk: i32,
    n_pk: i16,
    count: u8,
    onconf: u8,
    e_mode: u8,
    mut i_idx_no_seek: i32,
) {
    let v: VdbeRef = p_parse.p_vdbe.clone().expect("Vdbe deve estar alocado");
    let mut i_old: i32 = 0;

    // Posiciona o cursor na linha a apagar. Se ela não existe mais (pode ocorrer se
    // um programa de trigger já a apagou), não tenta apagá-la nem dispara triggers
    // DELETE.
    let i_label = vdbe_make_label(p_parse);
    let op_seek: u8 = if has_rowid(&p_tab.borrow()) { OP_NOTEXISTS } else { OP_NOTFOUND };
    if e_mode as i32 == ONEPASS_OFF {
        vdbe_add_op4_int(&mut v.borrow_mut(), op_seek as i32, i_data_cur, i_label, i_pk, n_pk as i32);
    }

    // Se há triggers a disparar, aloca uma faixa de registros para as referências
    // old.* nos triggers.
    if fk_required(p_parse, p_tab, None, 0) != 0 || p_trigger.is_some() {
        // TODO do C: poderia usar registros temporários, e tentar evitar a cópia do
        // registro do rowid.
        let mut mask: u32 = match p_trigger {
            Some(t) => trigger_colmask(
                p_parse,
                t,
                None,
                0,
                TRIGGER_BEFORE | TRIGGER_AFTER,
                p_tab,
                onconf,
            ),
            None => 0,
        };
        mask |= fk_oldmask(p_parse, p_tab);
        i_old = p_parse.n_mem + 1;
        let n_col = p_tab.borrow().n_col as i32;
        p_parse.n_mem += 1 + n_col;

        // Preenche o vetor de registros da pseudo-tabela OLD.*. Esses valores serão
        // usados por todos os triggers BEFORE e AFTER existentes.
        vdbe_add_op2(&mut v.borrow_mut(), OP_COPY as i32, i_pk, i_old);
        for i_col in 0..n_col {
            if mask == 0xffffffff || (i_col <= 31 && (mask & maskbit32(i_col as u32)) != 0) {
                let kk = table_column_to_storage(&p_tab.borrow(), i_col as i16) as i32;
                expr_code_get_column_of_table(
                    &mut v.borrow_mut(),
                    p_tab,
                    i_data_cur,
                    i_col,
                    i_old + kk + 1,
                );
            }
        }

        // Invoca os programas de trigger BEFORE DELETE.
        let addr_start = vdbe_current_addr(&v.borrow());
        if let Some(t) = p_trigger {
            code_row_trigger(
                p_parse,
                t,
                TK_DELETE,
                None,
                TRIGGER_BEFORE,
                p_tab,
                i_old,
                onconf,
                i_label,
            );
        }

        // Se algum trigger BEFORE foi codificado, posiciona de novo o cursor na linha
        // a apagar: os triggers podem ter movido o cursor ou apagado a linha para a
        // qual ele apontava.
        //
        // Desabilita também a otimização i_idx_no_seek, pois o trigger BEFORE pode
        // ter movido aquele cursor.
        if addr_start < vdbe_current_addr(&v.borrow()) {
            vdbe_add_op4_int(&mut v.borrow_mut(), op_seek as i32, i_data_cur, i_label, i_pk, n_pk as i32);
            i_idx_no_seek = -1;
        }

        // Faz o processamento de FK. Esta chamada verifica que nenhuma restrição de FK
        // que se refira a esta tabela (isto é, restrições de outras tabelas) é violada
        // ao apagar esta linha.
        fk_check(p_parse, &mut p_tab.borrow_mut(), i_old, 0, None, 0);
    }

    // Apaga as entradas de índice e de tabela. Pula esta etapa se p_tab é de fato
    // uma view (nesse caso o único efeito do DELETE é disparar os triggers INSTEAD OF).
    //
    // Se `count` não é zero, esta instrução OP_Delete deve invocar o update-hook. O
    // pre-update-hook, por outro lado, deve ser invocado a menos que p_tab seja uma
    // tabela de sistema. A diferença é que o update-hook não é invocado para linhas
    // removidas por REPLACE, mas o pre-update-hook é.
    if !is_view(&p_tab.borrow()) {
        let mut p5: u8 = 0;
        generate_row_index_delete(p_parse, p_tab, i_data_cur, i_idx_cur, None, i_idx_no_seek);
        vdbe_add_op2(
            &mut v.borrow_mut(),
            OP_DELETE as i32,
            i_data_cur,
            if count != 0 { OPFLAG_NCHANGE as i32 } else { 0 },
        );
        if p_parse.nested == 0 || p_tab.borrow().z_name.eq_ignore_ascii_case(b"sqlite_stat1") {
            vdbe_append_p4(&mut v.borrow_mut(), P4Value::Table(p_tab.clone()), P4_TABLE as i32);
        }
        if e_mode as i32 != ONEPASS_OFF {
            vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_AUXDELETE as u16);
        }
        if i_idx_no_seek >= 0 && i_idx_no_seek != i_data_cur {
            vdbe_add_op1(&mut v.borrow_mut(), OP_DELETE as i32, i_idx_no_seek);
        }
        if e_mode as i32 == ONEPASS_MULTI {
            p5 |= OPFLAG_SAVEPOSITION;
        }
        vdbe_change_p5(&mut v.borrow_mut(), p5 as u16);
    }

    // Faz as operações ON CASCADE, SET NULL ou SET DEFAULT necessárias para tratar
    // as linhas (possivelmente de outras tabelas) que referenciam, por chave
    // estrangeira, a linha recém apagada.
    fk_actions(p_parse, p_tab, None, i_old, None, 0);

    // Invoca os programas de trigger AFTER DELETE.
    if let Some(t) = p_trigger {
        code_row_trigger(
            p_parse,
            t,
            TK_DELETE,
            None,
            TRIGGER_AFTER,
            p_tab,
            i_old,
            onconf,
            i_label,
        );
    }

    // Salta para cá se a linha já tinha sido apagada antes de qualquer programa de
    // trigger BEFORE ser invocado, ou se um programa de trigger lança uma exceção
    // RAISE(IGNORE).
    vdbe_resolve_label(&mut v.borrow_mut(), i_label);
}

/// Gera código VDBE que apaga todas as entradas de índice associadas a uma única
/// linha de uma única tabela, `p_tab`.
///
/// Pré-condições:
///
///   1.  Um cursor de leitura/escrita `i_data_cur` deve estar aberto na b-tree de
///       armazenamento canônico da tabela `p_tab` (a própria tabela nas tabelas
///       rowid, ou o índice da chave primária nas WITHOUT ROWID).
///
///   2.  Cursores de leitura/escrita para todos os índices de `p_tab` devem estar
///       abertos como cursor `i_idx_cur+i` para o i-ésimo índice (o índice
///       `p_tab.p_index` é o índice 0).
///
///   3.  O cursor `i_data_cur` já deve estar posicionado na linha a apagar.
pub fn generate_row_index_delete(
    p_parse: &mut Parse,
    p_tab: &TableRef,
    i_data_cur: i32,
    i_idx_cur: i32,
    a_reg_idx: Option<&[i32]>,
    i_idx_no_seek: i32,
) {
    let mut r1: i32 = -1;
    let mut p_prior: Option<IndexRef> = None;
    let v: VdbeRef = p_parse.p_vdbe.clone().expect("Vdbe deve estar alocado");
    let p_pk: Option<IndexRef> = if has_rowid(&p_tab.borrow()) {
        None
    } else {
        primary_key_index(p_tab)
    };
    let mut i: i32 = 0;
    let mut p_idx: Option<IndexRef> = p_tab.borrow().p_index.clone();
    while let Some(idx) = p_idx {
        let is_pk = match &p_pk {
            Some(pk) => Rc::ptr_eq(pk, &idx),
            None => false,
        };
        let next = idx.borrow().p_next.clone();
        assert!(i_idx_cur + i != i_data_cur || is_pk);
        let skip = match a_reg_idx {
            Some(a) => a[i as usize] == 0,
            None => false,
        };
        if skip || is_pk || i_idx_cur + i == i_idx_no_seek {
            i += 1;
            p_idx = next;
            continue;
        }
        let mut i_part_idx_label: i32 = 0;
        r1 = generate_index_key(
            p_parse,
            &idx,
            i_data_cur,
            0,
            1,
            Some(&mut i_part_idx_label),
            p_prior.as_ref(),
            r1,
        );
        let (uniq_not_null, n_key_col, n_column) = {
            let ib = idx.borrow();
            (ib.uniq_not_null, ib.n_key_col as i32, ib.n_column as i32)
        };
        vdbe_add_op3(
            &mut v.borrow_mut(),
            OP_IDXDELETE as i32,
            i_idx_cur + i,
            r1,
            if uniq_not_null { n_key_col } else { n_column },
        );
        vdbe_change_p5(&mut v.borrow_mut(), 1); // Faz o IdxDelete falhar se não achar a entrada
        resolve_part_idx_label(p_parse, i_part_idx_label);
        p_prior = Some(idx);
        i += 1;
        p_idx = next;
    }
}

/// Gera código que monta uma chave de índice e a guarda no registro `reg_out`. A
/// chave será do índice `p_idx`, que é um índice de `p_tab`. `i_data_cur` é o
/// cursor aberto na tabela `p_tab` e apontando para a entrada a indexar. Se
/// `p_tab` é uma tabela WITHOUT ROWID, `i_data_cur` deve ser o cursor do índice
/// PRIMARY KEY.
///
/// Devolve um número de registro que é o primeiro de um bloco de registros com os
/// elementos da chave do índice. O bloco já foi desalocado quando a rotina retorna.
///
/// Se `pi_part_idx_label` não é None, preenche-o com um rótulo e salta para ele se
/// `p_idx` é um índice parcial que deve ser pulado. O rótulo deve ser resolvido com
/// `resolve_part_idx_label()`. Um índice parcial deve ser pulado se sua cláusula
/// WHERE for falsa ou nula. Se `p_idx` não é parcial, `pi_part_idx_label` recebe
/// zero, que é um rótulo vazio ignorado por `resolve_part_idx_label()`.
///
/// Os parâmetros `p_prior` e `reg_prior` implementam um cache para evitar cargas
/// de registro desnecessárias. Se `p_prior` não é None, é outro índice cuja chave
/// acabou de ser calculada no registro `reg_prior`. Se o índice atual gera sua
/// chave na mesma sequência de registros e `p_prior` e `p_idx` têm uma coluna em
/// comum, o registro dessa coluna já tem o valor correto e a carga é pulada. Isso
/// ajuda em DELETE ou INTEGRITY_CHECK numa tabela com vários índices, em especial
/// nas colunas ROWID ou PRIMARY KEY do índice.
pub fn generate_index_key(
    p_parse: &mut Parse,
    p_idx: &IndexRef,
    i_data_cur: i32,
    reg_out: i32,
    prefix_only: i32,
    pi_part_idx_label: Option<&mut i32>,
    p_prior: Option<&IndexRef>,
    reg_prior: i32,
) -> i32 {
    let v: VdbeRef = p_parse.p_vdbe.clone().expect("Vdbe deve estar alocado");
    let mut p_prior: Option<IndexRef> = p_prior.cloned();

    if let Some(label) = pi_part_idx_label {
        if p_idx.borrow().p_part_idx_where.is_some() {
            *label = vdbe_make_label(p_parse);
            p_parse.i_self_tab = i_data_cur + 1;
            expr_if_false_dup(
                p_parse,
                p_idx.borrow().p_part_idx_where.as_deref(),
                *label,
                SQLITE_JUMPIFNULL as i32,
            );
            p_parse.i_self_tab = 0;
            // Ticket a9efb42811fa41ee 2019-11-02: p_part_idx_where pode ter
            // corrompido os registros reg_prior
            p_prior = None;
        } else {
            *label = 0;
        }
    }
    let n_col: i32 = {
        let ib = p_idx.borrow();
        if prefix_only != 0 && ib.uniq_not_null {
            ib.n_key_col as i32
        } else {
            ib.n_column as i32
        }
    };
    let reg_base = get_temp_range(p_parse, n_col);
    if let Some(pr) = &p_prior {
        if reg_base != reg_prior || pr.borrow().p_part_idx_where.is_some() {
            p_prior = None;
        }
    }
    for j in 0..n_col {
        let col_j = p_idx.borrow().ai_column[j as usize];
        if let Some(pr) = &p_prior {
            let prior_col = pr.borrow().ai_column[j as usize];
            if prior_col == col_j && prior_col != XN_EXPR {
                // Esta coluna já foi calculada pelo índice anterior
                continue;
            }
        }
        expr_code_load_index_column(p_parse, p_idx, i_data_cur, j, reg_base + j);
        if col_j >= 0 {
            // Se a afinidade da coluna é REAL mas o número é inteiro, ele pode estar
            // guardado na tabela como inteiro (representação compacta) e convertido
            // para REAL por um OP_RealAffinity. Mas aqui o valor vai voltar a um
            // índice, onde deve ser convertido de novo para INTEGER. Então omite o
            // OP_RealAffinity, se ele estiver presente.
            vdbe_delete_prior_opcode(&mut v.borrow_mut(), OP_REALAFFINITY);
        }
    }
    if reg_out != 0 {
        vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, reg_base, n_col, reg_out);
    }
    release_temp_range(p_parse, reg_base, n_col);
    reg_base
}

/// Se uma chamada anterior a `generate_index_key()` gerou um rótulo de salto porque
/// o índice era parcial, esta rotina deve ser chamada para resolver esse rótulo.
pub fn resolve_part_idx_label(p_parse: &mut Parse, i_label: i32) {
    if i_label != 0 {
        let v: VdbeRef = p_parse.p_vdbe.clone().expect("Vdbe deve estar alocado");
        vdbe_resolve_label(&mut v.borrow_mut(), i_label);
    }
}

