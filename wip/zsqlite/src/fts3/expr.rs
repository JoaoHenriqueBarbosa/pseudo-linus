//! `fts3_expr.c`: o analisador da expressão MATCH do FTS3 (o operando direito do `MATCH`). A
//! sintaxe é simples, então o analisador e o tokenizador de consultas são escritos à mão.
//!
//! Com `SQLITE_ENABLE_FTS3_PARENTHESIS` (ligado no Debian) vale a sintaxe nova:
//!
//! ```text
//!   query ::= andexpr (OR andexpr)*.
//!   andexpr ::= notexpr (AND? notexpr)*.
//!   notexpr ::= nearexpr (NOT nearexpr)*.
//!   notexpr ::= LP query RP.
//!   nearexpr ::= phrase (NEAR distance_opt nearexpr)*.
//!   distance_opt ::= .
//!   distance_opt ::= / INTEGER.
//!   phrase ::= TOKEN.
//!   phrase ::= COLUMN:TOKEN.
//!   phrase ::= "TOKEN TOKEN TOKEN...".
//! ```
//!
//! Por isso `sqlite3_fts3_enable_parentheses` é 1 e o código da sintaxe antiga some: o qualificador
//! `-` de token, o `pNotBranch`, `ParseContext.isNot`, `Fts3Keyword.parenOnly` (todo operador vale)
//! e a precedência do `opPrecedence` sem parênteses (a precedência é o próprio `eType`: NEAR,
//! NOT, AND e OR, do mais forte ao mais fraco). `SQLITE_MAX_EXPR_DEPTH` está definido (1000). O
//! código de teste (`exprToString`, `fts3_exprtest`) não existe no build do Debian.
//!
//! Modelo v2: os nós vivem na arena [`Fts3ExprTree`] (ver `int.rs`). Durante a análise a arena é
//! do `ParseContext` e no fim vira a árvore devolvida. Cada nó solto durante a análise é só
//! largado (não há leitores de segmentos nem outro estado de avaliação ainda); o
//! `sqlite3Fts3ExprFree` público, que roda sobre uma árvore já avaliada, é [`fts3_expr_free`].

use crate::build::text_arg;
use crate::connection::Connection;
use crate::consts::{SQLITE_DONE, SQLITE_ERROR, SQLITE_MAX_EXPR_DEPTH, SQLITE_OK, SQLITE_TOOBIG};
use crate::printf::{mprintf, PrintfArg};
use crate::util::{at, strnicmp};

use super::int::{
    fts3_read_int, ExprId, Fts3Expr, Fts3ExprTree, Fts3Phrase, Fts3PhraseToken, Fts3Tokenizer,
    Fts3TokenizerCursor, FTSQUERY_AND, FTSQUERY_NEAR, FTSQUERY_NOT, FTSQUERY_OR, FTSQUERY_PHRASE,
    SQLITE_FTS3_MAX_EXPR_DEPTH,
};
use super::main::eval_phrase_cleanup;

/// `SQLITE_FTS3_DEFAULT_NEAR_PARAM`: a distância padrão de um NEAR.
const SQLITE_FTS3_DEFAULT_NEAR_PARAM: i32 = 10;

/// `ParseContext`.
struct ParseContext<'a> {
    /// `pTokenizer`.
    p_tokenizer: &'a dyn Fts3Tokenizer,
    /// `iLangid`: o idioma usado com o tokenizador.
    i_langid: i32,
    /// `azCol`/`nCol`: os nomes das colunas da tabela.
    az_col: &'a [Vec<u8>],
    /// `bFts4`: permite a sintaxe só do FTS4.
    b_fts4: bool,
    /// `iDefaultCol`: a coluna padrão da consulta.
    i_default_col: i32,
    /// `nNest`: o número de parênteses aninhados.
    n_nest: i32,
    /// A arena dos nós criados.
    tree: Fts3ExprTree,
}

/// `fts3isspace`: o `isspace` sem o comportamento indefinido de valores fora de `unsigned char`.
fn fts3_isspace(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0B || c == 0x0C
}

