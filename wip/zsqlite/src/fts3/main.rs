//! `fts3.c` (parte 1): o módulo virtual `fts3`/`fts4`: criação e conexão (análise dos argumentos,
//! tabelas sombra, esquema declarado), `xBestIndex`, o cursor, `xSync`/`xBegin`/`xCommit`/
//! `xRollback`, `xFindFunction` (as funções `snippet()`, `offsets()`, `matchinfo()` e `optimize()`),
//! `xRename`, os pontos de salvamento, `xShadowName`, `xIntegrity` e `sqlite3Fts3Init`. A parte 2
//! ([`super::main2`]: mesclagens, seleção de termos, `xFilter`/`xNext`/`xColumn`) e a parte 3
//! ([`super::main3`]: avaliação da consulta) são reexportadas no fim.
//!
//! As rotinas de `fts3.c` que `fts3_write.c` também usa (`fts3DbExec`, `CreateStatTable`,
//! `FirstFilter`, `DoclistPrev`, `SegReaderCursor`, ...) moram em [`super::write2`] e são
//! importadas de lá.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * **A tabela e os cursores.** A instância `Vtab` é a [`Fts3FullTable`]: a `Fts3Table` mais os
//!   cursores abertos (o cursor do núcleo, [`Fts3CsrHandle`], é só o id). Cada operação do cursor
//!   retira o `Fts3Cursor` da tabela com [`with_csr`], de modo que a função recebe a `Fts3Table` e
//!   o cursor ao mesmo tempo.
//! * **O valor ponteiro `"fts3cursor"`.** `sqlite3_result_pointer(pCtx, pCsr, "fts3cursor", 0)` é
//!   um [`Fts3CursorRef`] (o id do cursor); `fts3FunctionArg` o resolve por [`with_cursor`], que acha
//!   a tabela dona pela conexão. Fora das funções `snippet()` e companhia o valor é `NULL`, como
//!   no C.
//! * **Funções que escrevem o resultado** (`sqlite3Fts3Snippet`, `Offsets`, `Matchinfo`) devolvem um
//!   [`Fts3Result`] que a função SQL aplica ao contexto (a conexão do contexto está emprestada
//!   enquanto a tabela é retirada).
//! * **Sem `inTransaction` e `mxSavepoint`** (só `SQLITE_DEBUG`). `xDestroy` do tokenizador e os
//!   `sqlite3_free` são o `Drop`.
//! * **Contagem de referências do registro de tokenizadores** (`nRef`, `hashDestroy`): é a do `Rc`.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use crate::build::text_arg;
use crate::connection::{
    Connection, Context, IndexInfo, ModuleCaps, ScalarFn, UserData, Vtab, VtabCursor, VtabModule,
    VTableId,
};
use crate::consts::{
    SQLITE_AUTH, SQLITE_CORRUPT, SQLITE_DONE, SQLITE_ERROR, SQLITE_INDEX_CONSTRAINT_EQ,
    SQLITE_INDEX_CONSTRAINT_GE, SQLITE_INDEX_CONSTRAINT_GT, SQLITE_INDEX_CONSTRAINT_LE,
    SQLITE_INDEX_CONSTRAINT_LT, SQLITE_INDEX_CONSTRAINT_MATCH, SQLITE_INDEX_SCAN_UNIQUE, SQLITE_NOMEM,
    SQLITE_OK, SQLITE_VTAB_CONSTRAINT_SUPPORT, SQLITE_VTAB_INNOCUOUS,
};
use crate::legacy::exec;
use crate::main::{last_insert_rowid, overload_function, set_last_insert_rowid, table_column_metadata};
use crate::mem::Mem;
use crate::mem2::value_pointer;
use crate::prepare::prepare_v2;
use crate::printf::{mprintf, PrintfArg};
use crate::util::{err_str, stricmp, strnicmp};
use crate::vdbeapi::{
    column_count, column_int, column_name, finalize, result_blob, result_error, result_error_code,
    result_error_nomem, result_text, step, value_int, text_of,
};
use crate::vtab::{create_module, declare_vtab, vtab_config, with_vtab};

use super::hash::{Fts3Hash, FTS3_HASH_STRING};
use super::int::{
    fts3_dequote, fts3_read_int, Fts3Cursor, Fts3Index, Fts3Table, FTS3_DOCID_SEARCH,
    FTS3_FULLSCAN_SEARCH, FTS3_FULLTEXT_SEARCH, FTS3_HAVE_DOCID_GE, FTS3_HAVE_DOCID_LE,
    FTS3_HAVE_LANGID, FTS3_MAX_PENDING_DATA,
};
use super::porter::fts3_porter_tokenizer_module;
use super::snippet::{fts3_matchinfo, fts3_offsets, fts3_snippet, Fts3Result};
use super::tokenizer::{
    fts3_init_hash_table, fts3_init_tokenizer, fts3_is_id_char, fts3_next_token, Fts3HashWrapper,
};
use super::tokenizer1::fts3_simple_tokenizer_module;
use super::unicode::fts3_unicode_tokenizer_module;
use super::write::{
    fts3_c_str, fts3_create_stat_table, fts3_db_exec, fts3_incrmerge, fts3_integrity_check,
    fts3_max_level, fts3_optimize, fts3_pending_terms_clear, fts3_pending_terms_flush,
    fts3_segments_close, fts3_update_method,
};
use super::aux::fts3_init_aux;
use super::tokenize_vtab::fts3_init_tok;

pub use super::main2::*;
pub use super::main3::*;

// ---------------------------------------------------------------------------------------------
// A tabela e os cursores
// ---------------------------------------------------------------------------------------------

/// Um cursor aberto da tabela.
struct CsrEntry {
    /// O id do cursor (o valor que o cursor do núcleo guarda).
    id: i64,
    /// O cursor.
    csr: Fts3Cursor,
}

/// A instância `Vtab` de uma tabela `fts3` ou `fts4`.
pub struct Fts3FullTable {
    /// A tabela (`Fts3Table`, com o `base.zErrMsg`).
    pub base: Fts3Table,
    /// Os cursores abertos.
    cursors: Vec<CsrEntry>,
}

/// O cursor do núcleo: só o id do [`Fts3Cursor`] guardado na tabela.
pub struct Fts3CsrHandle {
    id: i64,
}

thread_local! {
    /// O próximo id de cursor (os ids nunca se repetem na linha de execução).
    static NEXT_CSR_ID: Cell<i64> = const { Cell::new(1) };
}

/// A `Fts3FullTable` de uma instância `Vtab`.
fn tab_of(vtab: &mut dyn Vtab) -> &mut Fts3FullTable {
    vtab.as_any_mut().downcast_mut::<Fts3FullTable>().expect("fts3: a instância não é uma Fts3FullTable")
}

