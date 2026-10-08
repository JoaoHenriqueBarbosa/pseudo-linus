//! Tradução de `select.c`, primeira parte (chunks `select_c.000` a `select_c.005`): criação de
//! `Select`, tipo de junção, processamento de JOIN/USING/NATURAL, o laço interno
//! (`selectInnerLoop`), DISTINCT, ordenador (`pushOntoSorter`, `generateSortTail`), `KeyInfo`,
//! nomes e tipos declarados das colunas do resultado, `sqlite3ResultSetOfSelect`, registradores
//! de LIMIT/OFFSET e a consulta recursiva (`generateWithRecursiveQuery`).
//!
//! Convenções desta fatia (modelo v2, ver CONVENTIONS.md):
//!
//! - Funções recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db` some. O
//!   `Vdbe` é `parse.p_vdbe` e as instruções vão por `vdbeaux::add_op0..4`.
//! - `sqlite3SelectDelete`, `clearSelect` e `sqlite3SelectDeleteGeneric` não existem: a posse em
//!   árvore faz o `Drop` (inclusive de `p_prior`, `p_win_defn` e das janelas nas expressões).
//! - Composto: `Select.p_prior` possui o select da esquerda e `has_next` responde ao
//!   `p->pNext!=0` (ver `sqlite_int.rs`). `findRightmost` some: a raiz já é o mais à direita.
//! - Opções do Debian: `SQLITE_ENABLE_COLUMN_METADATA`, `SQLITE_ENABLE_STMT_SCANSTATUS` e
//!   `EXPLAIN_COMMENTS` ligadas; `SQLITE_ENABLE_SORTER_REFERENCES` desligada (some `selectExprDefer`,
//!   `aDefer`, `nDefer`, `pExtra` e o ramo `bSorterRef`); `SQLITE_DEBUG` desligada (somem `assert`,
//!   `VdbeCoverage`, `testcase`, `TREETRACE`).
//! - `SortCtx.pOrderBy` e `SelectDest.pOrderBy` são cópias POSSUÍDAS (o C as compartilha com o
//!   `Select`; quem as monta entrega `ExprList::clone` ou move, e só leitura passa por elas).
//! - `SortCtx.pDeferredRowLoad` (ponteiro para uma variável local do C) vira `Option<RowLoadInfo>`
//!   por valor, porque `RowLoadInfo` só tem dois inteiros.
//! - O `NameContext` de `columnTypeImpl` só lê `pSrcList` e `pNext`; aqui é o `ColTypeCtx`
//!   privado, com referências compartilhadas (o `NameContext` do `sqlite_int.rs` carrega `&mut`
//!   invariante, que não permite encadear níveis de subconsulta).
//! - `own: Option<&Rc<Table>>` das rotinas de `expr` é sempre `None` aqui: nenhuma expressão de
//!   `SELECT` vive dentro de um `Table` do esquema.

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::select`.
pub use crate::select2::*;
pub use crate::select3::*;

use std::rc::Rc;

pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::auth::auth_check;
use crate::build::{
    affinity_type, column_set_coll, column_type, id_list_index,
    progress_check, token_arg,
};
use crate::connection::{Connection, Parse};
use crate::consts::{
    COLFLAG_HASCOLL, COLFLAG_HASTYPE, COLFLAG_NOEXPAND, COLFLAG_NOINSERT, COLNAME_COLUMN,
    COLNAME_DATABASE, COLNAME_DECLTYPE, COLNAME_NAME, COLNAME_TABLE, ENAME_NAME, EP_CAN_BE_NULL,
    EP_COLLATE, EP_INNER_ON, EP_INT_VALUE, EP_OUTER_ON, JT_CROSS, JT_ERROR, JT_INNER, JT_LEFT,
    JT_LTORJ, JT_NATURAL, JT_OUTER, JT_RIGHT, OPFLAG_APPEND, OPFLAG_USESEEKRESULT, OP_ADDIMM,
    OP_COLUMN, OP_COMPARE, OP_COPY, OP_DECRJUMPZERO, OP_DELETE, OP_EQ, OP_EXPLAIN, OP_FOUND,
    OP_GOSUB, OP_IDXDELETE, OP_IDXINSERT, OP_IDXLE, OP_IFNOT, OP_IFNOTZERO, OP_IFPOS, OP_INSERT,
    OP_INTEGER, OP_ISNULL, OP_JUMP, OP_LAST, OP_MAKERECORD, OP_MUSTBEINT, OP_NE, OP_NEWROWID,
    OP_NEXT, OP_NULL, OP_NULLROW, OP_OFFSETLIMIT, OP_ONCE, OP_OPENEPHEMERAL, OP_OPENPSEUDO,
    OP_RESETSORTER, OP_RESULTROW, OP_RETURN, OP_REWIND, OP_ROWDATA, OP_SCOPY, OP_SEQUENCE,
    OP_SEQUENCETEST, OP_SORT, OP_SORTERDATA, OP_SORTERINSERT, OP_SORTERNEXT, OP_SORTERSORT,
    OP_YIELD, SF_AGGREGATE, SF_FIXEDLIMIT, SF_RECURSIVE, SF_RESOLVED, SF_USESEPHEMERAL,
    SQLITE_AFF_BLOB, SQLITE_AFF_FLEXNUM, SQLITE_AFF_NONE, SQLITE_AFF_NUMERIC, SQLITE_AFF_TEXT,
    SQLITE_ECEL_DUP, SQLITE_ECEL_OMITREF, SQLITE_ECEL_REF, SQLITE_FACTOR_OUT_CONST,
    SQLITE_FULL_COL_NAMES, SQLITE_NULLEQ, SQLITE_N_STDTYPE, SQLITE_OK, SQLITE_RECURSIVE,
    SQLITE_SHORT_COL_NAMES, SRT_COROUTINE, SRT_DISCARD, SRT_DISTFIFO, SRT_DISTQUEUE, SRT_EPHEMTAB,
    SRT_EXCEPT, SRT_EXISTS, SRT_FIFO, SRT_MEM, SRT_OUTPUT, SRT_QUEUE, SRT_SET, SRT_TABLE,
    SRT_UNION, SRT_UPFROM, TK_ALL, TK_ASTERISK, TK_CAST, TK_COLUMN, TK_DOT, TK_EQ, TK_EXCEPT,
    TK_FUNCTION, TK_ID, TK_INTERSECT, TK_LIMIT, TK_SELECT, TK_UNION, WHERE_DISTINCT_NOOP,
    WHERE_DISTINCT_ORDERED, WHERE_DISTINCT_UNIQUE,
};
use crate::ctype::is_digit;
use crate::expr::{
    expr, expr_add_collate_string, expr_affinity, expr_and, expr_coll_seq, expr_data_type,
    expr_function, expr_list_append, expr_nn_coll_seq, expr_skip_collate_and_likely, p_expr,
};
use crate::expr::{
    expr_code, expr_code_expr_list, expr_code_move, expr_is_integer, get_temp_range, get_temp_reg,
    is_true_or_false, release_temp_range, release_temp_reg,
};
use crate::global::randomness;
use crate::hash::{hash_clear, hash_find, hash_init, hash_insert, Hash};
use crate::mem::{CollSeq, KeyInfo, StrDtor};
use crate::printf::{mprintf, PrintfArg};
use crate::resolve::create_column_expr;
// `sqlite3Select` e `sqlite3SelectPrep` vivem nos chunks seguintes de select.c (outra fatia).
use crate::sqlite_int::{
    Column, Expr, ExprList, ExprListItem, IdList, IdListItem, Select, SelectDest, SrcItem, SrcList,
    SrcU3, TabRef, Table, Token,
};
use crate::util::{
    error_msg, log_est, str_i_hash, str_icmp, strlen30, strnicmp, STD_TYPE, STD_TYPE_AFFINITY,
};
use crate::vdbe_types::{Vdbe, P4};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, change_p2, change_p4, change_p5,
    change_to_noop, explain, get_op, jump_here, make_label, resolve_label, scan_status_counters,
    scan_status_range, vdbe_comment, vdbe_create, vdbe_goto,
};
use crate::vdbeaux2::{set_col_name, set_num_cols};

// ---------------------------------------------------------------------------------------------
// Tipos locais de select.c
// ---------------------------------------------------------------------------------------------

/// `SORTFLAG_UseSorter`: usar `OP_SorterOpen` em vez de `OP_OpenEphemeral`.
pub const SORTFLAG_USE_SORTER: u8 = 0x01;

/// `DistinctCtx`: como processar o DISTINCT (parâmetro de `selectInnerLoop`).
#[derive(Clone, Copy, Debug, Default)]
pub struct DistinctCtx {
    /// 0: sem DISTINCT. 1: DISTINCT. 2: DISTINCT e ORDER BY.
    pub is_tnct: u8,
    /// Um dos operadores `WHERE_DISTINCT_*`.
    pub e_tnct_type: u8,
    /// Tabela efêmera do processamento de DISTINCT.
    pub tab_tnct: i32,
    /// Endereço do `OP_OpenEphemeral` de `tab_tnct`.
    pub addr_tnct: i32,
}

/// `RowLoadInfo`: o que, além de `pParse` e `pSelect`, `innerLoopLoadRow` precisa para carregar a
/// próxima linha que irá ao ordenador.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowLoadInfo {
    /// Os resultados vão para o array de registradores que começa aqui.
    pub reg_result: i32,
    /// Argumento de flags de `ExprCodeExprList()`.
    pub ecel_flags: u8,
}

/// `SortCtx`: informação sobre o ORDER BY (ou GROUP BY) da consulta sendo codificada.
#[derive(Clone, Default)]
pub struct SortCtx {
    /// O ORDER BY (ou GROUP BY); ver a nota do módulo sobre a posse.
    pub p_order_by: Option<Box<ExprList>>,
    /// Quantos termos do ORDER BY os índices já satisfazem.
    pub n_ob_sat: i32,
    /// Número do cursor do ordenador.
    pub i_e_cursor: i32,
    /// Registrador com o endereço de retorno da sub-rotina de saída de bloco.
    pub reg_return: i32,
    /// Rótulo do início da sub-rotina de saída de bloco.
    pub label_bk_out: i32,
    /// Endereço do `OP_SorterOpen` ou `OP_OpenEphemeral`.
    pub addr_sort_index: i32,
    /// Salta para cá quando termina (por exemplo, LIMIT atingido).
    pub label_done: i32,
    /// Salta para cá quando o ordenador está cheio.
    pub label_ob_lopt: i32,
    /// Zero ou mais bits `SORTFLAG_*`.
    pub sort_flags: u8,
    /// Carga adiada de linha, ou `None`.
    pub p_deferred_row_load: Option<RowLoadInfo>,
    /// Primeira instrução que empurra dados no ordenador (`STMT_SCANSTATUS`).
    pub addr_push: i32,
    /// Última instrução que empurra dados no ordenador (`STMT_SCANSTATUS`).
    pub addr_push_end: i32,
}

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------



/// `pEList->nExpr` (zero para a lista nula).
pub(crate) fn n_expr(l: Option<&ExprList>) -> i32 {
    l.map_or(0, |l| l.a.len() as i32)
}

/// O nome de uma coluna (`zCnName` até o primeiro NUL).
pub(crate) fn column_name(c: &Column) -> &[u8] {
    &c.z_cn_name[..strlen30(&c.z_cn_name) as usize]
}

/// Argumento `%s` do printf.
pub(crate) fn text(z: &[u8]) -> PrintfArg {
    PrintfArg::Text(Some(z.to_vec()))
}

// ---------------------------------------------------------------------------------------------
// Chunk 000: Select, SelectDest, tipo de junção, índice de coluna
// ---------------------------------------------------------------------------------------------

/// `sqlite3SelectDestInit`: inicializa um `SelectDest`.
pub fn select_dest_init(p_dest: &mut SelectDest, e_dest: i32, i_parm: i32) {
    p_dest.e_dest = e_dest as u8;
    p_dest.i_sd_parm = i_parm;
    p_dest.i_sd_parm2 = 0;
    p_dest.z_aff_sdst = None;
    p_dest.i_sdst = 0;
    p_dest.n_sdst = 0;
}

/// `sqlite3SelectNew`: aloca um `Select` novo. Os `Box` entregues passam a ser dele. (O ramo de
/// falha de alocação do C, que limpava tudo e devolvia NULL, não existe.)
pub fn select_new(
    parse: &mut Parse,
    p_e_list: Option<Box<ExprList>>,
    p_src: Option<Box<SrcList>>,
    p_where: Option<Box<Expr>>,
    p_group_by: Option<Box<ExprList>>,
    p_having: Option<Box<Expr>>,
    p_order_by: Option<Box<ExprList>>,
    sel_flags: u32,
    p_limit: Option<Box<Expr>>,
) -> Option<Box<Select>> {
    let p_e_list = match p_e_list {
        Some(l) => Some(l),
        None => expr_list_append(None, expr(TK_ASTERISK as i32, None)),
    };
    parse.n_select += 1;
    Some(Box::new(Select {
        op: TK_SELECT,
        n_select_row: 0,
        sel_flags,
        i_limit: 0,
        i_offset: 0,
        sel_id: parse.n_select as u32,
        addr_open_ephm: [-1, -1],
        p_e_list,
        p_src: Some(p_src.unwrap_or_default()),
        p_where,
        p_group_by,
        p_having,
        p_order_by,
        p_prior: None,
        has_next: false,
        p_limit,
        p_with: None,
        n_win_linked: 0,
        p_win_defn: Vec::new(),
    }))
}

/// `sqlite3JoinType`: dados 1 a 3 identificadores antes da palavra JOIN, o tipo de junção como
/// máscara de `JT_*`. Um tipo ilegal registra erro em `parse` e devolve `JT_INNER`.
pub fn join_type(
    db: &mut Connection,
    parse: &mut Parse,
    p_a: &Token,
    p_b: Option<&Token>,
    p_c: Option<&Token>,
) -> i32 {
    const Z_KEY_TEXT: &[u8] = b"naturaleftouterightfullinnercross";
    // (início em Z_KEY_TEXT, tamanho, código)
    const A_KEYWORD: [(usize, usize, u8); 7] = [
        (0, 7, JT_NATURAL),
        (6, 4, JT_LEFT | JT_OUTER),
        (10, 5, JT_OUTER),
        (14, 5, JT_RIGHT | JT_OUTER),
        (19, 4, JT_LEFT | JT_RIGHT | JT_OUTER),
        (23, 5, JT_INNER),
        (28, 5, JT_INNER | JT_CROSS),
    ];
    let mut jointype: u8 = 0;
    let ap_all: [Option<&Token>; 3] = [Some(p_a), p_b, p_c];
    for p in ap_all.iter() {
        let Some(p) = p else {
            break;
        };
        let mut j = 0;
        while j < A_KEYWORD.len() {
            let (i0, n_char, code) = A_KEYWORD[j];
            if p.z.len() == n_char
                && strnicmp(Some(&p.z), Some(&Z_KEY_TEXT[i0..i0 + n_char]), p.z.len() as i32) == 0
            {
                jointype |= code;
                break;
            }
            j += 1;
        }
        if j >= A_KEYWORD.len() {
            jointype |= JT_ERROR;
            break;
        }
    }
    if (jointype & (JT_INNER | JT_OUTER)) == (JT_INNER | JT_OUTER)
        || (jointype & JT_ERROR) != 0
        || (jointype & (JT_OUTER | JT_LEFT | JT_RIGHT)) == JT_OUTER
    {
        let z_sp1: &[u8] = if p_b.is_none() { b"" } else { b" " };
        let z_sp2: &[u8] = if p_c.is_none() { b"" } else { b" " };
        let args = [
            token_arg(parse, p_a),
            text(z_sp1),
            p_b.map_or(PrintfArg::Token(None), |t| token_arg(parse, t)),
            text(z_sp2),
            p_c.map_or(PrintfArg::Token(None), |t| token_arg(parse, t)),
        ];
        error_msg(db, parse, b"unknown join type: %T%s%T%s%T", &args);
        jointype = JT_INNER;
    }
    jointype as i32
}

/// `sqlite3ColumnIndex`: o índice de uma coluna da tabela, ou -1 se ela não existe.
pub fn column_index(p_tab: &Table, z_col: &[u8]) -> i32 {
    let h = str_i_hash(z_col);
    let n = (p_tab.n_col.max(0) as usize).min(p_tab.a_col.len());
    for (i, p_col) in p_tab.a_col[..n].iter().enumerate() {
        if p_col.h_name == h && str_icmp(column_name(p_col), z_col) == 0 {
            return i as i32;
        }
    }
    -1
}

/// `sqlite3SrcItemColumnUsed`: marca uma coluna de subconsulta como usada.
pub fn src_item_column_used(p_item: &mut SrcItem, i_col: i32) {
    if p_item.fg.is_nested_from {
        if let Some(p_results) = p_item.p_select.as_deref_mut().and_then(|s| s.p_e_list.as_deref_mut()) {
            if let Some(it) = p_results.a.get_mut(i_col as usize) {
                it.fg.b_used = true;
            }
        }
    }
}

/// `tableAndColumnIndex`: procura, de `i_start` a `i_end` (inclusive) em `p_src`, a primeira tabela
/// com uma coluna `z_col`. Devolve (índice da tabela, índice da coluna). `b_mark_used` equivale ao
/// `piTab != 0` do C: marca a coluna como usada (`sqlite3SrcItemColumnUsed`).
pub(crate) fn table_and_column_index(
    p_src: &mut SrcList,
    i_start: usize,
    i_end: usize,
    z_col: &[u8],
    b_mark_used: bool,
    b_ignore_hidden: bool,
) -> Option<(usize, i32)> {
    for i in i_start..=i_end {
        let Some(p_tab) = p_src.a[i].p_tab.clone() else {
            continue;
        };
        let i_col = column_index(&p_tab, z_col);
        if i_col >= 0 && (!b_ignore_hidden || !p_tab.a_col[i_col as usize].is_hidden()) {
            if b_mark_used {
                src_item_column_used(&mut p_src.a[i], i_col);
            }
            return Some((i, i_col));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Chunk 001: JOIN, carga de linha do ordenador, pushOntoSorter
// ---------------------------------------------------------------------------------------------

/// `sqlite3SetJoinExpr`: liga `joinFlag` em todos os termos da expressão e grava `w.iJoin = iTable`.
pub fn set_join_expr(p: Option<&mut Expr>, i_table: i32, join_flag: u32) {
    debug_assert!(join_flag == EP_OUTER_ON || join_flag == EP_INNER_ON);
    let mut cur = p;
    while let Some(e) = cur {
        e.set_property(join_flag);
        e.w = i_table;
        if e.op == TK_FUNCTION {
            if let Some(l) = e.x_list_mut() {
                for it in l.a.iter_mut() {
                    set_join_expr(it.p_expr.as_deref_mut(), i_table, join_flag);
                }
            }
        }
        set_join_expr(e.p_left.as_deref_mut(), i_table, join_flag);
        cur = e.p_right.as_deref_mut();
    }
}

/// `unsetJoinExpr`: desfaz o trabalho de `set_join_expr()`. Cada termo marcado com `EP_OuterON` e
/// `w.iJoin==iTable` vira um termo comum; com `iTable<0` limpa toda marca `EP_OuterON`/`EP_InnerON`.
/// Se `nullable`, `p` pode valer NULL mesmo sendo coluna NOT NULL (não remove `EP_CanBeNull`).
pub(crate) fn unset_join_expr(p: Option<&mut Expr>, i_table: i32, nullable: bool) {
    let mut cur = p;
    while let Some(e) = cur {
        if i_table < 0 || (e.has_property(EP_OUTER_ON) && e.w == i_table) {
            e.clear_property(EP_OUTER_ON | EP_INNER_ON);
            if i_table >= 0 {
                e.set_property(EP_INNER_ON);
            }
        }
        if e.op == TK_COLUMN && e.i_table == i_table && !nullable {
            e.clear_property(EP_CAN_BE_NULL);
        }
        if e.op == TK_FUNCTION {
            if let Some(l) = e.x_list_mut() {
                for it in l.a.iter_mut() {
                    unset_join_expr(it.p_expr.as_deref_mut(), i_table, nullable);
                }
            }
        }
        unset_join_expr(e.p_left.as_deref_mut(), i_table, nullable);
        cur = e.p_right.as_deref_mut();
    }
}

/// `sqlite3ProcessJoin`: processa a informação de junção de um SELECT. NATURAL vira USING; ON e
/// USING viram termos extras no WHERE marcados com `EP_OuterON`/`EP_InnerON`. Devolve o número de
/// erros encontrados.
pub(crate) fn process_join(db: &mut Connection, parse: &mut Parse, p: &mut Select) -> i32 {
    let Some(p_src) = p.p_src.as_deref_mut() else {
        return 0;
    };
    let n_src = p_src.a.len();
    for i in 0..n_src.saturating_sub(1) {
        let (Some(_p_left_tab), Some(p_right_tab)) =
            (p_src.a[i].p_tab.clone(), p_src.a[i + 1].p_tab.clone())
        else {
            continue;
        };
        let join_type = if (p_src.a[i + 1].fg.jointype & JT_OUTER) != 0 {
            EP_OUTER_ON
        } else {
            EP_INNER_ON
        };

        // Se é uma junção NATURAL, sintetiza a cláusula USING que diz quais colunas juntar.
        if (p_src.a[i + 1].fg.jointype & JT_NATURAL) != 0 {
            let mut p_using: Option<Box<IdList>> = None;
            if p_src.a[i + 1].fg.is_using || p_src.a[i + 1].p_on().is_some() {
                error_msg(
                    db,
                    parse,
                    b"a NATURAL join may not have an ON or USING clause",
                    &[],
                );
                return 1;
            }
            for j in 0..p_right_tab.n_col.max(0) as usize {
                let p_col = &p_right_tab.a_col[j];
                if p_col.is_hidden() {
                    continue;
                }
                let z_name = column_name(p_col).to_vec();
                if table_and_column_index(p_src, 0, i, &z_name, false, true).is_some() {
                    let l = p_using.get_or_insert_with(Default::default);
                    l.a.push(IdListItem { z_name: Some(z_name), idx: 0 });
                }
            }
            if let Some(u) = p_using {
                let p_right = &mut p_src.a[i + 1];
                p_right.fg.is_using = true;
                p_right.fg.is_synth_using = true;
                p_right.u3 = SrcU3::Using(Some(u));
            }
            if parse.n_err != 0 {
                return 1;
            }
        }

        // Cria termos extras no WHERE para cada coluna nomeada no USING: com USING(X,Y,Z) entre A
        // e B, acrescenta A.X=B.X AND A.Y=B.Y AND A.Z=B.Z. Erro se a coluna não está nas duas.
        if p_src.a[i + 1].fg.is_using {
            let is_synth_using = p_src.a[i + 1].fg.is_synth_using;
            let names: Vec<Vec<u8>> = p_src.a[i + 1]
                .p_using()
                .map(|l| l.a.iter().map(|x| x.z_name.clone().unwrap_or_default()).collect())
                .unwrap_or_default();
            for z_name in names.iter() {
                let i_right_col = column_index(&p_right_tab, z_name);
                let found = if i_right_col < 0 {
                    None
                } else {
                    table_and_column_index(p_src, 0, i, z_name, true, is_synth_using)
                };
                let Some((mut i_left, mut i_left_col)) = found else {
                    error_msg(
                        db,
                        parse,
                        b"cannot join using column %s - column not present in both tables",
                        &[text(z_name)],
                    );
                    return 1;
                };
                let mut p_e1 = create_column_expr(db, p_src, i_left, i_left_col);
                src_item_column_used(&mut p_src.a[i_left], i_left_col);
                if (p_src.a[0].fg.jointype & JT_LTORJ) != 0 {
                    // Este ramo roda se a consulta tem um ou mais RIGHT ou FULL JOIN. Se só uma
                    // tabela à esquerda tem a coluna zName, é um no-op. Com duas ou mais, monta
                    // um coalesce() que reúne todas; erro se mais de uma dessas referências não
                    // está também num USING anterior.
                    let mut p_func_args: Option<Box<ExprList>> = None;
                    let tk_coalesce = Token { z: b"coalesce".to_vec(), i_ofst: -1 };
                    while let Some((l, lc)) =
                        table_and_column_index(p_src, i_left + 1, i, z_name, true, is_synth_using)
                    {
                        i_left = l;
                        i_left_col = lc;
                        let in_prior_using = p_src.a[i_left].fg.is_using
                            && p_src.a[i_left]
                                .p_using()
                                .map_or(-1, |u| id_list_index(u, z_name))
                                >= 0;
                        if !in_prior_using {
                            error_msg(
                                db,
                                parse,
                                b"ambiguous reference to %s in USING()",
                                &[text(z_name)],
                            );
                            break;
                        }
                        p_func_args = expr_list_append(p_func_args, p_e1.take());
                        p_e1 = create_column_expr(db, p_src, i_left, i_left_col);
                        src_item_column_used(&mut p_src.a[i_left], i_left_col);
                    }
                    if p_func_args.is_some() {
                        p_func_args = expr_list_append(p_func_args, p_e1.take());
                        p_e1 = expr_function(db, parse, p_func_args, &tk_coalesce, 0);
                    }
                }
                let p_e2 = create_column_expr(db, p_src, i + 1, i_right_col);
                src_item_column_used(&mut p_src.a[i + 1], i_right_col);
                let e2_table = p_e2.as_deref().map(|e| e.i_table);
                let mut p_eq = p_expr(db, parse, TK_EQ as i32, p_e1, p_e2);
                if let Some(eq) = p_eq.as_deref_mut() {
                    eq.set_property(join_type);
                    eq.w = e2_table.unwrap_or(0);
                }
                p.p_where = expr_and(db, parse, p.p_where.take(), p_eq);
            }
        }
        // Acrescenta o ON ao fim do WHERE, ligado por AND.
        else if p_src.a[i + 1].p_on().is_some() {
            let i_cursor = p_src.a[i + 1].i_cursor;
            let mut p_on = match &mut p_src.a[i + 1].u3 {
                SrcU3::On(on) => on.take(),
                SrcU3::Using(_) => None,
            };
            set_join_expr(p_on.as_deref_mut(), i_cursor, join_type);
            p.p_where = expr_and(db, parse, p.p_where.take(), p_on);
            p_src.a[i + 1].fg.is_on = true;
        }
    }
    0
}

/// `innerLoopLoadRow`: carrega os dados da consulta no array de registradores que vai ao ordenador.
fn inner_loop_load_row(db: &mut Connection, parse: &mut Parse, p_select: &mut Select, p_info: &RowLoadInfo) {
    if let Some(l) = p_select.p_e_list.as_deref_mut() {
        expr_code_expr_list(db, parse, l, p_info.reg_result, 0, p_info.ecel_flags, None);
    }
}

/// `makeSorterRecord`: gera o `OP_MakeRecord` da entrada do ordenador. Devolve o registrador.
fn make_sorter_record(
    db: &mut Connection,
    parse: &mut Parse,
    p_sort: &SortCtx,
    p_select: &mut Select,
    reg_base: i32,
    n_base: i32,
) -> i32 {
    let n_ob_sat = p_sort.n_ob_sat;
    parse.n_mem += 1;
    let reg_out = parse.n_mem;
    if let Some(info) = p_sort.p_deferred_row_load {
        inner_loop_load_row(db, parse, p_select, &info);
    }
    add_op3(
        vdbe_of_parse(parse),
        OP_MAKERECORD as i32,
        reg_base + n_ob_sat,
        n_base - n_ob_sat,
        reg_out,
    );
    reg_out
}

/// `pushOntoSorter`: gera o código que empurra o registro dos registradores `reg_data` a
/// `reg_data+n_data-1` no ordenador.
fn push_onto_sorter(
    db: &mut Connection,
    parse: &mut Parse,
    p_sort: &mut SortCtx,
    p_select: &mut Select,
    reg_data: i32,
    reg_orig_data: i32,
    n_data: i32,
    n_prefix_reg: i32,
) {
    let b_seq = if (p_sort.sort_flags & SORTFLAG_USE_SORTER) == 0 { 1 } else { 0 };
    let n_expr = n_expr(p_sort.p_order_by.as_deref());
    let n_base = n_expr + b_seq + n_data;
    let mut reg_record = 0;
    let n_ob_sat = p_sort.n_ob_sat;
    let mut i_skip = 0;

    // Três casos: (1) os dados já foram empacotados num Record por um OP_MakeRecord anterior
    // (n_data==1, reg_data sem relação com reg_orig_data); (2) todas as colunas de saída entram no
    // registro (reg_data==reg_orig_data); (3) algumas colunas ficam de fora, e reg_orig_data é 0
    // para esta rotina não tentar copiar valores que talvez ainda não existam.
    p_sort.addr_push = current_addr(parse);

    let reg_base;
    if n_prefix_reg != 0 {
        reg_base = reg_data - n_prefix_reg;
    } else {
        reg_base = parse.n_mem + 1;
        parse.n_mem += n_base;
    }
    let i_limit = if p_select.i_offset != 0 { p_select.i_offset + 1 } else { p_select.i_limit };
    p_sort.label_done = make_label(parse);
    if let Some(ob) = p_sort.p_order_by.as_deref_mut() {
        expr_code_expr_list(
            db,
            parse,
            ob,
            reg_base,
            reg_orig_data,
            SQLITE_ECEL_DUP | if reg_orig_data != 0 { SQLITE_ECEL_REF } else { 0 },
            None,
        );
    }
    if b_seq != 0 {
        add_op2(vdbe_of_parse(parse), OP_SEQUENCE as i32, p_sort.i_e_cursor, reg_base + n_expr);
    }
    if n_prefix_reg == 0 && n_data > 0 {
        expr_code_move(parse, reg_data, reg_base + n_expr + b_seq, n_data);
    }
    if n_ob_sat > 0 {
        reg_record = make_sorter_record(db, parse, p_sort, p_select, reg_base, n_base);
        let reg_prev_key = parse.n_mem + 1;
        parse.n_mem += p_sort.n_ob_sat;
        let n_key = n_expr - p_sort.n_ob_sat + b_seq;
        let addr_first;
        {
            let v = vdbe_of_parse(parse);
            if b_seq != 0 {
                addr_first = add_op1(v, OP_IFNOT as i32, reg_base + n_expr);
            } else {
                addr_first = add_op1(v, OP_SEQUENCETEST as i32, p_sort.i_e_cursor);
            }
            add_op3(v, OP_COMPARE as i32, reg_prev_key, reg_base, p_sort.n_ob_sat);
        }
        // O OP_Compare recém-criado leva o KeyInfo antigo com os flags de ordem zerados (torna
        // o OP_Jump testável); o ordenador passa a usar um KeyInfo novo, só com os termos que
        // os índices ainda não satisfazem.
        let p_ki_old = {
            let v = vdbe_of_parse(parse);
            let Some(p_op) = get_op(v, p_sort.addr_sort_index) else {
                return;
            };
            p_op.p2 = n_key + n_data;
            match &p_op.p4 {
                P4::KeyInfo(k) => Rc::clone(k),
                _ => return,
            }
        };
        let mut ki_zeroed = (*p_ki_old).clone();
        let n_key_field = ki_zeroed.n_key_field as usize;
        for f in ki_zeroed.a_sort_flags.iter_mut().take(n_key_field) {
            *f = 0;
        }
        change_p4(vdbe_of_parse(parse), -1, P4::KeyInfo(Rc::new(ki_zeroed)));
        let n_extra = p_ki_old.n_all_field as i32 - p_ki_old.n_key_field as i32 - 1;
        let p_ki_new = match p_sort.p_order_by.as_deref() {
            Some(ob) => key_info_from_expr_list(db, parse, ob, n_ob_sat, n_extra),
            None => return,
        };
        if let Some(p_op) = get_op(vdbe_of_parse(parse), p_sort.addr_sort_index) {
            p_op.p4 = P4::KeyInfo(p_ki_new);
        }
        let addr_jmp = current_addr(parse);
        {
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_JUMP as i32, addr_jmp + 1, 0, addr_jmp + 1);
        }
        p_sort.label_bk_out = make_label(parse);
        parse.n_mem += 1;
        p_sort.reg_return = parse.n_mem;
        {
            let v = vdbe_of_parse(parse);
            add_op2(v, OP_GOSUB as i32, p_sort.reg_return, p_sort.label_bk_out);
            add_op1(v, OP_RESETSORTER as i32, p_sort.i_e_cursor);
            if i_limit != 0 {
                add_op2(v, OP_IFNOT as i32, i_limit, p_sort.label_done);
            }
            jump_here(v, addr_first);
        }
        expr_code_move(parse, reg_base, reg_prev_key, p_sort.n_ob_sat);
        jump_here(vdbe_of_parse(parse), addr_jmp);
    }
    if i_limit != 0 {
        // Neste ponto os valores da nova entrada do ordenador estão num array de registradores.
        // Precisam ser compostos num registro e inseridos se (a) há menos de LIMIT+OFFSET itens
        // ou (b) o novo registro é menor que o maior registro do ordenador. Se (b) vale e já há
        // LIMIT+OFFSET itens, apaga o maior antes de inserir: nunca há mais que LIMIT+OFFSET.
        // Se o novo registro não precisa entrar, salta para a próxima iteração do laço (para
        // `label_ob_lopt` se não for zero; senão, só pula a inserção).
        let i_csr = p_sort.i_e_cursor;
        let v = vdbe_of_parse(parse);
        let here = v.n_op();
        add_op2(v, OP_IFNOTZERO as i32, i_limit, here + 4);
        add_op2(v, OP_LAST as i32, i_csr, 0);
        i_skip = add_op4_int(v, OP_IDXLE as i32, i_csr, 0, reg_base + n_ob_sat, n_expr - n_ob_sat);
        add_op1(v, OP_DELETE as i32, i_csr);
    }
    if reg_record == 0 {
        reg_record = make_sorter_record(db, parse, p_sort, p_select, reg_base, n_base);
    }
    let op = if (p_sort.sort_flags & SORTFLAG_USE_SORTER) != 0 { OP_SORTERINSERT } else { OP_IDXINSERT };
    add_op4_int(
        vdbe_of_parse(parse),
        op as i32,
        p_sort.i_e_cursor,
        reg_record,
        reg_base + n_ob_sat,
        n_base - n_ob_sat,
    );
    if i_skip != 0 {
        let dest = if p_sort.label_ob_lopt != 0 { p_sort.label_ob_lopt } else { current_addr(parse) };
        change_p2(vdbe_of_parse(parse), i_skip, dest);
    }
    p_sort.addr_push_end = current_addr(parse) - 1;
}

// ---------------------------------------------------------------------------------------------
// Chunk 002: OFFSET, DISTINCT e o laço interno
// ---------------------------------------------------------------------------------------------

/// `codeOffset`: acrescenta o código que implementa o OFFSET.
pub(crate) fn code_offset(v: &mut Vdbe, i_offset: i32, i_continue: i32) {
    if i_offset > 0 {
        add_op3(v, OP_IFPOS as i32, i_offset, i_continue, 1);
        vdbe_comment(v, b"OFFSET", &[]);
    }
}

/// `codeDistinct`: confere se o array de registradores a partir de `reg_elem` forma uma entrada
/// distinta (usado por `SELECT DISTINCT` e por agregadas distintas). Três estratégias, conforme
/// `e_tnct_type`:
///
/// - `WHERE_DISTINCT_UNORDERED`/`WHERE_DISTINCT_NOOP`: tabela efêmera `i_tab` (aberta antes de o
///   código gerado rodar); se o registro já existe, salta para `addr_repeat`, senão o insere.
///   Devolve `i_tab`.
/// - `WHERE_DISTINCT_ORDERED`: linhas chegam ordenadas; compara com a linha anterior e salta para
///   `addr_repeat` se igual. Devolve o primeiro registrador do array da linha anterior (que o
///   chamador deve inicializar com NULL; `fix_distinct_open_eph` cuida disso).
/// - `WHERE_DISTINCT_UNIQUE`: já se sabe que as linhas são distintas. Devolve zero.
pub(crate) fn code_distinct(
    db: &mut Connection,
    parse: &mut Parse,
    e_tnct_type: u8,
    i_tab: i32,
    addr_repeat: i32,
    p_e_list: &ExprList,
    reg_elem: i32,
) -> i32 {
    let mut i_ret = 0;
    let n_result_col = p_e_list.a.len() as i32;
    match e_tnct_type as u32 {
        WHERE_DISTINCT_ORDERED => {
            // Aloca espaço para a linha anterior.
            let reg_prev = parse.n_mem + 1;
            i_ret = reg_prev;
            parse.n_mem += n_result_col;
            let i_jump = current_addr(parse) + n_result_col;
            for i in 0..n_result_col {
                let p_coll = expr_coll_seq(db, parse, p_e_list.a[i as usize].p_expr.as_deref(), None);
                let v = vdbe_of_parse(parse);
                if i < n_result_col - 1 {
                    add_op3(v, OP_NE as i32, reg_elem + i, i_jump, reg_prev + i);
                } else {
                    add_op3(v, OP_EQ as i32, reg_elem + i, addr_repeat, reg_prev + i);
                }
                change_p4(v, -1, P4::Coll(p_coll));
                change_p5(v, SQLITE_NULLEQ as u16);
            }
            add_op3(vdbe_of_parse(parse), OP_COPY as i32, reg_elem, reg_prev, n_result_col - 1);
        }
        WHERE_DISTINCT_UNIQUE => {
            // Nada a fazer.
        }
        _ => {
            let r1 = get_temp_reg(parse);
            let v = vdbe_of_parse(parse);
            add_op4_int(v, OP_FOUND as i32, i_tab, addr_repeat, reg_elem, n_result_col);
            add_op3(v, OP_MAKERECORD as i32, reg_elem, n_result_col, r1);
            add_op4_int(v, OP_IDXINSERT as i32, i_tab, r1, reg_elem, n_result_col);
            change_p5(v, OPFLAG_USESEEKRESULT as u16);
            release_temp_reg(parse, r1);
            i_ret = i_tab;
        }
    }
    i_ret
}

/// `fixDistinctOpenEph`: roda depois de `code_distinct()`; ajusta o `OP_OpenEphemeral` que ele
/// usou (às vezes `code_distinct` roda antes de o `OP_OpenEphemeral` ser colocado).
///
/// - NOOP/UNORDERED: nada a fazer.
/// - UNIQUE: a tabela efêmera não é necessária; o `OP_OpenEphemeral` vira `OP_Noop`.
/// - ORDERED: a tabela não é necessária, mas o registrador `i_val` precisa ser inicializado com
///   NULL; o `OP_OpenEphemeral` vira um `OP_Null` nele.
pub(crate) fn fix_distinct_open_eph(
    db: &mut Connection,
    parse: &mut Parse,
    e_tnct_type: u8,
    i_val: i32,
    i_open_eph_addr: i32,
) {
    if parse.n_err == 0
        && (e_tnct_type as u32 == WHERE_DISTINCT_UNIQUE || e_tnct_type as u32 == WHERE_DISTINCT_ORDERED)
    {
        let v = vdbe_of_parse(parse);
        change_to_noop(v, db, i_open_eph_addr);
        if get_op(v, i_open_eph_addr + 1).map_or(false, |o| o.opcode == OP_EXPLAIN) {
            change_to_noop(v, db, i_open_eph_addr + 1);
        }
        if e_tnct_type as u32 == WHERE_DISTINCT_ORDERED {
            // Muda o OP_OpenEphemeral para um OP_Null que liga MEM_Cleared no primeiro registrador
            // do valor anterior; assim o OP_Ne de `code_distinct` sempre falha na primeira
            // iteração, mesmo que a primeira linha seja toda NULL.
            if let Some(p_op) = get_op(v, i_open_eph_addr) {
                p_op.opcode = OP_NULL;
                p_op.p1 = 1;
                p_op.p2 = i_val;
            }
        }
    }
}

/// `selectInnerLoop`: gera o código do miolo do laço interno de um SELECT.
///
/// Se `src_tab` é negativo, as expressões de `p.p_e_list` são avaliadas para obter os dados da
/// linha. Se é zero ou mais, os dados vêm de `src_tab` e `p.p_e_list` só dá o número de colunas e
/// a colação de cada uma.
pub(crate) fn select_inner_loop(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    src_tab: i32,
    mut p_sort: Option<&mut SortCtx>,
    p_distinct: Option<&DistinctCtx>,
    p_dest: &mut SelectDest,
    i_continue: i32,
    i_break: i32,
) {
    let e_dest = p_dest.e_dest;
    let i_parm = p_dest.i_sd_parm;
    let mut n_prefix_reg = 0;

    debug_assert!(p.p_e_list.is_some());
    let has_distinct = p_distinct.map_or(WHERE_DISTINCT_NOOP as u8, |d| d.e_tnct_type);
    if p_sort.as_deref().map_or(false, |s| s.p_order_by.is_none()) {
        p_sort = None;
    }
    if p_sort.is_none() && has_distinct == 0 {
        debug_assert!(i_continue != 0);
        code_offset(vdbe_of_parse(parse), p.i_offset, i_continue);
    }

    // Busca as colunas pedidas.
    let mut n_result_col = n_expr(p.p_e_list.as_deref());

    if p_dest.i_sdst == 0 {
        if let Some(s) = p_sort.as_deref() {
            n_prefix_reg = n_expr(s.p_order_by.as_deref());
            if (s.sort_flags & SORTFLAG_USE_SORTER) == 0 {
                n_prefix_reg += 1;
            }
            parse.n_mem += n_prefix_reg;
        }
        p_dest.i_sdst = parse.n_mem + 1;
        parse.n_mem += n_result_col;
    } else if p_dest.i_sdst + n_result_col > parse.n_mem {
        // Condição de erro: por exemplo, um SELECT do lado direito de um INSERT com mais colunas
        // de resultado que colunas na tabela. O erro será apanhado e relatado depois, mas é
        // preciso alocar memória suficiente para não gerar erros espúrios no meio tempo.
        parse.n_mem += n_result_col;
    }
    p_dest.n_sdst = n_result_col;
    // Em geral reg_result é a primeira célula do array com a linha de resultado, e reg_orig vale o
    // mesmo. Com resultados para o ordenador, os valores das expressões que também fazem parte da
    // chave ficam de fora desse array e reg_orig é zero.
    let reg_result = p_dest.i_sdst;
    let mut reg_orig = reg_result;
    if src_tab >= 0 {
        for i in 0..n_result_col {
            let z_e_name = p.p_e_list.as_deref().and_then(|l| l.a[i as usize].z_e_name.clone());
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_COLUMN as i32, src_tab, i, reg_result + i);
            vdbe_comment(v, b"%s", &[PrintfArg::Text(z_e_name)]);
        }
    } else if e_dest != SRT_EXISTS {
        // Se o destino é uma expressão EXISTS(...), os valores devolvidos não são necessários.
        let mut ecel_flags: u8 = if e_dest == SRT_MEM || e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE {
            SQLITE_ECEL_DUP
        } else {
            0
        };
        if let Some(s) = p_sort.as_deref() {
            if has_distinct == 0 && e_dest != SRT_EPHEMTAB && e_dest != SRT_TABLE {
                // Para cada expressão de p.p_e_list que é cópia de uma expressão do ORDER BY, grava
                // em iOrderByCol um a mais que o índice dela na chave que push_onto_sorter() gera.
                // Assim p.p_e_list pode ficar fora do registro ordenado, poupando espaço e CPU.
                ecel_flags |= SQLITE_ECEL_OMITREF | SQLITE_ECEL_REF;

                let n_ob = n_expr(s.p_order_by.as_deref());
                for i in s.n_ob_sat..n_ob {
                    let j = s.p_order_by.as_deref().map_or(0, |l| l.a[i as usize].i_order_by_col);
                    if j > 0 {
                        if let Some(l) = p.p_e_list.as_deref_mut() {
                            l.a[j as usize - 1].i_order_by_col = (i + 1 - s.n_ob_sat) as u16;
                        }
                    }
                }

                // Ajusta n_result_col pelas colunas que as otimizações deste ramo omitem do
                // ordenador.
                if let Some(l) = p.p_e_list.as_deref() {
                    for it in l.a.iter() {
                        if it.i_order_by_col > 0 {
                            n_result_col -= 1;
                            reg_orig = 0;
                        }
                    }
                }
            }
        }
        let s_row_load_info = RowLoadInfo { reg_result, ecel_flags };
        if p.i_limit != 0 && (ecel_flags & SQLITE_ECEL_OMITREF) != 0 && n_prefix_reg > 0 {
            debug_assert!(has_distinct == 0);
            if let Some(s) = p_sort.as_deref_mut() {
                s.p_deferred_row_load = Some(s_row_load_info);
            }
            reg_orig = 0;
        } else {
            inner_loop_load_row(db, parse, p, &s_row_load_info);
        }
    }

    // Se a palavra DISTINCT está no SELECT e esta linha já foi vista, não a inclui no resultado.
    if has_distinct != 0 {
        let e_type = has_distinct;
        let i_tab0 = p_distinct.map_or(0, |d| d.tab_tnct);
        let addr_tnct = p_distinct.map_or(0, |d| d.addr_tnct);
        debug_assert!(n_result_col == n_expr(p.p_e_list.as_deref()));
        let i_tab = match p.p_e_list.as_deref() {
            Some(l) => code_distinct(db, parse, e_type, i_tab0, i_continue, l, reg_result),
            None => 0,
        };
        fix_distinct_open_eph(db, parse, e_type, i_tab, addr_tnct);
        if p_sort.is_none() {
            code_offset(vdbe_of_parse(parse), p.i_offset, i_continue);
        }
    }

    match e_dest {
        // Grava cada resultado na chave da tabela temporária i_parm.
        SRT_UNION => {
            let r1 = get_temp_reg(parse);
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_result, n_result_col, r1);
            add_op4_int(v, OP_IDXINSERT as i32, i_parm, r1, reg_result, n_result_col);
            release_temp_reg(parse, r1);
        }

        // Monta um registro com o resultado e o usa como chave para apagar elementos da tabela
        // temporária i_parm.
        SRT_EXCEPT => {
            add_op3(vdbe_of_parse(parse), OP_IDXDELETE as i32, i_parm, reg_result, n_result_col);
        }

        // Grava o resultado como dado, com uma chave única.
        SRT_FIFO | SRT_DISTFIFO | SRT_TABLE | SRT_EPHEMTAB => {
            let r1 = get_temp_range(parse, n_prefix_reg + 1);
            add_op3(
                vdbe_of_parse(parse),
                OP_MAKERECORD as i32,
                reg_result,
                n_result_col,
                r1 + n_prefix_reg,
            );
            if e_dest == SRT_DISTFIFO {
                // Com destino DistFifo o cursor (i_parm+1) está aberto num índice efêmero. Se a
                // linha atual já está no índice, não a grava na saída; senão a acrescenta ao índice
                // e segue para gravá-la também na tabela de saída.
                let v = vdbe_of_parse(parse);
                let addr = v.n_op() + 4;
                add_op4_int(v, OP_FOUND as i32, i_parm + 1, addr, r1, 0);
                add_op4_int(v, OP_IDXINSERT as i32, i_parm + 1, r1, reg_result, n_result_col);
            }
            if let Some(s) = p_sort.as_deref_mut() {
                debug_assert!(reg_result == reg_orig);
                push_onto_sorter(db, parse, s, p, r1 + n_prefix_reg, reg_orig, 1, n_prefix_reg);
            } else {
                let r2 = get_temp_reg(parse);
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_NEWROWID as i32, i_parm, r2);
                add_op3(v, OP_INSERT as i32, i_parm, r1, r2);
                change_p5(v, OPFLAG_APPEND as u16);
                release_temp_reg(parse, r2);
            }
            release_temp_range(parse, r1, n_prefix_reg + 1);
        }

        SRT_UPFROM => {
            if let Some(s) = p_sort.as_deref_mut() {
                push_onto_sorter(db, parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
            } else {
                let i2 = p_dest.i_sd_parm2;
                let r1 = get_temp_reg(parse);
                let neg = (i2 < 0) as i32;

                // Se o UPDATE FROM é um agregado que não casa nenhuma linha, ele ainda tenta
                // devolver uma, porque é isso que agregados fazem. Não grava essa linha vazia na
                // tabela de saída.
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_ISNULL as i32, reg_result, i_break);
                add_op3(v, OP_MAKERECORD as i32, reg_result + neg, n_result_col - neg, r1);
                if i2 < 0 {
                    add_op3(v, OP_INSERT as i32, i_parm, r1, reg_result);
                } else {
                    add_op4_int(v, OP_IDXINSERT as i32, i_parm, r1, reg_result, i2);
                }
            }
        }

        // Para o conjunto de um "expr IN (SELECT ...)" deve haver um item na pilha; grava-o na
        // tabela do conjunto com dado falso.
        SRT_SET => {
            if let Some(s) = p_sort.as_deref_mut() {
                // À primeira vista dá para otimizar o ORDER BY, já que a ordem das entradas do
                // conjunto não importa. Mas pode haver LIMIT, e então a ordem importa.
                push_onto_sorter(db, parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
            } else {
                let r1 = get_temp_reg(parse);
                let aff = p_dest.z_aff_sdst.clone().unwrap_or_default();
                debug_assert!(strlen30(&aff) == n_result_col);
                let v = vdbe_of_parse(parse);
                add_op4(v, OP_MAKERECORD as i32, reg_result, n_result_col, r1, P4::Text(aff));
                add_op4_int(v, OP_IDXINSERT as i32, i_parm, r1, reg_result, n_result_col);
                release_temp_reg(parse, r1);
            }
        }

        // Se alguma linha existe no resultado, registra o fato e aborta.
        SRT_EXISTS => {
            // O LIMIT termina o laço por nós.
            add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 1, i_parm);
        }

        // Se é um select escalar parte de uma expressão, grava os resultados na célula (ou array
        // de células) apropriada e sai do laço de varredura.
        SRT_MEM => {
            if let Some(s) = p_sort.as_deref_mut() {
                debug_assert!(n_result_col <= p_dest.n_sdst);
                push_onto_sorter(db, parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
            } else {
                debug_assert!(n_result_col == p_dest.n_sdst);
                debug_assert!(reg_result == i_parm);
                // O LIMIT salta para fora do laço por nós.
            }
        }

        SRT_COROUTINE | SRT_OUTPUT => {
            if let Some(s) = p_sort.as_deref_mut() {
                push_onto_sorter(db, parse, s, p, reg_result, reg_orig, n_result_col, n_prefix_reg);
            } else if e_dest == SRT_COROUTINE {
                add_op1(vdbe_of_parse(parse), OP_YIELD as i32, p_dest.i_sd_parm);
            } else {
                add_op2(vdbe_of_parse(parse), OP_RESULTROW as i32, reg_result, n_result_col);
            }
        }

        // Grava os resultados numa fila de prioridade ordenada por p_dest.p_order_by (`pSO`).
        // `i_parm` é o cursor de um índice com pSO.nExpr+2 colunas. Monta a chave com pSO nas
        // primeiras pSO.nExpr colunas, garante unicidade com um OP_Sequence final; a última coluna
        // é o registro como blob.
        SRT_DISTQUEUE | SRT_QUEUE => {
            let mut addr_test = 0;
            let so_cols: Vec<i32> = p_dest
                .p_order_by
                .as_deref()
                .map(|l| l.a.iter().map(|x| x.i_order_by_col as i32).collect())
                .unwrap_or_default();
            let n_key = so_cols.len() as i32;
            let r1 = get_temp_reg(parse);
            let r2 = get_temp_range(parse, n_key + 2);
            let r3 = r2 + n_key + 1;
            if e_dest == SRT_DISTQUEUE {
                // Com destino DistQueue, o cursor (i_parm+1) está aberto num segundo índice efêmero
                // com tudo o que já foi acrescentado à fila.
                addr_test = add_op4_int(
                    vdbe_of_parse(parse),
                    OP_FOUND as i32,
                    i_parm + 1,
                    0,
                    reg_result,
                    n_result_col,
                );
            }
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_result, n_result_col, r3);
            if e_dest == SRT_DISTQUEUE {
                add_op2(v, OP_IDXINSERT as i32, i_parm + 1, r3);
                change_p5(v, OPFLAG_USESEEKRESULT as u16);
            }
            for (i, col) in so_cols.iter().enumerate() {
                add_op2(v, OP_SCOPY as i32, reg_result + col - 1, r2 + i as i32);
            }
            add_op2(v, OP_SEQUENCE as i32, i_parm, r2 + n_key);
            add_op3(v, OP_MAKERECORD as i32, r2, n_key + 2, r1);
            add_op4_int(v, OP_IDXINSERT as i32, i_parm, r1, r2, n_key + 2);
            if addr_test != 0 {
                jump_here(v, addr_test);
            }
            release_temp_reg(parse, r1);
            release_temp_range(parse, r2, n_key + 2);
        }

        // Descarta os resultados. Usado por SELECT dentro do corpo de um TRIGGER: o objetivo é
        // chamar funções do usuário com efeito colateral; o resultado não interessa.
        _ => {
            debug_assert!(e_dest == SRT_DISCARD);
        }
    }

    // Salta para o fim do laço se o LIMIT foi atingido, exceto havendo ordenador, caso em que o
    // ordenador já limitou a saída.
    if p_sort.is_none() && p.i_limit != 0 {
        add_op2(vdbe_of_parse(parse), OP_DECRJUMPZERO as i32, p.i_limit, i_break);
    }
}

// ---------------------------------------------------------------------------------------------
// Chunk 003: KeyInfo, nome do operador, cauda do ordenador
// ---------------------------------------------------------------------------------------------

/// `sqlite3KeyInfoAlloc`: um `KeyInfo` para um índice de `n` colunas-chave e `x` colunas extras.
/// (`sqlite3KeyInfoRef` e `sqlite3KeyInfoUnref` são o `Rc::clone` e o `Drop`.)
pub fn key_info_alloc(db: &Connection, n: i32, x: i32) -> KeyInfo {
    let total = (n + x).max(0) as usize;
    KeyInfo {
        enc: db.enc,
        n_key_field: n as u16,
        n_all_field: (n + x) as u16,
        a_sort_flags: vec![0; total],
        a_coll: vec![None; total],
    }
}

/// `sqlite3KeyInfoFromExprList`: dada uma lista de expressões, gera o `KeyInfo` que registra a
/// colação de cada uma. Se a lista é um ORDER BY ou GROUP BY, o resultado serve para inicializar um
/// índice virtual que o implementa; se é o resultado de um SELECT, serve para o teste de DISTINCT.
pub fn key_info_from_expr_list(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: &ExprList,
    i_start: i32,
    n_extra: i32,
) -> Rc<KeyInfo> {
    let n_expr = p_list.a.len() as i32;
    let mut p_info = key_info_alloc(db, n_expr - i_start, n_extra + 1);
    for i in i_start..n_expr {
        let p_item = &p_list.a[i as usize];
        p_info.a_coll[(i - i_start) as usize] =
            Some(expr_nn_coll_seq(db, parse, p_item.p_expr.as_deref(), None));
        p_info.a_sort_flags[(i - i_start) as usize] = p_item.fg.sort_flags;
    }
    Rc::new(p_info)
}

/// `sqlite3SelectOpName`: o nome do operador de conexão, usado em mensagens de erro.
pub fn select_op_name(id: i32) -> &'static str {
    match id as u8 {
        TK_ALL => "UNION ALL",
        TK_INTERSECT => "INTERSECT",
        TK_EXCEPT => "EXCEPT",
        _ => "UNION",
    }
}

/// `explainTempTable`: a menos que haja um EXPLAIN QUERY PLAN em curso, é um no-op. Senão acrescenta
/// uma linha ao EQP com a legenda "USE TEMP B-TREE FOR xxx", com xxx igual a "DISTINCT",
/// "ORDER BY" ou "GROUP BY" conforme `z_usage`.
pub(crate) fn explain_temp_table(db: &mut Connection, parse: &mut Parse, z_usage: &str) {
    explain(parse, db, false, b"USE TEMP B-TREE FOR %s", &[text(z_usage.as_bytes())]);
}

/// `generateSortTail`: se o laço interno foi gerado com um `p_order_by` não nulo, os resultados
/// foram para um ordenador. Depois de terminado o laço é preciso rodar o ordenador e emitir os
/// resultados; esta rotina gera o código que faz isso.
pub(crate) fn generate_sort_tail(
    db: &mut Connection,
    parse: &mut Parse,
    p: &Select,
    p_sort: &SortCtx,
    n_column: i32,
    p_dest: &SelectDest,
) {
    let mut n_column = n_column;
    let addr_break = p_sort.label_done;
    let addr_continue = make_label(parse);
    let e_dest = p_dest.e_dest;
    let i_parm = p_dest.i_sd_parm;
    let n_ref_key = 0;
    let a_out_ex: &[ExprListItem] = match p.p_e_list.as_deref() {
        Some(l) => &l.a[..],
        None => &[],
    };

    let n_key = n_expr(p_sort.p_order_by.as_deref()) - p_sort.n_ob_sat;
    let z_last_term: &[u8] = if p_sort.n_ob_sat != 0 { b"LAST TERM OF " } else { b"" };
    let addr_explain = if p_sort.n_ob_sat == 0 || n_key == 1 {
        explain(parse, db, false, b"USE TEMP B-TREE FOR %sORDER BY", &[text(z_last_term)])
    } else {
        explain(
            parse,
            db,
            false,
            b"USE TEMP B-TREE FOR LAST %d TERMS OF ORDER BY",
            &[PrintfArg::Int(n_key as i64)],
        )
    };
    scan_status_range(vdbe_of_parse(parse), db, addr_explain, p_sort.addr_push, p_sort.addr_push_end);
    scan_status_counters(vdbe_of_parse(parse), db, addr_explain, addr_explain, p_sort.addr_push);

    debug_assert!(addr_break < 0);
    if p_sort.label_bk_out != 0 {
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_GOSUB as i32, p_sort.reg_return, p_sort.label_bk_out);
        vdbe_goto(v, addr_break);
        resolve_label(parse, db, p_sort.label_bk_out);
    }

    let i_tab = p_sort.i_e_cursor;
    let reg_rowid;
    let reg_row;
    if e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE || e_dest == SRT_MEM {
        if e_dest == SRT_MEM && p.i_offset != 0 {
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, p_dest.i_sdst);
        }
        reg_rowid = 0;
        reg_row = p_dest.i_sdst;
    } else {
        reg_rowid = get_temp_reg(parse);
        if e_dest == SRT_EPHEMTAB || e_dest == SRT_TABLE {
            reg_row = get_temp_reg(parse);
            n_column = 0;
        } else {
            reg_row = get_temp_range(parse, n_column);
        }
    }
    let i_sort_tab;
    let b_seq;
    let addr;
    if (p_sort.sort_flags & SORTFLAG_USE_SORTER) != 0 {
        parse.n_mem += 1;
        let reg_sort_out = parse.n_mem;
        i_sort_tab = parse.n_tab;
        parse.n_tab += 1;
        let mut addr_once = 0;
        let v = vdbe_of_parse(parse);
        if p_sort.label_bk_out != 0 {
            addr_once = add_op0(v, OP_ONCE as i32);
        }
        add_op3(v, OP_OPENPSEUDO as i32, i_sort_tab, reg_sort_out, n_key + 1 + n_column + n_ref_key);
        if addr_once != 0 {
            jump_here(v, addr_once);
        }
        addr = 1 + add_op2(v, OP_SORTERSORT as i32, i_tab, addr_break);
        debug_assert!(p.i_limit == 0 && p.i_offset == 0);
        add_op3(v, OP_SORTERDATA as i32, i_tab, reg_sort_out, i_sort_tab);
        b_seq = 0;
    } else {
        addr = 1 + add_op2(vdbe_of_parse(parse), OP_SORT as i32, i_tab, addr_break);
        code_offset(vdbe_of_parse(parse), p.i_offset, addr_continue);
        i_sort_tab = i_tab;
        b_seq = 1;
        if p.i_offset > 0 {
            add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, p.i_limit, -1);
        }
    }
    let mut i_col = n_key + b_seq - 1;
    for i in 0..n_column as usize {
        if a_out_ex[i].i_order_by_col == 0 {
            i_col += 1;
        }
    }
    for i in (0..n_column as usize).rev() {
        let i_read = if a_out_ex[i].i_order_by_col != 0 {
            a_out_ex[i].i_order_by_col as i32 - 1
        } else {
            let r = i_col;
            i_col -= 1;
            r
        };
        let v = vdbe_of_parse(parse);
        add_op3(v, OP_COLUMN as i32, i_sort_tab, i_read, reg_row + i as i32);
        vdbe_comment(v, b"%s", &[PrintfArg::Text(a_out_ex[i].z_e_name.clone())]);
    }
    scan_status_range(vdbe_of_parse(parse), db, addr_explain, addr_explain, -1);
    match e_dest {
        SRT_TABLE | SRT_EPHEMTAB => {
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_COLUMN as i32, i_sort_tab, n_key + b_seq, reg_row);
            add_op2(v, OP_NEWROWID as i32, i_parm, reg_rowid);
            add_op3(v, OP_INSERT as i32, i_parm, reg_row, reg_rowid);
            change_p5(v, OPFLAG_APPEND as u16);
        }
        SRT_SET => {
            let aff = p_dest.z_aff_sdst.clone().unwrap_or_default();
            debug_assert!(n_column == strlen30(&aff));
            let v = vdbe_of_parse(parse);
            add_op4(v, OP_MAKERECORD as i32, reg_row, n_column, reg_rowid, P4::Text(aff));
            add_op4_int(v, OP_IDXINSERT as i32, i_parm, reg_rowid, reg_row, n_column);
        }
        SRT_MEM => {
            // O LIMIT termina o laço por nós.
        }
        SRT_UPFROM => {
            let i2 = p_dest.i_sd_parm2;
            let r1 = get_temp_reg(parse);
            let neg = (i2 < 0) as i32;
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, reg_row + neg, n_column - neg, r1);
            if i2 < 0 {
                add_op3(v, OP_INSERT as i32, i_parm, r1, reg_row);
            } else {
                add_op4_int(v, OP_IDXINSERT as i32, i_parm, r1, reg_row, i2);
            }
        }
        _ => {
            debug_assert!(e_dest == SRT_OUTPUT || e_dest == SRT_COROUTINE);
            if e_dest == SRT_OUTPUT {
                add_op2(vdbe_of_parse(parse), OP_RESULTROW as i32, p_dest.i_sdst, n_column);
            } else {
                add_op1(vdbe_of_parse(parse), OP_YIELD as i32, p_dest.i_sd_parm);
            }
        }
    }
    if reg_rowid != 0 {
        if e_dest == SRT_SET {
            release_temp_range(parse, reg_row, n_column);
        } else {
            release_temp_reg(parse, reg_row);
        }
        release_temp_reg(parse, reg_rowid);
    }
    // O fim do laço.
    resolve_label(parse, db, addr_continue);
    if (p_sort.sort_flags & SORTFLAG_USE_SORTER) != 0 {
        add_op2(vdbe_of_parse(parse), OP_SORTERNEXT as i32, i_tab, addr);
    } else {
        add_op2(vdbe_of_parse(parse), OP_NEXT as i32, i_tab, addr);
    }
    let last = current_addr(parse) - 1;
    scan_status_range(vdbe_of_parse(parse), db, addr_explain, last, -1);
    if p_sort.reg_return != 0 {
        add_op1(vdbe_of_parse(parse), OP_RETURN as i32, p_sort.reg_return);
    }
    resolve_label(parse, db, addr_break);
}

// ---------------------------------------------------------------------------------------------
// Chunk 004: tipos declarados e nomes das colunas do resultado
// ---------------------------------------------------------------------------------------------

/// Contexto de nomes de `columnTypeImpl`: só o que a rotina lê do `NameContext` do C (`pSrcList` e
/// `pNext`). Ver a nota do módulo.
struct ColTypeCtx<'a> {
    p_src_list: Option<&'a SrcList>,
    p_next: Option<&'a ColTypeCtx<'a>>,
}

/// O que `columnTypeImpl` devolve com `SQLITE_ENABLE_COLUMN_METADATA`: o tipo declarado e a origem
/// (`pzOrigDb`, `pzOrigTab`, `pzOrigCol`) do resultado. Cada campo `None` é o ponteiro nulo do C.
#[derive(Default)]
pub(crate) struct ColumnTypeInfo {
    /// O tipo declarado da expressão.
    pub z_type: Option<Vec<u8>>,
    /// Nome do banco de origem.
    pub z_orig_db: Option<Vec<u8>>,
    /// Nome da tabela de origem.
    pub z_orig_tab: Option<Vec<u8>>,
    /// Nome da coluna de origem.
    pub z_orig_col: Option<Vec<u8>>,
}

/// `columnTypeImpl`: o "tipo declarado" da expressão: o tipo exato extraído do CREATE TABLE
/// original se a expressão é uma coluna. O tipo declarado de um ROWID é INTEGER. Quando uma
/// expressão conta como coluna pode ser complexo com subconsultas; o resultado de todos estes
/// SELECT conta como coluna:
///
/// ```text
/// SELECT col FROM tbl;
/// SELECT (SELECT col FROM tbl);
/// SELECT abc FROM (SELECT col AS abc FROM tbl);
/// ```
///
/// Para qualquer expressão que não seja coluna, o tipo declarado é NULL.
fn column_type_impl(db: &Connection, p_nc: &ColTypeCtx<'_>, p_expr: &Expr) -> ColumnTypeInfo {
    let mut info = ColumnTypeInfo::default();
    match p_expr.op {
        TK_COLUMN => {
            // A expressão é uma coluna. Localiza a tabela de onde ela sai em `pSrcList` do
            // contexto; pode ser uma tabela real do banco ou uma subconsulta.
            let mut p_tab: Option<Rc<Table>> = None;
            let mut p_s: Option<&Select> = None;
            let mut i_col = p_expr.i_column;
            let mut nc: Option<&ColTypeCtx<'_>> = Some(p_nc);
            while let Some(c) = nc {
                if p_tab.is_some() {
                    break;
                }
                let found = c
                    .p_src_list
                    .and_then(|l| l.a.iter().find(|it| it.i_cursor == p_expr.i_table));
                if let Some(it) = found {
                    p_tab = it.p_tab.clone();
                    p_s = it.p_select.as_deref();
                    if p_tab.is_some() {
                        break;
                    }
                }
                nc = c.p_next;
            }

            // Antigamente "SELECT new.x" num gatilho chegava aqui. Hoje ainda vale para instruções
            // como "CREATE TABLE t1(col INTEGER); SELECT (SELECT t1.col) FROM t1;", quando
            // columnType() é chamada em "t1.col" do sub-select: o tipo fica NULL, embora devesse ser
            // INTEGER. Não é problema: o tipo de "t1.col" nunca é usado; ao chamar em
            // "(SELECT t1.col)" o ramo TK_SELECT devolve o tipo certo.
            let Some(p_tab) = p_tab else {
                return info;
            };

            if let Some(p_s) = p_s {
                // A "tabela" é na verdade um sub-select ou uma view na cláusula FROM do SELECT.
                // Devolve o tipo declarado e os dados de origem da coluna do resultado do
                // sub-select. (Com `ViewCanHaveRowid` igual a 0, iCol<0 nunca chega aqui; o teste
                // `i_col >= 0` só evita a leitura fora do array que o C faria.)
                if i_col >= 0 && i_col < n_expr(p_s.p_e_list.as_deref()) {
                    let p = p_s
                        .p_e_list
                        .as_deref()
                        .and_then(|l| l.a[i_col as usize].p_expr.as_deref());
                    let s_nc = ColTypeCtx { p_src_list: p_s.p_src.as_deref(), p_next: nc };
                    if let Some(p) = p {
                        info = column_type_impl(db, &s_nc, p);
                    }
                }
            } else {
                // Uma tabela real ou uma tabela CTE.
                if i_col < 0 {
                    i_col = p_tab.i_p_key as i32;
                }
                if i_col < 0 {
                    info.z_type = Some(b"INTEGER".to_vec());
                    info.z_orig_col = Some(b"rowid".to_vec());
                } else {
                    let p_col = &p_tab.a_col[i_col as usize];
                    info.z_orig_col = Some(column_name(p_col).to_vec());
                    info.z_type = { let z = column_type(p_col, b""); if z.is_empty() { None } else { Some(z.to_vec()) } };
                }
                info.z_orig_tab = Some(p_tab.z_name.clone());
                if p_tab.p_schema.0 != 0 {
                    if let Some(i_db) = db.dbs.iter().position(|d| d.schema.id == p_tab.p_schema) {
                        info.z_orig_db = Some(db.dbs[i_db].z_db_s_name.clone());
                    }
                }
            }
        }
        TK_SELECT => {
            // A expressão é um sub-select. Devolve o tipo declarado e a origem da única coluna do
            // resultado do SELECT.
            if let Some(p_s) = p_expr.x_select() {
                let p = p_s.p_e_list.as_deref().and_then(|l| l.a.first()).and_then(|it| it.p_expr.as_deref());
                let s_nc = ColTypeCtx { p_src_list: p_s.p_src.as_deref(), p_next: Some(p_nc) };
                if let Some(p) = p {
                    info = column_type_impl(db, &s_nc, p);
                }
            }
        }
        _ => {}
    }
    info
}

