//! A conexão (`sqlite3` do sqliteInt.h) e tudo que gira em torno dela no modelo v2: `DbSlot`
//! (o `Db`), `FuncDef`, `Context` (o `sqlite3_context`), `Parse`, as tabelas virtuais
//! (`Module`, `VTable`, `VtabCtx` e os traits `VtabModule`, `Vtab`, `VtabCursor`,
//! `sqlite3_index_info`), `Savepoint`, `Lookaside`, `BusyHandler` e os pequenos tipos que o
//! `Parse` carrega. Os tipos do VDBE (`Vdbe`, `Op`, `VdbeCursor`, ...) vivem em
//! `crate::vdbe_types`; as árvores e o esquema (`Expr`, `Table`, `Schema`, ...) em
//! `crate::sqlite_int`. As constantes (flags, `SQLITE_STATE_*`, `SQLITE_FUNC_*`, `DB_*`, limites)
//! vivem em `crate::consts`.
//!
//! CAMPOS:
//!
//! `Connection` (`struct sqlite3`, derivado de `Default`):
//!   p_vfs: Option<VfsRef>                  pVfs
//!   stmts: Slab<Vdbe>                      pVdbe (os comandos, por handle `StmtId`)
//!   stmt_list: Vec<StmtId>                 a ordem da lista `pVdbe` (o C tem o mais novo na frente)
//!   p_dflt_coll: Option<Rc<CollSeq>>       pDfltColl
//!   dbs: Vec<DbSlot>                       aDb (nDb é o `len`)
//!   m_db_flags: u32, flags: u64            mDbFlags, flags
//!   last_rowid: i64, sz_mmap: i64          lastRowid, szMmap
//!   n_schema_lock: u32, open_flags: u32    nSchemaLock, openFlags
//!   err_code: i32, err_byte_offset: i32    errCode, errByteOffset
//!   err_mask: i32, i_sys_errno: i32        errMask, iSysErrno
//!   db_opt_flags: u32                      dbOptFlags
//!   enc: u8, auto_commit: u8, temp_store: u8, malloc_failed: u8, dflt_lock_mode: u8
//!   next_autovac: i8, suppress_err: u8, vtab_on_conflict: u8, is_transaction_savepoint: u8
//!   m_trace: u8, no_shared_cache: u8, n_sql_exec: u8, e_open_state: u8
//!   next_pagesize: i32                     nextPagesize
//!   n_change: i64, n_total_change: i64     nChange, nTotalChange
//!   a_limit: [i32; SQLITE_N_LIMIT]         aLimit
//!   n_max_sorter_mmap: i32                 nMaxSorterMmap
//!   init: InitInfo                         init
//!   n_vdbe_active/read/write/exec: i32     nVdbeActive, nVdbeRead, nVdbeWrite, nVdbeExec
//!   n_v_destroy: i32                       nVDestroy
//!   a_extension: Vec<DlHandle>             aExtension (nExtension é o `len`)
//!   x_trace: Option<TraceFn>               trace.xLegacy/xV2 e pTraceArg
//!   x_profile: Option<ProfileFn>           xProfile e pProfileArg
//!   x_commit_callback: Option<CommitHook>  xCommitCallback e pCommitArg
//!   x_rollback_callback: Option<RollbackHook>  xRollbackCallback e pRollbackArg
//!   x_update_callback: Option<UpdateHook>  xUpdateCallback e pUpdateArg
//!   x_autovac_pages: Option<AutovacPagesFn>  xAutovacPages, pAutovacPagesArg, xAutovacDestr
//!   x_pre_update_callback: Option<PreUpdateFn>  xPreUpdateCallback e pPreUpdateArg
//!   p_pre_update: Option<Box<PreUpdate>>   pPreUpdate
//!   x_wal_callback: Option<WalHook>        xWalCallback e pWalArg
//!   x_coll_needed: Option<CollNeededFn>    xCollNeeded, xCollNeeded16, pCollNeededArg
//!   coll_needed_16: bool                   verdadeiro quando o gancho é o `xCollNeeded16`
//!   err_msg: Option<Vec<u8>>               pErr (o texto UTF-8; `None` é o valor nulo)
//!   interrupted: Arc<AtomicBool>           u1.isInterrupted
//!   lookaside: Lookaside                   lookaside (só estatística)
//!   x_auth: Option<AuthFn>                 xAuth e pAuthArg
//!   x_progress: Option<ProgressFn>         xProgress e pProgressArg
//!   n_progress_ops: u32                    nProgressOps
//!   a_v_trans: Vec<VTableId>               aVTrans (nVTrans é o `len`)
//!   a_module: Hash<Rc<Module>>             aModule
//!   a_epo_tab: Hash<Rc<Table>>             Module.pEpoTab, movido para cá (ver as decisões)
//!   p_vtab_ctx: Vec<VtabCtx>               pVtabCtx + VtabCtx.pPrior como pilha
//!   vtabs: Slab<VTable>                    todos os `VTable` da conexão, por handle `VTableId`
//!   p_disconnect: Vec<VTableId>            pDisconnect
//!   a_func: Hash<Vec<Rc<FuncDef>>>         aFunc (a cadeia pNext é o `Vec`, na ordem do C)
//!   a_coll_seq: Hash<[Option<Rc<CollSeq>>; 3]>  aCollSeq (as 3 codificações)
//!   busy_handler: BusyHandler              busyHandler
//!   p_savepoint: Vec<Savepoint>            pSavepoint (o topo da pilha é o mais novo)
//!   n_analysis_limit: i32, busy_timeout: i32, n_savepoint: i32, n_statement: i32
//!   n_deferred_cons: i64, n_deferred_imm_cons: i64
//!   p_db_data: Vec<DbClientData>           pDbData
//!   (somem: mutex, pParse, aDbStatic, pnBytesFreed, bBenignMalloc, nExtension, nVTrans,
//!   as listas de UNLOCK_NOTIFY, userauth)
//!
//! `DbSlot` (`struct Db`, derivado de `Default`):
//!   z_db_s_name: Vec<u8>, bt: Option<Btree>, safety_level: u8, b_sync_set: bool, schema: Schema
//!
//! `InitInfo` (`struct sqlite3InitInfo`, derivado de `Default`):
//!   new_tnum: u32, i_db: u8, busy: u8, orphan_trigger: bool, imposter_table: bool,
//!   reopen_memdb: bool, az_init: Vec<Option<Vec<u8>>>
//!
//! `Lookaside`: b_disable: u32, sz: u16, sz_true: u16, n_slot: u32, an_stat: [u32; 3]
//! `Savepoint`: z_name: Vec<u8>, n_deferred_cons: i64, n_deferred_imm_cons: i64
//! `BusyHandler`: x_busy_handler: Option<Rc<dyn Fn(i32) -> i32>>, n_busy: Rc<Cell<i32>>
//! `DbClientData`: z_name: Vec<u8>, p_data: Option<Rc<dyn Any>>, x_destructor: Option<DestroyFn>
//!
//! `FuncDef` (derivado de `Default`):
//!   n_arg: i8, func_flags: u32, p_user_data: UserData, x_s_func: Option<ScalarFn>,
//!   x_finalize: Option<FinalFn>, x_value: Option<FinalFn>, x_inverse: Option<ScalarFn>,
//!   z_name: Vec<u8>, p_destructor: Option<Rc<FuncDestructor>>
//!   (somem: pNext e u.pHash: a cadeia é o `Vec` do hash)
//! `FuncDestructor`: p_user_data: Option<Rc<dyn Any>>, x_destroy: Option<DestroyFn> (nRef é o Rc)
//! `FuncCtx`: p_func: Rc<FuncDef>, argc: u8
//! `Context<'a>` (`sqlite3_context`):
//!   db: &'a mut Connection, p_aux_data: &'a mut Vec<AuxData>, i_current_time: &'a mut i64,
//!   out: Mem, arg_func: Rc<FuncDef>, agg: Option<Box<dyn Any>>, i_op: i32,
//!   is_pure_func: bool, is_error: i32, enc: u8, skip_flag: u8, argc: u8
//!
//! `Module`: p_module: Rc<dyn VtabModule>, z_name: Vec<u8>, p_aux: Option<Rc<dyn Any>>,
//!   x_destroy: Option<DestroyFn> (nRefModule é o Rc; pEpoTab vive em `a_epo_tab`)
//! `VTable`: p_mod: Rc<Module>, p_vtab: Option<Box<dyn Vtab>>, n_ref: i32, n_cursor: i32,
//!   b_constraint: u8, b_all_schemas: u8, e_vtab_risk: u8, i_savepoint: i32 (n_cursor é o
//!   `sqlite3_vtab.nRef`; `db` e `pNext` somem)
//! `VtabCtx`: p_v_table: VTableId, p_tab: Option<Box<TableBuilder>>, b_declared: bool
//! `IndexInfo` (`sqlite3_index_info`): a_constraint, a_order_by, a_constraint_usage, idx_num,
//!   idx_str, order_by_consumed, estimated_cost, estimated_rows, idx_flags, col_used
//! `ModuleCaps`: create, update, begin, sync, commit, rollback, find_function, rename,
//!   savepoint, release, rollback_to, integrity: bool
//!
//! `Parse` (derivado de `Default`): ver a struct; some `db` (as funções recebem `&mut
//! Connection`), `pParse`/`pOuterParse`, `pCleanup`, `nLabelAlloc`, `nTableLock`, `nVtabLock`.
//!
//! Decisões e desvios do C:
//!
//! * `Context` NÃO é guardado em lugar nenhum (o `P4::FuncCtx` só guarda `FuncCtx`). É montado
//!   a cada chamada de função SQL, porque carrega `db: &mut Connection`. Isso só é possível se o
//!   `Vdbe` que chama estiver FORA do slab (`Connection.stmts.take(id)` antes de executar e
//!   `put` depois), o que o contrato de `Slab::take` já previa. Quem executa o `Vdbe` empresta
//!   `v.p_aux_data` e `v.i_current_time` ao contexto (campos disjuntos de `v.a_mem`). A saída
//!   `out` é uma `Mem` PRÓPRIA: o `OP_Function` a move do registro de destino antes e devolve
//!   depois. O acumulador de agregado (`MEM_Agg`, `pMem` do C) viaja em `agg`: a chamada move de
//!   `Mem.agg` para `Context.agg` e devolve.
//! * Ganchos (`x_trace`, `x_commit_callback`, ...) são `Box<dyn FnMut>` SEM argumento de conexão
//!   (o gancho é chamado com `&mut Connection` emprestada). Os que precisam da conexão
//!   (`x_coll_needed`, `x_pre_update_callback`) recebem `&mut Connection` e são chamados com a
//!   técnica `take` do `Option`, devolvendo-os depois.
//! * `FuncDef` é `Rc` imutável e a cadeia `pNext` é o `Vec` do valor do hash (a ordem de
//!   iteração do C, que `pragma function_list` imprime). `sqlite3_create_function` sobre uma
//!   entrada existente SUBSTITUI o `Rc` na posição (o C altera no lugar): um comando já
//!   preparado continua com a função antiga, o que no C seria uma corrida.
//! * `Connection.interrupted` é `Arc<AtomicBool>` para o `sqlite3_interrupt` poder ser chamado de
//!   outro thread sem emprestar a conexão (clona-se o `Arc`).
//! * `Module.pEpoTab` vive em `Connection.a_epo_tab` (nome do módulo para a tabela eponímia):
//!   `Module` é compartilhado e imutável (`Rc`), e `sqlite3VtabEponymousTableInit` o alterava
//!   depois de criado.
//! * `VTable` mora em `Connection.vtabs` (slab) e o esquema aponta para ele por `VTableId`
//!   (assim `Table.u.vtab.p` é uma lista de `VTableId`, não de ponteiros). A instância
//!   `Box<dyn Vtab>` é mutada pelos métodos do módulo, o que exige acesso exclusivo: quem chama
//!   usa `vtabs.take(id)` e `put` (mesmo contrato do `Slab`) para passar a conexão junto.
//! * Os métodos de `Vtab` e `VtabCursor` recebem `&mut Connection` (o `fts3` e o `dbstat` rodam
//!   SQL sobre tabelas sombra) e o cursor recebe o `&mut dyn Vtab` dono (o C guarda `pVtab` no
//!   cursor). `Vtab::as_any_mut` permite o módulo recuperar o seu tipo concreto.
//! * `Connection.dbs[i].schema` é o `Schema` POR VALOR (não compartilhado). `BtShared.p_schema`
//!   fica `None`: sem cache compartilhado só há um dono.
//! * `Parse.p_toplevel` é uma posse invertida: ao codificar um gatilho (`sqlite3CodeRowTrigger`,
//!   `getRowTrigger`) o `Parse` externo é MOVIDO para dentro do `Parse` do gatilho
//!   (`child.p_toplevel = Some(Box::new(mem::take(parent)))`) e devolvido no fim
//!   (`*parent = *child.p_toplevel.take().unwrap()`). `Parse::toplevel` e `toplevel_mut`
//!   cobrem `sqlite3ParseToplevel`. Por isso `Parse` é movível, barato (só `Vec`/`Box`) e `Default`.
//! * `Parse.p_vdbe` é `Option<Box<Vdbe>>`: o `Parse` possui o `Vdbe` até `sqlite3FinishCoding`,
//!   quando ele entra em `Connection.stmts`. `Parse.p_reprepare` é o handle do comando sendo
//!   re-preparado.
//! * `Parse.p_idx_epr`, `p_idx_part_expr`: listas encadeadas do C onde o mais novo fica na
//!   frente e o código restaura o ponteiro salvo. Aqui são `Vec` onde o mais novo é o ÚLTIMO;
//!   "restaurar" é `truncate(len_salvo)`; percorrer da frente do C é percorrer em ordem inversa.
//!   `p_ainc` e `p_trigger_prg` idem. `p_rename` idem.
//! * `Parse.p_new_table` é `Box<TableBuilder>` e `Parse.p_new_index` é `Box<Index>` (a
//!   construção mutável do CONVENTIONS, item 7); fechar a tabela é `Rc::new(builder.finish())`.
//!   `Parse.p_with` é `Option<Box<With>>` e segue o protocolo mover para dentro e devolver do C
//!   (`pSavedWith = pParse->pWith; pParse->pWith = x; ...; pParse->pWith = pSavedWith`).
//! * `Parse.u1` (`addrCrTab` ou `pReturning`) vira dois campos, `addr_cr_tab` e `p_returning`.
//! * `Parse.z_tail` é o DESLOCAMENTO em bytes no SQL (o C guarda um ponteiro); `RenameToken.p`
//!   é um `usize` opaco (identidade do elemento da árvore, no `alter.rs`).
//! * O `CollSeq` de `mem.rs` não tem o estado "ainda sem função" que o C usa em `findCollSeqEntry`
//!   com `create=1` (as três entradas em branco que `sqlite3_collation_needed` preenche depois,
//!   com os índices já apontando para a mesma struct): é dúvida registrada ao fim.
//! * Sem `SQLITE_USER_AUTHENTICATION`, `SQLITE_ENABLE_UNLOCK_NOTIFY` (as ligações entre
//!   conexões), `SQLITE_ENABLE_NORMALIZE` e `SQLITE_DEBUG`/`SQLITE_COVERAGE_TEST`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::btree_types::{Btree, CursorId, Slab};
use crate::consts::SQLITE_N_LIMIT;
use crate::hash::Hash;
use crate::mem::{CollSeq, Mem};
use crate::os::{DlHandle, VfsRef};
use crate::sqlite_int::*;

