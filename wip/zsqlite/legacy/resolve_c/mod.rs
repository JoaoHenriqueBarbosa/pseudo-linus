// Mesclado das partes traduzidas de resolve_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Número de tabela mágico para indicar a tabela EXCLUDED numa instrução UPSERT.
pub const EXCLUDED_TABLE_NUMBER: i32 = 2;

/// Callback do walker: aumenta a profundidade de função de agregação (`Expr.op2`) de cada
/// nó `TK_AGG_FUNCTION` em `u.n`.
fn incr_agg_depth(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION {
        if let WalkerU::N(n) = p_walker.u {
            p_expr.op2 = p_expr.op2.wrapping_add(n as u8);
        }
    }
    WRC_CONTINUE
}

/// Percorre a árvore de expressão `p_expr` e aumenta a profundidade de função de agregação
/// (o campo `Expr.op2`) em `n` em cada nó `TK_AGG_FUNCTION`. Isso é necessário ao copiar um nó
/// `TK_AGG_FUNCTION` de uma consulta externa para uma subconsulta interna.
///
/// `incr_agg_function_depth(p_expr, n)` é a rotina principal; `incr_agg_depth` é auxiliar
/// (callback do walker). Veja também `window_extra_agg_func_depth()` em window.c.
pub(crate) fn incr_agg_function_depth(p_expr: &mut Expr, n: i32) {
    if n > 0 {
        let mut w = Walker {
            p_parse: None,
            x_expr_callback: Some(incr_agg_depth),
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: WalkerU::N(n),
        };
        walk_expr(&mut w, Some(p_expr));
    }
}

/// Transforma a expressão `p_expr` num alias para a coluna `i_col` do conjunto de resultado
/// em `p_e_list`.
///
/// Se a referência for seguida por um operador COLLATE, o operador COLLATE é preservado. Por
/// exemplo:
///
///     SELECT a+b, c+d FROM t1 ORDER BY 1 COLLATE nocase;
///
/// deve ser transformado em:
///
///     SELECT a+b, c+d FROM t1 ORDER BY (a+b) COLLATE nocase;
///
/// O parâmetro `n_subquery` diz quantos níveis de subconsulta o alias está afastado da
/// expressão original. O valor usual é zero, mas pode ser maior se o alias estiver contido
/// numa subconsulta da expressão original. O campo `Expr.op2` das estruturas
/// `TK_AGG_FUNCTION` precisa ser aumentado em `n_subquery`.
pub(crate) fn resolve_alias(
    p_parse: &mut Parse,
    p_e_list: &ExprList,
    i_col: i32,
    p_expr: &mut Expr,
    n_subquery: i32,
) {
    debug_assert!(i_col >= 0 && i_col < p_e_list.n_expr);
    let p_orig = p_e_list.a[i_col as usize].p_expr.as_deref();
    debug_assert!(p_orig.is_some());
    debug_assert!(!expr_has_property(p_expr, EP_REDUCED | EP_TOKEN_ONLY));
    if p_expr.p_agg_info.is_some() {
        return;
    }
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let p_dup = expr_dup(&db, p_orig.expect("expressão do resultado"), 0);
    if db.borrow().malloc_failed != 0 {
        expr_delete(&db, p_dup);
    } else if let Some(mut dup) = p_dup {
        incr_agg_function_depth(&mut dup, n_subquery);
        if p_expr.op == TK_COLLATE {
            debug_assert!(!expr_has_property(p_expr, EP_INT_VALUE));
            let z_token = p_expr.u.z_token.clone().unwrap_or_default();
            dup = expr_add_collate_string(p_parse, dup, &z_token);
        }
        // Troca o conteúdo de `pDup` e de `pExpr` (os três memcpy do C), de modo que `p_expr`
        // passe a ter a cópia do resultado e `dup` fique com o conteúdo antigo de `p_expr`.
        std::mem::swap(&mut *dup, p_expr);
        if expr_has_property(p_expr, EP_WIN_FUNC) {
            if let Some(p_win) = p_expr.y.p_win.as_ref() {
                // No C: `pExpr->y.pWin->pOwner = pExpr`. Com a árvore em `Box` o `Weak` de
                // `Window.p_owner` não consegue apontar para este nó; o integrador reconcilia
                // o tipo de `p_owner` com a árvore (ver nota do revisor).
                let _ = p_win;
            }
        }
        expr_deferred_delete(p_parse, dup);
    }
}

/// Subconsultas guardam os nomes originais de banco, tabela e coluna do conjunto de resultado
/// em `ExprList.a[].zSpan`, na forma "BANCO.TABELA.COLUNA", e marcam o item da lista de
/// expressões com `ExprList.a[].fg.eEName` igual a `ENAME_TAB`.
///
/// Verifica se o `zSpan`/`eEName` do item passado confere com `z_db`, `z_tab` e `z_col`. Se
/// algum deles for `None`, o campo confere com qualquer coisa. Retorna verdadeiro se há
/// correspondência, ou falso caso contrário.
///
/// Subconsultas `SF_NestedFrom` também guardam uma entrada para a coluna implícita rowid (ou
/// `_rowid_`, ou `oid`) com `fg.eEName` igual a `ENAME_ROWID` e `zSpan` igual a
/// "BANCO.TABELA.<alias-de-rowid>". Esse tipo de item confere se `z_col` for um alias de
/// rowid. Se `pb_rowid` não for `None`, `*pb_rowid` vira 1 quando houver este tipo de
/// correspondência.
pub fn match_e_name(
    p_item: &ExprListItem,
    z_col: Option<&[u8]>,
    z_tab: Option<&[u8]>,
    z_db: Option<&[u8]>,
    pb_rowid: Option<&mut i32>,
) -> i32 {
    let e_e_name = p_item.fg.e_e_name;
    if e_e_name != ENAME_TAB && (e_e_name != ENAME_ROWID || pb_rowid.is_none()) {
        return 0;
    }
    debug_assert!(pb_rowid.as_ref().map_or(true, |p| **p == 0));
    let mut z_span: &[u8] = p_item.z_e_name.as_deref().unwrap_or(b"");
    // O texto do C termina em NUL; aqui o fim da fatia faz esse papel.
    let mut n = z_span.iter().position(|&c| c == b'.').unwrap_or(z_span.len());
    if let Some(db) = z_db {
        if str_n_i_cmp(z_span, db, n as i32) != 0 || db.len() > n {
            return 0;
        }
    }
    z_span = if n + 1 <= z_span.len() { &z_span[n + 1..] } else { &z_span[z_span.len()..] };
    n = z_span.iter().position(|&c| c == b'.').unwrap_or(z_span.len());
    if let Some(tab) = z_tab {
        if str_n_i_cmp(z_span, tab, n as i32) != 0 || tab.len() > n {
            return 0;
        }
    }
    z_span = if n + 1 <= z_span.len() { &z_span[n + 1..] } else { &z_span[z_span.len()..] };
    if let Some(col) = z_col {
        if e_e_name == ENAME_TAB && str_i_cmp(z_span, col) != 0 {
            return 0;
        }
        if e_e_name == ENAME_ROWID && is_rowid(col) == 0 {
            return 0;
        }
    }
    if e_e_name == ENAME_ROWID {
        if let Some(pb) = pb_rowid {
            *pb = 1;
        }
    }
    1
}

/// Retorna verdadeiro se o recurso indesejado de string entre aspas duplas deve ser suportado.
pub(crate) fn are_double_quoted_strings_enabled(db: &Sqlite3, p_top_nc: &NameContext) -> bool {
    if db.init.busy != 0 {
        return true; // Sempre suportado para schemas legados
    }
    if (p_top_nc.nc_flags & NC_ISDDL) != 0 {
        // Analisando uma instrução DDL
        if writable_schema(db) && (db.flags & SQLITE_DQS_DML) != 0 {
            return true;
        }
        (db.flags & SQLITE_DQS_DDL) != 0
    } else {
        // Analisando uma instrução DML
        (db.flags & SQLITE_DQS_DML) != 0
    }
}

/// O argumento é garantidamente um nó `Expr` não nulo do tipo `TK_COLUMN`. Retorna a máscara
/// `colUsed` apropriada.
pub fn expr_col_used(p_expr: &Expr) -> Bitmask {
    let mut n = p_expr.i_column;
    debug_assert!(expr_use_y_tab(p_expr));
    let p_ex_tab_ref = p_expr.y.p_tab.as_ref().expect("tabela da coluna");
    let p_ex_tab = p_ex_tab_ref.borrow();
    debug_assert!(n < p_ex_tab.n_col as i32);
    if (p_ex_tab.tab_flags & TF_HAS_GENERATED) != 0
        && (p_ex_tab.a_col[n as usize].col_flags & COLFLAG_GENERATED) != 0
    {
        if p_ex_tab.n_col as i32 >= BMS {
            ALLBITS
        } else {
            maskbit(p_ex_tab.n_col as u32).wrapping_sub(1)
        }
    } else {
        if n >= BMS {
            n = BMS - 1;
        }
        (1 as Bitmask) << n
    }
}

/// Cria um novo termo de expressão para a coluna especificada por `p_match` e `i_column`.
/// Anexa esse termo ao conjunto de correspondências de FULL JOIN em `*pp_list`. Cria um novo
/// `*pp_list` se este for o primeiro termo do conjunto.
///
/// Em vez do `SrcItem *pMatch` do C, recebe os três campos que a rotina lê dele (cursor,
/// tabela e tipo de junção), porque o item vive dentro do `SrcList` que o chamador mantém
/// emprestado.
pub(crate) fn extend_fj_match(
    p_parse: &mut Parse,
    pp_list: &mut Option<Box<ExprList>>,
    i_cursor: i32,
    p_tab: Option<TableRef>,
    join_type: u8,
    i_column: i16,
) {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    if let Some(mut p_new) = expr_alloc(&mut db.borrow_mut(), TK_COLUMN as i32, None, false) {
        p_new.i_table = i_cursor;
        p_new.i_column = i_column as YnVar;
        p_new.y.p_tab = p_tab;
        debug_assert!((join_type & (JT_LEFT | JT_LTORJ)) != 0);
        expr_set_property(&mut p_new, EP_CAN_BE_NULL);
        *pp_list = expr_list_append(p_parse, pp_list.take(), Some(p_new));
    }
}

/// Retorna verdadeiro se `z_tab` é um nome válido para a tabela de schema `p_tab`.
#[inline(never)]
pub(crate) fn is_valid_schema_table_name(z_tab: &[u8], p_tab: &Table, z_db: Option<&[u8]>) -> bool {
    debug_assert!(p_tab.tnum == 1);
    if str_n_i_cmp(z_tab, b"sqlite_", 7) != 0 {
        return false;
    }
    let z_legacy: &[u8] = &p_tab.z_name;
    let suffix = |z: &'static [u8]| -> &'static [u8] { &z[7..] };
    let z_tab_suffix: &[u8] = if z_tab.len() >= 7 { &z_tab[7..] } else { b"" };
    if &z_legacy[7..] == suffix(LEGACY_TEMP_SCHEMA_TABLE) {
        if str_i_cmp(z_tab_suffix, suffix(PREFERRED_TEMP_SCHEMA_TABLE)) == 0 {
            return true;
        }
        if z_db.is_none() {
            return false;
        }
        if str_i_cmp(z_tab_suffix, suffix(LEGACY_SCHEMA_TABLE)) == 0 {
            return true;
        }
        if str_i_cmp(z_tab_suffix, suffix(PREFERRED_SCHEMA_TABLE)) == 0 {
            return true;
        }
    } else if str_i_cmp(z_tab_suffix, suffix(PREFERRED_SCHEMA_TABLE)) == 0 {
        return true;
    }
    false
}

// Nota para o integrador: `lookupName()` atravessa a fronteira entre este trecho e o seguinte
// no C (a função começa na linha 278 de `resolve_c.000.c` e termina em `resolve_c.001.c`).
// Como uma função Rust não pode ser dividida entre arquivos, a tradução completa de
// `lookup_name` fica em `part_001.rs`.


// ---- part_001.rs ----

/// Retorna o contexto de nomes que está `depth` níveis acima de `p_top_nc` (0 é o próprio
/// `p_top_nc`). Substitui o ponteiro `pNC` do C, que avançava por `pNC = pNC->pNext`.
pub(crate) fn nc_at(p_top_nc: &mut NameContext, depth: usize) -> &mut NameContext {
    let mut nc = p_top_nc;
    for _ in 0..depth {
        nc = nc.p_next.as_deref_mut().expect("contexto de nomes externo");
    }
    nc
}

/// Retorna o item da lista FROM apontado por `p_match`, um par (nível do contexto, índice).
fn match_src_item(p_top_nc: &mut NameContext, p_match: (usize, usize)) -> &mut SrcItem {
    &mut nc_at(p_top_nc, p_match.0)
        .p_src_list
        .as_mut()
        .expect("lista FROM do contexto")
        .a[p_match.1]
}

/// Copia os campos de `pMatch` que `extend_fj_match` e o resto de `lookup_name` leem: o cursor,
/// a tabela e o tipo de junção.
fn match_item_info(p_top_nc: &mut NameContext, p_match: (usize, usize)) -> (i32, Option<TableRef>, u8) {
    let p_item = match_src_item(p_top_nc, p_match);
    (p_item.i_cursor, p_item.p_tab.clone(), p_item.fg.jointype)
}

/// Dois schemas são o mesmo se são ambos ausentes ou se apontam para o mesmo objeto.
fn same_schema(p_a: &Option<Weak<RefCell<Schema>>>, p_b: &Option<SchemaRef>) -> bool {
    match (p_a, p_b) {
        (None, None) => true,
        (Some(a), Some(b)) => Weak::ptr_eq(a, &Rc::downgrade(b)),
        _ => false,
    }
}

/// Chave de identidade de um nó, no lugar do ponteiro `void*` que `sqlite3RenameTokenRemap()`
/// recebe no C.
pub(crate) fn rename_key<T>(p: &T) -> usize {
    p as *const T as usize
}

/// Verdadeiro se o item da lista FROM não é ligado à coluna `z_col` por uma cláusula USING.
fn item_not_using_col(p_item: &SrcItem, z_col: &[u8]) -> bool {
    p_item.fg.is_using == 0
        || match &p_item.u3 {
            SrcItemU3::Using(p_using) => id_list_index(p_using, z_col) < 0,
            SrcItemU3::On(_) => true,
        }
}

