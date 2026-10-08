//! resolve.c: a resolução de nomes. Percorre a árvore de sintaxe e liga cada identificador a uma
//! tabela e coluna.
//!
//! CONVENÇÕES DESTA TRADUÇÃO (o que muda em relação ao C):
//!
//! * O `NameContext` do C guarda `pParse` e uma cadeia `pNext` de ponteiros para os contextos de
//!   fora. Aqui o `Parse` e a `Connection` são parâmetros (`parse`, `db`) de todas as funções
//!   públicas, e o walker leva ambos no `ResolveCtx`. A cadeia de contextos é percorrida pelo
//!   trait `NcLevel` (o `p_next` do `NameContext` é um `&mut dyn NcLevel`), porque
//!   `&'a mut NameContext<'a>` não permite empilhar um contexto novo de vida mais curta sobre o de
//!   fora. Ver o relatório de campos que faltam em `sqlite_int.rs`.
//! * `pWinSelect` do C vira dois campos do `NameContext`: `p_win_defn` (cópia de
//!   `Select.pWinDefn`, só lida por `sqlite3WindowUpdate`) e `n_win_linked` (o contador de
//!   `Select.n_win_linked`, que `sqlite3WindowLink` incrementa).
//! * Os nomes `zDb`, `zTab` e `zCol` do C apontam para dentro da árvore que a própria rotina
//!   modifica depois; aqui são copiados antes.
//! * O endereço de um nó (chave de `RenameToken.p`) é `e as *const Expr as usize`; o endereço de
//!   `pExpr->y.pTab` é o de `Expr.y`.

use std::rc::Rc;

use crate::auth::{auth_check, auth_read};
use crate::build::{
    id_list_index, table_column_to_storage, writable_schema,
};
use crate::expr_code::is_rowid;
use crate::select::src_item_column_used;
use crate::callback::find_function;
use crate::connection::{Connection, FuncDef, Parse};
use crate::consts::{
    Bitmask, ALLBITS, BMS, COLFLAG_GENERATED, DBFLAG_INTERNAL_FUNC, ENAME_NAME, ENAME_ROWID,
    ENAME_TAB, EP_AGG, EP_CAN_BE_NULL, EP_CONST_FUNC, EP_DBL_QUOTED, EP_FROM_DDL, EP_INNER_ON,
    EP_INT_VALUE, EP_LEAF, EP_OUTER_ON, EP_SKIP, EP_TOKEN_ONLY, EP_UNLIKELY, EP_VAR_SELECT,
    EP_WIN, EP_WIN_FUNC, JT_LEFT, JT_LTORJ, JT_RIGHT, LEGACY_SCHEMA_TABLE,
    LEGACY_TEMP_SCHEMA_TABLE, NC_ALLOWAGG, NC_ALLOWWIN, NC_FROMDDL, NC_GENCOL, NC_HASAGG,
    NC_HASWIN, NC_IDXEXPR, NC_ISCHECK, NC_ISDDL, NC_MINMAXAGG, NC_NOSELECT, NC_ORDERAGG,
    NC_PARTIDX, NC_SELFREF, NC_SUBQUERY, NC_UBASEREG, NC_UELIST, NC_UUPSERT, NC_WHERE,
    PREFERRED_SCHEMA_TABLE, PREFERRED_TEMP_SCHEMA_TABLE, SF_AGGREGATE, SF_CONVERTED,
    SF_CORRELATED, SF_EXPANDED, SF_RESOLVED, SQLITE_AFF_INTEGER, SQLITE_DENY, SQLITE_DQS_DDL,
    SQLITE_DQS_DML, SQLITE_ERROR, SQLITE_FUNCTION, SQLITE_FUNC_ANYORDER, SQLITE_FUNC_CONSTANT,
    SQLITE_FUNC_DIRECT, SQLITE_FUNC_INTERNAL, SQLITE_FUNC_MINMAX, SQLITE_FUNC_SLOCHNG,
    SQLITE_FUNC_UNLIKELY, SQLITE_FUNC_UNSAFE, SQLITE_FUNC_WINDOW, SQLITE_LIMIT_COLUMN,
    SQLITE_OK, SQLITE_UTF8, SQLITE_WARNING, TF_HAS_GENERATED, TK_AGG_FUNCTION, TK_BETWEEN,
    TK_COLLATE, TK_COLUMN, TK_DELETE, TK_DOT, TK_EQ, TK_EXISTS, TK_FLOAT, TK_FUNCTION, TK_GE,
    TK_GT, TK_ID, TK_IN, TK_INSERT, TK_INTEGER, TK_IS, TK_ISNOT, TK_ISNULL, TK_LE, TK_LT, TK_NE,
    TK_NOTNULL, TK_NULL, TK_REGISTER, TK_ROW, TK_SELECT, TK_STRING, TK_TRIGGER,
    TK_TRUEFALSE, TK_TRUTH, TK_VARIABLE, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE,
};
use crate::expr::expr_token_arg;
use crate::expr::{
    expr_add_collate_string, expr_alloc, expr_can_be_null, expr_check_height, expr_compare,
    expr_dup, expr_function_usable, expr_id_to_true_false, expr_is_integer, expr_list_append,
    expr_order_by_aggregate_error, expr_vector_size, references_src_list,
};
use crate::printf::{PrintfArg, PrintfExprToken};
use crate::select::{select_prep, select_wrong_num_terms_error};
use crate::sqlite_int::{
    Expr, ExprList, ExprListItem, ExprU, ExprX, ExprY, NcU, NameContext, SchemaId, Select,
    SrcItem, SrcItemFg, SrcList, SrcU1, SrcU3, TabRef, Table, Upsert, Walker, Window, YnVar,
};
use crate::util::{at, atof, str_i_hash, str_icmp, stricmp, strlen30, strnicmp};
use crate::walker::{walk_expr, walk_expr_list, walk_expr_nn, walk_select, WALKER_FLAG_IN_RENAME};
use crate::alter::rename_token_remap;
use crate::util::{error_msg, record_error_offset_of_expr};
use crate::window::{window_link, window_unlink_from_select, window_update};

/// Número mágico de tabela que significa a tabela EXCLUDED de um UPSERT.
const EXCLUDED_TABLE_NUMBER: i32 = 2;

// ---------------------------------------------------------------------------------------------
// A cadeia de contextos e o contexto do walker
// ---------------------------------------------------------------------------------------------

/// Um nível da cadeia de `NameContext` (o `pNC` e seus `pNext`). Existe porque um contexto novo,
/// de vida curta, precisa apontar para o de fora, e `&'a mut NameContext<'a>` não deixa.
pub trait NcLevel {
    /// `pSrcList`, para ler e para marcar `colUsed`.
    fn src_list(&mut self) -> Option<&mut SrcList>;
    /// `uNC.pEList`, se o nível a tem.
    fn e_list(&self) -> Option<&ExprList>;
    /// `uNC.pUpsert`, se o nível o tem.
    fn upsert(&self) -> Option<&Upsert>;
    /// `uNC.iBaseReg`.
    fn base_reg(&self) -> i32;
    /// `ncFlags`.
    fn flags(&self) -> i32;
    /// Grava `ncFlags`.
    fn set_flags(&mut self, f: i32);
    /// `nRef`.
    fn n_ref(&self) -> i32;
    /// Grava `nRef`.
    fn set_n_ref(&mut self, n: i32);
    /// `nNestedSelect`.
    fn n_nested_select(&self) -> u32;
    /// `nNcErr`.
    fn n_nc_err(&self) -> i32;
    /// Grava `nNcErr`.
    fn set_n_nc_err(&mut self, n: i32);
    /// `pWinSelect->pWinDefn` (vazio se não há select de janelas).
    fn win_defn(&self) -> &[Window];
    /// O contador de janelas ligadas de `pWinSelect` (`None` quando `pWinSelect` é nulo).
    fn win_link(&mut self) -> Option<&mut u32>;
    /// `pNext`.
    fn next(&mut self) -> Option<&mut dyn NcLevel>;
}

impl<'a> NcLevel for NameContext<'a> {
    fn src_list(&mut self) -> Option<&mut SrcList> {
        self.p_src_list.as_deref_mut()
    }
    fn e_list(&self) -> Option<&ExprList> {
        match &self.u_nc {
            NcU::EList(l) => Some(*l),
            _ => None,
        }
    }
    fn upsert(&self) -> Option<&Upsert> {
        match &self.u_nc {
            NcU::Upsert(u) => Some(*u),
            _ => None,
        }
    }
    fn base_reg(&self) -> i32 {
        match &self.u_nc {
            NcU::BaseReg(r) => *r,
            _ => 0,
        }
    }
    fn flags(&self) -> i32 {
        self.nc_flags
    }
    fn set_flags(&mut self, f: i32) {
        self.nc_flags = f;
    }
    fn n_ref(&self) -> i32 {
        self.n_ref
    }
    fn set_n_ref(&mut self, n: i32) {
        self.n_ref = n;
    }
    fn n_nested_select(&self) -> u32 {
        self.n_nested_select
    }
    fn n_nc_err(&self) -> i32 {
        self.n_nc_err
    }
    fn set_n_nc_err(&mut self, n: i32) {
        self.n_nc_err = n;
    }
    fn win_defn(&self) -> &[Window] {
        &self.p_win_defn
    }
    fn win_link(&mut self) -> Option<&mut u32> {
        self.n_win_linked.as_deref_mut()
    }
    fn next(&mut self) -> Option<&mut dyn NcLevel> {
        match self.p_next.as_deref_mut() {
            Some(n) => Some(n),
            None => None,
        }
    }
}

/// O contexto do walker de resolução (o `pParse` e o `u.pNC` do C, mais a conexão).
pub struct ResolveCtx<'a, 'n> {
    /// O contexto de análise.
    pub parse: &'a mut Parse,
    /// A conexão.
    pub db: &'a mut Connection,
    /// `u.pNC`: o contexto de nomes corrente (o de fora, quando o walker resolve um SELECT).
    pub nc: Option<&'a mut NameContext<'n>>,
}

/// Um `NameContext` zerado (o `memset(&sNC, 0, sizeof(sNC))` do C).
pub fn name_context_new<'a>() -> NameContext<'a> {
    NameContext {
        p_src_list: None,
        u_nc: NcU::None,
        p_next: None,
        n_ref: 0,
        n_nc_err: 0,
        nc_flags: 0,
        n_nested_select: 0,
        p_win_defn: Vec::new(),
        n_win_linked: None,
    }
}

/// O nível `depth` da cadeia que começa em `top` (0 é o próprio `top`).
fn level_at<'a>(top: &'a mut dyn NcLevel, depth: usize) -> Option<&'a mut dyn NcLevel> {
    let mut cur: &'a mut dyn NcLevel = top;
    for _ in 0..depth {
        cur = cur.next()?;
    }
    Some(cur)
}

/// O resto de `z` a partir de `k` (vazio se `k` passa do fim).
fn tail(z: &[u8], k: usize) -> &[u8] {
    &z[k.min(z.len())..]
}

