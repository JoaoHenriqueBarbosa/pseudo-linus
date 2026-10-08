//! Tradução de `whereInt.h` (chunks `whereInt_h.000` e `whereInt_h.001`): as estruturas do
//! planejador de consultas (`WhereClause`, `WhereTerm`, `WhereInfo`, `WhereLoop`, `WhereLevel`,
//! `WhereMaskSet`, ...). As constantes (`WO_*`, `TERM_*`, `WHERE_*`, `SQLITE_BLDF*`) já estão em
//! `crate::consts` (`consts/where_int.rs`).
//!
//! Este módulo é o contrato de `whereexpr.rs`, `where.rs` e `wherecode.rs`. As decisões de
//! modelo (CONVENTIONS.md, itens 1, 3 e 6) são estas:
//!
//! # Arenas por `WhereInfo` e handles
//!
//! O C liga tudo por ponteiro e com ciclos (`WhereTerm.pWC`, `WhereClause.pWInfo`,
//! `WhereClause.pOuter`, `WhereLoop.aLTerm[]` apontando para termos, `WhereLevel.pWLoop`,
//! `WhereOrInfo.wc` contendo outro `WhereClause`). Tudo isso vira arena dentro do `WhereInfo`,
//! com handles `u32`:
//!
//! - `WhereInfo.clauses: Vec<WhereClause>` e `ClauseId`. `sWC` é o `ClauseId` guardado em
//!   `WhereInfo.s_wc` (quem monta o `WhereInfo` chama `whereexpr::where_clause_init`, que
//!   acrescenta a cláusula na arena e devolve o id). `WhereOrInfo.wc` e `WhereAndInfo.wc` são
//!   `ClauseId` de cláusulas na MESMA arena. O `WhereClause` temporário que `whereLoopAddOr`
//!   monta com `tempWC.a = pOrTerm` é uma cláusula nova com `a = vec![termo]` e `p_outer`.
//! - `WhereInfo.terms: Vec<WhereTerm>` e `TermId`. Uma cláusula guarda `a: Vec<TermId>`, na ordem
//!   do `a[]` do C; `pWC->a[i]` é `wi.terms[wi.clauses[wc].a[i]]`. `WhereTerm.i_parent` continua
//!   sendo POSIÇÃO em `a[]` da cláusula do termo (`p_wc`), como o `iParent` do C. Como um termo
//!   nunca muda de lugar na arena, o aviso do C "ponteiros para WhereTerm ficam inválidos depois
//!   de whereClauseInsert" não existe mais: um `TermId` vale para sempre.
//! - `WhereInfo.loops: Vec<WhereLoop>` e `LoopId`. A lista encadeada `pLoops`/`pNextLoop` é
//!   `WhereInfo.p_loops: Vec<LoopId>` NA ORDEM da lista do C (a ordem importa para o planejador);
//!   `pNextLoop` não existe. Remover uma `WhereLoop` da lista é tirar o id de `p_loops`; a vaga
//!   na arena fica (a arena morre com o `WhereInfo`, como o `pMemToFree` do C).
//!   `WherePath.aLoop`, `WhereLevel.pWLoop` e `WhereLoopBuilder.pNew` (este por valor) idem.
//! - `WhereMemBlock` e `sqlite3WhereMalloc`/`sqlite3WhereRealloc` não existem: a arena é o
//!   `Vec`. `nSlot`, `aStatic`, `nLSlot`, `aLTermSpace` e `WHERE_LOOP_XFER_SZ` também somem:
//!   `nTerm` é `a.len()` e `nLSlot` é `a_l_term.len()`.
//! - Ponteiros NÃO possuídos que o `WhereInfo` guardava para fora (`pParse`, `pTabList`,
//!   `pSelect`) viram parâmetros explícitos das funções de `where.rs` e `wherecode.rs`
//!   (`db: &mut Connection, parse: &mut Parse, p_tab_list: &mut SrcList, ...`). `pOrderBy` e
//!   `pResultSet` ficam como CÓPIAS possuídas (`expr_list_dup` feita em `sqlite3WhereBegin`);
//!   o planejador só as lê. `pWhere` (só usado em rastreio, `WHERETRACE_ENABLED`) some.
//! - `WhereClause.pOuter` é `p_outer: Option<ClauseId>`; `WhereClause.pWInfo` some (o `WhereInfo`
//!   é quem contém a arena e é passado em `wi: &mut WhereInfo`).
//!
//! # Posse das expressões: `WhereTerm.p_expr` é um `ExprRef`
//!
//! No C, `WhereTerm.pExpr` é um ponteiro que ora aponta para dentro da árvore original do WHERE
//! (termo não dinâmico, o `TERM_DYNAMIC` ausente), ora para uma expressão criada pelo
//! otimizador e possuída pela cláusula (`TERM_DYNAMIC`), ora para a MESMA expressão de outro
//! termo (as fatias de `(a,b) IN (SELECT ...)`). O planejador depende de três coisas desse
//! aliasing: (1) mutações no lugar (`exprCommute` troca os operandos do `Expr` original; o
//! `x IS NULL` sobre coluna NOT NULL vira `TK_TRUEFALSE` no original) que depois são vistas
//! quando o termo OR inteiro é codificado como expressão simples, e portanto aparecem no
//! EXPLAIN; (2) identidade por ponteiro (`pLoop->aLTerm[i]->pExpr==pX` em `wherecode.c` para
//! achar as fatias do mesmo IN); (3) os subtermos de um OR/AND serem subárvores do termo pai.
//!
//! Uma cópia por termo perderia (1) e (2), e `Rc<Expr>` não permite mutar. Por isso o
//! `WhereInfo` POSSUI as árvores num `WhereExprs` (`WhereInfo.exprs`: `Vec<Option<Box<Expr>>>`
//! de RAÍZES) e o termo guarda um `ExprRef { root, path }`: a raiz mais o caminho de passos
//! (`ExprStep::Left`, `Right`, `List(i)` = `x.pList->a[i].pExpr`) até o nó. Duas referências
//! designam o mesmo nó se e somente se são iguais (`==`): é o `pExpr==pX` do C. Mutar pelo
//! `ExprRef` (`WhereExprs::get_mut`) muta a árvore pai, exatamente como o C.
//!
//! - A árvore original do WHERE entra como raiz com `WhereExprs::add_root` (a decisão entre
//!   MOVER o `pWhere` do chamador para dentro, ou duplicá-lo com `expr_dup`, é de `where.rs`;
//!   `whereexpr.rs` só recebe o `ExprRef`). Os subtermos de `sqlite3WhereSplit` são
//!   `ExprRef` com caminho mais longo na mesma raiz.
//! - Um termo `TERM_DYNAMIC` tem uma raiz própria (`add_root` com o `Box<Expr>` novo); limpar a
//!   cláusula (`where_clause_clear`) remove a raiz, que é o `sqlite3ExprDelete` do C.
//! - Quem precisa do `&Expr` de um termo usa `wi.exprs.get(&wi.terms[t].p_expr)` (ou
//!   `WhereInfo::expr`); `&mut Expr` para `expr_code_target`/`expr_if_false` idem
//!   (`WhereInfo::expr_mut`). Para ler uma `Expr` enquanto altera OUTRO campo do `wi`, acesse
//!   os campos direto (`wi.exprs`, `wi.terms`, `wi.s_mask_set`): são disjuntos para o borrow
//!   checker; chamar método de `wi` empresta o `wi` inteiro.
//! - O sub-`WhereInfo` do `WHERE_MULTI_OR` (`sqlite3WhereBegin(pParse, pOrTab, pOrExpr, ...)`)
//!   recebe uma CÓPIA (`expr_dup`) da expressão do ramo OR: tem arena própria.
//!
//! # `u` dos termos e dos níveis
//!
//! As uniões do C viram campos separados (o C só lê o membro que o discriminante autoriza, e
//! `whereClauseInsert` zera a união inteira, o que `WhereTerm::reset_from_e_operator` repete):
//! `WhereTerm.u.x.{leftColumn,iField}` são `left_column`/`i_field`; `u.pOrInfo` e `u.pAndInfo`
//! são `p_or_info`/`p_and_info`. `WhereLoop.u.btree` e `u.vtab` são `btree`/`vtab`.
//! `WhereLevel.u.in.{nIn,aInLoop}` é `a_in_loop: Vec<InLoop>` (`nIn` é `len`) e
//! `u.pCoveringIdx` é `p_covering_idx`.
//!
//! # Fora do que o Debian compila
//!
//! `SQLITE_ENABLE_STAT4` (`pRec`, `nRecValid`, `TERM_HIGHTRUTH` vale 0), `SQLITE_DEBUG`
//! (`WhereLoop.cId`), `SQLITE_ENABLE_STMT_SCANSTATUS` (`WhereLevel.addrVisit`) e
//! `WHERETRACE_ENABLED` somem.
//!
//! `sqlite3WhereGetMask` é declarado em `whereInt.h` e implementado em `where.c`; aqui ele é
//! `WhereMaskSet::get_mask`, para `whereexpr.rs` não depender de `where.rs`. `where.rs` NÃO o
//! redefine. A inicialização `initMaskSet` é `WhereMaskSet::default()`.

