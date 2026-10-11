//! Tipos de sqliteInt.h que descrevem árvores de sintaxe e esquema, no modelo v2
//! (CONVENTIONS.md, itens 6 e 7): `Expr`, `ExprList`, `IdList`, `SrcList`, `Select`, `With`,
//! `Window`, `Upsert`, `Trigger*`, `Table`, `Column`, `Index`, `FKey`, `Schema`, `AggInfo`,
//! `NameContext`, `Walker`, `DbFixer`, `RenameToken`/`RenameCtx` e os pequenos registros que
//! os acompanham. As constantes (`EP_*`, `TF_*`, `SF_*`, `JT_*`, `OE_*`, `COLFLAG_*`, `NC_*`,
//! `SRT_*`, `TABTYP_*`...) vivem em `crate::consts`.
//!
//! Regras do modelo, válidas para tudo abaixo:
//!
//! - Ponteiro que POSSUI no C vira `Option<Box<T>>` (ou `Vec<T>` para lista encadeada ou
//!   array). Ponteiro que só APONTA para outro lugar vira handle (`SchemaId`, `AggInfoId`,
//!   `CteUseId`, `VTableId`), nome (chave de hash) ou cópia explícita, nunca referência.
//! - O esquema é `Rc<Table>`/`Rc<Index>`/`Rc<Trigger>`/`Rc<FKey>`; quem muda um objeto do
//!   esquema usa `Rc::make_mut` (copia só se houver outro dono). Não existe `Weak`, então as
//!   voltas do C (`Index.pTable`, `Index.pSchema`, `FKey.pFrom`, `TriggerStep.pTrig`,
//!   `Table.pSchema`) viram nome ou `SchemaId`, e quem precisa do pai recebe o pai.
//! - Lista encadeada do C (`pNext`) vira `Vec` na ordem da lista (índice 0 é a cabeça).
//!   Contadores `nExpr`, `nSrc`, `nId`, `nCte`, `nAlloc` somem: valem `Vec::len`.
//! - Campo de bits do C vira `bool` ou `u8` separado. `union` do C vira `enum` quando uma
//!   flag decide a variante, ou campos irmãos quando o C só lê o que escreveu.
//! - Texto é `Vec<u8>` (bytes), nunca `String`.
//!
//! CAMPOS (C -> Rust; o que some está listado ao fim de cada bloco):
//!
//! `Token`: z -> z: Vec<u8> (cópia), n -> z.len(), (novo) i_ofst: i32 (z - zSql no C).
//!
//! `Expr`: op, affExpr -> aff_expr, op2, flags, u {zToken, iValue} -> u: ExprU, pLeft/pRight
//!   -> p_left/p_right, x {pList, pSelect} -> x: ExprX, nHeight -> n_height, iTable -> i_table,
//!   iColumn -> i_column: YnVar, iAgg -> i_agg, w {iJoin, iOfst} -> w: i32 (um só inteiro),
//!   pAggInfo -> p_agg_info: Option<AggInfoId>, y {pTab, pWin, sub} -> y: ExprY.
//!   (somem: vvaFlags, que é SQLITE_DEBUG)
//!
//! `ExprList`: nExpr/nAlloc/a[] -> a: Vec<ExprListItem>.
//! `ExprListItem`: pExpr -> p_expr, zEName -> z_e_name, fg -> fg: ExprListFg,
//!   u.x.iOrderByCol/iAlias/u.iConstExprReg -> i_order_by_col, i_alias, i_const_expr_reg.
//!
//! `IdList`: nId/a[] -> a: Vec<IdListItem>, eU4 -> e_u4. `IdListItem`: zName -> z_name,
//!   u4.idx -> idx. (some: u4.pExpr, "NOT USED" no C)
//!
//! `SrcList`: nSrc/nAlloc/a[] -> a: Vec<SrcItem>.
//! `SrcItem`: pSchema -> p_schema: Option<SchemaId>, zDatabase/zName/zAlias -> z_database/
//!   z_name/z_alias, pTab -> p_tab, pSelect -> p_select, addrFillSub -> addr_fill_sub,
//!   regReturn -> reg_return, regResult -> reg_result, fg -> fg: SrcItemFg, iCursor ->
//!   i_cursor, u3 -> u3: SrcU3, colUsed -> col_used, u1 -> u1: SrcU1, u2 -> u2: SrcU2.
//! `OnOrUsing`: pOn/pUsing -> p_on/p_using.
//!
//! `Select`: op, nSelectRow -> n_select_row, selFlags -> sel_flags, iLimit/iOffset ->
//!   i_limit/i_offset, selId -> sel_id, addrOpenEphm -> addr_open_ephm, pEList -> p_e_list,
//!   pSrc -> p_src, pWhere -> p_where, pGroupBy -> p_group_by, pHaving -> p_having, pOrderBy ->
//!   p_order_by, pPrior -> p_prior, pLimit -> p_limit, pWith -> p_with, pWinDefn -> p_win_defn.
//!   (somem: pNext -> has_next: bool, pWin -> n_win_linked: u32)
//! `SelectDest`: eDest -> e_dest, iSDParm/iSDParm2 -> i_sd_parm/i_sd_parm2, iSdst -> i_sdst,
//!   nSdst -> n_sdst, zAffSdst -> z_aff_sdst, pOrderBy -> p_order_by.
//!
//! `With`: nCte/a[] -> a: Vec<Cte>, bView -> b_view. (some: pOuter, é a pilha de WITH do Parse)
//! `Cte`: zName -> z_name, pCols -> p_cols, pSelect -> p_select, zCteErr -> z_cte_err,
//!   pUse -> p_use: Option<CteUseId>, eM10d -> e_m10d.
//! `CteUse`: nUse, addrM9e, regRtn, iCur, nRowEst, eM10d em snake_case.
//!
//! `Window`: zName/zBase, pPartition, pOrderBy, eFrmType, eStart, eEnd, bImplicitFrame,
//!   eExclude, pStart, pEnd, pFilter, pWFunc, iEphCsr, regAccum, regResult, csrApp, regApp,
//!   regPart, nBufferCol, iArgCol, regOne, regStartRowid, regEndRowid, bExprArgs em snake_case,
//!   (novo) link_seq: u32. (somem: ppThis, pNextWin, pOwner)
//!
//! `Upsert`: pUpsertTarget, pUpsertTargetWhere, pUpsertSet, pUpsertWhere, pNextUpsert, isDoUpdate,
//!   isDup, pUpsertIdx, pUpsertSrc, regData, iDataCur, iIdxCur em snake_case. (some: pToFree)
//!
//! `Trigger`: zName -> z_name, table -> z_table, op, tr_tm, bReturning -> b_returning, pWhen ->
//!   p_when, pColumns -> p_columns, pSchema -> p_schema, pTabSchema -> p_tab_schema, step_list ->
//!   step_list: Vec<TriggerStep>. (some: pNext)
//! `TriggerStep`: op, orconf, pSelect, zTarget, pFrom, pWhere, pExprList, pIdList, pUpsert,
//!   zSpan em snake_case. (somem: pTrig, pNext, pLast)
//! `TriggerPrg`: pTrigger -> p_trigger, pProgram -> i_program, orconf, aColmask -> a_colmask.
//! `Returning`: pReturnEL, retTrig, iRetCur, nRetCol, iRetReg, zName. (somem: pParse, retTStep,
//!   que é o único elemento de `ret_trig.step_list`)
//!
//! `Table`: zName -> z_name, aCol -> a_col, pIndex -> p_index: Vec<Rc<Index>>, zColAff ->
//!   z_col_aff, pCheck -> p_check, tnum, tabFlags -> tab_flags, iPKey -> i_p_key, nCol ->
//!   n_col (pode ser -1), nNVCol -> n_nv_col, nRowLogEst -> n_row_log_est, szTabRow ->
//!   sz_tab_row, keyConf -> key_conf, eTabType -> e_tab_type, u -> u: TableU, pTrigger ->
//!   p_trigger: Vec<Rc<Trigger>>, pSchema -> p_schema: SchemaId.
//!   (somem: nTabRef, costMult, u.vtab.p)
//! `Column`: zCnName -> z_cn_name, notNull -> not_null, eCType -> e_c_type, affinity, szEst ->
//!   sz_est, hName -> h_name, iDflt -> i_dflt, colFlags -> col_flags.
//! `Index`: zName, aiColumn, aiRowLogEst, zColAff, aSortOrder, azColl, pPartIdxWhere, aColExpr,
//!   tnum, szIdxRow, nKeyCol, nColumn, onError, idxType, bUnordered, uniqNotNull, isResized,
//!   isCovering, noSkipScan, hasStat1, bLowQual, bNoQuery, bAscKeyBug, bHasVCol, bHasExpr,
//!   colNotIdxed em snake_case. (somem: pTable, pNext, pSchema, os campos de STAT4)
//! `FKey`: zTo -> z_to, (novo) z_from, nCol -> n_col, isDeferred, aAction -> a_action,
//!   aCol -> a_col: Vec<FKeyColMap>. (somem: pFrom, pNextFrom, pNextTo, pPrevTo, apTrigger)
//! `Schema`: schema_cookie, iGeneration, tblHash, idxHash, trigHash, fkeyHash, pSeqTab,
//!   file_format, enc, schemaFlags, cache_size em snake_case, (novo) id: SchemaId.
//!
//! `AggInfo`, `AggInfoCol`, `AggInfoFunc`: ver o bloco de cada um. `NameContext`, `Walker`,
//!   `DbFixer`, `RenameToken`, `RenameCtx`, `IndexedExpr`, `AutoincInfo`, `TableLock`,
//!   `Savepoint`, `Module`, `VTable`: ver o bloco de cada um.

use std::any::Any;
use std::rc::Rc;

