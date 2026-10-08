// Mesclado das partes traduzidas de upsert_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Libera uma lista de objetos Upsert (upsertDelete e sqlite3UpsertDelete
/// juntos: o C separa só por causa de NOINLINE, a posse em Rust já libera o resto).
pub fn upsert_delete(db: &Sqlite3Ref, mut p: Option<Box<Upsert>>) {
    while let Some(mut upsert) = p {
        p = upsert.p_next_upsert.take();
        expr_list_delete(db, upsert.p_upsert_target.take());
        expr_delete(db, upsert.p_upsert_target_where.take());
        expr_list_delete(db, upsert.p_upsert_set.take());
        expr_delete(db, upsert.p_upsert_where.take());
        // p_to_free e o próprio nó são liberados pelo drop do Box.
    }
}

/// Duplica um objeto Upsert.
pub fn upsert_dup(db: &Sqlite3Ref, p: Option<&Upsert>) -> Option<Box<Upsert>> {
    let p = p?;
    upsert_new(
        db,
        expr_list_dup(db, p.p_upsert_target.as_deref(), 0),
        expr_dup(db, p.p_upsert_target_where.as_deref(), 0),
        expr_list_dup(db, p.p_upsert_set.as_deref(), 0),
        expr_dup(db, p.p_upsert_where.as_deref(), 0),
        upsert_dup(db, p.p_next_upsert.as_deref()),
    )
}

/// Cria um novo objeto Upsert.
pub fn upsert_new(
    db: &Sqlite3Ref,
    p_target: Option<Box<ExprList>>,
    p_target_where: Option<Box<Expr>>,
    p_set: Option<Box<ExprList>>,
    p_where: Option<Box<Expr>>,
    p_next: Option<Box<Upsert>>,
) -> Option<Box<Upsert>> {
    match db_malloc_zero::<Upsert>(db) {
        None => {
            expr_list_delete(db, p_target);
            expr_delete(db, p_target_where);
            expr_list_delete(db, p_set);
            expr_delete(db, p_where);
            upsert_delete(db, p_next);
            None
        }
        Some(mut p_new) => {
            p_new.is_do_update = p_set.is_some() as u8;
            p_new.p_upsert_target = p_target;
            p_new.p_upsert_target_where = p_target_where;
            p_new.p_upsert_set = p_set;
            p_new.p_upsert_where = p_where;
            p_new.p_next_upsert = p_next;
            Some(p_new)
        }
    }
}

/// Sufixo ordinal do formato `%r` do printf do SQLite ("st", "nd", "rd", "th").
fn ordinal_suffix(n: i32) -> &'static [u8] {
    let x = n % 100;
    if (11..=13).contains(&x) {
        return b"th";
    }
    match x % 10 {
        1 => b"st",
        2 => b"nd",
        3 => b"rd",
        _ => b"th",
    }
}