/// A tabela em construção (CREATE TABLE em andamento): um `Table` mutável antes de virar `Rc`.
pub type TableBuilder = Table;
use crate::vdbe_types::{AuxData, PreUpdate, Vdbe, YDbMask};

// ---------------------------------------------------------------------------------------------
// Constantes do sqliteInt.h que a extração ainda não cobriu
// ---------------------------------------------------------------------------------------------

/// `Parse.eParseMode`: SQL normal.
pub const PARSE_MODE_NORMAL: u8 = 0;
/// `Parse.eParseMode`: dentro de `sqlite3_declare_vtab()`.
pub const PARSE_MODE_DECLARE_VTAB: u8 = 1;
/// `Parse.eParseMode`: renomeando objetos (ALTER TABLE RENAME).
pub const PARSE_MODE_RENAME: u8 = 2;
/// `Parse.eParseMode`: removendo o mapa de tokens do RENAME.
pub const PARSE_MODE_UNMAP: u8 = 3;

/// Segundo parâmetro de `sqlite3Savepoint()` e P1 do `OP_Savepoint`.
pub const SAVEPOINT_BEGIN: i32 = 0;
/// Ver `SAVEPOINT_BEGIN`.
pub const SAVEPOINT_RELEASE: i32 = 1;
/// Ver `SAVEPOINT_BEGIN`.
pub const SAVEPOINT_ROLLBACK: i32 = 2;

