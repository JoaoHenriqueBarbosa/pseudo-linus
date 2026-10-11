//! Tipos do VDBE (modelo v2): `Vdbe`, `Op` (`VdbeOp`), `P4`, `SubProgram`, `VdbeFrame`,
//! `VdbeCursor`, `AuxData`, `ScanStatus`, `PreUpdate` e companheiros de vdbeInt.h e vdbe.h.
//! As constantes (`P4_*`, `COLNAME_*`, `CURTYPE_*`, `MEM_*`, `VDBE_*_STATE`, `CACHE_STALE`,
//! `SQLITE_FRAME_MAGIC`, `SQLITE_PREPARE_*`) vivem em `crate::consts`; `Mem`, `KeyInfo`,
//! `CollSeq` e `UnpackedRecord` em `crate::mem`. A conexão, `FuncDef`, `FuncCtx`, handles
//! (`StmtId`, `VTableId`) e a tabela virtual (`VtabCursor`) vivem em `crate::connection`.
//!
//! CAMPOS:
//!
//! `VdbeOp` (alias `Op`), derivado de `Default`:
//!   opcode: u8                     opcode
//!   p5: u16                        p5
//!   p1: i32, p2: i32, p3: i32      p1, p2, p3
//!   p4: P4                         p4 + p4type (o tipo é a variante; `P4::p4_type()` devolve o `P4_*`)
//!   comment: Option<Vec<u8>>       zComment (SQLITE_ENABLE_EXPLAIN_COMMENTS)
//!   n_exec: u64, n_cycle: u64      nExec, nCycle (SQLITE_ENABLE_STMT_SCANSTATUS)
//!   (some: p4type, iSrcLine (VDBE_COVERAGE))
//!
//! `VdbeOpList`:
//!   opcode: u8, p1: i8, p2: i8, p3: i8
//!
//! `SubProgram` (compartilhado, imutável depois de pronto, por isso `Rc<SubProgram>`):
//!   a_op: Vec<Op>                  aOp (nOp é `a_op.len()`)
//!   n_mem: i32, n_csr: i32         nMem, nCsr
//!   a_once: Vec<u8>                aOnce (molde do tamanho: `(nOp+7)/8` bytes)
//!   token: usize                   token (identidade do gatilho, ver as decisões)
//!   (some: pNext: a lista de `Vdbe.pProgram` é `Vdbe.p_program`)
//!
//! `VdbeCursor` (derivado de `Default`):
//!   e_cur_type: u8                 eCurType (`CURTYPE_*`)
//!   i_db: i8                       iDb
//!   null_row: bool                 nullRow
//!   deferred_moveto: bool          deferredMoveto
//!   is_table: bool                 isTable
//!   is_ephemeral: bool             isEphemeral
//!   use_random_rowid: bool         useRandomRowid
//!   is_ordered: bool               isOrdered
//!   no_reuse: bool                 noReuse
//!   col_cache: bool                colCache
//!   seek_hit: u16                  seekHit
//!   p_btx: Option<Box<Btree>>      ub.pBtx (o arquivo temporário do cursor efêmero, possuído)
//!   a_alt_map: Vec<u32>            ub.aAltMap (vazio é o ponteiro nulo)
//!   seq_count: i64                 seqCount
//!   cache_status: u32              cacheStatus
//!   seek_result: i32               seekResult
//!   p_alt_cursor: Option<i32>      pAltCursor (número do cursor, índice em `Vdbe.ap_csr`)
//!   p_cursor: Option<CursorId>     uc.pCursor (cursor do btree, no `BtShared` do `Btree` do cursor)
//!   p_v_cur: Option<Box<dyn VtabCursor>>  uc.pVCur
//!   p_sorter: Option<Box<VdbeSorter>>     uc.pSorter
//!   p_key_info: Option<Rc<KeyInfo>>       pKeyInfo
//!   i_hdr_offset: u32              iHdrOffset
//!   pgno_root: u32                 pgnoRoot
//!   n_field: i16                   nField
//!   n_hdr_parsed: u16              nHdrParsed
//!   moveto_target: i64             movetoTarget
//!   a_offset: Vec<u32>             aOffset (`n_field + 1` entradas)
//!   a_row: Vec<u8>                 aRow (CÓPIA dos bytes da linha, ver as decisões)
//!   payload_size: u32              payloadSize
//!   sz_row: u32                    szRow
//!   p_cache: Option<Box<VdbeTxtBlbCache>>  pCache
//!   a_type: Vec<u32>               aType (`n_field` entradas)
//!   (somem: seekOp, wrFlag (SQLITE_DEBUG), maskUsed (COLUMN_USED_MASK desligado))
//!
//! `VdbeTxtBlbCache`:
//!   p_c_value: Vec<u8>, i_offset: i64, i_col: i32, cache_status: u32, col_cache_ctr: u32
//!
//! `VdbeFrame` (o estado SALVO do chamador; os registros e cursores do filho ficam em `Vdbe`):
//!   a_op: Option<Rc<SubProgram>>   aOp (programa do chamador; `None` é o programa principal)
//!   a_mem: Vec<Mem>                aMem do chamador (movido para fora do `Vdbe`)
//!   ap_csr: Vec<Option<Box<VdbeCursor>>>  apCsr do chamador (idem)
//!   a_once: Vec<u8>                aOnce (os bits de `OP_Once` do PROGRAMA FILHO)
//!   token: usize                   token
//!   last_rowid: i64                lastRowid
//!   p_aux_data: Vec<AuxData>       pAuxData do chamador
//!   n_cursor: i32, pc: i32, n_op: i32, n_mem: i32   nCursor, pc, nOp, nMem
//!   n_child_mem: i32, n_child_csr: i32              nChildMem, nChildCsr
//!   n_change: i64, n_db_change: i64                 nChange, nDbChange
//!   (somem: v, pParent (a pilha é `Vdbe.p_frame`), iFrameMagic (SQLITE_DEBUG))
//!
//! `AuxData`:
//!   i_aux_op: i32, i_aux_arg: i32  iAuxOp, iAuxArg
//!   p_aux: Option<Rc<dyn Any>>     pAux
//!   x_delete_aux: Option<DestroyFn>  xDeleteAux (roda no `Drop`)
//!   (some: pNextAux)
//!
//! `ScanStatus`:
//!   addr_explain: i32, a_addr_range: [i32; 6], addr_loop: i32, addr_visit: i32
//!   i_select_id: i32, n_est: i16 (LogEst), z_name: Option<Vec<u8>>
//!
//! `VListEntry` (o `VList` do C, `Parse.pVList` e `Vdbe.pVList`):
//!   i_var: i32, name: Vec<u8>
//!
//! `PreUpdate`:
//!   stmt: Option<StmtId>           v
//!   i_db: i32, cursor: Option<CursorId>, p_key_info/is_table via cursor (ver as decisões)
//!   op: i32, a_record: Vec<u8>, keyinfo: Option<Rc<KeyInfo>>
//!   p_unpacked: Option<Box<UnpackedRecord>>, p_new_unpacked: Option<Box<UnpackedRecord>>
//!   i_new_reg: i32, i_blob_write: i32, i_key1: i64, i_key2: i64
//!   a_new: Vec<Mem>, p_tab: Option<Rc<Table>>, p_pk: Option<Rc<Index>>
//!
//! `Vdbe` (derivado de `Default`):
//!   n_var: i32, n_mem: i32, n_cursor: i32     nVar, nMem, nCursor
//!   cache_ctr: u32, pc: i32, rc: i32          cacheCtr, pc, rc
//!   n_change: i64, i_statement: i32           nChange, iStatement
//!   i_current_time: i64                       iCurrentTime
//!   n_fk_constraint: i64                      nFkConstraint
//!   n_stmt_def_cons: i64, n_stmt_def_imm_cons: i64
//!   a_mem: Vec<Mem>                           aMem (índice 0 não usado: registros de 1 a n_mem)
//!   ap_csr: Vec<Option<Box<VdbeCursor>>>      apCsr
//!   a_var: Vec<Mem>                           aVar
//!   a_op: Vec<Op>                             aOp (nOp é `a_op.len()`)
//!   a_col_name: Vec<Mem>                      aColName
//!   p_result_row: Option<usize>               pResultRow (índice do primeiro registro da linha)
//!   z_err_msg: Option<Vec<u8>>                zErrMsg
//!   p_v_list: Vec<VListEntry>                 pVList
//!   start_time: i64                           startTime
//!   n_res_column: u16, n_res_alloc: u16       nResColumn, nResAlloc
//!   error_action: u8, min_write_file_format: u8, prep_flags: u8, e_vdbe_state: u8
//!   expired: u8, explain: u8                  expired:2, explain:2
//!   change_cnt_on: bool, uses_stmt_journal: bool, read_only: bool, b_is_reader: bool,
//!   have_eqp_ops: bool                        bitfields
//!   btree_mask: YDbMask, lock_mask: YDbMask   btreeMask, lockMask
//!   a_counter: [u32; 9]                       aCounter
//!   z_sql: Option<Vec<u8>>                    zSql
//!   p_frame: Vec<VdbeFrame>                   pFrame + pParent como pilha (o topo é o pFrame)
//!   p_del_frame: Vec<VdbeFrame>               pDelFrame
//!   p_cur_prog: Option<Rc<SubProgram>>        o `aOp` corrente (`None` é `a_op`)
//!   exp_mask: u32                             expmask
//!   p_program: Vec<Rc<SubProgram>>            pProgram (ordem do C é o inverso do `push`)
//!   p_aux_data: Vec<AuxData>                  pAuxData
//!   a_scan: Vec<ScanStatus>                   aScan (nScan é o `len`)
//!   limit_vdbe_op: i32                        cópia de `aLimit[SQLITE_LIMIT_VDBE_OP]` (ver as decisões)
//!   (somem: db, ppVPrev, pVNext (a lista é `Connection.stmt_list`), pParse, apArg, nOp,
//!   nOpAlloc, pFree, nFrame, rcApp, nWrite (SQLITE_DEBUG), zNormSql e pDblStr
//!   (SQLITE_ENABLE_NORMALIZE desligado))
//!
//! Decisões e desvios do C:
//!
//! * `Op.p4` é um `enum P4` com um dono por variante (o `union p4union` mais `p4type`). A
//!   distinção `P4_STATIC`/`P4_TRANSIENT`/`P4_DYNAMIC` só decide quem libera o texto, e em Rust
//!   tudo é possuído: as três viram `P4::Text` ou `P4::Blob` (`p4_type()` devolve `P4_DYNAMIC`).
//!   `P4_TABLE` e `P4_TABLEREF` ficam como variantes separadas porque o C testa as duas.
//!   `P4::IntArray` guarda o `u32 *ai` do C com a contagem NO ELEMENTO 0, como `ai[0]`.
//! * `P4::Subprogram` é `Rc<SubProgram>` (não `Box`): o programa também mora em
//!   `Vdbe.p_program` e é percorrido durante a execução de um `OP_Program`, quando o `Vdbe`
//!   troca o programa corrente por ele (`p_cur_prog`). Depois de pronto o `SubProgram` não muda.
//!   `SubProgram.token` é a identidade do gatilho (o C compara ponteiros): quem gera o
//!   programa usa `Rc::as_ptr(&trigger) as usize`.
//! * `P4::FuncCtx` guarda um `Rc<FuncCtx>` IMUTÁVEL (função e `argc`). O `sqlite3_context` do C
//!   (`Context`, em `crate::connection`) é montado a cada chamada pelo `OP_Function`,
//!   `OP_AggStep` e companhia, porque carrega `&mut Connection`; `isError`, `skipFlag` e
//!   `pOut` são estado de UMA chamada, nunca do opcode.
//! * Frames (`OP_Program`): o C troca os ponteiros `aOp`, `aMem`, `apCsr` do `Vdbe` pelos do
//!   programa filho e guarda os do chamador no frame. Aqui o frame guarda, por valor, os
//!   `a_mem` e `ap_csr` do CHAMADOR (movidos para fora do `Vdbe` com `mem::take`), o `Vdbe`
//!   passa a ter os do filho, e `p_cur_prog` troca o programa corrente. Voltar do frame é o
//!   caminho inverso. O truque `VdbeFrameMem` (registros dentro da memória do frame) e o
//!   reuso do frame guardado no registro `OP_Program.p3` não existem: o frame é novo a cada
//!   chamada. `p_del_frame` continua existindo porque fechar um cursor de btree exige
//!   `&mut Connection` (um `Drop` não alcança a conexão): `sqlite3VdbeFrameDelete` fecha os
//!   cursores do frame explicitamente, no mesmo ponto em que o C o faria.
//! * O mesmo vale para `VdbeCursor` e `Vdbe`: soltar um `VdbeCursor` NÃO fecha o cursor do
//!   btree (`p_cursor` é só um `CursorId`) nem o `Btree` efêmero de `p_btx` de forma ordenada.
//!   `sqlite3VdbeFreeCursor` é chamado explicitamente com a conexão.
//! * `VdbeCursor.a_row` é uma CÓPIA dos bytes da linha quando ela cabe numa página (o C
//!   aponta para dentro da página do pager); vazio com `sz_row == 0` é o ponteiro nulo. A
//!   cópia custa um memcpy por linha lida e elimina o aliasing com o pager.
//! * `VdbeCursor` não carrega `aType` e `aOffset` no fim da struct: são dois `Vec<u32>`
//!   dimensionados por `n_field` na alocação (o `allocateCursor` do C).
//! * `Vdbe.limit_vdbe_op` é cópia de `db->aLimit[SQLITE_LIMIT_VDBE_OP]` feita em
//!   `sqlite3VdbeCreate`: `sqlite3VdbeAddOp3` é chamada milhares de vezes por consulta e o
//!   `growOpArray` consulta o limite; a cópia evita passar a conexão em cada `add_op`. O
//!   `Vdbe` também NÃO tem `pParse`: enquanto o código é gerado o `Parse` possui o `Vdbe`
//!   (`Parse.p_vdbe`), e as funções que o C chamava por `v->pParse` recebem o `Parse`.
//! * `Vdbe.btree_mask`/`lock_mask` são `YDbMask = u32` (`SQLITE_MAX_ATTACHED` vale 10, menor
//!   que 30, então o C usa `unsigned int`).
//! * Valores ponteiro (`ValueList` do `sqlite3_vtab_in`, `sqlite3_value_pointer`) NÃO têm tipo
//!   aqui: dependem do `Mem` de ponteiro, adiado em `crate::mem`.
//! * `PreUpdate`: o C guarda `VdbeCursor *pCsr` e lê o payload velho pelo cursor do btree. O
//!   gancho recebe `&mut Connection` e `&mut PreUpdate` e precisa de `i_db` e do `CursorId`
//!   para chegar ao btree (`conn.dbs[i_db].bt`); `p_tab` e `p_pk` são os `Rc` do esquema.

