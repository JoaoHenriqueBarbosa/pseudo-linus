//! `update.c`: chunks `update_c.000` a `update_c.003` do SQLite 3.46.1. Rotinas chamadas pelo
//! analisador para gerar código de `UPDATE`: `sqlite3ColumnDefault`, `indexColumnIsBeingUpdated`,
//! `indexWhereClauseMightChange`, `exprRowColumn`, `updateFromSelect`, `sqlite3Update` e
//! `updateVirtualTable`.
//!
//! Convenções (as mesmas de `insert.rs`, `insert2.rs`, `delete.rs` e `build3.rs`, ver
//! CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db`
//!   some. O `Vdbe` é `parse.p_vdbe`; as funções do `vdbeaux` recebem `&mut Vdbe`.
//! - `Table` e `Index` do esquema são `Rc` imutáveis. A lista de índices é `Table.p_index`; a lista
//!   de gatilhos que o C passa como `Trigger*` (encadeada por `pNext`) é `&[Rc<Trigger>]`, e o
//!   ponteiro nulo é a fatia vazia.
//! - `sqlite3Update` consome a `SrcList`, o `ExprList` de mudanças, o WHERE, o ORDER BY e o LIMIT:
//!   os `sqlite3...Delete` do rótulo `update_cleanup` são o `Drop` das variáveis locais. O
//!   `pUpsert` é só lido (`iDataCur`, `iIdxCur` e a resolução de `excluded.*`), então chega como
//!   `Option<&Upsert>`: é o elo da cadeia que `sqlite3UpsertDoUpdate` escolheu.
//! - `aXRef`, `aRegIdx` e `aToOpen` (uma alocação só no C) são três `Vec`. `pRowidExpr` do C é um
//!   ponteiro para `pChanges->a[iRowidExpr].pExpr`: aqui só existe o índice `iRowidExpr`.
//! - O `NameContext` (`sNC`) guarda a lista FROM por referência mutável (a resolução liga
//!   `colUsed` nela), então ele só vive enquanto se resolvem nomes: duas janelas (a das
//!   expressões do SET e a do WHERE), com `ncFlags`, `nRef` e `nNcErr` passados de uma à outra
//!   para que o efeito seja o de um único `sNC`.
//! - `SQLITE_ENABLE_UPDATE_DELETE_LIMIT` está ligada no Debian (a gramática de `UPDATE` aceita
//!   ORDER BY e LIMIT): `sqlite3Update` chama `limit_where` (de `delete.rs`) quando a tabela não é
//!   view e não há FROM, e `updateFromSelect` recebe `pOrderBy` e `pLimit` (duplicados) e monta o
//!   `pGrp`, o `pLimit2` e o `pOrderBy2`. `pOrderBy` e `pLimit` que sobram chegam a
//!   `sqlite3MaterializeView`, como no C.
//! - `SQLITE_ENABLE_PREUPDATE_HOOK` está ligada: vale o `OP_Delete` com `OPFLAG_ISNOOP` e o
//!   `P4_TABLE` (o ramo `#else` com `if( hasFK>1 || chngKey )` não existe).
//!   `SQLITE_ALLOW_ROWID_IN_VIEW` não está ligada.
//! - Ramos `SQLITE_DEBUG` (`VdbeCoverage`, `testcase`, `OPFLAG_NOCHNG_MAGIC`) e `TREETRACE_ENABLED`
//!   não existem.
//! - `sqlite3AuthContextPush/Pop`: como em `delete.rs`, o Pop só é chamado se o Push foi.
//! - O `aXRef[j] = -1` do ramo `SQLITE_IGNORE` com `j<0` (rowid) escreve fora do vetor no C
//!   (comportamento indefinido); aqui o rowid simplesmente não é desfeito.

use std::rc::Rc;

