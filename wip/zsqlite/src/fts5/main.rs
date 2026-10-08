//! `fts5_main.c`: o módulo virtual `fts5`: a tabela (`Fts5FullTable`), os cursores, o
//! `xBestIndex`, o `xFilter`, o `xUpdate`, os savepoints, o `Fts5Global` (que implementa
//! [`Fts5Api`] e [`Fts5GlobalApi`]), a API de extensão das funções auxiliares (o cursor, aqui o
//! [`CsrApi`]), as funções SQL `fts5()` e `fts5_source_id()` e [`fts5_init`].
//!
//! Desvios do C, decorrentes do modelo v2 (ver `mod.rs` e `CONVENTIONS.md`):
//!
//! * **O estado do cursor mora na tabela.** O `Fts5Cursor` do C é alcançado de duas maneiras: pelo
//!   `sqlite3_vtab_cursor*` que o núcleo guarda e pela lista `Fts5Global.pCsr`, que a função SQL
//!   auxiliar (`bm25(tbl)`) usa para achar o cursor pelo id que a coluna oculta devolveu
//!   (`fts5CursorFromCsrid`). A segunda é inalcançável a partir do VDBE em execução, então o
//!   [`Fts5Cursor`] vive em `Fts5FullTable.cursors` e o cursor do núcleo é só o [`Fts5CsrHandle`]
//!   (o id). A busca por id percorre `db.vtabs` ([`fts5_table_from_csrid`]); o id é único na
//!   conexão porque sai do contador do [`Fts5Global`].
//! * **Cursor emprestado.** Enquanto uma operação roda, o cursor sai de `Fts5FullTable.cursors`
//!   ([`with_csr`]), o que dá `&mut Fts5FullTable` e `&mut Fts5Cursor` ao mesmo tempo.
//!   `fts5NewTransaction` e `fts5TripCursors` só rodam fora de qualquer operação de cursor.
//! * **O `Fts5ExtensionApi` não é o cursor.** É o [`CsrApi`], que empresta a conexão, a tabela e
//!   o cursor. O resultado da função auxiliar volta como [`Fts5AuxResult`] e quem chamou o aplica
//!   ao `sqlite3_context` (a API e o contexto precisariam da mesma conexão ao mesmo tempo).
//! * **O ordenador de `ORDER BY rank` não roda um comando aninhado.** O C prepara
//!   `SELECT rowid, rank FROM tbl ORDER BY bm25(tbl)` e o executa dentro do `xFilter`, o que
//!   reentra na mesma tabela virtual (com a instância retirada do `VTable` pelo núcleo isso não é
//!   possível). [`csr_first_sorted`] faz o mesmo trabalho sem o comando: percorre a expressão com
//!   um cursor de origem (o `FTS5_PLAN_SOURCE`), chama a função de rank em cada linha, guarda
//!   `(chave, rowid, blob de poslists)` e ordena de forma estável pela chave (o ordenador do
//!   VDBE mantém a ordem de inserção nos empates, que aqui é a ordem crescente de rowid). Os
//!   argumentos da função (`apRankArg`, literais) são avaliados uma vez por `SELECT`.
//! * `xRowid` não recebe a conexão; nos planos `SCAN` e `ROWID` o rowid da linha corrente é lido
//!   da coluna 0 do comando a cada `xNext` e guardado no cursor.
//! * `sqlite3Fts5ConfigFree` e o `xDelete` dos dados auxiliares são o `Drop`. O `xDestroy` do
//!   módulo é o `Drop` do `Fts5Global`.
//! * `sqlite3_overload_function` (`fts5CreateAux`) precisa da conexão, que o `Fts5Api` não tem:
//!   [`fts5_init`] sobrecarrega os nomes registrados logo depois de montar o `Fts5Global`.
//! * `fts5()` (`fts5Fts5Func`) grava o `Fts5Global` num [`Fts5ApiSlot`] passado como valor
//!   ponteiro `"fts5_api_ptr"` (no C, o endereço do `fts5_api`).
//! * Sem `SQLITE_DEBUG` somem `fts5CheckTransactionState`, `ts` e `sqlite3_fts5_may_be_corrupt`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{
    Connection, Context, IndexConstraintUsage, IndexInfo, ModuleCaps, ScalarFn, StmtId, UserData,
    VTableId, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    SQLITE_CORRUPT, SQLITE_DETERMINISTIC, SQLITE_DONE, SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ,
    SQLITE_INDEX_CONSTRAINT_GE, SQLITE_INDEX_CONSTRAINT_GLOB, SQLITE_INDEX_CONSTRAINT_GT,
    SQLITE_INDEX_CONSTRAINT_LE, SQLITE_INDEX_CONSTRAINT_LIKE, SQLITE_INDEX_CONSTRAINT_LT,
    SQLITE_INDEX_CONSTRAINT_MATCH, SQLITE_INDEX_SCAN_UNIQUE, SQLITE_INNOCUOUS, SQLITE_INTEGER,
    SQLITE_MISMATCH, SQLITE_NULL, SQLITE_OK, SQLITE_PREPARE_PERSISTENT, SQLITE_RANGE,
    SQLITE_REPLACE, SQLITE_ROW, SQLITE_UTF8, SQLITE_VTAB_CONSTRAINT_SUPPORT, SQLITE_VTAB_INNOCUOUS,
};
use crate::main::{create_function_api, errmsg, overload_function, SQLITE_SOURCE_ID};
use crate::mem::{value_numeric_type, value_type, Mem, StrDtor};
use crate::mem2::value_pointer;
use crate::prepare::prepare_v3;
use crate::printf::{mprintf, PrintfArg};
use crate::util::{at, err_str, str_icmp, strnicmp};
use crate::vdbeapi::{
    bind_int64, bind_value, column_count, column_int64, column_text, column_value, finalize,
    reset, result_double, result_error, result_error_code, result_int64, result_text,
    result_value, step, text_of, user_data, value_int, value_int64, vtab_nochange,
};
use crate::vtab::{create_module, vtab_config, vtab_on_conflict, with_vtab};

use super::aux::fts5_aux_init;
use super::buffer::{
    fts5_pos2column, fts5_pos2offset, Fts5Buffer, Fts5PoslistReader,
};
use super::config::{fts5_config_parse, fts5_config_parse_rank};
use super::expr::{
    fts5_expr_and, fts5_expr_new, fts5_expr_pattern, Fts5Expr,
};
use super::index::Fts5Index;
use super::index3::fts5_index_init;
use super::int::{
    Fts5Api, Fts5ApiTokenFn, Fts5AuxResult, Fts5Config, Fts5ExtensionApi, Fts5ExtensionFunction,
    Fts5GlobalApi, Fts5PhraseIter, Fts5TokenizerFactory, FTS5_CONTENT_NONE, FTS5_CONTENT_NORMAL,
    FTS5_CORRUPT, FTS5_CURRENT_VERSION, FTS5_CURRENT_VERSION_SECUREDELETE, FTS5_DEFAULT_RANK,
    FTS5_DETAIL_COLUMNS, FTS5_DETAIL_FULL, FTS5_PATTERN_GLOB, FTS5_PATTERN_LIKE,
    FTS5_STMT_LOOKUP, FTS5_STMT_SCAN_ASC, FTS5_STMT_SCAN_DESC, FTS5_TOKENIZE_AUX,
    FTS5_TOKEN_COLOCATED,
};
use super::storage::{fts5_drop_all, Fts5Storage};
use super::tokenize::fts5_tokenizer_init;
use super::varint::fts5_get_varint32;
use super::vocab::fts5_vocab_init;

// ---------------------------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------------------------

/// `FTS5_BI_*`: bits de `idxNum` que `xBestIndex` passa a `xFilter`.
const FTS5_BI_ORDER_RANK: i32 = 0x0020;
const FTS5_BI_ORDER_ROWID: i32 = 0x0040;
const FTS5_BI_ORDER_DESC: i32 = 0x0080;

/// `Fts5Cursor.csrflags`.
const FTS5CSR_EOF: i32 = 0x01;
const FTS5CSR_REQUIRE_CONTENT: i32 = 0x02;
const FTS5CSR_REQUIRE_DOCSIZE: i32 = 0x04;
const FTS5CSR_REQUIRE_INST: i32 = 0x08;
const FTS5CSR_REQUIRE_RESEEK: i32 = 0x20;
const FTS5CSR_REQUIRE_POSLIST: i32 = 0x40;
/// O que `fts5CsrNewrow` liga.
const FTS5CSR_NEWROW: i32 = FTS5CSR_REQUIRE_CONTENT
    | FTS5CSR_REQUIRE_DOCSIZE
    | FTS5CSR_REQUIRE_INST
    | FTS5CSR_REQUIRE_POSLIST;

/// `FTS5_PLAN_*`: os planos de consulta.
const FTS5_PLAN_MATCH: i32 = 1;
const FTS5_PLAN_SOURCE: i32 = 2;
const FTS5_PLAN_SPECIAL: i32 = 3;
const FTS5_PLAN_SORTED_MATCH: i32 = 4;
const FTS5_PLAN_SCAN: i32 = 5;
const FTS5_PLAN_ROWID: i32 = 6;

const LARGEST_INT64: i64 = i64::MAX;
const SMALLEST_INT64: i64 = i64::MIN;

// ---------------------------------------------------------------------------------------------
// Fts5Global: os tokenizadores e funções registrados (um por conexão)
// ---------------------------------------------------------------------------------------------

/// `Fts5Auxiliary`: uma função auxiliar registrada.
struct Fts5Auxiliary {
    /// O nome (`zFunc`).
    z_func: Vec<u8>,
    /// A implementação.
    x_func: Rc<dyn Fts5ExtensionFunction>,
}

/// `Fts5TokenizerModule`: um tipo de tokenizador registrado.
struct Fts5TokenizerModule {
    /// O nome.
    z_name: Vec<u8>,
    /// O construtor.
    factory: Rc<dyn Fts5TokenizerFactory>,
}

/// `Fts5Global`: criado quando o módulo é registrado e compartilhado pelas tabelas da conexão. Os
/// registros só mudam na montagem (o `Fts5Api` pede `&mut self`); depois disso só o contador de
/// ids de cursor muda.
#[derive(Default)]
pub struct Fts5Global {
    /// `iNextId`: gera os ids únicos de cursor.
    i_next_id: Cell<i64>,
    /// `pAux`, na ordem de registro (a busca vai do mais recente ao mais antigo, como a lista
    /// encadeada do C).
    ap_aux: Vec<Fts5Auxiliary>,
    /// `pTok`, na ordem de registro; o primeiro é o `pDfltTok`.
    ap_tok: Vec<Fts5TokenizerModule>,
}

impl Fts5Global {
    /// `++pGlobal->iNextId`.
    fn next_id(&self) -> i64 {
        let n = self.i_next_id.get() + 1;
        self.i_next_id.set(n);
        n
    }

    /// `fts5FindAuxiliary`: o índice da função auxiliar `z_name`.
    fn find_aux(&self, z_name: &[u8]) -> Option<usize> {
        (0..self.ap_aux.len()).rev().find(|&i| str_icmp(z_name, &self.ap_aux[i].z_func) == 0)
    }

    /// `fts5LocateTokenizer`: `None` pede o padrão.
    fn locate_tokenizer(&self, z_name: Option<&[u8]>) -> Option<&Fts5TokenizerModule> {
        match z_name {
            None => self.ap_tok.first(),
            Some(n) => self.ap_tok.iter().rev().find(|m| str_icmp(n, &m.z_name) == 0),
        }
    }
}

impl Fts5Api for Fts5Global {
    /// `fts5CreateTokenizer`.
    fn create_tokenizer(&mut self, z_name: &[u8], factory: Rc<dyn Fts5TokenizerFactory>) -> i32 {
        self.ap_tok.push(Fts5TokenizerModule { z_name: z_name.to_vec(), factory });
        SQLITE_OK
    }

    /// `fts5FindTokenizer`.
    fn find_tokenizer(&self, z_name: &[u8]) -> Option<Rc<dyn Fts5TokenizerFactory>> {
        self.locate_tokenizer(Some(z_name)).map(|m| m.factory.clone())
    }

    /// `fts5CreateAux` (sem o `sqlite3_overload_function`, que [`fts5_init`] faz).
    fn create_function(&mut self, z_name: &[u8], x_function: Rc<dyn Fts5ExtensionFunction>) -> i32 {
        self.ap_aux.push(Fts5Auxiliary { z_func: z_name.to_vec(), x_func: x_function });
        SQLITE_OK
    }
}