/// `sqlite3Fts3OpenTokenizer`: abre um cursor do tokenizador sobre `z` e o configura para o
/// idioma `i_langid` (se o tokenizador suporta `xLanguageid`). Fechar é soltar o cursor.
pub fn fts3_open_tokenizer(
    tokenizer: &dyn Fts3Tokenizer,
    i_langid: i32,
    z: &[u8],
) -> Result<Box<dyn Fts3TokenizerCursor>, i32> {
    let mut csr = tokenizer.open(z)?;
    if tokenizer.i_version() >= 1 {
        let rc = csr.language_id(i_langid);
        if rc != SQLITE_OK {
            return Err(rc);
        }
    }
    Ok(csr)
}

/// `getNextToken`: extrai o próximo token de `z` (todo o texto que resta) com o tokenizador e as
/// demais informações de `p`, e cria um nó FTSQUERY_PHRASE com esse único token. Se o fim do
/// texto vem antes de um token, `*pp_expr` fica `None`. `*pn_consumed` recebe os bytes consumidos.
fn get_next_token(
    p: &mut ParseContext<'_>,
    i_col: i32,
    z: &[u8],
    pp_expr: &mut Option<ExprId>,
    pn_consumed: &mut i32,
) -> i32 {
    let n = z.len();
    let mut p_ret: Option<ExprId> = None;

    /* `i` é o número máximo de bytes de entrada a tokenizar. */
    let mut i = 0usize;
    while i < n {
        if z[i] == b'(' || z[i] == b')' {
            break;
        }
        if z[i] == b'"' {
            break;
        }
        i += 1;
    }

    *pn_consumed = i as i32;
    let rc = match fts3_open_tokenizer(p.p_tokenizer, p.i_langid, &z[..i]) {
        Err(rc) => rc,
        Ok(mut csr) => match csr.next() {
            Ok(tok) => {
                let mut token =
                    Fts3PhraseToken { z: tok.z.to_vec(), ..Fts3PhraseToken::default() };
                let mut i_start = tok.i_start_offset;
                let mut i_end = tok.i_end_offset;

                if (i_end as usize) < n && z[i_end as usize] == b'*' {
                    token.is_prefix = true;
                    i_end += 1;
                }

                while p.b_fts4 && i_start > 0 && at(z, (i_start - 1) as usize) == b'^' {
                    token.b_first = true;
                    i_start -= 1;
                }

                let phrase = Fts3Phrase {
                    i_column: i_col,
                    a_token: vec![token],
                    ..Fts3Phrase::default()
                };
                let expr = Fts3Expr {
                    e_type: FTSQUERY_PHRASE,
                    p_phrase: Some(Box::new(phrase)),
                    ..Fts3Expr::default()
                };
                p_ret = Some(p.tree.alloc(expr));
                *pn_consumed = i_end;
                SQLITE_OK
            }
            Err(rc) => {
                if i != 0 && rc == SQLITE_DONE {
                    SQLITE_OK
                } else {
                    rc
                }
            }
        },
    };

    *pp_expr = p_ret;
    rc
}

/// `getNextString`: `z_input` é o conteúdo de uma string entre aspas de uma consulta (sem as
/// aspas). Tokeniza tudo e cria um nó FTSQUERY_PHRASE com os tokens. Devolve `SQLITE_OK` e o nó em
/// `*pp_expr`, ou o erro do tokenizador e `None`.
fn get_next_string(p: &mut ParseContext<'_>, z_input: &[u8], pp_expr: &mut Option<ExprId>) -> i32 {
    let n_input = z_input.len();
    let mut a_token: Vec<Fts3PhraseToken> = Vec::new();

    let rc = match fts3_open_tokenizer(p.p_tokenizer, p.i_langid, z_input) {
        Err(rc) => rc,
        Ok(mut csr) => loop {
            match csr.next() {
                Ok(tok) => {
                    let i_begin = tok.i_start_offset;
                    let i_end = tok.i_end_offset;
                    a_token.push(Fts3PhraseToken {
                        z: tok.z.to_vec(),
                        is_prefix: (i_end as usize) < n_input && z_input[i_end as usize] == b'*',
                        b_first: i_begin > 0 && at(z_input, (i_begin - 1) as usize) == b'^',
                        ..Fts3PhraseToken::default()
                    });
                }
                Err(rc) => break rc,
            }
        },
    };

    if rc == SQLITE_DONE {
        let phrase = Fts3Phrase { i_column: p.i_default_col, a_token, ..Fts3Phrase::default() };
        let expr = Fts3Expr {
            e_type: FTSQUERY_PHRASE,
            p_phrase: Some(Box::new(phrase)),
            ..Fts3Expr::default()
        };
        *pp_expr = Some(p.tree.alloc(expr));
        SQLITE_OK
    } else {
        *pp_expr = None;
        rc
    }
}

