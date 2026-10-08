//! Primeira parte de `build.c`: travas de tabela, `sqlite3FinishCoding`, `sqlite3NestedParse`,
//! localização de tabelas e índices, reset de esquema, colunas (`DEFAULT`, `NOT NULL`, `PRIMARY
//! KEY`, `CHECK`, `COLLATE`), `sqlite3StartTable` e `sqlite3AddReturning`.
//!
//! Decisões do modelo v2 que aparecem aqui (ver CONVENTIONS.md e `connection.rs`):
//!
//! - `Parse` não guarda a conexão: toda função recebe `db: &mut Connection` e `parse: &mut Parse`,
//!   nessa ordem. O `Vdbe` é do `Parse` (`parse.p_vdbe`) até `finish_coding`.
//! - `Table`, `Index` e o resto do esquema são `Rc` imutáveis. As funções de busca devolvem
//!   `Rc<Table>`; `find_index` devolve também a tabela dona, porque `Index` não tem `pTable`.
//! - A tabela em construção é `parse.p_new_table: Option<Box<TableBuilder>>`. Quem precisa
//!   chamar de volta o resto do analisador tira o `Box` do `Parse`, trabalha, e o devolve
//!   (`with_new_table`).
//! - `Column.z_cn_name` guarda `nome\0[tipo\0][colação\0]` numa só alocação, como o C. Os demais
//!   nomes (`Table.z_name`, `Index.z_name`, ...) são `Vec<u8>` sem NUL final.
//! - Fora do que o Debian liga: `SQLITE_ENABLE_HIDDEN_COLUMNS` (então `sqlite3ColumnPropertiesFromName`
//!   é macro vazia), `SQLITE_ENABLE_STAT4` (então `sqlite3FreeIndex` só libera memória e some),
//!   `SQLITE_USER_AUTHENTICATION`, `SQLITE_OMIT_*` e `SQLITE_DEBUG`. `sqlite3DeleteTableGeneric` e
//!   `sqlite3FreeIndex` não existem: servem só de destrutor por ponteiro, e o `Drop` do Rust os
//!   substitui. `sqlite3DbMaskAllZero` só existe com `SQLITE_MAX_ATTACHED>30` (aqui vale 10).

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::build`.
pub use crate::build2::*;
pub use crate::build3::*;

use std::rc::Rc;

use crate::connection::{Connection, Parse, TableBuilder, TableLock, OPFLAG_APPEND};
use crate::consts::{
    BTREE_FILE_FORMAT, BTREE_INTKEY, BTREE_TEXT_ENCODING, COLFLAG_GENERATED, COLFLAG_HASCOLL,
    COLFLAG_HASTYPE, COLFLAG_PRIMKEY, COLFLAG_UNIQUE, COLFLAG_VIRTUAL, COLTYPE_CUSTOM,
    COLTYPE_INTEGER, DBFLAG_PREFER_BUILTIN, DBFLAG_SCHEMA_CHANGE, DBFLAG_SCHEMA_KNOWN_OK,
    DBFLAG_VACUUM, DB_RESETWANTED, EP_INT_VALUE, EP_SKIP, EXPRDUP_REDUCE, LEGACY_SCHEMA_TABLE,
    LEGACY_TEMP_SCHEMA_TABLE, LOCATE_NOERR, LOCATE_VIEW, OMIT_TEMPDB, OP_BLOB, OP_CLOSE,
    OP_COLUMN, OP_CREATEBTREE, OP_FKCHECK, OP_HALT, OP_IF, OP_INSERT, OP_INTEGER, OP_JOURNALMODE,
    OP_NEWROWID, OP_NEXT, OP_OPENEPHEMERAL, OP_OPENWRITE, OP_READCOOKIE, OP_RESULTROW,
    OP_REWIND, OP_SETCOOKIE, OP_TABLELOCK, OP_TRANSACTION, OP_VBEGIN, PAGER_JOURNALMODE_QUERY,
    PREFERRED_SCHEMA_TABLE, PREFERRED_TEMP_SCHEMA_TABLE, SCHEMA_ROOT, SQLITE_AFF_BLOB,
    SQLITE_AFF_INTEGER, SQLITE_AFF_NUMERIC, SQLITE_AFF_REAL, SQLITE_AFF_TEXT, SQLITE_CREATE_TABLE,
    SQLITE_CREATE_TEMP_TABLE, SQLITE_CREATE_TEMP_VIEW, SQLITE_CREATE_VIEW, SQLITE_DEFENSIVE,
    SQLITE_DONE, SQLITE_ERROR, SQLITE_IDXTYPE_PRIMARYKEY, SQLITE_INSERT, SQLITE_LEGACY_FILE_FMT,
    SQLITE_LIMIT_COLUMN, SQLITE_LIMIT_LENGTH, SQLITE_MAX_FILE_FORMAT, SQLITE_N_STDTYPE,
    SQLITE_NOMEM, SQLITE_OK, SQLITE_PREPARE_NO_VTAB, SQLITE_SO_DESC, SQLITE_TOOBIG,
    SQLITE_WRITE_SCHEMA, TF_AUTOINCREMENT, TF_HAS_NOT_NULL, TF_HAS_PRIMARY_KEY, TF_HAS_VIRTUAL,
    TK_COLLATE, TK_ID, TK_RETURNING, TK_SPAN, TK_STRING, TRIGGER_AFTER, OE_NONE,
};
use crate::ctype::{is_digit, is_space, UPPER_TO_LOWER};
use crate::hash::{hash_find, hash_find_mut, hash_insert};
use crate::printf::{vm_printf, PrintfArg, PrintfToken};
use crate::sqlite_int::{
    Column, Expr, ExprList, ExprU, Index, Returning, SrcItem, Table, TableU, Token, Trigger,
    TriggerStep,
};
use crate::util::{
    at, dequote, get_int32, str_i_hash, str_icmp, strlen30, strnicmp, stricmp, STD_TYPE,
    STD_TYPE_AFFINITY, STD_TYPE_LEN,
};
use crate::vdbe_types::{db_mask_test, P4};

// Funções de outras fatias, chamadas pelo nome determinístico (assinaturas supostas no relatório).
use crate::alter::{rename_expr_unmap, rename_token_map, rename_token_remap};
use crate::attach::db_is_named;
use crate::auth::auth_check;
use crate::btree::btree_is_readonly;
use crate::callback::{locate_coll_seq, schema_clear};
use crate::expr::{
    expr_code, expr_dup, expr_is_constant_or_function, expr_list_append, expr_list_set_name,
    expr_skip_collate_mut,
};
use crate::fkey::fk_delete;
use crate::global::extra_schema_checks;
use crate::insert::auto_increment_begin;
use crate::pragma::pragma_vtab_register;
use crate::prepare::{read_schema, schema_to_index};
use crate::tokenize::run_parser;
use crate::util::{db_span_dup, dequote_token, error_msg};
use crate::select::get_vdbe;
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4,
    add_op4_int, change_p5, vdbe_comment, vdbe_goto, jump_here, make_ready,
    uses_btree,
};
use crate::vtab::{
    get_vtable, vtab_clear, vtab_eponymous_table_init,
    vtab_unlock_list,
};

// ---------------------------------------------------------------------------------------------
// Pequenos auxiliares de formatação de mensagens
// ---------------------------------------------------------------------------------------------

/// Argumento `%s` do `printf` interno a partir de bytes.
pub(crate) fn text_arg(z: &[u8]) -> PrintfArg {
    PrintfArg::Text(Some(z.to_vec()))
}

/// Argumento `%T` do `printf` interno a partir de um `Token`. O deslocamento dentro de
/// `Parse.z_tail` (a conferência `SQLITE_WITHIN` de `sqlite3RecordErrorByteOffset`) é calculado
/// aqui: `i_ofst` negativo (token que não vem do texto) fica fora.
pub(crate) fn token_arg(parse: &Parse, t: &Token) -> PrintfArg {
    let z_tail = parse.z_tail as i32;
    let tail_offset = if t.i_ofst >= z_tail { Some(t.i_ofst - z_tail) } else { None };
    PrintfArg::Token(Some(PrintfToken { z: t.z.clone(), tail_offset }))
}

/// Macro `SCHEMA_TABLE(x)` do sqliteInt.h: o nome legado da tabela de esquema do banco `i_db`.
pub fn schema_table(i_db: i32) -> &'static [u8] {
    if OMIT_TEMPDB == 0 && i_db == 1 {
        LEGACY_TEMP_SCHEMA_TABLE
    } else {
        LEGACY_SCHEMA_TABLE
    }
}

// ---------------------------------------------------------------------------------------------
// Travas de tabela (cache compartilhado)
// ---------------------------------------------------------------------------------------------

/// `lockTable`: registra no `Parse` de nível mais alto que a tabela deve ser travada em tempo de
/// execução. O código que faz a trava é gerado depois por `code_table_locks`.
fn lock_table(parse: &mut Parse, i_db: i32, i_tab: u32, is_write_lock: bool, z_name: &[u8]) {
    debug_assert!(i_db >= 0);
    let top = parse.toplevel_mut();
    for p in top.a_table_lock.iter_mut() {
        if p.i_db == i_db && p.i_tab == i_tab {
            p.is_write_lock = p.is_write_lock || is_write_lock;
            return;
        }
    }
    top.a_table_lock.push(TableLock {
        i_db,
        i_tab,
        is_write_lock,
        z_lock_name: z_name.to_vec(),
    });
}

/// `sqlite3TableLock`: registra o desejo de travar uma tabela em tempo de execução. Só tem efeito
/// no banco compartilhável (cache compartilhado), e nunca no banco TEMP.
pub fn table_lock(
    db: &Connection,
    parse: &mut Parse,
    i_db: i32,
    i_tab: u32,
    is_write_lock: bool,
    z_name: &[u8],
) {
    if i_db == 1 {
        return;
    }
    let sharable = db.dbs[i_db as usize].bt.as_ref().map_or(false, |b| b.sharable);
    if !sharable {
        return;
    }
    lock_table(parse, i_db, i_tab, is_write_lock, z_name);
}

/// `codeTableLocks`: gera um `OP_TableLock` para cada tabela travada pelo comando.
fn code_table_locks(parse: &mut Parse) {
    let Some(v) = parse.p_vdbe.as_deref_mut() else {
        return;
    };
    for p in parse.a_table_lock.iter() {
        add_op4(
            v,
            OP_TABLELOCK,
            p.i_db,
            p.i_tab as i32,
            p.is_write_lock as i32,
            P4::Text(p.z_lock_name.clone()),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// sqlite3FinishCoding e sqlite3NestedParse
// ---------------------------------------------------------------------------------------------

/// `sqlite3FinishCoding`: chamada depois que um comando foi analisado e o programa do VDBE
/// preparado. Dá os toques finais no programa e deixa o `Parse` pronto para o próximo comando.
/// Se houve erro, pode não existir código.
pub fn finish_coding(db: &mut Connection, parse: &mut Parse) {
    debug_assert!(parse.p_toplevel.is_none());
    if parse.nested != 0 {
        return;
    }
    if parse.n_err != 0 {
        if db.malloc_failed != 0 {
            parse.rc = SQLITE_NOMEM;
        }
        return;
    }
    debug_assert!(db.malloc_failed == 0);

    // Começa gerando o código de terminação no fim do programa.
    let mut have_v = parse.p_vdbe.is_some();
    if !have_v {
        if db.init.busy != 0 {
            parse.rc = SQLITE_DONE;
            return;
        }
        have_v = { get_vdbe(db, parse); true };
        if !have_v {
            parse.rc = SQLITE_ERROR;
        }
    }
    if have_v {
        if parse.b_returning != 0 {
            let ret = parse.p_returning.as_ref().map(|r| (r.n_ret_col, r.i_ret_cur, r.i_ret_reg));
            if let (Some((n_ret_col, i_ret_cur, reg)), Some(v)) =
                (ret, parse.p_vdbe.as_deref_mut())
            {
                if n_ret_col != 0 {
                    add_op0(v, OP_FKCHECK);
                    let addr_rewind = add_op1(v, OP_REWIND, i_ret_cur);
                    for i in 0..n_ret_col {
                        add_op3(v, OP_COLUMN, i_ret_cur, i, reg + i);
                    }
                    add_op2(v, OP_RESULTROW, reg, n_ret_col);
                    add_op2(v, OP_NEXT, i_ret_cur, addr_rewind + 1);
                    jump_here(v, addr_rewind);
                }
            }
        }
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            add_op0(v, OP_HALT);
            jump_here(v, 0);
        }

        // A máscara de cookies tem um bit por banco aberto (o 0 é o main, o 1 é o temp...). Os
        // bits ligados são os bancos usados: gera o código que abre a transação em cada um e
        // confere o cookie do esquema.
        debug_assert!(!db.dbs.is_empty());
        let mut i_db = 0usize;
        loop {
            if db_mask_test(parse.cookie_mask, i_db) {
                let write = db_mask_test(parse.write_mask, i_db) as i32;
                let comment = (parse.may_abort != 0 && parse.is_multi_write != 0) as i64;
                if let Some(v) = parse.p_vdbe.as_deref_mut() {
                    uses_btree(v, i_db as i32);
                    let schema = &db.dbs[i_db].schema;
                    add_op4_int(
                        v,
                        OP_TRANSACTION,
                        i_db as i32,
                        write,
                        schema.schema_cookie,
                        schema.i_generation,
                    );
                    if db.init.busy == 0 {
                        change_p5(v, 1);
                    }
                    vdbe_comment(v, b"usesStmtJournal=%d", &[PrintfArg::Int(comment)]);
                }
            }
            i_db += 1;
            if i_db >= db.dbs.len() {
                break;
            }
        }
        let vtab_locks = std::mem::take(&mut parse.ap_vtab_lock);
        for t in vtab_locks.iter() {
            if let (Some(id), Some(v)) = (get_vtable(db, t), parse.p_vdbe.as_deref_mut()) {
                add_op4(v, OP_VBEGIN, 0, 0, 0, P4::Vtab(id));
            }
        }

        // Depois de verificados os cookies e abertas as transações, obtém as travas de tabela
        // pedidas. Sem cache compartilhado não faz nada.
        if !parse.a_table_lock.is_empty() {
            code_table_locks(parse);
        }

        // Inicializa as estruturas de AUTOINCREMENT necessárias.
        if !parse.p_ainc.is_empty() {
            auto_increment_begin(db, parse);
        }

        // Codifica as expressões constantes fatoradas para fora dos laços internos.
        if parse.p_const_expr.is_some() {
            let mut list = parse.p_const_expr.take();
            parse.ok_const_factor = 0;
            if let Some(list) = list.as_deref_mut() {
                for item in list.a.iter_mut() {
                    debug_assert!(item.i_const_expr_reg > 0);
                    let reg = item.i_const_expr_reg;
                    if let Some(e) = item.p_expr.as_deref_mut() {
                        expr_code(db, parse, e, reg, None);
                    }
                }
            }
            parse.p_const_expr = list;
        }

        if parse.b_returning != 0 {
            let ret = parse.p_returning.as_ref().map(|r| (r.n_ret_col, r.i_ret_cur));
            if let (Some((n_ret_col, i_ret_cur)), Some(v)) = (ret, parse.p_vdbe.as_deref_mut()) {
                if n_ret_col != 0 {
                    add_op2(v, OP_OPENEPHEMERAL, i_ret_cur, n_ret_col);
                }
            }
        }

        // Por fim, volta ao início do código executável.
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            vdbe_goto(v, 1);
        }
    }

    // Deixa o programa do VDBE pronto para executar.
    debug_assert!(have_v || parse.n_err != 0);
    debug_assert!(db.malloc_failed == 0 || parse.n_err != 0);
    if parse.n_err == 0 {
        // No mínimo um cursor é necessário se há AUTOINCREMENT (ticket a696379c1f08866).
        debug_assert!(parse.p_ainc.is_empty() || parse.n_tab > 0);
        make_ready(parse);
        parse.rc = SQLITE_DONE;
    } else {
        parse.rc = SQLITE_ERROR;
    }
}

/// Troca entre dois `Parse` a região do `Parse` que o C zera antes e depois de cada recursão:
/// tudo de `sLastToken` em diante (`PARSE_TAIL`). `a` recebe o que `b` tinha e vice-versa.
fn swap_parse_tail(a: &mut Parse, b: &mut Parse) {
    std::mem::swap(&mut a.s_last_token, &mut b.s_last_token);
    std::mem::swap(&mut a.n_var, &mut b.n_var);
    std::mem::swap(&mut a.i_pk_sort_order, &mut b.i_pk_sort_order);
    std::mem::swap(&mut a.explain, &mut b.explain);
    std::mem::swap(&mut a.e_parse_mode, &mut b.e_parse_mode);
    std::mem::swap(&mut a.n_height, &mut b.n_height);
    std::mem::swap(&mut a.addr_explain, &mut b.addr_explain);
    std::mem::swap(&mut a.p_v_list, &mut b.p_v_list);
    std::mem::swap(&mut a.p_reprepare, &mut b.p_reprepare);
    std::mem::swap(&mut a.z_tail, &mut b.z_tail);
    std::mem::swap(&mut a.p_new_table, &mut b.p_new_table);
    std::mem::swap(&mut a.p_new_index, &mut b.p_new_index);
    std::mem::swap(&mut a.p_new_trigger, &mut b.p_new_trigger);
    std::mem::swap(&mut a.z_auth_context, &mut b.z_auth_context);
    std::mem::swap(&mut a.s_arg, &mut b.s_arg);
    std::mem::swap(&mut a.ap_vtab_lock, &mut b.ap_vtab_lock);
    std::mem::swap(&mut a.p_with, &mut b.p_with);
    std::mem::swap(&mut a.p_rename, &mut b.p_rename);
}

/// `sqlite3NestedParse`: roda o analisador e o gerador de código recursivamente sobre o comando
/// SQL formatado, acrescentando o código ao fim do `Parse` em construção. O `OP_Halt` final não
/// é acrescentado (quem o faz é o analisador mais externo), e as funções embutidas sempre têm
/// precedência sobre as do aplicativo.
pub fn nested_parse(db: &mut Connection, parse: &mut Parse, fmt: &[u8], args: &[PrintfArg]) {
    let saved_db_flags = db.m_db_flags;
    if parse.n_err != 0 {
        return;
    }
    if parse.e_parse_mode != 0 {
        return;
    }
    debug_assert!(parse.nested < 10); // O aninhamento tem profundidade limitada.
    let (z_sql, _acc) =
        vm_printf(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32, fmt, args);
    let Some(z_sql) = z_sql else {
        // Pode ser falta de memória ou o texto passar de SQLITE_LIMIT_LENGTH; no segundo caso é
        // preciso registrar o erro.
        if db.malloc_failed == 0 {
            parse.rc = SQLITE_TOOBIG;
        }
        parse.n_err += 1;
        return;
    };
    parse.nested += 1;
    let mut saved_tail = Parse::default();
    swap_parse_tail(parse, &mut saved_tail);
    db.m_db_flags |= DBFLAG_PREFER_BUILTIN;
    run_parser(db, parse, &z_sql);
    db.m_db_flags = saved_db_flags;
    swap_parse_tail(parse, &mut saved_tail);
    parse.nested -= 1;
}

// ---------------------------------------------------------------------------------------------
// Localização de tabelas e índices
// ---------------------------------------------------------------------------------------------

/// `sqlite3FindTable`: acha a tabela pelo nome e, opcionalmente, pelo nome do banco. Devolve
/// `None` se não achar. Sem `z_database`, todos os bancos são percorridos e a primeira tabela
/// que casa vale (sem checar nomes duplicados): TEMP primeiro, depois o main, depois os
/// anexados na ordem do ATTACH.
pub fn find_table(db: &Connection, z_name: &[u8], z_database: Option<&[u8]>) -> Option<Rc<Table>> {
    let mut p: Option<Rc<Table>>;
    if let Some(z_db) = z_database {
        let n_db = db.dbs.len();
        let mut i = 0usize;
        while i < n_db {
            if str_icmp(z_db, &db.dbs[i].z_db_s_name) == 0 {
                break;
            }
            i += 1;
        }
        if i >= n_db {
            // Nenhum nome oficial casou, mas "main" sempre vale para o esquema 0 (legado).
            if str_icmp(z_db, b"main") == 0 {
                i = 0;
            } else {
                return None;
            }
        }
        p = hash_find(&db.dbs[i].schema.tbl_hash, z_name).cloned();
        if p.is_none() && strnicmp(Some(z_name), Some(b"sqlite_"), 7) == 0 {
            let tail = z_name.get(7..).unwrap_or(&[]);
            if i == 1 {
                if str_icmp(tail, &PREFERRED_TEMP_SCHEMA_TABLE[7..]) == 0
                    || str_icmp(tail, &PREFERRED_SCHEMA_TABLE[7..]) == 0
                    || str_icmp(tail, &LEGACY_SCHEMA_TABLE[7..]) == 0
                {
                    p = hash_find(&db.dbs[1].schema.tbl_hash, LEGACY_TEMP_SCHEMA_TABLE).cloned();
                }
            } else if str_icmp(tail, &PREFERRED_SCHEMA_TABLE[7..]) == 0 {
                p = hash_find(&db.dbs[i].schema.tbl_hash, LEGACY_SCHEMA_TABLE).cloned();
            }
        }
    } else {
        // Casa primeiro com o TEMP.
        p = hash_find(&db.dbs[1].schema.tbl_hash, z_name).cloned();
        if p.is_some() {
            return p;
        }
        // O main é o segundo.
        p = hash_find(&db.dbs[0].schema.tbl_hash, z_name).cloned();
        if p.is_some() {
            return p;
        }
        // Os anexados ficam na ordem do ATTACH.
        for i in 2..db.dbs.len() {
            p = hash_find(&db.dbs[i].schema.tbl_hash, z_name).cloned();
            if p.is_some() {
                break;
            }
        }
        if p.is_none() && strnicmp(Some(z_name), Some(b"sqlite_"), 7) == 0 {
            let tail = z_name.get(7..).unwrap_or(&[]);
            if str_icmp(tail, &PREFERRED_SCHEMA_TABLE[7..]) == 0 {
                p = hash_find(&db.dbs[0].schema.tbl_hash, LEGACY_SCHEMA_TABLE).cloned();
            } else if str_icmp(tail, &PREFERRED_TEMP_SCHEMA_TABLE[7..]) == 0 {
                p = hash_find(&db.dbs[1].schema.tbl_hash, LEGACY_TEMP_SCHEMA_TABLE).cloned();
            }
        }
    }
    p
}

/// `sqlite3LocateTable`: como `find_table`, mas deixa uma mensagem de erro em `parse` quando não
/// acha. `flags` é `LOCATE_VIEW` e/ou `LOCATE_NOERR`.
pub fn locate_table(
    db: &mut Connection,
    parse: &mut Parse,
    flags: u32,
    z_name: &[u8],
    z_dbase: Option<&[u8]>,
) -> Option<Rc<Table>> {
    // Lê o esquema. Em caso de erro, deixa a mensagem e o código em `parse` e devolve `None`.
    if (db.m_db_flags & DBFLAG_SCHEMA_KNOWN_OK) == 0 && SQLITE_OK != read_schema(db, parse) {
        return None;
    }

    let mut p = find_table(db, z_name, z_dbase);
    if p.is_none() {
        // Se `z_name` não é de uma tabela criada com CREATE, pode ser de uma tabela virtual
        // eponímia.
        if (parse.prep_flags as u32 & SQLITE_PREPARE_NO_VTAB) == 0 && db.init.busy == 0 {
            let mut p_mod = hash_find(&db.a_module, z_name).cloned();
            if p_mod.is_none() && strnicmp(Some(z_name), Some(b"pragma_"), 7) == 0 {
                p_mod = pragma_vtab_register(db, z_name);
            }
            if let Some(m) = p_mod {
                if vtab_eponymous_table_init(db, parse, &m) != 0 {
                    return hash_find(&db.a_epo_tab, &m.z_name).cloned();
                }
            }
        }
        if (flags & LOCATE_NOERR) != 0 {
            return None;
        }
        parse.check_schema = 1;
    } else if p.as_ref().map_or(false, |t| t.is_virtual())
        && (parse.prep_flags as u32 & SQLITE_PREPARE_NO_VTAB) != 0
    {
        p = None;
    }

    match &p {
        None => {
            let z_msg: &[u8] =
                if (flags & LOCATE_VIEW) != 0 { b"no such view" } else { b"no such table" };
            if let Some(z_db) = z_dbase {
                error_msg(
                    db,
                    parse,
                    b"%s: %s.%s",
                    &[text_arg(z_msg), text_arg(z_db), text_arg(z_name)],
                );
            } else {
                error_msg(db, parse, b"%s: %s", &[text_arg(z_msg), text_arg(z_name)]);
            }
        }
        Some(t) => {
            debug_assert!(t.has_rowid() || t.i_p_key < 0);
        }
    }
    p
}

/// `sqlite3LocateTableItem`: envoltório de `locate_table` que restringe a busca ao esquema do
/// item (`p.p_schema`) quando ele existe. Isso acontece em views e programas de gatilho (ver
/// `sqlite3FixSrcList`).
pub fn locate_table_item(
    db: &mut Connection,
    parse: &mut Parse,
    flags: u32,
    p: &SrcItem,
) -> Option<Rc<Table>> {
    debug_assert!(p.p_schema.is_none() || p.z_database.is_none());
    let z_db: Option<Vec<u8>> = match p.p_schema {
        Some(schema) => {
            let i_db = schema_to_index(db, schema);
            Some(db.dbs[i_db as usize].z_db_s_name.clone())
        }
        None => p.z_database.clone(),
    };
    let z_name: &[u8] = p.z_name.as_deref().unwrap_or(&[]);
    locate_table(db, parse, flags, z_name, z_db.as_deref())
}

/// `sqlite3PreferredTableName`: o nome preferido das tabelas de sistema. Traduz os nomes legados
/// nos novos.
pub fn preferred_table_name(z_name: &[u8]) -> &[u8] {
    if strnicmp(Some(z_name), Some(b"sqlite_"), 7) == 0 {
        let tail = z_name.get(7..).unwrap_or(&[]);
        if str_icmp(tail, &LEGACY_SCHEMA_TABLE[7..]) == 0 {
            return PREFERRED_SCHEMA_TABLE;
        }
        if str_icmp(tail, &LEGACY_TEMP_SCHEMA_TABLE[7..]) == 0 {
            return PREFERRED_TEMP_SCHEMA_TABLE;
        }
    }
    z_name
}

/// `sqlite3FindIndex`: acha o índice pelo nome e pelo nome do banco que o contém. Sem `z_db`,
/// todos os bancos são percorridos e vale o primeiro índice que casa (TEMP primeiro, depois o
/// main, depois os anexados). Como `Index` não aponta para a tabela dona, devolve também a
/// tabela (o `pTable` do C).
pub fn find_index(
    db: &Connection,
    z_name: &[u8],
    z_db: Option<&[u8]>,
) -> Option<(Rc<Table>, Rc<Index>)> {
    for i in (OMIT_TEMPDB as usize)..db.dbs.len() {
        let j = if i < 2 { i ^ 1 } else { i }; // O TEMP vem antes do main.
        let schema = &db.dbs[j].schema;
        if let Some(z_db) = z_db {
            if !db_is_named(db, j, z_db) {
                continue;
            }
        }
        let Some(tab_name) = hash_find(&schema.idx_hash, z_name) else {
            continue;
        };
        let Some(tab) = hash_find(&schema.tbl_hash, tab_name) else {
            continue;
        };
        if let Some(idx) = tab.p_index.iter().find(|x| str_icmp(&x.z_name, z_name) == 0) {
            return Some((tab.clone(), idx.clone()));
        }
    }
    None
}

/// `sqlite3UnlinkAndDeleteIndex`: para o índice `z_idx_name` do banco `i_db`, tira o índice da
/// sua tabela e da tabela hash de índices e libera tudo o que ele ocupa.
pub fn unlink_and_delete_index(db: &mut Connection, i_db: usize, z_idx_name: &[u8]) {
    let tab_name = hash_insert(&mut db.dbs[i_db].schema.idx_hash, z_idx_name, None);
    if let Some(tab_name) = tab_name {
        // O índice precisa estar na lista da tabela dona.
        if let Some(tab) = hash_find_mut(&mut db.dbs[i_db].schema.tbl_hash, &tab_name) {
            if let Some(pos) = tab.p_index.iter().position(|x| str_icmp(&x.z_name, z_idx_name) == 0)
            {
                Rc::make_mut(tab).p_index.remove(pos);
            }
        }
    }
    db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
}

// ---------------------------------------------------------------------------------------------
// Reset de esquema
// ---------------------------------------------------------------------------------------------

/// `sqlite3CollapseDatabaseArray`: percorre `db.dbs` e remove os bancos que foram fechados. As
/// entradas 0 (main) e 1 (temp) nunca são candidatas.
pub fn collapse_database_array(db: &mut Connection) {
    let mut j = 2usize;
    let mut i = 2usize;
    while i < db.dbs.len() {
        if db.dbs[i].bt.is_none() {
            db.dbs[i].z_db_s_name = Vec::new();
        } else {
            if j < i {
                db.dbs.swap(j, i);
            }
            j += 1;
        }
        i += 1;
    }
    db.dbs.truncate(j);
}

/// `sqlite3ResetOneSchema`: reseta o esquema do banco `i_db` e também o TEMP. O reset é adiado
/// se `db.n_schema_lock` não for zero; os resets adiados rodam chamando com `i_db < 0`.
pub fn reset_one_schema(db: &mut Connection, i_db: i32) {
    debug_assert!(i_db < db.dbs.len() as i32);

    if i_db >= 0 {
        db.db_set_property(i_db as usize, DB_RESETWANTED);
        db.db_set_property(1, DB_RESETWANTED);
        db.m_db_flags &= !DBFLAG_SCHEMA_KNOWN_OK;
    }

    if db.n_schema_lock == 0 {
        for i in 0..db.dbs.len() {
            if db.db_has_property(i, DB_RESETWANTED) {
                schema_clear(db, i);
            }
        }
    }
}

/// `sqlite3ResetAllSchemasOfConnection`: apaga todo o esquema de todos os bancos (incluindo main
/// e temp) de uma conexão.
pub fn reset_all_schemas_of_connection(db: &mut Connection) {
    for i in 0..db.dbs.len() {
        if db.n_schema_lock == 0 {
            schema_clear(db, i);
        } else {
            db.db_set_property(i, DB_RESETWANTED);
        }
    }
    db.m_db_flags &= !(DBFLAG_SCHEMA_CHANGE | DBFLAG_SCHEMA_KNOWN_OK);
    vtab_unlock_list(db);
    if db.n_schema_lock == 0 {
        collapse_database_array(db);
    }
}

/// `sqlite3CommitInternalChanges`: chamada quando acontece um commit.
pub fn commit_internal_changes(db: &mut Connection) {
    db.m_db_flags &= !DBFLAG_SCHEMA_CHANGE;
}

// ---------------------------------------------------------------------------------------------
// Colunas: expressão, colação, remoção dos nomes
// ---------------------------------------------------------------------------------------------

/// `sqlite3ColumnSetExpr`: define a expressão associada à coluna `i_col`. Em geral é o DEFAULT,
/// mas pode ser a expressão que calcula uma coluna gerada.
pub fn column_set_expr(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &mut Table,
    i_col: usize,
    p_expr: Option<Box<Expr>>,
) {
    debug_assert!(p_tab.is_ordinary_table());
    let i_dflt = p_tab.a_col[i_col].i_dflt as usize;
    let TableU::Tab(info) = &mut p_tab.u else {
        return;
    };
    let n_expr = info.p_dflt_list.as_ref().map(|l| l.a.len());
    match n_expr {
        Some(n) if i_dflt != 0 && n >= i_dflt => {
            if let Some(list) = info.p_dflt_list.as_deref_mut() {
                list.a[i_dflt - 1].p_expr = p_expr;
            }
        }
        _ => {
            let novo = n_expr.map_or(1, |n| n + 1);
            let list = info.p_dflt_list.take();
            info.p_dflt_list = expr_list_append(list, p_expr);
            p_tab.a_col[i_col].i_dflt = novo as u16;
        }
    }
}

/// `sqlite3ColumnExpr`: a expressão associada à coluna (a cláusula DEFAULT ou o AS de uma coluna
/// gerada). `None` se a coluna não tem expressão.
pub fn column_expr<'a>(p_tab: &'a Table, p_col: &Column) -> Option<&'a Expr> {
    if p_col.i_dflt == 0 {
        return None;
    }
    if !p_tab.is_ordinary_table() {
        return None;
    }
    let list = p_tab.u_tab()?.p_dflt_list.as_deref()?;
    let i_dflt = p_col.i_dflt as usize;
    if list.a.len() < i_dflt {
        return None;
    }
    list.a[i_dflt - 1].p_expr.as_deref()
}

/// Posição do primeiro byte depois do nome (e do tipo, se houver) em `Column.z_cn_name`.
fn column_coll_offset(p_col: &Column) -> usize {
    let z = &p_col.z_cn_name;
    let mut n = strlen30(z) as usize + 1;
    if (p_col.col_flags & COLFLAG_HASTYPE) != 0 {
        n += strlen30(z.get(n..).unwrap_or(&[])) as usize + 1;
    }
    n
}

/// `sqlite3ColumnSetColl`: define o nome da colação da coluna.
pub fn column_set_coll(p_col: &mut Column, z_coll: &[u8]) {
    let n = column_coll_offset(p_col);
    let n_coll = strlen30(z_coll) as usize;
    p_col.z_cn_name.truncate(n);
    p_col.z_cn_name.extend_from_slice(&z_coll[..n_coll]);
    p_col.z_cn_name.push(0);
    p_col.col_flags |= COLFLAG_HASCOLL;
}

/// `sqlite3ColumnColl`: o nome da colação da coluna, sem o NUL final; `None` se não tem.
pub fn column_coll(p_col: &Column) -> Option<&[u8]> {
    if (p_col.col_flags & COLFLAG_HASCOLL) == 0 {
        return None;
    }
    let n = column_coll_offset(p_col);
    let rest = p_col.z_cn_name.get(n..).unwrap_or(&[]);
    Some(&rest[..strlen30(rest) as usize])
}

/// `sqlite3DeleteColumnNames`: libera a memória das colunas de uma tabela ou view
/// (`Table.a_col`) e da lista de DEFAULT.
pub fn delete_column_names(p_table: &mut Table) {
    if !p_table.a_col.is_empty() {
        p_table.a_col = Vec::new();
        p_table.n_col = 0;
        if let TableU::Tab(info) = &mut p_table.u {
            info.p_dflt_list = None;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Remoção de tabelas
// ---------------------------------------------------------------------------------------------

/// `deleteTable`: remove as estruturas em memória da tabela `p_table`. Nada muda em disco. Não
/// tira a tabela da tabela hash do esquema, mas desfaz os índices, as chaves estrangeiras e o
/// estado de tabela virtual que dependem dela.
fn delete_table_body(db: &mut Connection, p_table: &Table) {
    // Tira todos os índices da tabela hash de índices do esquema.
    if !p_table.p_index.is_empty() && !p_table.is_virtual() {
        let i_db = schema_to_index(db, p_table.p_schema);
        if i_db >= 0 && (i_db as usize) < db.dbs.len() {
            for p_index in p_table.p_index.iter() {
                hash_insert(&mut db.dbs[i_db as usize].schema.idx_hash, &p_index.z_name, None);
            }
        }
    }

    if p_table.is_ordinary_table() {
        fk_delete(db, p_table);
    } else if p_table.is_virtual() {
        vtab_clear(db, p_table);
    } else {
        // A view: a subconsulta (`u.view.p_select`) é liberada com a tabela.
        debug_assert!(p_table.is_view());
    }
    // O resto (colunas, nomes, CHECK) é liberado pelo `Drop`.
}

/// `sqlite3DeleteTable`: só desfaz a tabela quando a última referência (`Rc`) some; as demais
/// chamadas só largam a sua.
pub fn delete_table(db: &mut Connection, p_table: Option<Rc<Table>>) {
    let Some(t) = p_table else {
        return;
    };
    if Rc::strong_count(&t) > 1 {
        return;
    }
    delete_table_body(db, &t);
}

/// `sqlite3UnlinkAndDeleteTable`: tira a tabela da tabela hash e a apaga, com todos os seus
/// índices e chaves estrangeiras.
pub fn unlink_and_delete_table(db: &mut Connection, i_db: usize, z_tab_name: &[u8]) {
    debug_assert!(i_db < db.dbs.len());
    let p = hash_insert(&mut db.dbs[i_db].schema.tbl_hash, z_tab_name, None);
    delete_table(db, p);
    db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
}

// ---------------------------------------------------------------------------------------------
// Nomes: token, banco, duas partes
// ---------------------------------------------------------------------------------------------

/// `sqlite3NameFromToken`: o texto do token, sem as aspas (`"nome"`, `'nome'`, `[nome]` ou
/// `` `nome` ``) que cercam o corpo. `None` se não há token.
pub fn name_from_token(p_name: Option<&Token>) -> Option<Vec<u8>> {
    let t = p_name?;
    let mut z = t.z.clone();
    dequote(&mut z);
    let n = strlen30(&z) as usize;
    z.truncate(n);
    Some(z)
}

/// `sqlite3OpenSchemaTable`: abre para escrita, com o cursor 0, a tabela sqlite_schema do banco
/// `i_db`.
pub fn open_schema_table(db: &mut Connection, p: &mut Parse, i_db: i32) {
    let have = { get_vdbe(db, p); true };
    table_lock(db, p, i_db, SCHEMA_ROOT, true, LEGACY_SCHEMA_TABLE);
    if have {
        if let Some(v) = p.p_vdbe.as_deref_mut() {
            add_op4_int(v, OP_OPENWRITE, 0, SCHEMA_ROOT as i32, i_db, 5);
        }
    }
    if p.n_tab == 0 {
        p.n_tab = 1;
    }
}

/// `sqlite3FindDbName`: o índice em `db.dbs` do banco chamado `z_name` ("main", "temp" ou o nome
/// de um anexado), ou -1 se não existe.
pub fn find_db_name(db: &Connection, z_name: Option<&[u8]>) -> i32 {
    let mut i: i32 = -1;
    if let Some(z_name) = z_name {
        i = db.dbs.len() as i32 - 1;
        while i >= 0 {
            if 0 == stricmp(Some(&db.dbs[i as usize].z_db_s_name), Some(z_name)) {
                break;
            }
            // "main" é sempre um apelido aceitável para o banco primário, mesmo que ele tenha
            // sido renomeado com SQLITE_DBCONFIG_MAINDBNAME.
            if i == 0 && 0 == stricmp(Some(b"main"), Some(z_name)) {
                break;
            }
            i -= 1;
        }
    }
    i
}

/// `sqlite3FindDb`: o token contém o nome de um banco; devolve o índice dele em `db.dbs`, ou -1.
pub fn find_db(db: &Connection, p_name: &Token) -> i32 {
    let z_name = name_from_token(Some(p_name));
    find_db_name(db, z_name.as_deref())
}

/// `sqlite3TwoPartName`: o nome de uma tabela, view ou gatilho chega em dois tokens. Com
/// `CREATE TABLE xxx.yyy (...)`, `name1` é "xxx" e `name2` é "yyy"; com `CREATE TABLE yyy(...)`,
/// `name1` é "yyy" e `name2` é vazio. Devolve o índice do banco e o token que guarda o nome sem
/// qualificação. `None` quando houve erro (o C devolve -1).
pub fn two_part_name<'a>(
    db: &mut Connection,
    parse: &mut Parse,
    name1: &'a Token,
    name2: &'a Token,
) -> Option<(i32, &'a Token)> {
    if !name2.z.is_empty() {
        if db.init.busy != 0 {
            error_msg(db, parse, b"corrupt database", &[]);
            return None;
        }
        let i_db = find_db(db, name1);
        if i_db < 0 {
            let arg = token_arg(parse, name1);
            error_msg(db, parse, b"unknown database %T", &[arg]);
            return None;
        }
        Some((i_db, name2))
    } else {
        debug_assert!(
            db.init.i_db == 0
                || db.init.busy != 0
                || parse.in_special_parse()
                || (db.m_db_flags & DBFLAG_VACUUM) != 0
        );
        Some((db.init.i_db as i32, name1))
    }
}

/// `sqlite3WritableSchema`: verdadeiro se `PRAGMA writable_schema` está ligado (e `defensive`
/// desligado).
pub fn writable_schema(db: &Connection) -> bool {
    (db.flags & (SQLITE_WRITE_SCHEMA | SQLITE_DEFENSIVE)) == SQLITE_WRITE_SCHEMA
}

/// `sqlite3CheckObjectName`: confere se `z_name` é um nome legal para um objeto novo do esquema
/// (tabela, índice, view ou gatilho): todos valem, menos os que começam com "sqlite_" (em
/// qualquer caixa), reservados para uso interno. Ao ler o sqlite_schema, confere também se as
/// colunas "type", "name" e "tbl_name" são coerentes com o SQL.
pub fn check_object_name(
    db: &mut Connection,
    parse: &mut Parse,
    z_name: &[u8],
    z_type: &[u8],
    z_tbl_name: &[u8],
) -> i32 {
    if writable_schema(db) || db.init.imposter_table || !extra_schema_checks() {
        // Com writable_schema=ON estas conferências não valem.
        return SQLITE_OK;
    }
    if db.init.busy != 0 {
        let az = |i: usize| db.init.az_init.get(i).and_then(|o| o.as_deref());
        if stricmp(Some(z_type), az(0)) != 0
            || stricmp(Some(z_name), az(1)) != 0
            || stricmp(Some(z_tbl_name), az(2)) != 0
        {
            error_msg(db, parse, b"", &[]); // O corruptSchema() entrega o erro.
            return SQLITE_ERROR;
        }
    } else if (parse.nested == 0 && 0 == strnicmp(Some(z_name), Some(b"sqlite_"), 7))
        || (read_only_shadow_tables(db) && shadow_table_name(db, z_name))
    {
        error_msg(
            db,
            parse,
            b"object name reserved for internal use: %s",
            &[text_arg(z_name)],
        );
        return SQLITE_ERROR;
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Índices e colunas: mapeamentos
// ---------------------------------------------------------------------------------------------

/// `sqlite3PrimaryKeyIndex`: o índice da PRIMARY KEY da tabela.
pub fn primary_key_index(p_tab: &Table) -> Option<&Rc<Index>> {
    p_tab.p_index.iter().find(|p| p.is_primary_key_index())
}

/// `sqlite3TableColumnToIndex`: converte o número de uma coluna da tabela (como no CREATE TABLE)
/// em número de coluna do índice: a posição da primeira ocorrência da coluna `i_col` em `p_idx`,
/// ou -1 se o índice não a usa.
pub fn table_column_to_index(p_idx: &Index, i_col: i16) -> i16 {
    for i in 0..p_idx.n_column as usize {
        if i_col == p_idx.ai_column[i] {
            return i as i16;
        }
    }
    -1
}

/// `sqlite3StorageColumnToTable`: converte o número de coluna de armazenamento (a posição do
/// valor no registro em disco) no número de coluna da tabela (a posição no CREATE TABLE). O de
/// armazenamento é menor se e só se há colunas VIRTUAL à esquerda.
pub fn storage_column_to_table(p_tab: &Table, i_col: i16) -> i16 {
    let mut i_col = i_col;
    if (p_tab.tab_flags & TF_HAS_VIRTUAL) != 0 {
        let mut i: i16 = 0;
        while i <= i_col {
            if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
                i_col += 1;
            }
            i += 1;
        }
    }
    i_col
}

/// `sqlite3TableColumnToStorage`: converte o número de coluna da tabela no de armazenamento. Se
/// a coluna é a N-ésima VIRTUAL (começando em zero), o número é a quantidade de colunas não
/// VIRTUAL mais N: as colunas VIRTUAL vão para o fim. Com `CREATE TABLE ex(N,S,V,N,S,V,N,S,V)`
/// as entradas 0 a 8 viram 0 1 6 2 3 7 4 5 8. Se `i_col` é negativo (o ROWID) devolve `i_col`.
pub fn table_column_to_storage(p_tab: &Table, i_col: i16) -> i16 {
    debug_assert!(i_col < p_tab.n_col);
    if (p_tab.tab_flags & TF_HAS_VIRTUAL) == 0 || i_col < 0 {
        return i_col;
    }
    let mut n: i16 = 0;
    let mut i: i16 = 0;
    while i < i_col {
        if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) == 0 {
            n += 1;
        }
        i += 1;
    }
    if (p_tab.a_col[i as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
        // `i_col` é ela mesma uma coluna virtual.
        p_tab.n_nv_col + i - n
    } else {
        // `i_col` é uma coluna normal ou armazenada.
        n
    }
}

// ---------------------------------------------------------------------------------------------
// CREATE TABLE
// ---------------------------------------------------------------------------------------------

/// `sqlite3ForceNotReadOnly`: insere um único `OP_JournalMode` de consulta para forçar o comando
/// preparado a devolver falso em `sqlite3_stmt_readonly()`. Serve a CREATE TABLE IF NOT EXISTS e
/// parecidos quando a tabela já existe, para o comando (um no-op de leitura) ainda assim contar
/// como de escrita.
pub(crate) fn force_not_read_only(db: &mut Connection, parse: &mut Parse) {
    parse.n_mem += 1;
    let i_reg = parse.n_mem;
    {
        get_vdbe(db, parse);
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            add_op3(v, OP_JOURNALMODE, 0, i_reg, PAGER_JOURNALMODE_QUERY);
            uses_btree(v, 0);
        }
    }
}

/// `sqlite3StartTable`: começa a construir a representação em memória de uma tabela nova. É a
/// primeira das ações chamadas em resposta a um CREATE TABLE: roda depois dos tokens CREATE TABLE
/// e do nome. `is_temp` é verdadeiro se a tabela vai para o arquivo auxiliar (TEMP/TEMPORARY
/// entre CREATE e TABLE). A tabela nova entra em `parse.p_new_table`, e as ações seguintes
/// acrescentam informação a ela até `sqlite3EndTable`.
#[allow(clippy::too_many_arguments)]
pub fn start_table(
    db: &mut Connection,
    parse: &mut Parse,
    name1: &Token,
    name2: &Token,
    is_temp: i32,
    is_view: i32,
    is_virtual: i32,
    no_err: i32,
) {
    let mut is_temp = is_temp;
    let (i_db, p_name, z_name): (i32, &Token, Option<Vec<u8>>) =
        if db.init.busy != 0 && db.init.new_tnum == 1 {
            // Caso especial: analisando o esquema do sqlite_schema ou sqlite_temp_schema.
            let i_db = db.init.i_db as i32;
            (i_db, name1, Some(schema_table(i_db).to_vec()))
        } else {
            // O caso comum.
            let Some((d, p_name)) = two_part_name(db, parse, name1, name2) else {
                return;
            };
            let mut i_db = d;
            if OMIT_TEMPDB == 0 && is_temp != 0 && !name2.z.is_empty() && i_db != 1 {
                // Ao criar tabela temporária o nome não pode ser qualificado, a não ser que o
                // banco se chame "temp" de qualquer jeito.
                error_msg(db, parse, b"temporary table name must be unqualified", &[]);
                return;
            }
            if OMIT_TEMPDB == 0 && is_temp != 0 {
                i_db = 1;
            }
            let z_name = name_from_token(Some(p_name));
            if parse.in_rename_object() {
                if let Some(z) = &z_name {
                    rename_token_map(parse, z.as_ptr() as usize, p_name);
                }
            }
            (i_db, p_name, z_name)
        };
    parse.s_name_token = p_name.clone();
    let Some(z_name) = z_name else {
        return;
    };

    'begin_table_error: {
        let z_kind: &[u8] = if is_view != 0 { b"view" } else { b"table" };
        if check_object_name(db, parse, &z_name, z_kind, &z_name) != 0 {
            break 'begin_table_error;
        }
        if db.init.i_db == 1 {
            is_temp = 1;
        }
        {
            debug_assert!(is_temp == 0 || is_temp == 1);
            debug_assert!(is_view == 0 || is_view == 1);
            const A_CODE: [i32; 4] = [
                SQLITE_CREATE_TABLE,
                SQLITE_CREATE_TEMP_TABLE,
                SQLITE_CREATE_VIEW,
                SQLITE_CREATE_TEMP_VIEW,
            ];
            let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
            if auth_check(db, parse, SQLITE_INSERT, Some(schema_table(is_temp)), None, Some(&z_db))
                != 0
            {
                break 'begin_table_error;
            }
            if is_virtual == 0
                && auth_check(
                    db,
                    parse,
                    A_CODE[(is_temp + 2 * is_view) as usize],
                    Some(&z_name),
                    None,
                    Some(&z_db),
                ) != 0
            {
                break 'begin_table_error;
            }
        }

        // Garante que o nome novo não colide com um índice ou tabela existente no mesmo banco. A
        // exceção é um comando passado a sqlite3_declare_vtab(): ali só valem os nomes e tipos
        // das colunas, e não há o que testar.
        if !parse.in_special_parse() {
            let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
            if SQLITE_OK != read_schema(db, parse) {
                break 'begin_table_error;
            }
            if let Some(p_table) = find_table(db, &z_name, Some(&z_db)) {
                if no_err == 0 {
                    let z_what: &[u8] = if p_table.is_view() { b"view" } else { b"table" };
                    let arg_name = token_arg(parse, p_name);
                    error_msg(
                        db,
                        parse,
                        b"%s %T already exists",
                        &[text_arg(z_what), arg_name],
                    );
                } else {
                    debug_assert!(db.init.busy == 0);
                    code_verify_schema(db, parse, i_db);
                    force_not_read_only(db, parse);
                }
                break 'begin_table_error;
            }
            if find_index(db, &z_name, Some(&z_db)).is_some() {
                error_msg(
                    db,
                    parse,
                    b"there is already an index named %s",
                    &[text_arg(&z_name)],
                );
                break 'begin_table_error;
            }
        }

        let mut p_table = TableBuilder::default();
        p_table.z_name = z_name;
        p_table.i_p_key = -1;
        p_table.p_schema = db.dbs[i_db as usize].schema.id;
        p_table.n_row_log_est = 200; // 200 é sqlite3LogEst(1048576)
        debug_assert!(parse.p_new_table.is_none());
        parse.p_new_table = Some(Box::new(p_table));

        // Começa a gerar o código que insere o registro da tabela no sqlite_schema. É preciso
        // alocar já o número do registro da tabela, antes de qualquer PRIMARY KEY ou UNIQUE:
        // essas palavras criam índices, e o registro da tabela precisa vir antes dos deles.
        if db.init.busy == 0 {
            get_vdbe(db, parse);
            // `NULL_ROW` é a codificação de OP_Record de uma linha com 5 NULLs.
            const NULL_ROW: [u8; 6] = [6, 0, 0, 0, 0, 0];
            begin_write_operation(db, parse, 1, i_db);

            if is_virtual != 0 {
                if let Some(v) = parse.p_vdbe.as_deref_mut() {
                    add_op0(v, OP_VBEGIN);
                }
            }

            // Se o formato de arquivo e a codificação do banco não foram definidos, define agora.
            parse.n_mem += 1;
            parse.reg_rowid = parse.n_mem;
            let reg1 = parse.reg_rowid;
            parse.n_mem += 1;
            parse.reg_root = parse.n_mem;
            let reg2 = parse.reg_root;
            parse.n_mem += 1;
            let reg3 = parse.n_mem;
            let file_format = if (db.flags & SQLITE_LEGACY_FILE_FMT) != 0 {
                1
            } else {
                SQLITE_MAX_FILE_FORMAT
            };
            let enc = db.enc as i32;
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                add_op3(v, OP_READCOOKIE, i_db, reg3, BTREE_FILE_FORMAT as i32);
                uses_btree(v, i_db);
                let addr1 = add_op1(v, OP_IF, reg3);
                add_op3(v, OP_SETCOOKIE, i_db, BTREE_FILE_FORMAT as i32, file_format);
                add_op3(v, OP_SETCOOKIE, i_db, BTREE_TEXT_ENCODING as i32, enc);
                jump_here(v, addr1);

                // Isto só cria um registro reservado na sqlite_schema, ainda sem conteúdo. Ele
                // será trocado pela entrada de verdade no código gerado por sqlite3EndTable().
                // O rowid fica em `reg_rowid` e a página raiz da tabela em `reg_root`; os dois
                // valores são necessários ao código que sqlite3EndTable gera.
                if is_view != 0 || is_virtual != 0 {
                    add_op2(v, OP_INTEGER, 0, reg2);
                } else {
                    debug_assert!(parse.b_returning == 0);
                    parse.addr_cr_tab = add_op3(v, OP_CREATEBTREE, i_db, reg2, BTREE_INTKEY as i32);
                }
            }
            open_schema_table(db, parse, i_db);
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                add_op2(v, OP_NEWROWID, 0, reg1);
                add_op4(v, OP_BLOB, 6, reg3, 0, P4::Blob(NULL_ROW.to_vec()));
                add_op3(v, OP_INSERT, 0, reg3, reg1);
                change_p5(v, OPFLAG_APPEND);
                add_op0(v, OP_CLOSE);
            }
        }

        // Retorno normal (sem erro).
        return;
    }

    // Se há um erro, o fluxo chega aqui.
    parse.check_schema = 1;
}

/// Executa `f` com a tabela em construção (`parse.p_new_table`) fora do `Parse`, para que `f`
/// possa usar o `Parse` inteiro (mensagens de erro, lista de expressões) enquanto altera a
/// tabela. Devolve `None` se não há tabela em construção.
pub(crate) fn with_new_table<R>(
    parse: &mut Parse,
    f: impl FnOnce(&mut Parse, &mut TableBuilder) -> R,
) -> Option<R> {
    let mut p = parse.p_new_table.take()?;
    let r = f(parse, &mut p);
    parse.p_new_table = Some(p);
    Some(r)
}

/// `sqlite3DeleteReturning`: limpeza do RETURNING. Tira o gatilho transitório da tabela hash de
/// gatilhos do esquema TEMP. O restante (a lista de expressões) é liberado pelo `Drop`. Quem
/// reinicia o `Parse` (o equivalente de `sqlite3ParseObjectReset`) precisa chamar isto com
/// `parse.p_returning.take()`, no lugar da lista de limpeza do C.
pub(crate) fn delete_returning(db: &mut Connection, p_ret: Option<Box<Returning>>) {
    let Some(p_ret) = p_ret else {
        return;
    };
    if let Some(temp) = db.dbs.get_mut(1) {
        hash_insert(&mut temp.schema.trig_hash, &p_ret.z_name, None);
    }
}

/// `sqlite3AddReturning`: acrescenta a cláusula RETURNING ao comando em análise.
///
/// Cria um gatilho TEMP especial que dispara para cada linha do comando DML. Esse gatilho tem um
/// único SELECT cujo resultado é o argumento do RETURNING, a flag `b_returning` e o código de
/// operação TK_RETURNING em vez de TK_SELECT, para o gerador de código tratá-lo à parte. O
/// gatilho some sozinho no fim da análise. Como ainda não se sabe se o RETURNING é de DELETE,
/// INSERT ou UPDATE, o gatilho nasce como RETURNING e vira o tipo certo na primeira chamada de
/// `sqlite3TriggersExist()`.
///
/// O C compartilha `pList` entre `Returning.pReturnEL` e o passo do gatilho; aqui o passo guarda
/// uma cópia, e o gatilho existe em duas cópias idênticas (a do esquema TEMP e a de
/// `Returning.ret_trig`).
pub fn add_returning(db: &mut Connection, parse: &mut Parse, p_list: Option<Box<ExprList>>) {
    if parse.p_new_trigger.is_some() {
        error_msg(db, parse, b"cannot use RETURNING in a trigger", &[]);
    }
    parse.b_returning = 1;
    let z_name = format!("sqlite_returning_{:p}", &*parse as *const Parse).into_bytes();
    let tab_schema = db.dbs[1].schema.id;
    let step = TriggerStep {
        op: TK_RETURNING,
        p_expr_list: p_list.clone(),
        ..TriggerStep::default()
    };
    let ret_trig = Trigger {
        z_name: z_name.clone(),
        op: TK_RETURNING,
        tr_tm: TRIGGER_AFTER,
        b_returning: 1,
        p_schema: tab_schema,
        p_tab_schema: tab_schema,
        step_list: vec![step],
        ..Trigger::default()
    };
    let p_ret = Returning {
        p_return_el: p_list,
        ret_trig: ret_trig.clone(),
        z_name: z_name.clone(),
        ..Returning::default()
    };
    parse.p_returning = Some(Box::new(p_ret));
    // O C trata aqui a falha de alocação de `sqlite3HashInsert`; sem OOM em Rust, o valor
    // antigo (que não deveria existir) simplesmente é descartado.
    hash_insert(&mut db.dbs[1].schema.trig_hash, &z_name, Some(Rc::new(ret_trig)));
}

/// Parte de `add_column` que trabalha com a tabela em construção já fora do `Parse`.
fn add_column_to(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Table,
    s_name: Token,
    s_type: Token,
) {
    let mut s_name = s_name;
    let mut s_type = s_type;
    let mut e_type = COLTYPE_CUSTOM;
    let mut sz_est: u8 = 1;
    let mut affinity = SQLITE_AFF_BLOB;

    if p.n_col as i32 + 1 > db.a_limit[SQLITE_LIMIT_COLUMN as usize] {
        error_msg(db, parse, b"too many columns on %s", &[text_arg(&p.z_name)]);
        return;
    }
    if !parse.in_rename_object() {
        dequote_token(&mut s_name);
    }

    // Como as palavras-chave GENERATE ALWAYS podem virar identificadores no analisador, o nome
    // de tipo às vezes termina com "generated always". Detecta e descarta o texto a mais.
    let n_type = s_type.z.len();
    if n_type >= 16 && strnicmp(Some(&s_type.z[n_type - 6..]), Some(b"always"), 6) == 0 {
        let mut n = n_type - 6;
        while n > 0 && is_space(s_type.z[n - 1]) {
            n -= 1;
        }
        if n >= 9 && strnicmp(Some(&s_type.z[n - 9..]), Some(b"generated"), 9) == 0 {
            n -= 9;
            while n > 0 && is_space(s_type.z[n - 1]) {
                n -= 1;
            }
        }
        s_type.z.truncate(n);
    }

    // Procura os nomes de tipo padrão. Neles grava-se `Column.e_c_type` em vez de guardar o nome
    // do tipo depois do nome da coluna, para economizar espaço.
    if s_type.z.len() >= 3 {
        dequote_token(&mut s_type);
        for i in 0..SQLITE_N_STDTYPE {
            if s_type.z.len() == STD_TYPE_LEN[i] as usize
                && strnicmp(Some(&s_type.z), Some(STD_TYPE[i].as_bytes()), s_type.z.len() as i32)
                    == 0
            {
                s_type.z.clear();
                e_type = (i + 1) as u8;
                affinity = STD_TYPE_AFFINITY[i];
                if affinity <= SQLITE_AFF_TEXT {
                    sz_est = 5;
                }
                break;
            }
        }
    }

    let mut z: Vec<u8> = Vec::with_capacity(s_name.z.len() + 1 + s_type.z.len() + 1);
    z.extend_from_slice(&s_name.z);
    z.push(0);
    if parse.in_rename_object() {
        rename_token_map(parse, z.as_ptr() as usize, &s_name);
    }
    dequote(&mut z);
    let n_name = strlen30(&z) as usize;
    z.truncate(n_name);
    z.push(0);
    let h_name = str_i_hash(&z);
    for i in 0..p.n_col as usize {
        if p.a_col[i].h_name == h_name && str_icmp(&z, &p.a_col[i].z_cn_name) == 0 {
            error_msg(db, parse, b"duplicate column name: %s", &[text_arg(&z[..n_name])]);
            return;
        }
    }
    let mut p_col = Column { h_name, ..Column::default() };

    if s_type.z.is_empty() {
        // Sem tipo, a coluna tem a afinidade padrão BLOB e o tamanho estimado em 4 bytes.
        p_col.affinity = affinity;
        p_col.e_c_type = e_type;
        p_col.sz_est = sz_est;
    } else {
        let mut z_type = s_type.z.clone();
        z_type.push(0);
        dequote(&mut z_type);
        let n_t = strlen30(&z_type) as usize;
        z_type.truncate(n_t);
        z_type.push(0);
        p_col.affinity = affinity_type(&z_type, Some(&mut p_col));
        p_col.col_flags |= COLFLAG_HASTYPE;
        z.extend_from_slice(&z_type);
    }
    p_col.z_cn_name = z;
    p.a_col.push(p_col);
    p.n_col += 1;
    p.n_nv_col += 1;
    parse.constraint_name.z.clear();
}

/// `sqlite3AddColumn`: acrescenta uma coluna à tabela em construção. O analisador chama uma vez
/// por declaração de coluna do CREATE TABLE; `sqlite3StartTable()` roda antes.
pub fn add_column(db: &mut Connection, parse: &mut Parse, s_name: Token, s_type: Token) {
    with_new_table(parse, |parse, p| add_column_to(db, parse, p, s_name, s_type));
}

/// `sqlite3AddNotNull`: chamada pelo analisador no meio de um CREATE TABLE, depois de ver um
/// NOT NULL numa coluna. Liga `not_null` na coluna em construção.
pub fn add_not_null(parse: &mut Parse, on_error: i32) {
    let Some(p) = parse.p_new_table.as_deref_mut() else {
        return;
    };
    if p.n_col < 1 {
        return;
    }
    let i_last = p.n_col - 1;
    let p_col = &mut p.a_col[i_last as usize];
    p_col.not_null = on_error as u8;
    let is_unique = (p_col.col_flags & COLFLAG_UNIQUE) != 0;
    p.tab_flags |= TF_HAS_NOT_NULL;

    // Liga `uniq_not_null` nos índices UNIQUE ou PK que já foram criados sobre esta coluna.
    if is_unique {
        for p_idx in p.p_index.iter_mut() {
            debug_assert!(p_idx.n_key_col == 1 && p_idx.on_error != OE_NONE);
            if p_idx.ai_column[0] == i_last {
                Rc::make_mut(p_idx).uniq_not_null = true;
            }
        }
    }
}

/// Junta quatro letras no valor que `h` assume ao lê-las (`'c'<<24 | 'h'<<16 | 'a'<<8 | 'r'`).
const fn four_chars(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) + ((b as u32) << 16) + ((c as u32) << 8) + d as u32
}

/// `sqlite3AffinityType`: examina o nome de tipo de coluna `z_in` e devolve a afinidade. A busca
/// ignora a caixa e procura as subcadeias da tabela abaixo; se houver mais de uma, vale a de
/// cima. Por exemplo, 'BLOBINT' dá SQLITE_AFF_INTEGER.
///
/// ```text
/// 'INT'  -> INTEGER    'CHAR' -> TEXT    'CLOB' -> TEXT    'TEXT' -> TEXT
/// 'BLOB' -> BLOB       'REAL' -> REAL    'FLOA' -> REAL    'DOUB' -> REAL
/// ```
///
/// Nenhuma das subcadeias dá SQLITE_AFF_NUMERIC. Se `p_col` não é `None`, grava nele uma
/// estimativa do tamanho do campo, na escala em que o tamanho de um inteiro vale 1.
pub fn affinity_type(z_in: &[u8], p_col: Option<&mut Column>) -> u8 {
    let mut h: u32 = 0;
    let mut aff = SQLITE_AFF_NUMERIC;
    let mut z_char: Option<usize> = None;
    let mut i = 0usize;

    while at(z_in, i) != 0 {
        let x = at(z_in, i);
        h = (h << 8).wrapping_add(UPPER_TO_LOWER[x as usize] as u32);
        i += 1;
        if h == four_chars(b'c', b'h', b'a', b'r') {
            aff = SQLITE_AFF_TEXT; // CHAR
            z_char = Some(i);
        } else if h == four_chars(b'c', b'l', b'o', b'b') {
            aff = SQLITE_AFF_TEXT; // CLOB
        } else if h == four_chars(b't', b'e', b'x', b't') {
            aff = SQLITE_AFF_TEXT; // TEXT
        } else if h == four_chars(b'b', b'l', b'o', b'b')
            && (aff == SQLITE_AFF_NUMERIC || aff == SQLITE_AFF_REAL)
        {
            aff = SQLITE_AFF_BLOB; // BLOB
            if at(z_in, i) == b'(' {
                z_char = Some(i);
            }
        } else if h == four_chars(b'r', b'e', b'a', b'l') && aff == SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_REAL; // REAL
        } else if h == four_chars(b'f', b'l', b'o', b'a') && aff == SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_REAL; // FLOA
        } else if h == four_chars(b'd', b'o', b'u', b'b') && aff == SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_REAL; // DOUB
        } else if (h & 0x00FF_FFFF) == ((b'i' as u32) << 16) + ((b'n' as u32) << 8) + b't' as u32 {
            aff = SQLITE_AFF_INTEGER; // INT
            break;
        }
    }

    // Se `p_col` não é nulo, guarda uma estimativa do tamanho do campo, na escala em que o
    // tamanho de um inteiro vale 1.
    if let Some(p_col) = p_col {
        let mut v: i32 = 0; // o tamanho padrão é uns 4 bytes
        if aff < SQLITE_AFF_NUMERIC {
            if let Some(mut k) = z_char {
                while at(z_in, k) != 0 {
                    if is_digit(at(z_in, k)) {
                        // BLOB(k), VARCHAR(k), CHAR(k) -> r=(k/4+1)
                        if let Some(x) = get_int32(&z_in[k..]) {
                            v = x;
                        }
                        break;
                    }
                    k += 1;
                }
            } else {
                v = 16; // BLOB, TEXT, CLOB -> r=5 (uns 20 bytes)
            }
        }
        v = v / 4 + 1;
        if v > 255 {
            v = 255;
        }
        p_col.sz_est = v as u8;
    }
    aff
}