/// O endereço de um nó, usado como chave de `RenameToken.p`.
fn expr_addr(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// O texto de `u.zToken` copiado (vazio se o nó não tem texto).
fn token_of(e: &Expr) -> Vec<u8> {
    e.z_token().map(|z| z.to_vec()).unwrap_or_default()
}

/// Um argumento `%s` do `error_msg`.
fn txt(z: &[u8]) -> PrintfArg {
    PrintfArg::Text(Some(z.to_vec()))
}

/// O `sqlite3RecordErrorOffsetOfExpr` sem a gravação em `db`: o primeiro `iOfst > 0` descendo por
/// `pLeft`, para o argumento `%#T`.
fn expr_error_offset(e: &Expr) -> Option<i32> {
    let mut cur = Some(e);
    while let Some(x) = cur {
        if x.has_property(EP_OUTER_ON | EP_INNER_ON) || x.i_ofst() <= 0 {
            cur = x.p_left.as_deref();
        } else {
            return Some(x.i_ofst());
        }
    }
    None
}


/// `sqlite3ExprSkipCollateAndLikely` sobre uma posse: devolve a posição (o `Option<Box<Expr>>`)
/// que guarda o nó para onde o C chegaria. Serve para ler, alterar e trocar o nó.
fn skip_collate_and_likely_slot(slot: &mut Option<Box<Expr>>) -> &mut Option<Box<Expr>> {
    // Mede o caminho com empréstimo compartilhado (verdadeiro = argumento 0 da lista do
    // `unlikely()`, falso = `p_left` do COLLATE) e depois desce por ele com o mutável.
    let mut path: Vec<bool> = Vec::new();
    let mut cur = slot.as_deref();
    while let Some(x) = cur {
        if x.has_property(EP_UNLIKELY) {
            match x.x_list() {
                Some(l) if !l.a.is_empty() => {
                    path.push(true);
                    cur = l.a[0].p_expr.as_deref();
                }
                _ => break,
            }
        } else if x.has_property(EP_SKIP) {
            path.push(false);
            cur = x.p_left.as_deref();
        } else {
            break;
        }
    }
    let mut slot = slot;
    for via_list in path {
        let x = slot.as_deref_mut().expect("caminho medido acima");
        slot = if via_list {
            &mut x.x_list_mut().expect("lista medida acima").a[0].p_expr
        } else {
            &mut x.p_left
        };
    }
    slot
}

// ---------------------------------------------------------------------------------------------
// Profundidade de funções agregadas
// ---------------------------------------------------------------------------------------------

/// Callback do percurso: soma `u` ao `op2` de cada `TK_AGG_FUNCTION`.
fn incr_agg_depth(w: &mut Walker<i32>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_AGG_FUNCTION {
        p_expr.op2 = p_expr.op2.wrapping_add(w.u as u8);
    }
    WRC_CONTINUE
}

/// Percorre a árvore `p_expr` e aumenta em `n` a profundidade de função agregada (`Expr.op2`) de
/// cada nó `TK_AGG_FUNCTION`. Precisa acontecer quando se copia um `TK_AGG_FUNCTION` de uma
/// consulta de fora para uma subconsulta.
///
/// Ver também `sqlite3WindowExtraAggFuncDepth()` em window.c.
fn incr_agg_function_depth(p_expr: &mut Expr, n: i32) {
    if n > 0 {
        let mut w: Walker<i32> = Walker {
            x_expr_callback: Some(incr_agg_depth),
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: n,
        };
        walk_expr(&mut w, Some(p_expr));
    }
}

/// Transforma `p_expr` num alias da `i_col`-ésima coluna do resultado `p_e_list`.
///
/// Se a referência é seguida de um COLLATE, o COLLATE é preservado. Por exemplo
/// `SELECT a+b, c+d FROM t1 ORDER BY 1 COLLATE nocase;` vira
/// `SELECT a+b, c+d FROM t1 ORDER BY (a+b) COLLATE nocase;`.
///
/// `n_subquery` diz quantos níveis de subconsulta separam o alias da expressão original (o usual é
/// zero); o `op2` dos `TK_AGG_FUNCTION` sobe por essa quantidade.
fn resolve_alias(
    db: &mut Connection,
    parse: &mut Parse,
    p_e_list: &ExprList,
    i_col: usize,
    p_expr: &mut Expr,
    n_subquery: i32,
) {
    let p_orig = match p_e_list.a.get(i_col).and_then(|it| it.p_expr.as_deref()) {
        Some(o) => o,
        None => return,
    };
    if p_expr.p_agg_info.is_some() {
        return;
    }
    let p_dup = expr_dup(Some(p_orig), 0);
    if db.malloc_failed != 0 {
        return;
    }
    let mut p_dup = match p_dup {
        Some(d) => d,
        None => return,
    };
    incr_agg_function_depth(&mut p_dup, n_subquery);
    if p_expr.op == TK_COLLATE {
        let z = token_of(p_expr);
        p_dup = match expr_add_collate_string(Some(p_dup), &z) {
            Some(d) => d,
            None => return,
        };
    }
    // Troca o conteúdo dos dois nós. O nó que sai com o conteúdo antigo de `p_expr` é o
    // `sqlite3ExprDeferredDelete`: no modelo v2 não há ponteiros para ele, então basta soltar.
    std::mem::swap(&mut *p_dup, p_expr);
    drop(p_dup);
}

/// `sqlite3MatchEName`: as subconsultas guardam os nomes originais de banco, tabela e coluna dos
/// resultados em `ExprList.a[].zEName`, na forma "BANCO.TABELA.COLUNA", e marcam o item com
/// `fg.eEName = ENAME_TAB`.
///
/// Confere se o `zEName`/`eEName` do item casa com `z_db`, `z_tab` e `z_col`. Os que forem `None`
/// casam com qualquer coisa. Devolve 1 se há casamento e 0 se não há.
///
/// Subconsultas SF_NestedFrom também guardam uma entrada para o rowid implícito, com
/// `fg.eEName = ENAME_ROWID` e `zEName = "BANCO.TABELA.<apelido do rowid>"`. Esse tipo de item
/// casa se `z_col` é um apelido de rowid; nesse caso `*pb_rowid` vira 1.
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
    let mut z_span: &[u8] = p_item.z_e_name.as_deref().unwrap_or(&[]);
    let mut n = 0usize;
    while at(z_span, n) != 0 && at(z_span, n) != b'.' {
        n += 1;
    }
    if let Some(db) = z_db {
        if strnicmp(Some(z_span), Some(db), n as i32) != 0 || at(db, n) != 0 {
            return 0;
        }
    }
    z_span = tail(z_span, n + 1);
    n = 0;
    while at(z_span, n) != 0 && at(z_span, n) != b'.' {
        n += 1;
    }
    if let Some(tab) = z_tab {
        if strnicmp(Some(z_span), Some(tab), n as i32) != 0 || at(tab, n) != 0 {
            return 0;
        }
    }
    z_span = tail(z_span, n + 1);
    if let Some(col) = z_col {
        if e_e_name == ENAME_TAB && str_icmp(z_span, col) != 0 {
            return 0;
        }
        if e_e_name == ENAME_ROWID && !is_rowid(col) {
            return 0;
        }
    }
    if e_e_name == ENAME_ROWID {
        if let Some(r) = pb_rowid {
            *r = 1;
        }
    }
    1
}

/// Verdadeiro se a característica ruim das strings entre aspas duplas deve ser aceita.
/// `top_nc_flags` é o `ncFlags` do contexto de topo.
fn are_double_quoted_strings_enabled(db: &Connection, top_nc_flags: i32) -> bool {
    if db.init.busy != 0 {
        return true; // Sempre vale para esquemas antigos.
    }
    if (top_nc_flags & NC_ISDDL) != 0 {
        // Está analisando um comando DDL.
        if writable_schema(db) && (db.flags & SQLITE_DQS_DML) != 0 {
            return true;
        }
        (db.flags & SQLITE_DQS_DDL) != 0
    } else {
        // Está analisando um comando DML.
        (db.flags & SQLITE_DQS_DML) != 0
    }
}

/// A máscara `colUsed` de uma coluna de `p_tab`: a coluna `n` (que o C lê de `pExpr->iColumn`).
fn col_used_mask(p_tab: &Table, n: i32) -> Bitmask {
    let mut n = n;
    if (p_tab.tab_flags & TF_HAS_GENERATED) != 0
        && p_tab
            .a_col
            .get(n.max(0) as usize)
            .map_or(false, |c| (c.col_flags & COLFLAG_GENERATED) != 0)
    {
        let n_col = p_tab.n_col as i32;
        if n_col >= BMS {
            ALLBITS
        } else {
            (1u64 << n_col) - 1
        }
    } else {
        if n >= BMS {
            n = BMS - 1;
        }
        1u64 << n
    }
}

/// `sqlite3ExprColUsed`: o argumento é um nó `TK_COLUMN`; devolve a máscara `colUsed`.
/// `own` é a tabela dona da árvore, para o caso `TabRef::Own`.
pub fn expr_col_used(p_expr: &Expr, own: Option<&Rc<Table>>) -> Bitmask {
    let p_ex_tab: Option<&Rc<Table>> = match p_expr.y_tab() {
        Some(TabRef::Rc(t)) => Some(t),
        Some(TabRef::Own) => own,
        None => None,
    };
    match p_ex_tab {
        Some(t) => col_used_mask(t, p_expr.i_column),
        None => 0,
    }
}

/// O que `lookupName` guarda de um `pMatch` (o `SrcItem*` do C): onde ele está e o que se lê dele.
/// `depth` é o nível na cadeia de contextos e `idx` o índice em `pSrcList->a`.
#[derive(Clone)]
struct MatchSnap {
    depth: usize,
    idx: usize,
    i_cursor: i32,
    p_tab: Rc<Table>,
    jointype: u8,
    is_nested_from: bool,
}

/// Cria um termo de expressão para a coluna `i_column` de `p_match` e o acrescenta ao conjunto de
/// casamentos de FULL JOIN em `*pp_list`. Cria a lista se este é o primeiro termo.
fn extend_fj_match(
    db: &mut Connection,
    parse: &mut Parse,
    pp_list: &mut Option<Box<ExprList>>,
    p_match: &MatchSnap,
    i_column: i16,
) {
    if let Some(mut p_new) = expr_alloc(TK_COLUMN as i32, None, 0) {
        p_new.i_table = p_match.i_cursor;
        p_new.i_column = i_column as YnVar;
        p_new.y = ExprY::Tab(Some(TabRef::Rc(p_match.p_tab.clone())));
        p_new.set_property(EP_CAN_BE_NULL);
        *pp_list = expr_list_append(pp_list.take(), Some(p_new));
    }
}

/// Verdadeiro se `z_tab` é um nome válido para a tabela de esquema `p_tab`.
fn is_valid_schema_table_name(z_tab: &[u8], p_tab: &Table, z_db: Option<&[u8]>) -> bool {
    if strnicmp(Some(z_tab), Some(&b"sqlite_"[..]), 7) != 0 {
        return false;
    }
    let z_legacy: &[u8] = &p_tab.z_name;
    if tail(z_legacy, 7) == tail(LEGACY_TEMP_SCHEMA_TABLE, 7) {
        if str_icmp(tail(z_tab, 7), tail(PREFERRED_TEMP_SCHEMA_TABLE, 7)) == 0 {
            return true;
        }
        if z_db.is_none() {
            return false;
        }
        if str_icmp(tail(z_tab, 7), tail(LEGACY_SCHEMA_TABLE, 7)) == 0 {
            return true;
        }
        if str_icmp(tail(z_tab, 7), tail(PREFERRED_SCHEMA_TABLE, 7)) == 0 {
            return true;
        }
    } else if str_icmp(tail(z_tab, 7), tail(PREFERRED_SCHEMA_TABLE, 7)) == 0 {
        return true;
    }
    false
}

/// Verdadeiro se o item de FROM com `fg`/`u3` junta por USING a coluna `z_col`.
fn item_uses_col(fg: &SrcItemFg, u3: &SrcU3, z_col: &[u8]) -> bool {
    if !fg.is_using {
        return false;
    }
    match u3 {
        SrcU3::Using(Some(l)) => id_list_index(l, z_col) >= 0,
        _ => false,
    }
}

/// O tratamento do caso `cnt > 0` (a coluna já casou antes) do laço de colunas de `lookupName`.
/// Devolve verdadeiro se o chamador deve fazer `continue` do laço interno.
fn fj_step(
    db: &mut Connection,
    parse: &mut Parse,
    cnt: &mut i32,
    p_fj_match: &mut Option<Box<ExprList>>,
    uses_col: bool,
    jointype: u8,
    p_match: Option<&MatchSnap>,
    i_column: YnVar,
) -> bool {
    if *cnt > 0 {
        if !uses_col {
            // Duas ou mais tabelas têm uma coluna com o mesmo nome sem junção por USING: é erro.
            // Sinaliza apagando `p_fj_match` e deixando `cnt` passar de 1.
            *p_fj_match = None;
        } else if (jointype & JT_RIGHT) == 0 {
            // Um INNER ou LEFT JOIN: usa a tabela mais à esquerda.
            return true;
        } else if (jointype & JT_LEFT) == 0 {
            // Um RIGHT JOIN: usa a tabela mais à direita.
            *cnt = 0;
            *p_fj_match = None;
        } else if let Some(m) = p_match {
            // Num FULL JOIN é preciso montar uma chamada de coalesce().
            extend_fj_match(db, parse, p_fj_match, m, i_column as i16);
        }
    }
    false
}