/// Trata o caso em que um segundo item da lista FROM tem a coluna procurada (`cnt>0`). Este
/// trecho aparece duas vezes em `lookupName()` no C (subconsultas SF_NestedFrom e colunas
/// comuns); aqui é uma só função. Retorna verdadeiro quando o chamador deve dar `continue`.
fn resolve_duplicate_match(
    p_parse: &mut Parse,
    p_top_nc: &mut NameContext,
    depth: usize,
    i: usize,
    z_col: &[u8],
    cnt: &mut i32,
    p_fj_match: &mut Option<Box<ExprList>>,
    p_match: Option<(usize, usize)>,
    i_column: i16,
) -> bool {
    let (not_using, join_type) = {
        let p_item = &nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM do contexto").a[i];
        (item_not_using_col(p_item, z_col), p_item.fg.jointype)
    };
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    if not_using {
        // Duas ou mais tabelas têm o mesmo nome de coluna, não ligado por USING. Isso é um
        // erro. Sinaliza limpando `p_fj_match` e deixando `cnt` passar de 1.
        expr_list_delete(&db, p_fj_match.take());
    } else if (join_type & JT_RIGHT) == 0 {
        // Um INNER ou LEFT JOIN. Usa a tabela mais à esquerda.
        return true;
    } else if (join_type & JT_LEFT) == 0 {
        // Um RIGHT JOIN. Usa a tabela mais à direita.
        *cnt = 0;
        expr_list_delete(&db, p_fj_match.take());
    } else {
        // Para um FULL JOIN, é preciso construir uma função coalesce().
        let (i_cursor, p_tab, match_join_type) =
            match_item_info(p_top_nc, p_match.expect("correspondência anterior"));
        extend_fj_match(p_parse, p_fj_match, i_cursor, p_tab, match_join_type, i_column);
    }
    false
}

