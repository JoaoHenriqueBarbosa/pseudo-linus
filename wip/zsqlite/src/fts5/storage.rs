//! `fts5_storage.c`: o armazém do FTS5: as tabelas sombra `%_content`, `%_docsize` e `%_config`,
//! os totais por coluna (o registro de médias), a inserção e a remoção de documentos no índice
//! e a verificação de integridade.
//!
//! Desvios do C, decorrentes do modelo v2 (ver `mod.rs`):
//!
//! * O `Fts5Storage` NÃO guarda `pConfig` nem `pIndex`: todo método recebe `db`, a configuração
//!   (`&mut Fts5Config`, porque `bLock` e o `iCookie` mudam) e, quando precisa do índice, `idx`.
//! * `sqlite3Fts5StorageOpen` recebe também o índice (só para gravar a versão em `%_config`).
//! * `sqlite3Fts5StorageClose` precisa da conexão para finalizar os comandos preparados, então
//!   é o método [`Fts5Storage::close`], que o dono chama antes de largar o armazém.
//! * O `char **pzErrMsg` de [`Fts5Storage::stmt`] é `Option<&mut Option<Vec<u8>>>`.
//! * Os três laços de tokenização que alimentam o índice (`fts5StorageInsertCallback`) são a
//!   mesma função, [`insert_tokens`].

use crate::build::text_arg;
use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_INTEGER, SQLITE_MISMATCH, SQLITE_NOMEM, SQLITE_OK, SQLITE_PREPARE_NO_VTAB,
    SQLITE_PREPARE_PERSISTENT, SQLITE_RANGE, SQLITE_ROW,
};
use crate::legacy::exec_with_errmsg;
use crate::main::{errmsg, last_insert_rowid, set_last_insert_rowid};
use crate::mem::{value_type, Mem, StrDtor};
use crate::prepare::{prepare_v2, prepare_v3};
use crate::printf::{mprintf, PrintfArg};
use crate::vdbeapi::{
    bind_blob, bind_int, bind_int64, bind_null, bind_text, bind_value, column_blob, column_int64,
    column_text, finalize, reset, step, text_of, value_int64,
};

use super::buffer::{Fts5Buffer, Fts5Termset};
use super::index::Fts5Index;
use super::index3::{fts5_index_charlen_to_bytelen, fts5_index_entry_cksum};
use super::int::{
    Fts5Config, FTS5_CONTENT_EXTERNAL, FTS5_CONTENT_NONE, FTS5_CONTENT_NORMAL, FTS5_CORRUPT,
    FTS5_CURRENT_VERSION, FTS5_DETAIL_COLUMNS, FTS5_DETAIL_FULL, FTS5_DETAIL_NONE,
    FTS5_MAX_TOKEN_SIZE, FTS5_STMT_LOOKUP, FTS5_STMT_SCAN_ASC, FTS5_STMT_SCAN_DESC,
    FTS5_TOKENIZE_DOCUMENT, FTS5_TOKEN_COLOCATED,
};
use super::varint::fts5_get_varint32;

const FTS5_STMT_INSERT_CONTENT: usize = 3;
const FTS5_STMT_REPLACE_CONTENT: usize = 4;
const FTS5_STMT_DELETE_CONTENT: usize = 5;
const FTS5_STMT_REPLACE_DOCSIZE: usize = 6;
const FTS5_STMT_DELETE_DOCSIZE: usize = 7;
const FTS5_STMT_LOOKUP_DOCSIZE: usize = 8;
const FTS5_STMT_REPLACE_CONFIG: usize = 9;
const FTS5_STMT_SCAN: usize = 10;

/// `Fts5Storage`: o armazém de documentos de uma tabela FTS5.
#[derive(Debug, Default)]
pub struct Fts5Storage {
    /// Verdadeiro se `n_total_row` e `a_total_size` valem.
    pub b_totals_valid: bool,
    /// Total de linhas da tabela FTS.
    pub n_total_row: i64,
    /// Total de tokens de cada coluna.
    pub a_total_size: Vec<i64>,
    /// Os comandos preparados (a vaga fica vazia enquanto um cursor está com o comando).
    pub a_stmt: [Option<StmtId>; 11],
}

/// `fts5ExecPrintf`: formata o SQL e o executa com `sqlite3_exec`.
fn fts5_exec_printf(
    db: &mut Connection,
    pz_err: Option<&mut Option<Vec<u8>>>,
    fmt: &[u8],
    args: &[PrintfArg],
) -> i32 {
    match mprintf(fmt, args) {
        None => SQLITE_NOMEM,
        Some(z_sql) => exec_with_errmsg(db, &z_sql, None, pz_err),
    }
}

/// `sqlite3Fts5DropAll`: apaga todas as tabelas sombra.
pub fn fts5_drop_all(db: &mut Connection, cfg: &Fts5Config) -> i32 {
    let mut rc = fts5_exec_printf(
        db,
        None,
        b"DROP TABLE IF EXISTS %Q.'%q_data';\
          DROP TABLE IF EXISTS %Q.'%q_idx';\
          DROP TABLE IF EXISTS %Q.'%q_config';",
        &[
            text_arg(&cfg.z_db),
            text_arg(&cfg.z_name),
            text_arg(&cfg.z_db),
            text_arg(&cfg.z_name),
            text_arg(&cfg.z_db),
            text_arg(&cfg.z_name),
        ],
    );
    if rc == SQLITE_OK && cfg.b_columnsize != 0 {
        rc = fts5_exec_printf(
            db,
            None,
            b"DROP TABLE IF EXISTS %Q.'%q_docsize';",
            &[text_arg(&cfg.z_db), text_arg(&cfg.z_name)],
        );
    }
    if rc == SQLITE_OK && cfg.e_content == FTS5_CONTENT_NORMAL {
        rc = fts5_exec_printf(
            db,
            None,
            b"DROP TABLE IF EXISTS %Q.'%q_content';",
            &[text_arg(&cfg.z_db), text_arg(&cfg.z_name)],
        );
    }
    rc
}