/// `generateColumnTypes`: gera o código que informa ao VDBE os tipos declarados das colunas do
/// resultado.
pub(crate) fn generate_column_types(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: &SrcList,
    p_e_list: &ExprList,
) {
    let s_nc = ColTypeCtx { p_src_list: Some(p_tab_list), p_next: None };
    for (i, item) in p_e_list.a.iter().enumerate() {
        let Some(p) = item.p_expr.as_deref() else {
            continue;
        };
        let info = column_type_impl(db, &s_nc, p);

        // O vdbe precisa fazer a própria cópia do tipo e das demais cadeias específicas da coluna,
        // caso o esquema seja reiniciado antes de a máquina virtual ser apagada.
        let v = vdbe_of_parse(parse);
        let i = i as i32;
        set_col_name(v, db, i, COLNAME_DATABASE, info.z_orig_db.as_deref(), StrDtor::Transient);
        set_col_name(v, db, i, COLNAME_TABLE, info.z_orig_tab.as_deref(), StrDtor::Transient);
        set_col_name(v, db, i, COLNAME_COLUMN, info.z_orig_col.as_deref(), StrDtor::Transient);
        set_col_name(v, db, i, COLNAME_DECLTYPE, info.z_type.as_deref(), StrDtor::Transient);
    }
}