use crate::consts::{
    Bitmask, COLFLAG_HIDDEN, DB_RESETWANTED, DB_SCHEMALOADED, DB_UNRESETVIEWS, EP_FROM_DDL,
    EP_INNER_ON, EP_INT_VALUE, EP_IS_FALSE, EP_IS_TRUE, EP_OUTER_ON, EP_REDUCED, EP_SUBRTN,
    EP_TOKEN_ONLY, EP_WIN_FUNC, EP_X_IS_SELECT, OE_NONE, SF_NESTEDFROM, SQLITE_AFF_NUMERIC,
    SQLITE_IDXTYPE_PRIMARYKEY, SRT_DISTQUEUE, SRT_FIFO, TABTYP_NORM, TABTYP_VIEW, TABTYP_VTAB,
    TF_NO_VISIBLE_ROWID, TF_WITHOUT_ROWID, TK_COLUMN, TK_FILTER,
};
use crate::connection::FuncDef;
use crate::hash::{hash_init, Hash};
use std::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------------------------
// Tipos básicos e handles
// ---------------------------------------------------------------------------------------------

/// `ynVar`: com `SQLITE_MAX_VARIABLE_NUMBER=250000` (>32767) o C usa `int`.
pub type YnVar = i32;

/// `LogEst`: logaritmo de uma estimativa de linhas (`i16`).
pub type LogEst = i16;

/// Handle de um `Schema` (o `Schema*` do C). Estável: sobrevive a `DETACH` de outro banco, que
/// desloca os índices de `Connection.dbs`. `SchemaId(0)` é "nenhum". O índice do banco se acha
/// por varredura de `Connection.dbs` (o `sqlite3SchemaToIndex` do C também é linear).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SchemaId(pub u32);

/// Handle de um `AggInfo` dentro de `Parse.agg_infos` (o `AggInfo*` de `Expr.pAggInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AggInfoId(pub u32);

/// Handle de um `CteUse` dentro de `Parse.cte_uses` (o `CteUse*` de `SrcItem.u2` e `Cte.pUse`).
/// Vive até o fim da geração de código, como a lista de limpeza do C.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CteUseId(pub u32);

/// Handle de um `VTable` dentro de `Connection.vtabs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VTableId(pub u32);

/// `Token`: um pedaço do SQL. O C guarda `z` apontando para dentro de `Parse.zSql`; aqui o
/// texto é copiado e `i_ofst` guarda a posição (`z - zSql`), que o `RENAME` e `Expr.w.iOfst`
/// usam. `i_ofst` é -1 quando o token não vem do texto (literais internos).
#[derive(Clone, Debug, Default)]
pub struct Token {
    /// Texto do token (`n` do C é `z.len()`).
    pub z: Vec<u8>,
    /// Deslocamento do início do token em `zSql`.
    pub i_ofst: i32,
}

// ---------------------------------------------------------------------------------------------
// Expr
// ---------------------------------------------------------------------------------------------

/// `Expr.u`: o texto do token ou o valor inteiro, conforme `EP_IntValue`.
#[derive(Clone, Debug)]
pub enum ExprU {
    /// `zToken`: texto terminado e sem aspas; `None` é o ponteiro nulo.
    Token(Option<Vec<u8>>),
    /// `iValue`: inteiro não negativo (vale quando `EP_IntValue`).
    IValue(i32),
}

impl Default for ExprU {
    fn default() -> Self {
        ExprU::Token(None)
    }
}

/// `Expr.x`: a lista de argumentos ou a subconsulta, conforme `EP_xIsSelect`.
#[derive(Clone, Default)]
pub enum ExprX {
    /// Ponteiro nulo (`x.pList == NULL`).
    #[default]
    None,
    /// `x.pList`: IN, EXISTS, SELECT, CASE, FUNCTION, BETWEEN.
    List(Box<ExprList>),
    /// `x.pSelect`: vale quando `EP_xIsSelect`.
    Select(Box<Select>),
}

/// `Expr.y`: a tabela de um TK_COLUMN, a janela de uma função de janela (`EP_WinFunc`) ou os
/// dados da sub-rotina de IN/SELECT/EXISTS (`EP_Subrtn`).
#[derive(Clone)]
pub enum ExprY {
    /// `y.pTab`: `None` é o ponteiro nulo.
    Tab(Option<TabRef>),
    /// `y.pWin`: a janela é POSSUÍDA pela expressão, como no C (`sqlite3ExprDelete` a libera).
    Win(Box<Window>),
    /// `y.sub`: entrada da sub-rotina e registrador do endereço de retorno.
    Sub { i_addr: i32, reg_return: i32 },
}

impl Default for ExprY {
    fn default() -> Self {
        ExprY::Tab(None)
    }
}

/// Referência de um `TK_COLUMN` à sua tabela (`y.pTab`).
///
/// As expressões guardadas na própria tabela (CHECK, DEFAULT/GENERATED, índice sobre expressão,
/// WHERE de índice parcial) apontam, no C, para a tabela que as contém. Um `Rc<Table>` ali
/// formaria um ciclo (a tabela possui a expressão que possui a tabela) e não dá para construir
/// sem `Weak`. Por isso essas expressões levam `Own`, que significa "a tabela que contém esta
/// árvore"; quem lê uma expressão do esquema sempre tem a tabela dona à mão e a resolve com
/// `TabRef::resolve`.
#[derive(Clone)]
pub enum TabRef {
    /// Tabela do esquema ou de uma lista FROM.
    Rc(Rc<Table>),
    /// A tabela dona da árvore (expressão guardada no próprio `Table`).
    Own,
}

impl TabRef {
    /// A tabela apontada; `own` é a tabela dona da árvore.
    #[inline]
    pub fn resolve<'a>(&'a self, own: &'a Rc<Table>) -> &'a Rc<Table> {
        match self {
            TabRef::Rc(t) => t,
            TabRef::Own => own,
        }
    }
}

/// `Expr`: um nó da árvore de uma expressão. Todos os nós têm o tamanho cheio: `EP_Reduced`,
/// `EP_TokenOnly` e `EXPRDUP_REDUCE` do C só economizam memória e não existem como alocação aqui
/// (as flags ficam como o C as deixa, porque `ExprIsFullSize` é consultada).
#[derive(Clone, Default)]
pub struct Expr {
    /// Operação do nó (um `TK_*`).
    pub op: u8,
    /// Afinidade, ou o tipo de RAISE (`affExpr`).
    pub aff_expr: u8,
    /// TK_REGISTER/TK_TRUTH: `op` original; TK_COLUMN: o P5 de OP_Column; TK_AGG_FUNCTION:
    /// profundidade; TK_FUNCTION: flag `NC_SelfRef`.
    pub op2: u8,
    /// Flags `EP_*`.
    pub flags: u32,
    /// Texto do token ou valor inteiro (`u`).
    pub u: ExprU,
    /// Subnó esquerdo.
    pub p_left: Option<Box<Expr>>,
    /// Subnó direito.
    pub p_right: Option<Box<Expr>>,
    /// Lista ou subconsulta (`x`).
    pub x: ExprX,
    /// Altura da árvore que começa neste nó (`nHeight`, `SQLITE_MAX_EXPR_DEPTH>0`).
    pub n_height: i32,
    /// TK_COLUMN: cursor; TK_REGISTER: registrador; TK_TRIGGER: 1 novo, 0 velho; EP_Unlikely:
    /// 134217728 vezes a probabilidade; TK_IN: tabela efêmera; TK_SELECT_COLUMN: quantidade de
    /// colunas à esquerda; TK_SELECT: primeiro registrador do resultado.
    pub i_table: i32,
    /// TK_COLUMN: índice da coluna (-1 é o rowid); TK_VARIABLE: número da variável (>= 1);
    /// TK_SELECT_COLUMN: coluna do vetor de resultado.
    pub i_column: YnVar,
    /// Entrada em `AggInfo.a_col` ou `a_func`.
    pub i_agg: i16,
    /// União `w` do C: `iJoin` (se `EP_OuterON` ou `EP_InnerON`) ou `iOfst`. São o mesmo `int`,
    /// por isso um só campo; use `i_join`/`i_ofst` e os setters.
    pub w: i32,
    /// Usado por TK_AGG_COLUMN e TK_AGG_FUNCTION.
    pub p_agg_info: Option<AggInfoId>,
    /// Tabela, janela ou sub-rotina (`y`).
    pub y: ExprY,
}

