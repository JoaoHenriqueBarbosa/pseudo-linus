// Mesclado das partes traduzidas de auth_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Define ou remove a função de autorização de acesso.
///
/// A função de autorização é chamada durante a compilação para verificar que
/// o usuário tem permissão de leitura e/ou escrita em vários campos do banco.
/// O primeiro argumento da função de autorização é uma cópia do terceiro argumento
/// desta rotina. O segundo argumento é uma destas constantes:
///
/// - SQLITE_CREATE_INDEX
/// - SQLITE_CREATE_TABLE
/// - SQLITE_CREATE_TEMP_INDEX
/// - SQLITE_CREATE_TEMP_TABLE
/// - SQLITE_CREATE_TEMP_TRIGGER
/// - SQLITE_CREATE_TEMP_VIEW
/// - SQLITE_CREATE_TRIGGER
/// - SQLITE_CREATE_VIEW
/// - SQLITE_DELETE
/// - SQLITE_DROP_INDEX
/// - SQLITE_DROP_TABLE
/// - SQLITE_DROP_TEMP_INDEX
/// - SQLITE_DROP_TEMP_TABLE
/// - SQLITE_DROP_TEMP_TRIGGER
/// - SQLITE_DROP_TEMP_VIEW
/// - SQLITE_DROP_TRIGGER
/// - SQLITE_DROP_VIEW
/// - SQLITE_INSERT
/// - SQLITE_PRAGMA
/// - SQLITE_READ
/// - SQLITE_SELECT
/// - SQLITE_TRANSACTION
/// - SQLITE_UPDATE
///
/// O terceiro e quarto argumentos para a função de autorização são o nome
/// da tabela e coluna que estão sendo acessados. A função de autorização
/// deve retornar SQLITE_OK, SQLITE_DENY ou SQLITE_IGNORE. Se retorna SQLITE_OK,
/// o acesso é permitido. SQLITE_DENY significa que a instrução SQL nunca será
/// executada; a chamada sqlite3_exec() retorna com erro. SQLITE_IGNORE significa
/// que a instrução SQL deve ser executada mas tentativas de ler a coluna
/// especificada retornarão NULL e tentativas de escrever na coluna serão ignoradas.
///
/// Definir a função de autorização como NULL desabilita este hook. A configuração
/// padrão é NULL.
pub fn set_authorizer(
    db: &Sqlite3Ref,
    x_auth: Option<Sqlite3XAuth>,
    p_arg: CallbackArg,
) -> i32 {
    // SQLITE_ENABLE_API_ARMOR não faz parte das opções do Debian: sem a checagem.
    let mutex = db.borrow().mutex.clone();
    mutex_enter(mutex.as_ref());
    let has_auth = {
        let mut db_mut = db.borrow_mut();
        db_mut.x_auth = x_auth;
        db_mut.p_auth_arg = p_arg;
        db_mut.x_auth.is_some()
    };
    if has_auth {
        expire_prepared_statements(db, 1);
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Escreve uma mensagem de erro em pParse->zErrMsg explicando que
/// a função de autorização fornecida pelo usuário retornou um valor ilegal.
fn auth_bad_return_code(p_parse: &mut Parse) {
    error_msg(p_parse, b"authorizer malfunction");
    p_parse.rc = SQLITE_ERROR;
}

/// Texto de um `%s` do printf do SQLite para um argumento ausente: "(null)".
fn auth_str_or_null(z: Option<&[u8]>) -> Vec<u8> {
    match z {
        Some(s) => s.to_vec(),
        None => b"(null)".to_vec(),
    }
}

/// Invoca o callback de autorização para permissão de ler a coluna zCol da
/// tabela zTab no banco de dados zDb. Esta função assume que um callback de
/// autorização foi registrado (isto é, que sqlite3.xAuth não é NULL).
///
/// Se SQLITE_IGNORE é retornado e pExpr não é NULL, então pExpr é convertida
/// para uma expressão SQL NULL. Caso contrário, se pExpr é NULL, então
/// SQLITE_IGNORE é tratado como SQLITE_DENY. Neste caso um erro é deixado em pParse.
pub fn auth_read_col(
    p_parse: &mut Parse,
    z_tab: Option<&[u8]>,
    z_col: Option<&[u8]>,
    i_db: i32,
) -> i32 {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let (z_db, busy, x_auth, p_auth_arg, n_db) = {
        let db_ref = db.borrow();
        (
            db_ref.a_db[i_db as usize].z_db_s_name.clone(),
            db_ref.init.busy != 0,
            db_ref.x_auth.clone(),
            db_ref.p_auth_arg.clone(),
            db_ref.n_db,
        )
    };

    if busy {
        return SQLITE_OK;
    }
    // Nenhum borrow do banco fica preso durante o callback do usuário.
    let x_auth = x_auth.expect("xAuth deve estar registrado");
    let rc = x_auth(
        &p_auth_arg,
        SQLITE_READ,
        z_tab,
        z_col,
        z_db.as_deref(),
        p_parse.z_auth_context.as_deref(),
    );
    if rc == SQLITE_DENY {
        // z = mprintf("%s.%s", zTab, zCol)
        let mut z = auth_str_or_null(z_tab);
        z.push(b'.');
        z.extend_from_slice(&auth_str_or_null(z_col));
        // if( db->nDb>2 || iDb!=0 ) z = mprintf("%s.%z", zDb, z)
        if n_db > 2 || i_db != 0 {
            let mut z2 = auth_str_or_null(z_db.as_deref());
            z2.push(b'.');
            z2.extend_from_slice(&z);
            z = z2;
        }
        let mut msg = b"access to ".to_vec();
        msg.extend_from_slice(&z);
        msg.extend_from_slice(b" is prohibited");
        error_msg(p_parse, &msg);
        p_parse.rc = SQLITE_AUTH;
    } else if rc != SQLITE_IGNORE && rc != SQLITE_OK {
        auth_bad_return_code(p_parse);
    }
    rc
}

/// O pExpr deve ser uma expressão TK_COLUMN. A tabela referenciada
/// está em pTabList ou é a tabela NEW ou OLD de um trigger.
/// Verifica se é permitido ler esta coluna em particular.
///
/// Se a função de autorização retorna SQLITE_IGNORE, muda a instrução TK_COLUMN
/// para TK_NULL. Se retorna SQLITE_DENY, gera um erro.
pub fn auth_read(
    p_parse: &mut Parse,
    p_expr: &mut Expr,
    p_schema: Option<&SchemaRef>,
    p_tab_list: Option<&SrcList>,
) {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");

    assert!(p_expr.op == TK_COLUMN || p_expr.op == TK_TRIGGER);
    assert!(!in_rename_object(p_parse));
    assert!(db.borrow().x_auth.is_some());
    let i_db = schema_to_index(&db.borrow(), p_schema);
    if i_db < 0 {
        // Tentativa de ler uma coluna de uma subconsulta ou outra tabela temporária.
        return;
    }

    let mut p_tab: Option<TableRef> = None;
    if p_expr.op == TK_TRIGGER {
        p_tab = p_parse.p_trigger_tab.clone();
    } else {
        let tab_list = p_tab_list.expect("pTabList não pode ser NULL");
        for i_src in 0..tab_list.n_src as usize {
            if p_expr.i_table == tab_list.a[i_src].i_cursor {
                p_tab = tab_list.a[i_src].p_tab.clone();
                break;
            }
        }
    }
    let i_col = p_expr.i_column;
    let p_tab = match p_tab {
        Some(t) => t,
        None => return,
    };

    let (z_tab_name, z_col): (Vec<u8>, Vec<u8>) = {
        let tab = p_tab.borrow();
        let z_col = if i_col >= 0 {
            assert!(i_col < tab.n_col as i32);
            tab.a_col[i_col as usize].z_cn_name.clone()
        } else if tab.i_p_key >= 0 {
            assert!((tab.i_p_key as i32) < tab.n_col as i32);
            tab.a_col[tab.i_p_key as usize].z_cn_name.clone()
        } else {
            b"ROWID".to_vec()
        };
        (tab.z_name.clone(), z_col)
    };
    assert!(i_db >= 0 && i_db < db.borrow().n_db);
    if SQLITE_IGNORE == auth_read_col(p_parse, Some(&z_tab_name), Some(&z_col), i_db) {
        p_expr.op = TK_NULL;
    }
}

/// Faz uma verificação de autorização usando o código e argumentos fornecidos.
/// Retorna SQLITE_OK (zero), SQLITE_IGNORE ou SQLITE_DENY. Se SQLITE_DENY
/// é retornado, então a contagem de erros e mensagem de erro em pParse
/// são modificadas apropriadamente.
pub fn auth_check(
    p_parse: &mut Parse,
    code: i32,
    z_arg1: Option<&[u8]>,
    z_arg2: Option<&[u8]>,
    z_arg3: Option<&[u8]>,
) -> i32 {
    let db = p_parse.db.upgrade().expect("banco deve estar ativo");
    let (x_auth, p_auth_arg, busy) = {
        let db_ref = db.borrow();
        (db_ref.x_auth.clone(), db_ref.p_auth_arg.clone(), db_ref.init.busy != 0)
    };

    // Não faz nenhuma verificação de autorização se o banco está inicializando
    // ou se o parser está sendo invocado de dentro de sqlite3_declare_vtab.
    assert!(!in_rename_object(p_parse) || x_auth.is_none());
    let x_auth = match x_auth {
        Some(f) if !busy && !in_special_parse(p_parse) => f,
        _ => return SQLITE_OK,
    };

    // Do terceiro ao sexto parâmetros do callback, qualquer um pode ser NULL ou string.
    let mut rc = x_auth(
        &p_auth_arg,
        code,
        z_arg1,
        z_arg2,
        z_arg3,
        p_parse.z_auth_context.as_deref(),
    );
    if rc == SQLITE_DENY {
        error_msg(p_parse, b"not authorized");
        p_parse.rc = SQLITE_AUTH;
    } else if rc != SQLITE_OK && rc != SQLITE_IGNORE {
        rc = SQLITE_DENY;
        auth_bad_return_code(p_parse);
    }
    rc
}

/// Coloca um contexto de autorização. Depois que esta rotina é chamada, o
/// argumento zArg3 para callbacks de autorização será zContext até ser removido.
/// No C, pParse==0 faria desta rotina um no-op (aqui pParse é sempre válido).
///
/// Modelo sem ponteiros: `AuthContext.p_parse` é um `bool` que indica que o
/// contexto está ativo; o `Parse` volta como argumento em `auth_context_pop`.
pub fn auth_context_push(p_parse: &mut Parse, p_context: &mut AuthContext, z_context: Option<Vec<u8>>) {
    p_context.p_parse = true;
    p_context.z_auth_context = p_parse.z_auth_context.take();
    p_parse.z_auth_context = z_context;
}

/// Remove um contexto de autorização que foi anteriormente colocado
/// por sqlite3AuthContextPush
pub fn auth_context_pop(p_parse: &mut Parse, p_context: &mut AuthContext) {
    if p_context.p_parse {
        p_parse.z_auth_context = p_context.z_auth_context.take();
        p_context.p_parse = false;
    }
}