impl Fts5GlobalApi for Fts5Global {
    /// `sqlite3Fts5GetTokenizer`.
    fn get_tokenizer(
        &self,
        az_arg: &[Vec<u8>],
        config: &mut Fts5Config,
        pz_err: Option<&mut Option<Vec<u8>>>,
    ) -> i32 {
        let rc;
        match self.locate_tokenizer(az_arg.first().map(|v| v.as_slice())) {
            None => {
                rc = SQLITE_ERROR;
                if let (Some(pz), Some(name)) = (pz_err, az_arg.first()) {
                    *pz = mprintf(b"no such tokenizer: %s", &[text_arg(name)]);
                }
            }
            Some(p_mod) => {
                let args: &[Vec<u8>] = if az_arg.is_empty() { &[] } else { &az_arg[1..] };
                match p_mod.factory.create(self, args) {
                    Ok(tok) => {
                        config.e_pattern = tok.pattern();
                        config.p_tok = Some(tok);
                        rc = SQLITE_OK;
                    }
                    Err(e) => {
                        rc = e;
                        if let Some(pz) = pz_err {
                            if rc != crate::consts::SQLITE_NOMEM {
                                *pz = mprintf(b"error in tokenizer constructor", &[]);
                            }
                        }
                    }
                }
            }
        }
        if rc != SQLITE_OK {
            config.p_tok = None;
        }
        rc
    }
}

/// O dado do usuário da função SQL que sobrecarrega uma função auxiliar (o `Fts5Auxiliary*`).
pub struct Fts5AuxRef {
    /// O global da conexão.
    pub global: Rc<Fts5Global>,
    /// Índice em `Fts5Global.ap_aux`.
    pub i_aux: usize,
}

/// O valor ponteiro `"fts5_api_ptr"` que `fts5()` preenche (o `fts5_api**` do C).
#[derive(Default)]
pub struct Fts5ApiSlot(pub Cell<Option<Rc<Fts5Global>>>);

// ---------------------------------------------------------------------------------------------
// A tabela e o cursor
// ---------------------------------------------------------------------------------------------

/// `Fts5Auxdata`: o dado auxiliar de uma função num cursor (a chave é a função em execução).
struct Fts5Auxdata {
    /// `pAux`.
    p_aux: Option<usize>,
    /// `pPtr` (o `xDelete` é o `Drop`).
    p_ptr: Option<Rc<dyn Any>>,
}

/// Uma linha do ordenador: o rowid e o blob de poslists que a coluna `rank` da origem devolvia.
struct SorterRow {
    i_rowid: i64,
    blob: Vec<u8>,
}

/// `Fts5Sorter`: o resultado de `ORDER BY rank` já ordenado (ver o cabeçalho do módulo).
#[derive(Default)]
struct Fts5Sorter {
    /// As linhas, na ordem de saída.
    rows: Vec<SorterRow>,
    /// A próxima linha a entregar.
    i_next: usize,
    /// `iRowid`: a linha corrente.
    i_rowid: i64,
    /// `aPoslist`: as poslists da linha corrente.
    a_poslist: Vec<u8>,
    /// `nIdx`: o número de frases.
    n_idx: i32,
    /// `aIdx`: o fim de cada poslist em `a_poslist`.
    a_idx: Vec<i32>,
}

impl Fts5Sorter {
    /// As poslists da frase `i_phrase` na linha corrente: `aPoslist[aIdx[i-1]..aIdx[i]]`.
    /// `None` se a frase está fora de `aIdx`.
    fn phrase_list(&self, i_phrase: i32) -> Option<Vec<u8>> {
        let ip = usize::try_from(i_phrase).ok()?;
        let i_end = *self.a_idx.get(ip)? as usize;
        let i1 = if ip == 0 { 0 } else { *self.a_idx.get(ip - 1)? as usize };
        let end = (i1 + i_end.saturating_sub(i1)).min(self.a_poslist.len());
        Some(self.a_poslist[i1.min(end)..end].to_vec())
    }
}

/// `Fts5Cursor`.
pub struct Fts5Cursor {
    /// `iCsrId`.
    i_csr_id: i64,
    /// `aColumnSize`.
    a_column_size: Vec<i32>,

    /* Zerado em `reset_state` */
    /// `ePlan`.
    e_plan: i32,
    /// `bDesc`.
    b_desc: bool,
    /// `iFirstRowid`.
    i_first_rowid: i64,
    /// `iLastRowid`.
    i_last_rowid: i64,
    /// `pStmt`.
    p_stmt: Option<StmtId>,
    /// `pExpr`.
    p_expr: Option<Fts5Expr>,
    /// `pSorter`.
    p_sorter: Option<Fts5Sorter>,
    /// `csrflags`.
    csrflags: i32,
    /// `iSpecial`.
    i_special: i64,
    /// O rowid da linha corrente dos planos `SCAN` e `ROWID` (ver o cabeçalho do módulo).
    i_scan_rowid: i64,
    /// `zRank`.
    z_rank: Option<Vec<u8>>,
    /// `zRankArgs`.
    z_rank_args: Option<Vec<u8>>,
    /// `pRank`: índice da função de rank em `Fts5Global.ap_aux`.
    p_rank: Option<usize>,
    /// `apRankArg` (`nRankArg` é `len()`).
    ap_rank_arg: Vec<Mem>,
    /// `pAux`: a função auxiliar em execução.
    p_aux: Option<usize>,
    /// `pAuxdata`.
    p_auxdata: Vec<Fts5Auxdata>,
    /// `aInst`: (frase, coluna, deslocamento) de cada ocorrência (`nInstCount` é `len()`).
    a_inst: Vec<[i32; 3]>,
}

impl Fts5Cursor {
    fn new(i_csr_id: i64, n_col: i32) -> Fts5Cursor {
        Fts5Cursor {
            i_csr_id,
            a_column_size: vec![0; n_col.max(0) as usize],
            e_plan: 0,
            b_desc: false,
            i_first_rowid: 0,
            i_last_rowid: 0,
            p_stmt: None,
            p_expr: None,
            p_sorter: None,
            csrflags: 0,
            i_special: 0,
            i_scan_rowid: 0,
            z_rank: None,
            z_rank_args: None,
            p_rank: None,
            ap_rank_arg: Vec::new(),
            p_aux: None,
            p_auxdata: Vec::new(),
            a_inst: Vec::new(),
        }
    }

    /// O `memset(&pCsr->ePlan, 0, ...)` do C.
    fn reset_state(&mut self) {
        self.e_plan = 0;
        self.b_desc = false;
        self.i_first_rowid = 0;
        self.i_last_rowid = 0;
        self.p_stmt = None;
        self.p_expr = None;
        self.p_sorter = None;
        self.csrflags = 0;
        self.i_special = 0;
        self.i_scan_rowid = 0;
        self.z_rank = None;
        self.z_rank_args = None;
        self.p_rank = None;
        self.ap_rank_arg = Vec::new();
        self.p_aux = None;
        self.p_auxdata = Vec::new();
        self.a_inst = Vec::new();
    }
}

/// `Fts5FullTable` (com o `Fts5Table` dentro): a instância da tabela virtual.
pub struct Fts5FullTable {
    /// `p.pConfig`.
    pub config: Fts5Config,
    /// `p.pIndex`.
    pub index: Fts5Index,
    /// `pStorage`.
    pub storage: Fts5Storage,
    /// `pGlobal`.
    global: Rc<Fts5Global>,
    /// Os cursores abertos desta tabela (ver o cabeçalho do módulo).
    cursors: Vec<Fts5Cursor>,
    /// Cursores emprestados por [`with_csr`] (não estão em `cursors`, mas estão abertos).
    n_taken: usize,
    /// `iSavepoint`: `xSavepoint` bem-sucedido mais um.
    i_savepoint: i32,
    /// `base.zErrMsg`.
    z_err_msg: Option<Vec<u8>>,
}

/// O cursor do núcleo: só o id do [`Fts5Cursor`] guardado na tabela.
pub struct Fts5CsrHandle {
    id: i64,
}

/// A instância da tabela que o núcleo entrega.
pub(super) fn tab_of(vtab: &mut dyn Vtab) -> &mut Fts5FullTable {
    vtab.as_any_mut()
        .downcast_mut::<Fts5FullTable>()
        .expect("fts5: a instância não é uma Fts5FullTable")
}

/// Empresta o cursor `id` da tabela: `f` recebe a tabela e o cursor. `None` se o cursor não existe.
fn with_csr<R>(
    tab: &mut Fts5FullTable,
    id: i64,
    f: impl FnOnce(&mut Fts5FullTable, &mut Fts5Cursor) -> R,
) -> Option<R> {
    let pos = tab.cursors.iter().position(|c| c.i_csr_id == id)?;
    let mut csr = tab.cursors.swap_remove(pos);
    tab.n_taken += 1;
    let r = f(tab, &mut csr);
    tab.n_taken -= 1;
    tab.cursors.push(csr);
    Some(r)
}

/// `fts5CursorFromCsrid` + `sqlite3Fts5TableFromCsrid`: a tabela FTS5 da conexão que tem o
/// cursor `i_csr_id`.
pub fn fts5_table_from_csrid(db: &mut Connection, i_csr_id: i64) -> Option<VTableId> {
    for (id, vt) in db.vtabs.iter_mut() {
        if let Some(v) = vt.p_vtab.as_mut() {
            if let Some(t) = v.as_any_mut().downcast_mut::<Fts5FullTable>() {
                if t.cursors.iter().any(|c| c.i_csr_id == i_csr_id) {
                    return Some(VTableId::from_slot(id));
                }
            }
        }
    }
    None
}

impl Fts5FullTable {
    /// `pConfig->pzErrmsg = &base.zErrMsg`: arma a mensagem de erro da configuração.
    fn arm_errmsg(&mut self) {
        self.config.errmsg_target = true;
    }

    /// `pConfig->pzErrmsg = 0`: desarma e move a mensagem pendente para `base.zErrMsg`.
    fn disarm_errmsg(&mut self) {
        self.config.errmsg_target = false;
        if let Some(m) = self.config.errmsg.take() {
            self.z_err_msg = Some(m);
        }
    }

    /// `fts5IsContentless`.
    fn is_contentless(&self) -> bool {
        self.config.e_content == FTS5_CONTENT_NONE
    }

    /// `fts5FreeVtab`: fecha o índice e o armazém (a configuração é o `Drop`).
    fn free_vtab(&mut self, db: &mut Connection) {
        self.index.close(db);
        self.storage.close(db);
    }

    /// `fts5NewTransaction`.
    fn new_transaction(&mut self, db: &mut Connection) -> i32 {
        if !self.cursors.is_empty() || self.n_taken > 0 {
            return SQLITE_OK;
        }
        self.storage.reset(db, &self.config, &mut self.index)
    }

    /// `fts5TripCursors`: liga `FTS5CSR_REQUIRE_RESEEK` nos cursores `MATCH`.
    fn trip_cursors(&mut self) {
        for csr in self.cursors.iter_mut() {
            if csr.e_plan == FTS5_PLAN_MATCH {
                csr.csrflags |= FTS5CSR_REQUIRE_RESEEK;
            }
        }
    }

    /// `sqlite3Fts5FlushToDisk`.
    pub fn flush_to_disk(&mut self, db: &mut Connection) -> i32 {
        self.trip_cursors();
        self.storage.sync(db, &mut self.config, &mut self.index)
    }

    /// `fts5SetVtabError`.
    fn set_vtab_error(&mut self, fmt: &[u8], args: &[PrintfArg]) {
        self.z_err_msg = mprintf(fmt, args);
    }

    /// `fts5SpecialInsert`: o comando `INSERT INTO fts(fts) VALUES(z_cmd)` (e a variante com
    /// valor em `rank`).
    fn special_insert(&mut self, db: &mut Connection, z_cmd: &[u8], p_val: &Mem) -> i32 {
        let mut rc = SQLITE_OK;
        let mut b_error = 0;
        let mut b_load_config = false;

        if 0 == str_icmp(b"delete-all", z_cmd) {
            if self.config.e_content == FTS5_CONTENT_NORMAL {
                self.set_vtab_error(
                    b"'delete-all' may only be used with a contentless or external content fts5 table",
                    &[],
                );
                rc = SQLITE_ERROR;
            } else {
                rc = self.storage.delete_all(db, &mut self.config, &mut self.index);
            }
            b_load_config = true;
        } else if 0 == str_icmp(b"rebuild", z_cmd) {
            if self.config.e_content == FTS5_CONTENT_NONE {
                self.set_vtab_error(b"'rebuild' may not be used with a contentless fts5 table", &[]);
                rc = SQLITE_ERROR;
            } else {
                rc = self.storage.rebuild(db, &mut self.config, &mut self.index);
            }
            b_load_config = true;
        } else if 0 == str_icmp(b"optimize", z_cmd) {
            rc = self.storage.optimize(db, &mut self.config, &mut self.index);
        } else if 0 == str_icmp(b"merge", z_cmd) {
            let n_merge = value_int(p_val);
            rc = self.storage.merge(db, &mut self.config, &mut self.index, n_merge);
        } else if 0 == str_icmp(b"integrity-check", z_cmd) {
            let i_arg = value_int(p_val);
            rc = self.storage.integrity(db, &mut self.config, &mut self.index, i_arg);
        } else if 0 == str_icmp(b"flush", z_cmd) {
            rc = self.flush_to_disk(db);
        } else {
            rc = self.flush_to_disk(db);
            if rc == SQLITE_OK {
                rc = self.index.load_config(db, &mut self.config);
            }
            if rc == SQLITE_OK {
                let mut v = p_val.clone();
                rc = self.config.set_value(z_cmd, &mut v, &mut b_error);
            }
            if rc == SQLITE_OK {
                if b_error != 0 {
                    rc = SQLITE_ERROR;
                } else {
                    rc = self.storage.config_value(
                        db,
                        &mut self.config,
                        &mut self.index,
                        z_cmd,
                        Some(p_val),
                        0,
                    );
                }
            }
        }

        if rc == SQLITE_OK && b_load_config {
            self.config.i_cookie -= 1;
            rc = self.index.load_config(db, &mut self.config);
        }

        rc
    }

