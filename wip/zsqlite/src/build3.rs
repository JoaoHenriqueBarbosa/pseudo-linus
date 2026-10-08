//! Terceira parte de `build.c` (trechos 011 a 014): `sqlite3DefaultRowEst`, `sqlite3DropIndex`,
//! as listas de identificadores (`IdList`) e de origem (`SrcList`), as transações (`BEGIN`,
//! `COMMIT`, `ROLLBACK`, `SAVEPOINT`), o banco TEMP, a verificação do cookie do esquema, as
//! operações de escrita, `sqlite3HaltConstraint` e companhia, `REINDEX` e
//! `sqlite3KeyInfoOfIndex`. As CTEs (`sqlite3CteNew`, `sqlite3WithAdd`) vivem em `with.rs`.
//!
//! Segue as decisões de `build.rs` e `build2.rs` (ver os cabeçalhos deles e CONVENTIONS.md):
//!
//! - toda função que precisa da conexão recebe `db: &mut Connection` e `parse: &mut Parse`, nessa
//!   ordem; as que só mexem no `Parse` (`multi_write`, `may_abort`, `halt_constraint`) recebem só
//!   ele, como os chamadores já escritos (`expr_code`, `vdbeaux`);
//! - `Index` não tem `pTable`: `unique_constraint`, `rowid_constraint` e `reindex_table` recebem a
//!   tabela dona por parâmetro;
//! - o que o C libera com `sqlite3SrcListDelete`, `sqlite3IdListDelete`, `sqlite3CteDelete`,
//!   `sqlite3WithDelete` (e o `cteClear`) é o `Drop` do Rust, então essas rotinas não existem.
//!   `sqlite3ArrayAllocate` também some: era o `Vec::push` do C;
//! - `SrcList.nAlloc` não existe (`Vec`), e o limite `SQLITE_MAX_SRCLIST` continua valendo;
//! - `OOM` não existe em Rust, então os ramos `mallocFailed` e o `SQLITE_NOMEM` de
//!   `sqlite3BtreeSetPageSize` somem;
//! - o token vazio (`z == 0`) do C é um `Token` de `z` vazio; o `NOT INDEXED` (`z == 0, n == 1`) é
//!   reconhecido por `parse_reduce::is_not_indexed_token`.

use std::rc::Rc;

use crate::connection::{Connection, Parse};
use crate::consts::{
    JT_LTORJ, JT_RIGHT, OMIT_TEMPDB, OP_AUTOCOMMIT, OP_DROPINDEX, OP_HALT, OP_SAVEPOINT,
    OP_TRANSACTION, P4_STATIC, P5_CONSTRAINTUNIQUE, SF_MULTIVALUE, SF_NESTEDFROM, SQLITE_CANTOPEN,
    SQLITE_CONSTRAINT_PRIMARYKEY, SQLITE_CONSTRAINT_ROWID, SQLITE_CONSTRAINT_UNIQUE, SQLITE_DELETE,
    SQLITE_DROP_INDEX, SQLITE_DROP_TEMP_INDEX, SQLITE_ERROR_MISSING_COLLSEQ,
    SQLITE_ERROR_RETRY, SQLITE_IDXTYPE_APPDEF, SQLITE_LIMIT_LENGTH, SQLITE_OK,
    SQLITE_OPEN_CREATE, SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_EXCLUSIVE, SQLITE_OPEN_READWRITE,
    SQLITE_OPEN_TEMP_DB, SQLITE_SAVEPOINT, SQLITE_TRANSACTION, TK_DEFERRED, TK_EXCLUSIVE,
    TK_ROLLBACK, BTREE_MEMORY,
};
use crate::hash::{hash_find, hash_find_mut, hash_iter};
use crate::mem::KeyInfo;
use crate::printf::{PrintfArg, PrintfSrcAnon, PrintfSrcItem, StrAccum};
use crate::sqlite_int::{
    IdList, IdListItem, Index, LogEst, OnOrUsing, Select, SrcItem, SrcList, SrcU1, SrcU3, Table,
    Token,
};
use crate::util::{error_msg, str_icmp, STR_BINARY};
use crate::vdbe_types::{db_mask_set, db_mask_test, P4};
use crate::vdbeaux::{add_op0, add_op2, add_op4, change_p5};
use crate::select::get_vdbe;
use crate::vdbeaux2::uses_btree;

use crate::alter::rename_token_map;
use crate::auth::auth_check;
use crate::btree::{btree_is_readonly, btree_open, btree_set_page_size};
use crate::build::{
    find_index, find_table, force_not_read_only, name_from_token, nested_parse, schema_table,
    text_arg, two_part_name,
};
use crate::build2::{change_cookie, clear_stat_tables, destroy_root_page, refill_index};
use crate::callback::{find_coll_seq, locate_coll_seq};
use crate::parse_reduce::is_not_indexed_token;
use crate::prepare::{read_schema, schema_to_index};
pub use crate::select::{join_type, key_info_alloc};
pub use crate::util::{column_type, progress_check};

/// `SQLITE_MAX_SRCLIST`: o planejador de consultas não lida com mais de 64 tabelas numa junção;
/// qualquer valor acima disso serve para a maioria dos usos.
const SQLITE_MAX_SRCLIST: usize = 200;

// ---------------------------------------------------------------------------------------------
// Estimativas de linhas
// ---------------------------------------------------------------------------------------------