/// Dado o nome de uma coluna da forma X.Y.Z, Y.Z ou apenas Z, procura esse nome no conjunto de
/// tabelas de origem em `pSrcList` e faz o nó `p_expr` apontar de volta para essa coluna de
/// origem. As seguintes mudanças são feitas em `p_expr`:
///
/// - `iDb`: índice em `db->aDb[]` do banco X (mesmo que X seja implícito).
/// - `iTable`: número de cursor da tabela obtida da lista de origem.
/// - `y.pTab`: tabela X.Y (mesmo que X e/ou Y sejam implícitos).
/// - `iColumn`: número da coluna dentro da tabela.
/// - `op`: passa a `TK_COLUMN`.
/// - `pLeft` e `pRight`: o que apontavam é apagado.
///
/// `z_db` é o nome do banco (o "X"); pode ser `None`, o que significa nome da forma Y.Z ou Z.
/// `z_tab` é o nome da tabela (o "Y"); pode ser `None` se `z_db` também for. Se `z_tab` for
/// `None`, o nome é da forma Z e vale a coluna de qualquer tabela.
///
/// Se o nome não puder ser resolvido sem ambiguidade, deixa uma mensagem de erro em `p_parse`
/// e retorna `WRC_ABORT`. Retorna `WRC_PRUNE` em caso de sucesso.
///
/// A função cruza dois trechos do C (`resolve_c.000.c` e `resolve_c.001.c`); a tradução
/// inteira está aqui. O `pNC` do C é o par (`p_top_nc`, `depth`) e `pMatch` é o par
/// (nível, índice) do item na lista FROM.
///
/// No C o parâmetro `pRight` aponta para o próprio `p_expr` (caso `TK_ID`) ou para um filho
/// dele (caso `TK_DOT`), e a rotina apaga os filhos de `p_expr`. Como o Rust não admite essa
/// aliasing, o chamador entrega só o que a rotina lê de `pRight`: o texto do nome da coluna
/// (`z_col_in`, o `pRight->u.zToken`) e se o token estava entre aspas duplas
/// (`b_right_dbl_quoted`, o `ExprHasProperty(pRight, EP_DblQuoted)`).
pub(crate) fn lookup_name(
    p_parse: &mut Parse,
    z_db: Option<&[u8]>,
    z_tab: Option<&[u8]>,
    z_col_in: &[u8],
    b_right_dbl_quoted: bool,
    p_top_nc: &mut NameContext,
    p_expr: &mut Expr,
) -> i32 {
    let mut cnt: i32 = 0; // Número de nomes de coluna que casam
    let mut cnt_tab: i32 = 0; // Número de possíveis casamentos de "rowid"
    let mut n_subquery: i32 = 0; // Quantos níveis de subconsulta
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let mut p_match: Option<(usize, usize)> = None; // Item da lista FROM que casou
    let mut depth: usize = 0; // Posição de pNC a partir de p_top_nc
    let mut p_schema: Option<SchemaRef> = None; // Schema da expressão
    let mut e_new_expr_op: u8 = TK_COLUMN; // Novo valor de pExpr->op em caso de sucesso
    let mut p_fj_match: Option<Box<ExprList>> = None; // Casamentos de FULL JOIN .. USING
    let z_col: &[u8] = z_col_in;
    let mut z_db: Option<Vec<u8>> = z_db.map(|z| z.to_vec());

    debug_assert!(z_db.is_none() || z_tab.is_some());
    debug_assert!(!expr_has_property(p_expr, EP_TOKEN_ONLY | EP_REDUCED));

    // Inicializa o nó como sem correspondência.
    p_expr.i_table = -1;

    // Traduz o nome de schema em `z_db` para o schema correspondente. Se não for achado,
    // `p_schema` continua `None` e nada vai casar, o que resulta na mensagem de erro adequada
    // perto do fim desta rotina.
    if let Some(z_db_name) = z_db.clone() {
        if (p_top_nc.nc_flags & (NC_PARTIDX | NC_ISCHECK)) != 0 {
            // Ignora em silêncio qualificadores de banco dentro de restrições CHECK e índices
            // parciais. Não levanta erro porque isso poderia quebrar código legado e porque
            // ignorar o nome do banco não prejudica nada.
            z_db = None;
        } else {
            let db_ref = db.borrow();
            let n_db = db_ref.n_db as usize;
            let mut i = 0;
            while i < n_db {
                debug_assert!(db_ref.a_db[i].z_db_sname.is_some());
                if str_i_cmp(db_ref.a_db[i].z_db_sname.as_deref().unwrap_or(b""), &z_db_name) == 0 {
                    p_schema = db_ref.a_db[i].p_schema.clone();
                    break;
                }
                i += 1;
            }
            if i == n_db && str_i_cmp(b"main", &z_db_name) == 0 {
                // Este ramo é tomado quando o banco principal foi renomeado com
                // SQLITE_DBCONFIG_MAINDBNAME.
                p_schema = db_ref.a_db[0].p_schema.clone();
                z_db = db_ref.a_db[0].z_db_sname.clone();
            }
        }
    }

    // Começa no contexto mais interno e vai para fora até achar um casamento.
    debug_assert!(cnt == 0);
    'lookupname_end: {
        loop {
            let nc_flags = nc_at(p_top_nc, depth).nc_flags;
            let has_src_list = nc_at(p_top_nc, depth).p_src_list.is_some();

            if has_src_list {
                let n_src = nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM").n_src;
                for i in 0..n_src as usize {
                    let p_tab_ref: TableRef = nc_at(p_top_nc, depth)
                        .p_src_list
                        .as_ref()
                        .expect("lista FROM")
                        .a[i]
                        .p_tab
                        .clone()
                        .expect("tabela do item");
                    let p_tab = p_tab_ref.borrow();
                    debug_assert!(!p_tab.z_name.is_empty());
                    debug_assert!(p_tab.n_col > 0 || p_parse.n_err != 0);
                    let nested_from = {
                        let p_item = &nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM").a[i];
                        debug_assert!((p_item.fg.is_nested_from != 0) == is_nested_from(p_item.p_select.as_deref()));
                        p_item.fg.is_nested_from != 0
                    };
                    if nested_from {
                        // Aqui `p_item` é uma subconsulta formada a partir de um subconjunto
                        // entre parênteses dos termos da cláusula FROM. Exemplo:
                        //   .... FROM t1 LEFT JOIN (t2 RIGHT JOIN t3 USING(x)) USING(y) ...
                        //                          \_________________________/
                        //             Este p_item -------------^
                        let mut hit = false;
                        let n_expr = {
                            let p_item = &nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM").a[i];
                            debug_assert!(p_item.p_select.is_some());
                            let p_e_list = p_item
                                .p_select
                                .as_ref()
                                .expect("select do item")
                                .p_elist
                                .as_ref()
                                .expect("lista de resultado");
                            debug_assert!(p_e_list.n_expr == p_tab.n_col as i32);
                            p_e_list.n_expr
                        };
                        for j in 0..n_expr as usize {
                            let mut b_rowid: i32 = 0; // Verdadeiro se pode ser casamento de rowid
                            let matched = {
                                let p_item = &nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM").a[i];
                                let p_e_list = p_item.p_select.as_ref().expect("select do item").p_elist.as_ref().expect("lista de resultado");
                                match_e_name(&p_e_list.a[j], Some(z_col), z_tab, z_db.as_deref(), Some(&mut b_rowid))
                            };
                            if matched == 0 {
                                continue;
                            }
                            if b_rowid == 0 {
                                if cnt > 0
                                    && resolve_duplicate_match(
                                        p_parse,
                                        p_top_nc,
                                        depth,
                                        i,
                                        z_col,
                                        &mut cnt,
                                        &mut p_fj_match,
                                        p_match,
                                        p_expr.i_column as i16,
                                    )
                                {
                                    continue;
                                }
                                cnt += 1;
                                hit = true;
                            } else if cnt > 0 {
                                // Possível casamento de rowid, mas já houve um casamento real.
                                // Então este pode ser ignorado.
                                continue;
                            }
                            cnt_tab += 1;
                            p_match = Some((depth, i));
                            p_expr.i_column = j as YnVar;
                            let p_item = &mut nc_at(p_top_nc, depth).p_src_list.as_mut().expect("lista FROM").a[i];
                            let p_e_list = p_item.p_select.as_mut().expect("select do item").p_elist.as_mut().expect("lista de resultado");
                            p_e_list.a[j].fg.b_used = 1;

                            // rowid não pode fazer parte de uma cláusula USING (assert).
                            debug_assert!(b_rowid == 0 || p_e_list.a[j].fg.b_using_term == 0);
                            if p_e_list.a[j].fg.b_using_term != 0 {
                                break;
                            }
                        }
                        if hit || z_tab.is_none() {
                            continue;
                        }
                    }
                    debug_assert!(z_db.is_none() || z_tab.is_some());
                    if let Some(z_tab_name) = z_tab {
                        if z_db.is_some() {
                            if !same_schema(&p_tab.p_schema, &p_schema) {
                                continue;
                            }
                            if p_schema.is_none() && z_db.as_deref() != Some(b"*".as_slice()) {
                                continue;
                            }
                        }
                        let z_alias: Vec<u8> = nc_at(p_top_nc, depth).p_src_list.as_ref().expect("lista FROM").a[i].z_alias.clone();
                        if !z_alias.is_empty() {
                            if str_i_cmp(z_tab_name, &z_alias) != 0 {
                                continue;
                            }
                        } else if str_i_cmp(z_tab_name, &p_tab.z_name) != 0 {
                            if p_tab.tnum != 1 {
                                continue;
                            }
                            if !is_valid_schema_table_name(z_tab_name, &p_tab, z_db.as_deref()) {
                                continue;
                            }
                        }
                        debug_assert!(expr_use_y_tab(p_expr));
                        if in_rename_object(p_parse) && !z_alias.is_empty() {
                            rename_token_remap(p_parse, 0, rename_key(&p_expr.y.p_tab));
                        }
                    }
                    let h_col: u8 = str_i_hash(Some(z_col));
                    for j in 0..p_tab.n_col as usize {
                        if p_tab.a_col[j].h_name == h_col && str_i_cmp(&p_tab.a_col[j].z_cn_name, z_col) == 0 {
                            if cnt > 0
                                && resolve_duplicate_match(
                                    p_parse,
                                    p_top_nc,
                                    depth,
                                    i,
                                    z_col,
                                    &mut cnt,
                                    &mut p_fj_match,
                                    p_match,
                                    p_expr.i_column as i16,
                                )
                            {
                                continue;
                            }
                            cnt += 1;
                            p_match = Some((depth, i));
                            // Substitui o rowid (coluna -1) pela INTEGER PRIMARY KEY.
                            p_expr.i_column = if j as i32 == p_tab.i_p_key as i32 { -1 } else { (j as i16) as YnVar };
                            if nested_from {
                                src_item_column_used(match_src_item(p_top_nc, (depth, i)), j as i32);
                            }
                            break;
                        }
                    }
                    if cnt == 0 && visible_rowid(&p_tab) {
                        // `p_tab` é um possível casamento de ROWID. Anota isso e casa o ROWID
                        // depois, se for apropriado (procure "cntTab" para achar o código
                        // relacionado). Só permite casamento de ROWID se houver um único
                        // candidato. O ramo SQLITE_ALLOW_ROWID_IN_VIEW não existe no Debian.
                        cnt_tab += 1;
                        p_match = Some((depth, i));
                    }
                }
                if let Some(m) = p_match {
                    let (i_cursor, p_match_tab, match_join_type) = match_item_info(p_top_nc, m);
                    p_expr.i_table = i_cursor;
                    debug_assert!(expr_use_y_tab(p_expr));
                    p_expr.y.p_tab = p_match_tab;
                    if (match_join_type & (JT_LEFT | JT_LTORJ)) != 0 {
                        expr_set_property(p_expr, EP_CAN_BE_NULL);
                    }
                    p_schema = p_expr
                        .y
                        .p_tab
                        .as_ref()
                        .and_then(|t| t.borrow().p_schema.as_ref().and_then(|w| w.upgrade()));
                }
            } // if( pSrcList )

            // Se o nome ainda não foi resolvido, talvez seja uma referência de argumento de
            // trigger new.* ou old.*. Ou um excluded.* de um upsert. Ou uma referência na
            // cláusula RETURNING a uma tabela sendo modificada.
            if cnt == 0 && z_db.is_none() {
                let mut p_tab_opt: Option<TableRef> = None;
                if let Some(p_trigger_tab) = p_parse.p_trigger_tab.clone() {
                    let op = p_parse.e_trigger_op;
                    debug_assert!(op == TK_DELETE || op == TK_UPDATE || op == TK_INSERT);
                    if p_parse.b_returning != 0 {
                        let usable = (nc_flags & NC_UBASEREG) != 0
                            && (z_tab.is_none()
                                || str_i_cmp(z_tab.unwrap_or(b""), &p_trigger_tab.borrow().z_name) == 0
                                || is_valid_schema_table_name(z_tab.unwrap_or(b""), &p_trigger_tab.borrow(), None));
                        if usable {
                            p_expr.i_table = (op != TK_DELETE) as i32;
                            p_tab_opt = Some(p_trigger_tab);
                        }
                    } else if op != TK_DELETE && z_tab.map_or(false, |z| str_i_cmp(b"new", z) == 0) {
                        p_expr.i_table = 1;
                        p_tab_opt = Some(p_trigger_tab);
                    } else if op != TK_INSERT && z_tab.map_or(false, |z| str_i_cmp(b"old", z) == 0) {
                        p_expr.i_table = 0;
                        p_tab_opt = Some(p_trigger_tab);
                    }
                }
                if (nc_flags & NC_UUPSERT) != 0 && z_tab.is_some() {
                    if let NameContextUNC::Upsert(p_upsert) = &nc_at(p_top_nc, depth).u_nc {
                        if str_i_cmp(b"excluded", z_tab.unwrap_or(b"")) == 0 {
                            p_tab_opt = p_upsert.p_upsert_src.as_ref().expect("origem do upsert").a[0].p_tab.clone();
                            p_expr.i_table = EXCLUDED_TABLE_NUMBER;
                        }
                    }
                }

                if let Some(p_tab_ref) = p_tab_opt {
                    let p_tab = p_tab_ref.borrow();
                    let h_col: u8 = str_i_hash(Some(z_col));
                    p_schema = p_tab.p_schema.as_ref().and_then(|w| w.upgrade());
                    cnt_tab += 1;
                    let n_col = p_tab.n_col as i32;
                    let mut i_col: i32 = 0;
                    while i_col < n_col {
                        let p_col = &p_tab.a_col[i_col as usize];
                        if p_col.h_name == h_col && str_i_cmp(&p_col.z_cn_name, z_col) == 0 {
                            if i_col == p_tab.i_p_key as i32 {
                                i_col = -1;
                            }
                            break;
                        }
                        i_col += 1;
                    }
                    if i_col >= n_col && is_rowid(z_col) != 0 && visible_rowid(&p_tab) {
                        // IMP: R-51414-32910
                        i_col = -1;
                    }
                    if i_col < n_col {
                        cnt += 1;
                        p_match = None;
                        if p_expr.i_table == EXCLUDED_TABLE_NUMBER {
                            debug_assert!(expr_use_y_tab(p_expr));
                            if in_rename_object(p_parse) {
                                p_expr.i_column = i_col;
                                p_expr.y.p_tab = Some(p_tab_ref.clone());
                                e_new_expr_op = TK_COLUMN;
                            } else {
                                let reg_data = match &nc_at(p_top_nc, depth).u_nc {
                                    NameContextUNC::Upsert(p_upsert) => p_upsert.reg_data,
                                    _ => 0,
                                };
                                p_expr.i_table = reg_data + table_column_to_storage(&p_tab, i_col) as i32;
                                e_new_expr_op = TK_REGISTER;
                            }
                        } else {
                            debug_assert!(expr_use_y_tab(p_expr));
                            p_expr.y.p_tab = Some(p_tab_ref.clone());
                            if p_parse.b_returning != 0 {
                                e_new_expr_op = TK_REGISTER;
                                p_expr.op2 = TK_COLUMN;
                                p_expr.i_column = i_col;
                                let i_base_reg = match &nc_at(p_top_nc, depth).u_nc {
                                    NameContextUNC::IBaseReg(r) => *r,
                                    _ => 0,
                                };
                                p_expr.i_table = i_base_reg
                                    + (n_col + 1) * p_expr.i_table
                                    + table_column_to_storage(&p_tab, i_col) as i32
                                    + 1;
                            } else {
                                p_expr.i_column = i_col as i16 as YnVar;
                                e_new_expr_op = TK_TRIGGER;
                                if i_col < 0 {
                                    p_expr.aff_expr = SQLITE_AFF_INTEGER;
                                } else if p_expr.i_table == 0 {
                                    p_parse.oldmask |= if i_col >= 32 { 0xffffffff } else { 1u32 << i_col };
                                } else {
                                    p_parse.newmask |= if i_col >= 32 { 0xffffffff } else { 1u32 << i_col };
                                }
                            }
                        }
                    }
                }
            }

            // Talvez o nome seja uma referência ao ROWID.
            if cnt == 0
                && cnt_tab >= 1
                && p_match.is_some()
                && (nc_flags & (NC_IDXEXPR | NC_GENCOL)) == 0
                && is_rowid(z_col) != 0
            {
                let m = p_match.expect("casamento");
                let (rowid_ok, match_nested_from) = {
                    let p_item = match_src_item(p_top_nc, m);
                    let visible = p_item.p_tab.as_ref().map_or(false, |t| visible_rowid(&t.borrow()));
                    (visible || p_item.fg.is_nested_from != 0, p_item.fg.is_nested_from != 0)
                };
                debug_assert!(rowid_ok);
                if rowid_ok {
                    cnt = cnt_tab;
                    if !match_nested_from {
                        p_expr.i_column = -1;
                    }
                    p_expr.aff_expr = SQLITE_AFF_INTEGER;
                }
            }

            // Se a entrada é da forma Z (não Y.Z nem X.Y.Z), o nome Z pode se referir a um
            // alias do conjunto de resultado. Isso acontece, por exemplo, ao resolver nomes na
            // cláusula WHERE do comando:
            //
            //     SELECT a+b AS x FROM table WHERE x<10;
            //
            // Nesses casos, substitui `p_expr` por uma cópia da expressão que forma a entrada
            // do conjunto de resultado ("a+b" no exemplo) e retorna de imediato. A expressão do
            // resultado já deve ter sido resolvida quando a cláusula WHERE é resolvida.
            //
            // Usar uma coluna do resultado no WHERE, GROUP BY ou HAVING, ou como parte de uma
            // expressão maior no ORDER BY, não é SQL padrão. É uma extensão (esquisita) do
            // SQLite mantida só por compatibilidade com o passado.
            if cnt == 0 && (nc_flags & NC_UELIST) != 0 && z_tab.is_none() {
                let p_e_list: &ExprList = match &nc_at(p_top_nc, depth).u_nc {
                    NameContextUNC::EList(l) => l,
                    _ => panic!("uNC.pEList deveria estar em uso"),
                };
                for j in 0..p_e_list.n_expr as usize {
                    let z_as = p_e_list.a[j].z_e_name.as_deref();
                    if p_e_list.a[j].fg.e_e_name == ENAME_NAME && stricmp(z_as, Some(z_col)) == 0 {
                        debug_assert!(p_expr.p_left.is_none() && p_expr.p_right.is_none());
                        debug_assert!(!expr_use_x_list(p_expr) || p_expr.x.p_list.is_none());
                        debug_assert!(!expr_use_x_select(p_expr) || p_expr.x.p_select.is_none());
                        let p_orig: &Expr = p_e_list.a[j].p_expr.as_deref().expect("expressão do resultado");
                        if (nc_flags & NC_ALLOWAGG) == 0 && expr_has_property(p_orig, EP_AGG) {
                            error_msg(
                                p_parse,
                                b"misuse of aliased aggregate %s",
                                &[PrintfArg::Text(z_as.unwrap_or(b""))],
                            );
                            return WRC_ABORT;
                        }
                        if expr_has_property(p_orig, EP_WIN) && ((nc_flags & NC_ALLOWWIN) == 0 || depth != 0) {
                            error_msg(
                                p_parse,
                                b"misuse of aliased window function %s",
                                &[PrintfArg::Text(z_as.unwrap_or(b""))],
                            );
                            return WRC_ABORT;
                        }
                        if expr_vector_size(p_orig) != 1 {
                            error_msg(p_parse, b"row value misused", &[]);
                            return WRC_ABORT;
                        }
                        resolve_alias(p_parse, p_e_list, j as i32, p_expr, n_subquery);
                        cnt = 1;
                        p_match = None;
                        debug_assert!(z_tab.is_none() && z_db.is_none());
                        if in_rename_object(p_parse) {
                            rename_token_remap(p_parse, 0, rename_key(&*p_expr));
                        }
                        break 'lookupname_end;
                    }
                }
            }

            // Avança para o próximo contexto de nomes. O laço termina quando há um casamento
            // (`cnt>0`) ou quando acabam os contextos de nomes.
            if cnt != 0 {
                break;
            }
            if nc_at(p_top_nc, depth).p_next.is_none() {
                break;
            }
            depth += 1;
            n_subquery += 1;
        }

        // Se X e Y são nulos (só o nome de coluna Z foi dado) e o valor de Z está entre aspas
        // duplas, então Z é um literal string quando não casa com nenhum nome de coluna. Nesse
        // caso é preciso retornar já, sem mudar `p_expr`.
        //
        // Como não houve referência a contextos externos, os campos `nRef` não mudam em
        // nenhum contexto.
        if cnt == 0 && z_tab.is_none() {
            debug_assert!(p_expr.op == TK_ID);
            if expr_has_property(p_expr, EP_DBL_QUOTED) && are_double_quoted_strings_enabled(&db.borrow(), p_top_nc) {
                // Se um identificador entre aspas duplas não casa com nenhum nome de coluna
                // conhecido, trata-o como string.
                //
                // Esse truque foi posto nos primeiros dias do SQLite numa tentativa
                // equivocada de compatibilidade com o MySQL 3.x, que usava aspas duplas para
                // strings. O efeito é que nomes de identificador digitados errado viram
                // strings em silêncio em vez de causar erro, para frustração de incontáveis
                // programadores. Por enquanto só se registra um aviso.
                log(
                    SQLITE_WARNING,
                    b"double-quoted string literal: \"%w\"",
                    &[PrintfArg::Text(z_col)],
                );
                p_expr.op = TK_STRING;
                p_expr.y = ExprY::default();
                return WRC_PRUNE;
            }
            if expr_id_to_true_false(p_expr) != 0 {
                return WRC_PRUNE;
            }
        }

        // `cnt==0` significa que não houve casamento. `cnt>1` significa dois ou mais
        // casamentos.
        //
        // `cnt==0` é sempre um erro. `cnt>1` costuma ser erro, mas pode ser a repetição de
        // casamentos de um NATURAL LEFT JOIN ou de um LEFT JOIN USING.
        debug_assert!(p_fj_match.is_none() || cnt > 0);
        debug_assert!(!expr_has_property(p_expr, EP_X_IS_SELECT | EP_INT_VALUE));
        if cnt != 1 {
            if let Some(fj_n_expr) = p_fj_match.as_ref().map(|l| l.n_expr) {
                if fj_n_expr == cnt - 1 {
                    if expr_has_property(p_expr, EP_LEAF) {
                        expr_clear_property(p_expr, EP_LEAF);
                    } else {
                        expr_delete(&db, p_expr.p_left.take());
                        expr_delete(&db, p_expr.p_right.take());
                    }
                    let (i_cursor, p_match_tab, match_join_type) =
                        match_item_info(p_top_nc, p_match.expect("casamento do FULL JOIN"));
                    extend_fj_match(
                        p_parse,
                        &mut p_fj_match,
                        i_cursor,
                        p_match_tab,
                        match_join_type,
                        p_expr.i_column as i16,
                    );
                    p_expr.op = TK_FUNCTION;
                    p_expr.u.z_token = Some(b"coalesce".to_vec());
                    p_expr.x.p_list = p_fj_match.take();
                    cnt = 1;
                    break 'lookupname_end;
                } else {
                    expr_list_delete(&db, p_fj_match.take());
                }
            }
            let z_err: &[u8] = if cnt == 0 { b"no such column" } else { b"ambiguous column name" };
            if let Some(z_db_name) = z_db.as_deref() {
                error_msg(
                    p_parse,
                    b"%s: %s.%s.%s",
                    &[
                        PrintfArg::Text(z_err),
                        PrintfArg::Text(z_db_name),
                        PrintfArg::Text(z_tab.unwrap_or(b"")),
                        PrintfArg::Text(z_col),
                    ],
                );
            } else if let Some(z_tab_name) = z_tab {
                error_msg(
                    p_parse,
                    b"%s: %s.%s",
                    &[PrintfArg::Text(z_err), PrintfArg::Text(z_tab_name), PrintfArg::Text(z_col)],
                );
            } else if cnt == 0 && b_right_dbl_quoted {
                error_msg(
                    p_parse,
                    b"%s: \"%s\" - should this be a string literal in single-quotes?",
                    &[PrintfArg::Text(z_err), PrintfArg::Text(z_col)],
                );
            } else {
                error_msg(p_parse, b"%s: %s", &[PrintfArg::Text(z_err), PrintfArg::Text(z_col)]);
            }
            record_error_offset_of_expr(&mut db.borrow_mut(), Some(&*p_expr));
            p_parse.check_schema = 1;
            p_top_nc.n_nc_err += 1;
            e_new_expr_op = TK_NULL;
        }
        debug_assert!(p_fj_match.is_none());

        // Remove toda a subestrutura de `p_expr`.
        if !expr_has_property(p_expr, EP_TOKEN_ONLY | EP_LEAF) {
            expr_delete(&db, p_expr.p_left.take());
            expr_delete(&db, p_expr.p_right.take());
            expr_set_property(p_expr, EP_LEAF);
        }

        // Se uma coluna de uma tabela de `pSrcList` é referenciada, registra isso na máscara
        // `pSrcList.a[].colUsed`. A coluna 0 liga o bit 0, a coluna 1 liga o bit 1, e assim por
        // diante. O bit 63 é ligado se a 63a coluna ou qualquer posterior for usada.
        //
        // A máscara `colUsed` é uma otimização que ajuda a decidir se um índice é de
        // cobertura. A resposta correta sai mesmo que a máscara tenha bits a mais, mas é
        // importante evitar ligar bits além do número máximo de colunas da tabela (veja o
        // ticket [b92e5e8ec2cdbaa1]).
        //
        // Se uma coluna gerada é referenciada, liga os bits de todas as colunas da tabela.
        if let Some(m) = p_match {
            if p_expr.i_column >= 0 {
                let col_used = expr_col_used(p_expr);
                match_src_item(p_top_nc, m).col_used |= col_used;
            } else {
                match_src_item(p_top_nc, m).fg.rowid_used = 1;
            }
        }

        p_expr.op = e_new_expr_op;
    } // lookupname_end:

    if cnt == 1 {
        if db.borrow().x_auth.is_some() && (p_expr.op == TK_COLUMN || p_expr.op == TK_TRIGGER) {
            let p_schema_ref = p_schema.as_ref().map(|s| s.borrow());
            let p_nc = nc_at(p_top_nc, depth);
            auth_read(p_parse, p_expr, p_schema_ref.as_deref(), p_nc.p_src_list.as_deref());
        }
        // Incrementa `nRef` em todos os contextos de nomes, de `p_top_nc` até o ponto em que o
        // nome casou.
        for d in 0..=depth {
            nc_at(p_top_nc, d).n_ref += 1;
        }
        WRC_PRUNE
    } else {
        WRC_ABORT
    }
}


// ---- part_002.rs ----

/// Aloca e retorna uma expressão que carrega a coluna `i_col` da fonte de dados `i_src` da
/// lista `p_src`.
pub fn create_column_expr(
    db: &mut Sqlite3,
    p_src: &mut SrcList,
    i_src: i32,
    i_col: i32,
) -> Option<Box<Expr>> {
    let mut p = expr_alloc(db, TK_COLUMN as i32, None, false)?;
    let p_item = &mut p_src.a[i_src as usize];
    debug_assert!(expr_use_y_tab(&p));
    let p_tab_ref: TableRef = p_item.p_tab.clone().expect("tabela do item");
    p.y.p_tab = Some(p_tab_ref.clone());
    p.i_table = p_item.i_cursor;
    let p_tab = p_tab_ref.borrow();
    if p_tab.i_p_key as i32 == i_col {
        p.i_column = -1;
    } else {
        p.i_column = i_col as YnVar;
        if (p_tab.tab_flags & TF_HAS_GENERATED) != 0
            && (p_tab.a_col[i_col as usize].col_flags & COLFLAG_GENERATED) != 0
        {
            p_item.col_used = if p_tab.n_col >= 64 {
                ALLBITS
            } else {
                maskbit(p_tab.n_col as u32).wrapping_sub(1)
            };
        } else {
            p_item.col_used |= (1 as Bitmask) << (if i_col >= BMS { BMS - 1 } else { i_col });
        }
    }
    Some(p)
}

