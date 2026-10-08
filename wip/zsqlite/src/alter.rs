//! alter.c: o ALTER TABLE (RENAME TABLE, RENAME COLUMN, DROP COLUMN, ADD COLUMN), o mapa de
//! tokens que o analisador mantém em modo RENAME e as funções SQL internas que reescrevem o
//! texto do `sqlite_schema`.
//!
//! Decisões do modelo v2 (além das de CONVENTIONS.md):
//!
//! * **Identidade por endereço.** O C usa o ponteiro do elemento da árvore (`Expr*`, `zName`,
//!   `&pExpr->y.pTab`, `&pTab->iPKey`...) como chave de `RenameToken.p`. Aqui a chave é o
//!   endereço, como inteiro (`as_ptr() as usize`, `&*box as *const _ as usize`), e a comparação
//!   é entre inteiros, inteiramente segura. As convenções de endereço são as de quem chama
//!   `rename_token_map` (build.rs, expr.rs, trigger.rs...): buffer de texto para nomes, nó
//!   `Box` para `Expr`, `&expr.y` para a tabela de um `TK_COLUMN`, `&tab.i_p_key`, `&fkey.a_col[i]`.
//! * **Listas.** `Parse.p_rename` guarda o mais novo por último (o C encadeia o mais novo na
//!   frente); `RenameCtx.p_list` idem. Onde a ordem importa (`renameTokenFind` acha o mais novo,
//!   `renameColumnTokenNext` desempata pelo mais novo) a busca percorre de trás para a frente.
//! * **Árvores fora do `Parse` durante o percurso.** O C alias `sParse.pNewTable` com o `Walker`.
//!   Aqui a árvore (`p_new_table`, `p_new_index`, `p_new_trigger`) sai do `Parse` com `take`
//!   enquanto é percorrida e volta depois; o `Walker` possui `&mut Connection`, `&mut Parse`
//!   (para achar e remover tokens de `p_rename`) e o `RenameCtx` (`RenameWalk`).
//! * **`sCtx.pTab = sParse.pNewTable`.** A tabela em construção não é um `Rc<Table>`; as
//!   expressões dela guardam `TabRef::Own`. O campo `RenameWalk.own` diz que `ctx.p_tab` é a
//!   tabela dona da árvore percorrida e casa com `TabRef::Own`.
//! * **Funções SQL.** Cada uma calcula um `Outcome` com a conexão emprestada e só depois grava o
//!   resultado no `Context` (o C grava no meio do caminho; o efeito é o mesmo).
//! * `sqlite3BtreeEnterAll/LeaveAll` (cache compartilhado desligado) e as falhas de alocação do C
//!   não existem. `SQLITE_DEBUG` (`renameTokenCheckAll` e as asserções de offset) some.

use std::rc::Rc;

use crate::auth::auth_check;
use crate::build::{
    check_object_name, delete_table, find_db_name, find_index, find_table, locate_table,
    locate_table_item, name_from_token, nested_parse, primary_key_index, table_column_to_index,
    text_arg, token_arg, writable_schema,
};
use crate::build2::{change_cookie, is_shadow_table_of, read_only_shadow_tables, view_get_column_names};
use crate::build3::may_abort;
use crate::callback::insert_builtin_funcs;
use crate::connection::{
    Connection, Context, FuncDef, Parse, RenameToken as ParseRenameToken, UserData,
    PARSE_MODE_RENAME, PARSE_MODE_UNMAP,
};
use crate::consts::{
    COLFLAG_GENERATED, COLFLAG_PRIMKEY, COLFLAG_STORED, COLFLAG_UNIQUE, COLFLAG_VIRTUAL,
    EP_DBL_QUOTED, EP_LEAF, EP_TOKEN_ONLY, EP_WIN_FUNC, ENAME_NAME, ENAME_SPAN, INITFLAG_ALTERADD,
    INITFLAG_ALTERDROP, INITFLAG_ALTERRENAME, NC_UUPSERT, OP_ADDIMM, OP_COLUMN, OP_IDXINSERT,
    OP_IFPOS, OP_INSERT, OP_MAKERECORD, OP_NEXT, OP_NULL, OP_OPENWRITE, OP_READCOOKIE, OP_REWIND,
    OP_ROWID, OP_SETCOOKIE, OP_VRENAME, OPFLAG_SAVEPOSITION, SF_COPYCTE, SF_EXPANDED, SF_VIEW,
    SQLITE_AFF_BLOB, SQLITE_AFF_NUMERIC, SQLITE_AFF_REAL, SQLITE_ALTER_TABLE, SQLITE_CORRUPT_BKPT,
    SQLITE_DQS_DDL, SQLITE_DQS_DML, SQLITE_ERROR, SQLITE_FOREIGN_KEYS, SQLITE_FUNC_BUILTIN,
    SQLITE_FUNC_CONSTANT, SQLITE_FUNC_INTERNAL, SQLITE_LEGACY_ALTER, SQLITE_NOMEM, SQLITE_NULL,
    SQLITE_OK, SQLITE_UTF8, TABTYP_NORM, TF_EPONYMOUS, TF_SHADOW, TF_STRICT, TK_COLUMN, TK_NULL,
    TK_STRING, TK_TRIGGER, WRC_ABORT, WRC_CONTINUE, WRC_PRUNE, BTREE_FILE_FORMAT,
};
use crate::ctype::{is_id_char, is_quote, is_space};
use crate::expr::{expr_list_dup, with_dup};
use crate::expr_code::expr_code_get_column_of_table;
use crate::expr_code2::{get_temp_reg, release_temp_reg};
use crate::insert::open_table;
use crate::mem::{value_type, Mem, StrDtor};
use crate::mem2::value_from_expr;
use crate::prepare::{parse_object_init, parse_object_reset, run_parser, schema_to_index};
use crate::printf::{mprintf, snprintf, PrintfArg};
use crate::resolve::{name_context_new, resolve_expr_list_names, resolve_expr_names};
use crate::select::{column_index, get_vdbe, select_new};
use crate::select2::with_push;
use crate::select3::select_prep;
use crate::sqlite_int::{
    Expr, ExprList, FKeyColMap, IdList, Index, NcU, RenameCtx, RenameToken, Select, SrcItem,
    SrcList, TabRef, TableU, Table, Token, Trigger, TabInfo, Upsert, Walker, With,
};
use crate::trigger::trigger_step_src;
use crate::util::{error_msg, str_i_hash, str_icmp, stricmp, strlen30, strnicmp, dequote};
use crate::utf::utf8_char_len;
use crate::vdbeaux::{
    add_op1, add_op2, add_op3, add_op4_int, add_parse_schema_op, change_p4_vtab, change_p5,
    jump_here, load_string, vdbe_of_parse,
};
use crate::vdbeaux2::{uses_btree, vdbe_finalize};
use crate::vdbeapi::{
    result_error, result_error_code, result_int, result_text, result_value, value_free, value_int,
    value_text,
};
use crate::vtab::get_vtable;
use crate::walker::{walk_expr, walk_expr_list, walk_select, WALKER_FLAG_IN_RENAME};

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------

/// O texto UTF-8 de um argumento SQL até o primeiro NUL, ou `None` para NULL
/// (`sqlite3_value_text`).
fn arg_text(m: &Mem) -> Option<Vec<u8>> {
    if value_type(m) == SQLITE_NULL {
        return None;
    }
    let mut c = m.clone();
    value_text(&mut c).map(|z| z[..strlen30(z) as usize].to_vec())
}

/// `PrintfArg::Int` a partir de um booleano ou inteiro.
fn int_arg(v: i64) -> PrintfArg {
    PrintfArg::Int(v)
}

/// A parte de `z` até o primeiro NUL.
fn cstr(z: &[u8]) -> &[u8] {
    &z[..strlen30(z) as usize]
}

/// O endereço do buffer de texto, a chave de `RenameToken.p` para nomes (0 quando não há nome).
fn text_addr(z: &Option<Vec<u8>>) -> usize {
    z.as_ref().map_or(0, |z| z.as_ptr() as usize)
}