/// `sqlite3AddDefaultValue`: a expressão é o valor DEFAULT da coluna mais recente da tabela em
/// construção. Os valores padrão precisam ser constantes, senão é um erro. Chamada pelo
/// analisador no meio de um CREATE TABLE. `z_span` é o texto do valor (de `zStart` até `zEnd`).
pub fn add_default_value(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<Box<Expr>>,
    z_span: &[u8],
) {
    let mut p_expr = p_expr;
    with_new_table(parse, |parse, p| {
        let is_init = (db.init.busy != 0 && db.init.i_db != 1) as u8;
        debug_assert!(p.n_col >= 1);
        let i_col = (p.n_col - 1) as usize;
        if expr_is_constant_or_function(p_expr.as_deref_mut(), is_init) == 0 {
            let z_col_name = p.a_col[i_col].z_cn_name.clone();
            error_msg(
                db,
                parse,
                b"default value of column [%s] is not constant",
                &[text_arg(&z_col_name)],
            );
        } else if (p.a_col[i_col].col_flags & COLFLAG_GENERATED) != 0 {
            error_msg(db, parse, b"cannot use DEFAULT on a generated column", &[]);
        } else {
            // Usa-se uma cópia de `p_expr` em vez do original, porque ele contém tokens que
            // apontam para memória volátil.
            let x = Expr {
                op: TK_SPAN,
                u: ExprU::Token(Some(db_span_dup(z_span))),
                p_left: p_expr.take(),
                flags: EP_SKIP,
                ..Expr::default()
            };
            let p_dflt_expr = expr_dup(Some(&x), EXPRDUP_REDUCE);
            p_expr = x.p_left;
            column_set_expr(db, parse, p, i_col, p_dflt_expr);
        }
    });
    if parse.in_rename_object() {
        if let Some(e) = p_expr.as_deref() {
            rename_expr_unmap(parse, e);
        }
    }
}