/// `sqlite3GenerateColumnNames`: calcula os nomes das colunas de um SELECT.
///
/// A única garantia do SQLite é que, se a coluna tem um AS, esse é o nome. Mesmo assim incontáveis
/// aplicações assumiram coisas sobre os nomes, então esta rotina muda com extremo cuidado.
///
/// Os PRAGMA short_column_names e full_column_names estão obsoletos (padrão: short=ON, full=OFF),
/// mas ainda são suportados:
///
/// - short=OFF, full=OFF: o texto da expressão como aparece no SELECT (o zSpan).
/// - short=ON, full=OFF (padrão): se o resultado é uma coluna de tabela, só COLUMN; senão o zSpan.
/// - full=ON, short=ANY: se o resultado é uma coluna de tabela, TABLE.COLUMN; senão o zSpan.
///
/// `p_select` pode ser qualquer select do composto; os nomes vêm do mais à esquerda.
pub fn generate_column_names(db: &mut Connection, parse: &mut Parse, p_select: &Select) {
    if parse.col_names_set != 0 {
        return;
    }
    // Os nomes das colunas são os do termo mais à esquerda de um select composto.
    let mut p_select = p_select;
    while let Some(prior) = p_select.p_prior.as_deref() {
        p_select = prior;
    }
    let Some(p_tab_list) = p_select.p_src.as_deref() else {
        return;
    };
    let Some(p_e_list) = p_select.p_e_list.as_deref() else {
        return;
    };
    parse.col_names_set = 1;
    let full_name = (db.flags & SQLITE_FULL_COL_NAMES) != 0;
    let src_name = (db.flags & SQLITE_SHORT_COL_NAMES) != 0 || full_name;
    set_num_cols(vdbe_of_parse(parse), p_e_list.a.len() as i32);
    for (i, item) in p_e_list.a.iter().enumerate() {
        let i = i as i32;
        let Some(p) = item.p_expr.as_deref() else {
            continue;
        };
        if item.z_e_name.is_some() && item.fg.e_e_name == ENAME_NAME {
            // Um AS sempre tem a primeira prioridade.
            let z_name = item.z_e_name.as_deref();
            set_col_name(vdbe_of_parse(parse), db, i, COLNAME_NAME, z_name, StrDtor::Transient);
        } else if src_name && p.op == TK_COLUMN {
            let mut i_col = p.i_column;
            let Some(TabRef::Rc(p_tab)) = p.y_tab() else {
                continue;
            };
            if i_col < 0 {
                i_col = p_tab.i_p_key as i32;
            }
            let z_col: &[u8] = if i_col < 0 { b"rowid" } else { column_name(&p_tab.a_col[i_col as usize]) };
            if full_name {
                let mut z_name = p_tab.z_name.clone();
                z_name.push(b'.');
                z_name.extend_from_slice(z_col);
                set_col_name(vdbe_of_parse(parse), db, i, COLNAME_NAME, Some(&z_name), StrDtor::Transient);
            } else {
                set_col_name(vdbe_of_parse(parse), db, i, COLNAME_NAME, Some(z_col), StrDtor::Transient);
            }
        } else {
            let z = match item.z_e_name.as_deref() {
                None => mprintf(b"column%d", &[PrintfArg::Int(i as i64 + 1)]),
                Some(z) => Some(z.to_vec()),
            };
            set_col_name(vdbe_of_parse(parse), db, i, COLNAME_NAME, z.as_deref(), StrDtor::Transient);
        }
    }
    generate_column_types(db, parse, p_tab_list, p_e_list);
}