impl Expr {
    /// `ExprHasProperty`.
    #[inline]
    pub fn has_property(&self, p: u32) -> bool {
        (self.flags & p) != 0
    }
    /// `ExprHasAllProperty`.
    #[inline]
    pub fn has_all_property(&self, p: u32) -> bool {
        (self.flags & p) == p
    }
    /// `ExprSetProperty`.
    #[inline]
    pub fn set_property(&mut self, p: u32) {
        self.flags |= p;
    }
    /// `ExprClearProperty`.
    #[inline]
    pub fn clear_property(&mut self, p: u32) {
        self.flags &= !p;
    }
    /// `ExprAlwaysTrue`.
    #[inline]
    pub fn always_true(&self) -> bool {
        (self.flags & (EP_OUTER_ON | EP_IS_TRUE)) == EP_IS_TRUE
    }
    /// `ExprAlwaysFalse`.
    #[inline]
    pub fn always_false(&self) -> bool {
        (self.flags & (EP_OUTER_ON | EP_IS_FALSE)) == EP_IS_FALSE
    }
    /// `ExprIsFullSize`.
    #[inline]
    pub fn is_full_size(&self) -> bool {
        (self.flags & (EP_REDUCED | EP_TOKEN_ONLY)) == 0
    }
    /// `ExprUseUToken`.
    #[inline]
    pub fn use_u_token(&self) -> bool {
        (self.flags & EP_INT_VALUE) == 0
    }
    /// `ExprUseUValue`.
    #[inline]
    pub fn use_u_value(&self) -> bool {
        (self.flags & EP_INT_VALUE) != 0
    }
    /// `ExprUseWOfst`.
    #[inline]
    pub fn use_w_ofst(&self) -> bool {
        (self.flags & (EP_INNER_ON | EP_OUTER_ON)) == 0
    }
    /// `ExprUseWJoin`.
    #[inline]
    pub fn use_w_join(&self) -> bool {
        (self.flags & (EP_INNER_ON | EP_OUTER_ON)) != 0
    }
    /// `ExprUseXList`.
    #[inline]
    pub fn use_x_list(&self) -> bool {
        (self.flags & EP_X_IS_SELECT) == 0
    }
    /// `ExprUseXSelect`.
    #[inline]
    pub fn use_x_select(&self) -> bool {
        (self.flags & EP_X_IS_SELECT) != 0
    }
    /// `ExprUseYTab`.
    #[inline]
    pub fn use_y_tab(&self) -> bool {
        (self.flags & (EP_WIN_FUNC | EP_SUBRTN)) == 0
    }
    /// `ExprUseYWin`.
    #[inline]
    pub fn use_y_win(&self) -> bool {
        (self.flags & EP_WIN_FUNC) != 0
    }
    /// `ExprUseYSub`.
    #[inline]
    pub fn use_y_sub(&self) -> bool {
        (self.flags & EP_SUBRTN) != 0
    }
    /// `ExprHasProperty(E, EP_FromDDL)`, atalho usado em muitos pontos.
    #[inline]
    pub fn from_ddl(&self) -> bool {
        (self.flags & EP_FROM_DDL) != 0
    }

    /// `u.zToken` (vazio quando o nó guarda inteiro ou não tem texto).
    #[inline]
    pub fn z_token(&self) -> Option<&[u8]> {
        match &self.u {
            ExprU::Token(Some(z)) => Some(z.as_slice()),
            _ => None,
        }
    }
    /// `u.iValue` (zero quando o nó guarda texto).
    #[inline]
    pub fn i_value(&self) -> i32 {
        match &self.u {
            ExprU::IValue(v) => *v,
            _ => 0,
        }
    }
    /// `w.iJoin`.
    #[inline]
    pub fn i_join(&self) -> i32 {
        self.w
    }
    /// `w.iOfst`.
    #[inline]
    pub fn i_ofst(&self) -> i32 {
        self.w
    }
    /// `x.pList`.
    #[inline]
    pub fn x_list(&self) -> Option<&ExprList> {
        match &self.x {
            ExprX::List(l) => Some(l),
            _ => None,
        }
    }
    /// `x.pList`, mutável.
    #[inline]
    pub fn x_list_mut(&mut self) -> Option<&mut ExprList> {
        match &mut self.x {
            ExprX::List(l) => Some(l),
            _ => None,
        }
    }
    /// `x.pSelect`.
    #[inline]
    pub fn x_select(&self) -> Option<&Select> {
        match &self.x {
            ExprX::Select(s) => Some(s),
            _ => None,
        }
    }
    /// `x.pSelect`, mutável.
    #[inline]
    pub fn x_select_mut(&mut self) -> Option<&mut Select> {
        match &mut self.x {
            ExprX::Select(s) => Some(s),
            _ => None,
        }
    }
    /// `y.pWin`.
    #[inline]
    pub fn y_win(&self) -> Option<&Window> {
        match &self.y {
            ExprY::Win(w) => Some(w),
            _ => None,
        }
    }
    /// `y.pWin`, mutável.
    #[inline]
    pub fn y_win_mut(&mut self) -> Option<&mut Window> {
        match &mut self.y {
            ExprY::Win(w) => Some(w),
            _ => None,
        }
    }
    /// `y.pTab` (só vale quando `use_y_tab`).
    #[inline]
    pub fn y_tab(&self) -> Option<&TabRef> {
        match &self.y {
            ExprY::Tab(t) => t.as_ref(),
            _ => None,
        }
    }
    /// `ExprIsVtab`: TK_COLUMN sobre tabela virtual. `own` é a tabela dona da árvore, para o
    /// caso `TabRef::Own`.
    #[inline]
    pub fn is_vtab(&self, own: Option<&Rc<Table>>) -> bool {
        if self.op != TK_COLUMN {
            return false;
        }
        match self.y_tab() {
            Some(TabRef::Rc(t)) => t.e_tab_type == TABTYP_VTAB,
            Some(TabRef::Own) => own.map_or(false, |t| t.e_tab_type == TABTYP_VTAB),
            None => false,
        }
    }
    /// `IsWindowFunc`: função com OVER (e não só FILTER).
    #[inline]
    pub fn is_window_func(&self) -> bool {
        self.has_property(EP_WIN_FUNC)
            && self.y_win().map_or(false, |w| w.e_frm_type != TK_FILTER)
    }
}

/// Um item de `ExprList.fg`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExprListFg {
    /// Máscara de `KEYINFO_ORDER_*`.
    pub sort_flags: u8,
    /// Significado de `z_e_name` (`ENAME_*`, 2 bits no C).
    pub e_e_name: u8,
    /// O processamento do item terminou.
    pub done: bool,
    /// Expressão constante reutilizável.
    pub reusable: bool,
    /// Adia a avaliação para depois da ordenação.
    pub b_sorter_ref: bool,
    /// Há NULLS FIRST/LAST explícito.
    pub b_nulls: bool,
    /// A coluna foi usada numa subconsulta SF_NestedFrom.
    pub b_used: bool,
    /// Termo da cláusula USING de uma NestedFrom.
    pub b_using_term: bool,
    /// Termo auxiliar de NestedFrom que o "*" do pai não expande.
    pub b_no_expand: bool,
}

/// `ExprList_item`.
#[derive(Clone, Default)]
pub struct ExprListItem {
    /// A árvore da expressão.
    pub p_expr: Option<Box<Expr>>,
    /// Texto associado (`zEName`); o significado está em `fg.e_e_name`.
    pub z_e_name: Option<Vec<u8>>,
    /// Bits de `fg`.
    pub fg: ExprListFg,
    /// `u.x.iOrderByCol`: coluna do resultado, no ORDER BY.
    pub i_order_by_col: u16,
    /// `u.x.iAlias`: índice em `Parse.aAlias`.
    pub i_alias: u16,
    /// `u.iConstExprReg`: registrador onde o valor está em cache (só `Parse.pConstExpr`). No C
    /// divide a memória com `iOrderByCol`/`iAlias`, mas cada lista usa só um dos dois grupos.
    pub i_const_expr_reg: i32,
}

/// `ExprList`: lista de expressões, cada uma com nome opcional. `nExpr` é `a.len()`.
#[derive(Clone, Default)]
pub struct ExprList {
    /// Os itens.
    pub a: Vec<ExprListItem>,
}

// ---------------------------------------------------------------------------------------------
// IdList, SrcList, SrcItem
// ---------------------------------------------------------------------------------------------

/// `IdList_item`.
#[derive(Clone, Default)]
pub struct IdListItem {
    /// Nome do identificador.
    pub z_name: Option<Vec<u8>>,
    /// `u4.idx`: índice em `Table.a_col` da coluna chamada `z_name`. (`u4.pExpr` é "NOT USED".)
    pub idx: i32,
}

/// `IdList`: lista de identificadores (`a, b, c`).
#[derive(Clone, Default)]
pub struct IdList {
    /// `eU4`: qual membro de `u4` vale (`EU4_*`).
    pub e_u4: u8,
    /// Os itens (`nId` é `a.len()`).
    pub a: Vec<IdListItem>,
}

/// `SrcItem.fg`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SrcItemFg {
    /// Tipo de junção com a tabela anterior (`JT_*`).
    pub jointype: u8,
    /// Há NOT INDEXED.
    pub not_indexed: bool,
    /// Há INDEXED BY.
    pub is_indexed_by: bool,
    /// Sintaxe de função com valor de tabela.
    pub is_tab_func: bool,
    /// A subconsulta é correlacionada.
    pub is_correlated: bool,
    /// É uma view materializada.
    pub is_materialized: bool,
    /// Implementada como co-rotina.
    pub via_coroutine: bool,
    /// Referência recursiva num WITH.
    pub is_recursive: bool,
    /// Vem do sqlite_schema.
    pub from_ddl: bool,
    /// É uma CTE.
    pub is_cte: bool,
    /// Este item não pode casar com uma CTE.
    pub not_cte: bool,
    /// `u3` guarda o USING.
    pub is_using: bool,
    /// `u3` já guardou um ON não nulo.
    pub is_on: bool,
    /// O USING foi sintetizado de um NATURAL.
    pub is_synth_using: bool,
    /// `p_select` é uma subconsulta SF_NestedFrom.
    pub is_nested_from: bool,
    /// O ROWID da tabela é referenciado.
    pub rowid_used: bool,
}

/// `SrcItem.u1`: nome do índice, argumentos da função de tabela ou número de linhas do VALUES.
#[derive(Clone)]
pub enum SrcU1 {
    /// `zIndexedBy` (`fg.is_indexed_by`).
    IndexedBy(Option<Vec<u8>>),
    /// `pFuncArg` (`fg.is_tab_func`).
    FuncArg(Option<Box<ExprList>>),
    /// `nRow` (nenhuma das duas flags).
    NRow(u32),
}

impl Default for SrcU1 {
    fn default() -> Self {
        SrcU1::NRow(0)
    }
}

/// `SrcItem.u2`: o índice do INDEXED BY ou o uso da CTE.
#[derive(Clone)]
pub enum SrcU2 {
    /// `pIBIndex` (`fg.is_indexed_by`): índice resolvido, por `Rc`.
    IbIndex(Option<Rc<Index>>),
    /// `pCteUse` (`fg.is_cte`): handle em `Parse.cte_uses`.
    CteUse(Option<CteUseId>),
}