/// `fts5StorageRenameOne`.
fn fts5_storage_rename_one(
    db: &mut Connection,
    cfg: &Fts5Config,
    rc: &mut i32,
    z_tail: &[u8],
    z_name: &[u8],
) {
    if *rc == SQLITE_OK {
        *rc = fts5_exec_printf(
            db,
            None,
            b"ALTER TABLE %Q.'%q_%s' RENAME TO '%q_%s';",
            &[
                text_arg(&cfg.z_db),
                text_arg(&cfg.z_name),
                text_arg(z_tail),
                text_arg(z_name),
                text_arg(z_tail),
            ],
        );
    }
}

/// `sqlite3Fts5CreateTable`: cria a tabela sombra `z_post` com a definição `z_defn`.
pub fn fts5_create_table(
    db: &mut Connection,
    cfg: &Fts5Config,
    z_post: &[u8],
    z_defn: &[u8],
    b_without: bool,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    let mut z_err: Option<Vec<u8>> = None;
    let rc = fts5_exec_printf(
        db,
        Some(&mut z_err),
        b"CREATE TABLE %Q.'%q_%q'(%s)%s",
        &[
            text_arg(&cfg.z_db),
            text_arg(&cfg.z_name),
            text_arg(z_post),
            text_arg(z_defn),
            text_arg(if b_without { b" WITHOUT ROWID" } else { b"" }),
        ],
    );
    if let Some(e) = z_err {
        *pz_err = mprintf(
            b"fts5: error creating shadow table %q_%s: %s",
            &[text_arg(&cfg.z_name), text_arg(z_post), text_arg(&e)],
        );
    }
    rc
}

/// `fts5StorageInsertCallback` dentro de um laço de tokenização: tokeniza `text` (o documento) e
/// grava cada token em `idx` na coluna `i_col` (negativa numa remoção). `sz_col` conta os
/// tokens da coluna. Devolve o código de erro.
fn insert_tokens(
    idx: &mut Fts5Index,
    cfg: &Fts5Config,
    i_col: i32,
    text: Option<&[u8]>,
    sz_col: &mut i32,
) -> i32 {
    cfg.tokenize(FTS5_TOKENIZE_DOCUMENT, text, &mut |tflags, p_token, _, _| {
        let n_token = p_token.len().min(FTS5_MAX_TOKEN_SIZE);
        if (tflags & FTS5_TOKEN_COLOCATED) == 0 || *sz_col == 0 {
            *sz_col += 1;
        }
        idx.write(cfg, i_col, *sz_col - 1, &p_token[..n_token])
    })
}

/// `fts5StorageDecodeSizeArray`: devolve verdadeiro se o registro está corrompido.
fn fts5_storage_decode_size_array(a_col: &mut [i32], a_blob: &[u8]) -> bool {
    let mut i_off = 0usize;
    for c in a_col.iter_mut() {
        if i_off >= a_blob.len() {
            return true;
        }
        let (n, v) = fts5_get_varint32(&a_blob[i_off..]);
        i_off += n as usize;
        *c = v as i32;
    }
    i_off != a_blob.len()
}