    /// `fts5SpecialDelete`: `INSERT INTO fts(fts, rowid, ...) VALUES('delete', ...)`.
    fn special_delete(&mut self, db: &mut Connection, ap_val: &[Mem]) -> i32 {
        let mut rc = SQLITE_OK;
        let e_type1 = value_type(&ap_val[1]);
        if e_type1 == SQLITE_INTEGER {
            let i_del = value_int64(&ap_val[1]);
            rc = self.storage.delete(db, &mut self.config, &mut self.index, i_del, Some(&ap_val[2..]));
        }
        rc
    }

    /// `fts5StorageInsert`.
    fn storage_insert(&mut self, db: &mut Connection, rc: &mut i32, ap_val: &[Mem], pi_rowid: &mut i64) {
        let mut r = *rc;
        if r == SQLITE_OK {
            r = self.storage.content_insert(db, &mut self.config, ap_val, pi_rowid);
        }
        if r == SQLITE_OK {
            r = self.storage.index_insert(db, &mut self.config, &mut self.index, ap_val, *pi_rowid);
        }
        *rc = r;
    }
}

// ---------------------------------------------------------------------------------------------
// Funções do cursor
// ---------------------------------------------------------------------------------------------

/// `fts5StmtType`.
fn stmt_type(csr: &Fts5Cursor) -> i32 {
    if csr.e_plan == FTS5_PLAN_SCAN {
        if csr.b_desc {
            FTS5_STMT_SCAN_DESC
        } else {
            FTS5_STMT_SCAN_ASC
        }
    } else {
        FTS5_STMT_LOOKUP
    }
}

/// `fts5FreeCursorComponents`.
fn free_cursor_components(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor) {
    csr.a_inst = Vec::new();
    if let Some(stmt) = csr.p_stmt.take() {
        let e_stmt = stmt_type(csr);
        tab.storage.stmt_release(db, e_stmt, stmt);
    }
    csr.p_sorter = None;
    if let Some(expr) = csr.p_expr.take() {
        expr.free(db, &mut tab.index);
    }
    tab.index.close_reader(db);
    csr.reset_state();
}

/// `fts5CursorRowid`.
fn csr_rowid(csr: &Fts5Cursor) -> i64 {
    match (&csr.p_sorter, &csr.p_expr) {
        (Some(s), _) => s.i_rowid,
        (None, Some(e)) => e.rowid(),
        (None, None) => 0,
    }
}

/// `fts5SorterNext`: passa à próxima linha ordenada.
fn sorter_next(csr: &mut Fts5Cursor) -> i32 {
    let Some(sorter) = csr.p_sorter.as_mut() else {
        return SQLITE_OK;
    };
    if sorter.i_next >= sorter.rows.len() {
        csr.csrflags |= FTS5CSR_EOF | FTS5CSR_REQUIRE_CONTENT;
        return SQLITE_OK;
    }
    let i_row = sorter.i_next;
    sorter.i_next += 1;
    sorter.i_rowid = sorter.rows[i_row].i_rowid;
    let blob = std::mem::take(&mut sorter.rows[i_row].blob);

    /* `blob` vazio no `detail=none`. */
    if !blob.is_empty() {
        let mut a = 0usize;
        let mut i_off = 0i32;
        let mut i = 0i32;
        while i < sorter.n_idx - 1 {
            let (n, v) = fts5_get_varint32(&blob[a.min(blob.len())..]);
            a += n as usize;
            i_off += v as i32;
            sorter.a_idx[i as usize] = i_off;
            i += 1;
        }
        sorter.a_idx[i as usize] = (blob.len() as i32) - a as i32;
        sorter.a_poslist = blob[a.min(blob.len())..].to_vec();
    }

    csr.csrflags |= FTS5CSR_NEWROW;
    SQLITE_OK
}

/// `fts5CursorReseek`: se `FTS5CSR_REQUIRE_RESEEK` está ligado, reabre os iteradores e vai para
/// o rowid corrente ou o seguinte. `*pb_skip` vira verdadeiro se o rowid mudou.
fn csr_reseek(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    pb_skip: &mut bool,
) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(!*pb_skip);
    if csr.csrflags & FTS5CSR_REQUIRE_RESEEK != 0 {
        let b_desc = csr.b_desc;
        let Some(expr) = csr.p_expr.as_mut() else {
            return SQLITE_OK;
        };
        let i_rowid = expr.rowid();

        rc = expr.first(db, &mut tab.index, &mut tab.config, i_rowid, b_desc);
        if rc == SQLITE_OK && i_rowid != expr.rowid() {
            *pb_skip = true;
        }

        csr.csrflags &= !FTS5CSR_REQUIRE_RESEEK;
        csr.csrflags |= FTS5CSR_NEWROW;
        if expr.eof() {
            csr.csrflags |= FTS5CSR_EOF;
            *pb_skip = true;
        }
    }
    rc
}

/// `fts5NextMethod`: avança para a próxima linha que casa com a consulta.
fn csr_next(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor) -> i32 {
    debug_assert!((csr.e_plan < 3) == (csr.e_plan == FTS5_PLAN_MATCH || csr.e_plan == FTS5_PLAN_SOURCE));
    debug_assert!(csr.csrflags & FTS5CSR_EOF == 0);

    /* Numa tabela `tokendata=1` com plano MATCH, descarta os mapas de token acumulados pelo
    ** índice. Nos planos SOURCE e SORTED_MATCH o mapa vale para a consulta toda. */
    if csr.e_plan == FTS5_PLAN_MATCH && tab.config.b_tokendata != 0 {
        if let Some(expr) = csr.p_expr.as_mut() {
            expr.clear_tokens();
        }
    }

    let rc;
    if csr.e_plan < 3 {
        let mut b_skip = false;
        let rc0 = csr_reseek(db, tab, csr, &mut b_skip);
        if rc0 != SQLITE_OK || b_skip {
            return rc0;
        }
        let i_last = csr.i_last_rowid;
        let Some(expr) = csr.p_expr.as_mut() else {
            return SQLITE_OK;
        };
        rc = expr.next(db, &mut tab.index, &mut tab.config, i_last);
        if expr.eof() {
            csr.csrflags |= FTS5CSR_EOF;
        }
        csr.csrflags |= FTS5CSR_NEWROW;
    } else {
        match csr.e_plan {
            FTS5_PLAN_SPECIAL => {
                csr.csrflags |= FTS5CSR_EOF;
                rc = SQLITE_OK;
            }
            FTS5_PLAN_SORTED_MATCH => {
                rc = sorter_next(csr);
            }
            _ => {
                let stmt = csr.p_stmt.unwrap_or_default();
                tab.config.b_lock += 1;
                let mut r = step(db, stmt);
                tab.config.b_lock -= 1;
                if r != SQLITE_ROW {
                    csr.csrflags |= FTS5CSR_EOF;
                    r = reset(db, stmt);
                    if r != SQLITE_OK {
                        tab.z_err_msg = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
                    }
                } else {
                    r = SQLITE_OK;
                    csr.i_scan_rowid = column_int64(db, stmt, 0);
                }
                rc = r;
            }
        }
    }
    rc
}

/// `fts5CursorFirst`: o primeiro documento da expressão.
fn csr_first(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor, b_desc: bool) -> i32 {
    let i_first = csr.i_first_rowid;
    let Some(expr) = csr.p_expr.as_mut() else {
        return SQLITE_OK;
    };
    let rc = expr.first(db, &mut tab.index, &mut tab.config, i_first, b_desc);
    if expr.eof() {
        csr.csrflags |= FTS5CSR_EOF;
    }
    csr.csrflags |= FTS5CSR_NEWROW;
    rc
}

/// `fts5PoslistBlob`: o blob de poslists da linha corrente (as frases, uma a uma): `nPhrase-1`
/// varints com os tamanhos e depois as poslists concatenadas. Devolve o blob e o código de erro
/// (que o chamador do C ignora).
fn poslist_blob(tab: &Fts5FullTable, csr: &Fts5Cursor) -> (i32, Vec<u8>) {
    let mut rc = SQLITE_OK;
    let mut val = Fts5Buffer::new();
    let Some(expr) = csr.p_expr.as_ref() else {
        return (rc, val.p);
    };
    let n_phrase = expr.phrase_count();
    match tab.config.e_detail {
        FTS5_DETAIL_FULL => {
            for i in 0..(n_phrase - 1).max(0) {
                val.append_varint(expr.poslist(i).len() as i64);
            }
            for i in 0..n_phrase {
                val.append_blob(expr.poslist(i));
            }
        }
        FTS5_DETAIL_COLUMNS => {
            let mut i = 0;
            while rc == SQLITE_OK && i < n_phrase - 1 {
                val.append_varint(expr.phrase_collist(i).len() as i64);
                i += 1;
            }
            let mut i = 0;
            while rc == SQLITE_OK && i < n_phrase {
                val.append_blob(&expr.phrase_collist(i));
                i += 1;
            }
        }
        _ => {}
    }
    (rc, val.p)
}

/// `fts5SpecialMatch`: uma consulta especial (`MATCH '*reads'`, `'*id'`).
fn special_match(tab: &mut Fts5FullTable, csr: &mut Fts5Cursor, z_query: &[u8]) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p = 0usize;
    while at(z_query, p) == b' ' {
        p += 1;
    }
    let z = &z_query[p.min(z_query.len())..];
    let mut n = 0usize;
    while at(z, n) != 0 && at(z, n) != b' ' {
        n += 1;
    }

    debug_assert!(tab.z_err_msg.is_none());
    csr.e_plan = FTS5_PLAN_SPECIAL;

    if n == 5 && 0 == strnicmp(Some(b"reads".as_slice()), Some(z), n as i32) {
        csr.i_special = tab.index.reads() as i64;
    } else if n == 2 && 0 == strnicmp(Some(b"id".as_slice()), Some(z), n as i32) {
        csr.i_special = csr.i_csr_id;
    } else {
        /* Uma diretiva desconhecida. */
        tab.z_err_msg = mprintf(b"unknown special query: %s", &[text_arg(&z[..n.min(z.len())])]);
        rc = SQLITE_ERROR;
    }
    rc
}

/// Avalia `SELECT <z_args>` e copia os valores da linha (os argumentos finais da função de rank:
/// literais, o C os mantinha vivos no comando). O erro leva o código e se veio do `prepare`.
fn eval_rank_args(db: &mut Connection, z_args: &[u8]) -> Result<Vec<Mem>, (i32, bool)> {
    let Some(z_sql) = mprintf(b"SELECT %s", &[text_arg(z_args)]) else {
        return Err((crate::consts::SQLITE_NOMEM, false));
    };
    let (rc, stmt, _tail) = prepare_v3(db, &z_sql, -1, SQLITE_PREPARE_PERSISTENT);
    if rc != SQLITE_OK {
        return Err((rc, true));
    }
    let Some(p_stmt) = stmt else {
        return Ok(Vec::new());
    };
    if SQLITE_ROW == step(db, p_stmt) {
        let n = column_count(db, p_stmt);
        let vals: Vec<Mem> = (0..n).map(|i| column_value(db, p_stmt, i)).collect();
        finalize(db, p_stmt);
        Ok(vals)
    } else {
        let rc = finalize(db, p_stmt);
        debug_assert!(rc != SQLITE_OK);
        Err((if rc == SQLITE_OK { SQLITE_ERROR } else { rc }, false))
    }
}

/// `fts5FindRankFunction`: avalia os argumentos da função de rank do cursor (`zRankArgs`) e
/// localiza a função.
fn find_rank_function(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor) -> i32 {
    let mut rc = SQLITE_OK;

    if let Some(z_args) = csr.z_rank_args.clone() {
        match eval_rank_args(db, &z_args) {
            Ok(vals) => csr.ap_rank_arg = vals,
            Err((e, _)) => rc = e,
        }
    }

    let mut p_aux = None;
    if rc == SQLITE_OK {
        let z_rank = csr.z_rank.clone().unwrap_or_default();
        p_aux = tab.global.find_aux(&z_rank);
        if p_aux.is_none() {
            debug_assert!(tab.z_err_msg.is_none());
            tab.z_err_msg = mprintf(b"no such function: %s", &[text_arg(&z_rank)]);
            rc = SQLITE_ERROR;
        }
    }

    csr.p_rank = p_aux;
    rc
}