/// Dado o nome de uma coluna da forma X.Y.Z, Y.Z ou só Z, procura o nome no conjunto de tabelas
/// de origem e faz `p_expr` apontar para essa coluna. Em `p_expr` mudam:
///
/// * `i_table`: o número do cursor da tabela obtida de `pSrcList`;
/// * `y.pTab`: a tabela de X.Y (mesmo se X e/ou Y forem implícitos);
/// * `i_column`: a coluna dentro da tabela;
/// * `op`: passa a ser `TK_COLUMN`;
/// * `p_left` e `p_right`: o que apontavam é apagado.
///
/// `z_db` é o nome do banco (o "X"), `None` se a forma é Y.Z ou Z; `z_tab` é o nome da tabela (o
/// "Y"), `None` se a forma é Z; `z_col` é o "Z". `right_dbl_quoted` é `EP_DblQuoted` do nó que
/// guarda o Z (`pRight` do C).
///
/// Se o nome não se resolve sem ambiguidade, deixa a mensagem em `parse` e devolve `WRC_Abort`.
/// Em caso de sucesso devolve `WRC_Prune`.
fn lookup_name(
    db: &mut Connection,
    parse: &mut Parse,
    z_db_in: Option<Vec<u8>>,
    z_tab: Option<Vec<u8>>,
    z_col: Vec<u8>,
    right_dbl_quoted: bool,
    nc_top: &mut dyn NcLevel,
    p_expr: &mut Expr,
) -> i32 {
    let mut cnt: i32 = 0; // Número de nomes de coluna que casam.
    let mut cnt_tab: i32 = 0; // Número de casamentos potenciais de "rowid".
    let mut n_subquery: i32 = 0; // Quantos níveis de subconsulta.
    let mut p_match: Option<MatchSnap> = None; // O item de FROM que casou.
    let mut p_schema = SchemaId(0); // O esquema da expressão.
    let mut e_new_expr_op: u8 = TK_COLUMN; // Novo `op` de p_expr em caso de sucesso.
    let mut p_fj_match: Option<Box<ExprList>> = None; // Casamentos de FULL JOIN .. USING.
    let mut z_db = z_db_in;
    let mut depth: usize = 0; // O nível de `pNC` na cadeia.

    // Inicia o nó como "sem casamento".
    p_expr.i_table = -1;

    // Traduz o nome do esquema em `z_db` para o esquema correspondente. Se não achar, `p_schema`
    // fica nulo, nada casa e a mensagem de erro sai no fim da rotina.
    if let Some(zdb) = z_db.clone() {
        if (nc_top.flags() & (NC_PARTIDX | NC_ISCHECK)) != 0 {
            // Ignora em silêncio qualificadores de banco dentro de CHECK e de índices parciais.
            // Não gera erro: poderia quebrar o legado e ignorar o nome não faz mal.
            z_db = None;
        } else {
            let mut found = false;
            for slot in db.dbs.iter() {
                if str_icmp(&slot.z_db_s_name, &zdb) == 0 {
                    p_schema = slot.schema.id;
                    found = true;
                    break;
                }
            }
            if !found && str_icmp(b"main", &zdb) == 0 {
                // Este ramo vale quando o banco principal foi renomeado por
                // SQLITE_DBCONFIG_MAINDBNAME.
                p_schema = db.dbs[0].schema.id;
                z_db = Some(db.dbs[0].z_db_s_name.clone());
            }
        }
    }

    'body: {
        // Começa no contexto mais interno e anda para fora até achar um casamento.
        loop {
            let flags_here: i32;
            let mut goto_end = false;
            {
                let level = match level_at(&mut *nc_top, depth) {
                    Some(l) => l,
                    None => break,
                };
                flags_here = level.flags();
                let up_reg_data = level.upsert().map(|u| u.reg_data);
                let up_tab: Option<Rc<Table>> = level
                    .upsert()
                    .and_then(|u| u.p_upsert_src.as_deref())
                    .and_then(|s| s.a.first())
                    .and_then(|it| it.p_tab.clone());
                let base_reg = level.base_reg();

                if let Some(src) = level.src_list() {
                    let n_src = src.a.len();
                    for i in 0..n_src {
                        let p_item = &mut src.a[i];
                        let p_tab = match p_item.p_tab.clone() {
                            Some(t) => t,
                            None => continue,
                        };
                        if p_item.fg.is_nested_from {
                            // Aqui o item é uma subconsulta formada de um subconjunto entre
                            // parênteses dos termos do FROM. Exemplo:
                            //   .... FROM t1 LEFT JOIN (t2 RIGHT JOIN t3 USING(x)) USING(y) ...
                            //                          \_________________________/
                            //             Este p_item -------------^
                            let mut hit = false;
                            if let Some(e_list) = p_item
                                .p_select
                                .as_deref_mut()
                                .and_then(|s| s.p_e_list.as_deref_mut())
                            {
                                for j in 0..e_list.a.len() {
                                    let mut b_rowid: i32 = 0; // Verdadeiro se pode ser rowid.
                                    if match_e_name(
                                        &e_list.a[j],
                                        Some(z_col.as_slice()),
                                        z_tab.as_deref(),
                                        z_db.as_deref(),
                                        Some(&mut b_rowid),
                                    ) == 0
                                    {
                                        continue;
                                    }
                                    if b_rowid == 0 {
                                        if cnt > 0 {
                                            let uses =
                                                item_uses_col(&p_item.fg, &p_item.u3, &z_col);
                                            if fj_step(
                                                db,
                                                parse,
                                                &mut cnt,
                                                &mut p_fj_match,
                                                uses,
                                                p_item.fg.jointype,
                                                p_match.as_ref(),
                                                p_expr.i_column,
                                            ) {
                                                continue;
                                            }
                                        }
                                        cnt += 1;
                                        hit = true;
                                    } else if cnt > 0 {
                                        // Possível casamento de rowid, mas já houve um casamento
                                        // de verdade: ignora.
                                        continue;
                                    }
                                    cnt_tab += 1;
                                    p_match = Some(MatchSnap {
                                        depth,
                                        idx: i,
                                        i_cursor: p_item.i_cursor,
                                        p_tab: p_tab.clone(),
                                        jointype: p_item.fg.jointype,
                                        is_nested_from: true,
                                    });
                                    p_expr.i_column = j as YnVar;
                                    e_list.a[j].fg.b_used = true;

                                    // O rowid não pode fazer parte de um USING.
                                    if e_list.a[j].fg.b_using_term {
                                        break;
                                    }
                                }
                            }
                            if hit || z_tab.is_none() {
                                continue;
                            }
                        }
                        if let Some(ztab) = z_tab.as_deref() {
                            if z_db.is_some() {
                                if p_tab.p_schema != p_schema {
                                    continue;
                                }
                                if p_schema == SchemaId(0) && z_db.as_deref() != Some(&b"*"[..]) {
                                    continue;
                                }
                            }
                            if let Some(alias) = p_item.z_alias.as_deref() {
                                if str_icmp(ztab, alias) != 0 {
                                    continue;
                                }
                            } else if str_icmp(ztab, &p_tab.z_name) != 0 {
                                if p_tab.tnum != 1 {
                                    continue;
                                }
                                if !is_valid_schema_table_name(ztab, &p_tab, z_db.as_deref()) {
                                    continue;
                                }
                            }
                            if parse.in_rename_object() && p_item.z_alias.is_some() {
                                rename_token_remap(parse, 0, &p_expr.y as *const ExprY as usize);
                            }
                        }
                        let h_col = str_i_hash(&z_col);
                        let n_col = (p_tab.n_col.max(0) as usize).min(p_tab.a_col.len());
                        for j in 0..n_col {
                            let p_col = &p_tab.a_col[j];
                            if p_col.h_name == h_col && str_icmp(&p_col.z_cn_name, &z_col) == 0 {
                                if cnt > 0 {
                                    let uses = item_uses_col(&p_item.fg, &p_item.u3, &z_col);
                                    if fj_step(
                                        db,
                                        parse,
                                        &mut cnt,
                                        &mut p_fj_match,
                                        uses,
                                        p_item.fg.jointype,
                                        p_match.as_ref(),
                                        p_expr.i_column,
                                    ) {
                                        continue;
                                    }
                                }
                                cnt += 1;
                                p_match = Some(MatchSnap {
                                    depth,
                                    idx: i,
                                    i_cursor: p_item.i_cursor,
                                    p_tab: p_tab.clone(),
                                    jointype: p_item.fg.jointype,
                                    is_nested_from: p_item.fg.is_nested_from,
                                });
                                // Troca o rowid (coluna -1) pela INTEGER PRIMARY KEY.
                                p_expr.i_column = if j as i32 == p_tab.i_p_key as i32 {
                                    -1
                                } else {
                                    j as i16 as YnVar
                                };
                                if p_item.fg.is_nested_from {
                                    src_item_column_used(p_item, j as i32);
                                }
                                break;
                            }
                        }
                        if cnt == 0 && p_tab.visible_rowid() {
                            // `p_tab` é um possível casamento de ROWID. Guarda e casa o ROWID
                            // depois, se for o caso. Só se permite casamento de ROWID quando há
                            // um único candidato, que é sempre uma tabela e não uma VIEW.
                            cnt_tab += 1;
                            p_match = Some(MatchSnap {
                                depth,
                                idx: i,
                                i_cursor: p_item.i_cursor,
                                p_tab: p_tab.clone(),
                                jointype: p_item.fg.jointype,
                                is_nested_from: p_item.fg.is_nested_from,
                            });
                        }
                    }
                    if let Some(m) = &p_match {
                        p_expr.i_table = m.i_cursor;
                        p_expr.y = ExprY::Tab(Some(TabRef::Rc(m.p_tab.clone())));
                        if (m.jointype & (JT_LEFT | JT_LTORJ)) != 0 {
                            p_expr.set_property(EP_CAN_BE_NULL);
                        }
                        p_schema = m.p_tab.p_schema;
                    }
                } // if( pSrcList )

                // Se o nome ainda não se resolveu, talvez seja uma referência new.* ou old.* de
                // gatilho, ou um excluded.* de upsert, ou uma referência na cláusula RETURNING a
                // uma tabela sendo modificada.
                if cnt == 0 && z_db.is_none() {
                    let mut p_tab: Option<Rc<Table>> = None;
                    if let Some(trig_tab) = parse.p_trigger_tab.clone() {
                        let op = parse.e_trigger_op;
                        if parse.b_returning != 0 {
                            if (flags_here & NC_UBASEREG) != 0
                                && (z_tab.is_none()
                                    || str_icmp(z_tab.as_deref().unwrap_or(&[]), &trig_tab.z_name)
                                        == 0
                                    || is_valid_schema_table_name(
                                        z_tab.as_deref().unwrap_or(&[]),
                                        &trig_tab,
                                        None,
                                    ))
                            {
                                p_expr.i_table = (op != TK_DELETE) as i32;
                                p_tab = Some(trig_tab);
                            }
                        } else if op != TK_DELETE
                            && z_tab.as_deref().map_or(false, |t| str_icmp(b"new", t) == 0)
                        {
                            p_expr.i_table = 1;
                            p_tab = Some(trig_tab);
                        } else if op != TK_INSERT
                            && z_tab.as_deref().map_or(false, |t| str_icmp(b"old", t) == 0)
                        {
                            p_expr.i_table = 0;
                            p_tab = Some(trig_tab);
                        }
                    }
                    if (flags_here & NC_UUPSERT) != 0 && z_tab.is_some() {
                        if up_reg_data.is_some()
                            && z_tab.as_deref().map_or(false, |t| str_icmp(b"excluded", t) == 0)
                        {
                            p_tab = up_tab.clone();
                            p_expr.i_table = EXCLUDED_TABLE_NUMBER;
                        }
                    }
                    if let Some(p_tab) = p_tab {
                        let h_col = str_i_hash(&z_col);
                        p_schema = p_tab.p_schema;
                        cnt_tab += 1;
                        let n_col = p_tab.n_col as i32;
                        let mut i_col: i32 = 0;
                        while i_col < n_col {
                            let p_col = &p_tab.a_col[i_col as usize];
                            if p_col.h_name == h_col && str_icmp(&p_col.z_cn_name, &z_col) == 0 {
                                if i_col == p_tab.i_p_key as i32 {
                                    i_col = -1;
                                }
                                break;
                            }
                            i_col += 1;
                        }
                        if i_col >= n_col && is_rowid(&z_col) && p_tab.visible_rowid() {
                            // IMP: R-51414-32910
                            i_col = -1;
                        }
                        if i_col < n_col {
                            cnt += 1;
                            p_match = None;
                            if p_expr.i_table == EXCLUDED_TABLE_NUMBER {
                                if parse.in_rename_object() {
                                    p_expr.i_column = i_col;
                                    p_expr.y = ExprY::Tab(Some(TabRef::Rc(p_tab.clone())));
                                    e_new_expr_op = TK_COLUMN;
                                } else {
                                    p_expr.i_table = up_reg_data.unwrap_or(0)
                                        + table_column_to_storage(&p_tab, i_col as i16) as i32;
                                    e_new_expr_op = TK_REGISTER;
                                }
                            } else {
                                p_expr.y = ExprY::Tab(Some(TabRef::Rc(p_tab.clone())));
                                if parse.b_returning != 0 {
                                    e_new_expr_op = TK_REGISTER;
                                    p_expr.op2 = TK_COLUMN;
                                    p_expr.i_column = i_col;
                                    p_expr.i_table = base_reg
                                        + (p_tab.n_col as i32 + 1) * p_expr.i_table
                                        + table_column_to_storage(&p_tab, i_col as i16) as i32
                                        + 1;
                                } else {
                                    p_expr.i_column = i_col as i16 as YnVar;
                                    e_new_expr_op = TK_TRIGGER;
                                    if i_col < 0 {
                                        p_expr.aff_expr = SQLITE_AFF_INTEGER;
                                    } else if p_expr.i_table == 0 {
                                        parse.oldmask |= if i_col >= 32 {
                                            0xffff_ffff
                                        } else {
                                            1u32 << i_col
                                        };
                                    } else {
                                        parse.newmask |= if i_col >= 32 {
                                            0xffff_ffff
                                        } else {
                                            1u32 << i_col
                                        };
                                    }
                                }
                            }
                        }
                    }
                }

                // Talvez o nome seja uma referência ao ROWID.
                if cnt == 0 && cnt_tab >= 1 && (flags_here & (NC_IDXEXPR | NC_GENCOL)) == 0 {
                    if let Some(m) = &p_match {
                        if is_rowid(&z_col) && (m.p_tab.visible_rowid() || m.is_nested_from) {
                            cnt = cnt_tab;
                            if !m.is_nested_from {
                                p_expr.i_column = -1;
                            }
                            p_expr.aff_expr = SQLITE_AFF_INTEGER;
                        }
                    }
                }

                // Se a entrada tem a forma Z (e não Y.Z ou X.Y.Z), o nome Z pode ser um alias de
                // coluna do resultado. Acontece, por exemplo, ao resolver a cláusula WHERE de
                //
                //     SELECT a+b AS x FROM table WHERE x<10;
                //
                // Nesses casos substitui `p_expr` por uma cópia da expressão que forma a entrada
                // do resultado ("a+b" no exemplo) e sai na hora. A expressão do resultado já deve
                // ter sido resolvida quando a cláusula WHERE é resolvida.
                //
                // Usar uma coluna do resultado no WHERE, GROUP BY ou HAVING, ou como parte de uma
                // expressão maior do ORDER BY, não é SQL padrão: é uma extensão (esquisita) do
                // SQLite mantida só por compatibilidade com o legado.
                if cnt == 0 && (flags_here & NC_UELIST) != 0 && z_tab.is_none() {
                    if let Some(e_list) = level.e_list() {
                        for j in 0..e_list.a.len() {
                            let item = &e_list.a[j];
                            if item.fg.e_e_name == ENAME_NAME
                                && stricmp(item.z_e_name.as_deref(), Some(z_col.as_slice())) == 0
                            {
                                let z_as = item.z_e_name.clone().unwrap_or_default();
                                let p_orig = match item.p_expr.as_deref() {
                                    Some(o) => o,
                                    None => continue,
                                };
                                if (flags_here & NC_ALLOWAGG) == 0 && p_orig.has_property(EP_AGG) {
                                    error_msg(
                                        db,
                                        parse,
                                        b"misuse of aliased aggregate %s",
                                        &[txt(&z_as)],
                                    );
                                    return WRC_ABORT;
                                }
                                if p_orig.has_property(EP_WIN)
                                    && ((flags_here & NC_ALLOWWIN) == 0 || depth != 0)
                                {
                                    error_msg(
                                        db,
                                        parse,
                                        b"misuse of aliased window function %s",
                                        &[txt(&z_as)],
                                    );
                                    return WRC_ABORT;
                                }
                                if expr_vector_size(p_orig) != 1 {
                                    error_msg(db, parse, b"row value misused", &[]);
                                    return WRC_ABORT;
                                }
                                resolve_alias(db, parse, e_list, j, p_expr, n_subquery);
                                cnt = 1;
                                p_match = None;
                                if parse.in_rename_object() {
                                    rename_token_remap(parse, 0, expr_addr(p_expr));
                                }
                                goto_end = true;
                                break;
                            }
                        }
                    }
                }
            }
            if goto_end {
                break 'body;
            }

            // Avança para o próximo contexto de nomes. O laço termina quando há um casamento
            // (`cnt > 0`) ou quando acabam os contextos.
            if cnt != 0 {
                break;
            }
            depth += 1;
            n_subquery += 1;
            if level_at(&mut *nc_top, depth).is_none() {
                break;
            }
        }

        // Se X e Y são nulos (só o nome da coluna Z foi dado) e o valor de Z está entre aspas
        // duplas, Z é uma string literal se não casa com nenhuma coluna. Nesse caso sai na hora
        // sem alterar `p_expr`.
        //
        // Como nenhuma referência foi feita a contextos de fora, `nRef` não muda em nenhum.
        if cnt == 0 && z_tab.is_none() {
            if p_expr.has_property(EP_DBL_QUOTED)
                && are_double_quoted_strings_enabled(db, nc_top.flags())
            {
                // Se um identificador entre aspas duplas não casa com nenhuma coluna, trata-o
                // como string. Esse truque entrou nos primeiros dias do SQLite numa tentativa
                // infeliz de compatibilidade com o MySQL 3.x, que usava aspas duplas para
                // strings. Hoje só se emite um aviso.
                crate::global::log(
                    SQLITE_WARNING,
                    b"double-quoted string literal: \"%w\"",
                    &[txt(&z_col)],
                );
                p_expr.op = TK_STRING;
                p_expr.y = ExprY::Tab(None);
                return WRC_PRUNE;
            }
            if expr_id_to_true_false(p_expr) != 0 {
                return WRC_PRUNE;
            }
        }

        // `cnt == 0` é não ter casamento; `cnt > 1` é ter dois ou mais casamentos.
        //
        // `cnt == 0` é sempre erro. `cnt > 1` muitas vezes é erro, mas pode ser casamento
        // múltiplo de um NATURAL LEFT JOIN ou de um LEFT JOIN USING.
        if cnt != 1 {
            if let Some(mut fj) = p_fj_match.take() {
                if fj.a.len() as i32 == cnt - 1 {
                    if p_expr.has_property(EP_LEAF) {
                        p_expr.clear_property(EP_LEAF);
                    } else {
                        p_expr.p_left = None;
                        p_expr.p_right = None;
                    }
                    let mut slot = Some(fj);
                    if let Some(m) = &p_match {
                        extend_fj_match(db, parse, &mut slot, m, p_expr.i_column as i16);
                    }
                    fj = match slot {
                        Some(l) => l,
                        None => Box::new(ExprList::default()),
                    };
                    p_expr.op = TK_FUNCTION;
                    p_expr.u = ExprU::Token(Some(b"coalesce".to_vec()));
                    p_expr.x = ExprX::List(fj);
                    cnt = 1;
                    break 'body;
                } else {
                    drop(fj);
                }
            }
            let z_err: &[u8] = if cnt == 0 { b"no such column" } else { b"ambiguous column name" };
            if let Some(zdb) = z_db.as_deref() {
                error_msg(
                    db,
                    parse,
                    b"%s: %s.%s.%s",
                    &[txt(z_err), txt(zdb), txt(z_tab.as_deref().unwrap_or(&[])), txt(&z_col)],
                );
            } else if let Some(ztab) = z_tab.as_deref() {
                error_msg(db, parse, b"%s: %s.%s", &[txt(z_err), txt(ztab), txt(&z_col)]);
            } else if cnt == 0 && right_dbl_quoted {
                error_msg(
                    db,
                    parse,
                    b"%s: \"%s\" - should this be a string literal in single-quotes?",
                    &[txt(z_err), txt(&z_col)],
                );
            } else {
                error_msg(db, parse, b"%s: %s", &[txt(z_err), txt(&z_col)]);
            }
            record_error_offset_of_expr(db, Some(&*p_expr));
            parse.check_schema = 1;
            nc_top.set_n_nc_err(nc_top.n_nc_err() + 1);
            e_new_expr_op = TK_NULL;
        }

        // Remove toda a subestrutura de `p_expr`.
        if !p_expr.has_property(EP_TOKEN_ONLY | EP_LEAF) {
            p_expr.p_left = None;
            p_expr.p_right = None;
            p_expr.set_property(EP_LEAF);
        }

        // Se uma coluna de uma tabela de `pSrcList` é referenciada, registra o fato em
        // `pSrcList.a[].colUsed`. A coluna 0 liga o bit 0, a coluna 1 liga o bit 1 e assim por
        // diante. O bit 63 vale para a coluna 63 e as seguintes.
        //
        // A máscara é uma otimização que ajuda a decidir se um índice é de cobertura. A resposta
        // certa sai mesmo com bits a mais, mas é importante não ligar bits além do número máximo
        // de colunas da tabela (ver o ticket [b92e5e8ec2cdbaa1]).
        //
        // Se uma coluna gerada é referenciada, liga os bits de todas as colunas da tabela.
        if let Some(m) = &p_match {
            let col_used = if p_expr.i_column >= 0 {
                Some(expr_col_used(p_expr, None))
            } else {
                None
            };
            if let Some(level) = level_at(&mut *nc_top, m.depth) {
                if let Some(it) = level.src_list().and_then(|s| s.a.get_mut(m.idx)) {
                    match col_used {
                        Some(mask) => it.col_used |= mask,
                        None => it.fg.rowid_used = true,
                    }
                }
            }
        }

        p_expr.op = e_new_expr_op;
    }

    // lookupname_end:
    if cnt == 1 {
        if db.x_auth.is_some() && (p_expr.op == TK_COLUMN || p_expr.op == TK_TRIGGER) {
            if let Some(level) = level_at(&mut *nc_top, depth) {
                let src: Option<&SrcList> = level.src_list().map(|s| &*s);
                auth_read(db, parse, &mut *p_expr, p_schema, src);
            }
        }
        // Incrementa `nRef` em todos os contextos, do de topo até o ponto onde o nome casou.
        for d in 0..=depth {
            if let Some(l) = level_at(&mut *nc_top, d) {
                let n = l.n_ref();
                l.set_n_ref(n + 1);
            }
        }
        WRC_PRUNE
    } else {
        WRC_ABORT
    }
}