/// Empresta o cursor `id` da tabela: `f` recebe a `Fts3Table` e o cursor. `None` se o cursor não
/// existe.
fn with_csr<R>(
    tab: &mut Fts3FullTable,
    id: i64,
    f: impl FnOnce(&mut Fts3Table, &mut Fts3Cursor) -> R,
) -> Option<R> {
    let pos = tab.cursors.iter().position(|c| c.id == id)?;
    let mut e = tab.cursors.swap_remove(pos);
    let r = f(&mut tab.base, &mut e.csr);
    tab.cursors.push(e);
    Some(r)
}

/// A tabela FTS3/FTS4 da conexão que tem o cursor `id`.
fn fts3_table_from_csrid(db: &mut Connection, id: i64) -> Option<VTableId> {
    for (slot, vt) in db.vtabs.iter_mut() {
        if let Some(v) = vt.p_vtab.as_mut() {
            if let Some(t) = v.as_any_mut().downcast_mut::<Fts3FullTable>() {
                if t.cursors.iter().any(|c| c.id == id) {
                    return Some(VTableId::from_slot(slot));
                }
            }
        }
    }
    None
}

/// Roda `f` com a conexão, a tabela e o cursor `id` (o `Fts3Cursor *` do valor ponteiro).
fn with_cursor<R>(
    db: &mut Connection,
    id: i64,
    f: impl FnOnce(&mut Connection, &mut Fts3Table, &mut Fts3Cursor) -> R,
) -> Option<R> {
    let vid = fts3_table_from_csrid(db, id)?;
    with_vtab(db, vid, |db, vtab| {
        let tab = tab_of(vtab);
        with_csr(tab, id, |base, csr| f(db, base, csr))
    })
    .flatten()
}

// ---------------------------------------------------------------------------------------------
// Criação e conexão
// ---------------------------------------------------------------------------------------------

/// `fts3DisconnectMethod`: finaliza os comandos que a tabela guardou.
fn fts3_disconnect(db: &mut Connection, p: &mut Fts3Table) {
    if let Some(st) = p.p_seek_stmt.take() {
        finalize(db, st);
    }
    for slot in p.a_stmt.iter_mut() {
        if let Some(st) = slot.take() {
            finalize(db, st);
        }
    }
}

/// `fts3DeclareVtab`.
fn fts3_declare_vtab(db: &mut Connection, p_rc: &mut i32, p: &Fts3Table) {
    if *p_rc == SQLITE_OK {
        let z_languageid: &[u8] = p.z_languageid.as_deref().unwrap_or(b"__langid");
        vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
        vtab_config(db, SQLITE_VTAB_INNOCUOUS, 0);

        /* Lista das colunas do usuário da tabela virtual. */
        let mut z_cols: Vec<u8> = Vec::new();
        for c in p.az_column.iter() {
            if let Some(s) = mprintf(b"%Q, ", &[text_arg(c)]) {
                z_cols.extend_from_slice(&s);
            }
        }

        /* O `CREATE TABLE` inteiro que se passa ao SQLite. */
        *p_rc = match mprintf(
            b"CREATE TABLE x(%s %Q HIDDEN, docid HIDDEN, %Q HIDDEN)",
            &[text_arg(&z_cols), text_arg(&p.z_name), text_arg(z_languageid)],
        ) {
            None => SQLITE_NOMEM,
            Some(z_sql) => declare_vtab(db, &z_sql),
        };
    }
}

/// `fts3CreateTables`.
fn fts3_create_tables(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    let mut rc = SQLITE_OK;

    if p.z_content_tbl.is_none() {
        /* Lista das colunas do usuário da tabela de conteúdo. */
        let mut z_content_cols: Vec<u8> = b"docid INTEGER PRIMARY KEY".to_vec();
        for (i, z) in p.az_column.iter().enumerate() {
            if let Some(s) = mprintf(b", 'c%d%q'", &[PrintfArg::Int(i as i64), text_arg(z)]) {
                z_content_cols.extend_from_slice(&s);
            }
        }
        if p.z_languageid.is_some() {
            z_content_cols.extend_from_slice(b", langid");
        }

        fts3_db_exec(
            &mut rc,
            db,
            b"CREATE TABLE %Q.'%q_content'(%s)",
            &[text_arg(&p.z_db), text_arg(&p.z_name), text_arg(&z_content_cols)],
        );
    }

    /* Cria as outras tabelas. */
    fts3_db_exec(
        &mut rc,
        db,
        b"CREATE TABLE %Q.'%q_segments'(blockid INTEGER PRIMARY KEY, block BLOB);",
        &[text_arg(&p.z_db), text_arg(&p.z_name)],
    );
    fts3_db_exec(
        &mut rc,
        db,
        b"CREATE TABLE %Q.'%q_segdir'(level INTEGER,idx INTEGER,start_block INTEGER,leaves_end_block INTEGER,end_block INTEGER,root BLOB,PRIMARY KEY(level, idx));",
        &[text_arg(&p.z_db), text_arg(&p.z_name)],
    );
    if p.b_has_docsize {
        fts3_db_exec(
            &mut rc,
            db,
            b"CREATE TABLE %Q.'%q_docsize'(docid INTEGER PRIMARY KEY, size BLOB);",
            &[text_arg(&p.z_db), text_arg(&p.z_name)],
        );
    }
    debug_assert!((p.b_has_stat != 0) == p.b_fts4);
    if p.b_has_stat != 0 {
        fts3_create_stat_table(db, p, &mut rc);
    }
    rc
}

/// `fts3DatabasePageSize`.
fn fts3_database_page_size(db: &mut Connection, p_rc: &mut i32, p: &mut Fts3Table) {
    if *p_rc == SQLITE_OK {
        let mut rc;
        match mprintf(b"PRAGMA %Q.page_size", &[text_arg(&p.z_db)]) {
            None => rc = SQLITE_NOMEM,
            Some(z_sql) => {
                let (rc2, st, _) = prepare_v2(db, &z_sql, -1);
                rc = rc2;
                if rc == SQLITE_OK {
                    if let Some(st) = st {
                        step(db, st);
                        p.n_pgsz = column_int(db, st, 0);
                        rc = finalize(db, st);
                    }
                } else if rc == SQLITE_AUTH {
                    p.n_pgsz = 1024;
                    rc = SQLITE_OK;
                }
            }
        }
        debug_assert!(p.n_pgsz > 0 || rc != SQLITE_OK);
        *p_rc = rc;
    }
}

/// `fts3IsSpecialColumn`: se `z` é `chave=valor`, devolve o comprimento da chave e o valor (sem
/// aspas).
fn fts3_is_special_column(z: &[u8]) -> Option<(usize, Vec<u8>)> {
    let n_key = z.iter().position(|&c| c == b'=')?;
    let mut z_value = z[n_key + 1..].to_vec();
    fts3_dequote(&mut z_value);
    Some((n_key, z_value))
}

/// `fts3QuoteId`.
fn fts3_quote_id(z_input: &[u8]) -> Vec<u8> {
    let mut z = Vec::with_capacity(z_input.len() + 2);
    z.push(b'"');
    for &c in z_input {
        if c == b'"' {
            z.push(b'"');
        }
        z.push(c);
    }
    z.push(b'"');
    z
}

