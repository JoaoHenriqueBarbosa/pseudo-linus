//! Tradução de `select.c`, segunda parte (chunks `select_c.006` a `select_c.011`): o SELECT
//! composto (`multiSelect`, `multiSelectValues`, `multiSelectOrderBy` e a sub-rotina de saída), a
//! substituição de colunas do achatamento (`substExpr` e companhia), `flattenSubquery`, a
//! propagação de constantes do WHERE, o empurrão de termos do WHERE para subconsultas
//! (`pushDownWhereTerms`), a anulação de colunas de subconsulta não usadas, `minMaxQuery`,
//! `isSimpleCount`, `sqlite3IndexedByLookup`, a conversão de composto com ORDER BY em subconsulta
//! e a busca de CTE (`searchWith`). As decisões do modelo são as de `select.rs` (ver o cabeçalho
//! dele) e acrescentam o seguinte:
//!
//! - `Select.p_prior` possui o select da esquerda. Onde o C faz `p->pPrior = 0` para codificar `p`
//!   como se fosse simples, o `Box` sai de `p` (`take`) e volta depois; `p->pNext` é `has_next`.
//! - `findRightmost(p)->selFlags |= SF_UsesEphemeral` precisa do select mais à direita, que um
//!   elemento do meio da cadeia não alcança. Quem não é a raiz (`has_next`) grava o aviso numa
//!   variável de thread (`COMPOUND_USES_EPHEMERAL`) e a raiz o consome ao fim de `multi_select`,
//!   com guarda e restauração do valor anterior para as subconsultas aninhadas.
//! - `sqlite3Select` e `sqlite3SelectPrep` moram em `select3.rs` e este módulo as reexporta, que
//!   é onde `select.rs` e `resolve.rs` as procuram.
//! - A janela `Select.pWin` do C (a cabeça da lista) é achada por `first_window`: a janela
//!   ligada de menor `link_seq` entre as expressões do resultado e do ORDER BY.
//! - `SubstContext.pEList`/`pCList` são `&ExprList` de leitura. O `pParse->db` é o par
//!   `(db, parse)` guardado no contexto.
//! - `WhereConst` guarda, no lugar dos ponteiros `apExpr[i*2]`, o ENDEREÇO do nó da coluna (só
//!   para a comparação `pColumn==pExpr`, como `RenameToken.p`) mais `iTable`/`iColumn` e uma
//!   cópia do valor constante; as falhas de alocação (`pOomFault`) não existem.
//! - `recomputeColumnsUsed` recebe o índice do item na lista FROM do `Select` (o item vive dentro
//!   do select que o percurso varre) e acumula `colUsed` no contexto do walker.
//! - `With.pOuter` não existe: `searchWith` recebe a pilha de WITH do mais interno ao mais
//!   externo (`&[&With]`) e devolve (nível, índice da CTE); `with_push` segue o protocolo de
//!   `Parse.p_with` (mover para dentro e devolver o anterior).

use std::cell::Cell;
use std::rc::Rc;

use crate::auth::auth_check;
use crate::build::{src_list_append_from_term, src_list_enlarge};
use crate::connection::{Connection, Parse};
use crate::consts::{
    Bitmask, BMS, EP_CAN_BE_NULL, EP_COLLATE, EP_DISTINCT, EP_FIXED_COL, EP_IF_NULL_ROW,
    EP_INNER_ON, EP_INT_VALUE, EP_LEAF, EP_OUTER_ON, EP_SKIP, EP_UNLIKELY, EP_WIN_FUNC,
    JT_LTORJ, JT_OUTER, JT_RIGHT, KEYINFO_ORDER_BIGNULL, KEYINFO_ORDER_DESC, OPFLAG_APPEND,
    OPFLAG_PERMUTE, OP_CLOSE, OP_COMPARE, OP_COPY, OP_DECRJUMPZERO, OP_GOSUB, OP_IDXINSERT,
    OP_IFNOT, OP_INITCOROUTINE, OP_INSERT, OP_INTEGER, OP_JUMP, OP_MAKERECORD, OP_NEWROWID,
    OP_NEXT, OP_NOTFOUND, OP_OFFSETLIMIT, OP_OPENEPHEMERAL, OP_PERMUTATION, OP_RESULTROW,
    OP_RETURN, OP_REWIND, OP_ROWDATA, OP_YIELD, SF_AGGREGATE, SF_COMPOUND, SF_CONVERTED,
    SF_DISTINCT, SF_MULTIPART, SF_MULTIVALUE, SF_NOOPORDERBY, SF_PUSHDOWN,
    SF_RECURSIVE, SF_USESEPHEMERAL, SF_VALUES, SQLITE_AFF_BLOB, SQLITE_AFF_TEXT,
    SQLITE_BALANCED_MERGE, SQLITE_ERROR, SQLITE_FLTTN_UNION_ALL, SQLITE_FUNC_COUNT,
    SQLITE_MIN_MAX_OPT, SQLITE_NOMEM, SQLITE_OK, SQLITE_QUERY_FLATTENER, SQLITE_SELECT,
    SRT_COROUTINE, SRT_EPHEMTAB, SRT_EXCEPT, SRT_MEM, SRT_OUTPUT, SRT_SET, SRT_TABLE, SRT_UNION,
    TK_AGG_FUNCTION,
    TK_ALL, TK_AND, TK_ASTERISK, TK_COLLATE, TK_COLUMN, TK_EQ, TK_EXCEPT, TK_GE, TK_IF_NULL_ROW,
    TK_INTEGER, TK_INTERSECT, TK_IS, TK_NULL, TK_SELECT, TK_TRUEFALSE, TK_UNION, TOPBIT,
    WHERE_ORDERBY_MAX, WHERE_ORDERBY_MIN, WHERE_ORDERBY_NORMAL, WRC_ABORT, WRC_CONTINUE,
    WRC_PRUNE,
};
use crate::expr::{
    expr, expr_add_collate_string, expr_affinity, expr_and, expr_coll_seq, expr_compare_coll_seq,
    expr_dup, expr_is_vector, expr_list_append, expr_list_dup, p_expr, select_dup,
};
use crate::expr::{
    expr_can_be_null, expr_code_move, expr_is_constant, expr_is_constant_or_group_by,
    expr_is_integer, expr_is_single_table_constraint, expr_truth_value, get_temp_range,
    get_temp_reg, is_binary, release_temp_reg, vector_error_msg,
};
use crate::expr_code2::agg_info_persist_walker_init;
use crate::mem::{CollSeq, KeyInfo};
use crate::printf::PrintfArg;
use crate::resolve::{expr_col_used, resolve_order_group_by};
use crate::select::{
    code_offset, compute_limit_registers, current_addr, generate_with_recursive_query, get_vdbe,
    key_info_alloc, multi_select_coll_seq, multi_select_order_by_key_info, n_expr, nth_prior_mut,
    select_dest_init, select_inner_loop, select_op_name, set_join_expr, text, unset_join_expr,
    vdbe_of_parse,
};
use crate::sqlite_int::{
    AggInfo, AggInfoId, Expr, ExprList, ExprU, Select, SelectDest, SrcItem, SrcList, SrcU1, SrcU2,
    Table, Token, Walker, Window, With,
};
use crate::util::{error_msg, log_est, log_est_add, str_icmp};
use crate::vdbe_types::P4;
use crate::vdbeaux::{
    add_op1, add_op2, add_op3, add_op4, add_op4_int, change_p2, change_p4, change_p5,
    end_coroutine, explain, explain_pop, jump_here, make_label, noop_comment, resolve_label,
    vdbe_comment, vdbe_goto,
};
use crate::walker::{select_walk_noop, walk_expr, walk_select};

// `sqlite3Select` e `sqlite3SelectPrep` vivem em `select3.rs` (outra fatia); aqui são reexportadas.
pub use crate::select3::{select, select_prep};

thread_local! {
    /// Aviso de "o select mais à direita usa tabela efêmera" gravado por um elemento do meio de
    /// um composto (ver a nota do módulo).
    static COMPOUND_USES_EPHEMERAL: Cell<bool> = const { Cell::new(false) };
}

// ---------------------------------------------------------------------------------------------
// Chunk 006: SELECT composto
// ---------------------------------------------------------------------------------------------

/// `findRightmost(p)->selFlags |= SF_UsesEphemeral`: a raiz marca a si mesma; um elemento do meio
/// avisa a raiz pela variável de thread.
fn mark_rightmost_uses_ephemeral(p: &mut Select) {
    if p.has_next {
        COMPOUND_USES_EPHEMERAL.with(|c| c.set(true));
    } else {
        p.sel_flags |= SF_USESEPHEMERAL;
    }
}

/// `multiSelectValues`: trata o caso especial de um composto que nasce de uma cláusula VALUES.
/// Assim se evita a recursão profunda e não é preciso impor `SQLITE_LIMIT_COMPOUND_SELECT` num
/// VALUES. Devolve -1 se o select usa janelas (o chamador segue pelo caminho geral).
///
/// Como o Select nasce de um VALUES: (1) não há LIMIT/OFFSET, ou há um LIMIT de exatamente 1
/// (um VALUES dentro de uma expressão escalar), (2) todos os termos são UNION ALL, (3) não há
/// ORDER BY. No caso "LIMIT de exatamente 1" só o VALUES mais à esquerda precisa ser avaliado.
fn multi_select_values(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) -> i32 {
    let mut n_row: i32 = 1;
    let rc = 0;
    let b_show_all = p.p_limit.is_none();
    debug_assert!((p.sel_flags & SF_MULTIVALUE) != 0);
    // Desce ao select mais à esquerda contando as linhas.
    let mut depth = 0usize;
    {
        let mut cur: &Select = p;
        loop {
            debug_assert!((cur.sel_flags & SF_VALUES) != 0);
            if cur.n_win_linked > 0 {
                return -1;
            }
            match cur.p_prior.as_deref() {
                None => break,
                Some(prior) => {
                    cur = prior;
                    n_row += b_show_all as i32;
                    depth += 1;
                }
            }
        }
    }
    explain(
        parse,
        db,
        false,
        b"SCAN %d CONSTANT ROW%s",
        &[PrintfArg::Int(n_row as i64), text(if n_row == 1 { &b""[..] } else { &b"S"[..] })],
    );
    // Do mais à esquerda para a direita (o `p = p->pNext` do C).
    let mut d = depth;
    loop {
        let s = nth_prior_mut(p, d);
        select_inner_loop(db, parse, s, -1, None, None, p_dest, 1, 1);
        if !b_show_all {
            break;
        }
        s.n_select_row = n_row as i16;
        if d == 0 {
            break;
        }
        d -= 1;
    }
    rc
}

/// `hasAnchor`: verdadeiro se o SELECT, que se sabe ser a parte recursiva de uma CTE recursiva,
/// ainda tem os termos âncora presos. Se já foram removidos, devolve falso.
fn has_anchor(p: &Select) -> bool {
    let mut cur = Some(p);
    while let Some(s) = cur {
        if (s.sel_flags & SF_RECURSIVE) == 0 {
            return true;
        }
        cur = s.p_prior.as_deref();
    }
    false
}

/// `multiSelect`: processa um SELECT composto de duas ou mais consultas ligadas por UNION, UNION
/// ALL, EXCEPT ou INTERSECT.
///
/// `p` é a mais à direita das duas consultas; a da esquerda é `p.p_prior` (que também pode ser
/// composta, e então a rotina é chamada recursivamente). O resultado vai para o destino `p_dest`.
///
/// Exemplo: `SELECT a FROM t1 UNION SELECT b FROM t2 UNION SELECT c FROM t3` vira
/// `t3 -> t2 -> t1` pelos `p_prior`; chamada com a consulta de `t3`, `p_prior` é a de `t2` e
/// `p.op` é `TK_UNION`. Os selects sempre se agrupam da esquerda para a direita.
pub(crate) fn multi_select(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) -> i32 {
    if p.has_next {
        return multi_select_body(db, parse, p, p_dest);
    }
    // A raiz guarda e restaura o aviso, para que subconsultas aninhadas não se misturem.
    let saved = COMPOUND_USES_EPHEMERAL.with(|c| c.replace(false));
    let rc = multi_select_body(db, parse, p, p_dest);
    COMPOUND_USES_EPHEMERAL.with(|c| c.set(saved));
    rc
}