/// Dois índices (ou dois "nenhum") são o mesmo objeto, como a comparação de ponteiros do C.
fn same_index(a: &Option<IndexRef>, b: Option<&IndexRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Núcleo de sqlite3UpsertOfIndex: devolve a profundidade (0 é o próprio
/// p_upsert) da cláusula que se aplica ao índice, ou None se o C devolveria NULL.
/// A profundidade faz o papel da identidade de ponteiro que o modelo sem ponteiros não tem.
fn upsert_of_index_depth(p_upsert: Option<&Upsert>, p_idx: Option<&IndexRef>) -> Option<usize> {
    let mut cur = p_upsert;
    let mut depth = 0usize;
    while let Some(u) = cur {
        if u.p_upsert_target.is_none() || same_index(&u.p_upsert_idx, p_idx) {
            return Some(depth);
        }
        cur = u.p_next_upsert.as_deref();
        depth += 1;
    }
    None
}

/// Nó na posição `depth` da lista encadeada.
fn upsert_at(head: &Upsert, depth: usize) -> Option<&Upsert> {
    let mut cur = head;
    for _ in 0..depth {
        cur = cur.p_next_upsert.as_deref()?;
    }
    Some(cur)
}

/// Nó mutável na posição `depth` da lista encadeada.
fn upsert_at_mut(head: &mut Upsert, depth: usize) -> Option<&mut Upsert> {
    let mut cur = head;
    for _ in 0..depth {
        cur = cur.p_next_upsert.as_deref_mut()?;
    }
    Some(cur)
}

/// Analisa a cláusula ON CONFLICT descrita pelo nó `start` da lista `p_all`
/// (no C, pUpsert e pAll aliasam a mesma lista; aqui `p_all` é a cabeça e `start`
/// a posição de pUpsert nela). Resolve todos os símbolos no alvo do conflito.
///
/// Retorna SQLITE_OK se tudo funcionar, ou um código de erro se algo der errado.
pub fn upsert_analyze_target(
    p_parse: &mut Parse,
    p_tab_list: &SrcList,
    p_all: &mut Upsert,
    start: usize,
) -> i32 {
    debug_assert!(p_tab_list.n_src == 1);
    debug_assert!(p_tab_list.a[0].p_tab.is_some());
    debug_assert!(upsert_at(p_all, start).map_or(false, |u| u.p_upsert_target.is_some()));

    // Resolve todos os nomes simbólicos na cláusula alvo do conflito, que
    // inclui tanto a lista de colunas quanto a cláusula WHERE opcional do
    // índice parcial. O p_parse viaja como argumento dos resolvedores.
    let mut s_nc = NameContext::default();
    s_nc.p_src_list = Some(Box::new(p_tab_list.clone()));

    let p_tab_ref = p_tab_list.a[0].p_tab.clone().unwrap();
    let tab = p_tab_ref.borrow();
    let i_cursor = p_tab_list.a[0].i_cursor;

    let mut n_clause: i32 = 0;
    let mut depth = start;
    loop {
        {
            let cur = match upsert_at_mut(p_all, depth) {
                Some(c) => c,
                None => break,
            };
            if cur.p_upsert_target.is_none() {
                break;
            }
            let rc = resolve_expr_list_names(p_parse, &mut s_nc, cur.p_upsert_target.as_deref_mut());
            if rc != 0 {
                return rc;
            }
            let rc = resolve_expr_names(p_parse, &mut s_nc, cur.p_upsert_target_where.as_deref_mut());
            if rc != 0 {
                return rc;
            }
        }

        let cur = upsert_at(p_all, depth).unwrap();
        let p_target = cur.p_upsert_target.as_deref().unwrap();

        // Verifica se o alvo do conflito corresponde ao rowid.
        if has_rowid(&tab)
            && p_target.n_expr == 1
            && p_target.a[0]
                .p_expr
                .as_deref()
                .map_or(false, |t| t.op == TK_COLUMN && t.i_column == XN_ROWID)
        {
            // O alvo do conflito é o rowid da tabela primária
            debug_assert!(cur.p_upsert_idx.is_none());
            depth += 1;
            n_clause += 1;
            continue;
        }

        // Inicializa s_col[0..1] para ser uma árvore de análise de expressão
        // para uma única coluna de um índice. O nó s_col0 será o operador
        // TK_COLLATE e s_col1 será o operador TK_COLUMN. O código abaixo
        // preencherá os valores de colação e número de coluna específicos
        // antes de comparar com a expressão alvo do conflito.
        let mut s_col0 = Expr::default();
        let mut s_col1 = Expr::default();
        s_col0.op = TK_COLLATE;
        s_col1.op = TK_COLUMN;
        s_col1.i_table = i_cursor;

        // Verifica correspondências com outros índices
        let mut found: Option<IndexRef> = None;
        let mut p_idx = tab.p_index.clone();
        while let Some(idx_ref) = p_idx.take() {
            let idx = idx_ref.borrow();
            p_idx = idx.p_next.clone();
            if !is_unique_index(&idx) {
                continue;
            }
            if p_target.n_expr != idx.n_key_col as i32 {
                continue;
            }
            if let Some(part_where) = idx.p_part_idx_where.as_deref() {
                let target_where = match cur.p_upsert_target_where.as_deref() {
                    None => continue,
                    Some(w) => w,
                };
                if expr_compare(Some(&mut *p_parse), Some(target_where), Some(part_where), i_cursor) != 0 {
                    continue;
                }
            }
            let nn = idx.n_key_col as usize;
            let mut ii = 0usize;
            while ii < nn {
                s_col0.u.z_token = Some(idx.az_coll[ii].clone());
                let p_expr: &Expr = if idx.ai_column[ii] == XN_EXPR {
                    debug_assert!(idx.a_col_expr.is_some());
                    debug_assert!(idx.a_col_expr.as_ref().unwrap().n_expr as usize > ii);
                    debug_assert!(idx.b_has_expr != 0);
                    let e = idx.a_col_expr.as_ref().unwrap().a[ii].p_expr.as_deref().unwrap();
                    if e.op != TK_COLLATE {
                        s_col0.p_left = Some(Box::new(e.clone()));
                        &s_col0
                    } else {
                        e
                    }
                } else {
                    s_col1.i_column = idx.ai_column[ii];
                    s_col0.p_left = Some(Box::new(s_col1.clone()));
                    &s_col0
                };
                let mut jj = 0usize;
                while jj < nn {
                    if expr_compare(None, p_target.a[jj].p_expr.as_deref(), Some(p_expr), i_cursor) < 2 {
                        break; // Coluna ii do índice corresponde à coluna jj do alvo
                    }
                    jj += 1;
                }
                if jj >= nn {
                    // O alvo não contém correspondência para a coluna ii do índice
                    break;
                }
                ii += 1;
            }
            if ii < nn {
                // Coluna ii do índice não corresponde a nenhum termo do alvo do conflito.
                // Continua a busca com o próximo índice.
                continue;
            }
            found = Some(Rc::clone(&idx_ref));
            break;
        }

        let mut is_dup = false;
        if let Some(idx_ref) = found.as_ref() {
            // Na verdade isto deveria ser um erro: a cláusula ON CONFLICT duplicada
            // nunca dispara. Mas o problema só foi descoberto três anos depois que o
            // upsert com vários ON CONFLICT foi adicionado, então ele é ignorado em
            // silêncio para não quebrar aplicações que tenham cláusulas redundantes.
            is_dup = upsert_of_index_depth(Some(&*p_all), Some(idx_ref)) != Some(depth);
        }

        if found.is_none() && cur.p_upsert_idx.is_none() {
            let mut msg: Vec<u8> = Vec::new();
            if !(n_clause == 0 && cur.p_next_upsert.is_none()) {
                msg.extend_from_slice((n_clause + 1).to_string().as_bytes());
                msg.extend_from_slice(ordinal_suffix(n_clause + 1));
                msg.push(b' ');
            }
            msg.extend_from_slice(b"ON CONFLICT clause does not match any PRIMARY KEY or UNIQUE constraint");
            error_msg(p_parse, &msg);
            return SQLITE_ERROR;
        }

        if found.is_some() {
            let cur = upsert_at_mut(p_all, depth).unwrap();
            cur.p_upsert_idx = found;
            if is_dup {
                cur.is_dup = 1;
            }
        }

        depth += 1;
        n_clause += 1;
    }
    SQLITE_OK
}

/// Retorna verdadeiro se p_upsert é a última cláusula ON CONFLICT com um
/// alvo de conflito, ou se p_upsert é seguido por outra cláusula ON CONFLICT
/// que tem como alvo a INTEGER PRIMARY KEY.
pub fn upsert_next_is_ipk(p_upsert: Option<&Upsert>) -> i32 {
    let upsert = match p_upsert {
        None => return 0, // NEVER no C
        Some(u) => u,
    };
    let mut p_next = upsert.p_next_upsert.as_deref();
    loop {
        let next = match p_next {
            None => return 1,
            Some(n) => n,
        };
        if next.p_upsert_target.is_none() {
            return 1;
        }
        if next.p_upsert_idx.is_none() {
            return 1;
        }
        if next.is_dup == 0 {
            return 0;
        }
        p_next = next.p_next_upsert.as_deref();
    }
}

/// Dada a lista de cláusulas ON CONFLICT descrita por p_upsert e um índice
/// particular p_idx, devolve a cláusula ON CONFLICT que se aplica ao índice.
/// Se o índice não estiver sujeito a nenhuma cláusula, devolve None.
pub fn upsert_of_index<'a>(p_upsert: Option<&'a Upsert>, p_idx: Option<&IndexRef>) -> Option<&'a Upsert> {
    let depth = upsert_of_index_depth(p_upsert, p_idx)?;
    upsert_at(p_upsert?, depth)
}