impl Fts5Storage {
    /// `fts5StorageGetStmt`: prepara (se preciso) o comando `e_stmt` e o devolve zerado.
    fn get_stmt(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        e_stmt: usize,
        pz_err_msg: Option<&mut Option<Vec<u8>>>,
    ) -> Result<StmtId, i32> {
        let mut rc = SQLITE_OK;

        /* Se não há tabela %_docsize, ninguém pede comandos sobre ela. */
        debug_assert!(
            cfg.b_columnsize != 0
                || (e_stmt != FTS5_STMT_REPLACE_DOCSIZE
                    && e_stmt != FTS5_STMT_DELETE_DOCSIZE
                    && e_stmt != FTS5_STMT_LOOKUP_DOCSIZE)
        );
        debug_assert!(e_stmt < self.a_stmt.len());

        if self.a_stmt[e_stmt].is_none() {
            const AZ_STMT: [&[u8]; 11] = [
                b"SELECT %s FROM %s T WHERE T.%Q >= ? AND T.%Q <= ? ORDER BY T.%Q ASC",
                b"SELECT %s FROM %s T WHERE T.%Q <= ? AND T.%Q >= ? ORDER BY T.%Q DESC",
                b"SELECT %s FROM %s T WHERE T.%Q=?", /* LOOKUP  */
                b"INSERT INTO %Q.'%q_content' VALUES(%s)", /* INSERT_CONTENT  */
                b"REPLACE INTO %Q.'%q_content' VALUES(%s)", /* REPLACE_CONTENT */
                b"DELETE FROM %Q.'%q_content' WHERE id=?", /* DELETE_CONTENT  */
                b"REPLACE INTO %Q.'%q_docsize' VALUES(?,?%s)", /* REPLACE_DOCSIZE  */
                b"DELETE FROM %Q.'%q_docsize' WHERE id=?", /* DELETE_DOCSIZE  */
                b"SELECT sz%s FROM %Q.'%q_docsize' WHERE id=?", /* LOOKUP_DOCSIZE  */
                b"REPLACE INTO %Q.'%q_config' VALUES(?,?)", /* REPLACE_CONFIG */
                b"SELECT %s FROM %s AS T", /* SCAN */
            ];
            let opt = |z: &Option<Vec<u8>>| PrintfArg::Text(z.clone());
            let z_sql: Option<Vec<u8>> = match e_stmt {
                FTS5_STMT_SCAN => mprintf(
                    AZ_STMT[e_stmt],
                    &[text_arg(&cfg.z_content_exprlist), opt(&cfg.z_content)],
                ),
                x if x == FTS5_STMT_SCAN_ASC as usize || x == FTS5_STMT_SCAN_DESC as usize => {
                    mprintf(
                        AZ_STMT[e_stmt],
                        &[
                            text_arg(&cfg.z_content_exprlist),
                            opt(&cfg.z_content),
                            opt(&cfg.z_content_rowid),
                            opt(&cfg.z_content_rowid),
                            opt(&cfg.z_content_rowid),
                        ],
                    )
                }
                x if x == FTS5_STMT_LOOKUP as usize => mprintf(
                    AZ_STMT[e_stmt],
                    &[
                        text_arg(&cfg.z_content_exprlist),
                        opt(&cfg.z_content),
                        opt(&cfg.z_content_rowid),
                    ],
                ),
                FTS5_STMT_INSERT_CONTENT | FTS5_STMT_REPLACE_CONTENT => {
                    let n_col = cfg.n_col() as usize + 1;
                    let mut z_bind: Vec<u8> = Vec::with_capacity(n_col * 2);
                    for _ in 0..n_col {
                        z_bind.extend_from_slice(b"?,");
                    }
                    z_bind.pop();
                    mprintf(
                        AZ_STMT[e_stmt],
                        &[text_arg(&cfg.z_db), text_arg(&cfg.z_name), text_arg(&z_bind)],
                    )
                }
                FTS5_STMT_REPLACE_DOCSIZE => mprintf(
                    AZ_STMT[e_stmt],
                    &[
                        text_arg(&cfg.z_db),
                        text_arg(&cfg.z_name),
                        text_arg(if cfg.b_contentless_delete != 0 { b",?" } else { b"" }),
                    ],
                ),
                FTS5_STMT_LOOKUP_DOCSIZE => mprintf(
                    AZ_STMT[e_stmt],
                    &[
                        text_arg(if cfg.b_contentless_delete != 0 { b",origin" } else { b"" }),
                        text_arg(&cfg.z_db),
                        text_arg(&cfg.z_name),
                    ],
                ),
                _ => mprintf(AZ_STMT[e_stmt], &[text_arg(&cfg.z_db), text_arg(&cfg.z_name)]),
            };

            match z_sql {
                None => rc = SQLITE_NOMEM,
                Some(z_sql) => {
                    let mut f = SQLITE_PREPARE_PERSISTENT;
                    if e_stmt > FTS5_STMT_LOOKUP as usize {
                        f |= SQLITE_PREPARE_NO_VTAB;
                    }
                    cfg.b_lock += 1;
                    let (rc2, stmt, _tail) = prepare_v3(db, &z_sql, -1, f);
                    cfg.b_lock -= 1;
                    rc = rc2;
                    self.a_stmt[e_stmt] = stmt;
                    if rc != SQLITE_OK {
                        if let Some(pz) = pz_err_msg {
                            *pz = mprintf(b"%s", &[PrintfArg::Text(Some(errmsg(db)))]);
                        }
                    }
                }
            }
        }

        match self.a_stmt[e_stmt] {
            Some(id) => {
                reset(db, id);
                if rc == SQLITE_OK {
                    Ok(id)
                } else {
                    Err(rc)
                }
            }
            None => Err(if rc == SQLITE_OK { SQLITE_NOMEM } else { rc }),
        }
    }

    /// `sqlite3Fts5StorageRename`: renomeia as tabelas sombra.
    pub fn rename(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        z_name: &[u8],
    ) -> i32 {
        let mut rc = self.sync(db, cfg, idx);

        fts5_storage_rename_one(db, cfg, &mut rc, b"data", z_name);
        fts5_storage_rename_one(db, cfg, &mut rc, b"idx", z_name);
        fts5_storage_rename_one(db, cfg, &mut rc, b"config", z_name);
        if cfg.b_columnsize != 0 {
            fts5_storage_rename_one(db, cfg, &mut rc, b"docsize", z_name);
        }
        if cfg.e_content == FTS5_CONTENT_NORMAL {
            fts5_storage_rename_one(db, cfg, &mut rc, b"content", z_name);
        }
        rc
    }

    /// `sqlite3Fts5StorageOpen`: abre o armazém. Com `b_create` cria e inicializa as tabelas
    /// sombra. Em erro o armazém já saiu fechado.
    pub fn open(
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        b_create: bool,
        pz_err: &mut Option<Vec<u8>>,
    ) -> Result<Fts5Storage, i32> {
        let mut rc = SQLITE_OK;
        let mut p = Fts5Storage {
            a_total_size: vec![0; cfg.n_col() as usize],
            ..Fts5Storage::default()
        };

        if b_create {
            if cfg.e_content == FTS5_CONTENT_NORMAL {
                let mut z_defn = b"id INTEGER PRIMARY KEY".to_vec();
                for i in 0..cfg.n_col() {
                    z_defn.extend_from_slice(format!(", c{}", i).as_bytes());
                }
                rc = fts5_create_table(db, cfg, b"content", &z_defn, false, pz_err);
            }

            if rc == SQLITE_OK && cfg.b_columnsize != 0 {
                let mut z_cols: &[u8] = b"id INTEGER PRIMARY KEY, sz BLOB";
                if cfg.b_contentless_delete != 0 {
                    z_cols = b"id INTEGER PRIMARY KEY, sz BLOB, origin INTEGER";
                }
                rc = fts5_create_table(db, cfg, b"docsize", z_cols, false, pz_err);
            }
            if rc == SQLITE_OK {
                rc = fts5_create_table(db, cfg, b"config", b"k PRIMARY KEY, v", true, pz_err);
            }
            if rc == SQLITE_OK {
                rc = p.config_value(db, cfg, idx, b"version", None, FTS5_CURRENT_VERSION);
            }
        }

        if rc != SQLITE_OK {
            p.close(db);
            return Err(rc);
        }
        Ok(p)
    }