/// O corpo de `multiSelect` (ver `multi_select`).
fn multi_select_body(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut dest = p_dest.clone();
    // Cadeia de selects simples a apagar (`pDelete`); o `Drop` a libera ao sair.
    let mut p_delete: Option<Box<Select>> = None;

    // Só o último (mais à direita) SELECT da série pode ter ORDER BY ou LIMIT.
    debug_assert!(p.p_prior.is_some());
    debug_assert!((p.sel_flags & SF_RECURSIVE) == 0 || p.op == TK_ALL || p.op == TK_UNION);
    debug_assert!((p.sel_flags & SF_COMPOUND) != 0);
    get_vdbe(db, parse);

    'end: {
        // Cria a tabela temporária de destino, se for preciso.
        if dest.e_dest == SRT_EPHEMTAB {
            debug_assert!(p.p_e_list.is_some());
            add_op2(
                vdbe_of_parse(parse),
                OP_OPENEPHEMERAL as i32,
                dest.i_sd_parm,
                n_expr(p.p_e_list.as_deref()),
            );
            dest.e_dest = SRT_TABLE;
        }

        // Tratamento especial de um composto que nasce de uma cláusula VALUES.
        if (p.sel_flags & SF_MULTIVALUE) != 0 {
            rc = multi_select_values(db, parse, p, &mut dest);
            if rc >= 0 {
                break 'end;
            }
            rc = SQLITE_OK;
        }

        // Todos os SELECTs têm o mesmo número de elementos no resultado.
        debug_assert!(p.p_e_list.is_some());

        if (p.sel_flags & SF_RECURSIVE) != 0 && has_anchor(p) {
            generate_with_recursive_query(db, parse, p, &mut dest);
        } else if p.p_order_by.is_some() {
            // Os compostos com ORDER BY são tratados à parte.
            return multi_select_order_by(db, parse, p, p_dest);
        } else {
            if p.p_prior.as_deref().map_or(false, |x| x.p_prior.is_none()) {
                explain(parse, db, true, b"COMPOUND QUERY", &[]);
                explain(parse, db, true, b"LEFT-MOST SUBQUERY", &[]);
            }

            // Gera o código dos SELECTs da esquerda e da direita.
            match p.op {
                TK_ALL => {
                    let mut addr = 0;
                    let mut n_limit = 0;
                    let mut prior = p.p_prior.take().expect("pPrior");
                    debug_assert!(prior.p_limit.is_none());
                    prior.i_limit = p.i_limit;
                    prior.i_offset = p.i_offset;
                    prior.p_limit = p.p_limit.take();
                    rc = select(db, parse, &mut prior, &mut dest);
                    p.p_limit = prior.p_limit.take();
                    if rc != 0 {
                        p.p_prior = Some(prior);
                        break 'end;
                    }
                    p.i_limit = prior.i_limit;
                    p.i_offset = prior.i_offset;
                    if p.i_limit != 0 {
                        let v = vdbe_of_parse(parse);
                        addr = add_op1(v, OP_IFNOT as i32, p.i_limit);
                        vdbe_comment(v, b"Jump ahead if LIMIT reached", &[]);
                        if p.i_offset != 0 {
                            add_op3(v, OP_OFFSETLIMIT as i32, p.i_limit, p.i_offset + 1, p.i_offset);
                        }
                    }
                    explain(parse, db, true, b"UNION ALL", &[]);
                    rc = select(db, parse, p, &mut dest);
                    p_delete = p.p_prior.take();
                    p.n_select_row = log_est_add(p.n_select_row, prior.n_select_row);
                    p.p_prior = Some(prior);
                    let limit_is_int = match p.p_limit.as_deref().and_then(|l| l.p_left.as_deref()) {
                        Some(left) => expr_is_integer(left, &mut n_limit) != 0,
                        None => false,
                    };
                    if limit_is_int && n_limit > 0 && p.n_select_row > log_est(n_limit as u64) {
                        p.n_select_row = log_est(n_limit as u64);
                    }
                    if addr != 0 {
                        jump_here(vdbe_of_parse(parse), addr);
                    }
                }
                TK_EXCEPT | TK_UNION => {
                    let prior_op = SRT_UNION;
                    let union_tab;
                    if dest.e_dest == prior_op {
                        // Podemos reaproveitar a tabela temporária gerada por um SELECT à nossa direita.
                        debug_assert!(p.p_limit.is_none()); // Não permitido nos elementos da esquerda.
                        union_tab = dest.i_sd_parm;
                    } else {
                        // Precisamos criar a nossa tabela temporária para os resultados intermediários.
                        union_tab = parse.n_tab;
                        parse.n_tab += 1;
                        debug_assert!(p.p_order_by.is_none());
                        let addr = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, union_tab, 0);
                        debug_assert!(p.addr_open_ephm[0] == -1);
                        p.addr_open_ephm[0] = addr;
                        mark_rightmost_uses_ephemeral(p);
                        debug_assert!(p.p_e_list.is_some());
                    }

                    // Codifica os SELECTs à esquerda.
                    let mut prior = p.p_prior.take().expect("pPrior");
                    debug_assert!(prior.p_order_by.is_none());
                    let mut uniondest = SelectDest::default();
                    select_dest_init(&mut uniondest, prior_op as i32, union_tab);
                    rc = select(db, parse, &mut prior, &mut uniondest);
                    if rc != 0 {
                        p.p_prior = Some(prior);
                        break 'end;
                    }

                    // Codifica o SELECT corrente.
                    let op = if p.op == TK_EXCEPT {
                        SRT_EXCEPT
                    } else {
                        debug_assert!(p.op == TK_UNION);
                        SRT_UNION
                    };
                    let p_limit = p.p_limit.take();
                    uniondest.e_dest = op;
                    explain(
                        parse,
                        db,
                        true,
                        b"%s USING TEMP B-TREE",
                        &[text(select_op_name(p.op as i32).as_bytes())],
                    );
                    rc = select(db, parse, p, &mut uniondest);
                    debug_assert!(p.p_order_by.is_none());
                    p_delete = p.p_prior.take();
                    p.p_order_by = None;
                    if p.op == TK_UNION {
                        p.n_select_row = log_est_add(p.n_select_row, prior.n_select_row);
                    }
                    p.p_prior = Some(prior);
                    p.p_limit = p_limit;
                    p.i_limit = 0;
                    p.i_offset = 0;

                    // Converte os dados da tabela temporária na forma de que precisamos agora.
                    debug_assert!(union_tab == dest.i_sd_parm || dest.e_dest != prior_op);
                    debug_assert!(p.p_e_list.is_some());
                    if dest.e_dest != prior_op && db.malloc_failed == 0 {
                        let i_break = make_label(parse);
                        let i_cont = make_label(parse);
                        compute_limit_registers(db, parse, p, i_break);
                        add_op2(vdbe_of_parse(parse), OP_REWIND as i32, union_tab, i_break);
                        let i_start = current_addr(parse);
                        select_inner_loop(db, parse, p, union_tab, None, None, &mut dest, i_cont, i_break);
                        resolve_label(parse, db, i_cont);
                        add_op2(vdbe_of_parse(parse), OP_NEXT as i32, union_tab, i_start);
                        resolve_label(parse, db, i_break);
                        add_op2(vdbe_of_parse(parse), OP_CLOSE as i32, union_tab, 0);
                    }
                }
                _ => 'intersect: {
                    debug_assert!(p.op == TK_INTERSECT);
                    // INTERSECT é diferente dos outros: precisa de duas tabelas temporárias.
                    let tab1 = parse.n_tab;
                    parse.n_tab += 1;
                    let tab2 = parse.n_tab;
                    parse.n_tab += 1;
                    debug_assert!(p.p_order_by.is_none());

                    let addr = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, tab1, 0);
                    debug_assert!(p.addr_open_ephm[0] == -1);
                    p.addr_open_ephm[0] = addr;
                    mark_rightmost_uses_ephemeral(p);
                    debug_assert!(p.p_e_list.is_some());

                    // Codifica os SELECTs à esquerda na tabela temporária "tab1".
                    let mut prior = p.p_prior.take().expect("pPrior");
                    let mut intersectdest = SelectDest::default();
                    select_dest_init(&mut intersectdest, SRT_UNION as i32, tab1);
                    rc = select(db, parse, &mut prior, &mut intersectdest);
                    if rc != 0 {
                        p.p_prior = Some(prior);
                        break 'end;
                    }

                    // Codifica o SELECT corrente na tabela temporária "tab2".
                    let addr = add_op2(vdbe_of_parse(parse), OP_OPENEPHEMERAL as i32, tab2, 0);
                    debug_assert!(p.addr_open_ephm[1] == -1);
                    p.addr_open_ephm[1] = addr;
                    let p_limit = p.p_limit.take();
                    intersectdest.i_sd_parm = tab2;
                    explain(
                        parse,
                        db,
                        true,
                        b"%s USING TEMP B-TREE",
                        &[text(select_op_name(p.op as i32).as_bytes())],
                    );
                    rc = select(db, parse, p, &mut intersectdest);
                    p_delete = p.p_prior.take();
                    if p.n_select_row > prior.n_select_row {
                        p.n_select_row = prior.n_select_row;
                    }
                    p.p_prior = Some(prior);
                    p.p_limit = p_limit;

                    // Gera o código da interseção das duas tabelas temporárias.
                    if rc != 0 {
                        break 'intersect;
                    }
                    debug_assert!(p.p_e_list.is_some());
                    let i_break = make_label(parse);
                    let i_cont = make_label(parse);
                    compute_limit_registers(db, parse, p, i_break);
                    add_op2(vdbe_of_parse(parse), OP_REWIND as i32, tab1, i_break);
                    let r1 = get_temp_reg(parse);
                    let i_start = add_op2(vdbe_of_parse(parse), OP_ROWDATA as i32, tab1, r1);
                    add_op4_int(vdbe_of_parse(parse), OP_NOTFOUND as i32, tab2, i_cont, r1, 0);
                    release_temp_reg(parse, r1);
                    select_inner_loop(db, parse, p, tab1, None, None, &mut dest, i_cont, i_break);
                    resolve_label(parse, db, i_cont);
                    add_op2(vdbe_of_parse(parse), OP_NEXT as i32, tab1, i_start);
                    resolve_label(parse, db, i_break);
                    add_op2(vdbe_of_parse(parse), OP_CLOSE as i32, tab2, 0);
                    add_op2(vdbe_of_parse(parse), OP_CLOSE as i32, tab1, 0);
                }
            }

            if !p.has_next {
                explain_pop(parse);
            }
        }
        if parse.n_err != 0 {
            break 'end;
        }

        // Calcula as sequências de colação usadas pelas tabelas temporárias do composto e prende o
        // KeyInfo a todas elas. Esta seção só roda no SELECT mais à direita; o mais à direita
        // também a pula se não tem ORDER BY e não precisa de tabelas temporárias.
        if !p.has_next && COMPOUND_USES_EPHEMERAL.with(|c| c.get()) {
            p.sel_flags |= SF_USESEPHEMERAL;
        }
        if (p.sel_flags & SF_USESEPHEMERAL) != 0 {
            debug_assert!(!p.has_next);
            debug_assert!(p.p_e_list.is_some());
            let n_col = n_expr(p.p_e_list.as_deref());
            let mut p_key_info = key_info_alloc(db, n_col, 1);
            for i in 0..n_col {
                let c = multi_select_coll_seq(db, parse, p, i);
                p_key_info.a_coll[i as usize] = match c {
                    Some(c) => Some(c),
                    None => db.p_dflt_coll.clone(),
                };
            }
            let p_key_info: Rc<KeyInfo> = Rc::new(p_key_info);

            let mut p_loop: Option<&mut Select> = Some(&mut *p);
            while let Some(l) = p_loop {
                for i in 0..2 {
                    let addr = l.addr_open_ephm[i];
                    if addr < 0 {
                        // Se [0] não é usado, [1] também não é: dá para parar no primeiro livre.
                        debug_assert!(l.addr_open_ephm[1] < 0);
                        break;
                    }
                    let v = vdbe_of_parse(parse);
                    change_p2(v, addr, n_col);
                    change_p4(v, addr, P4::KeyInfo(Rc::clone(&p_key_info)));
                    l.addr_open_ephm[i] = -1;
                }
                p_loop = l.p_prior.as_deref_mut();
            }
        }
    }

    // multi_select_end:
    p_dest.i_sdst = dest.i_sdst;
    p_dest.n_sdst = dest.n_sdst;
    drop(p_delete);
    rc
}

// ---------------------------------------------------------------------------------------------
// Chunk 007: mensagem de erro, sub-rotina de saída, composto com ORDER BY
// ---------------------------------------------------------------------------------------------

/// `sqlite3SelectWrongNumTermsError`: mensagem de erro para quando dois ou mais termos de um
/// composto têm resultados de tamanhos diferentes.
pub fn select_wrong_num_terms_error(db: &mut Connection, parse: &mut Parse, op: u8, sel_flags: u32) {
    if (sel_flags & SF_VALUES) != 0 {
        error_msg(db, parse, b"all VALUES must have the same number of terms", &[]);
    } else {
        error_msg(
            db,
            parse,
            b"SELECTs to the left and right of %s do not have the same number of result columns",
            &[text(select_op_name(op as i32).as_bytes())],
        );
    }
}