/// Gera bytecode que faz um UPDATE como parte de um upsert.
///
/// Se p_idx é None, a restrição UNIQUE que falhou foi o IPK. Neste caso, i_cur
/// é um cursor aberto na b-tree da tabela, apontando para a linha conflitante.
/// Caso contrário, p_idx é a restrição que falhou e i_cur é um cursor que
/// aponta para a linha conflitante.
pub fn upsert_do_update(
    p_parse: &mut Parse,
    p_upsert: &Upsert,
    p_tab: &Table,
    p_idx: Option<&IndexRef>,
    i_cur: i32,
) {
    let v = p_parse.p_vdbe.clone().expect("vdbe");
    let db = p_parse.db.clone();
    let p_top = p_upsert;

    let i_data_cur = p_upsert.i_data_cur;
    let p_upsert = upsert_of_index(Some(p_top), p_idx).expect("upsert");
    // VdbeNoopComment só gera código com SQLITE_ENABLE_EXPLAIN_COMMENTS, ausente no Debian.
    if let Some(idx_ref) = p_idx {
        if i_cur != i_data_cur {
            if has_rowid(p_tab) {
                let reg_rowid = get_temp_reg(p_parse);
                vdbe_add_op2(&v, OP_IDXROWID, i_cur, reg_rowid);
                vdbe_add_op3(&v, OP_SEEKROWID, i_data_cur, 0, reg_rowid);
                release_temp_reg(p_parse, reg_rowid);
            } else {
                let p_pk_ref = primary_key_index(p_tab).unwrap();
                let p_pk = p_pk_ref.borrow();
                let idx = idx_ref.borrow();
                let n_pk = p_pk.n_key_col as i32;
                let i_pk = p_parse.n_mem + 1;
                p_parse.n_mem += n_pk;
                for i in 0..n_pk {
                    debug_assert!(p_pk.ai_column[i as usize] >= 0);
                    let k = table_column_to_index(&idx, p_pk.ai_column[i as usize]) as i32;
                    vdbe_add_op3(&v, OP_COLUMN, i_cur, k, i_pk + i);
                }
                // vdbe_verify_abortable existe só sob SQLITE_DEBUG.
                let addr = vdbe_add_op4_int(&v, OP_FOUND, i_data_cur, 0, i_pk, n_pk);
                vdbe_add_op4(&v, OP_HALT, SQLITE_CORRUPT, OE_ABORT, 0, Some(b"corrupt database"), P4_STATIC);
                may_abort(p_parse);
                vdbe_jump_here(&v, addr);
            }
        }
    }
    // p_upsert não é dono de p_top.p_upsert_src: o INSERT externo é. Então é
    // preciso uma cópia antes de passá-la para update().
    let p_src = src_list_dup(&db, p_top.p_upsert_src.as_deref(), 0);
    // As colunas excluded.* de afinidade REAL precisam virar real de verdade.
    for i in 0..p_tab.n_col as usize {
        if p_tab.a_col[i].affinity == SQLITE_AFF_REAL {
            vdbe_add_op1(&v, OP_REALAFFINITY, p_top.reg_data + i as i32);
        }
    }
    update(
        p_parse,
        p_src,
        expr_list_dup(&db, p_upsert.p_upsert_set.as_deref(), 0),
        expr_dup(&db, p_upsert.p_upsert_where.as_deref(), 0),
        OE_ABORT,
        None,
        None,
        Some(p_upsert),
    );
}