/// Uma palavra-chave de operador.
struct Fts3Keyword {
    /// `z`/`n`: o texto da palavra.
    z: &'static [u8],
    /// `eType`: o código.
    e_type: i32,
}

/// `aKeyword[]`: todas valem (o `parenOnly` só restringe a sintaxe antiga).
static A_KEYWORD: [Fts3Keyword; 4] = [
    Fts3Keyword { z: b"OR", e_type: FTSQUERY_OR },
    Fts3Keyword { z: b"AND", e_type: FTSQUERY_AND },
    Fts3Keyword { z: b"NOT", e_type: FTSQUERY_NOT },
    Fts3Keyword { z: b"NEAR", e_type: FTSQUERY_NEAR },
];

/// `getNextNode`: o próximo nó do texto `z`, ou `None` se chegou ao fim (SQLITE_DONE). Devolve
/// `SQLITE_OK` com o nó, `SQLITE_DONE` no fim ou num `)`, ou `SQLITE_ERROR` num erro de sintaxe.
fn get_next_node(
    p: &mut ParseContext<'_>,
    z: &[u8],
    pp_expr: &mut Option<ExprId>,
    pn_consumed: &mut i32,
) -> i32 {
    let n = z.len();

    /* Pula os espaços antes de procurar uma palavra-chave, um parêntese ou uma string entre
    ** aspas. */
    let mut z_input = 0usize;
    while z_input < n && fts3_isspace(z[z_input]) {
        z_input += 1;
    }
    let zi = &z[z_input..];
    let n_input = zi.len();
    if n_input == 0 {
        return SQLITE_DONE;
    }

    /* É uma palavra-chave? */
    for p_key in A_KEYWORD.iter() {
        let n_kw = p_key.z.len();
        if n_input >= n_kw && &zi[..n_kw] == p_key.z {
            let mut n_near = SQLITE_FTS3_DEFAULT_NEAR_PARAM;
            let mut n_key = n_kw as i32;

            /* Num NEAR, confere se há uma distância explícita. */
            if p_key.e_type == FTSQUERY_NEAR {
                debug_assert!(n_key == 4);
                if at(zi, 4) == b'/' && at(zi, 5).is_ascii_digit() {
                    n_key += 1 + fts3_read_int(&zi[(n_key + 1) as usize..], &mut n_near);
                }
            }

            /* Neste ponto é provavelmente uma palavra-chave. Mas, para ser, o byte seguinte precisa
            ** ser um espaço, um parêntese (de abrir ou de fechar), uma aspa ou o fim do texto. */
            let c_next = at(zi, n_key as usize);
            if fts3_isspace(c_next) || c_next == b'"' || c_next == b'(' || c_next == b')' || c_next == 0 {
                let expr = Fts3Expr { e_type: p_key.e_type, n_near, ..Fts3Expr::default() };
                *pp_expr = Some(p.tree.alloc(expr));
                *pn_consumed = (z_input as i32) + n_key;
                return SQLITE_OK;
            }

            /* Não era palavra-chave: o usuário escreveu um token como "ORacle". Segue. */
        }
    }

    /* É uma frase entre aspas? Então procura a aspa que fecha e passa a string inteira para
    ** `get_next_string`. O FTS3 não tem sintaxe para escapar uma aspa dentro da string. */
    if zi[0] == b'"' {
        let mut ii = 1usize;
        while ii < n_input && zi[ii] != b'"' {
            ii += 1;
        }
        *pn_consumed = (z_input + ii + 1) as i32;
        if ii == n_input {
            return SQLITE_ERROR;
        }
        return get_next_string(p, &zi[1..ii], pp_expr);
    }

    if zi[0] == b'(' {
        let mut n_consumed = 0;
        p.n_nest += 1;
        if p.n_nest > SQLITE_MAX_EXPR_DEPTH {
            return SQLITE_ERROR;
        }
        let rc = fts3_expr_parse_nodes(p, &zi[1..], pp_expr, &mut n_consumed);
        *pn_consumed = (z_input as i32) + 1 + n_consumed;
        return rc;
    } else if zi[0] == b')' {
        p.n_nest -= 1;
        *pn_consumed = (z_input as i32) + 1;
        *pp_expr = None;
        return SQLITE_DONE;
    }

    /* Se o controle chega aqui, é um token comum ou o fim do texto. Lê um token comum com o
    ** tokenizador. Antes, vê se há um especificador de coluna explícito.
    **
    ** Estranhamente, não dá para associar uma coluna a uma frase entre aspas, só a um token. Pode
    ** ter sido um acidente de implementação ou uma decisão do FTS3 original; de qualquer jeito,
    ** este módulo repete a limitação. */
    let mut i_col = p.i_default_col;
    let mut i_col_len = 0usize;
    let az_col = p.az_col;
    for (ii, z_str) in az_col.iter().enumerate() {
        let n_str = z_str.iter().position(|&c| c == 0).unwrap_or(z_str.len());
        if n_input > n_str
            && zi[n_str] == b':'
            && strnicmp(Some(&z_str[..n_str]), Some(zi), n_str as i32) == 0
        {
            i_col = ii as i32;
            i_col_len = z_input + n_str + 1;
            break;
        }
    }
    let rc = get_next_token(p, i_col, &z[i_col_len..], pp_expr, pn_consumed);
    *pn_consumed += i_col_len as i32;
    rc
}

