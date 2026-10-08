// Mesclado das partes traduzidas de pragma_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Interpreta a sequência de caracteres como um nível de segurança. Retorna 0 para OFF,
/// 1 para ON ou NORMAL, 2 para FULL, e 3 para EXTRA. Retorna 1 para sequência vazia ou
/// não reconhecida. As opções FULL e EXTRA são desabilitadas se o parâmetro omitFull for 1.
///
/// Note que os valores retornados são uma unidade menor que os valores que deveriam ser
/// passados para btree_set_safety_level(). Isto é feito para suportar código SQL legado.
/// O nível de segurança costumava ser booleano e scripts antigos podem ter usado números
/// 0 para OFF e 1 para ON.
pub fn get_safety_level(z: &[u8], omit_full: i32, dflt: u8) -> u8 {
    const Z_TEXT: &[u8] = b"onoffalseyestruextrafull";
    const I_OFFSET: &[u8] = &[0, 1, 2, 4, 9, 12, 15, 20];
    const I_LENGTH: &[u8] = &[2, 2, 3, 5, 3, 4, 5, 4];
    const I_VALUE: &[u8] = &[1, 0, 0, 0, 1, 1, 3, 2];

    if isdigit(z.first().copied().unwrap_or(0)) {
        return atoi(z) as u8;
    }

    let n = strlen30(Some(z));
    for i in 0..I_LENGTH.len() {
        if I_LENGTH[i] as i32 == n
            && strnicmp(Some(&Z_TEXT[I_OFFSET[i] as usize..]), Some(z), n) == 0
            && (omit_full == 0 || I_VALUE[i] <= 1)
        {
            return I_VALUE[i];
        }
    }

    dflt
}

/// Interpreta a sequência de caracteres como um valor booleano.
pub fn get_boolean(z: &[u8], dflt: u8) -> u8 {
    if get_safety_level(z, 1, dflt) != 0 { 1 } else { 0 }
}

/// Valor padrão do limite de análise usado por PRAGMA optimize (bit 0x10): o menor entre
/// este valor e o analysis_limit real, se este for diferente de zero.
pub const SQLITE_DEFAULT_OPTIMIZE_LIMIT: i32 = 2000;

/// Interpreta a sequência de caracteres como um valor de modo de bloqueio.
pub fn get_locking_mode(z: Option<&[u8]>) -> i32 {
    if let Some(z_str) = z {
        if str_i_cmp(z_str, b"exclusive") == 0 {
            return PAGER_LOCKINGMODE_EXCLUSIVE;
        }
        if str_i_cmp(z_str, b"normal") == 0 {
            return PAGER_LOCKINGMODE_NORMAL;
        }
    }
    PAGER_LOCKINGMODE_QUERY
}

/// Interpreta a sequência de caracteres como um valor de modo de auto-vacuum.
///
/// As sequências "none", "full" e "incremental" são aceitas, assim como seus
/// equivalentes numéricos: 0, 1 e 2, respectivamente.
pub fn get_auto_vacuum(z: &[u8]) -> i32 {
    if str_i_cmp(z, b"none") == 0 {
        return BTREE_AUTOVACUUM_NONE as i32;
    }
    if str_i_cmp(z, b"full") == 0 {
        return BTREE_AUTOVACUUM_FULL as i32;
    }
    if str_i_cmp(z, b"incremental") == 0 {
        return BTREE_AUTOVACUUM_INCR as i32;
    }
    let i = atoi(z);
    // O C faz `(u8)` do resultado do operador ternário.
    (if i >= 0 && i <= 2 { i } else { 0 }) as u8 as i32
}

/// Interpreta a sequência de caracteres como uma localização de banco de dados temporário.
/// Retorna 1 para bancos de dados temporários suportados por arquivo, 2 para a árvore
/// Rubro-Negra em memória e 0 para usar o padrão de tempo de compilação.
pub fn get_temp_store(z: &[u8]) -> i32 {
    if !z.is_empty() && z[0] >= b'0' && z[0] <= b'2' {
        return (z[0] - b'0') as i32;
    }
    if str_i_cmp(z, b"file") == 0 {
        return 1;
    }
    if str_i_cmp(z, b"memory") == 0 {
        return 2;
    }
    0
}

/// Invalida o armazenamento temporário, quando o armazenamento temporário é alterado
/// do padrão, ou quando "file" e o diretório temp_store foi alterado.
pub fn invalidate_temp_storage(p_parse: &mut Parse) -> i32 {
    let db_ref = p_parse.db.upgrade().expect("Parse sem conexão");
    let p_bt = db_ref.borrow().a_db[1].p_bt.clone();
    if let Some(p_bt) = p_bt {
        if db_ref.borrow().auto_commit == 0 || btree_txn_state(Some(&p_bt)) != SQLITE_TXN_NONE {
            error_msg(
                p_parse,
                b"temporary storage cannot be changed from within a transaction",
                &[],
            );
            return SQLITE_ERROR;
        }
        btree_close(&mut p_bt.borrow_mut());
        let mut db = db_ref.borrow_mut();
        db.a_db[1].p_bt = None;
        reset_all_schemas_of_connection(&mut db);
    }
    SQLITE_OK
}

/// Se o banco de dados TEMP estiver aberto, fecha-o e marca o esquema do banco de dados
/// como precisando ser recarregado. Isto deve ser feito ao usar os pragmas
/// SQLITE_TEMP_STORE ou DEFAULT_TEMP_STORE.
pub fn change_temp_storage(p_parse: &mut Parse, z_storage_type: &[u8]) -> i32 {
    let ts = get_temp_store(z_storage_type);
    let db_ref = p_parse.db.upgrade().expect("Parse sem conexão");
    if db_ref.borrow().temp_store as i32 == ts {
        return SQLITE_OK;
    }
    if invalidate_temp_storage(p_parse) != SQLITE_OK {
        return SQLITE_ERROR;
    }
    db_ref.borrow_mut().temp_store = ts as u8;
    SQLITE_OK
}

/// Define os nomes das colunas de resultado para um pragma.
pub fn set_pragma_result_column_names(v: &mut Vdbe, p_pragma: &PragmaName) {
    let n = p_pragma.n_prag_c_name;
    vdbe_set_num_cols(v, if n == 0 { 1 } else { n as i32 });
    if n == 0 {
        vdbe_set_col_name(v, 0, COLNAME_NAME, Some(p_pragma.z_name.as_bytes()), SQLITE_STATIC);
    } else {
        let mut j = p_pragma.i_prag_c_name as usize;
        for i in 0..(n as usize) {
            vdbe_set_col_name(
                v,
                i as i32,
                COLNAME_NAME,
                Some(PRAG_C_NAME[j].as_bytes()),
                SQLITE_STATIC,
            );
            j += 1;
        }
    }
}

/// Gera código para retornar um valor inteiro único.
pub fn return_single_int(v: &mut Vdbe, value: i64) {
    vdbe_add_op4_dup8(v, OP_INT64 as i32, 0, 1, 0, &value.to_ne_bytes(), P4_INT64);
    vdbe_add_op2(v, OP_RESULT_ROW as i32, 1, 1);
}

/// Gera código para retornar um valor de texto único.
pub fn return_single_text(v: &mut Vdbe, z_value: Option<&[u8]>) {
    if let Some(z_val) = z_value {
        vdbe_load_string(v, 1, z_val);
        vdbe_add_op2(v, OP_RESULT_ROW as i32, 1, 1);
    }
}

/// Define o nível de segurança e as bandeiras de pager para o pager iDb.
/// Ou se iDb < 0, define esses valores para todos os pagers.
pub fn set_all_pager_flags(db: &mut Sqlite3) {
    if db.auto_commit != 0 {
        let mut n = db.n_db;
        let mut i = 0usize;
        let pager_flags_mask = (PAGER_FULLFSYNC | PAGER_CKPT_FULLFSYNC | PAGER_CACHESPILL) as u64;
        while n > 0 {
            n -= 1;
            if let Some(p_bt) = db.a_db[i].p_bt.clone() {
                btree_set_pager_flags(
                    &mut p_bt.borrow_mut(),
                    db.a_db[i].safety_level as u32 | (db.flags & pager_flags_mask) as u32,
                );
            }
            i += 1;
        }
    }
}

/// Retorna um nome legível para uma ação de resolução de restrição.
pub fn action_name(action: u8) -> &'static str {
    match action {
        OE_SET_NULL => "SET NULL",
        OE_SET_DFLT => "SET DEFAULT",
        OE_CASCADE => "CASCADE",
        OE_RESTRICT => "RESTRICT",
        _ => {
            debug_assert!(action == OE_NONE);
            "NO ACTION"
        }
    }
}

/// O parâmetro eMode deve ser uma das constantes PAGER_JOURNALMODE_XXX definidas
/// em pager.h. Esta função retorna o nome do modo de journal correspondente em minúsculas.
pub fn journal_modename(e_mode: i32) -> Option<&'static str> {
    const AZ_MODE_NAME: &[&str] = &[
        "delete",
        "persist",
        "off",
        "truncate",
        "memory",
        "wal",
    ];

    if e_mode >= 0 && (e_mode as usize) < AZ_MODE_NAME.len() {
        Some(AZ_MODE_NAME[e_mode as usize])
    } else {
        None
    }
}

/// Localiza um pragma no array A_PRAGMA_NAME.
pub fn pragma_locate(z_name: &[u8]) -> Option<&'static PragmaName> {
    let mut upr = A_PRAGMA_NAME.len() as i32 - 1;
    let mut lwr = 0i32;
    let mut mid = 0i32;

    while lwr <= upr {
        mid = (lwr + upr) / 2;
        let rc = stricmp(Some(z_name), Some(A_PRAGMA_NAME[mid as usize].z_name.as_bytes()));
        if rc == 0 {
            return Some(&A_PRAGMA_NAME[mid as usize]);
        }
        if rc < 0 {
            upr = mid - 1;
        } else {
            lwr = mid + 1;
        }
    }

    if lwr > upr {
        None
    } else {
        Some(&A_PRAGMA_NAME[mid as usize])
    }
}

/// Cria zero ou mais entradas na saída para as funções SQL definidas por FuncDef p.
pub fn pragma_funclist_line(
    v: &mut Vdbe,
    p: Option<FuncDefRef>,
    is_builtin: i32,
    show_intern_funcs: i32,
) {
    const AZ_ENC: &[Option<&[u8]>] = &[None, Some(b"utf8"), Some(b"utf16le"), Some(b"utf16be")];

    let mut mask = SQLITE_DETERMINISTIC
        | SQLITE_DIRECTONLY
        | SQLITE_SUBTYPE
        | SQLITE_INNOCUOUS
        | SQLITE_FUNC_INTERNAL;

    if show_intern_funcs != 0 {
        mask = 0xffffffff;
    }

    let mut p_func = p;
    while let Some(func_ref) = p_func {
        let func_def = func_ref.borrow();
        let next = func_def.p_next.clone();

        if func_def.x_s_func.is_none() {
            p_func = next;
            continue;
        }

        if (func_def.func_flags & SQLITE_FUNC_INTERNAL) != 0 && show_intern_funcs == 0 {
            p_func = next;
            continue;
        }

        let z_type: &[u8] = if func_def.x_value.is_some() {
            b"w"
        } else if func_def.x_finalize.is_some() {
            b"a"
        } else {
            b"s"
        };

        let z_enc = AZ_ENC[(func_def.func_flags & SQLITE_FUNC_ENCMASK) as usize];

        vdbe_multi_load(
            v,
            1,
            b"sissii",
            &[
                MultiLoadArg::Str(Some(&func_def.z_name)),
                MultiLoadArg::Int(is_builtin),
                MultiLoadArg::Str(Some(z_type)),
                MultiLoadArg::Str(z_enc),
                MultiLoadArg::Int(func_def.n_arg as i32),
                MultiLoadArg::Int(((func_def.func_flags & mask) ^ SQLITE_INNOCUOUS) as i32),
            ],
        );

        p_func = next;
    }
}


// ---- part_001.rs ----

/// Estado compartilhado pelos tratadores de cada `case` do `switch` de `sqlite3Pragma`.
///
/// No C, o `switch` é um corpo único com as variáveis locais `zLeft`, `zRight`, `zDb`, `iDb`,
/// `db`, `v`, `pDb`, `pId2` e `pParse` visíveis em todos os `case`. Como o corpo é dividido em
/// vários arquivos, essas variáveis moram aqui e cada `case` vira uma função `pragma_xxx` que
/// recebe `&mut PragmaCtx`. O `goto pragma_out` do C vira um `return` do tratador, porque a
/// limpeza (`zLeft`, `zRight`) é feita pelo `Drop` dos `Vec`.
pub struct PragmaCtx<'a> {
    /// `pParse`.
    pub p_parse: &'a mut Parse,
    /// `db = pParse->db`.
    pub db: Sqlite3Ref,
    /// `v`: a instrução preparada em construção.
    pub v: VdbeRef,
    /// `iDb`: índice do banco em `db.aDb[]`.
    pub i_db: i32,
    /// `zLeft`: o identificador do pragma.
    pub z_left: Vec<u8>,
    /// `zRight`: o valor, ou `None`.
    pub z_right: Option<Vec<u8>>,
    /// `zDb`: nome do banco, ou `None` se o pragma não o nomeou.
    pub z_db: Option<Vec<u8>>,
    /// `pId2->n`: o `journal_mode` o altera para 1 ("PRAGMA journal_mode" vira "main.journal_mode").
    pub id2_n: u32,
    /// `pPragma`.
    pub p_pragma: &'static PragmaName,
}

/// Rotina auxiliar para PRAGMA integrity_check:
/// gera código para produzir uma linha de resultado de coluna única com o valor da string
/// no registrador 3. Decrementa a contagem de resultado no registrador 1 e para se o número
/// máximo de linhas de resultado foi emitido.
pub fn integrity_check_result_row(v: &mut Vdbe) -> i32 {
    vdbe_add_op2(v, OP_RESULTROW as i32, 3, 1);
    let addr = vdbe_add_op3(v, OP_IFPOS as i32, 1, vdbe_current_addr(v) + 2, 1);
    vdbe_add_op0(v, OP_HALT as i32);
    addr
}