use std::rc::Rc;

use crate::consts::{Bitmask, BMS};
use crate::sqlite_int::{Expr, ExprList, Index, LogEst};

// ---------------------------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------------------------

/// Handle de um `WhereTerm` em `WhereInfo.terms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TermId(pub u32);

/// Handle de um `WhereClause` em `WhereInfo.clauses`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ClauseId(pub u32);

/// Handle de um `WhereLoop` em `WhereInfo.loops`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct LoopId(pub u32);

// ---------------------------------------------------------------------------------------------
// Expressões do planejador (ver o cabeçalho do módulo)
// ---------------------------------------------------------------------------------------------

/// Um passo do caminho de uma raiz de `WhereExprs` até um nó.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprStep {
    /// `pLeft`.
    Left,
    /// `pRight`.
    Right,
    /// `x.pList->a[i].pExpr`.
    List(u32),
}

/// Referência a um nó de expressão de um `WhereInfo`: o `Expr*` do C. Igualdade de `ExprRef`
/// é igualdade de ponteiro.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ExprRef {
    /// Índice da raiz em `WhereExprs`.
    pub root: u32,
    /// Passos da raiz até o nó.
    pub path: Vec<ExprStep>,
}

impl ExprRef {
    /// A referência ao filho `step` deste nó.
    pub fn child(&self, step: ExprStep) -> ExprRef {
        let mut path = Vec::with_capacity(self.path.len() + 1);
        path.extend_from_slice(&self.path);
        path.push(step);
        ExprRef { root: self.root, path }
    }
}