/// Relata o erro de uma expressão que não é válida para algum conjunto de valores de
/// `pNC->ncFlags` determinado por `valid_mask`.
///
/// No C a função tem 5 parâmetros (`pExpr` a invalidar e `pError` a associar ao erro), mas em
/// todos os chamadores os dois são o mesmo nó, ou `pExpr` é nulo. Como o Rust não admite as duas
/// referências ao mesmo nó, `p_expr` serve aos dois papéis e `b_invalidate` diz se `p_expr.op`
/// vira `TK_NULL` (o caso em que o `pExpr` do C era não nulo).
///
/// Como otimização, a condição quase sempre é falsa (erros são raros); por isso o teste fica fora
/// da chamada, em `resolve_not_valid` (a macro `sqlite3ResolveNotValid` do C).
pub(crate) fn not_valid_impl(
    p_parse: &mut Parse,
    p_nc: &NameContext,
    z_msg: &[u8],
    p_expr: &mut Expr,
    b_invalidate: bool,
) {
    let z_in: &[u8] = if (p_nc.nc_flags & NC_IDXEXPR) != 0 {
        b"index expressions"
    } else if (p_nc.nc_flags & NC_ISCHECK) != 0 {
        b"CHECK constraints"
    } else if (p_nc.nc_flags & NC_GENCOL) != 0 {
        b"generated columns"
    } else {
        b"partial index WHERE clauses"
    };
    error_msg(
        p_parse,
        b"%s prohibited in %s",
        &[PrintfArg::Text(z_msg), PrintfArg::Text(z_in)],
    );
    if b_invalidate {
        p_expr.op = TK_NULL;
    }
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    record_error_offset_of_expr(&mut db.borrow_mut(), Some(&*p_expr));
}

/// A macro `sqlite3ResolveNotValid()` do C: chama `not_valid_impl` se algum bit de `x` estiver
/// ligado em `p_nc.nc_flags`.
#[inline]
pub(crate) fn resolve_not_valid(
    p_parse: &mut Parse,
    p_nc: &NameContext,
    z_msg: &[u8],
    x: i32,
    p_expr: &mut Expr,
    b_invalidate: bool,
) {
    debug_assert!((x & !(NC_ISCHECK | NC_PARTIDX | NC_IDXEXPR | NC_GENCOL)) == 0);
    if (p_nc.nc_flags & x) != 0 {
        not_valid_impl(p_parse, p_nc, z_msg, p_expr, b_invalidate);
    }
}

/// A expressão `p` deve codificar um valor de ponto flutuante entre 1,0 e 0,0. Retorna 1024 vezes
/// esse valor, ou -1 se `p` não for um valor de ponto flutuante entre 1,0 e 0,0.
pub(crate) fn expr_probability(p: &Expr) -> i32 {
    let mut r: f64 = -1.0;
    if p.op != TK_FLOAT {
        return -1;
    }
    debug_assert!(!expr_has_property(p, EP_INT_VALUE));
    let z_token: &[u8] = p.u.z_token.as_deref().unwrap_or(b"");
    ato_f(z_token, &mut r, (z_token.len() & 0x3fffffff) as i32, SQLITE_UTF8);
    debug_assert!(r >= 0.0);
    if r > 1.0 {
        return -1;
    }
    (r * 134217728.0) as i32
}

/// Retorna o `NameContext` do `Walker` (o `pWalker->u.pNC` do C). Cada acesso reemprestado
/// separadamente, porque as chamadas de `walk_*` precisam do `Walker` inteiro.
pub(crate) fn walker_nc(p_walker: &mut Walker) -> &mut NameContext {
    match &mut p_walker.u {
        WalkerU::Nc(p_nc) => p_nc,
        _ => panic!("Walker.u.pNC deveria estar em uso"),
    }
}

/// Versão com `&mut` de `expr_skip_collate_and_likely`: pula qualquer operador `TK_COLLATE` e
/// qualquer função unlikely(), likelihood() ou likely() na raiz da expressão.
pub(crate) fn expr_skip_collate_and_likely_mut(mut p_expr: &mut Expr) -> &mut Expr {
    loop {
        if !expr_has_property(p_expr, EP_SKIP | EP_UNLIKELY) {
            return p_expr;
        }
        if expr_has_property(p_expr, EP_UNLIKELY) {
            debug_assert!(expr_use_x_list(p_expr));
            debug_assert!(p_expr.op == TK_FUNCTION);
            p_expr = p_expr
                .x
                .p_list
                .as_mut()
                .expect("lista de argumentos")
                .a[0]
                .p_expr
                .as_deref_mut()
                .expect("argumento de unlikely");
        } else if p_expr.op == TK_COLLATE {
            p_expr = p_expr.p_left.as_deref_mut().expect("operando de COLLATE");
        } else {
            return p_expr;
        }
    }
}

/// Trecho comum dos casos `TK_IS`/`TK_ISNOT` (que caem nele, `deliberate_fall_through`) e
/// `TK_BETWEEN`, `TK_EQ`, `TK_NE`, `TK_LT`, `TK_LE`, `TK_GT` e `TK_GE` de `resolve_expr_step`:
/// confere que os dois lados da comparação têm o mesmo tamanho de vetor.
fn resolve_check_vector_sizes(p_parse: &ParseRef, p_expr: &Expr) {
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");
    if db.borrow().malloc_failed != 0 {
        return;
    }
    debug_assert!(p_expr.p_left.is_some());
    let n_left = expr_vector_size(p_expr.p_left.as_deref().expect("operando esquerdo"));
    let n_right = if p_expr.op == TK_BETWEEN {
        debug_assert!(expr_use_x_list(p_expr));
        let p_list = p_expr.x.p_list.as_ref().expect("lista do BETWEEN");
        let mut n_right = expr_vector_size(p_list.a[0].p_expr.as_deref().expect("limite inferior"));
        if n_right == n_left {
            n_right = expr_vector_size(p_list.a[1].p_expr.as_deref().expect("limite superior"));
        }
        n_right
    } else {
        debug_assert!(p_expr.p_right.is_some());
        expr_vector_size(p_expr.p_right.as_deref().expect("operando direito"))
    };
    if n_left != n_right {
        error_msg(&mut p_parse.borrow_mut(), b"row value misused", &[]);
        record_error_offset_of_expr(&mut db.borrow_mut(), Some(p_expr));
    }
}

