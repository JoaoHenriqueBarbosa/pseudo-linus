//! Tradução de `auth.c`: a API `sqlite3_set_authorizer()` e as verificações de autorização que o
//! compilador de SQL faz durante a preparação do comando.
//!
//! Notas do modelo v2:
//! - O `xAuth` com `pAuthArg` é um `AuthFn` (fechamento que já captura o argumento do usuário);
//!   por isso `sqlite3_set_authorizer` recebe um `Option<AuthFn>` e não o par função/ponteiro.
//! - `SQLITE_USER_AUTHENTICATION` está desligado: o argumento extra `zAuthUser` some.
//! - `AuthContext` guarda o valor salvo de `Parse.z_auth_context` (o `pParse` do C some; o
//!   chamador devolve o mesmo `Parse` ao `auth_context_pop`). O `AuthContext` zerado do C (que
//!   faz o Pop ser um no-op) é `AuthContext::default()`; como o struct só tem o valor salvo,
//!   o Pop só é chamado por quem fez o Push (ver `delete.rs`).

use crate::connection::{AuthContext, AuthFn, Connection, Parse};
use crate::consts::parse::{TK_COLUMN, TK_NULL, TK_TRIGGER};
use crate::consts::sqlite3::{
    SQLITE_AUTH, SQLITE_DENY, SQLITE_ERROR, SQLITE_IGNORE, SQLITE_OK, SQLITE_READ,
};
use crate::printf::{mprintf, PrintfArg};
use crate::prepare::schema_to_index;
use crate::sqlite_int::{Expr, SchemaId, SrcList};
use crate::util::error_msg;
use crate::vdbeaux3::expire_prepared_statements;

/// `sqlite3_set_authorizer`: define ou limpa a função de autorização de acesso. A função é
/// chamada durante a compilação para verificar que o usuário tem permissão de leitura e/ou
/// escrita nos vários campos do banco. `None` desliga o gancho (o padrão).
pub fn set_authorizer(db: &mut Connection, x_auth: Option<AuthFn>) -> i32 {
    db.x_auth = x_auth;
    if db.x_auth.is_some() {
        expire_prepared_statements(db, 1);
    }
    SQLITE_OK
}

/// `sqliteAuthBadReturnCode`: escreve em `parse.z_err_msg` que a função de autorização do
/// usuário devolveu um valor ilegal.
fn auth_bad_return_code(db: &mut Connection, parse: &mut Parse) {
    error_msg(db, parse, b"authorizer malfunction", &[]);
    parse.rc = SQLITE_ERROR;
}

/// Chama o `xAuth` registrado (o chamador garante que ele existe).
fn call_auth(
    db: &mut Connection,
    parse: &Parse,
    code: i32,
    z_arg1: Option<&[u8]>,
    z_arg2: Option<&[u8]>,
    z_arg3: Option<&[u8]>,
) -> i32 {
    let f = db.x_auth.as_mut().expect("xAuth registrado");
    f(code, z_arg1, z_arg2, z_arg3, parse.z_auth_context.as_deref())
}

/// `sqlite3AuthReadCol`: chama o gancho de autorização pedindo permissão para ler a coluna
/// `z_col` da tabela `z_tab` no banco `i_db`. Supõe que há um gancho registrado.
///
/// Se devolve `SQLITE_IGNORE` e `p_expr` não é nulo, quem chama troca a expressão por NULL;
/// senão o `SQLITE_IGNORE` vale como `SQLITE_DENY`, e o erro fica em `parse`.
pub fn auth_read_col(
    db: &mut Connection,
    parse: &mut Parse,
    z_tab: &[u8],
    z_col: &[u8],
    i_db: i32,
) -> i32 {
    let z_db = db.dbs[i_db as usize].z_db_s_name.clone(); // nome do esquema do banco
    if db.init.busy != 0 {
        return SQLITE_OK;
    }
    let rc = call_auth(db, parse, SQLITE_READ, Some(z_tab), Some(z_col), Some(&z_db));
    if rc == SQLITE_DENY {
        let mut z = mprintf(
            b"%s.%s",
            &[PrintfArg::Text(Some(z_tab.to_vec())), PrintfArg::Text(Some(z_col.to_vec()))],
        )
        .unwrap_or_default();
        if db.dbs.len() > 2 || i_db != 0 {
            z = mprintf(
                b"%s.%s",
                &[PrintfArg::Text(Some(z_db.clone())), PrintfArg::Text(Some(z))],
            )
            .unwrap_or_default();
        }
        error_msg(db, parse, b"access to %s is prohibited", &[PrintfArg::Text(Some(z))]);
        parse.rc = SQLITE_AUTH;
    } else if rc != SQLITE_IGNORE && rc != SQLITE_OK {
        auth_bad_return_code(db, parse);
    }
    rc
}