/// `sqlite3ColumnsFromExprList`: dada uma lista de expressões (a lista de resultado de um SELECT),
/// calcula nomes de coluna apropriados para uma tabela que guardaria essa lista. Todos os nomes
/// saem únicos. Só os nomes são calculados: `Column.zType`, `Column.zColl` e o resto ficam zerados.
///
/// Devolve `SQLITE_OK`. Com erro em `parse`, devolve `parse.rc` e deixa `*pn_col = 0` e
/// `pa_col` vazio. Ver também `generate_column_names`.
pub fn columns_from_expr_list(
    db: &mut Connection,
    parse: &mut Parse,
    p_e_list: Option<&ExprList>,
    pn_col: &mut i16,
    pa_col: &mut Vec<Column>,
) -> i32 {
    let mut ht: Hash<usize> = hash_init();
    let mut n_col: usize = p_e_list.map_or(0, |l| l.a.len());
    let mut a_col: Vec<Column> = vec![Column::default(); n_col];
    if n_col > 32767 {
        n_col = 32767;
    }
    *pn_col = n_col as i16;

    let mut i = 0;
    while i < n_col && parse.n_err == 0 {
        let Some(p_e_list) = p_e_list else {
            break;
        };
        let p_x = &p_e_list.a[i];
        // Obtém um nome apropriado para a coluna.
        let mut z_name: Option<Vec<u8>> = p_x.z_e_name.clone();
        if z_name.is_some() && p_x.fg.e_e_name == ENAME_NAME {
            // Se a coluna tem "AS <nome>", usa <nome>.
        } else {
            let mut p_col_expr = expr_skip_collate_and_likely(p_x.p_expr.as_deref());
            while let Some(e) = p_col_expr {
                if e.op != TK_DOT {
                    break;
                }
                p_col_expr = e.p_right.as_deref();
            }
            if let Some(e) = p_col_expr {
                if let (TK_COLUMN, Some(TabRef::Rc(p_tab))) = (e.op, e.y_tab()) {
                    // Para colunas usa o nome da coluna.
                    let mut i_col = e.i_column;
                    if i_col < 0 {
                        i_col = p_tab.i_p_key as i32;
                    }
                    z_name = Some(if i_col >= 0 {
                        column_name(&p_tab.a_col[i_col as usize]).to_vec()
                    } else {
                        b"rowid".to_vec()
                    });
                } else if e.op == TK_ID {
                    debug_assert!(!e.has_property(EP_INT_VALUE));
                    z_name = e.z_token().map(|z| z.to_vec());
                } else {
                    // Usa o texto original da expressão da coluna como nome (z_name já é o zEName).
                }
            }
        }
        let mut z_name: Vec<u8> = match z_name {
            Some(z) if is_true_or_false(&z[..strlen30(&z) as usize]) == 0 => z,
            _ => mprintf(b"column%d", &[PrintfArg::Int(i as i64 + 1)]).unwrap_or_default(),
        };

        // Garante que o nome da coluna é único. Se não é, acrescenta um inteiro ao nome.
        let mut cnt: u32 = 0;
        while let Some(&i_collide) = hash_find(&ht, &z_name) {
            if p_e_list.a[i_collide].fg.b_using_term {
                a_col[i].col_flags |= COLFLAG_NOEXPAND;
            }
            let mut n_name = strlen30(&z_name) as usize;
            if n_name > 0 {
                let mut j = n_name - 1;
                while j > 0 && is_digit(z_name[j]) {
                    j -= 1;
                }
                if z_name[j] == b':' {
                    n_name = j;
                }
            }
            cnt = cnt.wrapping_add(1);
            // `%.*z:%u`
            let mut z_new = z_name[..n_name].to_vec();
            z_new.push(b':');
            z_new.extend_from_slice(cnt.to_string().as_bytes());
            z_name = z_new;
            progress_check(parse, db);
            if cnt > 3 {
                let mut b = [0u8; 4];
                randomness(&mut b);
                cnt = u32::from_ne_bytes(b);
            }
        }
        // `zCnName` guarda `nome\0` (o tipo e a colação vêm depois, ver `Column`).
        let p_col = &mut a_col[i];
        p_col.h_name = str_i_hash(&z_name);
        let mut z_cn = z_name.clone();
        z_cn.push(0);
        p_col.z_cn_name = z_cn;
        if p_x.fg.b_no_expand {
            p_col.col_flags |= COLFLAG_NOEXPAND;
        }
        // `sqlite3ColumnPropertiesFromName` é macro vazia sem SQLITE_ENABLE_HIDDEN_COLUMNS.
        hash_insert(&mut ht, &z_name, Some(i));
        i += 1;
    }
    hash_clear(&mut ht);
    if parse.n_err != 0 {
        *pa_col = Vec::new();
        *pn_col = 0;
        return parse.rc;
    }
    *pa_col = a_col;
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Chunk 005: tipos de subconsulta, SELECT como tabela, LIMIT, composto, recursão
// ---------------------------------------------------------------------------------------------

/// A expressão do resultado `i` do select `s`.
fn result_expr(s: &Select, i: usize) -> Option<&Expr> {
    s.p_e_list.as_deref()?.a.get(i)?.p_expr.as_deref()
}

/// `sqlite3ExprAffinity` da coluna `i` do resultado de `s`.
fn result_affinity(s: &Select, i: usize) -> u8 {
    result_expr(s, i).map_or(SQLITE_AFF_NONE, |e| expr_affinity(e, None))
}

/// `sqlite3SubqueryColumnTypes`: `p_tab` é uma `Table` transitória que representa uma subconsulta
/// (um parêntese no FROM, uma VIEW ou uma CTE). Calcula o tipo de cada coluna a partir do `Select`
/// que implementa a subconsulta: o nome do tipo de dado (como num CREATE TABLE), a colação e a
/// afinidade. `p_select` é o select mais à direita do composto (a raiz); `aff` é a afinidade
/// padrão (`SQLITE_AFF_NONE` ou `SQLITE_AFF_BLOB`).
pub fn subquery_column_types(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &mut Table,
    p_select: &Select,
    aff: u8,
) {
    debug_assert!((p_select.sel_flags & SF_RESOLVED) != 0);
    debug_assert!(aff == SQLITE_AFF_NONE || aff == SQLITE_AFF_BLOB);
    if db.malloc_failed != 0 || parse.in_rename_object() {
        return;
    }
    // Da esquerda para a direita: `chain[0]` é o `pSelect` do C depois de `while(pSelect->pPrior)`
    // e o `pNext` de um elemento é o seguinte da lista.
    let chain = p_select.compound_chain();
    let n_chain = chain.len();
    let p_first = chain[0];
    let s_nc = ColTypeCtx { p_src_list: p_first.p_src.as_deref(), p_next: None };
    let n_col = (p_tab.n_col.max(0) as usize).min(p_tab.a_col.len());
    for i in 0..n_col {
        let mut m: i32 = 0;
        let mut k = 0usize; // índice de pS2 em `chain`
        p_tab.tab_flags |= (p_tab.a_col[i].col_flags & COLFLAG_NOINSERT) as u32;
        let p = result_expr(p_first, i);
        // pCol->szEst = ... // O tamanho estimado de colunas de tabelas SELECT nunca é usado.
        let mut affinity = p.map_or(SQLITE_AFF_NONE, |e| expr_affinity(e, None));
        while affinity <= SQLITE_AFF_NONE && k + 1 < n_chain {
            m |= expr_data_type(result_expr(chain[k], i), None);
            k += 1;
            affinity = result_affinity(chain[k], i);
        }
        if affinity <= SQLITE_AFF_NONE {
            affinity = aff;
        }
        if affinity >= SQLITE_AFF_TEXT && (k + 1 < n_chain || k != 0) {
            for s2 in chain.iter().skip(k + 1) {
                m |= expr_data_type(result_expr(s2, i), None);
            }
            if affinity == SQLITE_AFF_TEXT && (m & 0x01) != 0 {
                affinity = SQLITE_AFF_BLOB;
            } else if affinity >= SQLITE_AFF_NUMERIC && (m & 0x02) != 0 {
                affinity = SQLITE_AFF_BLOB;
            }
            if affinity >= SQLITE_AFF_NUMERIC && p.map_or(false, |e| e.op == TK_CAST) {
                affinity = SQLITE_AFF_FLEXNUM;
            }
        }
        let mut z_type = p.and_then(|e| column_type_impl(db, &s_nc, e).z_type);
        if z_type.as_deref().map_or(true, |zt| affinity != affinity_type(zt, None)) {
            if affinity == SQLITE_AFF_NUMERIC || affinity == SQLITE_AFF_FLEXNUM {
                z_type = Some(b"NUM".to_vec());
            } else {
                z_type = None;
                for j in 1..SQLITE_N_STDTYPE {
                    if STD_TYPE_AFFINITY[j] == affinity {
                        z_type = Some(STD_TYPE[j].as_bytes().to_vec());
                        break;
                    }
                }
            }
        }
        let p_col = &mut p_tab.a_col[i];
        p_col.affinity = affinity;
        if let Some(zt) = z_type {
            // `nome\0tipo\0`; o realloc do C descarta a colação antiga.
            let n = strlen30(&p_col.z_cn_name) as usize;
            let mut z_cn = p_col.z_cn_name[..n].to_vec();
            z_cn.push(0);
            z_cn.extend_from_slice(&zt[..strlen30(&zt) as usize]);
            z_cn.push(0);
            p_col.z_cn_name = z_cn;
            p_col.col_flags &= !(COLFLAG_HASTYPE | COLFLAG_HASCOLL);
            p_col.col_flags |= COLFLAG_HASTYPE;
        }
        let p_coll = expr_coll_seq(db, parse, p, None);
        if let Some(c) = p_coll {
            debug_assert!(p_tab.p_index.is_empty());
            column_set_coll(&mut p_tab.a_col[i], &c.name);
        }
    }
    p_tab.sz_tab_row = 1; // Qualquer valor diferente de zero serve.
}

/// `sqlite3ResultSetOfSelect`: dado um SELECT, gera a `Table` que descreve o conjunto de resultado
/// dele. Devolve `None` com erro. `p_select` é a raiz do composto; o chamador envolve a `Table` num
/// `Rc` se for guardá-la.
pub fn result_set_of_select(
    db: &mut Connection,
    parse: &mut Parse,
    p_select: &mut Select,
    aff: u8,
) -> Option<Table> {
    let saved_flags = db.flags;
    db.flags &= !SQLITE_FULL_COL_NAMES;
    db.flags |= SQLITE_SHORT_COL_NAMES;
    select_prep(db, parse, p_select, None);
    db.flags = saved_flags;
    if parse.n_err != 0 {
        return None;
    }
    let mut p_tab = Table::default();
    p_tab.z_name = Vec::new();
    p_tab.n_row_log_est = 200;
    debug_assert!(200 == log_est(1048576));
    let mut n_col = 0i16;
    let mut a_col = Vec::new();
    columns_from_expr_list(
        db,
        parse,
        p_select.compound_chain()[0].p_e_list.as_deref(),
        &mut n_col,
        &mut a_col,
    );
    p_tab.n_col = n_col;
    p_tab.a_col = a_col;
    subquery_column_types(db, parse, &mut p_tab, p_select, aff);
    p_tab.i_p_key = -1;
    Some(p_tab)
}

/// `sqlite3GetVdbe`: devolve o VDBE do contexto de análise, criando um se necessário.
pub fn get_vdbe<'a>(db: &Connection, parse: &'a mut Parse) -> &'a mut Vdbe {
    if parse.p_vdbe.is_none() {
        if parse.p_toplevel.is_none() && db.optimization_enabled(SQLITE_FACTOR_OUT_CONST) {
            parse.ok_const_factor = 1;
        }
        vdbe_create(parse, db);
    }
    vdbe_of_parse(parse)
}

