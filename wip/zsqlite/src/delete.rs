//! `delete.c`: chunks `delete_c.000` e `delete_c.001` do SQLite 3.46.1. Rotinas chamadas pelo
//! analisador para gerar código de `DELETE FROM`: `sqlite3SrcListLookup`, `sqlite3CodeChangeCount`,
//! `sqlite3IsReadOnly`, `sqlite3MaterializeView`, `sqlite3DeleteFrom`, `sqlite3GenerateRowDelete`,
//! `sqlite3GenerateRowIndexDelete`, `sqlite3GenerateIndexKey` e `sqlite3ResolvePartIdxLabel`.
//!
//! Convenções (as mesmas de `insert.rs`, `insert2.rs` e `build3.rs`, ver CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db`
//!   some. O `Vdbe` é `parse.p_vdbe`; as funções do `vdbeaux` recebem `&mut Vdbe`.
//! - `Table` e `Index` do esquema são `Rc` imutáveis. A lista de índices é `Table.p_index`; a lista
//!   de gatilhos que o C passa como `Trigger*` (encadeada por `pNext`) é `&[Rc<Trigger>]`, e o
//!   ponteiro nulo é a fatia vazia.
//! - `sqlite3DeleteFrom` consome a `SrcList`, o WHERE, o ORDER BY e o LIMIT: os `sqlite3...Delete`
//!   do rótulo `delete_from_cleanup` são o `Drop` das variáveis locais.
//! - `SQLITE_ENABLE_UPDATE_DELETE_LIMIT` está ligada no Debian (a gramática de `DELETE` aceita
//!   ORDER BY e LIMIT): `limit_where` (`sqlite3LimitWhere`) vive aqui e `sqlite3DeleteFrom` a chama
//!   quando a tabela não é view. `pOrderBy` e `pLimit` que sobram chegam a
//!   `sqlite3MaterializeView`, como no C.
//! - `SQLITE_ENABLE_PREUPDATE_HOOK` está ligada: a otimização de truncamento só vale sem
//!   `db.x_pre_update_callback`.
//! - Ramos `SQLITE_DEBUG` (`VdbeModuleComment`, `VdbeCoverage`, `testcase`) e `TREETRACE_ENABLED`
//!   não existem.
//! - `sqlite3VdbeSetColName` precisa da conexão (limite de comprimento), que `code_change_count`
//!   não recebe: o nome da coluna é gravado direto com `mem_set_str` e o limite padrão
//!   `SQLITE_MAX_LENGTH`; o nome tem 12 bytes, então o limite nunca é atingido.
//! - `sqlite3AuthContextPush/Pop` guardam o valor salvo de `Parse.z_auth_context` num
//!   `AuthContext`; como o `AuthContext` zerado do C (`pParse==0`) faz o Pop ser um no-op, aqui o
//!   Pop só é chamado se o Push foi.

use std::rc::Rc;