/// Esta rotina é o callback de `walk_expr()`.
///
/// Resolve nomes simbólicos em operadores `TK_COLUMN` para o nó atual da árvore de expressão.
/// Retorna 0 para continuar a busca árvore abaixo ou 2 para abortar a caminhada.
///
/// Também faz a verificação de erros e a resolução de nomes de função. O operador das funções de
/// agregação passa a `TK_AGG_FUNCTION`.
pub(crate) fn resolve_expr_step(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    let p_parse: ParseRef = walker_nc(p_walker).p_parse.clone().expect("o NameContext precisa do Parse");
    debug_assert!(p_walker.p_parse.as_ref().map_or(true, |w| Rc::ptr_eq(w, &p_parse)));
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");

    #[cfg(debug_assertions)]
    {
        let p_nc = walker_nc(p_walker);
        if let Some(p_src_list) = p_nc.p_src_list.as_ref() {
            if p_src_list.n_alloc > 0 {
                for i in 0..p_src_list.n_src as usize {
                    debug_assert!(
                        p_src_list.a[i].i_cursor >= 0 && p_src_list.a[i].i_cursor < p_parse.borrow().n_tab
                    );
                }
            }
        }
    }

    'switch: {
        match p_expr.op {
            // O operador especial TK_ROW significa usar o rowid da primeira coluna da cláusula
            // FROM. É usado no processamento de LIMIT e ORDER BY em UPDATE e DELETE, e no de
            // UPDATE ... FROM.
            TK_ROW => {
                let (p_tab, i_cursor) = {
                    let p_src_list = walker_nc(p_walker).p_src_list.as_ref().expect("lista FROM");
                    debug_assert!(p_src_list.n_src >= 1);
                    let p_item = &p_src_list.a[0];
                    (p_item.p_tab.clone(), p_item.i_cursor)
                };
                p_expr.op = TK_COLUMN;
                debug_assert!(expr_use_y_tab(p_expr));
                p_expr.y.p_tab = p_tab;
                p_expr.i_table = i_cursor;
                p_expr.i_column -= 1;
                p_expr.aff_expr = SQLITE_AFF_INTEGER;
            }

            // Uma otimização: tenta converter
            //
            //      "expr IS NOT NULL"  -->  "TRUE"
            //      "expr IS NULL"      -->  "FALSE"
            //
            // se for possível provar que "expr" nunca é NULL. É a "otimização de redução de força
            // do NOT NULL".
            //
            // Se a otimização ocorre, restaura também as contagens de referência dos
            // NameContext ao estado anterior à resolução da expressão "coluna" do lado esquerdo.
            // Isso evita que "coluna" conte como referenciada, o que poderia fazer um SELECT ser
            // marcado, por engano, como correlacionado.
            //
            // 2024-03-28: Cuidado com agregados. Uma coluna simples de uma tabela agregada ainda
            // pode valer NULL mesmo marcada como NOT NULL. Exemplo:
            //
            //       CREATE TABLE t1(a INT NOT NULL);
            //       SELECT a, a IS NULL, a IS NOT NULL, count(*) FROM t1;
            //
            // As expressões "a IS NULL" e "a IS NOT NULL" não podem ser otimizadas aqui porque,
            // neste ponto, ainda não se sabe se t1 está sendo agregada. É preciso assumir o pior
            // e omitir a otimização. Só é seguro aplicá-la dentro da cláusula WHERE.
            TK_NOTNULL | TK_ISNULL => {
                let mut an_ref: [i32; 8] = [0; 8];
                {
                    let mut p: Option<&NameContext> = Some(&*walker_nc(p_walker));
                    let mut i = 0;
                    while let Some(nc) = p {
                        if i >= an_ref.len() {
                            break;
                        }
                        an_ref[i] = nc.n_ref;
                        p = nc.p_next.as_deref();
                        i += 1;
                    }
                }
                walk_expr(p_walker, p_expr.p_left.as_deref_mut());
                if in_rename_object(&p_parse.borrow()) {
                    return WRC_PRUNE;
                }
                if expr_can_be_null(p_expr.p_left.as_deref().expect("operando")) != 0 {
                    // A expressão pode ser NULL. A otimização não se aplica.
                    return WRC_PRUNE;
                }
                {
                    let mut p: Option<&NameContext> = Some(&*walker_nc(p_walker));
                    while let Some(nc) = p {
                        if (nc.nc_flags & NC_WHERE) == 0 {
                            return WRC_PRUNE; // Fora de uma cláusula WHERE. Inseguro otimizar.
                        }
                        p = nc.p_next.as_deref();
                    }
                }
                debug_assert!(!expr_has_property(p_expr, EP_INT_VALUE));
                p_expr.u.i_value = (p_expr.op == TK_NOTNULL) as i32;
                p_expr.flags |= EP_INT_VALUE;
                p_expr.op = TK_INTEGER;
                {
                    let mut p: Option<&mut NameContext> = Some(walker_nc(p_walker));
                    let mut i = 0;
                    while let Some(nc) = p {
                        if i >= an_ref.len() {
                            break;
                        }
                        nc.n_ref = an_ref[i];
                        p = nc.p_next.as_deref_mut();
                        i += 1;
                    }
                }
                expr_delete(&db, p_expr.p_left.take());
                return WRC_PRUNE;
            }

            // Um nome de coluna:                    ID
            // Ou nome de tabela e de coluna:        ID.ID
            // Ou banco, tabela e coluna:            ID.ID.ID
            //
            // Os casos TK_ID e TK_DOT são combinados para haver uma só chamada a `lookup_name()`.
            // Assim o compilador pode expandi-la em linha, com ganho de tamanho e desempenho.
            TK_ID | TK_DOT => {
                let z_db: Option<Vec<u8>>;
                let z_table: Option<Vec<u8>>;
                let z_col: Vec<u8>;
                let b_right_dbl_quoted: bool;

                if p_expr.op == TK_ID {
                    z_db = None;
                    z_table = None;
                    debug_assert!(!expr_has_property(p_expr, EP_INT_VALUE));
                    z_col = p_expr.u.z_token.clone().expect("nome da coluna");
                    b_right_dbl_quoted = expr_has_property(p_expr, EP_DBL_QUOTED);
                } else {
                    resolve_not_valid(
                        &mut p_parse.borrow_mut(),
                        walker_nc(p_walker),
                        b"the \".\" operator",
                        NC_IDXEXPR | NC_GENCOL,
                        p_expr,
                        false,
                    );
                    let p_left_top = p_expr.p_left.as_deref().expect("operando esquerdo");
                    let p_right_top = p_expr.p_right.as_deref().expect("operando direito");
                    let (p_left, p_right): (&Expr, &Expr);
                    if p_right_top.op == TK_ID {
                        z_db = None;
                        p_left = p_left_top;
                        p_right = p_right_top;
                    } else {
                        debug_assert!(p_right_top.op == TK_DOT);
                        debug_assert!(!expr_has_property(p_right_top, EP_INT_VALUE));
                        z_db = p_left_top.u.z_token.clone();
                        p_left = p_right_top.p_left.as_deref().expect("tabela");
                        p_right = p_right_top.p_right.as_deref().expect("coluna");
                    }
                    debug_assert!(expr_use_u_token(p_left) && expr_use_u_token(p_right));
                    z_table = p_left.u.z_token.clone();
                    z_col = p_right.u.z_token.clone().expect("nome da coluna");
                    b_right_dbl_quoted = expr_has_property(p_right, EP_DBL_QUOTED);
                    debug_assert!(expr_use_y_tab(p_expr));
                    if in_rename_object(&p_parse.borrow()) {
                        rename_token_remap(&mut p_parse.borrow_mut(), rename_key(&*p_expr), rename_key(p_right));
                        rename_token_remap(&mut p_parse.borrow_mut(), rename_key(&p_expr.y.p_tab), rename_key(p_left));
                    }
                }
                return lookup_name(
                    &mut p_parse.borrow_mut(),
                    z_db.as_deref(),
                    z_table.as_deref(),
                    &z_col,
                    b_right_dbl_quoted,
                    walker_nc(p_walker),
                    p_expr,
                );
            }

            // Resolve nomes de função.
            TK_FUNCTION => {
                let n: i32 = p_expr.x.p_list.as_ref().map_or(0, |l| l.n_expr); // Número de argumentos
                let mut no_such_func = false; // Verdadeiro se a função não existe
                let mut wrong_num_args = false; // Verdadeiro se o número de argumentos está errado
                let mut is_agg = false; // Verdadeiro se é função de agregação
                let enc_val: u8 = enc(&db.borrow()); // A codificação do banco
                let saved_allow_flags = walker_nc(p_walker).nc_flags & (NC_ALLOWAGG | NC_ALLOWWIN);
                let p_win: Option<WindowRef> = if is_window_func(p_expr) { p_expr.y.p_win.clone() } else { None };
                debug_assert!(!expr_has_property(p_expr, EP_X_IS_SELECT | EP_INT_VALUE));
                debug_assert!(p_expr.p_left.as_ref().map_or(true, |l| l.op == TK_ORDER));
                let z_id: Vec<u8> = p_expr.u.z_token.clone().unwrap_or_default();
                let mut p_def: Option<FuncDefRef> = find_function(&mut db.borrow_mut(), &z_id, n, enc_val, false);
                if p_def.is_none() {
                    p_def = find_function(&mut db.borrow_mut(), &z_id, -2, enc_val, false);
                    if p_def.is_none() {
                        no_such_func = true;
                    } else {
                        wrong_num_args = true;
                    }
                } else {
                    let def: FuncDefRef = p_def.clone().expect("função");
                    let (def_flags, def_has_finalize, def_name_0, def_name) = {
                        let d = def.borrow();
                        (d.func_flags, d.x_finalize.is_some(), d.z_name.first().copied().unwrap_or(0), d.z_name.clone())
                    };
                    is_agg = def_has_finalize;
                    if (def_flags & SQLITE_FUNC_UNLIKELY) != 0 {
                        expr_set_property(p_expr, EP_UNLIKELY);
                        if n == 2 {
                            p_expr.i_table = expr_probability(
                                p_expr.x.p_list.as_ref().expect("lista").a[1].p_expr.as_deref().expect("argumento"),
                            );
                            if p_expr.i_table < 0 {
                                error_msg(
                                    &mut p_parse.borrow_mut(),
                                    b"second argument to %#T() must be a constant between 0.0 and 1.0",
                                    &[PrintfArg::Expr(&*p_expr)],
                                );
                                walker_nc(p_walker).n_nc_err += 1;
                            }
                        } else {
                            // EVIDENCE-OF: R-61304-29449 A função unlikely(X) equivale a
                            // likelihood(X, 0.0625).
                            // EVIDENCE-OF: R-01283-11636 A função unlikely(X) é abreviação de
                            // likelihood(X,0.0625).
                            // EVIDENCE-OF: R-36850-34127 A função likely(X) é abreviação de
                            // likelihood(X,0.9375).
                            // EVIDENCE-OF: R-53436-40973 A função likely(X) equivale a
                            // likelihood(X,0.9375).
                            // TUNING: a probabilidade de unlikely() é 0.0625; a de likely() é
                            // 0.9375.
                            p_expr.i_table = if def_name_0 == b'u' { 8388608 } else { 125829120 };
                        }
                    }
                    {
                        let auth = auth_check(&mut p_parse.borrow_mut(), SQLITE_FUNCTION, None, Some(&def_name), None);
                        if auth != SQLITE_OK {
                            if auth == SQLITE_DENY {
                                error_msg(
                                    &mut p_parse.borrow_mut(),
                                    b"not authorized to use function: %#T",
                                    &[PrintfArg::Expr(&*p_expr)],
                                );
                                walker_nc(p_walker).n_nc_err += 1;
                            }
                            p_expr.op = TK_NULL;
                            return WRC_PRUNE;
                        }
                    }
                    if (def_flags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) != 0 {
                        // Para o flag EP_ConstFunc, funções de data e hora e outras que mudam
                        // devagar valem como constantes, pois são constantes durante uma
                        // consulta. Isso permite tirá-las de laços internos.
                        expr_set_property(p_expr, EP_CONST_FUNC);
                    }
                    if (def_flags & SQLITE_FUNC_CONSTANT) == 0 {
                        // Funções claramente não determinísticas, como random(), mas também as de
                        // data e hora que usam 'now' e outras, como sqlite_version(), que podem
                        // mudar com o tempo, não servem num índice ou coluna gerada.
                        // Curiosamente, servem numa restrição CHECK. SQLServer, MySQL e
                        // PostgreSQL permitem isso.
                        resolve_not_valid(
                            &mut p_parse.borrow_mut(),
                            walker_nc(p_walker),
                            b"non-deterministic functions",
                            NC_IDXEXPR | NC_PARTIDX | NC_GENCOL,
                            p_expr,
                            false,
                        );
                    } else {
                        debug_assert!((NC_SELFREF & 0xff) == NC_SELFREF); // Precisa caber em 8 bits
                        p_expr.op2 = (walker_nc(p_walker).nc_flags & NC_SELFREF) as u8;
                        if (walker_nc(p_walker).nc_flags & NC_FROMDDL) != 0 {
                            expr_set_property(p_expr, EP_FROM_DDL);
                        }
                    }
                    if (def_flags & SQLITE_FUNC_INTERNAL) != 0
                        && p_parse.borrow().nested == 0
                        && (db.borrow().m_db_flags & DBFLAG_INTERNAL_FUNC) == 0
                    {
                        // Funções de uso interno são proibidas, a menos que o SQL esteja sendo
                        // compilado por `nested_parse()` ou que o controle de teste
                        // SQLITE_TESTCTRL_INTERNAL_FUNCTIONS tenha ativado funções internas
                        // para fins de teste.
                        no_such_func = true;
                        p_def = None;
                    } else if (def_flags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0
                        && !in_rename_object(&p_parse.borrow())
                    {
                        expr_function_usable(&mut p_parse.borrow_mut(), p_expr, &def.borrow());
                    }
                }

                let def_flags_now: u32 = p_def.as_ref().map_or(0, |d| d.borrow().func_flags);
                if !in_rename_object(&p_parse.borrow()) {
                    debug_assert!(
                        !is_agg
                            || (def_flags_now & SQLITE_FUNC_MINMAX) != 0
                            || p_def.as_ref().map_or(true, |d| {
                                let d = d.borrow();
                                (d.x_value.is_none() && d.x_inverse.is_none())
                                    || (d.x_value.is_some()
                                        && d.x_inverse.is_some()
                                        && d.x_s_func.is_some()
                                        && d.x_finalize.is_some())
                            })
                    );
                    let def_lacks_x_value = p_def.as_ref().map_or(false, |d| d.borrow().x_value.is_none());
                    let nc_flags = walker_nc(p_walker).nc_flags;
                    if def_lacks_x_value && p_win.is_some() {
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"%#T() may not be used as a window function",
                            &[PrintfArg::Expr(&*p_expr)],
                        );
                        walker_nc(p_walker).n_nc_err += 1;
                    } else if (is_agg && (nc_flags & NC_ALLOWAGG) == 0)
                        || (is_agg && (def_flags_now & SQLITE_FUNC_WINDOW) != 0 && p_win.is_none())
                        || (is_agg && p_win.is_some() && (nc_flags & NC_ALLOWWIN) == 0)
                    {
                        let z_type: &[u8] = if (def_flags_now & SQLITE_FUNC_WINDOW) != 0 || p_win.is_some() {
                            b"window"
                        } else {
                            b"aggregate"
                        };
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"misuse of %s function %#T()",
                            &[PrintfArg::Text(z_type), PrintfArg::Expr(&*p_expr)],
                        );
                        walker_nc(p_walker).n_nc_err += 1;
                        is_agg = false;
                    } else if no_such_func && db.borrow().init.busy == 0 {
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"no such function: %#T",
                            &[PrintfArg::Expr(&*p_expr)],
                        );
                        walker_nc(p_walker).n_nc_err += 1;
                    } else if wrong_num_args {
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"wrong number of arguments to function %#T()",
                            &[PrintfArg::Expr(&*p_expr)],
                        );
                        walker_nc(p_walker).n_nc_err += 1;
                    } else if !is_agg && expr_has_property(p_expr, EP_WIN_FUNC) {
                        error_msg(
                            &mut p_parse.borrow_mut(),
                            b"FILTER may not be used with non-aggregate %#T()",
                            &[PrintfArg::Expr(&*p_expr)],
                        );
                        walker_nc(p_walker).n_nc_err += 1;
                    } else if !is_agg && p_expr.p_left.is_some() {
                        expr_order_by_aggregate_error(&mut p_parse.borrow_mut(), p_expr);
                        walker_nc(p_walker).n_nc_err += 1;
                    }
                    if is_agg {
                        // Funções de janela não podem ser argumento de funções de agregação nem
                        // de outras funções de janela. Mas funções de agregação podem ser
                        // argumento de funções de janela.
                        walker_nc(p_walker).nc_flags &=
                            !(NC_ALLOWWIN | (if p_win.is_none() { NC_ALLOWAGG } else { 0 }));
                    }
                } else if expr_has_property(p_expr, EP_WIN_FUNC) || p_expr.p_left.is_some() {
                    is_agg = true;
                }
                walk_expr_list(p_walker, p_expr.x.p_list.as_deref_mut());
                if is_agg {
                    if let Some(p_left) = p_expr.p_left.as_deref_mut() {
                        debug_assert!(p_left.op == TK_ORDER);
                        debug_assert!(expr_use_x_list(p_left));
                        walk_expr_list(p_walker, p_left.x.p_list.as_deref_mut());
                    }
                    if let Some(p_win_ref) = p_win.clone() {
                        debug_assert!(expr_use_y_win(p_expr));
                        debug_assert!(p_expr.y.p_win.as_ref().map_or(false, |w| Rc::ptr_eq(w, &p_win_ref)));
                        if !in_rename_object(&p_parse.borrow()) {
                            {
                                let p_win_defn = walker_nc(p_walker)
                                    .p_win_select
                                    .as_ref()
                                    .and_then(|s| s.p_win_defn.as_deref());
                                window_update(
                                    &mut p_parse.borrow_mut(),
                                    p_win_defn,
                                    &mut p_win_ref.borrow_mut(),
                                    p_def.as_ref().expect("função de agregação"),
                                );
                            }
                            if db.borrow().malloc_failed != 0 {
                                break 'switch;
                            }
                        }
                        {
                            let mut w = p_win_ref.borrow_mut();
                            walk_expr_list(p_walker, w.p_partition.as_deref_mut());
                            walk_expr_list(p_walker, w.p_order_by.as_deref_mut());
                            walk_expr(p_walker, w.p_filter.as_deref_mut());
                        }
                        window_link(walker_nc(p_walker).p_win_select.as_deref_mut(), &p_win_ref);
                        walker_nc(p_walker).nc_flags |= NC_HASWIN;
                    } else {
                        p_expr.op = TK_AGG_FUNCTION;
                        p_expr.op2 = 0;
                        if expr_has_property(p_expr, EP_WIN_FUNC) {
                            let p_filter_win = p_expr.y.p_win.clone().expect("janela do FILTER");
                            walk_expr(p_walker, p_filter_win.borrow_mut().p_filter.as_deref_mut());
                        }
                        // Percorre os contextos externos (`pNC2` do C), pela posição.
                        let p_empty_src = SrcList { n_src: 0, n_alloc: 0, a: Vec::new() };
                        let mut p_nc2: Option<usize> = Some(0);
                        while let Some(d) = p_nc2 {
                            let (references, n_nested_select, has_next) = {
                                let nc2 = nc_at(walker_nc(p_walker), d);
                                let references = references_src_list(
                                    &p_parse,
                                    &*p_expr,
                                    nc2.p_src_list.as_deref().unwrap_or(&p_empty_src),
                                );
                                (references, nc2.n_nested_select, nc2.p_next.is_some())
                            };
                            if references != 0 {
                                break;
                            }
                            p_expr.op2 = p_expr.op2.wrapping_add((1 + n_nested_select) as u8);
                            p_nc2 = if has_next { Some(d + 1) } else { None };
                        }
                        debug_assert!(p_def.is_some() || in_rename_object(&p_parse.borrow()));
                        if let (Some(d), Some(p_def_ref)) = (p_nc2, p_def.as_ref()) {
                            let func_flags = p_def_ref.borrow().func_flags;
                            let nc2 = nc_at(walker_nc(p_walker), d);
                            p_expr.op2 = p_expr.op2.wrapping_add(nc2.n_nested_select as u8);
                            debug_assert!(SQLITE_FUNC_MINMAX == NC_MINMAXAGG as u32);
                            debug_assert!(SQLITE_FUNC_ANYORDER == NC_ORDERAGG as u32);
                            nc2.nc_flags |= NC_HASAGG
                                | (((func_flags ^ SQLITE_FUNC_ANYORDER) & (SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER))
                                    as i32);
                        }
                    }
                    walker_nc(p_walker).nc_flags |= saved_allow_flags;
                }
                // FIX ME: calcular `pExpr->affinity` pelo tipo de retorno esperado da função.
                return WRC_PRUNE;
            }

            TK_SELECT | TK_EXISTS | TK_IN => {
                if expr_use_x_select(p_expr) {
                    let n_ref = walker_nc(p_walker).n_ref;
                    debug_assert!(p_expr.x.p_select.is_some());
                    if (walker_nc(p_walker).nc_flags & NC_SELFREF) != 0 {
                        not_valid_impl(&mut p_parse.borrow_mut(), walker_nc(p_walker), b"subqueries", p_expr, true);
                    } else {
                        walk_select(p_walker, p_expr.x.p_select.as_deref_mut());
                    }
                    debug_assert!(walker_nc(p_walker).n_ref >= n_ref);
                    if n_ref != walker_nc(p_walker).n_ref {
                        expr_set_property(p_expr, EP_VAR_SELECT);
                        p_expr.x.p_select.as_mut().expect("subselect").sel_flags |= SF_CORRELATED;
                    }
                    walker_nc(p_walker).nc_flags |= NC_SUBQUERY;
                }
            }

            TK_VARIABLE => {
                resolve_not_valid(
                    &mut p_parse.borrow_mut(),
                    walker_nc(p_walker),
                    b"parameters",
                    NC_ISCHECK | NC_PARTIDX | NC_IDXEXPR | NC_GENCOL,
                    p_expr,
                    true,
                );
            }

            TK_IS | TK_ISNOT => {
                debug_assert!(!expr_has_property(p_expr, EP_REDUCED));
                // Trata os casos especiais "x IS TRUE", "x IS FALSE", "x IS NOT TRUE" e
                // "x IS NOT FALSE".
                let p_right = expr_skip_collate_and_likely_mut(p_expr.p_right.as_deref_mut().expect("operando direito"));
                if p_right.op == TK_ID || p_right.op == TK_TRUEFALSE {
                    let rc = resolve_expr_step(p_walker, p_right);
                    if rc == WRC_ABORT {
                        return WRC_ABORT;
                    }
                    if p_right.op == TK_TRUEFALSE {
                        p_expr.op2 = p_expr.op;
                        p_expr.op = TK_TRUTH;
                        return WRC_CONTINUE;
                    }
                }
                // Sem break: cai no teste de tamanho de vetor dos operadores de comparação.
                resolve_check_vector_sizes(&p_parse, p_expr);
            }

            TK_BETWEEN | TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
                resolve_check_vector_sizes(&p_parse, p_expr);
            }

            _ => {}
        }
    }
    debug_assert!(db.borrow().malloc_failed == 0 || p_parse.borrow().n_err != 0);
    if p_parse.borrow().n_err != 0 {
        WRC_ABORT
    } else {
        WRC_CONTINUE
    }
}