/// `fts3Appendf`: formata e acrescenta a `pz`.
fn fts3_appendf(pz: &mut Vec<u8>, z_format: &[u8], args: &[PrintfArg]) {
    if let Some(z) = mprintf(z_format, args) {
        pz.extend_from_slice(&z);
    }
}

/// `fts3ReadExprList`.
fn fts3_read_expr_list(p: &Fts3Table, z_func: Option<&[u8]>) -> Vec<u8> {
    let mut z_ret: Vec<u8> = Vec::new();

    if p.z_content_tbl.is_none() {
        let z_function: Vec<u8> = match z_func {
            None => Vec::new(),
            Some(f) => fts3_quote_id(f),
        };
        fts3_appendf(&mut z_ret, b"docid", &[]);
        for (i, c) in p.az_column.iter().enumerate() {
            fts3_appendf(
                &mut z_ret,
                b",%s(x.'c%d%q')",
                &[text_arg(&z_function), PrintfArg::Int(i as i64), text_arg(c)],
            );
        }
        if p.z_languageid.is_some() {
            fts3_appendf(&mut z_ret, b", x.%Q", &[text_arg(b"langid")]);
        }
    } else {
        fts3_appendf(&mut z_ret, b"rowid", &[]);
        for c in p.az_column.iter() {
            fts3_appendf(&mut z_ret, b", x.'%q'", &[text_arg(c)]);
        }
        if let Some(l) = p.z_languageid.as_deref() {
            fts3_appendf(&mut z_ret, b", x.%Q", &[text_arg(l)]);
        }
    }
    let content = p.z_content_tbl.as_deref();
    fts3_appendf(
        &mut z_ret,
        b" FROM '%q'.'%q%s' AS x",
        &[
            text_arg(&p.z_db),
            text_arg(content.unwrap_or(&p.z_name)),
            text_arg(if content.is_some() { b"" } else { b"_content" }),
        ],
    );
    z_ret
}

/// `fts3WriteExprList`.
fn fts3_write_expr_list(p: &Fts3Table, z_func: Option<&[u8]>) -> Vec<u8> {
    let mut z_ret: Vec<u8> = Vec::new();
    let z_function: Vec<u8> = match z_func {
        None => Vec::new(),
        Some(f) => fts3_quote_id(f),
    };
    fts3_appendf(&mut z_ret, b"?", &[]);
    for _ in 0..p.az_column.len() {
        fts3_appendf(&mut z_ret, b",%s(?)", &[text_arg(&z_function)]);
    }
    if p.z_languageid.is_some() {
        fts3_appendf(&mut z_ret, b", ?", &[]);
    }
    z_ret
}

/// `fts3GobbleInt`.
fn fts3_gobble_int(z: &[u8], pp: &mut usize, pn_out: &mut i32) -> i32 {
    const MAX_NPREFIX: i32 = 10_000_000;
    let mut n_int = 0i32;
    let n_byte = fts3_read_int(super::write::sl(z, *pp), &mut n_int);
    if n_int > MAX_NPREFIX {
        n_int = 0;
    }
    if n_byte == 0 {
        return SQLITE_ERROR;
    }
    *pn_out = n_int;
    *pp = (*pp as i64 + n_byte as i64).max(0) as usize;
    SQLITE_OK
}

/// `fts3PrefixParameter`: os comprimentos de prefixo do índice (o primeiro, do índice principal,
/// é 0).
fn fts3_prefix_parameter(z_param: Option<&[u8]>) -> Result<Vec<i32>, i32> {
    let mut n_index: i32 = 1; /* número de entradas */

    if let Some(z) = z_param {
        if !z.is_empty() {
            n_index += 1;
            for &c in z {
                if c == b',' {
                    n_index += 1;
                }
            }
        }
    }

    let mut a_index = vec![0i32; n_index as usize];
    if let Some(z) = z_param {
        let mut p = 0usize;
        let mut i: i32 = 1;
        while i < n_index {
            let mut n_prefix = 0;
            if fts3_gobble_int(z, &mut p, &mut n_prefix) != SQLITE_OK {
                return Err(SQLITE_ERROR);
            }
            debug_assert!(n_prefix >= 0);
            if n_prefix == 0 {
                n_index -= 1;
                i -= 1;
            } else {
                a_index[i as usize] = n_prefix;
            }
            p += 1;
            i += 1;
        }
    }

    a_index.truncate(n_index as usize);
    Ok(a_index)
}

