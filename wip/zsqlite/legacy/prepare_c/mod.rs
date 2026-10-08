// Mesclado das partes traduzidas de prepare_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Preenche a estrutura InitData com uma mensagem de erro que indica que o banco
/// está corrompido.
fn corrupt_schema(p_data: &mut InitData, az_obj: &[Option<Vec<u8>>], z_extra: Option<&[u8]>) {
    let db = p_data.db.clone();
    let malloc_failed = db.borrow().malloc_failed != 0;
    if malloc_failed {
        p_data.rc = SQLITE_NOMEM_BKPT;
    } else if p_data.pz_err_msg.is_some() {
        // Uma mensagem de erro já foi gerada. Não sobrescrever.
    } else if (p_data.m_init_flags & INITFLAG_ALTERMASK) != 0 {
        const AZ_ALTER_TYPE: [&[u8]; 3] = [b"rename", b"drop column", b"add column"];
        let alter_idx = ((p_data.m_init_flags & INITFLAG_ALTERMASK) - 1) as usize;
        // %s com ponteiro nulo imprime texto vazio no printf do SQLite.
        let z_obj0: &[u8] = az_obj.get(0).and_then(|x| x.as_deref()).unwrap_or(&b""[..]);
        let z_obj1: &[u8] = az_obj.get(1).and_then(|x| x.as_deref()).unwrap_or(&b""[..]);
        p_data.pz_err_msg = mprintf(
            &db,
            b"error in %s %s after %s: %s",
            &[
                PrintfArg::S(z_obj0),
                PrintfArg::S(z_obj1),
                PrintfArg::S(AZ_ALTER_TYPE[alter_idx]),
                PrintfArg::S(z_extra.unwrap_or(&b""[..])),
            ],
        );
        p_data.rc = SQLITE_ERROR;
    } else if (db.borrow().flags & SQLITE_WRITESCHEMA) != 0 {
        p_data.rc = corrupt_error(line!() as i32);
    } else {
        let z_obj: &[u8] = match az_obj.get(1).and_then(|x| x.as_deref()) {
            Some(o) => o,
            None => &b"?"[..],
        };
        let mut z = mprintf(&db, b"malformed database schema (%s)", &[PrintfArg::S(z_obj)]);
        if let Some(extra) = z_extra {
            if extra.first().copied().unwrap_or(0) != 0 {
                z = mprintf(&db, b"%z - %s", &[PrintfArg::Z(z), PrintfArg::S(extra)]);
            }
        }
        p_data.pz_err_msg = z;
        p_data.rc = corrupt_error(line!() as i32);
    }
}

/// Verifica se algum índice irmão (outro índice na mesma tabela) de pIndex tem o
/// mesmo número de página raiz e, se tiver, retorna verdadeiro. Isso indicaria um
/// schema corrompido.
pub fn index_has_duplicate_root_page(p_index: &IndexRef) -> bool {
    let (p_table, tnum) = {
        let idx = p_index.borrow();
        (idx.p_table.upgrade(), idx.tnum)
    };
    let mut p: Option<IndexRef> = match p_table {
        Some(t) => t.borrow().p_index.clone(),
        None => None,
    };
    while let Some(p_rc) = p {
        let (p_tnum, p_next) = {
            let n = p_rc.borrow();
            (n.tnum, n.p_next.clone())
        };
        if p_tnum == tnum && !Rc::ptr_eq(&p_rc, p_index) {
            return true;
        }
        p = p_next;
    }
    false
}

// A declaração antecipada de sqlite3Prepare() não existe em Rust: a função
// `prepare` está definida em part_001.