use std::any::Any;
use std::rc::Rc;

use crate::btree_types::{Btree, CursorId};
use crate::connection::{DestroyFn, FuncCtx, FuncDef, StmtId, VTableId, VtabCursor};
use crate::consts::{
    P4_COLLSEQ, P4_DYNAMIC, P4_EXPR, P4_FUNCCTX, P4_FUNCDEF, P4_INT32, P4_INT64, P4_INTARRAY,
    P4_KEYINFO, P4_MEM, P4_NOTUSED, P4_REAL, P4_SUBPROGRAM, P4_TABLE, P4_TABLEREF, P4_VTAB,
};
use crate::mem::{CollSeq, KeyInfo, Mem, UnpackedRecord};
use crate::sqlite_int::*;
use crate::vdbesort::VdbeSorter;

/// `yDbMask`: máscara dos bancos de `Connection.dbs` (`SQLITE_MAX_ATTACHED` <= 30).
pub type YDbMask = u32;

/// `DbMaskTest(M, I)`.
#[inline]
pub fn db_mask_test(m: YDbMask, i: usize) -> bool {
    (m & (1u32 << i)) != 0
}

/// `DbMaskSet(M, I)`.
#[inline]
pub fn db_mask_set(m: &mut YDbMask, i: usize) {
    *m |= 1u32 << i;
}