/// `sqlite3AuthRead`: `p_expr` é uma expressão TK_COLUMN (ou TK_TRIGGER). A tabela a que ela se
/// refere está em `p_tab_list` ou é a tabela NEW ou OLD de um gatilho. Verifica se é permitido
/// ler esta coluna.
///
/// Se o gancho devolve `SQLITE_IGNORE`, o TK_COLUMN vira TK_NULL. Se devolve `SQLITE_DENY`, gera
/// um erro.
pub fn auth_read(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: &mut Expr,
    p_schema: SchemaId,
    p_tab_list: Option<&SrcList>,
) {
    debug_assert!(p_expr.op == TK_COLUMN || p_expr.op == TK_TRIGGER);
    debug_assert!(!parse.in_rename_object());
    debug_assert!(db.x_auth.is_some());
    let i_db = schema_to_index(db, p_schema); // o índice do banco a que a expressão se refere
    if i_db < 0 {
        // Tentativa de ler uma coluna de uma subconsulta ou de outra tabela temporária.
        return;
    }

    let mut p_tab = None; // a tabela lida
    if p_expr.op == TK_TRIGGER {
        p_tab = parse.p_trigger_tab.clone();
    } else {
        debug_assert!(p_tab_list.is_some());
        if let Some(list) = p_tab_list {
            for item in list.a.iter() {
                if p_expr.i_table == item.i_cursor {
                    p_tab = item.p_tab.clone();
                    break;
                }
            }
        }
    }
    let i_col = p_expr.i_column as i32; // índice da coluna na tabela
    let p_tab = match p_tab {
        Some(t) => t,
        None => return,
    };

    let z_col: Vec<u8> = if i_col >= 0 {
        debug_assert!(i_col < p_tab.n_col as i32);
        p_tab.a_col[i_col as usize].z_cn_name.clone()
    } else if p_tab.i_p_key >= 0 {
        debug_assert!((p_tab.i_p_key as i32) < p_tab.n_col as i32);
        p_tab.a_col[p_tab.i_p_key as usize].z_cn_name.clone()
    } else {
        b"ROWID".to_vec()
    };
    debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
    if SQLITE_IGNORE == auth_read_col(db, parse, &p_tab.z_name, &z_col, i_db) {
        p_expr.op = TK_NULL;
    }
}

/// `sqlite3AuthCheck`: faz uma verificação de autorização com o código e os argumentos dados.
/// Devolve `SQLITE_OK` (zero), `SQLITE_IGNORE` ou `SQLITE_DENY`. Se devolve `SQLITE_DENY`, o
/// contador de erros e a mensagem em `parse` são atualizados.
pub fn auth_check(
    db: &mut Connection,
    parse: &mut Parse,
    code: i32,
    z_arg1: Option<&[u8]>,
    z_arg2: Option<&[u8]>,
    z_arg3: Option<&[u8]>,
) -> i32 {
    // Sem verificações enquanto o banco inicializa ou quando o analisador é chamado de dentro de
    // sqlite3_declare_vtab.
    debug_assert!(!parse.in_rename_object() || db.x_auth.is_none());
    if db.x_auth.is_none() || db.init.busy != 0 || parse.in_special_parse() {
        return SQLITE_OK;
    }

    // Os argumentos de 3 a 6 do gancho podem ser, cada um, NULL ou uma cadeia terminada em zero.
    let mut rc = call_auth(db, parse, code, z_arg1, z_arg2, z_arg3);
    if rc == SQLITE_DENY {
        error_msg(db, parse, b"not authorized", &[]);
        parse.rc = SQLITE_AUTH;
    } else if rc != SQLITE_OK && rc != SQLITE_IGNORE {
        rc = SQLITE_DENY;
        auth_bad_return_code(db, parse);
    }
    rc
}

/// `sqlite3AuthContextPush`: empilha um contexto de autorização. Depois desta chamada o
/// argumento `zArg3` dos ganchos de autorização será `z_context` até o contexto ser desempilhado.
pub fn auth_context_push(parse: &mut Parse, p_context: &mut AuthContext, z_context: &[u8]) {
    p_context.z_auth_context = parse.z_auth_context.take();
    parse.z_auth_context = Some(z_context.to_vec());
}

/// `sqlite3AuthContextPop`: desempilha um contexto empilhado por `auth_context_push`.
pub fn auth_context_pop(parse: &mut Parse, p_context: &mut AuthContext) {
    parse.z_auth_context = p_context.z_auth_context.take();
}
