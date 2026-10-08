// Mesclado das partes traduzidas de walker_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Modelo adotado (o integrador precisa ter estes apelidos no prelude):
// `ExprRef`, `ExprListRef`, `SelectRef`, `WindowRef`, `SrcListRef` = `Rc<RefCell<T>>`.
// Os callbacks do Walker mutam a árvore (resolução de nomes, renomeação), então a árvore não
// pode ser clonada: todos os nós são compartilhados por `Rc` e nenhum `borrow()` fica vivo
// durante uma chamada de callback ou de recursão.
//   x_expr_callback:    Option<fn(&mut Walker, &ExprRef) -> i32>
//   x_select_callback:  Option<fn(&mut Walker, &SelectRef) -> i32>
//   x_select_callback2: Option<fn(&mut Walker, &SelectRef)>

/// Caminha por todas as expressões ligadas à lista de objetos Window passada como segundo
/// argumento.
fn walk_window_list(p_walker: &mut Walker, p_list: Option<&WindowRef>, b_one_only: i32) -> i32 {
    let mut p_win = p_list.cloned();
    while let Some(win) = p_win {
        // Copia os campos para soltar o empréstimo antes de chamar os callbacks.
        let (p_order_by, p_partition, p_filter, p_start, p_end, p_next_win) = {
            let w = win.borrow();
            (
                w.p_order_by.clone(),
                w.p_partition.clone(),
                w.p_filter.clone(),
                w.p_start.clone(),
                w.p_end.clone(),
                w.p_next_win.clone(),
            )
        };
        let mut rc: i32;
        rc = walk_expr_list(p_walker, p_order_by.as_ref());
        if rc != 0 {
            return WRC_ABORT;
        }
        rc = walk_expr_list(p_walker, p_partition.as_ref());
        if rc != 0 {
            return WRC_ABORT;
        }
        rc = walk_expr(p_walker, p_filter.as_ref());
        if rc != 0 {
            return WRC_ABORT;
        }
        rc = walk_expr(p_walker, p_start.as_ref());
        if rc != 0 {
            return WRC_ABORT;
        }
        rc = walk_expr(p_walker, p_end.as_ref());
        if rc != 0 {
            return WRC_ABORT;
        }
        if b_one_only != 0 {
            break;
        }
        p_win = p_next_win;
    }
    WRC_CONTINUE
}

/// Caminha por uma árvore de expressão. Invoca o callback uma vez para cada nó da
/// expressão enquanto desce. (Em outras palavras, o callback é invocado antes de visitar
/// os filhos.)
///
/// O valor de retorno do callback deve ser uma das constantes WRC_* para especificar como
/// proceder com a caminhada:
///
/// - WRC_CONTINUE: continua descendo pela árvore.
/// - WRC_PRUNE: não desce nos nós filhos, mas deixa a caminhada seguir com os irmãos.
/// - WRC_ABORT: não faz mais callbacks; desenrola a pilha e retorna da chamada de topo.
///
/// O valor de retorno desta rotina é WRC_ABORT para abandonar a caminhada e WRC_CONTINUE
/// para continuar.
pub fn walk_expr_nn(p_walker: &mut Walker, p_expr: &ExprRef) -> i32 {
    let mut cur: ExprRef = p_expr.clone();
    loop {
        let rc = match p_walker.x_expr_callback {
            Some(cb) => cb(p_walker, &cur),
            None => WRC_CONTINUE,
        };
        if rc != 0 {
            return rc & WRC_ABORT;
        }
        // As propriedades são lidas depois do callback, que pode alterá-las.
        let (skip, p_left, p_right, use_select, p_select, p_list, has_win, p_win) = {
            let e = cur.borrow();
            (
                expr_has_property(&e, EP_TOKEN_ONLY | EP_LEAF),
                e.p_left.clone(),
                e.p_right.clone(),
                expr_use_x_select(&e),
                e.x.p_select.clone(),
                e.x.p_list.clone(),
                expr_has_property(&e, EP_WIN_FUNC),
                e.y.p_win.clone(),
            )
        };
        if !skip {
            debug_assert!(p_list.is_none() || p_right.is_none());
            if let Some(left) = &p_left {
                if walk_expr_nn(p_walker, left) != 0 {
                    return WRC_ABORT;
                }
            }
            if let Some(right) = p_right {
                debug_assert!(!has_win);
                cur = right;
                continue;
            } else if use_select {
                debug_assert!(!has_win);
                if walk_select(p_walker, p_select.as_ref()) != 0 {
                    return WRC_ABORT;
                }
            } else {
                if p_list.is_some() {
                    if walk_expr_list(p_walker, p_list.as_ref()) != 0 {
                        return WRC_ABORT;
                    }
                }
                if has_win {
                    if walk_window_list(p_walker, p_win.as_ref(), 1) != 0 {
                        return WRC_ABORT;
                    }
                }
            }
        }
        break;
    }
    WRC_CONTINUE
}

