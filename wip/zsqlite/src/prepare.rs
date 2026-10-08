//! Tradução de prepare.c (e do `sqlite3RunParser` de tokenize.c, que ainda não existia em
//! `tokenize.rs`): a implementação de `sqlite3_prepare()` e as rotinas que carregam o esquema do
//! banco do disco.
//!
//! Decisões do modelo v2 que aparecem aqui (ver CONVENTIONS.md e `connection.rs`):
//!
//! * Todas as funções recebem `db: &mut Connection` (e `parse: &mut Parse` quando o C recebia o
//!   `pParse`). O `Parse` não aponta para a conexão nem para o `Parse` externo
//!   (`pOuterParse`/`db->pParse` somem). `sqlite3ParseObjectInit` e `sqlite3ParseObjectReset`
//!   sobram como `parse_object_init` e `parse_object_reset`: o primeiro só cria o `Parse` (e
//!   avisa de falta de memória), o segundo desfaz o que não é posse do `Parse` (a contagem do
//!   lookaside). A lista `pCleanup` (`sqlite3ParserAddCleanup`) não existe: cada estrutura que
//!   o C registrava para limpar tardiamente é possuída por valor e se desfaz no `Drop`.
//! * O texto SQL é `&[u8]` e o fim dele é o fim da fatia ou o primeiro NUL, o que vier antes
//!   (`nBytes < 0` do C). O ponteiro `*pzTail` é o DESLOCAMENTO em bytes (o mesmo de
//!   `Parse.z_tail`). O `sqlite3_stmt**` é um `Option<StmtId>`: a função devolve a trinca
//!   `(rc, comando, deslocamento do resto)`.
//! * O `Vdbe` nasce dentro do `Parse` (`parse.p_vdbe`) e só entra em `Connection.stmts` (e em
//!   `Connection.stmt_list`, do mais antigo ao mais novo) quando o comando é aceito.
//! * `sqlite3Reprepare` recebe o `StmtId` do comando e o comando PRECISA estar na vaga do
//!   `Connection.stmts` (não emprestado por `take`): a recompilação lê as variáveis ligadas e o
//!   `explain` dele pelo handle (`Parse.p_reprepare`).
//! * `sqlite3_exec`, usado para ler a tabela de esquema, recebe o retorno de chamada como
//!   fechamento que também recebe a conexão (`crate::legacy::exec`), pois `init_callback`
//!   prepara comandos nela.
//! * O parser lemon é o de `parse_tables::parser`; o `sqlite3RunParser` mora aqui porque
//!   `tokenize.rs` não o traz. `tokenize.rs` deve reexportá-lo (`pub use crate::prepare::run_parser;`).
//! * Sem `SQLITE_ENABLE_API_ARMOR`, `SQLITE_DEBUG` (parser trace) e `SQLITE_OMIT_*`.

use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::btree::{btree_last_page, btree_set_cache_size};
use crate::btree_cursor::{btree_begin_trans, btree_commit, BtDb};
use crate::btree_types::Btree;
use crate::btree_write::{btree_get_meta, btree_schema_locked, btree_txn_state};
use crate::build::{
    commit_internal_changes, delete_table, find_index, reset_all_schemas_of_connection,
    reset_one_schema, schema_table, text_arg, token_arg,
};
use crate::callback::set_text_encoding;
use crate::connection::{Connection, Parse, StmtId};
use crate::consts::{
    BTREE_DEFAULT_CACHE_SIZE, BTREE_FILE_FORMAT, BTREE_SCHEMA_VERSION, BTREE_TEXT_ENCODING,
    DBFLAG_ENCODING_FIXED, DBFLAG_SCHEMA_CHANGE, DBFLAG_SCHEMA_KNOWN_OK, DBFLAG_VACUUM,
    DB_SCHEMALOADED, INITFLAG_ALTERMASK, SQLITE_CORRUPT_BKPT, SQLITE_DEFAULT_CACHE_SIZE,
    SQLITE_DONE, SQLITE_ERROR, SQLITE_ERROR_RETRY, SQLITE_INTERRUPT,
    SQLITE_IOERR_NOMEM, SQLITE_LEGACY_FILE_FMT, SQLITE_LIMIT_SQL_LENGTH, SQLITE_LOCKED,
    SQLITE_MAX_FILE_FORMAT, SQLITE_MISUSE, SQLITE_NOMEM, SQLITE_NOMEM_BKPT, SQLITE_NO_SCHEMA_ERROR,
    SQLITE_OK, SQLITE_PREPARE_MASK, SQLITE_PREPARE_PERSISTENT, SQLITE_PREPARE_SAVESQL,
    SQLITE_RESET_DATABASE, SQLITE_SCHEMA, SQLITE_TOOBIG, SQLITE_TXN_NONE, SQLITE_UTF16NATIVE,
    SQLITE_UTF8, SQLITE_WRITE_SCHEMA, TK_AS, TK_FILTER, TK_ID, TK_JOIN_KW, TK_LP, TK_OVER,
    TK_QNUMBER, TK_RP, TK_SEMI, TK_SPACE, TK_STRING, TK_WINDOW,
};
use crate::ctype::UPPER_TO_LOWER;
use crate::global::{extra_schema_checks, log};
use crate::hash::{hash_find_mut};
use crate::parse_tables::{parser, parser_finalize, parser_init, parser_fallback};
use crate::sqlite_int::{Index, SchemaId, Table, Token};
use crate::tokenize::get_token;
use crate::util::{abs_int32, at, err_str, error_msg, get_u_int32, oom_fault, str_icmp};
use crate::utf::{translate_bytes, utf16_byte_len, utf8_char_len};
use crate::vdbeaux::{vdbe_set_sql, vdbe_swap};
use crate::vdbeaux2::{vdbe_finalize, vdbe_reset_step_result};

/// O resultado de uma preparação: o código de retorno, o comando (se houve) e o deslocamento do
/// resto do texto depois do primeiro comando (`*pzTail`).
pub type PrepareResult = (i32, Option<StmtId>, usize);

/// `SQLITE_MAX_PREPARE_RETRY`: quantas vezes se tenta de novo preparar um comando que devolve
/// `SQLITE_ERROR_RETRY`.
const SQLITE_MAX_PREPARE_RETRY: i32 = 25;

/// `InitData`: o contexto de `sqlite3InitCallback` durante a leitura do esquema. A conexão (o
/// `db` do C) vai por parâmetro.
pub struct InitData<'a> {
    /// Qual banco (índice em `Connection.dbs`) está sendo inicializado.
    pub i_db: i32,
    /// Resultado da inicialização.
    pub rc: i32,
    /// Onde guardar a mensagem de erro (`char **pzErrMsg`).
    pub pz_err_msg: &'a mut Option<Vec<u8>>,
    /// Zero ou mais `INITFLAG_*`.
    pub m_init_flags: u32,
    /// Número de linhas já lidas de `sqlite_schema`.
    pub n_init_row: u32,
    /// Maior número de página do arquivo (0: sem limite).
    pub mx_page: u32,
}