/// `sqlite3StringToId`: compatibilidade com o passado. As versões antigas aceitavam strings como
/// nomes de coluna em índices, PRIMARY KEY e UNIQUE (`PRIMARY KEY('a')`,
/// `UNIQUE('b','c' COLLATE trim)`, `CREATE INDEX abc ON xyz('c','d' DESC)`). É esquisito, mas
/// continua aceito: converte a expressão de TK_STRING em TK_ID se ela é só um TK_STRING com um
/// COLLATE opcional. Qualquer outra expressão fica como está.
pub fn string_to_id(p: &mut Expr) {
    if p.op == TK_STRING {
        p.op = TK_ID;
    } else if p.op == TK_COLLATE {
        if let Some(l) = p.p_left.as_deref_mut() {
            if l.op == TK_STRING {
                l.op = TK_ID;
            }
        }
    }
}

/// `makeColumnPartOfPrimaryKey`: marca a coluna como parte da PRIMARY KEY.
pub(crate) fn make_column_part_of_primary_key(db: &mut Connection, parse: &mut Parse, p_col: &mut Column) {
    p_col.col_flags |= COLFLAG_PRIMKEY;
    if (p_col.col_flags & COLFLAG_GENERATED) != 0 {
        error_msg(db, parse, b"generated columns cannot be part of the PRIMARY KEY", &[]);
    }
}