/// `generateOutputSubroutine`: codifica uma sub-rotina de saída para a implementação por
/// co-rotina de um SELECT.
///
/// Os dados a emitir estão em `p_in.i_sdst`, `p_in.n_sdst` colunas; `p_dest` é para onde vai a
/// saída; `reg_return` é o registrador com o endereço de retorno da sub-rotina. Se `reg_prev > 0`
/// ele é o primeiro registrador de um vetor que guarda a saída anterior, e `mem[reg_prev]` é falso
/// se não houve saída anterior; então se gera o código que suprime duplicatas, comparando com
/// `p_key_info`. Se o LIMIT de `p.i_limit` é atingido, salta para `i_break`. Devolve o endereço.
fn generate_output_subroutine(
    db: &mut Connection,
    parse: &mut Parse,
    p: &Select,
    p_in: &SelectDest,
    p_dest: &mut SelectDest,
    reg_return: i32,
    reg_prev: i32,
    p_key_info: Option<&Rc<KeyInfo>>,
    i_break: i32,
) -> i32 {
    let addr = current_addr(parse);
    let i_continue = make_label(parse);

    // Suprime as duplicatas de UNION, EXCEPT e INTERSECT.
    if reg_prev != 0 {
        let v = vdbe_of_parse(parse);
        let addr1 = add_op1(v, OP_IFNOT as i32, reg_prev);
        let ki = Rc::clone(p_key_info.expect("pKeyInfo"));
        let addr2 = add_op4(
            v,
            OP_COMPARE as i32,
            p_in.i_sdst,
            reg_prev + 1,
            p_in.n_sdst,
            P4::KeyInfo(ki),
        );
        add_op3(v, OP_JUMP as i32, addr2 + 2, i_continue, addr2 + 2);
        jump_here(v, addr1);
        add_op3(v, OP_COPY as i32, p_in.i_sdst, reg_prev + 1, p_in.n_sdst - 1);
        add_op2(v, OP_INTEGER as i32, 1, reg_prev);
    }
    if db.malloc_failed != 0 {
        return 0;
    }

    // Suprime as primeiras OFFSET entradas se há cláusula OFFSET.
    code_offset(vdbe_of_parse(parse), p.i_offset, i_continue);

    match p_dest.e_dest {
        // Guarda o resultado como dado usando uma chave única.
        SRT_EPHEMTAB => {
            let r1 = get_temp_reg(parse);
            let r2 = get_temp_reg(parse);
            let v = vdbe_of_parse(parse);
            add_op3(v, OP_MAKERECORD as i32, p_in.i_sdst, p_in.n_sdst, r1);
            add_op2(v, OP_NEWROWID as i32, p_dest.i_sd_parm, r2);
            add_op3(v, OP_INSERT as i32, p_dest.i_sd_parm, r1, r2);
            change_p5(v, OPFLAG_APPEND as u16);
            release_temp_reg(parse, r2);
            release_temp_reg(parse, r1);
        }

        // Se criamos um conjunto para "expr IN (SELECT ...)".
        SRT_SET => {
            let r1 = get_temp_reg(parse);
            let aff = match p_dest.z_aff_sdst.as_ref() {
                Some(z) => {
                    let n = (p_in.n_sdst.max(0) as usize).min(z.len());
                    P4::Text(z[..n].to_vec())
                }
                None => P4::None,
            };
            let v = vdbe_of_parse(parse);
            add_op4(v, OP_MAKERECORD as i32, p_in.i_sdst, p_in.n_sdst, r1, aff);
            add_op4_int(v, OP_IDXINSERT as i32, p_dest.i_sd_parm, r1, p_in.i_sdst, p_in.n_sdst);
            release_temp_reg(parse, r1);
        }

        // Um SELECT escalar parte de uma expressão: guarda o resultado na célula de memória
        // apropriada e sai do laço. O SELECT pode devolver várias colunas se for o lado direito de
        // um IN com vetor. O LIMIT salta para fora do laço por nós.
        SRT_MEM => {
            expr_code_move(parse, p_in.i_sdst, p_dest.i_sd_parm, p_in.n_sdst);
        }

        // Os resultados ficam numa sequência de registradores a partir de `p_dest.i_sdst` e a
        // co-rotina cede a vez.
        SRT_COROUTINE => {
            if p_dest.i_sdst == 0 {
                p_dest.i_sdst = get_temp_range(parse, p_in.n_sdst);
                p_dest.n_sdst = p_in.n_sdst;
            }
            expr_code_move(parse, p_in.i_sdst, p_dest.i_sdst, p_in.n_sdst);
            add_op1(vdbe_of_parse(parse), OP_YIELD as i32, p_dest.i_sd_parm);
        }

        // Se nenhum dos anteriores, o destino só pode ser SRT_Output. Os resultados ficam numa
        // sequência de registradores e OP_ResultRow faz `sqlite3_step()` devolver a próxima linha.
        _ => {
            debug_assert!(p_dest.e_dest == SRT_OUTPUT);
            add_op2(vdbe_of_parse(parse), OP_RESULTROW as i32, p_in.i_sdst, p_in.n_sdst);
        }
    }

    // Salta para o fim do laço se o LIMIT foi atingido.
    if p.i_limit != 0 {
        add_op2(vdbe_of_parse(parse), OP_DECRJUMPZERO as i32, p.i_limit, i_break);
    }

    // Gera o retorno da sub-rotina.
    resolve_label(parse, db, i_continue);
    add_op1(vdbe_of_parse(parse), OP_RETURN as i32, reg_return);

    addr
}

/// `multiSelectOrderBy`: gerador alternativo de código de composto para quando há ORDER BY.
///
/// A consulta é `<selectA> <operador> <selectB> ORDER BY <lista>`, com o operador UNION ALL, UNION,
/// EXCEPT ou INTERSECT. A ideia é codificar A e B com o ORDER BY como co-rotinas, rodá-las em
/// paralelo e intercalar os resultados na saída. Além das duas co-rotinas há sete sub-rotinas:
///
/// - `outA`: move a saída da co-rotina A para a saída do composto.
/// - `outB`: idem para B (só UNION e UNION ALL; EXCEPT e INTERSECT nunca emitem linha só de B).
/// - `AltB`, `AeqB`, `AgtB`: chamadas quando há dado nas duas co-rotinas e A<B, A==B, A>B.
/// - `EofA`, `EofB`: chamadas quando os dados de A (ou de B) acabaram.
///
/// ```text
///              UNION ALL         UNION            EXCEPT          INTERSECT
///           -------------  -----------------  --------------  -----------------
///    AltB:   outA, nextA      outA, nextA       outA, nextA         nextA
///    AeqB:   outA, nextA         nextA             nextA         outA, nextA
///    AgtB:   outB, nextB      outB, nextB          nextB            nextB
///    EofA:   outB, nextB      outB, nextB          halt             halt
///    EofB:   outA, nextA      outA, nextA       outA, nextA         halt
/// ```
///
/// Em AltB, AeqB e AgtB, um EOF em A depois de nextA salta direto a EofA e um EOF em B depois de
/// nextB salta a EofB. Dentro de EofA e EofB, um EOF na entrada ou depois de nextX salta ao fim do
/// processamento do select. A remoção de duplicatas em UNION, EXCEPT e INTERSECT é feita na
/// sub-rotina de saída: o conjunto de registradores `reg_prev` guarda o último valor emitido e a
/// saída é pulada se o próximo resultado for igual.
///
/// O plano é implementar primeiro as duas co-rotinas e as sete sub-rotinas e depois a lógica de
/// controle no fim: `goto Init`, `coA`, `coB`, `outA`, `outB`, `EofA`, `EofB`, `AltB`, `AeqB`,
/// `AgtB`, `Init` (inicializa os registradores das co-rotinas, cede a A e a B), `Cmpr` (compara A
/// e B e salta a AltB, AeqB ou AgtB) e `End`. Elas não são chamadas por Gosub e não fazem Return.
pub(crate) fn multi_select_order_by(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_dest: &mut SelectDest,
) -> i32 {
    debug_assert!(p.p_order_by.is_some());
    let label_end = make_label(parse);
    let label_cmpr = make_label(parse);

    // Ajusta a cláusula ORDER BY.
    let op = p.op;
    debug_assert!(p.p_prior.as_deref().map_or(false, |x| x.p_order_by.is_none()));
    let mut n_order_by = n_expr(p.p_order_by.as_deref());

    // Para os operadores que não são UNION ALL é preciso garantir que o ORDER BY cubra todos os
    // termos do resultado. Acrescenta termos se for necessário.
    if op != TK_ALL {
        let n_result = n_expr(p.p_e_list.as_deref());
        let mut i = 1;
        while db.malloc_failed == 0 && i <= n_result {
            let found = p
                .p_order_by
                .as_deref()
                .map_or(false, |ob| ob.a.iter().any(|it| it.i_order_by_col as i32 == i));
            if !found {
                let Some(mut p_new) = expr(TK_INTEGER as i32, None) else {
                    return SQLITE_NOMEM;
                };
                p_new.flags |= EP_INT_VALUE;
                p_new.u = ExprU::IValue(i);
                p.p_order_by = expr_list_append(p.p_order_by.take(), Some(p_new));
                if let Some(ob) = p.p_order_by.as_deref_mut() {
                    ob.a[n_order_by as usize].i_order_by_col = i as u16;
                    n_order_by += 1;
                }
            }
            i += 1;
        }
    }

    // Calcula a permutação de comparação e o KeyInfo usados para decidir se a próxima linha vem de
    // A ou de B. Também acrescenta colações explícitas aos termos do ORDER BY, para que as
    // subconsultas da esquerda e da direita usem a colação correta.
    let mut a_permute: Vec<u32> = Vec::with_capacity(n_order_by as usize + 1);
    a_permute.push(n_order_by as u32);
    if let Some(ob) = p.p_order_by.as_deref() {
        for item in ob.a.iter().take(n_order_by as usize) {
            debug_assert!(item.i_order_by_col > 0);
            debug_assert!((item.i_order_by_col as i32) <= n_expr(p.p_e_list.as_deref()));
            a_permute.push((item.i_order_by_col as u32).wrapping_sub(1));
        }
    }
    let p_key_merge = multi_select_order_by_key_info(db, parse, p, 1);

    // Aloca uma faixa de registradores temporários e o KeyInfo da lógica que remove linhas
    // duplicadas quando o operador é UNION, EXCEPT ou INTERSECT (mas não UNION ALL).
    let reg_prev;
    let mut p_key_dup: Option<Rc<KeyInfo>> = None;
    if op == TK_ALL {
        reg_prev = 0;
    } else {
        let n_ex = n_expr(p.p_e_list.as_deref());
        debug_assert!(n_order_by >= n_ex || db.malloc_failed != 0);
        reg_prev = parse.n_mem + 1;
        parse.n_mem += n_ex + 1;
        add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_prev);
        let mut ki = key_info_alloc(db, n_ex, 1);
        for i in 0..n_ex {
            ki.a_coll[i as usize] = multi_select_coll_seq(db, parse, p, i);
            ki.a_sort_flags[i as usize] = 0;
        }
        p_key_dup = Some(Rc::new(ki));
    }

    // Separa a consulta da esquerda da da direita.
    let mut n_select = 1;
    if (op == TK_ALL || op == TK_UNION) && db.optimization_enabled(SQLITE_BALANCED_MERGE) {
        let mut cur: &Select = p;
        while cur.op == op {
            let Some(prior) = cur.p_prior.as_deref() else {
                break;
            };
            n_select += 1;
            debug_assert!(prior.has_next);
            cur = prior;
        }
    }
    // `pSplit` é o select a `split_depth` passos de `p` pela cadeia `p_prior`.
    let mut split_depth = 0usize;
    if n_select > 3 {
        let mut i = 2;
        while i < n_select {
            split_depth += 1;
            i += 2;
        }
    }
    let mut p_prior = nth_prior_mut(p, split_depth).p_prior.take().expect("pPrior");
    p_prior.has_next = false;
    debug_assert!(p.p_order_by.is_some());
    p_prior.p_order_by = expr_list_dup(p.p_order_by.as_deref(), 0);
    {
        let mut ob = p.p_order_by.take();
        resolve_order_group_by(
            db,
            parse,
            p.p_e_list.as_deref().expect("pEList"),
            ob.as_deref_mut(),
            b"ORDER",
        );
        p.p_order_by = ob;
        let mut ob = p_prior.p_order_by.take();
        resolve_order_group_by(
            db,
            parse,
            p_prior.p_e_list.as_deref().expect("pEList"),
            ob.as_deref_mut(),
            b"ORDER",
        );
        p_prior.p_order_by = ob;
    }

    // Calcula os registradores de limite.
    compute_limit_registers(db, parse, p, label_end);
    let (reg_limit_a, reg_limit_b);
    if p.i_limit != 0 && op == TK_ALL {
        parse.n_mem += 1;
        reg_limit_a = parse.n_mem;
        parse.n_mem += 1;
        reg_limit_b = parse.n_mem;
        let v = vdbe_of_parse(parse);
        add_op2(
            v,
            OP_COPY as i32,
            if p.i_offset != 0 { p.i_offset + 1 } else { p.i_limit },
            reg_limit_a,
        );
        add_op2(v, OP_COPY as i32, reg_limit_a, reg_limit_b);
    } else {
        reg_limit_a = 0;
        reg_limit_b = 0;
    }
    p.p_limit = None;

    parse.n_mem += 1;
    let reg_addr_a = parse.n_mem;
    parse.n_mem += 1;
    let reg_addr_b = parse.n_mem;
    parse.n_mem += 1;
    let reg_out_a = parse.n_mem;
    parse.n_mem += 1;
    let reg_out_b = parse.n_mem;
    let mut dest_a = SelectDest::default();
    let mut dest_b = SelectDest::default();
    select_dest_init(&mut dest_a, SRT_COROUTINE as i32, reg_addr_a);
    select_dest_init(&mut dest_b, SRT_COROUTINE as i32, reg_addr_b);

    explain(parse, db, true, b"MERGE (%s)", &[text(select_op_name(p.op as i32).as_bytes())]);

    // Gera uma co-rotina que avalia o SELECT à esquerda do operador, o "A".
    let addr_select_a = current_addr(parse) + 1;
    let mut addr1 = add_op3(
        vdbe_of_parse(parse),
        OP_INITCOROUTINE as i32,
        reg_addr_a,
        0,
        addr_select_a,
    );
    vdbe_comment(vdbe_of_parse(parse), b"left SELECT", &[]);
    p_prior.i_limit = reg_limit_a;
    explain(parse, db, true, b"LEFT", &[]);
    select(db, parse, &mut p_prior, &mut dest_a);
    end_coroutine(parse, reg_addr_a);
    jump_here(vdbe_of_parse(parse), addr1);

    // Gera uma co-rotina que avalia o SELECT à direita, o "B".
    let addr_select_b = current_addr(parse) + 1;
    addr1 = add_op3(
        vdbe_of_parse(parse),
        OP_INITCOROUTINE as i32,
        reg_addr_b,
        0,
        addr_select_b,
    );
    vdbe_comment(vdbe_of_parse(parse), b"right SELECT", &[]);
    let saved_limit = p.i_limit;
    let saved_offset = p.i_offset;
    p.i_limit = reg_limit_b;
    p.i_offset = 0;
    explain(parse, db, true, b"RIGHT", &[]);
    select(db, parse, p, &mut dest_b);
    p.i_limit = saved_limit;
    p.i_offset = saved_offset;
    end_coroutine(parse, reg_addr_b);

    // Gera a sub-rotina que emite a linha corrente do select A como a próxima linha do composto.
    noop_comment(vdbe_of_parse(parse), b"Output routine for A", &[]);
    let addr_out_a = generate_output_subroutine(
        db,
        parse,
        p,
        &dest_a,
        p_dest,
        reg_out_a,
        reg_prev,
        p_key_dup.as_ref(),
        label_end,
    );

    // Idem para a linha corrente do select B.
    let mut addr_out_b = 0;
    if op == TK_ALL || op == TK_UNION {
        noop_comment(vdbe_of_parse(parse), b"Output routine for B", &[]);
        addr_out_b = generate_output_subroutine(
            db,
            parse,
            p,
            &dest_b,
            p_dest,
            reg_out_b,
            reg_prev,
            p_key_dup.as_ref(),
            label_end,
        );
    }
    drop(p_key_dup);

    // Gera a sub-rotina que roda quando os resultados do select A acabaram e só resta dado em B.
    let addr_eof_a;
    let addr_eof_a_no_b;
    if op == TK_EXCEPT || op == TK_INTERSECT {
        addr_eof_a = label_end;
        addr_eof_a_no_b = label_end;
    } else {
        noop_comment(vdbe_of_parse(parse), b"eof-A subroutine", &[]);
        addr_eof_a = add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_out_b, addr_out_b);
        addr_eof_a_no_b = add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_b, label_end);
        vdbe_goto(vdbe_of_parse(parse), addr_eof_a);
        p.n_select_row = log_est_add(p.n_select_row, p_prior.n_select_row);
    }

    // Gera a sub-rotina que roda quando os resultados do select B acabaram e só resta dado em A.
    let addr_eof_b;
    if op == TK_INTERSECT {
        addr_eof_b = addr_eof_a;
        if p.n_select_row > p_prior.n_select_row {
            p.n_select_row = p_prior.n_select_row;
        }
    } else {
        noop_comment(vdbe_of_parse(parse), b"eof-B subroutine", &[]);
        addr_eof_b = add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_out_a, addr_out_a);
        add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_a, label_end);
        vdbe_goto(vdbe_of_parse(parse), addr_eof_b);
    }

    // Gera o código do caso A<B.
    noop_comment(vdbe_of_parse(parse), b"A-lt-B subroutine", &[]);
    let mut addr_alt_b = add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_out_a, addr_out_a);
    add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_a, addr_eof_a);
    vdbe_goto(vdbe_of_parse(parse), label_cmpr);

    // Gera o código do caso A==B.
    let addr_aeq_b;
    if op == TK_ALL {
        addr_aeq_b = addr_alt_b;
    } else if op == TK_INTERSECT {
        addr_aeq_b = addr_alt_b;
        addr_alt_b += 1;
    } else {
        noop_comment(vdbe_of_parse(parse), b"A-eq-B subroutine", &[]);
        addr_aeq_b = add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_a, addr_eof_a);
        vdbe_goto(vdbe_of_parse(parse), label_cmpr);
    }

    // Gera o código do caso A>B.
    noop_comment(vdbe_of_parse(parse), b"A-gt-B subroutine", &[]);
    let addr_agt_b = current_addr(parse);
    if op == TK_ALL || op == TK_UNION {
        add_op2(vdbe_of_parse(parse), OP_GOSUB as i32, reg_out_b, addr_out_b);
    }
    add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_b, addr_eof_b);
    vdbe_goto(vdbe_of_parse(parse), label_cmpr);

    // Este código roda uma vez para inicializar tudo.
    jump_here(vdbe_of_parse(parse), addr1);
    add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_a, addr_eof_a_no_b);
    add_op2(vdbe_of_parse(parse), OP_YIELD as i32, reg_addr_b, addr_eof_b);

    // Implementa o laço principal da intercalação.
    resolve_label(parse, db, label_cmpr);
    add_op4(vdbe_of_parse(parse), OP_PERMUTATION as i32, 0, 0, 0, P4::IntArray(a_permute));
    add_op4(
        vdbe_of_parse(parse),
        OP_COMPARE as i32,
        dest_a.i_sdst,
        dest_b.i_sdst,
        n_order_by,
        P4::KeyInfo(p_key_merge),
    );
    change_p5(vdbe_of_parse(parse), OPFLAG_PERMUTE as u16);
    add_op3(vdbe_of_parse(parse), OP_JUMP as i32, addr_alt_b, addr_aeq_b, addr_agt_b);

    // Salta para cá para terminar a consulta.
    resolve_label(parse, db, label_end);

    // Devolve o select da esquerda ao lugar. O que sobrou em `pSplit->pPrior` (o C o entrega à
    // lista de limpeza do Parse para liberar depois) cai com o `Drop`.
    p_prior.p_order_by = None;
    p_prior.has_next = true;
    nth_prior_mut(p, split_depth).p_prior = Some(p_prior);

    // TBD no C: inserir chamadas de sub-rotina para fechar cursores de subconsultas incompletas.
    explain_pop(parse);
    (parse.n_err != 0) as i32
}

