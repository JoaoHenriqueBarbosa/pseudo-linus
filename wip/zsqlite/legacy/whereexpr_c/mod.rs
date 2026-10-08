// Mesclado das partes traduzidas de whereexpr_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Modelo adotado neste arquivo (whereexpr.c inteiro, partes 000 a 003):
//  - WhereClause e WhereTerm vivem atrás de Rc<RefCell<..>> (`WhereClauseRef`, `WhereTermRef`);
//    o ponteiro `WhereTerm*` do C vira um clone do Rc, que continua válido depois de
//    `where_clause_insert` (o aviso do C sobre realocação de `a[]` deixa de existir).
//  - `WhereTerm.p_expr` é `ExprRef`. Os filhos de uma árvore Expr são acessados pelos auxiliares
//    `expr_left`, `expr_right` e `expr_list_item_ref`, que devolvem `ExprRef` e são do integrador.
//  - `WhereOrInfo.wc` e `WhereAndInfo.wc` precisam ser `WhereClauseRef` para que o `p_wc` de seus
//    termos (Weak) exista.

/// Auxiliar: o WhereInfo de uma cláusula (`pWC->pWInfo`).
fn where_info_of(p_wc: &WhereClauseRef) -> WhereInfoRef {
    p_wc.borrow()
        .p_w_info
        .upgrade()
        .expect("WhereClause sem WhereInfo")
}

/// Auxiliar: o Parse de uma cláusula (`pWC->pWInfo->pParse`).
fn where_parse_of(p_wc: &WhereClauseRef) -> ParseRef {
    where_info_of(p_wc).borrow().p_parse.clone()
}

/// Auxiliar: a conexão de um Parse (`pParse->db`).
fn parse_db(p_parse: &ParseRef) -> Sqlite3Ref {
    p_parse
        .borrow()
        .db
        .upgrade()
        .expect("Parse sem conexão")
}

/// Auxiliar: `db->mallocFailed`.
fn db_malloc_failed(db: &Sqlite3Ref) -> bool {
    db.borrow().malloc_failed != 0
}

/// Auxiliar: move uma Expr alocada (Box) para o modelo compartilhado dos termos.
fn expr_ref_new(p: Option<Box<Expr>>) -> Option<ExprRef> {
    p.map(|b| Rc::new(RefCell::new(*b)))
}

/// Auxiliar: devolve a posse de uma ExprRef que tem um único dono (inverso de `expr_ref_new`).
fn expr_ref_into_box(r: ExprRef) -> Box<Expr> {
    match Rc::try_unwrap(r) {
        Ok(cell) => Box::new(cell.into_inner()),
        Err(_) => panic!("expr_ref_into_box: ExprRef compartilhada"),
    }
}

/// Auxiliar: `sqlite3ExprDup(db, p, 0)` sobre uma ExprRef.
fn expr_dup_ref(db: &Sqlite3Ref, p: &ExprRef) -> Option<Box<Expr>> {
    expr_dup(&db.borrow(), &p.borrow(), 0)
}