/// `OP_Insert`: atualiza `db->nChange`. Também P2 (não P5) do `OP_Delete`.
pub const OPFLAG_NCHANGE: u16 = 0x01;
/// `OP_VColumn`: nochange para UPDATE.
pub const OPFLAG_NOCHNG: u16 = 0x01;
/// `OP_Column`: saída efêmera é aceitável.
pub const OPFLAG_EPHEM: u16 = 0x01;
/// Atualiza `db->lastRowid`.
pub const OPFLAG_LASTROWID: u16 = 0x20;
/// Este `OP_Insert` é um UPDATE do SQL.
pub const OPFLAG_ISUPDATE: u16 = 0x04;
/// Provavelmente é um append.
pub const OPFLAG_APPEND: u16 = 0x08;
/// Tenta evitar o seek em `BtreeInsert()`.
pub const OPFLAG_USESEEKRESULT: u16 = 0x10;
/// `OP_Delete` só faz o gancho de pré-atualização.
pub const OPFLAG_ISNOOP: u16 = 0x40;
/// `OP_Column` usado só para `length()`.
pub const OPFLAG_LENGTHARG: u16 = 0x40;
/// `OP_Column` usado só para `typeof()`.
pub const OPFLAG_TYPEOFARG: u16 = 0x80;
/// `OP_Column` usado só para `octet_length()`.
pub const OPFLAG_BYTELENARG: u16 = 0xc0;
/// `OP_Open*` abre um cursor em massa.
pub const OPFLAG_BULKCSR: u16 = 0x01;
/// `OP_Open*` só faz seek por igualdade.
pub const OPFLAG_SEEKEQ: u16 = 0x02;
/// `OP_Open*` usa `BTREE_FORDELETE`.
pub const OPFLAG_FORDELETE: u16 = 0x08;
/// P2 de `OP_Open*` é um número de registro.
pub const OPFLAG_P2ISREG: u16 = 0x10;
/// `OP_Compare` usa a permutação.
pub const OPFLAG_PERMUTE: u16 = 0x01;
/// `OP_Delete`/`OP_Insert` guardam a posição do cursor.
pub const OPFLAG_SAVEPOSITION: u16 = 0x02;
/// `OP_Delete`: índice de um DELETE.
pub const OPFLAG_AUXDELETE: u16 = 0x04;
/// `OP_MakeRecord`: o tipo serial 10 é aceitável.
pub const OPFLAG_NOCHNG_MAGIC: u16 = 0x6d;
/// `OP_Insert` usa uma célula pré-formatada.
pub const OPFLAG_PREFORMAT: u16 = 0x80;

// ---------------------------------------------------------------------------------------------
// Handles e aliases de função
// ---------------------------------------------------------------------------------------------

/// Handle de um comando preparado em `Connection.stmts` (o `sqlite3_stmt*` do C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct StmtId(pub u32);

impl StmtId {
    /// A vaga do `Slab` correspondente.
    #[inline]
    pub fn slot(self) -> CursorId {
        CursorId(self.0)
    }

    /// O handle de uma vaga devolvida por `Slab::insert`.
    #[inline]
    pub fn from_slot(id: CursorId) -> StmtId {
        StmtId(id.0)
    }
}

/// Handle de um `VTable` em `Connection.vtabs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VTableId(pub u32);

impl VTableId {
    /// A vaga do `Slab` correspondente.
    #[inline]
    pub fn slot(self) -> CursorId {
        CursorId(self.0)
    }

    /// O handle de uma vaga devolvida por `Slab::insert`.
    #[inline]
    pub fn from_slot(id: CursorId) -> VTableId {
        VTableId(id.0)
    }
}