/// `ADDR(X)` do vdbe.h: converte um rótulo de `sqlite3VdbeMakeLabel` em índice de `Parse.a_label`.
#[inline]
pub fn addr(x: i32) -> i32 {
    !x
}

// ---------------------------------------------------------------------------------------------
// Instruções
// ---------------------------------------------------------------------------------------------

/// O quarto operando de uma instrução (`union p4union` mais `VdbeOp.p4type`).
#[derive(Default)]
pub enum P4 {
    /// `P4_NOTUSED`.
    #[default]
    None,
    /// `P4_INT32`.
    Int32(i32),
    /// `P4_INT64`.
    Int64(i64),
    /// `P4_REAL`.
    Real(f64),
    /// `P4_STATIC`, `P4_TRANSIENT` e `P4_DYNAMIC` com texto.
    Text(Vec<u8>),
    /// `P4_DYNAMIC` com bytes (o `OP_Blob`, com o tamanho em `p1`).
    Blob(Vec<u8>),
    /// `P4_COLLSEQ`; `None` é o ponteiro nulo (BINARY implícita).
    Coll(Option<Rc<CollSeq>>),
    /// `P4_KEYINFO`.
    KeyInfo(Rc<KeyInfo>),
    /// `P4_FUNCDEF`.
    FuncDef(Rc<FuncDef>),
    /// `P4_FUNCCTX`.
    FuncCtx(Rc<FuncCtx>),
    /// `P4_MEM`.
    Mem(Box<Mem>),
    /// `P4_TABLE`.
    Table(Rc<Table>),
    /// `P4_TABLEREF` (o C conta referências; aqui é a contagem do `Rc`).
    TableRef(Rc<Table>),
    /// `P4_VTAB`.
    Vtab(VTableId),
    /// `P4_SUBPROGRAM`.
    Subprogram(Rc<SubProgram>),
    /// `P4_INTARRAY`: o `u32 *ai` do C, com a contagem em `[0]`.
    IntArray(Vec<u32>),
    /// `P4_EXPR` (só com `SQLITE_ENABLE_CURSOR_HINTS`).
    Expr(Box<Expr>),
}