/// `insertBinaryOperator`: acrescenta `p_new`, sempre um operador binário, à árvore cuja raiz é
/// `*pp_head` pela precedência relativa dele e dos nós já na árvore. `p_prev` é o nó inserido por
/// último. A raiz pode mudar, e então `*pp_head` é atualizado. A precedência de um operador é o
/// próprio `eType` (menor agrupa mais forte).
fn insert_binary_operator(
    t: &mut Fts3ExprTree,
    pp_head: &mut Option<ExprId>,
    p_prev: ExprId,
    p_new: ExprId,
) {
    let mut p_split = p_prev;
    while let Some(par) = t[p_split].p_parent {
        if t[par].e_type <= t[p_new].e_type {
            p_split = par;
        } else {
            break;
        }
    }

    if let Some(par) = t[p_split].p_parent {
        debug_assert!(t[par].p_right == Some(p_split));
        t[par].p_right = Some(p_new);
        t[p_new].p_parent = Some(par);
    } else {
        *pp_head = Some(p_new);
    }
    t[p_new].p_left = Some(p_split);
    t[p_split].p_parent = Some(p_new);
}

/// `fts3ExprParse`: analisa a expressão de `z`. Volta no fim do texto ou num `)` sem par. Em caso
/// de sucesso, `*pp_expr` recebe a raiz (ou `None` se não há nada) e `*pn_consumed` os bytes
/// lidos.
fn fts3_expr_parse_nodes(
    p: &mut ParseContext<'_>,
    z: &[u8],
    pp_expr: &mut Option<ExprId>,
    pn_consumed: &mut i32,
) -> i32 {
    let n = z.len() as i32;
    let mut p_ret: Option<ExprId> = None;
    let mut p_prev: Option<ExprId> = None;
    let mut n_in = n;
    let mut z_in = 0usize;
    let mut rc = SQLITE_OK;
    let mut is_require_phrase = true;
    let mut aborted = false; /* o `goto exprparse_out` do C */

    while rc == SQLITE_OK {
        let mut p_node: Option<ExprId> = None;
        let mut n_byte = 0i32;

        rc = get_next_node(p, &z[z_in..], &mut p_node, &mut n_byte);
        debug_assert!(n_byte > 0 || (rc != SQLITE_OK && p_node.is_none()));
        if rc == SQLITE_OK {
            if let Some(pn) = p_node {
                let e_type = p.tree[pn].e_type;
                let is_phrase = e_type == FTSQUERY_PHRASE || p.tree[pn].p_left.is_some();

                /* `is_require_phrase` é verdadeiro se é preciso uma frase ou uma expressão entre
                ** parênteses. Um operador binário (AND, OR, NOT ou NEAR) nesse momento é erro de
                ** sintaxe. */
                if !is_phrase && is_require_phrase {
                    p.tree.free_subtree(Some(pn), &mut |_| {});
                    rc = SQLITE_ERROR;
                    aborted = true;
                    break;
                }

                if is_phrase && !is_require_phrase {
                    /* Insere um AND implícito. */
                    debug_assert!(p_ret.is_some() && p_prev.is_some());
                    let p_and = p.tree.alloc(Fts3Expr { e_type: FTSQUERY_AND, ..Fts3Expr::default() });
                    insert_binary_operator(&mut p.tree, &mut p_ret, p_prev.unwrap_or(p_and), p_and);
                    p_prev = Some(p_and);
                }

                /* Este teste pega as tentativas de fazer um operando de NEAR ser algo que não uma
                ** frase, como `(expressão) NEAR frase` ou `frase NEAR (expressão)`. */
                if let Some(prev) = p_prev {
                    let prev_type = p.tree[prev].e_type;
                    if (e_type == FTSQUERY_NEAR && !is_phrase && prev_type != FTSQUERY_PHRASE)
                        || (e_type != FTSQUERY_PHRASE && is_phrase && prev_type == FTSQUERY_NEAR)
                    {
                        p.tree.free_subtree(Some(pn), &mut |_| {});
                        rc = SQLITE_ERROR;
                        aborted = true;
                        break;
                    }
                }

                if is_phrase {
                    if p_ret.is_some() {
                        let prev = p_prev.unwrap_or(pn);
                        debug_assert!(p.tree[prev].p_left.is_some() && p.tree[prev].p_right.is_none());
                        p.tree[prev].p_right = Some(pn);
                        p.tree[pn].p_parent = Some(prev);
                    } else {
                        p_ret = Some(pn);
                    }
                } else {
                    insert_binary_operator(&mut p.tree, &mut p_ret, p_prev.unwrap_or(pn), pn);
                }
                is_require_phrase = !is_phrase;
                p_prev = Some(pn);
            }
            debug_assert!(n_byte > 0);
        }
        debug_assert!(rc != SQLITE_OK || (n_byte > 0 && n_byte <= n_in));
        n_in -= n_byte;
        z_in += n_byte as usize;
    }

    if !aborted {
        if rc == SQLITE_DONE && p_ret.is_some() && is_require_phrase {
            rc = SQLITE_ERROR;
        }
        if rc == SQLITE_DONE {
            rc = SQLITE_OK;
        }
        *pn_consumed = n - n_in;
    }

    /* exprparse_out */
    if rc != SQLITE_OK {
        p.tree.free_subtree(p_ret, &mut |_| {});
        p_ret = None;
    }
    *pp_expr = p_ret;
    rc
}