/// `corruptSchema`: preenche a mensagem de erro de `p_data` para indicar que o banco está
/// corrompido. `az_obj` é o par (tipo, nome) do objeto em análise.
fn corrupt_schema(
    db: &mut Connection,
    p_data: &mut InitData<'_>,
    az_obj: &[Option<Vec<u8>>],
    z_extra: Option<&[u8]>,
) {
    if db.malloc_failed != 0 {
        p_data.rc = SQLITE_NOMEM_BKPT;
    } else if p_data.pz_err_msg.is_some() {
        // Já existe uma mensagem de erro: não a sobrescreve.
    } else if p_data.m_init_flags & INITFLAG_ALTERMASK != 0 {
        const AZ_ALTER_TYPE: [&[u8]; 3] = [b"rename", b"drop column", b"add column"];
        let kind = AZ_ALTER_TYPE[((p_data.m_init_flags & INITFLAG_ALTERMASK) - 1) as usize];
        *p_data.pz_err_msg = crate::printf::mprintf(
            b"error in %s %s after %s: %s",
            &[
                crate::printf::PrintfArg::Text(az_obj[0].clone()),
                crate::printf::PrintfArg::Text(az_obj[1].clone()),
                text_arg(kind),
                crate::printf::PrintfArg::Text(z_extra.map(|z| z.to_vec())),
            ],
        );
        p_data.rc = SQLITE_ERROR;
    } else if db.flags & SQLITE_WRITE_SCHEMA != 0 {
        p_data.rc = SQLITE_CORRUPT_BKPT;
    } else {
        let z_obj: &[u8] = az_obj[1].as_deref().unwrap_or(b"?");
        let mut z = crate::printf::mprintf(b"malformed database schema (%s)", &[text_arg(z_obj)]);
        if let Some(extra) = z_extra {
            if at(extra, 0) != 0 {
                z = crate::printf::mprintf(
                    b"%s - %s",
                    &[crate::printf::PrintfArg::Text(z), text_arg(extra)],
                );
            }
        }
        *p_data.pz_err_msg = z;
        p_data.rc = SQLITE_CORRUPT_BKPT;
    }
}

/// `sqlite3IndexHasDuplicateRootPage`: verdadeiro se algum índice irmão de `p_index` (outro
/// índice da mesma tabela) tem o mesmo número de página raiz, o que indica um esquema corrompido.
/// `p_index` é comparado por identidade: pode estar na lista da tabela ou ser um índice novo.
pub fn index_has_duplicate_root_page(p_table: &Table, p_index: &Index) -> bool {
    p_table
        .p_index
        .iter()
        .any(|p| p.tnum == p_index.tnum && !std::ptr::eq(&**p, p_index))
}

/// `sqlite3InitCallback`: o retorno de chamada do código que inicializa o banco (ver
/// `sqlite3Init()`). Também chamado pelo opcode `OP_ParseSchema`.
///
/// `argv`, com cinco colunas de uma linha de `sqlite_schema`:
///
/// * `argv[0]`: tipo do objeto: "table", "index", "trigger" ou "view".
/// * `argv[1]`: nome do objeto.
/// * `argv[2]`: tabela associada, se é índice ou gatilho.
/// * `argv[3]`: número da página raiz da tabela ou índice. 0 para gatilho ou view.
/// * `argv[4]`: texto SQL do CREATE.
///
/// `argv` vazio é o `argv == 0` do C (callback de resultado vazio).
pub fn init_callback(
    db: &mut Connection,
    p_data: &mut InitData<'_>,
    argv: &[Option<Vec<u8>>],
) -> i32 {
    let i_db = p_data.i_db;

    debug_assert!(argv.is_empty() || argv.len() == 5);
    db.m_db_flags |= DBFLAG_ENCODING_FIXED;
    if argv.is_empty() {
        return 0; // Pode acontecer com EMPTY_RESULT_CALLBACKS ligado.
    }
    p_data.n_init_row += 1;
    if db.malloc_failed != 0 {
        corrupt_schema(db, p_data, argv, None);
        return 1;
    }

    debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
    let sql_starts_cr = argv[4].as_deref().map_or(false, |z| {
        UPPER_TO_LOWER[at(z, 0) as usize] == b'c' && UPPER_TO_LOWER[at(z, 1) as usize] == b'r'
    });
    if argv[3].is_none() {
        corrupt_schema(db, p_data, argv, None);
    } else if sql_starts_cr {
        // Chama o parser para processar um CREATE TABLE, INDEX ou VIEW. Como `db.init.busy` é 1,
        // nenhum código do VDBE é gerado nem executado: o parser só monta as estruturas internas
        // que descrevem a tabela, o índice ou a view.
        //
        // Nenhum outro comando SQL válido, fora os CREATE, começa com as letras "C" e "R". Então
        // não é possível rodar outro tipo de comando durante a leitura do esquema, mesmo de um
        // esquema corrompido.
        let saved_i_db = db.init.i_db;
        debug_assert!(db.init.busy != 0);
        db.init.i_db = i_db as u8;
        let z_rootpage = argv[3].as_deref().unwrap_or(&[]);
        let new_tnum = get_u_int32(z_rootpage);
        db.init.new_tnum = new_tnum.unwrap_or(0);
        if new_tnum.is_none() || (db.init.new_tnum > p_data.mx_page && p_data.mx_page > 0) {
            if extra_schema_checks() {
                corrupt_schema(db, p_data, argv, Some(b"invalid rootpage"));
            }
        }
        db.init.orphan_trigger = false;
        db.init.az_init = argv.to_vec();
        let (_rcp, p_stmt, _tail) = prepare(db, argv[4].as_deref().unwrap_or(&[]), -1, 0, None);
        let rc = db.err_code;
        db.init.i_db = saved_i_db;
        if rc != SQLITE_OK {
            if db.init.orphan_trigger {
                debug_assert!(i_db == 1);
            } else {
                if rc > p_data.rc {
                    p_data.rc = rc;
                }
                if rc == SQLITE_NOMEM {
                    oom_fault(db);
                } else if rc != SQLITE_INTERRUPT && (rc & 0xFF) != SQLITE_LOCKED {
                    let msg = crate::main::errmsg(db);
                    corrupt_schema(db, p_data, argv, Some(msg.as_slice()));
                }
            }
        }
        db.init.az_init = Vec::new(); // Qualquer vetor de cadeias serve.
        if let Some(stmt) = p_stmt {
            crate::vdbeapi::finalize(db, stmt);
        }
    } else if argv[1].is_none() || argv[4].as_deref().map_or(false, |z| at(z, 0) != 0) {
        corrupt_schema(db, p_data, argv, None);
    } else {
        // Se a coluna SQL está vazia, é um índice criado para ser a PRIMARY KEY ou para cumprir
        // uma restrição UNIQUE de um CREATE TABLE. O índice já deve ter sido criado quando se
        // processou o CREATE TABLE. Resta só registrar o número da página raiz dele.
        let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
        let found = find_index(db, argv[1].as_deref().unwrap_or(&[]), Some(&z_db))
            .map(|(t, i)| (t.z_name.clone(), i.z_name.clone()));
        match found {
            None => corrupt_schema(db, p_data, argv, Some(b"orphan index")),
            Some((tab_name, idx_name)) => {
                let tnum = get_u_int32(argv[3].as_deref().unwrap_or(&[]));
                let mut invalid = tnum.is_none();
                let schema = &mut db.dbs[i_db as usize].schema;
                if let Some(tab) = hash_find_mut(&mut schema.tbl_hash, &tab_name) {
                    let tab = Rc::make_mut(tab);
                    if let Some(pos) =
                        tab.p_index.iter().position(|x| str_icmp(&x.z_name, &idx_name) == 0)
                    {
                        Rc::make_mut(&mut tab.p_index[pos]).tnum = tnum.unwrap_or(0);
                        let tab: &Table = tab;
                        let p_index = &tab.p_index[pos];
                        if p_index.tnum < 2
                            || p_index.tnum > p_data.mx_page
                            || index_has_duplicate_root_page(tab, p_index)
                        {
                            invalid = true;
                        }
                    }
                }
                if invalid && extra_schema_checks() {
                    corrupt_schema(db, p_data, argv, Some(b"invalid rootpage"));
                }
            }
        }
    }
    0
}