// ---------------------------------------------------------------------------------------------
// Chunk 008: substituição de colunas, renumeração de cursores
// ---------------------------------------------------------------------------------------------

/// `SubstContext`: descreve uma edição de substituição numa árvore de sintaxe. Toda referência a
/// colunas da tabela `i_table` é trocada pelas expressões correspondentes de `p_e_list`.
///
/// Sobre `is_outer_join`: indica que a substituição cai numa posição do pai que pode ser NULL por
/// causa de um OUTER JOIN (o operando direito de um LEFT JOIN, ou um dos esquerdos de um RIGHT
/// JOIN). Nos dois casos pode ser preciso contornar a expressão substituída com `OP_IfNullRow`:
/// uma constante inteira não é anulada pela flag nullRow da tabela, então se insere um
/// `OP_IfNullRow` que carrega NULL em vez da constante quando a flag está ligada. Exemplo:
/// `SELECT a,b,m,x FROM t1 LEFT JOIN (SELECT 59 AS m,x FROM t2) ON b=x;` com a subconsulta da
/// direita achatada precisa do `OP_IfNullRow` na frente do `OP_Integer` de "m".
struct SubstContext<'a> {
    /// A conexão (o `pParse->db` do C).
    db: &'a mut Connection,
    /// O contexto de análise.
    parse: &'a mut Parse,
    /// Troca as referências a esta tabela.
    i_table: i32,
    /// O novo número de tabela.
    i_new_table: i32,
    /// Insere `TK_IF_NULL_ROW` em cada substituição.
    is_outer_join: bool,
    /// As expressões de reposição.
    p_e_list: &'a ExprList,
    /// As colações das expressões de reposição.
    p_c_list: &'a ExprList,
}

/// Duas colações são a mesma entrada (o `pNat!=pColl` do C compara ponteiros, e a conexão guarda
/// uma entrada por nome e codificação).
fn same_coll(a: Option<&Rc<CollSeq>>, b: Option<&Rc<CollSeq>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y) || (x.name == y.name && x.enc == y.enc),
        _ => false,
    }
}

/// `substExpr`: percorre a expressão e troca cada referência a uma coluna da tabela `i_table` por
/// uma cópia da `iColumn`-ésima entrada de `p_e_list` (as referências ao ROWID ficam como estão).
///
/// Faz parte do achatamento: uma subconsulta cujo resultado é `p_e_list` aparece no FROM de um
/// SELECT com o cursor `i_table`; esta rotina faz a expressão apontar direto para a tabela fonte
/// da subconsulta, e não para o seu resultado.
fn subst_expr(subst: &mut SubstContext<'_>, p_expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
    let mut p_expr = p_expr?;
    if p_expr.has_property(EP_OUTER_ON | EP_INNER_ON) && p_expr.w == subst.i_table {
        p_expr.w = subst.i_new_table;
    }
    if p_expr.op == TK_COLUMN
        && p_expr.i_table == subst.i_table
        && !p_expr.has_property(EP_FIXED_COL)
    {
        // `SQLITE_ALLOW_ROWID_IN_VIEW` está desligada: não há o ramo `iColumn<0 -> TK_NULL`.
        let i_column = p_expr.i_column;
        debug_assert!(i_column >= 0);
        debug_assert!(p_expr.p_right.is_none());
        let p_e_list = subst.p_e_list;
        let p_c_list = subst.p_c_list;
        debug_assert!((i_column as usize) < p_e_list.a.len());
        let p_copy: &Expr = p_e_list.a[i_column as usize].p_expr.as_deref().expect("pCopy");
        if expr_is_vector(p_copy) {
            vector_error_msg(subst.db, subst.parse, p_copy);
            return Some(p_expr);
        }
        let mut p_new: Box<Expr> = if subst.is_outer_join
            && (p_copy.op != TK_COLUMN || p_copy.i_table != subst.i_new_table)
        {
            let mut if_null_row = Expr::default();
            if_null_row.op = TK_IF_NULL_ROW;
            if_null_row.p_left = expr_dup(Some(p_copy), 0);
            if_null_row.i_table = subst.i_new_table;
            if_null_row.i_column = -99;
            if_null_row.flags = EP_IF_NULL_ROW;
            Box::new(if_null_row)
        } else {
            match expr_dup(Some(p_copy), 0) {
                Some(n) => n,
                None => return Some(p_expr),
            }
        };
        if subst.is_outer_join {
            p_new.set_property(EP_CAN_BE_NULL);
        }
        if p_expr.has_property(EP_OUTER_ON | EP_INNER_ON) {
            set_join_expr(
                Some(&mut *p_new),
                p_expr.w,
                p_expr.flags & (EP_OUTER_ON | EP_INNER_ON),
            );
        }
        // `sqlite3ExprDelete(db, pExpr)`: o nó antigo cai aqui.
        drop(p_expr);
        if p_new.op == TK_TRUEFALSE {
            let v = expr_truth_value(&p_new);
            p_new.u = ExprU::IValue(v);
            p_new.op = TK_INTEGER;
            p_new.set_property(EP_INT_VALUE);
        }

        // Garante que a expressão tenha agora uma colação implícita, como tinha quando era coluna
        // de uma view ou subconsulta.
        let p_nat = expr_coll_seq(subst.db, subst.parse, Some(&p_new), None);
        let p_coll = expr_coll_seq(
            subst.db,
            subst.parse,
            p_c_list.a[i_column as usize].p_expr.as_deref(),
            None,
        );
        let needs_collate = !same_coll(p_nat.as_ref(), p_coll.as_ref())
            || (p_new.op != TK_COLUMN && p_new.op != TK_COLLATE);
        let mut result: Option<Box<Expr>> = if needs_collate {
            let z_coll: &[u8] = match p_coll.as_ref() {
                Some(c) => c.name.as_slice(),
                None => b"BINARY",
            };
            expr_add_collate_string(Some(p_new), z_coll)
        } else {
            Some(p_new)
        };
        if let Some(r) = result.as_deref_mut() {
            r.clear_property(EP_COLLATE);
        }
        return result;
    }
    if p_expr.op == TK_IF_NULL_ROW && p_expr.i_table == subst.i_table {
        p_expr.i_table = subst.i_new_table;
    }
    let left = p_expr.p_left.take();
    p_expr.p_left = subst_expr(subst, left);
    let right = p_expr.p_right.take();
    p_expr.p_right = subst_expr(subst, right);
    if p_expr.use_x_select() {
        subst_select(subst, p_expr.x_select_mut(), true);
    } else {
        subst_expr_list(subst, p_expr.x_list_mut());
    }
    if p_expr.has_property(EP_WIN_FUNC) {
        if let Some(p_win) = p_expr.y_win_mut() {
            let filter = p_win.p_filter.take();
            p_win.p_filter = subst_expr(subst, filter);
            subst_expr_list(subst, p_win.p_partition.as_deref_mut());
            subst_expr_list(subst, p_win.p_order_by.as_deref_mut());
        }
    }
    Some(p_expr)
}

/// `substExprList`: aplica `subst_expr` a cada expressão da lista.
fn subst_expr_list(subst: &mut SubstContext<'_>, p_list: Option<&mut ExprList>) {
    let Some(p_list) = p_list else {
        return;
    };
    for item in p_list.a.iter_mut() {
        let e = item.p_expr.take();
        item.p_expr = subst_expr(subst, e);
    }
}

/// `substSelect`: aplica a substituição ao SELECT `p`; com `do_prior` também aos `p_prior`.
fn subst_select(subst: &mut SubstContext<'_>, p: Option<&mut Select>, do_prior: bool) {
    let Some(mut p) = p else {
        return;
    };
    loop {
        subst_expr_list(subst, p.p_e_list.as_deref_mut());
        subst_expr_list(subst, p.p_group_by.as_deref_mut());
        subst_expr_list(subst, p.p_order_by.as_deref_mut());
        let having = p.p_having.take();
        p.p_having = subst_expr(subst, having);
        let where_ = p.p_where.take();
        p.p_where = subst_expr(subst, where_);
        debug_assert!(p.p_src.is_some());
        if let Some(p_src) = p.p_src.as_deref_mut() {
            for p_item in p_src.a.iter_mut() {
                subst_select(subst, p_item.p_select.as_deref_mut(), true);
                if p_item.fg.is_tab_func {
                    if let SrcU1::FuncArg(arg) = &mut p_item.u1 {
                        subst_expr_list(subst, arg.as_deref_mut());
                    }
                }
            }
        }
        if !do_prior {
            break;
        }
        match p.p_prior.as_deref_mut() {
            Some(prior) => p = prior,
            None => break,
        }
    }
}

/// O contexto do walker de `recomputeColumnsUsed`: o cursor do item e a máscara acumulada
/// (o `pWalker->u.pSrcItem` do C).
#[derive(Default)]
struct RecomputeCtx {
    /// `pItem->iCursor`.
    i_cursor: i32,
    /// O novo `pItem->colUsed`.
    col_used: Bitmask,
}