/// `fts5CursorParseRank`: o `rank MATCH ?` (ou o padrão da tabela).
fn csr_parse_rank(tab: &mut Fts5FullTable, csr: &mut Fts5Cursor, p_rank: Option<&Mem>) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(v) = p_rank {
        let z: Option<Vec<u8>> = text_of(v).map(|t| t.into_owned());
        match z.as_deref() {
            None => {
                if value_type(v) == SQLITE_NULL {
                    rc = SQLITE_ERROR;
                }
            }
            Some(zz) => match fts5_config_parse_rank(Some(zz)) {
                Ok((z_rank, z_rank_args)) => {
                    csr.z_rank = Some(z_rank);
                    csr.z_rank_args = z_rank_args;
                }
                Err(e) => rc = e,
            },
        }
        if rc == SQLITE_ERROR {
            tab.z_err_msg = mprintf(
                b"parse error in rank function: %s",
                &[PrintfArg::Text(z)],
            );
        }
    } else if let Some(z) = tab.config.z_rank.clone() {
        csr.z_rank = Some(z);
        csr.z_rank_args = tab.config.z_rank_args.clone();
    } else {
        csr.z_rank = Some(FTS5_DEFAULT_RANK.to_vec());
        csr.z_rank_args = None;
    }
    rc
}

/// `fts5GetRowidLimit`.
fn get_rowid_limit(p_val: Option<&Mem>, i_default: i64) -> i64 {
    if let Some(v) = p_val {
        let mut c = v.clone();
        if value_numeric_type(&mut c) == SQLITE_INTEGER {
            return value_int64(&c);
        }
    }
    i_default
}

/// A chave de ordenação do `rank` (o que `ORDER BY` compara): NULL, depois números, depois texto.
#[derive(Debug, Clone, PartialEq)]
enum SortKey {
    Null,
    Num(f64),
    Text(Vec<u8>),
}

impl SortKey {
    /// A ordem do `sqlite3MemCompare` com a colação BINARY.
    fn compare(&self, other: &SortKey) -> std::cmp::Ordering {
        use std::cmp::Ordering::{Equal, Greater, Less};
        match (self, other) {
            (SortKey::Null, SortKey::Null) => Equal,
            (SortKey::Null, _) => Less,
            (_, SortKey::Null) => Greater,
            (SortKey::Num(a), SortKey::Num(b)) => a.partial_cmp(b).unwrap_or(Equal),
            (SortKey::Num(_), SortKey::Text(_)) => Less,
            (SortKey::Text(_), SortKey::Num(_)) => Greater,
            (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
        }
    }
}

/// `fts5CursorFirstSorted`: o `ORDER BY rank`. Ver o cabeçalho do módulo: no lugar do comando
/// aninhado, percorre a expressão com um cursor de origem (`FTS5_PLAN_SOURCE`), chama a função de
/// rank em cada linha e ordena.
fn csr_first_sorted(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    b_desc: bool,
) -> i32 {
    let n_phrase = csr.p_expr.as_ref().map_or(0, |e| e.phrase_count());
    let z_rank = csr.z_rank.clone().unwrap_or_default();
    let mut rc = SQLITE_OK;

    /* Os argumentos da função de rank: literais, avaliados uma vez (`SELECT <args>`). */
    let mut rank_args: Vec<Mem> = Vec::new();
    if let Some(z_args) = csr.z_rank_args.clone() {
        match eval_rank_args(db, &z_args) {
            Ok(vals) => rank_args = vals,
            Err((e, b_prepare)) => {
                rc = e;
                if b_prepare {
                    tab.z_err_msg = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
                }
            }
        }
    }

    /* A função de rank (o `ORDER BY %s("tbl", ...)` do comando aninhado). */
    let mut i_aux = 0usize;
    if rc == SQLITE_OK {
        match tab.global.find_aux(&z_rank) {
            Some(i) => i_aux = i,
            None => {
                tab.z_err_msg = mprintf(b"no such function: %s", &[text_arg(&z_rank)]);
                rc = SQLITE_ERROR;
            }
        }
    }

    if rc == SQLITE_OK {
        /* O cursor de origem compartilha a expressão com o cursor que ordena. */
        let mut src = Fts5Cursor::new(csr.i_csr_id, tab.config.n_col());
        src.e_plan = FTS5_PLAN_SOURCE;
        src.p_expr = csr.p_expr.take();
        if b_desc {
            src.i_last_rowid = csr.i_first_rowid;
            src.i_first_rowid = csr.i_last_rowid;
        } else {
            src.i_last_rowid = csr.i_last_rowid;
            src.i_first_rowid = csr.i_first_rowid;
        }

        let mut rows: Vec<(SortKey, SorterRow)> = Vec::new();
        rc = csr_first(db, tab, &mut src, false);
        while rc == SQLITE_OK && src.csrflags & FTS5CSR_EOF == 0 {
            let i_rowid = csr_rowid(&src);
            let key = match api_invoke(db, tab, &mut src, i_aux, &rank_args) {
                Fts5AuxResult::Null => SortKey::Null,
                Fts5AuxResult::Double(d) => {
                    if d.is_nan() {
                        SortKey::Null
                    } else {
                        SortKey::Num(d)
                    }
                }
                Fts5AuxResult::Text(t) => SortKey::Text(t),
                Fts5AuxResult::Error(_) => {
                    rc = SQLITE_ERROR;
                    break;
                }
                Fts5AuxResult::ErrorCode(e) => {
                    rc = e;
                    break;
                }
            };
            let (_rc_blob, blob) = poslist_blob(tab, &src);
            rows.push((key, SorterRow { i_rowid, blob }));
            rc = csr_next(db, tab, &mut src);
        }

        csr.p_expr = src.p_expr.take();
        if let Some(stmt) = src.p_stmt.take() {
            let e_stmt = stmt_type(&src);
            tab.storage.stmt_release(db, e_stmt, stmt);
        }

        if rc == SQLITE_OK {
            /* `sort_by` é estável: os empates ficam na ordem de rowid crescente. */
            if b_desc {
                rows.sort_by(|a, b| b.0.compare(&a.0));
            } else {
                rows.sort_by(|a, b| a.0.compare(&b.0));
            }
            csr.p_sorter = Some(Fts5Sorter {
                rows: rows.into_iter().map(|(_, r)| r).collect(),
                i_next: 0,
                i_rowid: 0,
                a_poslist: Vec::new(),
                n_idx: n_phrase,
                a_idx: vec![0; n_phrase.max(1) as usize],
            });
            rc = sorter_next(csr);
        }
        if rc != SQLITE_OK {
            csr.p_sorter = None;
        }
    }

    rc
}

/// `fts5FilterMethod`.
fn csr_filter(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    idx_num: i32,
    idx_str: &[u8],
    ap_val: &[Mem],
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut p_rank: Option<&Mem> = None;
    let mut p_rowid_eq: Option<&Mem> = None;
    let mut p_rowid_le: Option<&Mem> = None;
    let mut p_rowid_ge: Option<&Mem> = None;
    let mut p_expr: Option<Fts5Expr> = None;
    let mut i_idx_str = 0usize;

    if tab.config.b_lock != 0 {
        tab.z_err_msg = mprintf(b"recursively defined fts5 content table", &[]);
        return SQLITE_ERROR;
    }

    if csr.e_plan != 0 {
        free_cursor_components(db, tab, csr);
    }

    debug_assert!(csr.p_stmt.is_none());
    debug_assert!(csr.p_expr.is_none());
    debug_assert!(csr.csrflags == 0);
    debug_assert!(csr.p_rank.is_none());
    debug_assert!(csr.z_rank.is_none());
    debug_assert!(csr.z_rank_args.is_none());

    tab.arm_errmsg();

    'filter_out: {
        /* Decodifica os argumentos passados a esta função. */
        for arg in ap_val.iter() {
            let c = at(idx_str, i_idx_str);
            i_idx_str += 1;
            match c {
                b'r' => p_rank = Some(arg),
                b'M' => {
                    let z_text: Vec<u8> = text_of(arg).map(|t| t.into_owned()).unwrap_or_default();
                    let i_col = parse_col(idx_str, &mut i_idx_str);

                    if z_text.first() == Some(&b'*') {
                        /* `MATCH '*...'`: não é uma consulta de texto, é um pedido de parâmetro
                        ** interno. */
                        rc = special_match(tab, csr, &z_text[1..]);
                        break 'filter_out;
                    }
                    rc = fts5_expr_new(&tab.config, false, i_col, &z_text, &mut p_expr, &mut tab.z_err_msg);
                    if rc == SQLITE_OK {
                        rc = fts5_expr_and(&tab.config, &mut csr.p_expr, p_expr.take());
                    }
                    if rc != SQLITE_OK {
                        break 'filter_out;
                    }
                }
                b'L' | b'G' => {
                    let b_glob = c == b'G';
                    let z_text: Option<Vec<u8>> = text_of(arg).map(|t| t.into_owned());
                    let i_col = parse_col(idx_str, &mut i_idx_str);
                    if let Some(t) = z_text {
                        rc = fts5_expr_pattern(&mut tab.config, b_glob, i_col, &t, &mut p_expr);
                    }
                    if rc == SQLITE_OK {
                        rc = fts5_expr_and(&tab.config, &mut csr.p_expr, p_expr.take());
                    }
                    if rc != SQLITE_OK {
                        break 'filter_out;
                    }
                }
                b'=' => p_rowid_eq = Some(arg),
                b'<' => p_rowid_le = Some(arg),
                _ => {
                    debug_assert!(c == b'>');
                    p_rowid_ge = Some(arg);
                }
            }
        }
        let b_order_by_rank = idx_num & FTS5_BI_ORDER_RANK != 0;
        let b_desc = idx_num & FTS5_BI_ORDER_DESC != 0;
        csr.b_desc = b_desc;

        /* Os limites de rowid do cursor. Só algumas estratégias os usam; está certo, porque o
        ** `xBestIndex` deixa `omit` desligado nas restrições de faixa do rowid. */
        if p_rowid_eq.is_some() {
            p_rowid_le = p_rowid_eq;
            p_rowid_ge = p_rowid_eq;
        }
        if b_desc {
            csr.i_first_rowid = get_rowid_limit(p_rowid_le, LARGEST_INT64);
            csr.i_last_rowid = get_rowid_limit(p_rowid_ge, SMALLEST_INT64);
        } else {
            csr.i_last_rowid = get_rowid_limit(p_rowid_le, LARGEST_INT64);
            csr.i_first_rowid = get_rowid_limit(p_rowid_ge, SMALLEST_INT64);
        }

        rc = tab.index.load_config(db, &mut tab.config);
        if rc != SQLITE_OK {
            break 'filter_out;
        }

        if csr.p_expr.is_some() {
            debug_assert!(rc == SQLITE_OK);
            rc = csr_parse_rank(tab, csr, p_rank);
            if rc == SQLITE_OK {
                if b_order_by_rank {
                    csr.e_plan = FTS5_PLAN_SORTED_MATCH;
                    rc = csr_first_sorted(db, tab, csr, b_desc);
                } else {
                    csr.e_plan = FTS5_PLAN_MATCH;
                    rc = csr_first(db, tab, csr, b_desc);
                }
            }
        } else if tab.config.z_content.is_none() {
            tab.z_err_msg = mprintf(
                b"%s: table does not support scanning",
                &[text_arg(&tab.config.z_name)],
            );
            rc = SQLITE_ERROR;
        } else {
            /* Uma varredura da tabela (FTS5_PLAN_SCAN) ou uma busca por rowid (FTS5_PLAN_ROWID). */
            csr.e_plan = if p_rowid_eq.is_some() { FTS5_PLAN_ROWID } else { FTS5_PLAN_SCAN };
            let e_stmt = stmt_type(csr);
            match tab.storage.stmt(db, &mut tab.config, e_stmt, Some(&mut tab.z_err_msg)) {
                Err(e) => rc = e,
                Ok(s) => {
                    csr.p_stmt = Some(s);
                    if let Some(eq) = p_rowid_eq {
                        debug_assert!(csr.e_plan == FTS5_PLAN_ROWID);
                        bind_value(db, s, 1, eq);
                    } else {
                        bind_int64(db, s, 1, csr.i_first_rowid);
                        bind_int64(db, s, 2, csr.i_last_rowid);
                    }
                    rc = csr_next(db, tab, csr);
                }
            }
        }
    }

    if let Some(e) = p_expr.take() {
        e.free(db, &mut tab.index);
    }
    tab.disarm_errmsg();
    rc
}

/// O número de coluna que segue um `M`, `L` ou `G` em `idxStr` (`do{...}while`).
fn parse_col(idx_str: &[u8], pos: &mut usize) -> i32 {
    let mut i_col = 0i32;
    loop {
        i_col = i_col * 10 + (at(idx_str, *pos) as i32 - b'0' as i32);
        *pos += 1;
        let c = at(idx_str, *pos);
        if !c.is_ascii_digit() {
            break;
        }
    }
    i_col
}