/// Esta é a rotina de callback para o código que inicializa o banco de dados.
/// Ver `init` para informação adicional. Esta rotina também é chamada do opcode
/// OP_ParseSchema do VDBE.
///
/// Cada callback contém as seguintes informações (argc é sempre 5 e o quarto
/// parâmetro do C, NotUsed, não existe aqui; `argv` nulo vira `None`):
///
///     argv[0] = tipo de objeto: "table", "index", "trigger", ou "view".
///     argv[1] = nome da coisa sendo criada
///     argv[2] = tabela associada se um índice ou trigger
///     argv[3] = número de página raiz para tabela ou índice. 0 para trigger ou view.
///     argv[4] = texto SQL para a instrução CREATE.
pub fn init_callback(p_init: &mut InitData, argv: Option<&[Option<Vec<u8>>]>) -> i32 {
    let db = p_init.db.clone();
    let i_db = p_init.i_db;

    db.borrow_mut().m_db_flags |= DBFLAG_ENCODINGFIXED;
    let argv = match argv {
        Some(a) => a,
        None => return 0, // Pode acontecer se EMPTY_RESULT_CALLBACKS estiverem ativados
    };
    debug_assert!(argv.len() == 5);
    p_init.n_init_row += 1;
    if db.borrow().malloc_failed != 0 {
        corrupt_schema(p_init, argv, None);
        return 1;
    }

    debug_assert!(i_db >= 0 && i_db < db.borrow().n_db);
    let z_sql: Option<&[u8]> = argv[4].as_deref();
    if argv[3].is_none() {
        corrupt_schema(p_init, argv, None);
    } else if z_sql.is_some()
        && b'c' == UPPER_TO_LOWER[z_sql.unwrap().first().copied().unwrap_or(0) as usize]
        && b'r' == UPPER_TO_LOWER[z_sql.unwrap().get(1).copied().unwrap_or(0) as usize]
    {
        // Chama o parser para processar um CREATE TABLE, INDEX ou VIEW.
        // Mas como db->init.busy vale 1, nenhum código VDBE é gerado nem
        // executado. Tudo que o parser faz é construir as estruturas de dados
        // internas que descrevem a tabela, o índice ou a view.
        //
        // Nenhuma outra instrução SQL válida, além das variantes de CREATE, pode
        // começar com as letras "C" e "R". Assim, não é possível rodar qualquer
        // outro tipo de instrução durante a leitura do schema, mesmo um schema
        // corrompido.
        let z_sql = z_sql.unwrap();
        let z_root = argv[3].as_deref().unwrap();
        let saved_i_db = db.borrow().init.i_db;

        db.borrow_mut().init.i_db = i_db as u8;
        let mut new_tnum = db.borrow().init.new_tnum;
        let got = get_u_int32(z_root, &mut new_tnum);
        db.borrow_mut().init.new_tnum = new_tnum;
        if got == 0 || (new_tnum > p_init.mx_page && p_init.mx_page > 0) {
            if config().b_extra_schema_checks != 0 {
                corrupt_schema(p_init, argv, Some(&b"invalid rootpage"[..]));
            }
        }
        db.borrow_mut().init.orphan_trigger = 0;
        db.borrow_mut().init.az_init = argv.to_vec();
        let mut p_stmt: Option<VdbeRef> = None;
        prepare(&db, z_sql, -1, 0, None, &mut p_stmt, None);
        let rc = db.borrow().err_code;
        db.borrow_mut().init.i_db = saved_i_db;
        if SQLITE_OK != rc {
            if db.borrow().init.orphan_trigger != 0 {
                debug_assert!(i_db == 1);
            } else {
                if rc > p_init.rc {
                    p_init.rc = rc;
                }
                if rc == SQLITE_NOMEM {
                    oom_fault(&db);
                } else if rc != SQLITE_INTERRUPT && (rc & 0xFF) != SQLITE_LOCKED {
                    let z_msg = api::errmsg(&db);
                    corrupt_schema(p_init, argv, Some(&z_msg[..]));
                }
            }
        }
        // Qualquer vetor de strings serve aqui.
        db.borrow_mut().init.az_init = STD_TYPE.iter().map(|s| Some(s.to_vec())).collect();
        api::finalize(p_stmt);
    } else if argv[1].is_none()
        || (z_sql.is_some() && z_sql.unwrap().first().copied().unwrap_or(0) != 0)
    {
        corrupt_schema(p_init, argv, None);
    } else {
        // Se a coluna SQL está em branco, este é um índice criado para ser a
        // PRIMARY KEY ou para cumprir uma restrição UNIQUE de um CREATE TABLE.
        // O índice já deveria ter sido criado quando processamos o CREATE TABLE.
        // Tudo que temos a fazer aqui é registrar o número de página raiz desse
        // índice.
        let z_db_sname = db.borrow().a_db[i_db as usize].z_db_sname.clone();
        let p_index = find_index(&db, argv[1].as_deref().unwrap(), &z_db_sname[..]);
        match p_index {
            None => corrupt_schema(p_init, argv, Some(&b"orphan index"[..])),
            Some(p_index) => {
                let mut tnum = p_index.borrow().tnum;
                let got = get_u_int32(argv[3].as_deref().unwrap(), &mut tnum);
                p_index.borrow_mut().tnum = tnum;
                if got == 0
                    || tnum < 2
                    || tnum > p_init.mx_page
                    || index_has_duplicate_root_page(&p_index)
                {
                    if config().b_extra_schema_checks != 0 {
                        corrupt_schema(p_init, argv, Some(&b"invalid rootpage"[..]));
                    }
                }
            }
        }
    }
    0
}

