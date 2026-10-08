// Mesclado das partes traduzidas de vdbe_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Cabeçalho do Virtual DataBase Engine (VDBE). Um único Vdbe é uma estrutura opaca; o
// `struct Vdbe` em si é definido na tradução de vdbeInt.h (aqui o C só faz o typedef),
// então nada é declarado para ele neste arquivo.

// Os nomes dos tipos abaixo, declarados em vdbeInt.h, são necessários para a definição de VdbeOp.

/// Sub-rotina usada para implementar um programa de trigger. É compartilhada (a lista de
/// sub-programas já visitados e o P4 de `OP_Program` apontam para o mesmo nó), por isso vive
/// atrás de `Rc<RefCell<_>>`.
pub struct SubProgram {
    /// Array de opcodes do sub-programa.
    pub a_op: Vec<VdbeOp>,
    /// Elementos em a_op.
    pub n_op: i32,
    /// Número de células de memória necessárias.
    pub n_mem: i32,
    /// Número de cursores necessários.
    pub n_csr: i32,
    /// Array de flags de OP_Once.
    pub a_once: Vec<u8>,
    /// Id usado só por identidade para triggers recursivos (o `void*` do C nunca é desreferenciado).
    pub token: usize,
    /// Próximo sub-programa já visitado.
    pub p_next: Option<SubProgramRef>,
}

pub type SubProgramRef = Rc<RefCell<SubProgram>>;

/// Quarto parâmetro de uma instrução (`union p4union` do C). A variante em uso acompanha
/// `VdbeOp::p4type`, que continua existindo porque o código do C decide pela constante P4_xxx.
pub enum P4Value {
    /// Nenhum P4 (P4_NOTUSED e P4_TRANSIENT; também o `z == NULL`).
    NotUsed,
    /// Texto estático ou transitório (P4_STATIC, P4_TRANSIENT com cópia): o campo `z`.
    Static(Vec<u8>),
    /// Texto alocado dinamicamente (P4_DYNAMIC): o campo `z`.
    Dynamic(Vec<u8>),
    /// Valor inteiro de 32 bits (P4_INT32): o campo `i`.
    Int32(i32),
    /// Inteiro de 64 bits (P4_INT64): o campo `pI64`.
    Int64(i64),
    /// Ponto flutuante de 64 bits (P4_REAL): o campo `pReal`.
    Real(f64),
    /// Vetor de inteiros de 32 bits (P4_INTARRAY): o campo `ai`.
    IntArray(Vec<u32>),
    /// Sequência de colação (P4_COLLSEQ): o campo `pColl`.
    CollSeq(Rc<RefCell<CollSeq>>),
    /// Sub-programa de trigger (P4_SUBPROGRAM): o campo `pProgram`.
    SubProgram(SubProgramRef),
    /// Tabela sem contagem de referência (P4_TABLE) ou com ela (P4_TABLEREF): o campo `pTab`.
    Table(TableRef),
    /// Definição de função (P4_FUNCDEF): o campo `pFunc`.
    FuncDef(Rc<FuncDef>),
    /// Contexto de função (P4_FUNCCTX): o campo `pCtx`.
    FuncCtx(Box<sqlite3_context>),
    /// Informação de chave de índice (P4_KEYINFO): o campo `pKeyInfo`.
    KeyInfo(Rc<RefCell<KeyInfo>>),
    /// Árvore de expressão (P4_EXPR): o campo `pExpr`.
    Expr(Box<Expr>),
    /// Valor Mem (P4_MEM): o campo `pMem`.
    Mem(Box<Mem>),
    /// Tabela virtual (P4_VTAB): o campo `pVtab`.
    VTab(Rc<RefCell<VTable>>),
}

/// Uma instrução única da máquina virtual: um opcode e até três operandos mais P4 e P5.
/// Sem SQLITE_ENABLE_EXPLAIN_COMMENTS, SQLITE_VDBE_COVERAGE, SQLITE_ENABLE_STMT_SCANSTATUS e
/// VDBE_PROFILE (opções que o Debian não liga), os campos opcionais não existem.
pub struct VdbeOp {
    /// Qual operação executar.
    pub opcode: u8,
    /// Uma das constantes P4_xxx para p4.
    pub p4type: i8,
    /// Quinto parâmetro, inteiro sem sinal de 16 bits.
    pub p5: u16,
    /// Primeiro operando.
    pub p1: i32,
    /// Segundo parâmetro (frequentemente o destino do salto).
    pub p2: i32,
    /// Terceiro parâmetro.
    pub p3: i32,
    /// Quarto parâmetro.
    pub p4: P4Value,
}