/// `sqlite3AddPrimaryKey`: define a PRIMARY KEY da tabela. `p_list` é a lista dos nomes de coluna
/// da chave; se é `None`, a chave é a coluna mais recente. Uma tabela tem no máximo uma chave
/// primária; a segunda é um erro.
///
/// Se a chave é uma só coluna do tipo INTEGER, tenta usá-la como rowid e grava seu índice em
/// `Table.i_p_key` (que fica em -1 sem INTEGER PRIMARY KEY). Se não for uma INTEGER PRIMARY KEY,
/// cria um índice único para a chave. Para INTEGER PRIMARY KEY não se cria índice.
pub fn add_primary_key(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: Option<Box<ExprList>>,
    on_error: i32,
    auto_inc: i32,
    sort_order: i32,
) {
    let mut p_list = p_list;
    let Some(mut p_tab) = parse.p_new_table.take() else {
        return;
    };
    let mut create_index_needed = false;
    'primary_key_exit: {
        if (p_tab.tab_flags & TF_HAS_PRIMARY_KEY) != 0 {
            error_msg(
                db,
                parse,
                b"table \"%s\" has more than one primary key",
                &[text_arg(&p_tab.z_name)],
            );
            break 'primary_key_exit;
        }
        p_tab.tab_flags |= TF_HAS_PRIMARY_KEY;
        let mut p_col: Option<usize> = None;
        let mut i_col: i32 = -1;
        let n_term: usize;
        match p_list.as_deref_mut() {
            None => {
                i_col = p_tab.n_col as i32 - 1;
                if i_col >= 0 {
                    p_col = Some(i_col as usize);
                    make_column_part_of_primary_key(db, parse, &mut p_tab.a_col[i_col as usize]);
                }
                n_term = 1;
            }
            Some(list) => {
                n_term = list.a.len();
                for i in 0..n_term {
                    let Some(e) = list.a[i].p_expr.as_deref_mut() else {
                        continue;
                    };
                    let p_c_expr = expr_skip_collate_mut(e);
                    string_to_id(p_c_expr);
                    if p_c_expr.op == TK_ID {
                        debug_assert!(!p_c_expr.has_property(EP_INT_VALUE));
                        let Some(z_c_name) = p_c_expr.z_token() else {
                            continue;
                        };
                        let n_col = p_tab.n_col as usize;
                        let mut k = 0usize;
                        while k < n_col {
                            if str_icmp(z_c_name, &p_tab.a_col[k].z_cn_name) == 0 {
                                p_col = Some(k);
                                make_column_part_of_primary_key(db, parse, &mut p_tab.a_col[k]);
                                break;
                            }
                            k += 1;
                        }
                        i_col = k as i32;
                    }
                }
            }
        }
        if n_term == 1
            && p_col.map_or(false, |c| p_tab.a_col[c].e_c_type == COLTYPE_INTEGER)
            && sort_order != SQLITE_SO_DESC
        {
            if parse.in_rename_object() {
                if let Some(list) = p_list.as_deref_mut() {
                    if let Some(e) = list.a[0].p_expr.as_deref_mut() {
                        let p_c_expr = expr_skip_collate_mut(e);
                        rename_token_remap(
                            parse,
                            &p_tab.i_p_key as *const i16 as usize,
                            p_c_expr as *const Expr as usize,
                        );
                    }
                }
            }
            p_tab.i_p_key = i_col as i16;
            p_tab.key_conf = on_error as u8;
            debug_assert!(auto_inc == 0 || auto_inc == 1);
            p_tab.tab_flags |= (auto_inc as u32) * TF_AUTOINCREMENT;
            if let Some(list) = p_list.as_deref() {
                parse.i_pk_sort_order = list.a[0].fg.sort_flags;
            }
            has_explicit_nulls(db, parse, p_list.as_deref());
        } else if auto_inc != 0 {
            error_msg(
                db,
                parse,
                b"AUTOINCREMENT is only allowed on an INTEGER PRIMARY KEY",
                &[],
            );
        } else {
            create_index_needed = true;
        }
    }
    parse.p_new_table = Some(p_tab);
    if create_index_needed {
        // O `create_index` consome a lista (o C zera `pList` depois da chamada).
        create_index(
            db,
            parse,
            None,
            None,
            None,
            p_list.take(),
            on_error,
            None,
            None,
            sort_order,
            0,
            SQLITE_IDXTYPE_PRIMARYKEY,
        );
    }
}