/// Tenta ler o schema do banco de dados e inicializar as estruturas de dados
/// internas para um único arquivo de banco de dados. O índice do arquivo é dado
/// por iDb. iDb==0 é usado para o banco principal. iDb==1 nunca deveria ser
/// usado. iDb>=2 é usado para bancos auxiliares. Retorna um dos códigos de erro
/// SQLITE_ para indicar sucesso ou falha.
///
/// A mensagem de erro do C (`*pzErrMsg`) fica em `pz_err_msg`; durante a
/// execução ela mora dentro do `InitData`, que é o alias que o C usa.
pub fn init_one(
    db: &Sqlite3Ref,
    i_db: i32,
    pz_err_msg: &mut Option<Vec<u8>>,
    m_flags: u32,
) -> i32 {
    let mut rc: i32 = SQLITE_OK;
    let mut opened_transaction = false;
    let mask: u32 = (db.borrow().m_db_flags & DBFLAG_ENCODINGFIXED) | !DBFLAG_ENCODINGFIXED;

    db.borrow_mut().init.busy = 1;

    // Constrói a representação em memória das tabelas de schema (sqlite_schema
    // ou sqlite_temp_schema) invocando o parser diretamente. O nome apropriado da
    // tabela será inserido automaticamente pelo parser, então podemos usar a
    // abreviação "x" aqui. O parser também marca automaticamente a tabela de
    // schema como somente leitura.
    let z_schema_tab_name: &[u8] = schema_table(i_db);
    let az_arg: Vec<Option<Vec<u8>>> = vec![
        Some(b"table".to_vec()),
        Some(z_schema_tab_name.to_vec()),
        Some(z_schema_tab_name.to_vec()),
        Some(b"1".to_vec()),
        Some(
            b"CREATE TABLE x(type text,name text,tbl_name text,rootpage int,sql text)".to_vec(),
        ),
        None,
    ];
    let mut init_data = InitData {
        db: db.clone(),
        i_db,
        rc: SQLITE_OK,
        pz_err_msg: pz_err_msg.take(),
        m_init_flags: m_flags,
        n_init_row: 0,
        mx_page: 0,
    };
    init_callback(&mut init_data, Some(&az_arg[..5]));
    db.borrow_mut().m_db_flags &= mask;

    'error_out: {
        if init_data.rc != 0 {
            rc = init_data.rc;
            break 'error_out;
        }

        // Cria um cursor para manter o banco aberto.
        let p_bt_opt = db.borrow().a_db[i_db as usize].p_bt.clone();
        let p_bt = match p_bt_opt {
            Some(b) => b,
            None => {
                debug_assert!(i_db == 1);
                db_set_property(&mut db.borrow_mut(), 1, DB_SCHEMALOADED);
                rc = SQLITE_OK;
                break 'error_out;
            }
        };

        // Se ainda não há transação somente leitura (ou de leitura e escrita)
        // aberta no b-tree, abre uma agora. Se uma transação for aberta, ela
        // será fechada antes de esta função retornar.
        btree_enter(&p_bt);
        'initone_error_out: {
            if btree_txn_state(&p_bt) == SQLITE_TXN_NONE {
                rc = btree_begin_trans(&p_bt, 0, None);
                if rc != SQLITE_OK {
                    set_string(&mut init_data.pz_err_msg, db, err_str(rc));
                    break 'initone_error_out;
                }
                opened_transaction = true;
            }

            // Obtém a meta informação do banco.
            //
            // Os valores meta são os seguintes:
            //    meta[0]   Schema cookie. Muda a cada mudança de schema.
            //    meta[1]   Formato de arquivo da camada de schema.
            //    meta[2]   Tamanho do cache de páginas.
            //    meta[3]   Maior rootpage (modo auto/incr_vacuum)
            //    meta[4]   Codificação de texto do banco. 1:UTF-8 2:UTF-16LE 3:UTF-16BE
            //    meta[5]   Versão do usuário
            //    meta[6]   Modo de vacuum incremental
            //    meta[7]   não usado
            //    meta[8]   não usado
            //    meta[9]   não usado
            //
            // Nota: os símbolos SQLITE_UTF* de sqliteInt.h correspondem aos
            // valores possíveis de meta[4].
            let mut meta = [0i32; 5];
            for i in 0..meta.len() {
                let mut v: u32 = 0;
                btree_get_meta(&p_bt, (i + 1) as i32, &mut v);
                meta[i] = v as i32;
            }
            if (db.borrow().flags & SQLITE_RESETDATABASE) != 0 {
                meta = [0i32; 5];
            }
            let p_schema: SchemaRef = db.borrow().a_db[i_db as usize]
                .p_schema
                .clone()
                .expect("schema ausente");
            p_schema.borrow_mut().schema_cookie = meta[BTREE_SCHEMA_VERSION as usize - 1];

            // Se abrindo um banco não vazio, verifica a codificação de texto.
            // Para o banco principal, define sqlite3.enc como a codificação do
            // banco principal. Para um banco anexado, é erro se a codificação
            // não for a mesma de sqlite3.enc.
            if meta[BTREE_TEXT_ENCODING as usize - 1] != 0 {
                let fixed = (db.borrow().m_db_flags & DBFLAG_ENCODINGFIXED) != 0;
                if i_db == 0 && !fixed {
                    // Se abrindo o banco principal, define ENC(db).
                    let mut encoding: u8 = (meta[BTREE_TEXT_ENCODING as usize - 1] as u8) & 3;
                    if encoding == 0 {
                        encoding = SQLITE_UTF8 as u8;
                    }
                    let (n_vdbe_active, cur_enc, m_db_flags) = {
                        let d = db.borrow();
                        (d.n_vdbe_active, enc(&d), d.m_db_flags)
                    };
                    if n_vdbe_active > 0 && encoding != cur_enc && (m_db_flags & DBFLAG_VACUUM) == 0
                    {
                        rc = SQLITE_LOCKED;
                        break 'initone_error_out;
                    } else {
                        set_text_encoding(db, encoding);
                    }
                } else {
                    // Se abrindo um banco anexado, a codificação precisa casar com ENC(db).
                    let cur_enc = enc(&db.borrow());
                    if ((meta[BTREE_TEXT_ENCODING as usize - 1] & 3) as u8) != cur_enc {
                        set_string(
                            &mut init_data.pz_err_msg,
                            db,
                            &b"attached databases must use the same text encoding as main database"[..],
                        );
                        rc = SQLITE_ERROR;
                        break 'initone_error_out;
                    }
                }
            }
            let cur_enc = enc(&db.borrow());
            p_schema.borrow_mut().enc = cur_enc;

            if p_schema.borrow().cache_size == 0 {
                let mut size = abs_int32(meta[BTREE_DEFAULT_CACHE_SIZE as usize - 1]);
                if size == 0 {
                    size = SQLITE_DEFAULT_CACHE_SIZE;
                }
                p_schema.borrow_mut().cache_size = size;
                btree_set_cache_size(&p_bt, size);
            }

            // file_format==1    Versão 3.0.0.
            // file_format==2    Versão 3.1.3.  // ALTER TABLE ADD COLUMN
            // file_format==3    Versão 3.1.4.  // idem, mas com padrões não nulos
            // file_format==4    Versão 3.3.0.  // Índices DESC. Constantes booleanas
            p_schema.borrow_mut().file_format = meta[BTREE_FILE_FORMAT as usize - 1] as u8;
            if p_schema.borrow().file_format == 0 {
                p_schema.borrow_mut().file_format = 1;
            }
            if p_schema.borrow().file_format > SQLITE_MAX_FILE_FORMAT as u8 {
                set_string(&mut init_data.pz_err_msg, db, &b"unsupported file format"[..]);
                rc = SQLITE_ERROR;
                break 'initone_error_out;
            }

            // Ticket #2804: quando abrimos um banco no formato de arquivo mais
            // novo, limpa a flag do pragma legacy_file_format para que um VACUUM
            // não rebaixe o banco e invalide índices descendentes que o usuário
            // possa ter criado.
            if i_db == 0 && meta[BTREE_FILE_FORMAT as usize - 1] >= 4 {
                db.borrow_mut().flags &= !(SQLITE_LEGACYFILEFMT as u64);
            }

            // Lê a informação de schema das tabelas de schema.
            debug_assert!(db.borrow().init.busy != 0);
            init_data.mx_page = btree_last_page(&p_bt);
            {
                let z_db_sname = db.borrow().a_db[i_db as usize].z_db_sname.clone();
                let z_sql = mprintf(
                    db,
                    b"SELECT*FROM\"%w\".%s ORDER BY rowid",
                    &[PrintfArg::W(&z_db_sname[..]), PrintfArg::S(z_schema_tab_name)],
                );
                let x_auth = db.borrow_mut().x_auth.take();
                rc = api::exec(
                    db,
                    z_sql.as_deref().unwrap_or(&b""[..]),
                    &mut |argv, _col_names| init_callback(&mut init_data, argv),
                    None,
                );
                db.borrow_mut().x_auth = x_auth;
                if rc == SQLITE_OK {
                    rc = init_data.rc;
                }
                if rc == SQLITE_OK {
                    analysis_load(db, i_db);
                }
            }
            let malloc_failed = db.borrow().malloc_failed != 0;
            if malloc_failed {
                rc = SQLITE_NOMEM_BKPT;
                reset_all_schemas_of_connection(db);
            } else {
                let no_schema_error = (db.borrow().flags & SQLITE_NOSCHEMAERROR) != 0;
                if rc == SQLITE_OK || (no_schema_error && rc != SQLITE_NOMEM) {
                    // Hack: se a flag SQLITE_NoSchemaError está definida, considera o
                    // schema carregado, mesmo que tenham ocorrido erros (exceto OOM).
                    // Nessa situação a operação sqlite3_prepare() atual falha, mas a
                    // seguinte tenta compilar a instrução fornecida contra o
                    // subconjunto do schema que foi carregado antes do erro.
                    //
                    // O objetivo principal é permitir acesso à tabela sqlite_schema
                    // mesmo quando o conteúdo dela foi corrompido.
                    db_set_property(&mut db.borrow_mut(), i_db as usize, DB_SCHEMALOADED);
                    rc = SQLITE_OK;
                }
            }
        }

        // Salto para erro que ocorre depois de alocar o cursor e chamar
        // btree_enter(). Para erro anterior a esse ponto, o salto é para
        // error_out.
        if opened_transaction {
            btree_commit(&p_bt);
        }
        btree_leave(&p_bt);
    }

    if rc != 0 {
        if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
            oom_fault(db);
        }
        reset_one_schema(db, i_db);
    }
    db.borrow_mut().init.busy = 0;
    *pz_err_msg = init_data.pz_err_msg.take();
    rc
}