    /// `sqlite3Fts5StorageClose`: finaliza os comandos preparados.
    pub fn close(&mut self, db: &mut Connection) -> i32 {
        for slot in self.a_stmt.iter_mut() {
            if let Some(id) = slot.take() {
                finalize(db, id);
            }
        }
        SQLITE_OK
    }

    /// `fts5StorageDeleteFromIndex`: se a linha `i_del` está em `%_content` (ou vem em `ap_val`),
    /// acrescenta ao índice as marcas de remoção. A linha de `%_content` não é removida aqui.
    fn delete_from_index(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_del: i64,
        ap_val: Option<&[Mem]>,
    ) -> i32 {
        let mut rc = SQLITE_OK;
        let mut p_seek: Option<StmtId> = None;

        if ap_val.is_none() {
            match self.get_stmt(db, cfg, FTS5_STMT_LOOKUP as usize, None) {
                Ok(s) => p_seek = Some(s),
                Err(rc) => return rc,
            }
            let s = p_seek.unwrap_or_default();
            bind_int64(db, s, 1, i_del);
            if step(db, s) != SQLITE_ROW {
                return reset(db, s);
            }
        }

        let cfg: &Fts5Config = cfg;
        let n_col = cfg.n_col();
        let mut i_col = 1;
        while rc == SQLITE_OK && i_col <= n_col {
            if cfg.ab_unindexed[(i_col - 1) as usize] == 0 {
                let z_text: Option<Vec<u8>> = match (p_seek, ap_val) {
                    (Some(s), _) => column_text(db, s, i_col).map(|t| t.to_vec()),
                    (None, Some(av)) => {
                        av.get((i_col - 1) as usize).and_then(text_of).map(|t| t.into_owned())
                    }
                    (None, None) => {
                        i_col += 1;
                        continue;
                    }
                };
                let mut sz_col = 0;
                rc = insert_tokens(idx, cfg, -1, z_text.as_deref(), &mut sz_col);
                self.a_total_size[(i_col - 1) as usize] -= sz_col as i64;
                if self.a_total_size[(i_col - 1) as usize] < 0 {
                    rc = FTS5_CORRUPT;
                }
            }
            i_col += 1;
        }
        if rc == SQLITE_OK && self.n_total_row < 1 {
            rc = FTS5_CORRUPT;
        } else {
            self.n_total_row -= 1;
        }

        let rc2 = match p_seek {
            Some(s) => reset(db, s),
            None => SQLITE_OK,
        };
        if rc == SQLITE_OK {
            rc = rc2;
        }
        rc
    }

    /// `fts5StorageContentlessDelete`: o DELETE numa tabela `contentless_delete=1`: acrescenta o
    /// tombstone da entrada `i_del`.
    fn contentless_delete(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_del: i64,
    ) -> i32 {
        let mut i_origin: i64 = 0;
        debug_assert!(cfg.b_contentless_delete != 0);
        debug_assert!(cfg.e_content == FTS5_CONTENT_NONE);

        /* Procura a origem do documento em %_docsize. */
        let mut rc = SQLITE_OK;
        match self.get_stmt(db, cfg, FTS5_STMT_LOOKUP_DOCSIZE, None) {
            Ok(p_lookup) => {
                bind_int64(db, p_lookup, 1, i_del);
                if SQLITE_ROW == step(db, p_lookup) {
                    i_origin = column_int64(db, p_lookup, 1);
                }
                rc = reset(db, p_lookup);
            }
            Err(e) => rc = e,
        }

        if rc == SQLITE_OK && i_origin != 0 {
            rc = idx.contentless_delete(db, cfg, i_origin, i_del);
        }
        rc
    }

    /// `fts5StorageInsertDocsize`: `INSERT OR REPLACE INTO %_docsize(id, sz) VALUES(i_rowid, buf)`.
    /// Sem tabela `%_docsize` (`columnsize=0`) não faz nada.
    fn insert_docsize(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_rowid: i64,
        p_buf: &Fts5Buffer,
    ) -> i32 {
        let mut rc = SQLITE_OK;
        if cfg.b_columnsize != 0 {
            match self.get_stmt(db, cfg, FTS5_STMT_REPLACE_DOCSIZE, None) {
                Err(e) => rc = e,
                Ok(p_replace) => {
                    bind_int64(db, p_replace, 1, i_rowid);
                    if cfg.b_contentless_delete != 0 {
                        let mut i_origin: i64 = 0;
                        rc = idx.get_origin(db, cfg, &mut i_origin);
                        bind_int64(db, p_replace, 3, i_origin);
                    }
                    if rc == SQLITE_OK {
                        bind_blob(
                            db,
                            p_replace,
                            2,
                            Some(&p_buf.p),
                            p_buf.n(),
                            StrDtor::Transient,
                        );
                        step(db, p_replace);
                        rc = reset(db, p_replace);
                        bind_null(db, p_replace, 2);
                    }
                }
            }
        }
        rc
    }

    /// `fts5StorageLoadTotals`: carrega o registro de médias em `n_total_row` e `a_total_size`.
    fn load_totals(
        &mut self,
        db: &mut Connection,
        cfg: &Fts5Config,
        idx: &mut Fts5Index,
        b_cache: bool,
    ) -> i32 {
        let mut rc = SQLITE_OK;
        if !self.b_totals_valid {
            rc = idx.get_averages(db, cfg, &mut self.n_total_row, &mut self.a_total_size);
            self.b_totals_valid = b_cache;
        }
        rc
    }

    /// `fts5StorageSaveTotals`: grava os totais no registro de médias.
    fn save_totals(&mut self, db: &mut Connection, cfg: &Fts5Config, idx: &mut Fts5Index) -> i32 {
        let mut buf = Fts5Buffer::new();
        buf.append_varint(self.n_total_row);
        for i in 0..cfg.n_col() as usize {
            buf.append_varint(self.a_total_size[i]);
        }
        idx.set_averages(db, cfg, &buf.p)
    }