/// `recomputeColumnsUsedExpr`: callback do walker de `recompute_columns_used`.
fn recompute_columns_used_expr(w: &mut Walker<RecomputeCtx>, p_expr: &mut Expr) -> i32 {
    if p_expr.op != TK_COLUMN {
        return WRC_CONTINUE;
    }
    if w.u.i_cursor != p_expr.i_table {
        return WRC_CONTINUE;
    }
    if p_expr.i_column < 0 {
        return WRC_CONTINUE;
    }
    w.u.col_used |= expr_col_used(p_expr, None);
    WRC_CONTINUE
}

/// `recomputeColumnsUsed`: `p_select` é um SELECT e o item `i_item` da sua lista FROM é o que
/// interessa; varre o SELECT inteiro e recalcula `colUsed` desse item.
fn recompute_columns_used(p_select: &mut Select, i_item: usize) {
    let (i_cursor, has_tab) = match p_select.p_src.as_deref() {
        Some(src) => (src.a[i_item].i_cursor, src.a[i_item].p_tab.is_some()),
        None => return,
    };
    if !has_tab {
        return;
    }
    let mut w: Walker<RecomputeCtx> = Walker {
        x_expr_callback: Some(recompute_columns_used_expr),
        x_select_callback: Some(select_walk_noop::<RecomputeCtx>),
        u: RecomputeCtx { i_cursor, col_used: 0 },
        ..Walker::default()
    };
    walk_select(&mut w, Some(&mut *p_select));
    if let Some(src) = p_select.p_src.as_deref_mut() {
        src.a[i_item].col_used = w.u.col_used;
    }
}

/// `srclistRenumberCursors`: atribui um número de cursor novo a cada item de `p_src`. Para cada
/// um registra em `a_csr_map[iOld+1] = iNew` (`a_csr_map[0]` é o tamanho do vetor, que o chamador
/// garante suficiente). Se `p_src` tem subselects, chama-se recursivamente na lista FROM de cada um,
/// com `i_except` igual a -1.
fn srclist_renumber_cursors(
    parse: &mut Parse,
    a_csr_map: &mut Vec<i32>,
    p_src: &mut SrcList,
    i_except: i32,
) {
    for (i, p_item) in p_src.a.iter_mut().enumerate() {
        if i as i32 != i_except {
            debug_assert!(p_item.i_cursor < a_csr_map[0]);
            let k = p_item.i_cursor as usize + 1;
            if !p_item.fg.is_recursive || a_csr_map[k] == 0 {
                a_csr_map[k] = parse.n_tab;
                parse.n_tab += 1;
            }
            p_item.i_cursor = a_csr_map[k];
            let mut p = p_item.p_select.as_deref_mut();
            while let Some(s) = p {
                if let Some(src) = s.p_src.as_deref_mut() {
                    srclist_renumber_cursors(parse, a_csr_map, src, -1);
                }
                p = s.p_prior.as_deref_mut();
            }
        }
    }
}

/// `renumberCursorDoMapping`: `*pi_cursor` é um número de cursor; troca-o se precisa de mapa.
fn renumber_cursor_do_mapping(a_csr_map: &[i32], pi_cursor: &mut i32) {
    let i_csr = *pi_cursor;
    if i_csr < a_csr_map[0] && a_csr_map[i_csr as usize + 1] > 0 {
        *pi_cursor = a_csr_map[i_csr as usize + 1];
    }
}

/// `renumberCursorsCb`: callback de expressão de `renumber_cursors`, atualiza os `Expr` aos novos
/// números de cursor.
fn renumber_cursors_cb(w: &mut Walker<Vec<i32>>, p_expr: &mut Expr) -> i32 {
    let op = p_expr.op;
    if op == TK_COLUMN || op == TK_IF_NULL_ROW {
        renumber_cursor_do_mapping(&w.u, &mut p_expr.i_table);
    }
    if p_expr.has_property(EP_OUTER_ON) {
        renumber_cursor_do_mapping(&w.u, &mut p_expr.w);
    }
    WRC_CONTINUE
}

/// `renumberCursors`: atribui um número de cursor novo a cada cursor da cláusula FROM do SELECT
/// `p` e a cada cursor do FROM dos subselects, recursivamente, exceto o `i_except`-ésimo item do
/// FROM de `p`, e atualiza todas as expressões. `a_csr_map` é espaço de trabalho; o chamador
/// garante que ele é maior que o maior cursor usado no select e que as entradas dos cursores que
/// não aparecem nos FROM descritos são zero.
fn renumber_cursors(parse: &mut Parse, p: &mut Select, i_except: i32, a_csr_map: &mut Vec<i32>) {
    if let Some(src) = p.p_src.as_deref_mut() {
        srclist_renumber_cursors(parse, a_csr_map, src, i_except);
    }
    // O mapa viaja pelo walker (`w.u.aiCol`) e volta, porque persiste entre as chamadas.
    let mut w: Walker<Vec<i32>> = Walker {
        x_expr_callback: Some(renumber_cursors_cb),
        x_select_callback: Some(select_walk_noop::<Vec<i32>>),
        u: std::mem::take(a_csr_map),
        ..Walker::default()
    };
    walk_select(&mut w, Some(p));
    *a_csr_map = w.u;
}

/// `findLeftmostExprlist`: se `p_sel` não faz parte de um composto, a sua lista de resultado; senão
/// a do select mais à esquerda do composto.
fn find_leftmost_exprlist(p_sel: &Select) -> Option<&ExprList> {
    let mut cur = p_sel;
    while let Some(prior) = cur.p_prior.as_deref() {
        cur = prior;
    }
    cur.p_e_list.as_deref()
}