/// Versão de walk_expr_nn que aceita expressão nula (retorna WRC_CONTINUE).
pub fn walk_expr(p_walker: &mut Walker, p_expr: Option<&ExprRef>) -> i32 {
    match p_expr {
        Some(e) => walk_expr_nn(p_walker, e),
        None => WRC_CONTINUE,
    }
}

/// Chama walk_expr() para cada expressão da lista p, ou até que uma solicitação de
/// aborto seja vista.
pub fn walk_expr_list(p_walker: &mut Walker, p: Option<&ExprListRef>) -> i32 {
    if let Some(list) = p {
        let n_expr = list.borrow().n_expr;
        let mut i = 0usize;
        while (i as i32) < n_expr {
            // O empréstimo da lista é solto antes de caminhar pela expressão do item.
            let p_item_expr = list.borrow().a[i].p_expr.clone();
            if walk_expr(p_walker, p_item_expr.as_ref()) != 0 {
                return WRC_ABORT;
            }
            i += 1;
        }
    }
    WRC_CONTINUE
}

/// Callback vazio para Walker.xSelectCallback2. Se este callback for definido, então a
/// lista Select.pWinDefn é percorrida.
pub fn walk_win_defn_dummy_callback(_p_walker: &mut Walker, _p: &SelectRef) {
    // Nenhuma operação.
}