    /// `sqlite3Fts5StorageDelete`: remove uma linha da tabela FTS.
    pub fn delete(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_del: i64,
        ap_val: Option<&[Mem]>,
    ) -> i32 {
        debug_assert!(cfg.e_content != FTS5_CONTENT_NORMAL || ap_val.is_none());
        let mut rc = self.load_totals(db, cfg, idx, true);

        /* Remove os registros do índice */
        if rc == SQLITE_OK {
            rc = idx.begin_write(db, cfg, true, i_del);
        }

        if rc == SQLITE_OK {
            if cfg.b_contentless_delete != 0 {
                rc = self.contentless_delete(db, cfg, idx, i_del);
            } else {
                rc = self.delete_from_index(db, cfg, idx, i_del, ap_val);
            }
        }

        /* Remove o registro de %_docsize */
        let mut p_del: Option<StmtId> = None;
        if rc == SQLITE_OK && cfg.b_columnsize != 0 {
            match self.get_stmt(db, cfg, FTS5_STMT_DELETE_DOCSIZE, None) {
                Ok(s) => {
                    p_del = Some(s);
                    bind_int64(db, s, 1, i_del);
                    step(db, s);
                    rc = reset(db, s);
                }
                Err(e) => rc = e,
            }
        }

        /* Remove o registro de %_content */
        if cfg.e_content == FTS5_CONTENT_NORMAL {
            if rc == SQLITE_OK {
                match self.get_stmt(db, cfg, FTS5_STMT_DELETE_CONTENT, None) {
                    Ok(s) => p_del = Some(s),
                    Err(e) => rc = e,
                }
            }
            if rc == SQLITE_OK {
                let s = p_del.unwrap_or_default();
                bind_int64(db, s, 1, i_del);
                step(db, s);
                rc = reset(db, s);
            }
        }

        rc
    }

    /// `sqlite3Fts5StorageDeleteAll`: apaga todas as entradas do índice FTS.
    pub fn delete_all(&mut self, db: &mut Connection, cfg: &mut Fts5Config, idx: &mut Fts5Index) -> i32 {
        self.b_totals_valid = false;

        /* Apaga o conteúdo de %_data e %_docsize. */
        let mut rc = fts5_exec_printf(
            db,
            None,
            b"DELETE FROM %Q.'%q_data';DELETE FROM %Q.'%q_idx';",
            &[
                text_arg(&cfg.z_db),
                text_arg(&cfg.z_name),
                text_arg(&cfg.z_db),
                text_arg(&cfg.z_name),
            ],
        );
        if rc == SQLITE_OK && cfg.b_columnsize != 0 {
            rc = fts5_exec_printf(
                db,
                None,
                b"DELETE FROM %Q.'%q_docsize';",
                &[text_arg(&cfg.z_db), text_arg(&cfg.z_name)],
            );
        }

        /* Reinicializa %_data: cria a estrutura inicial e o registro de médias. */
        if rc == SQLITE_OK {
            rc = idx.reinit(db, cfg);
        }
        if rc == SQLITE_OK {
            rc = self.config_value(db, cfg, idx, b"version", None, FTS5_CURRENT_VERSION);
        }
        rc
    }

    /// O corpo comum de `sqlite3Fts5StorageRebuild` (uma linha de `%_content`) e de
    /// `sqlite3Fts5StorageIndexInsert`: abre a escrita do documento `i_rowid`, tokeniza cada
    /// coluna indexada (`text_of_col` devolve o texto da coluna `i_col`, de 0), soma os tamanhos
    /// aos totais e grava a linha de `%_docsize`.
    fn index_row(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_rowid: i64,
        text_of_col: &mut dyn FnMut(&mut Connection, i32) -> Option<Vec<u8>>,
    ) -> i32 {
        let mut buf = Fts5Buffer::new();
        let mut rc = idx.begin_write(db, cfg, false, i_rowid);
        let n_col = cfg.n_col();
        let mut i_col = 0;
        while rc == SQLITE_OK && i_col < n_col {
            let mut sz_col = 0;
            if cfg.ab_unindexed[i_col as usize] == 0 {
                let z_text = text_of_col(db, i_col);
                rc = insert_tokens(idx, cfg, i_col, z_text.as_deref(), &mut sz_col);
            }
            buf.append_varint(sz_col as i64);
            self.a_total_size[i_col as usize] += sz_col as i64;
            i_col += 1;
        }
        self.n_total_row += 1;

        /* Grava o registro de %_docsize */
        if rc == SQLITE_OK {
            rc = self.insert_docsize(db, cfg, idx, i_rowid, &buf);
        }
        rc
    }

    /// `sqlite3Fts5StorageRebuild`: reconstrói o índice a partir do conteúdo.
    pub fn rebuild(&mut self, db: &mut Connection, cfg: &mut Fts5Config, idx: &mut Fts5Index) -> i32 {
        let mut rc = self.delete_all(db, cfg, idx);
        if rc == SQLITE_OK {
            rc = self.load_totals(db, cfg, idx, true);
        }

        let mut p_scan: Option<StmtId> = None;
        if rc == SQLITE_OK {
            let want_err = cfg.errmsg_target;
            let mut z_err: Option<Vec<u8>> = None;
            match self.get_stmt(db, cfg, FTS5_STMT_SCAN, if want_err { Some(&mut z_err) } else { None }) {
                Ok(s) => p_scan = Some(s),
                Err(e) => rc = e,
            }
            if want_err && z_err.is_some() {
                cfg.errmsg = z_err;
            }
        }

        if let Some(p_scan) = p_scan {
            while rc == SQLITE_OK && SQLITE_ROW == step(db, p_scan) {
                let i_rowid = column_int64(db, p_scan, 0);
                rc = self.index_row(db, cfg, idx, i_rowid, &mut |db, i_col| {
                    column_text(db, p_scan, i_col + 1).map(|t| t.to_vec())
                });
            }
            let rc2 = reset(db, p_scan);
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }

        /* Grava o registro de médias */
        if rc == SQLITE_OK {
            rc = self.save_totals(db, cfg, idx);
        }
        rc
    }