// Funções de outras fatias, chamadas pelo nome determinístico (assinaturas supostas no relatório).
pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::auth::{auth_check, auth_context_pop, auth_context_push};
use crate::build::{
    column_expr, primary_key_index, table_column_to_storage, text_arg,
};
use crate::build2::view_get_column_names;
use crate::build3::{begin_write_operation, key_info_of_index, may_abort, multi_write};
use crate::connection::{AuthContext, Connection, Parse};
use crate::consts::{
    ALLBITS, COLFLAG_GENERATED, COLFLAG_PRIMKEY, COLFLAG_VIRTUAL, EP_SUBQUERY, NC_UUPSERT,
    OE_ABORT, OE_DEFAULT, OE_REPLACE, ONEPASS_MULTI, ONEPASS_OFF, ONEPASS_SINGLE, OPFLAG_ISNOOP,
    OPFLAG_ISUPDATE, OPFLAG_NOCHNG, OPFLAG_SAVEPOSITION, OP_ADDIMM, OP_CLOSE, OP_COLUMN, OP_COPY,
    OP_DELETE, OP_FINISHSEEK, OP_IDXINSERT, OP_INSERT, OP_INTEGER, OP_ISNULL, OP_MAKERECORD,
    OP_MUSTBEINT, OP_NEWROWID, OP_NEXT, OP_NOTEXISTS, OP_NOTFOUND, OP_NULL, OP_ONCE,
    OP_OPENEPHEMERAL, OP_OPENWRITE, OP_REALAFFINITY, OP_REWIND, OP_ROWDATA, OP_ROWID, OP_SCOPY,
    OP_VCOLUMN, OP_VUPDATE, SF_INCLUDEHIDDEN, SF_ORDERBYREQD, SF_UFSRCCHECK, SF_UPDATEFROM,
    SQLITE_AFF_REAL, SQLITE_COUNT_ROWS, SQLITE_DENY, SQLITE_IGNORE, SQLITE_JUMPIFNULL,
    SQLITE_UPDATE, SRT_TABLE, SRT_UPFROM, TF_HAS_GENERATED, TK_ROW, TK_UPDATE, TRIGGER_AFTER,
    TRIGGER_BEFORE, WHERE_ONEPASS_DESIRED, WHERE_ONEPASS_MULTIROW, XN_EXPR, XN_ROWID,
};
use crate::delete::{
    generate_row_index_delete, is_read_only, limit_where, materialize_view, src_list_lookup,
    code_change_count,
};
use crate::expr::{expr_dup, expr_list_append, expr_list_dup};
use crate::expr_code::{expr_code_get_column_of_table, is_rowid};
use crate::expr_code2::{expr_code, expr_if_false_dup};
use crate::fkey::{fk_actions, fk_check, fk_oldmask, fk_required};
use crate::insert::{
    auto_increment_end, compute_generated_columns, index_affinity_str, table_affinity,
};
use crate::insert2::{
    complete_insertion, expr_references_updated_column, generate_constraint_checks,
    open_table_and_indices,
};
use crate::mem::{value_from_expr, Mem};
use crate::prepare::schema_to_index;
use crate::resolve::{name_context_new, resolve_expr_names};
use crate::select::{get_vdbe, select, select_dest_init, select_new};
use crate::sqlite_int::{
    Expr, ExprList, Index, NameContext, NcU, SelectDest, SrcList, Table, Trigger, Upsert,
};
use crate::trigger::{code_row_trigger, trigger_colmask, triggers_exist};
use crate::util::{error_msg, str_i_hash, str_icmp};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, append_p4, change_p4_vtab,
    change_p5, change_to_noop, jump_here, jump_here_or_pop_inst, make_label, resolve_label,
    vdbe_comment,
};
use crate::vdbeaux3::vdbe_count_changes;
use crate::vtab::{get_vtable, vtab_make_writable};
use crate::where_::{
    where_begin, where_end, where_ok_one_pass, where_uses_deferred_seek, WhereInfo,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



/// O `sNC` de `sqlite3Update`: `memset(&sNC, 0, sizeof(sNC)); sNC.pParse = pParse;
/// sNC.pSrcList = pTabList; sNC.uNC.pUpsert = pUpsert; sNC.ncFlags = NC_UUpsert;`. Sem upsert o
/// `uNC.pUpsert` é o ponteiro nulo (`NcU::None`) e a flag continua ligada.
fn update_name_context<'a>(p_src: &'a mut SrcList, p_upsert: Option<&'a Upsert>) -> NameContext<'a> {
    let mut s_nc = name_context_new();
    s_nc.p_src_list = Some(p_src);
    s_nc.u_nc = match p_upsert {
        Some(u) => NcU::Upsert(u),
        None => NcU::None,
    };
    s_nc.nc_flags = NC_UUPSERT;
    s_nc
}

// ---------------------------------------------------------------------------------------------
// chunk 000: ColumnDefault, indexColumnIsBeingUpdated, indexWhereClauseMightChange, exprRowColumn,
// updateFromSelect
// ---------------------------------------------------------------------------------------------

/// `sqlite3ColumnDefault`: a instrução codificada mais recentemente foi um `OP_Column` que lê a
/// i-ésima coluna da tabela `p_tab`. Esta rotina grava o parâmetro P4 do `OP_Column` com o valor
/// default, se houver.
///
/// O default de uma coluna é dado por uma cláusula DEFAULT na definição da coluna. Ela foi dada
/// pelo usuário quando a tabela foi criada, ou acrescentada depois à definição por um ALTER TABLE.
/// Se foi o ALTER TABLE, os registros que já estão na árvore da tabela em disco podem não ter um
/// valor para a coluna, e o default, tirado do P4 do `OP_Column`, é devolvido no lugar. Se foi na
/// criação, todos os registros têm valor para a coluna e o P4 não é necessário.
///
/// Definições de coluna criadas por ALTER TABLE só podem ter valores default literais: número,
/// nulo ou texto. (Se uma expressão mais complicada foi dada, ela é avaliada quando o ALTER TABLE
/// roda e um dos valores literais é gravado na `sqlite_schema`.)
///
/// Portanto o P4 só é necessário se o default da coluna é um número, texto ou nulo literal.
/// `sqlite3ValueFromExpr` sabe transformar essas expressões em valores.
///
/// Se a coluna tem afinidade REAL e a tabela é uma tabela de árvore comum (não virtual), o valor
/// pode ter sido gravado como inteiro. Nesse caso acrescenta um `OP_RealAffinity` para garantir
/// que foi convertido em REAL.
pub fn column_default(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i: i32,
    i_reg: i32,
) {
    debug_assert!(i < p_tab.n_col as i32);
    let p_col = &p_tab.a_col[i as usize];
    if p_col.i_dflt != 0 {
        let enc: u8 = db.enc;
        debug_assert!(!p_tab.is_view());
        vdbe_comment(
            vdbe_of_parse(parse),
            b"%s.%s",
            &[text_arg(&p_tab.z_name), text_arg(&p_col.z_cn_name)],
        );
        let mut p_value: Option<Box<Mem>> = None;
        value_from_expr(db, column_expr(p_tab, p_col), enc, p_col.affinity, &mut p_value);
        if let Some(val) = p_value {
            append_p4(vdbe_of_parse(parse), P4::Mem(val));
        }
    }
    if p_col.affinity == SQLITE_AFF_REAL && !p_tab.is_virtual() {
        add_op1(vdbe_of_parse(parse), OP_REALAFFINITY as i32, i_reg);
    }
}

/// `indexColumnIsBeingUpdated`: confere se a coluna `i_col` do índice `p_idx` referencia alguma
/// das colunas definidas por `a_xref` e `chng_rowid`. Verdadeiro se sim e falso se não. É uma
/// otimização. Falsos positivos degradam o desempenho, mas falsos negativos podem corromper um
/// índice e dar respostas erradas.
///
/// `a_xref[j]` é não negativo se a coluna j da tabela original está sendo atualizada.
/// `chng_rowid` é verdadeiro se o rowid da tabela está sendo atualizado.
fn index_column_is_being_updated(
    p_idx: &Index,
    i_col: usize,
    a_xref: &[i32],
    chng_rowid: bool,
) -> bool {
    let i_idx_col: i16 = p_idx.ai_column[i_col];
    debug_assert!(i_idx_col != XN_ROWID); // o rowid não se indexa
    if i_idx_col >= 0 {
        return a_xref[i_idx_col as usize] >= 0;
    }
    debug_assert!(i_idx_col == XN_EXPR);
    debug_assert!(p_idx.a_col_expr.is_some());
    match p_idx
        .a_col_expr
        .as_deref()
        .and_then(|l| l.a.get(i_col))
        .and_then(|it| it.p_expr.as_deref())
    {
        Some(e) => expr_references_updated_column(e, a_xref, chng_rowid),
        None => false,
    }
}

/// `indexWhereClauseMightChange`: confere se o índice `p_idx` é parcial e se a expressão
/// condicional dele pode mudar por causa de um UPDATE. Verdadeiro se o índice está sujeito a
/// mudança e falso se está garantido que não muda. É uma otimização. Falsos positivos degradam o
/// desempenho, mas falsos negativos podem corromper um índice e dar respostas erradas.
fn index_where_clause_might_change(p_idx: &Index, a_xref: &[i32], chng_rowid: bool) -> bool {
    match p_idx.p_partial_idx_where.as_deref() {
        None => false,
        Some(e) => expr_references_updated_column(e, a_xref, chng_rowid),
    }
}

/// `exprRowColumn`: aloca e devolve uma expressão do tipo `TK_ROW` com `Expr.iColumn` valendo
/// `i_col+1`. O resolvedor a transforma num `TK_COLUMN` que lê a coluna `i_col` da primeira
/// tabela da lista FROM (`pSrc->a[0]`).
fn expr_row_column(db: &mut Connection, parse: &mut Parse, i_col: i32) -> Option<Box<Expr>> {
    let mut p_ret = crate::expr::p_expr(db, parse, TK_ROW as i32, None, None);
    if let Some(r) = p_ret.as_deref_mut() {
        r.i_column = i_col + 1;
    }
    p_ret
}

/// `updateFromSelect`: gera o código do VM que roda a consulta
///
/// ```text
///   SELECT <other-columns>, pChanges FROM pTabList WHERE pWhere
/// ```
///
/// e grava os resultados na tabela efêmera já aberta como o cursor `i_eph`. Nenhum de `p_changes`,
/// `p_tab_list` ou `p_where` é modificado nem consumido: o chamador os descarta.
///
/// Exatamente como os resultados são gravados na tabela `i_eph`, e o que são as
/// `<other-columns>` da consulta, é determinado pelo tipo da tabela `pTabList->a[0].pTab`.
///
/// Se a tabela é WITHOUT ROWID, `p_pk` deve ser a sua PRIMARY KEY. Neste caso as `<other-columns>`
/// são as colunas da chave primária, em ordem, e os resultados vão para `i_eph` como chaves de
/// índice, com `OP_IdxInsert`.
///
/// Se a tabela é na verdade uma view, as `<other-columns>` são todas as colunas da view. Os
/// resultados vão para `i_eph` como registros com chaves inteiras atribuídas automaticamente.
///
/// Se a tabela é virtual ou uma tabela comum com chave inteira, as `<other-columns>` são o rowid
/// dela. Para tabela virtual os resultados vão para `i_eph` como registros com chaves inteiras
/// automáticas. Para tabelas intkey o rowid das `<other-columns>` é usado como chave inteira, e
/// os campos restantes formam o registro da tabela.
fn update_from_select(
    db: &mut Connection,
    parse: &mut Parse,
    i_eph: i32,
    p_pk: Option<&Rc<Index>>,
    p_changes: &ExprList,
    p_tab_list: &SrcList,
    p_where: Option<&Expr>,
    p_order_by: Option<&ExprList>,
    p_limit: Option<&Expr>,
) {
    let Some(p_tab) = p_tab_list.a[0].p_tab.clone() else {
        return;
    };
    let mut p_list: Option<Box<ExprList>> = None;
    let mut p_grp: Option<Box<ExprList>> = None;

    // `SQLITE_ENABLE_UPDATE_DELETE_LIMIT` está ligada.
    if p_order_by.is_some() && p_limit.is_none() {
        error_msg(db, parse, b"ORDER BY without LIMIT on UPDATE", &[]);
        return;
    }
    let p_order_by2 = expr_list_dup(p_order_by, 0);
    let p_limit2 = expr_dup(p_limit, 0);

    let mut p_src = crate::expr::src_list_dup(Some(p_tab_list), 0);
    let p_where2 = expr_dup(p_where, 0);

    debug_assert!(p_tab_list.a.len() > 1);
    if let Some(s) = p_src.as_deref_mut() {
        debug_assert!(s.a[0].fg.not_cte);
        s.a[0].i_cursor = -1;
        // `pSrc->a[0].pTab->nTabRef--; pSrc->a[0].pTab = 0;`: soltar o `Rc` da cópia.
        s.a[0].p_tab = None;
    }
    let e_dest: u8;
    if let Some(pk) = p_pk {
        for i in 0..pk.n_key_col as usize {
            let p_new = expr_row_column(db, parse, pk.ai_column[i] as i32);
            if p_limit.is_some() {
                p_grp = expr_list_append(p_grp, expr_dup(p_new.as_deref(), 0));
            }
            p_list = expr_list_append(p_list, p_new);
        }
        e_dest = if p_tab.is_virtual() { SRT_TABLE } else { SRT_UPFROM };
    } else if p_tab.is_view() {
        for i in 0..p_tab.n_col as i32 {
            let p_new = expr_row_column(db, parse, i);
            p_list = expr_list_append(p_list, p_new);
        }
        e_dest = SRT_TABLE;
    } else {
        e_dest = if p_tab.is_virtual() { SRT_TABLE } else { SRT_UPFROM };
        let p_row = crate::expr::p_expr(db, parse, TK_ROW as i32, None, None);
        p_list = expr_list_append(None, p_row);
        if p_limit.is_some() {
            let p_row_grp = crate::expr::p_expr(db, parse, TK_ROW as i32, None, None);
            p_grp = expr_list_append(None, p_row_grp);
        }
    }
    for it in p_changes.a.iter() {
        p_list = expr_list_append(p_list, expr_dup(it.p_expr.as_deref(), 0));
    }
    let mut p_select = select_new(
        parse,
        p_list,
        p_src,
        p_where2,
        p_grp,
        None,
        p_order_by2,
        SF_UFSRCCHECK | SF_INCLUDEHIDDEN | SF_UPDATEFROM,
        p_limit2,
    );
    if let Some(s) = p_select.as_deref_mut() {
        s.sel_flags |= SF_ORDERBYREQD;
    }
    let mut dest = SelectDest::default();
    select_dest_init(&mut dest, e_dest as i32, i_eph);
    dest.i_sd_parm2 = match p_pk {
        Some(pk) => pk.n_key_col as i32,
        None => -1,
    };
    if let Some(s) = p_select.as_deref_mut() {
        select(db, parse, s, &mut dest);
    }
}

// ---------------------------------------------------------------------------------------------
// chunks 001 e 002: sqlite3Update
// ---------------------------------------------------------------------------------------------

/// `sqlite3Update`: processa um comando UPDATE.
///
/// ```text
///   UPDATE OR IGNORE tbl SET a=b, c=d FROM tbl2... WHERE e<5 AND f NOT NULL;
///          \_______/ \_/     \______/      \_____/       \________________/
///           onError   |      pChanges         |                pWhere
///                     \_______________________/
///                               pTabList
/// ```
pub fn update(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: Option<Box<SrcList>>, // a tabela em que se mudam coisas
    p_changes: Option<Box<ExprList>>, // as coisas a mudar
    mut p_where: Option<Box<Expr>>,   // a cláusula WHERE; pode ser nula
    on_error: i32,                    // como tratar erros de restrição
    mut p_order_by: Option<Box<ExprList>>, // a cláusula ORDER BY; pode ser nula
    mut p_limit: Option<Box<Expr>>,   // a cláusula LIMIT; pode ser nula
    p_upsert: Option<&Upsert>,        // a cláusula ON CONFLICT, ou nada
) {
    let (Some(mut p_tab_list), Some(mut p_changes)) = (p_tab_list, p_changes) else {
        return;
    };
    let mut s_context = AuthContext::default(); // o contexto de autorização
    let mut ctx_pushed = false; // o `sContext.pParse` do C é não nulo

    'cleanup: {
        if parse.n_err != 0 {
            break 'cleanup;
        }

        // Localiza a tabela que queremos atualizar.
        let Some(mut p_tab) = src_list_lookup(db, parse, &mut p_tab_list) else {
            break 'cleanup;
        };
        let i_db = schema_to_index(db, p_tab.p_schema);

        // Descobre se há gatilhos e se a tabela atualizada é uma view.
        let mut tmask: i32 = 0; // máscara de TRIGGER_BEFORE|TRIGGER_AFTER
        let p_trigger: Vec<Rc<Trigger>> =
            triggers_exist(db, parse, &p_tab, TK_UPDATE as i32, Some(&*p_changes), &mut tmask);
        let is_view = p_tab.is_view(); // verdadeiro ao atualizar uma view (gatilho INSTEAD OF)
        debug_assert!(!p_trigger.is_empty() || tmask == 0);

        // Se houve cláusula FROM, `n_change_from` é o número de expressões da lista de mudanças;
        // senão é 0. Não pode haver FROM se esta função gera código de parte de um UPSERT.
        let n_change_from: usize = if p_tab_list.a.len() > 1 { p_changes.a.len() } else { 0 };
        debug_assert!(n_change_from == 0 || p_upsert.is_none());

        if !is_view && n_change_from == 0 {
            p_where = limit_where(
                db,
                parse,
                &mut p_tab_list,
                p_where.take(),
                p_order_by.take(),
                p_limit.take(),
                b"UPDATE",
            );
        }

        if view_get_column_names(db, parse, &mut p_tab) != 0 {
            break 'cleanup;
        }
        if is_read_only(db, parse, &p_tab, &p_trigger) != 0 {
            break 'cleanup;
        }

        // Aloca cursores para a tabela do banco e para todos os índices. Os cursores de índice
        // podem não ser usados, mas se forem precisam vir logo depois do cursor do banco; então
        // aloca espaço suficiente, por via das dúvidas.
        let i_base_cur: i32 = parse.n_tab; // número do cursor base
        parse.n_tab += 1;
        let mut i_data_cur: i32 = i_base_cur; // cursor da árvore canônica dos dados
        let mut i_idx_cur: i32 = i_data_cur + 1; // cursor do primeiro índice
        let p_pk: Option<Rc<Index>> = if p_tab.has_rowid() {
            None
        } else {
            primary_key_index(&p_tab).cloned()
        }; // o índice PRIMARY KEY das tabelas WITHOUT ROWID
        let mut n_idx: usize = 0; // número de índices que precisam de atualização
        for p_idx in p_tab.p_index.iter() {
            if p_pk.as_ref().map_or(false, |pk| Rc::ptr_eq(pk, p_idx)) {
                i_data_cur = parse.n_tab;
            }
            parse.n_tab += 1;
            n_idx += 1;
        }
        if let Some(u) = p_upsert {
            // Num UPSERT reaproveita os mesmos cursores que o INSERT já abriu.
            i_data_cur = u.i_data_cur;
            i_idx_cur = u.i_idx_cur;
            parse.n_tab = i_base_cur;
        }
        p_tab_list.a[0].i_cursor = i_data_cur;

        // Aloca `a_xref`, `a_reg_idx` e `a_to_open` e os inicializa com os valores padrão.
        let n_col = p_tab.n_col as usize;
        // `a_xref[i]` é o índice em `p_changes.a` da expressão da i-ésima coluna da tabela;
        // `a_xref[i]==-1` se a coluna não muda.
        let mut a_xref: Vec<i32> = vec![-1; n_col];
        // Registradores de cada índice e da tabela principal.
        let mut a_reg_idx: Vec<i32> = vec![0; n_idx + 1];
        // 1 para as tabelas e índices a abrir.
        let mut a_to_open: Vec<u8> = vec![1; n_idx + 2];
        a_to_open[n_idx + 1] = 0;

        // Começa a gerar código.
        get_vdbe(db, parse);
        if parse.p_vdbe.is_none() {
            break 'cleanup;
        }

        // Resolve os nomes de coluna em todas as expressões do UPDATE. Também acha o índice de
        // coluna de cada coluna a atualizar no array `p_changes`. Para cada coluna a atualizar,
        // garante que há autorização para mudá-la.
        let mut chng_rowid = false; // o rowid mudou numa tabela comum
        let mut chng_pk = false; // a PRIMARY KEY mudou numa tabela WITHOUT ROWID
        let mut i_rowid_expr: i32 = -1; // índice da atribuição "rowid=" (ou IPK) em p_changes
        let z_db_s_name = db.dbs[i_db as usize].z_db_s_name.clone();
        let nc_state: (i32, i32, i32); // `ncFlags`, `nRef` e `nNcErr` do sNC ao fim da janela
        {
            let mut s_nc = update_name_context(&mut p_tab_list, p_upsert);
            for i in 0..p_changes.a.len() {
                let z_e_name: Vec<u8> = p_changes.a[i].z_e_name.clone().unwrap_or_default();
                let h_col = str_i_hash(&z_e_name);
                // Num UPDATE com FROM não resolve as expressões aqui: a chamada a
                // `sqlite3Select()` mais abaixo é que faz isso.
                if n_change_from == 0
                    && resolve_expr_names(db, parse, &mut s_nc, p_changes.a[i].p_expr.as_deref_mut())
                        != 0
                {
                    break 'cleanup;
                }
                let mut j: i32 = 0;
                while (j as usize) < n_col {
                    let p_col = &p_tab.a_col[j as usize];
                    if p_col.h_name == h_col && str_icmp(&p_col.z_cn_name, &z_e_name) == 0 {
                        if j == p_tab.i_p_key as i32 {
                            chng_rowid = true;
                            i_rowid_expr = i as i32;
                        } else if p_pk.is_some() && (p_col.col_flags & COLFLAG_PRIMKEY) != 0 {
                            chng_pk = true;
                        } else if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
                            error_msg(
                                db,
                                parse,
                                b"cannot UPDATE generated column \"%s\"",
                                &[text_arg(&p_col.z_cn_name)],
                            );
                            break 'cleanup;
                        }
                        a_xref[j as usize] = i as i32;
                        break;
                    }
                    j += 1;
                }
                if j as usize >= n_col {
                    if p_pk.is_none() && is_rowid(&z_e_name) {
                        j = -1;
                        chng_rowid = true;
                        i_rowid_expr = i as i32;
                    } else {
                        error_msg(db, parse, b"no such column: %s", &[text_arg(&z_e_name)]);
                        parse.check_schema = 1;
                        break 'cleanup;
                    }
                }
                let z_col: &[u8] = if j < 0 {
                    &b"ROWID"[..]
                } else {
                    &p_tab.a_col[j as usize].z_cn_name[..]
                };
                let rc = auth_check(
                    db,
                    parse,
                    SQLITE_UPDATE,
                    Some(&p_tab.z_name[..]),
                    Some(z_col),
                    Some(&z_db_s_name[..]),
                );
                if rc == SQLITE_DENY {
                    break 'cleanup;
                } else if rc == SQLITE_IGNORE && j >= 0 {
                    a_xref[j as usize] = -1;
                }
            }
            nc_state = (s_nc.nc_flags, s_nc.n_ref, s_nc.n_nc_err);
        }
        debug_assert!(!(chng_rowid && chng_pk));
        let chng_key: i32 = chng_rowid as i32 + chng_pk as i32; // `chngPk` ou `chngRowid`

        // Marca como mutáveis as colunas geradas cujas expressões geradoras referenciam alguma
        // coluna que muda. O valor real de `a_xref[]` para colunas geradas não é usado, além de
        // conferir que não é negativo, então pode ser qualquer número não negativo. Usa 99999
        // para que o valor seja óbvio ao olhar `a_xref[]` num depurador.
        if (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
            loop {
                let mut b_progress = false;
                for i in 0..n_col {
                    if a_xref[i] >= 0 {
                        continue;
                    }
                    let p_col = &p_tab.a_col[i];
                    if (p_col.col_flags & COLFLAG_GENERATED) == 0 {
                        continue;
                    }
                    let refs = column_expr(&p_tab, p_col)
                        .map_or(false, |e| expr_references_updated_column(e, &a_xref, chng_rowid));
                    if refs {
                        a_xref[i] = 99999;
                        b_progress = true;
                    }
                }
                if !b_progress {
                    break;
                }
            }
        }

        // As expressões do SET não são usadas dentro do laço do WHERE, então zera a máscara
        // `colUsed`. Exceto numa tabela virtual: nesse caso liga todos os bits da máscara (para
        // garantir que a implementação da tabela virtual disponibilize todas as colunas).
        p_tab_list.a[0].col_used = if p_tab.is_virtual() { ALLBITS } else { 0 };

        let has_fk: i32 = fk_required(db, parse, &p_tab, Some(&a_xref[..]), chng_key); // há FK

        // Há uma entrada em `a_reg_idx[]` para cada índice da tabela atualizada. Preenche com o
        // número do registrador que guarda a chave de acesso ao índice.
        let mut b_replace: i32 = (on_error == OE_REPLACE as i32) as i32; // REPLACE pode ocorrer
        for (n_all_idx, p_idx) in p_tab.p_index.iter().enumerate() {
            let reg: i32;
            let is_pk = p_pk.as_ref().map_or(false, |pk| Rc::ptr_eq(pk, p_idx));
            if chng_key != 0
                || has_fk > 1
                || is_pk
                || index_where_clause_might_change(p_idx, &a_xref, chng_rowid)
            {
                parse.n_mem += 1;
                reg = parse.n_mem;
                parse.n_mem += p_idx.n_column as i32;
            } else {
                let mut r = 0;
                for i in 0..p_idx.n_key_col as usize {
                    if index_column_is_being_updated(p_idx, i, &a_xref, chng_rowid) {
                        parse.n_mem += 1;
                        r = parse.n_mem;
                        parse.n_mem += p_idx.n_column as i32;
                        if on_error == OE_DEFAULT as i32 && p_idx.on_error == OE_REPLACE {
                            b_replace = 1;
                        }
                        break;
                    }
                }
                reg = r;
            }
            if reg == 0 {
                a_to_open[n_all_idx + 1] = 0;
            }
            a_reg_idx[n_all_idx] = reg;
        }
        let n_all_idx = n_idx; // número total de índices
        parse.n_mem += 1;
        a_reg_idx[n_all_idx] = parse.n_mem; // registrador do registro da tabela
        if b_replace != 0 {
            // Se a resolução REPLACE pode ser invocada, abre cursores em todos os índices, caso
            // sejam necessários para apagar registros.
            for x in a_to_open.iter_mut().take(n_idx + 1) {
                *x = 1;
            }
        }

        if parse.nested == 0 {
            vdbe_count_changes(vdbe_of_parse(parse));
        }
        begin_write_operation(db, parse, (!p_trigger.is_empty() || has_fk != 0) as i32, i_db);

        // Aloca os registradores necessários.
        let mut reg_row_set: i32 = 0; // rowset das linhas a atualizar
        let mut reg_old_rowid: i32 = 0; // o rowid antigo
        let mut reg_new_rowid: i32 = 0; // o rowid novo
        let mut reg_new: i32 = 0; // conteúdo da tabela NEW.* nos gatilhos
        let mut reg_old: i32 = 0; // conteúdo da tabela OLD.* nos gatilhos
        if !p_tab.is_virtual() {
            // Por ora `reg_row_set` e `a_reg_idx[n_all_idx]` são o mesmo registrador. Se o rowset
            // for necessário, `a_reg_idx[n_all_idx]` será realocado. `a_reg_idx[n_all_idx]` é o
            // registrador em que o registro da tabela principal é escrito. `reg_row_set` guarda o
            // RowSet do algoritmo de duas passadas.
            debug_assert!(a_reg_idx[n_all_idx] == parse.n_mem);
            reg_row_set = a_reg_idx[n_all_idx];
            parse.n_mem += 1;
            reg_old_rowid = parse.n_mem;
            reg_new_rowid = reg_old_rowid;
            if chng_pk || !p_trigger.is_empty() || has_fk != 0 {
                reg_old = parse.n_mem + 1;
                parse.n_mem += n_col as i32;
            }
            if chng_key != 0 || !p_trigger.is_empty() || has_fk != 0 {
                parse.n_mem += 1;
                reg_new_rowid = parse.n_mem;
            }
            reg_new = parse.n_mem + 1;
            parse.n_mem += n_col as i32;
        }

        // Começa o contexto da view.
        if is_view {
            auth_context_push(parse, &mut s_context, &p_tab.z_name);
            ctx_pushed = true;
        }

        // Se estamos atualizando uma view, a realiza numa tabela efêmera.
        if n_change_from == 0 && is_view {
            materialize_view(
                db,
                parse,
                &p_tab,
                p_where.as_deref(),
                p_order_by.take(),
                p_limit.take(),
                i_data_cur,
            );
        }

        // Resolve os nomes de coluna em todas as expressões da cláusula WHERE.
        if n_change_from == 0 {
            let mut s_nc = update_name_context(&mut p_tab_list, p_upsert);
            s_nc.nc_flags = nc_state.0;
            s_nc.n_ref = nc_state.1;
            s_nc.n_nc_err = nc_state.2;
            if resolve_expr_names(db, parse, &mut s_nc, p_where.as_deref_mut()) != 0 {
                break 'cleanup;
            }
        }

        // As tabelas virtuais são tratadas à parte.
        if p_tab.is_virtual() {
            update_virtual_table(
                db,
                parse,
                &mut p_tab_list,
                &p_tab,
                &mut p_changes,
                i_rowid_expr,
                &a_xref,
                p_where.as_deref_mut(),
                on_error,
            );
            break 'cleanup;
        }

        // Salta para `label_break` para abandonar o processamento deste UPDATE.
        let label_break: i32 = make_label(parse); // salta aqui para sair do laço do UPDATE
        let mut label_continue: i32 = label_break; // salta aqui para o próximo passo do laço

        // Não é um UPSERT. Processamento normal. Começa inicializando a contagem de linhas
        // atualizadas.
        let mut reg_row_count: i32 = 0; // contagem de linhas mudadas
        if (db.flags & SQLITE_COUNT_ROWS) != 0
            && parse.p_trigger_tab.is_none()
            && parse.nested == 0
            && parse.b_returning == 0
            && p_upsert.is_none()
        {
            parse.n_mem += 1;
            reg_row_count = parse.n_mem;
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_row_count);
        }

        let mut i_eph: i32 = 0; // tabela efêmera com todos os valores de PRIMARY KEY
        let mut n_key: i32 = 0; // número de elementos de `reg_key` em WITHOUT ROWID
        let mut addr_open: i32 = 0; // endereço do OP_OpenEphemeral
        let mut i_pk: i32 = 0; // primeiro dos `n_pk` registradores com a PRIMARY KEY
        let mut n_pk: i32 = 0; // número de componentes da PRIMARY KEY
        let mut reg_key: i32 = 0; // valor composto da PRIMARY KEY
        if n_change_from == 0 && p_tab.has_rowid() {
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_NULL as i32, 0, reg_row_set, reg_old_rowid);
            i_eph = parse.n_tab;
            parse.n_tab += 1;
            addr_open = add_op3(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, i_eph, 0, reg_row_set);
        } else {
            debug_assert!(p_pk.is_some() || p_tab.has_rowid());
            n_pk = p_pk.as_ref().map_or(0, |pk| pk.n_key_col as i32);
            i_pk = parse.n_mem + 1;
            parse.n_mem += n_pk;
            parse.n_mem += n_change_from as i32;
            parse.n_mem += 1;
            reg_key = parse.n_mem;
            if p_upsert.is_none() {
                let n_eph_col: i32 = n_pk + n_change_from as i32 + if is_view { n_col as i32 } else { 0 };
                i_eph = parse.n_tab;
                parse.n_tab += 1;
                if p_pk.is_some() {
                    add_op3(vdbe_of_parse(parse), OP_NULL as i32, 0, i_pk, i_pk + n_pk - 1);
                }
                addr_open = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, i_eph, n_eph_col);
                if let Some(pk) = &p_pk {
                    if let Some(mut p_key_info) = key_info_of_index(parse, db, pk) {
                        Rc::make_mut(&mut p_key_info).n_all_field = n_eph_col as u16;
                        append_p4(vdbe_of_parse(parse), P4::KeyInfo(p_key_info));
                    }
                }
                if n_change_from != 0 {
                    update_from_select(
                        db,
                        parse,
                        i_eph,
                        p_pk.as_ref(),
                        &p_changes,
                        &p_tab_list,
                        p_where.as_deref(),
                        p_order_by.as_deref(),
                        p_limit.as_deref(),
                    );
                    if is_view {
                        i_data_cur = i_eph;
                    }
                }
            }
        }

        let mut p_w_info: Option<Box<WhereInfo>> = None; // informação sobre a cláusula WHERE
        let mut ai_cur_one_pass: [i32; 2] = [-1; 2]; // cursores de escrita do WHERE_ONEPASS
        let mut e_one_pass: i32 = ONEPASS_OFF; // valor ONEPASS_XXX do where.c
        let mut b_finish_seek = true; // o OP_FinishSeek é necessário
        if n_change_from != 0 {
            multi_write(parse);
            e_one_pass = ONEPASS_OFF;
            n_key = n_pk;
            reg_key = i_pk;
        } else {
            if p_upsert.is_some() {
                // Num UPSERT todos os cursores já foram abertos pelo INSERT externo e o cursor de
                // dados já aponta para a linha a atualizar. Então pula o código que procura a(s)
                // linha(s) a atualizar.
                e_one_pass = ONEPASS_SINGLE;
                expr_if_false_dup(
                    db,
                    parse,
                    p_where.as_deref(),
                    label_break,
                    SQLITE_JUMPIFNULL as i32,
                    None,
                );
                b_finish_seek = false;
            } else {
                // Começa a varredura do banco.
                //
                // Não considera a estratégia de passada única para um UPDATE de várias linhas se
                // há algo que possa perturbar o cursor usado no UPDATE:
                //   (1) Este é um UPDATE aninhado
                //   (2) Há gatilhos
                //   (3) Há restrições FOREIGN KEY
                //   (4) Há tratadores de conflito REPLACE
                //   (5) Há subconsultas no WHERE
                let mut flags: u16 = WHERE_ONEPASS_DESIRED as u16;
                if parse.nested == 0
                    && p_trigger.is_empty()
                    && has_fk == 0
                    && chng_key == 0
                    && b_replace == 0
                    && !p_where.as_deref().map_or(false, |w| w.has_property(EP_SUBQUERY))
                {
                    flags |= WHERE_ONEPASS_MULTIROW as u16;
                }
                p_w_info = where_begin(
                    db,
                    parse,
                    &mut p_tab_list,
                    p_where.as_deref_mut(),
                    None,
                    None,
                    None,
                    flags,
                    i_idx_cur,
                );
                if p_w_info.is_none() {
                    break 'cleanup;
                }

                // Uma estratégia de passada única que pode atualizar mais de uma linha não pode
                // ser usada se alguma coluna do índice usado na varredura está sendo atualizada.
                // Senão, havendo um índice em "b", comandos como o seguinte poderiam criar um laço
                // infinito:
                //
                //   UPDATE t1 SET b=b+1 WHERE b>?
                //
                // Volta para ONEPASS_OFF se o where.c escolheu uma estratégia ONEPASS_MULTI que
                // usa um índice com uma ou mais colunas atualizadas.
                e_one_pass = match p_w_info.as_deref() {
                    Some(w) => where_ok_one_pass(w, &mut ai_cur_one_pass),
                    None => ONEPASS_OFF,
                };
                b_finish_seek = p_w_info.as_deref().map_or(false, where_uses_deferred_seek);
                if e_one_pass != ONEPASS_SINGLE {
                    multi_write(parse);
                    if e_one_pass == ONEPASS_MULTI {
                        let i_cur = ai_cur_one_pass[1];
                        if i_cur >= 0
                            && i_cur != i_data_cur
                            && a_to_open[(i_cur - i_base_cur) as usize] != 0
                        {
                            e_one_pass = ONEPASS_OFF;
                        }
                        debug_assert!(i_cur != i_data_cur || !p_tab.has_rowid());
                    }
                }
            }

            if p_tab.has_rowid() {
                // Lê o rowid da linha corrente da varredura do WHERE. No modo ONEPASS_OFF grava o
                // rowid na FIFO. Em qualquer dos modos de uma passada o deixa em `reg_old_rowid`.
                add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_data_cur, reg_old_rowid);
                if e_one_pass == ONEPASS_OFF {
                    parse.n_mem += 1;
                    a_reg_idx[n_all_idx] = parse.n_mem;
                    add_op3(
                        vdbe_of_parse(parse),
                        OP_INSERT as i32,
                        i_eph,
                        reg_row_set,
                        reg_old_rowid,
                    );
                } else if addr_open != 0 {
                    change_to_noop(vdbe_of_parse(parse), db, addr_open);
                }
            } else {
                // Lê a PK da linha corrente num array de registradores. No modo ONEPASS_OFF
                // serializa o array num registro e o grava na tabela efêmera. Nos modos
                // ONEPASS_SINGLE ou MULTI muda o OP_OpenEphemeral para Noop (a tabela efêmera não
                // é necessária) e deixa os campos da PK no array de registradores.
                let Some(pk) = p_pk.clone() else {
                    break 'cleanup;
                };
                for i in 0..n_pk {
                    debug_assert!(pk.ai_column[i as usize] >= 0);
                    expr_code_get_column_of_table(
                        db,
                        parse,
                        &p_tab,
                        i_data_cur,
                        pk.ai_column[i as usize] as i32,
                        i_pk + i,
                    );
                }
                if e_one_pass != ONEPASS_OFF {
                    if addr_open != 0 {
                        change_to_noop(vdbe_of_parse(parse), db, addr_open);
                    }
                    n_key = n_pk;
                    reg_key = i_pk;
                } else {
                    let mut z_aff = index_affinity_str(&pk, &p_tab);
                    z_aff.truncate(n_pk as usize);
                    let v = vdbe_of_parse(parse);
                    add_op4(v, OP_MAKERECORD as i32, i_pk, n_pk, reg_key, P4::Text(z_aff));
                    add_op4_int(v, OP_IDXINSERT as i32, i_eph, reg_key, i_pk, n_pk);
                }
            }
        }

        let mut addr_top: i32 = 0; // endereço no VDBE do início do laço
        if p_upsert.is_none() {
            if n_change_from == 0 && e_one_pass != ONEPASS_MULTI {
                if let Some(w) = p_w_info.take() {
                    where_end(db, parse, &p_tab_list, w);
                }
            }

            if !is_view {
                let mut addr_once: i32 = 0;
                let mut i_not_used1: i32 = 0;
                let mut i_not_used2: i32 = 0;

                // Abre todo índice que precisa de atualização.
                if e_one_pass != ONEPASS_OFF {
                    if ai_cur_one_pass[0] >= 0 {
                        a_to_open[(ai_cur_one_pass[0] - i_base_cur) as usize] = 0;
                    }
                    if ai_cur_one_pass[1] >= 0 {
                        a_to_open[(ai_cur_one_pass[1] - i_base_cur) as usize] = 0;
                    }
                }

                if e_one_pass == ONEPASS_MULTI && (n_idx as i32 - (ai_cur_one_pass[1] >= 0) as i32) > 0 {
                    addr_once = add_op0(vdbe_of_parse(parse), OP_ONCE as i32);
                }
                open_table_and_indices(
                    db,
                    parse,
                    &p_tab,
                    OP_OPENWRITE,
                    0,
                    i_base_cur,
                    Some(&a_to_open[..]),
                    &mut i_not_used1,
                    &mut i_not_used2,
                );
                if addr_once != 0 {
                    jump_here_or_pop_inst(vdbe_of_parse(parse), addr_once);
                }
            }

            // Topo do laço do UPDATE.
            if e_one_pass != ONEPASS_OFF {
                if ai_cur_one_pass[0] != i_data_cur && ai_cur_one_pass[1] != i_data_cur {
                    debug_assert!(p_pk.is_some());
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        label_break,
                        reg_key,
                        n_key,
                    );
                }
                if e_one_pass != ONEPASS_SINGLE {
                    label_continue = make_label(parse);
                }
                add_op2(
                    vdbe_of_parse(parse),
                    OP_ISNULL as i32,
                    if p_pk.is_some() { reg_key } else { reg_old_rowid },
                    label_break,
                );
            } else if p_pk.is_some() || n_change_from != 0 {
                label_continue = make_label(parse);
                add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_eph, label_break);
                addr_top = current_addr(parse);
                if n_change_from != 0 {
                    if !is_view {
                        if p_pk.is_some() {
                            for i in 0..n_pk {
                                add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_eph, i, i_pk + i);
                            }
                            add_op4_int(
                                vdbe_of_parse(parse),
                                OP_NOTFOUND as i32,
                                i_data_cur,
                                label_continue,
                                i_pk,
                                n_pk,
                            );
                        } else {
                            add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_eph, reg_old_rowid);
                            add_op3(
                                vdbe_of_parse(parse),
                                OP_NOTEXISTS as i32,
                                i_data_cur,
                                label_continue,
                                reg_old_rowid,
                            );
                        }
                    }
                } else {
                    add_op2(vdbe_of_parse(parse), OP_ROWDATA as i32, i_eph, reg_key);
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        label_continue,
                        reg_key,
                        0,
                    );
                }
            } else {
                add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_eph, label_break);
                label_continue = make_label(parse);
                addr_top = add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_eph, reg_old_rowid);
                add_op3(
                    vdbe_of_parse(parse),
                    OP_NOTEXISTS as i32,
                    i_data_cur,
                    label_continue,
                    reg_old_rowid,
                );
            }
        }

        // Se o valor do rowid vai mudar, põe o valor novo no registrador `reg_new_rowid`. Se o
        // rowid não é modificado, `reg_new_rowid` é o mesmo registrador que `reg_old_rowid`, que
        // já está preenchido.
        debug_assert!(
            chng_key != 0 || !p_trigger.is_empty() || has_fk != 0 || reg_old_rowid == reg_new_rowid
        );
        if chng_rowid {
            debug_assert!(i_rowid_expr >= 0);
            if n_change_from == 0 {
                if let Some(e) = p_changes.a[i_rowid_expr as usize].p_expr.as_deref_mut() {
                    expr_code(db, parse, e, reg_new_rowid, None);
                }
            } else {
                add_op3(
                    vdbe_of_parse(parse),
                    OP_COLUMN as i32,
                    i_eph,
                    i_rowid_expr,
                    reg_new_rowid,
                );
            }
            add_op1(vdbe_of_parse(parse), OP_MUSTBEINT as i32, reg_new_rowid);
        }

        // Calcula o conteúdo antigo, anterior ao UPDATE, da linha mudada, se a informação é
        // necessária.
        if chng_pk || has_fk != 0 || !p_trigger.is_empty() {
            let mut oldmask: u32 = if has_fk != 0 { fk_oldmask(db, parse, &p_tab) } else { 0 };
            oldmask |= trigger_colmask(
                db,
                parse,
                &p_trigger,
                Some(&*p_changes),
                0,
                (TRIGGER_BEFORE | TRIGGER_AFTER) as i32,
                &p_tab,
                on_error,
            );
            for i in 0..n_col {
                let col_flags: u16 = p_tab.a_col[i].col_flags;
                let k = table_column_to_storage(&p_tab, i as i16) as i32 + reg_old;
                if oldmask == 0xffff_ffff
                    || (i < 32 && (oldmask & (1u32 << i)) != 0)
                    || (col_flags & COLFLAG_PRIMKEY) != 0
                {
                    expr_code_get_column_of_table(db, parse, &p_tab, i_data_cur, i as i32, k);
                } else {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, k);
                }
            }
            if !chng_rowid && p_pk.is_none() {
                add_op2(vdbe_of_parse(parse), OP_COPY as i32, reg_old_rowid, reg_new_rowid);
            }
        }

        // Preenche o array de registradores que começa em `reg_new` com os dados da linha nova.
        // Este array é usado para conferir constantes, criar os registros novos da tabela e dos
        // índices, e como valores das referências new.* dos gatilhos.
        //
        // Se há um ou mais gatilhos BEFORE, não preenche os registradores das colunas que (a) não
        // são modificadas por este UPDATE e (b) não são acessadas por referências new.*. Os
        // valores dos registradores não modificados pelo UPDATE precisam ser recarregados do banco
        // depois que os gatilhos BEFORE disparam (pois o gatilho pode tê-los modificado). Então
        // não carregar os que não serão usados elimina alguns opcodes redundantes.
        let newmask: u32 = trigger_colmask(
            db,
            parse,
            &p_trigger,
            Some(&*p_changes),
            1,
            TRIGGER_BEFORE as i32,
            &p_tab,
            on_error,
        );
        let mut k: i32 = reg_new;
        for i in 0..n_col {
            let col_flags: u16 = p_tab.a_col[i].col_flags;
            if i as i32 == p_tab.i_p_key as i32 {
                add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, k);
            } else if (col_flags & COLFLAG_GENERATED) != 0 {
                if (col_flags & COLFLAG_VIRTUAL) != 0 {
                    k -= 1;
                }
            } else {
                let j = a_xref[i];
                if j >= 0 {
                    if n_change_from != 0 {
                        let n_off: i32 = if is_view { n_col as i32 } else { n_pk };
                        debug_assert!(e_one_pass == ONEPASS_OFF);
                        add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_eph, n_off + j, k);
                    } else if let Some(e) = p_changes.a[j as usize].p_expr.as_deref_mut() {
                        expr_code(db, parse, e, k, None);
                    }
                } else if 0 == (tmask & TRIGGER_BEFORE as i32)
                    || i > 31
                    || (newmask & (1u32 << i)) != 0
                {
                    // Este ramo carrega num registrador o valor de uma coluna que não muda. Isso
                    // é feito se não há gatilhos BEFORE, ou se há um ou mais gatilhos BEFORE que
                    // usam este valor por uma referência new.* num programa de gatilho.
                    expr_code_get_column_of_table(db, parse, &p_tab, i_data_cur, i as i32, k);
                    b_finish_seek = false;
                } else {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, k);
                }
            }
            k += 1;
        }
        if (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
            compute_generated_columns(db, parse, reg_new, &p_tab);
        }

        // Dispara os gatilhos BEFORE UPDATE. Isso acontece antes de as restrições serem
        // verificadas. Dá para argumentar que está errado.
        if (tmask & TRIGGER_BEFORE as i32) != 0 {
            table_affinity(vdbe_of_parse(parse), &p_tab, reg_new);
            code_row_trigger(
                db,
                parse,
                &p_trigger,
                TK_UPDATE as i32,
                Some(&*p_changes),
                TRIGGER_BEFORE as i32,
                &p_tab,
                reg_old_rowid,
                on_error,
                label_continue,
            );

            if !is_view {
                // O gatilho de linha pode ter apagado a linha atualizada. Neste caso salta para a
                // próxima linha. Nenhum update ou gatilho AFTER é necessário. Este comportamento
                // (o que acontece quando a linha atualizada é apagada ou renomeada por um gatilho
                // BEFORE) é deixado indefinido na documentação.
                if p_pk.is_some() {
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        label_continue,
                        reg_key,
                        n_key,
                    );
                } else {
                    add_op3(
                        vdbe_of_parse(parse),
                        OP_NOTEXISTS as i32,
                        i_data_cur,
                        label_continue,
                        reg_old_rowid,
                    );
                }

                // Laço de recarga após o gatilho BEFORE: se o gatilho não apagou a linha, ainda
                // pode ter modificado algumas das colunas da linha atualizada. Carrega nos
                // registradores os valores de todas as colunas não modificadas pelo UPDATE, caso
                // isso tenha acontecido. Só as colunas não modificadas são recarregadas. Os
                // valores calculados para as colunas modificadas usam os valores anteriores ao
                // disparo do gatilho BEFORE. Ver o caso de teste trigger1-18.0 (acrescentado em
                // 2018-04-26) para um exemplo.
                let mut k: i32 = reg_new;
                for i in 0..n_col {
                    let col_flags: u16 = p_tab.a_col[i].col_flags;
                    if (col_flags & COLFLAG_GENERATED) != 0 {
                        if (col_flags & COLFLAG_VIRTUAL) != 0 {
                            k -= 1;
                        }
                    } else if a_xref[i] < 0 && i as i32 != p_tab.i_p_key as i32 {
                        expr_code_get_column_of_table(db, parse, &p_tab, i_data_cur, i as i32, k);
                    }
                    k += 1;
                }
                if (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
                    compute_generated_columns(db, parse, reg_new, &p_tab);
                }
            }
        }

        if !is_view {
            // Faz as conferências de restrição.
            debug_assert!(reg_old_rowid > 0);
            generate_constraint_checks(
                db,
                parse,
                &p_tab,
                &a_reg_idx,
                i_data_cur,
                i_idx_cur,
                reg_new_rowid,
                reg_old_rowid,
                chng_key as u8,
                on_error as u8,
                label_continue,
                &mut b_replace,
                Some(&a_xref[..]),
                None,
            );

            // Se a resolução de conflito REPLACE pode ter sido usada, ou se a PK da linha está
            // mudando, o `GenerateConstraintChecks()` acima pode ter movido o cursor
            // `i_data_cur`. Posiciona-o de novo.
            if b_replace != 0 || chng_key != 0 {
                if p_pk.is_some() {
                    add_op4_int(
                        vdbe_of_parse(parse),
                        OP_NOTFOUND as i32,
                        i_data_cur,
                        label_continue,
                        reg_key,
                        n_key,
                    );
                } else {
                    add_op3(
                        vdbe_of_parse(parse),
                        OP_NOTEXISTS as i32,
                        i_data_cur,
                        label_continue,
                        reg_old_rowid,
                    );
                }
            }

            // Faz as conferências de restrição de chave estrangeira.
            if has_fk != 0 {
                fk_check(db, parse, &p_tab, reg_old_rowid, 0, Some(&a_xref[..]), chng_key);
            }

            // Apaga as entradas de índice associadas ao registro corrente.
            generate_row_index_delete(db, parse, &p_tab, i_data_cur, i_idx_cur, Some(&a_reg_idx[..]), -1);

            // É preciso rodar o opcode OP_FinishSeek para resolver um OP_DeferredSeek anterior se
            // há qualquer possibilidade de não ter havido OP_Column desde que o OP_DeferredSeek foi
            // emitido. Mas queremos evitar o OP_FinishSeek se possível, pois ele custa ciclos de
            // CPU.
            if b_finish_seek {
                add_op1(vdbe_of_parse(parse), OP_FINISHSEEK as i32, i_data_cur);
            }

            // Se muda o valor do rowid, ou se há restrições de chave estrangeira a processar,
            // apaga o registro antigo. Senão, acrescenta um OP_Delete que não faz nada, para
            // invocar o pre-update-hook.
            //
            // Que `reg_new==reg_new_rowid+1` seja verdade também é importante para o
            // pre-update-hook. Se o chamador invoca `preupdate_new()`, o valor devolvido é
            // copiado da célula de memória `reg_new_rowid+1+iCol`, onde `iCol` é o índice de
            // coluna dado pelo usuário.
            debug_assert!(reg_new == reg_new_rowid + 1);
            let p2_delete: i32 = (OPFLAG_ISUPDATE
                | if has_fk > 1 || chng_key != 0 { 0 } else { OPFLAG_ISNOOP })
                as i32;
            add_op3(vdbe_of_parse(parse), OP_DELETE as i32, i_data_cur, p2_delete, reg_new_rowid);
            if e_one_pass == ONEPASS_MULTI {
                debug_assert!(has_fk == 0 && chng_key == 0);
                change_p5(vdbe_of_parse(parse), OPFLAG_SAVEPOSITION as u16);
            }
            if parse.nested == 0 {
                append_p4(vdbe_of_parse(parse), P4::Table(Rc::clone(&p_tab)));
            }

            if has_fk != 0 {
                fk_check(db, parse, &p_tab, 0, reg_new_rowid, Some(&a_xref[..]), chng_key);
            }

            // Insere as entradas novas de índice e o registro novo.
            complete_insertion(
                db,
                parse,
                &p_tab,
                i_data_cur,
                i_idx_cur,
                reg_new_rowid,
                &a_reg_idx,
                OPFLAG_ISUPDATE as i32
                    | if e_one_pass == ONEPASS_MULTI { OPFLAG_SAVEPOSITION as i32 } else { 0 },
                0,
                0,
            );

            // Faz as operações ON CASCADE, SET NULL ou SET DEFAULT necessárias para tratar as
            // linhas (possivelmente de outras tabelas) que se referem, por chave estrangeira, à
            // linha que acabou de ser atualizada.
            if has_fk != 0 {
                fk_actions(
                    db,
                    parse,
                    &p_tab,
                    Some(&*p_changes),
                    reg_old_rowid,
                    Some(&a_xref[..]),
                    chng_key,
                );
            }
        }

        // Incrementa o contador de linhas.
        if reg_row_count != 0 {
            add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, reg_row_count, 1);
        }

        if !p_trigger.is_empty() {
            code_row_trigger(
                db,
                parse,
                &p_trigger,
                TK_UPDATE as i32,
                Some(&*p_changes),
                TRIGGER_AFTER as i32,
                &p_tab,
                reg_old_rowid,
                on_error,
                label_continue,
            );
        }

        // Repete o acima com o próximo registro a atualizar, até que todos os registros
        // selecionados pela cláusula WHERE tenham sido atualizados.
        if e_one_pass == ONEPASS_SINGLE {
            // Nada a fazer no fim do laço de uma única passada.
        } else if e_one_pass == ONEPASS_MULTI {
            resolve_label(parse, db, label_continue);
            if let Some(w) = p_w_info.take() {
                where_end(db, parse, &p_tab_list, w);
            }
        } else {
            resolve_label(parse, db, label_continue);
            add_op2(vdbe_of_parse(parse), OP_NEXT as i32, i_eph, addr_top);
        }
        resolve_label(parse, db, label_break);

        // Atualiza a tabela sqlite_sequence guardando o conteúdo dos contadores de rowid máximo
        // registrados durante os inserts em tabelas autoincrement.
        if parse.nested == 0 && parse.p_trigger_tab.is_none() && p_upsert.is_none() {
            auto_increment_end(db, parse);
        }

        // Devolve o número de linhas que foram mudadas, se estamos acompanhando essa informação.
        if reg_row_count != 0 {
            code_change_count(vdbe_of_parse(parse), reg_row_count, b"rows updated");
        }
    }

    // update_cleanup:
    if ctx_pushed {
        auth_context_pop(parse, &mut s_context);
    }
    // `sqlite3DbFree(aXRef)`, `sqlite3SrcListDelete`, `sqlite3ExprListDelete(pChanges)` e
    // `sqlite3ExprDelete(pWhere)` do C são o `Drop` das variáveis locais.
}