/// Processa uma instrução pragma.
///
/// Pragmas são da forma `PRAGMA [schema.]id [= value]`. O identificador pode ser também uma
/// string. O valor é uma string, um identificador ou um número. Se `minus_flag` for verdadeiro,
/// o valor é um número precedido por um sinal de menos.
///
/// Se o lado esquerdo for "database.id", `p_id1` é o nome do banco e `p_id2` é o id. Se for
/// apenas "id", `p_id1` é o id e `p_id2` é uma string vazia.
pub fn pragma(
    p_parse: &mut Parse,
    p_id1: &Token,
    p_id2: &Token,
    p_value: Option<&Token>,
    minus_flag: i32,
) {
    let db: Sqlite3Ref = p_parse.db.upgrade().expect("Parse sem conexão");
    let v = match get_vdbe(p_parse) {
        Some(v) => v,
        None => return,
    };
    vdbe_run_only_once(&mut v.borrow_mut());
    p_parse.n_mem = 2;

    // Interpreta a parte [schema.] do pragma. iDb é o índice do banco ao qual este pragma
    // se aplica em db.aDb[].
    let mut p_id = Token { z: Vec::new(), n: 0 };
    let i_db = two_part_name(p_parse, p_id1, p_id2, &mut p_id);
    if i_db < 0 {
        return;
    }

    // Se o banco temporário foi nomeado explicitamente no pragma, garante que está aberto.
    if i_db == 1 && open_temp_database(p_parse) != 0 {
        return;
    }

    let z_left = match name_from_token(&db.borrow(), Some(&p_id)) {
        Some(z) => z,
        None => return,
    };
    let z_right: Option<Vec<u8>> = if minus_flag != 0 {
        // "-%T": o token é copiado cru, sem remover aspas.
        let mut z = vec![b'-'];
        if let Some(t) = p_value {
            z.extend_from_slice(&t.z[..t.n as usize]);
        }
        Some(z)
    } else {
        name_from_token(&db.borrow(), p_value)
    };

    let z_db: Option<Vec<u8>> = if p_id2.n > 0 {
        db.borrow().a_db[i_db as usize].z_db_sname.clone()
    } else {
        None
    };
    if auth_check(
        p_parse,
        SQLITE_PRAGMA,
        Some(&z_left),
        z_right.as_deref(),
        z_db.as_deref(),
    ) != 0
    {
        return;
    }

    // Envia um file-control SQLITE_FCNTL_PRAGMA para a conexão VFS subjacente. Se retornar
    // SQLITE_OK, assume que o VFS tratou o pragma e gera uma instrução preparada sem efeito.
    //
    // IMPLEMENTATION-OF: R-12238-55120 Sempre que uma instrução PRAGMA é analisada, um
    // file-control SQLITE_FCNTL_PRAGMA é enviado ao sqlite3_file aberto do arquivo do banco
    // a que o pragma se refere.
    //
    // IMPLEMENTATION-OF: R-29875-31678 O argumento do SQLITE_FCNTL_PRAGMA é um array de
    // ponteiros para strings no qual o segundo elemento é o nome do pragma e o terceiro é o
    // argumento do pragma, ou NULL se não houver argumento.
    let mut a_fcntl: [Option<Vec<u8>>; 4] = [None, Some(z_left.clone()), z_right.clone(), None];
    db.borrow_mut().busy_handler.n_busy = 0;
    let rc = file_control(&db, z_db.as_deref(), SQLITE_FCNTL_PRAGMA, Some(&mut a_fcntl));
    if rc == SQLITE_OK {
        let mut vb = v.borrow_mut();
        vdbe_set_num_cols(&mut vb, 1);
        vdbe_set_col_name(&mut vb, 0, COLNAME_NAME, a_fcntl[0].as_deref(), SQLITE_TRANSIENT);
        return_single_text(&mut vb, a_fcntl[0].as_deref());
        return;
    }
    if rc != SQLITE_NOTFOUND {
        if let Some(msg) = &a_fcntl[0] {
            error_msg(p_parse, b"%s", &[PrintfArg::Text(msg)]);
        }
        p_parse.n_err += 1;
        p_parse.rc = rc;
        return;
    }

    // Localiza o pragma na tabela de busca.
    let p_pragma = match pragma_locate(&z_left) {
        Some(p) => p,
        // IMP: R-43042-22504 Nenhuma mensagem de erro é gerada se um pragma desconhecido
        // é emitido.
        None => return,
    };

    // Garante que o esquema do banco está carregado se o pragma exigir.
    if (p_pragma.m_prag_flg & PRAG_FLG_NEED_SCHEMA) != 0 && read_schema(p_parse) != 0 {
        return;
    }

    // Registra os nomes das colunas de resultado para os pragmas que retornam resultados.
    if (p_pragma.m_prag_flg & PRAG_FLG_NO_COLUMNS) == 0
        && ((p_pragma.m_prag_flg & PRAG_FLG_NO_COLUMNS1) == 0 || z_right.is_none())
    {
        set_pragma_result_column_names(&mut v.borrow_mut(), p_pragma);
    }

    let mut ctx = PragmaCtx {
        p_parse,
        db,
        v,
        i_db,
        z_left,
        z_right,
        z_db,
        id2_n: p_id2.n,
        p_pragma,
    };

    // Salta para o tratador apropriado. Os `case` que dependem de SQLITE_OMIT_* ou de
    // SQLITE_DEBUG/SQLITE_TEST/Windows/Apple não existem neste build.
    match p_pragma.e_prag_typ {
        PRAG_TYP_DEFAULT_CACHE_SIZE => pragma_default_cache_size(&mut ctx),
        PRAG_TYP_PAGE_SIZE => pragma_page_size(&mut ctx),
        PRAG_TYP_SECURE_DELETE => pragma_secure_delete(&mut ctx),
        PRAG_TYP_PAGE_COUNT => pragma_page_count(&mut ctx),
        PRAG_TYP_LOCKING_MODE => pragma_locking_mode(&mut ctx),
        PRAG_TYP_JOURNAL_MODE => pragma_journal_mode(&mut ctx),
        PRAG_TYP_JOURNAL_SIZE_LIMIT => pragma_journal_size_limit(&mut ctx),
        PRAG_TYP_AUTO_VACUUM => pragma_auto_vacuum(&mut ctx),
        PRAG_TYP_INCREMENTAL_VACUUM => pragma_incremental_vacuum(&mut ctx),
        PRAG_TYP_CACHE_SIZE => pragma_cache_size(&mut ctx),
        PRAG_TYP_CACHE_SPILL => pragma_cache_spill(&mut ctx),
        PRAG_TYP_MMAP_SIZE => pragma_mmap_size(&mut ctx),
        PRAG_TYP_TEMP_STORE => pragma_temp_store(&mut ctx),
        PRAG_TYP_TEMP_STORE_DIRECTORY => pragma_temp_store_directory(&mut ctx),
        PRAG_TYP_SYNCHRONOUS => pragma_synchronous(&mut ctx),
        PRAG_TYP_FLAG => pragma_flag(&mut ctx),
        PRAG_TYP_TABLE_INFO => pragma_table_info(&mut ctx),
        PRAG_TYP_TABLE_LIST => pragma_table_list(&mut ctx),
        PRAG_TYP_INDEX_INFO => pragma_index_info(&mut ctx),
        PRAG_TYP_INDEX_LIST => pragma_index_list(&mut ctx),
        PRAG_TYP_DATABASE_LIST => pragma_database_list(&mut ctx),
        // Os demais `case` (database_list em diante, trechos 003 a 006 do C) são despachados
        // por `pragma_dispatch_rest`, a ser escrita pelo integrador a partir dos tratadores
        // pragma_xxx das partes 004 a 006, todos com a assinatura `fn(&mut PragmaCtx)`.
        _ => pragma_dispatch_rest(&mut ctx),
    }
}

/// PRAGMA [schema.]default_cache_size, PRAGMA [schema.]default_cache_size=N
///
/// A primeira forma informa a configuração persistente do tamanho do cache de páginas. O
/// valor retornado é o número máximo de páginas no cache. A segunda forma define tanto o
/// valor corrente quanto o valor persistente guardado no arquivo do banco.
///
/// Versões antigas do SQLite definiam o tamanho do cache como negativo para indicar
/// synchronous=OFF. Hoje o synchronous é sempre ligado por padrão, qualquer que seja o sinal.
/// Mesmo assim se toma o valor absoluto, por compatibilidade histórica.
fn pragma_default_cache_size(ctx: &mut PragmaCtx) {
    const GET_CACHE_SIZE: [VdbeOpList; 9] = [
        VdbeOpList { opcode: OP_TRANSACTION, p1: 0, p2: 0, p3: 0 }, // 0
        VdbeOpList { opcode: OP_READCOOKIE, p1: 0, p2: 1, p3: BTREE_DEFAULT_CACHE_SIZE as i8 }, // 1
        VdbeOpList { opcode: OP_IFPOS, p1: 1, p2: 8, p3: 0 },
        VdbeOpList { opcode: OP_INTEGER, p1: 0, p2: 2, p3: 0 },
        VdbeOpList { opcode: OP_SUBTRACT, p1: 1, p2: 2, p3: 1 },
        VdbeOpList { opcode: OP_IFPOS, p1: 1, p2: 8, p3: 0 },
        VdbeOpList { opcode: OP_INTEGER, p1: 0, p2: 1, p3: 0 }, // 6
        VdbeOpList { opcode: OP_NOOP, p1: 0, p2: 0, p3: 0 },
        VdbeOpList { opcode: OP_RESULTROW, p1: 1, p2: 1, p3: 0 },
    ];
    let i_ln = vdbe_offset_lineno(2);
    let i_db = ctx.i_db;
    let mut v = ctx.v.borrow_mut();
    vdbe_uses_btree(&mut v, i_db);
    match &ctx.z_right {
        None => {
            ctx.p_parse.n_mem += 2;
            let a_op = match vdbe_add_op_list(&mut v, GET_CACHE_SIZE.len() as i32, &GET_CACHE_SIZE, i_ln) {
                Some(a_op) => a_op,
                None => return,
            };
            v.a_op[a_op].p1 = i_db;
            v.a_op[a_op + 1].p1 = i_db;
            v.a_op[a_op + 6].p1 = SQLITE_DEFAULT_CACHE_SIZE;
        }
        Some(z_right) => {
            let size = abs_int32(atoi(z_right));
            begin_write_operation(ctx.p_parse, 0, i_db);
            vdbe_add_op3(&mut v, OP_SETCOOKIE as i32, i_db, BTREE_DEFAULT_CACHE_SIZE as i32, size);
            let (p_schema, p_bt) = {
                let db = ctx.db.borrow();
                (db.a_db[i_db as usize].p_schema.clone(), db.a_db[i_db as usize].p_bt.clone())
            };
            let p_schema = p_schema.expect("Db sem Schema");
            p_schema.borrow_mut().cache_size = size;
            let cache_size = p_schema.borrow().cache_size;
            btree_set_cache_size(&mut p_bt.expect("Db sem Btree").borrow_mut(), cache_size);
        }
    }
}

/// PRAGMA [schema.]page_size, PRAGMA [schema.]page_size=N
///
/// A primeira forma informa o tamanho de página do banco em bytes. A segunda o define. O
/// valor só pode ser definido se o banco ainda não foi criado.
fn pragma_page_size(ctx: &mut PragmaCtx) {
    let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
    match &ctx.z_right {
        None => {
            let size = btree_get_page_size(&p_bt.borrow());
            return_single_int(&mut ctx.v.borrow_mut(), size as i64);
        }
        Some(z_right) => {
            // O malloc pode falhar ao definir o tamanho de página, pois há um buffer interno
            // que o módulo do pager redimensiona com sqlite3_realloc().
            let next = atoi(z_right);
            ctx.db.borrow_mut().next_pagesize = next;
            if SQLITE_NOMEM == btree_set_page_size(&mut p_bt.borrow_mut(), next, 0, 0) {
                oom_fault(&mut ctx.db.borrow_mut());
            }
        }
    }
}

/// PRAGMA [schema.]secure_delete, PRAGMA [schema.]secure_delete=ON/OFF/FAST
///
/// A primeira forma informa a configuração do flag secure_delete. A segunda a altera e
/// informa o novo valor.
fn pragma_secure_delete(ctx: &mut PragmaCtx) {
    let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
    let mut b: i32 = -1;
    if let Some(z_right) = &ctx.z_right {
        if stricmp(Some(z_right), Some(b"fast")) == 0 {
            b = 2;
        } else {
            b = get_boolean(z_right, 0) as i32;
        }
    }
    if ctx.id2_n == 0 && b >= 0 {
        let n_db = ctx.db.borrow().n_db;
        for ii in 0..n_db {
            let p = ctx.db.borrow().a_db[ii as usize].p_bt.clone();
            let mut g = p.as_ref().map(|r| r.borrow_mut());
            btree_secure_delete(g.as_deref_mut(), b);
        }
    }
    b = btree_secure_delete(Some(&mut p_bt.borrow_mut()), b);
    return_single_int(&mut ctx.v.borrow_mut(), b as i64);
}

/// PRAGMA [schema.]max_page_count, PRAGMA [schema.]max_page_count=N
///
/// A primeira forma informa o número máximo de páginas do arquivo do banco. A segunda tenta
/// alterá-lo. As duas retornam o valor corrente.
///
/// O valor absoluto de N é usado. Isso não é documentado e pode mudar. O único objetivo é dar
/// um jeito fácil de testar a função abs_int32().
///
/// PRAGMA [schema.]page_count: retorna o número de páginas do banco especificado.
fn pragma_page_count(ctx: &mut PragmaCtx) {
    let mut x: i64 = 0;
    code_verify_schema(ctx.p_parse, ctx.i_db);
    ctx.p_parse.n_mem += 1;
    let i_reg = ctx.p_parse.n_mem;
    let mut v = ctx.v.borrow_mut();
    if tolower(ctx.z_left[0]) == b'p' {
        vdbe_add_op2(&mut v, OP_PAGECOUNT as i32, ctx.i_db, i_reg);
    } else {
        match &ctx.z_right {
            Some(z_right) if dec_or_hex_to_i64(z_right, &mut x) == 0 => {
                if x < 0 {
                    x = 0;
                } else if x > 0xfffffffe {
                    x = 0xfffffffe;
                }
            }
            _ => x = 0,
        }
        vdbe_add_op3(&mut v, OP_MAXPGCNT as i32, ctx.i_db, i_reg, x as i32);
    }
    vdbe_add_op2(&mut v, OP_RESULTROW as i32, i_reg, 1);
}

/// PRAGMA [schema.]locking_mode, PRAGMA [schema.]locking_mode = (normal|exclusive)
fn pragma_locking_mode(ctx: &mut PragmaCtx) {
    let mut z_ret: &[u8] = b"normal";
    let mut e_mode = get_locking_mode(ctx.z_right.as_deref());

    if ctx.id2_n == 0 && e_mode == PAGER_LOCKINGMODE_QUERY {
        // "PRAGMA locking_mode;" simples: é uma consulta ao modo de bloqueio padrão corrente
        // (que pode ser diferente do modo do banco main).
        e_mode = ctx.db.borrow().dflt_lock_mode as i32;
    } else {
        if ctx.id2_n == 0 {
            // Nenhum nome de banco foi dado no comando: o modo de bloqueio deve ser definido
            // em todos os bancos anexados e no arquivo main.
            //
            // Também se define sqlite3.dfltLockMode para que os bancos anexados depois usem
            // o mesmo modo.
            let n_db = ctx.db.borrow().n_db;
            for ii in 2..n_db {
                let p_bt = ctx.db.borrow().a_db[ii as usize].p_bt.clone().expect("Db sem Btree");
                let p_pager = btree_pager(&p_bt.borrow());
                pager_locking_mode(&mut p_pager.borrow_mut(), e_mode);
            }
            ctx.db.borrow_mut().dflt_lock_mode = e_mode as u8;
        }
        let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
        let p_pager = btree_pager(&p_bt.borrow());
        e_mode = pager_locking_mode(&mut p_pager.borrow_mut(), e_mode);
    }

    debug_assert!(e_mode == PAGER_LOCKINGMODE_NORMAL || e_mode == PAGER_LOCKINGMODE_EXCLUSIVE);
    if e_mode == PAGER_LOCKINGMODE_EXCLUSIVE {
        z_ret = b"exclusive";
    }
    return_single_text(&mut ctx.v.borrow_mut(), Some(z_ret));
}


// ---- part_002.rs ----

/// PRAGMA [schema.]journal_mode, PRAGMA [schema.]journal_mode =
/// (delete|persist|off|truncate|memory|wal|off)
///
/// O `case` começa no trecho 001 do C; o tratador inteiro mora aqui para não partir a função.
pub fn pragma_journal_mode(ctx: &mut PragmaCtx) {
    let mut e_mode: i32; // Um dos símbolos PAGER_JOURNALMODE_XXX
    let mut i_db = ctx.i_db;

    match &ctx.z_right {
        None => {
            // Sem a parte "=MODE" do pragma: faz uma consulta ao modo corrente.
            e_mode = PAGER_JOURNALMODE_QUERY;
        }
        Some(z_right) => {
            let n = strlen30(Some(z_right));
            e_mode = 0;
            let mut z_mode = journal_modename(e_mode);
            while let Some(z) = z_mode {
                if strnicmp(Some(z_right), Some(z.as_bytes()), n) == 0 {
                    break;
                }
                e_mode += 1;
                z_mode = journal_modename(e_mode);
            }
            if z_mode.is_none() {
                // Se a parte "=MODE" não casa com nenhum modo conhecido, faz uma consulta.
                e_mode = PAGER_JOURNALMODE_QUERY;
            }
            if e_mode == PAGER_JOURNALMODE_OFF && (ctx.db.borrow().flags & SQLITE_DEFENSIVE) != 0 {
                // Não permite journal-mode "OFF" em modo defensivo: o banco pode ser corrompido
                // por SQL comum quando o journal está desligado.
                e_mode = PAGER_JOURNALMODE_QUERY;
            }
        }
    }
    if e_mode == PAGER_JOURNALMODE_QUERY && ctx.id2_n == 0 {
        // Converte "PRAGMA journal_mode" em "PRAGMA main.journal_mode".
        i_db = 0;
        ctx.id2_n = 1;
    }
    let db = ctx.db.borrow();
    let mut v = ctx.v.borrow_mut();
    for ii in (0..db.n_db).rev() {
        if db.a_db[ii as usize].p_bt.is_some() && (ii == i_db || ctx.id2_n == 0) {
            vdbe_uses_btree(&mut v, ii);
            vdbe_add_op3(&mut v, OP_JOURNALMODE as i32, ii, 1, e_mode);
        }
    }
    vdbe_add_op2(&mut v, OP_RESULTROW as i32, 1, 1);
}

/// PRAGMA [schema.]journal_size_limit, PRAGMA [schema.]journal_size_limit=N
///
/// Obtém ou define o limite de tamanho dos arquivos de rollback journal.
pub fn pragma_journal_size_limit(ctx: &mut PragmaCtx) {
    let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
    let p_pager = btree_pager(&p_bt.borrow());
    let mut i_limit: i64 = -2;
    if let Some(z_right) = &ctx.z_right {
        dec_or_hex_to_i64(z_right, &mut i_limit);
        if i_limit < -1 {
            i_limit = -1;
        }
    }
    i_limit = pager_journal_size_limit(&mut p_pager.borrow_mut(), i_limit);
    return_single_int(&mut ctx.v.borrow_mut(), i_limit);
}

/// PRAGMA [schema.]auto_vacuum, PRAGMA [schema.]auto_vacuum=N
///
/// Obtém ou define o parâmetro 'auto-vacuum' do banco: 0 NONE, 1 FULL, 2 INCREMENTAL.
pub fn pragma_auto_vacuum(ctx: &mut PragmaCtx) {
    let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
    let i_db = ctx.i_db;
    match &ctx.z_right {
        None => {
            let r = btree_get_auto_vacuum(&mut p_bt.borrow_mut());
            return_single_int(&mut ctx.v.borrow_mut(), r as i64);
        }
        Some(z_right) => {
            let e_auto = get_auto_vacuum(z_right);
            debug_assert!(e_auto >= 0 && e_auto <= 2);
            ctx.db.borrow_mut().next_autovac = e_auto as i8;
            // Chama SetAutoVacuum() para inicializar os flags internos de auto e incr-vacuum.
            // Isto é necessário caso esta conexão crie o arquivo do banco: é importante que
            // ele seja criado capaz de auto-vacuum.
            let rc = btree_set_auto_vacuum(&mut p_bt.borrow_mut(), e_auto);
            if rc == SQLITE_OK && (e_auto == 1 || e_auto == 2) {
                // Ao definir o modo como "full" ou "incremental", grava o valor de meta[6] no
                // arquivo. Antes de gravar, confere que meta[3] indica que o banco realmente
                // é capaz de auto-vacuum.
                const SET_META6: [VdbeOpList; 5] = [
                    VdbeOpList { opcode: OP_TRANSACTION, p1: 0, p2: 1, p3: 0 }, // 0
                    VdbeOpList { opcode: OP_READCOOKIE, p1: 0, p2: 1, p3: BTREE_LARGEST_ROOT_PAGE as i8 },
                    VdbeOpList { opcode: OP_IF, p1: 1, p2: 0, p3: 0 }, // 2
                    VdbeOpList { opcode: OP_HALT, p1: SQLITE_OK as i8, p2: OE_ABORT as i8, p3: 0 }, // 3
                    VdbeOpList { opcode: OP_SETCOOKIE, p1: 0, p2: BTREE_INCR_VACUUM as i8, p3: 0 }, // 4
                ];
                let i_ln = vdbe_offset_lineno(2);
                let mut v = ctx.v.borrow_mut();
                let i_addr = vdbe_current_addr(&v);
                let a_op = match vdbe_add_op_list(&mut v, SET_META6.len() as i32, &SET_META6, i_ln) {
                    Some(a_op) => a_op,
                    None => return,
                };
                v.a_op[a_op].p1 = i_db;
                v.a_op[a_op + 1].p1 = i_db;
                v.a_op[a_op + 2].p2 = i_addr + 4;
                v.a_op[a_op + 4].p1 = i_db;
                v.a_op[a_op + 4].p3 = e_auto - 1;
                vdbe_uses_btree(&mut v, i_db);
            }
        }
    }
}