/// Roda `f` com o `Btree` de `dbs[i_db]` e o `BtDb` que as rotinas de transação do btree leem da
/// conexão (savepoints, leitores, busy handler, `autovacuum_pages`). `None` se o banco não tem
/// `Btree`.
fn with_bt<R>(
    db: &mut Connection,
    i_db: usize,
    f: impl FnOnce(&mut Btree, &mut BtDb<'_>) -> R,
) -> Option<R> {
    let Connection {
        dbs,
        x_autovac_pages,
        n_savepoint,
        n_vdbe_read,
        temp_store,
        busy_handler,
        ..
    } = db;
    let slot = dbs.get_mut(i_db)?;
    let name = slot.z_db_s_name.clone();
    let bt = slot.bt.as_mut()?;
    let has_autovac = x_autovac_pages.is_some();
    let mut cb = |n_orig: u32, n_free: u32, page_size: u32| -> u32 {
        match x_autovac_pages.as_mut() {
            Some(h) => h(&name, n_orig, n_free, page_size),
            None => 0,
        }
    };
    let busy_fn = busy_handler.x_busy_handler.clone();
    let n_busy = busy_handler.n_busy.clone();
    let has_busy = busy_fn.is_some();
    // `sqlite3InvokeBusyHandler`.
    let mut busy = move || -> bool {
        match &busy_fn {
            Some(h) if n_busy.get() >= 0 => {
                let rc = h(n_busy.get());
                if rc == 0 {
                    n_busy.set(-1);
                } else {
                    n_busy.set(n_busy.get() + 1);
                }
                rc != 0
            }
            _ => false,
        }
    };
    let mut bdb = BtDb {
        n_savepoint: *n_savepoint,
        n_vdbe_read: *n_vdbe_read,
        temp_in_memory: *temp_store == 2,
        busy: if has_busy { Some(&mut busy) } else { None },
        autovac_pages: if has_autovac { Some(&mut cb) } else { None },
    };
    Some(f(bt, &mut bdb))
}

/// `sqlite3BtreeBeginTrans(pBt, 0, 0)` sobre o banco `i_db`.
fn begin_read_trans(db: &mut Connection, i_db: usize) -> i32 {
    with_bt(db, i_db, |bt, bdb| btree_begin_trans(bt, 0, None, bdb)).unwrap_or(SQLITE_OK)
}

/// `sqlite3BtreeCommit(pBt)` sobre o banco `i_db`.
fn commit_trans(db: &mut Connection, i_db: usize) -> i32 {
    with_bt(db, i_db, |bt, bdb| btree_commit(bt, bdb)).unwrap_or(SQLITE_OK)
}

/// `sqlite3BtreeGetMeta(pBt, idx)` sobre o banco `i_db` (0 sem `Btree`).
fn get_meta(db: &Connection, i_db: usize, idx: u32) -> u32 {
    db.dbs[i_db].bt.as_ref().map_or(0, |bt| btree_get_meta(bt, idx as i32))
}

/// `sqlite3InitOne`: tenta ler o esquema e inicializar as estruturas internas de um único
/// arquivo de banco. O índice do banco é `i_db`: 0 é o principal, 1 nunca deve ser usado
/// (é o TEMP), 2 em diante são os anexados. Devolve um código `SQLITE_*`.
pub fn init_one(
    db: &mut Connection,
    i_db: i32,
    pz_err_msg: &mut Option<Vec<u8>>,
    m_flags: u32,
) -> i32 {
    let i = i_db as usize;
    let mask = (db.m_db_flags & DBFLAG_ENCODING_FIXED) | !DBFLAG_ENCODING_FIXED;

    debug_assert!(db.m_db_flags & DBFLAG_SCHEMA_KNOWN_OK == 0);
    debug_assert!(i_db >= 0 && i < db.dbs.len());

    db.init.busy = 1;

    // Monta a representação em memória das tabelas de esquema (sqlite_schema ou
    // sqlite_temp_schema) chamando o parser diretamente. O nome da tabela entra sozinho, então
    // basta a abreviação "x". O parser também marca a tabela como somente leitura.
    let z_schema_tab_name = schema_table(i_db);
    let az_arg: [Option<Vec<u8>>; 5] = [
        Some(b"table".to_vec()),
        Some(z_schema_tab_name.to_vec()),
        Some(z_schema_tab_name.to_vec()),
        Some(b"1".to_vec()),
        Some(
            b"CREATE TABLE x(type text,name text,tbl_name text,rootpage int,sql text)".to_vec(),
        ),
    ];
    let mut init_data = InitData {
        i_db,
        rc: SQLITE_OK,
        pz_err_msg,
        m_init_flags: m_flags,
        n_init_row: 0,
        mx_page: 0,
    };
    init_callback(db, &mut init_data, &az_arg);
    db.m_db_flags &= mask;

    let mut rc = SQLITE_OK;
    'error_out: {
        if init_data.rc != 0 {
            rc = init_data.rc;
            break 'error_out;
        }

        // Cria um cursor para manter o banco aberto.
        if db.dbs[i].bt.is_none() {
            debug_assert!(i_db == 1);
            db.db_set_property(1, DB_SCHEMALOADED);
            rc = SQLITE_OK;
            break 'error_out;
        }

        // Se ainda não há uma transação de leitura (ou escrita) aberta na árvore-b, abre uma
        // agora. Se abriu, ela é fechada antes de a função retornar.
        let mut opened_transaction = false;
        'initone_error_out: {
            if btree_txn_state(db.dbs[i].bt.as_ref()) == SQLITE_TXN_NONE {
                rc = begin_read_trans(db, i);
                if rc != SQLITE_OK {
                    *init_data.pz_err_msg = Some(err_str(rc).as_bytes().to_vec());
                    break 'initone_error_out;
                }
                opened_transaction = true;
            }

            // Lê as meta-informações do banco.
            //
            // Valores de meta:
            //    meta[0]   Cookie do esquema. Muda a cada mudança de esquema.
            //    meta[1]   Formato de arquivo da camada de esquema.
            //    meta[2]   Tamanho do cache de páginas.
            //    meta[3]   Maior página raiz (modo auto/incr_vacuum).
            //    meta[4]   Codificação do texto. 1:UTF-8 2:UTF-16LE 3:UTF-16BE
            //    meta[5]   Versão do usuário
            //    meta[6]   Modo de vacuum incremental
            //    meta[7..9] sem uso
            let mut meta = [0i32; 5];
            for (k, m) in meta.iter_mut().enumerate() {
                *m = get_meta(db, i, k as u32 + 1) as i32;
            }
            if (db.flags & SQLITE_RESET_DATABASE) != 0 {
                meta = [0; 5];
            }
            db.dbs[i].schema.schema_cookie = meta[(BTREE_SCHEMA_VERSION - 1) as usize];

            // Se abre um banco não vazio, confere a codificação do texto. No banco principal
            // define `sqlite3.enc` com a codificação dele. Num anexado é erro a codificação
            // diferir de `sqlite3.enc`.
            let meta_enc = meta[(BTREE_TEXT_ENCODING - 1) as usize];
            if meta_enc != 0 {
                if i_db == 0 && (db.m_db_flags & DBFLAG_ENCODING_FIXED) == 0 {
                    // Abrindo o banco principal: define ENC(db).
                    let mut encoding = (meta_enc as u8) & 3;
                    if encoding == 0 {
                        encoding = SQLITE_UTF8 as u8;
                    }
                    if db.n_vdbe_active > 0
                        && encoding != db.enc
                        && (db.m_db_flags & DBFLAG_VACUUM) == 0
                    {
                        rc = SQLITE_LOCKED;
                        break 'initone_error_out;
                    } else {
                        set_text_encoding(db, encoding);
                    }
                } else if (meta_enc & 3) != db.enc as i32 {
                    // Num banco anexado a codificação tem de casar com ENC(db).
                    *init_data.pz_err_msg = Some(
                        b"attached databases must use the same text encoding as main database"
                            .to_vec(),
                    );
                    rc = SQLITE_ERROR;
                    break 'initone_error_out;
                }
            }
            db.dbs[i].schema.enc = db.enc;

            if db.dbs[i].schema.cache_size == 0 {
                let mut size = abs_int32(meta[(BTREE_DEFAULT_CACHE_SIZE - 1) as usize]);
                if size == 0 {
                    size = SQLITE_DEFAULT_CACHE_SIZE;
                }
                db.dbs[i].schema.cache_size = size;
                if let Some(bt) = db.dbs[i].bt.as_mut() {
                    btree_set_cache_size(bt, size);
                }
            }

            // file_format==1    Versão 3.0.0.
            // file_format==2    Versão 3.1.3.  // ALTER TABLE ADD COLUMN
            // file_format==3    Versão 3.1.4.  // idem, com defaults não NULL
            // file_format==4    Versão 3.3.0.  // índices DESC. Constantes booleanas
            let file_format = meta[(BTREE_FILE_FORMAT - 1) as usize];
            db.dbs[i].schema.file_format = file_format as u8;
            if db.dbs[i].schema.file_format == 0 {
                db.dbs[i].schema.file_format = 1;
            }
            if db.dbs[i].schema.file_format as i32 > SQLITE_MAX_FILE_FORMAT {
                *init_data.pz_err_msg = Some(b"unsupported file format".to_vec());
                rc = SQLITE_ERROR;
                break 'initone_error_out;
            }

            // Ticket #2804: ao abrir um banco no formato novo, limpa a flag do pragma
            // legacy_file_format para que um VACUUM não faça o downgrade do banco e invalide
            // índices descendentes que o usuário possa ter criado.
            if i_db == 0 && file_format >= 4 {
                db.flags &= !SQLITE_LEGACY_FILE_FMT;
            }

            // Lê as informações do esquema das tabelas de esquema.
            debug_assert!(db.init.busy != 0);
            init_data.mx_page = db.dbs[i].bt.as_ref().map_or(0, btree_last_page);
            {
                let z_sql = crate::printf::mprintf(
                    b"SELECT*FROM\"%w\".%s ORDER BY rowid",
                    &[text_arg(&db.dbs[i].z_db_s_name), text_arg(z_schema_tab_name)],
                )
                .unwrap_or_default();
                // A autorização fica desligada durante a leitura do esquema.
                let x_auth = db.x_auth.take();
                let mut cb = |db: &mut Connection, argv: &[Option<Vec<u8>>], _cols: &[Vec<u8>]| {
                    init_callback(db, &mut init_data, argv)
                };
                rc = crate::legacy::exec(db, &z_sql, Some(&mut cb));
                db.x_auth = x_auth;
                if rc == SQLITE_OK {
                    rc = init_data.rc;
                }
                if rc == SQLITE_OK {
                    crate::analyze::analysis_load(db, i_db);
                }
            }
            if db.malloc_failed != 0 {
                rc = SQLITE_NOMEM_BKPT;
                reset_all_schemas_of_connection(db);
            } else if rc == SQLITE_OK
                || ((db.flags & SQLITE_NO_SCHEMA_ERROR) != 0 && rc != SQLITE_NOMEM)
            {
                // Hack: com a flag SQLITE_NoSchemaError o esquema é dado como carregado mesmo
                // que tenham ocorrido erros (exceto falta de memória). O `sqlite3_prepare()`
                // corrente falha, mas o seguinte tenta compilar o comando sobre o subconjunto do
                // esquema que foi carregado antes do erro.
                //
                // O objetivo é permitir o acesso à tabela sqlite_schema mesmo com o conteúdo
                // dela corrompido.
                db.db_set_property(i, DB_SCHEMALOADED);
                rc = SQLITE_OK;
            }
        }

        // `initone_error_out`: aqui se chega com erro depois de a transação ter sido aberta.
        if opened_transaction {
            commit_trans(db, i);
        }
    }

    // `error_out`.
    if rc != SQLITE_OK {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            oom_fault(db);
        }
        reset_one_schema(db, i_db);
    }
    db.init.busy = 0;
    rc
}