/// `fts3ExprCheckDepth`: `SQLITE_TOOBIG` se a profundidade da árvore passa de `n_max_depth`.
fn fts3_expr_check_depth(t: &Fts3ExprTree, p: Option<ExprId>, n_max_depth: i32) -> i32 {
    let Some(id) = p else {
        return SQLITE_OK;
    };
    if n_max_depth < 0 {
        return SQLITE_TOOBIG;
    }
    let rc = fts3_expr_check_depth(t, t[id].p_left, n_max_depth - 1);
    if rc == SQLITE_OK {
        fts3_expr_check_depth(t, t[id].p_right, n_max_depth - 1)
    } else {
        rc
    }
}

/// `fts3ExprBalance`: transforma a árvore em `*pp` numa equivalente mais balanceada, no próprio
/// lugar. `n_max_depth` é a profundidade máxima da subárvore balanceada. Em sucesso `*pp` é a nova
/// raiz; em erro, a árvore é solta e `*pp` fica `None`.
fn fts3_expr_balance(t: &mut Fts3ExprTree, pp: &mut Option<ExprId>, n_max_depth: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_root: Option<ExprId> = *pp; /* raiz inicial */
    let mut p_free: Option<ExprId> = None; /* lista de nós livres, ligada por p_parent */
    let e_type = p_root.map_or(0, |r| t[r].e_type);

    if n_max_depth == 0 {
        rc = SQLITE_ERROR;
    }

    if rc == SQLITE_OK {
        if e_type == FTSQUERY_AND || e_type == FTSQUERY_OR {
            let mut ap_leaf: Vec<Option<ExprId>> = vec![None; n_max_depth as usize];

            /* `p` é a folha mais à esquerda da árvore de nós de tipo `e_type`. */
            let mut p = p_root.unwrap_or(ExprId(0));
            while t[p].e_type == e_type {
                debug_assert!(t[p].p_parent.map_or(true, |par| t[par].p_left == Some(p)));
                debug_assert!(t[p].p_left.is_some() && t[p].p_right.is_some());
                p = t[p].p_left.unwrap_or(p);
            }

            /* Este laço roda uma vez para cada folha da árvore de nós de tipo `e_type`. */
            loop {
                let p_parent = t[p].p_parent; /* o pai corrente de p */

                debug_assert!(p_parent.map_or(true, |par| t[par].p_left == Some(p)));
                t[p].p_parent = None;
                if let Some(par) = p_parent {
                    t[par].p_left = None;
                } else {
                    p_root = None;
                }
                let mut p_cur = Some(p);
                rc = fts3_expr_balance(t, &mut p_cur, n_max_depth - 1);
                if rc != SQLITE_OK {
                    break;
                }

                let mut i_lvl = 0usize;
                while p_cur.is_some() && i_lvl < n_max_depth as usize {
                    match (ap_leaf[i_lvl], p_cur, p_free) {
                        (None, cur, _) => {
                            ap_leaf[i_lvl] = cur;
                            p_cur = None;
                        }
                        (Some(left), Some(cur), Some(pf)) => {
                            t[pf].p_left = Some(left);
                            t[pf].p_right = Some(cur);
                            t[left].p_parent = Some(pf);
                            t[cur].p_parent = Some(pf);

                            p_cur = Some(pf);
                            p_free = t[pf].p_parent;
                            t[pf].p_parent = None;
                            ap_leaf[i_lvl] = None;
                        }
                        _ => unreachable!("fts3: lista de nós livres vazia"),
                    }
                    i_lvl += 1;
                }
                if p_cur.is_some() {
                    t.free_subtree(p_cur, &mut |_| {});
                    rc = SQLITE_TOOBIG;
                    break;
                }

                /* Se era a última folha, sai do laço. */
                let Some(par) = p_parent else {
                    break;
                };

                /* `p` passa a ser a próxima folha da árvore de nós de tipo `e_type`. */
                p = t[par].p_right.unwrap_or(par);
                while t[p].e_type == e_type {
                    p = t[p].p_left.unwrap_or(p);
                }

                /* Tira `par` da árvore original. */
                debug_assert!(t[par].p_parent.map_or(true, |gp| t[gp].p_left == Some(par)));
                let par_right = t[par].p_right.unwrap_or(par);
                t[par_right].p_parent = t[par].p_parent;
                if let Some(gp) = t[par].p_parent {
                    t[gp].p_left = Some(par_right);
                } else {
                    debug_assert!(p_root == Some(par));
                    p_root = Some(par_right);
                }

                /* Põe `par` na lista de nós livres: ele será um nó interno da árvore nova. */
                t[par].p_parent = p_free;
                p_free = Some(par);
            }

            if rc == SQLITE_OK {
                let mut p_new: Option<ExprId> = None;
                for leaf in ap_leaf.iter().flatten().copied() {
                    match p_new {
                        None => {
                            p_new = Some(leaf);
                            t[leaf].p_parent = None;
                        }
                        Some(cur) => {
                            let pf = p_free.unwrap_or(leaf);
                            debug_assert!(p_free.is_some());
                            t[pf].p_right = Some(cur);
                            t[pf].p_left = Some(leaf);
                            t[leaf].p_parent = Some(pf);
                            t[cur].p_parent = Some(pf);

                            p_new = Some(pf);
                            p_free = t[pf].p_parent;
                            t[pf].p_parent = None;
                        }
                    }
                }
                p_root = p_new;
            } else {
                /* Deu erro: solta o conteúdo de `ap_leaf` e a lista `p_free`. O resto é solto pela
                ** liberação de `p_root` mais abaixo. */
                for leaf in ap_leaf.iter().flatten().copied() {
                    t.free_subtree(Some(leaf), &mut |_| {});
                }
                while let Some(del) = p_free {
                    p_free = t[del].p_parent;
                    t.release(del);
                }
            }

            debug_assert!(p_free.is_none());
        } else if e_type == FTSQUERY_NOT {
            if let Some(root) = p_root {
                let p_left = t[root].p_left;
                let p_right = t[root].p_right;

                t[root].p_left = None;
                t[root].p_right = None;
                if let Some(l) = p_left {
                    t[l].p_parent = None;
                }
                if let Some(r) = p_right {
                    t[r].p_parent = None;
                }

                let mut l = p_left;
                let mut r = p_right;
                rc = fts3_expr_balance(t, &mut l, n_max_depth - 1);
                if rc == SQLITE_OK {
                    rc = fts3_expr_balance(t, &mut r, n_max_depth - 1);
                }

                if rc != SQLITE_OK {
                    t.free_subtree(r, &mut |_| {});
                    t.free_subtree(l, &mut |_| {});
                } else if let (Some(l), Some(r)) = (l, r) {
                    t[root].p_left = Some(l);
                    t[l].p_parent = Some(root);
                    t[root].p_right = Some(r);
                    t[r].p_parent = Some(root);
                }
            }
        }
    }

    if rc != SQLITE_OK {
        t.free_subtree(p_root, &mut |_| {});
        p_root = None;
    }
    *pp = p_root;
    rc
}