/// `fts5SeekCursor`: posiciona o comando de leitura de `%_content` na linha do cursor.
fn seek_cursor(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    b_errormsg: bool,
) -> i32 {
    let mut rc = SQLITE_OK;

    /* Se o cursor ainda não tem comando, obtém um. */
    if csr.p_stmt.is_none() {
        let e_stmt = stmt_type(csr);
        let r = if b_errormsg {
            tab.storage.stmt(db, &mut tab.config, e_stmt, Some(&mut tab.z_err_msg))
        } else {
            tab.storage.stmt(db, &mut tab.config, e_stmt, None)
        };
        match r {
            Ok(s) => csr.p_stmt = Some(s),
            Err(e) => rc = e,
        }
        debug_assert!(rc != SQLITE_OK || tab.z_err_msg.is_none());
        debug_assert!(csr.csrflags & FTS5CSR_REQUIRE_CONTENT != 0);
    }

    if rc == SQLITE_OK && csr.csrflags & FTS5CSR_REQUIRE_CONTENT != 0 {
        let stmt = csr.p_stmt.unwrap_or_default();
        debug_assert!(csr.p_expr.is_some());
        reset(db, stmt);
        bind_int64(db, stmt, 1, csr_rowid(csr));
        tab.config.b_lock += 1;
        rc = step(db, stmt);
        tab.config.b_lock -= 1;
        if rc == SQLITE_ROW {
            rc = SQLITE_OK;
            csr.csrflags &= !FTS5CSR_REQUIRE_CONTENT;
        } else {
            rc = reset(db, stmt);
            if rc == SQLITE_OK {
                rc = FTS5_CORRUPT;
            } else if tab.config.errmsg_target {
                tab.config.errmsg = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
            }
        }
    }
    rc
}

/// `fts5ApiColumnText`: o texto da coluna da linha corrente; `Ok(None)` se NULL ou sem conteúdo.
fn csr_column_text(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    i_col: i32,
) -> Result<Option<Vec<u8>>, i32> {
    if i_col < 0 || i_col >= tab.config.n_col() {
        Err(SQLITE_RANGE)
    } else if tab.is_contentless() || csr.e_plan == FTS5_PLAN_SPECIAL {
        Ok(None)
    } else {
        let rc = seek_cursor(db, tab, csr, false);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        let stmt = csr.p_stmt.unwrap_or_default();
        Ok(column_text(db, stmt, i_col + 1).map(|t| t.to_vec()))
    }
}

/// `fts5CsrPoslist`: a poslist (cópia) da frase `i_phrase` na linha corrente.
fn csr_poslist(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    i_phrase: i32,
) -> Result<Vec<u8>, i32> {
    let mut rc = SQLITE_OK;
    let b_live = csr.p_sorter.is_none();
    let n_phrase = csr.p_expr.as_ref().map_or(0, |e| e.phrase_count());

    if i_phrase < 0 || i_phrase >= n_phrase {
        rc = SQLITE_RANGE;
    } else if csr.csrflags & FTS5CSR_REQUIRE_POSLIST != 0 {
        if tab.config.e_detail != FTS5_DETAIL_FULL {
            let mut a_populator = match csr.p_expr.as_mut() {
                Some(e) => e.clear_poslists(b_live),
                None => Vec::new(),
            };
            let mut i = 0;
            while i < tab.config.n_col() && rc == SQLITE_OK {
                match csr_column_text(db, tab, csr, i) {
                    Ok(z) => {
                        if let Some(expr) = csr.p_expr.as_mut() {
                            rc = expr.populate_poslists(
                                &mut tab.index,
                                &tab.config,
                                &mut a_populator,
                                i,
                                z.as_deref(),
                            );
                        }
                    }
                    Err(e) => rc = e,
                }
                i += 1;
            }

            if let (Some(sorter), Some(expr)) = (csr.p_sorter.as_ref(), csr.p_expr.as_mut()) {
                expr.check_poslists(sorter.i_rowid);
            }
        }
        csr.csrflags &= !FTS5CSR_REQUIRE_POSLIST;
    }

    if rc == SQLITE_OK {
        if let (Some(sorter), FTS5_DETAIL_FULL) = (csr.p_sorter.as_ref(), tab.config.e_detail) {
            Ok(sorter.phrase_list(i_phrase).unwrap_or_default())
        } else {
            match csr.p_expr.as_ref() {
                Some(e) => Ok(e.poslist(i_phrase).to_vec()),
                None => Ok(Vec::new()),
            }
        }
    } else {
        Err(rc)
    }
}

/// `fts5CacheInstArray`: preenche `csr.a_inst` com as ocorrências de frase da linha corrente.
fn cache_inst_array(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor) -> i32 {
    let mut rc = SQLITE_OK;
    let n_col = tab.config.n_col();
    let n_iter = csr.p_expr.as_ref().map_or(0, |e| e.phrase_count());

    /* As poslists de cada frase */
    let mut lists: Vec<Vec<u8>> = Vec::with_capacity(n_iter.max(0) as usize);
    for i in 0..n_iter {
        match csr_poslist(db, tab, csr, i) {
            Ok(a) => lists.push(a),
            Err(e) => {
                rc = e;
                break;
            }
        }
    }

    csr.a_inst.clear();
    if rc == SQLITE_OK {
        let mut a_iter: Vec<Fts5PoslistReader<'_>> =
            lists.iter().map(|a| Fts5PoslistReader::init(a)).collect();
        loop {
            let mut i_best: Option<usize> = None;
            for (i, it) in a_iter.iter().enumerate() {
                if it.b_eof == 0 && i_best.map_or(true, |b| it.i_pos < a_iter[b].i_pos) {
                    i_best = Some(i);
                }
            }
            let Some(b) = i_best else {
                break;
            };

            let i_pos = a_iter[b].i_pos;
            let i_col = fts5_pos2column(i_pos);
            csr.a_inst.push([b as i32, i_col, fts5_pos2offset(i_pos)]);
            if i_col < 0 || i_col >= n_col {
                rc = FTS5_CORRUPT;
                break;
            }
            a_iter[b].next();
        }
    }

    csr.csrflags &= !FTS5CSR_REQUIRE_INST;
    rc
}

/// Garante que `csr.a_inst` vale para a linha corrente (o `FTS5CSR_REQUIRE_INST` do C).
fn ensure_inst(db: &mut Connection, tab: &mut Fts5FullTable, csr: &mut Fts5Cursor) -> i32 {
    if csr.csrflags & FTS5CSR_REQUIRE_INST != 0 {
        cache_inst_array(db, tab, csr)
    } else {
        SQLITE_OK
    }
}

/// A ocorrência `i_idx` (frase, coluna, deslocamento) da linha corrente; `SQLITE_RANGE` fora da
/// faixa.
fn inst_at(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    i_idx: i32,
) -> Result<[i32; 3], i32> {
    let rc = ensure_inst(db, tab, csr);
    if rc != SQLITE_OK {
        return Err(rc);
    }
    usize::try_from(i_idx).ok().and_then(|i| csr.a_inst.get(i).copied()).ok_or(SQLITE_RANGE)
}

/// Carrega em `p_iter` a lista `data` (o `a`/`b` do C).
fn load_phrase_iter(p_iter: &mut Fts5PhraseIter, data: Vec<u8>) {
    p_iter.b = data.len();
    p_iter.data = data;
    p_iter.a = 0;
}

/// Lê um varint de 31 bits em `data[a..]`; além do fim, lê zeros (uma poslist corrompida não
/// derruba o processo).
fn varint_at(data: &[u8], a: usize) -> (usize, i32) {
    let (n, v) = fts5_get_varint32(data.get(a..).unwrap_or(&[]));
    (n as usize, v as i32)
}

/// `fts5ApiPhraseNext`.
fn phrase_next(p_iter: &mut Fts5PhraseIter, pi_col: &mut i32, pi_off: &mut i32) {
    if p_iter.a >= p_iter.b {
        *pi_col = -1;
        *pi_off = -1;
    } else {
        let (n, mut i_val) = varint_at(&p_iter.data, p_iter.a);
        p_iter.a += n;
        if i_val == 1 {
            let (n, v) = varint_at(&p_iter.data, p_iter.a);
            p_iter.a += n;
            *pi_col = v;
            *pi_off = 0;
            let (n, v) = varint_at(&p_iter.data, p_iter.a);
            p_iter.a += n;
            i_val = v;
        }
        *pi_off += i_val - 2;
    }
}

/// `fts5ApiPhraseNextColumn`.
fn phrase_next_column(e_detail: i32, p_iter: &mut Fts5PhraseIter, pi_col: &mut i32) {
    if e_detail == FTS5_DETAIL_COLUMNS {
        if p_iter.a >= p_iter.b {
            *pi_col = -1;
        } else {
            let (n, v) = varint_at(&p_iter.data, p_iter.a);
            p_iter.a += n;
            *pi_col += v - 2;
        }
    } else {
        loop {
            if p_iter.a >= p_iter.b {
                *pi_col = -1;
                return;
            }
            if p_iter.data.get(p_iter.a).copied().unwrap_or(0) == 0x01 {
                break;
            }
            let (n, _) = varint_at(&p_iter.data, p_iter.a);
            p_iter.a += n;
        }
        let (n, v) = varint_at(&p_iter.data, p_iter.a + 1);
        p_iter.a += 1 + n;
        *pi_col = v;
    }
}

// ---------------------------------------------------------------------------------------------
// A API de extensão (o `Fts5Context*` do C)
// ---------------------------------------------------------------------------------------------

/// O `Fts5ExtensionApi` de um cursor: empresta a conexão, a tabela e o cursor.
struct CsrApi<'a> {
    db: &'a mut Connection,
    tab: &'a mut Fts5FullTable,
    csr: &'a mut Fts5Cursor,
}