/// `sqlite3DefaultRowEst`: preenche `Index.ai_row_log_est` com informação padrão, para quando o
/// ANALYZE não rodou.
///
/// `ai_row_log_est[0]` deve conter o número de elementos do índice. Como não se sabe, chuta-se um
/// milhão. `ai_row_log_est[1]` estima as linhas da tabela que casam com um valor qualquer da
/// primeira coluna do índice, `ai_row_log_est[2]` as que casam com uma combinação das duas
/// primeiras colunas, e assim por diante. Sempre vale `a[N] <= a[N-1]` e `a[N] >= 1`.
///
/// `n_row_log_est` é o `pIdx->pTable->nRowLogEst` do C (o índice não conhece a tabela): a rotina o
/// eleva a 99 quando é menor, e o chamador grava o resultado de volta na tabela.
pub fn default_row_est(p_idx: &mut Index, n_row_log_est: &mut LogEst) {
    //                                 10,  9,  8,  7,  6
    const A_VAL: [LogEst; 5] = [33, 32, 30, 28, 26];
    let n_key_col = p_idx.n_key_col as usize;
    let n_copy = A_VAL.len().min(n_key_col);
    let is_unique = p_idx.is_unique_index();

    // Índices com estimativas padrão não devem ter dados do stat1.
    debug_assert!(!p_idx.has_stat1);

    // O primeiro elemento (o número de linhas do índice) é o número estimado de linhas da
    // tabela, ou a metade dele num índice parcial.
    //
    // 2020-05-27: se parte dos dados vem do sqlite_stat1 e o resto é chute, o número estimado de
    // linhas da tabela não pode ficar abaixo de 1000 (LogEst 99). Sem isso os índices sem dados
    // do stat1 acabam ignorados pelo planejador de consultas.
    let mut x = *n_row_log_est;
    if x < 99 {
        x = 99;
        *n_row_log_est = x;
    }
    if p_idx.p_partial_idx_where.is_some() {
        x -= 10; // 10 == sqlite3LogEst(2)
    }
    let a = &mut p_idx.ai_row_log_est;
    a[0] = x;

    // Estima que a[1] vale 10, a[2] vale 9, a[3] vale 8, a[4] vale 7, a[5] vale 6 e cada valor
    // seguinte (se houver) vale 5.
    a[1..1 + n_copy].copy_from_slice(&A_VAL[..n_copy]);
    for slot in a.iter_mut().take(n_key_col + 1).skip(n_copy + 1) {
        *slot = 23; // 23 == sqlite3LogEst(5)
    }

    // 0 == sqlite3LogEst(1)
    if is_unique {
        a[n_key_col] = 0;
    }
}

// ---------------------------------------------------------------------------------------------
// DROP INDEX
// ---------------------------------------------------------------------------------------------

/// O argumento `%S` do `printf` interno a partir de um `SrcItem`. Quando o item não tem alias nem
/// nome, o C olha o `Select` do item (ver `PrintfSrcAnon`).
pub(crate) fn src_item_arg(item: &SrcItem) -> PrintfArg {
    let anon = match item.p_select.as_deref() {
        Some(sel) if (sel.sel_flags & SF_NESTEDFROM) != 0 => {
            PrintfSrcAnon::NestedFrom { sel_id: sel.sel_id }
        }
        Some(sel) if (sel.sel_flags & SF_MULTIVALUE) != 0 => PrintfSrcAnon::MultiValue {
            n_row: match item.u1 {
                SrcU1::NRow(n) => n,
                _ => 0,
            },
        },
        Some(sel) => PrintfSrcAnon::Subquery { sel_id: sel.sel_id },
        None => PrintfSrcAnon::Subquery { sel_id: 0 },
    };
    PrintfArg::SrcItem(PrintfSrcItem {
        z_alias: item.z_alias.clone(),
        z_name: item.z_name.clone(),
        z_database: item.z_database.clone(),
        anon,
    })
}