/// `fts3ExprParseUnbalanced`: como `sqlite3Fts3ExprParse`, mas sem o rebalanceamento e sem a
/// conferência da profundidade máxima. `Ok(None)` é o resultado do texto nulo.
fn fts3_expr_parse_unbalanced(
    p_tokenizer: &dyn Fts3Tokenizer,
    i_langid: i32,
    az_col: &[Vec<u8>],
    b_fts4: bool,
    i_default_col: i32,
    z: Option<&[u8]>,
) -> Result<Option<Fts3ExprTree>, i32> {
    let Some(z) = z else {
        return Ok(None);
    };
    let mut s_parse = ParseContext {
        p_tokenizer,
        i_langid,
        az_col,
        b_fts4,
        i_default_col,
        n_nest: 0,
        tree: Fts3ExprTree::new(),
    };
    let mut p_expr: Option<ExprId> = None;
    let mut n_parsed = 0;
    let mut rc = fts3_expr_parse_nodes(&mut s_parse, z, &mut p_expr, &mut n_parsed);
    debug_assert!(rc == SQLITE_OK || p_expr.is_none());

    /* Confere se há parênteses sem par. */
    if rc == SQLITE_OK && s_parse.n_nest != 0 {
        rc = SQLITE_ERROR;
    }
    if rc != SQLITE_OK {
        return Err(rc);
    }
    s_parse.tree.root = p_expr;
    Ok(Some(s_parse.tree))
}