// ---- part_001.rs ----

/// Inicializa todos os arquivos de banco de dados: o arquivo principal, o arquivo
/// usado para armazenar tabelas temporárias e qualquer arquivo de banco adicional
/// criado por instruções ATTACH. Retorna um código de sucesso. Se ocorrer um
/// erro, escreve uma mensagem de erro em `pz_err_msg`.
///
/// Depois que um banco é inicializado, o bit DB_SchemaLoaded é ligado no campo
/// flags da estrutura Db.
pub fn init(db: &Sqlite3Ref, pz_err_msg: &mut Option<Vec<u8>>) -> i32 {
    let mut rc: i32;
    let commit_internal = (db.borrow().m_db_flags & DBFLAG_SCHEMACHANGE) == 0;

    debug_assert!(db.borrow().init.busy == 0);
    let schema_enc_value = schema_enc(&db.borrow());
    db.borrow_mut().enc = schema_enc_value;
    debug_assert!(db.borrow().n_db > 0);
    // Faz o schema principal primeiro.
    let main_loaded = db_has_property(&db.borrow(), 0, DB_SCHEMALOADED);
    if !main_loaded {
        rc = init_one(db, 0, pz_err_msg, 0);
        if rc != 0 {
            return rc;
        }
    }
    // Todos os outros schemas depois do principal. O schema "temp" deve ser o último.
    let mut i = db.borrow().n_db - 1;
    while i > 0 {
        let loaded = db_has_property(&db.borrow(), i as usize, DB_SCHEMALOADED);
        if !loaded {
            rc = init_one(db, i, pz_err_msg, 0);
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

/// Esta rotina não faz nada se o schema do banco já está inicializado. Caso
/// contrário, o schema é carregado. Um código de erro é devolvido.
pub fn read_schema(p_parse: &mut Parse) -> i32 {
    let mut rc = SQLITE_OK;
    let db = p_parse.db.clone();
    let busy = db.borrow().init.busy;
    if busy == 0 {
        rc = init(&db, &mut p_parse.z_err_msg);
        if rc != SQLITE_OK {
            p_parse.rc = rc;
            p_parse.n_err += 1;
        } else if db.borrow().no_shared_cache != 0 {
            db.borrow_mut().m_db_flags |= DBFLAG_SCHEMAKNOWNOK;
        }
    }
    rc
}

/// Verifica os cookies de schema em todos os bancos. Se algum cookie estiver
/// desatualizado, define pParse->rc como SQLITE_SCHEMA. Se todos estiverem em
/// dia, não altera pParse->rc.
fn schema_is_valid(p_parse: &mut Parse) {
    let db = p_parse.db.clone();

    debug_assert!(p_parse.check_schema != 0);
    let mut i_db: i32 = 0;
    while i_db < db.borrow().n_db {
        let cur = i_db;
        i_db += 1;
        let mut opened_transaction = false; // Verdadeiro se uma transação foi aberta
        // Banco b-tree de onde ler o cookie.
        let p_bt = match db.borrow().a_db[cur as usize].p_bt.clone() {
            Some(b) => b,
            None => continue,
        };

        // Se ainda não há transação somente leitura (ou de leitura e escrita)
        // aberta no b-tree, abre uma agora. Se uma transação for aberta, ela
        // será fechada logo depois de ler o meta valor.
        if btree_txn_state(&p_bt) == SQLITE_TXN_NONE {
            let rc = btree_begin_trans(&p_bt, 0, None);
            if rc == SQLITE_NOMEM || rc == SQLITE_IOERR_NOMEM {
                oom_fault(&db);
                p_parse.rc = SQLITE_NOMEM;
            }
            if rc != SQLITE_OK {
                return;
            }
            opened_transaction = true;
        }

        // Lê o cookie de schema do banco. Se não casar com o valor guardado na
        // representação de schema em memória, define Parse.rc como SQLITE_SCHEMA.
        let mut cookie: u32 = 0;
        btree_get_meta(&p_bt, BTREE_SCHEMA_VERSION, &mut cookie);
        let schema_cookie = db.borrow().a_db[cur as usize]
            .p_schema
            .as_ref()
            .expect("schema ausente")
            .borrow()
            .schema_cookie;
        if (cookie as i32) != schema_cookie {
            let loaded = db_has_property(&db.borrow(), cur as usize, DB_SCHEMALOADED);
            if loaded {
                p_parse.rc = SQLITE_SCHEMA;
            }
            reset_one_schema(&db, cur);
        }

        // Fecha a transação, se uma foi aberta.
        if opened_transaction {
            btree_commit(&p_bt);
        }
    }
}

/// Converte um schema no índice iDb que indica a qual arquivo de banco em
/// db->aDb[] o schema se refere.
///
/// Se o mesmo banco é anexado mais de uma vez, o primeiro anexado é devolvido.
pub fn schema_to_index(db: &Sqlite3, p_schema: Option<&SchemaRef>) -> i32 {
    let mut i: i32 = -32768;

    // Se pSchema é nulo, devolve -32768. Isso acontece quando código de expr.c
    // tenta resolver uma referência a uma tabela transitória (isto é, uma criada
    // por uma subconsulta). Nesse caso o valor de retorno desta função nunca
    // deve ser usado.
    //
    // Devolvemos -32768 em vez do mais usual -1 simplesmente porque usar -32768
    // como índice incorreto em db->aDb[] tem muito mais chance de causar uma
    // falha de segmentação do que -1 (claro que há assert() também, mas nunca
    // custa jogar com as probabilidades) e -32768 ainda cabe em um inteiro de 16
    // bits com sinal.
    if let Some(schema) = p_schema {
        i = 0;
        loop {
            debug_assert!(i < db.n_db);
            if let Some(Some(s)) = db.a_db.get(i as usize).map(|d| d.p_schema.as_ref()) {
                if Rc::ptr_eq(s, schema) {
                    break;
                }
            }
            i += 1;
        }
        debug_assert!(i >= 0 && i < db.n_db);
    }
    i
}

/// Libera toda a memória alocada no objeto pParse.
///
/// O encadeamento db->pParse (pParse->pOuterParse) não é modelado: o objeto
/// Parse vive na pilha de Rust e é passado por `&mut Parse` a quem precisa dele.
pub fn parse_object_reset(p_parse: &mut Parse) {
    let db = p_parse.db.clone();
    debug_assert!(p_parse.nested == 0);
    p_parse.a_table_lock = Vec::new();
    while let Some(p_cleanup) = p_parse.p_cleanup.take() {
        let mut p_cleanup = *p_cleanup;
        p_parse.p_cleanup = p_cleanup.p_next.take();
        (p_cleanup.x_cleanup)(&db);
    }
    p_parse.a_label = Vec::new();
    if p_parse.p_const_expr.is_some() {
        expr_list_delete(&db, p_parse.p_const_expr.take());
    }
    let mut d = db.borrow_mut();
    debug_assert!(d.lookaside.b_disable >= p_parse.disable_lookaside as u32);
    d.lookaside.b_disable -= p_parse.disable_lookaside as u32;
    d.lookaside.sz = if d.lookaside.b_disable != 0 { 0 } else { d.lookaside.sz_true };
}

/// Adiciona uma nova operação de limpeza a um Parser. A limpeza deve acontecer
/// quando o objeto parser for destruído. Mas cuidado: a limpeza pode acontecer
/// imediatamente.
///
/// Use este mecanismo para limpezas incomuns. Ele tem custo de preparação maior
/// (um malloc extra), então não deve ser usado para limpezas comuns que
/// acontecem na maioria das chamadas. Mas, para limpezas menos comuns,
/// economizamos uma comparação de ponteiro nulo em parse_object_reset(), o que
/// reduz o total de ciclos de CPU.
///
/// Se ocorrer erro de alocação, a limpeza acontece imediatamente.
///
/// O ponteiro `pPtr` do C vai capturado dentro do fecho `x_cleanup`. A função
/// devolve `true` quando a limpeza foi registrada e `false` quando ela rodou na
/// hora (o caso em que o C devolve NULL no lugar de pPtr).
pub fn parser_add_cleanup(p_parse: &mut Parse, x_cleanup: Box<dyn FnOnce(&Sqlite3Ref)>) -> bool {
    let db = p_parse.db.clone();
    let alloc_ok = if fault_sim(300) != 0 {
        oom_fault(&db);
        false
    } else {
        db_malloc_raw(&db, std::mem::size_of::<ParseCleanup>())
    };
    if alloc_ok {
        let p_cleanup = Box::new(ParseCleanup {
            p_next: p_parse.p_cleanup.take(),
            x_cleanup,
        });
        p_parse.p_cleanup = Some(p_cleanup);
        true
    } else {
        x_cleanup(&db);
        false
    }
}

/// Transforma memória bruta em um objeto Parse válido ligado à conexão de banco
/// db. Em Rust a "memória bruta" é um `Parse` novo: o cabeçalho e a cauda saem
/// zerados.
///
/// Chame parse_object_reset() para desfazer esta operação.
///
/// Cuidado: não confunda esta rotina com sqlite3ParseObjectInit() que é gerada
/// pelo Lemon.
pub fn parse_object_init(db: &Sqlite3Ref) -> Parse {
    let mut p_parse = Parse::new(db);
    if db.borrow().malloc_failed != 0 {
        error_msg(&mut p_parse, &b"out of memory"[..]);
    }
    p_parse
}

/// Número máximo de vezes que tentaremos de novo preparar uma instrução que
/// devolve SQLITE_ERROR_RETRY.
pub const SQLITE_MAX_PREPARE_RETRY: i32 = 25;

/// Compila a instrução SQL `z_sql`, codificada em UTF-8, em um identificador de
/// instrução. `z_sql` termina no fim do vetor ou no primeiro byte zero.
/// `pz_tail` recebe o deslocamento do fim do trecho analisado dentro de `z_sql`.
pub(crate) fn prepare(
    db: &Sqlite3Ref,
    z_sql: &[u8],
    n_bytes: i32,
    prep_flags: u32,
    p_reprepare: Option<&VdbeRef>,
    pp_stmt: &mut Option<VdbeRef>,
    pz_tail: Option<&mut Option<usize>>,
) -> i32 {
    let mut rc = SQLITE_OK; // Código de resultado

    // parse_object_init(&sParse, db), expandido no lugar por desempenho.
    let mut s_parse = Parse::new(db);
    if let Some(p_re) = p_reprepare {
        s_parse.p_reprepare = Some(p_re.clone());
        s_parse.explain = api::stmt_isexplain(p_re) as u8;
    } else {
        debug_assert!(s_parse.p_reprepare.is_none());
    }
    debug_assert!(pp_stmt.is_none());

    'end_prepare: {
        if db.borrow().malloc_failed != 0 {
            error_msg(&mut s_parse, &b"out of memory"[..]);
            rc = SQLITE_NOMEM;
            db.borrow_mut().err_code = rc;
            break 'end_prepare;
        }

        // Para uma instrução preparada de longo prazo, evita o uso de memória
        // lookaside.
        if (prep_flags & SQLITE_PREPARE_PERSISTENT) != 0 {
            s_parse.disable_lookaside += 1;
            let mut d = db.borrow_mut();
            d.lookaside.b_disable += 1;
            d.lookaside.sz = 0;
        }
        s_parse.prep_flags = (prep_flags & 0xff) as u8;

        // Verifica se é possível obter um bloqueio de leitura em todos os
        // schemas de banco. A impossibilidade de obter o bloqueio de leitura
        // indica que outra conexão guarda um bloqueio de escrita, o que por sua
        // vez significa que a outra conexão fez mudanças não confirmadas no
        // schema.
        //
        // Se prosseguíssemos e preparássemos a instrução contra as mudanças de
        // schema não confirmadas, e essas mudanças fossem depois revertidas e
        // outras fossem feitas no lugar, então, quando esta instrução preparada
        // fosse executar, o cookie de schema não detectaria a mudança. O desastre
        // seguiria.
        //
        // Esta thread segura os mutexes de todos os Btrees (por causa do
        // btree_enter_all() em lock_and_prepare()), então não é possível outra
        // thread iniciar uma nova mudança de schema enquanto esta rotina roda.
        // Logo, não precisamos segurar bloqueios no schema, só garantir que
        // ninguém mais os esteja segurando.
        //
        // Note que definir READ_UNCOMMITTED sobrepõe a maior parte da detecção
        // de bloqueio, mas *não* sobrepõe a detecção de bloqueio de schema, então
        // tudo isso ainda funciona mesmo com READ_UNCOMMITTED.
        if db.borrow().no_shared_cache == 0 {
            let mut i: i32 = 0;
            while i < db.borrow().n_db {
                let p_bt = db.borrow().a_db[i as usize].p_bt.clone();
                if let Some(p_bt) = p_bt {
                    rc = btree_schema_locked(&p_bt);
                    if rc != 0 {
                        let z_db = db.borrow().a_db[i as usize].z_db_sname.clone();
                        let mut z_msg = b"database schema is locked: ".to_vec();
                        z_msg.extend_from_slice(&z_db[..]);
                        error_with_msg(db, rc, Some(&z_msg[..]));
                        break 'end_prepare;
                    }
                }
                i += 1;
            }
        }

        if db.borrow().p_disconnect.is_some() {
            vtab_unlock_list(db);
        }

        if n_bytes >= 0
            && (n_bytes == 0 || z_sql.get(n_bytes as usize - 1).copied().unwrap_or(0) != 0)
        {
            let mx_len = db.borrow().a_limit[SQLITE_LIMIT_SQL_LENGTH as usize];
            if n_bytes > mx_len {
                error_with_msg(db, SQLITE_TOOBIG, Some(&b"statement too long"[..]));
                rc = api_exit(db, SQLITE_TOOBIG);
                break 'end_prepare;
            }
            let n = (n_bytes as usize).min(z_sql.len());
            match db_str_n_dup(db, &z_sql[..n]) {
                Some(z_sql_copy) => {
                    run_parser(&mut s_parse, &z_sql_copy[..]);
                    // zTail é um deslocamento: vale igual na cópia e em z_sql.
                }
                None => {
                    s_parse.z_tail = n_bytes as usize;
                }
            }
        } else {
            run_parser(&mut s_parse, z_sql);
        }
        debug_assert!(s_parse.n_query_loop == 0);

        if let Some(pz_tail) = pz_tail {
            *pz_tail = Some(s_parse.z_tail);
        }

        if db.borrow().init.busy == 0 {
            vdbe_set_sql(
                s_parse.p_vdbe.as_ref(),
                z_sql,
                s_parse.z_tail as i32,
                prep_flags as u8,
            );
        }
        if db.borrow().malloc_failed != 0 {
            s_parse.rc = SQLITE_NOMEM_BKPT;
            s_parse.check_schema = 0;
        }
        if s_parse.rc != SQLITE_OK && s_parse.rc != SQLITE_DONE {
            if s_parse.check_schema != 0 && db.borrow().init.busy == 0 {
                schema_is_valid(&mut s_parse);
            }
            if let Some(p_vdbe) = s_parse.p_vdbe.take() {
                vdbe_finalize(p_vdbe);
            }
            debug_assert!(pp_stmt.is_none());
            rc = s_parse.rc;
            match s_parse.z_err_msg.take() {
                Some(z_err_msg) => {
                    error_with_msg(db, rc, Some(&z_err_msg[..]));
                }
                None => {
                    error(db, rc);
                }
            }
        } else {
            debug_assert!(s_parse.z_err_msg.is_none());
            *pp_stmt = s_parse.p_vdbe.take();
            rc = SQLITE_OK;
            error_clear(db);
        }

        // Apaga as estruturas TriggerPrg alocadas ao analisar esta instrução.
        while let Some(mut p_t) = s_parse.p_trigger_prg.take() {
            s_parse.p_trigger_prg = p_t.p_next.take();
        }
    }

    parse_object_reset(&mut s_parse);
    rc
}


// ---- part_002.rs ----

/// Compila `z_sql` com bloqueio do banco, repetindo a tentativa até que ela
/// tenha sucesso ou encontre um erro permanente. Um problema de schema depois de
/// uma reinicialização de schema é considerado erro permanente.
///
/// `pz_tail` recebe o deslocamento, em bytes, do fim do trecho analisado dentro
/// de `z_sql` (o `*pzTail` do C).
fn lock_and_prepare(
    db: &Sqlite3Ref,
    z_sql: Option<&[u8]>,
    n_bytes: i32,
    prep_flags: u32,
    p_old: Option<&VdbeRef>,
    pp_stmt: &mut Option<VdbeRef>,
    mut pz_tail: Option<&mut Option<usize>>,
) -> i32 {
    let mut rc: i32;
    let mut cnt: i32 = 0;

    *pp_stmt = None;
    if !safety_check_ok(db) || z_sql.is_none() {
        return misuse_error(line!() as i32);
    }
    let z_sql = z_sql.unwrap();
    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    btree_enter_all(db);
    loop {
        // Faz várias tentativas de compilar o SQL, até que tenha sucesso ou
        // encontre um erro permanente. Um problema de schema depois de uma
        // reinicialização de schema é considerado erro permanente.
        rc = prepare(db, z_sql, n_bytes, prep_flags, p_old, pp_stmt, pz_tail.as_deref_mut());
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none());
        if rc == SQLITE_OK || db.borrow().malloc_failed != 0 {
            break;
        }
        // Condição do while do C: o segundo termo só é avaliado se o primeiro
        // for falso.
        let retry = if rc == SQLITE_ERROR_RETRY && {
            let c = cnt;
            cnt += 1;
            c < SQLITE_MAX_PREPARE_RETRY
        } {
            true
        } else if rc == SQLITE_SCHEMA {
            reset_one_schema(db, -1);
            let c = cnt;
            cnt += 1;
            c == 0
        } else {
            false
        };
        if !retry {
            break;
        }
    }
    btree_leave_all(db);
    rc = api_exit(db, rc);
    debug_assert!((rc & db.borrow().err_mask) == rc);
    db.borrow_mut().busy_handler.n_busy = 0;
    mutex_leave(&mutex);
    debug_assert!(rc == SQLITE_OK || pp_stmt.is_none());
    rc
}

/// Refaz a compilação de uma instrução depois de uma mudança de schema.
///
/// Se a instrução for recompilada com sucesso, devolve SQLITE_OK. Caso
/// contrário, se ela não puder ser recompilada porque outra conexão travou a
/// tabela sqlite3_schema, devolve SQLITE_LOCKED. Qualquer outro erro devolve
/// SQLITE_SCHEMA.
pub fn reprepare(p: &VdbeRef) -> i32 {
    let mut p_new: Option<VdbeRef> = None;

    let z_sql = api::sql(p);
    // reprepare só é chamada para instruções de prepare_v2().
    debug_assert!(z_sql.is_some());
    let db = vdbe_db(p);
    let prep_flags = vdbe_prepare_flags(p);
    let rc = lock_and_prepare(&db, z_sql.as_deref(), -1, prep_flags as u32, Some(p), &mut p_new, None);
    if rc != 0 {
        if rc == SQLITE_NOMEM {
            oom_fault(&db);
        }
        debug_assert!(p_new.is_none());
        return rc;
    } else {
        debug_assert!(p_new.is_some());
    }
    let p_new = p_new.unwrap();
    vdbe_swap(&p_new, p);
    transfer_bindings(&p_new, p);
    vdbe_reset_step_result(&p_new);
    vdbe_finalize(p_new);
    SQLITE_OK
}

/// Duas versões da API oficial, a legada e a nova. Na legada o texto SQL
/// original não fica guardado na instrução preparada, então, se ocorrer uma
/// mudança de schema, sqlite3_step() devolve SQLITE_SCHEMA. Na nova o texto é
/// mantido e a instrução é recompilada automaticamente quando o schema muda.
pub mod api {

    pub fn prepare(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        let rc = lock_and_prepare(db, z_sql, n_bytes, 0, None, pp_stmt, pz_tail);
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none()); // VERIFY: F13021
        rc
    }

    pub fn prepare_v2(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        // EVIDENCE-OF: R-37923-12173 sqlite3_prepare_v2() funciona exatamente
        // como sqlite3_prepare_v3() com prepFlags zero.
        //
        // Prova: o quinto parâmetro de lock_and_prepare é 0.
        let rc = lock_and_prepare(db, z_sql, n_bytes, SQLITE_PREPARE_SAVESQL, None, pp_stmt, pz_tail);
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none());
        rc
    }

    pub fn prepare_v3(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        prep_flags: u32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        // EVIDENCE-OF: R-56861-42673 sqlite3_prepare_v3() difere de
        // sqlite3_prepare_v2() apenas pelo parâmetro extra prepFlags, que é um
        // vetor de bits com zero ou mais das flags SQLITE_PREPARE_*.
        //
        // Prova: comparação com a implementação de prepare_v2() logo acima.
        let rc = lock_and_prepare(
            db,
            z_sql,
            n_bytes,
            SQLITE_PREPARE_SAVESQL | (prep_flags & SQLITE_PREPARE_MASK),
            None,
            pp_stmt,
            pz_tail,
        );
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none());
        rc
    }

    pub fn prepare16(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        let rc = prepare16(db, z_sql, n_bytes, 0, pp_stmt, pz_tail);
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none()); // VERIFY: F13021
        rc
    }

    pub fn prepare16_v2(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        let rc = prepare16(db, z_sql, n_bytes, SQLITE_PREPARE_SAVESQL, pp_stmt, pz_tail);
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none()); // VERIFY: F13021
        rc
    }

    pub fn prepare16_v3(
        db: &Sqlite3Ref,
        z_sql: Option<&[u8]>,
        n_bytes: i32,
        prep_flags: u32,
        pp_stmt: &mut Option<VdbeRef>,
        pz_tail: Option<&mut Option<usize>>,
    ) -> i32 {
        let rc = prepare16(
            db,
            z_sql,
            n_bytes,
            SQLITE_PREPARE_SAVESQL | (prep_flags & SQLITE_PREPARE_MASK),
            pp_stmt,
            pz_tail,
        );
        debug_assert!(rc == SQLITE_OK || pp_stmt.is_none()); // VERIFY: F13021
        rc
    }
}