// ---- part_003.rs ----

/// Lê `ExprList.a[].u.x.iOrderByCol` de um item.
fn get_order_by_col(p_item: &ExprListItem) -> u16 {
    match p_item.u {
        ExprListItemU::X { i_order_by_col, .. } => i_order_by_col,
        ExprListItemU::IConstExprReg(_) => 0,
    }
}

/// Escreve `ExprList.a[].u.x.iOrderByCol` de um item.
fn set_order_by_col(p_item: &mut ExprListItem, i_order_by_col_new: u16) {
    match &mut p_item.u {
        ExprListItemU::X { i_order_by_col, .. } => *i_order_by_col = i_order_by_col_new,
        u => *u = ExprListItemU::X { i_order_by_col: i_order_by_col_new, i_alias: 0 },
    }
}

/// `p_e_list` é uma lista de expressões que formam o conjunto de resultado de um SELECT. `p_e` é
/// um termo de uma cláusula ORDER BY ou GROUP BY. Esta rotina verifica se `p_e` é um
/// identificador simples que corresponde ao nome AS de um dos termos da lista. Se for, retorna um
/// inteiro entre 1 e N (N é o número de elementos de `p_e_list`) que corresponde à entrada
/// casada. Se não houver casamento, ou se `p_e` não for um identificador simples, retorna 0.
///
/// `p_e_list` já foi resolvida. `p_e` não foi.
pub(crate) fn resolve_as_name(_p_parse: &ParseRef, p_e_list: &ExprList, p_e: &Expr) -> i32 {
    if p_e.op == TK_ID {
        debug_assert!(!expr_has_property(p_e, EP_INT_VALUE));
        let z_col: &[u8] = p_e.u.z_token.as_deref().unwrap_or(b"");
        for i in 0..p_e_list.n_expr as usize {
            if p_e_list.a[i].fg.e_e_name == ENAME_NAME
                && stricmp(p_e_list.a[i].z_e_name.as_deref(), Some(z_col)) == 0
            {
                return i as i32 + 1;
            }
        }
    }
    0
}

/// `p_e` é uma expressão que é um único termo do ORDER BY de um SELECT composto. A expressão não
/// teve os nomes resolvidos.
///
/// No ponto em que esta rotina é chamada já se sabe que o termo do ORDER BY não é um índice
/// inteiro no conjunto de resultado; esse caso é tratado pela rotina chamadora.
///
/// Tenta casar `p_e` com colunas do resultado do SELECT mais à esquerda. Retorna o índice i da
/// coluna que casou, para indicar ao chamador que deve ordenar pela i-ésima coluna. A coluna mais
/// à esquerda é 1. Em outras palavras, o valor retornado é o mesmo inteiro que se usaria na
/// instrução SQL para indicar a coluna.
///
/// Se não houver casamento, retorna 0. Retorna -1 se ocorrer um erro.
///
/// O `NameContext` do C aponta para as listas do SELECT; aqui ele as possui, então elas saem do
/// `Select` enquanto os nomes são resolvidos e voltam para ele logo depois.
pub(crate) fn resolve_order_by_term_to_expr_list(
    p_parse: &ParseRef,
    p_select: &mut Select,
    p_e: &mut Expr,
) -> i32 {
    let mut i: i32 = 0;
    debug_assert!(expr_is_integer(p_e, &mut i) == 0);

    // Resolve todos os nomes na expressão do termo do ORDER BY.
    let mut nc = NameContext {
        p_parse: Some(p_parse.clone()),
        p_src_list: p_select.p_src.take(),
        u_nc: NameContextUNC::EList(p_select.p_elist.take().expect("lista de resultado")),
        p_next: None,
        n_ref: 0,
        n_nc_err: 0,
        nc_flags: NC_ALLOWAGG | NC_UELIST | NC_NOSELECT,
        n_nested_select: 0,
        p_win_select: None,
    };
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");
    let saved_supp_err: u8 = db.borrow().suppress_err;
    db.borrow_mut().suppress_err = 1;
    let rc = resolve_expr_names(&mut nc, p_e);
    db.borrow_mut().suppress_err = saved_supp_err;
    p_select.p_src = nc.p_src_list.take();
    if let NameContextUNC::EList(p_e_list) = nc.u_nc {
        p_select.p_elist = Some(p_e_list);
    }
    if rc != 0 {
        return 0;
    }

    // Tenta casar a expressão do ORDER BY com uma expressão do conjunto de resultado. Retorna o
    // índice (base 1) da entrada do resultado que casou.
    let p_e_list = p_select.p_elist.as_ref().expect("lista de resultado");
    for i in 0..p_e_list.n_expr as usize {
        if expr_compare(None, p_e_list.a[i].p_expr.as_deref(), Some(&*p_e), -1) < 2 {
            return i as i32 + 1;
        }
    }

    // Se não casou, retorna 0.
    0
}

/// Gera o erro de termo de ORDER BY ou GROUP BY fora do intervalo.
pub(crate) fn resolve_out_of_range_error(
    p_parse: &ParseRef,
    z_type: &[u8],
    i: i32,
    mx: i32,
    p_error: Option<&Expr>,
) {
    error_msg(
        &mut p_parse.borrow_mut(),
        b"%r %s BY term out of range - should be between 1 and %d",
        &[PrintfArg::Int(i as i64), PrintfArg::Text(z_type), PrintfArg::Int(mx as i64)],
    );
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");
    record_error_offset_of_expr(&mut db.borrow_mut(), p_error);
}

/// Percorre `d` ligações `pPrior` a partir de `p_select`.
fn select_prior_at(p_select: &mut Select, d: usize) -> &mut Select {
    let mut p = p_select;
    for _ in 0..d {
        p = p.p_prior.as_deref_mut().expect("SELECT anterior do composto");
    }
    p
}

/// Quantos passos `expr_skip_collate_and_likely` dá a partir de `p_expr` até chegar ao nó que
/// retorna. O C guarda o ponteiro `pE` dentro da árvore; aqui o nó é guardado pelo caminho.
fn skip_collate_and_likely_steps(p_expr: &Expr) -> usize {
    let mut n = 0;
    let mut p = p_expr;
    loop {
        if !expr_has_property(p, EP_SKIP | EP_UNLIKELY) {
            return n;
        }
        if expr_has_property(p, EP_UNLIKELY) {
            debug_assert!(expr_use_x_list(p));
            p = p.x.p_list.as_ref().expect("lista de argumentos").a[0].p_expr.as_deref().expect("argumento");
        } else if p.op == TK_COLLATE {
            p = p.p_left.as_deref().expect("operando de COLLATE");
        } else {
            return n;
        }
        n += 1;
    }
}

/// Segue `n` passos de `expr_skip_collate_and_likely` a partir de `p_expr` (versão mutável).
fn skip_collate_and_likely_nth(p_expr: &mut Expr, n: usize) -> &mut Expr {
    let mut p = p_expr;
    for _ in 0..n {
        if expr_has_property(p, EP_UNLIKELY) {
            p = p.x.p_list.as_mut().expect("lista de argumentos").a[0].p_expr.as_deref_mut().expect("argumento");
        } else {
            p = p.p_left.as_deref_mut().expect("operando de COLLATE");
        }
    }
    p
}

/// Analisa a cláusula ORDER BY de um SELECT composto. Modifica cada termo do ORDER BY para uma
/// constante inteira entre 1 e N, onde N é o número de colunas do SELECT composto.
///
/// Termos do ORDER BY que já são um inteiro entre 1 e N não são modificados. Termos que são
/// inteiros fora do intervalo de 1 a N geram erro. Termos que são expressões são comparados com
/// as expressões do resultado do SELECT composto, começando pelo SELECT mais à esquerda e
/// avançando para a direita. No primeiro casamento a expressão do ORDER BY é transformada no
/// número inteiro da coluna.
///
/// Retorna o número de erros vistos.
///
/// O ORDER BY sai do `Select` durante o trabalho (os SELECTs do composto são percorridos e
/// alterados enquanto ele é lido) e volta a ele no fim.
pub(crate) fn resolve_compound_order_by(p_parse: &ParseRef, p_select: &mut Select) -> i32 {
    let mut p_order_by = match p_select.p_order_by.take() {
        Some(p_order_by) => p_order_by,
        None => return 0,
    };
    let rc = resolve_compound_order_by_terms(p_parse, p_select, &mut p_order_by);
    p_select.p_order_by = Some(p_order_by);
    rc
}

/// Corpo de `resolve_compound_order_by` com o ORDER BY já separado do `Select`.
fn resolve_compound_order_by_terms(p_parse: &ParseRef, p_select: &mut Select, p_order_by: &mut ExprList) -> i32 {
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");
    let n_expr = p_order_by.n_expr as usize;
    if p_order_by.n_expr > db.borrow().a_limit[SQLITE_LIMIT_COLUMN as usize] {
        error_msg(&mut p_parse.borrow_mut(), b"too many terms in ORDER BY clause", &[]);
        return 1;
    }
    for i in 0..n_expr {
        p_order_by.a[i].fg.done = 0;
    }
    p_select.p_next = None;
    // No C o laço `while(pSelect->pPrior){ pSelect->pPrior->pNext = pSelect; pSelect = pSelect->pPrior; }`
    // monta as ligações `pNext` e vai ao SELECT mais à esquerda. Aqui as ligações `pNext` não são
    // materializadas (a árvore é dona só pelos `pPrior`); o SELECT corrente é dado pela
    // distância `d` até o SELECT original, que vai de `n_prior` (o mais à esquerda) até 0.
    let mut n_prior: usize = 0;
    {
        let mut p: &Select = p_select;
        while let Some(p_prior) = p.p_prior.as_deref() {
            n_prior += 1;
            p = p_prior;
        }
    }
    let mut more_to_do = true;
    let mut dist: Option<usize> = Some(n_prior);
    while let (Some(d), true) = (dist, more_to_do) {
        more_to_do = false;
        let p_cur: &mut Select = select_prior_at(p_select, d);
        debug_assert!(p_cur.p_elist.is_some());
        for i in 0..n_expr {
            let mut i_col: i32 = -1;
            if p_order_by.a[i].fg.done != 0 {
                continue;
            }
            let n_steps = skip_collate_and_likely_steps(p_order_by.a[i].p_expr.as_deref().expect("termo do ORDER BY"));
            let is_integer = {
                let p_e = skip_collate_and_likely_nth(p_order_by.a[i].p_expr.as_deref_mut().expect("termo"), n_steps);
                expr_is_integer(p_e, &mut i_col) != 0
            };
            if is_integer {
                let n_result = p_cur.p_elist.as_ref().expect("lista de resultado").n_expr;
                if i_col <= 0 || i_col > n_result {
                    let p_e = skip_collate_and_likely_nth(p_order_by.a[i].p_expr.as_deref_mut().expect("termo"), n_steps);
                    resolve_out_of_range_error(p_parse, b"ORDER", i as i32 + 1, n_result, Some(&*p_e));
                    return 1;
                }
            } else {
                i_col = {
                    let p_e = skip_collate_and_likely_nth(p_order_by.a[i].p_expr.as_deref_mut().expect("termo"), n_steps);
                    resolve_as_name(p_parse, p_cur.p_elist.as_deref().expect("lista de resultado"), p_e)
                };
                if i_col == 0 {
                    // Testa se a expressão `p_e` casa com um dos valores retornados por
                    // `p_cur`. No caso comum isso é feito duplicando a expressão, resolvendo os
                    // símbolos nela e comparando-a com cada expressão retornada pelo SELECT.
                    // Terminadas as comparações, a expressão duplicada é apagada.
                    //
                    // Se isto roda como parte de um ALTER TABLE e os símbolos resolvem com
                    // sucesso, resolve também os símbolos da expressão real. Isso deixa o código
                    // de alter.c modificar as referências a colunas dentro da expressão do
                    // ORDER BY como for preciso.
                    let p_dup = {
                        let p_e = skip_collate_and_likely_nth(p_order_by.a[i].p_expr.as_deref_mut().expect("termo"), n_steps);
                        expr_dup(&db, &*p_e, 0)
                    };
                    let mut p_dup = p_dup;
                    if db.borrow().malloc_failed == 0 {
                        let dup = p_dup.as_deref_mut().expect("cópia da expressão");
                        i_col = resolve_order_by_term_to_expr_list(p_parse, p_cur, dup);
                        if in_rename_object(&p_parse.borrow()) && i_col > 0 {
                            let p_e = skip_collate_and_likely_nth(p_order_by.a[i].p_expr.as_deref_mut().expect("termo"), n_steps);
                            resolve_order_by_term_to_expr_list(p_parse, p_cur, p_e);
                        }
                    }
                    expr_delete(&db, p_dup);
                }
            }
            if i_col > 0 {
                // Converte o termo do ORDER BY no número de coluna inteiro `i_col`, tomando o
                // cuidado de preservar a cláusula COLLATE, se existir.
                if !in_rename_object(&p_parse.borrow()) {
                    let mut p_new = match expr(&mut db.borrow_mut(), TK_INTEGER as i32, None) {
                        Some(p_new) => p_new,
                        None => return 1,
                    };
                    p_new.flags |= EP_INT_VALUE;
                    p_new.u.i_value = i_col;
                    if n_steps == 0 {
                        let p_old = p_order_by.a[i].p_expr.replace(p_new);
                        expr_delete(&db, p_old);
                    } else {
                        // O pai de `p_e` é o último TK_COLLATE da cadeia `pLeft`.
                        let p_parent = skip_collate_and_likely_nth(
                            p_order_by.a[i].p_expr.as_deref_mut().expect("termo"),
                            n_steps - 1,
                        );
                        debug_assert!(p_parent.op == TK_COLLATE);
                        let p_old = p_parent.p_left.replace(p_new);
                        expr_delete(&db, p_old);
                    }
                    set_order_by_col(&mut p_order_by.a[i], i_col as u16);
                }
                p_order_by.a[i].fg.done = 1;
            } else {
                more_to_do = true;
            }
        }
        dist = if d == 0 { None } else { Some(d - 1) };
    }
    for i in 0..n_expr {
        if p_order_by.a[i].fg.done == 0 {
            error_msg(
                &mut p_parse.borrow_mut(),
                b"%r ORDER BY term does not match any column in the result set",
                &[PrintfArg::Int(i as i64 + 1)],
            );
            return 1;
        }
    }
    0
}