// Funções de outras fatias, chamadas pelo nome determinístico (assinaturas supostas no relatório).
pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::auth::{auth_check, auth_context_pop, auth_context_push};
use crate::build::{
    delete_table, locate_table_item, primary_key_index, table_column_to_storage, table_lock,
    text_arg, writable_schema,
};
use crate::build2::{read_only_shadow_tables, view_get_column_names};
use crate::build3::{begin_write_operation, may_abort, multi_write, src_list_append};
use crate::connection::{AuthContext, Connection, Parse};
use crate::consts::{
    NC_SUBQUERY, OE_ABORT, OE_DEFAULT, ONEPASS_MULTI, ONEPASS_OFF, ONEPASS_SINGLE, OPFLAG_AUXDELETE,
    OPFLAG_FORDELETE, OPFLAG_NCHANGE, OPFLAG_SAVEPOSITION, OP_ADDIMM, OP_CLEAR, OP_CLOSE,
    OP_COLUMN, OP_COPY, OP_DELETE, OP_FINISHSEEK, OP_FKCHECK, OP_IDXDELETE, OP_IDXINSERT,
    OP_INTEGER, OP_MAKERECORD, OP_NEXT, OP_NOTEXISTS, OP_NOTFOUND, OP_NULL, OP_ONCE,
    OP_OPENEPHEMERAL, OP_OPENWRITE, OP_REALAFFINITY, OP_RESULTROW, OP_REWIND, OP_ROWDATA,
    OP_ROWSETADD, OP_ROWSETREAD, OP_VUPDATE, SF_INCLUDEHIDDEN, SQLITE_COUNT_ROWS, SQLITE_DELETE,
    SQLITE_DENY, SQLITE_JUMPIFNULL, SQLITE_OK, SQLITE_TRUSTED_SCHEMA, SRT_EPHEMTAB, TF_READONLY,
    TF_SHADOW, TK_DELETE, TK_ID, TK_IN, TK_ROW, TK_VECTOR, TRIGGER_AFTER, TRIGGER_BEFORE,
    WHERE_DUPLICATES_OK,
    WHERE_ONEPASS_DESIRED, WHERE_ONEPASS_MULTIROW, XN_EXPR,
};
use crate::expr::{
    expr, expr_dup, expr_list_append, expr_list_dup, p_expr, p_expr_add_select, src_list_dup,
};
use crate::expr_code::{expr_code_get_column_of_table, expr_code_load_index_column};
use crate::expr_code2::{expr_if_false_dup, get_temp_range, release_temp_range};
use crate::fkey::{fk_actions, fk_check, fk_oldmask, fk_required};
use crate::insert::{auto_increment_end, index_affinity_str};
use crate::mem::{mem_set_str, StrDtor, ENC_UTF8};
use crate::prepare::schema_to_index;
use crate::resolve::{name_context_new, resolve_expr_names};
use crate::select::{get_vdbe, select, select_dest_init, select_new};
use crate::select2::indexed_by_lookup;
use crate::sqlite_int::{
    Expr, ExprList, ExprX, Index, SelectDest, SrcList, SrcU1, SrcU2, Table, Trigger,
};
use crate::trigger::{code_row_trigger, trigger_colmask, triggers_exist};
use crate::util::{error_msg, str_icmp};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, append_p4, change_p4_vtab,
    change_p5, change_to_noop, delete_prior_opcode, jump_here, jump_here_or_pop_inst, make_label,
    resolve_label, set_p4_key_info, vdbe_goto,
};
use crate::vdbeaux2::set_num_cols;
use crate::vdbeaux3::vdbe_count_changes;
use crate::vtab::{get_vtable, vtab_make_writable};
use crate::where_::{
    where_begin, where_end, where_ok_one_pass, where_uses_deferred_seek, WhereInfo,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



// ---------------------------------------------------------------------------------------------
// chunk 000: SrcListLookup, CodeChangeCount, IsReadOnly, MaterializeView, DeleteFrom
// ---------------------------------------------------------------------------------------------

/// `sqlite3SrcListLookup`: um `SrcList` pode representar várias tabelas e subconsultas (como no
/// FROM de um SELECT), mas aqui contém o nome de uma única tabela, como num INSERT, DELETE ou
/// UPDATE. Procura a tabela no esquema e a devolve. Grava uma mensagem de erro e devolve `None`
/// se a tabela não é achada ou se ocorre outro erro.
///
/// Os campos inicializados em `p_src`: `a[0].p_tab` (a tabela) e `a[0].u2` (o índice do INDEXED BY,
/// se houver). O `nTabRef++` do C é o `Rc` clonado que fica em `a[0].p_tab`.
pub fn src_list_lookup(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: &mut SrcList,
) -> Option<Rc<Table>> {
    debug_assert!(!p_src.a.is_empty());
    let mut p_tab = locate_table_item(db, parse, 0, &p_src.a[0]);
    let p_old = p_src.a[0].p_tab.take();
    if p_old.is_some() {
        delete_table(db, p_old);
    }
    p_src.a[0].p_tab = p_tab.clone();
    p_src.a[0].fg.not_cte = true;
    if p_tab.is_some() && p_src.a[0].fg.is_indexed_by && indexed_by_lookup(db, parse, &mut p_src.a[0]) != 0 {
        p_tab = None;
    }
    p_tab
}

/// `sqlite3CodeChangeCount`: gera o bytecode que informa o número de linhas modificadas por um
/// DELETE, INSERT ou UPDATE.
pub fn code_change_count(v: &mut Vdbe, reg_counter: i32, z_col_name: &[u8]) {
    add_op0(v, OP_FKCHECK as i32);
    add_op2(v, OP_RESULTROW as i32, reg_counter, 1);
    set_num_cols(v, 1);
    // `sqlite3VdbeSetColName(v, 0, COLNAME_NAME, zColName, SQLITE_STATIC)`: com `COLNAME_NAME==0`
    // e uma coluna alocada, o `Mem` é o de índice 0 (ver a nota do cabeçalho sobre o limite).
    if let Some(m) = v.a_col_name.get_mut(0) {
        mem_set_str(
            m,
            Some(z_col_name),
            -1,
            ENC_UTF8,
            StrDtor::Static,
            crate::consts::SQLITE_MAX_LENGTH,
        );
    }
}

/// `vtabIsReadOnly`: verdadeiro se a tabela virtual `p_tab` é só de leitura.
fn vtab_is_read_only(db: &mut Connection, parse: &mut Parse, p_tab: &Rc<Table>) -> i32 {
    let Some((has_update, e_vtab_risk)) = get_vtable(db, p_tab)
        .and_then(|id| db.vtabs.get(id.slot()))
        .map(|vt| (vt.p_mod.p_module.caps().update, vt.e_vtab_risk))
    else {
        return 1;
    };
    if !has_update {
        return 1;
    }

    // Dentro de gatilhos:
    //   * Não se permite DELETE, INSERT ou UPDATE de tabelas virtuais SQLITE_VTAB_DIRECTONLY.
    //   * Só se permite DELETE, INSERT ou UPDATE de tabelas virtuais que não são
    //     SQLITE_VTAB_INNOCUOUS se PRAGMA trusted_schema=ON.
    if parse.p_toplevel.is_some()
        && (e_vtab_risk as i32) > (((db.flags & SQLITE_TRUSTED_SCHEMA) != 0) as i32)
    {
        error_msg(
            db,
            parse,
            b"unsafe use of virtual table \"%s\"",
            &[text_arg(&p_tab.z_name)],
        );
    }
    0
}

/// `tabIsReadOnly`.
fn tab_is_read_only(db: &mut Connection, parse: &mut Parse, p_tab: &Rc<Table>) -> i32 {
    if p_tab.is_virtual() {
        return vtab_is_read_only(db, parse, p_tab);
    }
    if (p_tab.tab_flags & (TF_READONLY | TF_SHADOW)) == 0 {
        return 0;
    }
    if (p_tab.tab_flags & TF_READONLY) != 0 {
        return (!writable_schema(db) && parse.nested == 0) as i32;
    }
    debug_assert!((p_tab.tab_flags & TF_SHADOW) != 0);
    read_only_shadow_tables(db) as i32
}

/// `sqlite3IsReadOnly`: confere se a tabela dada é gravável.
///
/// Se `p_tab` não é gravável gera uma mensagem de erro e devolve 1. Se é gravável mas houve outros
/// erros, devolve 1. Se é gravável e não houve erros antes, devolve 0.
pub fn is_read_only(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_trigger: &[Rc<Trigger>],
) -> i32 {
    if tab_is_read_only(db, parse, p_tab) != 0 {
        error_msg(db, parse, b"table %s may not be modified", &[text_arg(&p_tab.z_name)]);
        return 1;
    }
    if p_tab.is_view()
        && (p_trigger.is_empty() || (p_trigger[0].b_returning != 0 && p_trigger.len() == 1))
    {
        error_msg(
            db,
            parse,
            b"cannot modify %s because it is a view",
            &[text_arg(&p_tab.z_name)],
        );
        return 1;
    }
    0
}

/// `sqlite3MaterializeView`: avalia uma view e guarda o resultado numa tabela efêmera. O argumento
/// `p_where` é uma cláusula WHERE opcional que restringe o conjunto de linhas da view que vão para
/// a tabela efêmera.
pub(crate) fn materialize_view(
    db: &mut Connection,
    parse: &mut Parse,
    p_view: &Rc<Table>,
    p_where: Option<&Expr>,
    p_order_by: Option<Box<ExprList>>,
    p_limit: Option<Box<Expr>>,
    i_cur: i32,
) {
    let i_db = schema_to_index(db, p_view.p_schema);
    let p_where = expr_dup(p_where, 0);
    let mut p_from = src_list_append(db, parse, None, None, None);
    if let Some(f) = p_from.as_deref_mut() {
        debug_assert!(f.a.len() == 1);
        f.a[0].z_name = Some(p_view.z_name.clone());
        f.a[0].z_database = Some(db.dbs[i_db as usize].z_db_s_name.clone());
        debug_assert!(!f.a[0].fg.is_using);
    }
    let p_sel = select_new(
        parse,
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
    select_dest_init(&mut dest, SRT_EPHEMTAB as i32, i_cur);
    if let Some(mut sel) = p_sel {
        select(db, parse, &mut sel, &mut dest);
    }
}

/// `sqlite3LimitWhere`: gera uma árvore de expressão que implementa a parte WHERE, ORDER BY e
/// LIMIT/OFFSET dos comandos DELETE e UPDATE.
///
/// ```text
///     DELETE FROM table_wxyz WHERE a<5 ORDER BY a LIMIT 1;
///                            \__________________________/
///                               pLimitWhere (pInClause)
/// ```
///
/// `z_stmt_type` é `DELETE` ou `UPDATE`, para as mensagens de erro.
pub(crate) fn limit_where(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: &mut SrcList, // a cláusula FROM: quais tabelas varrer
    p_where: Option<Box<Expr>>, // a cláusula WHERE; pode ser nula
    p_order_by: Option<Box<ExprList>>, // a cláusula ORDER BY; pode ser nula
    p_limit: Option<Box<Expr>>, // a cláusula LIMIT; pode ser nula
    z_stmt_type: &[u8],
) -> Option<Box<Expr>> {
    // Confere que não há ORDER BY sem LIMIT.
    if p_order_by.is_some() && p_limit.is_none() {
        error_msg(db, parse, b"ORDER BY without LIMIT on %s", &[text_arg(z_stmt_type)]);
        return None;
    }

    // Só precisamos gerar uma expressão select se há um termo limit/offset a impor.
    if p_limit.is_none() {
        return p_where;
    }

    // Gera uma árvore de expressão select que impõe o termo limit/offset do DELETE ou UPDATE.
    // Por exemplo:
    //   DELETE FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
    // vira:
    //   DELETE FROM table_a WHERE rowid IN (
    //     SELECT rowid FROM table_a WHERE col1=1 ORDER BY col2 LIMIT 1 OFFSET 1
    //   );
    let p_tab = p_src.a[0].p_tab.clone()?;
    let p_lhs: Option<Box<Expr>>; // lado esquerdo do operador IN(SELECT...)
    let mut p_e_list: Option<Box<ExprList>> = None; // lista só com o rowid ou a chave primária
    if p_tab.has_rowid() {
        p_lhs = p_expr(db, parse, TK_ROW as i32, None, None);
        let p_row = p_expr(db, parse, TK_ROW as i32, None, None);
        p_e_list = expr_list_append(None, p_row);
    } else {
        let p_pk = primary_key_index(&p_tab)?;
        debug_assert!(p_pk.n_key_col >= 1);
        if p_pk.n_key_col == 1 {
            debug_assert!(p_pk.ai_column[0] >= 0 && (p_pk.ai_column[0] as usize) < p_tab.a_col.len());
            let z_name = &p_tab.a_col[p_pk.ai_column[0] as usize].z_cn_name;
            p_lhs = expr(TK_ID as i32, Some(z_name));
            p_e_list = expr_list_append(None, expr(TK_ID as i32, Some(z_name)));
        } else {
            for i in 0..p_pk.n_key_col as usize {
                debug_assert!(p_pk.ai_column[i] >= 0 && (p_pk.ai_column[i] as usize) < p_tab.a_col.len());
                let p = expr(TK_ID as i32, Some(&p_tab.a_col[p_pk.ai_column[i] as usize].z_cn_name));
                p_e_list = expr_list_append(p_e_list, p);
            }
            let mut p_vec = p_expr(db, parse, TK_VECTOR as i32, None, None);
            if let Some(l) = p_vec.as_deref_mut() {
                l.x = match expr_list_dup(p_e_list.as_deref(), 0) {
                    Some(list) => ExprX::List(list),
                    None => ExprX::None,
                };
            }
            p_lhs = p_vec;
        }
    }

    // Duplica a cláusula FROM, pois ela é necessária tanto pela árvore do DELETE/UPDATE quanto
    // pela subárvore do SELECT. O `pTab` fica de fora da cópia (o C zera `a[0].pTab` antes).
    let p_tab_saved = p_src.a[0].p_tab.take();
    let p_select_src = src_list_dup(Some(&*p_src), 0);
    p_src.a[0].p_tab = p_tab_saved;
    if p_src.a[0].fg.is_indexed_by {
        debug_assert!(!p_src.a[0].fg.is_cte);
        p_src.a[0].u2 = SrcU2::IbIndex(None);
        p_src.a[0].fg.is_indexed_by = false;
        // `sqlite3DbFree(db, pSrc->a[0].u1.zIndexedBy)`: o nome é solto ao trocar o `u1`.
        p_src.a[0].u1 = SrcU1::NRow(0);
    } else if p_src.a[0].fg.is_cte {
        if let SrcU2::CteUse(Some(id)) = &p_src.a[0].u2 {
            parse.cte_uses[id.0 as usize].n_use += 1;
        }
    }

    // Gera a árvore de expressão SELECT.
    let p_select = select_new(parse, p_e_list, p_select_src, p_where, None, None, p_order_by, 0, p_limit);

    // Gera a nova cláusula `WHERE rowid IN` para o DELETE/UPDATE.
    let mut p_in_clause = p_expr(db, parse, TK_IN as i32, p_lhs, None);
    p_expr_add_select(db, parse, p_in_clause.as_deref_mut(), p_select);
    p_in_clause
}

/// `sqlite3DeleteFrom`: gera o código de um comando `DELETE FROM`.
///
/// ```text
///     DELETE FROM table_wxyz WHERE a<5 AND b NOT NULL;
///                 \________/       \________________/
///                  p_tab_list            p_where
/// ```
pub fn delete_from(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: Option<Box<SrcList>>,
    mut p_where: Option<Box<Expr>>,
    mut p_order_by: Option<Box<ExprList>>,
    mut p_limit: Option<Box<Expr>>,
) {
    let Some(mut p_tab_list) = p_tab_list else {
        return;
    };
    let mut s_context = AuthContext::default(); // contexto de autorização
    let mut ctx_pushed = false; // o `sContext.pParse` do C é não nulo

    'cleanup: {
        if parse.n_err != 0 {
            break 'cleanup;
        }
        debug_assert!(p_tab_list.a.len() == 1);

        // Localiza a tabela de onde apagar. Ela precisa estar numa `SrcList` porque algumas das
        // sub-rotinas chamadas aceitam várias tabelas e esperam um `SrcList*` em vez de `Table*`.
        let Some(mut p_tab) = src_list_lookup(db, parse, &mut p_tab_list) else {
            break 'cleanup;
        };

        // Descobre se há gatilhos e se a tabela de que se apaga é uma view.
        let mut tmask: i32 = 0;
        let p_trigger: Vec<Rc<Trigger>> =
            triggers_exist(db, parse, &p_tab, TK_DELETE as i32, None, &mut tmask);
        let is_view = p_tab.is_view();
        let mut b_complex = !p_trigger.is_empty() || fk_required(db, parse, &p_tab, None, 0) != 0;

        // `SQLITE_ENABLE_UPDATE_DELETE_LIMIT`: o ORDER BY e o LIMIT viram um `rowid IN (SELECT ...)`
        // no WHERE; numa view eles seguem para `sqlite3MaterializeView`.
        if !is_view {
            p_where = limit_where(
                db,
                parse,
                &mut p_tab_list,
                p_where.take(),
                p_order_by.take(),
                p_limit.take(),
                b"DELETE",
            );
        }

        // Se `p_tab` é uma view, garante que foi inicializada.
        if view_get_column_names(db, parse, &mut p_tab) != 0 {
            break 'cleanup;
        }

        if is_read_only(db, parse, &p_tab, &p_trigger) != 0 {
            break 'cleanup;
        }
        let i_db = schema_to_index(db, p_tab.p_schema);
        debug_assert!((i_db as usize) < db.dbs.len());
        let z_db_s_name = db.dbs[i_db as usize].z_db_s_name.clone();
        let rcauth = auth_check(db, parse, SQLITE_DELETE, Some(&p_tab.z_name), None, Some(&z_db_s_name));
        debug_assert!(rcauth == SQLITE_OK || rcauth == SQLITE_DENY || rcauth == crate::consts::SQLITE_IGNORE);
        if rcauth == SQLITE_DENY {
            break 'cleanup;
        }
        debug_assert!(!is_view || !p_trigger.is_empty());

        // Atribui números de cursor à tabela e a todos os índices dela.
        debug_assert!(p_tab_list.a.len() == 1);
        let i_tab_cur = parse.n_tab;
        parse.n_tab += 1;
        p_tab_list.a[0].i_cursor = i_tab_cur;
        let mut n_idx: usize = 0; // número de índices
        for _ in p_tab.p_index.iter() {
            parse.n_tab += 1;
            n_idx += 1;
        }

        // Começa o contexto da view.
        if is_view {
            auth_context_push(parse, &mut s_context, &p_tab.z_name);
            ctx_pushed = true;
        }

        // Começa a gerar código.
        get_vdbe(db, parse);
        if parse.p_vdbe.is_none() {
            break 'cleanup;
        }
        if parse.nested == 0 {
            vdbe_count_changes(vdbe_of_parse(parse));
        }
        begin_write_operation(db, parse, b_complex as i32, i_db);

        // Se vamos apagar de uma view, a materializa numa tabela efêmera.
        let mut i_data_cur: i32 = 0; // cursor do VDBE da fonte canônica dos dados
        let mut i_idx_cur: i32 = 0; // número do cursor do primeiro índice
        if is_view {
            materialize_view(
                db,
                parse,
                &p_tab,
                p_where.as_deref(),
                p_order_by.take(),
                p_limit.take(),
                i_tab_cur,
            );
            i_data_cur = i_tab_cur;
            i_idx_cur = i_tab_cur;
        }

        // Resolve os nomes de coluna na cláusula WHERE.
        let (r_resolve, nc_flags) = {
            let mut s_nc = name_context_new();
            s_nc.p_src_list = Some(&mut *p_tab_list);
            let r = resolve_expr_names(db, parse, &mut s_nc, p_where.as_deref_mut());
            (r, s_nc.nc_flags)
        };
        if r_resolve != 0 {
            break 'cleanup;
        }

        // Inicializa o contador de linhas apagadas, se estamos contando linhas.
        let mut mem_cnt: i32 = 0; // célula de memória usada para contar mudanças
        if (db.flags & SQLITE_COUNT_ROWS) != 0
            && parse.nested == 0
            && parse.p_trigger_tab.is_none()
            && parse.b_returning == 0
        {
            parse.n_mem += 1;
            mem_cnt = parse.n_mem;
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, mem_cnt);
        }

        // Caso especial: um DELETE sem WHERE apaga tudo. É mais fácil simplesmente apagar a tabela
        // inteira. Antes da versão 3.6.5 essa otimização fazia a contagem de linhas modificadas
        // (o valor devolvido por `sqlite3_count_changes`) ficar errada.
        //
        // O termo `rcauth==SQLITE_OK` é o IMPLEMENTATION-OF: R-17228-37124: se o código da ação é
        // SQLITE_DELETE e o callback devolve SQLITE_IGNORE, o DELETE prossegue mas a otimização
        // de truncamento fica desligada e todas as linhas são apagadas uma a uma.
        let mut a_to_open: Option<Vec<u8>> = None; // abre o cursor iTabCur+j se `a_to_open[j]`
        if rcauth == SQLITE_OK
            && p_where.is_none()
            && !b_complex
            && !p_tab.is_virtual()
            && db.x_pre_update_callback.is_none()
        {
            debug_assert!(!is_view);
            table_lock(db, parse, i_db, p_tab.tnum, true, &p_tab.z_name);
            let p3_clear = if mem_cnt != 0 { mem_cnt } else { -1 };
            if p_tab.has_rowid() {
                add_op4(
                    vdbe_of_parse(parse),
                    OP_CLEAR as i32,
                    p_tab.tnum as i32,
                    i_db,
                    p3_clear,
                    P4::Text(p_tab.z_name.clone()),
                );
            }
            for p_idx in p_tab.p_index.iter() {
                if p_idx.is_primary_key_index() && !p_tab.has_rowid() {
                    add_op3(vdbe_of_parse(parse), OP_CLEAR as i32, p_idx.tnum as i32, i_db, p3_clear);
                } else {
                    add_op2(vdbe_of_parse(parse), OP_CLEAR as i32, p_idx.tnum as i32, i_db);
                }
            }
        } else {
            let mut wcf: u16 = (WHERE_ONEPASS_DESIRED | WHERE_DUPLICATES_OK) as u16;
            if (nc_flags & NC_SUBQUERY) != 0 {
                b_complex = true;
            }
            wcf |= if b_complex { 0 } else { WHERE_ONEPASS_MULTIROW as u16 };

            let p_pk: Option<Rc<Index>>; // o índice PRIMARY KEY da tabela
            let mut i_pk: i32 = 0; // primeiro dos `n_pk` registradores com o valor da PRIMARY KEY
            let mut n_pk: i32 = 1; // número de colunas da PRIMARY KEY
            let mut i_eph_cur: i32 = 0; // tabela efêmera com todos os valores de PRIMARY KEY
            let mut i_row_set: i32 = 0; // registrador do rowset das linhas a apagar
            let mut addr_eph_open: i32 = 0; // instrução que abre a tabela efêmera
            if p_tab.has_rowid() {
                // Numa tabela com rowid, inicializa o RowSet vazio.
                p_pk = None;
                debug_assert!(n_pk == 1);
                parse.n_mem += 1;
                i_row_set = parse.n_mem;
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, i_row_set);
            } else {
                // Numa tabela WITHOUT ROWID, cria uma tabela efêmera que guarda todas as chaves
                // primárias das linhas a apagar.
                let Some(pk) = primary_key_index(&p_tab).cloned() else {
                    break 'cleanup;
                };
                n_pk = pk.n_key_col as i32;
                i_pk = parse.n_mem + 1;
                parse.n_mem += n_pk;
                i_eph_cur = parse.n_tab;
                parse.n_tab += 1;
                addr_eph_open = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, i_eph_cur, n_pk);
                set_p4_key_info(parse, db, &pk);
                p_pk = Some(pk);
            }

            // Monta uma consulta que acha o rowid ou a chave primária de cada linha a apagar, a
            // partir do WHERE. `e_one_pass` indica a estratégia do delete:
            //
            //  ONEPASS_OFF:    duas passadas, com uma FIFO de rowids/valores de PK.
            //  ONEPASS_SINGLE: uma passada, no máximo uma linha apagada.
            //  ONEPASS_MULTI:  uma passada, qualquer número de linhas apagadas.
            let mut p_w_info: Option<Box<WhereInfo>> = where_begin(
                db,
                parse,
                &mut p_tab_list,
                p_where.as_deref_mut(),
                None,
                None,
                None,
                wcf,
                i_tab_cur + 1,
            );
            if p_w_info.is_none() {
                break 'cleanup;
            }
            let mut ai_cur_one_pass: [i32; 2] = [0; 2]; // cursores de escrita do WHERE_ONEPASS
            let e_one_pass: i32 = match p_w_info.as_deref() {
                Some(w) => where_ok_one_pass(w, &mut ai_cur_one_pass),
                None => ONEPASS_OFF,
            };
            debug_assert!(!p_tab.is_virtual() || e_one_pass != ONEPASS_MULTI);
            if e_one_pass != ONEPASS_SINGLE {
                multi_write(parse);
            }
            if p_w_info.as_deref().map_or(false, where_uses_deferred_seek) {
                add_op1(vdbe_of_parse(parse), OP_FINISHSEEK as i32, i_tab_cur);
            }

            // Conta o número de linhas a apagar.
            if mem_cnt != 0 {
                add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, mem_cnt, 1);
            }

            // Extrai o rowid ou a chave primária da linha corrente.
            let mut i_key: i32; // célula de memória com a chave da linha a apagar
            if let Some(pk) = &p_pk {
                for i in 0..n_pk {
                    debug_assert!(pk.ai_column[i as usize] >= 0);
                    expr_code_get_column_of_table(
                        db,
                        parse,
                        &p_tab,
                        i_tab_cur,
                        pk.ai_column[i as usize] as i32,
                        i_pk + i,
                    );
                }
                i_key = i_pk;
            } else {
                parse.n_mem += 1;
                i_key = parse.n_mem;
                expr_code_get_column_of_table(db, parse, &p_tab, i_tab_cur, -1, i_key);
            }

            let n_key: i32; // número de células de memória da chave da linha
            let mut addr_bypass: i32 = 0; // endereço do salto sobre a lógica de delete
            if e_one_pass != ONEPASS_OFF {
                // Com ONEPASS não é preciso guardar o rowid/chave primária: há só um, então fica
                // nos registradores e cai no código do delete.
                n_key = n_pk; // OP_Found usará uma chave desempacotada
                let mut a = vec![1u8; n_idx + 2];
                a[n_idx + 1] = 0;
                if ai_cur_one_pass[0] >= 0 {
                    a[(ai_cur_one_pass[0] - i_tab_cur) as usize] = 0;
                }
                if ai_cur_one_pass[1] >= 0 {
                    a[(ai_cur_one_pass[1] - i_tab_cur) as usize] = 0;
                }
                a_to_open = Some(a);
                if addr_eph_open != 0 {
                    change_to_noop(vdbe_of_parse(parse), db, addr_eph_open);
                }
                addr_bypass = make_label(parse);
            } else {
                if let Some(pk) = &p_pk {
                    // Acrescenta a chave primária desta linha à tabela temporária.
                    parse.n_mem += 1;
                    i_key = parse.n_mem;
                    n_key = 0; // zero diz ao OP_Found para usar uma chave composta
                    let mut z_aff = index_affinity_str(pk, &p_tab);
                    z_aff.truncate(n_pk as usize);
                    let v = vdbe_of_parse(parse);
                    add_op4(v, OP_MAKERECORD as i32, i_pk, n_pk, i_key, P4::Text(z_aff));
                    add_op4_int(v, OP_IDXINSERT as i32, i_eph_cur, i_key, i_pk, n_pk);
                } else {
                    // Acrescenta o rowid da linha a apagar ao RowSet.
                    n_key = 1; // OP_DeferredSeek sempre usa um único rowid
                    add_op2(vdbe_of_parse(parse), OP_ROWSETADD as i32, i_row_set, i_key);
                }
                if let Some(w) = p_w_info.take() {
                    where_end(db, parse, &p_tab_list, w);
                }
            }

            // A menos que seja uma view, abre cursores para a tabela de que se apaga e todos os
            // índices dela. Se é uma view, o único efeito do comando é disparar os gatilhos
            // INSTEAD OF.
            if !is_view {
                let mut i_addr_once: i32 = 0;
                if e_one_pass == ONEPASS_MULTI {
                    i_addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
                }
                crate::insert2::open_table_and_indices(
                    db,
                    parse,
                    &p_tab,
                    OP_OPENWRITE,
                    OPFLAG_FORDELETE,
                    i_tab_cur,
                    a_to_open.as_deref(),
                    &mut i_data_cur,
                    &mut i_idx_cur,
                );
                debug_assert!(p_pk.is_some() || p_tab.is_virtual() || i_data_cur == i_tab_cur);
                debug_assert!(p_pk.is_some() || p_tab.is_virtual() || i_idx_cur == i_data_cur + 1);
                if e_one_pass == ONEPASS_MULTI {
                    jump_here_or_pop_inst(vdbe_of_parse(parse), i_addr_once);
                }
            }

            // Monta um laço sobre os rowids/chaves primárias achados no laço do WHERE acima.
            let mut addr_loop: i32 = 0; // topo do laço de delete
            if e_one_pass != ONEPASS_OFF {
                debug_assert!(n_key == n_pk); // OP_Found usará uma chave desempacotada
                let to_open = a_to_open
                    .as_deref()
                    .map_or(false, |a| a[(i_data_cur - i_tab_cur) as usize] != 0);
                if !p_tab.is_virtual() && to_open {
                    debug_assert!(p_pk.is_some() || p_tab.is_view());
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        addr_bypass,
                        i_key,
                        n_key,
                    );
                }
            } else if p_pk.is_some() {
                let v = vdbe_of_parse(parse);
                addr_loop = add_op1(v, OP_REWIND as i32, i_eph_cur);
                if p_tab.is_virtual() {
                    add_op3(v, OP_COLUMN as i32, i_eph_cur, 0, i_key);
                } else {
                    add_op2(v, OP_ROWDATA as i32, i_eph_cur, i_key);
                }
                debug_assert!(n_key == 0); // OP_Found usará uma chave composta
            } else {
                addr_loop = add_op3(vdbe_of_parse(parse), OP_ROWSETREAD as i32, i_row_set, 0, i_key);
                debug_assert!(n_key == 1);
            }

            // Apaga a linha.
            if p_tab.is_virtual() {
                let p_vtab = get_vtable(db, &p_tab);
                vtab_make_writable(db, parse, &p_tab);
                debug_assert!(e_one_pass == ONEPASS_OFF || e_one_pass == ONEPASS_SINGLE);
                may_abort(parse);
                if e_one_pass == ONEPASS_SINGLE {
                    add_op1(vdbe_of_parse(parse), OP_CLOSE as i32, i_tab_cur);
                    if parse.p_toplevel.is_none() {
                        parse.is_multi_write = 0;
                    }
                }
                let a = add_op3(vdbe_of_parse(parse), OP_VUPDATE as i32, 0, 1, i_key);
                if let Some(id) = p_vtab {
                    change_p4_vtab(vdbe_of_parse(parse), db, a, id);
                }
                change_p5(vdbe_of_parse(parse), OE_ABORT as u16);
            } else {
                let count: u8 = (parse.nested == 0) as u8; // verdadeiro para contar mudanças
                generate_row_delete(
                    db,
                    parse,
                    &p_tab,
                    &p_trigger,
                    i_data_cur,
                    i_idx_cur,
                    i_key,
                    n_key,
                    count,
                    OE_DEFAULT,
                    e_one_pass,
                    ai_cur_one_pass[1],
                );
            }

            // Fim do laço sobre todos os rowids/chaves primárias.
            if e_one_pass != ONEPASS_OFF {
                resolve_label(parse, db, addr_bypass);
                if let Some(w) = p_w_info.take() {
                    where_end(db, parse, &p_tab_list, w);
                }
            } else if p_pk.is_some() {
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_NEXT as i32, i_eph_cur, addr_loop + 1);
                jump_here(v, addr_loop);
            } else {
                let v = vdbe_of_parse(parse);
                vdbe_goto(v, addr_loop);
                jump_here(v, addr_loop);
            }
        } // fim do caminho sem truncamento

        // Atualiza a tabela sqlite_sequence guardando o conteúdo dos contadores de rowid máximo
        // registrados durante os inserts em tabelas autoincrement.
        if parse.nested == 0 && parse.p_trigger_tab.is_none() {
            auto_increment_end(db, parse);
        }

        // Devolve o número de linhas apagadas. Se esta rotina gera código por causa de um
        // `sqlite3NestedParse()`, não chama a função de callback.
        if mem_cnt != 0 {
            code_change_count(vdbe_of_parse(parse), mem_cnt, b"rows deleted");
        }
    }

    // delete_from_cleanup:
    if ctx_pushed {
        auth_context_pop(parse, &mut s_context);
    }
    // `sqlite3SrcListDelete`, `sqlite3ExprDelete` e o `sqlite3DbNNFreeNN(aToOpen)` do C são o
    // `Drop` de `p_tab_list`, `p_where`, `p_order_by`, `p_limit` e `a_to_open`.
}