/// `void (*xSFunc)(sqlite3_context*,int,sqlite3_value**)`: função escalar ou passo de agregado.
pub type ScalarFn = fn(&mut Context<'_>, &[Mem]);

/// `void (*xFinalize)(sqlite3_context*)` e `xValue`: finalizador e valor corrente de agregado.
pub type FinalFn = fn(&mut Context<'_>);

/// Destrutor de um dado do usuário (`xDestroy`, `xDeleteAux`, `xDestructor`): recebe o dado.
pub type DestroyFn = Box<dyn FnOnce(Option<Rc<dyn Any>>)>;

/// `xCommitCallback`: devolve diferente de zero para transformar o COMMIT em ROLLBACK.
pub type CommitHook = Box<dyn FnMut() -> i32>;
/// `xRollbackCallback`.
pub type RollbackHook = Box<dyn FnMut()>;
/// `xUpdateCallback(op, zDb, zTable, rowid)`.
pub type UpdateHook = Box<dyn FnMut(i32, &[u8], &[u8], i64)>;
/// `xWalCallback(zDb, nPage)`.
pub type WalHook = Box<dyn FnMut(&[u8], i32) -> i32>;
/// `xAutovacPages(zDb, nDbPage, nFreePage, nBytePerPage)`.
pub type AutovacPagesFn = Box<dyn FnMut(&[u8], u32, u32, u32) -> u32>;
/// `xProgress`: diferente de zero interrompe.
pub type ProgressFn = Box<dyn FnMut() -> i32>;
/// `xAuth(action, a1, a2, a3, a4)`.
pub type AuthFn =
    Box<dyn FnMut(i32, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>, Option<&[u8]>) -> i32>;
/// `xPreUpdateCallback`.
pub type PreUpdateFn = Box<dyn FnMut(&mut Connection, &mut PreUpdate)>;
/// `xCollNeeded` e `xCollNeeded16` (`Connection.coll_needed_16` diz qual): recebe a codificação
/// e o nome (em UTF-8 ou UTF-16 conforme o gancho).
pub type CollNeededFn = Box<dyn FnMut(&mut Connection, i32, &[u8])>;
/// `xProfile(zSql, ns)`.
pub type ProfileFn = Box<dyn FnMut(&[u8], u64)>;
/// `trace.xLegacy` e `trace.xV2`: recebe o evento.
pub type TraceFn = Box<dyn FnMut(&TraceEvent)>;

/// O que o gancho de `sqlite3_trace_v2` recebe (`SQLITE_TRACE_STMT`, `PROFILE`, `ROW`, `CLOSE`).
pub enum TraceEvent {
    /// Início do comando, com o SQL expandido.
    Stmt { stmt: StmtId, sql: Vec<u8> },
    /// Fim do comando, com o tempo em nanossegundos.
    Profile { stmt: StmtId, ns: i64 },
    /// Uma linha de resultado.
    Row { stmt: StmtId },
    /// A conexão fechou.
    Close,
}

// ---------------------------------------------------------------------------------------------
// Funções SQL
// ---------------------------------------------------------------------------------------------

/// O `void *pUserData` de uma função: nada, um inteiro (`SQLITE_INT_TO_PTR`) ou um dado.
#[derive(Clone, Default)]
pub enum UserData {
    /// Ponteiro nulo.
    #[default]
    None,
    /// Inteiro embutido no ponteiro, como em `FUNCTION(abs, 1, 0, 0, absFunc)`.
    Int(isize),
    /// Dado do usuário.
    Ptr(Rc<dyn Any>),
}

/// `struct FuncDestructor`: o destrutor de `create_function_v2`, compartilhado pelos até três
/// `FuncDef` de uma chamada (`nRef` é a contagem do `Rc`).
#[derive(Default)]
pub struct FuncDestructor {
    /// O dado do usuário.
    pub p_user_data: Option<Rc<dyn Any>>,
    /// `xDestroy`, chamado quando o último `FuncDef` solta o destrutor.
    pub x_destroy: Option<DestroyFn>,
}

impl Drop for FuncDestructor {
    fn drop(&mut self) {
        if let Some(destroy) = self.x_destroy.take() {
            destroy(self.p_user_data.take());
        }
    }
}

/// `struct FuncDef`: a definição de uma função SQL.
#[derive(Default)]
pub struct FuncDef {
    /// Número de argumentos; -1 é ilimitado.
    pub n_arg: i8,
    /// Combinação de `SQLITE_FUNC_*` (e a codificação nos dois bits baixos).
    pub func_flags: u32,
    /// `pUserData`.
    pub p_user_data: UserData,
    /// Função escalar ou passo do agregado.
    pub x_s_func: Option<ScalarFn>,
    /// Finalizador do agregado.
    pub x_finalize: Option<FinalFn>,
    /// Valor corrente do agregado de janela.
    pub x_value: Option<FinalFn>,
    /// Passo inverso do agregado de janela.
    pub x_inverse: Option<ScalarFn>,
    /// Nome SQL da função.
    pub z_name: Vec<u8>,
    /// Destrutor por referência contada (funções do usuário).
    pub p_destructor: Option<Rc<FuncDestructor>>,
}

/// O que o `P4_FUNCCTX` guarda: o suficiente para montar um `Context` a cada chamada.
pub struct FuncCtx {
    /// A função.
    pub p_func: Rc<FuncDef>,
    /// Número de argumentos.
    pub argc: u8,
}

/// `struct sqlite3_context`: o primeiro argumento de toda função SQL.
pub struct Context<'a> {
    /// A conexão (`sqlite3_context_db_handle`).
    pub db: &'a mut Connection,
    /// Lista de auxdata do `Vdbe` (`pVdbe->pAuxData`).
    pub p_aux_data: &'a mut Vec<AuxData>,
    /// `pVdbe->iCurrentTime`.
    pub i_current_time: &'a mut i64,
    /// Onde a função grava o resultado.
    pub out: Mem,
    /// A função em execução.
    pub arg_func: Rc<FuncDef>,
    /// O contexto do agregado (o que `sqlite3_aggregate_context` entrega).
    pub agg: Option<Box<dyn Any>>,
    /// Número da instrução `OP_Function`.
    pub i_op: i32,
    /// A instrução é `OP_PureFunc` (para `sqlite3NotPureFunc`).
    pub is_pure_func: bool,
    /// Código de erro devolvido pela função.
    pub is_error: i32,
    /// Codificação dos resultados.
    pub enc: u8,
    /// Verdadeiro para pular a carga do acumulador.
    pub skip_flag: u8,
    /// Número de argumentos.
    pub argc: u8,
}

// ---------------------------------------------------------------------------------------------
// Tabelas virtuais
// ---------------------------------------------------------------------------------------------

/// Uma restrição do WHERE em `sqlite3_index_info.aConstraint`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexConstraint {
    /// Coluna restringida; -1 é o rowid.
    pub i_column: i32,
    /// Operador (`SQLITE_INDEX_CONSTRAINT_*`).
    pub op: u8,
    /// Verdadeiro se a restrição é utilizável.
    pub usable: bool,
    /// Uso interno; o `xBestIndex` ignora.
    pub i_term_offset: i32,
}

/// Um termo do ORDER BY em `sqlite3_index_info.aOrderBy`.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexOrderBy {
    /// Número da coluna.
    pub i_column: i32,
    /// Verdadeiro em DESC.
    pub desc: bool,
}

/// A saída por restrição de `xBestIndex` (`aConstraintUsage`).
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexConstraintUsage {
    /// Se positivo, a restrição é o argumento `argvIndex` do `xFilter`.
    pub argv_index: i32,
    /// Não codificar um teste para a restrição.
    pub omit: bool,
}

/// `struct sqlite3_index_info`.
#[derive(Debug, Clone, Default)]
pub struct IndexInfo {
    /// Entrada: restrições do WHERE.
    pub a_constraint: Vec<IndexConstraint>,
    /// Entrada: o ORDER BY.
    pub a_order_by: Vec<IndexOrderBy>,
    /// Saída: uma entrada por restrição.
    pub a_constraint_usage: Vec<IndexConstraintUsage>,
    /// Número que identifica o índice.
    pub idx_num: i32,
    /// Cadeia do índice (`idxStr`); `needToFreeIdxStr` some (é a posse do `Vec`).
    pub idx_str: Option<Vec<u8>>,
    /// Verdadeiro se a saída já está ordenada.
    pub order_by_consumed: i32,
    /// Custo estimado.
    pub estimated_cost: f64,
    /// Linhas estimadas (3.8.2).
    pub estimated_rows: i64,
    /// Máscara de `SQLITE_INDEX_SCAN_*` (3.9.0).
    pub idx_flags: i32,
    /// Entrada: máscara das colunas usadas pelo comando (3.10.0).
    pub col_used: u64,
}

/// Quais métodos opcionais um `VtabModule` implementa (ponteiros nulos no `sqlite3_module`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ModuleCaps {
    /// `xCreate` não nulo (nulo é tabela só eponímia).
    pub create: bool,
    /// `xUpdate` não nulo (nulo é somente leitura).
    pub update: bool,
    /// `xBegin`.
    pub begin: bool,
    /// `xSync`.
    pub sync: bool,
    /// `xCommit`.
    pub commit: bool,
    /// `xRollback`.
    pub rollback: bool,
    /// `xFindFunction`.
    pub find_function: bool,
    /// `xRename`.
    pub rename: bool,
    /// `xSavepoint`.
    pub savepoint: bool,
    /// `xRelease`.
    pub release: bool,
    /// `xRollbackTo`.
    pub rollback_to: bool,
    /// `xIntegrity` (versão 4).
    pub integrity: bool,
}

/// `sqlite3_module`: o módulo de tabela virtual. Os códigos de retorno são os `SQLITE_*` do C.
pub trait VtabModule {
    /// `iVersion`.
    fn i_version(&self) -> i32 {
        1
    }

    /// Os métodos opcionais presentes.
    fn caps(&self) -> ModuleCaps;

    /// Verdadeiro se `xCreate == xConnect` no C (tabela somente leitura ou eponímia sem estado).
    fn create_is_connect(&self) -> bool {
        false
    }

    /// `xCreate`: cria a tabela virtual. `argv` é `[módulo, banco, tabela, argumentos...]`.
    fn x_create(
        &self,
        _db: &mut Connection,
        _aux: &Option<Rc<dyn Any>>,
        _argv: &[Vec<u8>],
        _err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        Err(crate::consts::SQLITE_ERROR)
    }