/// O endereço do nó, a chave de `RenameToken.p` para `Expr`.
fn expr_addr(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// O nome do banco de índice `i_db` (vazio se o índice não existe).
fn db_name_at(db: &Connection, i_db: i32) -> Vec<u8> {
    usize::try_from(i_db)
        .ok()
        .and_then(|i| db.dbs.get(i))
        .map(|d| d.z_db_s_name.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Verificações e geração de código comuns
// ---------------------------------------------------------------------------------------------

/// `isAlterableTable`: tabelas de sistema, eponímias e (conforme o contexto) sombras não podem
/// ser alteradas. Deixa a mensagem em `parse` e devolve 1 se não pode; senão 0.
fn is_alterable_table(db: &mut Connection, parse: &mut Parse, p_tab: &Table) -> i32 {
    if 0 == strnicmp(Some(&p_tab.z_name), Some(b"sqlite_"), 7)
        || (p_tab.tab_flags & TF_EPONYMOUS) != 0
        || ((p_tab.tab_flags & TF_SHADOW) != 0 && read_only_shadow_tables(db))
    {
        error_msg(db, parse, b"table %s may not be altered", &[text_arg(&p_tab.z_name)]);
        return 1;
    }
    0
}

/// `renameTestSchema`: gera o código que confere que os esquemas de `z_db` e, se `b_temp` é falso,
/// de "temp" ainda podem ser analisados. Chamada no fim de um ALTER TABLE.
fn rename_test_schema(
    db: &mut Connection,
    parse: &mut Parse,
    z_db: &[u8],
    b_temp: bool,
    z_when: &[u8],
    b_no_dqs: bool,
) {
    parse.col_names_set = 1;
    // LEGACY_SCHEMA_TABLE é "sqlite_master".
    nested_parse(
        db,
        parse,
        b"SELECT 1 FROM \"%w\".sqlite_master WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' \
          AND sql NOT LIKE 'create virtual%%' \
          AND sqlite_rename_test(%Q, sql, type, name, %d, %Q, %d)=NULL ",
        &[
            text_arg(z_db),
            text_arg(z_db),
            int_arg(b_temp as i64),
            text_arg(z_when),
            int_arg(b_no_dqs as i64),
        ],
    );
    if !b_temp {
        nested_parse(
            db,
            parse,
            b"SELECT 1 FROM temp.sqlite_master WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' \
              AND sql NOT LIKE 'create virtual%%' \
              AND sqlite_rename_test(%Q, sql, type, name, 1, %Q, %d)=NULL ",
            &[text_arg(z_db), text_arg(z_when), int_arg(b_no_dqs as i64)],
        );
    }
}

/// `renameFixQuotes`: gera o código que troca, no `sql` do `sqlite_schema` de `z_db` (e do temp,
/// se `b_temp` é falso), as strings entre aspas duplas pelas equivalentes entre aspas simples.
fn rename_fix_quotes(db: &mut Connection, parse: &mut Parse, z_db: &[u8], b_temp: bool) {
    nested_parse(
        db,
        parse,
        b"UPDATE \"%w\".sqlite_master SET sql = sqlite_rename_quotefix(%Q, sql)\
          WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%'",
        &[text_arg(z_db), text_arg(z_db)],
    );
    if !b_temp {
        nested_parse(
            db,
            parse,
            b"UPDATE temp.sqlite_master SET sql = sqlite_rename_quotefix('temp', sql)\
              WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' AND sql NOT LIKE 'create virtual%%'",
            &[],
        );
    }
}

/// `renameReloadSchema`: gera o código que recarrega o esquema `i_db` e, se `i_db != 1`, o temp.
fn rename_reload_schema(db: &mut Connection, parse: &mut Parse, i_db: i32, p5: u16) {
    if parse.p_vdbe.is_some() {
        change_cookie(db, parse, i_db);
        add_parse_schema_op(parse, db, i_db, None, p5);
        if i_db != 1 {
            add_parse_schema_op(parse, db, 1, None, p5);
        }
    }
}

/// `sqlite3ErrorIfNotEmpty`: gera o código que levanta o erro `z_err` se a tabela não está vazia.
fn error_if_not_empty(
    db: &mut Connection,
    parse: &mut Parse,
    z_db: &[u8],
    z_tab: &[u8],
    z_err: &[u8],
) {
    nested_parse(
        db,
        parse,
        b"SELECT raise(ABORT,%Q) FROM \"%w\".\"%w\"",
        &[text_arg(z_err), text_arg(z_db), text_arg(z_tab)],
    );
}

/// `isRealTable`: views e tabelas virtuais não têm colunas renomeáveis nem removíveis. Deixa a
/// mensagem em `parse` e devolve 1 se é uma delas; senão 0.
fn is_real_table(db: &mut Connection, parse: &mut Parse, p_tab: &Table, b_drop: bool) -> i32 {
    let z_type: Option<&[u8]> = if p_tab.is_virtual() {
        Some(b"virtual table")
    } else if p_tab.is_view() {
        Some(b"view")
    } else {
        None
    };
    if let Some(z_type) = z_type {
        let z_what: &[u8] = if b_drop { b"drop column from" } else { b"rename columns of" };
        error_msg(
            db,
            parse,
            b"cannot %s %s \"%s\"",
            &[
                text_arg(z_what),
                text_arg(z_type),
                text_arg(&p_tab.z_name),
            ],
        );
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------------------------
// ALTER TABLE ... RENAME TO
// ---------------------------------------------------------------------------------------------

/// `sqlite3AlterRenameTable`: gera o código de `ALTER TABLE xxx RENAME TO yyy`. `p_src` é a tabela
/// a renomear e `p_name` o novo nome.
pub fn alter_rename_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: Option<Box<SrcList>>,
    p_name: &Token,
) {
    if let Some(src) = p_src {
        rename_table_body(db, parse, &src, p_name);
    }
}

/// O corpo de `sqlite3AlterRenameTable`; cada `return` é um `goto exit_rename_table`.
fn rename_table_body(db: &mut Connection, parse: &mut Parse, p_src: &SrcList, p_name: &Token) {
    debug_assert!(p_src.a.len() == 1);
    let Some(mut p_tab) = locate_table_item(db, parse, 0, &p_src.a[0]) else {
        return;
    };
    let i_db = schema_to_index(db, p_tab.p_schema);
    let z_db = db_name_at(db, i_db);

    // O novo nome sem aspas.
    let Some(z_name) = name_from_token(Some(p_name)) else {
        return;
    };

    // Confere que não existe tabela ou índice com este nome no banco `i_db`.
    if find_table(db, &z_name, Some(&z_db)).is_some()
        || find_index(db, &z_name, Some(&z_db)).is_some()
        || is_shadow_table_of(db, &p_tab, &z_name)
    {
        error_msg(
            db,
            parse,
            b"there is already another table or index with this name: %s",
            &[text_arg(&z_name)],
        );
        return;
    }

    // Não pode ser tabela de sistema nem nome reservado.
    if SQLITE_OK != is_alterable_table(db, parse, &p_tab) {
        return;
    }
    if SQLITE_OK != check_object_name(db, parse, &z_name, b"table", &z_name) {
        return;
    }

    if p_tab.is_view() {
        error_msg(db, parse, b"view %s may not be altered", &[text_arg(&p_tab.z_name)]);
        return;
    }

    // O gancho de autorização.
    if auth_check(db, parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&p_tab.z_name), None) != 0 {
        return;
    }

    if view_get_column_names(db, parse, &mut p_tab) != 0 {
        return;
    }
    let mut p_vtab = None;
    if p_tab.is_virtual() {
        if let Some(id) = get_vtable(db, &p_tab) {
            let has_rename =
                db.vtabs.get(id.slot()).map_or(false, |vt| vt.p_mod.p_module.caps().rename);
            if has_rename {
                p_vtab = Some(id);
            }
        }
    }

    // Abre a transação, altera o cookie do esquema e avisa que as funções escalares do SQL
    // aninhado podem levantar exceção.
    get_vdbe(db, parse);
    may_abort(parse);

    // Quantos caracteres UTF-8 tem o nome antigo.
    let z_tab_name = p_tab.z_name.clone();
    let n_tab_name = utf8_char_len(&z_tab_name, -1);

    // Reescreve todos os CREATE TABLE, INDEX, TRIGGER e VIEW para usar o nome novo.
    nested_parse(
        db,
        parse,
        b"UPDATE \"%w\".sqlite_master SET \
          sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, %d) \
          WHERE (type!='index' OR tbl_name=%Q COLLATE nocase)\
          AND   name NOT LIKE 'sqliteX_%%' ESCAPE 'X'",
        &[
            text_arg(&z_db),
            text_arg(&z_db),
            text_arg(&z_tab_name),
            text_arg(&z_name),
            int_arg((i_db == 1) as i64),
            text_arg(&z_tab_name),
        ],
    );

    // Atualiza as colunas tbl_name e name do sqlite_schema.
    nested_parse(
        db,
        parse,
        b"UPDATE %Q.sqlite_master SET \
          tbl_name = %Q, \
          name = CASE \
          WHEN type='table' THEN %Q \
          WHEN name LIKE 'sqliteX_autoindex%%' ESCAPE 'X' \
               AND type='index' THEN \
          'sqlite_autoindex_' || %Q || substr(name,%d+18) \
          ELSE name END \
          WHERE tbl_name=%Q COLLATE nocase AND \
          (type='table' OR type='index' OR type='trigger');",
        &[
            text_arg(&z_db),
            text_arg(&z_name),
            text_arg(&z_name),
            text_arg(&z_name),
            int_arg(n_tab_name as i64),
            text_arg(&z_tab_name),
        ],
    );

    // Se a sqlite_sequence existe neste banco, atualiza o nome nela.
    if find_table(db, b"sqlite_sequence", Some(&z_db)).is_some() {
        nested_parse(
            db,
            parse,
            b"UPDATE \"%w\".sqlite_sequence set name = %Q WHERE name = %Q",
            &[text_arg(&z_db), text_arg(&z_name), text_arg(&p_tab.z_name)],
        );
    }

    // Se a tabela não é do banco temp, edita as views e gatilhos do temp.
    if i_db != 1 {
        nested_parse(
            db,
            parse,
            b"UPDATE sqlite_temp_schema SET \
              sql = sqlite_rename_table(%Q, type, name, sql, %Q, %Q, 1), \
              tbl_name = \
              CASE WHEN tbl_name=%Q COLLATE nocase AND \
                sqlite_rename_test(%Q, sql, type, name, 1, 'after rename', 0) \
              THEN %Q ELSE tbl_name END \
              WHERE type IN ('view', 'trigger')",
            &[
                text_arg(&z_db),
                text_arg(&z_tab_name),
                text_arg(&z_name),
                text_arg(&z_tab_name),
                text_arg(&z_db),
                text_arg(&z_name),
            ],
        );
    }

    // Tabela virtual: chama o xRename do módulo, que renomeia os recursos dela.
    if let Some(id) = p_vtab {
        parse.n_mem += 1;
        let i = parse.n_mem;
        let v = vdbe_of_parse(parse);
        load_string(v, i, &z_name);
        let a = add_op3(v, OP_VRENAME as i32, i, 0, 0);
        change_p4_vtab(v, db, a, id);
    }

    rename_reload_schema(db, parse, i_db, INITFLAG_ALTERRENAME as u16);
    rename_test_schema(db, parse, &z_db, i_db == 1, b"after rename", false);
}

// ---------------------------------------------------------------------------------------------
// ALTER TABLE ... ADD COLUMN
// ---------------------------------------------------------------------------------------------

/// `sqlite3AlterFinishAddColumn`: chamada depois de um `ALTER TABLE ... ADD` ser analisado.
/// `p_col_def` é o texto da definição da coluna nova; `Parse.p_new_table` já ganhou a coluna.
pub fn alter_finish_add_column(db: &mut Connection, parse: &mut Parse, p_col_def: &Token) {
    if parse.n_err != 0 {
        return;
    }
    let Some(p_new) = parse.p_new_table.take() else {
        debug_assert!(false, "pParse->pNewTable");
        return;
    };
    finish_add_column_body(db, parse, &p_new, p_col_def);
    parse.p_new_table = Some(p_new);
}

/// O corpo de `sqlite3AlterFinishAddColumn`; `p_new` é a cópia de `pParse->pNewTable`.
fn finish_add_column_body(db: &mut Connection, parse: &mut Parse, p_new: &Table, p_col_def: &Token) {
    let i_db = schema_to_index(db, p_new.p_schema);
    let z_db = db_name_at(db, i_db);
    // Pula o prefixo "sqlite_altertab_" do nome.
    let z_tab: Vec<u8> = cstr(p_new.z_name.get(16..).unwrap_or(&[])).to_vec();
    debug_assert!(p_new.n_col > 0);
    let p_col = &p_new.a_col[p_new.n_col as usize - 1];
    let mut p_dflt: Option<&Expr> = crate::build::column_expr(p_new, p_col);
    let Some(p_tab) = find_table(db, &z_tab, Some(&z_db)) else {
        debug_assert!(false, "pTab");
        return;
    };

    // O gancho de autorização.
    if auth_check(db, parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&p_tab.z_name), None) != 0 {
        return;
    }

    // A coluna nova não pode ser PRIMARY KEY nem UNIQUE.
    if (p_col.col_flags & COLFLAG_PRIMKEY) != 0 {
        error_msg(db, parse, b"Cannot add a PRIMARY KEY column", &[]);
        return;
    }
    if !p_new.p_index.is_empty() {
        error_msg(db, parse, b"Cannot add a UNIQUE column", &[]);
        return;
    }
    if (p_col.col_flags & COLFLAG_GENERATED) == 0 {
        // Um DEFAULT literal NULL vale como sem default.
        debug_assert!(p_dflt.map_or(true, |d| d.op == crate::consts::TK_SPAN));
        if p_dflt.map_or(false, |d| d.p_left.as_deref().map_or(false, |l| l.op == TK_NULL)) {
            p_dflt = None;
        }
        debug_assert!(p_new.is_ordinary_table());
        if (db.flags & SQLITE_FOREIGN_KEYS) != 0
            && p_new.u_tab().map_or(false, |t| !t.p_f_key.is_empty())
            && p_dflt.is_some()
        {
            error_if_not_empty(
                db,
                parse,
                &z_db,
                &z_tab,
                b"Cannot add a REFERENCES column with non-NULL default value",
            );
        }
        if p_col.not_null != 0 && p_dflt.is_none() {
            error_if_not_empty(
                db,
                parse,
                &z_db,
                &z_tab,
                b"Cannot add a NOT NULL column with default value NULL",
            );
        }

        // O default precisa ser algo que `sqlite3ValueFromExpr()` entende (nada de CURRENT_TIME).
        if let Some(d) = p_dflt {
            let mut p_val: Option<Mem> = None;
            let rc = value_from_expr(db, d, SQLITE_UTF8 as u8, SQLITE_AFF_BLOB, &mut p_val);
            debug_assert!(rc == SQLITE_OK || rc == SQLITE_NOMEM);
            if rc != SQLITE_OK {
                return;
            }
            match p_val {
                None => error_if_not_empty(
                    db,
                    parse,
                    &z_db,
                    &z_tab,
                    b"Cannot add a column with non-constant default",
                ),
                Some(v) => value_free(v),
            }
        }
    } else if (p_col.col_flags & COLFLAG_STORED) != 0 {
        error_if_not_empty(db, parse, &z_db, &z_tab, b"cannot add a STORED column");
    }

    // Modifica o CREATE TABLE.
    let mut z_col = p_col_def.z.clone();
    if !z_col.is_empty() {
        let mut n = z_col.len();
        while n > 1 && (z_col[n - 1] == b';' || is_space(z_col[n - 1])) {
            n -= 1;
        }
        z_col.truncate(n);
        // substr() conta caracteres e addColOffset conta bytes: printf() faz a conversão.
        debug_assert!(p_tab.is_ordinary_table());
        let add_col_offset = p_new.u_tab().map_or(0, |t| t.add_col_offset);
        nested_parse(
            db,
            parse,
            b"UPDATE \"%w\".sqlite_master SET \
              sql = printf('%%.%ds, ',sql) || %Q \
              || substr(sql,1+length(printf('%%.%ds',sql))) \
              WHERE type = 'table' AND name = %Q",
            &[
                text_arg(&z_db),
                int_arg(add_col_offset as i64),
                text_arg(&z_col),
                int_arg(add_col_offset as i64),
                text_arg(&z_tab),
            ],
        );
    }

    get_vdbe(db, parse);
    // A versão do formato do esquema precisa ser pelo menos 3, mas nunca promover de menos de 3
    // para 4, que corromperia os índices DESC existentes.
    let r1 = get_temp_reg(parse);
    {
        let v = vdbe_of_parse(parse);
        add_op3(v, OP_READCOOKIE as i32, i_db, r1, BTREE_FILE_FORMAT as i32);
        uses_btree(v, i_db);
        add_op2(v, OP_ADDIMM as i32, r1, -2);
        let addr = v.n_op() + 2;
        add_op2(v, OP_IFPOS as i32, r1, addr);
        add_op3(v, OP_SETCOOKIE as i32, i_db, BTREE_FILE_FORMAT as i32, 3);
    }
    release_temp_reg(parse, r1);

    // Recarrega a definição da tabela.
    rename_reload_schema(db, parse, i_db, INITFLAG_ALTERADD as u16);

    // Confere que as restrições ainda valem.
    if p_new.p_check.is_some()
        || (p_col.not_null != 0 && (p_col.col_flags & COLFLAG_GENERATED) != 0)
        || (p_tab.tab_flags & TF_STRICT) != 0
    {
        nested_parse(
            db,
            parse,
            b"SELECT CASE WHEN quick_check GLOB 'CHECK*' \
              THEN raise(ABORT,'CHECK constraint failed') \
              WHEN quick_check GLOB 'non-* value in*' \
              THEN raise(ABORT,'type mismatch on DEFAULT') \
              ELSE raise(ABORT,'NOT NULL constraint failed') \
              END  FROM pragma_quick_check(%Q,%Q) \
              WHERE quick_check GLOB 'CHECK*' \
              OR quick_check GLOB 'NULL*' \
              OR quick_check GLOB 'non-* value in*'",
            &[text_arg(&z_tab), text_arg(&z_db)],
        );
    }
}

/// `sqlite3AlterBeginAddColumn`: chamada pelo analisador depois do nome da tabela de um
/// `ALTER TABLE <tabela> ADD`. Põe em `Parse.p_new_table` uma cópia parcial da tabela, com o
/// nome prefixado por "sqlite_altertab_", para o `add_column` e companhia acrescentarem a coluna.
pub fn alter_begin_add_column(db: &mut Connection, parse: &mut Parse, p_src: Option<Box<SrcList>>) {
    let Some(p_src) = p_src else {
        return;
    };
    debug_assert!(parse.p_new_table.is_none());
    if db.malloc_failed != 0 {
        return;
    }
    let Some(p_tab) = locate_table_item(db, parse, 0, &p_src.a[0]) else {
        return;
    };
    if p_tab.is_virtual() {
        error_msg(db, parse, b"virtual tables may not be altered", &[]);
        return;
    }

    // Uma VIEW não pode ser alterada.
    if p_tab.is_view() {
        error_msg(db, parse, b"Cannot add a column to a view", &[]);
        return;
    }
    if SQLITE_OK != is_alterable_table(db, parse, &p_tab) {
        return;
    }

    may_abort(parse);
    debug_assert!(p_tab.is_ordinary_table());
    let Some(p_tab_info) = p_tab.u_tab() else {
        return;
    };
    debug_assert!(p_tab_info.add_col_offset > 0);
    let i_db = schema_to_index(db, p_tab.p_schema);

    // A cópia da `Table` que o `sqlite3AddColumn()` e companhia vão alterar. O prefixo
    // "sqlite_altertab_" garante que o nome não colide com tabela de usuário. Como `sqlite3DbMallocZero`
    // no C, só `nCol`, `aCol`, `zName`, `pDfltList`, `pSchema` e `addColOffset` são preenchidos.
    let n_col = p_tab.n_col;
    debug_assert!(n_col > 0);
    let mut a_col = p_tab.a_col[..n_col as usize].to_vec();
    for p_col in a_col.iter_mut() {
        p_col.h_name = str_i_hash(&p_col.z_cn_name);
    }
    let mut z_name = b"sqlite_altertab_".to_vec();
    z_name.extend_from_slice(cstr(&p_tab.z_name));
    let p_new = Table {
        z_name,
        a_col,
        n_col,
        e_tab_type: TABTYP_NORM,
        u: TableU::Tab(TabInfo {
            add_col_offset: p_tab_info.add_col_offset,
            p_f_key: Vec::new(),
            p_dflt_list: expr_list_dup(p_tab_info.p_dflt_list.as_deref(), 0),
        }),
        p_schema: db.dbs[i_db as usize].schema.id,
        ..Table::default()
    };
    parse.p_new_table = Some(Box::new(p_new));
}

// ---------------------------------------------------------------------------------------------
// ALTER TABLE ... RENAME COLUMN
// ---------------------------------------------------------------------------------------------

/// `sqlite3AlterRenameColumn`: a redução `cmd ::= ALTER TABLE pSrc RENAME COLUMN pOld TO pNew`.
pub fn alter_rename_column(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: Option<Box<SrcList>>,
    p_old: &Token,
    p_new: &Token,
) {
    let Some(p_src) = p_src else {
        return;
    };
    // Acha a tabela a alterar.
    let Some(p_tab) = locate_table_item(db, parse, 0, &p_src.a[0]) else {
        return;
    };

    // Tabela de sistema, view e tabela virtual não.
    if SQLITE_OK != is_alterable_table(db, parse, &p_tab) {
        return;
    }
    if SQLITE_OK != is_real_table(db, parse, &p_tab, false) {
        return;
    }

    // Qual esquema guarda a tabela.
    let i_schema = schema_to_index(db, p_tab.p_schema);
    debug_assert!(i_schema >= 0);
    let z_db = db_name_at(db, i_schema);

    // O gancho de autorização.
    if auth_check(db, parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&p_tab.z_name), None) != 0 {
        return;
    }

    // O nome antigo precisa ser mesmo o de uma coluna.
    let Some(z_old) = name_from_token(Some(p_old)) else {
        return;
    };
    let mut i_col = 0usize;
    while i_col < p_tab.n_col as usize {
        if 0 == str_icmp(&p_tab.a_col[i_col].z_cn_name, &z_old) {
            break;
        }
        i_col += 1;
    }
    if i_col == p_tab.n_col as usize {
        let t = token_arg(parse, p_old);
        error_msg(db, parse, b"no such column: \"%T\"", &[t]);
        return;
    }

    // O esquema não pode ter strings entre aspas duplas.
    rename_test_schema(db, parse, &z_db, i_schema == 1, b"", false);
    rename_fix_quotes(db, parse, &z_db, i_schema == 1);

    // Faz a renomeação com um UPDATE recursivo que usa sqlite_rename_column().
    may_abort(parse);
    let Some(z_new) = name_from_token(Some(p_new)) else {
        return;
    };
    debug_assert!(!p_new.z.is_empty());
    let b_quote = is_quote(p_new.z[0]);
    nested_parse(
        db,
        parse,
        b"UPDATE \"%w\".sqlite_master SET \
          sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, %d) \
          WHERE name NOT LIKE 'sqliteX_%%' ESCAPE 'X' \
           AND (type != 'index' OR tbl_name = %Q)",
        &[
            text_arg(&z_db),
            text_arg(&z_db),
            text_arg(&p_tab.z_name),
            int_arg(i_col as i64),
            text_arg(&z_new),
            int_arg(b_quote as i64),
            int_arg((i_schema == 1) as i64),
            text_arg(&p_tab.z_name),
        ],
    );

    nested_parse(
        db,
        parse,
        b"UPDATE temp.sqlite_master SET \
          sql = sqlite_rename_column(sql, type, name, %Q, %Q, %d, %Q, %d, 1) \
          WHERE type IN ('trigger', 'view')",
        &[
            text_arg(&z_db),
            text_arg(&p_tab.z_name),
            int_arg(i_col as i64),
            text_arg(&z_new),
            int_arg(b_quote as i64),
        ],
    );

    // Apaga e recarrega o esquema.
    rename_reload_schema(db, parse, i_schema, INITFLAG_ALTERRENAME as u16);
    rename_test_schema(db, parse, &z_db, i_schema == 1, b"after rename", true);
}

// ---------------------------------------------------------------------------------------------
// O mapa de tokens
// ---------------------------------------------------------------------------------------------

/// `sqlite3RenameTokenMap`: registra que o elemento `p_ptr` da árvore foi criado pelo token
/// `p_token`. Devolve `p_ptr`, como o C, para uso em chamadas de cauda.
pub fn rename_token_map(parse: &mut Parse, p_ptr: usize, p_token: &Token) -> usize {
    debug_assert!(p_ptr != 0);
    if parse.e_parse_mode != PARSE_MODE_UNMAP {
        parse.p_rename.push(ParseRenameToken { p: p_ptr, t: p_token.clone() });
    }
    p_ptr
}

/// `sqlite3RenameTokenRemap`: já existe um `RenameToken` associado a `p_from`; passa o token a
/// `p_to`.
pub fn rename_token_remap(parse: &mut Parse, p_to: usize, p_from: usize) {
    if let Some(p) = parse.p_rename.iter_mut().rev().find(|t| t.p == p_from) {
        p.p = p_to;
    }
}

/// `renameTokenFind`: acha o `RenameToken` do elemento `p_ptr` (o mais novo que casa). Se há
/// `p_ctx`, o token sai de `Parse.p_rename` e entra na lista do contexto. Devolve uma cópia do
/// token (o texto e a posição), ou `None`.
fn rename_token_find(
    parse: &mut Parse,
    p_ctx: Option<&mut RenameCtx>,
    p_ptr: usize,
) -> Option<Token> {
    if p_ptr == 0 {
        return None;
    }
    let pos = parse.p_rename.iter().rposition(|t| t.p == p_ptr)?;
    let t = parse.p_rename[pos].t.clone();
    if let Some(ctx) = p_ctx {
        let tok = parse.p_rename.remove(pos);
        ctx.p_list.push(RenameToken { p: tok.p, t: tok.t });
    }
    Some(t)
}

// ---------------------------------------------------------------------------------------------
// Desfazer o mapeamento (sqlite3RenameExprUnmap e sqlite3RenameExprlistUnmap)
// ---------------------------------------------------------------------------------------------
//
// O percurso do `walker.rs` exige `&mut`, e quem chama `rename_expr_unmap` só tem `&Expr`. Estas
// rotinas refazem o percurso (a mesma ordem de `walk_expr_nn`, `walk_select` e
// `walk_select_expr`) sobre referências compartilhadas; só o `Parse` é alterado, e só para
// remapear tokens a 0. Desvio: o `renameWalkWith` do C roda `sqlite3SelectPrep` nas CTEs ainda
// não expandidas. Isso exige `&mut Select` e só cria mapeamentos de nós que aqui nem existem,
// porque a árvore nunca foi preparada; o desmapeamento dos nós existentes é completo.

/// `renameUnmapExprCb`.
fn unmap_expr_cb(parse: &mut Parse, p_expr: &Expr) {
    rename_token_remap(parse, 0, expr_addr(p_expr));
    if p_expr.use_y_tab() {
        rename_token_remap(parse, 0, &p_expr.y as *const _ as usize);
    }
}

/// `sqlite3WalkExprNN` com o callback de `renameUnmapExprCb`. `descend` é verdadeiro quando o
/// walker tem `xSelectCallback` (as subconsultas são percorridas).
fn unmap_expr_nn(parse: &mut Parse, p_expr: &Expr, descend: bool) -> i32 {
    let mut cur = p_expr;
    loop {
        unmap_expr_cb(parse, cur);
        if !cur.has_property(EP_TOKEN_ONLY | EP_LEAF) {
            if let Some(l) = cur.p_left.as_deref() {
                if unmap_expr_nn(parse, l, descend) != 0 {
                    return WRC_ABORT;
                }
            }
            if let Some(r) = cur.p_right.as_deref() {
                cur = r;
                continue;
            } else if cur.use_x_select() {
                if descend && unmap_select(parse, cur.x_select(), descend) != 0 {
                    return WRC_ABORT;
                }
            } else {
                if let Some(l) = cur.x_list() {
                    if unmap_expr_list(parse, Some(l), descend) != 0 {
                        return WRC_ABORT;
                    }
                }
                if cur.has_property(EP_WIN_FUNC) {
                    if let Some(w) = cur.y_win() {
                        if unmap_window(parse, w, descend) != 0 {
                            return WRC_ABORT;
                        }
                    }
                }
            }
        }
        break;
    }
    WRC_CONTINUE
}

/// `sqlite3WalkExpr` para o desmapeamento.
fn unmap_expr(parse: &mut Parse, p_expr: Option<&Expr>, descend: bool) -> i32 {
    match p_expr {
        Some(e) => unmap_expr_nn(parse, e, descend),
        None => WRC_CONTINUE,
    }
}

/// `sqlite3WalkExprList` para o desmapeamento.
fn unmap_expr_list(parse: &mut Parse, p: Option<&ExprList>, descend: bool) -> i32 {
    if let Some(list) = p {
        for item in list.a.iter() {
            if unmap_expr(parse, item.p_expr.as_deref(), descend) != 0 {
                return WRC_ABORT;
            }
        }
    }
    WRC_CONTINUE
}

/// `walkWindow` para o desmapeamento.
fn unmap_window(parse: &mut Parse, p_win: &crate::sqlite_int::Window, descend: bool) -> i32 {
    if unmap_expr_list(parse, p_win.p_order_by.as_deref(), descend) != 0
        || unmap_expr_list(parse, p_win.p_partition.as_deref(), descend) != 0
        || unmap_expr(parse, p_win.p_filter.as_deref(), descend) != 0
        || unmap_expr(parse, p_win.p_start.as_deref(), descend) != 0
        || unmap_expr(parse, p_win.p_end.as_deref(), descend) != 0
    {
        return WRC_ABORT;
    }
    WRC_CONTINUE
}

/// `unmapColumnIdlistNames`: desmapeia todos os tokens da lista de identificadores.
fn unmap_column_idlist_names(parse: &mut Parse, p_id_list: &IdList) {
    for item in p_id_list.a.iter() {
        rename_token_remap(parse, 0, text_addr(&item.z_name));
    }
}

/// `renameUnmapSelectCb`.
fn unmap_select_cb(parse: &mut Parse, p: &Select) -> i32 {
    if parse.n_err != 0 {
        return WRC_ABORT;
    }
    if (p.sel_flags & (SF_VIEW | SF_COPYCTE)) != 0 {
        return WRC_PRUNE;
    }
    if let Some(list) = p.p_e_list.as_deref() {
        for item in list.a.iter() {
            if item.z_e_name.is_some() && item.fg.e_e_name == ENAME_NAME {
                rename_token_remap(parse, 0, text_addr(&item.z_e_name));
            }
        }
    }
    if let Some(src) = p.p_src.as_deref() {
        for item in src.a.iter() {
            rename_token_remap(parse, 0, text_addr(&item.z_name));
            if !item.fg.is_using {
                unmap_expr(parse, item.p_on(), true);
            } else if let Some(using) = item.p_using() {
                unmap_column_idlist_names(parse, using);
            }
        }
    }
    // `renameWalkWith`, sem o SelectPrep (ver o comentário da seção).
    if let Some(with) = p.p_with.as_deref() {
        for cte in with.a.iter() {
            if let Some(s) = cte.p_select.as_deref() {
                unmap_select(parse, Some(s), true);
            }
            rename_exprlist_unmap(parse, cte.p_cols.as_deref());
        }
    }
    WRC_CONTINUE
}

/// `sqlite3WalkSelect` para o desmapeamento (sempre com `xSelectCallback`).
fn unmap_select(parse: &mut Parse, p: Option<&Select>, descend: bool) -> i32 {
    let mut cur = match p {
        Some(p) => p,
        None => return WRC_CONTINUE,
    };
    loop {
        let rc = unmap_select_cb(parse, cur);
        if rc != 0 {
            return rc & WRC_ABORT;
        }
        // sqlite3WalkSelectExpr (o walker está em modo RENAME, então as janelas da cláusula WINDOW
        // também são percorridas).
        if unmap_expr_list(parse, cur.p_e_list.as_deref(), descend) != 0
            || unmap_expr(parse, cur.p_where.as_deref(), descend) != 0
            || unmap_expr_list(parse, cur.p_group_by.as_deref(), descend) != 0
            || unmap_expr(parse, cur.p_having.as_deref(), descend) != 0
            || unmap_expr_list(parse, cur.p_order_by.as_deref(), descend) != 0
            || unmap_expr(parse, cur.p_limit.as_deref(), descend) != 0
        {
            return WRC_ABORT;
        }
        for w in cur.p_win_defn.iter() {
            if unmap_window(parse, w, descend) != 0 {
                return WRC_ABORT;
            }
        }
        // sqlite3WalkSelectFrom.
        if let Some(src) = cur.p_src.as_deref() {
            for item in src.a.iter() {
                if let Some(s) = item.p_select.as_deref() {
                    if unmap_select(parse, Some(s), descend) != 0 {
                        return WRC_ABORT;
                    }
                }
                if item.fg.is_tab_func {
                    if let crate::sqlite_int::SrcU1::FuncArg(arg) = &item.u1 {
                        if unmap_expr_list(parse, arg.as_deref(), descend) != 0 {
                            return WRC_ABORT;
                        }
                    }
                }
            }
        }
        match cur.p_prior.as_deref() {
            Some(prior) => cur = prior,
            None => break,
        }
    }
    WRC_CONTINUE
}

/// `sqlite3RenameExprUnmap`: tira da lista de renomeação todos os nós da expressão.
pub fn rename_expr_unmap(parse: &mut Parse, p_expr: &Expr) {
    let e_mode = parse.e_parse_mode;
    parse.e_parse_mode = PARSE_MODE_UNMAP;
    unmap_expr_nn(parse, p_expr, true);
    parse.e_parse_mode = e_mode;
}

/// `sqlite3RenameExprlistUnmap`: tira da lista de renomeação todos os nós da lista de expressões.
/// O walker do C só tem o callback de expressão, então as subconsultas não são percorridas.
pub fn rename_exprlist_unmap(parse: &mut Parse, p_e_list: Option<&ExprList>) {
    if let Some(list) = p_e_list {
        unmap_expr_list(parse, Some(list), false);
        for item in list.a.iter() {
            if item.fg.e_e_name == ENAME_NAME {
                rename_token_remap(parse, 0, text_addr(&item.z_e_name));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// O walker de renomeação
// ---------------------------------------------------------------------------------------------

/// O contexto do `Walker` das passagens de renomeação (o `pParse` e o `u.pRename` do C, mais a
/// conexão para o `SelectPrep` e a resolução). `own` diz que `ctx.p_tab` é a tabela dona da
/// árvore percorrida (a `Table` em construção), que casa com `TabRef::Own`.
struct RenameWalk<'a> {
    db: &'a mut Connection,
    parse: &'a mut Parse,
    ctx: &'a mut RenameCtx,
    own: bool,
}

type RenameWalker<'a> = Walker<RenameWalk<'a>>;

/// Monta o `Walker` de uma passagem de renomeação.
fn rename_walker<'a>(
    db: &'a mut Connection,
    parse: &'a mut Parse,
    ctx: &'a mut RenameCtx,
    x_expr: fn(&mut RenameWalker<'a>, &mut Expr) -> i32,
    x_select: fn(&mut RenameWalker<'a>, &mut Select) -> i32,
) -> RenameWalker<'a> {
    Walker {
        x_expr_callback: Some(x_expr),
        x_select_callback: Some(x_select),
        x_select_callback2: None,
        walker_depth: 0,
        e_code: 0,
        m_w_flags: WALKER_FLAG_IN_RENAME,
        u: RenameWalk { db, parse, ctx, own: false },
    }
}

/// `renameTokenFind(pParse, pCtx, p)` com o `Parse` e o contexto do walker.
fn find_in(u: &mut RenameWalk<'_>, p_ptr: usize) -> Option<Token> {
    rename_token_find(&mut *u.parse, Some(&mut *u.ctx), p_ptr)
}

/// `p->pTab == pExpr->y.pTab` para um `TK_COLUMN`.
fn expr_tab_is_ctx_tab(u: &RenameWalk<'_>, p_expr: &Expr) -> bool {
    match p_expr.y_tab() {
        Some(TabRef::Own) => u.own,
        Some(TabRef::Rc(t)) => {
            !u.own && u.ctx.p_tab.as_ref().map_or(false, |c| Rc::ptr_eq(c, t))
        }
        None => false,
    }
}

/// `pItem->pTab == p->pTab` para um termo do FROM.
fn item_tab_is_ctx_tab(u: &RenameWalk<'_>, p_item: &SrcItem) -> bool {
    if u.own {
        return false;
    }
    match (&p_item.p_tab, &u.ctx.p_tab) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `renameWalkWith`: percorre os SELECTs das cláusulas WITH presas ao select `p_select`.
fn rename_walk_with(w: &mut RenameWalker<'_>, p_select: &mut Select) {
    let Some(with) = p_select.p_with.as_deref_mut() else {
        return;
    };
    if with.a.is_empty() {
        return;
    }
    debug_assert!(with.a[0].p_select.is_some());
    let unexpanded =
        with.a[0].p_select.as_deref().map_or(false, |s| (s.sel_flags & SF_EXPANDED) == 0);
    // Uma cópia do WITH vai para a pilha do Parse: o original será expandido e resolvido logo
    // abaixo, e o código do analisador que usa a pilha falha com Select já expandido.
    let mut have_copy = false;
    let mut pushed: Option<Option<Box<With>>> = None;
    if unexpanded {
        let copy = with_dup(Some(&*with));
        have_copy = copy.is_some();
        if let Ok(prev) = with_push(&mut *w.u.parse, copy) {
            pushed = Some(prev);
        }
    }
    for i in 0..with.a.len() {
        let mut s_nc = name_context_new();
        if have_copy {
            if let Some(p) = with.a[i].p_select.as_deref_mut() {
                select_prep(&mut *w.u.db, &mut *w.u.parse, p, Some(&mut s_nc));
            }
        }
        if w.u.db.malloc_failed != 0 {
            return;
        }
        walk_select(w, with.a[i].p_select.as_deref_mut());
        rename_exprlist_unmap(&mut *w.u.parse, with.a[i].p_cols.as_deref());
    }
    if let Some(prev) = pushed {
        w.u.parse.p_with = prev;
    }
}

/// `renameColumnSelectCb`: o callback de SELECT que só existe para o walker descer nas
/// subconsultas; também percorre os WITH.
fn rename_column_select_cb(w: &mut RenameWalker<'_>, p: &mut Select) -> i32 {
    if (p.sel_flags & (SF_VIEW | SF_COPYCTE)) != 0 {
        return WRC_PRUNE;
    }
    rename_walk_with(w, p);
    WRC_CONTINUE
}

/// `renameColumnExprCb`: para cada `TK_COLUMN` (ou `TK_TRIGGER`) que referencia a coluna
/// renomeada, move o `RenameToken` dele para a lista do contexto.
fn rename_column_expr_cb(w: &mut RenameWalker<'_>, p_expr: &mut Expr) -> i32 {
    let i_col = w.u.ctx.i_col;
    let trigger_tab_is_ctx = match (&w.u.parse.p_trigger_tab, &w.u.ctx.p_tab) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    };
    if p_expr.op == TK_TRIGGER && p_expr.i_column == i_col && trigger_tab_is_ctx {
        find_in(&mut w.u, expr_addr(p_expr));
    } else if p_expr.op == TK_COLUMN
        && p_expr.i_column == i_col
        && p_expr.use_y_tab()
        && expr_tab_is_ctx_tab(&w.u, p_expr)
    {
        find_in(&mut w.u, expr_addr(p_expr));
    }
    WRC_CONTINUE
}

/// `renameTableExprCb`: o callback de expressão de `RENAME TABLE`.
fn rename_table_expr_cb(w: &mut RenameWalker<'_>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN && p_expr.use_y_tab() && expr_tab_is_ctx_tab(&w.u, p_expr) {
        find_in(&mut w.u, &p_expr.y as *const _ as usize);
    }
    WRC_CONTINUE
}

/// `renameTableSelectCb`: o callback de SELECT de `RENAME TABLE`.
fn rename_table_select_cb(w: &mut RenameWalker<'_>, p_select: &mut Select) -> i32 {
    if (p_select.sel_flags & (SF_VIEW | SF_COPYCTE)) != 0 {
        return WRC_PRUNE;
    }
    let Some(p_src) = p_select.p_src.as_deref() else {
        return WRC_ABORT;
    };
    for p_item in p_src.a.iter() {
        if item_tab_is_ctx_tab(&w.u, p_item) {
            find_in(&mut w.u, text_addr(&p_item.z_name));
        }
    }
    rename_walk_with(w, p_select);
    WRC_CONTINUE
}

/// `renameQuotefixExprCb`: o callback de expressão de `sqlite_rename_quotefix()`.
fn rename_quotefix_expr_cb(w: &mut RenameWalker<'_>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_STRING && (p_expr.flags & EP_DBL_QUOTED) != 0 {
        find_in(&mut w.u, expr_addr(p_expr));
    }
    WRC_CONTINUE
}

/// `renameColumnElistNames`: para cada nome da lista igual a `z_old`, move o token para `p_ctx`.
fn rename_column_elist_names(
    parse: &mut Parse,
    p_ctx: &mut RenameCtx,
    p_e_list: Option<&ExprList>,
    z_old: &[u8],
) {
    if let Some(list) = p_e_list {
        for item in list.a.iter() {
            if item.fg.e_e_name == ENAME_NAME {
                if let Some(z_name) = item.z_e_name.as_ref() {
                    if 0 == str_icmp(z_name, z_old) {
                        rename_token_find(parse, Some(&mut *p_ctx), z_name.as_ptr() as usize);
                    }
                }
            }
        }
    }
}

/// `renameColumnIdlistNames`: para cada nome da lista de identificadores igual a `z_old`, move o
/// token para `p_ctx`.
fn rename_column_idlist_names(
    parse: &mut Parse,
    p_ctx: &mut RenameCtx,
    p_id_list: Option<&IdList>,
    z_old: &[u8],
) {
    if let Some(list) = p_id_list {
        for item in list.a.iter() {
            if 0 == stricmp(item.z_name.as_deref(), Some(z_old)) {
                rename_token_find(parse, Some(&mut *p_ctx), text_addr(&item.z_name));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Análise do SQL, edição e limpeza
// ---------------------------------------------------------------------------------------------

/// `renameParseSql`: analisa `z_sql` em modo RENAME com um `Parse` novo, que devolve junto com o
/// código de retorno (o número de erros do analisador, como no C).
fn rename_parse_sql(
    db: &mut Connection,
    z_db: Option<&[u8]>,
    z_sql: Option<&[u8]>,
    b_temp: bool,
) -> (Parse, i32) {
    let mut p = parse_object_init(db);
    let Some(z_sql) = z_sql else {
        return (p, SQLITE_NOMEM);
    };
    if strnicmp(Some(z_sql), Some(b"CREATE "), 7) != 0 {
        return (p, SQLITE_CORRUPT_BKPT);
    }
    db.init.i_db = if b_temp { 1 } else { find_db_name(db, z_db) as u8 };
    p.e_parse_mode = PARSE_MODE_RENAME;
    p.n_query_loop = 1;
    p.z_sql = z_sql.to_vec();
    let mut rc = run_parser(db, &mut p, z_sql);
    if db.malloc_failed != 0 {
        rc = SQLITE_NOMEM;
    }
    if rc == SQLITE_OK
        && p.p_new_table.is_none()
        && p.p_new_index.is_empty()
        && p.p_new_trigger.is_none()
    {
        rc = SQLITE_CORRUPT_BKPT;
    }
    db.init.i_db = 0;
    (p, rc)
}

/// `renameEditSql`: edita `z_sql` trocando cada token da lista por `z_new` (sempre entre aspas se
/// `b_quote`) ou, com `z_new` ausente, por uma versão entre aspas simples dele mesmo. Os tokens
/// são aplicados do último para o primeiro, então as posições dos anteriores continuam valendo.
fn rename_edit_sql(
    mut tokens: Vec<RenameToken>,
    z_sql: &[u8],
    z_new: Option<&[u8]>,
    b_quote: bool,
) -> Vec<u8> {
    let sql = cstr(z_sql);
    let n_sql = sql.len();
    let mut out = sql.to_vec();

    // `zQuot`: o novo nome entre aspas duplas e um espaço; só os primeiros `nQuot` bytes valem,
    // salvo se o token é seguido de aspa dupla.
    let z_quot: Vec<u8> = match z_new {
        Some(n) => mprintf(b"\"%w\" ", &[text_arg(cstr(n))]).unwrap_or_default(),
        None => Vec::new(),
    };
    let n_quot = z_quot.len().saturating_sub(1);

    while !tokens.is_empty() {
        // `renameColumnTokenNext`: o token que aparece por último no SQL (o mais novo desempata).
        let mut best = 0usize;
        for (i, t) in tokens.iter().enumerate() {
            if t.t.i_ofst >= tokens[best].t.i_ofst {
                best = i;
            }
        }
        let tok = tokens.remove(best);
        let i_off = tok.t.i_ofst.max(0) as usize;
        let i_end = i_off + tok.t.z.len();
        let next_ch = sql.get(i_end).copied().unwrap_or(0);

        let z_replace: Vec<u8> = match z_new {
            Some(z_new) => {
                let first = tok.t.z.first().copied().unwrap_or(0);
                if !b_quote && is_id_char(first) {
                    cstr(z_new).to_vec()
                } else {
                    let mut n_replace = n_quot;
                    if next_ch == b'"' {
                        n_replace += 1;
                    }
                    z_quot[..n_replace.min(z_quot.len())].to_vec()
                }
            }
            None => {
                // Remove as aspas do token entre aspas duplas e o escreve de novo com aspas
                // simples. Se o caractere depois do token é aspa simples, acrescenta um espaço,
                // para que (SELECT "string"'alias') vire (SELECT 'string' 'alias').
                let mut buf1 = tok.t.z.clone();
                buf1.push(0);
                dequote(&mut buf1);
                let z_space: &[u8] = if next_ch == b'\'' { b" " } else { b"" };
                snprintf(
                    (n_sql * 2) as i32,
                    b"%Q%s",
                    &[PrintfArg::Text(Some(cstr(&buf1).to_vec())), text_arg(z_space)],
                )
            }
        };
        if i_end > out.len() {
            continue;
        }
        out.splice(i_off..i_end, z_replace);
    }
    out
}

/// `renameParseCleanup`: libera o conteúdo do `Parse` de uma passagem de renomeação.
fn rename_parse_cleanup(db: &mut Connection, parse: &mut Parse) {
    if let Some(v) = parse.p_vdbe.take() {
        vdbe_finalize(*v, db);
    }
    if let Some(t) = parse.p_new_table.take() {
        delete_table(db, Some(Rc::new(*t)));
    }
    parse.p_new_index.clear();
    parse.p_new_trigger = None;
    parse.z_err_msg = None;
    parse.p_rename.clear();
    parse_object_reset(db, parse);
}

/// `renameColumnParseError`: acrescenta contexto à mensagem de erro deixada em `parse.z_err_msg`.
fn rename_column_parse_error(
    z_when: &[u8],
    z_t: Option<Vec<u8>>,
    z_n: Option<Vec<u8>>,
    parse: &Parse,
) -> Vec<u8> {
    let z_sep: &[u8] = if z_when.is_empty() { b"" } else { b" " };
    mprintf(
        b"error in %s %s%s%s: %s",
        &[
            PrintfArg::Text(z_t),
            PrintfArg::Text(z_n),
            text_arg(z_sep),
            text_arg(z_when),
            PrintfArg::Text(parse.z_err_msg.clone()),
        ],
    )
    .unwrap_or_default()
}

/// `renameSetENames`: põe `val` em todos os `eEName` da lista.
fn rename_set_enames(p_e_list: Option<&mut ExprList>, val: u8) {
    if let Some(list) = p_e_list {
        for item in list.a.iter_mut() {
            debug_assert!(val == ENAME_NAME || item.fg.e_e_name == ENAME_NAME);
            item.fg.e_e_name = val;
        }
    }
}

/// `renameResolveTrigger`: resolve os símbolos do gatilho `p_new` (o `pParse->pNewTrigger` do C,
/// que o chamador tirou do `Parse`). Devolve `SQLITE_OK` ou um erro, com a mensagem em `parse`.
fn rename_resolve_trigger(db: &mut Connection, parse: &mut Parse, p_new: &mut Trigger) -> i32 {
    let mut s_nc = name_context_new();
    let i_tab_db = schema_to_index(db, p_new.p_tab_schema);
    let z_tab_db = db_name_at(db, i_tab_db);
    debug_assert!(p_new.p_tab_schema.0 != 0);
    parse.p_trigger_tab = find_table(db, &p_new.z_table, Some(&z_tab_db));
    parse.e_trigger_op = p_new.op;
    let mut rc = SQLITE_OK;
    if let Some(mut t) = parse.p_trigger_tab.clone() {
        rc = (view_get_column_names(db, parse, &mut t) != 0) as i32;
        parse.p_trigger_tab = Some(t);
    }

    // Resolve a cláusula WHEN.
    if rc == SQLITE_OK && p_new.p_when.is_some() {
        rc = resolve_expr_names(db, parse, &mut s_nc, p_new.p_when.as_deref_mut());
    }

    let trig_schema = p_new.p_schema;
    for p_step in p_new.step_list.iter_mut() {
        if rc != SQLITE_OK {
            break;
        }
        if let Some(sel) = p_step.p_select.as_deref_mut() {
            select_prep(db, parse, sel, Some(&mut s_nc));
            if parse.n_err != 0 {
                rc = parse.rc;
            }
        }
        if rc == SQLITE_OK && p_step.z_target.is_some() {
            let p_src = trigger_step_src(db, parse, p_step, trig_schema);
            let Some(p_src) = p_src else {
                rc = SQLITE_NOMEM;
                continue;
            };
            let had_list = p_step.p_expr_list.is_some();
            let p_sel = select_new(
                parse,
                p_step.p_expr_list.take(),
                Some(p_src),
                None,
                None,
                None,
                None,
                0,
                None,
            );
            let Some(mut p_sel) = p_sel else {
                rc = SQLITE_NOMEM;
                continue;
            };
            // `pStep->pExprList` é o lado direito de um UPDATE: os `zEName` são o texto das
            // expressões `<col> = <expr>`. Antes do SelectPrep trocam-se os eEName para
            // ENAME_SPAN (de ENAME_NAME), para que nenhum id de um ON() do FROM seja resolvido
            // como alias de coluna.
            if had_list {
                rename_set_enames(p_sel.p_e_list.as_deref_mut(), ENAME_SPAN);
            }
            select_prep(db, parse, &mut p_sel, None);
            if had_list {
                rename_set_enames(p_sel.p_e_list.as_deref_mut(), ENAME_NAME);
            }
            rc = if parse.n_err != 0 { SQLITE_ERROR } else { SQLITE_OK };
            if had_list {
                p_step.p_expr_list = p_sel.p_e_list.take();
            }
            let mut p_src = p_sel.p_src.take().unwrap_or_default();
            drop(p_sel);
            if let Some(p_from) = p_step.p_from.as_deref_mut() {
                for p in p_from.a.iter_mut() {
                    if rc != SQLITE_OK {
                        break;
                    }
                    if let Some(s) = p.p_select.as_deref_mut() {
                        select_prep(db, parse, s, None);
                    }
                }
            }
            if db.malloc_failed != 0 {
                rc = SQLITE_NOMEM;
            }
            // A cópia que o upsert usa como `pUpsertSrc` sai antes de o contexto de nomes tomar
            // `p_src` emprestado.
            let p_src_snap: Box<SrcList> = Box::new((*p_src).clone());
            let mut s_nc = name_context_new();
            s_nc.p_src_list = Some(&mut *p_src);
            if rc == SQLITE_OK && p_step.p_where.is_some() {
                rc = resolve_expr_names(db, parse, &mut s_nc, p_step.p_where.as_deref_mut());
            }
            if rc == SQLITE_OK {
                rc = resolve_expr_list_names(db, parse, &mut s_nc, p_step.p_expr_list.as_deref_mut());
            }
            debug_assert!(
                p_step.p_upsert.is_none()
                    || (p_step.p_where.is_none() && p_step.p_expr_list.is_none())
            );
            if rc == SQLITE_OK {
                if let Some(up) = p_step.p_upsert.as_deref_mut() {
                    // `pUpsert->pUpsertSrc = pSrc`, e `sNC.uNC.pUpsert = pUpsert`: o contexto de
                    // nomes lê só `regData` (zero aqui) e a lista `pUpsertSrc`, que vão num
                    // instantâneo para não aliasar as expressões que a resolução altera.
                    up.p_upsert_src = Some(p_src_snap);
                    let snap = Upsert { p_upsert_src: up.p_upsert_src.clone(), ..Upsert::default() };
                    s_nc.u_nc = NcU::Upsert(&snap);
                    s_nc.nc_flags = NC_UUPSERT;
                    rc = resolve_expr_list_names(db, parse, &mut s_nc, up.p_upsert_target.as_deref_mut());
                    if rc == SQLITE_OK {
                        rc = resolve_expr_list_names(db, parse, &mut s_nc, up.p_upsert_set.as_deref_mut());
                    }
                    if rc == SQLITE_OK {
                        rc = resolve_expr_names(db, parse, &mut s_nc, up.p_upsert_where.as_deref_mut());
                    }
                    if rc == SQLITE_OK {
                        rc = resolve_expr_names(
                            db,
                            parse,
                            &mut s_nc,
                            up.p_upsert_target_where.as_deref_mut(),
                        );
                    }
                    s_nc.nc_flags = 0;
                }
            }
        }
    }
    rc
}

/// `renameWalkTrigger`: percorre todos os `Select` e `Expr` do gatilho.
fn rename_walk_trigger(w: &mut RenameWalker<'_>, p_trigger: &mut Trigger) {
    // Tokens da cláusula WHEN.
    walk_expr(w, p_trigger.p_when.as_deref_mut());

    // Tokens dos passos.
    for p_step in p_trigger.step_list.iter_mut() {
        walk_select(w, p_step.p_select.as_deref_mut());
        walk_expr(w, p_step.p_where.as_deref_mut());
        walk_expr_list(w, p_step.p_expr_list.as_deref_mut());
        if let Some(up) = p_step.p_upsert.as_deref_mut() {
            walk_expr_list(w, up.p_upsert_target.as_deref_mut());
            walk_expr_list(w, up.p_upsert_set.as_deref_mut());
            walk_expr(w, up.p_upsert_where.as_deref_mut());
            walk_expr(w, up.p_upsert_target_where.as_deref_mut());
        }
        if let Some(from) = p_step.p_from.as_deref_mut() {
            for item in from.a.iter_mut() {
                walk_select(w, item.p_select.as_deref_mut());
            }
        }
    }
}

/// A seleção da view em `Table.u.view` (fora da tabela enquanto é percorrida).
fn take_view_select(p_tab: &mut Table) -> Option<Box<Select>> {
    match &mut p_tab.u {
        TableU::View(v) => v.p_select.take(),
        _ => None,
    }
}

/// Devolve a seleção que `take_view_select` tirou.
fn put_view_select(p_tab: &mut Table, p_select: Option<Box<Select>>) {
    if let TableU::View(v) = &mut p_tab.u {
        v.p_select = p_select;
    }
}

/// Percorre as expressões DEFAULT e GENERATED das colunas (`sqlite3ColumnExpr` de cada uma).
fn walk_column_exprs(w: &mut RenameWalker<'_>, p_tab: &mut Table) {
    let dflts: Vec<u16> = p_tab.a_col.iter().map(|c| c.i_dflt).collect();
    if let TableU::Tab(info) = &mut p_tab.u {
        if let Some(list) = info.p_dflt_list.as_deref_mut() {
            for d in dflts {
                if d != 0 && (d as usize) <= list.a.len() {
                    walk_expr(w, list.a[d as usize - 1].p_expr.as_deref_mut());
                }
            }
        }
    }
}

/// Percorre as expressões de CHECK e dos índices UNIQUE da tabela em construção (e dos índices
/// novos do `Parse`, que o chamador tirou de `p_new_index`).
fn walk_new_table_exprs(w: &mut RenameWalker<'_>, p_tab: &mut Table, p_new_index: &mut [Box<Index>]) {
    walk_expr_list(w, p_tab.p_check.as_deref_mut());
    for p_idx in p_tab.p_index.iter_mut() {
        // O índice é só da tabela em construção: a contagem do `Rc` é 1.
        if let Some(idx) = Rc::get_mut(p_idx) {
            walk_expr_list(w, idx.a_col_expr.as_deref_mut());
        }
    }
    for p_idx in p_new_index.iter_mut() {
        walk_expr_list(w, p_idx.a_col_expr.as_deref_mut());
    }
    walk_column_exprs(w, p_tab);
}

// ---------------------------------------------------------------------------------------------
// Resultado de uma função SQL interna
// ---------------------------------------------------------------------------------------------

/// O que a função SQL devolve, calculado com a conexão emprestada e gravado no `Context` depois.
enum Outcome {
    /// Nenhum resultado (NULL).
    Nothing,
    /// `sqlite3_result_text`.
    Text(Vec<u8>),
    /// `sqlite3_result_value(argv[i])`.
    Value(usize),
    /// `sqlite3_result_int`.
    Int(i32),
    /// `sqlite3_result_error` com a mensagem.
    Error(Vec<u8>),
    /// `sqlite3_result_error_code`.
    Code(i32),
}

/// Grava o resultado no `Context`.
fn apply_outcome(ctx: &mut Context<'_>, argv: &[Mem], o: Outcome) {
    match o {
        Outcome::Nothing => {}
        Outcome::Text(z) => {
            let n = strlen30(&z);
            result_text(ctx, Some(&z[..n as usize]), n, StrDtor::Transient);
        }
        Outcome::Value(i) => result_value(ctx, &argv[i]),
        Outcome::Int(v) => result_int(ctx, v),
        Outcome::Error(z) => {
            let n = strlen30(&z);
            result_error(ctx, &z[..n as usize], n);
        }
        Outcome::Code(rc) => result_error_code(ctx, rc),
    }
}

/// O erro comum das funções: com `writable_schema` devolve o SQL original (`orig`, o índice do
/// argumento); senão a mensagem do analisador com contexto (`with_message`) ou só o código.
fn failure_outcome(
    db: &Connection,
    rc: i32,
    orig: usize,
    parse: &Parse,
    argv: &[Mem],
    with_message: bool,
) -> Outcome {
    if rc == SQLITE_ERROR && writable_schema(db) {
        Outcome::Value(orig)
    } else if with_message && parse.z_err_msg.is_some() {
        Outcome::Error(rename_column_parse_error(b"", arg_text(&argv[1]), arg_text(&argv[2]), parse))
    } else {
        Outcome::Code(rc)
    }
}

// ---------------------------------------------------------------------------------------------
// sqlite_rename_column(SQL,TYPE,OBJ,DB,TABLE,COL,NEWNAME,QUOTE,TEMP)
// ---------------------------------------------------------------------------------------------

/// `renameColumnFunc`: faz a renomeação de coluna sobre o CREATE em `argv[0]`. O `iCol`-ésimo
/// campo (o primeiro é 0) da tabela `argv[4]` passa a se chamar `argv[6]`, entre aspas se
/// `argv[7]`. Só é alcançável por SQL criado por `sqlite3NestedParse()`.
fn rename_column_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let z_sql = arg_text(&argv[0]);
    let z_db = arg_text(&argv[3]);
    let z_table = arg_text(&argv[4]);
    let i_col = value_int(&argv[5]);
    let z_new = arg_text(&argv[6]);
    let b_quote = value_int(&argv[7]) != 0;
    let b_temp = value_int(&argv[8]) != 0;
    let (Some(z_sql), Some(z_table), Some(z_new)) = (z_sql, z_table, z_new) else {
        return;
    };
    if i_col < 0 {
        return;
    }
    let o = rename_column_run(&mut *ctx.db, argv, &z_sql, z_db.as_deref(), &z_table, i_col, &z_new, b_quote, b_temp);
    apply_outcome(ctx, argv, o);
}

/// O corpo de `renameColumnFunc`, com a conexão emprestada.
fn rename_column_run(
    db: &mut Connection,
    argv: &[Mem],
    z_sql: &[u8],
    z_db: Option<&[u8]>,
    z_table: &[u8],
    i_col: i32,
    z_new: &[u8],
    b_quote: bool,
    b_temp: bool,
) -> Outcome {
    let Some(p_tab) = find_table(db, z_table, z_db) else {
        return Outcome::Nothing;
    };
    if i_col >= p_tab.n_col as i32 {
        return Outcome::Nothing;
    }
    let z_old = cstr(&p_tab.a_col[i_col as usize].z_cn_name).to_vec();
    let mut s_ctx = RenameCtx {
        i_col: if i_col == p_tab.i_p_key as i32 { -1 } else { i_col },
        p_tab: Some(p_tab.clone()),
        ..RenameCtx::default()
    };

    let x_auth = db.x_auth.take();
    let (mut s_parse, mut rc) = rename_parse_sql(db, z_db, Some(z_sql), b_temp);

    if rc == SQLITE_OK {
        let mut w = rename_walker(
            &mut *db,
            &mut s_parse,
            &mut s_ctx,
            rename_column_expr_cb,
            rename_column_select_cb,
        );
        rc = rename_column_collect(&mut w, &p_tab, z_table, z_db, &z_old, i_col);
    }
    let o = if rc == SQLITE_OK {
        Outcome::Text(rename_edit_sql(std::mem::take(&mut s_ctx.p_list), z_sql, Some(z_new), b_quote))
    } else {
        failure_outcome(db, rc, 0, &s_parse, argv, true)
    };
    rename_parse_cleanup(db, &mut s_parse);
    db.x_auth = x_auth;
    o
}

/// Acha os tokens que referenciam a coluna renomeada na tabela, índice, view ou gatilho que o
/// `Parse` do walker analisou. Devolve `SQLITE_OK` ou o código que o C leva ao `_done`.
fn rename_column_collect(
    w: &mut RenameWalker<'_>,
    p_tab: &Rc<Table>,
    z_table: &[u8],
    z_db: Option<&[u8]>,
    z_old: &[u8],
    i_col: i32,
) -> i32 {
    if let Some(mut p_new) = w.u.parse.p_new_table.take() {
        let mut rc = SQLITE_OK;
        if p_new.is_view() {
            let mut p_select = take_view_select(&mut p_new);
            if let Some(sel) = p_select.as_deref_mut() {
                sel.sel_flags &= !SF_VIEW;
                w.u.parse.rc = SQLITE_OK;
                select_prep(&mut *w.u.db, &mut *w.u.parse, sel, None);
                rc = if w.u.db.malloc_failed != 0 { SQLITE_NOMEM } else { w.u.parse.rc };
                if rc == SQLITE_OK {
                    walk_select(w, Some(sel));
                }
            }
            put_view_select(&mut p_new, p_select);
        } else if p_new.is_ordinary_table() {
            // Uma tabela comum.
            let b_fk_only = stricmp(Some(z_table), Some(&p_new.z_name));
            w.u.own = true;
            if b_fk_only == 0 {
                if (i_col as usize) < p_new.a_col.len() {
                    let a = p_new.a_col[i_col as usize].z_cn_name.as_ptr() as usize;
                    find_in(&mut w.u, a);
                }
                if w.u.ctx.i_col < 0 {
                    find_in(&mut w.u, &p_new.i_p_key as *const i16 as usize);
                }
                let mut new_idx = std::mem::take(&mut w.u.parse.p_new_index);
                walk_new_table_exprs(w, &mut p_new, &mut new_idx);
                w.u.parse.p_new_index = new_idx;
            }
            if let Some(info) = p_new.u_tab() {
                for p_fkey in info.p_f_key.iter() {
                    for i in 0..p_fkey.a_col.len() {
                        if b_fk_only == 0 && p_fkey.a_col[i].i_from == i_col {
                            find_in(&mut w.u, &p_fkey.a_col[i] as *const FKeyColMap as usize);
                        }
                        if 0 == str_icmp(&p_fkey.z_to, z_table)
                            && 0 == stricmp(p_fkey.a_col[i].z_col.as_deref(), Some(z_old))
                        {
                            find_in(&mut w.u, text_addr(&p_fkey.a_col[i].z_col));
                        }
                    }
                }
            }
            w.u.own = false;
        }
        w.u.parse.p_new_table = Some(p_new);
        return rc;
    }
    if !w.u.parse.p_new_index.is_empty() {
        let mut idx = std::mem::take(&mut w.u.parse.p_new_index);
        walk_expr_list(w, idx[0].a_col_expr.as_deref_mut());
        walk_expr(w, idx[0].p_partial_idx_where.as_deref_mut());
        w.u.parse.p_new_index = idx;
        return SQLITE_OK;
    }
    // Um gatilho.
    let Some(mut p_trig) = w.u.parse.p_new_trigger.take() else {
        return SQLITE_CORRUPT_BKPT;
    };
    let rc = rename_resolve_trigger(&mut *w.u.db, &mut *w.u.parse, &mut p_trig);
    if rc != SQLITE_OK {
        w.u.parse.p_new_trigger = Some(p_trig);
        return rc;
    }
    for p_step in p_trig.step_list.iter() {
        if let Some(z_target) = p_step.z_target.as_ref() {
            let p_target = locate_table(&mut *w.u.db, &mut *w.u.parse, 0, z_target, z_db);
            if p_target.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_tab)) {
                if let Some(up) = p_step.p_upsert.as_deref() {
                    rename_column_elist_names(
                        &mut *w.u.parse,
                        &mut *w.u.ctx,
                        up.p_upsert_set.as_deref(),
                        z_old,
                    );
                }
                rename_column_idlist_names(
                    &mut *w.u.parse,
                    &mut *w.u.ctx,
                    p_step.p_id_list.as_deref(),
                    z_old,
                );
                rename_column_elist_names(
                    &mut *w.u.parse,
                    &mut *w.u.ctx,
                    p_step.p_expr_list.as_deref(),
                    z_old,
                );
            }
        }
    }

    // Tokens da cláusula UPDATE OF.
    if w.u.parse.p_trigger_tab.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_tab)) {
        rename_column_idlist_names(
            &mut *w.u.parse,
            &mut *w.u.ctx,
            p_trig.p_columns.as_deref(),
            z_old,
        );
    }

    // Tokens das expressões e dos selects.
    rename_walk_trigger(w, &mut p_trig);
    w.u.parse.p_new_trigger = Some(p_trig);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// sqlite_rename_table(DB,TYPE,OBJ,SQL,OLD,NEW,TEMP)
// ---------------------------------------------------------------------------------------------

/// `renameTableFunc`: devolve a instrução de esquema `argv[3]` com a tabela `argv[4]` renomeada
/// para `argv[5]`: as chaves estrangeiras que a usam como pai, o nome do próprio CREATE, as
/// views, índices e gatilhos que a citam.
fn rename_table_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let z_db = arg_text(&argv[0]);
    let z_input = arg_text(&argv[3]);
    let z_old = arg_text(&argv[4]);
    let z_new = arg_text(&argv[5]);
    let b_temp = value_int(&argv[6]) != 0;
    let (Some(z_input), Some(z_old), Some(z_new)) = (z_input, z_old, z_new) else {
        return;
    };
    let o = rename_table_run(&mut *ctx.db, argv, z_db.as_deref(), &z_input, &z_old, &z_new, b_temp);
    apply_outcome(ctx, argv, o);
}

/// O corpo de `renameTableFunc`, com a conexão emprestada.
fn rename_table_run(
    db: &mut Connection,
    argv: &[Mem],
    z_db: Option<&[u8]>,
    z_input: &[u8],
    z_old: &[u8],
    z_new: &[u8],
    b_temp: bool,
) -> Outcome {
    let x_auth = db.x_auth.take();
    let mut s_ctx = RenameCtx { p_tab: find_table(db, z_old, z_db), ..RenameCtx::default() };
    let (mut s_parse, mut rc) = rename_parse_sql(db, z_db, Some(z_input), b_temp);

    if rc == SQLITE_OK {
        let mut w = rename_walker(
            &mut *db,
            &mut s_parse,
            &mut s_ctx,
            rename_table_expr_cb,
            rename_table_select_cb,
        );
        rc = rename_table_collect(&mut w, z_old);
    }
    let o = if rc == SQLITE_OK {
        Outcome::Text(rename_edit_sql(std::mem::take(&mut s_ctx.p_list), z_input, Some(z_new), true))
    } else {
        failure_outcome(db, rc, 3, &s_parse, argv, true)
    };
    rename_parse_cleanup(db, &mut s_parse);
    db.x_auth = x_auth;
    o
}

/// A coleta de tokens de `renameTableFunc` (o `if( rc==SQLITE_OK ){...}` do C).
fn rename_table_collect(w: &mut RenameWalker<'_>, z_old: &[u8]) -> i32 {
    let mut rc = SQLITE_OK;
    let is_legacy = (w.u.db.flags & SQLITE_LEGACY_ALTER) != 0;
    if let Some(mut p_tab) = w.u.parse.p_new_table.take() {
        if p_tab.is_view() {
            if !is_legacy {
                let mut p_select = take_view_select(&mut p_tab);
                if let Some(sel) = p_select.as_deref_mut() {
                    let mut s_nc = name_context_new();
                    debug_assert!((sel.sel_flags & SF_VIEW) != 0);
                    sel.sel_flags &= !SF_VIEW;
                    select_prep(&mut *w.u.db, &mut *w.u.parse, sel, Some(&mut s_nc));
                    if w.u.parse.n_err != 0 {
                        rc = w.u.parse.rc;
                    } else {
                        walk_select(w, Some(sel));
                    }
                }
                put_view_select(&mut p_tab, p_select);
            }
        } else {
            // Faz as chaves estrangeiras apontarem para a tabela nova.
            if !p_tab.is_virtual() && (!is_legacy || (w.u.db.flags & SQLITE_FOREIGN_KEYS) != 0) {
                if let Some(info) = p_tab.u_tab() {
                    for p_fkey in info.p_f_key.iter() {
                        if stricmp(Some(&p_fkey.z_to), Some(z_old)) == 0 {
                            find_in(&mut w.u, p_fkey.z_to.as_ptr() as usize);
                        }
                    }
                }
            }

            // Se é a tabela alterada, corrige as referências nos CHECK e o nome logo depois de
            // "CREATE [VIRTUAL] TABLE".
            if stricmp(Some(z_old), Some(&p_tab.z_name)) == 0 {
                w.u.own = true;
                if !is_legacy {
                    walk_expr_list(w, p_tab.p_check.as_deref_mut());
                }
                find_in(&mut w.u, p_tab.z_name.as_ptr() as usize);
                w.u.own = false;
            }
        }
        w.u.parse.p_new_table = Some(p_tab);
    } else if !w.u.parse.p_new_index.is_empty() {
        let mut idx = std::mem::take(&mut w.u.parse.p_new_index);
        find_in(&mut w.u, idx[0].z_name.as_ptr() as usize);
        if !is_legacy {
            walk_expr(w, idx[0].p_partial_idx_where.as_deref_mut());
        }
        w.u.parse.p_new_index = idx;
    } else if let Some(mut p_trigger) = w.u.parse.p_new_trigger.take() {
        let tab_schema = w.u.ctx.p_tab.as_ref().map(|t| t.p_schema);
        if 0 == stricmp(Some(&p_trigger.z_table), Some(z_old))
            && tab_schema == Some(p_trigger.p_tab_schema)
        {
            find_in(&mut w.u, p_trigger.z_table.as_ptr() as usize);
        }
        if !is_legacy {
            rc = rename_resolve_trigger(&mut *w.u.db, &mut *w.u.parse, &mut p_trigger);
            if rc == SQLITE_OK {
                rename_walk_trigger(w, &mut p_trigger);
                for p_step in p_trigger.step_list.iter() {
                    if let Some(z_target) = p_step.z_target.as_ref() {
                        if 0 == str_icmp(z_target, z_old) {
                            find_in(&mut w.u, z_target.as_ptr() as usize);
                        }
                    }
                    if let Some(p_from) = p_step.p_from.as_deref() {
                        for p_item in p_from.a.iter() {
                            if 0 == stricmp(p_item.z_name.as_deref(), Some(z_old)) {
                                find_in(&mut w.u, text_addr(&p_item.z_name));
                            }
                        }
                    }
                }
            }
        }
        w.u.parse.p_new_trigger = Some(p_trigger);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// sqlite_rename_quotefix(DB,SQL)
// ---------------------------------------------------------------------------------------------

/// `renameQuotefixFunc`: reescreve o DDL `argv[1]` trocando as strings entre aspas duplas por
/// strings entre aspas simples. Se há erro no SQL de entrada levanta o erro, salvo com
/// `PRAGMA writable_schema=ON`, em que devolve a entrada sem mudança.
fn rename_quotefix_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let z_db = arg_text(&argv[0]);
    let z_input = arg_text(&argv[1]);
    let (Some(z_db), Some(z_input)) = (z_db, z_input) else {
        return;
    };
    let o = rename_quotefix_run(&mut *ctx.db, argv, &z_db, &z_input);
    apply_outcome(ctx, argv, o);
}

/// O corpo de `renameQuotefixFunc`, com a conexão emprestada.
fn rename_quotefix_run(db: &mut Connection, argv: &[Mem], z_db: &[u8], z_input: &[u8]) -> Outcome {
    let x_auth = db.x_auth.take();
    let (mut s_parse, mut rc) = rename_parse_sql(db, Some(z_db), Some(z_input), false);
    let mut o = Outcome::Nothing;
    if rc == SQLITE_OK {
        let mut s_ctx = RenameCtx::default();
        {
            let mut w = rename_walker(
                &mut *db,
                &mut s_parse,
                &mut s_ctx,
                rename_quotefix_expr_cb,
                rename_column_select_cb,
            );
            rc = rename_quotefix_collect(&mut w);
        }
        if rc == SQLITE_OK {
            o = Outcome::Text(rename_edit_sql(std::mem::take(&mut s_ctx.p_list), z_input, None, false));
        }
    }
    if rc != SQLITE_OK {
        o = failure_outcome(db, rc, 1, &s_parse, argv, false);
    }
    rename_parse_cleanup(db, &mut s_parse);
    db.x_auth = x_auth;
    o
}

/// A coleta de tokens de `renameQuotefixFunc`.
fn rename_quotefix_collect(w: &mut RenameWalker<'_>) -> i32 {
    let mut rc = SQLITE_OK;
    if let Some(mut p_new) = w.u.parse.p_new_table.take() {
        if p_new.is_view() {
            let mut p_select = take_view_select(&mut p_new);
            if let Some(sel) = p_select.as_deref_mut() {
                sel.sel_flags &= !SF_VIEW;
                w.u.parse.rc = SQLITE_OK;
                select_prep(&mut *w.u.db, &mut *w.u.parse, sel, None);
                rc = if w.u.db.malloc_failed != 0 { SQLITE_NOMEM } else { w.u.parse.rc };
                if rc == SQLITE_OK {
                    walk_select(w, Some(sel));
                }
            }
            put_view_select(&mut p_new, p_select);
        } else {
            walk_expr_list(w, p_new.p_check.as_deref_mut());
            walk_column_exprs(w, &mut p_new);
        }
        w.u.parse.p_new_table = Some(p_new);
    } else if !w.u.parse.p_new_index.is_empty() {
        let mut idx = std::mem::take(&mut w.u.parse.p_new_index);
        walk_expr_list(w, idx[0].a_col_expr.as_deref_mut());
        walk_expr(w, idx[0].p_partial_idx_where.as_deref_mut());
        w.u.parse.p_new_index = idx;
    } else if let Some(mut p_trig) = w.u.parse.p_new_trigger.take() {
        rc = rename_resolve_trigger(&mut *w.u.db, &mut *w.u.parse, &mut p_trig);
        if rc == SQLITE_OK {
            rename_walk_trigger(w, &mut p_trig);
        }
        w.u.parse.p_new_trigger = Some(p_trig);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// sqlite_rename_test(DB,SQL,TYPE,NAME,ISTEMP,WHEN,DQS)
// ---------------------------------------------------------------------------------------------

/// `renameTableTest`: confere que não há problema de análise nem de resolução de símbolos num
/// CREATE TRIGGER|TABLE|VIEW|INDEX. Levanta o erro (salvo com `writable_schema=ON`); se cria um
/// gatilho cuja tabela está no banco `argv[0]`, devolve 1; senão NULL.
fn rename_table_test(ctx: &mut Context<'_>, argv: &[Mem]) {
    let z_db = arg_text(&argv[0]);
    let z_input = arg_text(&argv[1]);
    let b_temp = value_int(&argv[4]) != 0;
    let z_when = arg_text(&argv[5]);
    let b_no_dqs = value_int(&argv[6]) != 0;
    let o = rename_test_run(&mut *ctx.db, argv, z_db, z_input, b_temp, z_when, b_no_dqs);
    apply_outcome(ctx, argv, o);
}

/// O corpo de `renameTableTest`, com a conexão emprestada.
fn rename_test_run(
    db: &mut Connection,
    argv: &[Mem],
    z_db: Option<Vec<u8>>,
    z_input: Option<Vec<u8>>,
    b_temp: bool,
    z_when: Option<Vec<u8>>,
    b_no_dqs: bool,
) -> Outcome {
    let is_legacy = (db.flags & SQLITE_LEGACY_ALTER) != 0;
    let x_auth = db.x_auth.take();
    let mut o = Outcome::Nothing;
    if let (Some(z_db), Some(z_input)) = (z_db, z_input) {
        let flags = db.flags;
        if b_no_dqs {
            db.flags &= !(SQLITE_DQS_DML | SQLITE_DQS_DDL);
        }
        let (mut s_parse, mut rc) = rename_parse_sql(db, Some(&z_db), Some(&z_input), b_temp);
        db.flags |= flags & (SQLITE_DQS_DML | SQLITE_DQS_DDL);
        if rc == SQLITE_OK {
            if !is_legacy && s_parse.p_new_table.as_deref().map_or(false, |t| t.is_view()) {
                let mut s_nc = name_context_new();
                if let Some(mut p_new) = s_parse.p_new_table.take() {
                    let mut p_select = take_view_select(&mut p_new);
                    if let Some(sel) = p_select.as_deref_mut() {
                        select_prep(db, &mut s_parse, sel, Some(&mut s_nc));
                    }
                    put_view_select(&mut p_new, p_select);
                    s_parse.p_new_table = Some(p_new);
                }
                if s_parse.n_err != 0 {
                    rc = s_parse.rc;
                }
            } else if let Some(mut p_trig) = s_parse.p_new_trigger.take() {
                if !is_legacy {
                    rc = rename_resolve_trigger(db, &mut s_parse, &mut p_trig);
                }
                if rc == SQLITE_OK {
                    let i1 = schema_to_index(db, p_trig.p_tab_schema);
                    let i2 = find_db_name(db, Some(&z_db));
                    if i1 == i2 {
                        // Saída do caso B.
                        o = Outcome::Int(1);
                    }
                }
                s_parse.p_new_trigger = Some(p_trig);
            }
        }
        if rc != SQLITE_OK {
            if let Some(z_when) = z_when.as_deref() {
                if !writable_schema(db) {
                    // Saída do caso A.
                    o = Outcome::Error(rename_column_parse_error(
                        z_when,
                        arg_text(&argv[2]),
                        arg_text(&argv[3]),
                        &s_parse,
                    ));
                }
            }
        }
        rename_parse_cleanup(db, &mut s_parse);
    }
    db.x_auth = x_auth;
    o
}

// ---------------------------------------------------------------------------------------------
// sqlite_drop_column(SCHEMA,SQL,COL)
// ---------------------------------------------------------------------------------------------

/// `dropColumnFunc`: `argv[0]` é o índice do esquema, `argv[1]` o CREATE TABLE a modificar e
/// `argv[2]` o índice da coluna a remover. Devolve o CREATE TABLE sem a coluna.
fn drop_column_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let i_schema = value_int(&argv[0]);
    let z_sql = arg_text(&argv[1]);
    let i_col = value_int(&argv[2]);
    let o = drop_column_run(&mut *ctx.db, i_schema, z_sql.as_deref(), i_col);
    apply_outcome(ctx, argv, o);
}

/// O corpo de `dropColumnFunc`, com a conexão emprestada.
fn drop_column_run(db: &mut Connection, i_schema: i32, z_sql: Option<&[u8]>, i_col: i32) -> Outcome {
    if usize::try_from(i_schema).map_or(true, |i| i >= db.dbs.len()) {
        return Outcome::Code(SQLITE_CORRUPT_BKPT);
    }
    let z_db = db_name_at(db, i_schema);
    let x_auth = db.x_auth.take();
    let (mut s_parse, mut rc) = rename_parse_sql(db, Some(&z_db), z_sql, i_schema == 1);
    let mut o = Outcome::Nothing;
    'done: {
        if rc != SQLITE_OK {
            break 'done;
        }
        let z_sql = z_sql.map(cstr).unwrap_or(&[]);
        let Some(p_tab) = s_parse.p_new_table.take() else {
            // Pode acontecer se o sqlite_schema está corrompido.
            rc = SQLITE_CORRUPT_BKPT;
            break 'done;
        };
        if p_tab.n_col == 1 || i_col < 0 || i_col >= p_tab.n_col as i32 {
            s_parse.p_new_table = Some(p_tab);
            rc = SQLITE_CORRUPT_BKPT;
            break 'done;
        }

        let p_col = rename_token_find(
            &mut s_parse,
            None,
            p_tab.a_col[i_col as usize].z_cn_name.as_ptr() as usize,
        );
        let z_end: usize;
        let col_ofst: usize;
        if let Some(p_col) = p_col {
            if i_col < p_tab.n_col as i32 - 1 {
                let p_end = rename_token_find(
                    &mut s_parse,
                    None,
                    p_tab.a_col[i_col as usize + 1].z_cn_name.as_ptr() as usize,
                );
                match p_end {
                    Some(e) => {
                        z_end = e.i_ofst.max(0) as usize;
                        col_ofst = p_col.i_ofst.max(0) as usize;
                    }
                    None => {
                        s_parse.p_new_table = Some(p_tab);
                        rc = SQLITE_CORRUPT_BKPT;
                        break 'done;
                    }
                }
            } else {
                debug_assert!(p_tab.is_ordinary_table());
                z_end = p_tab.u_tab().map_or(0, |t| t.add_col_offset).max(0) as usize;
                // Recua até a vírgula que antecede a última coluna.
                let mut c = (p_col.i_ofst.max(0) as usize).min(z_sql.len());
                while c < z_sql.len() && z_sql[c] != 0 && z_sql[c] != b',' && c > 0 {
                    c -= 1;
                }
                col_ofst = c;
            }
        } else {
            s_parse.p_new_table = Some(p_tab);
            rc = SQLITE_CORRUPT_BKPT;
            break 'done;
        }
        s_parse.p_new_table = Some(p_tab);

        let mut z_new = z_sql[..col_ofst.min(z_sql.len())].to_vec();
        z_new.extend_from_slice(z_sql.get(z_end..).unwrap_or(&[]));
        o = Outcome::Text(z_new);
    }
    rename_parse_cleanup(db, &mut s_parse);
    db.x_auth = x_auth;
    if rc != SQLITE_OK {
        o = Outcome::Code(rc);
    }
    o
}

/// `sqlite3AlterDropColumn`: chamada pelo analisador em `ALTER TABLE pSrc DROP COLUMN pName`.
pub fn alter_drop_column(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: Option<Box<SrcList>>,
    p_name: &Token,
) {
    let Some(p_src) = p_src else {
        return;
    };
    debug_assert!(parse.p_new_table.is_none());
    if db.malloc_failed != 0 {
        return;
    }
    let Some(p_tab) = locate_table_item(db, parse, 0, &p_src.a[0]) else {
        return;
    };

    // Não pode ser view, tabela virtual nem tabela de sistema.
    if SQLITE_OK != is_alterable_table(db, parse, &p_tab) {
        return;
    }
    if SQLITE_OK != is_real_table(db, parse, &p_tab, true) {
        return;
    }

    // Acha o índice da coluna.
    let Some(z_col) = name_from_token(Some(p_name)) else {
        return;
    };
    let i_col = column_index(&p_tab, &z_col);
    if i_col < 0 {
        let t = token_arg(parse, p_name);
        error_msg(db, parse, b"no such column: \"%T\"", &[t]);
        return;
    }

    // Coluna de PRIMARY KEY ou UNIQUE não pode ser removida.
    let flags = p_tab.a_col[i_col as usize].col_flags;
    if (flags & (COLFLAG_PRIMKEY | COLFLAG_UNIQUE)) != 0 {
        let z_kind: &[u8] = if (flags & COLFLAG_PRIMKEY) != 0 { b"PRIMARY KEY" } else { b"UNIQUE" };
        error_msg(
            db,
            parse,
            b"cannot drop %s column: \"%s\"",
            &[text_arg(z_kind), text_arg(&z_col)],
        );
        return;
    }

    // O número de colunas não pode chegar a zero.
    if p_tab.n_col <= 1 {
        error_msg(
            db,
            parse,
            b"cannot drop column \"%s\": no other columns exist",
            &[text_arg(&z_col)],
        );
        return;
    }

    // Edita o sqlite_schema.
    let i_db = schema_to_index(db, p_tab.p_schema);
    debug_assert!(i_db >= 0);
    let z_db = db_name_at(db, i_db);
    if auth_check(db, parse, SQLITE_ALTER_TABLE, Some(&z_db), Some(&p_tab.z_name), Some(&z_col)) != 0 {
        return;
    }
    rename_test_schema(db, parse, &z_db, i_db == 1, b"", false);
    rename_fix_quotes(db, parse, &z_db, i_db == 1);
    nested_parse(
        db,
        parse,
        b"UPDATE \"%w\".sqlite_master SET \
          sql = sqlite_drop_column(%d, sql, %d) \
          WHERE (type=='table' AND tbl_name=%Q COLLATE nocase)",
        &[
            text_arg(&z_db),
            int_arg(i_db as i64),
            int_arg(i_col as i64),
            text_arg(&p_tab.z_name),
        ],
    );

    // Apaga e recarrega o esquema.
    rename_reload_schema(db, parse, i_db, INITFLAG_ALTERDROP as u16);
    rename_test_schema(db, parse, &z_db, i_db == 1, b"after drop column", true);

    // Edita as linhas da tabela em disco.
    if parse.n_err == 0 && (p_tab.a_col[i_col as usize].col_flags & COLFLAG_VIRTUAL) == 0 {
        let mut n_field: i32 = 0; // Campos não virtuais depois da remoção.
        get_vdbe(db, parse);
        let i_cur = parse.n_tab;
        parse.n_tab += 1;
        open_table(db, parse, i_cur, i_db, &p_tab, OP_OPENWRITE);
        let addr = add_op1(vdbe_of_parse(parse), OP_REWIND as i32, i_cur);
        parse.n_mem += 1;
        let reg = parse.n_mem;
        let mut p_pk: Option<Rc<Index>> = None;
        if p_tab.has_rowid() {
            add_op2(vdbe_of_parse(parse), OP_ROWID as i32, i_cur, reg);
            parse.n_mem += p_tab.n_col as i32;
        } else {
            let pk = primary_key_index(&p_tab).cloned();
            if let Some(pk) = &pk {
                parse.n_mem += pk.n_column as i32;
                let v = vdbe_of_parse(parse);
                for i in 0..pk.n_key_col as i32 {
                    add_op3(v, OP_COLUMN as i32, i_cur, i, reg + i + 1);
                }
                n_field = pk.n_key_col as i32;
            }
            p_pk = pk;
        }
        parse.n_mem += 1;
        let reg_rec = parse.n_mem;
        for i in 0..p_tab.n_col as usize {
            if i as i32 != i_col && (p_tab.a_col[i].col_flags & COLFLAG_VIRTUAL) == 0 {
                let reg_out: i32;
                if let Some(pk) = &p_pk {
                    let i_pos = table_column_to_index(pk, i as i16) as i32;
                    let i_col_pos = table_column_to_index(pk, i_col as i16) as i32;
                    if i_pos < pk.n_key_col as i32 {
                        continue;
                    }
                    reg_out = reg + 1 + i_pos - (i_pos > i_col_pos) as i32;
                } else {
                    reg_out = reg + 1 + n_field;
                }
                if i as i32 == p_tab.i_p_key as i32 {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_out);
                } else {
                    // O C troca temporariamente a afinidade REAL por NUMERIC na tabela; a tabela
                    // do esquema é imutável, então a troca vale numa cópia.
                    if p_tab.a_col[i].affinity == SQLITE_AFF_REAL {
                        let mut t = (*p_tab).clone();
                        t.a_col[i].affinity = SQLITE_AFF_NUMERIC;
                        let t = Rc::new(t);
                        expr_code_get_column_of_table(db, parse, &t, i_cur, i as i32, reg_out);
                    } else {
                        expr_code_get_column_of_table(db, parse, &p_tab, i_cur, i as i32, reg_out);
                    }
                }
                n_field += 1;
            }
        }
        if n_field == 0 {
            // dbsqlfuzz 5f09e7bcc78b4954d06bf9f2400d7715f48d1fef
            parse.n_mem += 1;
            add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg + 1);
            n_field = 1;
        }
        let v = vdbe_of_parse(parse);
        add_op3(v, OP_MAKERECORD as i32, reg + 1, n_field, reg_rec);
        if let Some(pk) = &p_pk {
            add_op4_int(v, OP_IDXINSERT as i32, i_cur, reg_rec, reg + 1, pk.n_key_col as i32);
        } else {
            add_op3(v, OP_INSERT as i32, i_cur, reg_rec, reg);
        }
        change_p5(v, OPFLAG_SAVEPOSITION as u16);

        add_op2(v, OP_NEXT as i32, i_cur, addr + 1);
        jump_here(v, addr);
    }
}