// ---------------------------------------------------------------------------------------------
// chunk 001: GenerateRowDelete, GenerateRowIndexDelete, GenerateIndexKey, ResolvePartIdxLabel
// ---------------------------------------------------------------------------------------------

/// `sqlite3GenerateRowDelete`: gera código do VDBE que apaga uma linha de uma tabela. Removem-se a
/// entrada da tabela original e todas as dos índices.
///
/// Pré-condições:
///
///   1. `i_data_cur` é um cursor aberto na árvore que é o repositório canônico dos dados da tabela
///      (a própria tabela, num rowid; o índice da PRIMARY KEY, numa WITHOUT ROWID).
///   2. Cursores de leitura e escrita de todos os índices de `p_tab` estão abertos como o cursor
///      `i_idx_cur+i` para o i-ésimo índice.
///   3. A chave primária da linha a apagar está numa sequência de `n_pk` células de memória a
///      partir de `i_pk`. Se `n_pk==0`, o registro de busca montado por `OP_MakeRecord` está na
///      única célula `i_pk`.
///
/// `e_mode` é `ONEPASS_OFF`, `ONEPASS_SINGLE` ou `ONEPASS_MULTI`. Se não é `ONEPASS_OFF`, o cursor
/// `i_data_cur` já aponta para a linha a apagar; se é, a função precisa posicioná-lo na entrada
/// identificada por `i_pk` e `n_pk` antes de lê-la.
///
/// Com `ONEPASS_MULTI` (delete de uma passada que afeta várias linhas), se `i_idx_no_seek` é um
/// cursor válido (>=0) diferente de `i_data_cur`, a posição dele deve ser preservada depois do
/// delete; senão, a posição preservada é a de `i_data_cur`.
///
/// `i_idx_no_seek`, se é um cursor válido (>=0) diferente de `i_data_cur`, identifica um cursor de
/// índice (dentre os que começam em `i_idx_cur`) que já aponta para a entrada de índice a apagar.
/// A exceção é a otimização desligada quando há gatilhos BEFORE, porque o corpo do gatilho pode ter
/// movido o cursor.
pub fn generate_row_delete(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_trigger: &[Rc<Trigger>],
    i_data_cur: i32,
    i_idx_cur: i32,
    i_pk: i32,
    n_pk: i32,
    count: u8,
    onconf: u8,
    e_mode: i32,
    i_idx_no_seek: i32,
) {
    let mut i_idx_no_seek = i_idx_no_seek;
    let mut i_old: i32 = 0; // primeiro registrador do array OLD.*

    // O Vdbe já foi alocado a esta altura.
    debug_assert!(parse.p_vdbe.is_some());

    // Posiciona o cursor na linha a apagar. Se ela não existe mais (pode acontecer se um programa
    // de gatilho já a apagou), não tenta apagá-la nem dispara gatilhos DELETE.
    let i_label = make_label(parse); // rótulo resolvido no fim do código gerado
    let op_seek: u8 = if p_tab.has_rowid() { OP_NOTEXISTS } else { OP_NOTFOUND };
    if e_mode == ONEPASS_OFF {
        add_op4_int(vdbe_of_parse(parse), op_seek as i32, i_data_cur, i_label, i_pk, n_pk);
    }

    // Se há gatilhos a disparar, aloca um intervalo de registradores para as referências old.* dos
    // gatilhos.
    if fk_required(db, parse, p_tab, None, 0) != 0 || !p_trigger.is_empty() {
        // Nota do C: poderia usar registradores temporários aqui, e tentar evitar copiar o
        // conteúdo do registrador do rowid.
        let mut mask: u32 = trigger_colmask(
            db,
            parse,
            p_trigger,
            None,
            0,
            (TRIGGER_BEFORE | TRIGGER_AFTER) as i32,
            p_tab,
            onconf as i32,
        ); // máscara das colunas OLD.* em uso
        mask |= fk_oldmask(db, parse, p_tab);
        i_old = parse.n_mem + 1;
        parse.n_mem += 1 + p_tab.n_col as i32;

        // Preenche o array de registradores da pseudotabela OLD.*. Os valores serão usados pelos
        // gatilhos BEFORE e AFTER que existirem.
        add_op2(vdbe_of_parse(parse), OP_COPY as i32, i_pk, i_old);
        for i_col in 0..p_tab.n_col as i32 {
            if mask == 0xffff_ffff || (i_col <= 31 && (mask & (1u32 << i_col)) != 0) {
                let kk = table_column_to_storage(p_tab, i_col as i16) as i32;
                expr_code_get_column_of_table(db, parse, p_tab, i_data_cur, i_col, i_old + kk + 1);
            }
        }

        // Chama os programas dos gatilhos BEFORE DELETE.
        let addr_start = current_addr(parse); // início dos programas dos gatilhos BEFORE
        code_row_trigger(
            db,
            parse,
            p_trigger,
            TK_DELETE as i32,
            None,
            TRIGGER_BEFORE as i32,
            p_tab,
            i_old,
            onconf as i32,
            i_label,
        );

        // Se algum gatilho BEFORE foi gerado, posiciona o cursor de novo na linha a apagar: os
        // gatilhos podem ter movido o cursor ou apagado a linha para a qual ele apontava.
        //
        // Desliga também a otimização `i_idx_no_seek`, pois o gatilho BEFORE pode ter movido
        // aquele cursor.
        if addr_start < current_addr(parse) {
            add_op4_int(vdbe_of_parse(parse), op_seek as i32, i_data_cur, i_label, i_pk, n_pk);
            i_idx_no_seek = -1;
        }

        // Faz o processamento de chaves estrangeiras: confere que as restrições que referem esta
        // tabela (as ligadas a outras tabelas) não são violadas pelo apagamento da linha.
        fk_check(db, parse, p_tab, i_old, 0, None, 0);
    }

    // Apaga as entradas de índice e da tabela. Esta etapa é pulada se `p_tab` é na verdade uma view
    // (o único efeito do DELETE é disparar os gatilhos INSTEAD OF).
    //
    // Se `count` é diferente de zero, este OP_Delete deve chamar o update-hook. O pre-update-hook,
    // por outro lado, é chamado a menos que `p_tab` seja uma tabela do sistema. A diferença é que o
    // update-hook não é chamado para linhas removidas por REPLACE, mas o pre-update-hook é.
    if !p_tab.is_view() {
        let mut p5: u16 = 0;
        generate_row_index_delete(db, parse, p_tab, i_data_cur, i_idx_cur, None, i_idx_no_seek);
        let p2_delete = if count != 0 { OPFLAG_NCHANGE as i32 } else { 0 };
        add_op2(vdbe_of_parse(parse), OP_DELETE as i32, i_data_cur, p2_delete);
        if parse.nested == 0 || 0 == str_icmp(&p_tab.z_name, b"sqlite_stat1") {
            append_p4(vdbe_of_parse(parse), P4::Table(Rc::clone(p_tab)));
        }
        if e_mode != ONEPASS_OFF {
            change_p5(vdbe_of_parse(parse), OPFLAG_AUXDELETE as u16);
        }
        if i_idx_no_seek >= 0 && i_idx_no_seek != i_data_cur {
            add_op1(vdbe_of_parse(parse), OP_DELETE as i32, i_idx_no_seek);
        }
        if e_mode == ONEPASS_MULTI {
            p5 |= OPFLAG_SAVEPOSITION as u16;
        }
        change_p5(vdbe_of_parse(parse), p5);
    }

    // Faz as operações ON CASCADE, SET NULL ou SET DEFAULT necessárias para tratar as linhas
    // (possivelmente de outras tabelas) que se referem, por chave estrangeira, à linha recém
    // apagada.
    fk_actions(db, parse, p_tab, None, i_old, None, 0);

    // Chama os programas dos gatilhos AFTER DELETE.
    if !p_trigger.is_empty() {
        code_row_trigger(
            db,
            parse,
            p_trigger,
            TK_DELETE as i32,
            None,
            TRIGGER_AFTER as i32,
            p_tab,
            i_old,
            onconf as i32,
            i_label,
        );
    }

    // Salta para cá se a linha já tinha sido apagada antes de qualquer programa de gatilho BEFORE
    // ser chamado, ou se um programa de gatilho lança a exceção RAISE(IGNORE).
    resolve_label(parse, db, i_label);
}