    /// `xConnect`: conecta a uma tabela virtual existente.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32>;

    /// `xShadowName` (versão 3): verdadeiro se `suffix` nomeia uma tabela sombra.
    fn x_shadow_name(&self, _suffix: &[u8]) -> bool {
        false
    }
}

/// `sqlite3_vtab`: uma instância de tabela virtual. A mensagem de erro (`zErrMsg`) é do
/// implementador, acessada por `err_msg_mut`.
pub trait Vtab {
    /// O próprio objeto como `Any`, para o módulo recuperar o tipo concreto.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// `zErrMsg`.
    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>>;

    /// `xBestIndex`.
    fn best_index(&mut self, db: &mut Connection, info: &mut IndexInfo) -> i32;

    /// `xDisconnect`.
    fn disconnect(&mut self, db: &mut Connection) -> i32;

    /// `xDestroy`.
    fn destroy(&mut self, db: &mut Connection) -> i32;

    /// `xOpen`.
    fn open(&mut self, db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32>;

    /// `xUpdate`: `args` é o vetor do `OP_VUpdate` e `rowid` a saída.
    fn update(&mut self, _db: &mut Connection, _args: &[Mem], _rowid: &mut i64) -> i32 {
        crate::consts::SQLITE_READONLY
    }

    /// `xBegin`.
    fn begin(&mut self, _db: &mut Connection) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xSync`.
    fn sync(&mut self, _db: &mut Connection) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xCommit`.
    fn commit(&mut self, _db: &mut Connection) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xRollback`.
    fn rollback(&mut self, _db: &mut Connection) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xFindFunction`: devolve o código do operador (0 se não sobrecarrega) e a função.
    fn find_function(
        &mut self,
        _db: &mut Connection,
        _n_arg: i32,
        _name: &[u8],
        _out: &mut Option<(ScalarFn, UserData)>,
    ) -> i32 {
        0
    }

    /// `xRename`.
    fn rename(&mut self, _db: &mut Connection, _new_name: &[u8]) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xSavepoint`.
    fn savepoint(&mut self, _db: &mut Connection, _n: i32) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xRelease`.
    fn release(&mut self, _db: &mut Connection, _n: i32) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xRollbackTo`.
    fn rollback_to(&mut self, _db: &mut Connection, _n: i32) -> i32 {
        crate::consts::SQLITE_OK
    }

    /// `xIntegrity` (versão 4).
    fn integrity(
        &mut self,
        _db: &mut Connection,
        _schema: &[u8],
        _table: &[u8],
        _flags: i32,
        _err: &mut Option<Vec<u8>>,
    ) -> i32 {
        crate::consts::SQLITE_OK
    }
}

/// `sqlite3_vtab_cursor`: um cursor de tabela virtual. `vtab` é a instância dona, que o C
/// alcança por `pCursor->pVtab`.
pub trait VtabCursor {
    /// `xClose`.
    fn close(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32;

    /// `xFilter`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32;

    /// `xNext`.
    fn next(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32;

    /// `xEof`: diferente de zero no fim.
    fn eof(&mut self, vtab: &mut dyn Vtab) -> i32;

    /// `xColumn`: grava a coluna `i` em `ctx.out`.
    fn column(&mut self, vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32;

    /// `xRowid`.
    fn rowid(&mut self, vtab: &mut dyn Vtab, rowid: &mut i64) -> i32;
}

/// `struct Module`: um módulo de tabela virtual registrado (`sqlite3_create_module`).
pub struct Module {
    /// O módulo (`pModule`).
    pub p_module: Rc<dyn VtabModule>,
    /// Nome passado a `create_module()`.
    pub z_name: Vec<u8>,
    /// `pAux`.
    pub p_aux: Option<Rc<dyn Any>>,
    /// Destrutor do módulo, chamado quando a última referência (`Rc`) se desfaz.
    pub x_destroy: Option<DestroyFn>,
}

impl Drop for Module {
    fn drop(&mut self) {
        if let Some(destroy) = self.x_destroy.take() {
            destroy(self.p_aux.take());
        }
    }
}

/// `struct VTable`: uma conexão a uma tabela virtual (sem `Default`: tem um `Rc<Module>`).
pub struct VTable {
    /// A implementação do módulo.
    pub p_mod: Rc<Module>,
    /// A instância (`sqlite3_vtab`); `None` enquanto emprestada com `take` ou não construída.
    pub p_vtab: Option<Box<dyn Vtab>>,
    /// Número de referências a este `VTable`.
    pub n_ref: i32,
    /// Cursores abertos (o `sqlite3_vtab.nRef`).
    pub n_cursor: i32,
    /// Verdadeiro se há suporte a restrições.
    pub b_constraint: u8,
    /// Verdadeiro se pode usar qualquer esquema anexado.
    pub b_all_schemas: u8,
    /// Risco de permitir acesso hostil.
    pub e_vtab_risk: u8,
    /// Profundidade da pilha de SAVEPOINT.
    pub i_savepoint: i32,
}

/// `struct VtabCtx`: o contexto de um `xCreate`/`xConnect` em curso (`Connection.p_vtab_ctx`).
#[derive(Default)]
pub struct VtabCtx {
    /// O `VTable` em construção.
    pub p_v_table: VTableId,
    /// A tabela a que a virtual pertence (movida de `Parse.p_new_table` durante o construtor).
    pub p_tab: Option<Box<TableBuilder>>,
    /// Verdadeiro depois de `sqlite3_declare_vtab()`.
    pub b_declared: bool,
}

// ---------------------------------------------------------------------------------------------
// A conexão
// ---------------------------------------------------------------------------------------------

/// `struct Db`: um banco aberto na conexão (o `aDb[i]` do C).
#[derive(Default)]
pub struct DbSlot {
    /// Nome do banco (nome do esquema, não do arquivo).
    pub z_db_s_name: Vec<u8>,
    /// A árvore-b do arquivo.
    pub bt: Option<Btree>,
    /// Quão agressivo é o sync.
    pub safety_level: u8,
    /// Verdadeiro se `PRAGMA synchronous=N` rodou.
    pub b_sync_set: bool,
    /// O esquema do banco, por valor.
    pub schema: Schema,
}

/// `struct sqlite3InitInfo`: o estado da inicialização do esquema.
#[derive(Default)]
pub struct InitInfo {
    /// Página raiz da tabela sendo inicializada.
    pub new_tnum: u32,
    /// Qual arquivo está sendo inicializado.
    pub i_db: u8,
    /// Verdadeiro durante a inicialização.
    pub busy: u8,
    /// O último comando é um gatilho TEMP órfão.
    pub orphan_trigger: bool,
    /// Construindo uma tabela impostora.
    pub imposter_table: bool,
    /// ATTACH é na verdade a reabertura via MemDB.
    pub reopen_memdb: bool,
    /// As colunas "type", "name", "tbl_name" da linha de `sqlite_schema`.
    pub az_init: Vec<Option<Vec<u8>>>,
}

/// `struct Lookaside`: o `lookaside` do C vira alocação normal; ficam só as estatísticas que
/// `sqlite3_db_status` e `sqlite3_db_config` expõem.
#[derive(Debug, Clone, Copy, Default)]
pub struct Lookaside {
    /// Só opera quando zero.
    pub b_disable: u32,
    /// Tamanho de cada buffer.
    pub sz: u16,
    /// Valor verdadeiro de `sz`, mesmo desabilitado.
    pub sz_true: u16,
    /// Número de vagas.
    pub n_slot: u32,
    /// 0: acertos; 1: erros por tamanho; 2: erros por falta de vaga.
    pub an_stat: [u32; 3],
}

/// `struct Savepoint`.
#[derive(Debug, Clone, Default)]
pub struct Savepoint {
    /// Nome do savepoint.
    pub z_name: Vec<u8>,
    /// Violações de chave estrangeira adiadas.
    pub n_deferred_cons: i64,
    /// Violações imediatas adiadas.
    pub n_deferred_imm_cons: i64,
}

/// `struct BusyHandler`. O contador é compartilhado (`Rc<Cell>`) com o gancho que a conexão
/// instala no pager (`Pager.busy_handler`), porque `sqlite3Step` o zera por fora.
#[derive(Clone, Default)]
pub struct BusyHandler {
    /// A função de ocupado: recebe o número de tentativas e devolve diferente de zero para
    /// tentar de novo.
    pub x_busy_handler: Option<Rc<dyn Fn(i32) -> i32>>,
    /// Incrementado a cada chamada.
    pub n_busy: Rc<Cell<i32>>,
}

/// `struct DbClientData`: um dado de `sqlite3_set_clientdata`.
#[derive(Default)]
pub struct DbClientData {
    /// Nome do dado.
    pub z_name: Vec<u8>,
    /// O dado.
    pub p_data: Option<Rc<dyn Any>>,
    /// Destrutor, chamado em `Drop`.
    pub x_destructor: Option<DestroyFn>,
}

impl Drop for DbClientData {
    fn drop(&mut self) {
        if let Some(destroy) = self.x_destructor.take() {
            destroy(self.p_data.take());
        }
    }
}

/// `struct sqlite3`: a conexão. Possui tudo por valor (CONVENTIONS.md, item 1).
#[derive(Default)]
pub struct Connection {
    /// A interface com o SO.
    pub p_vfs: Option<VfsRef>,
    /// Os comandos preparados.
    pub stmts: Slab<Vdbe>,
    /// A ordem de `pVdbe`: do mais antigo para o mais novo (a lista do C começa no mais novo).
    pub stmt_list: Vec<StmtId>,
    /// A colação BINARY na codificação do banco.
    pub p_dflt_coll: Option<Rc<CollSeq>>,
    /// Todos os bancos (`aDb`).
    pub dbs: Vec<DbSlot>,
    /// Flags de estado interno (`DBFLAG_*`).
    pub m_db_flags: u32,
    /// Flags ajustáveis por pragma (`SQLITE_*` de 64 bits).
    pub flags: u64,
    /// Rowid do último INSERT.
    pub last_rowid: i64,
    /// Valor padrão de `mmap_size`.
    pub sz_mmap: i64,
    /// Não zera o esquema enquanto for diferente de zero.
    pub n_schema_lock: u32,
    /// Flags passadas a `xOpen`.
    pub open_flags: u32,
    /// Último código de erro.
    pub err_code: i32,
    /// Deslocamento do erro no SQL.
    pub err_byte_offset: i32,
    /// Máscara dos códigos de resultado.
    pub err_mask: i32,
    /// `errno` do último erro do sistema.
    pub i_sys_errno: i32,
    /// Flags de otimizações (`SQLITE_QUERY_FLATTENER` e as demais).
    pub db_opt_flags: u32,
    /// Codificação de texto.
    pub enc: u8,
    /// Flag de auto-commit.
    pub auto_commit: u8,
    /// 1: arquivo; 2: memória; 0: padrão.
    pub temp_store: u8,
    /// Nunca é diferente de zero: a alocação não falha em Rust (mantido para o código traduzido).
    pub malloc_failed: u8,
    /// Modo de trava padrão dos bancos anexados.
    pub dflt_lock_mode: u8,
    /// Auto-vacuum depois de um VACUUM, se >= 0.
    pub next_autovac: i8,
    /// Não emite mensagens de erro se diferente de zero.
    pub suppress_err: u8,
    /// Valor devolvido por `sqlite3_vtab_on_conflict`.
    pub vtab_on_conflict: u8,
    /// O savepoint mais externo é uma transação.
    pub is_transaction_savepoint: u8,
    /// Zero ou mais flags `SQLITE_TRACE_*`.
    pub m_trace: u8,
    /// Sem backends de cache compartilhado.
    pub no_shared_cache: u8,
    /// `OP_SqlExec` pendentes.
    pub n_sql_exec: u8,
    /// Um dos `SQLITE_STATE_*`.
    pub e_open_state: u8,
    /// Tamanho de página depois de um VACUUM, se > 0.
    pub next_pagesize: i32,
    /// O que `sqlite3_changes()` devolve.
    pub n_change: i64,
    /// O que `sqlite3_total_changes()` devolve.
    pub n_total_change: i64,
    /// Os limites (`SQLITE_LIMIT_*`).
    pub a_limit: [i32; SQLITE_N_LIMIT as usize],
    /// Tamanho máximo de regiões mapeadas pelo ordenador.
    pub n_max_sorter_mmap: i32,
    /// Informação da inicialização.
    pub init: InitInfo,
    /// VDBEs em execução.
    pub n_vdbe_active: i32,
    /// VDBEs ativos que leem ou escrevem.
    pub n_vdbe_read: i32,
    /// VDBEs ativos que leem e escrevem.
    pub n_vdbe_write: i32,
    /// Chamadas aninhadas de `VdbeExec()`.
    pub n_vdbe_exec: i32,
    /// `OP_VDestroy` ativos.
    pub n_v_destroy: i32,
    /// Bibliotecas de extensão carregadas.
    pub a_extension: Vec<DlHandle>,
    /// Gancho de trace.
    pub x_trace: Option<TraceFn>,
    /// Gancho de perfil.
    pub x_profile: Option<ProfileFn>,
    /// Gancho de commit.
    pub x_commit_callback: Option<CommitHook>,
    /// Gancho de rollback.
    pub x_rollback_callback: Option<RollbackHook>,
    /// Gancho de atualização.
    pub x_update_callback: Option<UpdateHook>,
    /// Gancho de `autovacuum_pages`.
    pub x_autovac_pages: Option<AutovacPagesFn>,
    /// Gancho de pré-atualização.
    pub x_pre_update_callback: Option<PreUpdateFn>,
    /// Contexto do gancho de pré-atualização ativo.
    pub p_pre_update: Option<Box<PreUpdate>>,
    /// Gancho do WAL.
    pub x_wal_callback: Option<WalHook>,
    /// Gancho `collation_needed`.
    pub x_coll_needed: Option<CollNeededFn>,
    /// O gancho acima é o `xCollNeeded16` (nome em UTF-16).
    pub coll_needed_16: bool,
    /// A mensagem de erro mais recente (`pErr`), em UTF-8; `None` é o valor nulo.
    pub err_msg: Option<Vec<u8>>,
    /// `u1.isInterrupted`.
    pub interrupted: Arc<AtomicBool>,
    /// A configuração de lookaside (só estatística).
    pub lookaside: Lookaside,
    /// Função de autorização.
    pub x_auth: Option<AuthFn>,
    /// Função de progresso.
    pub x_progress: Option<ProgressFn>,
    /// Opcodes entre duas chamadas de `x_progress`.
    pub n_progress_ops: u32,
    /// Tabelas virtuais com transação aberta.
    pub a_v_trans: Vec<VTableId>,
    /// Módulos (`sqlite3_create_module`).
    pub a_module: Hash<Rc<Module>>,
    /// Tabelas eponímias por nome de módulo.
    pub a_epo_tab: Hash<Rc<Table>>,
    /// Contextos de `xCreate`/`xConnect` ativos (o topo é o corrente).
    pub p_vtab_ctx: Vec<VtabCtx>,
    /// Os `VTable` da conexão.
    pub vtabs: Slab<VTable>,
    /// Desconectar estes no próximo `sqlite3_prepare()`.
    pub p_disconnect: Vec<VTableId>,
    /// Funções da conexão, por nome (a cadeia na ordem do C).
    pub a_func: Hash<Vec<Rc<FuncDef>>>,
    /// Todas as colações, por nome (UTF-8, UTF-16LE, UTF-16BE).
    pub a_coll_seq: Hash<[Option<Rc<CollSeq>>; 3]>,
    /// Função de ocupado.
    pub busy_handler: BusyHandler,
    /// Savepoints ativos (o topo é o mais novo).
    pub p_savepoint: Vec<Savepoint>,
    /// Linhas por índice no ANALYZE.
    pub n_analysis_limit: i32,
    /// Tempo do busy handler, em ms.
    pub busy_timeout: i32,
    /// Savepoints que não são a transação.
    pub n_savepoint: i32,
    /// Transações de statement aninhadas.
    pub n_statement: i32,
    /// Restrições adiadas líquidas nesta transação.
    pub n_deferred_cons: i64,
    /// Restrições imediatas adiadas líquidas.
    pub n_deferred_imm_cons: i64,
    /// Dados do `sqlite3_set_clientdata`.
    pub p_db_data: Vec<DbClientData>,
}

impl Connection {
    /// `SCHEMA_ENC(db)`.
    #[inline]
    pub fn schema_enc(&self) -> u8 {
        self.dbs[0].schema.enc
    }

    /// `DbHasProperty(D, I, P)`.
    #[inline]
    pub fn db_has_property(&self, i: usize, p: u16) -> bool {
        (self.dbs[i].schema.schema_flags & p) == p
    }

    /// `DbHasAnyProperty(D, I, P)`.
    #[inline]
    pub fn db_has_any_property(&self, i: usize, p: u16) -> bool {
        (self.dbs[i].schema.schema_flags & p) != 0
    }

    /// `DbSetProperty(D, I, P)`.
    #[inline]
    pub fn db_set_property(&mut self, i: usize, p: u16) {
        self.dbs[i].schema.schema_flags |= p;
    }

    /// `DbClearProperty(D, I, P)`.
    #[inline]
    pub fn db_clear_property(&mut self, i: usize, p: u16) {
        self.dbs[i].schema.schema_flags &= !p;
    }

    /// `OptimizationDisabled(db, mask)`.
    #[inline]
    pub fn optimization_disabled(&self, mask: u32) -> bool {
        (self.db_opt_flags & mask) != 0
    }

    /// `OptimizationEnabled(db, mask)`.
    #[inline]
    pub fn optimization_enabled(&self, mask: u32) -> bool {
        (self.db_opt_flags & mask) == 0
    }

    /// O comando do handle, se existir e não estiver emprestado por `take`.
    #[inline]
    pub fn stmt(&self, id: StmtId) -> Option<&Vdbe> {
        self.stmts.get(id.slot())
    }

    /// O comando do handle, para alterar.
    #[inline]
    pub fn stmt_mut(&mut self, id: StmtId) -> Option<&mut Vdbe> {
        self.stmts.get_mut(id.slot())
    }
}

// ---------------------------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------------------------

/// `struct TableLock`: uma trava de tabela exigida pelo comando (cache compartilhado).
#[derive(Debug, Clone, Default)]
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

/// `struct AutoincInfo`: o que o gerador de código sabe sobre o contador AUTOINCREMENT.
#[derive(Default)]
pub struct AutoincInfo {
    /// A tabela.
    pub p_tab: Option<Rc<Table>>,
    /// Índice em `Connection.dbs` do banco da tabela.
    pub i_db: i32,
    /// Registrador com o contador de rowid.
    pub reg_ctr: i32,
}

/// `struct TriggerPrg`: o programa de um gatilho que pode disparar durante a compilação.
#[derive(Default)]
pub struct TriggerPrg {
    /// O gatilho de onde o programa foi gerado.
    pub p_trigger: Option<Rc<Trigger>>,
    /// O subprograma para `p_trigger` e `orconf`.
    pub p_program: Option<Rc<crate::vdbe_types::SubProgram>>,
    /// Política padrão de ON CONFLICT.
    pub orconf: i32,
    /// Máscaras de colunas `old.*` e `new.*` acessadas.
    pub a_colmask: [u32; 2],
}

/// `struct IndexedExpr`: uma expressão de índice que pode ser lida do próprio índice.
#[derive(Default)]
pub struct IndexedExpr {
    /// A expressão contida no índice (cópia).
    pub p_expr: Option<Box<Expr>>,
    /// Cursor de dados associado ao índice.
    pub i_data_cur: i32,
    /// Cursor do índice.
    pub i_idx_cur: i32,
    /// Coluna do índice que guarda o valor de `p_expr`.
    pub i_idx_col: i32,
    /// Verdadeiro se é preciso um `OP_IfNullRow`.
    pub b_maybe_null_row: bool,
    /// Afinidade de `p_expr`.
    pub aff: u8,
    /// Nome do índice, só para comentários do bytecode.
    pub z_idx_name: Option<Vec<u8>>,
}

/// `struct RenameToken`: um token sujeito a renomeação por ALTER TABLE.
#[derive(Default)]
pub struct RenameToken {
    /// Identidade do elemento da árvore criado pelo token `t` (opaca, ver `alter.rs`).
    pub p: usize,
    /// O token que criou o elemento.
    pub t: Token,
}

/// `struct AuthContext`: salva `Parse.z_auth_context` para restaurar depois (o `pParse` some).
#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    /// O valor salvo de `Parse.z_auth_context`.
    pub z_auth_context: Option<Vec<u8>>,
}

/// `struct Parse`: o contexto de um analisador SQL. A `Connection` NÃO é um campo: as funções
/// recebem `&mut Connection` por parâmetro.
#[derive(Default)]
pub struct Parse {
    /// Mensagem de erro.
    pub z_err_msg: Option<Vec<u8>>,
    /// A máquina virtual em construção.
    pub p_vdbe: Option<Box<Vdbe>>,
    /// Código de retorno da execução.
    pub rc: i32,
    /// Verdadeiro depois que `OP_ColumnName` foi emitido.
    pub col_names_set: u8,
    /// Causa uma checagem do cookie do esquema depois de um erro.
    pub check_schema: u8,
    /// Chamadas aninhadas ao analisador ou gerador de código.
    pub nested: u8,
    /// Registradores temporários em `a_temp_reg`.
    pub n_temp_reg: u8,
    /// O comando pode alterar ou inserir várias linhas.
    pub is_multi_write: u8,
    /// O comando pode lançar uma exceção ABORT.
    pub may_abort: u8,
    /// Precisa chamar `convertCompoundSelectToSubquery()`.
    pub has_compound: u8,
    /// Pode fatorar expressões constantes.
    pub ok_const_factor: u8,
    /// Quantas vezes o lookaside foi desabilitado.
    pub disable_lookaside: u8,
    /// Flags `SQLITE_PREPARE_*`.
    pub prep_flags: u8,
    /// Nível de aninhamento de sub-rotinas do corpo do RIGHT JOIN.
    pub within_rj_subrtn: u8,
    /// O comando contém WITH.
    pub b_has_with: u8,
    /// Tamanho do bloco de registradores temporários.
    pub n_range_reg: i32,
    /// Primeiro registrador do bloco temporário.
    pub i_range_reg: i32,
    /// Número de erros vistos.
    pub n_err: i32,
    /// Cursores do VDBE já alocados.
    pub n_tab: i32,
    /// Células de memória usadas até agora.
    pub n_mem: i32,
    /// Bytes alocados para `Vdbe.a_op`.
    pub sz_op_alloc: i32,
    /// Tabela de um índice sobre expressão, ou o negativo do registrador base (CHECK).
    pub i_self_tab: i32,
    /// O NEGATIVO do número de rótulos usados.
    pub n_label: i32,
    /// Os rótulos.
    pub a_label: Vec<i32>,
    /// Expressões constantes.
    pub p_const_expr: Option<Box<ExprList>>,
    /// Expressões usadas pelos índices ativos.
    pub p_idx_epr: Vec<IndexedExpr>,
    /// Expressões restringidas pelas cláusulas WHERE dos índices.
    pub p_idx_part_expr: Vec<IndexedExpr>,
    /// Nome da restrição sendo analisada.
    pub constraint_name: Token,
    /// Bancos em que iniciar uma transação de escrita.
    pub write_mask: YDbMask,
    /// Bancos cujo cookie de esquema foi verificado.
    pub cookie_mask: YDbMask,
    /// Registrador com o rowid da entrada do CREATE TABLE.
    pub reg_rowid: i32,
    /// Registrador com o número da página raiz dos novos objetos.
    pub reg_root: i32,
    /// Máximo de argumentos passados a funções do usuário por um subprograma.
    pub n_max_arg: i32,
    /// Número de SELECTs (contador de `Select.sel_id`).
    pub n_select: i32,
    /// Passos de `xProgress` durante o `sqlite3_prepare()`.
    pub n_progress_steps: u32,
    /// Travas de tabela exigidas (modo de cache compartilhado).
    pub a_table_lock: Vec<TableLock>,
    /// Informação dos contadores AUTOINCREMENT.
    pub p_ainc: Vec<AutoincInfo>,
    /// O `Parse` do programa principal (posse invertida, ver as decisões).
    pub p_toplevel: Option<Box<Parse>>,
    /// Tabela para a qual os gatilhos estão sendo codificados.
    pub p_trigger_tab: Option<Rc<Table>>,
    /// Gatilhos já codificados.
    pub p_trigger_prg: Vec<TriggerPrg>,
    /// Endereço do `OP_CreateBtree` do CREATE TABLE (`u1.addrCrTab`).
    pub addr_cr_tab: i32,
    /// A cláusula RETURNING (`u1.pReturning`).
    pub p_returning: Option<Box<Returning>>,
    /// Máscara de colunas `old.*` referenciadas.
    pub oldmask: u32,
    /// Máscara de colunas `new.*` referenciadas.
    pub newmask: u32,
    /// Estimativa de iterações de uma consulta (`LogEst`, 10*log2(N)).
    pub n_query_loop: i16,
    /// `TK_UPDATE`, `TK_INSERT` ou `TK_DELETE`.
    pub e_trigger_op: u8,
    /// Codificando um gatilho de RETURNING.
    pub b_returning: u8,
    /// Política ON CONFLICT padrão dos passos do gatilho.
    pub e_orconf: u8,
    /// Verdadeiro para desabilitar gatilhos.
    pub disable_triggers: u8,
    /// Registradores temporários.
    pub a_temp_reg: [i32; 8],
    /// Token com o nome não qualificado do objeto do esquema.
    pub s_name_token: Token,
    /// O último token lido.
    pub s_last_token: Token,
    /// Número de variáveis `?` vistas até agora.
    pub n_var: i32,
    /// ASC ou DESC da INTEGER PRIMARY KEY.
    pub i_pk_sort_order: u8,
    /// Verdadeiro se a consulta tem EXPLAIN.
    pub explain: u8,
    /// Um dos `PARSE_MODE_*`.
    pub e_parse_mode: u8,
    /// Altura da árvore de expressão do sub-select corrente.
    pub n_height: i32,
    /// Endereço do `OP_Explain` corrente.
    pub addr_explain: i32,
    /// Mapa entre nomes de variáveis e números.
    pub p_v_list: Vec<crate::vdbe_types::VListEntry>,
    /// A VM sendo re-preparada (`sqlite3Reprepare()`).
    pub p_reprepare: Option<StmtId>,
    /// Deslocamento, no SQL, do texto depois do último ponto e vírgula analisado.
    pub z_tail: usize,
    /// A tabela sendo construída por CREATE TABLE.
    pub p_new_table: Option<Box<TableBuilder>>,
    /// O índice sendo construído por CREATE INDEX (e UNIQUEs redundantes no RENAME COLUMN).
    pub p_new_index: Option<Box<Index>>,
    /// O gatilho sendo construído por CREATE TRIGGER.
    pub p_new_trigger: Option<Box<Trigger>>,
    /// O sexto parâmetro dos ganchos `xAuth`.
    pub z_auth_context: Option<Vec<u8>>,
    /// Texto completo de um argumento de módulo.
    pub s_arg: Token,
    /// Tabelas virtuais a travar.
    pub ap_vtab_lock: Vec<Rc<Table>>,
    /// A cláusula WITH corrente.
    pub p_with: Option<Box<With>>,
    /// Tokens sujeitos a renomeação por ALTER TABLE (o mais novo é o último).
    pub p_rename: Vec<RenameToken>,
}

impl Parse {
    /// `sqlite3ParseToplevel(p)`.
    #[inline]
    pub fn toplevel(&self) -> &Parse {
        match &self.p_toplevel {
            Some(top) => top,
            None => self,
        }
    }

    /// `sqlite3ParseToplevel(p)`, para alterar.
    #[inline]
    pub fn toplevel_mut(&mut self) -> &mut Parse {
        if self.p_toplevel.is_some() {
            self.p_toplevel.as_mut().map(|b| &mut **b).unwrap()
        } else {
            self
        }
    }

    /// `IN_DECLARE_VTAB`.
    #[inline]
    pub fn in_declare_vtab(&self) -> bool {
        self.e_parse_mode == PARSE_MODE_DECLARE_VTAB
    }

    /// `IN_RENAME_OBJECT`.
    #[inline]
    pub fn in_rename_object(&self) -> bool {
        self.e_parse_mode >= PARSE_MODE_RENAME
    }

    /// `IN_SPECIAL_PARSE`.
    #[inline]
    pub fn in_special_parse(&self) -> bool {
        self.e_parse_mode != PARSE_MODE_NORMAL
    }
}

// ---------------------------------------------------------------------------------------------
// Companheiros de vdbeblob.c e backup.c
// ---------------------------------------------------------------------------------------------

/// `struct Incrblob` (vdbeblob.c): o `sqlite3_blob`. O cursor de btree é o cursor 0 do comando
/// `p_stmt`, alcançado por `Connection.stmts`.
pub struct Incrblob {
    /// Tamanho do blob aberto.
    pub n_byte: i32,
    /// Deslocamento do blob nos dados do cursor.
    pub i_offset: i32,
    /// Coluna da tabela em que o handle está aberto.
    pub i_col: u16,
    /// O comando que mantém o cursor aberto.
    pub p_stmt: StmtId,
    /// Nome do banco.
    pub z_db: Vec<u8>,
    /// A tabela.
    pub p_tab: Rc<Table>,
}

/// `struct sqlite3_backup` (backup.c). `pDestDb`/`pSrcDb` e `pDest`/`pSrc` viram índices em
/// `Connection.dbs` das duas conexões, que as funções recebem por parâmetro.
#[derive(Debug, Clone, Default)]
pub struct Sqlite3Backup {
    /// Índice do banco destino em `dbs` da conexão destino.
    pub dest_db_index: i32,
    /// Cookie original do esquema do destino.
    pub i_dest_schema: u32,
    /// Verdadeiro quando há transação de escrita no destino.
    pub b_dest_locked: bool,
    /// Próxima página de origem a copiar.
    pub i_next: u32,
    /// Índice do banco origem em `dbs` da conexão origem.
    pub src_db_index: i32,
    /// Código de erro do backup.
    pub rc: i32,
    /// Páginas que faltam (atualizado por `backup_step`).
    pub n_remaining: u32,
    /// Total de páginas a copiar.
    pub n_pagecount: u32,
    /// Verdadeiro depois que o backup foi registrado no pager.
    pub is_attached: bool,
}
