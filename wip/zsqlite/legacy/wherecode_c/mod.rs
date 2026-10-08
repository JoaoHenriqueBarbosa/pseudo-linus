// Mesclado das partes traduzidas de wherecode_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Itens abaixo de `SQLITE_OMIT_EXPLAIN` ficam todos presentes (a build do Debian não define o
// símbolo). `sqlite3WhereAddScanStatus` só existe com SQLITE_ENABLE_STMT_SCANSTATUS, que a build
// do Debian não liga, então ele some, e `IS_STMT_SCANSTATUS(db)` é sempre falso. O bloco
// `SQLITE_EXPLAIN_ESTIMATED_ROWS` e o rastreio `WHERETRACE_ENABLED` também não existem.

/// Monta a lista de argumentos de um `%s` (um `char*`) para `str_appendf`.
fn va_text(z: Vec<u8>) -> VaList<'static> {
    let mut ap = VaList::new();
    ap.args.push_back(VaArg::Text(Some(z)));
    ap
}

/// Retorna o nome da coluna i_ésima do índice p_idx.
fn explain_index_column_name(p_idx: &Index, i: usize) -> Vec<u8> {
    let i_col = p_idx.ai_column[i];
    if i_col == XN_EXPR {
        return b"<expr>".to_vec();
    }
    if i_col == XN_ROWID {
        return b"rowid".to_vec();
    }
    let p_table_ref = p_idx.p_table.upgrade().expect("explain_index_column_name: índice sem tabela");
    let p_table = p_table_ref.borrow();
    p_table.a_col[i_col as usize].z_cn_name.clone()
}

/// Esta rotina é um ajudante para explain_index_range() abaixo.
///
/// p_str contém o texto de uma expressão que estamos construindo um termo por vez.
/// Esta rotina adiciona um novo termo ao final da expressão. Os termos são
/// separados por AND, então adiciona o texto " AND " apenas para termos segundo e posteriores.
fn explain_append_term(
    p_str: &mut StrAccum,
    p_idx: &Index,
    n_term: i32,
    i_term: i32,
    b_and: i32,
    z_op: &[u8],
) {
    debug_assert!(n_term >= 1);
    if b_and != 0 {
        str_append(p_str, b" AND ", 5);
    }

    if n_term > 1 {
        str_append(p_str, b"(", 1);
    }
    for i in 0..n_term {
        if i != 0 {
            str_append(p_str, b",", 1);
        }
        let z = explain_index_column_name(p_idx, (i_term + i) as usize);
        str_appendall(p_str, &z);
    }
    if n_term > 1 {
        str_append(p_str, b")", 1);
    }

    str_append(p_str, z_op, 1);

    if n_term > 1 {
        str_append(p_str, b"(", 1);
    }
    for i in 0..n_term {
        if i != 0 {
            str_append(p_str, b",", 1);
        }
        str_append(p_str, b"?", 1);
    }
    if n_term > 1 {
        str_append(p_str, b")", 1);
    }
}

/// O argumento p_loop descreve uma estratégia para escanear uma tabela. Esta função
/// anexa texto a p_str que descreve o subconjunto de linhas da tabela escaneadas pela
/// estratégia na forma de uma expressão SQL.
///
/// Por exemplo, se a consulta:
///
///   SELECT * FROM t1 WHERE a=1 AND b>2;
///
/// for executada e houver um índice em (a, b), então esta função retorna uma
/// string similar a:
///
///   "a=? AND b>?"
fn explain_index_range(p_str: &mut StrAccum, p_loop: &WhereLoop) {
    let (n_eq, n_btm, n_top, p_index_ref) = match &p_loop.u {
        WhereLoopUnion::Btree { n_eq, n_btm, n_top, p_index, .. } => {
            (*n_eq as i32, *n_btm as i32, *n_top as i32, p_index.clone())
        }
        _ => unreachable!("explain_index_range: WhereLoop sem a parte btree"),
    };
    let p_index_rc = p_index_ref.expect("explain_index_range: sem índice");
    let p_index = p_index_rc.borrow();
    let n_skip = p_loop.n_skip as i32;

    if n_eq == 0 && (p_loop.ws_flags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) == 0 {
        return;
    }
    str_append(p_str, b" (", 2);
    let mut i = 0;
    while i < n_eq {
        let z = explain_index_column_name(&p_index, i as usize);
        if i != 0 {
            str_append(p_str, b" AND ", 5);
        }
        let mut ap = va_text(z);
        str_appendf(p_str, if i >= n_skip { b"%s=?" as &[u8] } else { b"ANY(%s)" }, &mut ap);
        i += 1;
    }

    let j = i;
    if (p_loop.ws_flags & WHERE_BTM_LIMIT) != 0 {
        explain_append_term(p_str, &p_index, n_btm, j, i, b">");
        i = 1;
    }
    if (p_loop.ws_flags & WHERE_TOP_LIMIT) != 0 {
        explain_append_term(p_str, &p_index, n_top, j, i, b"<");
    }
    str_append(p_str, b")", 1);
}

/// Esta função é um não-fazer a menos que estejamos processando um comando EXPLAIN QUERY PLAN,
/// ou se as estatísticas stmt_scanstatus_v2() estejam habilitadas, ou se SQLITE_DEBUG
/// foi definido no tempo de compilação. Se não for um não-fazer, um opcode OP_Explain único
/// é adicionado para descrever a estratégia de varredura de tabela em p_level.
///
/// Se um opcode OP_Explain é adicionado à VM, seu endereço é retornado.
/// Caso contrário, se nenhum OP_Explain for codificado, zero é retornado.
pub fn where_explain_one_scan(
    p_parse: &ParseRef,
    p_tab_list: &SrcList,
    p_level: &WhereLevel,
    w_ctrl_flags: u16,
) -> i32 {
    let mut ret = 0;
    let w_ctrl_flags = w_ctrl_flags as u32; // as constantes WHERE_* são u32
    let db_ref = p_parse.borrow().db.upgrade().expect("where_explain_one_scan: Parse sem db");
    let explain = parse_toplevel(p_parse).borrow().explain;
    if explain == 2 || is_stmt_scanstatus(&db_ref.borrow()) {
        let p_item = &p_tab_list.a[p_level.i_from as usize];
        let v_ref = p_parse.borrow().p_vdbe.clone().expect("where_explain_one_scan: Parse sem Vdbe");

        let p_loop_ref = p_level.p_w_loop.clone().expect("where_explain_one_scan: nível sem WhereLoop");
        let p_loop = p_loop_ref.borrow();
        let flags = p_loop.ws_flags;
        if (flags & WHERE_MULTI_OR) != 0 || (w_ctrl_flags & WHERE_OR_SUBCLAUSE) != 0 {
            return 0;
        }

        let n_eq_gt0 = match &p_loop.u {
            WhereLoopUnion::Btree { n_eq, .. } => *n_eq > 0,
            _ => false,
        };
        let is_search = (flags & (WHERE_BTM_LIMIT | WHERE_TOP_LIMIT)) != 0
            || ((flags & WHERE_VIRTUALTABLE) == 0 && n_eq_gt0)
            || (w_ctrl_flags & (WHERE_ORDERBY_MIN | WHERE_ORDERBY_MAX)) != 0;

        let mut str = StrAccum {
            db: None,
            z_text: Vec::new(),
            n_alloc: 0,
            mx_alloc: 0,
            n_char: 0,
            acc_error: 0,
            printf_flags: 0,
        };
        str_accum_init(&mut str, Some(db_ref.clone()), 100, SQLITE_MAX_LENGTH);
        str.printf_flags = SQLITE_PRINTF_INTERNAL;
        let mut ap = VaList::new();
        ap.args.push_back(VaArg::SrcItem(p_item));
        str_appendf(
            &mut str,
            if is_search { b"SEARCH %S" as &[u8] } else { b"SCAN %S" },
            &mut ap,
        );
        if (flags & (WHERE_IPK | WHERE_VIRTUALTABLE)) == 0 {
            let mut z_fmt: Option<&[u8]> = None;

            let p_idx_rc = match &p_loop.u {
                WhereLoopUnion::Btree { p_index, .. } => p_index.clone(),
                _ => None,
            }
            .expect("where_explain_one_scan: sem índice");
            let p_idx = p_idx_rc.borrow();
            debug_assert!((flags & WHERE_AUTO_INDEX) == 0 || (flags & WHERE_IDX_ONLY) != 0);
            let tab_has_rowid = has_rowid(&p_item.p_tab.as_ref().expect("SrcItem sem tabela").borrow());
            if !tab_has_rowid && is_primary_key_index(&p_idx) {
                if is_search {
                    z_fmt = Some(b"PRIMARY KEY");
                }
            } else if (flags & WHERE_PARTIALIDX) != 0 {
                z_fmt = Some(b"AUTOMATIC PARTIAL COVERING INDEX");
            } else if (flags & WHERE_AUTO_INDEX) != 0 {
                z_fmt = Some(b"AUTOMATIC COVERING INDEX");
            } else if (flags & WHERE_IDX_ONLY) != 0 {
                z_fmt = Some(b"COVERING INDEX %s");
            } else {
                z_fmt = Some(b"INDEX %s");
            }
            if let Some(z_fmt) = z_fmt {
                str_append(&mut str, b" USING ", 7);
                let mut ap = va_text(p_idx.z_name.clone());
                str_appendf(&mut str, z_fmt, &mut ap);
                explain_index_range(&mut str, &p_loop);
            }
        } else if (flags & WHERE_IPK) != 0 && (flags & WHERE_CONSTRAINT) != 0 {
            let c_range_op: u8;
            let z_rowid: &[u8] = b"rowid";
            let mut ap = va_text(z_rowid.to_vec());
            str_appendf(&mut str, b" USING INTEGER PRIMARY KEY (%s", &mut ap);
            if (flags & (WHERE_COLUMN_EQ | WHERE_COLUMN_IN)) != 0 {
                c_range_op = b'=';
            } else if (flags & WHERE_BOTH_LIMIT) == WHERE_BOTH_LIMIT {
                let mut ap = va_text(z_rowid.to_vec());
                str_appendf(&mut str, b">? AND %s", &mut ap);
                c_range_op = b'<';
            } else if (flags & WHERE_BTM_LIMIT) != 0 {
                c_range_op = b'>';
            } else {
                debug_assert!((flags & WHERE_TOP_LIMIT) != 0);
                c_range_op = b'<';
            }
            let mut ap = VaList::new();
            ap.args.push_back(VaArg::Int(c_range_op as i32));
            str_appendf(&mut str, b"%c?)", &mut ap);
        } else if (flags & WHERE_VIRTUALTABLE) != 0 {
            if let WhereLoopUnion::Vtab { idx_num, idx_str, .. } = &p_loop.u {
                let mut ap = VaList::new();
                ap.args.push_back(VaArg::Int(*idx_num));
                ap.args.push_back(VaArg::Text(Some(idx_str.clone())));
                str_appendf(&mut str, b" VIRTUAL TABLE INDEX %d:%s", &mut ap);
            }
        }
        if (p_item.fg.jointype & JT_LEFT) != 0 {
            let mut ap = VaList::new();
            str_appendf(&mut str, b" LEFT-JOIN", &mut ap);
        }
        let z_msg = str_accum_finish(&mut str);
        explain_breakpoint(b"", &z_msg);
        let p4 = match z_msg {
            Some(z) => P4Value::Dynamic(z),
            None => P4Value::NotUsed,
        };
        let addr_explain = p_parse.borrow().addr_explain;
        let mut v = v_ref.borrow_mut();
        let i_this = vdbe_current_addr(&v);
        ret = vdbe_add_op4(&mut v, OP_EXPLAIN as i32, i_this, addr_explain, 0, p4, P4_DYNAMIC as i8);
    }
    ret
}

/// Adiciona um opcode OP_Explain único que descreve um filtro Bloom.
///
/// Ou se não está processando EXPLAIN QUERY PLAN e não está numa build SQLITE_DEBUG
/// e/ou SQLITE_ENABLE_STMT_SCANSTATUS, então os opcodes OP_Explain não são
/// necessários e esta rotina é um não-fazer.
///
/// Se um opcode OP_Explain é adicionado à VM, seu endereço é retornado.
/// Caso contrário, se nenhum OP_Explain for codificado, zero é retornado.
pub fn where_explain_bloom_filter(
    p_parse: &Parse,
    p_w_info: &WhereInfo,
    p_level: &WhereLevel,
) -> i32 {
    let p_tab_list = p_w_info.p_tab_list.borrow();
    let p_item = &p_tab_list.a[p_level.i_from as usize];
    let v_ref = p_parse.p_vdbe.clone().expect("where_explain_bloom_filter: Parse sem Vdbe");
    let db = p_parse.db.upgrade();

    let mut str = StrAccum {
        db: None,
        z_text: Vec::new(),
        n_alloc: 0,
        mx_alloc: 0,
        n_char: 0,
        acc_error: 0,
        printf_flags: 0,
    };
    str_accum_init(&mut str, db, 100, SQLITE_MAX_LENGTH);
    str.printf_flags = SQLITE_PRINTF_INTERNAL;
    let mut ap = VaList::new();
    ap.args.push_back(VaArg::SrcItem(p_item));
    str_appendf(&mut str, b"BLOOM FILTER ON %S (", &mut ap);
    let p_loop_ref = p_level.p_w_loop.clone().expect("where_explain_bloom_filter: nível sem WhereLoop");
    let p_loop = p_loop_ref.borrow();
    if (p_loop.ws_flags & WHERE_IPK) != 0 {
        let p_tab = p_item.p_tab.as_ref().expect("SrcItem sem tabela").borrow();
        if p_tab.i_p_key >= 0 {
            let mut ap = va_text(p_tab.a_col[p_tab.i_p_key as usize].z_cn_name.clone());
            str_appendf(&mut str, b"%s=?", &mut ap);
        } else {
            let mut ap = VaList::new();
            str_appendf(&mut str, b"rowid=?", &mut ap);
        }
    } else {
        let (n_eq, p_index_ref) = match &p_loop.u {
            WhereLoopUnion::Btree { n_eq, p_index, .. } => (*n_eq as i32, p_index.clone()),
            _ => unreachable!("where_explain_bloom_filter: WhereLoop sem a parte btree"),
        };
        let p_index_rc = p_index_ref.expect("where_explain_bloom_filter: sem índice");
        let p_index = p_index_rc.borrow();
        let n_skip = p_loop.n_skip as i32;
        let mut i = n_skip;
        while i < n_eq {
            let z = explain_index_column_name(&p_index, i as usize);
            if i > n_skip {
                str_append(&mut str, b" AND ", 5);
            }
            let mut ap = va_text(z);
            str_appendf(&mut str, b"%s=?", &mut ap);
            i += 1;
        }
    }
    str_append(&mut str, b")", 1);
    let z_msg = str_accum_finish(&mut str);
    let p4 = match z_msg {
        Some(z) => P4Value::Dynamic(z),
        None => P4Value::NotUsed,
    };
    let mut v = v_ref.borrow_mut();
    let i_this = vdbe_current_addr(&v);
    let ret = vdbe_add_op4(&mut v, OP_EXPLAIN as i32, i_this, p_parse.addr_explain, 0, p4, P4_DYNAMIC as i8);

    let i_last = vdbe_current_addr(&v) - 1;
    vdbe_scan_status(&mut *v, i_last, 0, 0, 0, 0);
    ret
}

/// Desabilita um termo na cláusula WHERE. Exceto, não desabilita o termo
/// se ele controla um LEFT OUTER JOIN e não originou na cláusula ON
/// ou USING desse join.
///
/// Considere o termo t2.z='ok' nas seguintes consultas:
///
///   (1)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x WHERE t2.z='ok'
///   (2)  SELECT * FROM t1 LEFT JOIN t2 ON t1.a=t2.x AND t2.z='ok'
///   (3)  SELECT * FROM t1, t2 WHERE t1.a=t2.x AND t2.z='ok'
///
/// O t2.z='ok' é desabilitado no (2) porque origina da cláusula ON. O termo
/// é desabilitado em (3) porque não faz parte de um LEFT OUTER JOIN. Em (1), o termo
/// não é desabilitado.
///
/// Desabilitar um termo causa que esse termo não seja testado no loop interno
/// da junção. Desabilitar é uma otimização. Quando os termos são satisfeitos
/// por índices, desabilitamos para evitar testes redundantes no loop interno.
/// Obteríamos os resultados corretos se nada fosse jamais desabilitado,
/// mas as junções podem rodar um pouco mais lentamente. O truque é desabilitar
/// o máximo possível sem desabilitar demais. Se desabilitássemos em (1),
/// obteríamos a resposta errada. Veja o ticket #813.
///
/// Se todos os filhos de um termo são desabilitados, então esse termo também
/// é automaticamente desabilitado. Desta forma, os termos são desabilitados se
/// os termos virtuais derivados forem testados primeiro. Por exemplo:
///
///      x GLOB 'abc*' AND x>='abc' AND x<'acd'
///      \___________/     \______/     \_____/
///         pai            filho1       filho2
///
/// Apenas o termo pai estava na cláusula WHERE original. Os termos filho1
/// e filho2 foram adicionados pela otimização LIKE. Se ambos os termos
/// virtuais filhos forem válidos, então o teste do pai pode ser pulado.
///
/// Normalmente o termo pai é marcado como TERM_CODED. Mas se o termo pai
/// era originalmente TERM_LIKE, então o pai recebe TERM_LIKECOND em vez disso.
/// A marcação TERM_LIKECOND indica que o termo deveria ser codificado dentro
/// de um condicional de modo que seja apenas avaliado na segunda passagem de
/// um loop de otimização LIKE, ao escanear BLOBs em vez de strings.
fn disable_term(p_level: &WhereLevel, p_term: &WhereTermRef) {
    let mut n_loop = 0;
    let mut p_cur = p_term.clone();
    loop {
        let p_parent;
        {
            let mut t = p_cur.borrow_mut();
            let outer_on_ok = p_level.i_left_join == 0
                || expr_has_property(
                    &t.p_expr.as_ref().expect("disable_term: termo sem expressão").borrow(),
                    EP_OUTER_ON,
                );
            if !((t.wt_flags & TERM_CODED) == 0
                && outer_on_ok
                && (p_level.not_ready & t.prereq_all) == 0)
            {
                break;
            }
            if n_loop != 0 && (t.wt_flags & TERM_LIKE) != 0 {
                t.wt_flags |= TERM_LIKECOND;
            } else {
                t.wt_flags |= TERM_CODED;
            }
            if t.i_parent < 0 {
                break;
            }
            let p_wc = t.p_wc.upgrade().expect("disable_term: termo sem WhereClause");
            p_parent = p_wc.borrow().a[t.i_parent as usize].clone();
        }
        p_cur = p_parent;
        {
            let mut t = p_cur.borrow_mut();
            t.n_child = t.n_child.wrapping_sub(1);
            if t.n_child != 0 {
                break;
            }
        }
        n_loop += 1;
    }
}