/// `sqlite3DropIndex`: apaga um índice nomeado. Implementa o comando DROP INDEX. `p_name` é a
/// lista de um só item com o nome do índice.
pub fn drop_index(db: &mut Connection, parse: &mut Parse, p_name: Box<SrcList>, if_exists: i32) {
    'exit_drop_index: {
        if db.malloc_failed != 0 {
            break 'exit_drop_index;
        }
        debug_assert!(parse.n_err == 0); // Nunca chamada com erros anteriores que não sejam OOM.
        debug_assert!(p_name.a.len() == 1);
        if read_schema(db, parse) != SQLITE_OK {
            break 'exit_drop_index;
        }
        let item = &p_name.a[0];
        let found = find_index(
            db,
            item.z_name.as_deref().unwrap_or(&[]),
            item.z_database.as_deref(),
        );
        let Some((p_tab, p_index)) = found else {
            if if_exists == 0 {
                error_msg(db, parse, b"no such index: %S", &[src_item_arg(item)]);
            } else {
                code_verify_named_schema(db, parse, item.z_database.as_deref());
                force_not_read_only(db, parse);
            }
            parse.check_schema = 1;
            break 'exit_drop_index;
        };
        if p_index.idx_type != SQLITE_IDXTYPE_APPDEF {
            error_msg(
                db,
                parse,
                b"index associated with UNIQUE or PRIMARY KEY constraint cannot be dropped",
                &[],
            );
            break 'exit_drop_index;
        }
        let i_db = schema_to_index(db, p_tab.p_schema);
        {
            let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
            let z_tab = schema_table(i_db);
            if auth_check(db, parse, SQLITE_DELETE, Some(z_tab), None, Some(&z_db)) != 0 {
                break 'exit_drop_index;
            }
            let code = if OMIT_TEMPDB == 0 && i_db == 1 {
                SQLITE_DROP_TEMP_INDEX
            } else {
                SQLITE_DROP_INDEX
            };
            if auth_check(
                db,
                parse,
                code,
                Some(&p_index.z_name),
                Some(&p_tab.z_name),
                Some(&z_db),
            ) != 0
            {
                break 'exit_drop_index;
            }
        }

        // Gera o código que tira o índice da tabela de esquema no disco.
        {
            get_vdbe(db, parse);
            let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
            begin_write_operation(db, parse, 1, i_db);
            let fmt: Vec<u8> = [
                b"DELETE FROM %Q.".as_slice(),
                crate::consts::LEGACY_SCHEMA_TABLE,
                b" WHERE name=%Q AND type='index'".as_slice(),
            ]
            .concat();
            nested_parse(db, parse, &fmt, &[text_arg(&z_db), text_arg(&p_index.z_name)]);
            clear_stat_tables(db, parse, i_db, b"idx", &p_index.z_name);
            change_cookie(db, parse, i_db);
            destroy_root_page(db, parse, p_index.tnum, i_db);
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                add_op4(v, OP_DROPINDEX as i32, i_db, 0, 0, P4::Text(p_index.z_name.clone()));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// IdList
// ---------------------------------------------------------------------------------------------

/// `sqlite3IdListAppend`: acrescenta um elemento novo à `IdList` dada, criando a lista se for
/// preciso.
pub fn id_list_append(
    _db: &mut Connection,
    parse: &mut Parse,
    p_list: Option<Box<IdList>>,
    p_token: &Token,
) -> Option<Box<IdList>> {
    let mut p_list = p_list.unwrap_or_default();
    let z_name = name_from_token(Some(p_token));
    if parse.in_rename_object() {
        if let Some(z) = &z_name {
            rename_token_map(parse, z.as_ptr() as usize, p_token);
        }
    }
    p_list.a.push(IdListItem { z_name, idx: 0 });
    Some(p_list)
}

/// `sqlite3IdListIndex`: o índice em `p_list` do identificador `z_name`, ou -1 se não existe.
pub fn id_list_index(p_list: &IdList, z_name: &[u8]) -> i32 {
    for (i, item) in p_list.a.iter().enumerate() {
        if item.z_name.as_deref().map_or(false, |z| str_icmp(z_name, z) == 0) {
            return i as i32;
        }
    }
    -1
}

// ---------------------------------------------------------------------------------------------
// SrcList
// ---------------------------------------------------------------------------------------------

/// Um termo novo de `SrcList`: zerado, com `i_cursor` igual a -1 (o `memset` mais a atribuição do
/// `sqlite3SrcListEnlarge` e do `sqlite3SrcListAppend`).
fn new_src_item() -> SrcItem {
    SrcItem { i_cursor: -1, ..SrcItem::default() }
}

/// `sqlite3SrcListEnlarge`: abre `n_extra` termos novos em `p_src` a partir de `i_start` (que
/// começa em zero). Os termos novos são zerados.
///
/// Exemplo: uma lista com dois termos, A e B. Para acrescentar três ao fim, chama-se
/// `src_list_enlarge(.., 3, 2)` e sai A, B, nil, nil, nil. Com `i_start` igual a 1 sairia
/// A, nil, nil, nil, B; para pôr os novos na frente, `i_start` vale 0 e sai nil, nil, nil, A, B.
///
/// Se a lista fica grande demais deixa-se a lista original como está, registra-se o erro no
/// `Parse` e devolve-se falso.
pub fn src_list_enlarge(
    db: &mut Connection,
    parse: &mut Parse,
    p_src: &mut SrcList,
    n_extra: usize,
    i_start: usize,
) -> bool {
    debug_assert!(n_extra >= 1);
    debug_assert!(i_start <= p_src.a.len());
    if p_src.a.len() + n_extra >= SQLITE_MAX_SRCLIST {
        error_msg(
            db,
            parse,
            b"too many FROM clause terms, max: %d",
            &[PrintfArg::Int(SQLITE_MAX_SRCLIST as i64)],
        );
        return false;
    }
    // Os termos que vêm depois dos novos saem do caminho; os novos nascem zerados.
    p_src.a.splice(i_start..i_start, (0..n_extra).map(|_| new_src_item()));
    true
}

/// `sqlite3SrcListAppend`: acrescenta um nome de tabela novo à `SrcList` dada, criando a lista se
/// for preciso. O termo novo é criado mesmo que `p_table` seja `None`.
///
/// Devolve `None` se a lista cresce demais (a lista de entrada é liberada, como no C).
///
/// Se `p_database` não é nulo, a tabela tem um prefixo de banco ("banco.tabela"): `p_database`
/// é o nome da tabela e `p_table` o do banco. `Name` recebe o nome da tabela e `z_database` o
/// nome do banco, ou `None` se não há banco. Chamar `src_list_append(D, A, B, None)` quer dizer
/// que B é a tabela e o banco não foi dito; `src_list_append(D, A, B, C)` quer dizer que C é a
/// tabela e B o banco. Nunca ocorre C sem B. Os dois nomes são tratados como entre aspas e
/// perdem as aspas ao entrar na lista.
pub fn src_list_append(
    db: &mut Connection,
    parse: &mut Parse,
    p_list: Option<Box<SrcList>>,
    p_table: Option<&Token>,
    p_database: Option<&Token>,
) -> Option<Box<SrcList>> {
    debug_assert!(p_database.is_none() || p_table.is_some()); // Não pode haver C sem B.
    let mut p_list = match p_list {
        None => Box::new(SrcList { a: vec![new_src_item()] }),
        Some(mut l) => {
            let n = l.a.len();
            if !src_list_enlarge(db, parse, &mut l, 1, n) {
                return None;
            }
            l
        }
    };
    let p_database = p_database.filter(|d| !d.z.is_empty());
    let item = p_list.a.last_mut()?;
    if let Some(p_database) = p_database {
        item.z_name = name_from_token(Some(p_database));
        item.z_database = name_from_token(p_table);
    } else {
        item.z_name = name_from_token(p_table);
        item.z_database = None;
    }
    Some(p_list)
}

/// `sqlite3SrcListAssignCursors`: atribui números de cursor do VDBE a todas as tabelas de uma
/// `SrcList`.
pub fn src_list_assign_cursors(parse: &mut Parse, p_list: &mut SrcList) {
    for item in p_list.a.iter_mut() {
        if item.i_cursor >= 0 {
            continue;
        }
        item.i_cursor = parse.n_tab;
        parse.n_tab += 1;
        if let Some(sel) = item.p_select.as_deref_mut() {
            if let Some(src) = sel.p_src.as_deref_mut() {
                src_list_assign_cursors(parse, src);
            }
        }
    }
}

/// `sqlite3SrcListAppendFromTerm`: chamada pelo analisador para acrescentar um termo novo ao fim
/// de uma cláusula FROM em construção. `p` é a parte já construída (`None` se este é o primeiro
/// termo). `p_table` e `p_database` são o nome da tabela e do banco do termo (`p_database` é um
/// token vazio quando falta o qualificador, o caso usual). `p_alias` é o token do alias, se houver.
/// Num subselect `p_subquery` é o SELECT e `p_table` e `p_database` são `None`. `p_on_using` é o
/// conteúdo do ON ou do USING.
///
/// Devolve a nova `SrcList` com o termo acrescentado.
#[allow(clippy::too_many_arguments)]
pub fn src_list_append_from_term(
    db: &mut Connection,
    parse: &mut Parse,
    p: Option<Box<SrcList>>,
    p_table: Option<&Token>,
    p_database: Option<&Token>,
    p_alias: Option<&Token>,
    p_subquery: Option<Box<Select>>,
    p_on_using: Option<OnOrUsing>,
) -> Option<Box<SrcList>> {
    // Num erro, `p_subquery` e `p_on_using` são liberados pelo `Drop` (o `append_from_error` do C).
    if p.is_none() {
        if let Some(ou) = p_on_using.as_ref() {
            if ou.p_on.is_some() || ou.p_using.is_some() {
                let z = if ou.p_on.is_some() { b"ON".as_slice() } else { b"USING".as_slice() };
                error_msg(db, parse, b"a JOIN clause is required before %s", &[text_arg(z)]);
                return None;
            }
        }
    }
    let mut p = src_list_append(db, parse, p, p_table, p_database)?;
    debug_assert!(!p.a.is_empty());
    debug_assert!(p_table.is_some() == p_database.is_some());
    let in_rename = parse.in_rename_object();
    let item = p.a.last_mut()?;
    debug_assert!(item.z_name.is_none() || p_database.is_some());
    if in_rename {
        if let Some(z_name) = &item.z_name {
            // `(ALWAYS(pDatabase) && pDatabase->z) ? pDatabase : pTable`
            let token = match p_database {
                Some(d) if !d.z.is_empty() => Some(d),
                _ => p_table,
            };
            if let Some(token) = token {
                rename_token_map(parse, z_name.as_ptr() as usize, token);
            }
        }
    }
    debug_assert!(p_alias.is_some());
    if let Some(alias) = p_alias {
        if !alias.z.is_empty() {
            item.z_alias = name_from_token(Some(alias));
        }
    }
    if let Some(sub) = p_subquery {
        if (sub.sel_flags & SF_NESTEDFROM) != 0 {
            item.fg.is_nested_from = true;
        }
        item.p_select = Some(sub);
    }
    debug_assert!(!item.fg.is_using);
    match p_on_using {
        None => item.u3 = SrcU3::On(None),
        Some(OnOrUsing { p_on, p_using }) => {
            debug_assert!(p_on.is_none() || p_using.is_none());
            if p_using.is_some() {
                item.fg.is_using = true;
                item.u3 = SrcU3::Using(p_using);
            } else {
                item.u3 = SrcU3::On(p_on);
            }
        }
    }
    Some(p)
}

/// `sqlite3SrcListIndexedBy`: acrescenta um INDEXED BY ou NOT INDEXED ao termo mais recente da
/// lista de origem.
pub fn src_list_indexed_by(
    _db: &mut Connection,
    _parse: &mut Parse,
    p: Option<&mut SrcList>,
    p_indexed_by: &Token,
) {
    let Some(p) = p else {
        return;
    };
    if p_indexed_by.z.is_empty() {
        return;
    }
    debug_assert!(!p.a.is_empty());
    let Some(item) = p.a.last_mut() else {
        return;
    };
    debug_assert!(!item.fg.not_indexed);
    debug_assert!(!item.fg.is_indexed_by);
    debug_assert!(!item.fg.is_tab_func);
    if is_not_indexed_token(p_indexed_by) {
        // Veio uma cláusula "NOT INDEXED" (ver `indexed_opt` em parse.y).
        item.fg.not_indexed = true;
    } else {
        item.u1 = SrcU1::IndexedBy(name_from_token(Some(p_indexed_by)));
        item.fg.is_indexed_by = true;
        debug_assert!(!item.fg.is_cte); // Sem colisão na união u2.
    }
}

/// `sqlite3SrcListAppendList`: acrescenta o conteúdo de `p2` à `SrcList` `p1` e devolve o
/// resultado. As duas listas são consumidas. Se o acréscimo falha (lista grande demais) `p2` é
/// descartada e `p1` volta como estava.
pub fn src_list_append_list(
    db: &mut Connection,
    parse: &mut Parse,
    p1: Option<Box<SrcList>>,
    p2: Option<Box<SrcList>>,
) -> Option<Box<SrcList>> {
    let mut p1 = p1?;
    debug_assert!(p1.a.len() == 1);
    if let Some(mut p2) = p2 {
        let n = p2.a.len();
        if n >= 1 && src_list_enlarge(db, parse, &mut p1, n, 1) {
            for (k, item) in p2.a.drain(..).enumerate() {
                p1.a[1 + k] = item;
            }
            let lt_rj = JT_LTORJ & p1.a[1].fg.jointype;
            p1.a[0].fg.jointype |= lt_rj;
        }
    }
    Some(p1)
}

/// `sqlite3SrcListFuncArgs`: acrescenta a lista de argumentos ao termo da `SrcList` que é uma
/// função com valor de tabela.
pub fn src_list_func_args(
    _db: &mut Connection,
    _parse: &mut Parse,
    p: Option<&mut SrcList>,
    p_list: Option<Box<crate::sqlite_int::ExprList>>,
) {
    // Sem a lista de origem, `p_list` é descartada (o `Drop` faz o `sqlite3ExprListDelete`).
    if let Some(item) = p.and_then(|p| p.a.last_mut()) {
        debug_assert!(!item.fg.not_indexed);
        debug_assert!(!item.fg.is_indexed_by);
        debug_assert!(!item.fg.is_tab_func);
        item.u1 = SrcU1::FuncArg(p_list);
        item.fg.is_tab_func = true;
    }
}

/// `sqlite3SrcListShiftJoinType`: ao montar o FROM no analisador, o operador de junção fica ligado
/// ao operando da esquerda; o gerador de código espera o operador no operando da direita. Esta
/// rotina desloca todos os operadores da esquerda para a direita numa cláusula FROM inteira.
///
/// Exemplo: em `A natural cross join B` o operador é "natural cross join". Os operandos A e B
/// ficam em `p.a[0]` e `p.a[1]`; o analisador guarda o operador com A e esta rotina o passa para B.
///
/// Mudança adicional: todas as tabelas à esquerda do RIGHT JOIN mais à direita ganham `JT_LTORJ`
/// (mnemônico: Left Table Of Right Join), para o gerador de código reconhecer com facilidade que
/// a tabela é parte do operando esquerdo de pelo menos um RIGHT JOIN.
pub fn src_list_shift_join_type(_db: &mut Connection, _parse: &mut Parse, p: Option<&mut SrcList>) {
    let Some(p) = p else {
        return;
    };
    let n = p.a.len();
    if n <= 1 {
        return;
    }
    let mut all_flags: u8 = 0;
    for i in (1..n).rev() {
        let jt = p.a[i - 1].fg.jointype;
        p.a[i].fg.jointype = jt;
        all_flags |= jt;
    }
    p.a[0].fg.jointype = 0;

    // Todos os termos à esquerda de um RIGHT JOIN ganham a flag JT_LTORJ.
    if (all_flags & JT_RIGHT) != 0 {
        let mut i = n - 1;
        while i > 0 && (p.a[i].fg.jointype & JT_RIGHT) == 0 {
            i -= 1;
        }
        for j in (0..i).rev() {
            p.a[j].fg.jointype |= JT_LTORJ;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Transações e savepoints
// ---------------------------------------------------------------------------------------------

/// `sqlite3BeginTransaction`: gera o código do VDBE para um comando BEGIN.
pub fn begin_transaction(db: &mut Connection, parse: &mut Parse, ty: i32) {
    if auth_check(db, parse, SQLITE_TRANSACTION, Some(b"BEGIN"), None, None) != 0 {
        return;
    }
    get_vdbe(db, parse);
    if ty != TK_DEFERRED as i32 {
        for i in 0..db.dbs.len() {
            let e_txn_type = if db.dbs[i].bt.as_ref().map_or(false, btree_is_readonly) {
                0 // Transação de leitura.
            } else if ty == TK_EXCLUSIVE as i32 {
                2 // Transação exclusiva.
            } else {
                1 // Transação de escrita.
            };
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                add_op2(v, OP_TRANSACTION as i32, i as i32, e_txn_type);
                uses_btree(v, i as i32);
            }
        }
    }
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op0(v, OP_AUTOCOMMIT as i32);
    }
}

/// `sqlite3EndTransaction`: gera o código do VDBE para um COMMIT ou ROLLBACK. O código de
/// ROLLBACK sai se `e_type` é `TK_ROLLBACK`; senão sai o de COMMIT.
pub fn end_transaction(db: &mut Connection, parse: &mut Parse, e_type: i32) {
    let is_rollback = e_type == TK_ROLLBACK as i32;
    let z_op: &[u8] = if is_rollback { b"ROLLBACK" } else { b"COMMIT" };
    if auth_check(db, parse, SQLITE_TRANSACTION, Some(z_op), None, None) != 0 {
        return;
    }
    {
        get_vdbe(db, parse);
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            add_op2(v, OP_AUTOCOMMIT as i32, 1, is_rollback as i32);
        }
    }
}

/// `sqlite3Savepoint`: chamada pelo analisador ao ver um comando que cria, libera ou desfaz um
/// savepoint SQL. `op` é `SAVEPOINT_BEGIN`, `SAVEPOINT_RELEASE` ou `SAVEPOINT_ROLLBACK`.
pub fn savepoint(db: &mut Connection, parse: &mut Parse, op: i32, p_name: &Token) {
    const AZ: [&[u8]; 3] = [b"BEGIN", b"RELEASE", b"ROLLBACK"];
    let Some(z_name) = name_from_token(Some(p_name)) else {
        return;
    };
    let have_v = { get_vdbe(db, parse); true };
    if !have_v
        || auth_check(db, parse, SQLITE_SAVEPOINT, Some(AZ[op as usize]), Some(&z_name), None) != 0
    {
        return;
    }
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op4(v, OP_SAVEPOINT as i32, op, 0, 0, P4::Text(z_name));
    }
}

/// `sqlite3OpenTempDatabase`: garante que o banco TEMP está aberto e disponível para uso. Devolve
/// o número de erros; as mensagens ficam no `Parse`.
pub fn open_temp_database(db: &mut Connection, parse: &mut Parse) -> i32 {
    if db.dbs[1].bt.is_none() && parse.explain == 0 {
        let flags = SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE
            | SQLITE_OPEN_TEMP_DB;
        // `sqlite3TempInMemory(db)` com `SQLITE_TEMP_STORE == 1` (o padrão do Debian): quem chama
        // `btree_open` passa `BTREE_MEMORY`.
        let btree_flags = if db.temp_store == 2 { BTREE_MEMORY as i32 } else { 0 };
        let opened = match db.p_vfs.clone() {
            Some(vfs) => btree_open(vfs, None, btree_flags, flags),
            None => Err(SQLITE_CANTOPEN),
        };
        match opened {
            Err(rc) => {
                error_msg(
                    db,
                    parse,
                    b"unable to open a temporary database file for storing temporary tables",
                    &[],
                );
                parse.rc = rc;
                return 1;
            }
            Ok(mut bt) => {
                bt.db_index = 1;
                // O C descarta `SQLITE_NOMEM` aqui para `sqlite3OomFault`, que não existe.
                let _ = btree_set_page_size(&mut bt, db.next_pagesize, 0, 0);
                db.dbs[1].bt = Some(bt);
            }
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------
// Cookie do esquema, operações de escrita
// ---------------------------------------------------------------------------------------------

/// `sqlite3CodeVerifySchemaAtToplevel`: registra que o cookie do esquema do banco `i_db` precisa
/// ser verificado. O código que o verifica sai no fim do VDBE de nível mais alto, gerado depois
/// por `finish_coding`. `p_toplevel` é o `Parse` de nível mais alto.
fn code_verify_schema_at_toplevel(db: &mut Connection, p_toplevel: &mut Parse, i_db: i32) {
    debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
    debug_assert!(db.dbs[i_db as usize].bt.is_some() || i_db == 1);
    debug_assert!(i_db < crate::consts::SQLITE_MAX_DB);
    if !db_mask_test(p_toplevel.cookie_mask, i_db as usize) {
        db_mask_set(&mut p_toplevel.cookie_mask, i_db as usize);
        if OMIT_TEMPDB == 0 && i_db == 1 {
            open_temp_database(db, p_toplevel);
        }
    }
}

/// `sqlite3CodeVerifySchema`.
pub fn code_verify_schema(db: &mut Connection, parse: &mut Parse, i_db: i32) {
    code_verify_schema_at_toplevel(db, parse.toplevel_mut(), i_db);
}

/// `sqlite3CodeVerifyNamedSchema`: se `z_db` é `None`, chama `code_verify_schema` para cada banco
/// anexado; senão só para o banco chamado `z_db`.
pub fn code_verify_named_schema(db: &mut Connection, parse: &mut Parse, z_db: Option<&[u8]>) {
    for i in 0..db.dbs.len() {
        let p_db = &db.dbs[i];
        let hit = p_db.bt.is_some() && z_db.map_or(true, |z| str_icmp(z, &p_db.z_db_s_name) == 0);
        if hit {
            code_verify_schema(db, parse, i as i32);
        }
    }
}

/// `sqlite3BeginWriteOperation`: gera o código do VDBE que prepara uma operação que pode alterar
/// o banco.
///
/// Inicia uma transação se ainda não há uma em curso. Se já há, marca um checkpoint quando
/// `set_statement` é verdadeiro. O checkpoint serve às operações que podem falhar (por uma
/// restrição) no meio do caminho e precisam desfazer parte das escritas sem reverter a transação
/// inteira. Nas operações em que todas as restrições são conferidas antes de qualquer mudança
/// nunca é preciso desfazer uma escrita e o checkpoint não deve ser marcado.
pub fn begin_write_operation(
    db: &mut Connection,
    parse: &mut Parse,
    set_statement: i32,
    i_db: i32,
) {
    let p_toplevel = parse.toplevel_mut();
    code_verify_schema_at_toplevel(db, p_toplevel, i_db);
    db_mask_set(&mut p_toplevel.write_mask, i_db as usize);
    p_toplevel.is_multi_write |= set_statement as u8;
}

/// `sqlite3MultiWrite`: indica que o comando em construção pode escrever mais de uma entrada
/// (por exemplo apagar uma linha e inserir outra, inserir várias linhas numa tabela, ou inserir
/// uma linha e as entradas dos índices). Se houver um aborto depois de algumas escritas feitas,
/// será preciso desfazê-las.
pub fn multi_write(parse: &mut Parse) {
    parse.toplevel_mut().is_multi_write = 1;
}

/// `sqlite3MayAbort`: o gerador de código a chama ao descobrir que é possível abortar o comando
/// antes de ele terminar. Para abortar sem corromper o banco, o comando precisa estar protegido
/// por uma transação de comando.
///
/// Tecnicamente só seria preciso ligar `may_abort` se `is_multi_write` já estivesse ligada: há uma
/// dependência de tempo, o aborto tem de vir depois da escrita múltipla. Isso faria alguns
/// comandos com REPLACE ficarem um pouco mais rápidos, mas dificulta provar que o código está
/// certo, então o caminho seguro foi mantido e a otimização não se faz.
pub fn may_abort(parse: &mut Parse) {
    parse.toplevel_mut().may_abort = 1;
}

/// `sqlite3HaltConstraint`: gera um `OP_Halt` que faz o VDBE devolver um erro `SQLITE_CONSTRAINT`.
/// `on_error` decide quais (se algum) do comando e da transação em curso são revertidos.
///
/// `p4_type` (`P4_STATIC` ou `P4_TRANSIENT` no C, `P4_DYNAMIC` quando a mensagem é nova) só decide
/// quem libera a mensagem; aqui o `P4::Text` sempre possui uma cópia, então o valor não muda nada.
pub fn halt_constraint(
    parse: &mut Parse,
    err_code: i32,
    on_error: i32,
    p4: Option<&[u8]>,
    _p4_type: i8,
    p5_errmsg: u16,
) {
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!((err_code & 0xff) == crate::consts::SQLITE_CONSTRAINT || parse.nested != 0);
    if on_error == crate::consts::OE_ABORT as i32 {
        may_abort(parse);
    }
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op4(v, OP_HALT as i32, err_code, on_error, 0, p4.map_or(P4::None, |z| P4::Text(z.to_vec())));
        change_p5(v, p5_errmsg);
    }
}

/// `sqlite3UniqueConstraint`: gera um `OP_Halt` por violação de restrição UNIQUE ou PRIMARY KEY.
/// `p_tab` é a tabela dona do índice `p_idx` (o `pIdx->pTable` do C).
pub fn unique_constraint(
    db: &mut Connection,
    parse: &mut Parse,
    on_error: i32,
    p_tab: &Table,
    p_idx: &Index,
) {
    let mut err_msg = StrAccum::new(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
    if p_idx.a_col_expr.is_some() {
        err_msg.appendf(b"index '%q'", &[text_arg(&p_idx.z_name)]);
    } else {
        for j in 0..p_idx.n_key_col as usize {
            debug_assert!(p_idx.ai_column[j] >= 0);
            let p_col = &p_tab.a_col[p_idx.ai_column[j] as usize];
            if j > 0 {
                err_msg.append(b", ");
            }
            err_msg.append_all(&p_tab.z_name);
            err_msg.append(b".");
            err_msg.append_all(&p_col.z_cn_name);
        }
    }
    let z_err = err_msg.finish();
    let code = if p_idx.is_primary_key_index() {
        SQLITE_CONSTRAINT_PRIMARYKEY
    } else {
        SQLITE_CONSTRAINT_UNIQUE
    };
    halt_constraint(parse, code, on_error, z_err.as_deref(), P4_STATIC, P5_CONSTRAINTUNIQUE);
}

/// `sqlite3RowidConstraint`: gera um `OP_Halt` por rowid duplicado em `p_tab`.
pub fn rowid_constraint(parse: &mut Parse, on_error: i32, p_tab: &Table) {
    let (z_msg, rc) = if p_tab.i_p_key >= 0 {
        let p_col = &p_tab.a_col[p_tab.i_p_key as usize];
        (
            crate::printf::mprintf(b"%s.%s", &[text_arg(&p_tab.z_name), text_arg(&p_col.z_cn_name)]),
            SQLITE_CONSTRAINT_PRIMARYKEY,
        )
    } else {
        (
            crate::printf::mprintf(b"%s.rowid", &[text_arg(&p_tab.z_name)]),
            SQLITE_CONSTRAINT_ROWID,
        )
    };
    halt_constraint(parse, rc, on_error, z_msg.as_deref(), P4_STATIC, P5_CONSTRAINTUNIQUE);
}

// ---------------------------------------------------------------------------------------------
// REINDEX
// ---------------------------------------------------------------------------------------------

/// `collationMatch`: verdadeiro se `p_index` usa a sequência de colação `z_coll`.
fn collation_match(z_coll: &[u8], p_index: &Index) -> bool {
    for i in 0..p_index.n_column as usize {
        let z = &p_index.az_coll[i];
        debug_assert!(!z.is_empty() || p_index.ai_column[i] < 0);
        if p_index.ai_column[i] >= 0 && str_icmp(z, z_coll) == 0 {
            return true;
        }
    }
    false
}

/// `reindexTable`: recalcula todos os índices de `p_tab` que usam a colação `z_coll`. Se `z_coll`
/// é `None`, recalcula todos os índices da tabela.
fn reindex_table(db: &mut Connection, parse: &mut Parse, p_tab: &Table, z_coll: Option<&[u8]>) {
    if p_tab.is_virtual() {
        return;
    }
    for p_index in p_tab.p_index.iter() {
        if z_coll.map_or(true, |z| collation_match(z, p_index)) {
            let i_db = schema_to_index(db, p_tab.p_schema);
            begin_write_operation(db, parse, 0, i_db);
            refill_index(db, parse, p_tab, p_index, i_db, -1);
        }
    }
}

/// `reindexDatabases`: recalcula todos os índices de todas as tabelas de todos os bancos que usam
/// a colação `z_coll`. Se `z_coll` é `None`, recalcula todos os índices de todos os bancos.
fn reindex_databases(db: &mut Connection, parse: &mut Parse, z_coll: Option<&[u8]>) {
    for i_db in 0..db.dbs.len() {
        // As tabelas são copiadas (por `Rc`) porque `reindex_table` precisa da conexão inteira.
        let tabs: Vec<Rc<Table>> =
            hash_iter(&db.dbs[i_db].schema.tbl_hash).map(|(_, t)| Rc::clone(t)).collect();
        for p_tab in tabs {
            reindex_table(db, parse, &p_tab, z_coll);
        }
    }
}

/// `sqlite3Reindex`: gera o código do comando REINDEX.
///
/// ```text
///        REINDEX                            -- 1
///        REINDEX  <collation>               -- 2
///        REINDEX  ?<database>.?<tablename>  -- 3
///        REINDEX  ?<database>.?<indexname>  -- 4
/// ```
///
/// A forma 1 reconstrói todos os índices de todos os bancos anexados. A 2 reconstrói todos os
/// índices de todos os bancos que usam a colação chamada. As formas 3 e 4 reconstroem o índice
/// chamado ou todos os índices da tabela chamada.
pub fn reindex(
    db: &mut Connection,
    parse: &mut Parse,
    p_name1: Option<&Token>,
    p_name2: Option<&Token>,
) {
    // Lê o esquema do banco. Se houver erro, a mensagem e o código ficam no `Parse`.
    if read_schema(db, parse) != SQLITE_OK {
        return;
    }
    let Some(p_name1) = p_name1 else {
        reindex_databases(db, parse, None);
        return;
    };
    let name2 = p_name2.cloned().unwrap_or_default(); // `NEVER(pName2==0)`
    if name2.z.is_empty() {
        debug_assert!(!p_name1.z.is_empty());
        let Some(z_coll) = name_from_token(Some(p_name1)) else {
            return;
        };
        let enc = db.enc;
        if find_coll_seq(db, enc, Some(&z_coll), 0).is_some() {
            reindex_databases(db, parse, Some(&z_coll));
            return;
        }
    }
    let Some((i_db, p_obj_name)) = two_part_name(db, parse, p_name1, &name2) else {
        return;
    };
    let Some(z) = name_from_token(Some(p_obj_name)) else {
        return;
    };
    let z_db: Option<Vec<u8>> = if !name2.z.is_empty() {
        Some(db.dbs[i_db as usize].z_db_s_name.clone())
    } else {
        None
    };
    if let Some(p_tab) = find_table(db, &z, z_db.as_deref()) {
        reindex_table(db, parse, &p_tab, None);
        return;
    }
    if let Some((p_tab, p_index)) = find_index(db, &z, z_db.as_deref()) {
        let i_db = schema_to_index(db, p_tab.p_schema);
        begin_write_operation(db, parse, 0, i_db);
        refill_index(db, parse, &p_tab, &p_index, i_db, -1);
        return;
    }
    error_msg(db, parse, b"unable to identify the object to be reindexed", &[]);
}

// ---------------------------------------------------------------------------------------------
// KeyInfo de um índice
// ---------------------------------------------------------------------------------------------

/// Desativa o índice `p_idx` no esquema (`pIdx->bNoQuery = 1` do C). O `Index` do esquema é um
/// `Rc` imutável e quem chama só tem uma referência, então o índice é achado pelo nome, pela
/// página raiz e pelas colunas e alterado por `Rc::make_mut`.
fn deactivate_index(db: &mut Connection, p_idx: &Index) {
    for i_db in 0..db.dbs.len() {
        let schema = &mut db.dbs[i_db].schema;
        let Some(z_tab) = hash_find(&schema.idx_hash, &p_idx.z_name).cloned() else {
            continue;
        };
        let Some(p_tab) = hash_find_mut(&mut schema.tbl_hash, &z_tab) else {
            continue;
        };
        let found = p_tab.p_index.iter().position(|x| {
            str_icmp(&x.z_name, &p_idx.z_name) == 0
                && x.tnum == p_idx.tnum
                && x.ai_column == p_idx.ai_column
                && x.az_coll == p_idx.az_coll
        });
        if let Some(pos) = found {
            Rc::make_mut(&mut Rc::make_mut(p_tab).p_index[pos]).b_no_query = true;
            return;
        }
    }
}

/// `sqlite3KeyInfoOfIndex`: devolve um `KeyInfo` apropriado ao índice dado, ou `None` se há erro
/// no `Parse` (colação desconhecida, por exemplo). O `sqlite3KeyInfoUnref` do chamador é o `Drop`
/// do `Rc`.
pub fn key_info_of_index(
    parse: &mut Parse,
    db: &mut Connection,
    p_idx: &Index,
) -> Option<Rc<KeyInfo>> {
    let n_col = p_idx.n_column as i32;
    let n_key = p_idx.n_key_col as i32;
    if parse.n_err != 0 {
        return None;
    }
    let mut p_key = if p_idx.uniq_not_null {
        key_info_alloc(db, n_key, n_col - n_key)
    } else {
        key_info_alloc(db, n_col, 0)
    };
    for i in 0..n_col as usize {
        let z_coll = &p_idx.az_coll[i];
        // `zColl==sqlite3StrBINARY`: no C é uma comparação de ponteiros com a constante. Um
        // COLLATE BINARY escrito pelo usuário vira a colação BINARY, que se comporta (e se
        // exibe no EXPLAIN) igual à ausência de colação.
        p_key.a_coll[i] = if z_coll.as_slice() == STR_BINARY.as_bytes() {
            None
        } else {
            locate_coll_seq(db, parse, z_coll)
        };
        p_key.a_sort_flags[i] = p_idx.a_sort_order.get(i).copied().unwrap_or(0);
        debug_assert!((p_key.a_sort_flags[i] & crate::consts::KEYINFO_ORDER_BIGNULL) == 0);
    }
    if parse.n_err != 0 {
        debug_assert!(parse.rc == SQLITE_ERROR_MISSING_COLLSEQ);
        if !p_idx.b_no_query {
            // Desativa o índice porque ele contém uma colação desconhecida. A única forma de
            // reativá-lo é recarregar o esquema: acrescentar a colação que faltava depois não o
            // reativa. A aplicação teve a chance de registrar a colação no callback
            // collation-needed, e o SQLite, por simplicidade, não dá uma segunda chance.
            deactivate_index(db, p_idx);
            parse.rc = SQLITE_ERROR_RETRY;
        }
        return None;
    }
    Some(Rc::new(p_key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::OE_ABORT;

    fn index(n_key_col: u16, on_error: u8) -> Index {
        Index {
            n_key_col,
            n_column: n_key_col + 1,
            on_error,
            ai_row_log_est: vec![0; n_key_col as usize + 1],
            ..Index::default()
        }
    }

    #[test]
    fn default_row_est_matches_sqlite() {
        // Sem UNIQUE: 99 (mínimo de 1000 linhas), depois 33 32 30 28 26 e 23 daí em diante.
        let mut idx = index(7, 0);
        let mut n_row: LogEst = 50;
        default_row_est(&mut idx, &mut n_row);
        assert_eq!(n_row, 99);
        assert_eq!(idx.ai_row_log_est, vec![99, 33, 32, 30, 28, 26, 23, 23]);

        // UNIQUE zera a última entrada; índice com poucas colunas só copia o que existe.
        let mut idx = index(2, OE_ABORT as u8);
        let mut n_row: LogEst = 200;
        default_row_est(&mut idx, &mut n_row);
        assert_eq!(n_row, 200);
        assert_eq!(idx.ai_row_log_est, vec![200, 33, 0]);

        // Índice parcial: dez a menos na primeira entrada.
        let mut idx = index(1, 0);
        idx.p_partial_idx_where = Some(Box::default());
        let mut n_row: LogEst = 99;
        default_row_est(&mut idx, &mut n_row);
        assert_eq!(idx.ai_row_log_est, vec![89, 33]);
    }

    #[test]
    fn id_list_index_ignores_case() {
        let l = IdList {
            e_u4: 0,
            a: vec![
                IdListItem { z_name: Some(b"abc".to_vec()), idx: 0 },
                IdListItem { z_name: Some(b"Def".to_vec()), idx: 0 },
            ],
        };
        assert_eq!(id_list_index(&l, b"ABC"), 0);
        assert_eq!(id_list_index(&l, b"dEF"), 1);
        assert_eq!(id_list_index(&l, b"x"), -1);
    }

    #[test]
    fn collation_match_skips_rowid_columns() {
        let mut idx = index(2, 0);
        idx.ai_column = vec![0, 1, -1];
        idx.az_coll = vec![b"BINARY".to_vec(), b"NOCASE".to_vec(), b"NOCASE".to_vec()];
        assert!(collation_match(b"nocase", &idx));
        assert!(collation_match(b"binary", &idx));
        assert!(!collation_match(b"rtrim", &idx));
        idx.ai_column = vec![0, -1, -1];
        assert!(!collation_match(b"nocase", &idx));
    }

    #[test]
    fn src_item_arg_picks_anonymous_kind() {
        let mut item = new_src_item();
        item.p_select = Some(Box::new(Select { sel_flags: SF_NESTEDFROM, sel_id: 7, ..Select::default() }));
        match src_item_arg(&item) {
            PrintfArg::SrcItem(s) => {
                assert!(matches!(s.anon, PrintfSrcAnon::NestedFrom { sel_id: 7 }));
            }
            _ => panic!("esperava %S"),
        }
        assert_eq!(new_src_item().i_cursor, -1);
    }
}