    /// `sqlite3Fts5StorageOptimize`.
    pub fn optimize(&mut self, db: &mut Connection, cfg: &mut Fts5Config, idx: &mut Fts5Index) -> i32 {
        idx.optimize(db, cfg)
    }

    /// `sqlite3Fts5StorageMerge`.
    pub fn merge(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        n_merge: i32,
    ) -> i32 {
        idx.merge(db, cfg, n_merge)
    }

    /// `sqlite3Fts5StorageReset`.
    pub fn reset(&mut self, db: &mut Connection, cfg: &Fts5Config, idx: &mut Fts5Index) -> i32 {
        idx.reset(db, cfg)
    }

    /// `fts5StorageNewRowid`: aloca um rowid novo (tabelas de conteúdo externo, com NULL no rowid)
    /// inserindo uma linha provisória em `%_docsize`. Sem `%_docsize` devolve `SQLITE_MISMATCH`.
    fn new_rowid(&mut self, db: &mut Connection, cfg: &mut Fts5Config, pi_rowid: &mut i64) -> i32 {
        let mut rc = SQLITE_MISMATCH;
        if cfg.b_columnsize != 0 {
            match self.get_stmt(db, cfg, FTS5_STMT_REPLACE_DOCSIZE, None) {
                Ok(p_replace) => {
                    bind_null(db, p_replace, 1);
                    bind_null(db, p_replace, 2);
                    step(db, p_replace);
                    rc = reset(db, p_replace);
                }
                Err(e) => rc = e,
            }
            if rc == SQLITE_OK {
                *pi_rowid = last_insert_rowid(db);
            }
        }
        rc
    }

    /// `sqlite3Fts5StorageContentInsert`: insere uma linha nova em `%_content`.
    pub fn content_insert(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        ap_val: &[Mem],
        pi_rowid: &mut i64,
    ) -> i32 {
        let mut rc = SQLITE_OK;

        if cfg.e_content != FTS5_CONTENT_NORMAL {
            if value_type(&ap_val[1]) == SQLITE_INTEGER {
                *pi_rowid = value_int64(&ap_val[1]);
            } else {
                rc = self.new_rowid(db, cfg, pi_rowid);
            }
        } else {
            let mut p_insert: Option<StmtId> = None;
            match self.get_stmt(db, cfg, FTS5_STMT_INSERT_CONTENT, None) {
                Ok(s) => p_insert = Some(s),
                Err(e) => rc = e,
            }
            let mut i = 1;
            while rc == SQLITE_OK && i <= cfg.n_col() + 1 {
                rc = bind_value(db, p_insert.unwrap_or_default(), i, &ap_val[i as usize]);
                i += 1;
            }
            if rc == SQLITE_OK {
                let s = p_insert.unwrap_or_default();
                step(db, s);
                rc = reset(db, s);
            }
            *pi_rowid = last_insert_rowid(db);
        }

        rc
    }

    /// `sqlite3Fts5StorageIndexInsert`: insere entradas novas no índice FTS e em `%_docsize`.
    pub fn index_insert(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        ap_val: &[Mem],
        i_rowid: i64,
    ) -> i32 {
        let rc = self.load_totals(db, cfg, idx, true);
        if rc != SQLITE_OK {
            self.n_total_row += 1;
            return rc;
        }
        self.index_row(db, cfg, idx, i_rowid, &mut |_, i_col| {
            text_of(&ap_val[(i_col + 2) as usize]).map(|t| t.into_owned())
        })
    }

    /// `fts5StorageCount`: `SELECT count(*) FROM %_<suffix>`.
    fn count(&mut self, db: &mut Connection, cfg: &Fts5Config, z_suffix: &[u8], pn_row: &mut i64) -> i32 {
        let z_sql = mprintf(
            b"SELECT count(*) FROM %Q.'%q_%s'",
            &[text_arg(&cfg.z_db), text_arg(&cfg.z_name), text_arg(z_suffix)],
        );
        match z_sql {
            None => SQLITE_NOMEM,
            Some(z) => {
                let (mut rc, p_cnt, _tail) = prepare_v2(db, &z, -1);
                if rc == SQLITE_OK {
                    if let Some(p_cnt) = p_cnt {
                        if SQLITE_ROW == step(db, p_cnt) {
                            *pn_row = column_int64(db, p_cnt, 0);
                        }
                        rc = finalize(db, p_cnt);
                    }
                }
                rc
            }
        }
    }