// ---- part_001.rs ----

// Os `testcase()`, `VdbeCoverage*()` e `VdbeComment()` do C são no-ops nesta build (sem
// SQLITE_DEBUG, SQLITE_COVERAGE_TEST nem SQLITE_ENABLE_EXPLAIN_COMMENTS) e por isso não aparecem.

/// Codifica um opcode OP_Affinity para aplicar a cadeia de afinidade z_aff
/// aos n registradores começando em base.
///
/// Como uma otimização, entradas SQLITE_AFF_BLOB e SQLITE_AFF_NONE
/// (que são operações vazias) no início e fim de z_aff são ignoradas.
/// Se todas as entradas em z_aff são SQLITE_AFF_BLOB ou SQLITE_AFF_NONE,
/// então nenhum código é gerado.
///
/// Esta rotina faz sua própria cópia de z_aff de modo que o chamador fica
/// livre para modificar z_aff após o retorno.
fn code_apply_affinity(p_parse: &ParseRef, base: i32, n: i32, z_aff: Option<&[u8]>) {
    let z_aff = match z_aff {
        Some(z) => z,
        None => {
            debug_assert!(p_parse.borrow().db.upgrade().map_or(true, |d| d.borrow().malloc_failed != 0));
            return;
        }
    };
    let v = p_parse.borrow().p_vdbe.clone().expect("code_apply_affinity: Parse sem Vdbe");
    let mut base = base;
    let mut n = n;
    let mut off = 0usize;

    // Ajusta base e n para pular entradas SQLITE_AFF_BLOB e SQLITE_AFF_NONE
    // no início e fim da cadeia de afinidade.
    debug_assert!(SQLITE_AFF_NONE < SQLITE_AFF_BLOB);
    while n > 0 && z_aff[off] <= SQLITE_AFF_BLOB {
        n -= 1;
        base += 1;
        off += 1;
    }
    while n > 1 && z_aff[off + (n - 1) as usize] <= SQLITE_AFF_BLOB {
        n -= 1;
    }

    // Codifica o opcode OP_Affinity se há algo a fazer. O P4 é uma cópia de n bytes
    // (P4 com tipo n>=0 no C), por isso vdbe_change_p4() recebe n direto (i32).
    if n > 0 {
        let mut vb = v.borrow_mut();
        let addr = vdbe_add_op3(&mut vb, OP_AFFINITY as i32, base, n, 0);
        vdbe_change_p4(&mut vb, addr, P4Value::Static(z_aff[off..off + n as usize].to_vec()), n);
    }
}

/// A expressão p_right, que é o RHS de uma operação de comparação, é
/// ou um vetor de n elementos ou, se n==1, uma expressão escalar.
/// Antes da operação de comparação, a afinidade z_aff é a ser aplicada
/// aos valores de p_right. Esta função modifica caracteres dentro da
/// cadeia de afinidade para SQLITE_AFF_BLOB se:
///
///   * a comparação será realizada sem afinidade, ou
///   * a mudança de afinidade em z_aff é garantida não mudar o valor.
pub fn update_range_affinity_str(p_right: &Expr, n: i32, z_aff: &mut [u8]) {
    for i in 0..n {
        let p = vector_field_subexpr(p_right, i);
        if compare_affinity(p, z_aff[i as usize]) == SQLITE_AFF_BLOB
            || expr_needs_no_affinity_change(p, z_aff[i as usize]) != 0
        {
            z_aff[i as usize] = SQLITE_AFF_BLOB;
        }
    }
}

/// p_x é uma expressão do formato: (vetor) IN (SELECT ...)
/// Em outras palavras, é um operador IN de vetor com uma cláusula SELECT no
/// RHS. Mas nem todos os termos no vetor são indexáveis e os termos podem
/// não estar na ordem correta para indexação.
///
/// Esta rotina faz uma cópia da expressão p_x de entrada e depois ajusta
/// o vetor no LHS com mudanças correspondentes ao SELECT de modo que
/// o vetor contém apenas termos de índice e esses termos estão na ordem
/// correta. A expressão IN modificada é retornada. O chamador é responsável
/// por deletar a expressão retornada.
///
/// Exemplo:
///
///    CREATE TABLE t1(a,b,c,d,e,f);
///    CREATE INDEX t1x1 ON t1(e,c);
///    SELECT * FROM t1 WHERE (a,b,c,d,e) IN (SELECT v,w,x,y,z FROM t2)
///                           \_______________________________________/
///                                     A expressão p_x
///
/// Como apenas colunas e e c podem ser usadas com o índice, nessa ordem,
/// a expressão IN modificada que é retornada será:
///
///        (e,c) IN (SELECT z,x FROM t2)
///
/// O p_x reduzido é diferente do original (obviamente) e portanto só é
/// usado para indexação, para melhorar o desempenho. A expressão IN original
/// inalterada também precisa rodar em cada linha de saída por correção.
///
/// A identidade `pLoop->aLTerm[i]->pExpr==pX` do C vira `Rc::ptr_eq` sobre o `ExprRef`.
fn remove_unindexable_in_clause_terms(
    p_parse: &ParseRef,
    i_eq: i32,
    p_loop: &WhereLoop,
    p_x: &ExprRef,
) -> Option<Box<Expr>> {
    let db = p_parse.borrow().db.upgrade().expect("remove_unindexable_in_clause_terms: Parse sem db");
    let mut p_new = expr_dup(&db.borrow(), &p_x.borrow(), 0);
    if db.borrow().malloc_failed == 0 {
        let e = p_new.as_deref_mut().expect("remove_unindexable_in_clause_terms: expr_dup falhou");
        let x = &mut e.x;
        let p_left = &mut e.p_left;
        let mut cur: Option<&mut Select> = x.p_select.as_deref_mut();
        let mut first = true;
        while let Some(p_select) = cur {
            let mut p_orig_rhs: Box<ExprList> =
                p_select.p_elist.take().expect("remove_unindexable_in_clause_terms: SELECT sem lista");
            let mut p_orig_lhs: Option<Box<ExprList>> = None;
            let mut p_rhs: Option<Box<ExprList>> = None;
            let mut p_lhs: Option<Box<ExprList>> = None;

            debug_assert!(p_left.is_some());
            if first {
                p_orig_lhs = p_left.as_mut().expect("IN vetorial sem lado esquerdo").x.p_list.take();
            }
            for i in i_eq..p_loop.n_l_term as i32 {
                let p_term_ref = p_loop.a_l_term[i as usize].as_ref().expect("a_l_term nulo").clone();
                let p_term = p_term_ref.borrow();
                let same = match &p_term.p_expr {
                    Some(pe) => Rc::ptr_eq(pe, p_x),
                    None => false,
                };
                if same {
                    debug_assert!((p_term.e_operator & (WO_OR | WO_AND)) == 0);
                    let i_field = match &p_term.u {
                        WhereTermUnion::X { i_field, .. } => (*i_field - 1) as usize,
                        _ => unreachable!("remove_unindexable_in_clause_terms: termo sem u.x"),
                    };
                    if p_orig_rhs.a[i_field].p_expr.is_none() {
                        continue; // Coluna PK duplicada
                    }
                    let e_rhs = p_orig_rhs.a[i_field].p_expr.take();
                    p_rhs = expr_list_append(p_parse, p_rhs, e_rhs);
                    if let Some(lhs) = p_orig_lhs.as_mut() {
                        debug_assert!(lhs.a[i_field].p_expr.is_some());
                        let e_lhs = lhs.a[i_field].p_expr.take();
                        p_lhs = expr_list_append(p_parse, p_lhs, e_lhs);
                    }
                }
            }
            expr_list_delete(&db.borrow(), Some(p_orig_rhs));
            if let Some(orig_lhs) = p_orig_lhs {
                expr_list_delete(&db.borrow(), Some(orig_lhs));
                p_left.as_mut().expect("IN vetorial sem lado esquerdo").x.p_list = p_lhs;
            }
            p_select.p_elist = p_rhs;
            let lhs_single = first
                && p_left
                    .as_ref()
                    .and_then(|l| l.x.p_list.as_ref())
                    .map_or(false, |l| l.n_expr == 1);
            if lhs_single {
                // Cuidado para não gerar um TK_VECTOR contendo apenas um valor. Como o
                // parser nunca cria tal vetor, algumas sub-rotinas não lidam com o caso.
                let mut old = p_left.take().expect("IN vetorial sem lado esquerdo");
                let p = old.x.p_list.as_mut().expect("lista nula").a[0].p_expr.take();
                expr_delete(&db.borrow(), Some(old));
                *p_left = p;
            }
            if let Some(p_order_by) = p_select.p_order_by.as_mut() {
                // Se o SELECT tem ORDER BY, zera os i_order_by_col. Eles são não zero quando
                // um termo ORDER BY casa exatamente com um termo do conjunto de resultados.
                // Como o conjunto de resultados pode ter sido modificado ou reordenado, eles
                // deixam de valer. Como são só uma otimização, o mais simples é zerá-los.
                let n_expr = p_order_by.n_expr as usize;
                for item in p_order_by.a.iter_mut().take(n_expr) {
                    if let ExprListItemU::X { i_order_by_col, .. } = &mut item.u {
                        *i_order_by_col = 0;
                    }
                }
            }
            first = false;
            cur = p_select.p_prior.as_deref_mut();
        }
    }
    p_new
}

/// Gera código para um único termo de igualdade da cláusula WHERE. Um termo de igualdade
/// pode ser X=expr ou X IN (...). p_term é o termo a ser codificado.
///
/// O valor atual para a restrição é deixado em um registrador, cujo
/// índice é retornado. É feita uma tentativa de armazenar o resultado em i_target mas
/// isto é garantido apenas para restrições TK_ISNULL e TK_IN. Se a
/// restrição é TK_EQ ou TK_IS, o valor atual pode estar em
/// algum outro registrador e é responsabilidade do chamador compensar.
///
/// Para uma restrição do formato X=expr, a expressão é avaliada em
/// código de linha reta. Para restrições do formato X IN (...)
/// esta rotina configura um laço que iterará sobre todos os valores de X.
fn code_equality_term(
    p_parse: &ParseRef,
    p_term: &WhereTermRef,
    p_level: &mut WhereLevel,
    i_eq: i32,
    b_rev: i32,
    i_target: i32,
) -> i32 {
    let p_x_ref: ExprRef = p_term.borrow().p_expr.clone().expect("code_equality_term: termo sem expressão");
    let v = p_parse.borrow().p_vdbe.clone().expect("code_equality_term: Parse sem Vdbe");
    let p_loop_ref = p_level.p_w_loop.clone().expect("code_equality_term: nível sem WhereLoop");
    let i_reg: i32;
    let mut b_rev = b_rev;

    debug_assert!(Rc::ptr_eq(
        p_loop_ref.borrow().a_l_term[i_eq as usize].as_ref().expect("a_l_term nulo"),
        p_term
    ));
    debug_assert!(i_target > 0);
    let x_op = p_x_ref.borrow().op;
    if x_op == TK_EQ || x_op == TK_IS {
        let mut px = p_x_ref.borrow_mut();
        i_reg = expr_code_target(p_parse, px.p_right.as_deref_mut(), i_target);
    } else if x_op == TK_ISNULL {
        i_reg = i_target;
        vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, i_reg);
    } else {
        let mut e_type = IN_INDEX_NOOP;
        let mut i_tab: i32 = 0;
        let mut ai_map: Option<Vec<i32>> = None;
        let mut n_eq: i32 = 0;

        {
            let l = p_loop_ref.borrow();
            if (l.ws_flags & WHERE_VIRTUALTABLE) == 0 {
                if let WhereLoopUnion::Btree { p_index: Some(idx), .. } = &l.u {
                    if idx.borrow().a_sort_order[i_eq as usize] != 0 {
                        b_rev = (b_rev == 0) as i32;
                    }
                }
            }
        }
        debug_assert!(x_op == TK_IN);
        i_reg = i_target;

        for i in 0..i_eq {
            let dup = {
                let l = p_loop_ref.borrow();
                match &l.a_l_term[i as usize] {
                    Some(t) => match &t.borrow().p_expr {
                        Some(pe) => Rc::ptr_eq(pe, &p_x_ref),
                        None => false,
                    },
                    None => false,
                }
            };
            if dup {
                disable_term(p_level, p_term);
                return i_target;
            }
        }
        let n_l_term = p_loop_ref.borrow().n_l_term as i32;
        for i in i_eq..n_l_term {
            let l = p_loop_ref.borrow();
            let t = l.a_l_term[i as usize].as_ref().expect("a_l_term nulo");
            if Rc::ptr_eq(t.borrow().p_expr.as_ref().expect("termo sem expressão"), &p_x_ref) {
                n_eq += 1;
            }
        }

        i_tab = 0;
        let x_is_select = expr_use_x_select(&p_x_ref.borrow());
        let one_col = !x_is_select
            || p_x_ref
                .borrow()
                .x
                .p_select
                .as_ref()
                .expect("IN sem SELECT")
                .p_elist
                .as_ref()
                .expect("SELECT sem lista")
                .n_expr
                == 1;
        if one_col {
            e_type = find_in_index(p_parse, &mut p_x_ref.borrow_mut(), IN_INDEX_LOOP, None, None, &mut i_tab);
        } else {
            let (i_table_zero, has_subrtn) = {
                let e = p_x_ref.borrow();
                (e.i_table == 0, expr_has_property(&e, EP_SUBRTN))
            };
            if i_table_zero || !has_subrtn {
                let db = p_parse.borrow().db.upgrade().expect("code_equality_term: Parse sem db");
                let mut p_x_new = {
                    let l = p_loop_ref.borrow();
                    remove_unindexable_in_clause_terms(p_parse, i_eq, &l, &p_x_ref)
                };
                if db.borrow().malloc_failed == 0 {
                    let mut map = vec![0i32; n_eq as usize];
                    e_type = find_in_index(
                        p_parse,
                        p_x_new.as_deref_mut().expect("expressão IN reduzida nula"),
                        IN_INDEX_LOOP,
                        None,
                        Some(&mut map[..]),
                        &mut i_tab,
                    );
                    ai_map = Some(map);
                    p_x_ref.borrow_mut().i_table = i_tab;
                }
                expr_delete(&db.borrow(), p_x_new);
            } else {
                let n = expr_vector_size(p_x_ref.borrow().p_left.as_deref().expect("IN vetorial sem lado esquerdo"));
                let mut map = vec![0i32; std::cmp::max(n_eq, n) as usize];
                e_type = find_in_index(
                    p_parse,
                    &mut p_x_ref.borrow_mut(),
                    IN_INDEX_LOOP,
                    None,
                    Some(&mut map[..]),
                    &mut i_tab,
                );
                ai_map = Some(map);
            }
        }

        if e_type == IN_INDEX_INDEX_DESC {
            b_rev = (b_rev == 0) as i32;
        }
        vdbe_add_op2(
            &mut v.borrow_mut(),
            if b_rev != 0 { OP_LAST } else { OP_REWIND } as i32,
            i_tab,
            0,
        );

        debug_assert!((p_loop_ref.borrow().ws_flags & WHERE_MULTI_OR) == 0);
        p_loop_ref.borrow_mut().ws_flags |= WHERE_IN_ABLE;
        if !matches!(p_level.u, WhereLevelUnion::In { .. }) {
            // A união zerada do C: u.in.nIn==0 e aInLoop==NULL.
            p_level.u = WhereLevelUnion::In { n_in: 0, a_in_loop: Vec::new() };
        }
        let n_in_now = match &p_level.u {
            WhereLevelUnion::In { n_in, .. } => *n_in,
            _ => 0,
        };
        if n_in_now == 0 {
            p_level.addr_nxt = vdbe_make_label(&mut p_parse.borrow_mut());
        }
        if i_eq > 0 && (p_loop_ref.borrow().ws_flags & WHERE_IN_SEEKSCAN) == 0 {
            p_loop_ref.borrow_mut().ws_flags |= WHERE_IN_EARLYOUT;
        }

        let i_idx_cur = p_level.i_idx_cur;
        if let WhereLevelUnion::In { n_in, a_in_loop } = &mut p_level.u {
            let first_new = *n_in as usize;
            *n_in += n_eq;
            // sqlite3WhereRealloc() do C: o Vec cresce preservando o conteúdo.
            a_in_loop.resize_with(*n_in as usize, InLoop::default);
            let mut p_in = first_new;
            let mut vb = v.borrow_mut();
            let mut i_map = 0usize;
            for i in i_eq..n_l_term {
                let same = {
                    let l = p_loop_ref.borrow();
                    let t = l.a_l_term[i as usize].as_ref().expect("a_l_term nulo");
                    Rc::ptr_eq(t.borrow().p_expr.as_ref().expect("termo sem expressão"), &p_x_ref)
                };
                if same {
                    let i_out = i_reg + i - i_eq;
                    if e_type == IN_INDEX_ROWID {
                        a_in_loop[p_in].addr_in_top = vdbe_add_op2(&mut vb, OP_ROWID as i32, i_tab, i_out);
                    } else {
                        let i_col = match &ai_map {
                            Some(m) => {
                                let c = m[i_map];
                                i_map += 1;
                                c
                            }
                            None => 0,
                        };
                        a_in_loop[p_in].addr_in_top = vdbe_add_op3(&mut vb, OP_COLUMN as i32, i_tab, i_col, i_out);
                    }
                    vdbe_add_op1(&mut vb, OP_ISNULL as i32, i_out);
                    if i == i_eq {
                        a_in_loop[p_in].i_cur = i_tab;
                        a_in_loop[p_in].e_end_loop_op = if b_rev != 0 { OP_PREV } else { OP_NEXT };
                        if i_eq > 0 {
                            a_in_loop[p_in].i_base = i_reg - i;
                            a_in_loop[p_in].n_prefix = i;
                        } else {
                            a_in_loop[p_in].n_prefix = 0;
                        }
                    } else {
                        a_in_loop[p_in].e_end_loop_op = OP_NOOP;
                    }
                    p_in += 1;
                }
            }
            if i_eq > 0
                && (p_loop_ref.borrow().ws_flags & (WHERE_IN_SEEKSCAN | WHERE_VIRTUALTABLE)) == 0
            {
                vdbe_add_op3(&mut vb, OP_SEEKHIT as i32, i_idx_cur, 0, i_eq);
            }
        }
    }

    // Como uma otimização, tente desabilitar o termo da cláusula WHERE que
    // é o condutor do índice, pois sempre será verdadeiro. A resposta correta é
    // obtida independentemente, mas podemos obter a resposta com menos ciclos de CPU
    // omitindo o termo.
    //
    // Mas não desabilite o termo a menos que tenhamos certeza de que o termo não é
    // uma restrição transitiva. Para um exemplo em que isso não funciona, veja
    // https://sqlite.org/forum/forumpost/eb8613976a (2021-05-04)
    let ws_flags = p_loop_ref.borrow().ws_flags;
    if (ws_flags & WHERE_TRANSCONS) == 0 || (p_term.borrow().e_operator & WO_EQUIV) == 0 {
        disable_term(p_level, p_term);
    }

    i_reg
}