/// Auxiliar: igualdade de ponteiro entre duas CollSeq (o `!=` entre `CollSeq*` do C).
fn coll_seq_same(a: &Option<CollSeqRef>, b: &Option<CollSeqRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Desaloca toda a memória associada a um objeto WhereOrInfo.
fn where_or_info_delete(_db: &Sqlite3Ref, p: Box<WhereOrInfo>) {
    where_clause_clear(&p.wc);
    // sqlite3DbFree(db, p): o Box é liberado ao sair de escopo.
}

/// Desaloca toda a memória associada a um objeto WhereAndInfo.
fn where_and_info_delete(_db: &Sqlite3Ref, p: Box<WhereAndInfo>) {
    where_clause_clear(&p.wc);
    // sqlite3DbFree(db, p): o Box é liberado ao sair de escopo.
}

/// Adiciona uma única entrada WhereTerm nova ao objeto WhereClause pWC.
/// O novo WhereTerm é construído a partir de Expr p e com wtFlags.
/// O índice em pWC.a[] do novo WhereTerm é retornado em sucesso.
/// 0 é retornado se o novo WhereTerm não pôde ser adicionado por falha de alocação.
/// (No Rust a alocação não falha, então o ramo de falha do C não existe.)
///
/// Esta rotina aumenta o tamanho de pWC.a[] conforme necessário.
///
/// Se wtFlags inclui TERM_DYNAMIC, a responsabilidade de liberar a expressão p é
/// assumida pela WhereClause pWC (aqui, pelo Rc do termo).
fn where_clause_insert(p_wc: &WhereClauseRef, p: Option<ExprRef>, wt_flags: u16) -> i32 {
    let idx: i32;
    {
        let mut wc = p_wc.borrow_mut();
        if wc.n_term >= wc.n_slot {
            // pWC->a = sqlite3WhereMalloc(.. nSlot*2); memcpy; nSlot = nSlot*2.
            let n_new = wc.n_slot * 2;
            let extra = (n_new as usize).saturating_sub(wc.a.len());
            wc.a.reserve(extra);
            wc.n_slot = n_new;
        }
        idx = wc.n_term;
        wc.n_term += 1;
        if (wt_flags & TERM_VIRTUAL) == 0 {
            wc.n_base = wc.n_term;
        }
    }
    let truth_prob: LogEst = match &p {
        Some(e) if expr_has_property(&e.borrow(), EP_UNLIKELY) => {
            log_est(e.borrow().i_table as i64 as u64) - 270
        }
        _ => 1,
    };
    let p_expr = expr_skip_collate_and_likely(p);
    // O memset do C zera de eOperator em diante, inclusive iParent (o -1 anterior é
    // sobrescrito por esse memset): i_parent sai 0.
    let p_term = WhereTerm {
        p_expr,
        p_wc: Rc::downgrade(p_wc),
        truth_prob,
        wt_flags,
        e_operator: 0,
        n_child: 0,
        e_match_op: 0,
        i_parent: 0,
        left_cursor: 0,
        u: WhereTermUnion::X {
            left_column: 0,
            i_field: 0,
        },
        prereq_right: 0,
        prereq_all: 0,
    };
    p_wc.borrow_mut().a.push(Rc::new(RefCell::new(p_term)));
    idx
}

/// Retorna verdadeiro se o operador dado é um dos operadores permitidos para um termo
/// indexável de cláusula WHERE: "=", "<", ">", "<=", ">=", "IN", "IS" e "IS NULL".
fn allowed_op(op: u8) -> bool {
    debug_assert!(TK_GT > TK_EQ && TK_GT < TK_GE);
    debug_assert!(TK_LT > TK_EQ && TK_LT < TK_GE);
    debug_assert!(TK_LE > TK_EQ && TK_LE < TK_GE);
    debug_assert!(TK_GE == TK_EQ + 4);
    op == TK_IN || (op >= TK_EQ && op <= TK_GE) || op == TK_ISNULL || op == TK_IS
}

/// Comuta um operador de comparação. Expressões da forma "X op Y" viram "Y op X".
fn expr_commute(p_parse: &ParseRef, p_expr: &mut Expr) -> u16 {
    let differs = {
        let left = p_expr.p_left.as_deref().expect("comparação sem lado esquerdo");
        let right = p_expr.p_right.as_deref().expect("comparação sem lado direito");
        if left.op == TK_VECTOR || right.op == TK_VECTOR {
            true
        } else {
            let c1 = binary_compare_coll_seq(&mut p_parse.borrow_mut(), left, Some(right));
            let c2 = binary_compare_coll_seq(&mut p_parse.borrow_mut(), right, Some(left));
            !coll_seq_same(&c1, &c2)
        }
    };
    if differs {
        p_expr.flags ^= EP_COMMUTED;
    }
    std::mem::swap(&mut p_expr.p_right, &mut p_expr.p_left);
    if p_expr.op >= TK_GT {
        debug_assert!(TK_LT == TK_GT + 2);
        debug_assert!(TK_GE == TK_LE + 2);
        debug_assert!(TK_GT > TK_EQ);
        debug_assert!(TK_GT < TK_LE);
        debug_assert!(p_expr.op >= TK_GT && p_expr.op <= TK_GE);
        p_expr.op = ((p_expr.op - TK_GT) ^ 2) + TK_GT;
    }
    0
}

/// Traduz do operador TK_xx para a máscara de bits WO_xx.
fn operator_mask(op: u8) -> u16 {
    let c: u16;
    debug_assert!(allowed_op(op));
    if op == TK_IN {
        c = WO_IN;
    } else if op == TK_ISNULL {
        c = WO_ISNULL;
    } else if op == TK_IS {
        c = WO_IS;
    } else {
        debug_assert!(((WO_EQ as u32) << (op - TK_EQ)) < 0x7fff);
        c = (WO_EQ << (op - TK_EQ) as u32) as u16;
    }
    debug_assert!(op != TK_ISNULL || c == WO_ISNULL);
    debug_assert!(op != TK_IN || c == WO_IN);
    debug_assert!(op != TK_EQ || c == WO_EQ);
    debug_assert!(op != TK_LT || c == WO_LT);
    debug_assert!(op != TK_LE || c == WO_LE);
    debug_assert!(op != TK_GT || c == WO_GT);
    debug_assert!(op != TK_GE || c == WO_GE);
    debug_assert!(op != TK_IS || c == WO_IS);
    c
}

/// Verifica se a expressão dada é um operador LIKE ou GLOB que pode ser otimizado com
/// restrições de desigualdade. Retorna verdadeiro se for.
///
/// Para ser otimizável, o lado direito deve ser uma literal de string que não comece com
/// coringa. O lado esquerdo deve ser uma coluna que só pode ser NULL, string ou BLOB, nunca
/// número (tabelas virtuais não participam). A sequência de colação da coluna deve ser
/// apropriada ao operador.
///
/// `pp_prefix` recebe a expressão TK_STRING com o prefixo do padrão.
fn is_like_or_glob(
    p_parse: &ParseRef,
    p_expr: &Expr,
    pp_prefix: &mut Option<ExprRef>,
    p_is_complete: &mut i32,
    p_no_case: &mut i32,
) -> i32 {
    let mut z: Option<Vec<u8>> = None;
    let mut c: u8 = 0;
    let mut cnt: usize;
    let mut wc = [0u8; 4];
    let db = parse_db(p_parse);
    let mut p_val: Option<Box<Mem>> = None;

    if is_like_function(&db, p_expr, p_no_case, &mut wc) == 0 {
        return 0;
    }
    // SQLITE_EBCDIC não é definido no Debian: o ramo "if( *pnoCase ) return 0" some.
    debug_assert!(expr_use_x_list(p_expr));
    let p_list = p_expr.x.p_list.as_ref().expect("LIKE sem lista");
    let p_left: &Expr = p_list.a[1].p_expr.as_deref().expect("LIKE sem lado esquerdo");

    let p_right: &Expr =
        expr_skip_collate(p_list.a[0].p_expr.as_deref()).expect("LIKE sem lado direito");
    let op = p_right.op;
    if op == TK_VARIABLE && (db.borrow().flags & SQLITE_ENABLE_QPSG) == 0 {
        let p_reprepare = p_parse.borrow().p_reprepare.clone();
        let i_col = p_right.i_column as i32;
        {
            let rep = p_reprepare.as_ref().map(|v| v.borrow());
            p_val = vdbe_get_bound_value(rep.as_deref(), i_col, SQLITE_AFF_BLOB);
        }
        if let Some(v) = p_val.as_deref() {
            if value_type(v) == SQLITE_TEXT as i32 {
                z = value_text(p_val.as_deref_mut(), SQLITE_UTF8 as u8).map(|s| s.to_vec());
            }
        }
        let p_vdbe = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
        vdbe_set_varmask(&mut p_vdbe.borrow_mut(), i_col);
        debug_assert!(p_right.op == TK_VARIABLE || p_right.op == TK_REGISTER);
    } else if op == TK_STRING {
        debug_assert!(!expr_has_property(p_right, EP_INT_VALUE));
        z = Some(p_right.u.z_token.clone().unwrap_or_default());
    }
    if let Some(zv) = z.clone() {
        // O texto do C termina em NUL; fora do vetor lê-se 0.
        let at = |i: usize| -> u8 { zv.get(i).copied().unwrap_or(0) };

        // Conta os caracteres de prefixo antes do primeiro coringa.
        cnt = 0;
        loop {
            c = at(cnt);
            if !(c != 0 && c != wc[0] && c != wc[1] && c != wc[2]) {
                break;
            }
            cnt += 1;
            if c == wc[3] && at(cnt) != 0 {
                cnt += 1;
            }
        }

        // A otimização só é possível se (1) o padrão não começa com coringa e (2) o prefixo
        // sem coringa não termina em 0xff (ilegal), ou (3) o padrão não é um único caractere
        // de escape. A segunda condição é necessária para incrementar a chave de prefixo e
        // achar um limite superior. A terceira existe porque o chamador supõe pelo menos um
        // caractere depois de removidos os escapes.
        if (cnt > 1 || (cnt > 0 && at(0) != wc[3])) && 255 != at(cnt - 1) {
            // Casamento "completo" se o padrão termina com "*" ou "%".
            *p_is_complete = (c == wc[0] && at(cnt + 1) == 0) as i32;

            // Obtém o prefixo do padrão. Remove todos os escapes do prefixo.
            let z_len = zv.iter().position(|&b| b == 0).unwrap_or(zv.len());
            let mut p_prefix = expr(&db.borrow(), TK_STRING as i32, Some(&zv[..z_len]));
            if let Some(pre) = p_prefix.as_mut() {
                debug_assert!(!expr_has_property(pre, EP_INT_VALUE));
                // zNew[cnt] = 0
                let mut z_new: Vec<u8> = pre.u.z_token.clone().unwrap_or_default();
                z_new.truncate(cnt);
                z_new.push(0);
                let at_new = |v: &Vec<u8>, i: usize| -> u8 { v.get(i).copied().unwrap_or(0) };
                let mut out: Vec<u8> = Vec::new();
                let mut i_from = 0usize;
                while i_from < cnt {
                    if at_new(&z_new, i_from) == wc[3] {
                        i_from += 1;
                    }
                    out.push(at_new(&z_new, i_from));
                    i_from += 1;
                }
                let mut i_to = out.len();
                debug_assert!(i_to > 0);

                // Se o lado esquerdo não é uma coluna comum com afinidade TEXT, as fronteiras
                // do prefixo (inicial e final) não podem parecer um número. Senão o padrão
                // pode ser tratado como número e invalidar a otimização LIKE.
                //
                // Acertar isto é uma fonte persistente de bugs. Ver, por exemplo:
                //    2018-09-10 https://sqlite.org/src/info/c94369cae9b561b1
                //    2019-05-02 https://sqlite.org/src/info/b043a54c3de54b28
                //    2019-06-10 https://sqlite.org/src/info/fd76310a5e843e07
                //    2019-06-14 https://sqlite.org/src/info/ce8717f0885af975
                //    2019-09-03 https://sqlite.org/src/info/0f0428096f17252a
                let is_virtual_tab = p_left
                    .y
                    .p_tab
                    .as_ref()
                    .map_or(false, |t| is_virtual(&t.borrow()));
                if p_left.op != TK_COLUMN
                    || expr_affinity(p_left) != SQLITE_AFF_TEXT
                    || is_virtual_tab
                {
                    let mut r_dummy: f64 = 0.0;
                    let mut is_num = ato_f(&out[..i_to], &mut r_dummy, i_to as i32, SQLITE_UTF8 as u8);
                    if is_num <= 0 {
                        if i_to == 1 && out[0] == b'-' {
                            is_num = 1;
                        } else {
                            out[i_to - 1] = out[i_to - 1].wrapping_add(1);
                            is_num =
                                ato_f(&out[..i_to], &mut r_dummy, i_to as i32, SQLITE_UTF8 as u8);
                            out[i_to - 1] = out[i_to - 1].wrapping_sub(1);
                        }
                    }
                    if is_num > 0 {
                        // sqlite3ExprDelete(db, pPrefix) e sqlite3ValueFree(pVal)
                        drop(p_prefix);
                        value_free(p_val);
                        return 0;
                    }
                }
                out.truncate(i_to);
                i_to = out.len();
                let _ = i_to;
                pre.u.z_token = Some(out);
            }
            *pp_prefix = expr_ref_new(p_prefix);

            // Se o padrão do lado direito é um parâmetro vinculado, providencia reprepare do
            // comando quando o parâmetro for revinculado.
            if op == TK_VARIABLE {
                let v = p_parse.borrow().p_vdbe.clone().expect("Parse sem Vdbe");
                vdbe_set_varmask(&mut v.borrow_mut(), p_right.i_column as i32);
                debug_assert!(!expr_has_property(p_right, EP_INT_VALUE));
                let tok1 = p_right
                    .u
                    .z_token
                    .as_ref()
                    .and_then(|t| t.get(1).copied())
                    .unwrap_or(0);
                if *p_is_complete != 0 && tok1 != 0 {
                    // Se o lado direito do LIKE é uma variável e o valor atual dela dispensa
                    // chamar a função LIKE, nenhum OP_Variable seria adicionado, o que
                    // atrapalha sqlite3_bind_parameter_name(). Para contornar, adiciona um
                    // OP_Variable de enfeite aqui.
                    let r1 = get_temp_reg(&mut p_parse.borrow_mut());
                    expr_code_target(p_parse, p_right, r1);
                    let addr = vdbe_current_addr(&v.borrow()) - 1;
                    vdbe_change_p3(&mut v.borrow_mut(), addr, 0);
                    release_temp_reg(&mut p_parse.borrow_mut(), r1);
                }
            }
        } else {
            z = None;
        }
    }

    let rc = z.is_some() as i32;
    value_free(p_val);
    rc
}

/// Verifica se a expressão p_expr é uma forma que precisa ser passada ao método xBestIndex
/// de tabelas virtuais. Formas de interesse:
///
///          Expressão                    Operador de tabela virtual
///          -----------------------      ---------------------------------
///      1.  column MATCH expr            SQLITE_INDEX_CONSTRAINT_MATCH
///      2.  column GLOB expr             SQLITE_INDEX_CONSTRAINT_GLOB
///      3.  column LIKE expr             SQLITE_INDEX_CONSTRAINT_LIKE
///      4.  column REGEXP expr           SQLITE_INDEX_CONSTRAINT_REGEXP
///      5.  column != expr               SQLITE_INDEX_CONSTRAINT_NE
///      6.  expr != column               SQLITE_INDEX_CONSTRAINT_NE
///      7.  column IS NOT expr           SQLITE_INDEX_CONSTRAINT_ISNOT
///      8.  expr IS NOT column           SQLITE_INDEX_CONSTRAINT_ISNOT
///      9.  column IS NOT NULL           SQLITE_INDEX_CONSTRAINT_ISNOTNULL
///
/// Em todo caso "column" deve ser coluna de tabela virtual. Havendo casamento, `pp_left`
/// recebe a expressão "column", `pp_right` a expressão "expr" (mesmo nas formas (6) e (8),
/// em que a coluna está à direita) e `pe_op2` o operador de tabela virtual. O retorno é 1 ou
/// 2 se há casamento: o usual é 1, mas é 2 se o lado direito também é coluna de tabela
/// virtual nas formas (5) ou (7). Se nada casa, retorna 0.
fn is_auxiliary_vtab_operator(
    db: &Sqlite3Ref,
    p_expr: &ExprRef,
    pe_op2: &mut u8,
    pp_left: &mut Option<ExprRef>,
    pp_right: &mut Option<ExprRef>,
) -> i32 {
    let op = p_expr.borrow().op;
    if op == TK_FUNCTION {
        // static const struct Op2 { zOp; eOp2 } aOp[]
        const A_OP: [(&[u8], u8); 4] = [
            (b"match", SQLITE_INDEX_CONSTRAINT_MATCH as u8),
            (b"glob", SQLITE_INDEX_CONSTRAINT_GLOB as u8),
            (b"like", SQLITE_INDEX_CONSTRAINT_LIKE as u8),
            (b"regexp", SQLITE_INDEX_CONSTRAINT_REGEXP as u8),
        ];

        debug_assert!(expr_use_x_list(&p_expr.borrow()));
        let n_expr = match p_expr.borrow().x.p_list.as_ref() {
            Some(l) => l.n_expr,
            None => return 0,
        };
        if n_expr != 2 {
            return 0;
        }

        // Os operadores embutidos MATCH, GLOB, LIKE e REGEXP se ligam a uma tabela virtual no
        // segundo argumento, que é o operando esquerdo na forma infixa.
        //
        //       vtab_column MATCH expression
        //       MATCH(expression,vtab_column)
        let p_col = expr_list_item_ref(p_expr, 1).expect("função sem segundo argumento");
        if expr_is_vtab(&p_col.borrow()) {
            for (z_op, e_op2) in A_OP.iter() {
                debug_assert!(!expr_has_property(&p_expr.borrow(), EP_INT_VALUE));
                let tok = p_expr.borrow().u.z_token.clone().unwrap_or_default();
                if str_i_cmp(&tok, z_op) == 0 {
                    *pe_op2 = *e_op2;
                    *pp_right = expr_list_item_ref(p_expr, 0);
                    *pp_left = Some(p_col);
                    return 1;
                }
            }
        }

        // Também casamos com a primeira coluna de funções sobrecarregadas em que xFindFunction
        // devolve um valor de pelo menos SQLITE_INDEX_CONSTRAINT_FUNCTION.
        //
        //      OVERLOADED(vtab_column,expression)
        //
        // Historicamente xFindFunction esperava nomes de função em minúsculas. Neste uso,
        // porém, ele deve tratar nomes com caixa arbitrária.
        let p_col = expr_list_item_ref(p_expr, 0).expect("função sem primeiro argumento");
        if expr_is_vtab(&p_col.borrow()) {
            let p_tab = p_col
                .borrow()
                .y
                .p_tab
                .clone()
                .expect("coluna de tabela virtual sem Table");
            let p_vtab = v_table_get(db, &p_tab).p_vtab.clone();
            debug_assert!(p_vtab.is_some());
            let p_vtab = p_vtab.expect("VTable sem sqlite3_vtab");
            let p_mod = p_vtab.borrow().p_module.clone();
            debug_assert!(p_mod.is_some());
            debug_assert!(!expr_has_property(&p_expr.borrow(), EP_INT_VALUE));
            if let Some(m) = p_mod {
                let tok = p_expr.borrow().u.z_token.clone().unwrap_or_default();
                if m.has_find_function() {
                    // xNotUsed e pNotUsed são saídas ignoradas.
                    let i = m.x_find_function(&p_vtab, 2, &tok);
                    if i >= SQLITE_INDEX_CONSTRAINT_FUNCTION {
                        *pe_op2 = i as u8;
                        *pp_right = expr_list_item_ref(p_expr, 1);
                        *pp_left = Some(p_col);
                        return 1;
                    }
                }
            }
        }
    } else if op == TK_NE || op == TK_ISNOT || op == TK_NOTNULL {
        let mut res = 0;
        let mut p_left = expr_left(p_expr);
        let mut p_right = expr_right(p_expr);
        debug_assert!(p_left.as_ref().map_or(true, |l| {
            let l = l.borrow();
            l.op != TK_COLUMN || (expr_use_y_tab(&l) && l.y.p_tab.is_some())
        }));
        if let Some(l) = &p_left {
            if expr_is_vtab(&l.borrow()) {
                res += 1;
            }
        }
        debug_assert!(p_right.as_ref().map_or(true, |r| {
            let r = r.borrow();
            r.op != TK_COLUMN || (expr_use_y_tab(&r) && r.y.p_tab.is_some())
        }));
        if let Some(r) = &p_right {
            if expr_is_vtab(&r.borrow()) {
                res += 1;
                std::mem::swap(&mut p_left, &mut p_right);
            }
        }
        *pp_left = p_left;
        *pp_right = p_right;
        if op == TK_NE {
            *pe_op2 = SQLITE_INDEX_CONSTRAINT_NE as u8;
        }
        if op == TK_ISNOT {
            *pe_op2 = SQLITE_INDEX_CONSTRAINT_ISNOT as u8;
        }
        if op == TK_NOTNULL {
            *pe_op2 = SQLITE_INDEX_CONSTRAINT_ISNOTNULL as u8;
        }
        return res;
    }
    0
}


// ---- part_001.rs ----

/// Se a expressão pBase se originou na cláusula ON ou USING de um join, transfere as marcas
/// apropriadas para a expressão derivada.
fn transfer_join_markings(p_derived: Option<&ExprRef>, p_base: &ExprRef) {
    if let Some(derived) = p_derived {
        let base = p_base.borrow();
        if expr_has_property(&base, EP_OUTER_ON | EP_INNER_ON) {
            let mut d = derived.borrow_mut();
            d.flags |= base.flags & (EP_OUTER_ON | EP_INNER_ON);
            d.w.i_join = base.w.i_join;
        }
    }
}

/// Marca o termo i_child como filho do termo i_parent.
fn mark_term_as_child(p_wc: &WhereClauseRef, i_child: i32, i_parent: i32) {
    let (child, parent) = {
        let wc = p_wc.borrow();
        (wc.a[i_child as usize].clone(), wc.a[i_parent as usize].clone())
    };
    child.borrow_mut().i_parent = i_parent;
    child.borrow_mut().truth_prob = parent.borrow().truth_prob;
    parent.borrow_mut().n_child += 1;
}

/// Retorna o N-ésimo subtermo ligado por AND de p_term. Se p_term não é uma conjunção,
/// retorna o próprio p_term quando N==0. Se N excede o número de subtermos, retorna None.
fn where_nth_subterm(p_term: &WhereTermRef, n: i32) -> Option<WhereTermRef> {
    if p_term.borrow().e_operator != WO_AND {
        return if n == 0 { Some(p_term.clone()) } else { None };
    }
    let t = p_term.borrow();
    if let WhereTermUnion::AndInfo(Some(p_and_info)) = &t.u {
        let wc = p_and_info.wc.borrow();
        if n < wc.n_term {
            return Some(wc.a[n as usize].clone());
        }
    }
    None
}

/// Os subtermos p_one e p_two estão contidos na cláusula WHERE p_wc. Os dois subtermos estão
/// em disjunção (ligados por OR).
///
/// Se ambos têm a forma "A op B" com os mesmos valores A e B mas operadores diferentes, e se
/// os operadores são compatíveis (um é = e o outro é <, por exemplo), adiciona a p_wc um novo
/// termo AND virtual que combina os dois.
///
/// Exemplos:
///
///    x<y OR x=y    -->     x<=y
///    x=y OR x=y    -->     x=y
///    x<=y OR x<y   -->     x<=y
///
/// O seguinte NÃO é gerado:
///
///    x<y OR x>y    -->     x!=y
fn where_combine_disjuncts(
    p_src: &SrcList,
    p_wc: &WhereClauseRef,
    p_one: &WhereTermRef,
    p_two: &WhereTermRef,
) {
    let mut e_op: u16 = p_one.borrow().e_operator | p_two.borrow().e_operator;

    if ((p_one.borrow().wt_flags | p_two.borrow().wt_flags) & TERM_VNULL) != 0 {
        return;
    }
    if (p_one.borrow().e_operator & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (p_two.borrow().e_operator & (WO_EQ | WO_LT | WO_LE | WO_GT | WO_GE)) == 0 {
        return;
    }
    if (e_op & (WO_EQ | WO_LT | WO_LE)) != e_op && (e_op & (WO_EQ | WO_GT | WO_GE)) != e_op {
        return;
    }
    let e1 = p_one.borrow().p_expr.clone().expect("termo sem expressão");
    let e2 = p_two.borrow().p_expr.clone().expect("termo sem expressão");
    {
        let (b1, b2) = (e1.borrow(), e2.borrow());
        debug_assert!(b1.p_left.is_some() && b1.p_right.is_some());
        debug_assert!(b2.p_left.is_some() && b2.p_right.is_some());
        if expr_compare(
            None,
            b1.p_left.as_deref().unwrap(),
            b2.p_left.as_deref().unwrap(),
            -1,
        ) != 0
        {
            return;
        }
        if expr_compare(
            None,
            b1.p_right.as_deref().unwrap(),
            b2.p_right.as_deref().unwrap(),
            -1,
        ) != 0
        {
            return;
        }
    }
    // Se chegamos aqui, os dois subtermos podem ser combinados.
    if (e_op & (e_op - 1)) != 0 {
        if (e_op & (WO_LT | WO_LE)) != 0 {
            e_op = WO_LE;
        } else {
            debug_assert!((e_op & (WO_GT | WO_GE)) != 0);
            e_op = WO_GE;
        }
    }
    let db = parse_db(&where_parse_of(p_wc));
    let p_new = match expr_dup_ref(&db, &e1) {
        Some(n) => n,
        None => return,
    };
    let mut p_new = p_new;
    let mut op = TK_EQ;
    while e_op != (WO_EQ << (op - TK_EQ) as u32) {
        debug_assert!(op < TK_GE);
        op += 1;
    }
    p_new.op = op;
    let idx_new = where_clause_insert(
        p_wc,
        expr_ref_new(Some(p_new)),
        TERM_VIRTUAL | TERM_DYNAMIC,
    );
    expr_analyze(p_src, p_wc, idx_new);
}

/// Analisa um termo que consiste em dois ou mais subtermos ligados por OR. Em:
///
///     ... WHERE  (a=5) AND (b=7 OR c=9 OR d=13) AND (d=13)
///                          ^^^^^^^^^^^^^^^^^^^^
///
/// esta rotina analisa termos como o do meio. Um objeto WhereOrTerm é computado e anexado ao
/// termo analisado, qualquer que seja o resultado da análise. Logo:
///
///     WhereTerm.wtFlags   |=  TERM_ORINFO
///     WhereTerm.u.pOrInfo  =  um objeto WhereOrTerm alocado dinamicamente
///
/// O termo analisado deve ter dois ou mais subtermos ligados por OR. Um subtermo pode ser um
/// conjunto de sub-subtermos ligados por AND. Exemplos de termos em análise:
///
///     (A)     t1.x=t2.y OR t1.x=t2.z OR t1.y=15 OR t1.z=t3.a+5
///     (B)     x=expr1 OR expr2=x OR x=expr3
///     (C)     t1.x=t2.y OR (t1.x=t2.z AND t1.y=15)
///     (D)     x=expr1 OR (y>11 AND y<22 AND z LIKE '*hello*')
///     (E)     (p.a=1 AND q.b=2 AND r.c=3) OR (p.x=4 AND q.y=5 AND r.z=6)
///     (F)     x>A OR (x=A AND y>=B)
///
/// CASO 1:
///
/// Se todos os subtermos têm a forma T.C=expr para uma única coluna C de uma única tabela T
/// (exemplo B), cria um termo virtual que é uma expressão IN equivalente. Isto é, se o termo é
///
///      x = expr1  OR  expr2 = x  OR  x = expr3
///
/// cria um termo virtual assim:
///
///      x IN (expr1,expr2,expr3)
///
/// CASO 2:
///
/// Se há exatamente dois disjuntos e um lado tem x>A e o outro tem x=A (mesmo x e A),
/// adiciona à cláusula WHERE um novo termo conjuntivo virtual "x>=A". Exemplo:
///
///      x>A OR (x=A AND y>B)    adiciona:    x>=A
///
/// O conjunto adicionado pode ajudar no planejamento da consulta.
///
/// CASO 3:
///
/// Se todos os subtermos são indexáveis por uma única tabela T, define
///
///     WhereTerm.eOperator              =  WO_OR
///     WhereTerm.u.pOrInfo->indexable  |=  o número de cursor da tabela T
///
/// Um subtermo é "indexável" se tem a forma "T.C <op> <expr>", com C qualquer coluna da tabela
/// T e <op> um de "=", "<", "<=", ">", ">=", "IS NULL" ou "IN". Um subtermo também é
/// indexável se é um AND de dois ou mais sub-subtermos, ao menos um deles indexável. Subtermos
/// AND indexáveis têm eOperator igual a WO_AND e u.pAndInfo apontando para um objeto
/// WhereAndTerm alocado dinamicamente.
///
/// Por outro ângulo, "indexável" significa que o subtermo poderia ser usado com um índice, se
/// existir um adequado. A análise não considera se o índice existe; isso se decide noutro
/// lugar. Ela só olha se existem subtermos apropriados para indexação.
///
/// Todos os exemplos A a E satisfazem o caso 3. Mas se um termo também satisfaz o caso 1
/// (como B), o otimizador sempre prefere o caso 1, então nesse caso finge-se que o caso 3 não
/// vale.
///
/// Várias tabelas podem ser indexáveis. Por exemplo, (E) é indexável nas tabelas P, Q e R.
///
/// Termos que satisfazem o caso 3 são candidatos a busca com índices separados para achar
/// rowids de cada subtermo e compor a união de todos os rowids com um objeto RowSet. É
/// parecido com os "bitmap indices" de outros bancos.
///
/// SENÃO:
///
/// Se nenhum dos casos 1, 2 ou 3 vale, deixa eOperator em zero. O termo não é útil para busca.
fn expr_analyze_or_term(p_src: &SrcList, p_wc: &WhereClauseRef, idx_term: i32) {
    let p_w_info = where_info_of(p_wc);
    let p_parse = p_w_info.borrow().p_parse.clone();
    let db = parse_db(&p_parse);
    let p_term: WhereTermRef = p_wc.borrow().a[idx_term as usize].clone();
    let p_term_expr: ExprRef = p_term.borrow().p_expr.clone().expect("termo sem expressão");
    let mut i: i32;
    let mut indexable: Bitmask;
    let mut chng_to_in: Bitmask;

    // Quebra a cláusula OR nos subtermos. Eles ficam numa WhereClause dentro do objeto
    // WhereOrInfo anexado ao termo OR original.
    debug_assert!((p_term.borrow().wt_flags & (TERM_DYNAMIC | TERM_ORINFO | TERM_ANDINFO)) == 0);
    debug_assert!(p_term_expr.borrow().op == TK_OR);
    let p_or_wc: WhereClauseRef = where_clause_new();
    p_term.borrow_mut().u = WhereTermUnion::OrInfo(Some(Box::new(WhereOrInfo {
        wc: p_or_wc.clone(),
        indexable: 0,
    })));
    p_term.borrow_mut().wt_flags |= TERM_ORINFO;
    where_clause_init(&p_or_wc, &p_w_info);
    where_split(&p_or_wc, Some(p_term_expr.clone()), TK_OR);
    where_expr_analyze(p_src, &p_or_wc);
    if db_malloc_failed(&db) {
        return;
    }
    debug_assert!(p_or_wc.borrow().n_term >= 2);

    // Computa o conjunto de tabelas que podem satisfazer os casos 1 ou 3.
    indexable = !0u64;
    chng_to_in = !0u64;
    let n_or = p_or_wc.borrow().n_term;
    i = n_or - 1;
    let mut k: usize = 0;
    while i >= 0 && indexable != 0 {
        let p_or_term: WhereTermRef = p_or_wc.borrow().a[k].clone();
        if (p_or_term.borrow().e_operator & WO_SINGLE) == 0 {
            debug_assert!((p_or_term.borrow().wt_flags & (TERM_ANDINFO | TERM_ORINFO)) == 0);
            chng_to_in = 0;
            // pAndInfo = sqlite3DbMallocRawNN(..): a alocação não falha no Rust.
            let p_and_wc: WhereClauseRef = where_clause_new();
            let mut b: Bitmask = 0;
            p_or_term.borrow_mut().u = WhereTermUnion::AndInfo(Some(Box::new(WhereAndInfo {
                wc: p_and_wc.clone(),
            })));
            p_or_term.borrow_mut().wt_flags |= TERM_ANDINFO;
            p_or_term.borrow_mut().e_operator = WO_AND;
            p_or_term.borrow_mut().left_cursor = -1;
            where_clause_init(&p_and_wc, &p_w_info);
            where_split(&p_and_wc, p_or_term.borrow().p_expr.clone(), TK_AND);
            where_expr_analyze(p_src, &p_and_wc);
            p_and_wc.borrow_mut().p_outer = Some(Rc::downgrade(p_wc));
            if !db_malloc_failed(&db) {
                let n_and = p_and_wc.borrow().n_term;
                for j in 0..n_and {
                    let p_and_term = p_and_wc.borrow().a[j as usize].clone();
                    let t = p_and_term.borrow();
                    debug_assert!(t.p_expr.is_some());
                    if allowed_op(t.p_expr.as_ref().unwrap().borrow().op) || t.e_operator == WO_AUX {
                        b |= where_get_mask(&p_w_info.borrow().s_mask_set, t.left_cursor);
                    }
                }
            }
            indexable &= b;
        } else if (p_or_term.borrow().wt_flags & TERM_COPIED) != 0 {
            // Pula este termo por ora. Ele é revisitado ao processar o termo TERM_VIRTUAL
            // correspondente.
        } else {
            let mut b: Bitmask =
                where_get_mask(&p_w_info.borrow().s_mask_set, p_or_term.borrow().left_cursor);
            if (p_or_term.borrow().wt_flags & TERM_VIRTUAL) != 0 {
                let i_parent = p_or_term.borrow().i_parent;
                let p_other = p_or_wc.borrow().a[i_parent as usize].clone();
                b |= where_get_mask(&p_w_info.borrow().s_mask_set, p_other.borrow().left_cursor);
            }
            indexable &= b;
            if (p_or_term.borrow().e_operator & WO_EQ) == 0 {
                chng_to_in = 0;
            } else {
                chng_to_in &= b;
            }
        }
        i -= 1;
        k += 1;
    }

    // Registra o conjunto de tabelas que satisfazem o caso 3. O conjunto pode ser vazio.
    if let WhereTermUnion::OrInfo(Some(p_or_info)) = &mut p_term.borrow_mut().u {
        p_or_info.indexable = indexable;
    }
    p_term.borrow_mut().e_operator = WO_OR;
    p_term.borrow_mut().left_cursor = -1;
    if indexable != 0 {
        p_wc.borrow_mut().has_or = 1;
    }

    // Num OR de duas vias, tenta implementar o caso 2.
    if indexable != 0 && p_or_wc.borrow().n_term == 2 {
        let mut i_one = 0;
        let a0 = p_or_wc.borrow().a[0].clone();
        let a1 = p_or_wc.borrow().a[1].clone();
        loop {
            let p_one = where_nth_subterm(&a0, i_one);
            i_one += 1;
            let p_one = match p_one {
                Some(t) => t,
                None => break,
            };
            let mut i_two = 0;
            loop {
                let p_two = where_nth_subterm(&a1, i_two);
                i_two += 1;
                let p_two = match p_two {
                    Some(t) => t,
                    None => break,
                };
                where_combine_disjuncts(p_src, p_wc, &p_one, &p_two);
            }
        }
    }

    // chng_to_in guarda um conjunto de tabelas que PODEM satisfazer o caso 1. Mas é preciso
    // checar mais para saber se o caso 1 vale de fato.
    //
    // chng_to_in terá 0, 1 ou 2 bits. O caso de 0 bit significa que não há como transformar o
    // OR num operador IN, porque um ou mais termos contêm algo diferente de == sobre uma
    // coluna de uma única tabela. O caso de 1 bit significa que todo termo do OR tem a forma
    // "tabela.coluna=expr" para uma única tabela; o bit ligado corresponde à tabela comum.
    // Ainda é preciso conferir se a mesma coluna é usada em todos os termos. O caso de 2 bits
    // é quando todos os termos têm a forma "tabela1.coluna=tabela2.coluna"; talvez dê para
    // formar um IN com tabela1.coluna ou tabela2.coluna no lado esquerdo, se uma delas é comum
    // a todos os termos do OR.
    //
    // Termos "tabela.coluna1=tabela.coluna2" (mesma tabela dos dois lados do ==) não podem ser
    // otimizados.
    if chng_to_in != 0 {
        let mut ok_to_chng_to_in = false;
        let mut i_column: i32 = -1;
        let mut i_cursor: i32 = -1;
        let mut p_left: Option<ExprRef> = None;

        // Procura uma tabela e coluna que apareça de um lado ou do outro do == em todo
        // subtermo. Essa tabela e coluna ficam em i_cursor e i_column. Pode não existir.
        // ok_to_chng_to_in vale true se uma tabela e coluna adequadas são achadas.
        let mut j = 0;
        while j < 2 && !ok_to_chng_to_in {
            p_left = None;
            i = n_or - 1;
            k = 0;
            while i >= 0 {
                let p_or_term = p_or_wc.borrow().a[k].clone();
                debug_assert!((p_or_term.borrow().e_operator & WO_EQ) != 0);
                p_or_term.borrow_mut().wt_flags &= !TERM_OK;
                if p_or_term.borrow().left_cursor == i_cursor {
                    // Este é o caso de 2 bits, na segunda iteração, e o termo atual vem da
                    // primeira iteração. Pula este termo.
                    debug_assert!(j == 1);
                    i -= 1;
                    k += 1;
                    continue;
                }
                if (chng_to_in
                    & where_get_mask(&p_w_info.borrow().s_mask_set, p_or_term.borrow().left_cursor))
                    == 0
                {
                    // Este termo deve ter a forma t1.a==t2.b, com t2 no conjunto chng_to_in e
                    // t1 fora dele. O termo será precedido ou seguido por uma cópia invertida
                    // (t2.b==t1.a). Pula este termo e usa a inversão dele.
                    debug_assert!(
                        (p_or_term.borrow().wt_flags & (TERM_COPIED | TERM_VIRTUAL)) != 0
                    );
                    i -= 1;
                    k += 1;
                    continue;
                }
                debug_assert!((p_or_term.borrow().e_operator & (WO_OR | WO_AND)) == 0);
                if let WhereTermUnion::X { left_column, .. } = &p_or_term.borrow().u {
                    i_column = *left_column;
                }
                i_cursor = p_or_term.borrow().left_cursor;
                p_left = expr_left(p_or_term.borrow().p_expr.as_ref().unwrap());
                break;
            }
            if i < 0 {
                // Nenhuma tabela+coluna candidata foi achada. Só pode ocorrer na segunda
                // iteração.
                debug_assert!(j == 1);
                debug_assert!(is_power_of_two(chng_to_in as usize));
                debug_assert!(chng_to_in == where_get_mask(&p_w_info.borrow().s_mask_set, i_cursor));
                break;
            }

            // Achamos uma tabela e coluna candidatas. Confere se são comuns a todos os termos
            // da cláusula OR.
            ok_to_chng_to_in = true;
            while i >= 0 && ok_to_chng_to_in {
                let p_or_term = p_or_wc.borrow().a[k].clone();
                debug_assert!((p_or_term.borrow().e_operator & WO_EQ) != 0);
                debug_assert!((p_or_term.borrow().e_operator & (WO_OR | WO_AND)) == 0);
                if p_or_term.borrow().left_cursor != i_cursor {
                    p_or_term.borrow_mut().wt_flags &= !TERM_OK;
                } else {
                    let left_column = match &p_or_term.borrow().u {
                        WhereTermUnion::X { left_column, .. } => *left_column,
                        _ => 0,
                    };
                    let or_expr = p_or_term.borrow().p_expr.clone().unwrap();
                    let expr_differs = left_column == XN_EXPR as i32 && {
                        let ol = or_expr.borrow();
                        let pl = p_left.as_ref().expect("pLeft ausente").borrow();
                        expr_compare(
                            Some(&p_parse.borrow()),
                            ol.p_left.as_deref().unwrap(),
                            &pl,
                            -1,
                        ) != 0
                    };
                    if left_column != i_column || expr_differs {
                        ok_to_chng_to_in = false;
                    } else {
                        // Se o lado direito também é uma coluna, as afinidades dos dois lados
                        // devem ser tais que nenhuma conversão de tipo seja necessária à
                        // direita. (Ticket #2249)
                        let (aff_right, aff_left) = {
                            let oe = or_expr.borrow();
                            (
                                expr_affinity(oe.p_right.as_deref().unwrap()),
                                expr_affinity(oe.p_left.as_deref().unwrap()),
                            )
                        };
                        if aff_right != 0 && aff_right != aff_left {
                            ok_to_chng_to_in = false;
                        } else {
                            p_or_term.borrow_mut().wt_flags |= TERM_OK;
                        }
                    }
                }
                i -= 1;
                k += 1;
            }
            j += 1;
        }

        // Neste ponto ok_to_chng_to_in é true se o p_term original satisfaz o caso 1. Então
        // constrói um termo virtual que é p_term convertido num operador IN.
        if ok_to_chng_to_in {
            let mut p_list: Option<Box<ExprList>> = None;
            let mut p_left: Option<ExprRef> = None;

            i = n_or - 1;
            for kk in 0..(n_or as usize) {
                let p_or_term = p_or_wc.borrow().a[kk].clone();
                i -= 1;
                if (p_or_term.borrow().wt_flags & TERM_OK) == 0 {
                    continue;
                }
                debug_assert!((p_or_term.borrow().e_operator & WO_EQ) != 0);
                debug_assert!((p_or_term.borrow().e_operator & (WO_OR | WO_AND)) == 0);
                debug_assert!(p_or_term.borrow().left_cursor == i_cursor);
                let or_expr = p_or_term.borrow().p_expr.clone().unwrap();
                let p_dup = {
                    let oe = or_expr.borrow();
                    expr_dup(&db.borrow(), oe.p_right.as_deref().unwrap(), 0)
                };
                p_list = expr_list_append(&mut p_parse.borrow_mut(), p_list, p_dup);
                p_left = expr_left(&or_expr);
            }
            let _ = i;
            debug_assert!(p_left.is_some());
            let p_left = p_left.expect("pLeft ausente");
            let p_dup = expr_dup_ref(&db, &p_left);
            let p_new = p_expr(&mut p_parse.borrow_mut(), TK_IN as i32, p_dup, None);
            let p_new = expr_ref_new(p_new);
            if let Some(new_ref) = p_new {
                transfer_join_markings(Some(&new_ref), &p_term_expr);
                debug_assert!(expr_use_x_list(&new_ref.borrow()));
                new_ref.borrow_mut().x.p_list = p_list;
                let idx_new =
                    where_clause_insert(p_wc, Some(new_ref), TERM_VIRTUAL | TERM_DYNAMIC);
                expr_analyze(p_src, p_wc, idx_new);
                // pTerm = &pWC->a[idxTerm]; seria necessário se pTerm fosse reutilizado.
                mark_term_as_child(p_wc, idx_new, idx_term);
            } else {
                // sqlite3ExprListDelete(db, pList): p_list é liberada ao sair de escopo.
                drop(p_list);
            }
        }
    }
}


// ---- part_002.rs ----

/// Já sabemos que p_orig é um operador binário em que ambos os operandos são referências de
/// coluna. Esta rotina confere se p_orig é uma relação de equivalência:
///   1.  A otimização SQLITE_Transitive deve estar habilitada
///   2.  Deve ser um operador == ou IS
///   3.  Não originar da cláusula ON de um OUTER JOIN
///   4.  As afinidades de A e B devem ser compatíveis
///   5a. Ambos os operandos usam a mesma sequência de colação OU
///   5b. A sequência de colação geral é BINARY
/// Se a rotina retorna true, o lado direito pode ser substituído pelo esquerdo em qualquer
/// outro lugar da cláusula WHERE em que a coluna esquerda ocorra. É uma otimização: retornar 0
/// não faz mal. Mas retornar 1 quando não deveria pode produzir respostas incorretas.
fn term_is_equivalence(p_parse: &ParseRef, p_orig: &Expr) -> i32 {
    let db = parse_db(p_parse);
    if !optimization_enabled(&db.borrow(), SQLITE_TRANSITIVE) {
        return 0;
    }
    if p_orig.op != TK_EQ && p_orig.op != TK_IS {
        return 0;
    }
    if expr_has_property(p_orig, EP_OUTER_ON) {
        return 0;
    }
    let p_left = p_orig.p_left.as_deref().expect("comparação sem lado esquerdo");
    let p_right = p_orig.p_right.as_deref().expect("comparação sem lado direito");
    let aff1 = expr_affinity(p_left);
    let aff2 = expr_affinity(p_right);
    if aff1 != aff2 && (!is_numeric_affinity(aff1) || !is_numeric_affinity(aff2)) {
        return 0;
    }
    let p_coll = expr_compare_coll_seq(&mut p_parse.borrow_mut(), p_orig);
    {
        let guard = p_coll.as_ref().map(|c| c.borrow());
        if is_binary(guard.as_deref()) != 0 {
            return 1;
        }
    }
    expr_coll_seq_match(&mut p_parse.borrow_mut(), p_left, p_right) as i32
}

/// Percorre recursivamente as expressões de um comando SELECT e gera uma máscara de bits que
/// indica quais tabelas são usadas naquela árvore de expressões.
fn expr_select_usage(p_mask_set: &mut WhereMaskSet, p_s: Option<&Select>) -> Bitmask {
    let mut mask: Bitmask = 0;
    let mut p_s = p_s;
    while let Some(s) = p_s {
        let p_src = s.p_src.as_deref();
        mask |= where_expr_list_usage(p_mask_set, s.p_elist.as_deref());
        mask |= where_expr_list_usage(p_mask_set, s.p_group_by.as_deref());
        mask |= where_expr_list_usage(p_mask_set, s.p_order_by.as_deref());
        mask |= where_expr_usage(p_mask_set, s.p_where.as_deref());
        mask |= where_expr_usage(p_mask_set, s.p_having.as_deref());
        if let Some(src) = p_src {
            for i in 0..src.n_src {
                let item = &src.a[i as usize];
                mask |= expr_select_usage(p_mask_set, item.p_select.as_deref());
                if item.fg.is_using == 0 {
                    if let SrcItemU3::On(p_on) = &item.u3 {
                        mask |= where_expr_usage(p_mask_set, Some(p_on));
                    }
                }
                if item.fg.is_tab_func != 0 {
                    if let SrcItemU1::FuncArg(p_func_arg) = &item.u1 {
                        mask |= where_expr_list_usage(p_mask_set, Some(p_func_arg));
                    }
                }
            }
        }
        p_s = s.p_prior.as_deref();
    }
    mask
}

/// A expressão p_orig é um operando de um operador de comparação que pode ser útil para
/// indexação. Esta rotina confere se p_orig aparece em algum índice. Retorna TRUE (1) se
/// p_orig é um termo indexado e FALSE (0) se não. Se retorna TRUE, também grava em
/// ai_cur_col[0] o número do cursor da tabela indexada e em ai_cur_col[1] o número da coluna
/// indexada, ou XN_EXPR (-2) se uma expressão está sendo indexada.
///
/// Se p_orig é uma referência de coluna TK_COLUMN, a rotina sempre retorna true, mesmo que
/// aquela coluna não seja indexada, porque a coluna pode ser adicionada a um índice
/// automático depois.
fn expr_might_be_indexed_2(
    p_from: &SrcList,
    ai_cur_col: &mut [i32; 2],
    p_orig: &Expr,
    j: i32,
) -> i32 {
    let mut j = j;
    loop {
        let i_cur = p_from.a[j as usize].i_cursor;
        let mut p_idx: Option<IndexRef> = p_from.a[j as usize]
            .p_tab
            .as_ref()
            .and_then(|t| t.borrow().p_index.clone());
        while let Some(idx_ref) = p_idx {
            let idx = idx_ref.borrow();
            if let Some(a_col_expr) = idx.a_col_expr.as_ref() {
                for i in 0..idx.n_key_col as usize {
                    if idx.ai_column[i] != XN_EXPR {
                        continue;
                    }
                    debug_assert!(idx.b_has_expr);
                    let p_col_expr = a_col_expr.a[i].p_expr.as_deref().expect("índice sem expressão");
                    if expr_compare_skip(p_orig, p_col_expr, i_cur) == 0
                        && expr_is_constant(None, p_col_expr) == 0
                    {
                        ai_cur_col[0] = i_cur;
                        ai_cur_col[1] = XN_EXPR as i32;
                        return 1;
                    }
                }
            }
            let next = idx.p_next.clone();
            drop(idx);
            p_idx = next;
        }
        j += 1;
        if j >= p_from.n_src {
            break;
        }
    }
    0
}

fn expr_might_be_indexed(
    p_from: &SrcList,
    ai_cur_col: &mut [i32; 2],
    p_orig: &ExprRef,
    op: u8,
) -> i32 {
    // Se esta expressão é um vetor à esquerda ou à direita de uma restrição de desigualdade
    // (>, <, >= ou <=), faz o processamento no primeiro elemento do vetor.
    debug_assert!(TK_GT + 1 == TK_LE && TK_GT + 2 == TK_LT && TK_GT + 3 == TK_GE);
    debug_assert!(TK_IS < TK_GE && TK_ISNULL < TK_GE && TK_IN < TK_GE);
    debug_assert!(op <= TK_GE);
    let mut p_orig = p_orig.clone();
    if p_orig.borrow().op == TK_VECTOR && (op >= TK_GT && op <= TK_GE) {
        debug_assert!(expr_use_x_list(&p_orig.borrow()));
        p_orig = expr_list_item_ref(&p_orig, 0).expect("vetor vazio");
    }

    if p_orig.borrow().op == TK_COLUMN {
        ai_cur_col[0] = p_orig.borrow().i_table;
        ai_cur_col[1] = p_orig.borrow().i_column as i32;
        return 1;
    }

    for i in 0..p_from.n_src {
        let mut p_idx: Option<IndexRef> = p_from.a[i as usize]
            .p_tab
            .as_ref()
            .and_then(|t| t.borrow().p_index.clone());
        while let Some(idx_ref) = p_idx {
            if idx_ref.borrow().a_col_expr.is_some() {
                return expr_might_be_indexed_2(p_from, ai_cur_col, &p_orig.borrow(), i);
            }
            let next = idx_ref.borrow().p_next.clone();
            p_idx = next;
        }
    }
    0
}

/// A entrada desta rotina é uma estrutura WhereTerm com só o campo "pExpr" preenchido. O
/// trabalho da rotina é analisar a subexpressão e preencher todos os outros campos do
/// WhereTerm.
///
/// Se a expressão tem a forma "<expr> <op> X", ela é comutada para a forma padrão
/// "X <op> <expr>".
///
/// Se a expressão tem a forma "X <op> Y" com X e Y colunas, a expressão original fica
/// inalterada e um novo termo virtual "Y <op> X" é adicionado à cláusula WHERE e analisado em
/// separado. O termo original é marcado com TERM_COPIED e o novo com TERM_DYNAMIC (porque o
/// pExpr dele deve ser liberado com a WhereClause) e TERM_VIRTUAL (porque é uma cópia
/// comutada de um termo anterior). O termo original tem nChild=1 e a cópia tem idxParent
/// igual ao índice do original.
fn expr_analyze(p_src: &SrcList, p_wc: &WhereClauseRef, idx_term: i32) {
    let p_w_info = where_info_of(p_wc);
    let mut extra_right: Bitmask = 0;
    let mut p_str1: Option<ExprRef> = None;
    let mut is_complete: i32 = 0;
    let mut no_case: i32 = 0;
    let p_parse = p_w_info.borrow().p_parse.clone();
    let db = parse_db(&p_parse);
    let mut e_op2: u8 = 0;

    if db_malloc_failed(&db) {
        return;
    }
    debug_assert!(p_wc.borrow().n_term > idx_term);
    // O Rc do termo continua válido mesmo que a[] cresça: os "pTerm = &pWC->a[idxTerm]"
    // do C, que reobtêm o ponteiro após inserções, não precisam de equivalente.
    let p_term: WhereTermRef = p_wc.borrow().a[idx_term as usize].clone();
    let p_orig: ExprRef = p_term.borrow().p_expr.clone().expect("termo sem expressão");
    debug_assert!(p_orig.borrow().op != TK_AS && p_orig.borrow().op != TK_COLLATE);
    p_w_info.borrow_mut().s_mask_set.b_var_select = 0;
    let prereq_left: Bitmask = {
        let mut wi = p_w_info.borrow_mut();
        where_expr_usage(&mut wi.s_mask_set, p_orig.borrow().p_left.as_deref())
    };
    let op = p_orig.borrow().op;
    let mut prereq_all: Bitmask;
    if op == TK_IN {
        debug_assert!(p_orig.borrow().p_right.is_none());
        if expr_check_in(&p_parse, &p_orig.borrow()) != 0 {
            return;
        }
        let prereq_right = {
            let mut wi = p_w_info.borrow_mut();
            let e = p_orig.borrow();
            if expr_use_x_select(&e) {
                expr_select_usage(&mut wi.s_mask_set, e.x.p_select.as_deref())
            } else {
                where_expr_list_usage(&mut wi.s_mask_set, e.x.p_list.as_deref())
            }
        };
        p_term.borrow_mut().prereq_right = prereq_right;
        prereq_all = prereq_left | prereq_right;
    } else {
        let prereq_right = {
            let mut wi = p_w_info.borrow_mut();
            where_expr_usage(&mut wi.s_mask_set, p_orig.borrow().p_right.as_deref())
        };
        p_term.borrow_mut().prereq_right = prereq_right;
        let need_full = {
            let e = p_orig.borrow();
            e.p_left.is_none()
                || expr_has_property(&e, EP_X_IS_SELECT | EP_IF_NULL_ROW)
                || e.x.p_list.is_some()
        };
        if need_full {
            let mut wi = p_w_info.borrow_mut();
            prereq_all = where_expr_usage_nn(&mut wi.s_mask_set, &p_orig.borrow());
        } else {
            prereq_all = prereq_left | prereq_right;
        }
    }
    if p_w_info.borrow().s_mask_set.b_var_select != 0 {
        p_term.borrow_mut().wt_flags |= TERM_VARSELECT;
    }

    // O ramo SQLITE_DEBUG (printf de "Incorrect prereqAll") some.

    if expr_has_property(&p_orig.borrow(), EP_OUTER_ON | EP_INNER_ON) {
        let x: Bitmask = where_get_mask(&p_w_info.borrow().s_mask_set, p_orig.borrow().w.i_join);
        if expr_has_property(&p_orig.borrow(), EP_OUTER_ON) {
            prereq_all |= x;
            // Termos de ON não podem ser usados com índice na tabela à esquerda de um LEFT
            // JOIN. Ticket #3015
            extra_right = x.wrapping_sub(1);
            if (prereq_all >> 1) >= x {
                error_msg(
                    &mut p_parse.borrow_mut(),
                    b"ON clause references tables to its right",
                    &[],
                );
                return;
            }
        } else if (prereq_all >> 1) >= x {
            // O ON de um INNER JOIN referencia uma tabela à direita. A maioria dos outros
            // bancos gera erro. Mas o SQLite 3.0 a 3.38 colocava a restrição do ON na cláusula
            // WHERE e seguia em frente. A partir da 3.39, só gera erro se há RIGHT ou FULL
            // JOIN na consulta. Isso aproxima o SQLite de outros sistemas e preserva o legado.
            debug_assert!(p_src.n_src > 0);
            if p_src.n_src > 0 && (p_src.a[0].fg.jointype & JT_LTORJ) != 0 {
                error_msg(
                    &mut p_parse.borrow_mut(),
                    b"ON clause references tables to its right",
                    &[],
                );
                return;
            }
            expr_clear_property(&mut p_orig.borrow_mut(), EP_INNER_ON);
        }
    }
    {
        let mut t = p_term.borrow_mut();
        t.prereq_all = prereq_all;
        t.left_cursor = -1;
        t.i_parent = -1;
        t.e_operator = 0;
    }
    if allowed_op(op) {
        let mut ai_cur_col: [i32; 2] = [0; 2];
        let mut p_left: ExprRef = expr_skip_collate(expr_left(&p_orig)).expect("comparação sem lado esquerdo");
        let p_right: Option<ExprRef> = expr_skip_collate(expr_right(&p_orig));
        let op_mask: u16 = if (p_term.borrow().prereq_right & prereq_left) == 0 {
            WO_ALL
        } else {
            WO_EQUIV
        };

        let i_field_term = match &p_term.borrow().u {
            WhereTermUnion::X { i_field, .. } => *i_field,
            _ => 0,
        };
        if i_field_term > 0 {
            debug_assert!(op == TK_IN);
            debug_assert!(p_left.borrow().op == TK_VECTOR);
            debug_assert!(expr_use_x_list(&p_left.borrow()));
            p_left = expr_list_item_ref(&p_left, (i_field_term - 1) as usize)
                .expect("campo de vetor ausente");
        }

        if expr_might_be_indexed(p_src, &mut ai_cur_col, &p_left, op) != 0 {
            let mut t = p_term.borrow_mut();
            t.left_cursor = ai_cur_col[0];
            debug_assert!((t.e_operator & (WO_OR | WO_AND)) == 0);
            if let WhereTermUnion::X { left_column, .. } = &mut t.u {
                *left_column = ai_cur_col[1];
            }
            t.e_operator = operator_mask(op) & op_mask;
        }
        if op == TK_IS {
            p_term.borrow_mut().wt_flags |= TERM_IS;
        }
        let right_indexable = match &p_right {
            Some(r) => {
                expr_might_be_indexed(p_src, &mut ai_cur_col, r, op) != 0
                    && !expr_has_property(&r.borrow(), EP_FIXED_COL)
            }
            None => false,
        };
        if right_indexable {
            let p_new: WhereTermRef;
            let p_dup: ExprRef;
            let mut e_extra_op: u16 = 0; // Bits extras para pNew->eOperator
            debug_assert!(matches!(&p_term.borrow().u, WhereTermUnion::X { i_field: 0, .. }));
            if p_term.borrow().left_cursor >= 0 {
                let dup = expr_dup_ref(&db, &p_orig);
                if db_malloc_failed(&db) {
                    // sqlite3ExprDelete(db, pDup): o Box é liberado ao sair de escopo.
                    drop(dup);
                    return;
                }
                let dup_ref = expr_ref_new(dup).expect("duplicata ausente");
                let idx_new =
                    where_clause_insert(p_wc, Some(dup_ref.clone()), TERM_VIRTUAL | TERM_DYNAMIC);
                if idx_new == 0 {
                    return;
                }
                p_new = p_wc.borrow().a[idx_new as usize].clone();
                mark_term_as_child(p_wc, idx_new, idx_term);
                if op == TK_IS {
                    p_new.borrow_mut().wt_flags |= TERM_IS;
                }
                p_term.borrow_mut().wt_flags |= TERM_COPIED;

                if term_is_equivalence(&p_parse, &dup_ref.borrow()) != 0 {
                    p_term.borrow_mut().e_operator |= WO_EQUIV;
                    e_extra_op = WO_EQUIV;
                }
                p_dup = dup_ref;
            } else {
                p_dup = p_orig.clone();
                p_new = p_term.clone();
            }
            let commuted = expr_commute(&p_parse, &mut p_dup.borrow_mut());
            let mut n = p_new.borrow_mut();
            n.wt_flags |= commuted;
            n.left_cursor = ai_cur_col[0];
            debug_assert!((p_term.try_borrow().map_or(true, |t| (t.e_operator & (WO_OR | WO_AND)) == 0)));
            if let WhereTermUnion::X { left_column, .. } = &mut n.u {
                *left_column = ai_cur_col[1];
            }
            n.prereq_right = prereq_left | extra_right;
            n.prereq_all = prereq_all;
            n.e_operator = (operator_mask(p_dup.borrow().op).wrapping_add(e_extra_op)) & op_mask;
        } else if op == TK_ISNULL
            && !expr_has_property(&p_orig.borrow(), EP_OUTER_ON)
            && 0 == expr_can_be_null(&p_left.borrow())
        {
            debug_assert!(!expr_has_property(&p_orig.borrow(), EP_INT_VALUE));
            {
                let mut e = p_orig.borrow_mut();
                e.op = TK_TRUEFALSE; // Ver tag-20230504-1
                e.u.z_token = Some(b"false".to_vec());
                expr_set_property(&mut e, EP_IS_FALSE);
            }
            let mut t = p_term.borrow_mut();
            t.prereq_all = 0;
            t.e_operator = 0;
        }
    } else if p_orig.borrow().op == TK_BETWEEN && p_wc.borrow().op == TK_AND {
        // Se um termo é o operador BETWEEN, cria dois novos termos virtuais que definem a
        // faixa que o BETWEEN implementa. Por exemplo:
        //
        //      a BETWEEN b AND c
        //
        // é convertido em:
        //
        //      (a BETWEEN b AND c) AND (a>=b) AND (a<=c)
        //
        // Os dois novos termos são adicionados ao fim da WhereClause. São "dinâmicos" e filhos
        // do termo BETWEEN original. Ou seja, se o termo BETWEEN é codificado, os filhos são
        // pulados. Ou, se os filhos são satisfeitos por um índice, o BETWEEN original é
        // pulado.
        const OPS: [u8; 2] = [TK_GE, TK_LE];
        debug_assert!(expr_use_x_list(&p_orig.borrow()));
        debug_assert!(p_orig.borrow().x.p_list.is_some());
        debug_assert!(p_orig.borrow().x.p_list.as_ref().unwrap().n_expr == 2);
        for i in 0..2usize {
            let p_new_expr = {
                let e = p_orig.borrow();
                let dup_left = expr_dup(&db.borrow(), e.p_left.as_deref().unwrap(), 0);
                let dup_item = expr_dup(
                    &db.borrow(),
                    e.x.p_list.as_ref().unwrap().a[i].p_expr.as_deref().unwrap(),
                    0,
                );
                p_expr(&mut p_parse.borrow_mut(), OPS[i] as i32, dup_left, dup_item)
            };
            let p_new_expr = expr_ref_new(p_new_expr);
            transfer_join_markings(p_new_expr.as_ref(), &p_orig);
            let idx_new = where_clause_insert(p_wc, p_new_expr, TERM_VIRTUAL | TERM_DYNAMIC);
            expr_analyze(p_src, p_wc, idx_new);
            mark_term_as_child(p_wc, idx_new, idx_term);
        }
    } else if p_orig.borrow().op == TK_OR {
        // Analisa um termo composto por dois ou mais subtermos ligados por OR.
        debug_assert!(p_wc.borrow().op == TK_AND);
        expr_analyze_or_term(p_src, p_wc, idx_term);
    } else if p_orig.borrow().op == TK_NOTNULL {
        // A forma "x IS NOT NULL" às vezes é avaliada com mais eficiência como "x>NULL" se x
        // não é um INTEGER PRIMARY KEY. Então constrói um termo virtual dessa forma.
        //
        // O termo virtual deve ser marcado com TERM_VNULL.
        let p_left_ref = expr_left(&p_orig).expect("NOTNULL sem operando");
        let cond = {
            let l = p_left_ref.borrow();
            l.op == TK_COLUMN
                && l.i_column >= 0
                && !expr_has_property(&p_orig.borrow(), EP_OUTER_ON)
        };
        if cond {
            let p_new_expr = {
                let dup = expr_dup_ref(&db, &p_left_ref);
                let null_expr = expr_alloc(&db.borrow(), TK_NULL as i32, None, false);
                p_expr(&mut p_parse.borrow_mut(), TK_GT as i32, dup, null_expr)
            };
            let idx_new = where_clause_insert(
                p_wc,
                expr_ref_new(p_new_expr),
                TERM_VIRTUAL | TERM_DYNAMIC | TERM_VNULL,
            );
            if idx_new != 0 {
                let p_new_term = p_wc.borrow().a[idx_new as usize].clone();
                {
                    let mut n = p_new_term.borrow_mut();
                    n.prereq_right = 0;
                    n.left_cursor = p_left_ref.borrow().i_table;
                    if let WhereTermUnion::X { left_column, .. } = &mut n.u {
                        *left_column = p_left_ref.borrow().i_column as i32;
                    }
                    n.e_operator = WO_GT;
                }
                mark_term_as_child(p_wc, idx_new, idx_term);
                p_term.borrow_mut().wt_flags |= TERM_COPIED;
                p_new_term.borrow_mut().prereq_all = p_term.borrow().prereq_all;
            }
        }
    } else if p_orig.borrow().op == TK_FUNCTION
        && p_wc.borrow().op == TK_AND
        && is_like_or_glob(
            &p_parse,
            &p_orig.borrow(),
            &mut p_str1,
            &mut is_complete,
            &mut no_case,
        ) != 0
    {
        // Adiciona restrições para reduzir o espaço de busca num operador LIKE ou GLOB.
        //
        // Um padrão LIKE da forma "x LIKE 'aBc%'" vira as restrições
        //
        //          x>='ABC' AND x<'abd' AND x LIKE 'aBc%'
        //
        // O último caractere do prefixo "abc" é incrementado para formar a condição de
        // término "abd". Se a caixa não é significativa (o padrão do LIKE), o limite inferior
        // é todo em maiúsculas e o superior todo em minúsculas, para que os limites também
        // funcionem ao comparar BLOBs.
        let wt_flags: u16 = TERM_LIKEOPT | TERM_VIRTUAL | TERM_DYNAMIC;

        debug_assert!(expr_use_x_list(&p_orig.borrow()));
        let p_left = expr_list_item_ref(&p_orig, 1).expect("LIKE sem lado esquerdo");
        let p_str2: Option<ExprRef> = match &p_str1 {
            Some(s) => expr_ref_new(expr_dup_ref(&db, s)),
            None => None,
        };
        debug_assert!(p_str1.as_ref().map_or(true, |s| !expr_has_property(&s.borrow(), EP_INT_VALUE)));
        debug_assert!(p_str2.as_ref().map_or(true, |s| !expr_has_property(&s.borrow(), EP_INT_VALUE)));

        // Converte o limite inferior para maiúsculas e o superior para minúsculas
        // (maiúsculas vêm antes de minúsculas em ASCII) para que as restrições de faixa
        // também funcionem com BLOBs.
        if no_case != 0 && !db_malloc_failed(&parse_db(&p_parse)) {
            p_term.borrow_mut().wt_flags |= TERM_LIKE;
            let s1 = p_str1.as_ref().expect("pStr1 ausente");
            let s2 = p_str2.as_ref().expect("pStr2 ausente");
            let n = s1.borrow().u.z_token.as_ref().map_or(0, |t| {
                t.iter().position(|&b| b == 0).unwrap_or(t.len())
            });
            for i in 0..n {
                let c = s1.borrow().u.z_token.as_ref().unwrap()[i];
                s1.borrow_mut().u.z_token.as_mut().unwrap()[i] = toupper(c);
                s2.borrow_mut().u.z_token.as_mut().unwrap()[i] = tolower(c);
            }
        }

        if !db_malloc_failed(&db) {
            // c é o último caractere antes do primeiro coringa.
            let s2 = p_str2.as_ref().expect("pStr2 ausente");
            let mut s2b = s2.borrow_mut();
            let tok = s2b.u.z_token.as_mut().expect("pStr2 sem token");
            let len = tok.iter().position(|&b| b == 0).unwrap_or(tok.len());
            let p_c = len - 1;
            let mut c = tok[p_c];
            if no_case != 0 {
                // A ideia é incrementar o último caractere antes do primeiro coringa. Mas se
                // incrementarmos '@', ele cai na faixa alfabética em que as conversões de
                // caixa estragam a desigualdade. Para evitar isso, também roda o LIKE
                // completo em todas as expressões candidatas, zerando is_complete.
                if c == b'A' - 1 {
                    is_complete = 0;
                }
                c = UPPER_TO_LOWER[c as usize];
            }
            tok[p_c] = c.wrapping_add(1);
        }
        let z_coll_seq_name: &[u8] = if no_case != 0 { b"NOCASE" } else { STR_BINARY };
        // pStr1 e pStr2 passam a pertencer às novas expressões (sqlite3PExpr assume a posse).
        let coll1 = expr_dup_ref(&db, &p_left)
            .map(|e| expr_add_collate_string(&p_parse.borrow(), e, z_coll_seq_name));
        let rhs1 = p_str1.take().map(expr_ref_into_box);
        let p_new_expr1 = expr_ref_new(p_expr(&mut p_parse.borrow_mut(), TK_GE as i32, coll1, rhs1));
        transfer_join_markings(p_new_expr1.as_ref(), &p_orig);
        let idx_new1 = where_clause_insert(p_wc, p_new_expr1, wt_flags);
        let coll2 = expr_dup_ref(&db, &p_left)
            .map(|e| expr_add_collate_string(&p_parse.borrow(), e, z_coll_seq_name));
        let rhs2 = p_str2.map(expr_ref_into_box);
        let p_new_expr2 = expr_ref_new(p_expr(&mut p_parse.borrow_mut(), TK_LT as i32, coll2, rhs2));
        transfer_join_markings(p_new_expr2.as_ref(), &p_orig);
        let idx_new2 = where_clause_insert(p_wc, p_new_expr2, wt_flags);
        expr_analyze(p_src, p_wc, idx_new1);
        expr_analyze(p_src, p_wc, idx_new2);
        if is_complete != 0 {
            mark_term_as_child(p_wc, idx_new1, idx_term);
            mark_term_as_child(p_wc, idx_new2, idx_term);
        }
    }

    // Se há um termo vetorial == ou IS, por exemplo "(a, b) == (?, ?)", cria novos termos para
    // cada comparação de componente, "a = ?" e "b = ?". Os novos termos substituem por
    // completo a comparação vetorial original, que deixa de ser usada.
    //
    // Isto só é necessário se pelo menos um lado da comparação não é uma subconsulta.
    //
    // tag-20220128a
    let (vec_op, n_left) = {
        let e = p_orig.borrow();
        (
            e.op,
            e.p_left.as_deref().map_or(0, |l| expr_vector_size(l)),
        )
    };
    let vector_eq = (vec_op == TK_EQ || vec_op == TK_IS) && n_left > 1 && {
        let e = p_orig.borrow();
        let r_size = e.p_right.as_deref().map_or(0, |r| expr_vector_size(r));
        r_size == n_left
            && ((e.p_left.as_ref().unwrap().flags & EP_X_IS_SELECT) == 0
                || (e.p_right.as_ref().unwrap().flags & EP_X_IS_SELECT) == 0)
            && p_wc.borrow().op == TK_AND
    };
    if vector_eq {
        for i in 0..n_left {
            let l_ref = expr_left(&p_orig).expect("vetor sem lado esquerdo");
            let r_ref = expr_right(&p_orig).expect("vetor sem lado direito");
            let p_left = expr_for_vector_field(&mut p_parse.borrow_mut(), &mut l_ref.borrow_mut(), i, n_left);
            let p_right = expr_for_vector_field(&mut p_parse.borrow_mut(), &mut r_ref.borrow_mut(), i, n_left);

            let p_new = p_expr(&mut p_parse.borrow_mut(), vec_op as i32, p_left, p_right);
            let p_new = expr_ref_new(p_new);
            transfer_join_markings(p_new.as_ref(), &p_orig);
            let idx_new = where_clause_insert(p_wc, p_new, TERM_DYNAMIC | TERM_SLICE);
            expr_analyze(p_src, p_wc, idx_new);
        }
        let mut t = p_term.borrow_mut();
        t.wt_flags |= TERM_CODED | TERM_VIRTUAL; // Desabilita o original
        t.e_operator = WO_ROWVAL;
    } else if {
        // Se há um termo IN vetorial, por exemplo "(a, b) IN (SELECT ...)", cria um termo
        // virtual para cada componente do vetor. A expressão usada por cada um desses termos
        // virtuais é p_orig (a expressão vetorial IN(...) inteira). WhereTerm.u.x.iField
        // identifica o índice, no vetor da esquerda, que o termo virtual representa.
        //
        // Só funciona se o lado direito é um SELECT simples (não composto) que não usa
        // funções de janela.
        let e = p_orig.borrow();
        let i_field0 = matches!(&p_term.borrow().u, WhereTermUnion::X { i_field: 0, .. });
        e.op == TK_IN
            && i_field0
            && e.p_left.as_ref().unwrap().op == TK_VECTOR
            && expr_use_x_select(&e)
            && e.x.p_select.as_ref().map_or(false, |s| {
                (s.p_prior.is_none() || (s.sel_flags & SF_VALUES) != 0) && s.p_win.is_none()
            })
            && p_wc.borrow().op == TK_AND
    } {
        let n_vec = expr_vector_size(p_orig.borrow().p_left.as_deref().unwrap());
        for i in 0..n_vec {
            let idx_new = where_clause_insert(p_wc, Some(p_orig.clone()), TERM_VIRTUAL | TERM_SLICE);
            if let WhereTermUnion::X { i_field, .. } = &mut p_wc.borrow().a[idx_new as usize].borrow_mut().u {
                *i_field = i + 1;
            }
            expr_analyze(p_src, p_wc, idx_new);
            mark_term_as_child(p_wc, idx_new, idx_term);
        }
    } else if p_wc.borrow().op == TK_AND {
        // Adiciona um termo auxiliar WO_AUX ao conjunto de restrições se a expressão atual
        // tem a forma "column OP expr" em que OP é um operador repassado às tabelas virtuais
        // mas que normalmente não é otimizado em tabelas comuns. Ou seja, OP é um de MATCH,
        // LIKE, GLOB, REGEXP, !=, IS, IS NOT ou NOT NULL. A informação é usada pelos métodos
        // xBestIndex das tabelas virtuais. O otimizador nativo não tenta fazer nada com
        // funções MATCH.
        let mut p_right: Option<ExprRef> = None;
        let mut p_left: Option<ExprRef> = None;
        let mut res = is_auxiliary_vtab_operator(&db, &p_orig, &mut e_op2, &mut p_left, &mut p_right);
        while res > 0 {
            res -= 1;
            let prereq_expr: Bitmask = {
                let mut wi = p_w_info.borrow_mut();
                where_expr_usage(&mut wi.s_mask_set, p_right.as_ref().map(|r| r.borrow()).as_deref())
            };
            let prereq_column: Bitmask = {
                let mut wi = p_w_info.borrow_mut();
                where_expr_usage(&mut wi.s_mask_set, p_left.as_ref().map(|r| r.borrow()).as_deref())
            };
            if (prereq_expr & prereq_column) == 0 {
                let rhs = p_right.as_ref().and_then(|r| expr_dup_ref(&db, r));
                let p_new_expr = expr_ref_new(p_expr(&mut p_parse.borrow_mut(), TK_MATCH as i32, None, rhs));
                if expr_has_property(&p_orig.borrow(), EP_OUTER_ON) {
                    if let Some(n) = &p_new_expr {
                        expr_set_property(&mut n.borrow_mut(), EP_OUTER_ON);
                        n.borrow_mut().w.i_join = p_orig.borrow().w.i_join;
                    }
                }
                let idx_new = where_clause_insert(p_wc, p_new_expr, TERM_VIRTUAL | TERM_DYNAMIC);
                let p_new_term = p_wc.borrow().a[idx_new as usize].clone();
                {
                    let l = p_left.as_ref().expect("pLeft ausente").borrow();
                    let mut n = p_new_term.borrow_mut();
                    n.prereq_right = prereq_expr;
                    n.left_cursor = l.i_table;
                    if let WhereTermUnion::X { left_column, .. } = &mut n.u {
                        *left_column = l.i_column as i32;
                    }
                    n.e_operator = WO_AUX;
                    n.e_match_op = e_op2;
                }
                mark_term_as_child(p_wc, idx_new, idx_term);
                p_term.borrow_mut().wt_flags |= TERM_COPIED;
                p_new_term.borrow_mut().prereq_all = p_term.borrow().prereq_all;
            }
            std::mem::swap(&mut p_left, &mut p_right);
        }
    }

    // Impede que termos ON de um LEFT JOIN sejam usados para dirigir um índice de tabelas à
    // esquerda do join.
    p_term.borrow_mut().prereq_right |= extra_right;
}


// ---- part_003.rs ----

/// Auxiliar: uma WhereClause vazia, pronta para `where_clause_init` (no C é um objeto
/// preexistente, zerado pelo chamador; aqui é preciso construí-lo).
fn where_clause_new() -> WhereClauseRef {
    Rc::new(RefCell::new(WhereClause {
        p_w_info: Weak::new(),
        p_outer: None,
        op: 0,
        has_or: 0,
        n_term: 0,
        n_slot: 0,
        n_base: 0,
        a: Vec::new(),
    }))
}

/// Esta rotina identifica subexpressões na cláusula WHERE em que cada subexpressão é
/// separada pelo operador AND ou por outro operador dado no parâmetro op. A estrutura
/// WhereClause é preenchida com referências às subexpressões. Por exemplo:
///
///    WHERE  a=='hello' AND coalesce(b,11)<10 AND (c+12!=d OR c==22)
///           \________/     \_______________/     \________________/
///            slot[0]            slot[1]               slot[2]
///
/// A cláusula WHERE original em p_expr não é alterada. Tudo o que esta rotina faz é fazer as
/// entradas slot[] apontarem para subestruturas dentro de p_expr.
///
/// Na frase anterior e no diagrama, "slot[]" é o array WhereClause.a[]. O array slot[] cresce
/// conforme necessário para conter todos os termos da cláusula WHERE.
pub fn where_split(p_wc: &WhereClauseRef, p_expr_arg: Option<ExprRef>, op: u8) {
    let p_e2 = expr_skip_collate_and_likely(p_expr_arg.clone());
    p_wc.borrow_mut().op = op;
    debug_assert!(p_e2.is_some() || p_expr_arg.is_none());
    let p_e2 = match p_e2 {
        Some(e) => e,
        None => return,
    };
    if p_e2.borrow().op != op {
        where_clause_insert(p_wc, p_expr_arg, 0);
    } else {
        where_split(p_wc, expr_left(&p_e2), op);
        where_split(p_wc, expr_right(&p_e2), op);
    }
}

/// Adiciona um termo LIMIT (se e_match_op==SQLITE_INDEX_CONSTRAINT_LIMIT) ou OFFSET (se
/// e_match_op==SQLITE_INDEX_CONSTRAINT_OFFSET) à cláusula where passada como primeiro
/// argumento. O valor do termo está no registro i_reg.
///
/// No caso comum em que o valor é um inteiro simples (exemplo: "LIMIT 5 OFFSET 10") a
/// expressão é codificada como TK_INTEGER, para ficar disponível a sqlite3_vtab_rhs_value().
/// Se não, é codificada como expressão TK_REGISTER.
fn where_add_limit_expr(
    p_wc: &WhereClauseRef,
    i_reg: i32,
    p_limit_expr: Option<&Expr>,
    i_csr: i32,
    e_match_op: i32,
) {
    let p_parse = where_parse_of(p_wc);
    let db = parse_db(&p_parse);
    let mut i_val: i32 = 0;

    let p_new: Option<Box<Expr>>;
    if p_limit_expr.map_or(false, |e| expr_is_integer(e, &mut i_val) != 0) && i_val >= 0 {
        let mut p_val = match expr(&db.borrow(), TK_INTEGER as i32, None) {
            Some(v) => v,
            None => return,
        };
        expr_set_property(&mut p_val, EP_INT_VALUE);
        p_val.u.i_value = i_val;
        p_new = p_expr(&mut p_parse.borrow_mut(), TK_MATCH as i32, None, Some(p_val));
    } else {
        let mut p_val = match expr(&db.borrow(), TK_REGISTER as i32, None) {
            Some(v) => v,
            None => return,
        };
        p_val.i_table = i_reg;
        p_new = p_expr(&mut p_parse.borrow_mut(), TK_MATCH as i32, None, Some(p_val));
    }
    if let Some(new_box) = p_new {
        let idx = where_clause_insert(
            p_wc,
            expr_ref_new(Some(new_box)),
            TERM_DYNAMIC | TERM_VIRTUAL,
        );
        let p_term = p_wc.borrow().a[idx as usize].clone();
        let mut t = p_term.borrow_mut();
        t.left_cursor = i_csr;
        t.e_operator = WO_AUX;
        t.e_match_op = e_match_op as u8;
    }
}

/// Possivelmente adiciona termos correspondentes às cláusulas LIMIT e OFFSET do comando
/// SELECT passado como segundo argumento. Esses termos só são adicionados se:
///
///   1. O SELECT tem uma cláusula LIMIT, e
///   2. O SELECT não é uma consulta agregada nem DISTINCT, e
///   3. O SELECT tem exatamente um objeto na cláusula FROM, e esse objeto é uma tabela
///      virtual, e
///   4. Não há termos na cláusula WHERE que não serão passados ao método xBestIndex da tabela
///      virtual.
///   5. A cláusula ORDER BY, se houver, será disponibilizada ao método xBestIndex.
///
/// Termos LIMIT e OFFSET são ignorados pela maior parte do código do planejador. Existem só
/// para serem passados ao método xBestIndex da única tabela virtual na cláusula FROM do
/// SELECT.
pub fn where_add_limit(p_wc: &WhereClauseRef, p: &Select) {
    debug_assert!(p.p_limit.is_some()); // 1 -- verificado pelo chamador
    let p_src = p.p_src.as_deref().expect("SELECT sem FROM");
    if p.p_group_by.is_none()
        && (p.sel_flags & (SF_DISTINCT | SF_AGGREGATE)) == 0 // 2
        && (p_src.n_src == 1
            && p_src.a[0]
                .p_tab
                .as_ref()
                .map_or(false, |t| is_virtual(&t.borrow()))) // 3
    {
        let p_order_by = p.p_order_by.as_deref();
        let i_csr = p_src.a[0].i_cursor;

        // Verifica a condição (4). Retorna cedo se não for atendida.
        let n_term = p_wc.borrow().n_term;
        for ii in 0..n_term {
            let t = p_wc.borrow().a[ii as usize].clone();
            let t = t.borrow();
            if (t.wt_flags & TERM_CODED) != 0 {
                // Este termo é uma operação vetorial que foi decomposta em outros termos
                // posteriores. Pode ser ignorado. Ver tag-20220128a
                debug_assert!((t.wt_flags & TERM_VIRTUAL) != 0);
                debug_assert!(t.e_operator == WO_ROWVAL);
                continue;
            }
            if t.n_child != 0 {
                // Se este termo tem filhos, eles também estão no array pWC->a[]. Então este
                // termo pode ser ignorado, pois o LIMIT só é adicionado se cada termo filho
                // passar no teste (leftCursor==iCsr) abaixo.
                continue;
            }
            if t.left_cursor != i_csr {
                return;
            }
            if t.prereq_right != 0 {
                return;
            }
        }

        // Verifica a condição (5). Retorna cedo se não for atendida.
        if let Some(order_by) = p_order_by {
            for ii in 0..order_by.n_expr as usize {
                let p_e = order_by.a[ii].p_expr.as_deref().expect("ORDER BY sem expressão");
                if p_e.op != TK_COLUMN {
                    return;
                }
                if p_e.i_table != i_csr {
                    return;
                }
                if (order_by.a[ii].fg.sort_flags & KEYINFO_ORDER_BIGNULL) != 0 {
                    return;
                }
            }
        }

        // Todas as condições foram atendidas. Adiciona os termos ao objeto where-clause.
        let p_limit = p.p_limit.as_deref().expect("SELECT sem LIMIT");
        debug_assert!(p_limit.op == TK_LIMIT);
        if p.i_offset != 0 && (p.sel_flags & SF_COMPOUND) == 0 {
            where_add_limit_expr(
                p_wc,
                p.i_offset,
                p_limit.p_right.as_deref(),
                i_csr,
                SQLITE_INDEX_CONSTRAINT_OFFSET,
            );
        }
        if p.i_offset == 0 || (p.sel_flags & SF_COMPOUND) == 0 {
            where_add_limit_expr(
                p_wc,
                p.i_limit,
                p_limit.p_left.as_deref(),
                i_csr,
                SQLITE_INDEX_CONSTRAINT_LIMIT,
            );
        }
    }
}

/// Inicializa uma estrutura WhereClause pré-alocada.
pub fn where_clause_init(p_wc: &WhereClauseRef, p_w_info: &WhereInfoRef) {
    let mut wc = p_wc.borrow_mut();
    wc.p_w_info = Rc::downgrade(p_w_info);
    wc.has_or = 0;
    wc.p_outer = None;
    wc.n_term = 0;
    wc.n_base = 0;
    // pWC->nSlot = ArraySize(pWC->aStatic): aStatic tem 8 entradas.
    wc.n_slot = 8;
    wc.a = Vec::with_capacity(8);
}

/// Desaloca uma estrutura WhereClause. A estrutura WhereClause em si não é liberada. Esta
/// rotina é o inverso de where_clause_init().
pub fn where_clause_clear(p_wc: &WhereClauseRef) {
    let db = parse_db(&where_parse_of(p_wc));
    debug_assert!(p_wc.borrow().n_term >= p_wc.borrow().n_base);
    let n_term = p_wc.borrow().n_term;
    if n_term > 0 {
        // Verifica que todo termo depois de pWC->nBase é virtual (SQLITE_DEBUG).
        #[cfg(debug_assertions)]
        {
            let wc = p_wc.borrow();
            for i in wc.n_base..wc.n_term {
                debug_assert!((wc.a[i as usize].borrow().wt_flags & TERM_VIRTUAL) != 0);
            }
        }
        for i in 0..n_term as usize {
            let a = p_wc.borrow().a[i].clone();
            let mut a = a.borrow_mut();
            debug_assert!(a.e_match_op == 0 || a.e_operator == WO_AUX);
            if (a.wt_flags & TERM_DYNAMIC) != 0 {
                // sqlite3ExprDelete(db, a->pExpr)
                a.p_expr = None;
            }
            if (a.wt_flags & (TERM_ORINFO | TERM_ANDINFO)) != 0 {
                let u = std::mem::replace(
                    &mut a.u,
                    WhereTermUnion::X {
                        left_column: 0,
                        i_field: 0,
                    },
                );
                if (a.wt_flags & TERM_ORINFO) != 0 {
                    debug_assert!((a.wt_flags & TERM_ANDINFO) == 0);
                    if let WhereTermUnion::OrInfo(Some(p_or_info)) = u {
                        where_or_info_delete(&db, p_or_info);
                    }
                } else {
                    debug_assert!((a.wt_flags & TERM_ANDINFO) != 0);
                    if let WhereTermUnion::AndInfo(Some(p_and_info)) = u {
                        where_and_info_delete(&db, p_and_info);
                    }
                }
            }
        }
    }
}

/// Estas rotinas percorrem (recursivamente) uma árvore de expressões e geram uma máscara de
/// bits que indica quais tabelas são usadas na árvore.
///
/// where_expr_usage(MaskSet, Expr) ->
///
///       Retorna um Bitmask de todas as tabelas referenciadas por Expr. Expr pode ser None,
///       caso em que 0 é retornado.
///
/// where_expr_usage_nn(MaskSet, Expr) ->
///
///       Igual a where_expr_usage(), exceto que Expr não pode ser None. O sufixo "nn" quer
///       dizer "not null".
///
/// where_expr_list_usage(MaskSet, ExprList) ->
///
///       Retorna um Bitmask de todas as tabelas referenciadas por toda expressão da lista
///       ExprList. ExprList pode ser None, caso em que 0 é retornado.
///
/// where_expr_usage_full(MaskSet, ExprList) ->
///
///       Uso interno. Chamada só por where_expr_usage_nn() para expressões complexas que
///       exigem empilhar valores de registro. Muitas chamadas a where_expr_usage_nn() não
///       precisam da análise mais complexa feita por esta rotina. Por isso as contas dela
///       ficam numa função "no-inline" separada, para evitar o custo de empilhar no caso
///       comum em que não é necessário.
#[inline(never)]
fn where_expr_usage_full(p_mask_set: &mut WhereMaskSet, p: &Expr) -> Bitmask {
    let mut mask: Bitmask = if p.op == TK_IF_NULL_ROW {
        where_get_mask(p_mask_set, p.i_table)
    } else {
        0
    };
    if let Some(l) = p.p_left.as_deref() {
        mask |= where_expr_usage_nn(p_mask_set, l);
    }
    if let Some(r) = p.p_right.as_deref() {
        mask |= where_expr_usage_nn(p_mask_set, r);
        debug_assert!(p.x.p_list.is_none());
    } else if expr_use_x_select(p) {
        if expr_has_property(p, EP_VAR_SELECT) {
            p_mask_set.b_var_select = 1;
        }
        mask |= expr_select_usage(p_mask_set, p.x.p_select.as_deref());
    } else if p.x.p_list.is_some() {
        mask |= where_expr_list_usage(p_mask_set, p.x.p_list.as_deref());
    }
    // SQLITE_OMIT_WINDOWFUNC não é definido no Debian: o ramo de janela fica.
    if (p.op == TK_FUNCTION || p.op == TK_AGG_FUNCTION) && expr_use_y_win(p) {
        let p_win = p.y.p_win.as_ref().expect("função de janela sem Window");
        let w = p_win.borrow();
        mask |= where_expr_list_usage(p_mask_set, w.p_partition.as_deref());
        mask |= where_expr_list_usage(p_mask_set, w.p_order_by.as_deref());
        mask |= where_expr_usage(p_mask_set, w.p_filter.as_deref());
    }
    mask
}

pub fn where_expr_usage_nn(p_mask_set: &mut WhereMaskSet, p: &Expr) -> Bitmask {
    if p.op == TK_COLUMN && !expr_has_property(p, EP_FIXED_COL) {
        where_get_mask(p_mask_set, p.i_table)
    } else if expr_has_property(p, EP_TOKEN_ONLY | EP_LEAF) {
        debug_assert!(p.op != TK_IF_NULL_ROW);
        0
    } else {
        where_expr_usage_full(p_mask_set, p)
    }
}

pub fn where_expr_usage(p_mask_set: &mut WhereMaskSet, p: Option<&Expr>) -> Bitmask {
    match p {
        Some(e) => where_expr_usage_nn(p_mask_set, e),
        None => 0,
    }
}

pub fn where_expr_list_usage(p_mask_set: &mut WhereMaskSet, p_list: Option<&ExprList>) -> Bitmask {
    let mut mask: Bitmask = 0;
    if let Some(list) = p_list {
        for i in 0..list.n_expr as usize {
            mask |= where_expr_usage(p_mask_set, list.a[i].p_expr.as_deref());
        }
    }
    mask
}

/// Chama expr_analyze em todos os termos de uma cláusula WHERE.
///
/// Note que expr_analyze() pode adicionar novos termos virtuais ao fim da cláusula WHERE.
/// Não queremos analisar esses novos termos virtuais, então a análise começa pelo fim e
/// avança para o começo, de modo que os termos virtuais adicionados nunca sejam processados.
pub fn where_expr_analyze(p_tab_list: &SrcList, p_wc: &WhereClauseRef) {
    let n_term = p_wc.borrow().n_term;
    let mut i = n_term - 1;
    while i >= 0 {
        expr_analyze(p_tab_list, p_wc, i);
        i -= 1;
    }
}

/// Para funções com valor de tabela, transforma os argumentos da função em novos termos da
/// cláusula WHERE.
///
/// Cada argumento da função se traduz numa restrição de igualdade contra uma coluna HIDDEN
/// da tabela.
pub fn where_tab_func_args(p_parse: &ParseRef, p_item: &mut SrcItem, p_wc: &WhereClauseRef) {
    if p_item.fg.is_tab_func == 0 {
        return;
    }
    let p_tab = p_item.p_tab.clone().expect("função de tabela sem Table");
    let n_expr = match &p_item.u1 {
        SrcItemU1::FuncArg(p_args) => p_args.n_expr,
        _ => return,
    };
    let db = parse_db(p_parse);
    let mut k: i32 = 0;
    for j in 0..n_expr {
        while k < p_tab.borrow().n_col as i32
            && (p_tab.borrow().a_col[k as usize].col_flags & COLFLAG_HIDDEN) == 0
        {
            k += 1;
        }
        if k >= p_tab.borrow().n_col as i32 {
            error_msg(
                &mut p_parse.borrow_mut(),
                b"too many arguments on %s() - max %d",
                &[
                    PrintfArg::Text(p_tab.borrow().z_name.clone()),
                    PrintfArg::Int(j as i64),
                ],
            );
            return;
        }
        let mut p_col_ref = match expr_alloc(&db.borrow(), TK_COLUMN as i32, None, false) {
            Some(c) => c,
            None => return,
        };
        p_col_ref.i_table = p_item.i_cursor;
        p_col_ref.i_column = k as _;
        k += 1;
        debug_assert!(expr_use_y_tab(&p_col_ref));
        p_col_ref.y.p_tab = Some(p_tab.clone());
        p_item.col_used |= expr_col_used(&p_col_ref);
        let p_arg_dup = match &p_item.u1 {
            SrcItemU1::FuncArg(p_args) => expr_dup(
                &db.borrow(),
                p_args.a[j as usize].p_expr.as_deref().expect("argumento ausente"),
                0,
            ),
            _ => None,
        };
        let p_rhs = p_expr(&mut p_parse.borrow_mut(), TK_UPLUS as i32, p_arg_dup, None);
        let mut p_term = p_expr(&mut p_parse.borrow_mut(), TK_EQ as i32, Some(p_col_ref), p_rhs);
        let join_type: u32;
        if (p_item.fg.jointype & (JT_LEFT | JT_RIGHT)) != 0 {
            // testtag-20230227a e testtag-20230227b
            join_type = EP_OUTER_ON;
        } else {
            // testtag-20230227c
            join_type = EP_INNER_ON;
        }
        set_join_expr(p_term.as_deref_mut(), p_item.i_cursor, join_type);
        where_clause_insert(p_wc, expr_ref_new(p_term), TERM_DYNAMIC);
    }
}