/// `sqlite3CreateColumnExpr`: aloca e devolve uma expressão que carrega a coluna `i_col` da fonte
/// de dados `i_src` da `SrcList` `p_src`.
pub fn create_column_expr(
    db: &mut Connection,
    p_src: &mut SrcList,
    i_src: usize,
    i_col: i32,
) -> Option<Box<Expr>> {
    let mut p = expr_alloc(TK_COLUMN as i32, None, 0)?;
    let p_item = &mut p_src.a[i_src];
    let p_tab = match p_item.p_tab.clone() {
        Some(t) => t,
        None => return Some(p),
    };
    p.y = ExprY::Tab(Some(TabRef::Rc(p_tab.clone())));
    p.i_table = p_item.i_cursor;
    if p_tab.i_p_key as i32 == i_col {
        p.i_column = -1;
    } else {
        p.i_column = i_col as YnVar;
        if (p_tab.tab_flags & TF_HAS_GENERATED) != 0
            && p_tab
                .a_col
                .get(i_col as usize)
                .map_or(false, |c| (c.col_flags & COLFLAG_GENERATED) != 0)
        {
            p_item.col_used = if p_tab.n_col as i32 >= 64 {
                ALLBITS
            } else {
                (1u64 << p_tab.n_col) - 1
            };
        } else {
            p_item.col_used |= 1u64 << (if i_col >= BMS { BMS - 1 } else { i_col });
        }
    }
    Some(p)
}

/// O que `notValidImpl` faz: relata que a expressão não é válida em algum dos contextos de
/// `nc_flags`. Quando `invalidate` é verdadeiro, troca o nó por `TK_NULL`. O erro é sempre
/// associado ao próprio `p_expr`.
///
/// No C, `sqlite3ResolveNotValid` é uma macro que põe a condição fora da chamada, porque o
/// condicional é quase sempre falso (erros são raros).
fn not_valid_impl(
    db: &mut Connection,
    parse: &mut Parse,
    nc_flags: i32,
    z_msg: &[u8],
    p_expr: &mut Expr,
    invalidate: bool,
) {
    let mut z_in: &[u8] = b"partial index WHERE clauses";
    if (nc_flags & NC_IDXEXPR) != 0 {
        z_in = b"index expressions";
    } else if (nc_flags & NC_ISCHECK) != 0 {
        z_in = b"CHECK constraints";
    } else if (nc_flags & NC_GENCOL) != 0 {
        z_in = b"generated columns";
    }
    error_msg(db, parse, b"%s prohibited in %s", &[txt(z_msg), txt(z_in)]);
    if invalidate {
        p_expr.op = TK_NULL;
    }
    record_error_offset_of_expr(db, Some(&*p_expr));
}

/// A expressão `p` deve codificar um ponto flutuante entre 1.0 e 0.0. Devolve 1024 vezes esse
/// valor, ou -1 se `p` não é um ponto flutuante entre 1.0 e 0.0.
fn expr_probability(p: &Expr) -> i32 {
    if p.op != TK_FLOAT {
        return -1;
    }
    let z = match p.z_token() {
        Some(z) => z,
        None => return -1,
    };
    let (_, r) = atof(z, strlen30(z), SQLITE_UTF8 as u8, true);
    if r > 1.0 {
        return -1;
    }
    (r * 134217728.0) as i32
}

/// Verifica se os tamanhos dos vetores dos dois lados de uma comparação (ou BETWEEN) são iguais.
fn check_vector_sizes(db: &mut Connection, parse: &mut Parse, p_expr: &Expr) {
    let p_left = match p_expr.p_left.as_deref() {
        Some(l) => l,
        None => return,
    };
    let n_left = expr_vector_size(p_left);
    let n_right = if p_expr.op == TK_BETWEEN {
        let list = match p_expr.x_list() {
            Some(l) => l,
            None => return,
        };
        let size_of = |i: usize| -> i32 {
            list.a
                .get(i)
                .and_then(|it| it.p_expr.as_deref())
                .map_or(1, |e| expr_vector_size(e))
        };
        let mut n_right = size_of(0);
        if n_right == n_left {
            n_right = size_of(1);
        }
        n_right
    } else {
        match p_expr.p_right.as_deref() {
            Some(r) => expr_vector_size(r),
            None => return,
        }
    };
    if n_left != n_right {
        error_msg(db, parse, b"row value misused", &[]);
        record_error_offset_of_expr(db, Some(p_expr));
    }
}