impl P4 {
    /// O `VdbeOp.p4type` que corresponde à variante.
    pub fn p4_type(&self) -> i8 {
        match self {
            P4::None => P4_NOTUSED,
            P4::Int32(_) => P4_INT32,
            P4::Int64(_) => P4_INT64,
            P4::Real(_) => P4_REAL,
            P4::Text(_) | P4::Blob(_) => P4_DYNAMIC,
            P4::Coll(_) => P4_COLLSEQ,
            P4::KeyInfo(_) => P4_KEYINFO,
            P4::FuncDef(_) => P4_FUNCDEF,
            P4::FuncCtx(_) => P4_FUNCCTX,
            P4::Mem(_) => P4_MEM,
            P4::Table(_) => P4_TABLE,
            P4::TableRef(_) => P4_TABLEREF,
            P4::Vtab(_) => P4_VTAB,
            P4::Subprogram(_) => P4_SUBPROGRAM,
            P4::IntArray(_) => P4_INTARRAY,
            P4::Expr(_) => P4_EXPR,
        }
    }
}

/// `struct VdbeOp`: uma instrução da máquina virtual.
#[derive(Default)]
pub struct VdbeOp {
    /// O que fazer (`OP_*`).
    pub opcode: u8,
    /// Quinto parâmetro, inteiro de 16 bits sem sinal.
    pub p5: u16,
    /// Primeiro operando.
    pub p1: i32,
    /// Segundo operando (muitas vezes o destino do salto).
    pub p2: i32,
    /// Terceiro operando.
    pub p3: i32,
    /// Quarto operando.
    pub p4: P4,
    /// Comentário para legibilidade (`SQLITE_ENABLE_EXPLAIN_COMMENTS`).
    pub comment: Option<Vec<u8>>,
    /// Quantas vezes a instrução rodou (`SQLITE_ENABLE_STMT_SCANSTATUS`).
    pub n_exec: u64,
    /// Ciclos gastos na instrução (`SQLITE_ENABLE_STMT_SCANSTATUS`).
    pub n_cycle: u64,
}