impl Default for SrcU2 {
    fn default() -> Self {
        SrcU2::IbIndex(None)
    }
}

/// `SrcItem.u3`: o ON ou o USING, conforme `fg.is_using`.
#[derive(Clone)]
pub enum SrcU3 {
    /// `pOn`.
    On(Option<Box<Expr>>),
    /// `pUsing`.
    Using(Option<Box<IdList>>),
}

impl Default for SrcU3 {
    fn default() -> Self {
        SrcU3::On(None)
    }
}

/// `SrcItem`: um termo da cláusula FROM. (Na 3.46.1 só existem `u1`, `u2` e `u3`.)
#[derive(Clone, Default)]
pub struct SrcItem {
    /// Esquema ao qual o item foi fixado (`sqlite3FixSrcList`).
    pub p_schema: Option<SchemaId>,
    /// Nome do banco que guarda a tabela.
    pub z_database: Option<Vec<u8>>,
    /// Nome da tabela (o "A" de "A AS B").
    pub z_name: Option<Vec<u8>>,
    /// O "B" de "A AS B".
    pub z_alias: Option<Vec<u8>>,
    /// A tabela do esquema que corresponde a `z_name`.
    pub p_tab: Option<Rc<Table>>,
    /// SELECT usado no lugar do nome da tabela.
    pub p_select: Option<Box<Select>>,
    /// Endereço da sub-rotina que materializa a subconsulta.
    pub addr_fill_sub: i32,
    /// Registrador com o endereço de retorno de `addr_fill_sub`.
    pub reg_return: i32,
    /// Registradores com o resultado de uma co-rotina.
    pub reg_result: i32,
    /// Bits de `fg`.
    pub fg: SrcItemFg,
    /// Cursor do VDBE.
    pub i_cursor: i32,
    /// ON ou USING.
    pub u3: SrcU3,
    /// Bit N ligado se a coluna N é usada (o bit 63 vale para as colunas 63 em diante).
    pub col_used: Bitmask,
    /// Nome do índice, argumentos da função ou número de linhas.
    pub u1: SrcU1,
    /// Índice do INDEXED BY ou uso da CTE.
    pub u2: SrcU2,
}

impl SrcItem {
    /// `u3.pOn` (`None` quando o item usa USING).
    #[inline]
    pub fn p_on(&self) -> Option<&Expr> {
        match &self.u3 {
            SrcU3::On(Some(e)) => Some(e),
            _ => None,
        }
    }
    /// `u3.pUsing` (`None` quando o item usa ON).
    #[inline]
    pub fn p_using(&self) -> Option<&IdList> {
        match &self.u3 {
            SrcU3::Using(Some(l)) => Some(l),
            _ => None,
        }
    }
}

/// `OnOrUsing`: um ON ou um USING (nunca os dois, às vezes nenhum).
#[derive(Clone, Default)]
pub struct OnOrUsing {
    /// Cláusula ON.
    pub p_on: Option<Box<Expr>>,
    /// Cláusula USING.
    pub p_using: Option<Box<IdList>>,
}

/// `SrcList`: uma ou mais tabelas fonte (FROM, ou alvo de DELETE/INSERT/UPDATE).
/// `nSrc` é `a.len()`.
#[derive(Clone, Default)]
pub struct SrcList {
    /// Os termos.
    pub a: Vec<SrcItem>,
}

// ---------------------------------------------------------------------------------------------
// Select, SelectDest, With, Cte, CteUse
// ---------------------------------------------------------------------------------------------

/// `Select`: tudo o que o gerador de código precisa de um SELECT.
///
/// Composto: `p_prior` POSSUI o select à esquerda (a raiz é o mais à direita). O `pNext` do C
/// (o select à direita, não possuído) não existe; sobra `has_next`, que responde ao
/// `p->pNext==0` do C. Os percursos "para a direita" do C (resolução do ORDER BY composto, o
/// laço de `multiSelect`) devem sair da raiz e coletar a cadeia de `p_prior` (ver
/// `Select::compound_chain`).
#[derive(Clone, Default)]
pub struct Select {
    /// TK_UNION, TK_ALL, TK_INTERSECT ou TK_EXCEPT (ou TK_SELECT num select simples).
    pub op: u8,
    /// Estimativa de linhas do resultado.
    pub n_select_row: LogEst,
    /// Flags `SF_*`.
    pub sel_flags: u32,
    /// Registrador do contador de LIMIT.
    pub i_limit: i32,
    /// Registrador do contador de OFFSET.
    pub i_offset: i32,
    /// Identificador único do select.
    pub sel_id: u32,
    /// Endereços dos OP_OpenEphemeral deste select.
    pub addr_open_ephm: [i32; 2],
    /// Os campos do resultado.
    pub p_e_list: Option<Box<ExprList>>,
    /// A cláusula FROM.
    pub p_src: Option<Box<SrcList>>,
    /// WHERE.
    pub p_where: Option<Box<Expr>>,
    /// GROUP BY.
    pub p_group_by: Option<Box<ExprList>>,
    /// HAVING.
    pub p_having: Option<Box<Expr>>,
    /// ORDER BY.
    pub p_order_by: Option<Box<ExprList>>,
    /// Select anterior (à esquerda) de um composto.
    pub p_prior: Option<Box<Select>>,
    /// `pNext != NULL`: existe um select à direita neste composto.
    pub has_next: bool,
    /// LIMIT (um TK_LIMIT com o limite em `p_left` e o offset em `p_right`); `None` é sem LIMIT.
    pub p_limit: Option<Box<Expr>>,
    /// Cláusula WITH presa a este select.
    pub p_with: Option<Box<With>>,
    /// `pWin`: quantas janelas já foram ligadas a este select (`sqlite3WindowLink`). As janelas
    /// em si moram nas expressões (`ExprY::Win`); a ordem da lista do C é `Window.link_seq`
    /// crescente. Zero é "lista vazia".
    pub n_win_linked: u32,
    /// `pWinDefn`: as definições nomeadas da cláusula WINDOW.
    pub p_win_defn: Vec<Window>,
}

impl Select {
    /// `IsNestedFrom`.
    #[inline]
    pub fn is_nested_from(s: Option<&Select>) -> bool {
        s.map_or(false, |s| (s.sel_flags & SF_NESTEDFROM) != 0)
    }

    /// A cadeia de um composto da esquerda para a direita, terminando em `self`. O `p = p->pNext`
    /// que parte do select mais à esquerda no C é um percurso desta lista.
    pub fn compound_chain(&self) -> Vec<&Select> {
        let mut v: Vec<&Select> = Vec::new();
        let mut p = Some(self);
        while let Some(s) = p {
            v.push(s);
            p = s.p_prior.as_deref();
        }
        v.reverse();
        v
    }
}

/// `SelectDest`: para onde vão os resultados de um SELECT.
#[derive(Clone, Default)]
pub struct SelectDest {
    /// Como dispor dos resultados (`SRT_*`).
    pub e_dest: u8,
    /// Parâmetro do método de descarte.
    pub i_sd_parm: i32,
    /// Segundo parâmetro.
    pub i_sd_parm2: i32,
    /// Primeiro registrador onde os resultados são escritos.
    pub i_sdst: i32,
    /// Quantidade de registradores alocados.
    pub n_sdst: i32,
    /// Afinidade usada em SRT_Set.
    pub z_aff_sdst: Option<Vec<u8>>,
    /// Colunas-chave de SRT_Queue e SRT_DistQueue.
    pub p_order_by: Option<Box<ExprList>>,
}

impl SelectDest {
    /// `IgnorableDistinct`: o DISTINCT é ignorado para este destino (implica `ignorable_orderby`).
    #[inline]
    pub fn ignorable_distinct(&self) -> bool {
        self.e_dest <= SRT_DISTQUEUE
    }
    /// `IgnorableOrderby`.
    #[inline]
    pub fn ignorable_orderby(&self) -> bool {
        self.e_dest <= SRT_FIFO
    }
}

/// `Cte`: uma expressão de tabela comum.
#[derive(Clone, Default)]
pub struct Cte {
    /// Nome da CTE.
    pub z_name: Option<Vec<u8>>,
    /// Nomes explícitos das colunas, ou `None`.
    pub p_cols: Option<Box<ExprList>>,
    /// A definição da CTE.
    pub p_select: Option<Box<Select>>,
    /// Mensagem de erro para referência circular (estática no C).
    pub z_cte_err: Option<&'static str>,
    /// Uso da CTE (handle em `Parse.cte_uses`).
    pub p_use: Option<CteUseId>,
    /// A flag MATERIALIZED (`M10D_*`).
    pub e_m10d: u8,
}

/// `With`: uma cláusula WITH com uma ou mais CTEs. O `pOuter` do C (a cláusula WITH que contém
/// esta, ligada por `sqlite3WithPush`) não é do `With`: é a pilha de WITH do `Parse`.
#[derive(Clone, Default)]
pub struct With {
    /// As CTEs (`nCte` é `a.len()`).
    pub a: Vec<Cte>,
    /// Pertence ao select mais externo de uma view.
    pub b_view: i32,
}

/// `CteUse`: o que deve sobreviver de uma CTE durante todo o parse (a `Cte` pode ser apagada
/// por otimização). Vive em `Parse.cte_uses`, indexado por `CteUseId`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CteUse {
    /// Quantos usam a CTE.
    pub n_use: i32,
    /// Início da sub-rotina de materialização.
    pub addr_m9e: i32,
    /// Registrador do endereço de retorno de `addr_m9e`.
    pub reg_rtn: i32,
    /// Tabela efêmera com a materialização.
    pub i_cur: i32,
    /// Estimativa de linhas.
    pub n_row_est: LogEst,
    /// A flag MATERIALIZED.
    pub e_m10d: u8,
}