/// `sqlite3GenerateRowIndexDelete`: gera código do VDBE que apaga todas as entradas de índice
/// associadas a uma linha de uma tabela, `p_tab`.
///
/// Pré-condições:
///
///   1. Um cursor de leitura e escrita `i_data_cur` está aberto no armazenamento canônico da tabela
///      (a própria tabela, ou o índice da PRIMARY KEY numa WITHOUT ROWID).
///   2. Cursores de leitura e escrita de todos os índices estão abertos como `i_idx_cur+i` para o
///      i-ésimo índice (o primeiro de `p_tab.p_index` é o índice 0).
///   3. O cursor `i_data_cur` já está posicionado na linha a apagar.
///
/// `a_reg_idx`: só apaga se `a_reg_idx` é `Some` e `a_reg_idx[i]>0` (a entrada 0 pula o índice).
/// `i_idx_no_seek`: não apaga deste cursor.
pub fn generate_row_index_delete(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_data_cur: i32,
    i_idx_cur: i32,
    a_reg_idx: Option<&[i32]>,
    i_idx_no_seek: i32,
) {
    let mut r1: i32 = -1; // registrador com uma chave de índice
    let mut p_prior: Option<Rc<Index>> = None; // índice anterior
    // O índice PRIMARY KEY, ou nada para tabelas com rowid.
    let p_pk: Option<Rc<Index>> =
        if p_tab.has_rowid() { None } else { primary_key_index(p_tab).cloned() };
    for (i, p_idx) in p_tab.p_index.iter().enumerate() {
        let i = i as i32;
        debug_assert!(
            i_idx_cur + i != i_data_cur || p_pk.as_ref().map_or(false, |pk| Rc::ptr_eq(pk, p_idx))
        );
        if let Some(a) = a_reg_idx {
            if a[i as usize] == 0 {
                continue;
            }
        }
        if p_pk.as_ref().map_or(false, |pk| Rc::ptr_eq(pk, p_idx)) {
            continue;
        }
        if i_idx_cur + i == i_idx_no_seek {
            continue;
        }
        let mut i_part_idx_label: i32 = 0; // destino do salto que pula entradas de índice parcial
        r1 = generate_index_key(
            db,
            parse,
            p_tab,
            p_idx,
            i_data_cur,
            0,
            1,
            &mut i_part_idx_label,
            p_prior.as_deref(),
            r1,
        );
        let p3: i32 = (if p_idx.uniq_not_null { p_idx.n_key_col } else { p_idx.n_column }) as i32;
        let v = vdbe_of_parse(parse);
        add_op3(v, OP_IDXDELETE as i32, i_idx_cur + i, r1, p3);
        change_p5(v, 1); // faz o IdxDelete dar erro se não achar a entrada
        resolve_part_idx_label(db, parse, i_part_idx_label);
        p_prior = Some(Rc::clone(p_idx));
    }
}