// ---------------------------------------------------------------------------------------------
// chunk 003: updateVirtualTable
// ---------------------------------------------------------------------------------------------

/// `updateVirtualTable`: gera o código de um UPDATE de uma tabela virtual.
///
/// Há duas estratégias possíveis: a padrão e a "onepass" especial. A onepass só é usada se a
/// implementação da tabela virtual indica que `p_where` pode casar no máximo uma linha.
///
/// A estratégia padrão é criar uma tabela efêmera que contém, para cada linha a mudar:
///
///   (A)  O rowid original da linha.
///   (B)  O rowid revisado da linha.
///   (C)  O conteúdo de toda coluna da linha.
///
/// Depois percorre o conteúdo dessa tabela efêmera executando um VUpdate para cada linha. Ao
/// terminar, descarta a tabela efêmera.
///
/// A estratégia "onepass" não usa tabela efêmera. Em vez disso guarda os mesmos valores (A, B e C
/// acima) num array de registradores e faz uma única invocação de VUpdate.
///
/// `i_rowid_expr` é o índice, em `p_changes`, da expressão que recalcula o rowid (o `pRowid` do C,
/// ponteiro para ela), ou negativo se o rowid não muda.
fn update_virtual_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: &mut SrcList,        // a tabela virtual a modificar
    p_tab: &Rc<Table>,          // a tabela virtual
    p_changes: &mut ExprList,   // as colunas a mudar no UPDATE
    i_rowid_expr: i32,          // expressão que recalcula o rowid
    a_xref: &[i32],             // mapa das colunas de `p_tab` para entradas de `p_changes`
    mut p_where: Option<&mut Expr>, // cláusula WHERE do UPDATE
    on_error: i32,              // estratégia ON CONFLICT
) {
    debug_assert!(parse.p_vdbe.is_some());
    let p_vtab = get_vtable(db, p_tab);
    let mut p_w_info: Option<Box<WhereInfo>> = None;
    let n_col = p_tab.n_col as usize;
    let n_arg: i32 = 2 + n_col as i32; // número de argumentos de VUpdate
    let i_csr: i32 = p_src.a[0].i_cursor; // cursor usado na varredura da tabela virtual
    let e_one_pass: i32; // verdadeiro para usar a estratégia onepass

    // Aloca `n_arg` registradores em que reunir os argumentos de VUpdate. Depois cria e abre a
    // tabela efêmera em que os registros montados com esses argumentos ficam guardados
    // temporariamente.
    let ephem_tab: i32 = parse.n_tab; // tabela com o resultado do SELECT
    parse.n_tab += 1;
    let mut addr: i32 = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, ephem_tab, n_arg);
    let reg_arg: i32 = parse.n_mem + 1; // primeiro registrador do array de argumentos de VUpdate
    parse.n_mem += n_arg;
    if p_src.a.len() > 1 {
        let mut p_pk: Option<Rc<Index>> = None;
        let p_row: Option<Box<Expr>>;
        if p_tab.has_rowid() {
            if i_rowid_expr >= 0 {
                p_row = expr_dup(p_changes.a[i_rowid_expr as usize].p_expr.as_deref(), 0);
            } else {
                p_row = crate::expr::p_expr(db, parse, TK_ROW as i32, None, None);
            }
        } else {
            // coluna da PRIMARY KEY
            p_pk = primary_key_index(p_tab).cloned();
            let Some(pk) = p_pk.as_ref() else {
                return;
            };
            debug_assert!(pk.n_key_col == 1);
            let i_pk: i16 = pk.ai_column[0];
            if a_xref[i_pk as usize] >= 0 {
                p_row = expr_dup(p_changes.a[a_xref[i_pk as usize] as usize].p_expr.as_deref(), 0);
            } else {
                p_row = expr_row_column(db, parse, i_pk as i32);
            }
        }
        let mut p_list = expr_list_append(None, p_row);

        for i in 0..n_col {
            if a_xref[i] >= 0 {
                p_list = expr_list_append(
                    p_list,
                    expr_dup(p_changes.a[a_xref[i] as usize].p_expr.as_deref(), 0),
                );
            } else {
                let mut p_row_expr = expr_row_column(db, parse, i as i32);
                if let Some(e) = p_row_expr.as_deref_mut() {
                    e.op2 = OPFLAG_NOCHNG;
                }
                p_list = expr_list_append(p_list, p_row_expr);
            }
        }

        if let Some(l) = p_list.as_deref() {
            update_from_select(db, parse, ephem_tab, p_pk.as_ref(), l, p_src, p_where.as_deref(), None, None);
        }
        e_one_pass = ONEPASS_OFF;
    } else {
        let reg_rec: i32; // registrador em que montar o registro
        let reg_rowid: i32; // registrador do rowid da tabela efêmera
        parse.n_mem += 1;
        reg_rec = parse.n_mem;
        parse.n_mem += 1;
        reg_rowid = parse.n_mem;

        // Começa a varredura da tabela virtual.
        p_w_info = where_begin(
            db,
            parse,
            p_src,
            p_where.as_deref_mut(),
            None,
            None,
            None,
            WHERE_ONEPASS_DESIRED as u16,
            0,
        );
        if p_w_info.is_none() {
            return;
        }

        // Preenche os registradores de argumentos.
        for i in 0..n_col {
            debug_assert!((p_tab.a_col[i].col_flags & COLFLAG_GENERATED) == 0);
            if a_xref[i] >= 0 {
                if let Some(e) = p_changes.a[a_xref[i] as usize].p_expr.as_deref_mut() {
                    expr_code(db, parse, e, reg_arg + 2 + i as i32, None);
                }
            } else {
                let v = vdbe_of_parse(parse);
                add_op3(v, OP_VCOLUMN as i32, i_csr, i as i32, reg_arg + 2 + i as i32);
                change_p5(v, OPFLAG_NOCHNG as u16); // para sqlite3_vtab_nochange()
            }
        }
        if p_tab.has_rowid() {
            add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_csr, reg_arg);
            if i_rowid_expr >= 0 {
                if let Some(e) = p_changes.a[i_rowid_expr as usize].p_expr.as_deref_mut() {
                    expr_code(db, parse, e, reg_arg + 1, None);
                }
            } else {
                add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_csr, reg_arg + 1);
            }
        } else {
            // índice PRIMARY KEY
            let Some(pk) = primary_key_index(p_tab).cloned() else {
                return;
            };
            debug_assert!(pk.n_key_col == 1);
            let i_pk: i16 = pk.ai_column[0]; // coluna da PRIMARY KEY
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_VCOLUMN as i32, i_csr, i_pk as i32, reg_arg);
            add_op2(v, OP_SCOPY as i32, reg_arg + 2 + i_pk as i32, reg_arg + 1);
        }

        let mut a_dummy: [i32; 2] = [0; 2]; // argumento sem uso de sqlite3WhereOkOnePass()
        e_one_pass = match p_w_info.as_deref() {
            Some(w) => where_ok_one_pass(w, &mut a_dummy),
            None => ONEPASS_OFF,
        };

        // Não existe ONEPASS_MULTI em tabelas virtuais.
        debug_assert!(e_one_pass == ONEPASS_OFF || e_one_pass == ONEPASS_SINGLE);

        if e_one_pass != ONEPASS_OFF {
            // Usando a estratégia onepass, transforma em no-op o OP_OpenEphemeral gerado acima.
            change_to_noop(vdbe_of_parse(parse), db, addr);
            add_op1(vdbe_of_parse(parse), OP_CLOSE as i32, i_csr);
        } else {
            // Cria um registro com o conteúdo dos registradores de argumentos e o insere na
            // tabela efêmera.
            multi_write(parse);
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_arg, n_arg, reg_rec);
            add_op2(v, OP_NEWROWID as i32, ephem_tab, reg_rowid);
            add_op3(v, OP_INSERT as i32, ephem_tab, reg_rec, reg_rowid);
        }
    }

    if e_one_pass == ONEPASS_OFF {
        // Termina a varredura da tabela virtual.
        if p_src.a.len() == 1 {
            if let Some(w) = p_w_info.take() {
                where_end(db, parse, p_src, w);
            }
        }

        // Começa a percorrer a tabela efêmera.
        addr = add_op1(vdbe_of_parse(parse), OP_REWIND as i32, ephem_tab);

        // Extrai os argumentos da linha corrente da tabela efêmera e invoca o método VUpdate.
        for i in 0..n_arg {
            add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, ephem_tab, i, reg_arg + i);
        }
    }
    vtab_make_writable(db, parse, p_tab);
    let a_vupdate = add_op3(vdbe_of_parse(parse), OP_VUPDATE as i32, 0, n_arg, reg_arg);
    if let Some(id) = p_vtab {
        change_p4_vtab(vdbe_of_parse(parse), db, a_vupdate, id);
    }
    change_p5(
        vdbe_of_parse(parse),
        (if on_error == OE_DEFAULT as i32 { OE_ABORT as i32 } else { on_error }) as u16,
    );
    may_abort(parse);

    // Fim da varredura da tabela efêmera. Ou, se usa a estratégia onepass, salta para cá se a
    // varredura visitou zero linhas.
    if e_one_pass == ONEPASS_OFF {
        add_op2(vdbe_of_parse(parse), OP_NEXT as i32, ephem_tab, addr + 1);
        jump_here(vdbe_of_parse(parse), addr);
        add_op2(vdbe_of_parse(parse), OP_CLOSE as i32, ephem_tab, 0);
    } else if let Some(w) = p_w_info.take() {
        where_end(db, parse, p_src, w);
    }
}