/// `typedef struct VdbeOp Op`.
pub type Op = VdbeOp;

/// `struct VdbeOpList`: a versão menor de `VdbeOp` do `sqlite3VdbeAddOpList`.
#[derive(Debug, Clone, Copy, Default)]
pub struct VdbeOpList {
    pub opcode: u8,
    pub p1: i8,
    pub p2: i8,
    pub p3: i8,
}

/// `struct SubProgram`: a sub-rotina que implementa um programa de gatilho.
#[derive(Default)]
pub struct SubProgram {
    /// As instruções do subprograma.
    pub a_op: Vec<Op>,
    /// Células de memória necessárias.
    pub n_mem: i32,
    /// Cursores necessários.
    pub n_csr: i32,
    /// Molde dos flags de `OP_Once` (`(nOp+7)/8` bytes zerados).
    pub a_once: Vec<u8>,
    /// Identidade que serve à detecção de gatilhos recursivos.
    pub token: usize,
}

// ---------------------------------------------------------------------------------------------
// Cursores, frames, auxdata
// ---------------------------------------------------------------------------------------------

/// `struct VdbeTxtBlbCache`: cache de valores TEXT ou BLOB grandes de um cursor.
#[derive(Default)]
pub struct VdbeTxtBlbCache {
    /// O buffer com o valor (`pCValue`).
    pub p_c_value: Vec<u8>,
    /// Deslocamento no arquivo da linha em cache.
    pub i_offset: i64,
    /// Coluna para a qual o cache vale.
    pub i_col: i32,
    /// Valor de `Vdbe.cache_ctr`.
    pub cache_status: u32,
    /// Contador de cache de colunas.
    pub col_cache_ctr: u32,
}