// ---------------------------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------------------------

/// `Window`: a cláusula OVER de uma função de janela, uma definição da cláusula WINDOW, ou o
/// FILTER de um agregado (`e_frm_type == TK_FILTER`, só `p_filter` vale).
///
/// No C a janela de uma função é a mesma de `Select.pWin`, com `pOwner` apontando de volta para a
/// expressão. Aqui a POSSE é da expressão (`ExprY::Win`); `pOwner`, `pNextWin` e `ppThis` somem.
/// A ordem da lista `Select.pWin` é `link_seq`; o código de `window.rs` que percorre
/// `pSelect->pWin` coleta as expressões de função de janela do select e as ordena por
/// `link_seq`, recebendo assim a expressão dona junto com a janela.
#[derive(Clone, Default)]
pub struct Window {
    /// Nome da janela (pode ser `None`).
    pub z_name: Option<Vec<u8>>,
    /// Nome da janela base, para encadeamento.
    pub z_base: Option<Vec<u8>>,
    /// PARTITION BY.
    pub p_partition: Option<Box<ExprList>>,
    /// ORDER BY.
    pub p_order_by: Option<Box<ExprList>>,
    /// TK_RANGE, TK_GROUPS, TK_ROWS ou 0.
    pub e_frm_type: u8,
    /// TK_UNBOUNDED, TK_CURRENT, TK_PRECEDING ou TK_FOLLOWING.
    pub e_start: u8,
    /// Idem, para o fim do quadro.
    pub e_end: u8,
    /// O quadro foi especificado implicitamente.
    pub b_implicit_frame: u8,
    /// TK_NO, TK_CURRENT, TK_TIES, TK_GROUP ou 0.
    pub e_exclude: u8,
    /// Expressão de "<expr> PRECEDING".
    pub p_start: Option<Box<Expr>>,
    /// Expressão de "<expr> FOLLOWING".
    pub p_end: Option<Box<Expr>>,
    /// A expressão FILTER.
    pub p_filter: Option<Box<Expr>>,
    /// A função.
    pub p_w_func: Option<Rc<FuncDef>>,
    /// Buffer de partição ou de pares.
    pub i_eph_csr: i32,
    /// Acumulador.
    pub reg_accum: i32,
    /// Resultado provisório.
    pub reg_result: i32,
    /// Cursor da função (min/max).
    pub csr_app: i32,
    /// Registrador da função (também min/max).
    pub reg_app: i32,
    /// Registradores com os valores do PARTITION BY.
    pub reg_part: i32,
    /// Colunas da tabela de buffer.
    pub n_buffer_col: i32,
    /// Deslocamento do primeiro argumento desta função.
    pub i_arg_col: i32,
    /// Registrador com a constante 1.
    pub reg_one: i32,
    /// Registrador do rowid inicial.
    pub reg_start_rowid: i32,
    /// Registrador do rowid final.
    pub reg_end_rowid: i32,
    /// Adia a avaliação dos argumentos (por causa de SQLITE_SUBTYPE).
    pub b_expr_args: u8,
    /// Posição na lista `Select.pWin` (1, 2, ...); 0 enquanto a janela não foi ligada.
    pub link_seq: u32,
}

// ---------------------------------------------------------------------------------------------
// Upsert
// ---------------------------------------------------------------------------------------------

/// `Upsert`: uma cláusula ON CONFLICT. As cadeias `p_next_upsert` são possuídas em ordem.
#[derive(Clone, Default)]
pub struct Upsert {
    /// Alvo do conflito (opcional).
    pub p_upsert_target: Option<Box<ExprList>>,
    /// WHERE do alvo (índices parciais).
    pub p_upsert_target_where: Option<Box<Expr>>,
    /// SET do DO UPDATE (`None` é DO NOTHING).
    pub p_upsert_set: Option<Box<ExprList>>,
    /// WHERE do DO UPDATE.
    pub p_upsert_where: Option<Box<Expr>>,
    /// Próxima cláusula ON CONFLICT.
    pub p_next_upsert: Option<Box<Upsert>>,
    /// Verdadeiro em DO UPDATE, falso em DO NOTHING.
    pub is_do_update: bool,
    /// Verdadeiro se for a 2a ou posterior com o mesmo `p_upsert_idx`.
    pub is_dup: bool,
    /// Restrição UNIQUE indicada pelo alvo.
    pub p_upsert_idx: Option<Rc<Index>>,
    /// Tabela a atualizar (cópia da lista do INSERT; o C a duplica antes de usar).
    pub p_upsert_src: Option<Box<SrcList>>,
    /// Primeiro registrador do array de VALUES.
    pub reg_data: i32,
    /// Cursor de dados.
    pub i_data_cur: i32,
    /// Primeiro cursor de índice.
    pub i_idx_cur: i32,
}

// ---------------------------------------------------------------------------------------------
// Trigger
// ---------------------------------------------------------------------------------------------

/// `TriggerStep`: um comando SQL do programa de um gatilho. Os passos formam o `Vec` de
/// `Trigger.step_list` (`pNext`/`pLast` somem); `pTrig` também some: quem tem o passo tem o
/// gatilho.
#[derive(Clone, Default)]
pub struct TriggerStep {
    /// TK_DELETE, TK_UPDATE, TK_INSERT, TK_SELECT ou TK_RETURNING.
    pub op: u8,
    /// `OE_Rollback` etc.
    pub orconf: u8,
    /// SELECT, ou o lado direito de INSERT INTO ... SELECT.
    pub p_select: Option<Box<Select>>,
    /// Tabela alvo de DELETE, UPDATE e INSERT.
    pub z_target: Option<Vec<u8>>,
    /// FROM do UPDATE.
    pub p_from: Option<Box<SrcList>>,
    /// WHERE de DELETE ou UPDATE.
    pub p_where: Option<Box<Expr>>,
    /// SET do UPDATE, ou a lista do RETURNING.
    pub p_expr_list: Option<Box<ExprList>>,
    /// Nomes de coluna do INSERT.
    pub p_id_list: Option<Box<IdList>>,
    /// Upsert do INSERT.
    pub p_upsert: Option<Box<Upsert>>,
    /// Texto original do comando.
    pub z_span: Option<Vec<u8>>,
}

/// `Trigger`: um gatilho do esquema. Fica em `Schema.trig_hash` (por nome) e em
/// `Table.p_trigger` (por tabela), nos dois casos como `Rc<Trigger>`.
#[derive(Clone, Default)]
pub struct Trigger {
    /// Nome do gatilho.
    pub z_name: Vec<u8>,
    /// Tabela ou view a que o gatilho se aplica (`table` no C).
    pub z_table: Vec<u8>,
    /// TK_DELETE, TK_UPDATE ou TK_INSERT.
    pub op: u8,
    /// `TRIGGER_BEFORE` ou `TRIGGER_AFTER`.
    pub tr_tm: u8,
    /// Este gatilho implementa um RETURNING.
    pub b_returning: u8,
    /// WHEN (pode ser `None`).
    pub p_when: Option<Box<Expr>>,
    /// Colunas de "UPDATE OF <lista>".
    pub p_columns: Option<Box<IdList>>,
    /// Esquema que contém o gatilho.
    pub p_schema: SchemaId,
    /// Esquema que contém a tabela.
    pub p_tab_schema: SchemaId,
    /// Os passos do programa, na ordem.
    pub step_list: Vec<TriggerStep>,
}

/// `TriggerPrg`: um subprograma do VDBE gerado para um gatilho. A lista de `Parse.pTriggerPrg`
/// é `Parse.trigger_prgs: Vec<TriggerPrg>`; a identidade do gatilho é `Rc::ptr_eq`.
#[derive(Clone, Default)]
pub struct TriggerPrg {
    /// Gatilho que originou o programa.
    pub p_trigger: Option<Rc<Trigger>>,
    /// Handle do `SubProgram` (índice em `Parse.sub_programs`).
    pub i_program: Option<u32>,
    /// Política ON CONFLICT padrão.
    pub orconf: i32,
    /// Máscaras de colunas old.* e new.* acessadas.
    pub a_colmask: [u32; 2],
}

