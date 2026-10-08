//! walker.c: o percurso das árvores de sintaxe de um comando SQL.
//!
//! O `Walker` do C guarda `pParse` e a `union u` de ponteiros; aqui o `Walker<C>` (em
//! `crate::sqlite_int`) possui o contexto `C` da passagem. Duas informações que o C tirava do
//! `pParse` e do endereço dos callbacks viram bits de `Walker.m_w_flags`, definidos abaixo:
//!
//! * `WALKER_FLAG_IN_RENAME`: o `pParse` do walker existe e está em `IN_RENAME_OBJECT`. Quem
//!   monta um `Walker` para o ALTER TABLE RENAME (ou para a resolução de nomes dentro dele) liga
//!   o bit.
//! * `WALKER_FLAG_POP_WITH`: o `xSelectCallback2` é `sqlite3SelectPopWith`. O C compara o
//!   endereço da função, mas o `Walker<C>` de cada passagem tem um `C` diferente, e quem monta o
//!   walker do `select_pop_with` liga o bit.
//!
//! Aqui o `xSelectCallback2 == sqlite3WalkWinDefnDummyCallback` continua sendo comparação de
//! endereço de função.

use crate::consts::{
    EP_LEAF, EP_TOKEN_ONLY, EP_WIN_FUNC, WRC_ABORT, WRC_CONTINUE,
};
use crate::sqlite_int::{Expr, ExprList, Select, SrcU1, Walker, Window};

/// O `pParse` do walker está em `IN_RENAME_OBJECT` (ver o comentário do módulo).
pub const WALKER_FLAG_IN_RENAME: u16 = 0x8000;

/// O `xSelectCallback2` do walker é `sqlite3SelectPopWith` (ver o comentário do módulo).
pub const WALKER_FLAG_POP_WITH: u16 = 0x4000;