/// `sqlite3Init`: inicializa todos os arquivos de banco (o principal, o das tabelas temporárias
/// e os anexados com ATTACH). Devolve um código de sucesso; em erro grava uma mensagem em
/// `pz_err_msg`. Depois da inicialização o bit `DB_SchemaLoaded` fica ligado nas flags do `Db`.
pub fn init(db: &mut Connection, pz_err_msg: &mut Option<Vec<u8>>) -> i32 {
    let commit_internal = (db.m_db_flags & DBFLAG_SCHEMA_CHANGE) == 0;

    debug_assert!(db.init.busy == 0);
    db.enc = db.schema_enc();
    debug_assert!(!db.dbs.is_empty());
    // Primeiro o esquema principal.
    if !db.db_has_property(0, DB_SCHEMALOADED) {
        let rc = init_one(db, 0, pz_err_msg, 0);
        if rc != 0 {
            return rc;
        }
    }
    // Todos os outros depois do principal. O esquema "temp" tem de ser o último.
    let mut i = db.dbs.len() as i32 - 1;
    while i > 0 {
        if !db.db_has_property(i as usize, DB_SCHEMALOADED) {
            let rc = init_one(db, i, pz_err_msg, 0);
            if rc != 0 {
                return rc;
            }
        }
        i -= 1;
    }
    if commit_internal {
        commit_internal_changes(db);
    }
    SQLITE_OK
}