/// `Returning`: o RETURNING de um INSERT, UPDATE ou DELETE. O gatilho transitório que o
/// implementa tem um único passo (o `retTStep` do C), em `ret_trig.step_list[0]`.
#[derive(Clone, Default)]
pub struct Returning {
    /// Expressões a devolver.
    pub p_return_el: Option<Box<ExprList>>,
    /// O gatilho transitório.
    pub ret_trig: Trigger,
    /// Tabela transitória com os resultados.
    pub i_ret_cur: i32,
    /// Colunas depois da expansão.
    pub n_ret_col: i32,
    /// Primeiro registrador da linha do RETURNING.
    pub i_ret_reg: i32,
    /// Nome do gatilho ("sqlite_returning_%p").
    pub z_name: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Table, Column, Index, FKey, Schema
// ---------------------------------------------------------------------------------------------

/// `Column`: uma coluna de tabela.
///
/// `z_cn_name` guarda, numa só alocação como no C, o nome, o tipo declarado e a colação, cada um
/// terminado por 0x00: `nome\0[tipo\0][colação\0]`. O tipo só existe se `COLFLAG_HASTYPE` e a
/// colação só se `COLFLAG_HASCOLL`.
#[derive(Clone, Default)]
pub struct Column {
    /// Nome (mais tipo e colação, ver acima).
    pub z_cn_name: Vec<u8>,
    /// Código OE_ do NOT NULL (4 bits).
    pub not_null: u8,
    /// Um dos `COLTYPE_*` (4 bits).
    pub e_c_type: u8,
    /// Um dos `SQLITE_AFF_*`.
    pub affinity: u8,
    /// Estimativa do tamanho do valor (sizeof(INT)==1).
    pub sz_est: u8,
    /// Hash do nome para busca rápida.
    pub h_name: u8,
    /// Índice 1-based do DEFAULT em `Table.u.tab.p_dflt_list`; 0 é nenhum.
    pub i_dflt: u16,
    /// Propriedades `COLFLAG_*`.
    pub col_flags: u16,
}

impl Column {
    /// `IsHiddenColumn`.
    #[inline]
    pub fn is_hidden(&self) -> bool {
        (self.col_flags & COLFLAG_HIDDEN) != 0
    }
}

/// `FKey.aCol[i]` (`struct sColMap`): mapeia uma coluna da tabela filha para uma da pai.
#[derive(Clone, Default)]
pub struct FKeyColMap {
    /// Índice da coluna na tabela filha.
    pub i_from: i32,
    /// Nome da coluna na tabela pai; `None` é a PRIMARY KEY.
    pub z_col: Option<Vec<u8>>,
}

/// `FKey`: uma restrição FOREIGN KEY. Imutável depois de criada. Vive em `Table.u.tab.p_f_key`
/// da tabela filha e em `Schema.fkey_hash` sob o nome da tabela pai, nos dois como `Rc<FKey>`.
///
/// `pFrom` vira `z_from` (nome da tabela filha, no mesmo `Schema` que a pai). `pNextFrom`,
/// `pNextTo` e `pPrevTo` somem (são os `Vec`). `apTrigger` (o cache dos gatilhos de ação) também
/// some: o esquema é imutável, então o cache fica no `Parse`.
#[derive(Clone, Default)]
pub struct FKey {
    /// Nome da tabela filha (a que contém o REFERENCES).
    pub z_from: Vec<u8>,
    /// Nome da tabela pai.
    pub z_to: Vec<u8>,
    /// Quantidade de colunas (`nCol`; é `a_col.len()`).
    pub n_col: i32,
    /// Verdadeiro se a checagem é adiada até o COMMIT.
    pub is_deferred: u8,
    /// Ações de ON DELETE e ON UPDATE.
    pub a_action: [u8; 2],
    /// Uma entrada por coluna.
    pub a_col: Vec<FKeyColMap>,
}

/// `Table.u.tab`: dados das tabelas comuns.
#[derive(Clone, Default)]
pub struct TabInfo {
    /// Deslocamento no CREATE TABLE onde entra uma nova coluna.
    pub add_col_offset: i32,
    /// As chaves estrangeiras desta tabela, na ordem da lista do C.
    pub p_f_key: Vec<Rc<FKey>>,
    /// DEFAULT das colunas, ou a cláusula AS das colunas geradas.
    pub p_dflt_list: Option<Box<ExprList>>,
}

/// `Table.u.view`: dados das views.
#[derive(Clone, Default)]
pub struct ViewInfo {
    /// Definição da view.
    pub p_select: Option<Box<Select>>,
}

/// `Table.u.vtab`: dados das tabelas virtuais. A lista `p` de `VTable` do C não mora aqui: o
/// estado vivo de uma tabela virtual é da conexão (`Connection.vtabs`, `VTableId`), porque o
/// `Table` é imutável e compartilhado.
#[derive(Clone, Default)]
pub struct VTabInfo {
    /// Quantidade de argumentos do módulo.
    pub n_arg: i32,
    /// 0: módulo, 1: esquema, 2: nome da vtab, 3...: argumentos.
    pub az_arg: Vec<Vec<u8>>,
}

/// `Table.u`: a união de `tab`, `view` e `vtab`, escolhida por `Table.e_tab_type`.
#[derive(Clone)]
pub enum TableU {
    /// `TABTYP_NORM`.
    Tab(TabInfo),
    /// `TABTYP_VIEW`.
    View(ViewInfo),
    /// `TABTYP_VTAB`.
    VTab(VTabInfo),
}

impl Default for TableU {
    fn default() -> Self {
        TableU::Tab(TabInfo::default())
    }
}

/// `Table`: uma tabela, tabela virtual ou view do esquema. Imutável depois de pronta; quem a
/// altera (ANALYZE, CREATE TRIGGER, ALTER) usa `Rc::make_mut`.
///
/// `n_col` convive com `a_col.len()`: o C usa `nCol = -1` como marcador de "view sendo
/// resolvida" (detecção de view circular). `nTabRef` e `costMult` não existem (a contagem é a do
/// `Rc`, e COSTMULT é opção desligada). `pSchema` vira `p_schema: SchemaId`.
///
/// A tabela em construção (CREATE TABLE em andamento, `Parse.p_new_table`) é um `Table`
/// mutável comum, chamado `TableBuilder` (alias em `connection`); só ao fechar a declaração ele é envolvido em `Rc`.
#[derive(Clone, Default)]
pub struct Table {
    /// Nome da tabela ou view.
    pub z_name: Vec<u8>,
    /// As colunas.
    pub a_col: Vec<Column>,
    /// Índices SQL da tabela, na ordem da lista do C.
    pub p_index: Vec<Rc<Index>>,
    /// Afinidade de cada coluna.
    pub z_col_aff: Option<Vec<u8>>,
    /// Restrições CHECK (numa view, os nomes das colunas).
    pub p_check: Option<Box<ExprList>>,
    /// Página raiz da árvore da tabela.
    pub tnum: u32,
    /// Máscara de `TF_*`.
    pub tab_flags: u32,
    /// Se não for negativo, `a_col[i_p_key]` é o rowid.
    pub i_p_key: i16,
    /// Quantidade de colunas (pode ser -1, ver acima).
    pub n_col: i16,
    /// Colunas que não são VIRTUAL.
    pub n_nv_col: i16,
    /// Estimativa de linhas (sqlite_stat1).
    pub n_row_log_est: LogEst,
    /// Estimativa do tamanho de cada linha em bytes.
    pub sz_tab_row: LogEst,
    /// O que fazer em conflito de unicidade em `i_p_key`.
    pub key_conf: u8,
    /// `TABTYP_NORM`, `TABTYP_VTAB` ou `TABTYP_VIEW`.
    pub e_tab_type: u8,
    /// Dados específicos do tipo.
    pub u: TableU,
    /// Gatilhos desta tabela, o mais novo primeiro (a lista `pTrigger` do C).
    pub p_trigger: Vec<Rc<Trigger>>,
    /// Esquema que contém a tabela.
    pub p_schema: SchemaId,
}

impl Table {
    /// `IsView`.
    #[inline]
    pub fn is_view(&self) -> bool {
        self.e_tab_type == TABTYP_VIEW
    }
    /// `IsOrdinaryTable`.
    #[inline]
    pub fn is_ordinary_table(&self) -> bool {
        self.e_tab_type == TABTYP_NORM
    }
    /// `IsVirtual`.
    #[inline]
    pub fn is_virtual(&self) -> bool {
        self.e_tab_type == TABTYP_VTAB
    }
    /// `HasRowid`.
    #[inline]
    pub fn has_rowid(&self) -> bool {
        (self.tab_flags & TF_WITHOUT_ROWID) == 0
    }
    /// `VisibleRowid`.
    #[inline]
    pub fn visible_rowid(&self) -> bool {
        (self.tab_flags & TF_NO_VISIBLE_ROWID) == 0
    }
    /// `u.tab`, se for tabela comum.
    #[inline]
    pub fn u_tab(&self) -> Option<&TabInfo> {
        match &self.u {
            TableU::Tab(t) => Some(t),
            _ => None,
        }
    }
    /// `u.view`, se for view.
    #[inline]
    pub fn u_view(&self) -> Option<&ViewInfo> {
        match &self.u {
            TableU::View(v) => Some(v),
            _ => None,
        }
    }
    /// `u.vtab`, se for tabela virtual.
    #[inline]
    pub fn u_vtab(&self) -> Option<&VTabInfo> {
        match &self.u {
            TableU::VTab(v) => Some(v),
            _ => None,
        }
    }
}

/// `Index`: um índice SQL. Pertence a `Table.p_index`; `Schema.idx_hash` só guarda o NOME da
/// tabela dona. `pTable`, `pNext` e `pSchema` não existem: quem tem o índice tem a tabela. STAT4
/// está desligado no Debian, então `aSample`, `nSample`, `aAvgEq`, `aiRowEst` e companhia somem.
///
/// Tamanhos: `ai_column` e `az_coll` têm `n_column` entradas; `a_sort_order` tem `n_key_col`;
/// `ai_row_log_est` tem `n_key_col + 1`. O `Index.tnum` de um índice transitório guarda o
/// endereço de uma instrução do VDBE, não uma página (ver `convertToWithoutRowidTable`).
#[derive(Clone, Default)]
pub struct Index {
    /// Nome do índice.
    pub z_name: Vec<u8>,
    /// Colunas da tabela usadas pelo índice (a primeira é 0; negativos são `XN_ROWID`/`XN_EXPR`).
    pub ai_column: Vec<i16>,
    /// Do ANALYZE: estimativa de linhas selecionadas por coluna.
    pub ai_row_log_est: Vec<LogEst>,
    /// Afinidade de cada coluna.
    pub z_col_aff: Option<Vec<u8>>,
    /// Por coluna: verdadeiro é DESC, falso é ASC.
    pub a_sort_order: Vec<u8>,
    /// Nomes das colações de cada coluna.
    pub az_coll: Vec<Vec<u8>>,
    /// WHERE dos índices parciais.
    pub p_partial_idx_where: Option<Box<Expr>>,
    /// Expressões das colunas.
    pub a_col_expr: Option<Box<ExprList>>,
    /// Página raiz do índice.
    pub tnum: u32,
    /// Tamanho médio estimado de uma linha em bytes.
    pub sz_idx_row: LogEst,
    /// Colunas que formam a chave.
    pub n_key_col: u16,
    /// Colunas guardadas no índice.
    pub n_column: u16,
    /// OE_Abort, OE_Ignore, OE_Replace ou OE_None.
    pub on_error: u8,
    /// 0 normal, 1 UNIQUE, 2 PRIMARY KEY, 3 IPK (`SQLITE_IDXTYPE_*`, 2 bits no C).
    pub idx_type: u8,
    /// Usar só para consultas de igualdade e IN.
    pub b_unordered: bool,
    /// UNIQUE e NOT NULL em todas as colunas.
    pub uniq_not_null: bool,
    /// `resizeIndexObject()` já foi chamado.
    pub is_resized: bool,
    /// É um índice de cobertura.
    pub is_covering: bool,
    /// Não tentar skip-scan.
    pub no_skip_scan: bool,
    /// `ai_row_log_est` vem do sqlite_stat1.
    pub has_stat1: bool,
    /// O sqlite_stat1 diz que o índice é de baixa qualidade.
    pub b_low_qual: bool,
    /// Não usar o índice para otimizar consultas.
    pub b_no_query: bool,
    /// Vale o bug bba7b69f9849b5bf.
    pub b_asc_key_bug: bool,
    /// O índice referencia colunas VIRTUAL.
    pub b_has_v_col: bool,
    /// O índice contém uma expressão.
    pub b_has_expr: bool,
    /// Colunas da tabela que NÃO estão no índice.
    pub col_not_idxed: Bitmask,
}

impl Index {
    /// `IsPrimaryKeyIndex`.
    #[inline]
    pub fn is_primary_key_index(&self) -> bool {
        self.idx_type == SQLITE_IDXTYPE_PRIMARYKEY
    }
    /// `IsUniqueIndex`.
    #[inline]
    pub fn is_unique_index(&self) -> bool {
        self.on_error != OE_NONE
    }
}

/// Próximo `SchemaId` (começa em 1; 0 é "nenhum").
static NEXT_SCHEMA_ID: AtomicU32 = AtomicU32::new(1);

/// `Schema`: o esquema de um banco. É possuído pelo `DbSlot` da conexão e nunca compartilhado
/// (cache compartilhado desligado), então não há contagem de referências.
///
/// `idx_hash` mapeia o nome do índice ao nome da TABELA dona, que está em `tbl_hash`; o `Index`
/// em si fica só em `Table.p_index`, para que ANALYZE altere um lugar só. `fkey_hash` mapeia o
/// nome da tabela pai à lista dos `FKey` que a referenciam (a lista `pNextTo`, cabeça primeiro).
pub struct Schema {
    /// Identidade estável deste esquema (o `Schema*` do C).
    pub id: SchemaId,
    /// Versão do esquema deste arquivo.
    pub schema_cookie: i32,
    /// Contador de geração, incrementado a cada mudança.
    pub i_generation: i32,
    /// Todas as tabelas, por nome.
    pub tbl_hash: Hash<Rc<Table>>,
    /// Todos os índices nomeados: nome do índice para nome da tabela dona.
    pub idx_hash: Hash<Vec<u8>>,
    /// Todos os gatilhos, por nome.
    pub trig_hash: Hash<Rc<Trigger>>,
    /// Chaves estrangeiras, pelo nome da tabela pai.
    pub fkey_hash: Hash<Vec<Rc<FKey>>>,
    /// A tabela sqlite_sequence do AUTOINCREMENT.
    pub p_seq_tab: Option<Rc<Table>>,
    /// Versão do formato do esquema deste arquivo.
    pub file_format: u8,
    /// Codificação de texto do banco.
    pub enc: u8,
    /// Flags do esquema (`DB_*`).
    pub schema_flags: u16,
    /// Páginas do cache.
    pub cache_size: i32,
}

impl Schema {
    /// Esquema vazio com uma identidade nova (o `sqlite3SchemaGet` que aloca).
    pub fn new() -> Schema {
        Schema {
            id: SchemaId(NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)),
            schema_cookie: 0,
            i_generation: 0,
            tbl_hash: hash_init(),
            idx_hash: hash_init(),
            trig_hash: hash_init(),
            fkey_hash: hash_init(),
            p_seq_tab: None,
            file_format: 0,
            enc: 0,
            schema_flags: 0,
            cache_size: 0,
        }
    }
    /// `DbHasProperty`: todos os bits de `p` ligados.
    #[inline]
    pub fn has_property(&self, p: u16) -> bool {
        (self.schema_flags & p) == p
    }
    /// `DbHasAnyProperty`.
    #[inline]
    pub fn has_any_property(&self, p: u16) -> bool {
        (self.schema_flags & p) != 0
    }
    /// `DbSetProperty`.
    #[inline]
    pub fn set_property(&mut self, p: u16) {
        self.schema_flags |= p;
    }
    /// `DbClearProperty`.
    #[inline]
    pub fn clear_property(&mut self, p: u16) {
        self.schema_flags &= !p;
    }
    /// `DB_SchemaLoaded`.
    #[inline]
    pub fn is_loaded(&self) -> bool {
        self.has_property(DB_SCHEMALOADED)
    }
    /// `DB_UnresetViews`.
    #[inline]
    pub fn has_unreset_views(&self) -> bool {
        self.has_property(DB_UNRESETVIEWS)
    }
    /// `DB_ResetWanted`.
    #[inline]
    pub fn reset_wanted(&self) -> bool {
        self.has_property(DB_RESETWANTED)
    }
}