/// `walkWindowList` para a janela de uma função (`bOneOnly` verdadeiro): percorre todas as
/// expressões de uma só janela (ORDER BY, PARTITION BY, FILTER, início e fim do quadro).
fn walk_window<C>(w: &mut Walker<C>, p_win: &mut Window) -> i32 {
    if walk_expr_list(w, p_win.p_order_by.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr_list(w, p_win.p_partition.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p_win.p_filter.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p_win.p_start.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p_win.p_end.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    WRC_CONTINUE
}

/// `walkWindowList` para a lista de definições de `Select.pWinDefn` (`bOneOnly` falso).
fn walk_window_list<C>(w: &mut Walker<C>, p_list: &mut [Window]) -> i32 {
    for p_win in p_list.iter_mut() {
        if walk_window(w, p_win) != 0 {
            return WRC_ABORT;
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkExprNN`: percorre uma árvore de expressão. O callback é chamado uma vez por nó,
/// descendo (ou seja, antes de visitar os filhos).
///
/// O valor devolvido pelo callback é um dos `WRC_*`:
///
/// * `WRC_CONTINUE`: continua descendo.
/// * `WRC_PRUNE`: não desce aos filhos, mas segue para os irmãos.
/// * `WRC_ABORT`: nenhum callback mais; desenrola a pilha e sai da chamada de topo.
///
/// O resultado desta rotina é `WRC_ABORT` para abandonar o percurso e `WRC_CONTINUE` para
/// continuar.
pub fn walk_expr_nn<C>(w: &mut Walker<C>, p_expr: &mut Expr) -> i32 {
    let mut cur: &mut Expr = p_expr;
    loop {
        if let Some(f) = w.x_expr_callback {
            let rc = f(w, cur);
            if rc != 0 {
                return rc & WRC_ABORT;
            }
        }
        if !cur.has_property(EP_TOKEN_ONLY | EP_LEAF) {
            if let Some(l) = cur.p_left.as_deref_mut() {
                if walk_expr_nn(w, l) != 0 {
                    return WRC_ABORT;
                }
            }
            if cur.p_right.is_some() {
                if let Some(r) = cur.p_right.as_deref_mut() {
                    cur = r;
                    continue;
                }
            } else if cur.use_x_select() {
                if walk_select(w, cur.x_select_mut()) != 0 {
                    return WRC_ABORT;
                }
            } else {
                if cur.x_list().is_some() && walk_expr_list(w, cur.x_list_mut()) != 0 {
                    return WRC_ABORT;
                }
                if cur.has_property(EP_WIN_FUNC) {
                    if let Some(p_win) = cur.y_win_mut() {
                        if walk_window(w, p_win) != 0 {
                            return WRC_ABORT;
                        }
                    }
                }
            }
        }
        break;
    }
    WRC_CONTINUE
}

/// `sqlite3WalkExpr`: como [`walk_expr_nn`], aceitando a expressão nula.
pub fn walk_expr<C>(w: &mut Walker<C>, p_expr: Option<&mut Expr>) -> i32 {
    match p_expr {
        Some(e) => walk_expr_nn(w, e),
        None => WRC_CONTINUE,
    }
}

/// `sqlite3WalkExprList`: chama [`walk_expr`] para cada expressão da lista ou até um pedido de
/// abortar.
pub fn walk_expr_list<C>(w: &mut Walker<C>, p: Option<&mut ExprList>) -> i32 {
    if let Some(list) = p {
        for item in list.a.iter_mut() {
            if walk_expr(w, item.p_expr.as_deref_mut()) != 0 {
                return WRC_ABORT;
            }
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkWinDefnDummyCallback`: callback vazio de `Walker.xSelectCallback2`. Se ele estiver
/// instalado, a lista `Select.pWinDefn` é percorrida.
pub fn walk_win_defn_dummy_callback<C>(_w: &mut Walker<C>, _p: &mut Select) {
    // Sem efeito.
}

/// `sqlite3WalkSelectExpr`: percorre todas as expressões associadas ao SELECT `p`. Não chama o
/// callback de SELECT em `p`, mas chama (claro) os callbacks de expressão e os de SELECT das
/// subconsultas. Devolve `WRC_ABORT` ou `WRC_CONTINUE`.
pub fn walk_select_expr<C>(w: &mut Walker<C>, p: &mut Select) -> i32 {
    if walk_expr_list(w, p.p_e_list.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p.p_where.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr_list(w, p.p_group_by.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p.p_having.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr_list(w, p.p_order_by.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if walk_expr(w, p.p_limit.as_deref_mut()) != 0 {
        return WRC_ABORT;
    }
    if !p.p_win_defn.is_empty() {
        let dummy = walk_win_defn_dummy_callback::<C> as fn(&mut Walker<C>, &mut Select);
        let is_dummy = w
            .x_select_callback2
            .map_or(false, |f| f as usize == dummy as usize);
        if is_dummy
            || (w.m_w_flags & WALKER_FLAG_IN_RENAME) != 0
            || (w.m_w_flags & WALKER_FLAG_POP_WITH) != 0
        {
            // O que segue pode devolver WRC_ABORT se houver símbolos que não se resolvem (por
            // exemplo uma tabela que não existe) numa definição de janela.
            return walk_window_list(w, &mut p.p_win_defn);
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkSelectFrom`: percorre as árvores de todas as subconsultas da cláusula FROM do
/// SELECT `p`. Não chama o callback de SELECT em `p`, mas chama em cada subconsulta do FROM e em
/// todas as subconsultas mais abaixo. Devolve `WRC_ABORT` ou `WRC_CONTINUE`.
pub fn walk_select_from<C>(w: &mut Walker<C>, p: &mut Select) -> i32 {
    if let Some(p_src) = p.p_src.as_deref_mut() {
        for p_item in p_src.a.iter_mut() {
            if p_item.p_select.is_some() && walk_select(w, p_item.p_select.as_deref_mut()) != 0 {
                return WRC_ABORT;
            }
            if p_item.fg.is_tab_func {
                if let SrcU1::FuncArg(arg) = &mut p_item.u1 {
                    if walk_expr_list(w, arg.as_deref_mut()) != 0 {
                        return WRC_ABORT;
                    }
                }
            }
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkSelect`: chama `walk_expr` para cada expressão do SELECT `p` e `walk_select` para
/// as subconsultas do FROM e para a cadeia composta `p->pPrior`.
///
/// Se existir, `x_select_callback` é chamado antes do percurso das expressões e do FROM. O
/// `x_select_callback2` é chamado depois, mas só se os dois forem não nulos e se as expressões e
/// o FROM devolverem `WRC_CONTINUE`.
///
/// Devolve `WRC_CONTINUE` em condições normais e `WRC_ABORT` se houver pedido de abortar. Se o
/// walker não tem `x_select_callback`, a rotina não faz nada e devolve `WRC_CONTINUE`.
pub fn walk_select<C>(w: &mut Walker<C>, p: Option<&mut Select>) -> i32 {
    let mut p = match p {
        Some(p) => p,
        None => return WRC_CONTINUE,
    };
    let cb = match w.x_select_callback {
        Some(cb) => cb,
        None => return WRC_CONTINUE,
    };
    loop {
        let rc = cb(w, p);
        if rc != 0 {
            return rc & WRC_ABORT;
        }
        if walk_select_expr(w, p) != 0 || walk_select_from(w, p) != 0 {
            return WRC_ABORT;
        }
        if let Some(cb2) = w.x_select_callback2 {
            cb2(w, p);
        }
        match p.p_prior.as_deref_mut() {
            Some(prior) => p = prior,
            None => break,
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkerDepthIncrease`: aumenta `walker_depth` ao entrar numa subconsulta.
pub fn walker_depth_increase<C>(w: &mut Walker<C>, _p_select: &mut Select) -> i32 {
    w.walker_depth += 1;
    WRC_CONTINUE
}

/// `sqlite3WalkerDepthDecrease`: diminui `walker_depth` ao sair da subconsulta.
pub fn walker_depth_decrease<C>(w: &mut Walker<C>, _p_select: &mut Select) {
    w.walker_depth -= 1;
}

/// `sqlite3ExprWalkNoop`: callback vazio do percurso. Quando é o `x_expr_callback`, as árvores de
/// expressão são percorridas sem ação em cada nó; presume-se que o `x_select_callback` faça algo
/// útil para cada subconsulta da árvore.
pub fn expr_walk_noop<C>(_w: &mut Walker<C>, _e: &mut Expr) -> i32 {
    WRC_CONTINUE
}

/// `sqlite3SelectWalkNoop`: callback vazio do percurso para SELECTs.
pub fn select_walk_noop<C>(_w: &mut Walker<C>, _p: &mut Select) -> i32 {
    WRC_CONTINUE
}