/// Verifica cada termo da cláusula ORDER BY ou GROUP BY `p_order_by` do SELECT `p_select`. Se
/// algum termo é referência a uma expressão do conjunto de resultado (como determina o campo
/// `ExprList.a.u.x.iOrderByCol`), converte esse termo numa cópia da coluna correspondente do
/// resultado.
///
/// Se algum erro for detectado, acrescenta uma mensagem de erro a `p_parse` e retorna não zero.
/// Retorna zero se não houver erros.
///
/// `p_order_by` é a cláusula já separada do `Select` pelo chamador (no C é `pSelect->pOrderBy`
/// ou `pSelect->pGroupBy`); esta rotina só lê a lista de resultado de `p_select`.
pub fn resolve_order_group_by(
    p_parse: &ParseRef,
    p_select: &Select,
    p_order_by: Option<&mut ExprList>,
    z_type: &[u8],
) -> i32 {
    let db = p_parse.borrow().db.upgrade().expect("banco deve estar ativo");
    let p_order_by = match p_order_by {
        Some(p_order_by) if db.borrow().malloc_failed == 0 && !in_rename_object(&p_parse.borrow()) => p_order_by,
        _ => return 0,
    };
    if p_order_by.n_expr > db.borrow().a_limit[SQLITE_LIMIT_COLUMN as usize] {
        error_msg(
            &mut p_parse.borrow_mut(),
            b"too many terms in %s BY clause",
            &[PrintfArg::Text(z_type)],
        );
        return 1;
    }
    let p_e_list = p_select.p_elist.as_deref().expect("sqlite3SelectNew() garante a lista de resultado");
    for i in 0..p_order_by.n_expr as usize {
        let p_item = &mut p_order_by.a[i];
        let i_order_by_col = get_order_by_col(p_item) as i32;
        if i_order_by_col != 0 {
            if i_order_by_col > p_e_list.n_expr {
                resolve_out_of_range_error(p_parse, z_type, i as i32 + 1, p_e_list.n_expr, None);
                return 1;
            }
            resolve_alias(
                &mut p_parse.borrow_mut(),
                p_e_list,
                i_order_by_col - 1,
                p_item.p_expr.as_deref_mut().expect("termo"),
                0,
            );
        }
    }
    0
}

/// Retorno de chamada do `Walker` para `window_remove_expr_from_select()`.
pub(crate) fn resolve_remove_windows_cb(p_walker: &mut Walker, p_expr: &mut Expr) -> i32 {
    if expr_has_property(p_expr, EP_WIN_FUNC) {
        let p_win: WindowRef = p_expr.y.p_win.clone().expect("janela da função");
        if let WalkerU::Select(p_select) = &mut p_walker.u {
            window_unlink_from_select(&mut p_select.p_win, &p_win);
        }
    }
    WRC_CONTINUE
}

/// Remove qualquer objeto `Window` pertencente à expressão `p_expr` da lista `Select.pWin` do
/// `Select` `p_select`.
///
/// Ponto de integração: o `Walker.u.pSelect` do C é um ponteiro emprestado; o `WalkerU::Select`
/// do cabeçalho é um `Box<Select>` dono. `walker_lend_select` e `walker_take_back_select` fazem
/// o empréstimo (movem o `Select` para o `Walker` e o devolvem), e o integrador os liga ao tipo
/// final de `WalkerU::Select`.
pub(crate) fn window_remove_expr_from_select(p_select: &mut Select, p_expr: &mut Expr) {
    if p_select.p_win.is_some() {
        let mut s_walker = Walker {
            p_parse: None,
            x_expr_callback: Some(resolve_remove_windows_cb),
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: WalkerU::None,
        };
        walker_lend_select(&mut s_walker, p_select);
        walk_expr(&mut s_walker, Some(p_expr));
        walker_take_back_select(&mut s_walker, p_select);
    }
}

/// `p_order_by` é uma cláusula ORDER BY ou GROUP BY do SELECT `p_select`. O contexto de nomes do
/// SELECT é `p_nc`. `z_type` é "ORDER" ou "GROUP", conforme o tipo da cláusula.
///
/// Esta rotina resolve cada termo da cláusula numa expressão. Se o termo é um inteiro I entre 1 e
/// N (N é o número de colunas do conjunto de resultado do SELECT), a expressão da resolução é uma
/// cópia da I-ésima expressão do resultado. Se o termo é um identificador que corresponde ao nome
/// AS de uma expressão do resultado, o termo resolve numa cópia dessa expressão. Caso contrário a
/// expressão é resolvida do jeito usual, com `resolve_expr_names()`.
///
/// Retorna o número de erros. Se houver erros, pode ficar uma mensagem de erro apropriada em
/// `p_parse` (exceto erros de falta de memória).
///
/// É a `resolveOrderGroupBy()` estática do C; leva o sufixo `_internal` porque o nome em
/// snake_case colidiria com o da pública `sqlite3ResolveOrderGroupBy()` (`resolve_order_group_by`).
pub(crate) fn resolve_order_group_by_internal(
    p_nc: &mut NameContext,
    p_select: &mut Select,
    p_order_by: &mut ExprList,
    z_type: &[u8],
) -> i32 {
    let p_parse: ParseRef = p_nc.p_parse.clone().expect("o NameContext precisa do Parse");
    let n_result = p_select.p_elist.as_ref().expect("lista de resultado").n_expr; // Termos do resultado
    for i in 0..p_order_by.n_expr as usize {
        let p_item = &mut p_order_by.a[i];
        let (i_as_col, i_int_col) = {
            let p_e2 = match expr_skip_collate_and_likely(p_item.p_expr.as_deref()) {
                Some(p_e2) => p_e2,
                None => continue,
            };
            let mut i_as_col = 0;
            if z_type[0] != b'G' {
                i_as_col = resolve_as_name(&p_parse, p_select.p_elist.as_deref().expect("lista de resultado"), p_e2);
            }
            let mut i_int_col: Option<i32> = None;
            if i_as_col <= 0 {
                let mut i_col: i32 = 0;
                if expr_is_integer(p_e2, &mut i_col) != 0 {
                    // O termo do ORDER BY é uma constante inteira. De novo, define o número da
                    // coluna para que `resolve_order_group_by()` converta o termo numa cópia da
                    // expressão do resultado.
                    if i_col < 1 || i_col > 0xffff {
                        resolve_out_of_range_error(&p_parse, z_type, i as i32 + 1, n_result, Some(p_e2));
                        return 1;
                    }
                    i_int_col = Some(i_col);
                }
            }
            (i_as_col, i_int_col)
        };
        if i_as_col > 0 {
            // Se um casamento de nome AS é achado, marca esta coluna do ORDER BY como cópia da
            // i_as_col-ésima coluna do resultado. A chamada seguinte a
            // `resolve_order_group_by()` converte a expressão numa cópia da i_as_col-ésima
            // expressão do resultado.
            set_order_by_col(p_item, i_as_col as u16);
            continue;
        }
        if let Some(i_col) = i_int_col {
            set_order_by_col(p_item, i_col as u16);
            continue;
        }

        // Caso contrário, trata o termo do ORDER BY como uma expressão comum.
        set_order_by_col(p_item, 0);
        if resolve_expr_names(p_nc, p_item.p_expr.as_deref_mut().expect("termo")) != 0 {
            return 1;
        }
        for j in 0..n_result as usize {
            let same = expr_compare(
                None,
                p_item.p_expr.as_deref(),
                p_select.p_elist.as_ref().expect("lista de resultado").a[j].p_expr.as_deref(),
                -1,
            ) == 0;
            if same {
                // Como esta expressão está sendo trocada por uma referência a uma expressão
                // idêntica do resultado, remove todos os objetos Window que pertencem a ela da
                // lista `Select.pWin`.
                window_remove_expr_from_select(p_select, p_item.p_expr.as_deref_mut().expect("termo"));
                set_order_by_col(p_item, (j + 1) as u16);
            }
        }
    }
    resolve_order_group_by(&p_parse, p_select, Some(p_order_by), z_type)
}


// ---- part_004.rs ----

/// Resolve nomes no SELECT `p` e em todos os seus descendentes.
fn resolve_select_step(walker: &mut Walker, p: &SelectRef) -> i32 {
    if (p.borrow().sel_flags & SF_RESOLVED) != 0 {
        return WRC_PRUNE;
    }
    let p_outer_nc: Option<NameContextRef> = walker.u_nc.clone();
    let p_parse = walker.p_parse.clone();
    let db = p_parse.borrow().db.clone();

    // Normalmente `select_expand()` é chamado antes e já expandiu este SELECT. Porém, se for uma
    // subconsulta dentro de uma expressão, `resolve_expr_names()` é chamado sem uma chamada
    // anterior a `select_expand()`. Nesse caso, deixa `select_prep()` fazer todo o processamento
    // deste SELECT: ele invoca `select_expand()` e esta rotina na ordem correta.
    if (p.borrow().sel_flags & SF_EXPANDED) == 0 {
        select_prep(&p_parse, p, p_outer_nc.as_ref());
        return if p_parse.borrow().n_err != 0 { WRC_ABORT } else { WRC_PRUNE };
    }

    let is_compound: i32 = if p.borrow().p_prior.is_some() { 1 } else { 0 };
    let mut n_compound: i32 = 0;
    let p_leftmost = p.clone();
    let mut cursor: Option<SelectRef> = Some(p.clone());
    while let Some(p) = cursor {
        debug_assert!((p.borrow().sel_flags & SF_EXPANDED) != 0);
        debug_assert!((p.borrow().sel_flags & SF_RESOLVED) == 0);
        p.borrow_mut().sel_flags |= SF_RESOLVED;

        // Resolve as expressões das cláusulas LIMIT e OFFSET. Elas não podem referenciar nomes,
        // então passa um NameContext vazio.
        let mut nc0 = NameContext::new(p_parse.clone());
        nc0.p_win_select = Some(p.clone());
        let s_nc: NameContextRef = Rc::new(RefCell::new(nc0));
        let p_limit = p.borrow().p_limit.clone();
        if resolve_expr_names(&s_nc, p_limit.as_ref()) != 0 {
            return WRC_ABORT;
        }

        // Se SF_CONVERTED está ligado, este Select foi criado por
        // `convert_compound_select_to_subquery()`. Nesse caso o ORDER BY (`p.p_order_by`) deve ser
        // resolvido como parte da subconsulta, não do pai. Este bloco move o `p_order_by` para a
        // subconsulta. Ele volta depois que os nomes forem resolvidos.
        if (p.borrow().sel_flags & SF_CONVERTED) != 0 {
            let p_src = p.borrow().p_src.clone().unwrap();
            let p_sub = p_src.borrow().a[0].p_select.clone().unwrap();
            debug_assert!(p_src.borrow().n_src == 1 && p.borrow().p_order_by.is_some());
            debug_assert!(p_sub.borrow().p_prior.is_some() && p_sub.borrow().p_order_by.is_none());
            let order_by = p.borrow_mut().p_order_by.take();
            p_sub.borrow_mut().p_order_by = order_by;
        }

        // Resolve recursivamente os nomes de todas as subconsultas da cláusula FROM.
        if let Some(outer) = &p_outer_nc {
            outer.borrow_mut().n_nested_select += 1;
        }
        let p_src = p.borrow().p_src.clone().unwrap();
        let n_src = p_src.borrow().n_src;
        for i in 0..n_src as usize {
            let (item_select, item_name) = {
                let sb = p_src.borrow();
                (sb.a[i].p_select.clone(), sb.a[i].z_name.clone())
            };
            debug_assert!(item_name.is_some() || item_select.is_some());
            if let Some(sub) = item_select {
                if (sub.borrow().sel_flags & SF_RESOLVED) == 0 {
                    let n_ref: i32 = match &p_outer_nc {
                        Some(outer) => outer.borrow().n_ref,
                        None => 0,
                    };
                    let z_saved_context = p_parse.borrow().z_auth_context.clone();

                    if item_name.is_some() {
                        p_parse.borrow_mut().z_auth_context = item_name.clone();
                    }
                    resolve_select_names(&p_parse, &sub, p_outer_nc.as_ref());
                    p_parse.borrow_mut().z_auth_context = z_saved_context;
                    if p_parse.borrow().n_err != 0 {
                        return WRC_ABORT;
                    }
                    debug_assert!(db.borrow().malloc_failed == 0);

                    // Se o número de referências ao contexto externo mudou quando as expressões
                    // da subconsulta foram resolvidas, a subconsulta é correlacionada. Só é
                    // preciso conferir a contagem do contexto externo mais interno, pois
                    // `lookup_name()` incrementa a contagem de todos os contextos entre o atual
                    // e o que contém a coluna quando resolve um nome.
                    if let Some(outer) = &p_outer_nc {
                        let now = outer.borrow().n_ref;
                        debug_assert!(!p_src.borrow().a[i].fg.is_correlated && now >= n_ref);
                        p_src.borrow_mut().a[i].fg.is_correlated = now > n_ref;
                    }
                }
            }
        }
        if let Some(outer) = &p_outer_nc {
            if outer.borrow().n_nested_select > 0 {
                outer.borrow_mut().n_nested_select -= 1;
            }
        }

        // Monta o contexto de nomes local passado a `resolve_expr_names()` para resolver a lista
        // de expressões do conjunto de resultados.
        {
            let mut nc = s_nc.borrow_mut();
            nc.nc_flags = NC_ALLOWAGG | NC_ALLOWWIN;
            nc.p_src_list = Some(p_src.clone());
            nc.p_next = p_outer_nc.clone();
        }

        // Resolve os nomes do conjunto de resultados.
        let p_e_list = p.borrow().p_e_list.clone();
        if resolve_expr_list_names(&s_nc, p_e_list.as_ref()) != 0 {
            return WRC_ABORT;
        }
        s_nc.borrow_mut().nc_flags &= !NC_ALLOWWIN;

        // Se não há funções agregadas no conjunto de resultados nem GROUP BY, não permite
        // agregadas nas outras expressões.
        debug_assert!((p.borrow().sel_flags & SF_AGGREGATE) == 0);
        let p_group_by = p.borrow().p_group_by.clone();
        let cur_nc_flags = s_nc.borrow().nc_flags;
        if p_group_by.is_some() || (cur_nc_flags & NC_HASAGG) != 0 {
            debug_assert!(NC_MINMAXAGG as u32 == SF_MINMAXAGG as u32);
            debug_assert!(NC_ORDERAGG as u32 == SF_ORDERBYREQD as u32);
            p.borrow_mut().sel_flags |=
                SF_AGGREGATE | ((cur_nc_flags & (NC_MINMAXAGG | NC_ORDERAGG)) as u32);
        } else {
            s_nc.borrow_mut().nc_flags &= !NC_ALLOWAGG;
        }

        // Adiciona a lista de colunas de saída ao contexto de nomes antes de analisar as outras
        // expressões do SELECT, para que expressões do WHERE (etc.) possam referenciar
        // expressões do conjunto de resultados por apelido.
        //
        // Ponto menor: nesse caso a expressão é reavaliada a cada referência.
        debug_assert!((s_nc.borrow().nc_flags & (NC_UAGGINFO | NC_UUPSERT | NC_UBASEREG)) == 0);
        {
            let mut nc = s_nc.borrow_mut();
            nc.u_nc_p_e_list = p_e_list.clone();
            nc.nc_flags |= NC_UELIST;
        }
        let p_having = p.borrow().p_having.clone();
        if p_having.is_some() {
            if (p.borrow().sel_flags & SF_AGGREGATE) == 0 {
                error_msg(&p_parse, "HAVING clause on a non-aggregate query");
                return WRC_ABORT;
            }
            if resolve_expr_names(&s_nc, p_having.as_ref()) != 0 {
                return WRC_ABORT;
            }
        }
        s_nc.borrow_mut().nc_flags |= NC_WHERE;
        let p_where = p.borrow().p_where.clone();
        if resolve_expr_names(&s_nc, p_where.as_ref()) != 0 {
            return WRC_ABORT;
        }
        s_nc.borrow_mut().nc_flags &= !NC_WHERE;

        // Resolve os nomes dos argumentos de funções com valor de tabela.
        for i in 0..n_src as usize {
            let (is_tab_func, func_arg) = {
                let sb = p_src.borrow();
                (sb.a[i].fg.is_tab_func, sb.a[i].u1_p_func_arg.clone())
            };
            if is_tab_func && resolve_expr_list_names(&s_nc, func_arg.as_ref()) != 0 {
                return WRC_ABORT;
            }
        }

        // SQLITE_OMIT_WINDOWFUNC não está definido no Debian.
        if in_rename_object(&p_parse) {
            let mut p_win = p.borrow().p_win_defn.clone();
            while let Some(win) = p_win {
                let (win_order_by, win_partition) = {
                    let wb = win.borrow();
                    (wb.p_order_by.clone(), wb.p_partition.clone())
                };
                if resolve_expr_list_names(&s_nc, win_order_by.as_ref()) != 0
                    || resolve_expr_list_names(&s_nc, win_partition.as_ref()) != 0
                {
                    return WRC_ABORT;
                }
                p_win = win.borrow().p_next_win.clone();
            }
        }

        // O ORDER BY e o GROUP BY não podem referenciar termos de consultas externas.
        {
            let mut nc = s_nc.borrow_mut();
            nc.p_next = None;
            nc.nc_flags |= NC_ALLOWAGG | NC_ALLOWWIN;
        }

        // Se for uma consulta composta convertida, move o ORDER BY da subconsulta de volta para
        // a consulta pai. Neste ponto cada termo do ORDER BY já foi transformado em um valor
        // inteiro. Esses inteiros são substituídos por cópias das expressões correspondentes do
        // conjunto de resultados pela chamada a `resolve_order_group_by()` abaixo.
        if (p.borrow().sel_flags & SF_CONVERTED) != 0 {
            let p_sub = p_src.borrow().a[0].p_select.clone().unwrap();
            let order_by = p_sub.borrow_mut().p_order_by.take();
            p.borrow_mut().p_order_by = order_by;
        }

        // Processa o ORDER BY dos SELECT simples. O ORDER BY dos SELECT compostos é tratado
        // abaixo, depois que os conjuntos de resultados de todos os elementos do composto
        // foram resolvidos.
        //
        // Se há ORDER BY em um termo do composto que não seja o mais à direita, é erro de
        // sintaxe. Mas o erro só é detectado bem depois, então é preciso resolver mesmo assim os
        // símbolos desse ORDER BY incorreto, por consistência.
        let p_order_by = p.borrow().p_order_by.clone();
        if let Some(order_by) = &p_order_by {
            // Adia o ORDER BY mais à direita de um composto.
            if is_compound <= n_compound && resolve_order_group_by(&s_nc, &p, order_by, "ORDER") != 0 {
                return WRC_ABORT;
            }
        }
        if db.borrow().malloc_failed != 0 {
            return WRC_ABORT;
        }
        s_nc.borrow_mut().nc_flags &= !NC_ALLOWWIN;

        // Resolve o GROUP BY. Ao mesmo tempo, garante que ele não contém funções agregadas.
        if let Some(group_by) = &p_group_by {
            if resolve_order_group_by(&s_nc, &p, group_by, "GROUP") != 0
                || db.borrow().malloc_failed != 0
            {
                return WRC_ABORT;
            }
            let n_expr = group_by.borrow().n_expr;
            for i in 0..n_expr as usize {
                let item_expr = group_by.borrow().a[i].p_expr.clone();
                if let Some(e) = &item_expr {
                    if expr_has_property(e, EP_AGG) {
                        error_msg(
                            &p_parse,
                            "aggregate functions are not allowed in the GROUP BY clause",
                        );
                        return WRC_ABORT;
                    }
                }
            }
        }

        // Se faz parte de um SELECT composto, confere se tem o número certo de expressões na
        // lista de seleção.
        let p_next = p.borrow().p_next.as_ref().and_then(|w| w.upgrade());
        if let Some(next) = p_next {
            let n_this = p.borrow().p_e_list.as_ref().unwrap().borrow().n_expr;
            let n_next = next.borrow().p_e_list.as_ref().unwrap().borrow().n_expr;
            if n_this != n_next {
                select_wrong_num_terms_error(&p_parse, &next);
                return WRC_ABORT;
            }
        }

        // Avança para o próximo termo do composto.
        cursor = p.borrow().p_prior.clone();
        n_compound += 1;
    }

    // Resolve o ORDER BY de um SELECT composto depois que todos os termos foram resolvidos.
    if is_compound != 0 && resolve_compound_order_by(&p_parse, &p_leftmost) != 0 {
        return WRC_ABORT;
    }

    WRC_PRUNE
}