/// O filho `step` de `e`.
pub fn expr_child(e: &Expr, step: ExprStep) -> Option<&Expr> {
    match step {
        ExprStep::Left => e.p_left.as_deref(),
        ExprStep::Right => e.p_right.as_deref(),
        ExprStep::List(i) => e.x_list()?.a.get(i as usize)?.p_expr.as_deref(),
    }
}

/// O filho `step` de `e`, para alterar.
pub fn expr_child_mut(e: &mut Expr, step: ExprStep) -> Option<&mut Expr> {
    match step {
        ExprStep::Left => e.p_left.as_deref_mut(),
        ExprStep::Right => e.p_right.as_deref_mut(),
        ExprStep::List(i) => e.x_list_mut()?.a.get_mut(i as usize)?.p_expr.as_deref_mut(),
    }
}

/// As raízes das árvores de expressão que o planejador usa: a do WHERE e as que o otimizador
/// cria (`TERM_DYNAMIC`).
#[derive(Default)]
pub struct WhereExprs {
    roots: Vec<Option<Box<Expr>>>,
}

impl WhereExprs {
    /// Passa a posse de `e` para a arena e devolve a referência à raiz.
    pub fn add_root(&mut self, e: Box<Expr>) -> ExprRef {
        self.roots.push(Some(e));
        ExprRef { root: (self.roots.len() - 1) as u32, path: Vec::new() }
    }

    /// O nó designado por `r`.
    pub fn get(&self, r: &ExprRef) -> Option<&Expr> {
        let mut cur: &Expr = self.roots.get(r.root as usize)?.as_deref()?;
        for s in &r.path {
            cur = expr_child(cur, *s)?;
        }
        Some(cur)
    }

    /// O nó designado por `r`, para alterar.
    pub fn get_mut(&mut self, r: &ExprRef) -> Option<&mut Expr> {
        let mut cur: &mut Expr = self.roots.get_mut(r.root as usize)?.as_deref_mut()?;
        for s in &r.path {
            cur = expr_child_mut(cur, *s)?;
        }
        Some(cur)
    }

    /// Tira da arena a raiz de `r` (o caminho é ignorado) e a devolve: o `sqlite3ExprDelete` de
    /// um termo dinâmico é deixar o `Box` cair.
    pub fn remove_root(&mut self, r: &ExprRef) -> Option<Box<Expr>> {
        self.roots.get_mut(r.root as usize)?.take()
    }
}