    /// `sqlite3Fts5StorageIntegrity`: confere que o índice FTS bate com `%_content`. Devolve
    /// `SQLITE_OK`, `FTS5_CORRUPT` ou outro erro se não deu para conferir.
    pub fn integrity(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        i_arg: i32,
    ) -> i32 {
        let n_col = cfg.n_col() as usize;
        let mut rc = SQLITE_OK;
        let mut a_col_size: Vec<i32> = vec![0; n_col];
        let mut a_total_size: Vec<i64> = vec![0; n_col];
        let mut cksum: u64 = 0;

        let b_use_cksum = cfg.e_content == FTS5_CONTENT_NORMAL
            || (cfg.e_content == FTS5_CONTENT_EXTERNAL && i_arg != 0);
        if b_use_cksum {
            /* Gera a soma de verificação esperada do índice a partir de %_content. */
            match self.get_stmt(db, cfg, FTS5_STMT_SCAN, None) {
                Err(e) => rc = e,
                Ok(p_scan) => {
                    while SQLITE_ROW == step(db, p_scan) {
                        let i_rowid = column_int64(db, p_scan, 0);
                        let mut p_termset: Option<Fts5Termset> = None;
                        if cfg.b_columnsize != 0 {
                            rc = self.docsize(db, cfg, i_rowid, &mut a_col_size);
                        }
                        if rc == SQLITE_OK && cfg.e_detail == FTS5_DETAIL_NONE {
                            p_termset = Some(Fts5Termset::new());
                        }
                        let mut i = 0usize;
                        while rc == SQLITE_OK && i < n_col {
                            if cfg.ab_unindexed[i] != 0 {
                                i += 1;
                                continue;
                            }
                            let mut sz_col = 0;
                            if cfg.e_detail == FTS5_DETAIL_COLUMNS {
                                p_termset = Some(Fts5Termset::new());
                            }
                            let z_text = column_text(db, p_scan, i as i32 + 1).map(|t| t.to_vec());
                            let c: &Fts5Config = cfg;
                            rc = c.tokenize(
                                FTS5_TOKENIZE_DOCUMENT,
                                z_text.as_deref(),
                                &mut |tflags, p_token, _, _| {
                                    integrity_callback(
                                        c,
                                        i_rowid,
                                        i as i32,
                                        &mut sz_col,
                                        &mut cksum,
                                        &mut p_termset,
                                        tflags,
                                        p_token,
                                    )
                                },
                            );
                            if rc == SQLITE_OK && cfg.b_columnsize != 0 && sz_col != a_col_size[i] {
                                rc = FTS5_CORRUPT;
                            }
                            a_total_size[i] += sz_col as i64;
                            if cfg.e_detail == FTS5_DETAIL_COLUMNS {
                                p_termset = None;
                            }
                            i += 1;
                        }

                        if rc != SQLITE_OK {
                            break;
                        }
                    }
                    let rc2 = reset(db, p_scan);
                    if rc == SQLITE_OK {
                        rc = rc2;
                    }
                }
            }

            /* Confere o registro de totais ("médias") */
            if rc == SQLITE_OK {
                rc = self.load_totals(db, cfg, idx, false);
                let mut i = 0;
                while rc == SQLITE_OK && i < n_col {
                    if self.a_total_size[i] != a_total_size[i] {
                        rc = FTS5_CORRUPT;
                    }
                    i += 1;
                }
            }

            /* Confere que %_docsize e %_content têm o número esperado de linhas. */
            if rc == SQLITE_OK && cfg.e_content == FTS5_CONTENT_NORMAL {
                let mut n_row: i64 = 0;
                rc = self.count(db, cfg, b"content", &mut n_row);
                if rc == SQLITE_OK && n_row != self.n_total_row {
                    rc = FTS5_CORRUPT;
                }
            }
            if rc == SQLITE_OK && cfg.b_columnsize != 0 {
                let mut n_row: i64 = 0;
                rc = self.count(db, cfg, b"docsize", &mut n_row);
                if rc == SQLITE_OK && n_row != self.n_total_row {
                    rc = FTS5_CORRUPT;
                }
            }
        }

        /* Passa a soma esperada ao índice, que a confere (entre outras coisas) com a soma que
        ** ele mesmo calcula. */
        if rc == SQLITE_OK {
            rc = idx.integrity_check(db, cfg, cksum, b_use_cksum);
        }
        rc
    }

    /// `sqlite3Fts5StorageStmt`: um comando para ler `%_content`; o armazém o cede ao chamador
    /// (devolva-o com [`Fts5Storage::stmt_release`]).
    pub fn stmt(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        e_stmt: i32,
        pz_err_msg: Option<&mut Option<Vec<u8>>>,
    ) -> Result<StmtId, i32> {
        debug_assert!(
            e_stmt == FTS5_STMT_SCAN_ASC || e_stmt == FTS5_STMT_SCAN_DESC || e_stmt == FTS5_STMT_LOOKUP
        );
        let s = self.get_stmt(db, cfg, e_stmt as usize, pz_err_msg)?;
        self.a_stmt[e_stmt as usize] = None;
        Ok(s)
    }

    /// `sqlite3Fts5StorageStmtRelease`: devolve um comando obtido com [`Fts5Storage::stmt`].
    pub fn stmt_release(&mut self, db: &mut Connection, e_stmt: i32, p_stmt: StmtId) {
        debug_assert!(
            e_stmt == FTS5_STMT_SCAN_ASC || e_stmt == FTS5_STMT_SCAN_DESC || e_stmt == FTS5_STMT_LOOKUP
        );
        if self.a_stmt[e_stmt as usize].is_none() {
            reset(db, p_stmt);
            self.a_stmt[e_stmt as usize] = Some(p_stmt);
        } else {
            finalize(db, p_stmt);
        }
    }

    /// `sqlite3Fts5StorageDocsize`: lê o registro de `%_docsize` da linha `i_rowid` em `a_col`.
    pub fn docsize(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        i_rowid: i64,
        a_col: &mut [i32],
    ) -> i32 {
        let n_col = cfg.n_col() as usize;
        debug_assert!(cfg.b_columnsize != 0);
        match self.get_stmt(db, cfg, FTS5_STMT_LOOKUP_DOCSIZE, None) {
            Err(rc) => rc,
            Ok(p_lookup) => {
                let mut b_corrupt = true;
                bind_int64(db, p_lookup, 1, i_rowid);
                if SQLITE_ROW == step(db, p_lookup) {
                    let a_blob: Vec<u8> =
                        column_blob(db, p_lookup, 0).map(|b| b.to_vec()).unwrap_or_default();
                    if !fts5_storage_decode_size_array(&mut a_col[..n_col], &a_blob) {
                        b_corrupt = false;
                    }
                }
                let mut rc = reset(db, p_lookup);
                if b_corrupt && rc == SQLITE_OK {
                    rc = FTS5_CORRUPT;
                }
                rc
            }
        }
    }