// ---------------------------------------------------------------------------------------------
// Registro das funções internas
// ---------------------------------------------------------------------------------------------

/// Macro `INTERNAL_FUNCTION`.
fn internal_function(name: &str, n_arg: i8, f: crate::connection::ScalarFn) -> FuncDef {
    FuncDef {
        n_arg,
        func_flags: SQLITE_FUNC_BUILTIN
            | SQLITE_FUNC_INTERNAL
            | SQLITE_UTF8 as u32
            | SQLITE_FUNC_CONSTANT,
        p_user_data: UserData::None,
        x_s_func: Some(f),
        x_finalize: None,
        x_value: None,
        x_inverse: None,
        z_name: name.as_bytes().to_vec(),
        p_destructor: None,
    }
}

/// `sqlite3AlterFunctions`: registra as funções embutidas que implementam o ALTER TABLE.
pub fn alter_functions() {
    insert_builtin_funcs(vec![
        internal_function("sqlite_rename_column", 9, rename_column_func),
        internal_function("sqlite_rename_table", 7, rename_table_func),
        internal_function("sqlite_rename_test", 7, rename_table_test),
        internal_function("sqlite_drop_column", 3, drop_column_func),
        internal_function("sqlite_rename_quotefix", 2, rename_quotefix_func),
    ]);
}