/// `computeLimitRegisters`: calcula `i_limit` e `i_offset` do SELECT a partir das expressões de
/// `p_limit` (`p_left` guarda o que aparece depois de LIMIT e `p_right` o que vem depois de
/// OFFSET). Esta rotina só muda os valores se `p_limit.p_left`/`p_right` definem algum. O registrador `i_offset` (se
/// existe) é inicializado com o OFFSET, `i_limit` com o LIMIT e `i_offset+1` com LIMIT+OFFSET.
///
/// Só se `p_limit.p_left` não é nulo os registradores são redefinidos: o UNION ALL usa isso para
/// forçar o reuso dos mesmos registradores em vários SELECT.
///
/// "LIMIT -1" sempre mostra todas as linhas; "LIMIT 0" significa nenhuma linha.
pub(crate) fn compute_limit_registers(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    i_break: i32,
) {
    if p.i_limit != 0 {
        return;
    }
    let Some(p_limit) = p.p_limit.as_deref_mut() else {
        return;
    };
    debug_assert!(p_limit.op == TK_LIMIT);
    debug_assert!(p_limit.p_left.is_some());
    parse.n_mem += 1;
    let i_limit = parse.n_mem;
    p.i_limit = i_limit;
    get_vdbe(db, parse);
    let mut n = 0i32;
    let left_is_int = p_limit.p_left.as_deref().map_or(false, |l| expr_is_integer(l, &mut n) != 0);
    if left_is_int {
        let v = vdbe_of_parse(parse);
        add_op2(v, OP_INTEGER as i32, n, i_limit);
        vdbe_comment(v, b"LIMIT counter", &[]);
        if n == 0 {
            vdbe_goto(v, i_break);
        } else if n >= 0 && p.n_select_row > log_est(n as u64) {
            p.n_select_row = log_est(n as u64);
            p.sel_flags |= SF_FIXEDLIMIT;
        }
    } else {
        if let Some(l) = p_limit.p_left.as_deref_mut() {
            expr_code(db, parse, l, i_limit, None);
        }
        let v = vdbe_of_parse(parse);
        add_op1(v, OP_MUSTBEINT as i32, i_limit);
        vdbe_comment(v, b"LIMIT counter", &[]);
        add_op2(v, OP_IFNOT as i32, i_limit, i_break);
    }
    if let Some(r) = p_limit.p_right.as_deref_mut() {
        parse.n_mem += 1;
        let i_offset = parse.n_mem;
        p.i_offset = i_offset;
        parse.n_mem += 1; // Aloca um registrador extra para limit+offset.
        expr_code(db, parse, r, i_offset, None);
        let v = vdbe_of_parse(parse);
        add_op1(v, OP_MUSTBEINT as i32, i_offset);
        vdbe_comment(v, b"OFFSET counter", &[]);
        add_op3(v, OP_OFFSETLIMIT as i32, i_limit, i_offset + 1, i_offset);
        vdbe_comment(v, b"LIMIT+OFFSET", &[]);
    }
}