// ---------------------------------------------------------------------------------------------
// WhereClause, WhereTerm
// ---------------------------------------------------------------------------------------------

/// `WhereTerm`: um termo da cláusula WHERE. Ver o cabeçalho do módulo para `p_expr`, `p_wc` e
/// as uniões.
#[derive(Clone, Default)]
pub struct WhereTerm {
    /// A subexpressão que é este termo.
    pub p_expr: ExprRef,
    /// A cláusula a que o termo pertence (`pWC`).
    pub p_wc: ClauseId,
    /// Probabilidade de verdade desta expressão.
    pub truth_prob: LogEst,
    /// Bits `TERM_*`.
    pub wt_flags: u16,
    /// Um valor `WO_*` que descreve `<op>`.
    pub e_operator: u16,
    /// Número de filhos que precisam me desabilitar.
    pub n_child: u8,
    /// Operador para termos MATCH/LIKE/GLOB/REGEXP de tabela virtual.
    pub e_match_op: u8,
    /// Desabilita `pWC->a[i_parent]` quando este termo é desabilitado (-1 é nenhum).
    pub i_parent: i32,
    /// Cursor de X em "X <op> <expr>".
    pub left_cursor: i32,
    /// `u.x.leftColumn`: coluna de X em "X <op> <expr>".
    pub left_column: i32,
    /// `u.x.iField`: campo em `(?,?,?) IN (SELECT...)`.
    pub i_field: i32,
    /// `u.pOrInfo`: informação extra se `e_operator & WO_OR`.
    pub p_or_info: Option<WhereOrInfo>,
    /// `u.pAndInfo`: informação extra se `e_operator & WO_AND`.
    pub p_and_info: Option<WhereAndInfo>,
    /// Máscara das tabelas usadas por `pExpr->pRight`.
    pub prereq_right: Bitmask,
    /// Máscara das tabelas referenciadas por `pExpr`.
    pub prereq_all: Bitmask,
}

impl WhereTerm {
    /// O `memset(&pTerm->eOperator, 0, sizeof(WhereTerm) - offsetof(WhereTerm,eOperator))` de
    /// `whereClauseInsert`: zera tudo de `e_operator` em diante, inclusive as uniões.
    pub fn reset_from_e_operator(&mut self) {
        self.e_operator = 0;
        self.n_child = 0;
        self.e_match_op = 0;
        self.i_parent = 0;
        self.left_cursor = 0;
        self.left_column = 0;
        self.i_field = 0;
        self.p_or_info = None;
        self.p_and_info = None;
        self.prereq_right = 0;
        self.prereq_all = 0;
    }
}

/// `WhereClause`: todas as informações sobre uma cláusula WHERE (um contêiner de termos).
/// `nTerm` é `a.len()`.
#[derive(Clone, Default)]
pub struct WhereClause {
    /// Conjunção externa (`pOuter`).
    pub p_outer: Option<ClauseId>,
    /// Operador de divisão: `TK_AND` ou `TK_OR`.
    pub op: u8,
    /// Verdadeiro se algum `a[].eOperator` é `WO_OR`.
    pub has_or: u8,
    /// Número de termos até o último não virtual.
    pub n_base: i32,
    /// Os termos (`a[]`).
    pub a: Vec<TermId>,
}

/// `WhereOrInfo`: informação de um termo `WO_OR`.
#[derive(Clone, Default)]
pub struct WhereOrInfo {
    /// Decomposição em subtermos (cláusula na arena do `WhereInfo`).
    pub wc: ClauseId,
    /// Máscara de todas as tabelas indexáveis da cláusula.
    pub indexable: Bitmask,
}

/// `WhereAndInfo`: informação de um termo `WO_AND`.
#[derive(Clone, Default)]
pub struct WhereAndInfo {
    /// A subexpressão decomposta (cláusula na arena do `WhereInfo`).
    pub wc: ClauseId,
}

// ---------------------------------------------------------------------------------------------
// WhereMaskSet
// ---------------------------------------------------------------------------------------------

/// `WhereMaskSet`: o mapeamento entre números de cursor do VDBE e bits das máscaras.
#[derive(Clone)]
pub struct WhereMaskSet {
    /// Usado por `sqlite3WhereExprUsage()`.
    pub b_var_select: i32,
    /// Número de valores de cursor atribuídos.
    pub n: i32,
    /// Cursor atribuído a cada bit.
    pub ix: [i32; BMS as usize],
}