/// `sqlite3AddCheckConstraint`: acrescenta uma restrição CHECK à tabela em construção. `z_span`
/// é o texto que vai do "(" de abertura (incluído) até o ")" de fechamento (excluído).
pub fn add_check_constraint(
    db: &mut Connection,
    parse: &mut Parse,
    p_check_expr: Option<Box<Expr>>,
    z_span: &Token,
) {
    let Some(mut p_tab) = parse.p_new_table.take() else {
        return; // A expressão é liberada pelo `Drop`.
    };
    let readonly = db.dbs[db.init.i_db as usize].bt.as_ref().map_or(false, btree_is_readonly);
    if !parse.in_declare_vtab() && !readonly {
        p_tab.p_check = expr_list_append(p_tab.p_check.take(), p_check_expr);
        if !parse.constraint_name.z.is_empty() {
            let name = parse.constraint_name.clone();
            expr_list_set_name(parse, p_tab.p_check.as_deref_mut(), &name, 1);
        } else {
            let z = &z_span.z;
            let mut start = 1usize; // pula o "("
            while start < z.len() && is_space(z[start]) {
                start += 1;
            }
            let mut end = z.len();
            while end > start && is_space(z[end - 1]) {
                end -= 1;
            }
            let t = Token {
                z: z[start.min(end)..end].to_vec(),
                i_ofst: if z_span.i_ofst >= 0 { z_span.i_ofst + start as i32 } else { -1 },
            };
            expr_list_set_name(parse, p_tab.p_check.as_deref_mut(), &t, 1);
        }
    }
    parse.p_new_table = Some(p_tab);
}