/// Compila a instrução SQL `z_sql`, codificada em UTF-16, em um identificador de
/// instrução. `pz_tail` recebe o deslocamento, em bytes, dentro de `z_sql`.
fn prepare16(
    db: &Sqlite3Ref,
    z_sql: Option<&[u8]>,
    mut n_bytes: i32,
    prep_flags: u32,
    pp_stmt: &mut Option<VdbeRef>,
    pz_tail: Option<&mut Option<usize>>,
) -> i32 {
    // Esta função transforma primeiro a string em UTF-16 para UTF-8 e depois
    // chama sqlite3_prepare(). A parte difícil é descobrir o ponteiro a devolver
    // em *pzTail.
    let mut z_tail8: Option<usize> = None;
    let mut rc = SQLITE_OK;

    *pp_stmt = None;
    if !safety_check_ok(db) || z_sql.is_none() {
        return misuse_error(line!() as i32);
    }
    let z_sql = z_sql.unwrap();
    if n_bytes >= 0 {
        let mut sz: i32 = 0;
        while sz < n_bytes
            && (z_sql.get(sz as usize).copied().unwrap_or(0) != 0
                || z_sql.get(sz as usize + 1).copied().unwrap_or(0) != 0)
        {
            sz += 2;
        }
        n_bytes = sz;
    }
    let mutex = db.borrow().mutex.clone();
    mutex_enter(&mutex);
    let z_sql8 = utf16to8(db, z_sql, n_bytes, SQLITE_UTF16NATIVE);
    if let Some(z8) = z_sql8.as_deref() {
        rc = lock_and_prepare(db, Some(z8), -1, prep_flags, None, pp_stmt, Some(&mut z_tail8));
    }

    if let (Some(tail8), Some(pz)) = (z_tail8, pz_tail) {
        // Se prepare devolve um ponteiro de fim, calculamos o ponteiro
        // equivalente na string UTF-16 contando os caracteres unicode entre
        // zSql8 e zTail8 e avançando o mesmo número de caracteres na string
        // UTF-16.
        let chars_parsed = utf8_char_len(z_sql8.as_deref().unwrap(), tail8 as i32);
        *pz = Some(utf16_byte_len(z_sql, chars_parsed) as usize);
    }
    // z_sql8 é liberado ao sair de escopo (o db_free do C).
    drop(z_sql8);
    rc = api_exit(db, rc);
    mutex_leave(&mutex);
    rc
}