impl Default for Schema {
    fn default() -> Self {
        Schema::new()
    }
}

/// `sqlite3IsNumericAffinity`.
#[inline]
pub fn is_numeric_affinity(aff: u8) -> bool {
    aff >= SQLITE_AFF_NUMERIC
}

// ---------------------------------------------------------------------------------------------
// AggInfo
// ---------------------------------------------------------------------------------------------

/// `AggInfo.aCol[i]` (`struct AggInfo_col`): uma coluna das tabelas fonte usada num agregado.
///
/// `pCExpr` apontava para um nó da árvore do select; aqui é uma CÓPIA, tirada quando a coluna é
/// registrada. As comparações de identidade do C (`pCol->pCExpr==pExpr`) viram comparação por
/// `i_table`/`i_column` ou pelo `i_agg` já gravado na expressão.
#[derive(Clone, Default)]
pub struct AggInfoCol {
    /// Tabela fonte.
    pub p_tab: Option<Rc<Table>>,
    /// A expressão original (cópia).
    pub p_c_expr: Option<Box<Expr>>,
    /// Cursor da tabela fonte.
    pub i_table: i32,
    /// Coluna na tabela fonte.
    pub i_column: i16,
    /// Coluna no índice de ordenação.
    pub i_sorter_column: i16,
}

/// `AggInfo.aFunc[i]` (`struct AggInfo_func`): uma função agregada. `pFExpr` é uma CÓPIA da
/// expressão da função (ver `AggInfoCol`).
#[derive(Clone, Default)]
pub struct AggInfoFunc {
    /// A expressão que codifica a função (cópia).
    pub p_f_expr: Option<Box<Expr>>,
    /// A implementação da função agregada.
    pub p_func: Option<Rc<FuncDef>>,
    /// Tabela efêmera que garante o DISTINCT.
    pub i_distinct: i32,
    /// Endereço do OP_OpenEphemeral.
    pub i_dist_addr: i32,
    /// Tabela efêmera que implementa o ORDER BY.
    pub i_ob_tab: i32,
    /// `i_ob_tab` tem colunas de carga separadas da chave.
    pub b_ob_payload: u8,
    /// Impor unicidade nas chaves de `i_ob_tab`.
    pub b_ob_unique: u8,
    /// Passar o subtipo pelo ordenador.
    pub b_use_subtype: u8,
}

/// `AggInfo`: o que o gerador de código precisa de um SELECT com funções agregadas.
/// `nColumn` e `nFunc` são `a_col.len()` e `a_func.len()`. `pGroupBy` apontava para o GROUP BY do
/// select; aqui é uma cópia (o select continua dono do original).
#[derive(Clone, Default)]
pub struct AggInfo {
    /// Modo direto: lê das tabelas fonte e não dos acumuladores.
    pub direct_mode: u8,
    /// No modo direto, usar o índice de ordenação e não a tabela fonte.
    pub use_sorting_idx: u8,
    /// Colunas do índice de ordenação.
    pub n_sorting_column: u16,
    /// Cursor do índice de ordenação.
    pub sorting_idx: i32,
    /// Cursor da pseudotabela.
    pub sorting_idx_p_tab: i32,
    /// Primeiro registrador do intervalo de `a_col` e `a_func`.
    pub i_first_reg: i32,
    /// A cláusula GROUP BY (cópia).
    pub p_group_by: Option<Box<ExprList>>,
    /// Colunas usadas das tabelas fonte.
    pub a_col: Vec<AggInfoCol>,
    /// Colunas que aparecem na saída; as demais só servem de parâmetro de agregadas.
    pub n_accumulator: i32,
    /// Funções agregadas.
    pub a_func: Vec<AggInfoFunc>,
    /// Select ao qual este `AggInfo` pertence.
    pub sel_id: u32,
}

impl AggInfo {
    /// `AggInfoColumnReg`.
    #[inline]
    pub fn column_reg(&self, i: i32) -> i32 {
        debug_assert!(self.i_first_reg != 0);
        self.i_first_reg + i
    }
    /// `AggInfoFuncReg`.
    #[inline]
    pub fn func_reg(&self, i: i32) -> i32 {
        debug_assert!(self.i_first_reg != 0);
        self.i_first_reg + self.a_col.len() as i32 + i
    }
}

// ---------------------------------------------------------------------------------------------
// NameContext, Walker, DbFixer
// ---------------------------------------------------------------------------------------------