/// Callback do `sqlite3WalkExpr()`.
///
/// Resolve os nomes simbólicos em operadores `TK_COLUMN` para o nó corrente da árvore. Devolve 0
/// para continuar a busca árvore abaixo ou 2 para abortar o percurso.
///
/// Também confere erros e resolve nomes de função. O operador das funções agregadas muda para
/// `TK_AGG_FUNCTION`.
fn resolve_expr_step(w: &mut Walker<ResolveCtx<'_, '_>>, p_expr: &mut Expr) -> i32 {
    'sw: {
        match p_expr.op {
            // O operador especial TK_ROW manda usar o rowid da primeira coluna do FROM. Serve às
            // cláusulas LIMIT e ORDER BY do UPDATE e do DELETE e ao UPDATE ... FROM.
            TK_ROW => {
                let nc = match w.u.nc.as_deref_mut() {
                    Some(n) => n,
                    None => break 'sw,
                };
                if let Some(p_item) = nc.p_src_list.as_deref_mut().and_then(|s| s.a.first()) {
                    p_expr.op = TK_COLUMN;
                    p_expr.y = ExprY::Tab(p_item.p_tab.clone().map(TabRef::Rc));
                    p_expr.i_table = p_item.i_cursor;
                    p_expr.i_column -= 1;
                    p_expr.aff_expr = SQLITE_AFF_INTEGER;
                }
            }

            // Uma otimização: tenta converter
            //
            //      "expr IS NOT NULL"  -->  "TRUE"
            //      "expr IS NULL"      -->  "FALSE"
            //
            // se for possível provar que "expr" nunca é NULL. É a "redução de força de NOT NULL".
            //
            // Se a otimização acontece, restaura também as contagens de referência do
            // NameContext ao estado anterior à resolução da coluna do lado esquerdo. Isso evita
            // que "column" conte como referenciada, o que poderia marcar um SELECT como
            // correlacionado por engano.
            //
            // 2024-03-28: cuidado com agregados. Uma coluna solta de uma tabela agregada ainda
            // pode valer NULL mesmo marcada NOT NULL:
            //
            //       CREATE TABLE t1(a INT NOT NULL);
            //       SELECT a, a IS NULL, a IS NOT NULL, count(*) FROM t1;
            //
            // Ao chegar aqui ainda não se sabe se t1 é agregada. É preciso supor o pior e omitir
            // a otimização. Só é seguro aplicá-la dentro da cláusula WHERE.
            TK_NOTNULL | TK_ISNULL => {
                let mut an_ref = [0i32; 8];
                {
                    let top: &mut dyn NcLevel = match w.u.nc.as_deref_mut() {
                        Some(n) => n,
                        None => break 'sw,
                    };
                    let mut lvl: Option<&mut dyn NcLevel> = Some(top);
                    let mut i = 0usize;
                    while let Some(l) = lvl {
                        if i >= an_ref.len() {
                            break;
                        }
                        an_ref[i] = l.n_ref();
                        i += 1;
                        lvl = l.next();
                    }
                }
                walk_expr(w, p_expr.p_left.as_deref_mut());
                if w.u.parse.in_rename_object() {
                    return WRC_PRUNE;
                }
                if expr_can_be_null(p_expr.p_left.as_deref()) {
                    // A expressão pode ser NULL: a otimização não vale.
                    return WRC_PRUNE;
                }
                {
                    let top: &mut dyn NcLevel = match w.u.nc.as_deref_mut() {
                        Some(n) => n,
                        None => break 'sw,
                    };
                    let mut lvl: Option<&mut dyn NcLevel> = Some(top);
                    while let Some(l) = lvl {
                        if (l.flags() & NC_WHERE) == 0 {
                            return WRC_PRUNE; // Fora de um WHERE: inseguro otimizar.
                        }
                        lvl = l.next();
                    }
                }
                p_expr.u = ExprU::IValue((p_expr.op == TK_NOTNULL) as i32);
                p_expr.flags |= EP_INT_VALUE;
                p_expr.op = TK_INTEGER;
                {
                    let top: &mut dyn NcLevel = match w.u.nc.as_deref_mut() {
                        Some(n) => n,
                        None => break 'sw,
                    };
                    let mut lvl: Option<&mut dyn NcLevel> = Some(top);
                    let mut i = 0usize;
                    while let Some(l) = lvl {
                        if i >= an_ref.len() {
                            break;
                        }
                        l.set_n_ref(an_ref[i]);
                        i += 1;
                        lvl = l.next();
                    }
                }
                p_expr.p_left = None;
                return WRC_PRUNE;
            }

            // Um nome de coluna:                  ID
            // Ou nome de tabela e de coluna:      ID.ID
            // Ou banco, tabela e coluna:          ID.ID.ID
            //
            // Os casos TK_ID e TK_DOT ficam juntos para haver uma só chamada a `lookup_name()`.
            TK_ID | TK_DOT => {
                let z_db: Option<Vec<u8>>;
                let z_table: Option<Vec<u8>>;
                let z_col: Vec<u8>;
                let right_dbl_quoted: bool;
                if p_expr.op == TK_ID {
                    z_db = None;
                    z_table = None;
                    z_col = token_of(p_expr);
                    right_dbl_quoted = p_expr.has_property(EP_DBL_QUOTED);
                } else {
                    let nc_flags = match w.u.nc.as_deref() {
                        Some(n) => n.nc_flags,
                        None => break 'sw,
                    };
                    if (nc_flags & (NC_IDXEXPR | NC_GENCOL)) != 0 {
                        let ResolveCtx { parse, db, .. } = &mut w.u;
                        not_valid_impl(
                            db,
                            parse,
                            nc_flags,
                            b"the \".\" operator",
                            p_expr,
                            false,
                        );
                    }
                    let (p_left0, p_right0) =
                        match (p_expr.p_left.as_deref(), p_expr.p_right.as_deref()) {
                            (Some(l), Some(r)) => (l, r),
                            _ => return WRC_PRUNE,
                        };
                    let p_left: &Expr;
                    let p_right: &Expr;
                    if p_right0.op == TK_ID {
                        z_db = None;
                        p_left = p_left0;
                        p_right = p_right0;
                    } else {
                        z_db = Some(token_of(p_left0));
                        p_left = match p_right0.p_left.as_deref() {
                            Some(l) => l,
                            None => return WRC_PRUNE,
                        };
                        p_right = match p_right0.p_right.as_deref() {
                            Some(r) => r,
                            None => return WRC_PRUNE,
                        };
                    }
                    z_table = Some(token_of(p_left));
                    z_col = token_of(p_right);
                    right_dbl_quoted = p_right.has_property(EP_DBL_QUOTED);
                    if w.u.parse.in_rename_object() {
                        let a_expr = expr_addr(p_expr);
                        let a_right = expr_addr(p_right);
                        let a_left = expr_addr(p_left);
                        let a_y = &p_expr.y as *const ExprY as usize;
                        rename_token_remap(w.u.parse, a_expr, a_right);
                        rename_token_remap(w.u.parse, a_y, a_left);
                    }
                }
                let ResolveCtx { parse, db, nc } = &mut w.u;
                let nc: &mut dyn NcLevel = match nc.as_deref_mut() {
                    Some(n) => n,
                    None => break 'sw,
                };
                return lookup_name(
                    db,
                    parse,
                    z_db,
                    z_table,
                    z_col,
                    right_dbl_quoted,
                    nc,
                    p_expr,
                );
            }

            // Resolve nomes de função.
            TK_FUNCTION => {
                let n: i32 = p_expr.x_list().map_or(0, |l| l.a.len() as i32); // Número de argumentos.
                let mut no_such_func = false; // Verdadeiro se a função não existe.
                let mut wrong_num_args = false; // Verdadeiro se o número de argumentos é errado.
                let mut is_agg = false; // Verdadeiro se é função agregada.
                let z_id = token_of(p_expr); // O nome da função.
                let has_win = p_expr.is_window_func();
                let saved_allow_flags: i32;
                let mut p_def: Option<Rc<FuncDef>>;
                let in_rename = w.u.parse.in_rename_object();
                {
                    let ResolveCtx { parse, db, nc } = &mut w.u;
                    let nc: &mut dyn NcLevel = match nc.as_deref_mut() {
                        Some(n) => n,
                        None => break 'sw,
                    };
                    let enc = db.enc;
                    saved_allow_flags = nc.flags() & (NC_ALLOWAGG | NC_ALLOWWIN);
                    p_def = find_function(db, &z_id, n, enc, 0);
                    if p_def.is_none() {
                        p_def = find_function(db, &z_id, -2, enc, 0);
                        if p_def.is_none() {
                            no_such_func = true;
                        } else {
                            wrong_num_args = true;
                        }
                    } else if let Some(def) = p_def.clone() {
                        is_agg = def.x_finalize.is_some();
                        if (def.func_flags & SQLITE_FUNC_UNLIKELY) != 0 {
                            p_expr.set_property(EP_UNLIKELY);
                            if n == 2 {
                                p_expr.i_table = p_expr
                                    .x_list()
                                    .and_then(|l| l.a.get(1))
                                    .and_then(|it| it.p_expr.as_deref())
                                    .map_or(-1, |e| expr_probability(e));
                                if p_expr.i_table < 0 {
                                    error_msg(
                                        db,
                                        parse,
                                        b"second argument to %#T() must be a constant between 0.0 and 1.0",
                                        &[expr_token_arg(p_expr)],
                                    );
                                    nc.set_n_nc_err(nc.n_nc_err() + 1);
                                }
                            } else {
                                // EVIDENCE-OF: R-61304-29449 A função unlikely(X) equivale a
                                // likelihood(X, 0.0625). A função likely(X) é a abreviação de
                                // likelihood(X,0.9375).
                                // TUNING: a probabilidade de unlikely() é 0.0625 e a de likely()
                                // é 0.9375.
                                p_expr.i_table =
                                    if def.z_name.first() == Some(&b'u') { 8388608 } else { 125829120 };
                            }
                        }
                        let auth = auth_check(db, parse, SQLITE_FUNCTION, None, Some(def.z_name.as_slice()), None);
                        if auth != SQLITE_OK {
                            if auth == SQLITE_DENY {
                                error_msg(
                                    db,
                                    parse,
                                    b"not authorized to use function: %#T",
                                    &[expr_token_arg(p_expr)],
                                );
                                nc.set_n_nc_err(nc.n_nc_err() + 1);
                            }
                            p_expr.op = TK_NULL;
                            return WRC_PRUNE;
                        }
                        if (def.func_flags & (SQLITE_FUNC_CONSTANT | SQLITE_FUNC_SLOCHNG)) != 0 {
                            // Para o EP_ConstFunc, funções de data e hora e outras que mudam
                            // devagar valem como constantes: são constantes durante uma consulta.
                            // Isso permite tirá-las de laços internos.
                            p_expr.set_property(EP_CONST_FUNC);
                        }
                        if (def.func_flags & SQLITE_FUNC_CONSTANT) == 0 {
                            // Funções claramente não determinísticas, como random(), mas também
                            // as de data e hora que usam 'now' e outras como sqlite_version(),
                            // não podem entrar num índice ou coluna gerada. Curiosamente podem
                            // entrar num CHECK (SQLServer, MySQL e PostgreSQL também aceitam).
                            let f = nc.flags();
                            if (f & (NC_IDXEXPR | NC_PARTIDX | NC_GENCOL)) != 0 {
                                not_valid_impl(
                                    db,
                                    parse,
                                    f,
                                    b"non-deterministic functions",
                                    p_expr,
                                    false,
                                );
                            }
                        } else {
                            // `NC_SelfRef & 0xff == NC_SelfRef`: cabe em 8 bits.
                            p_expr.op2 = (nc.flags() & NC_SELFREF) as u8;
                            if (nc.flags() & NC_FROMDDL) != 0 {
                                p_expr.set_property(EP_FROM_DDL);
                            }
                        }
                        if (def.func_flags & SQLITE_FUNC_INTERNAL) != 0
                            && parse.nested == 0
                            && (db.m_db_flags & DBFLAG_INTERNAL_FUNC) == 0
                        {
                            // Funções só de uso interno são proibidas, a menos que o SQL esteja
                            // sendo compilado por `sqlite3NestedParse()` ou que o controle de
                            // teste SQLITE_TESTCTRL_INTERNAL_FUNCTIONS as tenha ativado.
                            no_such_func = true;
                            p_def = None;
                        } else if (def.func_flags & (SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE)) != 0
                            && !in_rename
                        {
                            expr_function_usable(db, parse, &*p_expr, &def);
                        }
                    }

                    if !in_rename {
                        let def_ref = p_def.as_deref();
                        let f = nc.flags();
                        if def_ref.map_or(false, |d| d.x_value.is_none()) && has_win {
                            error_msg(
                                db,
                                parse,
                                b"%#T() may not be used as a window function",
                                &[expr_token_arg(p_expr)],
                            );
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                        } else if (is_agg && (f & NC_ALLOWAGG) == 0)
                            || (is_agg
                                && def_ref.map_or(false, |d| (d.func_flags & SQLITE_FUNC_WINDOW) != 0)
                                && !has_win)
                            || (is_agg && has_win && (f & NC_ALLOWWIN) == 0)
                        {
                            let z_type: &[u8] = if def_ref
                                .map_or(false, |d| (d.func_flags & SQLITE_FUNC_WINDOW) != 0)
                                || has_win
                            {
                                b"window"
                            } else {
                                b"aggregate"
                            };
                            error_msg(
                                db,
                                parse,
                                b"misuse of %s function %#T()",
                                &[txt(z_type), expr_token_arg(p_expr)],
                            );
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                            is_agg = false;
                        } else if no_such_func && db.init.busy == 0 {
                            error_msg(
                                db,
                                parse,
                                b"no such function: %#T",
                                &[expr_token_arg(p_expr)],
                            );
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                        } else if wrong_num_args {
                            error_msg(
                                db,
                                parse,
                                b"wrong number of arguments to function %#T()",
                                &[expr_token_arg(p_expr)],
                            );
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                        } else if !is_agg && p_expr.has_property(EP_WIN_FUNC) {
                            error_msg(
                                db,
                                parse,
                                b"FILTER may not be used with non-aggregate %#T()",
                                &[expr_token_arg(p_expr)],
                            );
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                        } else if !is_agg && p_expr.p_left.is_some() {
                            expr_order_by_aggregate_error(db, parse, &*p_expr);
                            nc.set_n_nc_err(nc.n_nc_err() + 1);
                        }
                        if is_agg {
                            // Funções de janela não podem ser argumento de agregadas nem de
                            // outras funções de janela. Mas agregadas podem ser argumento de
                            // funções de janela.
                            let f = nc.flags();
                            nc.set_flags(f & !(NC_ALLOWWIN | if !has_win { NC_ALLOWAGG } else { 0 }));
                        }
                    } else if p_expr.has_property(EP_WIN_FUNC) || p_expr.p_left.is_some() {
                        is_agg = true;
                    }
                }
                walk_expr_list(w, p_expr.x_list_mut());
                if is_agg {
                    if p_expr.p_left.is_some() {
                        let l = p_expr.p_left.as_deref_mut().and_then(|e| e.x_list_mut());
                        walk_expr_list(w, l);
                    }
                    if has_win {
                        if !in_rename {
                            let ResolveCtx { parse, db, nc } = &mut w.u;
                            if let (Some(nc), Some(win), Some(def)) =
                                (nc.as_deref_mut(), p_expr.y_win_mut(), p_def.as_ref())
                            {
                                window_update(parse, db, nc.win_defn(), win, def);
                            }
                            if db.malloc_failed != 0 {
                                break 'sw;
                            }
                        }
                        walk_expr_list(
                            w,
                            p_expr.y_win_mut().and_then(|x| x.p_partition.as_deref_mut()),
                        );
                        walk_expr_list(
                            w,
                            p_expr.y_win_mut().and_then(|x| x.p_order_by.as_deref_mut()),
                        );
                        walk_expr(w, p_expr.y_win_mut().and_then(|x| x.p_filter.as_deref_mut()));
                        if let Some(nc) = w.u.nc.as_deref_mut() {
                            if let (Some(cnt), Some(win)) = (nc.win_link(), p_expr.y_win_mut()) {
                                window_link(cnt, win);
                            }
                            nc.nc_flags |= NC_HASWIN;
                        }
                    } else {
                        p_expr.op = TK_AGG_FUNCTION;
                        p_expr.op2 = 0;
                        if p_expr.has_property(EP_WIN_FUNC) {
                            walk_expr(
                                w,
                                p_expr.y_win_mut().and_then(|x| x.p_filter.as_deref_mut()),
                            );
                        }
                        let ResolveCtx { parse, db, nc } = &mut w.u;
                        let top: &mut dyn NcLevel = match nc.as_deref_mut() {
                            Some(n) => n,
                            None => break 'sw,
                        };
                        // `pNC2`: sobe pelos contextos de fora até achar o que contém a fonte
                        // referenciada.
                        let mut depth2 = 0usize;
                        let mut found = false;
                        while let Some(l) = level_at(&mut *top, depth2) {
                            let refs = {
                                let sl: Option<&SrcList> = l.src_list().map(|s| &*s);
                                references_src_list(&mut *p_expr, sl) != 0
                            };
                            if refs {
                                found = true;
                                break;
                            }
                            let nn = l.n_nested_select();
                            p_expr.op2 = p_expr.op2.wrapping_add(1).wrapping_add(nn as u8);
                            depth2 += 1;
                        }
                        if found {
                            if let Some(def) = p_def.as_deref() {
                                if let Some(l) = level_at(&mut *top, depth2) {
                                    p_expr.op2 = p_expr.op2.wrapping_add(l.n_nested_select() as u8);
                                    // SQLITE_FUNC_MINMAX == NC_MinMaxAgg e
                                    // SQLITE_FUNC_ANYORDER == NC_OrderAgg.
                                    let extra = ((def.func_flags ^ SQLITE_FUNC_ANYORDER)
                                        & (SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER))
                                        as i32;
                                    let f = l.flags();
                                    l.set_flags(f | NC_HASAGG | extra);
                                }
                            }
                        }
                    }
                    if let Some(nc) = w.u.nc.as_deref_mut() {
                        nc.nc_flags |= saved_allow_flags;
                    }
                }
                // FIX ME: calcular `affinity` da expressão a partir do tipo de retorno esperado
                // da função.
                return WRC_PRUNE;
            }

            TK_SELECT | TK_EXISTS | TK_IN => {
                if p_expr.use_x_select() {
                    let (n_ref, nc_flags) = match w.u.nc.as_deref() {
                        Some(n) => (n.n_ref, n.nc_flags),
                        None => break 'sw,
                    };
                    if (nc_flags & NC_SELFREF) != 0 {
                        let ResolveCtx { parse, db, .. } = &mut w.u;
                        not_valid_impl(db, parse, nc_flags, b"subqueries", p_expr, true);
                    } else {
                        walk_select(w, p_expr.x_select_mut());
                    }
                    if let Some(nc) = w.u.nc.as_deref_mut() {
                        if n_ref != nc.n_ref {
                            p_expr.set_property(EP_VAR_SELECT);
                            if let Some(s) = p_expr.x_select_mut() {
                                s.sel_flags |= SF_CORRELATED;
                            }
                        }
                        nc.nc_flags |= NC_SUBQUERY;
                    }
                }
            }

            TK_VARIABLE => {
                let nc_flags = match w.u.nc.as_deref() {
                    Some(n) => n.nc_flags,
                    None => break 'sw,
                };
                if (nc_flags & (NC_ISCHECK | NC_PARTIDX | NC_IDXEXPR | NC_GENCOL)) != 0 {
                    let ResolveCtx { parse, db, .. } = &mut w.u;
                    not_valid_impl(db, parse, nc_flags, b"parameters", p_expr, true);
                }
            }

            TK_IS | TK_ISNOT => {
                // Trata os casos especiais "x IS TRUE", "x IS FALSE", "x IS NOT TRUE" e
                // "x IS NOT FALSE".
                let is_true_false;
                {
                    let p_right = skip_collate_and_likely_slot(&mut p_expr.p_right).as_deref_mut();
                    match p_right {
                        Some(r) if r.op == TK_ID || r.op == TK_TRUEFALSE => {
                            let rc = resolve_expr_step(w, r);
                            if rc == WRC_ABORT {
                                return WRC_ABORT;
                            }
                            is_true_false = r.op == TK_TRUEFALSE;
                        }
                        _ => is_true_false = false,
                    }
                }
                if is_true_false {
                    p_expr.op2 = p_expr.op;
                    p_expr.op = TK_TRUTH;
                    return WRC_CONTINUE;
                }
                // Sem `break` de propósito: segue para a conferência dos vetores.
                if w.u.db.malloc_failed != 0 {
                    break 'sw;
                }
                let ResolveCtx { parse, db, .. } = &mut w.u;
                check_vector_sizes(db, parse, &*p_expr);
            }

            TK_BETWEEN | TK_EQ | TK_NE | TK_LT | TK_LE | TK_GT | TK_GE => {
                if w.u.db.malloc_failed != 0 {
                    break 'sw;
                }
                let ResolveCtx { parse, db, .. } = &mut w.u;
                check_vector_sizes(db, parse, &*p_expr);
            }

            _ => {}
        }
    }
    if w.u.parse.n_err != 0 {
        WRC_ABORT
    } else {
        WRC_CONTINUE
    }
}