impl Fts5ExtensionApi for CsrApi<'_> {
    /// `fts5ApiUserData`: o `Fts5ExtensionFunction` guarda os próprios dados; nada a devolver.
    fn x_user_data(&self) -> Option<Rc<dyn Any>> {
        None
    }

    /// `fts5ApiColumnCount`.
    fn x_column_count(&mut self) -> i32 {
        self.tab.config.n_col()
    }

    /// `fts5ApiRowCount`.
    fn x_row_count(&mut self, pn_row: &mut i64) -> i32 {
        let t = &mut *self.tab;
        t.storage.row_count(&mut *self.db, &t.config, &mut t.index, pn_row)
    }

    /// `fts5ApiColumnTotalSize`.
    fn x_column_total_size(&mut self, i_col: i32, pn_token: &mut i64) -> i32 {
        let t = &mut *self.tab;
        t.storage.size(&mut *self.db, &t.config, &mut t.index, i_col, pn_token)
    }

    /// `fts5ApiTokenize`.
    fn x_tokenize(&mut self, text: &[u8], x_token: Fts5ApiTokenFn<'_>) -> i32 {
        let Some(tok) = self.tab.config.p_tok.clone() else {
            return SQLITE_OK;
        };
        tok.tokenize(FTS5_TOKENIZE_AUX, text, &mut |tflags, p_token, i_start, i_end| {
            x_token(&mut *self, tflags, p_token, i_start, i_end)
        })
    }

    /// `fts5ApiPhraseCount`.
    fn x_phrase_count(&mut self) -> i32 {
        self.csr.p_expr.as_ref().map_or(0, |e| e.phrase_count())
    }

    /// `fts5ApiPhraseSize`.
    fn x_phrase_size(&mut self, i_phrase: i32) -> i32 {
        self.csr.p_expr.as_ref().map_or(0, |e| e.phrase_size(i_phrase))
    }

    /// `fts5ApiInstCount`.
    fn x_inst_count(&mut self, pn_inst: &mut i32) -> i32 {
        let rc = ensure_inst(&mut *self.db, &mut *self.tab, &mut *self.csr);
        if rc == SQLITE_OK {
            *pn_inst = self.csr.a_inst.len() as i32;
        }
        rc
    }

    /// `fts5ApiInst`.
    fn x_inst(&mut self, i_idx: i32, pi_phrase: &mut i32, pi_col: &mut i32, pi_off: &mut i32) -> i32 {
        match inst_at(&mut *self.db, &mut *self.tab, &mut *self.csr, i_idx) {
            Ok(a) => {
                *pi_phrase = a[0];
                *pi_col = a[1];
                *pi_off = a[2];
                SQLITE_OK
            }
            Err(rc) => rc,
        }
    }

    /// `fts5ApiRowid`.
    fn x_rowid(&mut self) -> i64 {
        csr_rowid(self.csr)
    }

    /// `fts5ApiColumnText`.
    fn x_column_text(&mut self, i_col: i32) -> Result<Option<Vec<u8>>, i32> {
        csr_column_text(&mut *self.db, &mut *self.tab, &mut *self.csr, i_col)
    }

    /// `fts5ApiColumnSize`.
    fn x_column_size(&mut self, i_col: i32, pn_token: &mut i32) -> i32 {
        let mut rc = SQLITE_OK;
        let n_col = self.tab.config.n_col();

        if self.csr.csrflags & FTS5CSR_REQUIRE_DOCSIZE != 0 {
            if self.tab.config.b_columnsize != 0 {
                let i_rowid = csr_rowid(self.csr);
                let t = &mut *self.tab;
                rc = t.storage.docsize(&mut *self.db, &mut t.config, i_rowid, &mut self.csr.a_column_size);
            } else if self.tab.config.z_content.is_none() {
                for i in 0..n_col as usize {
                    if self.tab.config.ab_unindexed[i] == 0 {
                        self.csr.a_column_size[i] = -1;
                    }
                }
            } else {
                let mut i = 0;
                while rc == SQLITE_OK && i < n_col {
                    if self.tab.config.ab_unindexed[i as usize] == 0 {
                        self.csr.a_column_size[i as usize] = 0;
                        match csr_column_text(&mut *self.db, &mut *self.tab, &mut *self.csr, i) {
                            Ok(z) => {
                                let mut cnt = 0;
                                rc = self.tab.config.tokenize(
                                    FTS5_TOKENIZE_AUX,
                                    z.as_deref(),
                                    &mut |tflags, _, _, _| {
                                        if (tflags & FTS5_TOKEN_COLOCATED) == 0 {
                                            cnt += 1;
                                        }
                                        SQLITE_OK
                                    },
                                );
                                self.csr.a_column_size[i as usize] = cnt;
                            }
                            Err(e) => rc = e,
                        }
                    }
                    i += 1;
                }
            }
            self.csr.csrflags &= !FTS5CSR_REQUIRE_DOCSIZE;
        }
        if i_col < 0 {
            *pn_token = 0;
            for i in 0..n_col as usize {
                *pn_token += self.csr.a_column_size[i];
            }
        } else if i_col < n_col {
            *pn_token = self.csr.a_column_size[i_col as usize];
        } else {
            *pn_token = 0;
            rc = SQLITE_RANGE;
        }
        rc
    }

    /// `fts5ApiQueryPhrase`.
    fn x_query_phrase(
        &mut self,
        i_phrase: i32,
        x_callback: &mut dyn FnMut(&mut dyn Fts5ExtensionApi) -> i32,
    ) -> i32 {
        let mut new = Fts5Cursor::new(self.tab.global.next_id(), self.tab.config.n_col());
        new.e_plan = FTS5_PLAN_MATCH;
        new.i_first_rowid = SMALLEST_INT64;
        new.i_last_rowid = LARGEST_INT64;

        let mut rc = SQLITE_OK;
        match self.csr.p_expr.as_ref() {
            Some(e) => match e.clone_phrase(&self.tab.config, i_phrase) {
                Ok(c) => new.p_expr = Some(c),
                Err(e) => rc = e,
            },
            None => rc = SQLITE_RANGE,
        }

        if rc == SQLITE_OK {
            rc = csr_first(&mut *self.db, &mut *self.tab, &mut new, false);
            while rc == SQLITE_OK && new.csrflags & FTS5CSR_EOF == 0 {
                let mut api = CsrApi { db: &mut *self.db, tab: &mut *self.tab, csr: &mut new };
                rc = x_callback(&mut api);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                    }
                    break;
                }
                rc = csr_next(&mut *self.db, &mut *self.tab, &mut new);
            }
        }

        free_cursor_components(&mut *self.db, &mut *self.tab, &mut new);
        rc
    }

    /// `fts5ApiSetAuxdata`.
    fn x_set_auxdata(&mut self, p_aux: Option<Rc<dyn Any>>) -> i32 {
        let cur = self.csr.p_aux;
        match self.csr.p_auxdata.iter().position(|d| d.p_aux == cur) {
            Some(pos) => self.csr.p_auxdata[pos].p_ptr = p_aux,
            None => self.csr.p_auxdata.insert(0, Fts5Auxdata { p_aux: cur, p_ptr: p_aux }),
        }
        SQLITE_OK
    }

    /// `fts5ApiGetAuxdata`.
    fn x_get_auxdata(&mut self, b_clear: bool) -> Option<Rc<dyn Any>> {
        let cur = self.csr.p_aux;
        let d = self.csr.p_auxdata.iter_mut().find(|d| d.p_aux == cur)?;
        if b_clear {
            d.p_ptr.take()
        } else {
            d.p_ptr.clone()
        }
    }

    /// `fts5ApiPhraseFirst`.
    fn x_phrase_first(
        &mut self,
        i_phrase: i32,
        p_iter: &mut Fts5PhraseIter,
        pi_col: &mut i32,
        pi_off: &mut i32,
    ) -> i32 {
        match csr_poslist(&mut *self.db, &mut *self.tab, &mut *self.csr, i_phrase) {
            Ok(a) => {
                load_phrase_iter(p_iter, a);
                *pi_col = 0;
                *pi_off = 0;
                phrase_next(p_iter, pi_col, pi_off);
                SQLITE_OK
            }
            Err(e) => e,
        }
    }

    /// `fts5ApiPhraseNext`.
    fn x_phrase_next(&mut self, p_iter: &mut Fts5PhraseIter, pi_col: &mut i32, pi_off: &mut i32) {
        phrase_next(p_iter, pi_col, pi_off);
    }

    /// `fts5ApiPhraseFirstColumn`.
    fn x_phrase_first_column(
        &mut self,
        i_phrase: i32,
        p_iter: &mut Fts5PhraseIter,
        pi_col: &mut i32,
    ) -> i32 {
        let e_detail = self.tab.config.e_detail;
        if e_detail == FTS5_DETAIL_COLUMNS {
            let data: Result<Vec<u8>, i32> = match self.csr.p_sorter.as_ref() {
                Some(sorter) => sorter.phrase_list(i_phrase).ok_or(SQLITE_RANGE),
                None => match self.csr.p_expr.as_ref() {
                    Some(e) if i_phrase >= 0 && i_phrase < e.phrase_count() => {
                        Ok(e.phrase_collist(i_phrase))
                    }
                    _ => Err(SQLITE_RANGE),
                },
            };
            match data {
                Ok(d) => {
                    load_phrase_iter(p_iter, d);
                    *pi_col = 0;
                    phrase_next_column(e_detail, p_iter, pi_col);
                    SQLITE_OK
                }
                Err(e) => e,
            }
        } else {
            match csr_poslist(&mut *self.db, &mut *self.tab, &mut *self.csr, i_phrase) {
                Ok(a) => {
                    let n = a.len();
                    load_phrase_iter(p_iter, a);
                    if n == 0 {
                        *pi_col = -1;
                    } else if p_iter.data[0] == 0x01 {
                        let (nb, v) = varint_at(&p_iter.data, 1);
                        p_iter.a = 1 + nb;
                        *pi_col = v;
                    } else {
                        *pi_col = 0;
                    }
                    SQLITE_OK
                }
                Err(e) => e,
            }
        }
    }

    /// `fts5ApiPhraseNextColumn`.
    fn x_phrase_next_column(&mut self, p_iter: &mut Fts5PhraseIter, pi_col: &mut i32) {
        phrase_next_column(self.tab.config.e_detail, p_iter, pi_col);
    }

    /// `fts5ApiQueryToken`.
    fn x_query_token(&mut self, i_phrase: i32, i_token: i32) -> Result<Vec<u8>, i32> {
        match self.csr.p_expr.as_ref() {
            Some(e) => e.query_token(i_phrase, i_token),
            None => Err(SQLITE_RANGE),
        }
    }

    /// `fts5ApiInstToken`.
    fn x_inst_token(&mut self, i_idx: i32, i_token: i32) -> Result<Vec<u8>, i32> {
        let [i_phrase, i_col, i_off] =
            inst_at(&mut *self.db, &mut *self.tab, &mut *self.csr, i_idx)?;
        let i_rowid = csr_rowid(self.csr);
        match self.csr.p_expr.as_ref() {
            Some(e) => e
                .inst_token(&self.tab.config, i_rowid, i_phrase, i_col, i_off, i_token)
                .map(|t| t.unwrap_or_default()),
            None => Err(SQLITE_RANGE),
        }
    }
}

/// `fts5ApiInvoke`: roda a função auxiliar `i_aux` sobre o cursor.
fn api_invoke(
    db: &mut Connection,
    tab: &mut Fts5FullTable,
    csr: &mut Fts5Cursor,
    i_aux: usize,
    args: &[Mem],
) -> Fts5AuxResult {
    let func = tab.global.ap_aux[i_aux].x_func.clone();
    debug_assert!(csr.p_aux.is_none());
    csr.p_aux = Some(i_aux);
    let res = {
        let mut api = CsrApi { db: &mut *db, tab: &mut *tab, csr: &mut *csr };
        func.call(&mut api, args)
    };
    csr.p_aux = None;
    res
}

/// Aplica o resultado de uma função auxiliar ao `sqlite3_context`.
fn apply_aux_result(ctx: &mut Context<'_>, res: Fts5AuxResult) {
    match res {
        Fts5AuxResult::Null => {}
        Fts5AuxResult::Double(d) => result_double(ctx, d),
        Fts5AuxResult::Text(t) => result_text(ctx, Some(&t), t.len() as i32, StrDtor::Transient),
        Fts5AuxResult::Error(m) => result_error(ctx, &m, m.len() as i32),
        Fts5AuxResult::ErrorCode(rc) => result_error_code(ctx, rc),
    }
}