impl Default for WhereMaskSet {
    /// `initMaskSet`: `n = 0` e `ix[0] = -99`.
    fn default() -> Self {
        let mut ix = [0i32; BMS as usize];
        ix[0] = -99;
        WhereMaskSet { b_var_select: 0, n: 0, ix }
    }
}

impl WhereMaskSet {
    /// `sqlite3WhereGetMask`: a máscara do cursor, ou 0 se ele não está no conjunto.
    pub fn get_mask(&self, i_cursor: i32) -> Bitmask {
        debug_assert!(self.n <= BMS);
        debug_assert!(self.n > 0 || self.ix[0] < 0);
        debug_assert!(i_cursor >= -1);
        if self.ix[0] == i_cursor {
            return 1;
        }
        for i in 1..self.n {
            if self.ix[i as usize] == i_cursor {
                return 1u64 << i;
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------------------------
// WhereLoop, WherePath, WhereOrSet
// ---------------------------------------------------------------------------------------------

/// `WhereLoop.u.btree`: informação das tabelas e índices b-tree.
#[derive(Clone, Default)]
pub struct WhereLoopBtree {
    /// Número de restrições de igualdade.
    pub n_eq: u16,
    /// Tamanho do vetor BTM.
    pub n_btm: u16,
    /// Tamanho do vetor TOP.
    pub n_top: u16,
    /// Colunas do índice usadas para ordenar o DISTINCT.
    pub n_distinct_col: u16,
    /// O índice usado, ou `None`.
    pub p_index: Option<Rc<Index>>,
}

/// `WhereLoop.u.vtab`: informação das tabelas virtuais.
#[derive(Clone, Default)]
pub struct WhereLoopVtab {
    /// Número do índice.
    pub idx_num: i32,
    /// Verdadeiro se `sqlite3_free(idxStr)` era preciso (sem sentido aqui: `idx_str` é posse).
    pub need_free: bool,
    /// Verdadeiro para deixar a tabela virtual tratar o OFFSET.
    pub b_omit_offset: bool,
    /// Verdadeiro se satisfaz o ORDER BY.
    pub is_ordered: i8,
    /// Termos que podem ser omitidos.
    pub omit_mask: u16,
    /// Identificador do índice.
    pub idx_str: Option<Vec<u8>>,
    /// Termos a tratar como IN(...) em vez de ==.
    pub m_handle_in: u32,
}

/// `WhereLoop`: um algoritmo para avaliar um termo de uma junção.
///
/// `whereLoopXfer` copia os campos até `n_skip` inclusive, e depois os `n_l_term` primeiros de
/// `a_l_term` (o `WHERE_LOOP_XFER_SZ` do C).
#[derive(Clone, Default)]
pub struct WhereLoop {
    /// Máscara dos outros loops que precisam rodar antes.
    pub prereq: Bitmask,
    /// Máscara que identifica a tabela `i_tab`.
    pub mask_self: Bitmask,
    /// Posição na cláusula FROM da tabela deste loop.
    pub i_tab: u8,
    /// Número do índice de ordenação (0 é nenhum).
    pub i_sort_idx: u8,
    /// Custo de preparação único (por exemplo criar um índice transitório).
    pub r_setup: LogEst,
    /// Custo de rodar cada volta do loop.
    pub r_run: LogEst,
    /// Número estimado de linhas de saída.
    pub n_out: LogEst,
    /// `u.btree`.
    pub btree: WhereLoopBtree,
    /// `u.vtab`.
    pub vtab: WhereLoopVtab,
    /// Bits `WHERE_*` que descrevem o plano.
    pub ws_flags: u32,
    /// Número de entradas usadas em `a_l_term`.
    pub n_l_term: u16,
    /// Número de entradas `None` em `a_l_term`.
    pub n_skip: u16,
    /// Termos usados (`aLTerm`); `a_l_term.len()` é o `nLSlot` do C.
    pub a_l_term: Vec<Option<TermId>>,
}

/// `WhereOrCost`: os pré-requisitos e o custo de rodar a subconsulta de um operando do OR.
#[derive(Clone, Copy, Default)]
pub struct WhereOrCost {
    /// Pré-requisitos.
    pub prereq: Bitmask,
    /// Custo de rodar esta subconsulta.
    pub r_run: LogEst,
    /// Número de saídas desta subconsulta.
    pub n_out: LogEst,
}

/// `N_OR_COST`, reexportado do `consts` para o tamanho do vetor.
pub use crate::consts::N_OR_COST;

/// `WhereOrSet`: os melhores `N_OR_COST` custos possíveis do processamento de um OR.
#[derive(Clone, Copy, Default)]
pub struct WhereOrSet {
    /// Número de entradas válidas de `a`.
    pub n: u16,
    /// Conjunto dos melhores custos.
    pub a: [WhereOrCost; N_OR_COST],
}

/// `WherePath`: uma sequência de `WhereLoop` que implementa parte ou todo um plano.
#[derive(Clone, Default)]
pub struct WherePath {
    /// Máscara de todos os loops do caminho.
    pub mask_loop: Bitmask,
    /// Loops de `a_loop` que devem ser invertidos para o ORDER BY.
    pub rev_loop: Bitmask,
    /// Número estimado de linhas geradas pelo caminho.
    pub n_row: LogEst,
    /// Custo total do caminho.
    pub r_cost: LogEst,
    /// Custo total ignorando o custo de ordenação.
    pub r_unsorted: LogEst,
    /// Número de termos do ORDER BY satisfeitos; -1 é desconhecido.
    pub is_ordered: i8,
    /// Os loops do caminho.
    pub a_loop: Vec<LoopId>,
}

// ---------------------------------------------------------------------------------------------
// WhereLevel
// ---------------------------------------------------------------------------------------------

/// `WhereRightJoin`: informação extra de um `WhereLevel` que é RIGHT JOIN.
#[derive(Clone, Copy, Default)]
pub struct WhereRightJoin {
    /// Cursor usado para saber as linhas já casadas.
    pub i_match: i32,
    /// Filtro de Bloom de `iRJMatch`.
    pub reg_bloom: i32,
    /// Registrador de retorno da sub-rotina interna.
    pub reg_return: i32,
    /// Endereço inicial da sub-rotina interna.
    pub addr_subrtn: i32,
    /// Último opcode da sub-rotina interna.
    pub end_subrtn: i32,
}

/// `WhereLevel.u.in.aInLoop[i]` (`struct InLoop`).
#[derive(Clone, Copy, Default)]
pub struct InLoop {
    /// Cursor do VDBE usado por este operador IN.
    pub i_cur: i32,
    /// Topo do laço do IN.
    pub addr_in_top: i32,
    /// Registrador base do registro de chave multi-coluna.
    pub i_base: i32,
    /// Número de entradas anteriores da chave.
    pub n_prefix: i32,
    /// Terminador do laço do IN: `OP_Next` ou `OP_Prev`.
    pub e_end_loop_op: u8,
}

/// `WhereLevel`: o que é preciso para implementar um laço aninhado da cláusula WHERE. Descreve a
/// implementação, ao contrário do `WhereLoop`, que descreve o algoritmo.
#[derive(Clone, Default)]
pub struct WhereLevel {
    /// Célula de memória que implementa o LEFT OUTER JOIN.
    pub i_left_join: i32,
    /// Cursor do VDBE que acessa a tabela.
    pub i_tab_cur: i32,
    /// Cursor do VDBE que acessa `pIdx`.
    pub i_idx_cur: i32,
    /// Salte aqui para sair do laço.
    pub addr_brk: i32,
    /// Salte aqui para começar a próxima combinação de IN.
    pub addr_nxt: i32,
    /// Salte aqui para a próxima iteração do skip-scan.
    pub addr_skip: i32,
    /// Salte aqui para continuar com o próximo ciclo do laço.
    pub addr_cont: i32,
    /// Primeira instrução do interior do laço.
    pub addr_first: i32,
    /// Começo do corpo do laço.
    pub addr_body: i32,
    /// Registrador do flag de big-null (verdadeiro se a varredura de NULL é necessária).
    pub reg_bignull: i32,
    /// Salte aqui para a próxima parte da varredura big-null.
    pub addr_bignull: i32,
    /// Contador do processamento do intervalo LIKE (registrador, vezes 2).
    pub i_like_rep_cntr: u32,
    /// Endereço do processamento do intervalo LIKE.
    pub addr_like_rep: i32,
    /// Filtro de Bloom.
    pub reg_filter: i32,
    /// Informação extra do RIGHT JOIN.
    pub p_rj: Option<Box<WhereRightJoin>>,
    /// Qual entrada da cláusula FROM.
    pub i_from: u8,
    /// Opcode que termina o laço.
    pub op: u8,
    /// P3 do opcode que termina o laço.
    pub p3: u8,
    /// P5 do opcode que termina o laço.
    pub p5: u8,
    /// P1 do opcode que termina o laço.
    pub p1: i32,
    /// P2 do opcode que termina o laço.
    pub p2: i32,
    /// `u.in.aInLoop` (vale com `WHERE_IN_ABLE`); `nIn` é `len`.
    pub a_in_loop: Vec<InLoop>,
    /// `u.pCoveringIdx`: possível índice de cobertura de `WHERE_MULTI_OR`.
    pub p_covering_idx: Option<Rc<Index>>,
    /// O `WhereLoop` escolhido.
    pub p_w_loop: LoopId,
    /// Entradas do FROM não utilizáveis neste nível.
    pub not_ready: Bitmask,
}

// ---------------------------------------------------------------------------------------------
// WhereScan, WhereLoopBuilder
// ---------------------------------------------------------------------------------------------

/// `WhereScan`: o iterador que localiza termos úteis ao planejador.
#[derive(Clone, Default)]
pub struct WhereScan {
    /// Cláusula original, a mais interna.
    pub p_orig_wc: ClauseId,
    /// Cláusula sendo varrida agora.
    pub p_wc: ClauseId,
    /// Sequência de colação exigida, se houver.
    pub z_coll_name: Option<Vec<u8>>,
    /// Procure esta expressão de índice (cópia da de `Index.a_col_expr`).
    pub p_idx_expr: Option<Box<Expr>>,
    /// Retome a varredura em `pWC->a[k]`.
    pub k: i32,
    /// Operadores aceitáveis.
    pub op_mask: u32,
    /// Deve casar esta afinidade, se `z_coll_name` não é nulo.
    pub idxaff: u8,
    /// Posição atual em `ai_cur` e `ai_column`.
    pub i_equiv: u8,
    /// Número de entradas de `ai_cur` e `ai_column`.
    pub n_equiv: u8,
    /// Cursores da classe de equivalência.
    pub ai_cur: [i32; 11],
    /// Coluna correspondente de cada cursor da classe de equivalência.
    pub ai_column: [i16; 11],
}

/// `WhereLoopBuilder`: tudo o que é preciso para construir os `WhereLoop` de uma consulta.
/// `pWInfo` é o `wi: &mut WhereInfo` que as funções recebem em separado.
#[derive(Clone, Default)]
pub struct WhereLoopBuilder {
    /// Termos da cláusula WHERE.
    pub p_wc: ClauseId,
    /// O `WhereLoop` modelo (possuído pelo construtor).
    pub p_new: Box<WhereLoop>,
    /// Registra aqui os melhores loops, se presente.
    pub p_or_set: Option<WhereOrSet>,
    /// Primeiro conjunto de flags `SQLITE_BLDF1_*`.
    pub bld_flags1: u8,
    /// Segundo conjunto de flags `SQLITE_BLDF2_*`.
    pub bld_flags2: u8,
    /// Limitador de busca.
    pub i_plan_limit: u32,
}

// ---------------------------------------------------------------------------------------------
// WhereInfo
// ---------------------------------------------------------------------------------------------

/// `WhereInfo`: o estado completo do planejador, devolvido pela primeira metade do WHERE e
/// passado para a segunda. Ver o cabeçalho do módulo para os campos que não existem.
#[derive(Default)]
pub struct WhereInfo {
    /// A cláusula ORDER BY (cópia possuída), ou `None`.
    pub p_order_by: Option<Box<ExprList>>,
    /// O conjunto de resultados (cópia possuída).
    pub p_result_set: Option<Box<ExprList>>,
    /// Cursores `OP_OpenWrite` da otimização ONEPASS.
    pub ai_cur_one_pass: [i32; 2],
    /// Salte aqui para continuar com o próximo registro.
    pub i_continue: i32,
    /// Salte aqui para sair do laço.
    pub i_break: i32,
    /// `pParse->nQueryLoop` fora do laço do WHERE.
    pub saved_n_query_loop: i32,
    /// Flags passadas originalmente a `sqlite3WhereBegin()`.
    pub wctrl_flags: u16,
    /// LIMIT, se `wctrl_flags` tem `WHERE_USE_LIMIT`.
    pub i_limit: LogEst,
    /// Número de laços aninhados.
    pub n_level: u8,
    /// Número de termos do ORDER BY satisfeitos por índices.
    pub n_ob_sat: i8,
    /// `ONEPASS_OFF`, `ONEPASS_SINGLE` ou `ONEPASS_MULTI`.
    pub e_one_pass: u8,
    /// Um dos valores `WHERE_DISTINCT_*`.
    pub e_distinct: u8,
    /// Usa `OP_DeferredSeek`.
    pub b_deferred_seek: bool,
    /// Nem todos os termos do WHERE foram resolvidos pelo laço externo.
    pub untested_terms: bool,
    /// Verdadeiro se só o laço mais interno é ordenado.
    pub b_ordered_inner_loop: bool,
    /// Verdadeiro se realmente ordenado (e não só agrupado).
    pub sorted: bool,
    /// Número estimado de linhas de saída.
    pub n_row_out: LogEst,
    /// O começo mesmo do laço do WHERE.
    pub i_top: i32,
    /// Fim da própria cláusula WHERE.
    pub i_end_where: i32,
    /// Lista de todos os `WhereLoop` (na ordem da lista do C), por id.
    pub p_loops: Vec<LoopId>,
    /// Máscara dos termos do ORDER BY que precisam de inversão.
    pub rev_mask: Bitmask,
    /// Decomposição da cláusula WHERE (`sWC`).
    pub s_wc: ClauseId,
    /// Mapa de números de cursor para máscaras.
    pub s_mask_set: WhereMaskSet,
    /// Informação de cada laço aninhado (`a[]`).
    pub a: Vec<WhereLevel>,
    /// Arena de cláusulas.
    pub clauses: Vec<WhereClause>,
    /// Arena de termos.
    pub terms: Vec<WhereTerm>,
    /// Arena de `WhereLoop`.
    pub loops: Vec<WhereLoop>,
    /// Arena das árvores de expressão dos termos.
    pub exprs: WhereExprs,
    /// A raiz da cópia do WHERE na arena, quando há WHERE. O C percorre o `pSelect->pWhere`
    /// original em `whereIsCoveringIndex`; aqui o WHERE do select pode estar fora dele durante o
    /// `where_begin`, e a cópia da arena tem as mesmas referências de coluna.
    pub where_root: Option<ExprRef>,
}

impl WhereInfo {
    /// O termo `id`.
    #[inline]
    pub fn term(&self, id: TermId) -> &WhereTerm {
        &self.terms[id.0 as usize]
    }

    /// O termo `id`, para alterar.
    #[inline]
    pub fn term_mut(&mut self, id: TermId) -> &mut WhereTerm {
        &mut self.terms[id.0 as usize]
    }

    /// A cláusula `id`.
    #[inline]
    pub fn clause(&self, id: ClauseId) -> &WhereClause {
        &self.clauses[id.0 as usize]
    }

    /// A cláusula `id`, para alterar.
    #[inline]
    pub fn clause_mut(&mut self, id: ClauseId) -> &mut WhereClause {
        &mut self.clauses[id.0 as usize]
    }

    /// `pWC->a[i]`: o termo da posição `i` da cláusula.
    #[inline]
    pub fn term_at(&self, wc: ClauseId, i: usize) -> TermId {
        self.clauses[wc.0 as usize].a[i]
    }

    /// `pWC->nTerm`.
    #[inline]
    pub fn n_term(&self, wc: ClauseId) -> usize {
        self.clauses[wc.0 as usize].a.len()
    }

    /// `pTerm->pExpr`.
    #[inline]
    pub fn expr(&self, id: TermId) -> &Expr {
        self.exprs.get(&self.terms[id.0 as usize].p_expr).expect("WhereTerm.pExpr")
    }

    /// `pTerm->pExpr`, para alterar ou gerar código.
    #[inline]
    pub fn expr_mut(&mut self, id: TermId) -> &mut Expr {
        let r = &self.terms[id.0 as usize].p_expr;
        self.exprs.get_mut(r).expect("WhereTerm.pExpr")
    }

    /// O `WhereLoop` `id`.
    #[inline]
    pub fn w_loop(&self, id: LoopId) -> &WhereLoop {
        &self.loops[id.0 as usize]
    }

    /// O `WhereLoop` `id`, para alterar.
    #[inline]
    pub fn w_loop_mut(&mut self, id: LoopId) -> &mut WhereLoop {
        &mut self.loops[id.0 as usize]
    }
}