// ---------------------------------------------------------------------------------------------
// ORDER BY e GROUP BY
// ---------------------------------------------------------------------------------------------

/// `p_e_list` é a lista de expressões que formam o resultado de um SELECT. `p_e` é um termo do
/// ORDER BY ou do GROUP BY. Confere se `p_e` é um identificador simples que corresponde ao nome
/// AS de um dos termos de `p_e_list`. Se for, devolve um inteiro de 1 a N (N é o número de
/// elementos de `p_e_list`) que indica a entrada. Se não há casamento, ou se `p_e` não é um
/// identificador simples, devolve 0.
///
/// `p_e_list` já foi resolvida; `p_e` não.
fn resolve_as_name(p_e_list: &ExprList, p_e: &Expr) -> i32 {
    if p_e.op == TK_ID {
        let z_col = p_e.z_token();
        for (i, item) in p_e_list.a.iter().enumerate() {
            if item.fg.e_e_name == ENAME_NAME
                && stricmp(item.z_e_name.as_deref(), z_col) == 0
            {
                return i as i32 + 1;
            }
        }
    }
    0
}

/// `p_e` é um termo isolado do ORDER BY de um SELECT composto. A expressão não foi resolvida.
///
/// Quando esta rotina é chamada já se sabe que o termo não é um índice inteiro para o resultado
/// (esse caso é do chamador).
///
/// Tenta casar `p_e` com as colunas do resultado do SELECT mais à esquerda. Devolve o índice `i`
/// da coluna que casou, para o chamador ordenar pela i-ésima coluna. A coluna mais à esquerda é
/// a 1: o mesmo inteiro que o SQL usaria para indicar a coluna.
///
/// Se não casa, devolve 0. Se há erro, devolve -1.
fn resolve_order_by_term_to_expr_list(
    db: &mut Connection,
    parse: &mut Parse,
    p_e_list: &ExprList,
    p_src: &mut SrcList,
    p_e: &mut Expr,
) -> i32 {
    // Resolve todos os nomes da expressão do termo do ORDER BY.
    let mut nc = name_context_new();
    nc.p_src_list = Some(p_src);
    nc.u_nc = NcU::EList(p_e_list);
    nc.nc_flags = NC_ALLOWAGG | NC_UELIST | NC_NOSELECT;
    nc.n_nc_err = 0;
    let saved_supp_err = db.suppress_err;
    db.suppress_err = 1;
    let rc = resolve_expr_names(db, parse, &mut nc, Some(&mut *p_e));
    db.suppress_err = saved_supp_err;
    if rc != 0 {
        return 0;
    }

    // Tenta casar a expressão do ORDER BY com uma expressão do resultado. Devolve o índice
    // (base 1) da entrada que casou.
    for (i, item) in p_e_list.a.iter().enumerate() {
        if expr_compare(None, item.p_expr.as_deref(), Some(&*p_e), -1) < 2 {
            return i as i32 + 1;
        }
    }

    // Se não casa, devolve 0.
    0
}

/// Gera o erro de termo do ORDER BY ou GROUP BY fora da faixa.
fn resolve_out_of_range_error(
    db: &mut Connection,
    parse: &mut Parse,
    z_type: &[u8],
    i: i32,
    mx: i32,
    p_error: Option<&Expr>,
) {
    error_msg(
        db,
        parse,
        b"%r %s BY term out of range - should be between 1 and %d",
        &[PrintfArg::Int(i as i64), txt(z_type), PrintfArg::Int(mx as i64)],
    );
    record_error_offset_of_expr(db, p_error);
}

/// Analisa a cláusula ORDER BY de um SELECT composto. Altera cada termo do ORDER BY para uma
/// constante inteira entre 1 e N, sendo N o número de colunas do SELECT composto.
///
/// Termos que já são inteiros entre 1 e N ficam como estão. Termos inteiros fora da faixa geram
/// erro. Termos que são expressões são comparados com as expressões do resultado do SELECT
/// composto, do mais à esquerda para a direita. No primeiro casamento o termo vira o número da
/// coluna.
///
/// Devolve o número de erros vistos.
fn resolve_compound_order_by(
    db: &mut Connection,
    parse: &mut Parse,
    p_select: &mut Select,
) -> i32 {
    let Select { p_order_by, p_e_list, p_src, p_prior, .. } = p_select;
    let p_order_by = match p_order_by.as_deref_mut() {
        Some(o) => o,
        None => return 0,
    };
    if p_order_by.a.len() as i32 > db.a_limit[SQLITE_LIMIT_COLUMN as usize] {
        error_msg(db, parse, b"too many terms in ORDER BY clause", &[]);
        return 1;
    }
    for item in p_order_by.a.iter_mut() {
        item.fg.done = false;
    }

    // Os termos do composto, do mais à esquerda ao mais à direita (a cadeia `pNext` do C).
    let mut terms: Vec<(&ExprList, &mut SrcList)> = Vec::new();
    if let (Some(el), Some(sr)) = (p_e_list.as_deref(), p_src.as_deref_mut()) {
        terms.push((el, sr));
    }
    let mut cur = p_prior.as_deref_mut();
    while let Some(s) = cur {
        let Select { p_e_list, p_src, p_prior, .. } = s;
        if let (Some(el), Some(sr)) = (p_e_list.as_deref(), p_src.as_deref_mut()) {
            terms.push((el, sr));
        }
        cur = p_prior.as_deref_mut();
    }
    terms.reverse();

    let mut more_to_do = true;
    let mut idx = 0usize;
    while idx < terms.len() && more_to_do {
        more_to_do = false;
        let (p_e_list_t, p_src_t) = {
            let t = &mut terms[idx];
            (t.0, &mut *t.1)
        };
        for i in 0..p_order_by.a.len() {
            let p_item = &mut p_order_by.a[i];
            if p_item.fg.done {
                continue;
            }
            let slot = skip_collate_and_likely_slot(&mut p_item.p_expr);
            let p_e = match slot.as_deref_mut() {
                Some(e) => e,
                None => continue,
            };
            let mut i_col: i32 = -1;
            if expr_is_integer(p_e, &mut i_col) != 0 {
                if i_col <= 0 || i_col > p_e_list_t.a.len() as i32 {
                    resolve_out_of_range_error(
                        db,
                        parse,
                        b"ORDER",
                        i as i32 + 1,
                        p_e_list_t.a.len() as i32,
                        Some(&*p_e),
                    );
                    return 1;
                }
            } else {
                i_col = resolve_as_name(p_e_list_t, p_e);
                if i_col == 0 {
                    // Agora testa se `p_e` casa com um dos valores devolvidos por `p_select`.
                    // No caso usual duplica a expressão, resolve os símbolos nela e compara com
                    // cada expressão do resultado; no fim apaga a cópia.
                    //
                    // Se isto roda como parte de um ALTER TABLE e os símbolos se resolvem,
                    // resolve também os símbolos da expressão de verdade, para o código de
                    // alter.c poder alterar as referências a colunas dentro do ORDER BY.
                    let p_dup = expr_dup(Some(&*p_e), 0);
                    if db.malloc_failed == 0 {
                        if let Some(mut dup) = p_dup {
                            i_col = resolve_order_by_term_to_expr_list(
                                db, parse, p_e_list_t, p_src_t, &mut dup,
                            );
                            if parse.in_rename_object() && i_col > 0 {
                                resolve_order_by_term_to_expr_list(
                                    db, parse, p_e_list_t, p_src_t, p_e,
                                );
                            }
                        }
                    }
                }
            }
            if i_col > 0 {
                // Converte o termo do ORDER BY no número inteiro da coluna `i_col`, preservando
                // o COLLATE se existir.
                if !parse.in_rename_object() {
                    let mut p_new = match crate::expr::expr(TK_INTEGER as i32, None) {
                        Some(n) => n,
                        None => return 1,
                    };
                    p_new.flags |= EP_INT_VALUE;
                    p_new.u = ExprU::IValue(i_col);
                    *slot = Some(p_new);
                    p_item.i_order_by_col = i_col as u16;
                }
                p_item.fg.done = true;
            } else {
                more_to_do = true;
            }
        }
        idx += 1;
    }
    for i in 0..p_order_by.a.len() {
        if !p_order_by.a[i].fg.done {
            error_msg(
                db,
                parse,
                b"%r ORDER BY term does not match any column in the result set",
                &[PrintfArg::Int(i as i64 + 1)],
            );
            return 1;
        }
    }
    0
}

/// `sqlite3ResolveOrderGroupBy`: confere cada termo do ORDER BY ou GROUP BY `p_order_by` do
/// SELECT cuja lista de resultado é `p_e_list`. Se um termo referencia uma expressão do resultado
/// (o campo `ExprList.a.u.x.iOrderByCol`), converte o termo numa cópia da coluna do resultado.
///
/// Se encontra erros, deixa a mensagem em `parse` e devolve diferente de zero. Sem erros devolve
/// zero.
pub fn resolve_order_group_by(
    db: &mut Connection,
    parse: &mut Parse,
    p_e_list: &ExprList,
    p_order_by: Option<&mut ExprList>,
    z_type: &[u8],
) -> i32 {
    let p_order_by = match p_order_by {
        Some(o) => o,
        None => return 0,
    };
    if db.malloc_failed != 0 || parse.in_rename_object() {
        return 0;
    }
    if p_order_by.a.len() as i32 > db.a_limit[SQLITE_LIMIT_COLUMN as usize] {
        error_msg(db, parse, b"too many terms in %s BY clause", &[txt(z_type)]);
        return 1;
    }
    for i in 0..p_order_by.a.len() {
        let col = p_order_by.a[i].i_order_by_col;
        if col != 0 {
            if col as usize > p_e_list.a.len() {
                resolve_out_of_range_error(
                    db,
                    parse,
                    z_type,
                    i as i32 + 1,
                    p_e_list.a.len() as i32,
                    None,
                );
                return 1;
            }
            if let Some(e) = p_order_by.a[i].p_expr.as_deref_mut() {
                resolve_alias(db, parse, p_e_list, col as usize - 1, e, 0);
            }
        }
    }
    0
}