/// Gera código que avaliará todas as restrições == e IN para uma
/// varredura de índice.
///
/// Por exemplo, considere a tabela t1(a,b,c,d,e,f) com índice i1(a,b,c).
/// Suponha que a cláusula WHERE seja esta: a==5 AND b IN (1,2,3) AND c>5 AND c<10
/// O índice tem até três restrições de igualdade, mas neste
/// exemplo, o terceiro valor "c" é uma desigualdade. Então apenas dois
/// termos são codificados. Esta rotina gerará código para avaliar
/// a==5 e b IN (1,2,3). Os valores atuais para a e b serão armazenados
/// em registradores consecutivos e o índice do primeiro registrador é retornado.
///
/// No exemplo acima n_eq==2. Mas esta sub-rotina funciona para qualquer valor
/// de n_eq, inclusive 0. Se n_eq==0, esta rotina é quase um no-op.
/// A única coisa que faz é alocar a célula de memória p_level.i_mem e
/// calcular a cadeia de afinidade.
///
/// O parâmetro n_extra_reg é 0 ou 1. É 0 se todas as restrições da cláusula WHERE
/// são == ou IN e estão cobertas por n_eq. n_extra_reg é 1 se há
/// uma restrição de desigualdade (como o "c>=5 AND c<10" do exemplo) que
/// ocorre depois das restrições de igualdade n_eq.
///
/// Esta rotina aloca um intervalo de n_eq+n_extra_reg células de memória e retorna
/// o índice da primeira célula desse intervalo. O código que chama esta rotina
/// usará esse intervalo de memória para guardar as chaves das condições de
/// início e término do laço. Se um ou mais operadores IN aparecem, então
/// esta rotina aloca n_eq células de memória adicionais para uso interno.
///
/// Antes de retornar, o segundo elemento do resultado é uma cópia da cadeia de
/// afinidade de colunas do índice. As entradas da cópia associadas a restrições de
/// igualdade que usam afinidade BLOB ou NONE ficam SQLITE_AFF_BLOB. Isto é para
/// tratar SQL como o seguinte:
///
///   CREATE TABLE t1(a TEXT PRIMARY KEY, b);
///   SELECT ... FROM t1 AS t2, t1 WHERE t1.a = t2.b;
///
/// No exemplo acima, o índice em t1(a) tem afinidade TEXT. Mas como
/// o lado direito da restrição de igualdade (t2.b) tem afinidade BLOB/NONE,
/// nenhuma conversão deve ser tentada antes de usar um valor t2.b como parte de
/// uma chave para buscar no índice. Logo o primeiro byte da cadeia de afinidade
/// devolvida neste exemplo seria SQLITE_AFF_BLOB.
///
/// Devolve reg_base; `pz_aff` (o `char **pzAff` do C) recebe a cópia da cadeia de afinidade, ou
/// None só se faltou memória (`zAff==0` do C).
fn code_all_equality_terms(
    p_parse: &ParseRef,
    p_level: &mut WhereLevel,
    b_rev: i32,
    n_extra_reg: i32,
    pz_aff: &mut Option<Vec<u8>>,
) -> i32 {
    let v = p_parse.borrow().p_vdbe.clone().expect("code_all_equality_terms: Parse sem Vdbe");

    // Este módulo só é chamado em planos de consulta que usam um índice.
    let p_loop_ref = p_level.p_w_loop.clone().expect("code_all_equality_terms: nível sem WhereLoop");
    let (n_eq, n_skip, p_idx_ref) = {
        let p_loop = p_loop_ref.borrow();
        debug_assert!((p_loop.ws_flags & WHERE_VIRTUALTABLE) == 0);
        match &p_loop.u {
            WhereLoopUnion::Btree { n_eq, p_index, .. } => (
                *n_eq as i32,
                p_loop.n_skip as i32,
                p_index.clone().expect("code_all_equality_terms: sem índice"),
            ),
            _ => unreachable!("code_all_equality_terms: WhereLoop sem a parte btree"),
        }
    };

    // Descobre quantas células de memória precisamos e as aloca.
    let mut reg_base;
    let n_reg;
    {
        let mut pp = p_parse.borrow_mut();
        reg_base = pp.n_mem + 1;
        n_reg = n_eq + n_extra_reg;
        pp.n_mem += n_reg;
    }

    let db = p_parse.borrow().db.upgrade().expect("code_all_equality_terms: Parse sem db");
    let mut z_aff: Option<Vec<u8>> = index_affinity_str(&db, &p_idx_ref);
    debug_assert!(z_aff.is_some() || db.borrow().malloc_failed != 0);

    if n_skip != 0 {
        let i_idx_cur = p_level.i_idx_cur;
        let mut vb = v.borrow_mut();
        vdbe_add_op3(&mut vb, OP_NULL as i32, 0, reg_base, reg_base + n_skip - 1);
        vdbe_add_op1(&mut vb, if b_rev != 0 { OP_LAST } else { OP_REWIND } as i32, i_idx_cur);
        let j = vdbe_add_op0(&mut vb, OP_GOTO as i32);
        debug_assert!(p_level.addr_skip == 0);
        p_level.addr_skip = vdbe_add_op4_int(
            &mut vb,
            if b_rev != 0 { OP_SEEKLT } else { OP_SEEKGT } as i32,
            i_idx_cur,
            0,
            reg_base,
            n_skip,
        );
        vdbe_jump_here(&mut vb, j);
        for j in 0..n_skip {
            vdbe_add_op3(&mut vb, OP_COLUMN as i32, i_idx_cur, j, reg_base + j);
        }
    }

    // Avalia as restrições de igualdade.
    debug_assert!(z_aff.as_ref().map_or(true, |z| z.len() as i32 >= n_eq));
    for j in n_skip..n_eq {
        let p_term: WhereTermRef =
            p_loop_ref.borrow().a_l_term[j as usize].as_ref().expect("a_l_term nulo").clone();
        // O caso a seguir ocorre em índices com colunas redundantes.
        // Ex: CREATE INDEX i1 ON t1(a,b,a); SELECT * FROM t1 WHERE a=0 AND b=0;
        let r1 = code_equality_term(p_parse, &p_term, p_level, j, b_rev, reg_base + j);
        if r1 != reg_base + j {
            if n_reg == 1 {
                release_temp_reg(&mut p_parse.borrow_mut(), reg_base);
                reg_base = r1;
            } else {
                vdbe_add_op2(&mut v.borrow_mut(), OP_COPY as i32, r1, reg_base + j);
            }
        }
        let (e_operator, wt_flags, p_expr_ref) = {
            let t = p_term.borrow();
            (t.e_operator, t.wt_flags, t.p_expr.clone().expect("termo sem expressão"))
        };
        if (e_operator & WO_IN) != 0 {
            if (p_expr_ref.borrow().flags & EP_X_IS_SELECT) != 0 {
                // Nenhuma afinidade precisa (nem deve) ser aplicada a um valor do RHS de uma
                // expressão "? IN (SELECT ...)". find_in_index() já garantiu que a
                // afinidade da comparação foi aplicada ao valor.
                if let Some(z) = z_aff.as_mut() {
                    z[j as usize] = SQLITE_AFF_BLOB;
                }
            }
        } else if (e_operator & WO_ISNULL) == 0 {
            let p_expr = p_expr_ref.borrow();
            let p_right = p_expr.p_right.as_deref().expect("termo sem lado direito");
            if (wt_flags & TERM_IS) == 0 && expr_can_be_null(p_right) != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg_base + j, p_level.addr_brk);
            }
            if p_parse.borrow().n_err == 0 {
                debug_assert!(db.borrow().malloc_failed == 0);
                let z = z_aff.as_mut().expect("code_all_equality_terms: z_aff nulo");
                if compare_affinity(p_right, z[j as usize]) == SQLITE_AFF_BLOB {
                    z[j as usize] = SQLITE_AFF_BLOB;
                }
                if expr_needs_no_affinity_change(p_right, z[j as usize]) != 0 {
                    z[j as usize] = SQLITE_AFF_BLOB;
                }
            }
        }
    }
    *pz_aff = z_aff;
    reg_base
}


// ---- part_002.rs ----

// `SQLITE_ENABLE_CURSOR_HINTS` não está ligado na build do Debian 13. Por isso a estrutura
// `CCurHint`, `codeCursorHintCheckExpr`, `codeCursorHintIsOrFunction`, `codeCursorHintFixExpr` e
// `codeCursorHint` não existem aqui: `codeCursorHint(A,B,C,D)` é um no-op no C, e os chamadores
// (where_code_one_loop) simplesmente não o chamam. `SQLITE_LIKE_DOESNT_MATCH_BLOBS` também não
// está definido, então `where_like_optimization_string_fixup` existe.

/// Se a instrução codificada mais recentemente é uma restrição de intervalo de
/// constante (literal de string) que veio da otimização LIKE, define P3 e P5
/// no opcode OP_String para que a string seja convertida para BLOB nos momentos apropriados.
///
/// A otimização LIKE tenta avaliar "x LIKE 'abc%'" como uma expressão de intervalo:
/// "x>='ABC' AND x<'abd'". Mas isso exige que o laço de varredura de intervalo rode
/// duas vezes: uma para strings e outra para BLOBs. Os opcodes OP_String na segunda
/// passada convertem os limites superior e inferior de constantes string para blobs.
/// Esta rotina faz as alterações necessárias aos opcodes OP_String para que isso
/// aconteça.
pub fn where_like_optimization_string_fixup(v: &VdbeRef, p_level: &WhereLevel, p_term: &WhereTerm) {
    if (p_term.wt_flags & TERM_LIKEOPT) != 0 {
        debug_assert!(p_level.i_like_rep_cntr > 0);
        let mut vb = v.borrow_mut();
        let malloc_failed = vb.db.upgrade().map_or(false, |d| d.borrow().malloc_failed != 0);
        let p_op = vdbe_get_last_op(&mut vb);
        debug_assert!(p_op.opcode == OP_STRING8 || malloc_failed);
        p_op.p3 = (p_level.i_like_rep_cntr >> 1) as i32; // Registrador que guarda o contador
        p_op.p5 = (p_level.i_like_rep_cntr & 1) as u16; // ASC ou DESC
    }
}

/// O cursor i_cur está aberto num intkey b-tree (uma tabela). O registrador i_rowid
/// contém um valor de rowid acabado de ler do cursor i_idx_cur, aberto no índice p_idx.
/// Esta função gera código para fazer uma busca diferida do cursor i_cur para o rowid
/// armazenado no registrador i_rowid.
///
/// Normalmente, isto é apenas:
///
///   OP_DeferredSeek $i_cur $i_rowid
///
/// O que causa uma busca em $i_cur para a linha com rowid $i_rowid.
///
/// Porém, se a varredura sendo codificada agora é um ramo de um loop OR e a
/// instrução sendo codificada é um SELECT, então informação adicional é adicionada
/// que pode permitir OP_Column omitir a busca e em vez disso fazer sua busca no
/// índice, evitando uma operação de busca cara. Para habilitar esta otimização,
/// P3 de OP_DeferredSeek é definido para i_idx_cur e P4 é definido para um array
/// de inteiros contendo uma entrada para cada coluna da tabela. Para cada coluna
/// da tabela, se a coluna é a i-ésima coluna do índice, então a entrada de array
/// correspondente é definida para (i+1). Se a coluna não aparece no índice, a
/// entrada de array é definida para 0. O opcode OP_Column pode verificar este
/// array para ver se a coluna que quer está no índice e se está, ele substituirá
/// o cursor de índice e o número de coluna e continuará com esses novos valores,
/// em vez de buscar o cursor da tabela.
pub fn code_deferred_seek(p_w_info: &mut WhereInfo, p_idx: &Index, i_cur: i32, i_idx_cur: i32) {
    let p_parse = p_w_info.p_parse.clone();
    let v = p_parse.borrow().p_vdbe.clone().expect("code_deferred_seek: Parse sem Vdbe");

    debug_assert!(i_idx_cur > 0);
    debug_assert!(p_idx.ai_column[p_idx.n_column as usize - 1] == -1);

    p_w_info.b_deferred_seek = true;
    let mut vb = v.borrow_mut();
    vdbe_add_op3(&mut vb, OP_DEFERREDSEEK as i32, i_idx_cur, 0, i_cur);
    let write_mask = parse_toplevel(&p_parse).borrow().write_mask;
    if ((p_w_info.wctrl_flags as u32) & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) != 0
        && db_mask_all_zero(write_mask)
    {
        let p_tab_ref = p_idx.p_table.upgrade().expect("code_deferred_seek: índice sem tabela");
        let p_tab = p_tab_ref.borrow();
        let mut ai: Vec<u32> = vec![0; p_tab.n_col as usize + 1];
        ai[0] = p_tab.n_col as u32;
        for i in 0..(p_idx.n_column as usize - 1) {
            debug_assert!((p_idx.ai_column[i] as i32) < p_tab.n_col as i32);
            let x1 = p_idx.ai_column[i];
            let x2 = table_column_to_storage(&p_tab, x1);
            if x1 >= 0 {
                ai[x2 as usize + 1] = (i + 1) as u32;
            }
        }
        vdbe_change_p4(&mut vb, -1, P4Value::IntArray(ai), P4_INTARRAY as i32);
    }
}

/// Se a expressão passada como segundo argumento é um vetor, gera código para
/// escrever os primeiros n_reg elementos do vetor num array de registradores
/// começando em i_reg.
///
/// Se a expressão não é um vetor, então n_reg deve ser passado 1. Neste caso,
/// gera código para avaliar a expressão e deixar o resultado no registrador i_reg.
pub fn code_expr_or_vector(p_parse: &ParseRef, p: Option<&mut Expr>, i_reg: i32, n_reg: i32) {
    debug_assert!(n_reg > 0);
    match p {
        Some(p) if expr_is_vector(p) => {
            if expr_use_x_select(p) {
                let v = p_parse.borrow().p_vdbe.clone().expect("code_expr_or_vector: Parse sem Vdbe");
                debug_assert!(p.op == TK_SELECT);
                let i_select = code_subselect(p_parse, p);
                vdbe_add_op3(&mut v.borrow_mut(), OP_COPY as i32, i_select, i_reg, n_reg - 1);
            } else {
                debug_assert!(expr_use_x_list(p));
                let p_list = p.x.p_list.as_ref().expect("code_expr_or_vector: vetor sem lista");
                debug_assert!(n_reg <= p_list.n_expr);
                for i in 0..n_reg {
                    expr_code(&mut p_parse.borrow_mut(), p_list.a[i as usize].p_expr.as_deref(), i_reg + i);
                }
            }
        }
        other => {
            debug_assert!(n_reg == 1 || p_parse.borrow().n_err != 0);
            expr_code(&mut p_parse.borrow_mut(), other.as_deref(), i_reg);
        }
    }
}


// ---- part_003.rs ----

// ATENÇÃO, integrador: esta parte NÃO é um arquivo só de itens. Ela termina no MEIO de
// `where_code_one_loop_start` (sqlite3WhereCodeOneLoopStart), dentro do ramo "Case 2" da cadeia
// if/else-if, e as partes 004, 005 e 006 continuam o mesmo corpo. O fechamento da função fica
// no fim da parte 006 (que termina com `return`).
//
// Aliasing do C: em `sqlite3WhereCodeOneLoopStart`, `pLevel` é `&pWInfo->a[iLevel]`. Em Rust os
// dois chegam como `&mut` separados, então o chamador precisa destacar o nível (por exemplo
// `std::mem::take`/`replace` de `p_w_info.a[i_level]`) e recolocá-lo depois. Enquanto está
// destacado, a entrada `p_w_info.a[i_level]` é um placeholder e NÃO vale: onde o C lê
// `pWInfo->a[iLevel]`, o código abaixo usa `p_level`.

/// A expressão p_truth é sempre verdadeira porque é a cláusula WHERE de um
/// índice parcial que está dirigindo um laço de consulta. Percorre todos os
/// termos da cláusula WHERE da consulta e, se algum desses termos deve ser
/// verdadeiro porque p_truth é verdadeiro, marca esses termos da cláusula WHERE
/// como codificados.
fn where_apply_partial_index_constraints(
    p_truth: &Option<Box<Expr>>,
    i_tab_cur: i32,
    p_wc: &WhereClause,
) {
    let mut p_truth = p_truth.as_deref().expect("where_apply_partial_index_constraints: expressão nula");
    while p_truth.op == TK_AND {
        where_apply_partial_index_constraints(&p_truth.p_left, i_tab_cur, p_wc);
        p_truth = p_truth.p_right.as_deref().expect("TK_AND sem lado direito");
    }
    for i in 0..p_wc.n_term as usize {
        let p_term = &p_wc.a[i];
        if (p_term.borrow().wt_flags & TERM_CODED) != 0 {
            continue;
        }
        let p_expr_ref = p_term.borrow().p_expr.clone().expect("termo sem expressão");
        if expr_compare(None, &p_expr_ref.borrow(), p_truth, i_tab_cur) == 0 {
            p_term.borrow_mut().wt_flags |= TERM_CODED;
        }
    }
}