/// `fts3ContentColumns`: os nomes das colunas da tabela de conteúdo `z_tbl`.
fn fts3_content_columns(
    db: &mut Connection,
    z_db: &[u8],
    z_tbl: &[u8],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, i32> {
    let z_sql = mprintf(b"SELECT * FROM %Q.%Q", &[text_arg(z_db), text_arg(z_tbl)]).ok_or(SQLITE_NOMEM)?;
    let (rc, st, _) = prepare_v2(db, &z_sql, -1);
    if rc != SQLITE_OK {
        let m = crate::main::errmsg(db);
        *pz_err = mprintf(b"%s", &[text_arg(&m)]);
        return Err(rc);
    }
    let Some(st) = st else { return Err(SQLITE_ERROR) };

    let n_col = column_count(db, st);
    let mut az_col: Vec<Vec<u8>> = Vec::with_capacity(n_col.max(0) as usize);
    for i in 0..n_col {
        let z = column_name(db, st, i).unwrap_or_default();
        az_col.push(fts3_c_str(&z).to_vec());
    }
    finalize(db, st);
    Ok(az_col)
}

/// `fts3InitVtab`: o trabalho de `xCreate` e de `xConnect`.
fn fts3_init_vtab(
    is_create: bool,
    db: &mut Connection,
    p_hash: &Fts3HashWrapper,
    argv: &[Vec<u8>],
    pz_err: &mut Option<Vec<u8>>,
) -> Result<Box<dyn Vtab>, i32> {
    let argc = argv.len();
    let is_fts4 = argv.first().is_some_and(|a| a.get(3) == Some(&b'4'));
    let mut rc = SQLITE_OK;
    let mut p_tokenizer: Option<Rc<dyn super::int::Fts3Tokenizer>> = None;

    /* Os resultados da análise das opções chave=valor do FTS4. */
    let mut b_no_docsize = false;
    let mut b_desc_idx = false;
    let mut z_prefix: Option<Vec<u8>> = None;
    let mut z_compress: Option<Vec<u8>> = None;
    let mut z_uncompress: Option<Vec<u8>> = None;
    let mut z_content: Option<Vec<u8>> = None;
    let mut z_languageid: Option<Vec<u8>> = None;
    let mut az_notindexed: Vec<Option<Vec<u8>>> = Vec::new();
    let mut a_col: Vec<Vec<u8>> = Vec::new();

    let z_db_name: Vec<u8> = fts3_c_str(argv.get(1).map_or(&[][..], |v| v)).to_vec();
    let z_tbl_name: Vec<u8> = fts3_c_str(argv.get(2).map_or(&[][..], |v| v)).to_vec();

    /* Percorre todos os argumentos do usuário (os nomes das colunas e os argumentos especiais):
    ** conta as colunas e, se há especificação de tokenizador, o cria. */
    let mut i = 3usize;
    while rc == SQLITE_OK && i < argc {
        let z: &[u8] = fts3_c_str(&argv[i]);

        /* É a especificação de um tokenizador? */
        if p_tokenizer.is_none()
            && z.len() > 8
            && strnicmp(Some(z), Some(b"tokenize".as_slice()), 8) == 0
            && !fts3_is_id_char(z[8])
        {
            match fts3_init_tokenizer(p_hash, &z[9..], pz_err) {
                Ok(t) => p_tokenizer = Some(t),
                Err(e) => rc = e,
            }
        }
        /* É um argumento especial do FTS4? */
        else if let Some((n_key, z_val)) = is_fts4.then(|| fts3_is_special_column(z)).flatten() {
            const FTS4_OPT: [&[u8]; 8] = [
                b"matchinfo",
                b"prefix",
                b"compress",
                b"uncompress",
                b"order",
                b"content",
                b"languageid",
                b"notindexed",
            ];
            let i_opt = FTS4_OPT
                .iter()
                .position(|o| n_key == o.len() && strnicmp(Some(z), Some(*o), o.len() as i32) == 0)
                .unwrap_or(FTS4_OPT.len());
            match i_opt {
                0 => {
                    /* MATCHINFO */
                    if z_val.len() != 4 || strnicmp(Some(&z_val), Some(b"fts3".as_slice()), 4) != 0 {
                        *pz_err = mprintf(b"unrecognized matchinfo: %s", &[text_arg(&z_val)]);
                        rc = SQLITE_ERROR;
                    }
                    b_no_docsize = true;
                }
                1 => z_prefix = Some(z_val),
                2 => z_compress = Some(z_val),
                3 => z_uncompress = Some(z_val),
                4 => {
                    /* ORDER */
                    if (z_val.len() != 3 || strnicmp(Some(&z_val), Some(b"asc".as_slice()), 3) != 0)
                        && (z_val.len() != 4 || strnicmp(Some(&z_val), Some(b"desc".as_slice()), 4) != 0)
                    {
                        *pz_err = mprintf(b"unrecognized order: %s", &[text_arg(&z_val)]);
                        rc = SQLITE_ERROR;
                    }
                    b_desc_idx = matches!(z_val.first(), Some(b'd') | Some(b'D'));
                }
                5 => z_content = Some(z_val),
                6 => z_languageid = Some(z_val),
                7 => az_notindexed.push(Some(z_val)),
                _ => {
                    *pz_err = mprintf(b"unrecognized parameter: %s", &[text_arg(z)]);
                    rc = SQLITE_ERROR;
                }
            }
        }
        /* Senão, o argumento é o nome de uma coluna. */
        else {
            a_col.push(z.to_vec());
        }
        i += 1;
    }

    /* Com a opção content=xxx: 1. ignora compress= e uncompress=; 2. se nenhuma coluna foi dada
    ** no CREATE VIRTUAL TABLE, usa todas as colunas da tabela de conteúdo. */
    if rc == SQLITE_OK {
        if let Some(content) = z_content.as_deref() {
            z_compress = None;
            z_uncompress = None;
            if a_col.is_empty() {
                match fts3_content_columns(db, &z_db_name, content, pz_err) {
                    Ok(cols) => {
                        a_col = cols;
                        /* Com languageid=, tira a coluna do id de idioma de `a_col`. */
                        if let Some(l) = z_languageid.as_deref() {
                            if let Some(j) = a_col
                                .iter()
                                .position(|c| stricmp(Some(l), Some(c.as_slice())) == 0)
                            {
                                a_col.remove(j);
                            }
                        }
                    }
                    Err(e) => rc = e,
                }
            }
        }
    }
    if rc != SQLITE_OK {
        return Err(rc);
    }

    if a_col.is_empty() {
        a_col.push(b"content".to_vec());
    }

    let p_tokenizer = match p_tokenizer {
        Some(t) => t,
        None => match fts3_init_tokenizer(p_hash, b"simple", pz_err) {
            Ok(t) => t,
            Err(e) => return Err(e),
        },
    };

    let a_prefix = match fts3_prefix_parameter(z_prefix.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            if e == SQLITE_ERROR {
                *pz_err = mprintf(
                    b"error parsing prefix parameter: %s",
                    &[text_arg(z_prefix.as_deref().unwrap_or(b""))],
                );
            }
            return Err(e);
        }
    };

    /* Aloca e preenche a estrutura Fts3Table. */
    let mut p = Fts3Table::new(z_db_name.clone(), z_tbl_name);
    let n_col = a_col.len();
    p.p_tokenizer = Some(p_tokenizer);
    p.n_max_pending_data = FTS3_MAX_PENDING_DATA;
    p.n_pending_data = 0;
    p.b_has_docsize = is_fts4 && !b_no_docsize;
    p.b_has_stat = is_fts4 as u8;
    p.b_fts4 = is_fts4;
    p.b_desc_idx = b_desc_idx;
    p.n_autoincrmerge = 0xff; /* 0xff quer dizer que o valor é desconhecido */
    p.z_content_tbl = z_content;
    p.z_languageid = z_languageid;

    p.n_index = a_prefix.len() as i32;
    p.a_index = a_prefix
        .iter()
        .map(|&n_prefix| Fts3Index { n_prefix, h_pending: Fts3Hash::new(FTS3_HASH_STRING) })
        .collect();
    p.ab_notindexed = vec![0u8; n_col];

    /* Preenche o vetor azColumn. */
    for z_col in a_col.iter() {
        let mut name: Vec<u8> = match fts3_next_token(z_col) {
            Some((s, n)) if n > 0 => z_col[s..s + n].to_vec(),
            _ => Vec::new(),
        };
        fts3_dequote(&mut name);
        p.az_column.push(name);
    }

    /* Preenche o vetor abNotindexed. */
    for i_col in 0..n_col {
        let n = p.az_column[i_col].len();
        for slot in az_notindexed.iter_mut() {
            let hit = slot.as_ref().is_some_and(|z_not| {
                n == z_not.len()
                    && strnicmp(Some(p.az_column[i_col].as_slice()), Some(z_not.as_slice()), n as i32) == 0
            });
            if hit {
                p.ab_notindexed[i_col] = 1;
                *slot = None;
            }
        }
    }
    for slot in az_notindexed.iter() {
        if let Some(z) = slot {
            *pz_err = mprintf(b"no such column: %s", &[text_arg(z)]);
            rc = SQLITE_ERROR;
        }
    }

    if rc == SQLITE_OK && z_compress.is_some() != z_uncompress.is_some() {
        let z_miss: &[u8] = if z_compress.is_none() { b"compress" } else { b"uncompress" };
        rc = SQLITE_ERROR;
        *pz_err = mprintf(b"missing %s parameter in fts4 constructor", &[text_arg(z_miss)]);
    }
    p.z_read_exprlist = Some(fts3_read_expr_list(&p, z_uncompress.as_deref()));
    p.z_write_exprlist = Some(fts3_write_expr_list(&p, z_compress.as_deref()));
    if rc != SQLITE_OK {
        return Err(rc);
    }

    /* Num `xCreate`, cria as tabelas de apoio no banco. */
    if is_create {
        rc = fts3_create_tables(db, &mut p);
    }

    /* Vê se uma tabela fts3 antiga foi "atualizada" pela adição de `%_stat`, de modo que possa
    ** usar a fusão incremental. */
    if !is_fts4 && !is_create {
        p.b_has_stat = 2;
    }

    /* O tamanho de página do banco, de que o custo de carregar doclists grandes depende. */
    fts3_database_page_size(db, &mut rc, &mut p);
    p.n_node_size = p.n_pgsz - 35;

    /* Declara o esquema da tabela ao SQLite. */
    fts3_declare_vtab(db, &mut rc, &p);

    if rc != SQLITE_OK {
        fts3_disconnect(db, &mut p);
        return Err(rc);
    }
    Ok(Box::new(Fts3FullTable { base: p, cursors: Vec::new() }))
}