/// Callback do walker de `windowRemoveExprFromSelect`.
fn resolve_remove_windows_cb(_w: &mut Walker<()>, p_expr: &mut Expr) -> i32 {
    if p_expr.has_property(EP_WIN_FUNC) {
        if let Some(p_win) = p_expr.y_win_mut() {
            window_unlink_from_select(p_win);
        }
    }
    WRC_CONTINUE
}

/// Remove da lista `Select.pWin` os objetos `Window` que pertencem à expressão `p_expr`.
/// `has_win` é o `pSelect->pWin != 0` do C.
fn window_remove_expr_from_select(has_win: bool, p_expr: Option<&mut Expr>) {
    if has_win {
        let mut w: Walker<()> = Walker {
            x_expr_callback: Some(resolve_remove_windows_cb),
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: (),
        };
        walk_expr(&mut w, p_expr);
    }
}

/// `p_order_by` é um ORDER BY ou GROUP BY do SELECT cuja lista de resultado é `p_e_list`. O
/// contexto de nomes do SELECT é `nc`. `z_type` é "ORDER" ou "GROUP". `has_win` diz se o SELECT
/// tem janelas ligadas (`pSelect->pWin`).
///
/// Resolve cada termo da cláusula numa expressão. Se o termo é um inteiro I entre 1 e N (N é o
/// número de colunas do resultado), a expressão resolvida é uma cópia da I-ésima expressão do
/// resultado. Se o termo é um identificador que corresponde ao nome AS de uma expressão do
/// resultado, resolve para uma cópia dessa expressão. Nos demais casos resolve do modo usual, com
/// `resolve_expr_names()`.
///
/// Devolve o número de erros. Se há erros pode haver uma mensagem em `parse` (menos falta de
/// memória).
fn resolve_order_group_by_clause(
    db: &mut Connection,
    parse: &mut Parse,
    nc: &mut NameContext<'_>,
    p_e_list: &ExprList,
    has_win: bool,
    p_order_by: &mut ExprList,
    z_type: &[u8],
) -> i32 {
    let n_result = p_e_list.a.len() as i32; // Número de termos do resultado.
    for i in 0..p_order_by.a.len() {
        let p_item = &mut p_order_by.a[i];
        {
            let p_e2 = match skip_collate_and_likely_slot(&mut p_item.p_expr).as_deref() {
                Some(e) => e,
                None => continue,
            };
            if z_type.first() != Some(&b'G') {
                let i_col = resolve_as_name(p_e_list, p_e2);
                if i_col > 0 {
                    // Se há casamento com um nome AS, marca esta coluna do ORDER BY como cópia da
                    // `i_col`-ésima coluna do resultado. A chamada seguinte a
                    // `resolve_order_group_by()` converte a expressão numa cópia da
                    // `i_col`-ésima expressão do resultado.
                    p_item.i_order_by_col = i_col as u16;
                    continue;
                }
            }
            let mut i_col: i32 = 0;
            if expr_is_integer(p_e2, &mut i_col) != 0 {
                // O termo do ORDER BY é uma constante inteira. De novo, grava o número da coluna
                // para `resolve_order_group_by()` converter o termo numa cópia da expressão do
                // resultado.
                if i_col < 1 || i_col > 0xffff {
                    resolve_out_of_range_error(db, parse, z_type, i as i32 + 1, n_result, Some(p_e2));
                    return 1;
                }
                p_item.i_order_by_col = i_col as u16;
                continue;
            }
        }

        // Nos demais casos trata o termo como uma expressão comum.
        p_item.i_order_by_col = 0;
        if resolve_expr_names(db, parse, nc, p_item.p_expr.as_deref_mut()) != 0 {
            return 1;
        }
        for j in 0..p_e_list.a.len() {
            if expr_compare(None, p_item.p_expr.as_deref(), p_e_list.a[j].p_expr.as_deref(), -1) == 0
            {
                // Como a expressão virou uma referência a uma expressão idêntica do resultado,
                // tira da lista `Select.pWin` todos os objetos Window que pertencem a ela.
                window_remove_expr_from_select(has_win, p_item.p_expr.as_deref_mut());
                p_item.i_order_by_col = (j + 1) as u16;
            }
        }
    }
    resolve_order_group_by(db, parse, p_e_list, Some(p_order_by), z_type)
}

// ---------------------------------------------------------------------------------------------
// SELECT
// ---------------------------------------------------------------------------------------------

/// O que se guarda do termo à direita de um composto (o `pNext` do C) ao resolver o da esquerda.
struct NextTerm {
    op: u8,
    sel_flags: u32,
    n_expr: usize,
}

/// A parte do corpo de `resolveSelectStep` que vale para um termo `p` do composto (o corpo do
/// `while(p)` do C). Devolve diferente de zero para abortar.
///
/// `outer` é o contexto que contém este SELECT (`pOuterNC`); `is_compound` é `isCompound`,
/// `n_compound` é quantos termos já foram processados, e `next` é o termo à direita.
fn resolve_select_term(
    db: &mut Connection,
    parse: &mut Parse,
    outer: &mut Option<&mut NameContext<'_>>,
    p: &mut Select,
    is_compound: bool,
    n_compound: i32,
    next: Option<&NextTerm>,
) -> i32 {
    p.sel_flags |= SF_RESOLVED;

    // Resolve as expressões das cláusulas LIMIT e OFFSET. Elas não podem referenciar nomes, então
    // usa um NameContext vazio.
    {
        let mut limit_nc = name_context_new();
        limit_nc.n_win_linked = Some(&mut p.n_win_linked);
        if resolve_expr_names(db, parse, &mut limit_nc, p.p_limit.as_deref_mut()) != 0 {
            return 1;
        }
    }

    // Se a flag SF_Converted está ligada, este Select foi criado por
    // `convertCompoundSelectToSubquery()`. Nesse caso o ORDER BY (`p.p_order_by`) deve ser
    // resolvido como parte da subconsulta e não do pai. Este bloco move o ORDER BY para a
    // subconsulta; ele volta depois que os nomes são resolvidos.
    if (p.sel_flags & SF_CONVERTED) != 0 {
        let ob = p.p_order_by.take();
        match p
            .p_src
            .as_deref_mut()
            .and_then(|s| s.a.get_mut(0))
            .and_then(|it| it.p_select.as_deref_mut())
        {
            Some(sub) => sub.p_order_by = ob,
            None => p.p_order_by = ob,
        }
    }

    // Resolve recursivamente os nomes de todas as subconsultas da cláusula FROM.
    if let Some(o) = outer.as_deref_mut() {
        o.n_nested_select += 1;
    }
    if let Some(src) = p.p_src.as_deref_mut() {
        for p_item in src.a.iter_mut() {
            if let Some(sel) = p_item.p_select.as_deref_mut() {
                if (sel.sel_flags & SF_RESOLVED) == 0 {
                    let n_ref = outer.as_deref().map_or(0, |o| o.n_ref);
                    let z_saved_context = parse.z_auth_context.clone();
                    if let Some(name) = &p_item.z_name {
                        parse.z_auth_context = Some(name.clone());
                    }
                    resolve_select_names(db, parse, sel, outer.as_deref_mut());
                    parse.z_auth_context = z_saved_context;
                    if parse.n_err != 0 {
                        return 1;
                    }

                    // Se o número de referências ao contexto de fora mudou ao resolver as
                    // expressões da subconsulta, ela é correlacionada. Só é preciso conferir o
                    // contador do contexto de fora mais interno, porque `lookup_name()` aumenta o
                    // contador de todos os contextos entre o corrente e o que contém a coluna.
                    if let Some(o) = outer.as_deref() {
                        p_item.fg.is_correlated = o.n_ref > n_ref;
                    }
                }
            }
        }
    }
    if let Some(o) = outer.as_deref_mut() {
        if o.n_nested_select > 0 {
            o.n_nested_select -= 1;
        }
    }

    // Monta o contexto de nomes local que se passa para resolver a lista do resultado.
    let mut s_nc = name_context_new();
    s_nc.nc_flags = NC_ALLOWAGG | NC_ALLOWWIN;
    s_nc.p_src_list = p.p_src.as_deref_mut();
    s_nc.p_next = outer.as_deref_mut().map(|o| o as &mut dyn NcLevel);
    s_nc.n_win_linked = Some(&mut p.n_win_linked);
    s_nc.p_win_defn = p.p_win_defn.clone();

    // Resolve os nomes do resultado.
    if resolve_expr_list_names(db, parse, &mut s_nc, p.p_e_list.as_deref_mut()) != 0 {
        return 1;
    }
    s_nc.nc_flags &= !NC_ALLOWWIN;

    // Se não há função agregada no resultado nem GROUP BY, não permite agregadas nas outras
    // expressões.
    if p.p_group_by.is_some() || (s_nc.nc_flags & NC_HASAGG) != 0 {
        // NC_MinMaxAgg == SF_MinMaxAgg e NC_OrderAgg == SF_OrderByReqd.
        p.sel_flags |= SF_AGGREGATE | ((s_nc.nc_flags & (NC_MINMAXAGG | NC_ORDERAGG)) as u32);
    } else {
        s_nc.nc_flags &= !NC_ALLOWAGG;
    }

    // Põe a lista de colunas do resultado no contexto de nomes antes de analisar as outras
    // expressões do SELECT. Assim as expressões do WHERE (e outras) podem se referir a
    // expressões pelos aliases do resultado.
    //
    // Ponto menor: nesse caso a expressão é reavaliada a cada referência.
    if let Some(el) = p.p_e_list.as_deref() {
        s_nc.u_nc = NcU::EList(el);
    }
    s_nc.nc_flags |= NC_UELIST;
    if p.p_having.is_some() {
        if (p.sel_flags & SF_AGGREGATE) == 0 {
            error_msg(db, parse, b"HAVING clause on a non-aggregate query", &[]);
            return 1;
        }
        if resolve_expr_names(db, parse, &mut s_nc, p.p_having.as_deref_mut()) != 0 {
            return 1;
        }
    }
    s_nc.nc_flags |= NC_WHERE;
    if resolve_expr_names(db, parse, &mut s_nc, p.p_where.as_deref_mut()) != 0 {
        return 1;
    }
    s_nc.nc_flags &= !NC_WHERE;

    // Resolve os nomes nos argumentos de funções com valor de tabela. A lista de argumentos sai
    // do item enquanto é resolvida, porque o contexto `s_nc` possui o empréstimo da `SrcList`.
    let n_src = s_nc.p_src_list.as_ref().map_or(0, |s| s.a.len());
    for i in 0..n_src {
        let arg = match s_nc.p_src_list.as_deref_mut().and_then(|s| s.a.get_mut(i)) {
            Some(it) if it.fg.is_tab_func => match &mut it.u1 {
                SrcU1::FuncArg(a) => a.take(),
                _ => None,
            },
            _ => None,
        };
        if let Some(mut a) = arg {
            let rc = resolve_expr_list_names(db, parse, &mut s_nc, Some(&mut *a));
            if let Some(it) = s_nc.p_src_list.as_deref_mut().and_then(|s| s.a.get_mut(i)) {
                if let SrcU1::FuncArg(slot) = &mut it.u1 {
                    *slot = Some(a);
                }
            }
            if rc != 0 {
                return 1;
            }
        }
    }

    if parse.in_rename_object() {
        for p_win in p.p_win_defn.iter_mut() {
            if resolve_expr_list_names(db, parse, &mut s_nc, p_win.p_order_by.as_deref_mut()) != 0
                || resolve_expr_list_names(db, parse, &mut s_nc, p_win.p_partition.as_deref_mut())
                    != 0
            {
                return 1;
            }
        }
    }

    // O ORDER BY e o GROUP BY não podem se referir a termos de consultas de fora.
    s_nc.p_next = None;
    s_nc.nc_flags |= NC_ALLOWAGG | NC_ALLOWWIN;

    // Se é uma consulta composta convertida, move o ORDER BY da subconsulta de volta para a
    // consulta pai. Nesse ponto cada termo do ORDER BY virou um valor inteiro. Os inteiros são
    // trocados por cópias das expressões do resultado correspondentes pela chamada a
    // `resolve_order_group_by_clause()` mais abaixo.
    if (p.sel_flags & SF_CONVERTED) != 0 {
        let ob = s_nc
            .p_src_list
            .as_deref_mut()
            .and_then(|s| s.a.get_mut(0))
            .and_then(|it| it.p_select.as_deref_mut())
            .and_then(|sub| sub.p_order_by.take());
        p.p_order_by = ob;
    }

    // Processa o ORDER BY dos SELECTs simples. O ORDER BY dos compostos é tratado depois que os
    // resultados de todos os termos do composto foram resolvidos.
    //
    // Se há ORDER BY num termo de composto que não é o mais à direita, é erro de sintaxe. Mas o
    // erro só é detectado bem mais tarde, então é preciso resolver os símbolos desse ORDER BY
    // errado, por consistência.
    let has_win = s_nc.n_win_linked.as_deref().map_or(false, |n| *n > 0);
    if p.p_order_by.is_some() && (is_compound as i32) <= n_compound {
        // Adia o ORDER BY do termo mais à direita de um composto.
        if let (Some(el), Some(ob)) = (p.p_e_list.as_deref(), p.p_order_by.as_deref_mut()) {
            if resolve_order_group_by_clause(db, parse, &mut s_nc, el, has_win, ob, b"ORDER") != 0 {
                return 1;
            }
        }
    }
    if db.malloc_failed != 0 {
        return 1;
    }
    s_nc.nc_flags &= !NC_ALLOWWIN;

    // Resolve o GROUP BY. Ao mesmo tempo confere que ele não contém funções agregadas.
    if p.p_group_by.is_some() {
        if let (Some(el), Some(gb)) = (p.p_e_list.as_deref(), p.p_group_by.as_deref_mut()) {
            if resolve_order_group_by_clause(db, parse, &mut s_nc, el, has_win, gb, b"GROUP") != 0
                || db.malloc_failed != 0
            {
                return 1;
            }
            for p_item in gb.a.iter() {
                if p_item.p_expr.as_deref().map_or(false, |e| e.has_property(EP_AGG)) {
                    error_msg(
                        db,
                        parse,
                        b"aggregate functions are not allowed in the GROUP BY clause",
                        &[],
                    );
                    return 1;
                }
            }
        }
    }

    // Se este termo faz parte de um SELECT composto, confere se o resultado tem o número certo de
    // expressões.
    if let Some(nx) = next {
        let n_here = p.p_e_list.as_deref().map_or(0, |e| e.a.len());
        if n_here != nx.n_expr {
            select_wrong_num_terms_error(db, parse, nx.op, nx.sel_flags);
            return 1;
        }
    }
    0
}