/// `sqlite3ReadSchema`: não faz nada se o esquema já está inicializado. Senão o carrega. Devolve
/// um código de erro.
pub fn read_schema(db: &mut Connection, parse: &mut Parse) -> i32 {
    let mut rc = SQLITE_OK;
    if db.init.busy == 0 {
        rc = init(db, &mut parse.z_err_msg);
        if rc != SQLITE_OK {
            parse.rc = rc;
            parse.n_err += 1;
        } else if db.no_shared_cache != 0 {
            db.m_db_flags |= DBFLAG_SCHEMA_KNOWN_OK;
        }
    }
    rc
}

/// `schemaIsValid`: confere os cookies de esquema de todos os bancos. Se algum está velho, põe
/// `parse.rc` em `SQLITE_SCHEMA`. Se todos estão em dia, não mexe em `parse.rc`.
fn schema_is_valid(db: &mut Connection, parse: &mut Parse) {
    debug_assert!(parse.check_schema != 0);
    let mut i_db = 0usize;
    while i_db < db.dbs.len() {
        let mut opened_transaction = false; // Verdadeiro se abriu uma transação.
        if db.dbs[i_db].bt.is_none() {
            i_db += 1;
            continue;
        }

        // Se ainda não há uma transação de leitura (ou escrita) aberta na árvore-b, abre uma
        // agora. Se abriu, ela é fechada logo depois de lido o meta-valor.
        if btree_txn_state(db.dbs[i_db].bt.as_ref()) == SQLITE_TXN_NONE {
            let rc = begin_read_trans(db, i_db);
            if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                oom_fault(db);
                parse.rc = SQLITE_NOMEM;
            }
            if rc != SQLITE_OK {
                return;
            }
            opened_transaction = true;
        }

        // Lê o cookie do esquema do banco. Se não casa com o valor guardado na representação em
        // memória do esquema, põe `Parse.rc` em SQLITE_SCHEMA.
        let cookie = get_meta(db, i_db, BTREE_SCHEMA_VERSION) as i32;
        if cookie != db.dbs[i_db].schema.schema_cookie {
            if db.db_has_property(i_db, DB_SCHEMALOADED) {
                parse.rc = SQLITE_SCHEMA;
            }
            reset_one_schema(db, i_db as i32);
        }

        // Fecha a transação, se abriu uma.
        if opened_transaction {
            commit_trans(db, i_db);
        }
        i_db += 1;
    }
}

/// `sqlite3SchemaToIndex`: converte um esquema (o `SchemaId`) no índice `iDb` do banco em
/// `db.dbs`. Se o mesmo banco foi anexado mais de uma vez, devolve o primeiro anexado.
///
/// Se `schema` é nenhum (`SchemaId(0)`), devolve -32768. Isso acontece quando o código de
/// expr.c tenta resolver uma referência a uma tabela transitória (criada por um sub-select);
/// o valor então não deve ser usado. O C devolve -32768 em vez do usual -1 porque é muito mais
/// provável um índice assim estourar o acesso a `aDb[]`, e ainda cabe num inteiro de 16 bits.
pub fn schema_to_index(db: &Connection, schema: SchemaId) -> i32 {
    let mut i = -32768;
    if schema != SchemaId(0) {
        match db.dbs.iter().position(|d| d.schema.id == schema) {
            Some(pos) => i = pos as i32,
            None => debug_assert!(false, "esquema fora de Connection.dbs"),
        }
    }
    i
}

/// `sqlite3ParseObjectInit`: um `Parse` novo para a conexão `db`. O encadeamento com o `Parse`
/// externo (`pOuterParse`, `db->pParse`) não existe no modelo v2.
pub fn parse_object_init(db: &mut Connection) -> Parse {
    let mut parse = Parse::default();
    if db.malloc_failed != 0 {
        error_msg(db, &mut parse, b"out of memory", &[]);
    }
    parse
}

/// `sqlite3ParseObjectReset`: libera o que o `Parse` guarda e devolve à conexão o que ele tomou
/// emprestado. As tabelas de travas, os rótulos e as expressões constantes se desfazem aqui
/// (no C são memória da conexão); o resto cai com o `Drop` do `Parse`.
pub fn parse_object_reset(db: &mut Connection, parse: &mut Parse) {
    debug_assert!(parse.nested == 0);
    parse.a_table_lock = Vec::new();
    parse.a_label = Vec::new();
    parse.p_const_expr = None;
    debug_assert!(db.lookaside.b_disable >= parse.disable_lookaside as u32);
    db.lookaside.b_disable = db.lookaside.b_disable.saturating_sub(parse.disable_lookaside as u32);
    db.lookaside.sz =
        if db.lookaside.b_disable != 0 { 0 } else { db.lookaside.sz_true };
}

/// Põe um comando recém-aceito em `Connection.stmts` e o encadeia como o mais novo.
fn register_stmt(db: &mut Connection, v: crate::vdbe_types::Vdbe) -> StmtId {
    let id = StmtId::from_slot(db.stmts.insert(v));
    db.stmt_list.push(id);
    id
}