/// `struct VdbeCursor`: o invólucro dos vários tipos de cursor do VDBE (btree, ordenador, tabela
/// virtual, pseudotabela de uma linha).
#[derive(Default)]
pub struct VdbeCursor {
    /// Um dos `CURTYPE_*`.
    pub e_cur_type: u8,
    /// Índice do banco do cursor em `Connection.dbs`.
    pub i_db: i8,
    /// Verdadeiro se aponta para uma linha sem dados.
    pub null_row: bool,
    /// Falta chamar `sqlite3BtreeMoveto()`.
    pub deferred_moveto: bool,
    /// Verdadeiro em tabelas rowid, falso em índices.
    pub is_table: bool,
    /// Tabela efêmera.
    pub is_ephemeral: bool,
    /// Gera números de registro semialeatórios.
    pub use_random_rowid: bool,
    /// Verdadeiro se a tabela não é `BTREE_UNORDERED`.
    pub is_ordered: bool,
    /// `OpenEphemeral` não pode reusar este cursor.
    pub no_reuse: bool,
    /// O cache `p_cache` está inicializado.
    pub col_cache: bool,
    /// Ver `OP_SeekHit` e `OP_IfNoHope`.
    pub seek_hit: u16,
    /// `ub.pBtx`: arquivo temporário de um cursor efêmero.
    pub p_btx: Option<Box<Btree>>,
    /// `ub.aAltMap`: mapeamento de colunas de tabela para índice (vazio é o nulo).
    pub a_alt_map: Vec<u32>,
    /// Contador de sequência.
    pub seq_count: i64,
    /// O cache de `OP_Column` vale se for igual a `Vdbe.cache_ctr`.
    pub cache_status: u32,
    /// Resultado do último `sqlite3BtreeMoveto()` (em pseudotabela, o registro do conteúdo).
    pub seek_result: i32,
    /// Número do cursor de índice associado de onde ler.
    pub p_alt_cursor: Option<i32>,
    /// `uc.pCursor`: cursor do btree (`CURTYPE_BTREE` ou `CURTYPE_PSEUDO`).
    pub p_cursor: Option<CursorId>,
    /// `uc.pVCur`: cursor da tabela virtual (`CURTYPE_VTAB`).
    pub p_v_cur: Option<Box<dyn VtabCursor>>,
    /// `uc.pSorter`: o ordenador (`CURTYPE_SORTER`).
    pub p_sorter: Option<Box<VdbeSorter>>,
    /// Como comparar as chaves de cursores de índice.
    pub p_key_info: Option<Rc<KeyInfo>>,
    /// Deslocamento do próximo byte do cabeçalho ainda não lido.
    pub i_hdr_offset: u32,
    /// Página raiz do cursor do btree.
    pub pgno_root: u32,
    /// Número de campos do cabeçalho.
    pub n_field: i16,
    /// Campos do cabeçalho já lidos.
    pub n_hdr_parsed: u16,
    /// Argumento do `sqlite3BtreeMoveto()` adiado.
    pub moveto_target: i64,
    /// Deslocamentos dos campos (`n_field + 1` entradas).
    pub a_offset: Vec<u32>,
    /// Cópia dos bytes da linha corrente quando ela cabe numa página.
    pub a_row: Vec<u8>,
    /// Tamanho total do registro.
    pub payload_size: u32,
    /// Bytes disponíveis em `a_row`.
    pub sz_row: u32,
    /// Cache de TEXT e BLOB grandes.
    pub p_cache: Option<Box<VdbeTxtBlbCache>>,
    /// Tipos seriais decodificados (`n_field` entradas).
    pub a_type: Vec<u32>,
}

impl VdbeCursor {
    /// `IsNullCursor(P)`.
    #[inline]
    pub fn is_null_cursor(&self) -> bool {
        self.e_cur_type == crate::consts::CURTYPE_PSEUDO && self.null_row && self.seek_result == 0
    }
}

/// `struct AuxData`: dado auxiliar de `sqlite3_set_auxdata`, destruído quando a VM termina.
#[derive(Default)]
pub struct AuxData {
    /// Número da instrução `OP_Function`.
    pub i_aux_op: i32,
    /// Índice do argumento da função.
    pub i_aux_arg: i32,
    /// O dado.
    pub p_aux: Option<Rc<dyn Any>>,
    /// Destrutor (`xDeleteAux`), chamado uma vez em `Drop`.
    pub x_delete_aux: Option<DestroyFn>,
}

impl Drop for AuxData {
    fn drop(&mut self) {
        if let Some(destroy) = self.x_delete_aux.take() {
            destroy(self.p_aux.take());
        }
    }
}