// ---------------------------------------------------------------------------------------------
// A tabela virtual
// ---------------------------------------------------------------------------------------------

/// `fts3SetHasStat`.
fn fts3_set_has_stat(db: &mut Connection, p: &mut Fts3Table) -> i32 {
    if p.b_has_stat == 2 {
        let z_tbl = mprintf(b"%s_stat", &[text_arg(&p.z_name)]).unwrap_or_default();
        let (res, _) = table_column_metadata(db, Some(&p.z_db), &z_tbl, None);
        p.b_has_stat = (res == SQLITE_OK) as u8;
    }
    SQLITE_OK
}

impl Vtab for Fts3FullTable {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.base.z_err_msg
    }

    /// `fts3BestIndexMethod`.
    fn best_index(&mut self, _db: &mut Connection, p_info: &mut IndexInfo) -> i32 {
        let p = &self.base;
        let n_column = p.n_column();
        let mut i_cons: Option<usize> = None; /* a restrição a usar */
        let mut i_langid_cons: Option<usize> = None; /* `langid=x` */
        let mut i_docid_ge: Option<usize> = None; /* `docid>=x` */
        let mut i_docid_le: Option<usize> = None; /* `docid<=x` */

        if p.b_lock != 0 {
            return SQLITE_ERROR;
        }
        if p_info.a_constraint_usage.len() < p_info.a_constraint.len() {
            p_info.a_constraint_usage.resize(p_info.a_constraint.len(), Default::default());
        }

        /* Por padrão usa uma varredura total da tabela, que é cara; procura nas restrições uma
        ** estratégia melhor. */
        p_info.idx_num = FTS3_FULLSCAN_SEARCH;
        p_info.estimated_cost = 5_000_000.0;
        for (i, p_cons) in p_info.a_constraint.iter().enumerate() {
            let op = p_cons.op as i32;
            if !p_cons.usable {
                if op == SQLITE_INDEX_CONSTRAINT_MATCH {
                    /* Há uma restrição MATCH inutilizável: se o planejador usar o resultado desta
                    ** chamada, o usuário verá o erro "unable to use function MATCH in the
                    ** requested context". Para desencorajar, devolve um custo muito alto. */
                    p_info.idx_num = FTS3_FULLSCAN_SEARCH;
                    p_info.estimated_cost = 1e50;
                    p_info.estimated_rows = 1i64 << 50;
                    return SQLITE_OK;
                }
                continue;
            }

            let b_docid = p_cons.i_column < 0 || p_cons.i_column == n_column + 1;

            /* Busca direta por rowid ou docid: custo 1.0. */
            if i_cons.is_none() && op == SQLITE_INDEX_CONSTRAINT_EQ && b_docid {
                p_info.idx_num = FTS3_DOCID_SEARCH;
                p_info.estimated_cost = 1.0;
                i_cons = Some(i);
            }

            /* Uma restrição MATCH: usa a busca de texto completo. Com mais de uma, usa a
            ** primeira; com MATCH e busca direta, prefere o MATCH. */
            if op == SQLITE_INDEX_CONSTRAINT_MATCH && p_cons.i_column >= 0 && p_cons.i_column <= n_column {
                p_info.idx_num = FTS3_FULLTEXT_SEARCH + p_cons.i_column;
                p_info.estimated_cost = 2.0;
                i_cons = Some(i);
            }

            /* Igualdade na coluna langid. */
            if op == SQLITE_INDEX_CONSTRAINT_EQ && p_cons.i_column == n_column + 2 {
                i_langid_cons = Some(i);
            }

            if b_docid {
                if op == SQLITE_INDEX_CONSTRAINT_GE || op == SQLITE_INDEX_CONSTRAINT_GT {
                    i_docid_ge = Some(i);
                } else if op == SQLITE_INDEX_CONSTRAINT_LE || op == SQLITE_INDEX_CONSTRAINT_LT {
                    i_docid_le = Some(i);
                }
            }
        }

        /* Com a estratégia docid=? ou rowid=?, liga a flag UNIQUE. */
        if p_info.idx_num == FTS3_DOCID_SEARCH {
            p_info.idx_flags |= SQLITE_INDEX_SCAN_UNIQUE;
        }

        let mut i_idx = 1;
        if let Some(i) = i_cons {
            p_info.a_constraint_usage[i].argv_index = i_idx;
            p_info.a_constraint_usage[i].omit = true;
            i_idx += 1;
        }
        if let Some(i) = i_langid_cons {
            p_info.idx_num |= FTS3_HAVE_LANGID;
            p_info.a_constraint_usage[i].argv_index = i_idx;
            i_idx += 1;
        }
        if let Some(i) = i_docid_ge {
            p_info.idx_num |= FTS3_HAVE_DOCID_GE;
            p_info.a_constraint_usage[i].argv_index = i_idx;
            i_idx += 1;
        }
        if let Some(i) = i_docid_le {
            p_info.idx_num |= FTS3_HAVE_DOCID_LE;
            p_info.a_constraint_usage[i].argv_index = i_idx;
        }

        /* Qualquer que seja a estratégia, o FTS entrega as linhas em ordem de rowid (ou docid),
        ** crescente ou decrescente. */
        if p_info.a_order_by.len() == 1 {
            let p_order = p_info.a_order_by[0];
            if p_order.i_column < 0 || p_order.i_column == n_column + 1 {
                p_info.idx_str = Some(if p_order.desc { b"DESC".to_vec() } else { b"ASC".to_vec() });
                p_info.order_by_consumed = 1;
            }
        }

        SQLITE_OK
    }

    /// `fts3DisconnectMethod`.
    fn disconnect(&mut self, db: &mut Connection) -> i32 {
        fts3_disconnect(db, &mut self.base);
        SQLITE_OK
    }

    /// `fts3DestroyMethod`.
    fn destroy(&mut self, db: &mut Connection) -> i32 {
        let p = &self.base;
        let mut rc = SQLITE_OK;
        let (z_db, z_name) = (text_arg(&p.z_db), text_arg(&p.z_name));
        let z_comment: &[u8] = if p.z_content_tbl.is_some() { b"--" } else { b"" };

        /* Apaga as tabelas sombra. */
        fts3_db_exec(
            &mut rc,
            db,
            b"DROP TABLE IF EXISTS %Q.'%q_segments';DROP TABLE IF EXISTS %Q.'%q_segdir';DROP TABLE IF EXISTS %Q.'%q_docsize';DROP TABLE IF EXISTS %Q.'%q_stat';%s DROP TABLE IF EXISTS %Q.'%q_content';",
            &[
                z_db.clone(),
                z_name.clone(),
                z_db.clone(),
                z_name.clone(),
                z_db.clone(),
                z_name.clone(),
                z_db.clone(),
                z_name.clone(),
                text_arg(z_comment),
                z_db,
                z_name,
            ],
        );

        if rc == SQLITE_OK {
            self.disconnect(db)
        } else {
            rc
        }
    }

    /// `fts3OpenMethod`.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        let id = NEXT_CSR_ID.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        self.cursors.push(CsrEntry { id, csr: Fts3Cursor::default() });
        Ok(Box::new(Fts3CsrHandle { id }))
    }

    /// `fts3UpdateMethod`.
    fn update(&mut self, db: &mut Connection, args: &[Mem], rowid: &mut i64) -> i32 {
        fts3_update_method(db, &mut self.base, args, rowid)
    }

    /// `fts3BeginMethod`.
    fn begin(&mut self, db: &mut Connection) -> i32 {
        debug_assert!(self.base.p_segments.is_none());
        debug_assert!(self.base.n_pending_data == 0);
        self.base.n_leaf_add = 0;
        fts3_set_has_stat(db, &mut self.base)
    }

    /// `fts3SyncMethod`.
    fn sync(&mut self, db: &mut Connection) -> i32 {
        /* Depois de uma fusão incremental os segmentos de entrada, em geral não consumidos por
        ** inteiro, são atualizados no lugar. Isso escreve (8*(1+N)) blocos para segmentos de
        ** altura N, então a fusão só é tentada se escrever pelo menos 64 blocos folha. */
        const N_MIN_MERGE: u32 = 64;

        let p = &mut self.base;
        let i_last_rowid = last_insert_rowid(db);

        let mut rc = fts3_pending_terms_flush(db, p);
        if rc == SQLITE_OK
            && p.n_leaf_add > (N_MIN_MERGE / 16)
            && p.n_autoincrmerge != 0
            && p.n_autoincrmerge != 0xff
        {
            let mut mx_level = 0; /* o maior nível relativo no banco */

            rc = fts3_max_level(db, p, &mut mx_level);
            debug_assert!(rc == SQLITE_OK || mx_level == 0);
            let mut a = p.n_leaf_add.wrapping_mul(mx_level as u32) as i32; /* parâmetro A */
            a += a / 2;
            if a > N_MIN_MERGE as i32 {
                let n_auto = p.n_autoincrmerge;
                rc = fts3_incrmerge(db, p, a, n_auto);
            }
        }
        fts3_segments_close(db, p);
        set_last_insert_rowid(db, i_last_rowid);
        rc
    }

    /// `fts3CommitMethod`.
    fn commit(&mut self, _db: &mut Connection) -> i32 {
        debug_assert!(self.base.n_pending_data == 0);
        debug_assert!(self.base.p_segments.is_none());
        SQLITE_OK
    }

    /// `fts3RollbackMethod`.
    fn rollback(&mut self, _db: &mut Connection) -> i32 {
        fts3_pending_terms_clear(&mut self.base);
        SQLITE_OK
    }

    /// `fts3FindFunctionMethod`.
    fn find_function(
        &mut self,
        _db: &mut Connection,
        _n_arg: i32,
        z_name: &[u8],
        out: &mut Option<(ScalarFn, UserData)>,
    ) -> i32 {
        let f: ScalarFn = match z_name {
            b"snippet" => fts3_snippet_func,
            b"offsets" => fts3_offsets_func,
            b"optimize" => fts3_optimize_func,
            b"matchinfo" => fts3_matchinfo_func,
            /* Nenhuma função com esse nome: devolve 0. */
            _ => return 0,
        };
        *out = Some((f, UserData::None));
        1
    }

    /// `fts3RenameMethod`.
    fn rename(&mut self, db: &mut Connection, z_name: &[u8]) -> i32 {
        let p = &mut self.base;

        /* Neste ponto se sabe se `%_stat` existe ou não: bHasStat não pode ser 2. */
        let mut rc = fts3_set_has_stat(db, p);

        /* A tabela de termos pendentes está sempre vazia aqui: um `ALTER TABLE RENAME` numa
        ** transação sempre abre um ponto de salvamento, e o `xSavepoint` a descarrega. */
        debug_assert!(p.n_pending_data == 0);
        if rc == SQLITE_OK {
            rc = fts3_pending_terms_flush(db, p);
        }

        p.b_ignore_savepoint = true;

        let args = [text_arg(&p.z_db), text_arg(&p.z_name), text_arg(z_name)];
        if p.z_content_tbl.is_none() {
            fts3_db_exec(&mut rc, db, b"ALTER TABLE %Q.'%q_content'  RENAME TO '%q_content';", &args);
        }
        if p.b_has_docsize {
            fts3_db_exec(&mut rc, db, b"ALTER TABLE %Q.'%q_docsize'  RENAME TO '%q_docsize';", &args);
        }
        if p.b_has_stat != 0 {
            fts3_db_exec(&mut rc, db, b"ALTER TABLE %Q.'%q_stat'  RENAME TO '%q_stat';", &args);
        }
        fts3_db_exec(&mut rc, db, b"ALTER TABLE %Q.'%q_segments' RENAME TO '%q_segments';", &args);
        fts3_db_exec(&mut rc, db, b"ALTER TABLE %Q.'%q_segdir'   RENAME TO '%q_segdir';", &args);

        p.b_ignore_savepoint = false;
        rc
    }

    /// `fts3SavepointMethod`.
    fn savepoint(&mut self, db: &mut Connection, i_savepoint: i32) -> i32 {
        let mut rc = SQLITE_OK;
        let p_tab = &mut self.base;

        if !p_tab.b_ignore_savepoint {
            if p_tab.a_index.first().is_some_and(|ix| ix.h_pending.count() > 0) {
                match mprintf(
                    b"INSERT INTO %Q.%Q(%Q) VALUES('flush')",
                    &[text_arg(&p_tab.z_db), text_arg(&p_tab.z_name), text_arg(&p_tab.z_name)],
                ) {
                    Some(z_sql) => {
                        p_tab.b_ignore_savepoint = true;
                        rc = exec(db, &z_sql, None);
                        p_tab.b_ignore_savepoint = false;
                    }
                    None => rc = SQLITE_NOMEM,
                }
            }
            if rc == SQLITE_OK {
                p_tab.i_savepoint = i_savepoint + 1;
            }
        }
        rc
    }

    /// `fts3ReleaseMethod`.
    fn release(&mut self, _db: &mut Connection, i_savepoint: i32) -> i32 {
        self.base.i_savepoint = i_savepoint;
        SQLITE_OK
    }

    /// `fts3RollbackToMethod`.
    fn rollback_to(&mut self, _db: &mut Connection, i_savepoint: i32) -> i32 {
        if (i_savepoint + 1) <= self.base.i_savepoint {
            fts3_pending_terms_clear(&mut self.base);
        }
        SQLITE_OK
    }

    /// `fts3IntegrityMethod`.
    fn integrity(
        &mut self,
        db: &mut Connection,
        z_schema: &[u8],
        z_tabname: &[u8],
        _flags: i32,
        pz_err: &mut Option<Vec<u8>>,
    ) -> i32 {
        let p = &mut self.base;
        let mut b_ok = false;
        let mut rc = fts3_integrity_check(db, p, &mut b_ok);
        debug_assert!(rc != crate::consts::SQLITE_CORRUPT_VTAB);
        let version = PrintfArg::Int(if p.b_fts4 { 4 } else { 3 });
        if rc == SQLITE_ERROR || (rc & 0xFF) == SQLITE_CORRUPT {
            *pz_err = mprintf(
                b"unable to validate the inverted index for FTS%d table %s.%s: %s",
                &[version, text_arg(z_schema), text_arg(z_tabname), text_arg(err_str(rc).as_bytes())],
            );
            if pz_err.is_some() {
                rc = SQLITE_OK;
            }
        } else if rc == SQLITE_OK && !b_ok {
            *pz_err = mprintf(
                b"malformed inverted index for FTS%d table %s.%s",
                &[version, text_arg(z_schema), text_arg(z_tabname)],
            );
            if pz_err.is_none() {
                rc = SQLITE_NOMEM;
            }
        }
        fts3_segments_close(db, p);
        rc
    }
}