/// PRAGMA [schema.]incremental_vacuum(N)
///
/// Executa N passos de vacuum incremental no banco.
pub fn pragma_incremental_vacuum(ctx: &mut PragmaCtx) {
    let mut i_limit: i32 = 0;
    let ok = match &ctx.z_right {
        Some(z_right) => get_int32(z_right, &mut i_limit) != 0,
        None => false,
    };
    if !ok || i_limit <= 0 {
        i_limit = 0x7fffffff;
    }
    begin_write_operation(ctx.p_parse, 0, ctx.i_db);
    let mut v = ctx.v.borrow_mut();
    vdbe_add_op2(&mut v, OP_INTEGER as i32, i_limit, 1);
    let addr = vdbe_add_op1(&mut v, OP_INCRVACUUM as i32, ctx.i_db);
    vdbe_add_op1(&mut v, OP_RESULTROW as i32, 1);
    vdbe_add_op2(&mut v, OP_ADDIMM as i32, 1, -1);
    vdbe_add_op2(&mut v, OP_IFPOS as i32, 1, addr);
    vdbe_jump_here(&mut v, addr);
}

/// PRAGMA [schema.]cache_size, PRAGMA [schema.]cache_size=N
///
/// A primeira forma informa o tamanho local do cache de páginas. A segunda o define. Se N é
/// positivo, é o número de páginas no cache. Se é negativo, o número de páginas é ajustado
/// para que o cache use -N kibibytes de memória.
pub fn pragma_cache_size(ctx: &mut PragmaCtx) {
    let (p_schema, p_bt) = {
        let db = ctx.db.borrow();
        (db.a_db[ctx.i_db as usize].p_schema.clone(), db.a_db[ctx.i_db as usize].p_bt.clone())
    };
    let p_schema = p_schema.expect("Db sem Schema");
    match &ctx.z_right {
        None => {
            let cache_size = p_schema.borrow().cache_size;
            return_single_int(&mut ctx.v.borrow_mut(), cache_size as i64);
        }
        Some(z_right) => {
            let size = atoi(z_right);
            p_schema.borrow_mut().cache_size = size;
            btree_set_cache_size(&mut p_bt.expect("Db sem Btree").borrow_mut(), size);
        }
    }
}

/// PRAGMA [schema.]cache_spill, PRAGMA cache_spill=BOOLEAN, PRAGMA [schema.]cache_spill=N
///
/// A primeira forma informa o tamanho local de spill do cache. A segunda liga ou desliga o
/// spill; ao ligar, o tamanho é o cache_size corrente. A terceira define um tamanho de spill
/// que pode ser diferente do tamanho do cache. Se N é positivo, é o número de páginas; se é
/// negativo, o número de páginas é ajustado para -N kibibytes.
///
/// Se o número de páginas de cache_spill é menor que o de cache_size, não há spill até a
/// contagem de páginas passar o cache_size. A forma cache_spill=BOOLEAN vale para todos os
/// esquemas anexados, não só o especificado.
pub fn pragma_cache_spill(ctx: &mut PragmaCtx) {
    let p_bt = ctx.db.borrow().a_db[ctx.i_db as usize].p_bt.clone().expect("Db sem Btree");
    match &ctx.z_right {
        None => {
            let v = if (ctx.db.borrow().flags & SQLITE_CACHE_SPILL) == 0 {
                0
            } else {
                btree_set_spill_size(&mut p_bt.borrow_mut(), 0)
            };
            return_single_int(&mut ctx.v.borrow_mut(), v as i64);
        }
        Some(z_right) => {
            let mut size: i32 = 1;
            if get_int32(z_right, &mut size) != 0 {
                btree_set_spill_size(&mut p_bt.borrow_mut(), size);
            }
            let mut db = ctx.db.borrow_mut();
            if get_boolean(z_right, (size != 0) as u8) != 0 {
                db.flags |= SQLITE_CACHE_SPILL;
            } else {
                db.flags &= !SQLITE_CACHE_SPILL;
            }
            set_all_pager_flags(&mut db);
        }
    }
}

/// PRAGMA [schema.]mmap_size(N)
///
/// Usado para definir o limite de mapeamento. O limite restringe o tamanho agregado de todas
/// as regiões do arquivo do banco mapeadas em memória. Se for zero, o mapeamento não é usado.
/// Se N é negativo, vale o limite padrão de sqlite3_config(SQLITE_CONFIG_MMAP_SIZE). O
/// parâmetro N é medido em bytes.
///
/// O valor é consultivo: o VFS pode mapear tão pouco ou tanto quanto quiser. Exceto que, se N
/// é 0, as camadas superiores nunca chamam as interfaces xFetch do VFS.
pub fn pragma_mmap_size(ctx: &mut PragmaCtx) {
    // SQLITE_MAX_MMAP_SIZE>0 no Debian (Linux).
    let mut sz: i64 = 0;
    if let Some(z_right) = &ctx.z_right {
        dec_or_hex_to_i64(z_right, &mut sz);
        if sz < 0 {
            sz = config_mut().sz_mmap;
        }
        if ctx.id2_n == 0 {
            ctx.db.borrow_mut().sz_mmap = sz;
        }
        let n_db = ctx.db.borrow().n_db;
        for ii in (0..n_db).rev() {
            let p_bt = ctx.db.borrow().a_db[ii as usize].p_bt.clone();
            if let Some(p_bt) = p_bt {
                if ii == ctx.i_db || ctx.id2_n == 0 {
                    btree_set_mmap_limit(&mut p_bt.borrow_mut(), sz);
                }
            }
        }
    }
    sz = -1;
    let rc = file_control(&ctx.db, ctx.z_db.as_deref(), SQLITE_FCNTL_MMAP_SIZE, Some(&mut sz));
    if rc == SQLITE_OK {
        return_single_int(&mut ctx.v.borrow_mut(), sz);
    } else if rc != SQLITE_NOTFOUND {
        ctx.p_parse.n_err += 1;
        ctx.p_parse.rc = rc;
    }
}

/// PRAGMA temp_store, PRAGMA temp_store = "default"|"memory"|"file"
///
/// Retorna ou define o valor local do flag temp_store. Mudar o valor local não altera o arquivo
/// em disco e o padrão volta na próxima abertura do banco.
///
/// É possível que as opções de compilação da biblioteca se sobreponham a esta configuração.
pub fn pragma_temp_store(ctx: &mut PragmaCtx) {
    match &ctx.z_right {
        None => {
            let ts = ctx.db.borrow().temp_store;
            return_single_int(&mut ctx.v.borrow_mut(), ts as i64);
        }
        Some(z_right) => {
            change_temp_storage(ctx.p_parse, z_right);
        }
    }
}

/// PRAGMA temp_store_directory, PRAGMA temp_store_directory = ""|"directory_name"
///
/// Retorna ou define o valor local do flag temp_store_directory. Mudar o valor define um
/// diretório específico para os arquivos temporários. Definir como cadeia vazia volta à busca
/// padrão do diretório temporário. Se o diretório mudar, invalida o armazenamento temporário.
pub fn pragma_temp_store_directory(ctx: &mut PragmaCtx) {
    let p_mutex = mutex_alloc(SQLITE_MUTEX_STATIC_TEMPDIR);
    mutex_enter(p_mutex.as_deref());
    match ctx.z_right.clone() {
        None => {
            return_single_text(&mut ctx.v.borrow_mut(), temp_directory().as_deref());
        }
        Some(z_right) => {
            if !z_right.is_empty() {
                let mut res: i32 = 0;
                let p_vfs = ctx.db.borrow().p_vfs.clone().expect("conexão sem VFS");
                let rc = os_access(&*p_vfs, &z_right, SQLITE_ACCESS_READWRITE, &mut res);
                if rc != SQLITE_OK || res == 0 {
                    error_msg(ctx.p_parse, b"not a writable directory", &[]);
                    mutex_leave(mutex_alloc(SQLITE_MUTEX_STATIC_TEMPDIR).as_deref());
                    return;
                }
            }
            // SQLITE_TEMP_STORE vale 1 no Debian: só invalida se temp_store <= 1.
            if ctx.db.borrow().temp_store <= 1 {
                invalidate_temp_storage(ctx.p_parse);
            }
            if !z_right.is_empty() {
                set_temp_directory(Some(z_right));
            } else {
                set_temp_directory(None);
            }
        }
    }
    mutex_leave(mutex_alloc(SQLITE_MUTEX_STATIC_TEMPDIR).as_deref());
}


// ---- part_003.rs ----

// Os `case` PragTyp_LOCK_PROXY_FILE (SQLITE_ENABLE_LOCKING_STYLE, só Apple) e PragTyp_STATS
// (SQLITE_DEBUG) não existem neste build, como manda o CONVENTIONS.md.

/// PRAGMA [schema.]synchronous, PRAGMA [schema.]synchronous=OFF|ON|NORMAL|FULL|EXTRA
///
/// Retorna ou define o valor local do flag synchronous. Mudar o valor local não altera o
/// arquivo em disco e o padrão volta na próxima abertura do banco.
pub fn pragma_synchronous(ctx: &mut PragmaCtx) {
    match &ctx.z_right {
        None => {
            let level = ctx.db.borrow().a_db[ctx.i_db as usize].safety_level;
            return_single_int(&mut ctx.v.borrow_mut(), level as i64 - 1);
        }
        Some(z_right) => {
            if ctx.db.borrow().auto_commit == 0 {
                error_msg(ctx.p_parse, b"Safety level may not be changed inside a transaction", &[]);
            } else if ctx.i_db != 1 {
                let mut i_level =
                    (get_safety_level(z_right, 0, 1) as i32 + 1) & PAGER_SYNCHRONOUS_MASK as i32;
                if i_level == 0 {
                    i_level = 1;
                }
                let mut db = ctx.db.borrow_mut();
                db.a_db[ctx.i_db as usize].safety_level = i_level as u8;
                db.a_db[ctx.i_db as usize].b_sync_set = 1;
                set_all_pager_flags(&mut db);
            }
        }
    }
}

/// PRAGMA de flag (case PragTyp_FLAG): pragmas cujo valor é um bit de `db.flags`.
pub fn pragma_flag(ctx: &mut PragmaCtx) {
    let p_pragma = ctx.p_pragma;
    match &ctx.z_right {
        None => {
            set_pragma_result_column_names(&mut ctx.v.borrow_mut(), p_pragma);
            let set = (ctx.db.borrow().flags & p_pragma.i_arg) != 0;
            return_single_int(&mut ctx.v.borrow_mut(), set as i64);
        }
        Some(z_right) => {
            let mut mask: u64 = p_pragma.i_arg; // Máscara dos bits a ligar ou desligar.
            let mut db = ctx.db.borrow_mut();
            if db.auto_commit == 0 {
                // O suporte a chave estrangeira não pode ser ligado ou desligado fora do modo
                // auto-commit.
                mask &= !SQLITE_FOREIGN_KEYS;
            }

            if get_boolean(z_right, 0) != 0 {
                if (mask & SQLITE_WRITE_SCHEMA) == 0 || (db.flags & SQLITE_DEFENSIVE) == 0 {
                    db.flags |= mask;
                }
            } else {
                db.flags &= !mask;
                if mask == SQLITE_DEFER_FKS {
                    db.n_deferred_imm_cons = 0;
                }
                if (mask & SQLITE_WRITE_SCHEMA) != 0 && stricmp(Some(z_right), Some(b"reset")) == 0 {
                    // IMP: R-60817-01178 Se o argumento é "RESET", a escrita do esquema é
                    // desligada (como em "PRAGMA writable_schema=OFF") e, além disso, o
                    // esquema é recarregado.
                    reset_all_schemas_of_connection(&mut db);
                }
            }

            // Muitos dos pragmas de flag modificam o código gerado pelo compilador SQL (por
            // exemplo count_changes). Por isso se adiciona um opcode que expira todas as
            // instruções compiladas depois de modificar o valor de um pragma.
            vdbe_add_op0(&mut ctx.v.borrow_mut(), OP_EXPIRE as i32);
            set_all_pager_flags(&mut db);
        }
    }
}

/// PRAGMA table_info(<table>)
///
/// Retorna uma linha para cada coluna da tabela nomeada. As colunas do resultado são:
/// cid (número da coluna, da esquerda para a direita a partir de 0), name, type (tipo
/// declarado), notnull (verdadeiro se 'NOT NULL' faz parte da declaração), dflt_value (valor
/// padrão, se houver) e pk (diferente de zero para campos da chave primária).
pub fn pragma_table_info(ctx: &mut PragmaCtx) {
    let z_right = match &ctx.z_right {
        Some(z) => z.clone(),
        None => return,
    };
    code_verify_named_schema(ctx.p_parse, ctx.z_db.as_deref());
    let p_tab = match locate_table(ctx.p_parse, LOCATE_NOERR, &z_right, ctx.z_db.as_deref()) {
        Some(t) => t,
        None => return,
    };
    let mut n_hidden: i32 = 0;
    let p_pk = primary_key_index(&p_tab.borrow());
    ctx.p_parse.n_mem = 7;
    view_get_column_names(ctx.p_parse, &p_tab);
    let tab = p_tab.borrow();
    let i_arg = ctx.p_pragma.i_arg;
    for i in 0..tab.n_col as i32 {
        let p_col = &tab.a_col[i as usize];
        let mut is_hidden: i32 = 0;
        if (p_col.col_flags & COLFLAG_NOINSERT) != 0 {
            if i_arg == 0 {
                n_hidden += 1;
                continue;
            }
            if (p_col.col_flags & COLFLAG_VIRTUAL) != 0 {
                is_hidden = 2; // GENERATED ALWAYS AS ... VIRTUAL
            } else if (p_col.col_flags & COLFLAG_STORED) != 0 {
                is_hidden = 3; // GENERATED ALWAYS AS ... STORED
            } else {
                debug_assert!((p_col.col_flags & COLFLAG_HIDDEN) != 0);
                is_hidden = 1; // HIDDEN
            }
        }
        let k: i32 = if (p_col.col_flags & COLFLAG_PRIMKEY) == 0 {
            0
        } else if let Some(p_pk) = &p_pk {
            let pk = p_pk.borrow();
            let mut k = 1;
            while k <= tab.n_col as i32 && pk.ai_column[(k - 1) as usize] as i32 != i {
                k += 1;
            }
            k
        } else {
            1
        };
        let p_col_expr = column_expr(&tab, p_col);
        debug_assert!(p_col_expr.map_or(true, |e| e.op == TK_SPAN || is_hidden >= 2));
        debug_assert!(
            p_col_expr.map_or(true, |e| !expr_has_property(e, EP_INTVALUE) || is_hidden >= 2)
        );
        // O nome fica antes do primeiro zero de z_cn_name; o tipo declarado vem depois dele.
        let n_name = p_col.z_cn_name.iter().position(|&b| b == 0).unwrap_or(p_col.z_cn_name.len());
        let z_token: Option<&[u8]> = if is_hidden >= 2 || p_col_expr.is_none() {
            None
        } else {
            p_col_expr.and_then(|e| e.u.z_token.as_deref())
        };
        let a_arg = [
            MultiLoadArg::Int(i - n_hidden),
            MultiLoadArg::Str(Some(&p_col.z_cn_name[..n_name])),
            MultiLoadArg::Str(column_type(p_col, Some(b""))),
            MultiLoadArg::Int(if p_col.not_null != 0 { 1 } else { 0 }),
            MultiLoadArg::Str(z_token),
            MultiLoadArg::Int(k),
            MultiLoadArg::Int(is_hidden),
        ];
        let n_arg = if i_arg != 0 { 7 } else { 6 };
        vdbe_multi_load(
            &mut ctx.v.borrow_mut(),
            1,
            if i_arg != 0 { b"issisii" } else { b"issisi" },
            &a_arg[..n_arg],
        );
    }
}