/// `compoundHasDifferentAffinities`: verdadeiro se alguma coluna do resultado do composto tem
/// afinidades incompatíveis em um ou mais dos seus braços.
fn compound_has_different_affinities(p: &Select) -> bool {
    debug_assert!(p.p_prior.is_some());
    let p_list = p.p_e_list.as_deref().expect("pEList");
    for (ii, item) in p_list.a.iter().enumerate() {
        let aff = expr_affinity(item.p_expr.as_deref().expect("pExpr"), None);
        let mut p_sub1 = p.p_prior.as_deref();
        while let Some(sub) = p_sub1 {
            let e = sub.p_e_list.as_deref().expect("pEList").a[ii].p_expr.as_deref().expect("pExpr");
            if expr_affinity(e, None) != aff {
                return true;
            }
            p_sub1 = sub.p_prior.as_deref();
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------
// Chunk 009: achatamento de subconsultas
// ---------------------------------------------------------------------------------------------

/// `flattenSubquery`: tenta achatar subconsultas como otimização de desempenho. Devolve 1 se
/// mudou algo e 0 se não achatou.
///
/// Exemplo: `SELECT a FROM (SELECT x+y AS a FROM t1 WHERE z<100) WHERE a>5` vira
/// `SELECT x+y AS a FROM t1 WHERE z<100 AND a>5`: uma varredura só, e os índices de `t1` podem
/// ser usados. O achatamento está sujeito às restrições numeradas do C (3, 4, 7, 8, 9, 11, 13 a
/// 23, 25 a 28, com as numeradas `(**)` abandonadas), conferidas no início desta rotina.
///
/// `p` é a consulta externa e a subconsulta é `p.p_src.a[i_from]`. `is_agg` é verdadeiro se a
/// consulta externa usa agregadas. Toda a análise de expressões precisa ter acontecido na externa
/// e na subconsulta antes desta rotina.
pub(crate) fn flatten_subquery(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    i_from: usize,
    is_agg: bool,
) -> i32 {
    let z_saved_auth_context = parse.z_auth_context.clone();
    let mut is_outer_join = false;
    let mut a_csr_map: Option<Vec<i32>> = None;

    // Confere se o achatamento é permitido. Devolve 0 se não.
    debug_assert!(p.p_prior.is_none());
    if db.optimization_disabled(SQLITE_QUERY_FLATTENER) {
        return 0;
    }
    let i_parent;
    {
        let p_src = p.p_src.as_deref().expect("pSrc");
        debug_assert!(i_from < p_src.a.len());
        let p_subitem = &p_src.a[i_from];
        i_parent = p_subitem.i_cursor;
        let p_sub = p_subitem.p_select.as_deref().expect("pSub");

        if p.n_win_linked > 0 || p_sub.n_win_linked > 0 {
            return 0; // Restrição (25)
        }

        let p_sub_src = p_sub.p_src.as_deref().expect("pSubSrc");
        // Antes da 3.1.2, quando LIMIT e OFFSET eram constantes simples, algum combinado deles
        // era permitido porque se calculava na compilação. Com expressões arbitrárias vieram as
        // restrições (13) e (14).
        if p_sub.p_limit.is_some() && p.p_limit.is_some() {
            return 0; // Restrição (13)
        }
        if p_sub.p_limit.as_deref().map_or(false, |l| l.p_right.is_some()) {
            return 0; // Restrição (14)
        }
        if (p.sel_flags & SF_COMPOUND) != 0 && p_sub.p_limit.is_some() {
            return 0; // Restrição (15)
        }
        if p_sub_src.a.is_empty() {
            return 0; // Restrição (7)
        }
        if (p_sub.sel_flags & SF_DISTINCT) != 0 {
            return 0; // Restrição (4)
        }
        if p_sub.p_limit.is_some() && (p_src.a.len() > 1 || is_agg) {
            return 0; // Restrições (8)(9)
        }
        if p.p_order_by.is_some() && p_sub.p_order_by.is_some() {
            return 0; // Restrição (11)
        }
        if is_agg && p_sub.p_order_by.is_some() {
            return 0; // Restrição (16)
        }
        if p_sub.p_limit.is_some() && p.p_where.is_some() {
            return 0; // Restrição (19)
        }
        if p_sub.p_limit.is_some() && (p.sel_flags & SF_DISTINCT) != 0 {
            return 0; // Restrição (21)
        }
        if (p_sub.sel_flags & SF_RECURSIVE) != 0 {
            return 0; // Restrição (22)
        }

        // Se a subconsulta é o operando direito de um LEFT JOIN, ela não pode ser uma junção (3a).
        // `t1 LEFT OUTER JOIN (t2 JOIN t3)` achatada viraria `(t1 LEFT OUTER JOIN t2) JOIN t3`,
        // que não é a mesma coisa. Ver os tickets #306, #350 e #3300.
        if (p_subitem.fg.jointype & (JT_OUTER | JT_LTORJ)) != 0 {
            if p_sub_src.a.len() > 1 // (3a)
                || p_sub_src.a[0].p_tab.as_deref().map_or(false, |t| t.is_virtual()) // (3b)
                || (p.sel_flags & SF_DISTINCT) != 0 // (3d)
                || (p_subitem.fg.jointype & JT_RIGHT) != 0 // (26)
            {
                return 0;
            }
            is_outer_join = true;
        }

        debug_assert!(!p_sub_src.a.is_empty()); // Verdade pela restrição (7)
        if i_from > 0 && (p_sub_src.a[0].fg.jointype & JT_LTORJ) != 0 {
            return 0; // Restrição (27a)
        }

        // A condição (28) é barrada pelo chamador.

        // Restrição (17): se a subconsulta é um SELECT composto, só pode usar UNION ALL, e nenhum
        // dos selects simples que o formam pode ser agregado ou DISTINCT.
        if p_sub.p_prior.is_some() {
            if p_sub.p_order_by.is_some() {
                return 0; // Restrição (20)
            }
            if is_agg || (p.sel_flags & SF_DISTINCT) != 0 || is_outer_join {
                return 0; // (17d1), (17d2) ou (17f)
            }
            let mut p_sub1 = Some(p_sub);
            while let Some(sub1) = p_sub1 {
                debug_assert!((p_sub.sel_flags & SF_RECURSIVE) == 0);
                if (sub1.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) != 0 // (17b)
                    || (sub1.p_prior.is_some() && sub1.op != TK_ALL) // (17a)
                    || sub1.p_src.as_deref().map_or(0, |s| s.a.len()) < 1 // (17c)
                    || sub1.n_win_linked > 0 // (17e)
                {
                    return 0;
                }
                if i_from > 0
                    && (sub1.p_src.as_deref().expect("pSrc").a[0].fg.jointype & JT_LTORJ) != 0
                {
                    // Sem esta restrição a flag JT_LTORJ ficaria omitida nas tabelas da esquerda
                    // do right join achatado.
                    return 0; // Restrições (17g), (27b)
                }
                p_sub1 = sub1.p_prior.as_deref();
            }

            // Restrição (18).
            if let Some(ob) = p.p_order_by.as_deref() {
                if ob.a.iter().any(|it| it.i_order_by_col == 0) {
                    return 0;
                }
            }

            // Restrição (23).
            if (p.sel_flags & SF_RECURSIVE) != 0 {
                return 0;
            }

            // Restrição (17h).
            if compound_has_different_affinities(p_sub) {
                return 0;
            }

            if p_src.a.len() > 1 {
                if parse.n_select > 500 {
                    return 0;
                }
                if db.optimization_disabled(SQLITE_FLTTN_UNION_ALL) {
                    return 0;
                }
                let mut map = vec![0i32; parse.n_tab as usize + 1];
                map[0] = parse.n_tab;
                a_csr_map = Some(map);
            }
        }
    }

    // Se chegamos aqui, o achatamento é permitido.

    // Autoriza a subconsulta.
    parse.z_auth_context = p.p_src.as_deref().expect("pSrc").a[i_from].z_name.clone();
    let _ = auth_check(db, parse, SQLITE_SELECT, None, None, None);
    parse.z_auth_context = z_saved_auth_context;

    // Apaga as estruturas transitórias associadas à subconsulta.
    let mut p_sub1: Box<Select> = {
        let p_subitem = &mut p.p_src.as_deref_mut().expect("pSrc").a[i_from];
        let sub = p_subitem.p_select.take().expect("pSub");
        p_subitem.z_database = None;
        p_subitem.z_name = None;
        p_subitem.z_alias = None;
        debug_assert!(p_subitem.fg.is_using || p_subitem.p_on().is_none());
        sub
    };

    // Se a subconsulta é um SELECT composto, então (pelas restrições 17 e 18) ela é um UNION ALL e
    // a consulta pai tem a forma `SELECT <lista> FROM (<subconsulta>) <where>` seguida de ORDER BY,
    // LIMIT e/ou OFFSET. Este bloco cria N-1 cópias do pai, sem ORDER BY, LIMIT nem OFFSET, e as
    // junta à esquerda do original por UNION ALL, onde N é o número de selects simples da
    // subconsulta composta. É o "achatamento de subconsulta composta".
    let mut n_priors = 0usize;
    {
        let mut c: &Select = &p_sub1;
        while let Some(x) = c.p_prior.as_deref() {
            n_priors += 1;
            c = x;
        }
    }
    for _ in 0..n_priors {
        let p_order_by = p.p_order_by.take();
        let p_limit = p.p_limit.take();
        let p_prior = p.p_prior.take();
        let p_item_tab = p.p_src.as_deref_mut().expect("pSrc").a[i_from].p_tab.take();
        let p_new = select_dup(Some(&*p), 0);
        p.p_limit = p_limit;
        p.p_order_by = p_order_by;
        p.op = TK_ALL;
        p.p_src.as_deref_mut().expect("pSrc").a[i_from].p_tab = p_item_tab;
        match p_new {
            None => {
                p.p_prior = p_prior;
            }
            Some(mut p_new) => {
                parse.n_select += 1;
                p_new.sel_id = parse.n_select as u32;
                if let Some(map) = a_csr_map.as_mut() {
                    renumber_cursors(parse, &mut p_new, i_from as i32, map);
                }
                p_new.p_prior = p_prior;
                p_new.has_next = true;
                p.p_prior = Some(p_new);
            }
        }
    }
    drop(a_csr_map);

    // Adia a destruição do `Table` associado à subconsulta até o fim da geração de código, pois
    // ainda podem existir `Expr.pTab` que o referenciam (ticket #3346). Com `Rc` a tabela só cai
    // quando o último `Expr` a soltar, então basta largar a referência do item.
    p.p_src.as_deref_mut().expect("pSrc").a[i_from].p_tab = None;

    // O laço seguinte roda uma vez por termo do achatamento de subconsulta composta (ou só uma
    // vez, nos outros tipos de achatamento). Ele move todos os elementos FROM da subconsulta para
    // o FROM da consulta externa. Antes disso guarda em `i_parent` o cursor do elemento FROM
    // original da externa, que nunca mais será usado: o código seguinte varre as expressões
    // trocando as referências a `i_parent` por expressões que apontam para os elementos FROM
    // copiados.
    let mut i_new_parent = -1;
    for k in 0..=n_priors {
        // O `pSrc` do C neste ponto é o do passo anterior (ou `p.pSrc` na primeira volta).
        let ltorj = {
            let prev = nth_prior_mut(p, k.saturating_sub(1));
            prev.p_src.as_deref().expect("pSrc").a[i_from].fg.jointype & JT_LTORJ
        };
        let jointype: u8 = if k == 0 {
            // Primeira volta: o tipo de junção do item da subconsulta.
            p.p_src.as_deref().expect("pSrc").a[i_from].fg.jointype
        } else {
            0
        };
        let p_parent = nth_prior_mut(p, k);
        let p_sub = nth_prior_mut(&mut p_sub1, k);
        let n_sub_src = p_sub.p_src.as_deref().expect("pSubSrc").a.len();

        // A subconsulta usa um só espaço do FROM da externa. Se tem mais de um elemento no seu
        // FROM, a externa se expande para abrir espaço para todos os elementos da subconsulta.
        // Ex.: `SELECT * FROM tabA, (SELECT * FROM sub1, sub2), tabB;` tem 3 espaços no FROM e o
        // do meio vira dois, ficando 4.
        if n_sub_src > 1 {
            src_list_enlarge(
                db,
                parse,
                p_parent.p_src.as_deref_mut().expect("pSrc"),
                n_sub_src - 1,
                i_from + 1,
            );
        }

        // Transfere os termos do FROM da subconsulta para a consulta externa.
        {
            let p_src = p_parent.p_src.as_deref_mut().expect("pSrc");
            let p_sub_src = p_sub.p_src.as_deref_mut().expect("pSubSrc");
            for i in 0..n_sub_src {
                let p_item = &mut p_src.a[i + i_from];
                // Um USING do item antigo cai com o `Drop` na atribuição.
                debug_assert!(!p_item.fg.is_tab_func);
                *p_item = std::mem::take(&mut p_sub_src.a[i]);
                p_item.fg.jointype |= ltorj;
                i_new_parent = p_item.i_cursor;
            }
            p_src.a[i_from].fg.jointype &= JT_LTORJ;
            p_src.a[i_from].fg.jointype |= jointype | ltorj;
        }

        // Começa a substituir as expressões do resultado da subconsulta nas referências a
        // `i_parent` da consulta externa. Ex.:
        // `SELECT a+5, b*10 FROM (SELECT x*3 AS a, y+10 AS b FROM t1) WHERE a>b;` troca cada "a"
        // por "x*3" e cada "b" por "y+10".
        if p_sub.p_order_by.is_some() && (p_parent.sel_flags & SF_NOOPORDERBY) == 0 {
            // Os `iOrderByCol` não nulos indicam que a expressão do ORDER BY é idêntica à
            // `iOrderByCol`-ésima expressão do pSub. Esses valores não correspondem
            // necessariamente a colunas do pParent, então se zeram antes de passar o ORDER BY
            // (ticket [d11a6e908f]).
            let mut p_order_by = p_sub.p_order_by.take();
            if let Some(ob) = p_order_by.as_deref_mut() {
                for it in ob.a.iter_mut() {
                    it.i_order_by_col = 0;
                }
            }
            debug_assert!(p_parent.p_order_by.is_none());
            p_parent.p_order_by = p_order_by;
        }
        let mut p_where = p_sub.p_where.take();
        if is_outer_join {
            set_join_expr(p_where.as_deref_mut(), i_new_parent, EP_OUTER_ON);
        }
        if let Some(w) = p_where {
            p_parent.p_where = match p_parent.p_where.take() {
                Some(pw) => p_expr(db, parse, TK_AND as i32, Some(w), Some(pw)),
                None => Some(w),
            };
        }
        if db.malloc_failed == 0 {
            let mut x = SubstContext {
                db: &mut *db,
                parse: &mut *parse,
                i_table: i_parent,
                i_new_table: i_new_parent,
                is_outer_join,
                p_e_list: p_sub.p_e_list.as_deref().expect("pEList"),
                p_c_list: find_leftmost_exprlist(p_sub).expect("pEList"),
            };
            subst_select(&mut x, Some(&mut *p_parent), false);
        }

        // O select achatado é composto se a consulta interna ou a externa for composta.
        p_parent.sel_flags |= p_sub.sel_flags & SF_COMPOUND;
        debug_assert!((p_sub.sel_flags & SF_DISTINCT) == 0); // restrição (17b)

        // `SELECT ... FROM (SELECT ... LIMIT a OFFSET b) LIMIT x OFFSET y;` Somar a e b não
        // funciona se algum limite for negativo.
        if p_sub.p_limit.is_some() {
            p_parent.p_limit = p_sub.p_limit.take();
        }

        // Recalcula as máscaras `SrcItem.colUsed` das tabelas achatadas.
        for i in 0..n_sub_src {
            recompute_columns_used(p_parent, i + i_from);
        }
    }

    // Por fim, apaga o que sobrou da subconsulta e devolve sucesso.
    let mut w: Walker<()> = Walker::default();
    agg_info_persist_walker_init(&mut w);
    walk_select(&mut w, Some(&mut *p_sub1));
    drop(p_sub1);

    1
}

// ---------------------------------------------------------------------------------------------
// Chunk 010: propagação de constantes e push-down do WHERE
// ---------------------------------------------------------------------------------------------

/// Uma coluna fixada a um valor constante por um termo `COLUMN=VALUE` do WHERE. `addr` é o
/// endereço do nó da coluna (identidade para o `pColumn==pExpr` do C, ver a nota do módulo).
struct ConstCol {
    /// Endereço do nó `TK_COLUMN` do termo.
    addr: usize,
    /// `pColumn->iTable`.
    i_table: i32,
    /// `pColumn->iColumn`.
    i_column: i32,
    /// A afinidade da coluna é BLOB.
    aff_blob: bool,
}

/// `WhereConst`: guarda todos os valores de coluna fixados por restrições `COLUMN=VALUE` do WHERE.
#[derive(Default)]
struct WhereConst {
    /// Número de vezes que uma constante foi propagada.
    n_chng: i32,
    /// Pelo menos uma coluna do vetor tem afinidade BLOB.
    has_aff_blob: bool,
    /// Quais expressões ON excluir da consideração: `EP_OuterON` ou `EP_InnerON|EP_OuterON`.
    m_exclude_on: u32,
    /// Os termos COLUMN (o `apExpr[i*2]` do C).
    cols: Vec<ConstCol>,
    /// Os termos VALUE (o `apExpr[i*2+1]`), cópias.
    values: Vec<Box<Expr>>,
}

/// `constInsert`: acrescenta uma entrada em `p_const`, sem duplicar colunas e só se for adequado.
/// O chamador garante que a coluna é `TK_COLUMN` e o valor é constante; `p_expr` é a expressão
/// inteira `COLUMN=VALUE` ou `VALUE=COLUMN`, e `column_is_right` diz de que lado está a coluna.
fn const_insert(
    db: &mut Connection,
    parse: &mut Parse,
    p_const: &mut WhereConst,
    p_expr: &Expr,
    column_is_right: bool,
) {
    let p_left = p_expr.p_left.as_deref().expect("pLeft");
    let p_right = p_expr.p_right.as_deref().expect("pRight");
    let (p_column, p_value) = if column_is_right { (p_right, p_left) } else { (p_left, p_right) };
    debug_assert!(p_column.op == TK_COLUMN);

    if p_column.has_property(EP_FIXED_COL) {
        return;
    }
    if expr_affinity(p_value, None) != 0 {
        return;
    }
    if !is_binary(expr_compare_coll_seq(db, parse, p_expr, None).as_ref()) {
        return;
    }

    // Ticket [cf5ed20f] de 2018-10-25: a mesma coluna não entra mais de uma vez.
    for c in p_const.cols.iter() {
        if c.i_table == p_column.i_table && c.i_column == p_column.i_column {
            return; // Já existe.
        }
    }
    let aff_blob = expr_affinity(p_column, None) == SQLITE_AFF_BLOB;
    if aff_blob {
        p_const.has_aff_blob = true;
    }
    if let Some(v) = expr_dup(Some(p_value), 0) {
        p_const.cols.push(ConstCol {
            addr: p_column as *const Expr as usize,
            i_table: p_column.i_table,
            i_column: p_column.i_column,
            aff_blob,
        });
        p_const.values.push(v);
    }
}

/// `findConstInWhere`: acha todos os termos `COLUMN=VALUE` ou `VALUE=COLUMN` de `p_expr` em que
/// VALUE é constante e o termo tem de ser verdadeiro por fazer parte dos ANDs do topo, e os
/// acrescenta a `p_const`.
fn find_const_in_where(
    db: &mut Connection,
    parse: &mut Parse,
    p_const: &mut WhereConst,
    p_expr: &mut Expr,
) {
    if p_expr.has_property(p_const.m_exclude_on) {
        return;
    }
    if p_expr.op == TK_AND {
        if let Some(r) = p_expr.p_right.as_deref_mut() {
            find_const_in_where(db, parse, p_const, r);
        }
        if let Some(l) = p_expr.p_left.as_deref_mut() {
            find_const_in_where(db, parse, p_const, l);
        }
        return;
    }
    if p_expr.op != TK_EQ {
        return;
    }
    debug_assert!(p_expr.p_right.is_some());
    debug_assert!(p_expr.p_left.is_some());
    let right_is_column = p_expr.p_right.as_deref().map_or(false, |r| r.op == TK_COLUMN);
    if right_is_column
        && expr_is_constant(Some((&mut *db, &mut *parse)), p_expr.p_left.as_deref_mut()) != 0
    {
        const_insert(db, parse, p_const, p_expr, true);
    }
    let left_is_column = p_expr.p_left.as_deref().map_or(false, |l| l.op == TK_COLUMN);
    if left_is_column
        && expr_is_constant(Some((&mut *db, &mut *parse)), p_expr.p_right.as_deref_mut()) != 0
    {
        const_insert(db, parse, p_const, p_expr, false);
    }
}

/// `propagateConstantExprRewriteOne`: `p_expr` é candidata a ser trocada por um valor. Se é
/// equivalente a uma das colunas de `p_const`, ganha o valor correspondente (como `EP_FixedCol` e
/// o valor pendurado em `p_left`). Exceto se `b_ignore_aff_blob` e a afinidade é BLOB.
fn propagate_constant_expr_rewrite_one(
    p_const: &mut WhereConst,
    p_expr: &mut Expr,
    b_ignore_aff_blob: bool,
) -> i32 {
    if p_expr.op != TK_COLUMN {
        return WRC_CONTINUE;
    }
    if p_expr.has_property(EP_FIXED_COL | p_const.m_exclude_on) {
        return WRC_CONTINUE;
    }
    let this = p_expr as *const Expr as usize;
    for i in 0..p_const.cols.len() {
        if p_const.cols[i].addr == this {
            continue;
        }
        if p_const.cols[i].i_table != p_expr.i_table {
            continue;
        }
        if p_const.cols[i].i_column != p_expr.i_column {
            continue;
        }
        if b_ignore_aff_blob && p_const.cols[i].aff_blob {
            break;
        }
        // Achou: acrescenta a propriedade EP_FixedCol.
        p_const.n_chng += 1;
        p_expr.clear_property(EP_LEAF);
        p_expr.set_property(EP_FIXED_COL);
        debug_assert!(p_expr.p_left.is_none());
        p_expr.p_left = expr_dup(Some(&p_const.values[i]), 0);
        break;
    }
    WRC_PRUNE
}

/// `propagateConstantExprRewrite`: callback de expressão do walker. `p_expr` é um nó do WHERE; vê
/// se há substituições a fazer nele ou nos filhos imediatos, a partir de `w.u` (o `WhereConst`).
/// A troca vale se `p_expr` é uma coluna de afinidade que não seja BLOB igual a uma de `w.u`, ou
/// um operador de comparação binário (=, <=, >=, <, >) com afinidade que não seja TEXT e um filho
/// imediato que é coluna igual a uma de `w.u`.
fn propagate_constant_expr_rewrite(w: &mut Walker<WhereConst>, p_expr: &mut Expr) -> i32 {
    let has_aff_blob = w.u.has_aff_blob;
    debug_assert!(TK_EQ + 1 == crate::consts::TK_GT);
    debug_assert!(TK_EQ + 2 == crate::consts::TK_LE);
    debug_assert!(TK_EQ + 3 == crate::consts::TK_LT);
    debug_assert!(TK_EQ + 4 == TK_GE);
    if has_aff_blob {
        if (p_expr.op >= TK_EQ && p_expr.op <= TK_GE) || p_expr.op == TK_IS {
            if let Some(l) = p_expr.p_left.as_deref_mut() {
                propagate_constant_expr_rewrite_one(&mut w.u, l, false);
            }
            let left_aff = p_expr.p_left.as_deref().map_or(0, |l| expr_affinity(l, None));
            if left_aff != SQLITE_AFF_TEXT {
                if let Some(r) = p_expr.p_right.as_deref_mut() {
                    propagate_constant_expr_rewrite_one(&mut w.u, r, false);
                }
            }
        }
    }
    propagate_constant_expr_rewrite_one(&mut w.u, p_expr, has_aff_blob)
}

/// `propagateConstants`: a otimização de propagação de constantes do WHERE. Se o WHERE tem termos
/// `COLUMN=CONSTANT` ou `CONSTANT=COLUMN` ligados por AND no topo e fora de um ON de LEFT JOIN, em
/// toda a consulta as outras ocorrências de COLUMN são trocadas por CONSTANT. Devolve o número de
/// transformações feitas.
///
/// Ex.: `WHERE t1.a=39 AND t2.b=t1.a AND t3.c=t2.b` vira `t1.a=39 AND t2.b=39 AND t3.c=39`. A
/// afinidade e a colação tornam isso delicado (`a INT, b TEXT` com `WHERE a=123 AND b=a` não é o
/// mesmo que `b=123`), por isso a árvore não muda de "b=a" para "b=123": o "a" de "b=a" ganha
/// `EP_FixedCol` e o "123" fica pendurado em `p_left`, e o gerador de código usa a constante. Só
/// vale se o termo "a=123" usa a colação BINARY (2021-05-25, post 6a06202608: colunas de afinidade
/// BLOB só propagam com operadores que causam a conversão correta de tipos).
pub(crate) fn propagate_constants(db: &mut Connection, parse: &mut Parse, p: &mut Select) -> i32 {
    let mut n_chng = 0;
    loop {
        let mut x = WhereConst::default();
        let has_ltorj = p.p_src.as_deref().map_or(false, |s| {
            !s.a.is_empty() && (s.a[0].fg.jointype & JT_LTORJ) != 0
        });
        if has_ltorj {
            // Não propaga constantes por nenhum ON se há RIGHT JOIN na consulta.
            x.m_exclude_on = EP_INNER_ON | EP_OUTER_ON;
        } else {
            // Não propaga constantes pelo ON de um LEFT JOIN.
            x.m_exclude_on = EP_OUTER_ON;
        }
        if let Some(w) = p.p_where.as_deref_mut() {
            find_const_in_where(db, parse, &mut x, w);
        }
        let mut chng = 0;
        if !x.cols.is_empty() {
            let mut w: Walker<WhereConst> = Walker {
                x_expr_callback: Some(propagate_constant_expr_rewrite),
                x_select_callback: Some(select_walk_noop::<WhereConst>),
                x_select_callback2: None,
                walker_depth: 0,
                u: x,
                ..Walker::default()
            };
            walk_expr(&mut w, p.p_where.as_deref_mut());
            chng = w.u.n_chng;
            n_chng += chng;
        }
        if chng == 0 {
            break;
        }
    }
    n_chng
}

/// A janela `Select.pWin` do C (a cabeça da lista): a janela ligada de menor `link_seq` entre as
/// expressões do resultado e do ORDER BY do select.
fn first_window(p: &Select) -> Option<&Window> {
    fn visit<'a>(e: &'a Expr, best: &mut Option<&'a Window>) {
        if e.has_property(EP_WIN_FUNC) {
            if let Some(w) = e.y_win() {
                if w.link_seq != 0 && best.map_or(true, |b| w.link_seq < b.link_seq) {
                    *best = Some(w);
                }
            }
        }
        if let Some(l) = e.p_left.as_deref() {
            visit(l, best);
        }
        if let Some(r) = e.p_right.as_deref() {
            visit(r, best);
        }
        if let Some(list) = e.x_list() {
            for it in list.a.iter() {
                if let Some(x) = it.p_expr.as_deref() {
                    visit(x, best);
                }
            }
        }
    }
    let mut best: Option<&Window> = None;
    for list in [p.p_e_list.as_deref(), p.p_order_by.as_deref()].into_iter().flatten() {
        for it in list.a.iter() {
            if let Some(e) = it.p_expr.as_deref() {
                visit(e, &mut best);
            }
        }
    }
    best
}

/// `pushDownWindowCheck`: decide se é seguro empurrar a expressão `p_expr` do WHERE para a
/// subconsulta `p_subq`, que tem pelo menos uma função de janela. Só é seguro se a expressão tem
/// apenas constantes e cópias de expressões do PARTITION BY de todas as janelas da subconsulta:
/// filtrar partições inteiras é seguro, linhas dentro de uma partição não. Garantido na chamada:
/// a subconsulta usa uma só moldura de janela e ela tem PARTITION BY.
fn push_down_window_check(
    db: &mut Connection,
    parse: &mut Parse,
    p_subq: &Select,
    p_expr: &mut Expr,
) -> bool {
    let win = first_window(p_subq).expect("pWin");
    debug_assert!(win.p_partition.is_some());
    debug_assert!((p_subq.sel_flags & SF_MULTIPART) == 0);
    debug_assert!(p_subq.p_prior.is_none());
    expr_is_constant_or_group_by(db, parse, Some(p_expr), win.p_partition.as_deref(), None) != 0
}

/// `pushDownWhereTerms`: copia termos relevantes do WHERE da consulta externa para o WHERE da
/// subconsulta. Ex.: `SELECT * FROM (SELECT a AS x, c-d AS y FROM t1) WHERE x=5 AND y=10;` vira
/// `... FROM (SELECT a AS x, c-d AS y FROM t1 WHERE a=5 AND c-d=10) WHERE x=5 AND y=10;`. É a
/// "otimização de push-down do WHERE" (não confundir com a "de push-down do MySQL").
///
/// Não se tenta a otimização se: (2) a consulta interna é a parte recursiva de uma CTE; (3) tem
/// LIMIT; (4) é o operando direito de um LEFT JOIN e a expressão não vem do ON desse LEFT JOIN;
/// (5) a expressão vem do ON ou USING de um LEFT JOIN em que `i_cursor` não é a tabela da direita
/// (o exemplo clássico: `(b2=2)` não pode entrar na subconsulta `bb` de
/// `... JOIN bb ON (a1=b2) LEFT JOIN cc ON (b2=2)`); (6) há funções de janela: (6a) múltiplas
/// partições incompatíveis, (6b) composto com janelas, (6c) o WHERE não é só constantes e cópias de
/// expressões do PARTITION BY de todas as janelas; (7) a CTE deve ser materializada (restrição do
/// chamador); (8) composto com UNION, INTERSECT ou EXCEPT em que alguma coluna não usa BINARY;
/// (9) a expressão vem do ON/USING de uma junção, a subconsulta está à direita dele e há um RIGHT
/// ou FULL JOIN no meio; (10) a consulta interna é a tabela direita de um RIGHT JOIN; (11) é um
/// VALUES; (12, só com `SQLITE_ALLOW_ROWID_IN_VIEW`, desligada) "rowid ISNULL".
///
/// `p_subq` é a subconsulta de `p_src_list.a[i_src]`: o chamador a tira do item
/// (`Option::take`) durante a chamada e a devolve depois. Devolve 0 se nada mudou e diferente de
/// zero se um ou mais termos do WHERE foram duplicados na subconsulta.
pub(crate) fn push_down_where_terms(
    db: &mut Connection,
    parse: &mut Parse,
    p_subq: &mut Select,
    p_where: Option<&mut Expr>,
    p_src_list: &SrcList,
    i_src: usize,
) -> i32 {
    let mut n_chng = 0;
    let p_src = &p_src_list.a[i_src];
    let Some(mut p_where) = p_where else {
        return 0;
    };
    if (p_subq.sel_flags & (SF_RECURSIVE | SF_MULTIPART)) != 0 {
        return 0; // restrições (2) e (11)
    }
    if (p_src.fg.jointype & (JT_LTORJ | JT_RIGHT)) != 0 {
        return 0; // restrição (10)
    }

    if p_subq.p_prior.is_some() {
        let mut not_union_all = false;
        let mut p_sel = Some(&*p_subq);
        while let Some(s) = p_sel {
            let op = s.op;
            debug_assert!(
                op == TK_ALL || op == TK_SELECT || op == TK_UNION || op == TK_INTERSECT || op == TK_EXCEPT
            );
            if op != TK_ALL && op != TK_SELECT {
                not_union_all = true;
            }
            if s.n_win_linked > 0 {
                return 0; // restrição (6b)
            }
            p_sel = s.p_prior.as_deref();
        }
        if not_union_all {
            // Se algum braço usa UNION, INTERSECT ou EXCEPT, nenhuma coluna pode ter colação que
            // não seja BINARY.
            let mut p_sel = Some(&*p_subq);
            while let Some(s) = p_sel {
                let p_list = s.p_e_list.as_deref().expect("pEList");
                for item in p_list.a.iter() {
                    let p_coll = expr_coll_seq(db, parse, item.p_expr.as_deref(), None);
                    if !is_binary(p_coll.as_ref()) {
                        return 0; // restrição (8)
                    }
                }
                p_sel = s.p_prior.as_deref();
            }
        }
    } else if p_subq.n_win_linked > 0
        && first_window(p_subq).map_or(true, |w| w.p_partition.is_none())
    {
        return 0;
    }

    if p_subq.p_limit.is_some() {
        return 0; // restrição (3)
    }
    while p_where.op == TK_AND {
        n_chng += push_down_where_terms(
            db,
            parse,
            &mut *p_subq,
            p_where.p_right.as_deref_mut(),
            p_src_list,
            i_src,
        );
        p_where = p_where.p_left.as_deref_mut().expect("pLeft");
    }

    // Os testes (4), (5) e (9) agora estão em `sqlite3ExprIsSingleTableConstraint()`; a restrição
    // (12) só existe com `SQLITE_ALLOW_ROWID_IN_VIEW`.
    if expr_is_single_table_constraint(&mut *p_where, p_src_list, i_src, true) != 0 {
        n_chng += 1;
        p_subq.sel_flags |= SF_PUSHDOWN;
        let mut cur: Option<&mut Select> = Some(p_subq);
        while let Some(sq) = cur {
            let mut p_new = expr_dup(Some(&*p_where), 0);
            unset_join_expr(p_new.as_deref_mut(), -1, true);
            let mut p_new = {
                let mut x = SubstContext {
                    db: &mut *db,
                    parse: &mut *parse,
                    i_table: p_src.i_cursor,
                    i_new_table: p_src.i_cursor,
                    is_outer_join: false,
                    p_e_list: sq.p_e_list.as_deref().expect("pEList"),
                    p_c_list: find_leftmost_exprlist(sq).expect("pEList"),
                };
                subst_expr(&mut x, p_new)
            };
            if sq.n_win_linked > 0
                && !push_down_window_check(db, parse, sq, p_new.as_deref_mut().expect("pNew"))
            {
                // A restrição (6c) impediu o push-down neste caso.
                n_chng -= 1;
                break;
            }
            if (sq.sel_flags & SF_AGGREGATE) != 0 {
                sq.p_having = expr_and(db, parse, sq.p_having.take(), p_new);
            } else {
                sq.p_where = expr_and(db, parse, sq.p_where.take(), p_new);
            }
            cur = sq.p_prior.as_deref_mut();
        }
    }
    n_chng
}

// ---------------------------------------------------------------------------------------------
// Chunk 011: colunas não usadas, min/max, count(*), INDEXED BY, composto com COLLATE, CTE
// ---------------------------------------------------------------------------------------------

/// `disableUnusedSubqueryResultColumns`: confere se uma subconsulta tem colunas de resultado que
/// nunca são usadas; se tem, o valor delas vira NULL para não gastar trabalho calculando-as.
/// Devolve o número de colunas trocadas por NULL.
pub(crate) fn disable_unused_subquery_result_columns(p_item: &mut SrcItem) -> i32 {
    let mut n_chng = 0;
    if p_item.fg.is_correlated || p_item.fg.is_cte {
        return 0;
    }
    let p_tab = p_item.p_tab.clone().expect("pTab");
    let col_used_item = p_item.col_used;
    let p_sub = p_item.p_select.as_deref_mut().expect("pSelect");
    debug_assert!(p_sub.p_e_list.as_deref().map_or(0, |l| l.a.len()) as i32 == p_tab.n_col as i32);
    {
        let mut p_x = Some(&*p_sub);
        while let Some(x) = p_x {
            if (x.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) != 0 {
                return 0;
            }
            if x.p_prior.is_some() && x.op != TK_ALL {
                // A otimização não vale em compostos que usam UNION, INTERSECT ou EXCEPT.
                return 0;
            }
            if x.n_win_linked > 0 {
                // Nem em subconsultas com funções de janela.
                return 0;
            }
            p_x = x.p_prior.as_deref();
        }
    }
    let mut col_used: Bitmask = col_used_item;
    if let Some(p_list) = p_sub.p_order_by.as_deref() {
        for item in p_list.a.iter() {
            let mut i_col = item.i_order_by_col;
            if i_col > 0 {
                i_col -= 1;
                let sh = if i_col as i32 >= BMS { BMS - 1 } else { i_col as i32 };
                col_used |= (1 as Bitmask) << sh;
            }
        }
    }
    let n_col = p_tab.n_col as i32;
    for j in 0..n_col {
        let m: Bitmask = if j < BMS - 1 { (1 as Bitmask) << j } else { TOPBIT };
        if (m & col_used) != 0 {
            continue;
        }
        let mut p_x: Option<&mut Select> = Some(&mut *p_sub);
        while let Some(x) = p_x {
            let p_y = x
                .p_e_list
                .as_deref_mut()
                .expect("pEList")
                .a[j as usize]
                .p_expr
                .as_deref_mut()
                .expect("pExpr");
            if p_y.op != TK_NULL {
                p_y.op = TK_NULL;
                p_y.clear_property(EP_SKIP | EP_UNLIKELY);
                x.sel_flags |= SF_PUSHDOWN;
                n_chng += 1;
            }
            p_x = x.p_prior.as_deref_mut();
        }
    }
    n_chng
}

/// `minMaxQuery`: `p_func` é a única função agregada da consulta; confere se a consulta é
/// candidata à otimização min/max. Se for, grava em `*pp_min_max` a cláusula ORDER BY a usar e
/// devolve `WHERE_ORDERBY_MIN` ou `WHERE_ORDERBY_MAX` conforme `p_func` seja min() ou max();
/// senão devolve `WHERE_ORDERBY_NORMAL` (zero). Deve ser chamada depois de localizadas as
/// funções agregadas mas antes da análise de agregadas dos argumentos.
pub(crate) fn min_max_query(
    db: &Connection,
    p_func: &Expr,
    pp_min_max: &mut Option<Box<ExprList>>,
) -> u32 {
    let mut e_ret = WHERE_ORDERBY_NORMAL;
    let mut sort_flags: u8 = 0;

    debug_assert!(pp_min_max.is_none());
    debug_assert!(p_func.op == TK_AGG_FUNCTION);
    debug_assert!(!p_func.is_window_func());
    let p_e_list = match p_func.x_list() {
        Some(l) => l,
        None => return e_ret,
    };
    if p_e_list.a.len() != 1
        || p_func.has_property(EP_WIN_FUNC)
        || db.optimization_disabled(SQLITE_MIN_MAX_OPT)
    {
        return e_ret;
    }
    debug_assert!(!p_func.has_property(EP_INT_VALUE));
    let z_func = p_func.z_token().unwrap_or(b"");
    if str_icmp(z_func, b"min") == 0 {
        e_ret = WHERE_ORDERBY_MIN;
        if expr_can_be_null(p_e_list.a[0].p_expr.as_deref()) {
            sort_flags = KEYINFO_ORDER_BIGNULL;
        }
    } else if str_icmp(z_func, b"max") == 0 {
        e_ret = WHERE_ORDERBY_MAX;
        sort_flags = KEYINFO_ORDER_DESC;
    } else {
        return e_ret;
    }
    *pp_min_max = expr_list_dup(Some(p_e_list), 0);
    if let Some(p_order_by) = pp_min_max.as_deref_mut() {
        p_order_by.a[0].fg.sort_flags = sort_flags;
    }
    e_ret
}

/// `isSimpleCount`: `p` é um SELECT agregado e `p_agg_info` o seu `AggInfo` (identificado por
/// `agg_id`). Se a forma é `SELECT count(*) FROM <tbl>`, com `<tbl>` uma tabela do banco e não
/// uma subconsulta ou view, devolve a tabela; senão `None`. Um `None` a mais só custa tempo, mas
/// uma tabela a mais dá resultado errado: na dúvida, `None`.
pub(crate) fn is_simple_count(
    p: &Select,
    agg_id: AggInfoId,
    p_agg_info: &AggInfo,
) -> Option<Rc<Table>> {
    debug_assert!(p.p_group_by.is_none());

    let p_src = p.p_src.as_deref()?;
    if p.p_where.is_some()
        || p.p_e_list.as_deref().map_or(0, |l| l.a.len()) != 1
        || p_src.a.len() != 1
        || p_src.a[0].p_select.is_some()
        || p_agg_info.a_func.len() != 1
        || p.p_having.is_some()
    {
        return None;
    }
    let p_tab = p_src.a[0].p_tab.clone()?;
    debug_assert!(!p_tab.is_view());
    if !p_tab.is_ordinary_table() {
        return None;
    }
    let p_expr = p.p_e_list.as_deref()?.a[0].p_expr.as_deref()?;
    if p_expr.op != TK_AGG_FUNCTION {
        return None;
    }
    if p_expr.p_agg_info != Some(agg_id) {
        return None;
    }
    let func_flags = p_agg_info.a_func[0].p_func.as_ref().map_or(0, |f| f.func_flags);
    if (func_flags & SQLITE_FUNC_COUNT) == 0 {
        return None;
    }
    if p_expr.has_property(EP_DISTINCT | EP_WIN_FUNC) {
        return None;
    }
    Some(p_tab)
}

/// `sqlite3IndexedByLookup`: se o item da lista FROM tem INDEXED BY, procura o índice nomeado. Se
/// a cláusula existe e o índice não é achado, devolve `SQLITE_ERROR` e deixa o erro em `parse`;
/// senão grava o índice em `u2` (`pIBIndex`) e devolve `SQLITE_OK`.
pub fn indexed_by_lookup(db: &mut Connection, parse: &mut Parse, p_from: &mut SrcItem) -> i32 {
    let p_tab = p_from.p_tab.clone().expect("pTab");
    let z_indexed_by: Vec<u8> = match &p_from.u1 {
        SrcU1::IndexedBy(Some(z)) => z.clone(),
        _ => Vec::new(),
    };
    debug_assert!(p_from.fg.is_indexed_by);

    let p_idx = p_tab.p_index.iter().find(|i| str_icmp(&i.z_name, &z_indexed_by) == 0).cloned();
    match p_idx {
        None => {
            error_msg(db, parse, b"no such index: %s", &[text(&z_indexed_by)]);
            parse.check_schema = 1;
            SQLITE_ERROR
        }
        Some(idx) => {
            debug_assert!(!p_from.fg.is_cte);
            p_from.u2 = SrcU2::IbIndex(Some(idx));
            SQLITE_OK
        }
    }
}

/// O contexto do walker de `convertCompoundSelectToSubquery`: o `pWalker->pParse` do C.
pub(crate) struct ConvertCtx<'a> {
    /// A conexão (o `pParse->db`).
    pub db: &'a mut Connection,
    /// O contexto de análise.
    pub parse: &'a mut Parse,
}

/// `convertCompoundSelectToSubquery`: detecta SELECTs compostos cujo ORDER BY usa uma colação
/// alternativa, como em `SELECT ... FROM t1 EXCEPT SELECT ... FROM t2 ORDER BY .. COLLATE ...`, e
/// os reescreve como subconsulta: `SELECT * FROM (SELECT ... EXCEPT SELECT ...) ORDER BY ..
/// COLLATE ...`. A transformação é necessária porque `multiSelectOrderBy()` gera um algoritmo de
/// intercalação que exige a mesma colação nas colunas do resultado e no ORDER BY (ticket
/// 6709574d2a). Só vale para EXCEPT, INTERSECT e UNION: UNION ALL funciona mesmo com COLLATE.
pub(crate) fn convert_compound_select_to_subquery(
    w: &mut Walker<ConvertCtx<'_>>,
    p: &mut Select,
) -> i32 {
    if p.p_prior.is_none() {
        return WRC_CONTINUE;
    }
    let Some(p_order_by) = p.p_order_by.as_deref() else {
        return WRC_CONTINUE;
    };
    let mut p_x = Some(&*p);
    while let Some(x) = p_x {
        if x.op != TK_ALL && x.op != TK_SELECT {
            break;
        }
        p_x = x.p_prior.as_deref();
    }
    if p_x.is_none() {
        return WRC_CONTINUE;
    }
    // Se `iOrderByCol` já não é zero, o ORDER BY já foi casado com uma coluna do resultado (o
    // SELECT foi reescrito para funções de janela e voltou a `sqlite3SelectPrep`): a reescrita
    // não é necessária.
    if p_order_by.a[0].i_order_by_col != 0 {
        return WRC_CONTINUE;
    }
    let mut i = p_order_by.a.len() as i32 - 1;
    while i >= 0 {
        if p_order_by.a[i as usize].p_expr.as_deref().map_or(false, |e| (e.flags & EP_COLLATE) != 0)
        {
            break;
        }
        i -= 1;
    }
    if i < 0 {
        return WRC_CONTINUE;
    }

    // Se chegamos aqui, a transformação é necessária.
    let db = &mut *w.u.db;
    let parse = &mut *w.u.parse;
    let dummy = Token::default();
    let Some(mut p_new_src) = src_list_append_from_term(
        db,
        parse,
        None,
        None,
        None,
        Some(&dummy),
        Some(Box::new(Select::default())),
        None,
    ) else {
        return WRC_ABORT;
    };
    // `*pNew = *p`: o novo select leva tudo, menos GROUP BY, HAVING, ORDER BY e LIMIT, que
    // continuam na externa (no C os ponteiros eram copiados e os do novo zerados).
    let mut p_new = Box::new(Select::default());
    p_new.op = p.op;
    p_new.n_select_row = p.n_select_row;
    p_new.sel_flags = p.sel_flags;
    p_new.i_limit = p.i_limit;
    p_new.i_offset = p.i_offset;
    p_new.sel_id = p.sel_id;
    p_new.addr_open_ephm = p.addr_open_ephm;
    p_new.n_win_linked = p.n_win_linked;
    p_new.has_next = p.has_next;
    p_new.p_e_list = p.p_e_list.take();
    p_new.p_src = p.p_src.take();
    p_new.p_where = p.p_where.take();
    p_new.p_prior = p.p_prior.take();
    p_new.p_with = p.p_with.take();
    p_new.p_win_defn = std::mem::take(&mut p.p_win_defn);
    debug_assert!(p_new.p_prior.is_some());
    if let Some(prior) = p_new.p_prior.as_deref_mut() {
        prior.has_next = true;
    }
    p_new_src.a[0].p_select = Some(p_new);
    p.p_src = Some(p_new_src);
    p.p_e_list = expr_list_append(None, expr(TK_ASTERISK as i32, None));
    p.op = TK_SELECT;
    p.has_next = false;
    p.sel_flags &= !SF_COMPOUND;
    debug_assert!((p.sel_flags & SF_CONVERTED) == 0);
    p.sel_flags |= SF_CONVERTED;
    WRC_CONTINUE
}

/// `cannotBeFunction`: se o termo `p_from` do FROM tem argumentos de função com valor de tabela,
/// deixa um erro em `parse` e devolve diferente de zero, pois ali não pode ser uma função.
pub(crate) fn cannot_be_function(db: &mut Connection, parse: &mut Parse, p_from: &SrcItem) -> i32 {
    if p_from.fg.is_tab_func {
        error_msg(
            db,
            parse,
            b"'%s' is not a function",
            &[PrintfArg::Text(p_from.z_name.clone())],
        );
        return 1;
    }
    0
}

/// `searchWith`: `p_with` é a pilha de cláusulas WITH aninhadas, da mais interna à mais externa.
/// Se a tabela do FROM de nome `z_name` é de fato uma CTE, devolve (nível da pilha, índice da CTE
/// naquele WITH); senão `None`.
pub(crate) fn search_with(p_with: &[&With], z_name: &[u8]) -> Option<(usize, usize)> {
    for (level, p) in p_with.iter().enumerate() {
        for (i, cte) in p.a.iter().enumerate() {
            if str_icmp(z_name, cte.z_name.as_deref().unwrap_or(b"")) == 0 {
                return Some((level, i));
            }
        }
        if p.b_view != 0 {
            break;
        }
    }
    None
}

/// `sqlite3WithPush`: empilha a cláusula WITH `p_with` como a mais interna. O código mantém uma
/// pilha de WITH ativas, a mais interna no topo; o `Parse` guarda o topo (`Parse.p_with`) pelo
/// protocolo "mover para dentro e devolver". Se há `p_with` e não houve erro de análise, a nova
/// cláusula entra em `parse.p_with` e se devolve `Ok(anterior)`, a cláusula que estava no topo
/// (o `pOuter` do C) para o chamador restaurar ao desempilhar. Senão nada é empilhado e a
/// cláusula volta em `Err(p_with)`. (O `bFree` do C, que passava a posse ao Parse para liberar no
/// fim, é o próprio movimento de posse para `parse.p_with`.)
pub(crate) fn with_push(
    parse: &mut Parse,
    p_with: Option<Box<With>>,
) -> Result<Option<Box<With>>, Option<Box<With>>> {
    match p_with {
        Some(w) if parse.n_err == 0 => Ok(parse.p_with.replace(w)),
        other => Err(other),
    }
}