// ---------------------------------------------------------------------------------------------
// O cursor do núcleo
// ---------------------------------------------------------------------------------------------

impl VtabCursor for Fts3CsrHandle {
    /// `fts3CloseMethod`.
    fn close(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        if let Some(pos) = tab.cursors.iter().position(|c| c.id == self.id) {
            let mut e = tab.cursors.swap_remove(pos);
            debug_assert!(tab.base.p_segments.is_none());
            fts3_clear_cursor(db, &mut tab.base, &mut e.csr);
        }
        SQLITE_OK
    }

    /// `fts3FilterMethod`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        idx_num: i32,
        idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let tab = tab_of(vtab);
        with_csr(tab, self.id, |p, csr| fts3_filter(db, p, csr, idx_num, idx_str, argv))
            .unwrap_or(SQLITE_ERROR)
    }

    /// `fts3NextMethod`.
    fn next(&mut self, db: &mut Connection, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        with_csr(tab, self.id, |p, csr| fts3_next(db, p, csr)).unwrap_or(SQLITE_ERROR)
    }

    /// `fts3EofMethod` (a limpeza do cursor no fim fica para `xClose` e o `xFilter` seguinte; ver
    /// o cabeçalho de `main2.rs`).
    fn eof(&mut self, vtab: &mut dyn Vtab) -> i32 {
        let tab = tab_of(vtab);
        match tab.cursors.iter().find(|c| c.id == self.id) {
            Some(c) => c.csr.is_eof as i32,
            None => 1,
        }
    }

    /// `fts3ColumnMethod`.
    fn column(&mut self, vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let tab = tab_of(vtab);
        let id = self.id;
        with_csr(tab, id, |p, csr| fts3_column(p, id, csr, ctx, i)).unwrap_or(SQLITE_ERROR)
    }

    /// `fts3RowidMethod`.
    fn rowid(&mut self, vtab: &mut dyn Vtab, p_rowid: &mut i64) -> i32 {
        let tab = tab_of(vtab);
        match tab.cursors.iter().find(|c| c.id == self.id) {
            Some(c) => {
                *p_rowid = c.csr.i_prev_id;
                SQLITE_OK
            }
            None => SQLITE_ERROR,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// As funções SQL sobrecarregadas
// ---------------------------------------------------------------------------------------------

/// Aplica o resultado de uma das funções ao contexto.
fn apply_result(ctx: &mut Context<'_>, res: Fts3Result) {
    match res {
        Fts3Result::Null => {}
        Fts3Result::Text(t) => result_text(ctx, Some(&t), t.len() as i32, crate::mem::StrDtor::Transient),
        Fts3Result::Blob(b) => result_blob(ctx, Some(&b), b.len() as i32, crate::mem::StrDtor::Transient),
        Fts3Result::Error(m) => result_error(ctx, &m, -1),
        Fts3Result::ErrorCode(rc) => result_error_code(ctx, rc),
        Fts3Result::Nomem => result_error_nomem(ctx),
    }
}

/// `fts3FunctionArg`: o id do cursor que `args[0]` carrega; senão grava o erro no contexto.
fn fts3_function_arg(ctx: &mut Context<'_>, z_func: &[u8], p_val: &Mem) -> Option<i64> {
    match value_pointer(p_val, b"fts3cursor").and_then(|a| a.downcast_ref::<Fts3CursorRef>()) {
        Some(r) => Some(r.id),
        None => {
            let z_err = mprintf(b"illegal first argument to %s", &[text_arg(z_func)]).unwrap_or_default();
            result_error(ctx, &z_err, -1);
            None
        }
    }
}

/// Roda `f` sobre o cursor `id` e aplica o resultado; cursor que não existe é o argumento inválido.
fn run_on_cursor(
    ctx: &mut Context<'_>,
    z_func: &[u8],
    id: i64,
    f: impl FnOnce(&mut Connection, &mut Fts3Table, &mut Fts3Cursor) -> Fts3Result,
) {
    match with_cursor(&mut *ctx.db, id, f) {
        Some(r) => apply_result(ctx, r),
        None => {
            let z_err = mprintf(b"illegal first argument to %s", &[text_arg(z_func)]).unwrap_or_default();
            result_error(ctx, &z_err, -1);
        }
    }
}

/// `fts3CursorSeek(pContext, pCsr)` como passo de uma função: o erro vira o resultado.
fn seek_or_error(db: &mut Connection, p: &mut Fts3Table, csr: &mut Fts3Cursor) -> Option<Fts3Result> {
    let rc = fts3_cursor_seek(db, p, csr);
    if rc != SQLITE_OK {
        Some(Fts3Result::ErrorCode(rc))
    } else {
        None
    }
}

/// `fts3SnippetFunc`.
fn fts3_snippet_func(ctx: &mut Context<'_>, ap_val: &[Mem]) {
    let n_val = ap_val.len();
    let mut z_start: Option<Vec<u8>> = Some(b"<b>".to_vec());
    let mut z_end: Option<Vec<u8>> = Some(b"</b>".to_vec());
    let mut z_ellipsis: Option<Vec<u8>> = Some(b"<b>...</b>".to_vec());
    let mut i_col: i32 = -1;
    let mut n_token: i32 = 15; /* o número padrão de tokens do snippet */

    debug_assert!(n_val >= 1);

    if n_val > 6 {
        result_error(ctx, b"wrong number of arguments to function snippet()", -1);
        return;
    }
    let Some(id) = fts3_function_arg(ctx, b"snippet", &ap_val[0]) else { return };

    let text = |m: &Mem| text_of(m).map(|c| fts3_c_str(&c).to_vec());
    if n_val >= 6 {
        n_token = value_int(&ap_val[5]);
    }
    if n_val >= 5 {
        i_col = value_int(&ap_val[4]);
    }
    if n_val >= 4 {
        z_ellipsis = text(&ap_val[3]);
    }
    if n_val >= 3 {
        z_end = text(&ap_val[2]);
    }
    if n_val >= 2 {
        z_start = text(&ap_val[1]);
    }
    let (Some(z_start), Some(z_end), Some(z_ellipsis)) = (z_start, z_end, z_ellipsis) else {
        result_error_nomem(ctx);
        return;
    };
    if n_token == 0 {
        result_text(ctx, Some(&b""[..]), -1, crate::mem::StrDtor::Static);
    } else {
        run_on_cursor(ctx, b"snippet", id, |db, p, csr| {
            if let Some(e) = seek_or_error(db, p, csr) {
                return e;
            }
            fts3_snippet(db, p, csr, &z_start, &z_end, &z_ellipsis, i_col, n_token)
        });
    }
}

/// `fts3OffsetsFunc`.
fn fts3_offsets_func(ctx: &mut Context<'_>, ap_val: &[Mem]) {
    debug_assert!(ap_val.len() == 1);
    let Some(id) = fts3_function_arg(ctx, b"offsets", &ap_val[0]) else { return };
    run_on_cursor(ctx, b"offsets", id, |db, p, csr| {
        if let Some(e) = seek_or_error(db, p, csr) {
            return e;
        }
        fts3_offsets(db, p, csr)
    });
}

/// `fts3OptimizeFunc`.
fn fts3_optimize_func(ctx: &mut Context<'_>, ap_val: &[Mem]) {
    debug_assert!(ap_val.len() == 1);
    let Some(id) = fts3_function_arg(ctx, b"optimize", &ap_val[0]) else { return };
    run_on_cursor(ctx, b"optimize", id, |db, p, _csr| match fts3_optimize(db, p) {
        SQLITE_OK => Fts3Result::Text(b"Index optimized".to_vec()),
        SQLITE_DONE => Fts3Result::Text(b"Index already optimal".to_vec()),
        rc => Fts3Result::ErrorCode(rc),
    });
}

/// `fts3MatchinfoFunc`.
fn fts3_matchinfo_func(ctx: &mut Context<'_>, ap_val: &[Mem]) {
    debug_assert!(ap_val.len() == 1 || ap_val.len() == 2);
    let Some(id) = fts3_function_arg(ctx, b"matchinfo", &ap_val[0]) else { return };
    let z_arg: Option<Vec<u8>> = if ap_val.len() > 1 {
        text_of(&ap_val[1]).map(|c| fts3_c_str(&c).to_vec())
    } else {
        None
    };
    run_on_cursor(ctx, b"matchinfo", id, |db, p, csr| fts3_matchinfo(db, p, csr, z_arg.as_deref()));
}

// ---------------------------------------------------------------------------------------------
// O módulo e sqlite3Fts3Init
// ---------------------------------------------------------------------------------------------

/// `fts3Module`: o módulo dos nomes `fts3` e `fts4`.
struct Fts3Module;

/// O registro de tokenizadores que é o `pAux` do módulo.
fn hash_of(aux: &Option<Rc<dyn Any>>) -> Result<Rc<Fts3HashWrapper>, i32> {
    aux.as_ref()
        .and_then(|a| Rc::clone(a).downcast::<Fts3HashWrapper>().ok())
        .ok_or(SQLITE_ERROR)
}

impl VtabModule for Fts3Module {
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

    /// `fts3CreateMethod`.
    fn x_create(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let hash = hash_of(aux)?;
        fts3_init_vtab(true, db, &hash, argv, err)
    }

    /// `fts3ConnectMethod`.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let hash = hash_of(aux)?;
        fts3_init_vtab(false, db, &hash, argv, err)
    }

    /// `fts3ShadowName`.
    fn x_shadow_name(&self, suffix: &[u8]) -> bool {
        const AZ_NAME: [&[u8]; 5] = [b"content", b"docsize", b"segdir", b"segments", b"stat"];
        AZ_NAME.iter().any(|n| stricmp(Some(suffix), Some(*n)) == 0)
    }
}

/// `sqlite3Fts3Init`: registra os módulos `fts3`, `fts4`, `fts4aux` e `fts3tokenize`, a função
/// `fts3_tokenizer()` e as sobrecargas de `snippet()`, `offsets()`, `matchinfo()` e `optimize()`.
pub fn fts3_init(db: &mut Connection) -> i32 {
    let mut rc = fts3_init_aux(db);
    if rc != SQLITE_OK {
        return rc;
    }

    /* A tabela hash dos tokenizadores, com os embutidos. */
    let p_hash = Rc::new(Fts3HashWrapper::new());
    p_hash.insert_module(b"simple", fts3_simple_tokenizer_module());
    p_hash.insert_module(b"porter", fts3_porter_tokenizer_module());
    p_hash.insert_module(b"unicode61", fts3_unicode_tokenizer_module());

    /* Cria a função que dá acesso à tabela hash e sobrecarrega as quatro funções escalares. Se
    ** der certo, registra os módulos. */
    rc = fts3_init_hash_table(db, &p_hash, b"fts3_tokenizer");
    for (name, n_arg) in [
        (b"snippet".as_slice(), -1),
        (b"offsets".as_slice(), 1),
        (b"matchinfo".as_slice(), 1),
        (b"matchinfo".as_slice(), 2),
        (b"optimize".as_slice(), 1),
    ] {
        if rc != SQLITE_OK {
            return rc;
        }
        rc = overload_function(db, name, n_arg);
    }
    if rc != SQLITE_OK {
        return rc;
    }

    let aux: Rc<dyn Any> = p_hash.clone();
    rc = create_module(db, b"fts3", Some(Rc::new(Fts3Module)), Some(aux.clone()), None);
    if rc == SQLITE_OK {
        rc = create_module(db, b"fts4", Some(Rc::new(Fts3Module)), Some(aux), None);
    }
    if rc == SQLITE_OK {
        rc = fts3_init_tok(db, p_hash);
    }
    rc
}