/// `sqlite3Prepare`: compila o comando SQL UTF-8 `z_sql` num comando preparado. `n_bytes` é o
/// tamanho do texto (negativo: vai até o NUL ou o fim da fatia). `p_reprepare` é o comando que
/// está sendo recompilado. Devolve `(rc, comando, deslocamento do resto)`.
pub fn prepare(
    db: &mut Connection,
    z_sql: &[u8],
    n_bytes: i32,
    prep_flags: u32,
    p_reprepare: Option<StmtId>,
) -> PrepareResult {
    let mut rc = SQLITE_OK; // Código de retorno.
    let mut p_stmt: Option<StmtId> = None;
    let mut tail = 0usize;
    let mut parse = Parse::default(); // Contexto de análise.

    if let Some(old) = p_reprepare {
        parse.p_reprepare = Some(old);
        parse.explain = db.stmt(old).map_or(0, |v| v.explain);
    }

    'end_prepare: {
        if db.malloc_failed != 0 {
            error_msg(db, &mut parse, b"out of memory", &[]);
            rc = SQLITE_NOMEM;
            db.err_code = rc;
            break 'end_prepare;
        }

        // Para um comando preparado de longa duração, evita o lookaside.
        if prep_flags & SQLITE_PREPARE_PERSISTENT != 0 {
            parse.disable_lookaside += 1;
            db.lookaside.b_disable += 1;
            db.lookaside.sz = 0;
        }
        parse.prep_flags = (prep_flags & 0xff) as u8;

        // Confere se é possível obter uma trava de leitura em todos os esquemas. Não conseguir
        // quer dizer que outra conexão tem uma trava de escrita, ou seja, tem mudanças de esquema
        // não confirmadas. Se se prosseguisse e o comando fosse preparado contra o esquema ainda
        // não confirmado, e as mudanças depois fossem desfeitas e outras no lugar delas, o cookie
        // do esquema não perceberia a mudança quando o comando fosse executar. O desastre viria.
        //
        // Note que READ_UNCOMMITTED desliga a maior parte da detecção de travas, mas NÃO a de
        // travas de esquema, então tudo isso continua valendo com READ_UNCOMMITTED.
        if db.no_shared_cache == 0 {
            for i in 0..db.dbs.len() {
                let Some(bt) = db.dbs[i].bt.as_mut() else {
                    continue;
                };
                rc = btree_schema_locked(bt);
                if rc != 0 {
                    let z_db = db.dbs[i].z_db_s_name.clone();
                    crate::main::error_with_msg(
                        db,
                        rc,
                        b"database schema is locked: %s",
                        &[text_arg(&z_db)],
                    );
                    break 'end_prepare;
                }
            }
        }

        if !db.p_disconnect.is_empty() {
            crate::vtab::vtab_unlock_list(db);
        }

        if n_bytes >= 0 && (n_bytes == 0 || at(z_sql, (n_bytes - 1) as usize) != 0) {
            let mx_len = db.a_limit[SQLITE_LIMIT_SQL_LENGTH as usize];
            if n_bytes > mx_len {
                crate::main::error_with_msg(db, SQLITE_TOOBIG, b"statement too long", &[]);
                rc = crate::main::api_exit(db, SQLITE_TOOBIG);
                break 'end_prepare;
            }
            // O C copia `nBytes` bytes para ter o terminador NUL; a fatia já termina onde o
            // texto termina e o parser trata o fim dela como NUL.
            let n = (n_bytes as usize).min(z_sql.len());
            run_parser(db, &mut parse, &z_sql[..n]);
        } else {
            run_parser(db, &mut parse, z_sql);
        }
        debug_assert!(parse.n_query_loop == 0);

        tail = parse.z_tail;

        if db.init.busy == 0 {
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                vdbe_set_sql(v, z_sql, tail as i32, prep_flags as u8);
            }
        }
        if db.malloc_failed != 0 {
            parse.rc = SQLITE_NOMEM_BKPT;
            parse.check_schema = 0;
        }
        if parse.rc != SQLITE_OK && parse.rc != SQLITE_DONE {
            if parse.check_schema != 0 && db.init.busy == 0 {
                schema_is_valid(db, &mut parse);
            }
            if let Some(v) = parse.p_vdbe.take() {
                vdbe_finalize(*v, db);
            }
            rc = parse.rc;
            if let Some(msg) = parse.z_err_msg.take() {
                crate::main::error_with_msg(
                    db,
                    rc,
                    b"%s",
                    &[crate::printf::PrintfArg::Text(Some(msg))],
                );
            } else {
                crate::main::error(db, rc);
            }
        } else {
            debug_assert!(parse.z_err_msg.is_none());
            p_stmt = parse.p_vdbe.take().map(|v| register_stmt(db, *v));
            rc = SQLITE_OK;
            crate::main::error_clear(db);
        }

        // Apaga os `TriggerPrg` alocados na análise deste comando.
        parse.p_trigger_prg = Vec::new();
    }

    parse_object_reset(db, &mut parse);
    (rc, p_stmt, tail)
}

/// `sqlite3LockAndPrepare`: a `prepare` com as repetições. O C ainda pega o mutex da conexão e
/// entra em todos os btrees; sem threads compartilhando a conexão isso some.
pub fn lock_and_prepare(
    db: &mut Connection,
    z_sql: &[u8],
    n_bytes: i32,
    prep_flags: u32,
    p_old: Option<StmtId>,
) -> PrepareResult {
    if !crate::main::safety_check_ok(db) {
        return (SQLITE_MISUSE, None, 0);
    }
    let mut cnt = 0;
    let mut result;
    loop {
        // Várias tentativas de compilar o SQL, até que dê certo ou haja um erro permanente. Um
        // problema de esquema depois de um reset do esquema é considerado erro permanente.
        result = prepare(db, z_sql, n_bytes, prep_flags, p_old);
        let rc = result.0;
        debug_assert!(rc == SQLITE_OK || result.1.is_none());
        if rc == SQLITE_OK || db.malloc_failed != 0 {
            break;
        }
        let retry = (rc == SQLITE_ERROR_RETRY && {
            let c = cnt;
            cnt += 1;
            c < SQLITE_MAX_PREPARE_RETRY
        }) || (rc == SQLITE_SCHEMA && {
            reset_one_schema(db, -1);
            let c = cnt;
            cnt += 1;
            c == 0
        });
        if !retry {
            break;
        }
    }
    result.0 = crate::main::api_exit(db, result.0);
    debug_assert!((result.0 & db.err_mask) == result.0);
    db.busy_handler.n_busy.set(0);
    debug_assert!(result.0 == SQLITE_OK || result.1.is_none());
    result
}

/// `sqlite3Reprepare`: refaz a compilação de um comando depois de uma mudança de esquema.
///
/// Se o comando é recompilado, devolve `SQLITE_OK`. Se não pode ser recompilado porque outra
/// conexão travou a tabela sqlite_schema, devolve `SQLITE_LOCKED`. Qualquer outro erro devolve
/// `SQLITE_SCHEMA`. O comando `p` precisa estar na vaga de `Connection.stmts`.
pub fn reprepare(db: &mut Connection, p: StmtId) -> i32 {
    let (z_sql, prep_flags) = match db.stmt(p) {
        Some(v) => (v.z_sql.clone(), v.prep_flags),
        None => return SQLITE_MISUSE,
    };
    // `sqlite3Reprepare` só é chamada para comandos de `prepare_v2()`, que guardam o SQL.
    debug_assert!(z_sql.is_some());
    let z_sql = z_sql.unwrap_or_default();
    let (rc, p_new, _tail) = lock_and_prepare(db, &z_sql, -1, prep_flags as u32, Some(p));
    if rc != 0 {
        if rc == SQLITE_NOMEM {
            oom_fault(db);
        }
        debug_assert!(p_new.is_none());
        return rc;
    }
    let Some(new_id) = p_new else {
        debug_assert!(false, "prepare sem comando e sem erro");
        return SQLITE_NOMEM;
    };
    let Some(mut new_vdbe) = db.stmts.take(new_id.slot()) else {
        return SQLITE_NOMEM;
    };
    if let Some(old) = db.stmts.get_mut(p.slot()) {
        vdbe_swap(&mut new_vdbe, old);
        crate::vdbeapi::transfer_bindings(&mut new_vdbe, old);
    }
    vdbe_reset_step_result(&mut new_vdbe);
    // `sqlite3VdbeFinalize(pNew)`: tira o comando (que agora tem o programa velho) da conexão.
    db.stmts.remove(new_id.slot());
    db.stmt_list.retain(|s| *s != new_id);
    vdbe_finalize(new_vdbe, db);
    SQLITE_OK
}