/// `sqlite3AddCollateType`: define a função de colação da coluna mais recente da tabela em
/// construção.
pub fn add_collate_type(db: &mut Connection, parse: &mut Parse, p_token: &Token) {
    if parse.in_rename_object() {
        return;
    }
    with_new_table(parse, |parse, p| {
        if p.n_col < 1 {
            return;
        }
        let i = (p.n_col - 1) as usize;
        let Some(z_coll) = name_from_token(Some(p_token)) else {
            return;
        };
        if locate_coll_seq(db, parse, &z_coll).is_some() {
            column_set_coll(&mut p.a_col[i], &z_coll);

            // Se a coluna foi declarada como "<nome> PRIMARY KEY COLLATE <tipo>", pode ter sido
            // criado um índice sobre ela antes de a colação entrar. Corrige isso se for o caso.
            let new_coll: Vec<u8> = column_coll(&p.a_col[i]).map(|z| z.to_vec()).unwrap_or_default();
            for p_idx in p.p_index.iter_mut() {
                debug_assert!(p_idx.n_key_col == 1);
                if p_idx.ai_column[0] as usize == i {
                    Rc::make_mut(p_idx).az_coll[0] = new_coll.clone();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_by_substring() {
        assert_eq!(affinity_type(b"INTEGER", None), SQLITE_AFF_INTEGER);
        assert_eq!(affinity_type(b"BLOBINT", None), SQLITE_AFF_INTEGER);
        assert_eq!(affinity_type(b"VARCHAR(10)", None), SQLITE_AFF_TEXT);
        assert_eq!(affinity_type(b"double precision", None), SQLITE_AFF_REAL);
        assert_eq!(affinity_type(b"FLOATING", None), SQLITE_AFF_REAL);
        // "POINT" contém "INT": o SQLite dá INTEGER a "FLOATING POINT".
        assert_eq!(affinity_type(b"FLOATING POINT", None), SQLITE_AFF_INTEGER);
        assert_eq!(affinity_type(b"BLOB", None), SQLITE_AFF_BLOB);
        assert_eq!(affinity_type(b"DECIMAL(10,5)", None), SQLITE_AFF_NUMERIC);
        assert_eq!(affinity_type(b"", None), SQLITE_AFF_NUMERIC);
    }

    #[test]
    fn affinity_size_estimate() {
        let mut c = Column::default();
        affinity_type(b"VARCHAR(10)", Some(&mut c));
        assert_eq!(c.sz_est, 10 / 4 + 1);
        affinity_type(b"TEXT", Some(&mut c));
        assert_eq!(c.sz_est, 16 / 4 + 1);
        affinity_type(b"NUMERIC", Some(&mut c));
        assert_eq!(c.sz_est, 1);
    }

    #[test]
    fn column_collation_roundtrip() {
        let mut c = Column { z_cn_name: b"nome\0".to_vec(), ..Column::default() };
        assert_eq!(column_coll(&c), None);
        column_set_coll(&mut c, b"NOCASE");
        assert_eq!(column_coll(&c), Some(&b"NOCASE"[..]));
        let mut t = Column { z_cn_name: b"x\0INT\0".to_vec(), col_flags: COLFLAG_HASTYPE, ..Column::default() };
        column_set_coll(&mut t, b"RTRIM");
        assert_eq!(t.z_cn_name, b"x\0INT\0RTRIM\0".to_vec());
        assert_eq!(column_coll(&t), Some(&b"RTRIM"[..]));
    }

    #[test]
    fn schema_table_names() {
        assert_eq!(schema_table(0), LEGACY_SCHEMA_TABLE);
        assert_eq!(schema_table(1), LEGACY_TEMP_SCHEMA_TABLE);
    }
}

/// `pTab->tabFlags |= flags` para uma tabela já publicada no esquema: as tabelas do esquema são
/// `Rc` imutáveis, então a entrada do `tbl_hash` é que recebe a cópia com a flag.
pub fn table_flags_or(db: &mut Connection, p_tab: &Rc<Table>, flags: u32) {
    let i_db = crate::prepare::schema_to_index(db, p_tab.p_schema);
    if i_db < 0 {
        return;
    }
    if let Some(entry) = hash_find_mut(&mut db.dbs[i_db as usize].schema.tbl_hash, &p_tab.z_name) {
        Rc::make_mut(entry).tab_flags |= flags;
    }
}