/// Esta rotina é chamada logo após um OP_Filter ter sido gerado e antes da
/// busca em índice correspondente ser realizada. Esta rotina verifica se há
/// filtros Bloom adicionais em laços internos que possam ser verificados antes
/// da busca em índice. Se houver filtros Bloom de laço interno disponíveis,
/// avalia esses filtros agora, antes da busca em índice. A ideia é que uma
/// verificação de filtro Bloom é muito mais rápida que uma busca em índice, e
/// o filtro Bloom pode retornar falso, o que significa que a busca em índice
/// pode ser pulada.
///
/// Sabemos que um laço interno usa um filtro Bloom porque tem o
/// WhereLevel.reg_filter definido. Se um filtro Bloom de laço interno for
/// verificado, então limpa o valor WhereLevel.reg_filter para evitar que o
/// filtro Bloom seja verificado uma segunda vez quando o laço interno for
/// avaliado.
fn filter_pull_down(
    p_parse: &ParseRef,
    p_w_info: &mut WhereInfo,
    i_level: i32,
    addr_nxt: i32,
    not_ready: Bitmask,
) {
    let mut i_level = i_level;
    loop {
        i_level += 1;
        if i_level >= p_w_info.n_level as i32 {
            break;
        }
        let p_level = &mut p_w_info.a[i_level as usize];
        let p_loop_ref = p_level.p_w_loop.clone().expect("filter_pull_down: nível sem WhereLoop");
        if p_level.reg_filter == 0 {
            continue;
        }
        if p_loop_ref.borrow().n_skip != 0 {
            continue;
        }
        //         ,--- Porque sqlite3ConstructBloomFilter() não terá definido
        //  vvvvv--'    p_level.reg_filter se isto fosse verdade.
        if (p_loop_ref.borrow().prereq & not_ready) != 0 {
            continue;
        }
        debug_assert!(p_level.addr_brk == 0);
        p_level.addr_brk = addr_nxt;
        let ws_flags = p_loop_ref.borrow().ws_flags;
        let v = p_parse.borrow().p_vdbe.clone().expect("filter_pull_down: Parse sem Vdbe");
        if (ws_flags & WHERE_IPK) != 0 {
            let p_term = p_loop_ref.borrow().a_l_term[0].clone().expect("a_l_term[0] nulo");
            debug_assert!(p_term.borrow().p_expr.is_some());
            let reg_rowid = get_temp_reg(&mut p_parse.borrow_mut());
            let reg_rowid = code_equality_term(p_parse, &p_term, p_level, 0, 0, reg_rowid);
            let mut vb = v.borrow_mut();
            vdbe_add_op2(&mut vb, OP_MUSTBEINT as i32, reg_rowid, addr_nxt);
            vdbe_add_op4_int(&mut vb, OP_FILTER as i32, p_level.reg_filter, addr_nxt, reg_rowid, 1);
        } else {
            let n_eq = match &p_loop_ref.borrow().u {
                WhereLoopUnion::Btree { n_eq, .. } => *n_eq as i32,
                _ => unreachable!("filter_pull_down: WhereLoop sem a parte btree"),
            };
            let mut z_start_aff: Option<Vec<u8>> = None;
            debug_assert!((ws_flags & WHERE_INDEXED) != 0);
            debug_assert!((ws_flags & WHERE_COLUMN_IN) == 0);
            let r1 = code_all_equality_terms(p_parse, p_level, 0, 0, &mut z_start_aff);
            code_apply_affinity(p_parse, r1, n_eq, z_start_aff.as_deref());
            vdbe_add_op4_int(&mut v.borrow_mut(), OP_FILTER as i32, p_level.reg_filter, addr_nxt, r1, n_eq);
        }
        p_level.reg_filter = 0;
        p_level.addr_brk = 0;
    }
}

/// O laço p_loop é um nível WHERE_INDEXED que usa pelo menos um operador IN(...).
/// Retorna verdadeiro se o nível p_loop é garantido visitar apenas uma linha para
/// cada chave gerada para o índice.
fn where_loop_is_one_row(p_loop: &WhereLoop) -> i32 {
    let (n_eq, p_index_ref) = match &p_loop.u {
        WhereLoopUnion::Btree { n_eq, p_index, .. } => (*n_eq, p_index.clone()),
        _ => unreachable!("where_loop_is_one_row: WhereLoop sem a parte btree"),
    };
    let p_index = p_index_ref.expect("where_loop_is_one_row: sem índice");
    let p_index = p_index.borrow();
    if p_index.on_error != 0 && p_loop.n_skip == 0 && n_eq == p_index.n_key_col {
        for ii in 0..n_eq as usize {
            let p_term = p_loop.a_l_term[ii].as_ref().expect("a_l_term nulo");
            if (p_term.borrow().e_operator & (WO_IS | WO_ISNULL)) != 0 {
                return 0;
            }
        }
        return 1;
    }
    0
}