/// PRAGMA table_list
///
/// Retorna uma linha para cada tabela, tabela virtual ou view do esquema inteiro: schema (banco
/// anexado que contém a tabela), name, type ("table", "view", "virtual", "shadow"), ncol,
/// wr (verdadeiro para tabela WITHOUT ROWID) e strict (verdadeiro para tabela STRICT).
pub fn pragma_table_list(ctx: &mut PragmaCtx) {
    ctx.p_parse.n_mem = 6;
    code_verify_named_schema(ctx.p_parse, ctx.z_db.as_deref());
    let n_db = ctx.db.borrow().n_db;
    for ii in 0..n_db {
        let (z_db_sname, p_schema) = {
            let db = ctx.db.borrow();
            let d = &db.a_db[ii as usize];
            (d.z_db_sname.clone().expect("Db sem nome"), d.p_schema.clone().expect("Db sem Schema"))
        };
        if let Some(z_db) = &ctx.z_db {
            if stricmp(Some(z_db), Some(&z_db_sname)) != 0 {
                continue;
            }
        }

        // Garante que o campo Table.nCol está inicializado para todas as views e tabelas
        // virtuais. Cada vez que se inicializa um Table.nCol, a tabela hash pode ser
        // perturbada, então a varredura de inicialização recomeça.
        let mut init_n_col = hash_count(&p_schema.borrow().tbl_hash) as i32;
        while init_n_col > 0 {
            init_n_col -= 1;
            let mut k = hash_first(&p_schema.borrow().tbl_hash);
            loop {
                let elem = match k {
                    Some(e) => e,
                    None => {
                        init_n_col = 0;
                        break;
                    }
                };
                let p_tab: TableRef = hash_data(&elem.borrow())
                    .downcast_ref::<TableRef>()
                    .expect("elemento de tblHash não é Table")
                    .clone();
                if p_tab.borrow().n_col == 0 {
                    let z_name = p_tab.borrow().z_name.clone();
                    let z_sql = m_printf(&ctx.db, b"SELECT*FROM\"%w\"", &[PrintfArg::Text(&z_name)]);
                    if let Some(z_sql) = z_sql {
                        let mut p_dummy: Option<VdbeRef> = None;
                        let _ = api::prepare(&ctx.db, &z_sql, -1, &mut p_dummy, None);
                        let _ = api::finalize(p_dummy);
                    }
                    if ctx.db.borrow().malloc_failed != 0 {
                        error_msg(ctx.p_parse, b"out of memory", &[]);
                        ctx.p_parse.rc = SQLITE_NOMEM_BKPT;
                    }
                    break;
                }
                k = hash_next(&elem.borrow());
            }
        }

        let mut k = hash_first(&p_schema.borrow().tbl_hash);
        while let Some(elem) = k {
            let p_tab: TableRef = hash_data(&elem.borrow())
                .downcast_ref::<TableRef>()
                .expect("elemento de tblHash não é Table")
                .clone();
            k = hash_next(&elem.borrow());
            let tab = p_tab.borrow();
            if let Some(z_right) = &ctx.z_right {
                if stricmp(Some(z_right), Some(&tab.z_name)) != 0 {
                    continue;
                }
            }
            let z_type: &[u8] = if is_view(&tab) {
                b"view"
            } else if is_virtual(&tab) {
                b"virtual"
            } else if (tab.tab_flags & TF_SHADOW) != 0 {
                b"shadow"
            } else {
                b"table"
            };
            vdbe_multi_load(
                &mut ctx.v.borrow_mut(),
                1,
                b"sssiii",
                &[
                    MultiLoadArg::Str(Some(&z_db_sname)),
                    MultiLoadArg::Str(Some(preferred_table_name(&tab.z_name))),
                    MultiLoadArg::Str(Some(z_type)),
                    MultiLoadArg::Int(tab.n_col as i32),
                    MultiLoadArg::Int(((tab.tab_flags & TF_WITHOUT_ROWID) != 0) as i32),
                    MultiLoadArg::Int(((tab.tab_flags & TF_STRICT) != 0) as i32),
                ],
            );
        }
    }
}

/// PRAGMA index_info(<index>) e PRAGMA index_xinfo(<index>) (case PragTyp_INDEX_INFO).
pub fn pragma_index_info(ctx: &mut PragmaCtx) {
    let z_right = match &ctx.z_right {
        Some(z) => z.clone(),
        None => return,
    };
    let mut p_idx = find_index(&ctx.db.borrow(), &z_right, ctx.z_db.as_deref());
    if p_idx.is_none() {
        // Se não há índice chamado zRight, vê se há uma tabela WITHOUT ROWID com esse nome e,
        // se houver, mostra a estrutura do índice da PRIMARY KEY dessa tabela.
        let p_tab = locate_table(ctx.p_parse, LOCATE_NOERR, &z_right, ctx.z_db.as_deref());
        if let Some(p_tab) = p_tab {
            if !has_rowid(&p_tab.borrow()) {
                p_idx = primary_key_index(&p_tab.borrow());
            }
        }
    }
    let p_idx = match p_idx {
        Some(i) => i,
        None => return,
    };
    let idx = p_idx.borrow();
    let i_idx_db = {
        let db = ctx.db.borrow();
        let g = idx.p_schema.as_ref().map(|s| s.borrow());
        schema_to_index(&db, g.as_deref())
    };
    let i_arg = ctx.p_pragma.i_arg;
    let mx: i32;
    if i_arg != 0 {
        // PRAGMA index_xinfo (versão nova, com mais linhas e colunas)
        mx = idx.n_column as i32;
        ctx.p_parse.n_mem = 6;
    } else {
        // PRAGMA index_info (versão legada)
        mx = idx.n_key_col as i32;
        ctx.p_parse.n_mem = 3;
    }
    let p_tab = idx.p_table.upgrade().expect("Index sem Table");
    code_verify_schema(ctx.p_parse, i_idx_db);
    debug_assert!(ctx.p_parse.n_mem <= ctx.p_pragma.n_prag_c_name as i32);
    let tab = p_tab.borrow();
    for i in 0..mx {
        let cnum: i16 = idx.ai_column[i as usize];
        let z_cn_name: Option<&[u8]> = if cnum < 0 {
            None
        } else {
            let n = &tab.a_col[cnum as usize].z_cn_name;
            Some(&n[..n.iter().position(|&b| b == 0).unwrap_or(n.len())])
        };
        let mut v = ctx.v.borrow_mut();
        vdbe_multi_load(
            &mut v,
            1,
            b"iisX",
            &[MultiLoadArg::Int(i), MultiLoadArg::Int(cnum as i32), MultiLoadArg::Str(z_cn_name)],
        );
        if i_arg != 0 {
            vdbe_multi_load(
                &mut v,
                4,
                b"isiX",
                &[
                    MultiLoadArg::Int(idx.a_sort_order[i as usize] as i32),
                    MultiLoadArg::Str(Some(&idx.az_coll[i as usize])),
                    MultiLoadArg::Int((i < idx.n_key_col as i32) as i32),
                ],
            );
        }
        vdbe_add_op2(&mut v, OP_RESULTROW as i32, 1, ctx.p_parse.n_mem);
    }
}

/// PRAGMA index_list(<table>)
pub fn pragma_index_list(ctx: &mut PragmaCtx) {
    let z_right = match &ctx.z_right {
        Some(z) => z.clone(),
        None => return,
    };
    let p_tab = match find_table(&ctx.db.borrow(), &z_right, ctx.z_db.as_deref()) {
        Some(t) => t,
        None => return,
    };
    let tab = p_tab.borrow();
    let i_tab_db = {
        let db = ctx.db.borrow();
        let g = tab.p_schema.as_ref().map(|s| s.borrow());
        schema_to_index(&db, g.as_deref())
    };
    ctx.p_parse.n_mem = 5;
    code_verify_schema(ctx.p_parse, i_tab_db);
    const AZ_ORIGIN: [&[u8]; 3] = [b"c", b"u", b"pk"];
    let mut p = tab.p_index.clone();
    let mut i: i32 = 0;
    while let Some(p_idx) = p {
        let idx = p_idx.borrow();
        vdbe_multi_load(
            &mut ctx.v.borrow_mut(),
            1,
            b"isisi",
            &[
                MultiLoadArg::Int(i),
                MultiLoadArg::Str(Some(&idx.z_name)),
                MultiLoadArg::Int(is_unique_index(&idx) as i32),
                MultiLoadArg::Str(Some(AZ_ORIGIN[idx.idx_type as usize])),
                MultiLoadArg::Int(idx.p_part_idx_where.is_some() as i32),
            ],
        );
        p = idx.p_next.clone();
        i += 1;
    }
}

/// PRAGMA database_list
///
/// O `case` começa no fim do trecho 003 do C e termina no início do 004; o tratador inteiro
/// mora aqui.
pub fn pragma_database_list(ctx: &mut PragmaCtx) {
    ctx.p_parse.n_mem = 3;
    let n_db = ctx.db.borrow().n_db;
    for i in 0..n_db {
        let (p_bt, z_db_sname) = {
            let db = ctx.db.borrow();
            (db.a_db[i as usize].p_bt.clone(), db.a_db[i as usize].z_db_sname.clone())
        };
        let p_bt = match p_bt {
            Some(b) => b,
            None => continue,
        };
        debug_assert!(z_db_sname.is_some());
        let z_filename = btree_get_filename(&p_bt.borrow());
        vdbe_multi_load(
            &mut ctx.v.borrow_mut(),
            1,
            b"iss",
            &[
                MultiLoadArg::Int(i),
                MultiLoadArg::Str(z_db_sname.as_deref()),
                MultiLoadArg::Str(z_filename.as_deref()),
            ],
        );
    }
}


// ---- part_004.rs ----

// Notas desta parte (pragma.c, chunk 004):
//  - Cada `case PragTyp_xxx` vira uma função própria. O `case PragTyp_INTEGRITY_CHECK`, que
//    atravessa os chunks 004 a 006, vira `pragma_integrity_check_begin` (prólogo),
//    `integrity_check_btree_pass` (b-tree, chunk 004), `integrity_check_index_counts` e
//    `integrity_check_table_rows` (chunk 005, com o rabo que está no começo do chunk 006),
//    `integrity_check_virtual_tables` e `integrity_check_end_code` (chunk 006).
//  - Convenção: `p_parse: &mut Parse` e `v: &VdbeRef`; o `borrow_mut()` do Vdbe é sempre temporário,
//    nunca segurado durante chamada que também pegue o Vdbe pelo Parse.
//  - Os `assert()` e `VdbeCoverage()` do C (só SQLITE_DEBUG e SQLITE_COVERAGE_TEST) não existem.

/// Devolve a tabela guardada num elemento de `Hash` de tabelas (`sqliteHashData(x)`).
fn hash_table(e: &HashElemRef) -> Option<TableRef> {
    e.borrow().data.downcast_ref::<TableRef>().cloned()
}

/// Lista, na ordem de iteração do `Hash`, as tabelas do schema do banco `i_db`
/// (`for(x=sqliteHashFirst(pTbls); x; x=sqliteHashNext(x))`).
fn schema_tables(db: &Sqlite3Ref, i_db: i32) -> Vec<TableRef> {
    let mut out = Vec::new();
    let db_b = db.borrow();
    let schema = db_b.a_db[i_db as usize].p_schema.as_ref().expect("schema ausente");
    let mut x = hash_first(&schema.borrow().tbl_hash);
    while let Some(e) = x {
        if let Some(t) = hash_table(&e) {
            out.push(t);
        }
        x = hash_next(&e.borrow());
    }
    out
}

/// `sqlite3MPrintf(db, z_format, ...)` para formatos cujos argumentos são só texto (`%s`).
fn m_printf_texts(db: &Sqlite3Ref, z_format: &[u8], args: &[&[u8]]) -> Vec<u8> {
    let mut ap = VaList::new();
    for a in args {
        ap.args.push_back(VaArg::Text(Some(a.to_vec())));
    }
    m_printf(db, z_format, &mut ap).unwrap_or_default()
}

/// `sqlite3SchemaToIndex(db, pTab->pSchema)`.
fn table_schema_index(db: &Sqlite3Ref, p_tab: &TableRef) -> i32 {
    let sch = p_tab.borrow().p_schema.as_ref().and_then(|w| w.upgrade());
    let dbb = db.borrow();
    match sch {
        Some(s) => schema_to_index(&dbb, Some(&s.borrow())),
        None => schema_to_index(&dbb, None),
    }
}

/// Percorre a lista encadeada de índices da tabela (`for(pIdx=pTab->pIndex; pIdx; pIdx=pIdx->pNext)`).
fn table_indexes(p_tab: &TableRef) -> Vec<IndexRef> {
    let mut out = Vec::new();
    let mut p = p_tab.borrow().p_index.clone();
    while let Some(idx) = p {
        p = idx.borrow().p_next.clone();
        out.push(idx);
    }
    out
}

/// PRAGMA database_list
pub fn pragma_database_list(p_parse: &mut Parse, v: &VdbeRef, db: &Sqlite3Ref) {
    p_parse.n_mem = 3;
    let n_db = db.borrow().n_db;
    for i in 0..n_db {
        let (z_name, z_file) = {
            let db_b = db.borrow();
            let p_db = &db_b.a_db[i as usize];
            let p_bt = match p_db.p_bt.as_ref() {
                None => continue,
                Some(b) => b,
            };
            (p_db.z_db_s_name.clone(), btree_get_filename(p_bt))
        };
        vdbe_multi_load(
            &mut v.borrow_mut(),
            1,
            b"iss",
            &[
                MultiLoadArg::Int(i),
                MultiLoadArg::Str(Some(&z_name)),
                MultiLoadArg::Str(Some(&z_file)),
            ],
        );
    }
}

/// PRAGMA collation_list
pub fn pragma_collation_list(p_parse: &mut Parse, v: &VdbeRef, db: &Sqlite3Ref) {
    let mut i = 0;
    p_parse.n_mem = 2;
    let mut p = hash_first(&db.borrow().a_coll_seq);
    while let Some(e) = p {
        let z_name = {
            let eb = e.borrow();
            let p_coll = hash_data(&eb)
                .downcast_ref::<Vec<CollSeq>>()
                .expect("dado de aCollSeq não é CollSeq");
            p_coll[0].z_name.clone()
        };
        vdbe_multi_load(
            &mut v.borrow_mut(),
            1,
            b"is",
            &[MultiLoadArg::Int(i), MultiLoadArg::Str(Some(&z_name))],
        );
        i += 1;
        p = hash_next(&e.borrow());
    }
}

/// PRAGMA function_list (SQLITE_OMIT_INTROSPECTION_PRAGMAS não está definido)
pub fn pragma_function_list(p_parse: &mut Parse, v: &VdbeRef, db: &Sqlite3Ref) {
    let show_intern_func = if (db.borrow().m_db_flags & DBFLAG_INTERNALFUNC) != 0 { 1 } else { 0 };
    p_parse.n_mem = 6;
    for i in 0..SQLITE_FUNC_HASH_SZ {
        // `sqlite3BuiltinFunctions.a[i]` e a corrente `p->u.pHash` (a própria função
        // percorre a corrente em `pragma_funclist_line`).
        let p = builtin_functions_bucket(i);
        pragma_funclist_line(&mut v.borrow_mut(), p, 1, show_intern_func);
    }
    let mut j = hash_first(&db.borrow().a_func);
    while let Some(e) = j {
        let p = e.borrow().data.downcast_ref::<FuncDefRef>().cloned();
        pragma_funclist_line(&mut v.borrow_mut(), p, 0, show_intern_func);
        j = hash_next(&e.borrow());
    }
}

/// PRAGMA module_list (SQLITE_OMIT_VIRTUALTABLE não está definido)
pub fn pragma_module_list(p_parse: &mut Parse, v: &VdbeRef, db: &Sqlite3Ref) {
    p_parse.n_mem = 1;
    let mut j = hash_first(&db.borrow().a_module);
    while let Some(e) = j {
        let z_name = {
            let eb = e.borrow();
            let p_mod = hash_data(&eb)
                .downcast_ref::<ModuleRef>()
                .expect("dado de aModule não é Module");
            p_mod.borrow().z_name.clone()
        };
        vdbe_multi_load(&mut v.borrow_mut(), 1, b"s", &[MultiLoadArg::Str(Some(&z_name))]);
        j = hash_next(&e.borrow());
    }
}

/// PRAGMA pragma_list
pub fn pragma_pragma_list(v: &VdbeRef) {
    for p in A_PRAGMA_NAME.iter() {
        vdbe_multi_load(
            &mut v.borrow_mut(),
            1,
            b"s",
            &[MultiLoadArg::Str(Some(p.z_name.as_bytes()))],
        );
    }
}

/// PRAGMA foreign_key_list(<table>)
pub fn pragma_foreign_key_list(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    z_right: Option<&[u8]>,
    z_db: Option<&[u8]>,
) {
    let z_right = match z_right {
        Some(z) => z,
        None => return,
    };
    let p_tab = match find_table(&db.borrow(), z_right, z_db) {
        Some(t) => t,
        None => return,
    };
    if !is_ordinary_table(&p_tab.borrow()) {
        return;
    }
    let mut p_fk = p_tab.borrow().u.tab.p_fkey.clone();
    if p_fk.is_some() {
        let i_tab_db = table_schema_index(db, &p_tab);
        let mut i = 0;
        p_parse.n_mem = 8;
        code_verify_schema(p_parse, i_tab_db);
        while let Some(fk_ref) = p_fk {
            {
                let fk = fk_ref.borrow();
                let tab = p_tab.borrow();
                for j in 0..fk.n_col as usize {
                    vdbe_multi_load(
                        &mut v.borrow_mut(),
                        1,
                        b"iissssss",
                        &[
                            MultiLoadArg::Int(i),
                            MultiLoadArg::Int(j as i32),
                            MultiLoadArg::Str(Some(&fk.z_to)),
                            MultiLoadArg::Str(Some(&tab.a_col[fk.a_col[j].i_from as usize].z_cn_name)),
                            MultiLoadArg::Str(fk.a_col[j].z_col.as_deref()),
                            MultiLoadArg::Str(Some(action_name(fk.a_action[1]).as_bytes())), // ON UPDATE
                            MultiLoadArg::Str(Some(action_name(fk.a_action[0]).as_bytes())), // ON DELETE
                            MultiLoadArg::Str(Some(b"NONE")),
                        ],
                    );
                }
            }
            i += 1;
            p_fk = fk_ref.borrow().p_next_from.clone();
        }
    }
}