/// Percorre uma árvore de expressão e resolve referências a colunas de tabela e a colunas do
/// conjunto de resultados. Ao mesmo tempo confere o uso de funções e liga um flag se aparecer
/// alguma função agregada.
///
/// Para resolver colunas de tabela procura nós (ou subárvores) da forma X.Y.Z, Y.Z ou só Z, onde
/// X é o nome de um banco ("main", "temp" ou o nome simbólico de um ATTACH), Y é o nome de uma
/// tabela da cláusula FROM (ou "old"/"new" num gatilho) e Z é o nome de uma coluna da tabela Y.
///
/// O nó raiz da subárvore é alterado assim: `op` vira TK_COLUMN, `p_tab` aponta para a Table de
/// X.Y, `i_column` é o índice da coluna (-1 para o rowid) e `i_table` é o número do cursor VDBE.
///
/// Para resolver referências ao conjunto de resultados, procura expressões da forma Z (sem X e Y)
/// iguais ao lado direito de um AS do conjunto de resultados do SELECT. Z é substituído por uma
/// cópia do lado esquerdo. A resolução de tabelas e funções ocorre na expressão substituída.
/// Por exemplo, em `SELECT a+b AS x, c+d AS y FROM t1 ORDER BY x;` o termo "x" do ORDER BY vira
/// "a+b".
///
/// Chamadas de função são conferidas (função definida, número de argumentos correto). Se for
/// agregada, liga NC_HASAGG e o opcode passa de TK_FUNCTION para TK_AGG_FUNCTION. Se a expressão
/// contém agregadas, a propriedade EP_AGG é ligada nela.
///
/// Deixa uma mensagem de erro em `p_parse` se algo estiver errado. Retorna o número de erros.
pub fn resolve_expr_names(p_nc: &NameContextRef, p_expr: Option<&ExprRef>) -> i32 {
    let p_expr = match p_expr {
        None => return SQLITE_OK,
        Some(e) => e,
    };
    const AGG_MASK: i32 = NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG;
    let (saved_has_agg, p_parse, no_select) = {
        let mut nc = p_nc.borrow_mut();
        let saved = nc.nc_flags & AGG_MASK;
        nc.nc_flags &= !AGG_MASK;
        (saved, nc.p_parse.clone(), (nc.nc_flags & NC_NOSELECT) != 0)
    };
    let mut w = Walker::new(p_parse.clone());
    w.x_expr_callback = Some(resolve_expr_step);
    w.x_select_callback = if no_select { None } else { Some(resolve_select_step) };
    w.x_select_callback2 = None;
    w.u_nc = Some(p_nc.clone());

    // SQLITE_MAX_EXPR_DEPTH>0 (o padrão é 1000).
    let height = p_expr.borrow().n_height;
    p_parse.borrow_mut().n_height += height;
    let total_height = p_parse.borrow().n_height;
    if expr_check_height(&p_parse, total_height) != 0 {
        return SQLITE_ERROR;
    }
    walk_expr_nn(&mut w, p_expr);
    p_parse.borrow_mut().n_height -= height;

    debug_assert!(EP_AGG as i32 == NC_HASAGG);
    debug_assert!(EP_WIN as i32 == NC_HASWIN);
    let flags = p_nc.borrow().nc_flags;
    expr_set_property(p_expr, (flags & (NC_HASAGG | NC_HASWIN)) as u32);
    p_nc.borrow_mut().nc_flags |= saved_has_agg;
    let n_nc_err = p_nc.borrow().n_nc_err;
    if n_nc_err > 0 || p_parse.borrow().n_err > 0 { 1 } else { 0 }
}

/// Resolve todos os nomes de todas as expressões de uma lista de expressões. Igual a
/// `resolve_expr_names()`, mas para uma lista em vez de uma expressão.
///
/// O retorno é SQLITE_OK (0) em sucesso ou SQLITE_ERROR (1) em falha.
pub fn resolve_expr_list_names(p_nc: &NameContextRef, p_list: Option<&ExprListRef>) -> i32 {
    let p_list = match p_list {
        None => return SQLITE_OK,
        Some(l) => l,
    };
    const AGG_MASK: i32 = NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG;
    let p_parse = p_nc.borrow().p_parse.clone();
    let mut w = Walker::new(p_parse.clone());
    w.x_expr_callback = Some(resolve_expr_step);
    w.x_select_callback = Some(resolve_select_step);
    w.x_select_callback2 = None;
    w.u_nc = Some(p_nc.clone());
    let mut saved_has_agg: i32 = {
        let mut nc = p_nc.borrow_mut();
        let saved = nc.nc_flags & AGG_MASK;
        nc.nc_flags &= !AGG_MASK;
        saved
    };
    let n_expr = p_list.borrow().n_expr;
    for i in 0..n_expr as usize {
        let p_expr = match p_list.borrow().a[i].p_expr.clone() {
            None => continue,
            Some(e) => e,
        };
        // SQLITE_MAX_EXPR_DEPTH>0 (o padrão é 1000).
        let height = p_expr.borrow().n_height;
        p_parse.borrow_mut().n_height += height;
        let total_height = p_parse.borrow().n_height;
        if expr_check_height(&p_parse, total_height) != 0 {
            return SQLITE_ERROR;
        }
        walk_expr_nn(&mut w, &p_expr);
        p_parse.borrow_mut().n_height -= height;

        debug_assert!(EP_AGG as i32 == NC_HASAGG);
        debug_assert!(EP_WIN as i32 == NC_HASWIN);
        let flags = p_nc.borrow().nc_flags;
        if (flags & AGG_MASK) != 0 {
            expr_set_property(&p_expr, (flags & (NC_HASAGG | NC_HASWIN)) as u32);
            saved_has_agg |= flags & AGG_MASK;
            p_nc.borrow_mut().nc_flags &= !AGG_MASK;
        }
        if p_parse.borrow().n_err > 0 {
            return SQLITE_ERROR;
        }
    }
    p_nc.borrow_mut().nc_flags |= saved_has_agg;
    SQLITE_OK
}


// ---- part_005.rs ----

/// Resolve todos os nomes de todas as expressões de um SELECT e de todos os seus descendentes,
/// incluindo compostos via `p_prior`, subconsultas em expressões e subconsultas usadas como
/// termos da cláusula FROM.
///
/// Veja `resolve_expr_names()` para a descrição das transformações que ocorrem. Todos os SELECT
/// devem ter sido expandidos por `select_expand()` antes desta rotina.
pub fn resolve_select_names(
    p_parse: &ParseRef,
    p: &SelectRef,
    p_outer_nc: Option<&NameContextRef>,
) {
    let mut w = Walker::new(p_parse.clone());
    w.x_expr_callback = Some(resolve_expr_step);
    w.x_select_callback = Some(resolve_select_step);
    w.x_select_callback2 = None;
    w.u_nc = p_outer_nc.cloned();
    walk_select(&mut w, p);
}

/// Resolve nomes em expressões que só podem referenciar uma única tabela ou nenhuma. Exemplos,
/// com o flag de "tipo" em `type_`:
///
///    (1) restrições CHECK                         NC_ISCHECK
///    (2) WHERE de índices parciais                NC_PARTIDX
///    (3) expressões em índices sobre expressões   NC_IDXEXPR
///    (4) argumentos de expressão do VACUUM INTO   0
///    (5) expressões GENERATED ALWAYS AS           NC_GENCOL
///
/// Em todos os casos exceto (4), `i_table` dos nós TK_COLUMN da expressão vira -1 e `i_column`
/// recebe o número da coluna. No caso (4), nós TK_COLUMN causam erro. Qualquer erro deixa uma
/// mensagem em `p_parse`.
pub fn resolve_self_reference(
    p_parse: &ParseRef,
    p_tab: Option<&TableRef>,
    mut type_: i32,
    p_expr: Option<&ExprRef>,
    p_list: Option<&ExprListRef>,
) -> i32 {
    debug_assert!(type_ == 0 || p_tab.is_some());
    debug_assert!(
        type_ == NC_ISCHECK
            || type_ == NC_PARTIDX
            || type_ == NC_IDXEXPR
            || type_ == NC_GENCOL
            || p_tab.is_none()
    );
    // SrcList falso para `p_parse.p_new_table`.
    let mut s_src = SrcList::default();
    let mut s_nc = NameContext::new(p_parse.clone());
    if let Some(tab) = p_tab {
        let mut item = SrcItem::default();
        item.z_name = tab.borrow().z_name.clone();
        item.p_tab = Some(tab.clone());
        item.i_cursor = -1;
        s_src.n_src = 1;
        s_src.a.push(item);

        let tab_schema = tab.borrow().p_schema.clone();
        let temp_schema = p_parse.borrow().db.borrow().a_db[1].p_schema.clone();
        let same_schema = match (&tab_schema, &temp_schema) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same_schema {
            // Faz EP_FromDDL ser ligado nos nós TK_FUNCTION de elementos de esquemas não TEMP.
            type_ |= NC_FROMDDL;
        }
    }
    s_nc.p_src_list = Some(Rc::new(RefCell::new(s_src)));
    s_nc.nc_flags = type_ | NC_ISDDL;
    let s_nc: NameContextRef = Rc::new(RefCell::new(s_nc));
    let mut rc = resolve_expr_names(&s_nc, p_expr);
    if rc != SQLITE_OK {
        return rc;
    }
    if p_list.is_some() {
        rc = resolve_expr_list_names(&s_nc, p_list);
    }
    rc
}