/// Gera código para o início do laço i_level na implementação da cláusula WHERE
/// descrita por p_w_info.
///
/// FRAGMENTO: esta função continua nas partes 004, 005 e 006.
#[allow(unused_variables, unused_assignments, unused_mut)]
pub fn where_code_one_loop_start(
    p_parse: &ParseRef,   // Contexto de análise
    v: &VdbeRef,          // Instrução preparada em construção
    p_w_info: &mut WhereInfo, // Informação completa sobre a cláusula WHERE
    i_level: i32,         // Qual nível de p_w_info.a[] deve ser codificado
    p_level: &mut WhereLevel, // O ponteiro do nível atual
    not_ready: Bitmask,   // Quais tabelas estão disponíveis agora
) -> Bitmask {
    let mut j: i32; // Contadores de laço
    let mut k: i32;
    let i_cur: i32; // O cursor VDBE da tabela
    let mut addr_nxt: i32; // Para onde saltar para continuar com o próximo caso IN
    let b_rev: i32; // Verdadeiro se precisamos varrer em ordem reversa
    let p_loop: WhereLoopRef = p_level.p_w_loop.clone().expect("where_code_one_loop_start: nível sem WhereLoop");
    let mut p_term: Option<WhereTermRef>; // Um termo da cláusula WHERE
    let db: Sqlite3Ref = p_parse.borrow().db.upgrade().expect("where_code_one_loop_start: Parse sem db");
    let p_tab_list_rc = p_w_info.p_tab_list.clone();
    let p_tab_list_guard = p_tab_list_rc.borrow();
    let p_tab_item: &SrcItem = &p_tab_list_guard.a[p_level.i_from as usize]; // Termo FROM sendo codificado
    let addr_brk: i32; // Salta aqui para sair do laço
    let addr_halt: i32; // addr_brk do laço mais externo
    let addr_cont: i32; // Salta aqui para continuar com o próximo ciclo
    let mut i_rowid_reg: i32 = 0; // O rowid fica neste registrador, se não zero
    let mut i_release_reg: i32 = 0; // Registrador temporário a liberar antes de retornar
    let mut p_idx: Option<IndexRef> = None; // Índice usado pelo laço (se houver)

    i_cur = p_tab_item.i_cursor;
    p_level.not_ready = not_ready & !where_get_mask(&p_w_info.s_mask_set, i_cur);
    b_rev = ((p_w_info.rev_mask >> i_level) & 1) as i32;

    // Cria rótulos para as instruções "break" e "continue" do laço atual.
    // Salta para addr_brk para sair de um laço. Salta para cont para ir direto
    // à próxima iteração do laço.
    //
    // Quando há um operador IN, também temos um rótulo "addr_nxt" que
    // significa continuar com a próxima combinação de valores IN. Quando
    // não há operadores IN nas restrições, o rótulo "addr_nxt" é o mesmo
    // que "addr_brk".
    addr_brk = vdbe_make_label(&mut p_parse.borrow_mut());
    p_level.addr_brk = addr_brk;
    p_level.addr_nxt = addr_brk;
    addr_cont = vdbe_make_label(&mut p_parse.borrow_mut());
    p_level.addr_cont = addr_cont;

    // Se esta é a tabela direita de um LEFT OUTER JOIN, aloca e inicializa
    // uma célula de memória que registra se esta tabela casa com alguma
    // linha da tabela esquerda do join.
    debug_assert!(
        ((p_w_info.wctrl_flags as u32) & (WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN)) != 0
            || p_level.i_from > 0
            || (p_tab_item.fg.jointype & JT_LEFT) == 0
    );
    if p_level.i_from > 0 && (p_tab_item.fg.jointype & JT_LEFT) != 0 {
        let i_left_join = {
            let mut pm = p_parse.borrow_mut();
            pm.n_mem += 1;
            pm.n_mem
        };
        p_level.i_left_join = i_left_join;
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_level.i_left_join);
    }

    // Calcula um endereço seguro para saltar se descobrirmos que a tabela deste
    // laço está vazia e nunca pode contribuir conteúdo.
    j = i_level;
    while j > 0 {
        // p_w_info.a[i_level] é o próprio p_level (ver o aviso no topo do arquivo).
        let (has_left_join, has_rj) = if j == i_level {
            (p_level.i_left_join != 0, p_level.p_rj.is_some())
        } else {
            (p_w_info.a[j as usize].i_left_join != 0, p_w_info.a[j as usize].p_rj.is_some())
        };
        if has_left_join {
            break;
        }
        if has_rj {
            break;
        }
        j -= 1;
    }
    addr_halt = if j == i_level { p_level.addr_brk } else { p_w_info.a[j as usize].addr_brk };

    // Caso especial de uma subconsulta da cláusula FROM implementada como co-rotina
    if p_tab_item.fg.via_coroutine != 0 {
        let reg_yield = p_tab_item.reg_return;
        let mut vb = v.borrow_mut();
        vdbe_add_op3(&mut vb, OP_INITCOROUTINE as i32, reg_yield, 0, p_tab_item.addr_fill_sub);
        p_level.p2 = vdbe_add_op2(&mut vb, OP_YIELD as i32, reg_yield, addr_brk);
        p_level.op = OP_GOTO;
    } else if (p_loop.borrow().ws_flags & WHERE_VIRTUALTABLE) != 0 {
        // Caso 1: A tabela é uma tabela virtual. Usa VFilter e VNext para
        //         acessar os dados.
        let n_constraint = p_loop.borrow().n_l_term as i32;
        let (idx_num, m_handle_in, omit_mask, b_omit_offset) = match &p_loop.borrow().u {
            WhereLoopUnion::Vtab { idx_num, m_handle_in, omit_mask, b_omit_offset, .. } => {
                (*idx_num, *m_handle_in, *omit_mask, *b_omit_offset)
            }
            _ => unreachable!("where_code_one_loop_start: WhereLoop sem a parte vtab"),
        };

        let i_reg = get_temp_range(&mut p_parse.borrow_mut(), n_constraint + 2); // Valor de P3 do OP_VFilter
        let mut addr_not_found = p_level.addr_brk;
        for j in 0..n_constraint {
            let i_target = i_reg + j + 2;
            let p_term_j = match p_loop.borrow().a_l_term[j as usize].clone() {
                Some(t) => t,
                None => continue,
            };
            let (e_operator, e_match_op) = {
                let t = p_term_j.borrow();
                (t.e_operator, t.e_match_op)
            };
            let p_term_expr: ExprRef = p_term_j.borrow().p_expr.clone().expect("termo sem expressão");
            if (e_operator & WO_IN) != 0 {
                if (smaskbit32(j as u32) & m_handle_in) != 0 {
                    let (i_tab, i_cache) = {
                        let mut pm = p_parse.borrow_mut();
                        let i_tab = pm.n_tab;
                        pm.n_tab += 1;
                        pm.n_mem += 1;
                        (i_tab, pm.n_mem)
                    };
                    code_rhs_of_in(p_parse, &mut p_term_expr.borrow_mut(), i_tab);
                    vdbe_add_op3(&mut v.borrow_mut(), OP_VINITIN as i32, i_tab, i_target, i_cache);
                } else {
                    code_equality_term(p_parse, &p_term_j, p_level, j, b_rev, i_target);
                    addr_not_found = p_level.addr_nxt;
                }
            } else {
                {
                    let mut te = p_term_expr.borrow_mut();
                    code_expr_or_vector(p_parse, te.p_right.as_deref_mut(), i_target, 1);
                }
                if e_match_op as i32 == SQLITE_INDEX_CONSTRAINT_OFFSET && b_omit_offset {
                    debug_assert!(e_operator == WO_AUX);
                    let p_select = p_w_info.p_select.clone().expect("where_code_one_loop_start: sem SELECT");
                    debug_assert!(p_select.borrow().i_offset > 0);
                    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, p_select.borrow().i_offset);
                }
            }
        }
        // O P4 do OP_VFilter é o idxStr: se u.vtab.needFree, o opcode passa a ser o dono do
        // texto (P4_DYNAMIC), senão ele é estático. O texto continua no WhereLoop (cópia).
        let (z_idx_str, need_free) = match &p_loop.borrow().u {
            WhereLoopUnion::Vtab { idx_str, need_free, .. } => (idx_str.clone(), *need_free),
            _ => unreachable!("where_code_one_loop_start: WhereLoop sem a parte vtab"),
        };
        let (p4, p4_type) = if z_idx_str.is_empty() {
            (P4Value::NotUsed, P4_STATIC)
        } else if need_free {
            (P4Value::Dynamic(z_idx_str), P4_DYNAMIC)
        } else {
            (P4Value::Static(z_idx_str), P4_STATIC)
        };
        {
            let mut vb = v.borrow_mut();
            vdbe_add_op2(&mut vb, OP_INTEGER as i32, idx_num, i_reg);
            vdbe_add_op2(&mut vb, OP_INTEGER as i32, n_constraint, i_reg + 1);
            vdbe_add_op4(&mut vb, OP_VFILTER as i32, i_cur, addr_not_found, i_reg, p4, p4_type);
        }
        {
            let mut l = p_loop.borrow_mut();
            if let WhereLoopUnion::Vtab { need_free, idx_str, .. } = &mut l.u {
                *need_free = false;
                // Um OOM dentro do AddOp4(OP_VFilter) acima pode ter liberado u.vtab.idxStr.
                // Zera para evitar uso depois de liberar.
                if db.borrow().malloc_failed != 0 {
                    idx_str.clear();
                }
            }
        }
        p_level.p1 = i_cur;
        p_level.op = if p_w_info.e_one_pass != 0 { OP_NOOP } else { OP_VNEXT };
        p_level.p2 = vdbe_current_addr(&v.borrow());
        debug_assert!((p_loop.borrow().ws_flags & WHERE_MULTI_OR) == 0);

        for j in 0..n_constraint {
            let p_term_j = p_loop.borrow().a_l_term[j as usize].clone().expect("a_l_term nulo");
            if j < 16 && ((omit_mask >> j) & 1) != 0 {
                disable_term(p_level, &p_term_j);
                continue;
            }
            let e_operator = p_term_j.borrow().e_operator;
            if (e_operator & WO_IN) != 0
                && (smaskbit32(j as u32) & m_handle_in) == 0
                && db.borrow().malloc_failed == 0
            {
                // Recarrega o valor da restrição em reg[i_reg+j+2]. O mesmo valor foi
                // carregado no mesmo registrador antes do OP_VFilter, mas a implementação
                // de xFilter pode ter mudado o tipo ou a codificação do valor no
                // registrador, então ele *precisa* ser recarregado.
                let n_in = match &p_level.u {
                    WhereLevelUnion::In { n_in, .. } => *n_in,
                    _ => 0,
                };
                for i_in in 0..n_in {
                    let addr_in_top = match &p_level.u {
                        WhereLevelUnion::In { a_in_loop, .. } => a_in_loop[i_in as usize].addr_in_top,
                        _ => unreachable!(),
                    };
                    let mut vb = v.borrow_mut();
                    let (opcode, op_p1, op_p2, op_p3) = {
                        let p_op = vdbe_get_op(&mut vb, addr_in_top);
                        (p_op.opcode, p_op.p1, p_op.p2, p_op.p3)
                    };
                    if (opcode == OP_COLUMN && op_p3 == i_reg + j + 2)
                        || (opcode == OP_ROWID && op_p2 == i_reg + j + 2)
                    {
                        vdbe_add_op3(&mut vb, opcode as i32, op_p1, op_p2, op_p3);
                        break;
                    }
                }

                // Gera código que continua para a próxima linha se a restrição IN
                // não for satisfeita.
                let mut p_compare: Option<Box<Expr>> = p_expr(&mut p_parse.borrow_mut(), TK_EQ as i32, None, None);
                if db.borrow().malloc_failed == 0 {
                    let i_fld = match &p_term_j.borrow().u {
                        WhereTermUnion::X { i_field, .. } => *i_field,
                        _ => 0,
                    };
                    let p_term_expr: ExprRef = p_term_j.borrow().p_expr.clone().expect("termo sem expressão");
                    let mut te = p_term_expr.borrow_mut();
                    debug_assert!(te.p_left.is_some());
                    let cmp = p_compare.as_deref_mut().expect("p_compare nulo");
                    // No C, p_compare->pLeft apenas aponta para a subárvore do termo (sem posse) e
                    // é zerado antes do sqlite3ExprDelete(). Aqui a subárvore é movida para
                    // p_compare e devolvida ao dono depois de gerar o código, preservando a
                    // identidade do nó.
                    if i_fld > 0 {
                        let p_left = te.p_left.as_mut().expect("termo sem lado esquerdo");
                        debug_assert!(p_left.op == TK_VECTOR);
                        debug_assert!(expr_use_x_list(p_left));
                        let p_list = p_left.x.p_list.as_mut().expect("vetor sem lista");
                        debug_assert!(i_fld <= p_list.n_expr);
                        cmp.p_left = p_list.a[(i_fld - 1) as usize].p_expr.take();
                    } else {
                        cmp.p_left = te.p_left.take();
                    }
                    cmp.p_right = expr(&db.borrow(), TK_REGISTER as i32, None);
                    if let Some(p_right) = cmp.p_right.as_mut() {
                        p_right.i_table = i_reg + j + 2;
                        expr_if_false(&mut p_parse.borrow_mut(), cmp, p_level.addr_cont, SQLITE_JUMPIFNULL as i32);
                    }
                    if i_fld > 0 {
                        let p_left = te.p_left.as_mut().expect("termo sem lado esquerdo");
                        let p_list = p_left.x.p_list.as_mut().expect("vetor sem lista");
                        p_list.a[(i_fld - 1) as usize].p_expr = cmp.p_left.take();
                    } else {
                        te.p_left = cmp.p_left.take();
                    }
                }
                expr_delete(&db.borrow(), p_compare);
            }
        }

        // Estes registradores precisam ser preservados caso haja um laço de operador IN.
        // Poderíamos desalocar os registradores aqui (e talvez reusá-los depois) se
        // (p_loop.ws_flags & WHERE_IN_ABLE)==0. Mas parece mais simples e seguro
        // simplesmente não reusar os registradores.
        //
        //    release_temp_range(p_parse, i_reg, n_constraint+2);
    } else if (p_loop.borrow().ws_flags & WHERE_IPK) != 0
        && (p_loop.borrow().ws_flags & (WHERE_COLUMN_IN | WHERE_COLUMN_EQ)) != 0
    {
        // Caso 2: Podemos referenciar diretamente uma única linha usando uma
        //         comparação de igualdade com o campo ROWID. Ou
        //         referenciamos várias linhas usando uma construção
        //         "rowid IN (...)".
        debug_assert!(match &p_loop.borrow().u {
            WhereLoopUnion::Btree { n_eq, .. } => *n_eq == 1,
            _ => false,
        });
        p_term = p_loop.borrow().a_l_term[0].clone();
        debug_assert!(p_term.is_some());
        debug_assert!(p_term.as_ref().unwrap().borrow().p_expr.is_some());
        // (continua na parte 004)


// ---- part_004.rs ----
// FRAGMENTO de corpo de função (não é um arquivo de itens).
//
// O trecho C wherecode_c.004.c é o MEIO de `sqlite3WhereCodeOneLoopStart`
// (where_code_one_loop_start), cortado dentro do ramo "Case 2" da cadeia
// if/else-if. As partes 004, 005 e 006 são três pedaços CONSECUTIVOS desse
// mesmo corpo; o integrador cola os três, nessa ordem, logo depois do ramo
// "Case 2" aberto no fim da parte 003 (a parte 003 NÃO deve fechar a função nem
// o if: o fechamento fica no fim da parte 006, que termina com `return`).
//
// Contrato de variáveis locais, declaradas pela parte 003 no início da função:
//   p_parse: ParseRef, v: VdbeRef, p_w_info: &mut WhereInfo (no C, pWInfo),
//   p_level: &mut WhereLevel, p_loop: WhereLoopRef, p_tab_item: SrcItem,
//   db, i_cur, i_level, not_ready, b_rev: i32 (0 ou 1), addr_brk, addr_cont,
//   addr_halt: i32, e os locais mutáveis do cabeçalho do C:
//   j: i32, k: i32, p_term: Option<WhereTermRef>, p_idx: Option<IndexRef>,
//   i_rowid_reg: i32, i_release_reg: i32, addr_nxt: i32.
// O wsFlags do laço é lido por `p_loop.borrow().ws_flags`.
// VdbeComment, VdbeCoverage e testcase não geram nada no build do Debian
// (sem SQLITE_DEBUG nem SQLITE_ENABLE_EXPLAIN_COMMENTS), por isso não aparecem.
// assert() do C vira debug_assert!() (o build do Debian não os compila).

        i_release_reg = {
            let mut pm = p_parse.borrow_mut();
            pm.n_mem += 1;
            pm.n_mem
        };
        i_rowid_reg = code_equality_term(
            &p_parse,
            &p_term.as_ref().unwrap().borrow(),
            p_level,
            0,
            b_rev,
            i_release_reg,
        );
        if i_rowid_reg != i_release_reg {
            release_temp_reg(&mut p_parse.borrow_mut(), i_release_reg);
        }
        addr_nxt = p_level.addr_nxt;
        if p_level.reg_filter != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_MUSTBEINT as i32, i_rowid_reg, addr_nxt);
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_FILTER as i32,
                p_level.reg_filter,
                addr_nxt,
                i_rowid_reg,
                1,
            );
            filter_pull_down(&p_parse, p_w_info, i_level, addr_nxt, not_ready);
        }
        vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKROWID as i32, i_cur, addr_nxt, i_rowid_reg);
        p_level.op = OP_NOOP;
    } else if (p_loop.borrow().ws_flags & WHERE_IPK) != 0
        && (p_loop.borrow().ws_flags & WHERE_COLUMN_RANGE) != 0
    {
        // Caso 3: temos uma comparação de desigualdade contra o campo ROWID.
        let mut test_op: u8 = OP_NOOP;
        let start: i32;
        let mut mem_end_value: i32 = 0;
        let mut p_start: Option<WhereTermRef>;
        let mut p_end: Option<WhereTermRef>;

        j = 0;
        p_start = None;
        p_end = None;
        if (p_loop.borrow().ws_flags & WHERE_BTM_LIMIT) != 0 {
            p_start = p_loop.borrow().a_l_term[j as usize].clone();
            j += 1;
        }
        if (p_loop.borrow().ws_flags & WHERE_TOP_LIMIT) != 0 {
            p_end = p_loop.borrow().a_l_term[j as usize].clone();
            j += 1;
        }
        debug_assert!(p_start.is_some() || p_end.is_some());
        if b_rev != 0 {
            p_term = p_start.clone();
            p_start = p_end.clone();
            p_end = p_term.clone();
        }
        code_cursor_hint(&p_tab_item, p_w_info, p_level, p_end.as_ref());
        if let Some(start_term) = p_start.clone() {
            let r1: i32;
            let mut r_temp: i32 = 0;
            let op: u8;

            // A tabela abaixo mapeia códigos TK_xx nos opcodes de busca
            // correspondentes. Depende de uma ordenação particular dos TK_xx.
            const A_MOVE_OP: [u8; 4] = [
                OP_SEEKGT, // TK_GT
                OP_SEEKLE, // TK_LE
                OP_SEEKLT, // TK_LT
                OP_SEEKGE, // TK_GE
            ];
            debug_assert!(TK_LE == TK_GT + 1);
            debug_assert!(TK_LT == TK_GT + 2);
            debug_assert!(TK_GE == TK_GT + 3);

            debug_assert!((start_term.borrow().wt_flags & TERM_VNULL) == 0);
            let p_x_ref = start_term.borrow().p_expr.clone().unwrap();
            let p_x = p_x_ref.borrow();
            if expr_is_vector(p_x.p_right.as_deref().unwrap()) {
                r_temp = get_temp_reg(&mut p_parse.borrow_mut());
                r1 = r_temp;
                code_expr_or_vector(&p_parse, p_x.p_right.as_deref(), r1, 1);
                op = A_MOVE_OP[((((p_x.op as i32) - (TK_GT as i32) - 1) & 0x3) | 0x1) as usize];
                debug_assert!(p_x.op != TK_GT || op == OP_SEEKGE);
                debug_assert!(p_x.op != TK_GE || op == OP_SEEKGE);
                debug_assert!(p_x.op != TK_LT || op == OP_SEEKLE);
                debug_assert!(p_x.op != TK_LE || op == OP_SEEKLE);
            } else {
                r1 = expr_code_temp(&mut p_parse.borrow_mut(), p_x.p_right.as_deref(), &mut r_temp);
                disable_term(p_level, &start_term.borrow());
                op = A_MOVE_OP[((p_x.op as i32) - (TK_GT as i32)) as usize];
            }
            vdbe_add_op3(&mut v.borrow_mut(), op as i32, i_cur, addr_brk, r1);
            release_temp_reg(&mut p_parse.borrow_mut(), r_temp);
        } else {
            vdbe_add_op2(
                &mut v.borrow_mut(),
                (if b_rev != 0 { OP_LAST } else { OP_REWIND }) as i32,
                i_cur,
                addr_halt,
            );
        }
        if let Some(end_term) = p_end.clone() {
            let p_x_ref = end_term.borrow().p_expr.clone().unwrap();
            let p_x = p_x_ref.borrow();
            debug_assert!((end_term.borrow().wt_flags & TERM_VNULL) == 0);
            mem_end_value = {
                let mut pm = p_parse.borrow_mut();
                pm.n_mem += 1;
                pm.n_mem
            };
            code_expr_or_vector(&p_parse, p_x.p_right.as_deref(), mem_end_value, 1);
            if !expr_is_vector(p_x.p_right.as_deref().unwrap())
                && (p_x.op == TK_LT || p_x.op == TK_GT)
            {
                test_op = if b_rev != 0 { OP_LE } else { OP_GE };
            } else {
                test_op = if b_rev != 0 { OP_LT } else { OP_GT };
            }
            if !expr_is_vector(p_x.p_right.as_deref().unwrap()) {
                disable_term(p_level, &end_term.borrow());
            }
        }
        start = vdbe_current_addr(&v.borrow());
        p_level.op = if b_rev != 0 { OP_PREV } else { OP_NEXT };
        p_level.p1 = i_cur;
        p_level.p2 = start;
        debug_assert!(p_level.p5 == 0);
        if test_op != OP_NOOP {
            i_rowid_reg = {
                let mut pm = p_parse.borrow_mut();
                pm.n_mem += 1;
                pm.n_mem
            };
            vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, i_cur, i_rowid_reg);
            vdbe_add_op3(&mut v.borrow_mut(), test_op as i32, mem_end_value, addr_brk, i_rowid_reg);
            vdbe_change_p5(&mut v.borrow_mut(), (SQLITE_AFF_NUMERIC | SQLITE_JUMPIFNULL) as u16);
        }
    } else if (p_loop.borrow().ws_flags & WHERE_INDEXED) != 0 {
        // Caso 4: uma varredura usando um índice.
        //
        // A cláusula WHERE pode conter zero ou mais termos de igualdade ("=="
        // ou "IN") que se referem às N colunas mais à esquerda do índice. Pode
        // conter também restrições de desigualdade (>, <, >= ou <=) na coluna
        // do índice que vem logo depois das N igualdades. Só a coluna mais à
        // direita pode ser desigualdade; as demais usam "==" e "IN". Por
        // exemplo, se o índice é em (x,y,z), todas as cláusulas abaixo são
        // otimizadas:
        //
        //            x=5
        //            x=5 AND y=10
        //            x=5 AND y<10
        //            x=5 AND y>5 AND y<10
        //            x=5 AND y=5 AND z<=10
        //
        // O termo z<10 da cláusula seguinte não pode ser usado, só o x=5:
        //
        //            x=5 AND z<10
        //
        // N pode ser zero se há restrições de desigualdade. Se não há
        // restrições de desigualdade, N é pelo menos um.
        //
        // Este caso também é usado quando não há restrições na cláusula WHERE
        // mas um índice é escolhido mesmo assim, para forçar a ordem de saída
        // a obedecer um ORDER BY.
        const A_START_OP: [u8; 8] = [
            0,
            0,
            OP_REWIND,  // 2: (!start_constraints && startEq &&  !bRev)
            OP_LAST,    // 3: (!start_constraints && startEq &&   bRev)
            OP_SEEKGT, // 4: (start_constraints  && !startEq && !bRev)
            OP_SEEKLT, // 5: (start_constraints  && !startEq &&  bRev)
            OP_SEEKGE, // 6: (start_constraints  &&  startEq && !bRev)
            OP_SEEKLE, // 7: (start_constraints  &&  startEq &&  bRev)
        ];
        const A_END_OP: [u8; 4] = [
            OP_IDXGE, // 0: (end_constraints && !bRev && !endEq)
            OP_IDXGT, // 1: (end_constraints && !bRev &&  endEq)
            OP_IDXLE, // 2: (end_constraints &&  bRev && !endEq)
            OP_IDXLT, // 3: (end_constraints &&  bRev &&  endEq)
        ];
        let (n_eq, mut n_btm, mut n_top) = match &p_loop.borrow().u {
            WhereLoopUnion::Btree { n_eq, n_btm, n_top, .. } => (*n_eq, *n_btm, *n_top),
            _ => unreachable!(),
        };
        let reg_base: i32; // Registrador base com os valores das restrições
        let mut p_range_start: Option<WhereTermRef> = None; // Desigualdade no início do intervalo
        let mut p_range_end: Option<WhereTermRef> = None; // Desigualdade no fim do intervalo
        let mut start_eq: i32; // Verdadeiro se o início usa ==, >= ou <=
        let mut end_eq: i32; // Verdadeiro se o fim usa ==, >= ou <=
        let mut start_constraints: i32; // O início do intervalo é restrito
        let mut n_constraint: i32; // Número de termos de restrição
        let i_idx_cur: i32; // Cursor VDBE do índice
        let mut n_extra_reg: i32 = 0; // Registradores extras necessários
        let mut op: i32; // Opcode da instrução
        let mut z_start_aff: Option<Vec<u8>> = None; // Afinidade do início da restrição
        let mut z_end_aff: Option<Vec<u8>> = None; // Afinidade do fim da restrição
        let mut b_seek_past_null: u8 = 0; // Verdadeiro para pular os NULLs iniciais
        let mut b_stop_at_null: u8 = 0; // Acrescenta condição para terminar nos NULLs
        let omit_table: bool; // Verdadeiro se usamos só o índice
        let mut reg_bignull: i32 = 0; // Registrador do flag "big-null"
        let mut addr_seek_scan: i32 = 0; // Endereço do OP_SeekScan, se houver

        p_idx = match &p_loop.borrow().u {
            WhereLoopUnion::Btree { p_index, .. } => p_index.clone(),
            _ => None,
        };
        let p_idx_rc = p_idx.clone().unwrap();
        i_idx_cur = p_level.i_idx_cur;
        debug_assert!(n_eq >= p_loop.borrow().n_skip);

        // Acha os termos de desigualdade do início e do fim do intervalo.
        j = n_eq as i32;
        if (p_loop.borrow().ws_flags & WHERE_BTM_LIMIT) != 0 {
            p_range_start = p_loop.borrow().a_l_term[j as usize].clone();
            j += 1;
            n_extra_reg = n_extra_reg.max(match &p_loop.borrow().u {
                WhereLoopUnion::Btree { n_btm, .. } => *n_btm as i32,
                _ => 0,
            });
            // As restrições do otimizador de LIKE sempre ocorrem em pares.
            debug_assert!(
                (p_range_start.as_ref().unwrap().borrow().wt_flags & TERM_LIKEOPT) == 0
                    || (p_loop.borrow().ws_flags & WHERE_TOP_LIMIT) != 0
            );
        }
        if (p_loop.borrow().ws_flags & WHERE_TOP_LIMIT) != 0 {
            p_range_end = p_loop.borrow().a_l_term[j as usize].clone();
            j += 1;
            n_extra_reg = n_extra_reg.max(match &p_loop.borrow().u {
                WhereLoopUnion::Btree { n_top, .. } => *n_top as i32,
                _ => 0,
            });
            // SQLITE_LIKE_DOESNT_MATCH_BLOBS não está definido no Debian.
            if (p_range_end.as_ref().unwrap().borrow().wt_flags & TERM_LIKEOPT) != 0 {
                debug_assert!(p_range_start.is_some()); // restrições LIKE
                debug_assert!((p_range_start.as_ref().unwrap().borrow().wt_flags & TERM_LIKEOPT) != 0); // ocorrem em pares
                p_level.i_like_rep_cntr = {
                    let mut pm = p_parse.borrow_mut();
                    pm.n_mem += 1;
                    pm.n_mem as u32
                };
                vdbe_add_op2(
                    &mut v.borrow_mut(),
                    OP_INTEGER as i32,
                    1,
                    p_level.i_like_rep_cntr as i32,
                );
                p_level.addr_like_rep = vdbe_current_addr(&v.borrow());
                // iLikeRepCntr guarda na verdade 2x o número do registrador. O
                // bit de baixo indica se a ordem de busca é ASC ou DESC.
                debug_assert!((b_rev & !1) == 0);
                p_level.i_like_rep_cntr <<= 1;
                p_level.i_like_rep_cntr |= (b_rev
                    ^ ((p_idx_rc.borrow().a_sort_order[n_eq as usize] == SQLITE_SO_DESC) as i32))
                    as u32;
            }
            if p_range_start.is_none() {
                j = p_idx_rc.borrow().ai_column[n_eq as usize] as i32;
                let not_null_zero = j >= 0 && {
                    let tab = p_idx_rc.borrow().p_table.upgrade().unwrap();
                    let r = tab.borrow().a_col[j as usize].not_null == 0;
                    r
                };
                if not_null_zero || j == XN_EXPR as i32 {
                    b_seek_past_null = 1;
                }
            }
        }
        debug_assert!(
            p_range_end.is_none()
                || (p_range_end.as_ref().unwrap().borrow().wt_flags & TERM_VNULL) == 0
        );

        // Se o flag WHERE_BIGNULL_SORT está ligado, a coluna nEq do índice usa
        // uma ordenação "big-null" não padrão (ASC NULLS LAST ou DESC NULLS
        // FIRST). Nos dois casos fazem-se varreduras ordenadas separadas das
        // entradas do índice em que a coluna é nula e das em que não é. Na
        // ordem ASC, as entradas não nulas são varridas primeiro. Na DESC, as
        // nulas vêm primeiro.
        if (p_loop.borrow().ws_flags & (WHERE_TOP_LIMIT | WHERE_BTM_LIMIT)) == 0
            && (p_loop.borrow().ws_flags & WHERE_BIGNULL_SORT) != 0
        {
            debug_assert!(b_seek_past_null == 0 && n_extra_reg == 0 && n_btm == 0 && n_top == 0);
            debug_assert!(p_range_end.is_none() && p_range_start.is_none());
            n_extra_reg = 1;
            b_seek_past_null = 1;
            reg_bignull = {
                let mut pm = p_parse.borrow_mut();
                pm.n_mem += 1;
                pm.n_mem
            };
            p_level.reg_bignull = reg_bignull;
            if p_level.i_left_join != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_bignull);
            }
            p_level.addr_bignull = vdbe_make_label(&mut p_parse.borrow_mut());
        }

        // Se fazemos uma varredura reversa num índice ascendente, ou direta num
        // descendente, troca os termos de início e fim (pRangeStart e pRangeEnd).
        if (n_eq < p_idx_rc.borrow().n_column)
            && b_rev == ((p_idx_rc.borrow().a_sort_order[n_eq as usize] == SQLITE_SO_ASC) as i32)
        {
            std::mem::swap(&mut p_range_end, &mut p_range_start);
            std::mem::swap(&mut b_seek_past_null, &mut b_stop_at_null);
            std::mem::swap(&mut n_btm, &mut n_top);
        }

        if i_level > 0 && (p_loop.borrow().ws_flags & WHERE_IN_SEEKSCAN) != 0 {
            // Caso OP_SeekScan seja usado, garante que o cursor do índice não
            // aponte para uma linha válida na primeira iteração deste laço.
            vdbe_add_op1(&mut v.borrow_mut(), OP_NULLROW as i32, i_idx_cur);
        }

        // Gera código para avaliar todos os termos de restrição com == ou IN e
        // guardar os valores num array de registradores a partir de regBase.
        code_cursor_hint(&p_tab_item, p_w_info, p_level, p_range_end.as_ref());
        reg_base = code_all_equality_terms(&p_parse, p_level, b_rev, n_extra_reg, &mut z_start_aff);
        debug_assert!(z_start_aff.is_none() || z_start_aff.as_ref().unwrap().len() >= n_eq as usize);
        if z_start_aff.is_some() && n_top != 0 {
            z_end_aff = Some(z_start_aff.as_ref().unwrap()[n_eq as usize..].to_vec());
        }
        addr_nxt = if reg_bignull != 0 { p_level.addr_bignull } else { p_level.addr_nxt };

        start_eq = (p_range_start.is_none()
            || (p_range_start.as_ref().unwrap().borrow().e_operator & (WO_LE | WO_GE)) != 0)
            as i32;
        end_eq = (p_range_end.is_none()
            || (p_range_end.as_ref().unwrap().borrow().e_operator & (WO_LE | WO_GE)) != 0)
            as i32;
        start_constraints = (p_range_start.is_some() || n_eq > 0) as i32;

        // Posiciona o cursor do índice no início do intervalo.
        n_constraint = n_eq as i32;
        if let Some(range_start) = p_range_start.clone() {
            let p_x_ref = range_start.borrow().p_expr.clone().unwrap();
            let p_x = p_x_ref.borrow();
            let p_right = p_x.p_right.as_deref();
            code_expr_or_vector(&p_parse, p_right, reg_base + n_eq as i32, n_btm as i32);
            where_like_optimization_string_fixup(&mut v.clone(), p_level, &range_start.borrow());
            if (range_start.borrow().wt_flags & TERM_VNULL) == 0 && expr_can_be_null(p_right.unwrap()) != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg_base + n_eq as i32, addr_nxt);
            }
            if let Some(aff) = z_start_aff.as_mut() {
                // updateRangeAffinityStr(pRight, nBtm, &zStartAff[nEq])
                let mut tail = aff[n_eq as usize..].to_vec();
                update_range_affinity_str(p_right.unwrap(), n_btm as i32, &mut tail);
                aff[n_eq as usize..].copy_from_slice(&tail);
            }
            n_constraint += n_btm as i32;
            if !expr_is_vector(p_right.unwrap()) {
                disable_term(p_level, &range_start.borrow());
            } else {
                start_eq = 1;
            }
            b_seek_past_null = 0;
        } else if b_seek_past_null != 0 {
            start_eq = 0;
            vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_base + n_eq as i32);
            start_constraints = 1;
            n_constraint += 1;
        } else if reg_bignull != 0 {
            vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_base + n_eq as i32);
            start_constraints = 1;
            n_constraint += 1;
        }
        code_apply_affinity(
            &p_parse,
            reg_base,
            n_constraint - b_seek_past_null as i32,
            z_start_aff.as_deref(),
        );
        if p_loop.borrow().n_skip > 0 && n_constraint == p_loop.borrow().n_skip as i32 {
            // A lógica de skip-scan dentro da chamada a codeAllEqualityTerms()
            // acima já deixou o cursor na linha correta, então nenhuma busca
            // adicional é necessária.
        } else {
            if reg_bignull != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, reg_bignull);
            }
            if p_level.reg_filter != 0 {
                vdbe_add_op4_int(
                    &mut v.borrow_mut(),
                    OP_FILTER as i32,
                    p_level.reg_filter,
                    addr_nxt,
                    reg_base,
                    n_eq as i32,
                );
                filter_pull_down(&p_parse, p_w_info, i_level, addr_nxt, not_ready);
            }

            op = A_START_OP[((start_constraints << 2) + (start_eq << 1) + b_rev) as usize] as i32;
            debug_assert!(op != 0);
            if (p_loop.borrow().ws_flags & WHERE_IN_SEEKSCAN) != 0 && op == OP_SEEKGE as i32 {
                debug_assert!(reg_bignull == 0);
                // TUNING: o opcode OP_SeekScan procura reduzir o número de
                // operações de busca caras substituindo uma única busca por


// ---- part_005.rs ----
// FRAGMENTO de corpo de função (não é um arquivo de itens). Continuação direta
// da parte 004; veja o contrato no cabeçalho dela. Este trecho entra no meio do
// comentário "TUNING" do Case 4 e termina dentro do Case 5 (OR múltiplo), logo
// depois do `if( pWC->nTerm>1 ){...}` que monta pAndExpr. O bloco do Case 5
// continua aberto na parte 006.

                // 1 ou mais operações de passo. A questão é: quantos passos
                // devemos tentar antes de desistir e fazer uma busca. O custo
                // de uma busca é proporcional ao logaritmo do número de
                // entradas da árvore, então basear o número de passos a tentar
                // no número estimado de linhas da btree parece um bom palpite.
                addr_seek_scan = vdbe_add_op1(
                    &mut v.borrow_mut(),
                    OP_SEEKSCAN as i32,
                    ((p_idx_rc.borrow().ai_row_log_est[0] as i32) + 9) / 10,
                );
                if p_range_start.is_some() || p_range_end.is_some() {
                    vdbe_change_p5(&mut v.borrow_mut(), 1);
                    let addr_next = vdbe_current_addr(&v.borrow()) + 1;
                    vdbe_change_p2(&mut v.borrow_mut(), addr_seek_scan, addr_next);
                    addr_seek_scan = 0;
                }
            }
            vdbe_add_op4_int(&mut v.borrow_mut(), op, i_idx_cur, addr_nxt, reg_base, n_constraint);

            debug_assert!(b_seek_past_null == 0 || b_stop_at_null == 0);
            if reg_bignull != 0 {
                debug_assert!(b_seek_past_null == 1 || b_stop_at_null == 1);
                debug_assert!(b_seek_past_null == (b_stop_at_null == 0) as u8);
                debug_assert!(b_stop_at_null as i32 == start_eq);
                let addr_skip_goto = vdbe_current_addr(&v.borrow()) + 2;
                vdbe_add_op2(&mut v.borrow_mut(), OP_GOTO as i32, 0, addr_skip_goto);
                op = A_START_OP[(((n_constraint > 1) as i32) * 4 + 2 + b_rev) as usize] as i32;
                vdbe_add_op4_int(
                    &mut v.borrow_mut(),
                    op,
                    i_idx_cur,
                    addr_nxt,
                    reg_base,
                    n_constraint - start_eq,
                );
                debug_assert!(
                    op == OP_REWIND as i32
                        || op == OP_LAST as i32
                        || op == OP_SEEKGE as i32
                        || op == OP_SEEKLE as i32
                );
            }
        }

        // Carrega o valor da restrição de desigualdade no fim do intervalo (se
        // houver).
        n_constraint = n_eq as i32;
        debug_assert!(p_level.p2 == 0);
        if let Some(range_end) = p_range_end.clone() {
            let p_x_ref = range_end.borrow().p_expr.clone().unwrap();
            let p_x = p_x_ref.borrow();
            let p_right = p_x.p_right.as_deref();
            debug_assert!(addr_seek_scan == 0);
            code_expr_or_vector(&p_parse, p_right, reg_base + n_eq as i32, n_top as i32);
            where_like_optimization_string_fixup(&mut v.clone(), p_level, &range_end.borrow());
            if (range_end.borrow().wt_flags & TERM_VNULL) == 0 && expr_can_be_null(p_right.unwrap()) != 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg_base + n_eq as i32, addr_nxt);
            }
            if let Some(end_aff) = z_end_aff.as_mut() {
                update_range_affinity_str(p_right.unwrap(), n_top as i32, end_aff);
                code_apply_affinity(&p_parse, reg_base + n_eq as i32, n_top as i32, Some(end_aff.as_slice()));
            } else {
                // No C: assert( pParse->db->mallocFailed ).
            }
            n_constraint += n_top as i32;

            if !expr_is_vector(p_right.unwrap()) {
                disable_term(p_level, &range_end.borrow());
            } else {
                end_eq = 1;
            }
        } else if b_stop_at_null != 0 {
            if reg_bignull == 0 {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_base + n_eq as i32);
                end_eq = 0;
            }
            n_constraint += 1;
        }
        // sqlite3DbNNFreeNN(zStartAff) e (zEndAff): liberados pelo drop.
        drop(z_start_aff);
        drop(z_end_aff);

        // Topo do corpo do laço.
        p_level.p2 = vdbe_current_addr(&v.borrow());

        // Verifica se o cursor do índice passou do fim do intervalo.
        if n_constraint != 0 {
            if reg_bignull != 0 {
                // Exceto: pula a verificação de fim de intervalo durante a
                // varredura de NULLs.
                let addr_if_not = vdbe_current_addr(&v.borrow()) + 3;
                vdbe_add_op2(&mut v.borrow_mut(), OP_IFNOT as i32, reg_bignull, addr_if_not);
            }
            op = A_END_OP[(b_rev * 2 + end_eq) as usize] as i32;
            vdbe_add_op4_int(&mut v.borrow_mut(), op, i_idx_cur, addr_nxt, reg_base, n_constraint);
            if addr_seek_scan != 0 {
                vdbe_jump_here(&mut v.borrow_mut(), addr_seek_scan);
            }
        }
        if reg_bignull != 0 {
            // Durante uma varredura de NULLs, verifica se chegamos ao fim dos
            // NULLs.
            debug_assert!(b_seek_past_null == (b_stop_at_null == 0) as u8);
            debug_assert!(b_seek_past_null + b_stop_at_null == 1);
            debug_assert!(n_constraint + (b_seek_past_null as i32) > 0);
            let addr_if = vdbe_current_addr(&v.borrow()) + 2;
            vdbe_add_op2(&mut v.borrow_mut(), OP_IF as i32, reg_bignull, addr_if);
            op = A_END_OP[(b_rev * 2 + b_seek_past_null as i32) as usize] as i32;
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                op,
                i_idx_cur,
                addr_nxt,
                reg_base,
                n_constraint + b_seek_past_null as i32,
            );
        }

        if (p_loop.borrow().ws_flags & WHERE_IN_EARLYOUT) != 0 {
            vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKHIT as i32, i_idx_cur, n_eq as i32, n_eq as i32);
        }

        // Posiciona o cursor da tabela, se necessário.
        omit_table = (p_loop.borrow().ws_flags & WHERE_IDX_ONLY) != 0
            && (p_w_info.wctrl_flags & ((WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN) as u16)) == 0;
        let idx_table = p_idx_rc.borrow().p_table.upgrade().unwrap();
        if omit_table {
            // pIdx é um índice de cobertura. Não é preciso acessar a tabela
            // principal.
        } else if has_rowid(&idx_table.borrow()) {
            code_deferred_seek(p_w_info, p_idx.as_ref(), i_cur, i_idx_cur);
        } else if i_cur != i_idx_cur {
            let p_pk = primary_key_index(&idx_table.borrow()).unwrap();
            let n_key_col = p_pk.borrow().n_key_col as i32;
            i_rowid_reg = get_temp_range(&mut p_parse.borrow_mut(), n_key_col);
            j = 0;
            while j < n_key_col {
                k = table_column_to_index(&p_idx_rc.borrow(), p_pk.borrow().ai_column[j as usize]) as i32;
                vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_idx_cur, k, i_rowid_reg + j);
                j += 1;
            }
            vdbe_add_op4_int(
                &mut v.borrow_mut(),
                OP_NOTFOUND as i32,
                i_cur,
                addr_cont,
                i_rowid_reg,
                n_key_col,
            );
        }

        if p_level.i_left_join == 0 {
            // Se um índice parcial dirige o laço, tenta eliminar da consulta os
            // termos da cláusula WHERE que precisam ser verdadeiros por causa
            // da cláusula WHERE do índice parcial.
            //
            // Ticket 623eff57e76d45f6, de 02/11/2019: esta otimização não
            // funciona para um LEFT JOIN.
            if p_idx_rc.borrow().p_part_idx_where.is_some() {
                where_apply_partial_index_constraints(
                    &p_idx_rc.borrow().p_part_idx_where,
                    i_cur,
                    &mut p_w_info.s_wc,
                );
            }
        } else {
            // O assert seguinte não é um requisito, é só uma observação: a
            // otimização de OR não funciona para a tabela do lado direito de um
            // LEFT JOIN.
            debug_assert!(
                (p_w_info.wctrl_flags & ((WHERE_OR_SUBCLAUSE | WHERE_RIGHT_JOIN) as u16)) == 0
            );
        }

        // Registra a instrução usada para terminar o laço.
        if (p_loop.borrow().ws_flags & WHERE_ONEROW) != 0
            || (match &p_level.u {
                WhereLevelUnion::In { n_in, .. } => *n_in != 0,
                _ => false,
            } && reg_bignull == 0
                && where_loop_is_one_row(&p_loop.borrow()) != 0)
        {
            p_level.op = OP_NOOP;
        } else if b_rev != 0 {
            p_level.op = OP_PREV;
        } else {
            p_level.op = OP_NEXT;
        }
        p_level.p1 = i_idx_cur;
        p_level.p3 = if (p_loop.borrow().ws_flags & WHERE_UNQ_WANTED) != 0 { 1 } else { 0 };
        if (p_loop.borrow().ws_flags & WHERE_CONSTRAINT) == 0 {
            p_level.p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        } else {
            debug_assert!(p_level.p5 == 0);
        }
        if omit_table {
            p_idx = None;
        }
    } else
    // SQLITE_OMIT_OR_OPTIMIZATION não está definido no Debian.
    if (p_loop.borrow().ws_flags & WHERE_MULTI_OR) != 0 {
        // Caso 5: dois ou mais termos indexados separadamente, ligados por OR.
        //
        // Exemplo:
        //
        //   CREATE TABLE t1(a,b,c,d);
        //   CREATE INDEX i1 ON t1(a);
        //   CREATE INDEX i2 ON t1(b);
        //   CREATE INDEX i3 ON t1(c);
        //
        //   SELECT * FROM t1 WHERE a=5 OR b=7 OR (c=11 AND d=13)
        //
        // No exemplo há três termos indexados ligados por OR. O topo do laço
        // fica assim:
        //
        //          Null       1                # Zera o rowset no reg 1
        //
        // Depois, para cada termo indexado, o seguinte. Os argumentos de
        // RowSetTest são tais que o rowid da linha atual é inserido no RowSet.
        // Se já estiver presente, o controle pula o Gosub e vai direto ao
        // código gerado por WhereEnd().
        //
        //        sqlite3WhereBegin(<term>)
        //          RowSetTest                  # Insere o rowid no rowset
        //          Gosub      2 A
        //        sqlite3WhereEnd()
        //
        // Depois disso, o código que termina o laço. O rótulo A, alvo do Gosub
        // acima, salta para a instrução logo após o Goto.
        //
        //          Null       1                # Zera o rowset no reg 1
        //          Goto       B                # O laço terminou.
        //
        //       A: <corpo do laço>             # Devolve dados, o que for.
        //
        //          Return     2                # Volta para o Gosub
        //
        //       B: <depois do laço>
        //
        // Acrescentado em 26/05/2014: se a tabela é WITHOUT ROWID, usa um
        // índice efêmero no lugar de um RowSet para registrar as chaves
        // primárias das linhas já vistas.
        let p_or_wc_a: Vec<WhereTermRef>; // Termos do OR (pOrWc->a)
        let p_or_wc_n_term: i32; // pOrWc->nTerm
        let p_or_tab: SrcListRef; // Lista de tabelas encurtada para gerar o OR
        let mut p_cov: Option<IndexRef> = None; // Possível índice de cobertura
        let i_cov_cur: i32 = {
            let mut pm = p_parse.borrow_mut();
            let r = pm.n_tab;
            pm.n_tab += 1;
            r
        }; // Cursor usado nas varreduras de índice (se houver)

        let reg_return: i32 = {
            let mut pm = p_parse.borrow_mut();
            pm.n_mem += 1;
            pm.n_mem
        }; // Registrador usado com OP_Gosub
        let mut reg_rowset: i32 = 0; // Registrador do objeto RowSet
        let mut reg_rowid: i32 = 0; // Registrador com o rowid
        let i_loop_body: i32 = vdbe_make_label(&mut p_parse.borrow_mut()); // Início do corpo do laço
        let i_ret_init: i32; // Endereço da inicialização de regReturn
        let mut untested_terms: bool = false; // Alguns termos não foram testados por completo
        let mut ii: i32; // Contador de laço
        let mut p_and_expr: Option<Box<Expr>> = None; // Expressão ".. AND (...)"
        let p_tab: TableRef = p_tab_item.p_tab.clone().unwrap();

        p_term = p_loop.borrow().a_l_term[0].clone();
        debug_assert!(p_term.is_some());
        debug_assert!((p_term.as_ref().unwrap().borrow().e_operator & WO_OR) != 0);
        debug_assert!((p_term.as_ref().unwrap().borrow().wt_flags & TERM_ORINFO) != 0);
        match &p_term.as_ref().unwrap().borrow().u {
            WhereTermUnion::OrInfo(Some(info)) => {
                p_or_wc_a = info.wc.a.clone();
                p_or_wc_n_term = info.wc.n_term;
            }
            _ => unreachable!(),
        }
        p_level.op = OP_RETURN;
        p_level.p1 = reg_return;

        // Monta em pOrTab uma nova SrcList com a tabela varrida por este laço
        // no slot a[0] e todas as tabelas notReady nos slots a[1..]. Ela vira a
        // SrcList da chamada recursiva a sqlite3WhereBegin().
        if p_w_info.n_level > 1 {
            let n_not_ready: i32; // Número de tabelas notReady
            n_not_ready = p_w_info.n_level as i32 - i_level - 1;
            let mut new_list = SrcList::default();
            new_list.n_alloc = (n_not_ready + 1) as u32;
            new_list.n_src = new_list.n_alloc as i32;
            new_list.a.push(p_tab_item.clone());
            k = 1;
            while k <= n_not_ready {
                let i_from_k = p_w_info.a[(i_level + k) as usize].i_from as usize;
                let item = p_w_info.p_tab_list.borrow().a[i_from_k].clone();
                new_list.a.push(item);
                k += 1;
            }
            p_or_tab = Rc::new(RefCell::new(new_list));
        } else {
            p_or_tab = p_w_info.p_tab_list.clone();
        }

        // Inicializa o registrador do rowset com NULL. Um NULL de SQL equivale
        // a um rowset vazio. Ou cria um índice efêmero capaz de guardar chaves
        // primárias, no caso de WITHOUT ROWID.
        //
        // Também inicializa regReturn com o endereço da instrução logo depois
        // do OP_Return no fim do laço. Isso é necessário em alguns casos
        // obscuros de LEFT JOIN em que o controle pula o topo do laço e cai no
        // corpo dele. Nesse caso a resposta correta do código de fim de laço (o
        // OP_Return) é seguir para a instrução seguinte, como faz um OP_Next
        // chamado sobre um cursor não inicializado.
        if (p_w_info.wctrl_flags & (WHERE_DUPLICATES_OK as u16)) == 0 {
            if has_rowid(&p_tab.borrow()) {
                reg_rowset = {
                    let mut pm = p_parse.borrow_mut();
                    pm.n_mem += 1;
                    pm.n_mem
                };
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_rowset);
            } else {
                let p_pk = primary_key_index(&p_tab.borrow()).unwrap();
                reg_rowset = {
                    let mut pm = p_parse.borrow_mut();
                    let r = pm.n_tab;
                    pm.n_tab += 1;
                    r
                };
                vdbe_add_op2(
                    &mut v.borrow_mut(),
                    OP_OPENEPHEMERAL as i32,
                    reg_rowset,
                    p_pk.borrow().n_key_col as i32,
                );
                vdbe_set_p4_key_info(&mut p_parse.borrow_mut(), &p_pk);
            }
            reg_rowid = {
                let mut pm = p_parse.borrow_mut();
                pm.n_mem += 1;
                pm.n_mem
            };
        }
        i_ret_init = vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, reg_return);

        // Se a cláusula WHERE original z é da forma (x1 OR x2 OR ...) AND y,
        // então para cada termo xN avalia-se a subexpressão: xN AND y. Assim os
        // termos de y que foram fatorados na disjunção são pegos pelas chamadas
        // recursivas a sqlite3WhereBegin() abaixo.
        //
        // Na verdade, cada subexpressão é convertida em "xN AND w", onde w são
        // os termos "interessantes" de z: os que não vieram da cláusula ON ou
        // USING de um LEFT JOIN, e os que podem ser usados como índices.
        //
        // Esta otimização também só vale se o termo (x1 OR x2 OR ...) não está
        // contido na cláusula ON de um LEFT JOIN. Veja o ticket
        // http://www.sqlite.org/src/info/f2369304e4
        //
        // 04/02/2022: não empurra fatias de uma comparação de row-value. Em
        // outras palavras, "w" ou "y" não podem ser uma fatia de um vetor. Do
        // contrário, a inicialização do operando direito da comparação vetorial
        // pode não ocorrer, ou ocorrer só num ramo do OR que não é tomado.
        // dbsqlfuzz 80a9fade844b4fb43564efc972bcb2c68270f5d1.
        //
        // 03/03/2022: não empurra expressões que envolvem subconsultas. A
        // subconsulta pode ser codificada como sub-rotina. Qualquer referência a
        // tabela na subconsulta pode ser resolvida para referências ao índice do
        // ramo do OR em que a sub-rotina é codificada. Mas se a sub-rotina é
        // chamada de outro ramo do OR que usa outro índice, essas referências
        // não funcionam. tag-20220303a
        // https://sqlite.org/forum/forumpost/36937b197273d403
        if p_w_info.s_wc.n_term > 1 {
            let mut i_term: i32;
            i_term = 0;
            while i_term < p_w_info.s_wc.n_term {
                let term_i = p_w_info.s_wc.a[i_term as usize].clone();
                let p_expr_i = term_i.borrow().p_expr.clone();
                if Rc::ptr_eq(&term_i, p_term.as_ref().unwrap()) {
                    i_term += 1;
                    continue;
                }
                if (term_i.borrow().wt_flags & (TERM_VIRTUAL | TERM_CODED | TERM_SLICE)) != 0 {
                    i_term += 1;
                    continue;
                }
                if (term_i.borrow().e_operator & WO_ALL) == 0 {
                    i_term += 1;
                    continue;
                }
                if (p_expr_i.as_ref().unwrap().borrow().flags & EP_SUBQUERY) != 0 {
                    // tag-20220303a
                    i_term += 1;
                    continue;
                }
                let p_dup = expr_dup(&db, Some(&*p_expr_i.as_ref().unwrap().borrow()), 0);
                p_and_expr = expr_and(&mut p_parse.borrow_mut(), p_and_expr, p_dup);
                i_term += 1;
            }
            if p_and_expr.is_some() {
                // O bit extra 0x10000 do opcode é mascarado e não entra no novo
                // Expr.op. Porém ele torna falsa a comparação op==TK_AND dentro
                // de sqlite3PExpr(), o que impede o sqlite3PExpr() de aplicar a
                // otimização de curto-circuito do AND, que não queremos aqui.
                p_and_expr = p_expr(&mut p_parse.borrow_mut(), (TK_AND as i32) | 0x10000, None, p_and_expr);
            }
        }