/// `struct VdbeFrame`: o estado salvo do chamador enquanto um subprograma (`OP_Program`) roda.
#[derive(Default)]
pub struct VdbeFrame {
    /// Programa do chamador (`None` é o programa principal).
    pub a_op: Option<Rc<SubProgram>>,
    /// Registros do chamador (movidos para fora do `Vdbe`).
    pub a_mem: Vec<Mem>,
    /// Cursores do chamador (movidos para fora do `Vdbe`).
    pub ap_csr: Vec<Option<Box<VdbeCursor>>>,
    /// Bits de `OP_Once` do programa filho.
    pub a_once: Vec<u8>,
    /// Cópia de `SubProgram.token`.
    pub token: usize,
    /// Último rowid inserido (`Connection.last_rowid`).
    pub last_rowid: i64,
    /// Lista de auxdata do chamador.
    pub p_aux_data: Vec<AuxData>,
    /// Entradas de `ap_csr`.
    pub n_cursor: i32,
    /// Contador de programa no chamador.
    pub pc: i32,
    /// Tamanho de `a_op`.
    pub n_op: i32,
    /// Entradas de `a_mem`.
    pub n_mem: i32,
    /// Células de memória do frame filho.
    pub n_child_mem: i32,
    /// Cursores do frame filho.
    pub n_child_csr: i32,
    /// Mudanças do comando (`Vdbe.n_change`).
    pub n_change: i64,
    /// Valor de `Connection.n_change`.
    pub n_db_change: i64,
}

// ---------------------------------------------------------------------------------------------
// Vdbe
// ---------------------------------------------------------------------------------------------

/// `struct ScanStatus`: um valor do `sqlite3_stmt_scanstatus()`.
#[derive(Default)]
pub struct ScanStatus {
    /// Endereço do `OP_Explain` do laço.
    pub addr_explain: i32,
    /// Até três faixas de endereços (início e fim, inclusivos) cujos ciclos se somam.
    pub a_addr_range: [i32; 6],
    /// Endereço do contador de laços.
    pub addr_loop: i32,
    /// Endereço do contador de linhas visitadas.
    pub addr_visit: i32,
    /// O "Select-ID" do laço.
    pub i_select_id: i32,
    /// Estimativa de linhas de saída por laço (`LogEst`).
    pub n_est: i16,
    /// Nome da tabela ou do índice.
    pub z_name: Option<Vec<u8>>,
}

/// Uma entrada do `VList` do C: um parâmetro com nome (`:x`, `@x`, `$x`) e seu número.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VListEntry {
    /// Número do parâmetro.
    pub i_var: i32,
    /// Nome com o prefixo.
    pub name: Vec<u8>,
}