/// PRAGMA foreign_key_check / foreign_key_check(<table>) (FOREIGN_KEY e TRIGGER habilitados)
pub fn pragma_foreign_key_check(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    z_right: Option<&[u8]>,
    i_db: &mut i32,
    z_db: &mut Option<Vec<u8>>,
) {
    let reg_result = p_parse.n_mem + 1;
    p_parse.n_mem += 4;
    p_parse.n_mem += 1;
    let reg_row = p_parse.n_mem;
    // `k` é o laço sobre as tabelas do schema; com `zRight` vale 1 iteração só.
    let tables = schema_tables(db, *i_db);
    let mut k_idx = 0usize;
    let mut k_live = !tables.is_empty();
    while k_live {
        let p_tab: Option<TableRef>;
        if let Some(z) = z_right {
            p_tab = locate_table(p_parse, 0, z, z_db.as_deref());
            k_live = false;
        } else {
            p_tab = Some(tables[k_idx].clone());
            k_idx += 1;
            k_live = k_idx < tables.len();
        }
        let p_tab = match p_tab {
            Some(t) => t,
            None => continue,
        };
        if !is_ordinary_table(&p_tab.borrow()) || p_tab.borrow().u.tab.p_fkey.is_none() {
            continue;
        }
        *i_db = table_schema_index(db, &p_tab);
        *z_db = Some(db.borrow().a_db[*i_db as usize].z_db_s_name.clone());
        code_verify_schema(p_parse, *i_db);
        let (tnum, z_tab_name, n_col) = {
            let t = p_tab.borrow();
            (t.tnum, t.z_name.clone(), t.n_col as i32)
        };
        table_lock(p_parse, *i_db, tnum, 0, &z_tab_name);
        touch_register(p_parse, n_col + reg_row);
        open_table(p_parse, 0, *i_db, &p_tab.borrow(), OP_OPENREAD as i32);
        vdbe_load_string(&mut v.borrow_mut(), reg_result, &z_tab_name);
        let mut i = 1;
        let mut p_fk = p_tab.borrow().u.tab.p_fkey.clone();
        while let Some(fk_ref) = p_fk.clone() {
            let z_to = fk_ref.borrow().z_to.clone();
            let p_parent = find_table(&db.borrow(), &z_to, z_db.as_deref());
            if let Some(p_parent) = p_parent {
                let mut p_idx: Option<IndexRef> = None;
                table_lock(p_parse, *i_db, p_parent.borrow().tnum, 0, &p_parent.borrow().z_name.clone());
                let x = fk_locate_index(p_parse, &p_parent, &fk_ref, &mut p_idx, None);
                if x == 0 {
                    match p_idx {
                        None => open_table(p_parse, i, *i_db, &p_parent.borrow(), OP_OPENREAD as i32),
                        Some(idx) => {
                            vdbe_add_op3(&mut v.borrow_mut(), OP_OPENREAD as i32, i, idx.borrow().tnum as i32, *i_db);
                            vdbe_set_p4_key_info(p_parse, &idx);
                        }
                    }
                } else {
                    k_live = false;
                    break;
                }
            }
            i += 1;
            p_fk = fk_ref.borrow().p_next_from.clone();
        }
        if p_fk.is_some() {
            break;
        }
        if p_parse.n_tab < i {
            p_parse.n_tab = i;
        }
        let addr_top = vdbe_add_op1(&mut v.borrow_mut(), OP_REWIND as i32, 0);
        let mut i = 1;
        let mut p_fk = p_tab.borrow().u.tab.p_fkey.clone();
        while let Some(fk_ref) = p_fk {
            let (z_to, fk_n_col) = {
                let fk = fk_ref.borrow();
                (fk.z_to.clone(), fk.n_col)
            };
            let p_parent = find_table(&db.borrow(), &z_to, z_db.as_deref());
            let mut p_idx: Option<IndexRef> = None;
            let mut ai_cols: Option<Vec<i32>> = None;
            if let Some(ref p_parent) = p_parent {
                fk_locate_index(p_parse, p_parent, &fk_ref, &mut p_idx, Some(&mut ai_cols));
            }
            let addr_ok = vdbe_make_label(p_parse);

            // Gera código para ler os valores da chave filha para os registradores
            // regRow..regRow+n. Se algum for NULL, a linha não causa violação de FK.
            touch_register(p_parse, reg_row + fk_n_col);
            for j in 0..fk_n_col {
                let i_col = match ai_cols {
                    Some(ref a) => a[j as usize],
                    None => fk_ref.borrow().a_col[j as usize].i_from,
                };
                expr_code_get_column_of_table(v, &p_tab, 0, i_col, reg_row + j);
                vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, reg_row + j, addr_ok);
            }

            // Gera código para consultar o índice do pai atrás de uma chave igual.
            // Achando, salta para addrOk.
            if let Some(ref idx) = p_idx {
                let z_aff = index_affinity_str(&mut db.borrow_mut(), &mut idx.borrow_mut());
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_AFFINITY as i32,
                    reg_row,
                    fk_n_col,
                    0,
                    P4Value::Static(z_aff.unwrap_or_default()),
                    fk_n_col,
                );
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_FOUND as i32, i, addr_ok, reg_row, fk_n_col);
            } else if p_parent.is_some() {
                let jmp = vdbe_current_addr(&v.borrow()) + 2;
                vdbe_add_op3(&mut v.borrow_mut(), OP_SEEKROWID as i32, i, jmp, reg_row);
                vdbe_goto(&mut v.borrow_mut(), addr_ok);
            }

            // Gera código para reportar a violação de FK a quem chamou.
            if has_rowid(&p_tab.borrow()) {
                vdbe_add_op2(&mut v.borrow_mut(), OP_ROWID as i32, 0, reg_result + 1);
            } else {
                vdbe_add_op2(&mut v.borrow_mut(), OP_NULL as i32, 0, reg_result + 1);
            }
            // "siX": o `X` interrompe o laço sem gerar OP_ResultRow.
            vdbe_multi_load(
                &mut v.borrow_mut(),
                reg_result + 2,
                b"siX",
                &[MultiLoadArg::Str(Some(&z_to)), MultiLoadArg::Int(i - 1)],
            );
            vdbe_add_op2(&mut v.borrow_mut(), OP_RESULTROW as i32, reg_result, 4);
            vdbe_resolve_label(&mut v.borrow_mut(), addr_ok);
            i += 1;
            p_fk = fk_ref.borrow().p_next_from.clone();
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, 0, addr_top + 1);
        vdbe_jump_here(&mut v.borrow_mut(), addr_top);
    }
}

/// PRAGMA case_sensitive_like = BOOLEAN: reinstala LIKE e GLOB; a variante de LIKE
/// será ou não sensível a maiúsculas conforme o lado direito.
pub fn pragma_case_sensitive_like(db: &Sqlite3Ref, z_right: Option<&[u8]>) {
    if let Some(z) = z_right {
        register_like_functions(db, get_boolean(z, 0) as i32);
    }
}

/// Máximo de erros do integrity_check quando o usuário não dá `N`.
pub const SQLITE_INTEGRITY_CHECK_ERROR_MAX: i32 = 100;

/// Estado do `PRAGMA integrity_check` / `quick_check` que atravessa os laços por banco.
pub struct IntegrityCheck {
    /// Verdadeiro para `quick_check`.
    pub is_quick: bool,
    /// Banco verificado, ou -1 para todos.
    pub i_db: i32,
    /// Número máximo de erros.
    pub mx_err: i32,
    /// Se não for `None`, só esta tabela é verificada.
    pub p_obj_tab: Option<TableRef>,
}

/// Prólogo do PRAGMA integrity_check, até o `OP_Integer` do contador de erros em reg[1].
///
///    PRAGMA integrity_check
///    PRAGMA integrity_check(N)
///    PRAGMA quick_check
///    PRAGMA quick_check(N)
///
/// Verifica a integridade do banco. O `quick_check` é a versão reduzida, que não confere
/// os índices cruzados (tempo linear contra O(N log N)). O máximo de erros é 100 por padrão;
/// o parâmetro N pode ser o nome de uma tabela, e então só ela é verificada (a freelist só
/// se a tabela for `sqlite_schema` ou um de seus apelidos).
pub fn pragma_integrity_check_begin(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    z_left: &[u8],
    z_right: Option<&[u8]>,
    p_value: &Token,
    p_id2: &Token,
    i_db: i32,
) -> IntegrityCheck {
    let is_quick = tolower(z_left[0]) == b'q';
    let mut p_obj_tab: Option<TableRef> = None;

    // Com "PRAGMA <db>.integrity_check", iDb é o índice de <db> e só ele é verificado.
    // Com "PRAGMA integrity_check" simples, iDb vale 0 e vira -1: verifica todos os anexados.
    let i_db = if p_id2.z.is_none() { -1 } else { i_db };

    // Inicializa o programa do VDBE
    p_parse.n_mem = 6;

    // Define o máximo de erros
    let mut mx_err = SQLITE_INTEGRITY_CHECK_ERROR_MAX;
    if let Some(z_right) = z_right {
        let z_val = p_value.z.as_deref().unwrap_or(&[]);
        if get_int32(z_val, &mut mx_err) != 0 {
            if mx_err <= 0 {
                mx_err = SQLITE_INTEGRITY_CHECK_ERROR_MAX;
            }
        } else {
            let z_schema = if i_db >= 0 {
                Some(db.borrow().a_db[i_db as usize].z_db_s_name.clone())
            } else {
                None
            };
            p_obj_tab = locate_table(p_parse, 0, z_right, z_schema.as_deref());
        }
    }
    vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, mx_err - 1, 1); // reg[1] guarda os erros restantes
    IntegrityCheck { is_quick, i_db, mx_err, p_obj_tab }
}

/// Parte do integrity_check que cuida da b-tree do banco `i`: acha as páginas raiz de todas
/// as tabelas e índices e gera o `OP_IntegrityCk`. Devolve `false` quando não há nada a
/// verificar neste banco (`if( cnt==0 ) continue;`).
pub fn integrity_check_btree_pass(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    st: &IntegrityCheck,
    i: i32,
    tables: &[TableRef],
) -> bool {
    code_verify_schema(p_parse, i);
    p_parse.ok_const_factor = 0; // tag-20230327-1

    // Faz a verificação de integridade da B-Tree. Começa achando os números das páginas
    // raiz de todas as tabelas e índices do banco.
    let mut cnt: i32 = 0;
    for p_tab in tables {
        if let Some(ref obj) = st.p_obj_tab {
            if !Rc::ptr_eq(obj, p_tab) {
                continue;
            }
        }
        if has_rowid(&p_tab.borrow()) {
            cnt += 1;
        }
        cnt += table_indexes(p_tab).len() as i32;
    }
    if cnt == 0 {
        return false;
    }
    if st.p_obj_tab.is_some() {
        cnt += 1;
    }
    let mut a_root: Vec<u32> = vec![0; (cnt + 1) as usize];
    cnt = 0;
    if st.p_obj_tab.is_some() {
        cnt += 1;
        a_root[cnt as usize] = 0;
    }
    for p_tab in tables {
        if let Some(ref obj) = st.p_obj_tab {
            if !Rc::ptr_eq(obj, p_tab) {
                continue;
            }
        }
        if has_rowid(&p_tab.borrow()) {
            cnt += 1;
            a_root[cnt as usize] = p_tab.borrow().tnum;
        }
        for p_idx in table_indexes(p_tab) {
            cnt += 1;
            a_root[cnt as usize] = p_idx.borrow().tnum;
        }
    }
    a_root[0] = cnt as u32;

    // Garante que há registradores suficientes alocados
    touch_register(p_parse, 8 + cnt);
    clear_temp_reg_cache(p_parse);

    // Faz as verificações de integridade das b-trees
    vdbe_add_op4(
        &mut v.borrow_mut(),
        OP_INTEGRITYCK as i32,
        1,
        cnt,
        8,
        P4Value::IntArray(a_root),
        P4_INTARRAY as i32,
    );
    vdbe_change_p5(&mut v.borrow_mut(), i as u16);
    let addr = vdbe_add_op1(&mut v.borrow_mut(), OP_ISNULL as i32, 2);
    let z_msg = {
        let z_name = db.borrow().a_db[i as usize].z_db_s_name.clone();
        m_printf_texts(db, b"*** in database %s ***\n", &[&z_name])
    };
    vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_msg), P4_DYNAMIC as i32);
    vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 2, 3, 3);
    integrity_check_result_row(&mut v.borrow_mut());
    vdbe_jump_here(&mut v.borrow_mut(), addr);
    true
}


// ---- part_005.rs ----

// Notas desta parte (pragma.c, chunk 005): continuação do `PRAGMA integrity_check`.
//  - O prólogo e a passada da b-tree estão em part_004; `integrity_check_result_row` está em part_001.
//  - O laço por tabela termina no começo do chunk 006 (`OP_Next`, `JumpHere(loopTop-1)` e
//    `ReleaseTempRange`); esse rabo está aqui, dentro de `integrity_check_table_rows`, para a
//    função não ser cortada ao meio.

/// Confere se todos os índices têm o número certo de linhas (reg[8+i] contra reg[8+iTab]).
pub fn integrity_check_index_counts(v: &VdbeRef, st: &IntegrityCheck, tables: &[TableRef]) {
    let mut cnt: i32 = if st.p_obj_tab.is_some() { 1 } else { 0 };
    vdbe_load_string(&mut v.borrow_mut(), 2, b"wrong # of entries in index ");
    for p_tab in tables {
        if let Some(ref obj) = st.p_obj_tab {
            if !Rc::ptr_eq(obj, p_tab) {
                continue;
            }
        }
        let indexes = table_indexes(p_tab);
        let i_tab: i32;
        if has_rowid(&p_tab.borrow()) {
            i_tab = cnt;
            cnt += 1;
        } else {
            let mut t = cnt;
            for p_idx in &indexes {
                if is_primary_key_index(&p_idx.borrow()) {
                    break;
                }
                t += 1;
            }
            i_tab = t;
        }
        for p_idx in &indexes {
            if p_idx.borrow().p_part_idx_where.is_none() {
                let addr = vdbe_add_op3(&mut v.borrow_mut(), OP_EQ as i32, 8 + cnt, 0, 8 + i_tab);
                let z_name = p_idx.borrow().z_name.clone();
                vdbe_load_string(&mut v.borrow_mut(), 4, &z_name);
                vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 4, 2, 3);
                integrity_check_result_row(&mut v.borrow_mut());
                vdbe_jump_here(&mut v.borrow_mut(), addr);
            }
            cnt += 1;
        }
    }
}

/// Máscara de tipos aceitos pelo `OP_IsType` para cada tipo de tabela STRICT
/// (ANY, BLOB, INT, INTEGER, REAL, TEXT).
const A_STD_TYPE_MASK: [u8; 6] = [
    0x1f, // ANY
    0x18, // BLOB
    0x11, // INT
    0x11, // INTEGER
    0x13, // REAL
    0x14, // TEXT
];