/// Versão menor de VdbeOp usada por `vdbe_add_op_list()` porque ocupa menos espaço.
#[derive(Clone, Copy)]
pub struct VdbeOpList {
    /// Qual operação executar.
    pub opcode: u8,
    /// Primeiro operando.
    pub p1: i8,
    /// Segundo parâmetro (frequentemente o destino do salto).
    pub p2: i8,
    /// Terceiro parâmetro.
    pub p3: i8,
}

// Valores permitidos de VdbeOp.p4type.
/// O parâmetro P4 não é usado.
pub const P4_NOTUSED: i8 = 0;
/// P4 é um ponteiro para uma string transitória.
pub const P4_TRANSIENT: i8 = 0;
/// Ponteiro para uma string estática.
pub const P4_STATIC: i8 = -1;
/// P4 é um ponteiro para uma estrutura CollSeq.
pub const P4_COLLSEQ: i8 = -2;
/// P4 é um inteiro de 32 bits com sinal.
pub const P4_INT32: i8 = -3;
/// P4 é um ponteiro para uma estrutura SubProgram.
pub const P4_SUBPROGRAM: i8 = -4;
/// P4 é um ponteiro para uma estrutura Table.
pub const P4_TABLE: i8 = -5;
// Os de cima não possuem recursos. Os de baixo precisam ser liberados.
pub const P4_FREE_IF_LE: i8 = -6;
/// Ponteiro para memória de sqliteMalloc().
pub const P4_DYNAMIC: i8 = -6;
/// P4 é um ponteiro para uma estrutura FuncDef.
pub const P4_FUNCDEF: i8 = -7;
/// P4 é um ponteiro para uma estrutura KeyInfo.
pub const P4_KEYINFO: i8 = -8;
/// P4 é um ponteiro para uma árvore Expr.
pub const P4_EXPR: i8 = -9;
/// P4 é um ponteiro para uma estrutura Mem.
pub const P4_MEM: i8 = -10;
/// P4 é um ponteiro para uma estrutura sqlite3_vtab.
pub const P4_VTAB: i8 = -11;
/// P4 é um valor de ponto flutuante de 64 bits.
pub const P4_REAL: i8 = -12;
/// P4 é um inteiro de 64 bits com sinal.
pub const P4_INT64: i8 = -13;
/// P4 é um vetor de inteiros de 32 bits.
pub const P4_INTARRAY: i8 = -14;
/// P4 é um ponteiro para um objeto sqlite3_context.
pub const P4_FUNCCTX: i8 = -15;
/// Como P4_TABLE, mas com contagem de referência.
pub const P4_TABLEREF: i8 = -16;

// Códigos de mensagem de erro para OP_Halt (nome do C em MAIÚSCULAS, pela regra de constantes).
pub const P5_CONSTRAINTNOTNULL: u16 = 1;
pub const P5_CONSTRAINTUNIQUE: u16 = 2;
pub const P5_CONSTRAINTCHECK: u16 = 3;
pub const P5_CONSTRAINTFK: u16 = 4;

// O array Vdbe.aColName contém 5n estruturas Mem, onde n é o número de colunas de dados
// devolvidas pelo statement.
pub const COLNAME_NAME: i32 = 0;
pub const COLNAME_DECLTYPE: i32 = 1;
pub const COLNAME_DATABASE: i32 = 2;
pub const COLNAME_TABLE: i32 = 3;
pub const COLNAME_COLUMN: i32 = 4;
/// Número de símbolos COLNAME_xxx (SQLITE_ENABLE_COLUMN_METADATA está ligado no Debian).
pub const COLNAME_N: i32 = 5;

/// Converte um label devolvido por `vdbe_make_label()` num índice do array Parse.aLabel[], que
/// guarda o endereço resolvido do label (macro ADDR).
#[inline]
pub fn addr(x: i32) -> i32 {
    !x
}

// O makefile varre vdbe.c e gera opcodes.h; os OP_XXX vêm da tradução de opcodes.h.