/// `struct Vdbe`: uma instância da máquina virtual. O handle é `StmtId` (o `sqlite3_stmt`).
#[derive(Default)]
pub struct Vdbe {
    /// Entradas de `a_var`.
    pub n_var: i32,
    /// Células de memória alocadas (os registros vão de 1 a `n_mem`).
    pub n_mem: i32,
    /// Posições de `ap_csr`.
    pub n_cursor: i32,
    /// Contador de geração do cache de linhas dos cursores (nunca vale `CACHE_STALE`).
    pub cache_ctr: u32,
    /// Contador de programa.
    pub pc: i32,
    /// Valor a devolver.
    pub rc: i32,
    /// Mudanças no banco desde o último reset.
    pub n_change: i64,
    /// Número do statement (0 se não abriu um).
    pub i_statement: i32,
    /// Valor de `julianday('now')` neste comando.
    pub i_current_time: i64,
    /// Restrições de chave estrangeira imediatas desta VM.
    pub n_fk_constraint: i64,
    /// Restrições adiadas quando o statement começou.
    pub n_stmt_def_cons: i64,
    /// Restrições imediatas adiadas quando o statement começou.
    pub n_stmt_def_imm_cons: i64,
    /// As células de memória.
    pub a_mem: Vec<Mem>,
    /// Um elemento por cursor aberto.
    pub ap_csr: Vec<Option<Box<VdbeCursor>>>,
    /// Valores do `OP_Variable`.
    pub a_var: Vec<Mem>,
    /// O programa principal.
    pub a_op: Vec<Op>,
    /// Nomes das colunas devolvidas (`COLNAME_N` por coluna).
    pub a_col_name: Vec<Mem>,
    /// Índice em `a_mem` do primeiro registro da linha de saída corrente.
    pub p_result_row: Option<usize>,
    /// Mensagem de erro.
    pub z_err_msg: Option<Vec<u8>>,
    /// Nomes das variáveis.
    pub p_v_list: Vec<VListEntry>,
    /// Hora em que a consulta começou (perfilamento).
    pub start_time: i64,
    /// Colunas de uma linha do resultado.
    pub n_res_column: u16,
    /// Posições alocadas em `a_col_name` (em colunas).
    pub n_res_alloc: u16,
    /// Ação de recuperação em caso de erro.
    pub error_action: u8,
    /// Formato de arquivo mínimo para bancos graváveis.
    pub min_write_file_format: u8,
    /// Flags `SQLITE_PREPARE_*`.
    pub prep_flags: u8,
    /// Um dos `VDBE_*_STATE`.
    pub e_vdbe_state: u8,
    /// 1: recompilar já; 2: quando conveniente (`expired:2`).
    pub expired: u8,
    /// 0: normal, 1: EXPLAIN, 2: EXPLAIN QUERY PLAN (`explain:2`).
    pub explain: u8,
    /// Atualizar o contador de mudanças.
    pub change_cnt_on: bool,
    /// Usa um diário de statement.
    pub uses_stmt_journal: bool,
    /// Verdadeiro em comandos que não escrevem.
    pub read_only: bool,
    /// Verdadeiro em comandos que leem.
    pub b_is_reader: bool,
    /// O bytecode suporta EXPLAIN QUERY PLAN.
    pub have_eqp_ops: bool,
    /// Bancos de `Connection.dbs` referenciados.
    pub btree_mask: YDbMask,
    /// Subconjunto de `btree_mask` que precisa de trava.
    pub lock_mask: YDbMask,
    /// Contadores do `sqlite3_stmt_status()`.
    pub a_counter: [u32; 9],
    /// Texto do SQL que gerou o comando.
    pub z_sql: Option<Vec<u8>>,
    /// Pilha de frames de `OP_Program` (o topo é o `pFrame` do C).
    pub p_frame: Vec<VdbeFrame>,
    /// Frames a liberar quando a VM reinicia.
    pub p_del_frame: Vec<VdbeFrame>,
    /// Programa corrente (`None` é `a_op`).
    pub p_cur_prog: Option<Rc<SubProgram>>,
    /// Ligar a estas variáveis invalida a VM.
    pub exp_mask: u32,
    /// Todos os subprogramas usados pela VM (a lista do C tem o mais novo na frente).
    pub p_program: Vec<Rc<SubProgram>>,
    /// Alocações de auxdata.
    pub p_aux_data: Vec<AuxData>,
    /// Definições do `sqlite3_stmt_scanstatus()`.
    pub a_scan: Vec<ScanStatus>,
    /// Cópia de `aLimit[SQLITE_LIMIT_VDBE_OP]`.
    pub limit_vdbe_op: i32,
}

impl Vdbe {
    /// `p->nOp`.
    #[inline]
    pub fn n_op(&self) -> i32 {
        self.a_op.len() as i32
    }
}

/// `struct PreUpdate`: o contexto das funções `sqlite3_preupdate_*()`.
#[derive(Default)]
pub struct PreUpdate {
    /// A VM em execução.
    pub stmt: Option<StmtId>,
    /// Banco do cursor de onde ler os valores velhos.
    pub i_db: i32,
    /// Cursor do btree de onde ler os valores velhos.
    pub cursor: Option<CursorId>,
    /// Um de `SQLITE_INSERT`, `SQLITE_UPDATE`, `SQLITE_DELETE`.
    pub op: i32,
    /// Registro velho (`old.*`).
    pub a_record: Vec<u8>,
    /// Como comparar as chaves do índice.
    pub keyinfo: Option<Rc<KeyInfo>>,
    /// `a_record` desempacotado.
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// Registro novo (`new.*`) desempacotado.
    pub p_new_unpacked: Option<Box<UnpackedRecord>>,
    /// Registrador dos valores novos.
    pub i_new_reg: i32,
    /// Valor devolvido por `preupdate_blobwrite()`.
    pub i_blob_write: i32,
    /// Primeira chave passada ao gancho.
    pub i_key1: i64,
    /// Segunda chave passada ao gancho.
    pub i_key2: i64,
    /// Valores novos (`new.*`).
    pub a_new: Vec<Mem>,
    /// Objeto do esquema sendo atualizado.
    pub p_tab: Option<Rc<Table>>,
    /// Índice da chave primária se `p_tab` é WITHOUT ROWID.
    pub p_pk: Option<Rc<Index>>,
}