/// Gera o código que confere, linha a linha, as tabelas comuns e seus índices: tipos das
/// colunas, restrições CHECK e entradas de índice.
pub fn integrity_check_table_rows(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    st: &IntegrityCheck,
    tables: &[TableRef],
) {
    let is_quick = st.is_quick;
    for p_tab in tables {
        if let Some(ref obj) = st.p_obj_tab {
            if !Rc::ptr_eq(obj, p_tab) {
                continue;
            }
        }
        if !is_ordinary_table(&p_tab.borrow()) {
            continue;
        }
        let indexes = table_indexes(p_tab);
        let mut p_prior: Option<IndexRef> = None;
        let mut r1: i32 = -1;
        let p_pk: Option<IndexRef>;
        let r2: i32;
        if is_quick || has_rowid(&p_tab.borrow()) {
            p_pk = None;
            r2 = 0;
        } else {
            let pk = primary_key_index(&p_tab.borrow()).expect("WITHOUT ROWID sem chave primária");
            let n_key = pk.borrow().n_key_col as i32;
            r2 = get_temp_range(p_parse, n_key);
            vdbe_add_op3(&mut v.borrow_mut(), OP_NULL as i32, 1, r2, r2 + n_key - 1);
            p_pk = Some(pk);
        }
        let mut i_data_cur: i32 = 0;
        let mut i_idx_cur: i32 = 0;
        open_table_and_indices(p_parse, p_tab, OP_OPENREAD as i32, 0, 1, None, &mut i_data_cur, &mut i_idx_cur);
        // reg[7] conta as entradas da tabela; reg[8+i] conta as do i-ésimo índice.
        vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, 7);
        let mut j: i32 = 0;
        for _ in &indexes {
            vdbe_add_op2(&mut v.borrow_mut(), OP_INTEGER as i32, 0, 8 + j); // contador de entradas do índice
            j += 1;
        }
        vdbe_add_op2(&mut v.borrow_mut(), OP_REWIND as i32, i_data_cur, 0);
        let loop_top = vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, 7, 1);

        // Lê a coluna mais à direita da tabela. Isso faz o cabeçalho inteiro do registro ser
        // analisado e conferido, e preenche o cache de colunas do cursor que o OP_IsType usa,
        // então o passo é obrigatório.
        let has_rowid_tab = has_rowid(&p_tab.borrow());
        let mx_col: i32 = if has_rowid_tab {
            let t = p_tab.borrow();
            let mut mx = -1;
            for jj in 0..t.n_col as usize {
                if (t.a_col[jj].col_flags & COLFLAG_VIRTUAL) == 0 {
                    mx += 1;
                }
            }
            if mx == t.i_p_key as i32 {
                mx -= 1;
            }
            mx
        } else {
            // Colunas COLFLAG_VIRTUAL não entram na contagem de colunas do índice PK de
            // WITHOUT ROWID, então não há o que descontar aqui.
            primary_key_index(&p_tab.borrow()).expect("sem PK").borrow().n_column as i32 - 1
        };
        if mx_col >= 0 {
            vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_data_cur, mx_col, 3);
            vdbe_typeof_column(&mut v.borrow_mut(), 3);
        }

        if !is_quick {
            if let Some(ref pk) = p_pk {
                // Confere que as chaves de WITHOUT ROWID estão em ordem crescente
                let n_key = pk.borrow().n_key_col as i32;
                let a1 = vdbe_add_op4_int(&mut v.borrow_mut(), OP_IDXGT as i32, i_data_cur, 0, r2, n_key);
                vdbe_add_op1(&mut v.borrow_mut(), OP_ISNULL as i32, r2);
                let z_err = {
                    let z_tab_name = p_tab.borrow().z_name.clone();
                    m_printf_texts(db, b"row not in PRIMARY KEY order for %s", &[&z_tab_name])
                };
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
                integrity_check_result_row(&mut v.borrow_mut());
                vdbe_jump_here(&mut v.borrow_mut(), a1);
                vdbe_jump_here(&mut v.borrow_mut(), a1 + 1);
                for jj in 0..n_key {
                    expr_code_load_index_column(&p_parse_ref(p_parse), pk, i_data_cur, jj, r2 + jj);
                }
            }
        }

        // Confere os tipos de dados de todas as colunas:
        //
        //   (1) colunas NOT NULL não podem conter NULL
        //   (2) o tipo tem que ser exato nas colunas não ANY de tabelas STRICT
        //   (3) o tipo de colunas TEXT em tabelas não STRICT tem que ser NULL, TEXT ou BLOB
        //   (4) o tipo de colunas numéricas em tabelas não STRICT não pode ser um TEXT que
        //       possa ser convertido em número sem perda
        let b_strict = (p_tab.borrow().tab_flags & TF_STRICT) != 0;
        let n_col = p_tab.borrow().n_col as i32;
        for jj in 0..n_col {
            let (not_null, e_c_type, affinity, col_flags, i_dflt, z_cn_name) = {
                let t = p_tab.borrow();
                if jj == t.i_p_key as i32 {
                    continue;
                }
                let c = &t.a_col[jj as usize];
                (c.not_null, c.e_c_type, c.affinity, c.col_flags, c.i_dflt, c.z_cn_name.clone())
            };
            let z_tab_name = p_tab.borrow().z_name.clone();
            let do_type_check = if b_strict { e_c_type > COLTYPE_ANY } else { affinity > SQLITE_AFF_BLOB };
            if not_null == 0 && !do_type_check {
                continue;
            }

            // Calcula os operandos que o OP_IsType vai precisar
            let mut p4: i32 = SQLITE_NULL;
            let p1: i32;
            let p3: i32;
            if (col_flags & COLFLAG_VIRTUAL) != 0 {
                expr_code_get_column_of_table(v, p_tab, i_data_cur, jj, 3);
                p1 = -1;
                p3 = 3;
            } else {
                if i_dflt != 0 {
                    let mut p_dflt_value: Option<Box<Mem>> = None;
                    {
                        let t = p_tab.borrow();
                        let enc = db.borrow().enc;
                        value_from_expr(db, column_expr(&t, &t.a_col[jj as usize]), enc, affinity, &mut p_dflt_value);
                    }
                    if let Some(ref m) = p_dflt_value {
                        p4 = value_type(m);
                    }
                }
                p1 = i_data_cur;
                if !has_rowid_tab {
                    let pk = primary_key_index(&p_tab.borrow()).expect("sem PK");
                    p3 = table_column_to_index(&pk.borrow(), jj as i16) as i32;
                } else {
                    p3 = table_column_to_storage(&p_tab.borrow(), jj as i16) as i32;
                }
            }

            let label_error = vdbe_make_label(p_parse); // salta aqui para reportar erro
            let label_ok = vdbe_make_label(p_parse); // salta aqui se tudo estiver certo
            if not_null != 0 {
                // (1) colunas NOT NULL não podem conter NULL
                let jmp3: i32;
                let jmp2 = vdbe_add_op4_int(&mut v.borrow_mut(), OP_ISTYPE as i32, p1, label_ok, p3, p4);
                if p1 < 0 {
                    vdbe_change_p5(&mut v.borrow_mut(), 0x0f); // INT, REAL, TEXT ou BLOB
                    jmp3 = jmp2;
                } else {
                    vdbe_change_p5(&mut v.borrow_mut(), 0x0d); // INT, TEXT ou BLOB
                    // O OP_IsType não detecta NaN no arquivo do banco, que deve valer como NULL.
                    // Se o tipo do cabeçalho for REAL, é preciso carregar o dado de verdade com
                    // OP_Column para saber com segurança se o valor é NULL.
                    vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, p1, p3, 3);
                    column_default(&mut v.borrow_mut(), &p_tab.borrow(), jj, 3);
                    jmp3 = vdbe_add_op2(&mut v.borrow_mut(), OP_NOTNULL as i32, 3, label_ok);
                }
                let z_err = m_printf_texts(db, b"NULL value in %s.%s", &[&z_tab_name, &z_cn_name]);
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
                if do_type_check {
                    vdbe_goto(&mut v.borrow_mut(), label_error);
                    vdbe_jump_here(&mut v.borrow_mut(), jmp2);
                    vdbe_jump_here(&mut v.borrow_mut(), jmp3);
                } else {
                    // o bytecode do VDBE segue em frente
                }
            }
            if b_strict && do_type_check {
                // (2) o tipo tem que ser exato nas colunas não ANY de tabelas STRICT
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_ISTYPE as i32, p1, label_ok, p3, p4);
                vdbe_change_p5(&mut v.borrow_mut(), A_STD_TYPE_MASK[e_c_type as usize - 1] as u16);
                let z_err = m_printf_texts(
                    db,
                    b"non-%s value in %s.%s",
                    &[STD_TYPE[e_c_type as usize - 1].as_bytes(), &z_tab_name, &z_cn_name],
                );
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
            } else if !b_strict && affinity == SQLITE_AFF_TEXT {
                // (3) colunas TEXT em tabelas não STRICT: NULL, TEXT ou BLOB
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_ISTYPE as i32, p1, label_ok, p3, p4);
                vdbe_change_p5(&mut v.borrow_mut(), 0x1c); // NULL, TEXT ou BLOB
                let z_err = m_printf_texts(db, b"NUMERIC value in %s.%s", &[&z_tab_name, &z_cn_name]);
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
            } else if !b_strict && affinity >= SQLITE_AFF_NUMERIC {
                // (4) colunas numéricas em tabelas não STRICT não podem ser um TEXT
                //     conversível em número
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_ISTYPE as i32, p1, label_ok, p3, p4);
                vdbe_change_p5(&mut v.borrow_mut(), 0x1b); // NULL, INT, FLOAT ou BLOB
                if p1 >= 0 {
                    expr_code_get_column_of_table(v, p_tab, i_data_cur, jj, 3);
                }
                vdbe_add_op4(
                    &mut v.borrow_mut(),
                    OP_AFFINITY as i32,
                    3,
                    1,
                    0,
                    P4Value::Static(b"C".to_vec()),
                    P4_STATIC as i32,
                );
                vdbe_add_op4_int(&mut v.borrow_mut(), OP_ISTYPE as i32, -1, label_ok, 3, p4);
                vdbe_change_p5(&mut v.borrow_mut(), 0x1c); // NULL, TEXT ou BLOB
                let z_err = m_printf_texts(db, b"TEXT value in %s.%s", &[&z_tab_name, &z_cn_name]);
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
            }
            vdbe_resolve_label(&mut v.borrow_mut(), label_error);
            integrity_check_result_row(&mut v.borrow_mut());
            vdbe_resolve_label(&mut v.borrow_mut(), label_ok);
        }

        // Confere as restrições CHECK
        let ignore_checks = (db.borrow().flags & SQLITE_IGNORECHECKS) != 0;
        let has_check = p_tab.borrow().p_check.is_some();
        if has_check && !ignore_checks {
            let p_check = {
                let t = p_tab.borrow();
                expr_list_dup(db, t.p_check.as_deref().unwrap(), 0)
            };
            if let Some(p_check) = p_check {
                let addr_ck_fault = vdbe_make_label(p_parse);
                let addr_ck_ok = vdbe_make_label(p_parse);
                p_parse.i_self_tab = i_data_cur + 1;
                let mut k = p_check.n_expr - 1;
                while k > 0 {
                    expr_if_false(p_parse, p_check.a[k as usize].p_expr.as_deref().unwrap(), addr_ck_fault, 0);
                    k -= 1;
                }
                expr_if_true(
                    p_parse,
                    p_check.a[0].p_expr.as_deref().unwrap(),
                    addr_ck_ok,
                    SQLITE_JUMPIFNULL as i32,
                );
                vdbe_resolve_label(&mut v.borrow_mut(), addr_ck_fault);
                p_parse.i_self_tab = 0;
                let z_err = {
                    let z_tab_name = p_tab.borrow().z_name.clone();
                    m_printf_texts(db, b"CHECK constraint failed in %s", &[&z_tab_name])
                };
                vdbe_add_op4(&mut v.borrow_mut(), OP_STRING8 as i32, 0, 3, 0, P4Value::Dynamic(z_err), P4_DYNAMIC as i32);
                integrity_check_result_row(&mut v.borrow_mut());
                vdbe_resolve_label(&mut v.borrow_mut(), addr_ck_ok);
            }
        }
        if !is_quick {
            // Omite os demais testes no quick_check.
            // Valida as entradas de índice da linha atual
            let mut j: i32 = 0;
            for p_idx in &indexes {
                let ck_uniq = vdbe_make_label(p_parse);
                if let Some(ref pk) = p_pk {
                    if Rc::ptr_eq(pk, p_idx) {
                        j += 1;
                        continue;
                    }
                }
                let mut jmp3: i32 = 0;
                r1 = generate_index_key(p_parse, p_idx, i_data_cur, 0, 0, Some(&mut jmp3), p_prior.as_ref(), r1);
                p_prior = Some(p_idx.clone());
                vdbe_add_op2(&mut v.borrow_mut(), OP_ADDIMM as i32, 8 + j, 1); // incrementa a contagem de entradas
                // Confere que existe uma entrada de índice para a linha atual da tabela
                let (n_column, n_key_col, z_idx_name) = {
                    let ib = p_idx.borrow();
                    (ib.n_column as i32, ib.n_key_col as i32, ib.z_name.clone())
                };
                let jmp2 = vdbe_add_op4_int(&mut v.borrow_mut(), OP_FOUND as i32, i_idx_cur + j, ck_uniq, r1, n_column);
                vdbe_load_string(&mut v.borrow_mut(), 3, b"row ");
                vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 7, 3, 3);
                vdbe_load_string(&mut v.borrow_mut(), 4, b" missing from index ");
                vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 4, 3, 3);
                let jmp5 = vdbe_load_string(&mut v.borrow_mut(), 4, &z_idx_name);
                vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 4, 3, 3);
                let jmp4 = integrity_check_result_row(&mut v.borrow_mut());
                vdbe_jump_here(&mut v.borrow_mut(), jmp2);

                // O opcode OP_IdxRowid é uma versão otimizada do OP_Column que extrai o rowid
                // do fim do registro do índice, mas só acerta se o registro não tiver bytes
                // sobrando no fim. Confere que é o caso.
                if has_rowid_tab {
                    vdbe_add_op2(&mut v.borrow_mut(), OP_IDXROWID as i32, i_idx_cur + j, 3);
                    let jmp7 = vdbe_add_op3(&mut v.borrow_mut(), OP_EQ as i32, 3, 0, r1 + n_column - 1);
                    vdbe_load_string(&mut v.borrow_mut(), 3, b"rowid not at end-of-record for row ");
                    vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 7, 3, 3);
                    vdbe_load_string(&mut v.borrow_mut(), 4, b" of index ");
                    vdbe_goto(&mut v.borrow_mut(), jmp5 - 1);
                    vdbe_jump_here(&mut v.borrow_mut(), jmp7);
                }

                // Colunas indexadas com colação diferente de BINARY ainda têm que guardar
                // exatamente o mesmo texto da tabela.
                let mut label6: i32 = 0;
                for kk in 0..n_key_col {
                    if p_idx.borrow().az_coll[kk as usize].as_slice() == STR_BINARY {
                        continue;
                    }
                    if label6 == 0 {
                        label6 = vdbe_make_label(p_parse);
                    }
                    vdbe_add_op3(&mut v.borrow_mut(), OP_COLUMN as i32, i_idx_cur + j, kk, 3);
                    vdbe_add_op3(&mut v.borrow_mut(), OP_NE as i32, 3, label6, r1 + kk);
                }
                if label6 != 0 {
                    let jmp6 = vdbe_add_op0(&mut v.borrow_mut(), OP_GOTO as i32);
                    vdbe_resolve_label(&mut v.borrow_mut(), label6);
                    vdbe_load_string(&mut v.borrow_mut(), 3, b"row ");
                    vdbe_add_op3(&mut v.borrow_mut(), OP_CONCAT as i32, 7, 3, 3);
                    vdbe_load_string(&mut v.borrow_mut(), 4, b" values differ from index ");
                    vdbe_goto(&mut v.borrow_mut(), jmp5 - 1);
                    vdbe_jump_here(&mut v.borrow_mut(), jmp6);
                }

                // Em índices UNIQUE, confere que só existe uma entrada com a chave atual. A
                // entrada é única se (1) alguma coluna for NULL ou (2) a próxima entrada tiver
                // chave diferente.
                if is_unique_index(&p_idx.borrow()) {
                    let uniq_ok = vdbe_make_label(p_parse);
                    for kk in 0..n_key_col {
                        let i_col = p_idx.borrow().ai_column[kk as usize];
                        if i_col >= 0 && p_tab.borrow().a_col[i_col as usize].not_null != 0 {
                            continue;
                        }
                        vdbe_add_op2(&mut v.borrow_mut(), OP_ISNULL as i32, r1 + kk, uniq_ok);
                    }
                    let jmp6 = vdbe_add_op1(&mut v.borrow_mut(), OP_NEXT as i32, i_idx_cur + j);
                    vdbe_goto(&mut v.borrow_mut(), uniq_ok);
                    vdbe_jump_here(&mut v.borrow_mut(), jmp6);
                    vdbe_add_op4_int(&mut v.borrow_mut(), OP_IDXGT as i32, i_idx_cur + j, uniq_ok, r1, n_key_col);
                    vdbe_load_string(&mut v.borrow_mut(), 3, b"non-unique entry in index ");
                    vdbe_goto(&mut v.borrow_mut(), jmp5);
                    vdbe_resolve_label(&mut v.borrow_mut(), uniq_ok);
                }
                vdbe_jump_here(&mut v.borrow_mut(), jmp4);
                resolve_part_idx_label(p_parse, jmp3);
                j += 1;
            }
        }

        // Rabo do laço por tabela (começo do chunk 006 do C)
        vdbe_add_op2(&mut v.borrow_mut(), OP_NEXT as i32, i_data_cur, loop_top);
        vdbe_jump_here(&mut v.borrow_mut(), loop_top - 1);
        if let Some(ref pk) = p_pk {
            let n_key = pk.borrow().n_key_col as i32;
            release_temp_range(p_parse, r2, n_key);
        }
    }
}


// ---- part_006.rs ----

/// Segunda passada do integrity_check: invoca o método xIntegrity em todas as tabelas
/// virtuais do banco `i` (SQLITE_OMIT_VIRTUALTABLE não está definido).
pub fn integrity_check_virtual_tables(
    p_parse: &mut Parse,
    v: &VdbeRef,
    db: &Sqlite3Ref,
    st: &IntegrityCheck,
    i: i32,
    tables: &[TableRef],
) {
    for p_tab in tables {
        if let Some(ref obj) = st.p_obj_tab {
            if !Rc::ptr_eq(obj, p_tab) {
                continue;
            }
        }
        if is_ordinary_table(&p_tab.borrow()) {
            continue;
        }
        if !is_virtual(&p_tab.borrow()) {
            continue;
        }
        if p_tab.borrow().n_col <= 0 {
            let z_mod = p_tab.borrow().u.vtab.az_arg[0].clone();
            if hash_find(&db.borrow().a_module, &z_mod).is_none() {
                continue;
            }
        }
        view_get_column_names(p_parse, p_tab);
        let p_vtab_ref = match p_tab.borrow().u.vtab.p.clone() {
            None => continue,
            Some(vt) => vt,
        };
        let p_vtab = match p_vtab_ref.borrow().p_vtab.clone() {
            None => continue,
            Some(pv) => pv,
        };
        let p_module = match p_vtab.borrow().p_module.clone() {
            None => continue,
            Some(m) => m,
        };
        if p_module.i_version < 4 {
            continue;
        }
        if p_module.x_integrity.is_none() {
            continue;
        }
        vdbe_add_op3(&mut v.borrow_mut(), OP_VCHECK as i32, i, 3, if st.is_quick { 1 } else { 0 });
        p_tab.borrow_mut().n_tab_ref += 1;
        vdbe_append_p4(&mut v.borrow_mut(), P4Value::Table(p_tab.clone()), P4_TABLEREF as i32);
        let a1 = vdbe_add_op1(&mut v.borrow_mut(), OP_ISNULL as i32, 3);
        integrity_check_result_row(&mut v.borrow_mut());
        vdbe_jump_here(&mut v.borrow_mut(), a1);
    }
}