// Flags SQLITE_PREPARE_* não públicas.
/// Preserva o texto SQL.
pub const SQLITE_PREPARE_SAVESQL: u32 = 0x80;
/// Máscara das flags públicas.
pub const SQLITE_PREPARE_MASK: u32 = 0x0f;

/// Função de comparação de registros (`typedef int (*RecordCompare)(int,const void*,UnpackedRecord*)`).
pub type RecordCompare = fn(i32, &[u8], &mut UnpackedRecord) -> i32;

// Os protótipos SQLITE_PRIVATE do C são as funções traduzidas em vdbeaux.c, vdbeapi.c etc.,
// pelas regras de nomes. As macros ExplainQueryPlan(P), ExplainQueryPlanPop(P) e
// ExplainQueryPlanParent(P) são chamadas diretas a `vdbe_explain`, `vdbe_explain_pop` e
// `vdbe_explain_parent` (EXPLAIN não está omitido); ExplainQueryPlan2(V,P) cai em
// ExplainQueryPlan(P) porque STMT_SCANSTATUS está desligado. VdbeComment, VdbeNoopComment e
// VdbeModuleComment expandem para nada (EXPLAIN_COMMENTS desligado): as chamadas somem.

// Macros que viram nada sem SQLITE_DEBUG: mantidas como funções vazias para os chamadores
// traduzidos pelas regras de nomes continuarem compilando.
#[inline]
pub fn vdbe_verify_no_malloc_required<A, B>(_p: A, _n: B) {}
#[inline]
pub fn vdbe_verify_no_result_row<A>(_p: A) {}
#[inline]
pub fn vdbe_verify_abortable<A, B>(_p: A, _n: B) {}
#[inline]
pub fn vdbe_no_jumps_outside_subrtn<A, B, C, D>(_p: A, _a: B, _b: C, _c: D) {}
#[inline]
pub fn vdbe_release_registers<A, B, C, D, E>(_p: A, _addr: B, _n: C, _mask: D, _f: E) {}
/// `sqlite3ExplainBreakpoint(A,B)` é um no-op fora de SQLITE_DEBUG.
#[inline]
pub fn explain_breakpoint<A, B>(_a: A, _b: B) {}


// ---- part_001.rs ----

// As macros VdbeCoverage* marcam ramos do VDBE para teste de cobertura. Sem SQLITE_VDBE_COVERAGE
// (o Debian não liga) todas expandem para nada, e `sqlite3VdbeSetLineNumber` não existe. Ficam
// aqui como funções vazias, genéricas no primeiro argumento, para os chamadores traduzidos pelas
// regras de nomes não precisarem decidir o tipo (Vdbe, VdbeRef ou referência).

#[inline]
pub fn vdbe_coverage<V>(_v: V) {}

#[inline]
pub fn vdbe_coverage_if<V>(_v: V, _x: bool) {}

#[inline]
pub fn vdbe_coverage_always_taken<V>(_v: V) {}

#[inline]
pub fn vdbe_coverage_never_taken<V>(_v: V) {}

#[inline]
pub fn vdbe_coverage_never_null<V>(_v: V) {}

#[inline]
pub fn vdbe_coverage_never_null_if<V>(_v: V, _x: bool) {}

#[inline]
pub fn vdbe_coverage_eq_ne<V>(_v: V) {}

/// `VDBE_OFFSET_LINENO(x)` vale 0 sem cobertura.
#[inline]
pub fn vdbe_offset_lineno(_x: i32) -> i32 {
    0
}

// Sem SQLITE_ENABLE_STMT_SCANSTATUS, as três rotinas de scan status são macros vazias.
#[inline]
pub fn vdbe_scan_status<V, N>(_v: V, _a: i32, _b: i32, _c: i32, _est: LogEst, _name: N) {}

#[inline]
pub fn vdbe_scan_status_range<V>(_v: V, _a: i32, _b: i32, _c: i32) {}

#[inline]
pub fn vdbe_scan_status_counters<V>(_v: V, _a: i32, _b: i32, _c: i32) {}

// `sqlite3VdbePrintOp` só existe com SQLITE_DEBUG ou VDBE_PROFILE e
// `sqlite3CursorRangeHintExprCheck` só com CURSOR_HINTS junto de SQLITE_DEBUG: nenhum está
// ligado no Debian, então ambos somem.