/// `fts5ApiCallback`: a função SQL que sobrecarrega uma função auxiliar (`bm25(tbl, ...)`).
fn fts5_api_callback(ctx: &mut Context<'_>, args: &[Mem]) {
    let UserData::Ptr(p) = user_data(ctx) else {
        return;
    };
    let Some(p_aux) = p.downcast_ref::<Fts5AuxRef>() else {
        return;
    };
    if args.is_empty() {
        return;
    }
    let i_csr_id = value_int64(&args[0]);
    let i_aux = p_aux.i_aux;

    let mut res: Option<Fts5AuxResult> = None;
    if let Some(vid) = fts5_table_from_csrid(&mut *ctx.db, i_csr_id) {
        res = with_vtab(&mut *ctx.db, vid, |db, vtab| {
            let tab = tab_of(vtab);
            with_csr(tab, i_csr_id, |tab, csr| {
                if csr.e_plan == 0 {
                    None
                } else {
                    Some(api_invoke(db, tab, csr, i_aux, &args[1..]))
                }
            })
            .flatten()
        })
        .flatten();
    }
    match res {
        Some(r) => apply_aux_result(ctx, r),
        None => {
            let z_err = mprintf(b"no such cursor: %lld", &[PrintfArg::Int(i_csr_id)]).unwrap_or_default();
            result_error(ctx, &z_err, z_err.len() as i32);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// xColumn
// ---------------------------------------------------------------------------------------------

/// `fts5ColumnMethod`.
fn csr_column(tab: &mut Fts5FullTable, csr: &mut Fts5Cursor, ctx: &mut Context<'_>, i_col: i32) -> i32 {
    let mut rc = SQLITE_OK;
    let n_col = tab.config.n_col();
    debug_assert!(csr.csrflags & FTS5CSR_EOF == 0);

    if csr.e_plan == FTS5_PLAN_SPECIAL {
        if i_col == n_col {
            result_int64(ctx, csr.i_special);
        }
    } else if i_col == n_col {
        /* A coluna especial com o nome da tabela: o id do cursor, que só serve de primeiro
        ** argumento de uma função auxiliar. */
        result_int64(ctx, csr.i_csr_id);
    } else if i_col == n_col + 1 {
        /* A coluna "rank". */
        if csr.e_plan == FTS5_PLAN_SOURCE {
            let (_rc, blob) = poslist_blob(tab, csr);
            crate::vdbeapi::result_blob(ctx, Some(&blob), blob.len() as i32, StrDtor::Transient);
        } else if csr.e_plan == FTS5_PLAN_MATCH || csr.e_plan == FTS5_PLAN_SORTED_MATCH {
            if csr.p_rank.is_some() || {
                rc = find_rank_function(&mut *ctx.db, tab, csr);
                rc == SQLITE_OK
            } {
                if let Some(i_aux) = csr.p_rank {
                    let args = std::mem::take(&mut csr.ap_rank_arg);
                    let res = api_invoke(&mut *ctx.db, tab, csr, i_aux, &args);
                    csr.ap_rank_arg = args;
                    apply_aux_result(ctx, res);
                }
            }
        }
    } else if !tab.is_contentless() {
        tab.arm_errmsg();
        rc = seek_cursor(&mut *ctx.db, tab, csr, true);
        if rc == SQLITE_OK {
            let stmt = csr.p_stmt.unwrap_or_default();
            let v = column_value(&mut *ctx.db, stmt, i_col + 1);
            result_value(ctx, &v);
        }
        tab.disarm_errmsg();
    } else if tab.config.b_contentless_delete != 0 && vtab_nochange(ctx) {
        let z_err = mprintf(
            b"cannot UPDATE a subset of columns on fts5 contentless-delete table: %s",
            &[text_arg(&tab.config.z_name)],
        )
        .unwrap_or_default();
        result_error(ctx, &z_err, z_err.len() as i32);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// O cursor do núcleo
// ---------------------------------------------------------------------------------------------

impl VtabCursor for Fts5CsrHandle {
    /// `fts5CloseMethod`.
    fn close(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        if let Some(pos) = tab.cursors.iter().position(|c| c.i_csr_id == self.id) {
            let mut csr = tab.cursors.swap_remove(pos);
            free_cursor_components(db, tab, &mut csr);
        }
        SQLITE_OK
    }

    /// `fts5FilterMethod`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let tab = tab_of(vtab);
        with_csr(tab, self.id, |tab, csr| {
            csr_filter(db, tab, csr, idx_num, idx_str.unwrap_or(&[]), argv)
        })
        .unwrap_or(SQLITE_ERROR)
    }

    /// `fts5NextMethod`.
    fn next(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        with_csr(tab, self.id, |tab, csr| csr_next(db, tab, csr)).unwrap_or(SQLITE_ERROR)
    }

    /// `fts5EofMethod`.
    fn eof(&mut self, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        match tab.cursors.iter().find(|c| c.i_csr_id == self.id) {
            Some(c) => (c.csrflags & FTS5CSR_EOF != 0) as i32,
            None => 1,
        }
    }

    /// `fts5ColumnMethod`.
    fn column(&mut self, vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let tab = tab_of(vtab);
        with_csr(tab, self.id, |tab, csr| csr_column(tab, csr, ctx, i)).unwrap_or(SQLITE_ERROR)
    }

    /// `fts5RowidMethod`.
    fn rowid(&mut self, vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        let tab = tab_of(vtab);
        let Some(csr) = tab.cursors.iter().find(|c| c.i_csr_id == self.id) else {
            return SQLITE_ERROR;
        };
        debug_assert!(csr.csrflags & FTS5CSR_EOF == 0);
        match csr.e_plan {
            FTS5_PLAN_SPECIAL => *p_rowid = 0,
            FTS5_PLAN_SOURCE | FTS5_PLAN_MATCH | FTS5_PLAN_SORTED_MATCH => {
                *p_rowid = csr_rowid(csr);
            }
            _ => *p_rowid = csr.i_scan_rowid,
        }
        SQLITE_OK
    }
}

// ---------------------------------------------------------------------------------------------
// A tabela virtual
// ---------------------------------------------------------------------------------------------

/// `fts5SetUniqueFlag`.
fn set_unique_flag(info: &mut IndexInfo) {
    info.idx_flags |= SQLITE_INDEX_SCAN_UNIQUE;
}

/// `fts5UsePatternMatch`.
fn use_pattern_match(e_pattern: i32, op: i32) -> bool {
    debug_assert!(FTS5_PATTERN_GLOB == SQLITE_INDEX_CONSTRAINT_GLOB);
    debug_assert!(FTS5_PATTERN_LIKE == SQLITE_INDEX_CONSTRAINT_LIKE);
    if e_pattern == FTS5_PATTERN_GLOB && op == FTS5_PATTERN_GLOB {
        return true;
    }
    if e_pattern == FTS5_PATTERN_LIKE && (op == FTS5_PATTERN_LIKE || op == FTS5_PATTERN_GLOB) {
        return true;
    }
    false
}

impl Vtab for Fts5FullTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `fts5BestIndexMethod`: procura, no WHERE, um MATCH contra a coluna da tabela, um MATCH
    /// contra `rank`, um MATCH contra outra coluna, `rowid ==`, `rowid <` ou `<=` e `rowid >` ou
    /// `>=`; no ORDER BY, `rank` e `rowid`. O `idxNum` é uma máscara de `FTS5_BI_ORDER_*`; o
    /// `idxStr` leva um item por argumento do `xFilter`: `m` (MATCH na tabela), `r` (rank),
    /// `M<col>` (outra coluna), `L<col>`/`G<col>` (LIKE/GLOB), `=`, `<` e `>`.
    ///
    /// Custos: com MATCH, sem outras restrições 1000, uma restrição de faixa de rowid 750, as duas
    /// 500, rowid `==` 100; sem MATCH, 1000000, 750000, 250000 e 10. Um MATCH inutilizável dá
    /// 1e50.
    fn best_index(&mut self, _db: &mut Connection, p_info: &mut IndexInfo) -> i32 {
        let n_col = self.config.n_col();
        let e_pattern = self.config.e_pattern;
        let b_tokendata = self.config.b_tokendata != 0;
        let mut idx_flags = 0;
        let mut idx_str: Vec<u8> = Vec::new();
        let mut i_cons = 0;

        let mut b_seen_eq = false;
        let mut b_seen_gt = false;
        let mut b_seen_lt = false;
        let mut b_seen_match = false;
        let mut b_seen_rank = false;

        if self.config.b_lock != 0 {
            self.z_err_msg = mprintf(b"recursively defined fts5 content table", &[]);
            return SQLITE_ERROR;
        }

        let n_constraint = p_info.a_constraint.len();
        if p_info.a_constraint_usage.len() < n_constraint {
            p_info.a_constraint_usage.resize(n_constraint, IndexConstraintUsage::default());
        }

        for i in 0..n_constraint {
            let p = p_info.a_constraint[i];
            let i_col = p.i_column;
            let op = p.op as i32;
            if op == SQLITE_INDEX_CONSTRAINT_MATCH || (op == SQLITE_INDEX_CONSTRAINT_EQ && i_col >= n_col) {
                /* Um MATCH ou equivalente */
                if !p.usable || i_col < 0 {
                    /* Como há um MATCH inutilizável, o plano é inutilizável: custo proibitivo. */
                    p_info.estimated_cost = 1e50;
                    p_info.idx_str = Some(idx_str);
                    return SQLITE_OK;
                } else {
                    if i_col == n_col + 1 {
                        if b_seen_rank {
                            continue;
                        }
                        idx_str.push(b'r');
                        b_seen_rank = true;
                    } else if i_col >= 0 {
                        b_seen_match = true;
                        idx_str.push(b'M');
                        idx_str.extend_from_slice(i_col.to_string().as_bytes());
                    }
                    i_cons += 1;
                    p_info.a_constraint_usage[i].argv_index = i_cons;
                    p_info.a_constraint_usage[i].omit = true;
                }
            } else if p.usable {
                if i_col >= 0 && i_col < n_col && use_pattern_match(e_pattern, op) {
                    debug_assert!(op == FTS5_PATTERN_LIKE || op == FTS5_PATTERN_GLOB);
                    idx_str.push(if op == FTS5_PATTERN_LIKE { b'L' } else { b'G' });
                    idx_str.extend_from_slice(i_col.to_string().as_bytes());
                    i_cons += 1;
                    p_info.a_constraint_usage[i].argv_index = i_cons;
                } else if !b_seen_eq && op == SQLITE_INDEX_CONSTRAINT_EQ && i_col < 0 {
                    idx_str.push(b'=');
                    b_seen_eq = true;
                    i_cons += 1;
                    p_info.a_constraint_usage[i].argv_index = i_cons;
                }
            }
        }

        if !b_seen_eq {
            for i in 0..n_constraint {
                let p = p_info.a_constraint[i];
                if p.i_column < 0 && p.usable {
                    let op = p.op as i32;
                    if op == SQLITE_INDEX_CONSTRAINT_LT || op == SQLITE_INDEX_CONSTRAINT_LE {
                        if b_seen_lt {
                            continue;
                        }
                        idx_str.push(b'<');
                        i_cons += 1;
                        p_info.a_constraint_usage[i].argv_index = i_cons;
                        b_seen_lt = true;
                    } else if op == SQLITE_INDEX_CONSTRAINT_GT || op == SQLITE_INDEX_CONSTRAINT_GE {
                        if b_seen_gt {
                            continue;
                        }
                        idx_str.push(b'>');
                        i_cons += 1;
                        p_info.a_constraint_usage[i].argv_index = i_cons;
                        b_seen_gt = true;
                    }
                }
            }
        }
        p_info.idx_str = Some(idx_str);

        /* O ORDER BY: as tabelas `tokendata=1` ainda não tratam `ORDER BY rowid DESC`. */
        if p_info.a_order_by.len() == 1 {
            let i_sort = p_info.a_order_by[0].i_column;
            if i_sort == n_col + 1 && b_seen_match {
                idx_flags |= FTS5_BI_ORDER_RANK;
            } else if i_sort == -1 && (!p_info.a_order_by[0].desc || !b_tokendata) {
                idx_flags |= FTS5_BI_ORDER_ROWID;
            }
            if idx_flags & (FTS5_BI_ORDER_RANK | FTS5_BI_ORDER_ROWID) != 0 {
                p_info.order_by_consumed = 1;
                if p_info.a_order_by[0].desc {
                    idx_flags |= FTS5_BI_ORDER_DESC;
                }
            }
        }

        /* O custo estimado a partir do que foi visto. */
        if b_seen_eq {
            p_info.estimated_cost = if b_seen_match { 100.0 } else { 10.0 };
            if !b_seen_match {
                set_unique_flag(p_info);
            }
        } else if b_seen_lt && b_seen_gt {
            p_info.estimated_cost = if b_seen_match { 500.0 } else { 250000.0 };
        } else if b_seen_lt || b_seen_gt {
            p_info.estimated_cost = if b_seen_match { 750.0 } else { 750000.0 };
        } else {
            p_info.estimated_cost = if b_seen_match { 1000.0 } else { 1000000.0 };
        }

        p_info.idx_num = idx_flags;
        SQLITE_OK
    }

    /// `fts5DisconnectMethod`.
    fn disconnect(&mut self, db: &mut Connection) -> i32 {
        self.free_vtab(db);
        SQLITE_OK
    }

    /// `fts5DestroyMethod`.
    fn destroy(&mut self, db: &mut Connection) -> i32 {
        let rc = fts5_drop_all(db, &self.config);
        if rc == SQLITE_OK {
            self.free_vtab(db);
        }
        rc
    }

    /// `fts5OpenMethod`.
    fn open(&mut self, db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        let rc = self.new_transaction(db);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        let id = self.global.next_id();
        self.cursors.push(Fts5Cursor::new(id, self.config.n_col()));
        Ok(Box::new(Fts5CsrHandle { id }))
    }

    /// `fts5UpdateMethod`: o `xUpdate`. Um DELETE passa um argumento (o rowid); um UPDATE ou INSERT
    /// passa: o rowid antigo ou NULL, o rowid novo, os valores das `nCol` colunas e os das duas
    /// colunas ocultas (o nome da tabela e `rank`).
    fn update(&mut self, db: &mut Connection, ap_val: &[Mem], p_rowid: &mut i64) -> i32 {
        let n_arg = ap_val.len();
        let n_col = self.config.n_col() as usize;
        let mut rc = SQLITE_OK;
        let mut b_update_or_delete = false;

        debug_assert!(n_arg == 1 || n_arg == 2 + n_col + 2);
        debug_assert!(self.z_err_msg.is_none());
        debug_assert!(!self.config.errmsg_target);
        if self.config.pgsz == 0 {
            rc = self.index.load_config(db, &mut self.config);
            if rc != SQLITE_OK {
                return rc;
            }
        }

        self.arm_errmsg();

        /* Põe os cursores ativos em REQUIRE_SEEK. */
        self.trip_cursors();

        let e_type0 = value_type(&ap_val[0]);
        if e_type0 == SQLITE_NULL && value_type(&ap_val[2 + n_col]) != SQLITE_NULL {
            /* Um INSERT "especial", tratado à parte. */
            let z: Vec<u8> = text_of(&ap_val[2 + n_col]).map(|t| t.into_owned()).unwrap_or_default();
            if self.config.e_content != FTS5_CONTENT_NORMAL && 0 == str_icmp(b"delete", &z) {
                if self.config.b_contentless_delete != 0 {
                    self.set_vtab_error(b"'delete' may not be used with a contentless_delete=1 table", &[]);
                    rc = SQLITE_ERROR;
                } else {
                    rc = self.special_delete(db, ap_val);
                    b_update_or_delete = true;
                }
            } else {
                rc = self.special_insert(db, &z, &ap_val[2 + n_col + 1]);
            }
        } else {
            /* Um INSERT, UPDATE ou DELETE comum. O conflito de rowid precisa ser detectado antes
            ** de qualquer alteração no arquivo. Quatro casos: DELETE, UPDATE (rowid igual), UPDATE
            ** (rowid alterado) e INSERT; os dois últimos podem violar a restrição de rowid. */
            let mut e_conflict = crate::consts::SQLITE_ABORT;
            if self.config.e_content == FTS5_CONTENT_NORMAL || self.config.b_contentless_delete != 0 {
                e_conflict = vtab_on_conflict(db);
            }

            debug_assert!(e_type0 == SQLITE_INTEGER || e_type0 == SQLITE_NULL);
            debug_assert!(n_arg != 1 || e_type0 == SQLITE_INTEGER);

            /* UPDATE e DELETE em tabelas sem conteúdo não existem (salvo `contentless_delete=1`). */
            if e_type0 == SQLITE_INTEGER
                && self.config.e_content == FTS5_CONTENT_NONE
                && self.config.b_contentless_delete == 0
            {
                self.z_err_msg = mprintf(
                    b"cannot %s contentless fts5 table: %s",
                    &[
                        text_arg(if n_arg > 1 { b"UPDATE" } else { b"DELETE from" }),
                        text_arg(&self.config.z_name),
                    ],
                );
                rc = SQLITE_ERROR;
            } else if n_arg == 1 {
                /* DELETE */
                let i_del = value_int64(&ap_val[0]);
                rc = self.storage.delete(db, &mut self.config, &mut self.index, i_del, None);
                b_update_or_delete = true;
            } else {
                /* INSERT ou UPDATE */
                let mut v1 = ap_val[1].clone();
                let e_type1 = value_numeric_type(&mut v1);

                if e_type1 != SQLITE_INTEGER && e_type1 != SQLITE_NULL {
                    rc = SQLITE_MISMATCH;
                } else if e_type0 != SQLITE_INTEGER {
                    /* Um INSERT. No modo REPLACE, remove antes a linha atual (se há). */
                    if e_conflict == SQLITE_REPLACE && e_type1 == SQLITE_INTEGER {
                        let i_new = value_int64(&v1);
                        rc = self.storage.delete(db, &mut self.config, &mut self.index, i_new, None);
                        b_update_or_delete = true;
                    }
                    self.storage_insert(db, &mut rc, ap_val, p_rowid);
                } else {
                    /* Um UPDATE */
                    let i_old = value_int64(&ap_val[0]); /* Rowid antigo */
                    let i_new = value_int64(&v1); /* Rowid novo */
                    if e_type1 == SQLITE_INTEGER && i_old != i_new {
                        if e_conflict == SQLITE_REPLACE {
                            rc = self.storage.delete(db, &mut self.config, &mut self.index, i_old, None);
                            if rc == SQLITE_OK {
                                rc = self.storage.delete(db, &mut self.config, &mut self.index, i_new, None);
                            }
                            self.storage_insert(db, &mut rc, ap_val, p_rowid);
                        } else {
                            rc = self.storage.content_insert(db, &mut self.config, ap_val, p_rowid);
                            if rc == SQLITE_OK {
                                rc = self.storage.delete(db, &mut self.config, &mut self.index, i_old, None);
                            }
                            if rc == SQLITE_OK {
                                rc = self.storage.index_insert(
                                    db,
                                    &mut self.config,
                                    &mut self.index,
                                    ap_val,
                                    *p_rowid,
                                );
                            }
                        }
                    } else {
                        rc = self.storage.delete(db, &mut self.config, &mut self.index, i_old, None);
                        self.storage_insert(db, &mut rc, ap_val, p_rowid);
                    }
                    b_update_or_delete = true;
                }
            }
        }

        if rc == SQLITE_OK
            && b_update_or_delete
            && self.config.b_secure_delete != 0
            && self.config.i_version == FTS5_CURRENT_VERSION
        {
            rc = self.storage.config_value(
                db,
                &mut self.config,
                &mut self.index,
                b"version",
                None,
                FTS5_CURRENT_VERSION_SECUREDELETE,
            );
            if rc == SQLITE_OK {
                self.config.i_version = FTS5_CURRENT_VERSION_SECUREDELETE;
            }
        }

        self.disarm_errmsg();
        rc
    }

    /// `fts5BeginMethod`.
    fn begin(&mut self, db: &mut Connection) -> i32 {
        self.new_transaction(db);
        SQLITE_OK
    }

    /// `fts5SyncMethod`.
    fn sync(&mut self, db: &mut Connection) -> i32 {
        self.arm_errmsg();
        let rc = self.flush_to_disk(db);
        self.disarm_errmsg();
        rc
    }

    /// `fts5CommitMethod`: o que estava na memória já foi gravado pelo `xSync`.
    fn commit(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `fts5RollbackMethod`: descarta o que está pendente no hash.
    fn rollback(&mut self, db: &mut Connection) -> i32 {
        self.storage.rollback(db, &mut self.index)
    }

    /// `fts5FindFunctionMethod`.
    fn find_function(
        &mut self,
        _db: &mut Connection,
        _n_arg: i32,
        z_name: &[u8],
        out: &mut Option<(ScalarFn, UserData)>,
    ) -> i32 {
        match self.global.find_aux(z_name) {
            Some(i_aux) => {
                let r: Rc<dyn Any> = Rc::new(Fts5AuxRef { global: self.global.clone(), i_aux });
                *out = Some((fts5_api_callback as ScalarFn, UserData::Ptr(r)));
                1
            }
            None => 0,
        }
    }

    /// `fts5RenameMethod`.
    fn rename(&mut self, db: &mut Connection, z_name: &[u8]) -> i32 {
        self.storage.rename(db, &mut self.config, &mut self.index, z_name)
    }

    /// `fts5SavepointMethod`: grava o hash pendente.
    fn savepoint(&mut self, db: &mut Connection, i_savepoint: i32) -> i32 {
        let rc = self.flush_to_disk(db);
        if rc == SQLITE_OK {
            self.i_savepoint = i_savepoint + 1;
        }
        rc
    }

    /// `fts5ReleaseMethod`.
    fn release(&mut self, db: &mut Connection, i_savepoint: i32) -> i32 {
        let mut rc = SQLITE_OK;
        if (i_savepoint + 1) < self.i_savepoint {
            rc = self.flush_to_disk(db);
            if rc == SQLITE_OK {
                self.i_savepoint = i_savepoint;
            }
        }
        rc
    }

    /// `fts5RollbackToMethod`: descarta o hash pendente.
    fn rollback_to(&mut self, db: &mut Connection, i_savepoint: i32) -> i32 {
        let mut rc = SQLITE_OK;
        self.trip_cursors();
        if (i_savepoint + 1) <= self.i_savepoint {
            self.config.pgsz = 0;
            rc = self.storage.rollback(db, &mut self.index);
        }
        rc
    }

    /// `fts5IntegrityMethod`.
    fn integrity(
        &mut self,
        db: &mut Connection,
        z_schema: &[u8],
        z_tabname: &[u8],
        _flags: i32,
        pz_err: &mut Option<Vec<u8>>,
    ) -> i32 {
        debug_assert!(pz_err.is_none());
        debug_assert!(!self.config.errmsg_target);
        self.config.errmsg_target = true;
        let mut rc = self.storage.integrity(db, &mut self.config, &mut self.index, 0);
        self.config.errmsg_target = false;
        if let Some(m) = self.config.errmsg.take() {
            *pz_err = Some(m);
        }
        if pz_err.is_none() && rc != SQLITE_OK {
            if (rc & 0xff) == SQLITE_CORRUPT {
                *pz_err = mprintf(
                    b"malformed inverted index for FTS5 table %s.%s",
                    &[text_arg(z_schema), text_arg(z_tabname)],
                );
                rc = SQLITE_OK;
            } else {
                *pz_err = mprintf(
                    b"unable to validate the inverted index for FTS5 table %s.%s: %s",
                    &[
                        text_arg(z_schema),
                        text_arg(z_tabname),
                        text_arg(err_str(rc).as_bytes()),
                    ],
                );
            }
        }

        self.index.close_reader(db);
        rc
    }
}

// ---------------------------------------------------------------------------------------------
// O módulo
// ---------------------------------------------------------------------------------------------

/// `fts5InitVtab`: o trabalho de `xCreate` e `xConnect`. `argv[0]` é o nome do módulo,
/// `argv[1]` o banco, `argv[2]` a tabela e o resto são as colunas e opções.
fn fts5_init_vtab(
    b_create: bool,
    db: &mut Connection,
    p_aux: &Option<Rc<dyn Any>>,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Box<dyn Vtab>, i32> {
    let Some(global) = p_aux.as_ref().and_then(|a| Rc::clone(a).downcast::<Fts5Global>().ok()) else {
        return Err(SQLITE_ERROR);
    };

    /* Interpreta a configuração */
    let mut config = fts5_config_parse(&*global, argv, pz_err)?;

    /* Abre o subsistema de índice */
    let mut index = match Fts5Index::open(db, &mut config, b_create, pz_err) {
        Ok(i) => i,
        Err(rc) => return Err(rc),
    };

    /* Abre o subsistema de armazenamento */
    let mut storage = match Fts5Storage::open(db, &mut config, &mut index, b_create, pz_err) {
        Ok(s) => s,
        Err(rc) => {
            index.close(db);
            return Err(rc);
        }
    };

    /* sqlite3_declare_vtab() */
    let mut rc = config.declare_vtab(db);

    /* Carrega a configuração inicial */
    if rc == SQLITE_OK {
        debug_assert!(!config.errmsg_target);
        config.errmsg_target = true;
        rc = index.load_config(db, &mut config);
        index.rollback(db);
        config.errmsg_target = false;
        if let Some(m) = config.errmsg.take() {
            *pz_err = Some(m);
        }
    }

    if rc == SQLITE_OK && config.e_content == FTS5_CONTENT_NORMAL {
        rc = vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
    }
    if rc == SQLITE_OK {
        rc = vtab_config(db, SQLITE_VTAB_INNOCUOUS, 0);
    }

    if rc != SQLITE_OK {
        index.close(db);
        storage.close(db);
        return Err(rc);
    }
    Ok(Box::new(Fts5FullTable {
        config,
        index,
        storage,
        global,
        cursors: Vec::new(),
        n_taken: 0,
        i_savepoint: 0,
        z_err_msg: None,
    }))
}

/// `fts5Mod`: o módulo `fts5`.
struct Fts5Module;

impl VtabModule for Fts5Module {
    fn i_version(&self) -> i32 {
        4
    }

    fn caps(&self) -> ModuleCaps {
        ModuleCaps {
            create: true,
            update: true,
            begin: true,
            sync: true,
            commit: true,
            rollback: true,
            find_function: true,
            rename: true,
            savepoint: true,
            release: true,
            rollback_to: true,
            integrity: true,
        }
    }

    /// `fts5CreateMethod`.
    fn x_create(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts5_init_vtab(true, db, aux, argv, err)
    }

    /// `fts5ConnectMethod`.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        fts5_init_vtab(false, db, aux, argv, err)
    }

    /// `fts5ShadowName`.
    fn x_shadow_name(&self, z_name: &[u8]) -> bool {
        const AZ_NAME: [&[u8]; 5] = [b"config", b"content", b"data", b"docsize", b"idx"];
        AZ_NAME.iter().any(|n| str_icmp(z_name, n) == 0)
    }
}

/// `fts5Fts5Func`: `fts5(ptr)` grava o `Fts5Global` no valor ponteiro `"fts5_api_ptr"`.
fn fts5_fts5_func(ctx: &mut Context<'_>, ap_arg: &[Mem]) {
    let UserData::Ptr(p) = user_data(ctx) else {
        return;
    };
    let Ok(global) = Rc::clone(&p).downcast::<Fts5Global>() else {
        return;
    };
    debug_assert!(ap_arg.len() == 1);
    if let Some(slot) = ap_arg.first().and_then(|a| value_pointer(a, b"fts5_api_ptr")) {
        if let Some(slot) = slot.downcast_ref::<Fts5ApiSlot>() {
            slot.0.set(Some(global));
        }
    }
}

/// `fts5SourceIdFunc`: `fts5_source_id()`.
fn fts5_source_id_func(ctx: &mut Context<'_>, _ap_unused: &[Mem]) {
    let z = format!("fts5: {}", SQLITE_SOURCE_ID).into_bytes();
    result_text(ctx, Some(&z), z.len() as i32, StrDtor::Transient);
}

/// `fts5Init` (`sqlite3Fts5Init`): registra o módulo `fts5`, os tokenizadores e as funções
/// auxiliares embutidos, o módulo `fts5vocab` e as funções `fts5()` e `fts5_source_id()`.
pub fn fts5_init(db: &mut Connection) -> i32 {
    /* Monta o `Fts5Global` com o que o `sqlite3Fts5AuxInit` e o `sqlite3Fts5TokenizerInit`
    ** registram (nenhum dos dois toca a conexão). */
    let mut global = Fts5Global::default();
    let mut rc = fts5_aux_init(&mut global);
    if rc == SQLITE_OK {
        rc = fts5_tokenizer_init(&mut global);
    }
    if rc != SQLITE_OK {
        return rc;
    }
    let global = Rc::new(global);
    let p: Rc<dyn Any> = global.clone();

    rc = create_module(db, b"fts5", Some(Rc::new(Fts5Module)), Some(p.clone()), None);
    if rc == SQLITE_OK {
        rc = fts5_index_init(db);
    }
    if rc == SQLITE_OK {
        rc = super::expr::fts5_expr_init(db);
    }
    /* `fts5CreateAux`: sobrecarrega o nome de cada função auxiliar, na ordem de registro. */
    let mut i = 0;
    while rc == SQLITE_OK && i < global.ap_aux.len() {
        rc = overload_function(db, &global.ap_aux[i].z_func, -1);
        i += 1;
    }
    if rc == SQLITE_OK {
        rc = fts5_vocab_init(db, p.clone());
    }
    if rc == SQLITE_OK {
        rc = create_function_api(
            db,
            b"fts5",
            1,
            SQLITE_UTF8,
            UserData::Ptr(p.clone()),
            Some(fts5_fts5_func),
            None,
            None,
            None,
            None,
            None,
        );
    }
    if rc == SQLITE_OK {
        rc = create_function_api(
            db,
            b"fts5_source_id",
            0,
            SQLITE_UTF8 | SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS,
            UserData::Ptr(p),
            Some(fts5_source_id_func),
            None,
            None,
            None,
            None,
            None,
        );
    }
    rc
}