/// `sqlite3_prepare`: a versão antiga da API. O SQL original NÃO é guardado no comando, então uma
/// mudança de esquema faz `sqlite3_step()` devolver `SQLITE_SCHEMA`.
pub fn prepare_v1(db: &mut Connection, z_sql: &[u8], n_bytes: i32) -> PrepareResult {
    let r = lock_and_prepare(db, z_sql, n_bytes, 0, None);
    debug_assert!(r.0 == SQLITE_OK || r.1.is_none());
    r
}

/// `sqlite3_prepare_v2`: o SQL original é guardado no comando, que é recompilado sozinho quando
/// o esquema muda. Funciona exatamente como `prepare_v3` com `prepFlags` zero.
pub fn prepare_v2(db: &mut Connection, z_sql: &[u8], n_bytes: i32) -> PrepareResult {
    // EVIDENCE-OF: R-37923-12173 O `sqlite3_prepare_v2()` funciona exatamente como o
    // `sqlite3_prepare_v3()` com `prepFlags` zero.
    let r = lock_and_prepare(db, z_sql, n_bytes, SQLITE_PREPARE_SAVESQL, None);
    debug_assert!(r.0 == SQLITE_OK || r.1.is_none());
    r
}

/// `sqlite3_prepare_v3`: como `prepare_v2`, com as flags `SQLITE_PREPARE_*` de `prep_flags`.
pub fn prepare_v3(db: &mut Connection, z_sql: &[u8], n_bytes: i32, prep_flags: u32) -> PrepareResult {
    // EVIDENCE-OF: R-56861-42673 O `sqlite3_prepare_v3()` difere do `sqlite3_prepare_v2()` só
    // por ter o parâmetro `prepFlags`, um conjunto de bits com zero ou mais flags
    // `SQLITE_PREPARE_*`.
    let r = lock_and_prepare(
        db,
        z_sql,
        n_bytes,
        SQLITE_PREPARE_SAVESQL | (prep_flags & SQLITE_PREPARE_MASK),
        None,
    );
    debug_assert!(r.0 == SQLITE_OK || r.1.is_none());
    r
}

/// `sqlite3Prepare16`: compila o comando SQL UTF-16 `z_sql` (na ordem de bytes nativa) num
/// comando preparado. Converte para UTF-8, chama `lock_and_prepare` e traduz o deslocamento do
/// resto do texto de volta para bytes de UTF-16. `n_bytes` negativo vai até o par de NUL.
fn prepare16(db: &mut Connection, z_sql: &[u8], n_bytes: i32, prep_flags: u32) -> PrepareResult {
    if !crate::main::safety_check_ok(db) {
        return (SQLITE_MISUSE, None, 0);
    }
    let limit = if n_bytes >= 0 { n_bytes as usize } else { usize::MAX };
    let mut sz = 0usize;
    while sz < limit && (at(z_sql, sz) != 0 || at(z_sql, sz + 1) != 0) {
        sz += 2;
    }
    let z_sql8 = translate_bytes(
        &z_sql[..sz.min(z_sql.len())],
        SQLITE_UTF16NATIVE as u8,
        SQLITE_UTF8 as u8,
    );
    let (rc, p_stmt, tail8) = lock_and_prepare(db, &z_sql8, -1, prep_flags, None);

    // Se `sqlite3_prepare` devolve o resto do texto, calcula o deslocamento equivalente na cadeia
    // UTF-16 contando os caracteres Unicode entre o começo e o resto em UTF-8, e avançando o
    // mesmo número de caracteres na UTF-16.
    let chars_parsed = utf8_char_len(&z_sql8, tail8 as i32);
    let tail16 = utf16_byte_len(z_sql, chars_parsed) as usize;
    (crate::main::api_exit(db, rc), p_stmt, tail16)
}

/// `sqlite3_prepare16`: a versão antiga da API para UTF-16.
pub fn prepare16_v1(db: &mut Connection, z_sql: &[u8], n_bytes: i32) -> PrepareResult {
    prepare16(db, z_sql, n_bytes, 0)
}

/// `sqlite3_prepare16_v2`.
pub fn prepare16_v2(db: &mut Connection, z_sql: &[u8], n_bytes: i32) -> PrepareResult {
    prepare16(db, z_sql, n_bytes, SQLITE_PREPARE_SAVESQL)
}

/// `sqlite3_prepare16_v3`.
pub fn prepare16_v3(
    db: &mut Connection,
    z_sql: &[u8],
    n_bytes: i32,
    prep_flags: u32,
) -> PrepareResult {
    prepare16(db, z_sql, n_bytes, SQLITE_PREPARE_SAVESQL | (prep_flags & SQLITE_PREPARE_MASK))
}

// ---------------------------------------------------------------------------------------------
// sqlite3RunParser (tokenize.c)
// ---------------------------------------------------------------------------------------------

/// `getToken` (tokenize.c): o tipo do próximo token de `z` a partir de `*pos`, pulando os
/// espaços, e avança `*pos` para depois dele. Tudo o que pode ser um identificador vira `TK_ID`.
fn get_token_kind(z: &[u8], pos: &mut usize) -> i32 {
    let mut t = 0i32;
    loop {
        *pos += get_token(z.get(*pos..).unwrap_or(&[]), &mut t) as usize;
        if t != TK_SPACE as i32 {
            break;
        }
    }
    if t == TK_ID as i32
        || t == TK_STRING as i32
        || t == TK_JOIN_KW as i32
        || t == TK_WINDOW as i32
        || t == TK_OVER as i32
        || parser_fallback(t) == TK_ID as i32
    {
        t = TK_ID as i32;
    }
    t
}

// As três funções a seguir são chamadas logo depois de o tokenizador ler as palavras WINDOW,
// OVER e FILTER, para decidir se o token é palavra-chave ou identificador SQL. Isso não dá para
// resolver com o `%fallback` do lemon, por causa da ambiguidade de construções como
//
//   SELECT sum(x) OVER ...
//
// onde "OVER" tanto pode ser palavra-chave como o apelido da expressão sum(x). Um
// `%fallback ID OVER` na gramática faria "OVER" valer sempre como apelido, e seria impossível
// chamar uma função de janela sem FILTER.
//
// WINDOW é palavra-chave se o token seguinte é um identificador (ou palavra-chave que pode
// virar identificador) e o seguinte a esse é TK_AS.
//
// OVER é palavra-chave se o token anterior foi TK_RP e o seguinte é TK_LP ou um identificador.
//
// FILTER é palavra-chave se o token anterior foi TK_RP e o seguinte é TK_LP.