/// Código final do `PRAGMA integrity_check`: decrementa o contador de erros e devolve
/// "ok" ou a mensagem de SQLITE_CORRUPT.
pub fn integrity_check_end_code(v: &VdbeRef, mx_err: i32) {
    let end_code: [VdbeOpList; 7] = [
        VdbeOpList { opcode: OP_ADDIMM, p1: 1, p2: 0, p3: 0 },     // 0
        VdbeOpList { opcode: OP_IFNOTZERO, p1: 1, p2: 4, p3: 0 },  // 1
        VdbeOpList { opcode: OP_STRING8, p1: 0, p2: 3, p3: 0 },    // 2
        VdbeOpList { opcode: OP_RESULTROW, p1: 3, p2: 1, p3: 0 },  // 3
        VdbeOpList { opcode: OP_HALT, p1: 0, p2: 0, p3: 0 },       // 4
        VdbeOpList { opcode: OP_STRING8, p1: 0, p2: 3, p3: 0 },    // 5
        VdbeOpList { opcode: OP_GOTO, p1: 0, p2: 3, p3: 0 },       // 6
    ];
    let mut vm = v.borrow_mut();
    // `iLn = VDBE_OFFSET_LINENO(2)` só tem efeito com SQLITE_ENABLE_EXPLAIN_COMMENTS.
    if let Some(first) = vdbe_add_op_list(&mut vm, end_code.len() as i32, &end_code, 0) {
        vm.a_op[first].p2 = 1 - mx_err;
        vm.a_op[first + 2].p4type = P4_STATIC;
        vm.a_op[first + 2].p4 = P4Value::Static(b"ok".to_vec());
        vm.a_op[first + 5].p4type = P4_STATIC;
        vm.a_op[first + 5].p4 = P4Value::Static(err_str(SQLITE_CORRUPT).to_vec());
    }
    let addr = vdbe_current_addr(&vm) - 2;
    vdbe_change_p3(&mut vm, 0, addr);
}

#[cfg(not(feature = "SQLITE_OMIT_UTF16"))]
/// PRAGMA encoding
/// PRAGMA encoding = "utf-8"|"utf-16"|"utf-16le"|"utf-16be"
///
/// Na primeira forma, este pragma retorna a codificação do banco de dados principal.
/// Se o banco de dados não foi inicializado, ele será inicializado agora.
///
/// A segunda forma deste pragma é uma operação sem efeito se o arquivo do banco de
/// dados principal não foi inicializado. Neste caso, ela define a codificação padrão
/// que será usada para o arquivo do banco de dados principal se um novo arquivo
/// for criado. Se um arquivo de banco de dados principal existente for aberto,
/// a codificação de texto padrão para o banco de dados existente será usada.
fn pragma_encoding(
    p_parse: &mut Parse,
    p_pragma: &PragmaName,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    struct EncName {
        z_name: &'static str,
        enc: u8,
    }

    let enc_names = [
        EncName { z_name: "UTF8", enc: SQLITE_UTF8 as u8 },
        EncName { z_name: "UTF-8", enc: SQLITE_UTF8 as u8 },    // Deve ser elemento [1]
        EncName { z_name: "UTF-16le", enc: SQLITE_UTF16LE as u8 },  // Deve ser elemento [2]
        EncName { z_name: "UTF-16be", enc: SQLITE_UTF16BE as u8 },  // Deve ser elemento [3]
        EncName { z_name: "UTF16le", enc: SQLITE_UTF16LE as u8 },
        EncName { z_name: "UTF16be", enc: SQLITE_UTF16BE as u8 },
        EncName { z_name: "UTF-16", enc: 0 },                   // SQLITE_UTF16NATIVE
        EncName { z_name: "UTF16", enc: 0 },                    // SQLITE_UTF16NATIVE
    ];

    let db = &mut p_parse.p_db;

    if z_right.is_none() {
        // "PRAGMA encoding"
        if read_schema(p_parse) {
            return;
        }

        return_single_text(v, Some(enc_names[(*db).enc() as usize].z_name.as_bytes()));
    } else {
        // "PRAGMA encoding = XXX"
        // Muda o valor de sqlite.enc apenas se o handle do banco de dados
        // não estiver inicializado. Se o banco de dados principal existir,
        // o novo valor de sqlite.enc será sobrescrito quando o esquema for
        // carregado novamente. Se ele não existir, será criado usando a
        // nova codificação.
        if (db.m_db_flags & DBFLAG_ENCODING_FIXED) == 0 {
            for enc_name in &enc_names {
                if str_i_cmp(z_right.unwrap_or(&[]), enc_name.z_name.as_bytes()) == 0 {
                    let enc = if enc_name.enc != 0 {
                        enc_name.enc
                    } else {
                        SQLITE_UTF16NATIVE as u8
                    };
                    db.set_enc(enc as i32);
                    set_text_encoding(db, enc as i32);
                    break;
                }
            }

            let mut found = false;
            for enc_name in &enc_names {
                if str_i_cmp(z_right.unwrap_or(&[]), enc_name.z_name.as_bytes()) == 0 {
                    found = true;
                    break;
                }
            }

            if !found {
                error_msg(
                    p_parse,
                    b"unsupported encoding: %s",
                    &[PrintfArg::Text(z_right.unwrap_or(&[]).to_vec())],
                );
            }
        }
    }
}

#[cfg(not(feature = "SQLITE_OMIT_SCHEMA_VERSION_PRAGMAS"))]
/// PRAGMA [schema.]schema_version
/// PRAGMA [schema.]schema_version = <integer>
/// PRAGMA [schema.]user_version = <integer>
/// PRAGMA [schema.]freelist_count
/// PRAGMA [schema.]data_version
/// PRAGMA [schema.]application_id = <integer>
fn pragma_header_value(
    p_parse: &mut Parse,
    p_pragma: &PragmaName,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
    i_db: i32,
) {
    let i_cookie = p_pragma.i_arg;

    vdbe_uses_btree(v, i_db);

    if z_right.is_some() && (p_pragma.m_prag_flg & PRAGFLG_READONLY) == 0 {
        // Escreve o valor do cookie especificado
        let set_cookie: [VdbeOpList; 2] = [
            VdbeOpList { opcode: OP_TRANSACTION, p1: 0, p2: 1, p3: 0 }, // 0
            VdbeOpList { opcode: OP_SETCOOKIE, p1: 0, p2: 0, p3: 0 },   // 1
        ];

        // `sqlite3VdbeVerifyNoMallocRequired` é macro vazia fora de SQLITE_DEBUG.
        if let Some(first) = vdbe_add_op_list(v, set_cookie.len() as i32, &set_cookie, 0) {
            v.a_op[first].p1 = i_db;
            v.a_op[first + 1].p1 = i_db;
            v.a_op[first + 1].p2 = i_cookie;
            v.a_op[first + 1].p3 = atoi(z_right.unwrap_or(&[]));
            v.a_op[first + 1].p5 = 1;

            let defensive = {
                let db = p_parse.db.upgrade().expect("db ausente");
                let flags = db.borrow().flags;
                (flags & SQLITE_DEFENSIVE) != 0
            };
            if i_cookie == (BTREE_SCHEMA_VERSION as i32) && defensive {
                // Não permite o uso de PRAGMA schema_version=VALUE em modo defensivo.
                // Muda a instrução OP_SetCookie para uma operação sem efeito.
                v.a_op[first + 1].opcode = OP_NOOP;
            }
        }
    } else {
        // Lê o valor do cookie especificado
        let read_cookie: [VdbeOpList; 3] = [
            VdbeOpList { opcode: OP_TRANSACTION, p1: 0, p2: 0, p3: 0 }, // 0
            VdbeOpList { opcode: OP_READCOOKIE, p1: 0, p2: 1, p3: 0 },  // 1
            VdbeOpList { opcode: OP_RESULTROW, p1: 1, p2: 1, p3: 0 },   // 2
        ];

        if let Some(first) = vdbe_add_op_list(v, read_cookie.len() as i32, &read_cookie, 0) {
            v.a_op[first].p1 = i_db;
            v.a_op[first + 1].p1 = i_db;
            v.a_op[first + 1].p3 = i_cookie;
            vdbe_reusable(v);
        }
    }
}

#[cfg(not(feature = "SQLITE_OMIT_COMPILEOPTION_DIAGS"))]
/// PRAGMA compile_options
/// Retorna os nomes de todas as opções de compilação usadas nesta build,
/// uma opção por linha.
fn pragma_compile_options(
    p_parse: &mut Parse,
    v: &mut Vdbe,
) {
    p_parse.n_mem = 1;
    let mut i = 0;

    loop {
        let z_opt = compileoption_get(i);
        if z_opt.is_none() {
            break;
        }

        vdbe_load_string(v, 1, z_opt.unwrap().as_bytes());
        vdbe_add_op2(v, OP_RESULTROW as i32, 1, 1);
        i += 1;
    }

    vdbe_reusable(v);
}

#[cfg(not(feature = "SQLITE_OMIT_WAL"))]
/// PRAGMA [schema.]wal_checkpoint = passive|full|restart|truncate
/// Ponto de verificação do banco de dados.
fn pragma_wal_checkpoint(
    p_parse: &mut Parse,
    p_id2: Option<&Token>,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
    i_db: i32,
) {
    let i_bt = if p_id2.is_some() && p_id2.unwrap().n > 0 {
        i_db
    } else {
        SQLITE_MAX_DB
    };

    let mut e_mode = SQLITE_CHECKPOINT_PASSIVE;

    if let Some(z) = z_right {
        if str_i_cmp(z, b"full") == 0 {
            e_mode = SQLITE_CHECKPOINT_FULL;
        } else if str_i_cmp(z, b"restart") == 0 {
            e_mode = SQLITE_CHECKPOINT_RESTART;
        } else if str_i_cmp(z, b"truncate") == 0 {
            e_mode = SQLITE_CHECKPOINT_TRUNCATE;
        }
    }

    p_parse.n_mem = 3;
    vdbe_add_op3(v, OP_CHECKPOINT as i32, i_bt, e_mode, 1);
    vdbe_add_op2(v, OP_RESULTROW as i32, 1, 3);
}

#[cfg(not(feature = "SQLITE_OMIT_WAL"))]
/// PRAGMA wal_autocheckpoint
/// PRAGMA wal_autocheckpoint = N
///
/// Configura uma conexão de banco de dados para fazer ponto de verificação
/// automaticamente após acumular N quadros no registro. Ou consulta o valor
/// atual de N.
fn pragma_wal_autocheckpoint(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    let db = &mut p_parse.p_db;

    if let Some(z) = z_right {
        sqlite3_wal_autocheckpoint(db, atoi(z));
    }

    let val = if db.x_wal_callback == Some(sqlite3_wal_default_hook) {
        if let Some(arg) = db.p_wal_arg {
            arg as i64
        } else {
            0
        }
    } else {
        0
    };

    return_single_int(v, val);
}

/// PRAGMA shrink_memory
///
/// Este pragma faz com que a conexão do banco de dados sobre a qual é
/// invocado libere o máximo de memória possível, chamando
/// sqlite3_db_release_memory().
fn pragma_shrink_memory(
    p_parse: &mut Parse,
) {
    sqlite3_db_release_memory(&mut p_parse.p_db);
}

/// PRAGMA optimize
/// PRAGMA optimize(MASK)
/// PRAGMA schema.optimize
/// PRAGMA schema.optimize(MASK)
///
/// Tenta otimizar o banco de dados. Todos os esquemas são otimizados nas
/// duas primeiras formas, e apenas o esquema especificado é otimizado nas
/// duas últimas formas.
fn pragma_optimize(
    p_parse: &mut Parse,
    p_pragma: &PragmaName,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
    z_db: Option<&str>,
    i_db: i32,
) {
    let db = &mut p_parse.p_db;
    let mut op_mask: u32 = 0xfffe;
    let mut n_limit: i32 = 0;

    if let Some(z) = z_right {
        op_mask = atoi(z) as u32;
        if (op_mask & 0x02) == 0 {
            return;
        }
    }

    if (op_mask & 0x10) == 0 {
        n_limit = 0;
    } else if db.n_analysis_limit > 0 && db.n_analysis_limit < SQLITE_DEFAULT_OPTIMIZE_LIMIT {
        n_limit = 0;
    } else {
        n_limit = SQLITE_DEFAULT_OPTIMIZE_LIMIT;
    }

    let i_tab_cur = p_parse.n_tab;
    p_parse.n_tab = p_parse.n_tab.wrapping_add(1);

    let i_db_last = if z_db.is_some() { i_db } else { db.n_db - 1 };
    let mut current_db = i_db;

    while current_db <= i_db_last {
        if current_db == 1 {
            current_db += 1;
            continue;
        }

        code_verify_schema(p_parse, current_db);
        let p_schema = db.a_db[current_db as usize].p_schema;

        if let Some(schema) = p_schema {
            let mut k = hash_first(&schema.tbl_hash);

            while let Some(elem_ref) = k {
                let p_tab = match hash_data(&elem_ref).downcast_ref::<Table>() {
                    Some(t) => t,
                    None => {
                        k = hash_next(&elem_ref);
                        continue;
                    }
                };

                // Isto só funciona para tabelas ordinárias
                if !is_ordinary_table(p_tab) {
                    k = hash_next(&elem_ref);
                    continue;
                }

                // Não varre tabelas de sistema
                if str_ni_cmp(p_tab.z_name.as_bytes(), b"sqlite_", 7) == 0 {
                    k = hash_next(&elem_ref);
                    continue;
                }

                let mut sz_threshold = p_tab.n_row_log_est;
                let mut n_index = 0;

                let mut p_idx = p_tab.p_index;
                while let Some(idx) = p_idx {
                    n_index += 1;
                    if !idx.has_stat1 {
                        sz_threshold = -1;  // Sempre analisa se algum índice não tem estatísticas
                    }
                    p_idx = idx.p_next.clone();
                }

                // Se a tabela pTab não foi usada de uma forma que se beneficiaria
                // de ter estatísticas de análise durante a sessão atual, então
                // pule-a, a menos que o bit de máscara 0x10000 esteja definido.
                if (p_tab.tab_flags & TF_MAYBE_REANALYZE) != 0 {
                    // Verifica a mudança de tamanho se stat1 foi usado em uma consulta
                } else if (op_mask & 0x10000) != 0 {
                    // Verifica a mudança de tamanho se 0x10000 está definido
                } else if p_tab.p_index.is_some() && sz_threshold < 0 {
                    // Faz análise se índices não analisados existem
                } else {
                    // Caso contrário, pode pular esta tabela
                    k = hash_next(&elem_ref);
                    continue;
                }

                let n_check_outer = 1;
                if n_check_outer == 2 {
                    // Se ANALYZE pode ser invocado duas ou mais vezes, mantenha
                    // uma transação de escrita para eficiência
                    begin_write_operation(p_parse, 0, current_db);
                }

                open_table(p_parse, i_tab_cur, current_db, p_tab, OP_OPENREAD as i32);

                if sz_threshold >= 0 {
                    let i_range: i32 = 33;  // Mudança de tamanho 10x
                    let sz_val_low = if sz_threshold >= i_range {
                        sz_threshold - i_range
                    } else {
                        -1
                    };
                    let sz_val_high = sz_threshold + i_range;

                    vdbe_add_op4_int(
                        v,
                        OP_IFSIZEBETWEEN as i32,
                        i_tab_cur,
                        vdbe_current_addr(v) + 2 + ((op_mask & 1) as i32),
                        sz_val_low,
                        sz_val_high,
                    );
                } else {
                    vdbe_add_op2(
                        v,
                        OP_REWIND as i32,
                        i_tab_cur,
                        vdbe_current_addr(v) + 2 + ((op_mask & 1) as i32),
                    );
                }

                let z_sub_sql = m_printf(db, b"ANALYZE \"%w\".\"%w\"", db.a_db[current_db as usize].z_db_s_name.as_bytes(), p_tab.z_name.as_bytes());

                if (op_mask & 0x01) != 0 {
                    let r1 = get_temp_reg(p_parse);
                    vdbe_add_op4(
                        v,
                        OP_STRING8 as i32,
                        0,
                        r1,
                        0,
                        z_sub_sql.as_ref(),
                        P4_DYNAMIC,
                    );
                    vdbe_add_op2(v, OP_RESULTROW as i32, r1, 1);
                } else {
                    vdbe_add_op4(
                        v,
                        OP_SQLEXEC as i32,
                        if n_limit != 0 { 0x02 } else { 0 },
                        n_limit,
                        0,
                        z_sub_sql.as_ref(),
                        P4_DYNAMIC,
                    );
                }

                k = hash_next(&elem_ref);
            }
        }

        current_db += 1;
    }

    vdbe_add_op0(v, OP_EXPIRE as i32);

    // Em um esquema com muitas tabelas e índices, reduza a análise_limit
    // para evitar tempo de execução em excesso no pior caso.
    if !db.malloc_failed && n_limit > 0 {
        let mut n_btree: i32 = 0;
        current_db = i_db;
        while current_db <= i_db_last {
            if current_db == 1 {
                current_db += 1;
                continue;
            }

            if let Some(schema) = db.a_db[current_db as usize].p_schema {
                let mut k = hash_first(&schema.tbl_hash);
                while let Some(elem_ref) = k {
                    if let Some(p_tab) = hash_data(&elem_ref).downcast_ref::<Table>() {
                        if is_ordinary_table(p_tab) && str_ni_cmp(p_tab.z_name.as_bytes(), b"sqlite_", 7) != 0 {
                            let mut n_index = 0;
                            let mut p_idx = p_tab.p_index;
                            while p_idx.is_some() {
                                n_index += 1;
                                if let Some(idx) = &p_idx {
                                    p_idx = idx.p_next.clone();
                                }
                            }
                            n_btree += n_index + 1;
                        }
                    }
                    k = hash_next(&elem_ref);
                }
            }

            current_db += 1;
        }

        if n_btree > 100 {
            let n_limit_scaled = 100 * n_limit / n_btree;
            let n_limit_final = if n_limit_scaled < 100 { 100 } else { n_limit_scaled };

            let i_end = vdbe_current_addr(v);
            let mut i_addr = 0;

            while i_addr < i_end {
                let a_op = vdbe_get_op(v, i_addr);
                if a_op.opcode == OP_SQLEXEC {
                    a_op.p2 = n_limit_final;
                }
                i_addr += 1;
            }
        }
    }
}