    /// `sqlite3Fts5StorageSize`: o total de tokens da coluna `i_col` (ou da tabela, se negativa).
    pub fn size(
        &mut self,
        db: &mut Connection,
        cfg: &Fts5Config,
        idx: &mut Fts5Index,
        i_col: i32,
        pn_token: &mut i64,
    ) -> i32 {
        let mut rc = self.load_totals(db, cfg, idx, false);
        if rc == SQLITE_OK {
            *pn_token = 0;
            if i_col < 0 {
                for i in 0..cfg.n_col() as usize {
                    *pn_token += self.a_total_size[i];
                }
            } else if i_col < cfg.n_col() {
                *pn_token = self.a_total_size[i_col as usize];
            } else {
                rc = SQLITE_RANGE;
            }
        }
        rc
    }

    /// `sqlite3Fts5StorageRowCount`.
    pub fn row_count(
        &mut self,
        db: &mut Connection,
        cfg: &Fts5Config,
        idx: &mut Fts5Index,
        pn_row: &mut i64,
    ) -> i32 {
        let mut rc = self.load_totals(db, cfg, idx, false);
        if rc == SQLITE_OK {
            /* `n_total_row` zero não quer dizer banco corrompido (a tabela pode ter zero linhas),
            ** mas esta função só é chamada pelo xRowCount(), que não pode ser invocado numa tabela
            ** sem linhas. Daí o FTS5_CORRUPT. */
            *pn_row = self.n_total_row;
            if self.n_total_row <= 0 {
                rc = FTS5_CORRUPT;
            }
        }
        rc
    }

    /// `sqlite3Fts5StorageSync`: grava em disco o que está em memória.
    pub fn sync(&mut self, db: &mut Connection, cfg: &mut Fts5Config, idx: &mut Fts5Index) -> i32 {
        let mut rc = SQLITE_OK;
        let i_last_rowid = last_insert_rowid(db);
        if self.b_totals_valid {
            rc = self.save_totals(db, cfg, idx);
            if rc == SQLITE_OK {
                self.b_totals_valid = false;
            }
        }
        if rc == SQLITE_OK {
            rc = idx.sync(db, cfg);
        }
        set_last_insert_rowid(db, i_last_rowid);
        rc
    }

    /// `sqlite3Fts5StorageRollback`.
    pub fn rollback(&mut self, db: &mut Connection, idx: &mut Fts5Index) -> i32 {
        self.b_totals_valid = false;
        idx.rollback(db)
    }

    /// `sqlite3Fts5StorageConfigValue`: grava `z` = `p_val` (ou `i_val`) em `%_config`. Com
    /// `p_val` também incrementa o cookie da configuração.
    pub fn config_value(
        &mut self,
        db: &mut Connection,
        cfg: &mut Fts5Config,
        idx: &mut Fts5Index,
        z: &[u8],
        p_val: Option<&Mem>,
        i_val: i32,
    ) -> i32 {
        let mut rc;
        match self.get_stmt(db, cfg, FTS5_STMT_REPLACE_CONFIG, None) {
            Err(e) => rc = e,
            Ok(p_replace) => {
                bind_text(db, p_replace, 1, Some(z), z.len() as i32, StrDtor::Transient);
                match p_val {
                    Some(v) => {
                        bind_value(db, p_replace, 2, v);
                    }
                    None => {
                        bind_int(db, p_replace, 2, i_val);
                    }
                }
                step(db, p_replace);
                rc = reset(db, p_replace);
                bind_null(db, p_replace, 1);
            }
        }
        if rc == SQLITE_OK && p_val.is_some() {
            let i_new = cfg.i_cookie + 1;
            rc = idx.set_cookie(db, cfg, i_new);
            if rc == SQLITE_OK {
                cfg.i_cookie = i_new;
            }
        }
        rc
    }
}

/// `fts5StorageIntegrityCallback`: o callback de tokenização da verificação de integridade.
/// Acumula em `cksum` a soma de verificação das entradas do documento `i_rowid`, coluna `i_col`.
#[allow(clippy::too_many_arguments)]
fn integrity_callback(
    cfg: &Fts5Config,
    i_rowid: i64,
    i_col_in: i32,
    sz_col: &mut i32,
    cksum: &mut u64,
    p_termset: &mut Option<Fts5Termset>,
    tflags: i32,
    p_token_full: &[u8],
) -> i32 {
    let n_token = p_token_full.len().min(FTS5_MAX_TOKEN_SIZE);
    let p_token = &p_token_full[..n_token];

    if (tflags & FTS5_TOKEN_COLOCATED) == 0 || *sz_col == 0 {
        *sz_col += 1;
    }

    let (i_pos, i_col) = match cfg.e_detail {
        FTS5_DETAIL_FULL => (*sz_col - 1, i_col_in),
        FTS5_DETAIL_COLUMNS => (i_col_in, 0),
        _ => {
            debug_assert!(cfg.e_detail == FTS5_DETAIL_NONE);
            (0, 0)
        }
    };

    /* Sem conjunto de termos (`detail=full`) nada está "presente". */
    let b_present = match p_termset.as_mut() {
        Some(t) => t.add(0, p_token),
        None => false,
    };
    if !b_present {
        *cksum ^= fts5_index_entry_cksum(i_rowid, i_col, i_pos, 0, p_token);
    }

    for ii in 0..cfg.n_prefix() {
        let n_char = cfg.a_prefix[ii as usize];
        let n_byte = fts5_index_charlen_to_bytelen(p_token, n_token as i32, n_char);
        if n_byte != 0 {
            let sub = &p_token[..n_byte as usize];
            let b_present = match p_termset.as_mut() {
                Some(t) => t.add(ii + 1, sub),
                None => false,
            };
            if !b_present {
                *cksum ^= fts5_index_entry_cksum(i_rowid, i_col, i_pos, ii + 1, sub);
            }
        }
    }

    SQLITE_OK
}