// ---- part_006.rs ----
// FRAGMENTO de corpo de função (não é um arquivo de itens). Continuação direta
// da parte 005; veja o contrato no cabeçalho da parte 004. Este trecho entra no
// meio do Case 5 (OR múltiplo), logo depois da montagem de p_and_expr, e vai até
// o fim de `sqlite3WhereCodeOneLoopStart`: ele fecha o bloco do Case 5, escreve
// o Case 6, e termina com o `return` e a chave de fechamento da função.
//
// Variáveis herdadas do Case 5 (parte 005): p_or_wc_a, p_or_wc_n_term,
// p_or_tab, p_cov, i_cov_cur, reg_return, reg_rowset, reg_rowid, i_loop_body,
// i_ret_init, untested_terms, ii, p_and_expr, p_tab.
//
// SQLITE_ENABLE_STMT_SCANSTATUS não está no build do Debian: sqlite3WhereAddScanStatus
// é um macro vazio e pLevel->addrVisit não existe; ambos somem aqui.
// WHERETRACE_ENABLED também não está definido: os blocos de trace somem.

        // Roda uma cláusula WHERE separada para cada termo da cláusula OR.
        // Depois de eliminar duplicatas de outras cláusulas WHERE, a ação de
        // cada sub-WHERE é invocar o corpo principal do laço como sub-rotina.
        explain_query_plan(&p_parse, 1, b"MULTI-INDEX OR");
        ii = 0;
        while ii < p_or_wc_n_term {
            let p_or_term = p_or_wc_a[ii as usize].clone();
            if p_or_term.borrow().left_cursor == i_cur || (p_or_term.borrow().e_operator & WO_AND) != 0 {
                let mut jmp1: i32 = 0; // Endereço da operação de salto
                // Cópia local do termo da cláusula OR. Quando há p_and_expr, a
                // cópia é movida para p_and_expr.p_left durante a chamada e
                // devolvida depois (no C os dois ponteiros apontam para o mesmo
                // nó, e pDelete é quem libera).
                let mut p_delete: Option<Box<Expr>> = {
                    let e = p_or_term.borrow().p_expr.clone().unwrap();
                    let r = expr_dup(&db, Some(&*e.borrow()), 0);
                    r
                };
                if db.borrow().malloc_failed != 0 {
                    expr_delete(&db.borrow(), p_delete);
                    ii += 1;
                    continue;
                }
                if let Some(and_expr) = p_and_expr.as_mut() {
                    and_expr.p_left = p_delete.take();
                }
                // Percorre as entradas da tabela que casam com o termo pOrTerm.
                explain_query_plan(&p_parse, 1, format!("INDEX {}", ii + 1).as_bytes());
                let p_sub_w_info: Option<WhereInfoRef> = {
                    let where_expr: Option<&Expr> = if p_and_expr.is_some() {
                        p_and_expr.as_deref()
                    } else {
                        p_delete.as_deref()
                    };
                    where_begin(
                        &p_parse,
                        p_or_tab.clone(),
                        where_expr,
                        None,
                        None,
                        None,
                        WHERE_OR_SUBCLAUSE as u16,
                        i_cov_cur,
                    )
                };
                debug_assert!(p_sub_w_info.is_some() || p_parse.borrow().n_err != 0);
                if let Some(sub) = p_sub_w_info {
                    // O valor devolvido de sqlite3WhereExplainOneScan() só serve
                    // ao sqlite3WhereAddScanStatus(), que é vazio neste build.
                    let _addr_explain = where_explain_one_scan(
                        &p_parse,
                        &p_or_tab.borrow(),
                        &sub.borrow().a[0],
                        0,
                    );

                    // Este é o corpo da sub-WHERE. Primeiro pula as linhas
                    // duplicadas de sub-WHEREs anteriores, e registra o rowid (ou
                    // a PRIMARY KEY) da linha atual para que a mesma linha seja
                    // pulada nas sub-WHEREs seguintes.
                    if (p_w_info.wctrl_flags & (WHERE_DUPLICATES_OK as u16)) == 0 {
                        let i_set: i32 = if ii == p_or_wc_n_term - 1 { -1 } else { ii };
                        if has_rowid(&p_tab.borrow()) {
                            expr_code_get_column_of_table(&v, &p_tab, i_cur, -1, reg_rowid);
                            jmp1 = vdbe_add_op4_int(
                                &mut v.borrow_mut(),
                                OP_ROWSETTEST as i32,
                                reg_rowset,
                                0,
                                reg_rowid,
                                i_set,
                            );
                        } else {
                            let p_pk = primary_key_index(&p_tab.borrow()).unwrap();
                            let n_pk: i32 = p_pk.borrow().n_key_col as i32;
                            let mut i_pk: i32;
                            let r: i32;

                            // Lê a PK para um array de registradores temporários.
                            r = get_temp_range(&mut p_parse.borrow_mut(), n_pk);
                            i_pk = 0;
                            while i_pk < n_pk {
                                let i_col = p_pk.borrow().ai_column[i_pk as usize] as i32;
                                expr_code_get_column_of_table(&v, &p_tab, i_cur, i_col, r + i_pk);
                                i_pk += 1;
                            }

                            // Verifica se a tabela temporária já contém esta
                            // chave. Se sim, a linha já foi incluída no
                            // resultado e pode ser ignorada (saltando sobre o
                            // Gosub abaixo). Senão, insere a chave na tabela
                            // temporária e processa a linha.
                            //
                            // Usa algumas das mesmas otimizações de
                            // OP_RowSetTest: se iSet é zero, supõe que a chave
                            // não pode já estar na tabela temporária. E se iSet
                            // é -1, supõe que não há necessidade de inserir a
                            // chave, pois ela nunca será testada.
                            if i_set != 0 {
                                jmp1 = vdbe_add_op4_int(
                                    &mut v.borrow_mut(),
                                    OP_FOUND as i32,
                                    reg_rowset,
                                    0,
                                    r,
                                    n_pk,
                                );
                            }
                            if i_set >= 0 {
                                vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, r, n_pk, reg_rowid);
                                vdbe_add_op4_int(
                                    &mut v.borrow_mut(),
                                    OP_IDXINSERT as i32,
                                    reg_rowset,
                                    reg_rowid,
                                    r,
                                    n_pk,
                                );
                                if i_set != 0 {
                                    vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_USESEEKRESULT as u16);
                                }
                            }

                            // Libera o array de registradores temporários.
                            release_temp_range(&mut p_parse.borrow_mut(), r, n_pk);
                        }
                    }

                    // Invoca o corpo principal do laço como sub-rotina.
                    vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, reg_return, i_loop_body);

                    // Salta para cá (pulando a sub-rotina do corpo do laço) se a
                    // linha atual da sub-WHERE é duplicata de sub-WHEREs
                    // anteriores.
                    if jmp1 != 0 {
                        vdbe_jump_here(&mut v.borrow_mut(), jmp1);
                    }

                    // O flag pSubWInfo->untestedTerms significa que este termo
                    // OR continha um ou mais termos AND de uma tabela notReady.
                    // Os termos da tabela notReady não puderam ser testados e
                    // precisarão ser testados depois.
                    if sub.borrow().untested_terms {
                        untested_terms = true;
                    }

                    // Se todos os termos ligados por OR são otimizados com o
                    // mesmo índice, e o índice é aberto com o mesmo número de
                    // cursor por cada chamada a sqlite3WhereBegin() feita por
                    // este laço, pode ser possível usar esse índice como índice
                    // de cobertura.
                    //
                    // Se a chamada a sqlite3WhereBegin() acima resultou numa
                    // varredura que usa um índice, e este é o primeiro termo
                    // ligado por OR processado ou o índice é o mesmo usado por
                    // todos os termos anteriores, pCov recebe o candidato a
                    // índice de cobertura. Caso contrário, pCov vira NULL para
                    // indicar que nenhum candidato a índice de cobertura estará
                    // disponível.
                    let p_sub_loop: WhereLoopRef = sub.borrow().a[0].p_w_loop.clone().unwrap();
                    debug_assert!((p_sub_loop.borrow().ws_flags & WHERE_AUTO_INDEX) == 0);
                    let sub_index: Option<IndexRef> = match &p_sub_loop.borrow().u {
                        WhereLoopUnion::Btree { p_index, .. } => p_index.clone(),
                        _ => None,
                    };
                    let same_as_cov = match (&sub_index, &p_cov) {
                        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                        _ => false,
                    };
                    if (p_sub_loop.borrow().ws_flags & WHERE_INDEXED) != 0
                        && (ii == 0 || same_as_cov)
                        && (has_rowid(&p_tab.borrow())
                            || sub_index.as_ref().unwrap().borrow().idx_type != SQLITE_IDXTYPE_PRIMARYKEY)
                    {
                        debug_assert!(sub.borrow().a[0].i_idx_cur == i_cov_cur);
                        p_cov = sub_index;
                    } else {
                        p_cov = None;
                    }
                    if where_uses_deferred_seek(&sub) != 0 {
                        p_w_info.b_deferred_seek = true;
                    }

                    // Termina o laço pelas entradas da tabela que casam com o
                    // termo pOrTerm.
                    where_end(sub);
                    explain_query_plan_pop(&p_parse);
                }
                // Recupera a cópia do termo (que estava em p_and_expr.p_left) e
                // a libera: sqlite3ExprDelete(db, pDelete).
                if let Some(and_expr) = p_and_expr.as_mut() {
                    p_delete = and_expr.p_left.take();
                }
                expr_delete(&db.borrow(), p_delete);
            }
            ii += 1;
        }
        explain_query_plan_pop(&p_parse);
        debug_assert!(Rc::ptr_eq(p_level.p_w_loop.as_ref().unwrap(), &p_loop));
        debug_assert!((p_loop.borrow().ws_flags & WHERE_MULTI_OR) != 0);
        debug_assert!((p_loop.borrow().ws_flags & WHERE_IN_ABLE) == 0);
        p_level.u = WhereLevelUnion::CoveringIdx(p_cov.clone());
        if p_cov.is_some() {
            p_level.i_idx_cur = i_cov_cur;
        }
        if let Some(mut and_expr) = p_and_expr.take() {
            and_expr.p_left = None;
            expr_delete(&db.borrow(), Some(and_expr));
        }
        let addr_here = vdbe_current_addr(&v.borrow());
        vdbe_change_p1(&mut v.borrow_mut(), i_ret_init, addr_here);
        vdbe_goto(&mut v.borrow_mut(), p_level.addr_brk);
        vdbe_resolve_label(&mut v.borrow_mut(), i_loop_body);

        // Aponta o operando P2 do opcode OP_Return que vai terminar o laço
        // atual para este ponto, que é o topo do próximo laço que o contém. O
        // formatador de bytecode usa esse valor de P2 como dica para indentar
        // tudo o que está entre este ponto e o OP_Return final. Veja
        // tag-20220407a em vdbe.c e shell.c.
        debug_assert!(p_level.op == OP_RETURN);
        p_level.p2 = vdbe_current_addr(&v.borrow());

        // sqlite3DbFreeNN(db, pOrTab) quando nLevel>1: liberado pelo drop.
        drop(p_or_tab);
        if !untested_terms {
            disable_term(p_level, &p_term.as_ref().unwrap().borrow());
        }
    } else {
        // Caso 6: não há índice utilizável. Precisamos fazer uma varredura
        // completa da tabela inteira.
        const A_STEP: [u8; 2] = [OP_NEXT, OP_PREV];
        const A_START: [u8; 2] = [OP_REWIND, OP_LAST];
        debug_assert!(b_rev == 0 || b_rev == 1);
        if p_tab_item.fg.is_recursive != 0 {
            // Tabelas marcadas isRecursive têm uma única linha, guardada num
            // pseudo-cursor. Não é preciso fazer Rewind nem Next nesses
            // cursores.
            p_level.op = OP_NOOP;
        } else {
            code_cursor_hint(&p_tab_item, p_w_info, p_level, None);
            p_level.op = A_STEP[b_rev as usize];
            p_level.p1 = i_cur;
            p_level.p2 = 1 + vdbe_add_op2(&mut v.borrow_mut(), A_START[b_rev as usize] as i32, i_cur, addr_halt);
            p_level.p5 = SQLITE_STMTSTATUS_FULLSCAN_STEP as u8;
        }
    }

    // Insere código para testar toda subexpressão que pode ser completamente
    // calculada com o conjunto atual de tabelas.
    //
    // Este laço pode rodar de uma a três vezes, dependendo das restrições a
    // gerar. O valor da variável de pilha iLoop determina as restrições
    // codificadas por cada iteração, assim:
    //
    // iLoop==1: Codifica só as expressões inteiramente cobertas por pIdx.
    // iLoop==2: Codifica as expressões restantes que não contêm subconsultas
    //           correlacionadas.
    // iLoop==3: Codifica todas as expressões restantes.
    //
    // Faz-se um esforço para pular iterações desnecessárias do laço.
    //
    // Esta otimização, de fazer as restrições simples da consulta ocorrerem
    // antes das mais complexas, é chamada de otimização "push-down" no MySQL.
    // Aqui no SQLite o nome é "MySQL push-down", já que existe também outra
    // otimização totalmente sem relação chamada "WHERE-clause push-down". Às
    // vezes o qualificador é omitido, gerando ambiguidade, então cuidado.
    let mut i_loop: i32 = if p_idx.is_some() { 1 } else { 2 };
    loop {
        let mut i_next: i32 = 0; // Próximo valor de iLoop
        let n_term_loop = p_w_info.s_wc.n_term;
        let mut i_t: i32 = 0;
        while i_t < n_term_loop {
            let term = p_w_info.s_wc.a[i_t as usize].clone();
            i_t += 1;
            let mut skip_like_addr: i32 = 0;
            if (term.borrow().wt_flags & (TERM_VIRTUAL | TERM_CODED)) != 0 {
                continue;
            }
            if (term.borrow().prereq_all & p_level.not_ready) != 0 {
                p_w_info.untested_terms = true;
                continue;
            }
            let p_e_ref = term.borrow().p_expr.clone().unwrap();
            if (p_tab_item.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0 {
                let e_flags = p_e_ref.borrow().flags;
                if (e_flags & (EP_OUTER_ON | EP_INNER_ON)) == 0 {
                    // Adia o processamento das restrições da cláusula WHERE até
                    // depois do processamento do outer join. tag-20220513a
                    continue;
                } else if (p_tab_item.fg.jointype & JT_LEFT) == JT_LEFT && (e_flags & EP_OUTER_ON) == 0 {
                    continue;
                } else {
                    let m: Bitmask = where_get_mask(&p_w_info.s_mask_set, p_e_ref.borrow().w.i_join);
                    if (m & p_level.not_ready) != 0 {
                        // Uma cláusula ON que ainda não está madura.
                        continue;
                    }
                }
            }
            if i_loop == 1
                && expr_covered_by_index(&mut *p_e_ref.borrow_mut(), p_level.i_tab_cur, p_idx.as_ref()) == 0
            {
                i_next = 2;
                continue;
            }
            if i_loop < 3 && (term.borrow().wt_flags & TERM_VARSELECT) != 0 {
                if i_next == 0 {
                    i_next = 3;
                }
                continue;
            }

            if (term.borrow().wt_flags & TERM_LIKECOND) != 0 {
                // Se o flag TERM_LIKECOND está ligado, a busca por intervalo é
                // suficiente para garantir que o operador LIKE é verdadeiro, e
                // podemos pular a chamada à função like(A,B). Mas isso só vale
                // para strings. Então não pula a chamada da função na passada
                // que compara BLOBs. (SQLITE_LIKE_DOESNT_MATCH_BLOBS não está
                // definido no Debian.)
                let x: u32 = p_level.i_like_rep_cntr;
                if x > 0 {
                    skip_like_addr = vdbe_add_op1(
                        &mut v.borrow_mut(),
                        (if (x & 1) != 0 { OP_IFNOT } else { OP_IF }) as i32,
                        (x >> 1) as i32,
                    );
                }
            }
            expr_if_false(&mut p_parse.borrow_mut(), &*p_e_ref.borrow(), addr_cont, SQLITE_JUMPIFNULL as i32);
            if skip_like_addr != 0 {
                vdbe_jump_here(&mut v.borrow_mut(), skip_like_addr);
            }
            term.borrow_mut().wt_flags |= TERM_CODED;
        }
        i_loop = i_next;
        if i_loop <= 0 {
            break;
        }
    }

    // Insere código para testar restrições implícitas baseadas na
    // transitividade do operador "==".
    //
    // Exemplo: se a cláusula WHERE contém "t1.a=t2.b" e "t2.b=123", e estamos
    // codificando o laço de t1 com o laço de t2 ainda não codificado, então não
    // podemos usar a restrição "t1.a=t2.b", mas podemos codificar a restrição
    // implícita "t1.a=123".
    let n_base_loop = p_w_info.s_wc.n_base;
    let mut i_b: i32 = 0;
    while i_b < n_base_loop {
        let term = p_w_info.s_wc.a[i_b as usize].clone();
        i_b += 1;
        if (term.borrow().wt_flags & (TERM_VIRTUAL | TERM_CODED)) != 0 {
            continue;
        }
        if (term.borrow().e_operator & (WO_EQ | WO_IS)) == 0 {
            continue;
        }
        if (term.borrow().e_operator & WO_EQUIV) == 0 {
            continue;
        }
        if term.borrow().left_cursor != i_cur {
            continue;
        }
        if (p_tab_item.fg.jointype & (JT_LEFT | JT_LTORJ | JT_RIGHT)) != 0 {
            continue;
        }
        let p_e_ref = term.borrow().p_expr.clone().unwrap();
        debug_assert!((p_e_ref.borrow().flags & EP_OUTER_ON) == 0);
        debug_assert!((term.borrow().prereq_right & p_level.not_ready) != 0);
        debug_assert!((term.borrow().e_operator & (WO_OR | WO_AND)) == 0);
        let left_column = match &term.borrow().u {
            WhereTermUnion::X { left_column, .. } => *left_column,
            _ => unreachable!(),
        };
        let p_alt: Option<WhereTermRef> = where_find_term(
            &p_w_info.s_wc,
            i_cur,
            left_column,
            not_ready,
            (WO_EQ | WO_IN | WO_IS) as u32,
            None,
        );
        let p_alt = match p_alt {
            Some(a) => a,
            None => continue,
        };
        if (p_alt.borrow().wt_flags & TERM_CODED) != 0 {
            continue;
        }
        let p_alt_expr_ref = p_alt.borrow().p_expr.clone().unwrap();
        if (p_alt.borrow().e_operator & WO_IN) != 0
            && expr_use_x_select(&p_alt_expr_ref.borrow())
            && p_alt_expr_ref
                .borrow()
                .x
                .p_select
                .as_ref()
                .unwrap()
                .p_e_list
                .as_ref()
                .unwrap()
                .n_expr
                > 1
        {
            continue;
        }
        // sEAlt = *pAlt->pExpr; sEAlt.pLeft = pE->pLeft;
        let mut s_e_alt: Expr = p_alt_expr_ref.borrow().clone();
        s_e_alt.p_left = p_e_ref.borrow().p_left.clone();
        expr_if_false(&mut p_parse.borrow_mut(), &s_e_alt, addr_cont, SQLITE_JUMPIFNULL as i32);
        p_alt.borrow_mut().wt_flags |= TERM_CODED;
    }

    // Para um RIGHT OUTER JOIN, registra o fato de que a linha atual foi
    // casada pelo menos uma vez.
    if p_level.p_rj.is_some() {
        let p_tab_rj: TableRef;
        let n_pk: i32;
        let r: i32;
        let jmp1: i32;
        let (rj_i_match, rj_reg_bloom) = {
            let rj = p_level.p_rj.as_ref().unwrap();
            (rj.i_match, rj.reg_bloom)
        };

        // pTab é a tabela do lado direito do RIGHT JOIN. Gera código que
        // registra que a linha atual dessa tabela foi casada pelo menos uma
        // vez. Isso é feito guardando a PK da linha tanto no índice iMatch
        // quanto no filtro de Bloom regBloom.
        p_tab_rj = p_w_info.p_tab_list.borrow().a[p_level.i_from as usize].p_tab.clone().unwrap();
        if has_rowid(&p_tab_rj.borrow()) {
            r = get_temp_range(&mut p_parse.borrow_mut(), 2);
            expr_code_get_column_of_table(&v, &p_tab_rj, p_level.i_tab_cur, -1, r + 1);
            n_pk = 1;
        } else {
            let mut i_pk: i32;
            let p_pk = primary_key_index(&p_tab_rj.borrow()).unwrap();
            n_pk = p_pk.borrow().n_key_col as i32;
            r = get_temp_range(&mut p_parse.borrow_mut(), n_pk + 1);
            i_pk = 0;
            while i_pk < n_pk {
                let i_col = p_pk.borrow().ai_column[i_pk as usize] as i32;
                expr_code_get_column_of_table(&v, &p_tab_rj, i_cur, i_col, r + 1 + i_pk);
                i_pk += 1;
            }
        }
        jmp1 = vdbe_add_op4_int(&mut v.borrow_mut(), OP_FOUND as i32, rj_i_match, 0, r + 1, n_pk);
        vdbe_add_op3(&mut v.borrow_mut(), OP_MAKERECORD as i32, r + 1, n_pk, r);
        vdbe_add_op4_int(&mut v.borrow_mut(), OP_IDXINSERT as i32, rj_i_match, r, r + 1, n_pk);
        vdbe_add_op4_int(&mut v.borrow_mut(), OP_FILTERADD as i32, rj_reg_bloom, 0, r + 1, n_pk);
        vdbe_change_p5(&mut v.borrow_mut(), OPFLAG_USESEEKRESULT as u16);
        vdbe_jump_here(&mut v.borrow_mut(), jmp1);
        release_temp_range(&mut p_parse.borrow_mut(), r, n_pk + 1);
    }

    // Para um LEFT OUTER JOIN, gera código que registra o fato de que pelo
    // menos uma linha da tabela direita casou com a tabela esquerda.
    //
    // O `goto code_outer_join_constraints` do C pula para dentro do bloco
    // `if( pLevel->pRJ )` seguinte, passando por cima da sub-rotina do RIGHT
    // JOIN. Aqui o salto vira o flag goto_code_outer_join_constraints.
    let mut goto_code_outer_join_constraints = false;
    if p_level.i_left_join != 0 {
        p_level.addr_first = vdbe_current_addr(&v.borrow());
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 1, p_level.i_left_join);
        if p_level.p_rj.is_none() {
            goto_code_outer_join_constraints = true; // restrições da cláusula WHERE
        }
    }

    if p_level.p_rj.is_some() || goto_code_outer_join_constraints {
        if !goto_code_outer_join_constraints {
            // Cria uma sub-rotina usada para processar todos os laços internos
            // e o código do RIGHT JOIN. Na operação normal, a sub-rotina fica
            // em linha com o resto do código. Mas no fim roda um laço separado
            // que invoca esta sub-rotina para as linhas não casadas de pTab,
            // com todas as tabelas à esquerda postas em NULL.
            let rj_reg_return = p_level.p_rj.as_ref().unwrap().reg_return;
            vdbe_add_op2(&mut v.borrow_mut(), OP_BEGINSUBRTN as i32, 0, rj_reg_return);
            let addr_subrtn = vdbe_current_addr(&v.borrow());
            p_level.p_rj.as_mut().unwrap().addr_subrtn = addr_subrtn;
            debug_assert!(p_parse.borrow().within_rj_subrtn < 255);
            p_parse.borrow_mut().within_rj_subrtn += 1;

            // As restrições da cláusula WHERE precisam ser adiadas até depois
            // que a eliminação de linhas do outer join termina, já que elas se
            // aplicam ao resultado do OUTER JOIN. O laço seguinte gera as
            // verificações de restrição da cláusula WHERE apropriadas.
            // tag-20220513a.
        }
        // code_outer_join_constraints:
        j = 0;
        while j < p_w_info.s_wc.n_base {
            let term = p_w_info.s_wc.a[j as usize].clone();
            j += 1;
            if (term.borrow().wt_flags & (TERM_VIRTUAL | TERM_CODED)) != 0 {
                continue;
            }
            if (term.borrow().prereq_all & p_level.not_ready) != 0 {
                debug_assert!(p_w_info.untested_terms);
                continue;
            }
            if (p_tab_item.fg.jointype & JT_LTORJ) != 0 {
                continue;
            }
            let p_e_ref = term.borrow().p_expr.clone().unwrap();
            expr_if_false(&mut p_parse.borrow_mut(), &*p_e_ref.borrow(), addr_cont, SQLITE_JUMPIFNULL as i32);
            term.borrow_mut().wt_flags |= TERM_CODED;
        }
    }

    p_level.not_ready
}