/// `analyzeWindowKeyword`: `z[start..]` é o texto depois de WINDOW.
fn analyze_window_keyword(z: &[u8], start: usize) -> i32 {
    let mut pos = start;
    if get_token_kind(z, &mut pos) != TK_ID as i32 {
        return TK_ID as i32;
    }
    if get_token_kind(z, &mut pos) != TK_AS as i32 {
        return TK_ID as i32;
    }
    TK_WINDOW as i32
}

/// `analyzeOverKeyword`: `z[start..]` é o texto depois de OVER.
fn analyze_over_keyword(z: &[u8], start: usize, last_token: i32) -> i32 {
    if last_token == TK_RP as i32 {
        let mut pos = start;
        let t = get_token_kind(z, &mut pos);
        if t == TK_LP as i32 || t == TK_ID as i32 {
            return TK_OVER as i32;
        }
    }
    TK_ID as i32
}

/// `analyzeFilterKeyword`: `z[start..]` é o texto depois de FILTER.
fn analyze_filter_keyword(z: &[u8], start: usize, last_token: i32) -> i32 {
    let mut pos = start;
    if last_token == TK_RP as i32 && get_token_kind(z, &mut pos) == TK_LP as i32 {
        return TK_FILTER as i32;
    }
    TK_ID as i32
}

/// `sqlite3RunParser`: roda o parser sobre o texto SQL `z_sql` (que vai até o NUL ou o fim da
/// fatia). Devolve o número de erros. Ao terminar, `parse.z_tail` é o deslocamento do texto que
/// vem depois do último comando analisado.
pub fn run_parser(db: &mut Connection, parse: &mut Parse, z_sql: &[u8]) -> i32 {
    let mut n_err = 0; // Número de erros encontrados.
    let mut last_token_parsed: i32 = -1; // Tipo do token anterior.
    let mut mx_sql_len = db.a_limit[SQLITE_LIMIT_SQL_LENGTH as usize]; // Tamanho máximo do SQL.

    if db.n_vdbe_active == 0 {
        db.interrupted.store(false, Ordering::SeqCst);
    }
    parse.rc = SQLITE_OK;
    parse.z_tail = 0;
    let mut engine = parser_init(); // O parser LALR(1) gerado pelo lemon.
    debug_assert!(parse.p_new_table.is_none());
    debug_assert!(parse.p_new_trigger.is_none());
    debug_assert!(parse.n_var == 0);
    debug_assert!(parse.p_v_list.is_empty());
    let mut pos = 0usize; // O `zSql` do C: o próximo byte não lido.
    loop {
        let mut token_type = 0i32; // Tipo do próximo token.
        let mut n = get_token(z_sql.get(pos..).unwrap_or(&[]), &mut token_type); // Tamanho dele.
        mx_sql_len -= n;
        if mx_sql_len < 0 {
            parse.rc = SQLITE_TOOBIG;
            parse.n_err += 1;
            break;
        }
        if token_type >= TK_WINDOW as i32 {
            debug_assert!(
                token_type == TK_SPACE as i32
                    || token_type == TK_OVER as i32
                    || token_type == TK_FILTER as i32
                    || token_type == crate::consts::TK_ILLEGAL as i32
                    || token_type == TK_WINDOW as i32
                    || token_type == TK_QNUMBER as i32
            );
            if db.interrupted.load(Ordering::SeqCst) {
                parse.rc = SQLITE_INTERRUPT;
                parse.n_err += 1;
                break;
            }
            if token_type == TK_SPACE as i32 {
                pos += n as usize;
                continue;
            }
            if at(z_sql, pos) == 0 {
                // Ao chegar ao fim do texto, chama o parser mais duas vezes, com os tokens
                // TK_SEMI e 0, nessa ordem.
                if last_token_parsed == TK_SEMI as i32 {
                    token_type = 0;
                } else if last_token_parsed == 0 {
                    break;
                } else {
                    token_type = TK_SEMI as i32;
                }
                n = 0;
            } else if token_type == TK_WINDOW as i32 {
                debug_assert!(n == 6);
                token_type = analyze_window_keyword(z_sql, pos + 6);
            } else if token_type == TK_OVER as i32 {
                debug_assert!(n == 4);
                token_type = analyze_over_keyword(z_sql, pos + 4, last_token_parsed);
            } else if token_type == TK_FILTER as i32 {
                debug_assert!(n == 6);
                token_type = analyze_filter_keyword(z_sql, pos + 6, last_token_parsed);
            } else if token_type != TK_QNUMBER as i32 {
                let x = Token {
                    z: z_sql[pos.min(z_sql.len())..(pos + n as usize).min(z_sql.len())].to_vec(),
                    i_ofst: pos as i32,
                };
                let arg = token_arg(parse, &x);
                error_msg(db, parse, b"unrecognized token: \"%T\"", &[arg]);
                break;
            }
        }
        let start = pos.min(z_sql.len());
        let end = (pos + n as usize).min(z_sql.len());
        parse.s_last_token = Token { z: z_sql[start..end].to_vec(), i_ofst: pos as i32 };
        let last = parse.s_last_token.clone();
        parser(&mut engine, token_type, last, parse, db);
        last_token_parsed = token_type;
        pos += n as usize;
        debug_assert!(db.malloc_failed == 0 || parse.rc != SQLITE_OK);
        if parse.rc != SQLITE_OK {
            break;
        }
    }
    parser_finalize(&mut engine);
    drop(engine);
    if db.malloc_failed != 0 {
        parse.rc = SQLITE_NOMEM_BKPT;
    }
    if parse.z_err_msg.is_some() || (parse.rc != SQLITE_OK && parse.rc != SQLITE_DONE) {
        if parse.z_err_msg.is_none() {
            parse.z_err_msg = crate::printf::mprintf(
                b"%s",
                &[text_arg(err_str(parse.rc).as_bytes())],
            );
        }
        log(
            parse.rc,
            b"%s in \"%s\"",
            &[
                crate::printf::PrintfArg::Text(parse.z_err_msg.clone()),
                text_arg(z_sql.get(parse.z_tail..).unwrap_or(&[])),
            ],
        );
        n_err += 1;
    }
    parse.z_tail = pos;
    parse.ap_vtab_lock = Vec::new();

    if !parse.in_special_parse() {
        // Se `declareVtab` está ligado, não apaga a tabela montada em `p_new_table`: quem chamou
        // (vtab.c) cuida de liberar a `Table`.
        if let Some(t) = parse.p_new_table.take() {
            delete_table(db, Some(Rc::new(*t)));
        }
    }
    if parse.p_new_trigger.is_some() && !parse.in_rename_object() {
        parse.p_new_trigger = None;
    }
    parse.p_v_list = Vec::new();
    debug_assert!(n_err == 0 || parse.rc != SQLITE_OK);
    n_err
}