/// PRAGMA busy_timeout
/// PRAGMA busy_timeout = N
///
/// Chama sqlite3_busy_timeout(db, N). Retorna o valor de timeout atual
/// se um for definido. Se nenhum manipulador ocupado ou um manipulador
/// ocupado diferente estiver definido, 0 será retornado. Definir o
/// busy_timeout para 0 ou negativo desativa o timeout.
fn pragma_busy_timeout(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    let db = &mut p_parse.p_db;

    if let Some(z) = z_right {
        sqlite3_busy_timeout(db, atoi(z));
    }

    return_single_int(v, db.busy_timeout as i64);
}

/// PRAGMA soft_heap_limit
/// PRAGMA soft_heap_limit = N
///
/// Este pragma invoca a interface sqlite3_soft_heap_limit64() com o
/// argumento N, se N for especificado e for um inteiro não negativo.
/// O pragma soft_heap_limit sempre retorna o mesmo inteiro que seria
/// retornado pela função C sqlite3_soft_heap_limit64(-1).
fn pragma_soft_heap_limit(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    if let Some(z) = z_right {
        if let Ok(n) = dec_or_hex_to_i64(z) {
            sqlite3_soft_heap_limit64(n);
        }
    }

    return_single_int(v, sqlite3_soft_heap_limit64(-1));
}

/// PRAGMA hard_heap_limit
/// PRAGMA hard_heap_limit = N
///
/// Invoca sqlite3_hard_heap_limit64() para consultar ou definir o limite
/// de heap difícil. O limite de heap difícil pode ser ativado ou reduzido
/// por este pragma, mas não elevado ou desativado. Apenas a API C
/// sqlite3_hard_heap_limit64() pode elevar ou desativar o limite de heap
/// difícil. Isto permite que uma aplicação defina uma restrição de limite
/// de heap que não pode ser relaxada por um script SQL não confiável.
fn pragma_hard_heap_limit(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    if let Some(z) = z_right {
        if let Ok(n) = dec_or_hex_to_i64(z) {
            let i_prior = sqlite3_hard_heap_limit64(-1);
            if n > 0 && (i_prior == 0 || i_prior > n) {
                sqlite3_hard_heap_limit64(n);
            }
        }
    }

    return_single_int(v, sqlite3_hard_heap_limit64(-1));
}

/// PRAGMA threads
/// PRAGMA threads = N
///
/// Configura o número máximo de threads do trabalhador. Retorna o novo
/// máximo, que pode ser menor que o solicitado.
fn pragma_threads(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    let db = &mut p_parse.p_db;

    if let Some(z) = z_right {
        if let Ok(n) = dec_or_hex_to_i64(z) {
            if n >= 0 {
                sqlite3_limit(db, SQLITE_LIMIT_WORKER_THREADS, (n & 0x7fffffff) as i32);
            }
        }
    }

    return_single_int(v, sqlite3_limit(db, SQLITE_LIMIT_WORKER_THREADS, -1) as i64);
}

/// PRAGMA analysis_limit
/// PRAGMA analysis_limit = N
///
/// Configura o número máximo de linhas que ANALYZE examinará em cada
/// índice que ele procura. Retorna o novo limite.
fn pragma_analysis_limit(
    p_parse: &mut Parse,
    v: &mut Vdbe,
    z_right: Option<&[u8]>,
) {
    let db = &mut p_parse.p_db;

    if let Some(z) = z_right {
        if let Ok(n) = dec_or_hex_to_i64(z) {
            if n >= 0 {
                db.n_analysis_limit = (n & 0x7fffffff) as i32;
            }
        }
    }

    return_single_int(v, db.n_analysis_limit as i64);
}

#[cfg(any(feature = "SQLITE_DEBUG", feature = "SQLITE_TEST"))]
/// Relata o estado atual dos bloqueios de arquivo para todos os bancos de dados
fn pragma_lock_status(
    p_parse: &mut Parse,
    v: &mut Vdbe,
) {
    const AZ_LOCK_NAME: &[&str] = &["unlocked", "shared", "reserved", "pending", "exclusive"];

    let db = &mut p_parse.p_db;
    p_parse.n_mem = 2;

    for i in 0..db.n_db {
        let z_state = if db.a_db[i].z_db_s_name.is_empty() {
            continue;
        } else if let Some(ref p_bt) = db.a_db[i].p_bt {
            if sqlite3_btree_pager(p_bt).is_none() {
                "closed"
            } else {
                let mut j: i32 = 0;
                if sqlite3_file_control(
                    db,
                    if i == 0 { None } else { Some(&db.a_db[i].z_db_s_name) },
                    SQLITE_FCNTL_LOCKSTATE,
                    &mut j,
                ) == SQLITE_OK
                {
                    AZ_LOCK_NAME[j as usize]
                } else {
                    "unknown"
                }
            }
        } else {
            "closed"
        };

        vdbe_multi_load(v, 1, b"ss", db.a_db[i].z_db_s_name.as_bytes(), z_state.as_bytes());
    }
}

#[cfg(feature = "SQLITE_ENABLE_CEROD")]
/// Ativa extensões CEROD
fn pragma_activate_extensions(
    z_right: Option<&[u8]>,
) {
    if let Some(z) = z_right {
        if str_ni_cmp(z, b"cerod-", 6) == 0 {
            sqlite3_activate_cerod(&z[6..]);
        }
    }
}


// ---- part_007.rs ----

/// Estrutura de instância de tabela virtual para pragma epônimo.
pub struct PragmaVtab {
    /// Classe base, deve ser primeiro.
    pub base: Sqlite3Vtab,
    /// Conexão de banco de dados à qual ela pertence.
    pub db: Option<Sqlite3Ref>,
    /// Nome do pragma.
    pub p_name: Option<&'static PragmaName>,
    /// Número de colunas ocultas.
    pub n_hidden: u8,
    /// Índice da primeira coluna oculta.
    pub i_hidden: u8,
}

/// Estrutura de cursor de tabela virtual para pragma epônimo.
pub struct PragmaVtabCursor {
    /// Classe base, deve ser primeiro.
    pub base: Sqlite3VtabCursor,
    /// Instrução pragma a executar.
    pub p_pragma: Option<Box<Sqlite3Stmt>>,
    /// Identificador da linha atual.
    pub i_rowid: i64,
    /// Valores do argumento e do esquema.
    pub az_arg: [Option<Vec<u8>>; 2],
}

/// Método xConnect do módulo de tabela virtual pragma.
pub fn pragma_vtab_connect(
    db: Option<Sqlite3Ref>,
    p_aux: &'static PragmaName,
    _argc: i32,
    _argv: &[Vec<u8>],
    pp_vtab: &mut Option<Box<Sqlite3Vtab>>,
    pz_err: &mut Option<Vec<u8>>,
) -> i32 {
    // O `void *pAux` do C é o `PragmaName` registrado com o módulo (sem ponteiro bruto).
    let p_pragma = p_aux;

    let mut acc = StrAccum::default();
    let mut z_buf = [0u8; 200];
    let mut c_sep = b'(';

    str_accum_init(&mut acc, db.clone(), Some(&mut z_buf), 200, 0);
    str_appendall(&mut acc, b"CREATE TABLE x");

    let mut i = 0usize;
    let mut j = p_pragma.i_prag_c_name as usize;
    while i < p_pragma.n_prag_c_name as usize {
        str_appendf(&mut acc, b"%c\"%s\"", &[PrintfArg::Char(c_sep), PrintfArg::Bytes(PRAG_C_NAME[j].as_bytes())]);
        c_sep = b',';
        i += 1;
        j += 1;
    }

    if i == 0 {
        str_appendf(&mut acc, b"(\"%s\"", &[PrintfArg::Bytes(p_pragma.z_name.as_bytes())]);
        i += 1;
    }

    let mut j = 0u8;
    if (p_pragma.m_prag_flg & PRAG_FLG_RESULT1) != 0 {
        str_appendall(&mut acc, b",arg HIDDEN");
        j += 1;
    }
    if (p_pragma.m_prag_flg & (PRAG_FLG_SCHEMA_OPT | PRAG_FLG_SCHEMA_REQ)) != 0 {
        str_appendall(&mut acc, b",schema HIDDEN");
        j += 1;
    }

    str_append(&mut acc, b")", 1);
    str_accum_finish(&mut acc);

    debug_assert!(z_buf.len() < 200);

    let mut rc = sqlite3_declare_vtab(db.as_ref(), &z_buf);
    if rc == SQLITE_OK {
        let mut p_tab = PragmaVtab {
            base: Sqlite3Vtab {
                p_module: None,
                n_ref: 0,
                z_err_msg: None,
            },
            db: db.clone(),
            p_name: Some(p_pragma),
            i_hidden: i as u8,
            n_hidden: j,
        };

        let mut p_vtab = Box::new(p_tab.base);
        *pp_vtab = Some(p_vtab);
    } else if let Some(db_ref) = db {
        let db_borrow = db_ref.borrow();
        *pz_err = sqlite3_mprintf(b"%s", sqlite3_errmsg(&db_borrow));
    }

    rc
}

/// Método xDisconnect do módulo de tabela virtual pragma.
pub fn pragma_vtab_disconnect(_p_vtab: &mut Sqlite3Vtab) -> i32 {
    SQLITE_OK
}

/// Descobre o melhor índice para procurar uma tabela virtual pragma.
///
/// Não há realmente escolhas de índice. Mas queremos encorajar o planejador
/// de consultas a fornecer restrições == o máximo possível, especialmente
/// no primeiro parâmetro oculto. Então retornamos um custo alto se parâmetros
/// ocultos não forem restritos.
pub fn pragma_vtab_best_index(
    _tab: &mut Sqlite3Vtab,
    p_idx_info: &mut Sqlite3IndexInfo,
) -> i32 {
    let p_tab = _tab;
    let mut seen = [0i32; 2];

    p_idx_info.estimated_cost = 1.0;
    if p_tab.n_hidden == 0 {
        return SQLITE_OK;
    }

    if let Some(ref constraints) = p_idx_info.a_constraint {
        for i in 0..p_idx_info.n_constraint as usize {
            let p_constraint = &constraints[i];
            if p_constraint.i_column < p_tab.i_hidden as i32 {
                continue;
            }
            if p_constraint.op != SQLITE_INDEX_CONSTRAINT_EQ as u8 {
                continue;
            }
            if p_constraint.usable == 0 {
                return SQLITE_CONSTRAINT;
            }

            let j = (p_constraint.i_column - p_tab.i_hidden as i32) as usize;
            debug_assert!(j < 2);
            seen[j] = (i + 1) as i32;
        }
    }

    if seen[0] == 0 {
        p_idx_info.estimated_cost = 2147483647.0;
        p_idx_info.estimated_rows = 2147483647;
        return SQLITE_OK;
    }

    let j = (seen[0] - 1) as usize;
    if let Some(ref mut usage) = p_idx_info.a_constraint_usage {
        usage[j].argv_index = 1;
        usage[j].omit = 1;
    }

    p_idx_info.estimated_cost = 20.0;
    p_idx_info.estimated_rows = 20;

    if seen[1] != 0 {
        let j = (seen[1] - 1) as usize;
        if let Some(ref mut usage) = p_idx_info.a_constraint_usage {
            usage[j].argv_index = 2;
            usage[j].omit = 1;
        }
    }

    SQLITE_OK
}

/// Cria um novo cursor para a tabela virtual pragma.
pub fn pragma_vtab_open(
    _p_vtab: &mut Sqlite3Vtab,
    pp_cursor: &mut Option<Box<Sqlite3VtabCursor>>,
) -> i32 {
    let p_csr = PragmaVtabCursor {
        base: Sqlite3VtabCursor {
            p_vtab: None,
        },
        p_pragma: None,
        i_rowid: 0,
        az_arg: [None, None],
    };

    let mut cursor = Box::new(p_csr.base);
    *pp_cursor = Some(cursor);
    SQLITE_OK
}

/// Limpa todo o conteúdo do cursor da tabela virtual pragma.
pub fn pragma_vtab_cursor_clear(p_csr: &mut PragmaVtabCursor) {
    if let Some(p_pragma) = p_csr.p_pragma.take() {
        sqlite3_finalize(&*p_pragma);
    }
    p_csr.i_rowid = 0;
    for i in 0..2 {
        p_csr.az_arg[i] = None;
    }
}

/// Fecha um cursor da tabela virtual pragma.
pub fn pragma_vtab_close(cur: &mut Sqlite3VtabCursor) -> i32 {
    // Aqui precisamos downcast para PragmaVtabCursor
    // Como isso será chamado através da vtable, precisamos de um mecanismo
    // O código não pode ser traduzido literalmente aqui
    SQLITE_OK
}

/// Avança o cursor da tabela virtual pragma para a próxima linha.
pub fn pragma_vtab_next(p_vtab_cursor: &mut Sqlite3VtabCursor) -> i32 {
    // Downcast necessário aqui também
    let mut rc = SQLITE_OK;
    SQLITE_OK
}

/// Método xFilter do módulo de tabela virtual pragma.
pub fn pragma_vtab_filter(
    p_vtab_cursor: &mut Sqlite3VtabCursor,
    _idx_num: i32,
    _idx_str: Option<&[u8]>,
    argc: i32,
    _argv: *const Option<Sqlite3Value>,
) -> i32 {
    // Downcast necessário
    let mut rc = SQLITE_OK;

    // Implementação incompleta, necessária estrutura adequada para downcast
    SQLITE_OK
}

/// Método xEof do módulo de tabela virtual pragma.
pub fn pragma_vtab_eof(p_vtab_cursor: &mut Sqlite3VtabCursor) -> i32 {
    // Downcast necessário
    0
}

/// Método xColumn da tabela virtual pragma que retorna a coluna correspondente do PRAGMA.
pub fn pragma_vtab_column(
    p_vtab_cursor: &mut Sqlite3VtabCursor,
    _ctx: &mut Sqlite3Context,
    _i: i32,
) -> i32 {
    // Downcast necessário
    SQLITE_OK
}

/// Método xRowid do módulo de tabela virtual pragma.
pub fn pragma_vtab_rowid(
    p_vtab_cursor: &mut Sqlite3VtabCursor,
    p: &mut i64,
) -> i32 {
    // Downcast necessário
    SQLITE_OK
}

/// Objeto de módulo de tabela virtual pragma.
pub static PRAGMA_VTAB_MODULE: Sqlite3Module = Sqlite3Module {
    i_version: 0,
    x_create: None,
    x_connect: Some(pragma_vtab_connect as fn(Option<Sqlite3Ref>, *mut std::ffi::c_void, i32, *const *const u8, &mut Option<Box<Sqlite3Vtab>>, &mut Option<Vec<u8>>) -> i32),
    x_best_index: Some(pragma_vtab_best_index as fn(&mut Sqlite3Vtab, &mut Sqlite3IndexInfo) -> i32),
    x_disconnect: Some(pragma_vtab_disconnect as fn(&mut Sqlite3Vtab) -> i32),
    x_destroy: None,
    x_open: Some(pragma_vtab_open as fn(&mut Sqlite3Vtab, &mut Option<Box<Sqlite3VtabCursor>>) -> i32),
    x_close: None,
    x_filter: None,
    x_next: None,
    x_eof: None,
    x_column: None,
    x_rowid: None,
    x_update: None,
    x_begin: None,
    x_sync: None,
    x_commit: None,
    x_rollback: None,
    x_find_function: None,
    x_rename: None,
    x_savepoint: None,
    x_release: None,
    x_rollback_to: None,
    x_shadow_name: None,
    x_integrity: None,
};

/// Verifica se zTabName é realmente o nome de um pragma. Se for,
/// registra uma tabela virtual epônima para esse pragma e retorna
/// um ponteiro para o objeto Module da nova tabela virtual.
pub fn sqlite3_pragma_vtab_register(
    db: Option<Sqlite3Ref>,
    z_name: &[u8],
) -> Option<Box<Sqlite3Module>> {
    debug_assert!(sqlite3_strnicmp(z_name, b"pragma_", 7) == 0);

    if let Some(db_ref) = db.as_ref() {
        let p_name = pragma_locate(&z_name[7..]);
        if p_name.is_none() {
            return None;
        }
        let p_name = p_name.unwrap();

        if (p_name.m_prag_flg & (PRAG_FLG_RESULT0 | PRAG_FLG_RESULT1)) == 0 {
            return None;
        }

        // Registro do módulo no banco de dados
        sqlite3_vtab_create_module(db_ref, z_name, &PRAGMA_VTAB_MODULE, p_name as *const _ as *mut std::ffi::c_void, None)
    } else {
        None
    }
}