/// `sqlite3Fts3ExprParse`: analisa a expressão MATCH `z` (uma cadeia C: o texto vale até o
/// primeiro NUL; `None` é o ponteiro nulo) e devolve a árvore, ou `Ok(None)` se não há expressão
/// (texto nulo ou vazio). Em erro devolve o código e, para erros de sintaxe, a mensagem em
/// `pz_err`.
///
/// `tokenizer` normaliza os tokens da consulta. `az_col` são os nomes das colunas da tabela, da
/// esquerda para a direita (o `nCol` do C é o `len()`). `i_default_col` é a coluna à esquerda do
/// `MATCH` (ou -1 se os tokens podem casar com qualquer coluna).
pub fn fts3_expr_parse(
    tokenizer: &dyn Fts3Tokenizer,
    i_langid: i32,
    az_col: &[Vec<u8>],
    b_fts4: bool,
    i_default_col: i32,
    z: Option<&[u8]>,
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Option<Fts3ExprTree>, i32> {
    let z = z.map(|z| &z[..z.iter().position(|&c| c == 0).unwrap_or(z.len())]);
    let mut result =
        fts3_expr_parse_unbalanced(tokenizer, i_langid, az_col, b_fts4, i_default_col, z);

    /* Rebalanceia a expressão e confere se a profundidade não passa de
    ** SQLITE_FTS3_MAX_EXPR_DEPTH. */
    let mut rc = SQLITE_OK;
    if let Ok(Some(tree)) = result.as_mut() {
        if tree.root.is_some() {
            let mut root = tree.root;
            rc = fts3_expr_balance(tree, &mut root, SQLITE_FTS3_MAX_EXPR_DEPTH);
            tree.root = root;
            if rc == SQLITE_OK {
                rc = fts3_expr_check_depth(tree, tree.root, SQLITE_FTS3_MAX_EXPR_DEPTH);
            }
        }
    }
    if rc != SQLITE_OK {
        result = Err(rc);
    }

    match result {
        Ok(tree) => Ok(tree.filter(|t| t.root.is_some())),
        Err(rc) => {
            let mut rc = rc;
            if rc == SQLITE_TOOBIG {
                *pz_err = mprintf(
                    b"FTS expression tree is too large (maximum depth %d)",
                    &[PrintfArg::Int(SQLITE_FTS3_MAX_EXPR_DEPTH as i64)],
                );
                rc = SQLITE_ERROR;
            } else if rc == SQLITE_ERROR {
                *pz_err = mprintf(b"malformed MATCH expression: [%s]", &[text_arg(z.unwrap_or(&[]))]);
            }
            Err(rc)
        }
    }
}

/// `sqlite3Fts3ExprFree`: solta uma árvore de expressão já avaliada: cada nó roda o
/// `sqlite3Fts3EvalPhraseCleanup` da frase dele (fecha os leitores de segmentos, o que precisa da
/// conexão) e é solto. Não usa recursão, para uma consulta enorme não estourar a pilha.
pub fn fts3_expr_free(db: &mut Connection, mut tree: Fts3ExprTree) {
    let root = tree.root.take();
    tree.free_subtree(root, &mut |node| {
        debug_assert!(node.e_type == FTSQUERY_PHRASE || node.p_phrase.is_none());
        if let Some(phrase) = node.p_phrase.as_mut() {
            eval_phrase_cleanup(db, phrase);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fts3::tokenizer1::fts3_simple_tokenizer_module;

    /// Escreve a árvore como o `exprToString` do C de teste (prefixo, `{esq} {dir}`).
    fn to_string(t: &Fts3ExprTree, id: Option<ExprId>) -> String {
        let Some(id) = id else {
            return String::new();
        };
        let e = &t[id];
        let mut s = String::new();
        match e.e_type {
            FTSQUERY_PHRASE => {
                let ph = e.p_phrase.as_ref().unwrap();
                s.push_str(&format!("PHRASE {} 0", ph.i_column));
                for tok in &ph.a_token {
                    s.push_str(&format!(" {}{}", String::from_utf8_lossy(&tok.z), if tok.is_prefix { "+" } else { "" }));
                }
                return s;
            }
            FTSQUERY_NEAR => s.push_str(&format!("NEAR/{} ", e.n_near)),
            FTSQUERY_NOT => s.push_str("NOT "),
            FTSQUERY_AND => s.push_str("AND "),
            FTSQUERY_OR => s.push_str("OR "),
            _ => {}
        }
        s.push('{');
        s.push_str(&to_string(t, e.p_left));
        s.push_str("} {");
        s.push_str(&to_string(t, e.p_right));
        s.push('}');
        s
    }

    fn parse(q: &str) -> Result<String, (i32, String)> {
        let tok = fts3_simple_tokenizer_module().create(&[]).unwrap();
        let cols = vec![b"a".to_vec(), b"b".to_vec()];
        let mut err = None;
        match fts3_expr_parse(&*tok, 0, &cols, false, 2, Some(q.as_bytes()), &mut err) {
            Ok(Some(t)) => Ok(to_string(&t, t.root)),
            Ok(None) => Ok(String::new()),
            Err(rc) => Err((rc, String::from_utf8(err.unwrap_or_default()).unwrap())),
        }
    }

    #[test]
    fn precedence_and_implicit_and() {
        assert_eq!(parse("one two").unwrap(), "AND {PHRASE 2 0 one} {PHRASE 2 0 two}");
        assert_eq!(
            parse("a OR b c").unwrap(),
            "OR {PHRASE 2 0 a} {AND {PHRASE 2 0 b} {PHRASE 2 0 c}}"
        );
        assert_eq!(parse("b:x*").unwrap(), "PHRASE 1 0 x+");
        assert_eq!(parse("\"one two\"").unwrap(), "PHRASE 2 0 one two");
        assert_eq!(parse("").unwrap(), "");
    }

    #[test]
    fn near_not_and_errors() {
        assert_eq!(parse("a NEAR/3 b").unwrap(), "NEAR/3 {PHRASE 2 0 a} {PHRASE 2 0 b}");
        assert_eq!(parse("a NOT b").unwrap(), "NOT {PHRASE 2 0 a} {PHRASE 2 0 b}");
        let (rc, msg) = parse("a OR").unwrap_err();
        assert_eq!(rc, SQLITE_ERROR);
        assert_eq!(msg, "malformed MATCH expression: [a OR]");
        assert_eq!(parse("(a").unwrap_err().0, SQLITE_ERROR);
        assert_eq!(parse("OR a").unwrap_err().0, SQLITE_ERROR);
    }
}