/// Resolve os nomes do SELECT `p` e de todos os seus descendentes.
fn resolve_select_step(w: &mut Walker<ResolveCtx<'_, '_>>, p: &mut Select) -> i32 {
    if (p.sel_flags & SF_RESOLVED) != 0 {
        return WRC_PRUNE;
    }
    let ResolveCtx { parse, db, nc: outer } = &mut w.u;

    // Normalmente `sqlite3SelectExpand()` roda primeiro e já expandiu este SELECT. Mas se é uma
    // subconsulta dentro de uma expressão, `sqlite3ResolveExprNames()` é chamada sem o
    // `sqlite3SelectExpand()` antes. Nesse caso `sqlite3SelectPrep()` faz todo o trabalho deste
    // SELECT: ele chama `sqlite3SelectExpand()` e esta rotina na ordem certa.
    if (p.sel_flags & SF_EXPANDED) == 0 {
        select_prep(db, parse, p, outer.as_deref_mut());
        return if parse.n_err != 0 { WRC_ABORT } else { WRC_PRUNE };
    }

    let is_compound = p.p_prior.is_some(); // Verdadeiro se `p` é um SELECT composto.
    let mut n_compound: i32 = 0; // Termos do composto já processados.
    let mut next: Option<NextTerm> = None;
    {
        let mut cur: Option<&mut Select> = Some(&mut *p);
        while let Some(term) = cur {
            if resolve_select_term(
                db,
                parse,
                outer,
                term,
                is_compound,
                n_compound,
                next.as_ref(),
            ) != 0
            {
                return WRC_ABORT;
            }

            // Avança para o próximo termo do composto.
            next = Some(NextTerm {
                op: term.op,
                sel_flags: term.sel_flags,
                n_expr: term.p_e_list.as_deref().map_or(0, |e| e.a.len()),
            });
            cur = term.p_prior.as_deref_mut();
            n_compound += 1;
        }
    }

    // Resolve o ORDER BY de um SELECT composto depois que todos os termos do composto foram
    // resolvidos.
    if is_compound && resolve_compound_order_by(db, parse, p) != 0 {
        return WRC_ABORT;
    }

    WRC_PRUNE
}

/// O `mWFlags` do walker de resolução: liga o bit de rename quando o `Parse` está num
/// `IN_RENAME_OBJECT`.
fn resolve_walker_flags(parse: &Parse) -> u16 {
    if parse.in_rename_object() {
        WALKER_FLAG_IN_RENAME
    } else {
        0
    }
}

/// Esta rotina percorre uma árvore de expressão e resolve referências a colunas de tabelas e a
/// colunas do resultado. Ao mesmo tempo confere o uso de funções e liga uma flag se vê alguma
/// função agregada.
///
/// Para resolver colunas de tabelas procura nós (ou subárvores) da forma X.Y.Z, Y.Z ou só Z, onde
///
/// * X é o nome de um banco, por exemplo "main", "temp" ou o nome simbólico de um banco anexado;
/// * Y é o nome de uma tabela do FROM, ou, num gatilho, um dos nomes especiais "old" ou "new";
/// * Z é o nome de uma coluna da tabela Y.
///
/// O nó na raiz da subárvore muda assim: `op` vira `TK_COLUMN`, `y.pTab` aponta para a tabela de
/// X.Y, `i_column` é o índice da coluna em X.Y (-1 para o rowid) e `i_table` é o número do
/// cursor do VDBE de X.Y.
///
/// Para resolver referências ao resultado procura nós da forma Z (sem X e Y) em que Z casa com o
/// lado direito de um AS no resultado de um SELECT. A expressão Z é trocada por uma cópia do
/// lado esquerdo da expressão do resultado. A resolução de nomes de tabela e de função acontece
/// na expressão trocada. Por exemplo, em
///
/// ```text
///      SELECT a+b AS x, c+d AS y FROM t1 ORDER BY x;
/// ```
///
/// o "x" do ORDER BY vira "a+b":
///
/// ```text
///      SELECT a+b AS x, c+d AS y FROM t1 ORDER BY a+b;
/// ```
///
/// As chamadas de função são conferidas: a função deve existir e receber o número certo de
/// argumentos. Se a função é agregada, liga `NC_HasAgg` e o `op` muda de `TK_FUNCTION` para
/// `TK_AGG_FUNCTION`. Se uma expressão contém funções agregadas, liga `EP_Agg` nela.
///
/// Se algo está errado deixa a mensagem em `parse`. Devolve o número de erros.
pub fn resolve_expr_names(
    db: &mut Connection,
    parse: &mut Parse,
    nc: &mut NameContext<'_>,
    p_expr: Option<&mut Expr>,
) -> i32 {
    let p_expr = match p_expr {
        Some(e) => e,
        None => return SQLITE_OK,
    };
    let saved_has_agg = nc.nc_flags & (NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
    nc.nc_flags &= !(NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
    let m_w_flags = resolve_walker_flags(parse);
    let no_select = (nc.nc_flags & NC_NOSELECT) != 0;
    parse.n_height += p_expr.n_height;
    let n_height = parse.n_height;
    if expr_check_height(db, parse, n_height) != 0 {
        return SQLITE_ERROR;
    }
    {
        let mut w = Walker {
            x_expr_callback: Some(resolve_expr_step),
            x_select_callback: if no_select { None } else { Some(resolve_select_step) },
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags,
            u: ResolveCtx { parse: &mut *parse, db: &mut *db, nc: Some(&mut *nc) },
        };
        walk_expr_nn(&mut w, &mut *p_expr);
    }
    parse.n_height -= p_expr.n_height;
    // EP_Agg == NC_HasAgg e EP_Win == NC_HasWin.
    p_expr.set_property((nc.nc_flags & (NC_HASAGG | NC_HASWIN)) as u32);
    nc.nc_flags |= saved_has_agg;
    (nc.n_nc_err > 0 || parse.n_err > 0) as i32
}

/// Resolve todos os nomes de todas as expressões de uma lista de expressões. É como
/// [`resolve_expr_names`], mas para uma lista.
///
/// O resultado é `SQLITE_OK` (0) em caso de sucesso e `SQLITE_ERROR` (1) em caso de falha.
pub fn resolve_expr_list_names(
    db: &mut Connection,
    parse: &mut Parse,
    nc: &mut NameContext<'_>,
    p_list: Option<&mut ExprList>,
) -> i32 {
    let p_list = match p_list {
        Some(l) => l,
        None => return SQLITE_OK,
    };
    let mut saved_has_agg: i32 = nc.nc_flags & (NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
    nc.nc_flags &= !(NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
    let m_w_flags = resolve_walker_flags(parse);
    for item in p_list.a.iter_mut() {
        let p_expr = match item.p_expr.as_deref_mut() {
            Some(e) => e,
            None => continue,
        };
        parse.n_height += p_expr.n_height;
        let n_height = parse.n_height;
        if expr_check_height(db, parse, n_height) != 0 {
            return SQLITE_ERROR;
        }
        {
            let mut w = Walker {
                x_expr_callback: Some(resolve_expr_step),
                x_select_callback: Some(resolve_select_step),
                x_select_callback2: None,
                walker_depth: 0,
                e_code: 0,
                m_w_flags,
                u: ResolveCtx { parse: &mut *parse, db: &mut *db, nc: Some(&mut *nc) },
            };
            walk_expr_nn(&mut w, &mut *p_expr);
        }
        parse.n_height -= p_expr.n_height;
        // EP_Agg == NC_HasAgg e EP_Win == NC_HasWin.
        if (nc.nc_flags & (NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG)) != 0 {
            p_expr.set_property((nc.nc_flags & (NC_HASAGG | NC_HASWIN)) as u32);
            saved_has_agg |= nc.nc_flags & (NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
            nc.nc_flags &= !(NC_HASAGG | NC_MINMAXAGG | NC_HASWIN | NC_ORDERAGG);
        }
        if parse.n_err > 0 {
            return SQLITE_ERROR;
        }
    }
    nc.nc_flags |= saved_has_agg;
    SQLITE_OK
}

/// Resolve todos os nomes de todas as expressões de um SELECT e de todos os seus descendentes,
/// incluindo os compostos de `p->pPrior`, as subconsultas em expressões e as subconsultas usadas
/// como termos do FROM.
///
/// Ver [`resolve_expr_names`] para a descrição das transformações. Todos os SELECTs devem ter
/// sido expandidos com `sqlite3SelectExpand()` antes desta rotina.
pub fn resolve_select_names(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Select,
    p_outer_nc: Option<&mut NameContext<'_>>,
) {
    let m_w_flags = resolve_walker_flags(parse);
    let mut w = Walker {
        x_expr_callback: Some(resolve_expr_step),
        x_select_callback: Some(resolve_select_step),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags,
        u: ResolveCtx { parse: &mut *parse, db: &mut *db, nc: p_outer_nc },
    };
    walk_select(&mut w, Some(p));
}

/// Callback que troca a tabela do snapshot de `resolve_self_reference` pela marca `TabRef::Own`.
fn fix_own_tab(w: &mut Walker<Rc<Table>>, p_expr: &mut Expr) -> i32 {
    let is_snapshot = match &p_expr.y {
        ExprY::Tab(Some(TabRef::Rc(t))) => Rc::ptr_eq(t, &w.u),
        _ => false,
    };
    if is_snapshot {
        p_expr.y = ExprY::Tab(Some(TabRef::Own));
    }
    WRC_CONTINUE
}

/// Resolve nomes em expressões que só podem referenciar uma tabela ou nenhuma tabela. Exemplos:
///
/// | caso | flag de tipo |
/// |------|--------------|
/// | (1) restrições CHECK | `NC_IsCheck` |
/// | (2) WHERE de índices parciais | `NC_PartIdx` |
/// | (3) expressões em índices sobre expressões | `NC_IdxExpr` |
/// | (4) expressões argumento do VACUUM INTO | 0 |
/// | (5) expressões GENERATED ALWAYS AS | `NC_GenCol` |
///
/// Em todos os casos, menos o (4), `Expr.iTable` dos nós `TK_COLUMN` vale -1 e `Expr.iColumn`
/// guarda o número da coluna. No caso (4) um nó `TK_COLUMN` causa erro.
///
/// Qualquer erro deixa uma mensagem em `parse`.
///
/// Os nós `TK_COLUMN` resolvidos apontam para a tabela dona da árvore como `TabRef::Own`.
pub fn resolve_self_reference(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: Option<&Table>,
    type_: i32,
    mut p_expr: Option<&mut Expr>,
    mut p_list: Option<&mut ExprList>,
) -> i32 {
    let mut type_ = type_;
    let mut s_src = SrcList::default(); // Uma SrcList falsa para `Parse.p_new_table`.
    let mut snapshot: Option<Rc<Table>> = None;
    if let Some(tab) = p_tab {
        // O que a resolução lê da tabela: nome, colunas e flags.
        let snap = Rc::new(Table {
            z_name: tab.z_name.clone(),
            a_col: tab.a_col.clone(),
            tnum: tab.tnum,
            tab_flags: tab.tab_flags,
            i_p_key: tab.i_p_key,
            n_col: tab.n_col,
            n_nv_col: tab.n_nv_col,
            key_conf: tab.key_conf,
            e_tab_type: tab.e_tab_type,
            p_schema: tab.p_schema,
            ..Table::default()
        });
        s_src.a.push(SrcItem {
            z_name: Some(tab.z_name.clone()),
            p_tab: Some(snap.clone()),
            i_cursor: -1,
            ..SrcItem::default()
        });
        snapshot = Some(snap);
        if db.dbs.get(1).map_or(true, |d| tab.p_schema != d.schema.id) {
            // Faz o EP_FromDDL ser ligado nos nós TK_FUNCTION de elementos de esquema que não
            // são do TEMP.
            type_ |= NC_FROMDDL;
        }
    }
    let mut rc;
    {
        let mut s_nc = name_context_new();
        s_nc.p_src_list = Some(&mut s_src);
        s_nc.nc_flags = type_ | NC_ISDDL;
        rc = resolve_expr_names(db, parse, &mut s_nc, p_expr.as_deref_mut());
        if rc != SQLITE_OK {
            return rc;
        }
        if p_list.is_some() {
            rc = resolve_expr_list_names(db, parse, &mut s_nc, p_list.as_deref_mut());
        }
    }
    if let Some(snap) = snapshot {
        let mut w: Walker<Rc<Table>> = Walker {
            x_expr_callback: Some(fix_own_tab),
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: snap,
        };
        walk_expr(&mut w, p_expr);
        if let Some(l) = p_list {
            walk_expr_list(&mut w, Some(l));
        }
    }
    rc
}