/// `NameContext.uNC`: qual dos quatro membros vale é dado por `NC_UEList`, `NC_UAggInfo`,
/// `NC_UUpsert` e `NC_UBaseReg` em `nc_flags`.
pub enum NcU<'a> {
    /// Nenhum.
    None,
    /// `pEList`: colunas do resultado, para resolver aliases.
    EList(&'a ExprList),
    /// `pAggInfo`.
    AggInfo(AggInfoId),
    /// `pUpsert`.
    Upsert(&'a Upsert),
    /// `iBaseReg` (TK_REGISTER no RETURNING).
    BaseReg(i32),
}

/// `NameContext`: o contexto em que se resolvem nomes de tabelas e colunas. É uma VISÃO sem
/// posse: `p_src_list` e `u_nc` apontam para pedaços do select em resolução e `p_next` é o
/// contexto do select externo (`&mut`, porque a busca incrementa `n_ref` nos externos). O
/// `pParse` não está aqui: `resolve.rs` o passa por parâmetro. `pWinSelect` também não: quem
/// precisa de `sqlite3WindowLink`/`WindowUpdate` recebe o `&mut Select` explicitamente.
pub struct NameContext<'a> {
    /// Tabelas usadas para resolver nomes.
    pub p_src_list: Option<&'a SrcList>,
    /// Lista de resultado, `AggInfo`, upsert ou registrador-base.
    pub u_nc: NcU<'a>,
    /// Contexto externo.
    pub p_next: Option<&'a mut NameContext<'a>>,
    /// Quantos nomes este contexto resolveu.
    pub n_ref: i32,
    /// Erros encontrados.
    pub n_nc_err: i32,
    /// Flags `NC_*`.
    pub nc_flags: i32,
    /// Selects aninhados que usam este contexto.
    pub n_nested_select: u32,
}

/// `Walker`: o contexto passado ao percorrer a árvore. O `union u` do C (onze tipos de ponteiro
/// para estruturas externas) vira o parâmetro genérico `C`: cada passagem define o seu tipo de
/// contexto (por exemplo `Walker<RenameCtx>`, `Walker<DbFixer>`, `Walker<WhereConst>`) e o `Walker`
/// o possui. O `pParse` também vai dentro de `C` quando o callback precisa dele.
pub struct Walker<C> {
    /// Callback das expressões; devolve `WRC_*`.
    pub x_expr_callback: Option<fn(&mut Walker<C>, &mut Expr) -> i32>,
    /// Callback dos SELECTs; devolve `WRC_*`.
    pub x_select_callback: Option<fn(&mut Walker<C>, &mut Select) -> i32>,
    /// Segundo callback dos SELECTs (depois dos filhos).
    pub x_select_callback2: Option<fn(&mut Walker<C>, &mut Select)>,
    /// Quantidade de subconsultas.
    pub walker_depth: i32,
    /// Um código de processamento pequeno.
    pub e_code: u16,
    /// Flags dependentes do uso.
    pub m_w_flags: u16,
    /// Dados extras para os callbacks (`u`).
    pub u: C,
}

impl<C: Default> Default for Walker<C> {
    fn default() -> Self {
        Walker {
            x_expr_callback: None,
            x_select_callback: None,
            x_select_callback2: None,
            walker_depth: 0,
            e_code: 0,
            m_w_flags: 0,
            u: C::default(),
        }
    }
}

/// `DbFixer`: o contexto de `sqlite3FixSelect` & cia. O `Walker w` e o `pParse` do C saem
/// daqui: o percurso usa `Walker<DbFixer>`, que possui este registro como `u`.
#[derive(Clone, Default)]
pub struct DbFixer {
    /// Esquema ao qual os itens serão fixados.
    pub p_schema: SchemaId,
    /// Verdadeiro para entradas do esquema TEMP.
    pub b_temp: bool,
    /// Nome do banco em que todos os objetos devem estar.
    pub z_db: Vec<u8>,
    /// Tipo do contêiner, para mensagens de erro.
    pub z_type: &'static str,
    /// Nome do contêiner, para mensagens de erro.
    pub p_name: Token,
}

// ---------------------------------------------------------------------------------------------
// ALTER ... RENAME
// ---------------------------------------------------------------------------------------------

/// `RenameToken` (alter.c): liga um elemento da árvore ao token que o criou. O C usa o ENDEREÇO
/// do elemento como chave. Aqui `p` é o endereço do elemento (`as_ptr() as usize` do buffer de
/// texto, ou `&*box as *const _ as usize` do nó `Box`), uma comparação de inteiros inteiramente
/// segura; só vale com nós em `Box` ou buffers de texto não vazios, que não mudam de endereço
/// quando o dono se move. A lista `pNext` é o `Vec` de `Parse.p_rename` e de `RenameCtx.p_list`.
#[derive(Clone, Debug, Default)]
pub struct RenameToken {
    /// Endereço do elemento da árvore criado pelo token `t`.
    pub p: usize,
    /// O token que criou o elemento.
    pub t: Token,
}

/// `RenameCtx` (alter.c): contexto de um ALTER TABLE RENAME COLUMN.
#[derive(Clone, Default)]
pub struct RenameCtx {
    /// Tokens a sobrescrever.
    pub p_list: Vec<RenameToken>,
    /// Índice da coluna renomeada.
    pub i_col: i32,
    /// Tabela sendo alterada.
    pub p_tab: Option<Rc<Table>>,
    /// Nome antigo da coluna.
    pub z_old: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Registros pequenos de geração de código
// ---------------------------------------------------------------------------------------------

/// `AutoincInfo`: dados do AUTOINCREMENT de uma tabela durante a geração de código. A lista
/// `pNext` é `Parse.p_ainc: Vec<AutoincInfo>`.
#[derive(Clone, Default)]
pub struct AutoincInfo {
    /// A tabela a que o bloco se refere.
    pub p_tab: Option<Rc<Table>>,
    /// Índice em `Connection.dbs` do banco que guarda a tabela.
    pub i_db: i32,
    /// Registrador com o contador de rowid.
    pub reg_ctr: i32,
}

/// `IndexedExpr`: expressão de um índice que pode ser lida do índice em vez de recalculada. A
/// lista `pIENext` é `Parse.p_idx_expr: Vec<IndexedExpr>`. `pExpr` é uma cópia da expressão do
/// índice.
#[derive(Clone, Default)]
pub struct IndexedExpr {
    /// A expressão contida no índice (cópia).
    pub p_expr: Option<Box<Expr>>,
    /// Cursor de dados associado ao índice.
    pub i_data_cur: i32,
    /// Cursor do índice.
    pub i_idx_cur: i32,
    /// Coluna do índice que contém o valor de `p_expr`.
    pub i_idx_col: i32,
    /// É preciso um OP_IfNullRow.
    pub b_maybe_null_row: u8,
    /// Afinidade da expressão.
    pub aff: u8,
}

/// `TableLock` (build.c): trava de tabela pedida em tempo de execução (cache compartilhado).
#[derive(Clone, Default)]
pub struct TableLock {
    /// Banco que contém a tabela.
    pub i_db: i32,
    /// Página raiz da tabela.
    pub i_tab: u32,
    /// Verdadeiro para trava de escrita.
    pub is_write_lock: bool,
    /// Nome da tabela.
    pub z_lock_name: Vec<u8>,
}

// ---------------------------------------------------------------------------------------------
// Savepoint, Module, VTable
// ---------------------------------------------------------------------------------------------

/// `Savepoint`: um savepoint aberto. A lista `pNext` (o mais recente primeiro) é
/// `Connection.savepoints: Vec<Savepoint>` com o mais recente por ÚLTIMO.
#[derive(Clone, Debug, Default)]
pub struct Savepoint {
    /// Nome do savepoint.
    pub z_name: Vec<u8>,
    /// Violações de FK adiadas.
    pub n_deferred_cons: i64,
    /// Violações imediatas de FK adiadas.
    pub n_deferred_imm_cons: i64,
}

/// `Module`: um módulo de tabela virtual (`sqlite3_create_module`), em `Connection.modules`.
/// `p_module` e `p_aux` são `dyn Any` até o módulo `vtab` fixar o trait das callbacks
/// (`sqlite3_module`); `xDestroy` é o `Drop`; `nRefModule` é a contagem do `Rc`.
#[derive(Clone, Default)]
pub struct Module {
    /// Nome passado a `create_module`.
    pub z_name: Vec<u8>,
    /// As callbacks do módulo.
    pub p_module: Option<Rc<dyn Any>>,
    /// `pAux` passado a `create_module`.
    pub p_aux: Option<Rc<dyn Any>>,
    /// A tabela eponímia do módulo.
    pub p_epo_tab: Option<Rc<Table>>,
}

/// `VTable`: a instância viva de uma tabela virtual numa conexão, em `Connection.vtabs`
/// (`VTableId`). A tabela é achada por `(schema, z_tab_name)` e o módulo por `z_mod_name`
/// (`pMod` e `db` do C), porque o `Table` é imutável e compartilhado e não pode apontar para
/// estado vivo. `nRef` e `pNext` somem: a lista de VTables por conexão é o próprio slab.
#[derive(Default)]
pub struct VTable {
    /// Esquema que contém a tabela virtual.
    pub schema: SchemaId,
    /// Nome da tabela virtual.
    pub z_tab_name: Vec<u8>,
    /// Nome do módulo (`Table.u.vtab.az_arg[0]`).
    pub z_mod_name: Vec<u8>,
    /// A instância `sqlite3_vtab` criada pelo módulo.
    pub p_vtab: Option<Box<dyn Any>>,
    /// Verdadeiro se há suporte a restrições.
    pub b_constraint: u8,
    /// Pode usar qualquer esquema anexado.
    pub b_all_schemas: u8,
    /// Risco de deixar um atacante acessar (`SQLITE_VTABRISK_*`).
    pub e_vtab_risk: u8,
    /// Profundidade da pilha de SAVEPOINT.
    pub i_savepoint: i32,
}