/// Caminha por todas as expressões associadas com a declaração SELECT p. Não invoca o
/// callback SELECT em p, mas invoca (é claro) qualquer expr callback e SELECT callback que
/// venha de subconsultas. Retorna WRC_ABORT ou WRC_CONTINUE.
pub fn walk_select_expr(p_walker: &mut Walker, p: &SelectRef) -> i32 {
    let (p_e_list, p_where, p_group_by, p_having, p_order_by, p_limit) = {
        let s = p.borrow();
        (
            s.p_e_list.clone(),
            s.p_where.clone(),
            s.p_group_by.clone(),
            s.p_having.clone(),
            s.p_order_by.clone(),
            s.p_limit.clone(),
        )
    };
    if walk_expr_list(p_walker, p_e_list.as_ref()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(p_walker, p_where.as_ref()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr_list(p_walker, p_group_by.as_ref()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(p_walker, p_having.as_ref()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr_list(p_walker, p_order_by.as_ref()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(p_walker, p_limit.as_ref()) != 0 {
        return WRC_ABORT;
    }
    // O pWinDefn é lido depois dos walks acima, como no C.
    let p_win_defn = p.borrow().p_win_defn.clone();
    if p_win_defn.is_some() {
        let cb2 = p_walker.x_select_callback2.map(|f| f as usize);
        let is_dummy = cb2 == Some(walk_win_defn_dummy_callback as usize);
        let is_pop_with = cb2 == Some(select_pop_with as usize);
        let in_rename = match &p_walker.p_parse {
            Some(p_parse) => in_rename_object(p_parse),
            None => false,
        };
        if is_dummy || in_rename || is_pop_with {
            // O que segue pode retornar WRC_ABORT se houver símbolos não resolvíveis (por
            // exemplo, uma tabela que não existe) em uma definição de janela.
            let rc = walk_window_list(p_walker, p_win_defn.as_ref(), 0);
            return rc;
        }
    }
    WRC_CONTINUE
}

/// Caminha pelas árvores de análise associadas com todas as subconsultas na cláusula FROM
/// da declaração SELECT p. Não invoca o callback SELECT em p, mas invoca em cada subconsulta
/// da cláusula FROM e em qualquer subconsulta mais adiante na árvore. Retorna WRC_ABORT ou
/// WRC_CONTINUE.
pub fn walk_select_from(p_walker: &mut Walker, p: &SelectRef) -> i32 {
    let p_src = p.borrow().p_src.clone();
    if let Some(src) = p_src {
        let n_src = src.borrow().n_src;
        let mut i = 0usize;
        while (i as i32) < n_src {
            let (p_select, is_tab_func, p_func_arg) = {
                let s = src.borrow();
                let item = &s.a[i];
                let func_arg = match &item.u1 {
                    SrcItemU1::FuncArg(arg) => arg.clone(),
                    _ => None,
                };
                (item.p_select.clone(), item.fg.is_tab_func != 0, func_arg)
            };
            if p_select.is_some() && walk_select(p_walker, p_select.as_ref()) != 0 {
                return WRC_ABORT;
            }
            if is_tab_func && walk_expr_list(p_walker, p_func_arg.as_ref()) != 0 {
                return WRC_ABORT;
            }
            i += 1;
        }
    }
    WRC_CONTINUE
}

/// Chama walk_expr() para cada expressão da declaração Select p. Invoca walk_select() para
/// subconsultas na cláusula FROM e na cadeia de SELECT composto, p.pPrior.
///
/// Se não for NULL, o callback xSelectCallback() é invocado antes da caminhada das
/// expressões e da cláusula FROM. O método xSelectCallback2() é invocado após a caminhada
/// das expressões e da cláusula FROM, mas somente se xSelectCallback e xSelectCallback2
/// forem ambos não NULL e se as expressões e a cláusula FROM retornarem WRC_CONTINUE.
///
/// Retorna WRC_CONTINUE nas condições normais. Retorna WRC_ABORT se houver uma solicitação
/// de aborto.
///
/// Se o Walker não tiver um xSelectCallback(), esta rotina não faz nada e retorna
/// WRC_CONTINUE.
pub fn walk_select(p_walker: &mut Walker, p: Option<&SelectRef>) -> i32 {
    let mut cur = match p {
        Some(s) => s.clone(),
        None => return WRC_CONTINUE,
    };
    let cb = match p_walker.x_select_callback {
        Some(cb) => cb,
        None => return WRC_CONTINUE,
    };
    loop {
        let rc = cb(p_walker, &cur);
        if rc != 0 {
            return rc & WRC_ABORT;
        }
        if walk_select_expr(p_walker, &cur) != 0 || walk_select_from(p_walker, &cur) != 0 {
            return WRC_ABORT;
        }
        if let Some(cb2) = p_walker.x_select_callback2 {
            cb2(p_walker, &cur);
        }
        let p_prior = cur.borrow().p_prior.clone();
        match p_prior {
            Some(prior) => cur = prior,
            None => break,
        }
    }
    WRC_CONTINUE
}

/// Aumenta a walkerDepth ao entrar em uma subconsulta e diminui ao sair dela.
pub fn walker_depth_increase(p_walker: &mut Walker, _p_select: &SelectRef) -> i32 {
    p_walker.walker_depth += 1;
    WRC_CONTINUE
}

pub fn walker_depth_decrease(p_walker: &mut Walker, _p_select: &SelectRef) {
    p_walker.walker_depth -= 1;
}

/// Rotina sem efeito para o caminhador da árvore de análise.
///
/// Quando esta rotina é o Walker.xExprCallback, as árvores de expressão são percorridas sem
/// nenhuma ação em cada nó. Presumivelmente, quando ela é usada como Walker.xExprCallback,
/// o Walker.xSelectCallback é configurado para fazer algo útil em cada subconsulta da
/// árvore de análise.
pub fn expr_walk_noop(_not_used: &mut Walker, _not_used2: &ExprRef) -> i32 {
    WRC_CONTINUE
}

/// Rotina sem efeito para o caminhador da árvore de análise para declarações SELECT.
pub fn select_walk_noop(_not_used: &mut Walker, _not_used2: &SelectRef) -> i32 {
    WRC_CONTINUE
}