/// `sqlite3GenerateIndexKey`: gera código que monta uma chave de índice e a guarda no registrador
/// `reg_out`. A chave é do índice `p_idx`, que é um índice de `p_tab`. `i_data_cur` é o cursor
/// aberto em `p_tab` apontando para a entrada que precisa ser indexada; se `p_tab` é WITHOUT ROWID,
/// ele é o cursor do índice da PRIMARY KEY.
///
/// Devolve o número do primeiro registrador de um bloco que contém os elementos da chave. O bloco de
/// registradores já foi devolvido quando a rotina retorna.
///
/// Se `pi_part_idx_label` não é nulo (aqui sempre é, é uma referência), grava nele um rótulo para
/// onde saltar se `p_idx` é um índice parcial que deve ser pulado; o rótulo se resolve com
/// `resolve_part_idx_label`. Um índice parcial deve ser pulado se o WHERE dele dá falso ou nulo.
/// Se `p_idx` não é parcial, grava 0, que é um rótulo vazio ignorado por `resolve_part_idx_label`.
///
/// `p_prior` e `reg_prior` implementam um cache para evitar cargas de registradores desnecessárias.
/// Se `p_prior` é `Some`, é outro índice cuja chave acabou de ser calculada em `reg_prior`. Se o
/// índice corrente gera a chave nos mesmos registradores e os dois compartilham uma coluna, o
/// registrador dessa coluna já tem o valor certo e a carga é pulada. A otimização ajuda num DELETE
/// ou INTEGRITY_CHECK de uma tabela com vários índices, principalmente nas colunas ROWID ou
/// PRIMARY KEY dos índices.
pub fn generate_index_key(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    p_idx: &Index,
    i_data_cur: i32,
    reg_out: i32,
    prefix_only: i32,
    pi_part_idx_label: &mut i32,
    p_prior: Option<&Index>,
    reg_prior: i32,
) -> i32 {
    let mut p_prior = p_prior;

    if p_idx.p_partial_idx_where.is_some() {
        *pi_part_idx_label = make_label(parse);
        parse.i_self_tab = i_data_cur + 1;
        expr_if_false_dup(
            db,
            parse,
            p_idx.p_partial_idx_where.as_deref(),
            *pi_part_idx_label,
            SQLITE_JUMPIFNULL as i32,
            Some(p_tab),
        );
        parse.i_self_tab = 0;
        // Ticket a9efb42811fa41ee de 2019-11-02: o pPartIdxWhere pode ter corrompido os
        // registradores de `reg_prior`.
        p_prior = None;
    } else {
        *pi_part_idx_label = 0;
    }
    let n_col: i32 = if prefix_only != 0 && p_idx.uniq_not_null {
        p_idx.n_key_col as i32
    } else {
        p_idx.n_column as i32
    };
    let reg_base = get_temp_range(parse, n_col);
    if let Some(pp) = p_prior {
        if reg_base != reg_prior || pp.p_partial_idx_where.is_some() {
            p_prior = None;
        }
    }
    for j in 0..n_col {
        if let Some(pp) = p_prior {
            if pp.ai_column[j as usize] == p_idx.ai_column[j as usize]
                && pp.ai_column[j as usize] != XN_EXPR
            {
                // Esta coluna já foi calculada pelo índice anterior.
                continue;
            }
        }
        expr_code_load_index_column(db, parse, p_tab, p_idx, i_data_cur, j, reg_base + j);
        if p_idx.ai_column[j as usize] >= 0 {
            // Se a afinidade da coluna é REAL mas o número é inteiro, ele pode estar guardado na
            // tabela como inteiro (representação compacta) e depois convertido para REAL por um
            // OP_RealAffinity. Mas estamos prestes a gravá-lo de volta num índice, onde deve ser
            // convertido para INTEGER de novo. Por isso se omite o OP_RealAffinity, se presente.
            delete_prior_opcode(vdbe_of_parse(parse), db, OP_REALAFFINITY);
        }
    }
    if reg_out != 0 {
        add_op3(vdbe_of_parse(parse), OP_MAKERECORD as i32, reg_base, n_col, reg_out);
    }
    release_temp_range(parse, reg_base, n_col);
    reg_base
}

/// `sqlite3ResolvePartIdxLabel`: se uma chamada anterior de `generate_index_key` gerou um rótulo de
/// salto por ser índice parcial, esta rotina o resolve.
pub fn resolve_part_idx_label(db: &mut Connection, parse: &mut Parse, i_label: i32) {
    if i_label != 0 {
        resolve_label(parse, db, i_label);
    }
}