// ---- part_007.rs ----

/// Gera o código do laço que encontra todos os termos não casados de um RIGHT JOIN.
///
/// Desvio da assinatura do C `(pWInfo, iLevel, pLevel)`: o `pLevel` do C é
/// `&pWInfo->a[iLevel]`, e passar os dois ao mesmo tempo (um `&WhereInfoRef`
/// e um `&mut WhereLevel` dentro dele) não é possível com `Rc<RefCell<>>`.
/// Aqui só se recebe o `p_w_info` e o `i_level`, e o nível é lido por índice.
/// `sqlite3VdbeNoJumpsOutsideSubrtn` só existe sob SQLITE_DEBUG e some.
#[inline(never)]
pub fn where_right_join_loop(p_w_info: &WhereInfoRef, i_level: i32) {
    let p_parse: ParseRef = p_w_info.borrow().p_parse.clone();
    let v: VdbeRef = p_parse.borrow().p_vdbe.clone().unwrap();
    let db = p_parse.borrow().db.upgrade().unwrap();
    let mut p_sub_where: Option<Box<Expr>> = None;
    let mut m_all: Bitmask = 0;
    let mut k: i32;

    // Campos de pLevel (e de pLevel->pRJ, pLevel->pWLoop) lidos uma vez.
    let (rj_addr_subrtn, rj_reg_return, rj_i_match, rj_reg_bloom, i_from, i_tab_cur, loop_mask_self) = {
        let wi = p_w_info.borrow();
        let p_level = &wi.a[i_level as usize];
        let rj = p_level.p_rj.as_ref().unwrap();
        (
            rj.addr_subrtn,
            rj.reg_return,
            rj.i_match,
            rj.reg_bloom,
            p_level.i_from as usize,
            p_level.i_tab_cur,
            p_level.p_w_loop.as_ref().unwrap().borrow().mask_self,
        )
    };
    let p_tab_item: SrcItem = p_w_info.borrow().p_tab_list.borrow().a[i_from].clone();
    let p_tab: TableRef = p_tab_item.p_tab.clone().unwrap();

    let mut explain_text: Vec<u8> = b"RIGHT-JOIN ".to_vec();
    explain_text.extend_from_slice(&p_tab.borrow().z_name);
    explain_query_plan(&p_parse, 1, &explain_text);

    k = 0;
    while k < i_level {
        // Dados do nível k e da tabela dele, lidos sem segurar o empréstimo
        // durante a geração de código.
        let (via_coroutine, reg_result, n_expr_sel, i_tab_cur_k, i_idx_cur_k, mask_k) = {
            let wi = p_w_info.borrow();
            let lvl = &wi.a[k as usize];
            let p_loop_k = lvl.p_w_loop.as_ref().unwrap().borrow();
            debug_assert!(p_loop_k.i_tab == lvl.i_from);
            let tab_list = wi.p_tab_list.borrow();
            let p_right = &tab_list.a[lvl.i_from as usize];
            let n_expr_sel = if p_right.fg.via_coroutine != 0 {
                p_right.p_select.as_ref().unwrap().p_e_list.as_ref().unwrap().n_expr
            } else {
                0
            };
            (
                p_right.fg.via_coroutine != 0,
                p_right.reg_result,
                n_expr_sel,
                lvl.i_tab_cur,
                lvl.i_idx_cur,
                p_loop_k.mask_self,
            )
        };
        m_all |= mask_k;
        if via_coroutine {
            vdbe_add_op3(
                &mut v.borrow_mut(),
                OP_NULL as i32,
                0,
                reg_result,
                reg_result + n_expr_sel - 1,
            );
        }
        vdbe_add_op1(&mut v.borrow_mut(), OP_NULLROW as i32, i_tab_cur_k);
        if i_idx_cur_k != 0 {
            vdbe_add_op1(&mut v.borrow_mut(), OP_NULLROW as i32, i_idx_cur_k);
        }
        k += 1;
    }
    if (p_tab_item.fg.jointype & JT_LTORJ) == 0 {
        m_all |= loop_mask_self;
        let (terms, n_term) = {
            let wi = p_w_info.borrow();
            (wi.s_wc.a.clone(), wi.s_wc.n_term)
        };
        k = 0;
        while k < n_term {
            let p_term = terms[k as usize].clone();
            if (p_term.borrow().wt_flags & (TERM_VIRTUAL | TERM_SLICE)) != 0
                && p_term.borrow().e_operator != WO_ROWVAL
            {
                break;
            }
            if (p_term.borrow().prereq_all & !m_all) != 0 {
                k += 1;
                continue;
            }
            let p_expr_term = p_term.borrow().p_expr.clone().unwrap();
            if (p_expr_term.borrow().flags & (EP_OUTER_ON | EP_INNER_ON)) != 0 {
                k += 1;
                continue;
            }
            let p_dup = expr_dup(&db, Some(&*p_expr_term.borrow()), 0);
            p_sub_where = expr_and(&mut p_parse.borrow_mut(), p_sub_where, p_dup);
            k += 1;
        }
    }
    let mut s_from = SrcList::default();
    s_from.n_src = 1;
    s_from.n_alloc = 1;
    s_from.a.push(p_tab_item.clone());
    s_from.a[0].fg.jointype = 0;
    debug_assert!(p_parse.borrow().within_rj_subrtn < 100);
    p_parse.borrow_mut().within_rj_subrtn += 1;
    let p_sub_w_info: Option<WhereInfoRef> = where_begin(
        &p_parse,
        Rc::new(RefCell::new(s_from)),
        p_sub_where.as_deref(),
        None,
        None,
        None,
        WHERE_RIGHT_JOIN as u16,
        0,
    );
    if let Some(sub) = p_sub_w_info {
        let i_cur: i32 = i_tab_cur;
        let r: i32 = {
            let mut pm = p_parse.borrow_mut();
            pm.n_mem += 1;
            pm.n_mem
        };
        let n_pk: i32;
        let jmp: i32;
        let addr_cont: i32 = sub.borrow().i_continue; // sqlite3WhereContinueLabel(pSubWInfo)
        if has_rowid(&p_tab.borrow()) {
            expr_code_get_column_of_table(&v, &p_tab, i_cur, -1, r);
            n_pk = 1;
        } else {
            let mut i_pk: i32;
            let p_pk = primary_key_index(&p_tab.borrow()).unwrap();
            n_pk = p_pk.borrow().n_key_col as i32;
            p_parse.borrow_mut().n_mem += n_pk - 1;
            i_pk = 0;
            while i_pk < n_pk {
                let i_col = p_pk.borrow().ai_column[i_pk as usize] as i32;
                expr_code_get_column_of_table(&v, &p_tab, i_cur, i_col, r + i_pk);
                i_pk += 1;
            }
        }
        jmp = vdbe_add_op4_int(&mut v.borrow_mut(), OP_FILTER as i32, rj_reg_bloom, 0, r, n_pk);
        vdbe_add_op4_int(&mut v.borrow_mut(), OP_FOUND as i32, rj_i_match, addr_cont, r, n_pk);
        vdbe_jump_here(&mut v.borrow_mut(), jmp);
        vdbe_add_op2(&mut v.borrow_mut(), OP_GOSUB as i32, rj_reg_return, rj_addr_subrtn);
        where_end(sub);
    }
    expr_delete(&db.borrow(), p_sub_where);
    explain_query_plan_pop(&p_parse);
    debug_assert!(p_parse.borrow().within_rj_subrtn > 0);
    p_parse.borrow_mut().within_rj_subrtn -= 1;
}