/// `multiSelectCollSeq`: a colação apropriada da coluna `i_col` do resultado do select composto
/// `p`, ou `None` se a coluna não tem colação padrão. A colação vem do termo mais à esquerda que
/// tem uma.
pub(crate) fn multi_select_coll_seq(
    db: &mut Connection,
    parse: &mut Parse,
    p: &Select,
    i_col: i32,
) -> Option<Rc<CollSeq>> {
    let mut p_ret = match p.p_prior.as_deref() {
        Some(prior) => multi_select_coll_seq(db, parse, prior, i_col),
        None => None,
    };
    debug_assert!(i_col >= 0);
    // `i_col` é menor que nExpr: senão a resolução de nomes teria dado erro antes de chegar aqui.
    if p_ret.is_none() && i_col >= 0 && i_col < n_expr(p.p_e_list.as_deref()) {
        p_ret = expr_coll_seq(db, parse, result_expr(p, i_col as usize), None);
    }
    p_ret
}

/// `multiSelectOrderByKeyInfo`: `p` é um select composto com ORDER BY; devolve o `KeyInfo` que
/// implementa esse ORDER BY. Como no C, acrescenta aos termos sem COLLATE o COLLATE da coluna do
/// resultado.
pub(crate) fn multi_select_order_by_key_info(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    n_extra: i32,
) -> Rc<KeyInfo> {
    // O ORDER BY sai do select durante o laço para `multi_select_coll_seq` poder ler `p` inteiro.
    let mut p_order_by = p.p_order_by.take();
    let n_order_by = n_expr(p_order_by.as_deref());
    let mut p_ret = key_info_alloc(db, n_order_by + n_extra, 1);
    if let Some(ob) = p_order_by.as_deref_mut() {
        for i in 0..n_order_by as usize {
            let p_item = &mut ob.a[i];
            let p_coll;
            let has_collate = p_item.p_expr.as_deref().map_or(false, |t| (t.flags & EP_COLLATE) != 0);
            if has_collate {
                p_coll = expr_coll_seq(db, parse, p_item.p_expr.as_deref(), None);
            } else {
                let mut c = multi_select_coll_seq(db, parse, p, p_item.i_order_by_col as i32 - 1);
                if c.is_none() {
                    c = db.p_dflt_coll.clone();
                }
                if let Some(cc) = c.as_ref() {
                    p_item.p_expr = expr_add_collate_string(p_item.p_expr.take(), &cc.name);
                }
                p_coll = c;
            }
            p_ret.a_coll[i] = p_coll;
            p_ret.a_sort_flags[i] = p_item.fg.sort_flags;
        }
    }
    p.p_order_by = p_order_by;
    Rc::new(p_ret)
}

/// O select a `n` passos de `p` seguindo `p_prior` (`n == 0` é o próprio `p`).
pub(crate) fn nth_prior_mut(p: &mut Select, n: usize) -> &mut Select {
    let mut cur = p;
    for _ in 0..n {
        cur = cur.p_prior.as_deref_mut().expect("pPrior");
    }
    cur
}

/// `generateWithRecursiveQuery`: gera o código VDBE que calcula o conteúdo de uma consulta WITH
/// RECURSIVE da forma
///
/// ```text
///   <tabela-recursiva> AS (<consulta-inicial> UNION [ALL] <consulta-recursiva>)
///                          \_________________/             \________________/
///                              p.p_prior                          p
/// ```
///
/// Há exatamente uma referência à tabela recursiva no FROM da consulta recursiva, marcada com
/// `SrcItem.fg.is_recursive`.
///
/// A consulta inicial roda uma vez e gera o conjunto inicial de linhas, que vão para uma tabela
/// Queue. As linhas saem da Queue uma a uma e vão para `p_dest`. A linha extraída (agora na tabela
/// iCurrent) vira o conteúdo da tabela recursiva para uma rodada da consulta recursiva, cuja saída
/// volta para a Queue. Repete até a Queue esvaziar.
///
/// Se o operador é UNION, nenhuma linha duplicada entra na Queue: a tabela iDistinct guarda cópia
/// de tudo o que já entrou e descarta duplicatas. Com UNION ALL duplicatas valem. Com ORDER BY, as
/// entradas da Queue ficam em ordem e a primeira é extraída a cada ciclo; sem ele, é uma FIFO.
///
/// Com LIMIT, a iteração para depois de LIMIT linhas; LIMIT zero não emite nenhuma e negativo emite
/// todas. Com OFFSET positivo, as primeiras OFFSET saídas são descartadas e o LIMIT só começa a
/// contar depois.
pub(crate) fn generate_with_recursive_query(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) {
    let n_col = n_expr(p.p_e_list.as_deref());
    let mut i_current = 0;
    let mut i_distinct = 0;

    if p.n_win_linked > 0 {
        error_msg(db, parse, b"cannot use window functions in recursive queries", &[]);
        return;
    }

    // Obtém autorização para fazer uma consulta recursiva.
    if auth_check(db, parse, SQLITE_RECURSIVE, None, None, None) != 0 {
        return;
    }

    // Processa as cláusulas LIMIT e OFFSET, se existem.
    let addr_break = make_label(parse);
    p.n_select_row = 320; // 4 bilhões de linhas
    compute_limit_registers(db, parse, p, addr_break);
    let p_limit = p.p_limit.take();
    let reg_limit = p.i_limit;
    let reg_offset = p.i_offset;
    p.i_limit = 0;
    p.i_offset = 0;
    let has_order_by = p.p_order_by.is_some();

    // Localiza o número do cursor da tabela Current.
    if let Some(p_src) = p.p_src.as_deref() {
        for it in p_src.a.iter() {
            if it.fg.is_recursive {
                i_current = it.i_cursor;
                break;
            }
        }
    }

    // Aloca os números de cursor de Queue e Distinct. O de Distinct deve ser exatamente um a mais
    // que o de Queue para os destinos SRT_DistFifo e SRT_DistQueue funcionarem.
    let i_queue = parse.n_tab;
    parse.n_tab += 1;
    let e_dest = if p.op == TK_UNION {
        i_distinct = parse.n_tab;
        parse.n_tab += 1;
        if has_order_by { SRT_DISTQUEUE } else { SRT_DISTFIFO }
    } else if has_order_by {
        SRT_QUEUE
    } else {
        SRT_FIFO
    };
    let mut dest_queue = SelectDest::default();
    select_dest_init(&mut dest_queue, e_dest as i32, i_queue);

    // Aloca os cursores de Current, Queue e Distinct.
    parse.n_mem += 1;
    let reg_current = parse.n_mem;
    add_op3(vdbe_of_parse(parse), OP_OPENPSEUDO as i32, i_current, reg_current, n_col);
    if has_order_by {
        let p_key_info = multi_select_order_by_key_info(db, parse, p, 1);
        let n_ob = n_expr(p.p_order_by.as_deref());
        add_op4(
            vdbe_of_parse(parse),
            OP_OPENEPHEMERAL as i32,
            i_queue,
            n_ob + 2,
            0,
            P4::KeyInfo(p_key_info),
        );
        // `destQueue.pOrderBy` é cópia de leitura do ORDER BY (ver a nota do módulo).
        dest_queue.p_order_by = p.p_order_by.clone();
    } else {
        add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, i_queue, n_col);
    }
    vdbe_comment(vdbe_of_parse(parse), b"Queue table", &[]);
    if i_distinct != 0 {
        p.addr_open_ephm[0] = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, i_distinct, 0);
        p.sel_flags |= SF_USESEPHEMERAL;
    }

    // Separa o ORDER BY do select composto.
    let p_order_by = p.p_order_by.take();

    // Descobre quantos elementos do select composto fazem parte da consulta recursiva. Garante que
    // nenhum elemento recursivo usa agregadas. Marca os elementos recursivos como UNION ALL mesmo
    // que sejam UNION, pois a distinção é imposta pela tabela iDistinct. `depth` fica no termo
    // recursivo mais à esquerda (pFirstRec).
    let mut setup: Option<Box<Select>> = None;
    let mut depth = 0usize;
    'end: {
        loop {
            let p_first_rec = nth_prior_mut(p, depth);
            if (p_first_rec.sel_flags & SF_AGGREGATE) != 0 {
                error_msg(db, parse, b"recursive aggregate queries not supported", &[]);
                break 'end;
            }
            p_first_rec.op = TK_ALL;
            let prior_recursive = p_first_rec
                .p_prior
                .as_deref()
                .map_or(false, |x| (x.sel_flags & SF_RECURSIVE) != 0);
            if !prior_recursive {
                break;
            }
            depth += 1;
        }

        // Guarda os resultados da consulta inicial na Queue. O select inicial sai da cadeia durante
        // a chamada (`pSetup->pNext = 0` no C) e volta depois.
        let mut p_setup = match nth_prior_mut(p, depth).p_prior.take() {
            Some(s) => s,
            None => break 'end,
        };
        p_setup.has_next = false;
        explain(parse, db, true, b"SETUP", &[]);
        let rc = select(db, parse, &mut p_setup, &mut dest_queue);
        p_setup.has_next = true;
        setup = Some(p_setup);
        if rc != 0 {
            break 'end;
        }

        // Acha a próxima linha da Queue e a emite.
        let addr_top = add_op2(vdbe_of_parse(parse), OP_REWIND as i32, i_queue, addr_break);

        // Transfere a próxima linha da Queue para Current.
        add_op1(vdbe_of_parse(parse), OP_NULLROW as i32, i_current); // Para limpar o cache de colunas.
        if p_order_by.is_some() {
            let n_ob = n_expr(p_order_by.as_deref());
            add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, i_queue, n_ob + 1, reg_current);
        } else {
            add_op2(vdbe_of_parse(parse), OP_ROWDATA as i32, i_queue, reg_current);
        }
        add_op1(vdbe_of_parse(parse), OP_DELETE as i32, i_queue);

        // Emite a única linha de Current.
        let addr_cont = make_label(parse);
        code_offset(vdbe_of_parse(parse), reg_offset, addr_cont);
        select_inner_loop(db, parse, p, i_current, None, None, p_dest, addr_cont, addr_break);
        if reg_limit != 0 {
            add_op2(vdbe_of_parse(parse), OP_DECRJUMPZERO as i32, reg_limit, addr_break);
        }
        resolve_label(parse, db, addr_cont);

        // Executa o SELECT recursivo com a única linha de Current como valor da tabela recursiva e
        // guarda os resultados na Queue.
        explain(parse, db, true, b"RECURSIVE STEP", &[]);
        select(db, parse, p, &mut dest_queue);

        // Continua o laço até a Queue esvaziar.
        vdbe_goto(vdbe_of_parse(parse), addr_top);
        resolve_label(parse, db, addr_break);
    }

    // end_of_recursive_query:
    if let Some(s) = setup {
        nth_prior_mut(p, depth).p_prior = Some(s);
    }
    p.p_order_by = p_order_by;
    p.p_limit = p_limit;
}
