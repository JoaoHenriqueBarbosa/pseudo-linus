//! `pragma.c`, segunda metade: os pragmas longos (`table_info`, `table_list`, `index_info`,
//! `index_list`, `function_list`, `foreign_key_list`, `foreign_key_check`, `integrity_check`,
//! `optimize`, `temp_store_directory`, os limites de heap e `wal_autocheckpoint`) e a tabela
//! virtual epônima `pragma_xxx` (`PragmaVtab`, `PragmaVtabCursor`, `sqlite3PragmaVtabRegister`).
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - o laço sobre `sqliteHashFirst` das tabelas é um laço sobre uma cópia do vetor de `Rc<Table>`
//!   (o esquema é imutável e a iteração pode disparar `prepare`, que o reconstrói);
//! - `PragmaVtab` e `PragmaVtabCursor` implementam `Vtab` e `VtabCursor`; o `pAux` do módulo é o
//!   índice do pragma em `PRAGMA_NAMES`;
//! - `sqlite3_temp_directory` mora em `os_unix`, que só expõe o gravador: este módulo guarda uma
//!   cópia do valor que o PRAGMA gravou, para o modo de consulta;
//! - o limite de memória (`sqlite3_soft_heap_limit64` e `sqlite3_hard_heap_limit64`) é estado
//!   deste módulo: `malloc.c` ainda não foi portado, e com `DEFAULT_MEMSTATUS=0` o uso de memória
//!   é sempre zero, então baixar o limite não libera nada;
//! - o valor corrente de `wal_autocheckpoint` (o N que o gancho padrão guarda em `pWalArg`) não é
//!   legível no `WalHook`; o PRAGMA guarda o último N que ele mesmo gravou, por conexão;
//! - `azColl[kk]==sqlite3StrBINARY` (igualdade de ponteiro no C) é a comparação do texto com
//!   "BINARY";
//! - `sqlite3VdbeVerifyNoMallocRequired`, `VdbeCoverage` e as asserções de depuração somem.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::build::{
    column_expr, find_index, find_table, locate_table, preferred_table_name, primary_key_index,
    table_column_to_index, table_column_to_storage, table_lock, text_arg,
};
use crate::build2::view_get_column_names;
use crate::build3::{begin_write_operation, code_verify_named_schema, code_verify_schema};
use crate::callback::builtin_functions_in_order;
use crate::connection::{
    Connection, Context, FuncDef, IndexInfo, Module, ModuleCaps, Parse, Vtab, VtabCursor, VtabModule,
};
use crate::consts::{
    COLFLAG_HIDDEN, COLFLAG_NOINSERT, COLFLAG_PRIMKEY, COLFLAG_STORED, COLFLAG_VIRTUAL,
    COLTYPE_ANY, DBFLAG_INTERNAL_FUNC, LOCATE_NOERR, OE_CASCADE, OE_RESTRICT, OE_SET_DFLT,
    OE_SET_NULL, OMIT_TEMPDB, OP_ADDIMM, OP_AFFINITY, OP_COLUMN, OP_CONCAT, OP_EQ, OP_EXPIRE,
    OP_FOUND, OP_GOTO, OP_HALT, OP_IDXGT, OP_IDXROWID, OP_IFNOTZERO, OP_IFPOS, OP_IFSIZEBETWEEN,
    OP_INTEGER, OP_INTEGRITYCK, OP_ISNULL, OP_ISTYPE, OP_NE, OP_NEXT, OP_NOTNULL, OP_NULL,
    OP_OPENREAD, OP_RESULTROW, OP_REWIND, OP_ROWID, OP_SEEKROWID, OP_SQLEXEC, OP_STRING8,
    OP_VCHECK, PRAG_FLG_RESULT0, PRAG_FLG_RESULT1, PRAG_FLG_SCHEMA_OPT, PRAG_FLG_SCHEMA_REQ,
    SQLITE_ACCESS_READWRITE, SQLITE_AFF_BLOB, SQLITE_AFF_NUMERIC, SQLITE_AFF_TEXT,
    SQLITE_CONSTRAINT, SQLITE_CORRUPT, SQLITE_DETERMINISTIC, SQLITE_DIRECTONLY, SQLITE_ERROR,
    SQLITE_FUNC_ENCMASK, SQLITE_FUNC_INTERNAL, SQLITE_IGNORE_CHECKS, SQLITE_INDEX_CONSTRAINT_EQ,
    SQLITE_INNOCUOUS, SQLITE_JUMPIFNULL, SQLITE_LIMIT_SQL_LENGTH, SQLITE_NOMEM, SQLITE_NULL,
    SQLITE_OK, SQLITE_ROW, SQLITE_SUBTYPE, TF_MAYBE_REANALYZE, TF_SHADOW, TF_STRICT,
    TF_WITHOUT_ROWID, XN_ROWID,
};
use crate::ctype::to_lower;
use crate::delete::{generate_index_key, resolve_part_idx_label};
use crate::expr::expr_list_dup;
use crate::expr_code::{expr_code_get_column_of_table, expr_code_load_index_column};
use crate::expr_code2::{
    clear_temp_reg_cache, expr_if_false, expr_if_true, get_temp_range, get_temp_reg,
    release_temp_range, touch_register,
};
use crate::fkey::fk_locate_index;
use crate::hash::{hash_count, hash_find, hash_iter};
use crate::insert::{index_affinity_str, open_table};
use crate::insert2::open_table_and_indices;
use crate::main::{db_printf, errmsg};
use crate::mem::{value_type, Mem, StrDtor};
use crate::mem2::value_from_expr;
use crate::os_unix::set_temp_directory;
use crate::pager::cstr;
use crate::pragma::{
    invalidate_temp_storage, ol, pragma_locate, return_single_int, return_single_text, PragmaCtx,
    PragmaName, PRAGMA_NAMES, PRAG_C_NAME,
};
use crate::prepare::{prepare, prepare_v2, schema_to_index};
use crate::printf::{PrintfArg, StrAccum};
use crate::sqlite_int::{Column, ExprU, Index, Table};
use crate::update::column_default;
use crate::util::{at, atoi, dec_or_hex_to_i64, err_str, error_msg, get_int32, str_icmp, STD_TYPE};
use crate::vdbe_types::{Vdbe, VdbeOpList, P4};
use crate::vdbeapi::{column_value, finalize, result_text, result_value, step, value_text};
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, add_op_list, append_p4, change_p3,
    change_p5, current_addr, jump_here, load_string, make_label, multi_load, resolve_label,
    set_p4_key_info, typeof_column, vdbe_goto, vdbe_of_parse,
};
use crate::vtab::{declare_vtab, get_vtable, vtab_create_module};

/// `SQLITE_DEFAULT_OPTIMIZE_LIMIT`: o limite de análise de `PRAGMA optimize` com a máscara 0x10.
const SQLITE_DEFAULT_OPTIMIZE_LIMIT: i32 = 2000;

/// `SQLITE_INTEGRITY_CHECK_ERROR_MAX`: o máximo padrão de erros do `integrity_check`.
const SQLITE_INTEGRITY_CHECK_ERROR_MAX: i32 = 100;

/// O nome da coluna `z_cn_name` (até o primeiro NUL), como texto próprio.
fn col_name(p_col: &Column) -> Vec<u8> {
    cstr(&p_col.z_cn_name).to_vec()
}

/// Uma cópia das tabelas do esquema `i_db`, na ordem de iteração da tabela hash.
fn schema_tables(db: &Connection, i_db: usize) -> Vec<Rc<Table>> {
    hash_iter(&db.dbs[i_db].schema.tbl_hash).map(|(_, t)| Rc::clone(t)).collect()
}

// ---------------------------------------------------------------------------------------------
// temp_store_directory
// ---------------------------------------------------------------------------------------------

/// O valor que o PRAGMA gravou em `sqlite3_temp_directory` (ver o cabeçalho do módulo).
static TEMP_DIRECTORY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// `PRAGMA temp_store_directory` e `PRAGMA temp_store_directory = ""|"directory_name"`: informa
/// ou muda o diretório de arquivos temporários. Uma cadeia vazia volta à busca padrão. Se o
/// diretório muda, o armazenamento temporário é invalidado.
pub(crate) fn pragma_temp_store_directory(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let Some(zr) = c.z_right else {
        let cur = TEMP_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner).clone();
        return_single_text(vdbe_of_parse(parse), cur.as_deref());
        return;
    };
    if !zr.is_empty() {
        let mut res = 0;
        let rc = match db.p_vfs.as_ref() {
            Some(vfs) => vfs.access(zr, SQLITE_ACCESS_READWRITE, &mut res),
            None => SQLITE_ERROR,
        };
        if rc != SQLITE_OK || res == 0 {
            error_msg(db, parse, b"not a writable directory", &[]);
            return;
        }
    }
    // `SQLITE_TEMP_STORE==1`.
    if db.temp_store <= 1 {
        invalidate_temp_storage(db, parse);
    }
    let new_dir = if zr.is_empty() { None } else { Some(zr.to_vec()) };
    *TEMP_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner) = new_dir.clone();
    set_temp_directory(new_dir);
}

// ---------------------------------------------------------------------------------------------
// Limites de heap, threads e wal_autocheckpoint
// ---------------------------------------------------------------------------------------------

/// `mem0.alarmThreshold`: o limite brando.
static SOFT_HEAP_LIMIT: AtomicI64 = AtomicI64::new(0);

/// `mem0.hardLimit`: o limite duro.
static HARD_HEAP_LIMIT: AtomicI64 = AtomicI64::new(0);

/// `sqlite3_soft_heap_limit64`: muda o limite brando (se `n` não é negativo) e devolve o anterior.
/// Um limite duro ativo manda: `n` maior que ele, ou zero, vira o limite duro.
fn soft_heap_limit64(n: i64) -> i64 {
    let prior = SOFT_HEAP_LIMIT.load(Ordering::SeqCst);
    if n < 0 {
        return prior;
    }
    let hard = HARD_HEAP_LIMIT.load(Ordering::SeqCst);
    let n = if hard > 0 && (n > hard || n == 0) { hard } else { n };
    SOFT_HEAP_LIMIT.store(n, Ordering::SeqCst);
    prior
}

/// `sqlite3_hard_heap_limit64`: muda o limite duro (se `n` não é negativo) e devolve o anterior.
/// O limite brando desce junto, se for maior ou zero.
fn hard_heap_limit64(n: i64) -> i64 {
    let prior = HARD_HEAP_LIMIT.load(Ordering::SeqCst);
    if n >= 0 {
        HARD_HEAP_LIMIT.store(n, Ordering::SeqCst);
        let soft = SOFT_HEAP_LIMIT.load(Ordering::SeqCst);
        if n < soft || soft == 0 {
            SOFT_HEAP_LIMIT.store(n, Ordering::SeqCst);
        }
    }
    prior
}

/// `PRAGMA soft_heap_limit[=N]`.
///
/// IMPLEMENTATION-OF: R-26343-45930 This pragma invokes the sqlite3_soft_heap_limit64()
/// interface with the argument N, if N is specified and is a non-negative integer.
/// IMPLEMENTATION-OF: R-64451-07163 The soft_heap_limit pragma always returns the same integer
/// that would be returned by the sqlite3_soft_heap_limit64(-1) C-language function.
pub(crate) fn pragma_soft_heap_limit(parse: &mut Parse, c: &PragmaCtx) {
    let mut n: i64 = 0;
    if let Some(zr) = c.z_right {
        if dec_or_hex_to_i64(zr, &mut n) == SQLITE_OK {
            soft_heap_limit64(n);
        }
    }
    return_single_int(vdbe_of_parse(parse), soft_heap_limit64(-1));
}

/// `PRAGMA hard_heap_limit[=N]`: consulta ou muda o limite duro. O pragma só pode ativá-lo ou
/// baixá-lo, nunca subi-lo nem desativá-lo; só a API em C pode. Assim uma aplicação fixa um
/// limite que um SQL não confiável não afrouxa.
pub(crate) fn pragma_hard_heap_limit(parse: &mut Parse, c: &PragmaCtx) {
    let mut n: i64 = 0;
    if let Some(zr) = c.z_right {
        if dec_or_hex_to_i64(zr, &mut n) == SQLITE_OK {
            let prior = hard_heap_limit64(-1);
            if n > 0 && (prior == 0 || prior > n) {
                hard_heap_limit64(n);
            }
        }
    }
    return_single_int(vdbe_of_parse(parse), hard_heap_limit64(-1));
}

thread_local! {
    /// O último N gravado por `PRAGMA wal_autocheckpoint`, por conexão (a chave é o id do
    /// esquema principal).
    static WAL_AUTOCHECKPOINT: RefCell<HashMap<u32, i32>> = RefCell::new(HashMap::new());
}

/// Guarda o N que o PRAGMA acabou de gravar para a conexão.
pub(crate) fn wal_autocheckpoint_remember(db: &Connection, n: i32) {
    WAL_AUTOCHECKPOINT.with(|m| {
        m.borrow_mut().insert(db.dbs[0].schema.id.0, n);
    });
}

/// O N do gancho padrão de `wal_autocheckpoint`: zero se a conexão não tem gancho, senão o último
/// valor gravado pelo PRAGMA (1000 enquanto nenhum foi gravado).
pub(crate) fn wal_autocheckpoint_value(db: &Connection) -> i32 {
    if db.x_wal_callback.is_none() {
        return 0;
    }
    WAL_AUTOCHECKPOINT.with(|m| {
        m.borrow().get(&db.dbs[0].schema.id.0).copied().unwrap_or(crate::consts::SQLITE_DEFAULT_WAL_AUTOCHECKPOINT)
    })
}

// ---------------------------------------------------------------------------------------------
// Pragmas de esquema
// ---------------------------------------------------------------------------------------------

/// `PRAGMA table_info(<table>)` e `PRAGMA table_xinfo(<table>)`: uma linha por coluna da tabela:
/// cid (número da coluna, da esquerda para a direita a partir de 0), name, type (tipo declarado),
/// notnull (verdadeiro se há NOT NULL), dflt_value (o padrão, se há) e pk (diferente de zero nas
/// colunas da chave primária). O xinfo acrescenta `hidden`.
pub(crate) fn pragma_table_info(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let Some(zr) = c.z_right else {
        return;
    };
    code_verify_named_schema(db, parse, c.z_db);
    let Some(mut p_tab) = locate_table(db, parse, LOCATE_NOERR, zr, c.z_db) else {
        return;
    };
    let mut n_hidden = 0;
    let p_pk = primary_key_index(&p_tab).cloned();
    parse.n_mem = 7;
    view_get_column_names(db, parse, &mut p_tab);
    for i in 0..p_tab.n_col.max(0) as usize {
        let p_col = &p_tab.a_col[i];
        let mut is_hidden = 0;
        if (p_col.col_flags & COLFLAG_NOINSERT) != 0 {
            if c.pragma.i_arg == 0 {
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
        let k: i64 = if (p_col.col_flags & COLFLAG_PRIMKEY) == 0 {
            0
        } else if let Some(pk) = &p_pk {
            let mut k = 1usize;
            while k <= p_tab.n_col as usize && pk.ai_column.get(k - 1).map_or(true, |&a| a != i as i16) {
                k += 1;
            }
            k as i64
        } else {
            1
        };
        let dflt = match column_expr(&p_tab, p_col) {
            Some(e) if is_hidden < 2 => match &e.u {
                ExprU::Token(z) => z.clone(),
                ExprU::IValue(_) => None,
            },
            _ => None,
        };
        let z_type = crate::util::column_type(p_col, b"").to_vec();
        let args = [
            PrintfArg::Int(i as i64 - n_hidden),
            text_arg(&col_name(p_col)),
            text_arg(&z_type),
            PrintfArg::Int((p_col.not_null != 0) as i64),
            PrintfArg::Text(dflt),
            PrintfArg::Int(k),
            PrintfArg::Int(is_hidden),
        ];
        let z_types: &[u8] = if c.pragma.i_arg != 0 { b"issisii" } else { b"issisi" };
        multi_load(vdbe_of_parse(parse), 1, z_types, &args);
    }
}

/// `PRAGMA table_list`: uma linha por tabela, tabela virtual ou view do esquema inteiro: schema
/// (banco anexado), name, type ("table", "view", "virtual" ou "shadow"), ncol, wr (WITHOUT ROWID)
/// e strict.
pub(crate) fn pragma_table_list(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    parse.n_mem = 6;
    code_verify_named_schema(db, parse, c.z_db);
    for ii in 0..db.dbs.len() {
        if let Some(z_db) = c.z_db {
            if str_icmp(z_db, &db.dbs[ii].z_db_s_name) != 0 {
                continue;
            }
        }
        // Garante que `Table.n_col` está inicializado em todas as views e tabelas virtuais. Cada
        // inicialização pode perturbar a tabela hash, então a varredura recomeça.
        let mut init_n_col = hash_count(&db.dbs[ii].schema.tbl_hash) as i64;
        while init_n_col > 0 {
            init_n_col -= 1;
            let empty = hash_iter(&db.dbs[ii].schema.tbl_hash)
                .find(|(_, t)| t.n_col == 0)
                .map(|(_, t)| t.z_name.clone());
            match empty {
                None => init_n_col = 0,
                Some(name) => {
                    if let Some(z_sql) = db_printf(db, b"SELECT*FROM\"%w\"", &[text_arg(&name)]) {
                        let (_, p_dummy, _) = prepare(db, &z_sql, -1, 0, None);
                        if let Some(id) = p_dummy {
                            finalize(db, id);
                        }
                    }
                }
            }
        }

        let db_name = db.dbs[ii].z_db_s_name.clone();
        for p_tab in schema_tables(db, ii) {
            if let Some(zr) = c.z_right {
                if str_icmp(zr, &p_tab.z_name) != 0 {
                    continue;
                }
            }
            let z_type: &[u8] = if p_tab.is_view() {
                b"view"
            } else if p_tab.is_virtual() {
                b"virtual"
            } else if (p_tab.tab_flags & TF_SHADOW) != 0 {
                b"shadow"
            } else {
                b"table"
            };
            multi_load(
                vdbe_of_parse(parse),
                1,
                b"sssiii",
                &[
                    text_arg(&db_name),
                    text_arg(preferred_table_name(&p_tab.z_name)),
                    text_arg(z_type),
                    PrintfArg::Int(p_tab.n_col as i64),
                    PrintfArg::Int(((p_tab.tab_flags & TF_WITHOUT_ROWID) != 0) as i64),
                    PrintfArg::Int(((p_tab.tab_flags & TF_STRICT) != 0) as i64),
                ],
            );
        }
    }
}

/// `PRAGMA index_info(<index>)` e `PRAGMA index_xinfo(<index>)`: uma linha por coluna do índice.
/// Se não há índice com o nome mas há uma tabela WITHOUT ROWID com ele, mostra a estrutura do
/// índice da PRIMARY KEY dela.
pub(crate) fn pragma_index_info(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let Some(zr) = c.z_right else {
        return;
    };
    let mut found = find_index(db, zr, c.z_db);
    if found.is_none() {
        // Se não há índice chamado `zr`, vê se há uma tabela WITHOUT ROWID com esse nome e, se
        // há, mostra a estrutura do índice da PRIMARY KEY dela.
        if let Some(p_tab) = locate_table(db, parse, LOCATE_NOERR, zr, c.z_db) {
            if !p_tab.has_rowid() {
                if let Some(pk) = primary_key_index(&p_tab).cloned() {
                    found = Some((Rc::clone(&p_tab), pk));
                }
            }
        }
    }
    let Some((p_tab, p_idx)) = found else {
        return;
    };
    let i_idx_db = schema_to_index(db, p_tab.p_schema);
    let mx = if c.pragma.i_arg != 0 {
        // PRAGMA index_xinfo (a versão nova, com mais linhas e colunas)
        parse.n_mem = 6;
        p_idx.n_column as usize
    } else {
        // PRAGMA index_info (a versão legada)
        parse.n_mem = 3;
        p_idx.n_key_col as usize
    };
    code_verify_schema(db, parse, i_idx_db);
    for i in 0..mx {
        let cnum = p_idx.ai_column[i];
        let z_name = if cnum < 0 { None } else { Some(col_name(&p_tab.a_col[cnum as usize])) };
        multi_load(
            vdbe_of_parse(parse),
            1,
            b"iisX",
            &[PrintfArg::Int(i as i64), PrintfArg::Int(cnum as i64), PrintfArg::Text(z_name)],
        );
        if c.pragma.i_arg != 0 {
            multi_load(
                vdbe_of_parse(parse),
                4,
                b"isiX",
                &[
                    PrintfArg::Int(p_idx.a_sort_order[i] as i64),
                    text_arg(&p_idx.az_coll[i]),
                    PrintfArg::Int((i < p_idx.n_key_col as usize) as i64),
                ],
            );
        }
        let n_mem = parse.n_mem;
        add_op2(vdbe_of_parse(parse), OP_RESULTROW, 1, n_mem);
    }
}

/// `PRAGMA index_list(<table>)`: uma linha por índice da tabela.
pub(crate) fn pragma_index_list(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    const AZ_ORIGIN: [&[u8]; 3] = [b"c", b"u", b"pk"];
    let Some(zr) = c.z_right else {
        return;
    };
    let Some(p_tab) = find_table(db, zr, c.z_db) else {
        return;
    };
    let i_tab_db = schema_to_index(db, p_tab.p_schema);
    parse.n_mem = 5;
    code_verify_schema(db, parse, i_tab_db);
    for (i, p_idx) in p_tab.p_index.iter().enumerate() {
        let origin = AZ_ORIGIN.get(p_idx.idx_type as usize).copied();
        multi_load(
            vdbe_of_parse(parse),
            1,
            b"isisi",
            &[
                PrintfArg::Int(i as i64),
                text_arg(&p_idx.z_name),
                PrintfArg::Int(p_idx.is_unique_index() as i64),
                PrintfArg::Text(origin.map(<[u8]>::to_vec)),
                PrintfArg::Int(p_idx.p_partial_idx_where.is_some() as i64),
            ],
        );
    }
}

/// `pragmaFunclistLine`: cria zero ou mais linhas do resultado para as funções SQL definidas por
/// `p`.
fn pragma_funclist_line(v: &mut Vdbe, p: &FuncDef, is_builtin: bool, show_intern_funcs: bool) {
    const AZ_ENC: [Option<&[u8]>; 4] =
        [None, Some(b"utf8" as &[u8]), Some(b"utf16le" as &[u8]), Some(b"utf16be" as &[u8])];
    let mask: u32 = if show_intern_funcs {
        0xffff_ffff
    } else {
        (SQLITE_DETERMINISTIC | SQLITE_DIRECTONLY | SQLITE_SUBTYPE | SQLITE_INNOCUOUS) as u32
            | SQLITE_FUNC_INTERNAL
    };
    if p.x_s_func.is_none() {
        return;
    }
    if (p.func_flags & SQLITE_FUNC_INTERNAL) != 0 && !show_intern_funcs {
        return;
    }
    let z_type: &[u8] = if p.x_value.is_some() {
        b"w"
    } else if p.x_finalize.is_some() {
        b"a"
    } else {
        b"s"
    };
    multi_load(
        v,
        1,
        b"sissii",
        &[
            text_arg(&p.z_name),
            PrintfArg::Int(is_builtin as i64),
            text_arg(z_type),
            PrintfArg::Text(AZ_ENC[(p.func_flags & SQLITE_FUNC_ENCMASK) as usize].map(<[u8]>::to_vec)),
            PrintfArg::Int(p.n_arg as i64),
            PrintfArg::Int((((p.func_flags & mask) ^ SQLITE_INNOCUOUS as u32) as i32) as i64),
        ],
    );
}

/// `PRAGMA function_list`: uma linha por função SQL (embutidas primeiro, depois as da conexão).
pub(crate) fn pragma_function_list(db: &mut Connection, parse: &mut Parse) {
    let show_intern_func = (db.m_db_flags & DBFLAG_INTERNAL_FUNC) != 0;
    parse.n_mem = 6;
    let v = vdbe_of_parse(parse);
    for p in builtin_functions_in_order() {
        pragma_funclist_line(v, &p, true, show_intern_func);
    }
    for (_, chain) in hash_iter(&db.a_func) {
        for p in chain {
            pragma_funclist_line(v, p, false, show_intern_func);
        }
    }
}

/// `actionName`: o nome legível de uma ação de resolução de restrição.
fn action_name(action: u8) -> &'static [u8] {
    match action {
        OE_SET_NULL => b"SET NULL",
        OE_SET_DFLT => b"SET DEFAULT",
        OE_CASCADE => b"CASCADE",
        OE_RESTRICT => b"RESTRICT",
        _ => b"NO ACTION",
    }
}

/// `PRAGMA foreign_key_list(<table>)`: uma linha por coluna de cada chave estrangeira da tabela.
pub(crate) fn pragma_foreign_key_list(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    let Some(zr) = c.z_right else {
        return;
    };
    let Some(p_tab) = find_table(db, zr, c.z_db) else {
        return;
    };
    let Some(info) = p_tab.u_tab() else {
        return;
    };
    if info.p_f_key.is_empty() {
        return;
    }
    let i_tab_db = schema_to_index(db, p_tab.p_schema);
    parse.n_mem = 8;
    code_verify_schema(db, parse, i_tab_db);
    for (i, p_fk) in info.p_f_key.iter().enumerate() {
        for (j, col) in p_fk.a_col.iter().enumerate() {
            multi_load(
                vdbe_of_parse(parse),
                1,
                b"iissssss",
                &[
                    PrintfArg::Int(i as i64),
                    PrintfArg::Int(j as i64),
                    text_arg(&p_fk.z_to),
                    text_arg(&col_name(&p_tab.a_col[col.i_from as usize])),
                    PrintfArg::Text(col.z_col.clone()),
                    text_arg(action_name(p_fk.a_action[1])), // ON UPDATE
                    text_arg(action_name(p_fk.a_action[0])), // ON DELETE
                    text_arg(b"NONE"),
                ],
            );
        }
    }
}

/// `PRAGMA foreign_key_check` e `PRAGMA foreign_key_check(<table>)`: uma linha por violação de
/// chave estrangeira (a tabela filha, o rowid, a tabela pai e o número da chave).
pub(crate) fn pragma_foreign_key_check(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    macro_rules! vd {
        () => {
            vdbe_of_parse(parse)
        };
    }
    let mut i_db = c.i_db; // Número do banco da tabela corrente.
    let mut z_db: Option<Vec<u8>> = c.z_db.map(<[u8]>::to_vec);
    let reg_result = parse.n_mem + 1; // Três registradores para uma linha do resultado.
    parse.n_mem += 4;
    parse.n_mem += 1;
    let reg_row = parse.n_mem; // Registradores para uma linha de `p_tab`.
    let mut tabs = schema_tables(db, i_db as usize).into_iter().peekable();
    let mut k = tabs.peek().is_some();
    while k {
        let p_tab: Option<Rc<Table>> = if let Some(zr) = c.z_right {
            k = false;
            locate_table(db, parse, 0, zr, z_db.as_deref())
        } else {
            let t = tabs.next();
            k = tabs.peek().is_some();
            t
        };
        let Some(p_tab) = p_tab else {
            continue;
        };
        let p_fkeys = match p_tab.u_tab() {
            Some(info) if !info.p_f_key.is_empty() => info.p_f_key.clone(),
            _ => continue,
        };
        i_db = schema_to_index(db, p_tab.p_schema);
        z_db = Some(db.dbs[i_db as usize].z_db_s_name.clone());
        code_verify_schema(db, parse, i_db);
        table_lock(db, parse, i_db, p_tab.tnum, false, &p_tab.z_name);
        touch_register(parse, p_tab.n_col as i32 + reg_row);
        open_table(db, parse, 0, i_db, &p_tab, OP_OPENREAD);
        load_string(vd!(), reg_result, &p_tab.z_name);
        let mut i = 1;
        let mut failed = false;
        for p_fk in p_fkeys.iter() {
            let Some(p_parent) = find_table(db, &p_fk.z_to, z_db.as_deref()) else {
                i += 1;
                continue;
            };
            let mut p_idx: Option<Rc<Index>> = None;
            table_lock(db, parse, i_db, p_parent.tnum, false, &p_parent.z_name);
            let x = fk_locate_index(db, parse, &p_parent, p_fk, &mut p_idx, None);
            if x == 0 {
                match &p_idx {
                    None => open_table(db, parse, i, i_db, &p_parent, OP_OPENREAD),
                    Some(idx) => {
                        add_op3(vd!(), OP_OPENREAD, i, idx.tnum as i32, i_db);
                        set_p4_key_info(parse, db, idx);
                    }
                }
            } else {
                k = false;
                failed = true;
                break;
            }
            i += 1;
        }
        debug_assert!(parse.n_err > 0 || !failed);
        if failed {
            break;
        }
        if parse.n_tab < i {
            parse.n_tab = i;
        }
        let addr_top = add_op1(vd!(), OP_REWIND, 0);
        for (idx_fk, p_fk) in p_fkeys.iter().enumerate() {
            let i = idx_fk as i32 + 1;
            let p_parent = find_table(db, &p_fk.z_to, z_db.as_deref());
            let mut p_idx: Option<Rc<Index>> = None;
            let mut ai_cols: Option<Vec<i32>> = None;
            if let Some(pp) = &p_parent {
                fk_locate_index(db, parse, pp, p_fk, &mut p_idx, Some(&mut ai_cols));
            }
            let addr_ok = make_label(parse);

            // Gera o código que lê os valores da chave filha para os registradores
            // reg_row..reg_row+n. Se algum valor é NULL a linha não causa violação: salta direto
            // para `addr_ok`.
            let n_col = p_fk.a_col.len();
            touch_register(parse, reg_row + n_col as i32);
            for j in 0..n_col {
                let i_col = match &ai_cols {
                    Some(a) => a[j],
                    None => p_fk.a_col[j].i_from,
                };
                expr_code_get_column_of_table(db, parse, &p_tab, 0, i_col, reg_row + j as i32);
                add_op2(vd!(), OP_ISNULL, reg_row + j as i32, addr_ok);
            }

            // Gera o código que procura no índice pai uma chave que case. Se acha, salta para
            // `addr_ok`.
            match (&p_idx, &p_parent) {
                (Some(idx), Some(pp)) => {
                    let mut z_aff = index_affinity_str(idx, pp);
                    z_aff.truncate(n_col);
                    let v = vd!();
                    add_op4(v, OP_AFFINITY, reg_row, n_col as i32, 0, P4::Text(z_aff));
                    add_op4_int(v, OP_FOUND, i, addr_ok, reg_row, n_col as i32);
                }
                (None, Some(_)) => {
                    let jmp = current_addr(parse) + 2;
                    let v = vd!();
                    add_op3(v, OP_SEEKROWID, i, jmp, reg_row);
                    vdbe_goto(v, addr_ok);
                    debug_assert!(n_col == 1);
                }
                _ => {}
            }

            // Gera o código que informa a violação a quem chamou.
            let v = vd!();
            if p_tab.has_rowid() {
                add_op2(v, OP_ROWID, 0, reg_result + 1);
            } else {
                add_op2(v, OP_NULL, 0, reg_result + 1);
            }
            multi_load(v, reg_result + 2, b"siX", &[text_arg(&p_fk.z_to), PrintfArg::Int(i as i64 - 1)]);
            add_op2(v, OP_RESULTROW, reg_result, 4);
            resolve_label(parse, db, addr_ok);
        }
        add_op2(vd!(), OP_NEXT, 0, addr_top + 1);
        jump_here(vd!(), addr_top);
    }
}

// ---------------------------------------------------------------------------------------------
// integrity_check e quick_check
// ---------------------------------------------------------------------------------------------

/// `integrityCheckResultRow`: subrotina de `PRAGMA integrity_check`: gera o código que devolve uma
/// linha de uma coluna com a cadeia do registrador 3, decrementa a contagem de linhas do
/// registrador 1 e para quando o máximo de linhas foi emitido.
fn integrity_check_result_row(v: &mut Vdbe) -> i32 {
    add_op2(v, OP_RESULTROW, 3, 1);
    let cur = v.n_op();
    let addr = add_op3(v, OP_IFPOS, 1, cur + 2, 1);
    add_op0(v, OP_HALT);
    addr
}

/// `PRAGMA integrity_check`, `integrity_check(N)`, `quick_check` e `quick_check(N)`: verifica a
/// integridade do banco.
///
/// O `quick_check` é uma versão reduzida, feita para achar a maior parte da corrupção sem o custo
/// de cruzar os índices: é linear no tempo, enquanto o `integrity_check` é O(N log N).
///
/// O máximo padrão é 100 erros; um parâmetro numérico N dá outro. N também pode ser o nome de uma
/// tabela: então só ela é verificada, e a lista livre só se a tabela nomeada é "sqlite_schema" (ou
/// um apelido).
///
/// Todos os esquemas são verificados por padrão; para um só use `PRAGMA schema.integrity_check`.
pub(crate) fn pragma_integrity_check(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    macro_rules! vd {
        () => {
            vdbe_of_parse(parse)
        };
    }
    let is_quick = to_lower(at(c.z_left, 0)) == b'q';

    // Se o comando foi "PRAGMA <db>.integrity_check", `i_db` é o índice de <db> e o VDBE só
    // verifica o banco `i_db`. Senão, com o comando simples "PRAGMA integrity_check" (ou
    // "quick_check"), `i_db` vira -1 e o VDBE verifica todos os bancos anexados.
    let mut i_db = c.i_db;
    debug_assert!(i_db >= 0);
    debug_assert!(i_db == 0 || c.id2_has_z);
    if !c.id2_has_z {
        i_db = -1;
    }

    // Inicializa o programa do VDBE.
    parse.n_mem = 6;

    // Fixa o máximo de erros.
    let mut mx_err = SQLITE_INTEGRITY_CHECK_ERROR_MAX;
    let mut p_obj_tab: Option<Rc<Table>> = None; // Verifica só esta tabela, se não for nula.
    if let Some(zr) = c.z_right {
        let z_value = c.value.map_or(&[][..], |t| t.z.as_slice());
        match get_int32(z_value) {
            Some(n) => {
                mx_err = if n <= 0 { SQLITE_INTEGRITY_CHECK_ERROR_MAX } else { n };
            }
            None => {
                let z_obj_db = if i_db >= 0 { Some(db.dbs[i_db as usize].z_db_s_name.clone()) } else { None };
                p_obj_tab = locate_table(db, parse, 0, zr, z_obj_db.as_deref());
            }
        }
    }
    add_op2(vd!(), OP_INTEGER, mx_err - 1, 1); // reg[1] guarda os erros restantes

    // Verifica a integridade de cada arquivo de banco.
    for i in 0..db.dbs.len() {
        if OMIT_TEMPDB != 0 && i == 1 {
            continue;
        }
        if i_db >= 0 && i as i32 != i_db {
            continue;
        }
        code_verify_schema(db, parse, i as i32);
        parse.ok_const_factor = 0; // tag-20230327-1

        // Verifica a integridade da árvore-b: começa achando os números das páginas raiz de
        // todas as tabelas e índices do banco.
        let all_tbls = schema_tables(db, i);
        let tbls: Vec<&Rc<Table>> = all_tbls
            .iter()
            .filter(|t| p_obj_tab.as_ref().map_or(true, |o| Rc::ptr_eq(o, t)))
            .collect();
        let mut cnt: usize = 0; // Número de entradas de `a_root`.
        for p_tab in &tbls {
            if p_tab.has_rowid() {
                cnt += 1;
            }
            cnt += p_tab.p_index.len();
        }
        if cnt == 0 {
            continue;
        }
        if p_obj_tab.is_some() {
            cnt += 1;
        }
        let mut a_root: Vec<u32> = vec![0; cnt + 1]; // Páginas raiz de todas as árvores-b.
        cnt = 0;
        if p_obj_tab.is_some() {
            cnt += 1;
            a_root[cnt] = 0;
        }
        for p_tab in &tbls {
            if p_tab.has_rowid() {
                cnt += 1;
                a_root[cnt] = p_tab.tnum;
            }
            for p_idx in p_tab.p_index.iter() {
                cnt += 1;
                a_root[cnt] = p_idx.tnum;
            }
        }
        a_root[0] = cnt as u32;

        // Garante que há registradores suficientes.
        touch_register(parse, 8 + cnt as i32);
        clear_temp_reg_cache(parse);

        // Faz as verificações da árvore-b.
        let db_name = db.dbs[i].z_db_s_name.clone();
        let z_in_db =
            db_printf(db, b"*** in database %s ***\n", &[text_arg(&db_name)]).unwrap_or_default();
        let v = vd!();
        add_op4(v, OP_INTEGRITYCK, 1, cnt as i32, 8, P4::IntArray(a_root));
        change_p5(v, i as u16);
        let addr = add_op1(v, OP_ISNULL, 2);
        add_op4(v, OP_STRING8, 0, 3, 0, P4::Text(z_in_db));
        add_op3(v, OP_CONCAT, 2, 3, 3);
        integrity_check_result_row(v);
        jump_here(v, addr);

        // Confere se os índices têm o número certo de linhas.
        cnt = if p_obj_tab.is_some() { 1 } else { 0 };
        load_string(v, 2, b"wrong # of entries in index ");
        for p_tab in &tbls {
            let mut i_tab = cnt;
            if p_tab.has_rowid() {
                cnt += 1;
            } else {
                for p_idx in p_tab.p_index.iter() {
                    if p_idx.is_primary_key_index() {
                        break;
                    }
                    i_tab += 1;
                }
            }
            for p_idx in p_tab.p_index.iter() {
                if p_idx.p_partial_idx_where.is_none() {
                    let addr = add_op3(v, OP_EQ, 8 + cnt as i32, 0, 8 + i_tab as i32);
                    load_string(v, 4, &p_idx.z_name);
                    add_op3(v, OP_CONCAT, 4, 2, 3);
                    integrity_check_result_row(v);
                    jump_here(v, addr);
                }
                cnt += 1;
            }
        }

        // Garante que todos os índices foram construídos direito.
        for p_tab in &tbls {
            let p_tab: &Rc<Table> = p_tab;
            if !p_tab.is_ordinary_table() {
                continue;
            }
            let mut p_prior: Option<Rc<Index>> = None; // Índice anterior.
            let mut r1: i32 = -1;
            let p_pk: Option<Rc<Index>>;
            let r2: i32; // Chave anterior, nas tabelas WITHOUT ROWID.
            if is_quick || p_tab.has_rowid() {
                p_pk = None;
                r2 = 0;
            } else {
                let pk = primary_key_index(p_tab).cloned();
                let n_key = pk.as_ref().map_or(0, |p| p.n_key_col as i32);
                r2 = get_temp_range(parse, n_key);
                add_op3(vd!(), OP_NULL, 1, r2, r2 + n_key - 1);
                p_pk = pk;
            }
            let mut i_data_cur = 0;
            let mut i_idx_cur = 0;
            open_table_and_indices(
                db,
                parse,
                p_tab,
                crate::consts::OP_OPENREAD,
                0,
                1,
                None,
                &mut i_data_cur,
                &mut i_idx_cur,
            );
            // reg[7] conta as entradas da tabela e reg[8+i] as do i-ésimo índice.
            let v = vd!();
            add_op2(v, OP_INTEGER, 0, 7);
            for j in 0..p_tab.p_index.len() {
                add_op2(v, OP_INTEGER, 0, 8 + j as i32); // contador de entradas do índice
            }
            debug_assert!(parse.n_mem >= 8 + p_tab.p_index.len() as i32);
            let v = vd!();
            add_op2(v, OP_REWIND, i_data_cur, 0);
            let loop_top = add_op2(v, OP_ADDIMM, 7, 1);

            // Lê a coluna mais à direita da tabela. Isso faz o cabeçalho inteiro do registro ser
            // analisado e conferido, e preenche o cache de colunas do cursor que o `OP_IsType`
            // usa, então o passo é obrigatório.
            let mx_col: i32;
            if p_tab.has_rowid() {
                let mut m = -1;
                for col in p_tab.a_col.iter() {
                    if (col.col_flags & COLFLAG_VIRTUAL) == 0 {
                        m += 1;
                    }
                }
                if m == p_tab.i_p_key as i32 {
                    m -= 1;
                }
                mx_col = m;
            } else {
                // As colunas COLFLAG_VIRTUAL não entram na contagem do índice PK do WITHOUT
                // ROWID, então não há por que considerá-las.
                mx_col = primary_key_index(p_tab).map_or(0, |p| p.n_column as i32) - 1;
            }
            if mx_col >= 0 {
                let v = vd!();
                add_op3(v, OP_COLUMN, i_data_cur, mx_col, 3);
                typeof_column(v, 3);
            }

            if !is_quick {
                if let Some(pk) = &p_pk {
                    // Confere se as chaves do WITHOUT ROWID estão em ordem crescente.
                    let z_err = db_printf(
                        db,
                        b"row not in PRIMARY KEY order for %s",
                        &[text_arg(&p_tab.z_name)],
                    )
                    .unwrap_or_default();
                    let v = vd!();
                    let a1 = add_op4_int(v, OP_IDXGT, i_data_cur, 0, r2, pk.n_key_col as i32);
                    add_op1(v, OP_ISNULL, r2);
                    add_op4(v, OP_STRING8, 0, 3, 0, P4::Text(z_err));
                    integrity_check_result_row(v);
                    jump_here(v, a1);
                    jump_here(v, a1 + 1);
                    for j in 0..pk.n_key_col as i32 {
                        expr_code_load_index_column(db, parse, p_tab, pk, i_data_cur, j, r2 + j);
                    }
                }
            }

            // Confere os tipos de dados de todas as colunas:
            //
            //   (1) colunas NOT NULL não podem conter NULL;
            //   (2) o tipo tem de ser exato nas colunas não-ANY de tabelas STRICT;
            //   (3) o tipo das colunas TEXT de tabelas não-STRICT tem de ser NULL, TEXT ou BLOB;
            //   (4) o tipo das colunas numéricas de tabelas não-STRICT não pode ser um TEXT que
            //       converte sem perdas para número.
            let b_strict = (p_tab.tab_flags & TF_STRICT) != 0;
            for j in 0..p_tab.n_col.max(0) as usize {
                let p_col = &p_tab.a_col[j]; // A coluna a conferir.
                if j as i16 == p_tab.i_p_key {
                    continue;
                }
                let do_type_check = if b_strict {
                    p_col.e_c_type > COLTYPE_ANY
                } else {
                    p_col.affinity > SQLITE_AFF_BLOB
                };
                if p_col.not_null == 0 && !do_type_check {
                    continue;
                }

                // Calcula os operandos do `OP_IsType`.
                let mut p4 = SQLITE_NULL;
                let p1: i32;
                let p3: i32;
                if (p_col.col_flags & COLFLAG_VIRTUAL) != 0 {
                    expr_code_get_column_of_table(db, parse, p_tab, i_data_cur, j as i32, 3);
                    p1 = -1;
                    p3 = 3;
                } else {
                    if p_col.i_dflt != 0 {
                        let mut p_dflt_value: Option<Mem> = None;
                        let enc = db.enc;
                        value_from_expr(db, column_expr(p_tab, p_col), enc, p_col.affinity, &mut p_dflt_value);
                        if let Some(m) = &p_dflt_value {
                            p4 = value_type(m);
                        }
                    }
                    p1 = i_data_cur;
                    if !p_tab.has_rowid() {
                        p3 = primary_key_index(p_tab)
                            .map_or(0, |pk| table_column_to_index(pk, j as i16) as i32);
                    } else {
                        p3 = table_column_to_storage(p_tab, j as i16) as i32;
                    }
                }

                let label_error = make_label(parse); // Salta aqui para informar um erro.
                let label_ok = make_label(parse); // Salta aqui se tudo parece certo.
                let tab_name = p_tab.z_name.clone();
                let cn = col_name(p_col);
                let z_err: Vec<u8>;
                if p_col.not_null != 0 {
                    // (1) colunas NOT NULL não podem conter NULL
                    let v = vd!();
                    let jmp2 = add_op4_int(v, OP_ISTYPE, p1, label_ok, p3, p4);
                    let jmp3;
                    if p1 < 0 {
                        change_p5(v, 0x0f); // INT, REAL, TEXT ou BLOB
                        jmp3 = jmp2;
                    } else {
                        change_p5(v, 0x0d); // INT, TEXT ou BLOB
                        // O `OP_IsType` não detecta NaN no arquivo, que deve valer como NULL.
                        // Então, se o tipo do cabeçalho é REAL, é preciso carregar o dado com
                        // `OP_Column` para saber de verdade se é NULL.
                        add_op3(v, OP_COLUMN, p1, p3, 3);
                        column_default(db, parse, p_tab, j as i32, 3);
                        jmp3 = add_op2(vd!(), OP_NOTNULL, 3, label_ok);
                    }
                    let z = db_printf(
                        db,
                        b"NULL value in %s.%s",
                        &[text_arg(&tab_name), text_arg(&cn)],
                    )
                    .unwrap_or_default();
                    let v = vd!();
                    add_op4(v, OP_STRING8, 0, 3, 0, P4::Text(z));
                    if do_type_check {
                        vdbe_goto(v, label_error);
                        jump_here(v, jmp2);
                        jump_here(v, jmp3);
                    }
                    // Senão o bytecode do VDBE segue em frente.
                }
                if b_strict && do_type_check {
                    // (2) o tipo tem de ser exato nas colunas não-ANY de tabelas STRICT
                    const A_STD_TYPE_MASK: [u16; 6] = [
                        0x1f, // ANY
                        0x18, // BLOB
                        0x11, // INT
                        0x11, // INTEGER
                        0x13, // REAL
                        0x14, // TEXT
                    ];
                    let v = vd!();
                    add_op4_int(v, OP_ISTYPE, p1, label_ok, p3, p4);
                    debug_assert!(p_col.e_c_type >= 1 && p_col.e_c_type as usize <= A_STD_TYPE_MASK.len());
                    change_p5(v, A_STD_TYPE_MASK[p_col.e_c_type as usize - 1]);
                    z_err = db_printf(
                        db,
                        b"non-%s value in %s.%s",
                        &[
                            text_arg(STD_TYPE[p_col.e_c_type as usize - 1].as_bytes()),
                            text_arg(&tab_name),
                            text_arg(&cn),
                        ],
                    )
                    .unwrap_or_default();
                    add_op4(vd!(), OP_STRING8, 0, 3, 0, P4::Text(z_err));
                } else if !b_strict && p_col.affinity == SQLITE_AFF_TEXT {
                    // (3) o tipo das colunas TEXT de tabelas não-STRICT tem de ser NULL, TEXT ou
                    // BLOB
                    let v = vd!();
                    add_op4_int(v, OP_ISTYPE, p1, label_ok, p3, p4);
                    change_p5(v, 0x1c); // NULL, TEXT ou BLOB
                    z_err = db_printf(
                        db,
                        b"NUMERIC value in %s.%s",
                        &[text_arg(&tab_name), text_arg(&cn)],
                    )
                    .unwrap_or_default();
                    add_op4(vd!(), OP_STRING8, 0, 3, 0, P4::Text(z_err));
                } else if !b_strict && p_col.affinity >= SQLITE_AFF_NUMERIC {
                    // (4) o tipo das colunas numéricas de tabelas não-STRICT não pode ser um TEXT
                    // que converte para número
                    let v = vd!();
                    add_op4_int(v, OP_ISTYPE, p1, label_ok, p3, p4);
                    change_p5(v, 0x1b); // NULL, INT, FLOAT ou BLOB
                    if p1 >= 0 {
                        expr_code_get_column_of_table(db, parse, p_tab, i_data_cur, j as i32, 3);
                    }
                    let v = vd!();
                    add_op4(v, OP_AFFINITY, 3, 1, 0, P4::Text(b"C".to_vec()));
                    add_op4_int(v, OP_ISTYPE, -1, label_ok, 3, p4);
                    change_p5(v, 0x1c); // NULL, TEXT ou BLOB
                    z_err = db_printf(
                        db,
                        b"TEXT value in %s.%s",
                        &[text_arg(&tab_name), text_arg(&cn)],
                    )
                    .unwrap_or_default();
                    add_op4(vd!(), OP_STRING8, 0, 3, 0, P4::Text(z_err));
                }
                resolve_label(parse, db, label_error);
                integrity_check_result_row(vd!());
                resolve_label(parse, db, label_ok);
            }

            // Confere as restrições CHECK.
            if p_tab.p_check.is_some() && (db.flags & SQLITE_IGNORE_CHECKS) == 0 {
                if let Some(mut p_check) = expr_list_dup(p_tab.p_check.as_deref(), 0) {
                    let addr_ck_fault = make_label(parse);
                    let addr_ck_ok = make_label(parse);
                    parse.i_self_tab = i_data_cur + 1;
                    for k in (1..p_check.a.len()).rev() {
                        if let Some(e) = p_check.a[k].p_expr.as_deref_mut() {
                            expr_if_false(db, parse, e, addr_ck_fault, 0, Some(p_tab));
                        }
                    }
                    if let Some(e) = p_check.a[0].p_expr.as_deref_mut() {
                        expr_if_true(db, parse, e, addr_ck_ok, SQLITE_JUMPIFNULL as i32, Some(p_tab));
                    }
                    resolve_label(parse, db, addr_ck_fault);
                    parse.i_self_tab = 0;
                    let z_err = db_printf(
                        db,
                        b"CHECK constraint failed in %s",
                        &[text_arg(&p_tab.z_name)],
                    )
                    .unwrap_or_default();
                    let v = vd!();
                    add_op4(v, OP_STRING8, 0, 3, 0, P4::Text(z_err));
                    integrity_check_result_row(v);
                    resolve_label(parse, db, addr_ck_ok);
                }
            }
            if !is_quick {
                // Omite os demais testes no quick_check. Valida as entradas de índice da linha
                // corrente.
                for (j, p_idx) in p_tab.p_index.iter().enumerate() {
                    let j = j as i32;
                    let ck_uniq = make_label(parse);
                    if p_pk.as_ref().map_or(false, |pk| Rc::ptr_eq(pk, p_idx)) {
                        continue;
                    }
                    let mut jmp3 = 0;
                    r1 = generate_index_key(
                        db,
                        parse,
                        p_tab,
                        p_idx,
                        i_data_cur,
                        0,
                        0,
                        &mut jmp3,
                        p_prior.as_deref(),
                        r1,
                    );
                    p_prior = Some(Rc::clone(p_idx));
                    let v = vd!();
                    add_op2(v, OP_ADDIMM, 8 + j, 1); // incrementa a contagem de entradas
                    // Confere se existe uma entrada de índice para a linha corrente da tabela.
                    let jmp2 = add_op4_int(v, OP_FOUND, i_idx_cur + j, ck_uniq, r1, p_idx.n_column as i32);
                    load_string(v, 3, b"row ");
                    add_op3(v, OP_CONCAT, 7, 3, 3);
                    load_string(v, 4, b" missing from index ");
                    add_op3(v, OP_CONCAT, 4, 3, 3);
                    let jmp5 = load_string(v, 4, &p_idx.z_name);
                    add_op3(v, OP_CONCAT, 4, 3, 3);
                    let jmp4 = integrity_check_result_row(v);
                    jump_here(v, jmp2);

                    // O `OP_IdxRowid` é uma versão otimizada do `OP_Column` que extrai o rowid do
                    // fim do registro do índice. Mas só funciona se o registro não tem bytes
                    // extras no fim; confere que é o caso.
                    if p_tab.has_rowid() {
                        add_op2(v, OP_IDXROWID, i_idx_cur + j, 3);
                        let jmp7 = add_op3(v, OP_EQ, 3, 0, r1 + p_idx.n_column as i32 - 1);
                        load_string(v, 3, b"rowid not at end-of-record for row ");
                        add_op3(v, OP_CONCAT, 7, 3, 3);
                        load_string(v, 4, b" of index ");
                        vdbe_goto(v, jmp5 - 1);
                        jump_here(v, jmp7);
                    }

                    // Colunas indexadas com colação que não é BINARY ainda têm de guardar
                    // exatamente o mesmo texto da tabela.
                    let mut label6 = 0;
                    for kk in 0..p_idx.n_key_col as i32 {
                        if p_idx.az_coll[kk as usize].as_slice() == crate::util::STR_BINARY.as_bytes() {
                            continue;
                        }
                        if label6 == 0 {
                            label6 = make_label(parse);
                        }
                        let v = vd!();
                        add_op3(v, OP_COLUMN, i_idx_cur + j, kk, 3);
                        add_op3(v, OP_NE, 3, label6, r1 + kk);
                    }
                    if label6 != 0 {
                        let v = vd!();
                        let jmp6 = add_op0(v, crate::consts::OP_GOTO);
                        resolve_label(parse, db, label6);
                        let v = vd!();
                        load_string(v, 3, b"row ");
                        add_op3(v, OP_CONCAT, 7, 3, 3);
                        load_string(v, 4, b" values differ from index ");
                        vdbe_goto(v, jmp5 - 1);
                        jump_here(v, jmp6);
                    }

                    // Nos índices UNIQUE, confere que só existe uma entrada com a chave corrente.
                    // A entrada é única se (1) alguma coluna é NULL ou (2) a próxima entrada tem
                    // outra chave.
                    if p_idx.is_unique_index() {
                        let uniq_ok = make_label(parse);
                        for kk in 0..p_idx.n_key_col as usize {
                            let i_col = p_idx.ai_column[kk];
                            debug_assert!(i_col != XN_ROWID && (i_col as i32) < p_tab.n_col as i32);
                            if i_col >= 0 && p_tab.a_col[i_col as usize].not_null != 0 {
                                continue;
                            }
                            add_op2(vd!(), OP_ISNULL, r1 + kk as i32, uniq_ok);
                        }
                        let v = vd!();
                        let jmp6 = add_op1(v, OP_NEXT, i_idx_cur + j);
                        vdbe_goto(v, uniq_ok);
                        jump_here(v, jmp6);
                        add_op4_int(v, OP_IDXGT, i_idx_cur + j, uniq_ok, r1, p_idx.n_key_col as i32);
                        load_string(v, 3, b"non-unique entry in index ");
                        vdbe_goto(v, jmp5);
                        resolve_label(parse, db, uniq_ok);
                    }
                    jump_here(vd!(), jmp4);
                    resolve_part_idx_label(db, parse, jmp3);
                }
            }
            let v = vd!();
            add_op2(v, OP_NEXT, i_data_cur, loop_top);
            jump_here(v, loop_top - 1);
            if p_pk.is_some() {
                debug_assert!(!is_quick);
                let n_key = p_pk.as_ref().map_or(0, |p| p.n_key_col as i32);
                release_temp_range(parse, r2, n_key);
            }
        }

        // Segunda passada: chama o método `xIntegrity` de todas as tabelas virtuais.
        for p_tab in &tbls {
            if p_tab.is_ordinary_table() || !p_tab.is_virtual() {
                continue;
            }
            if p_tab.n_col <= 0 {
                let z_mod = p_tab.u_vtab().and_then(|u| u.az_arg.first()).cloned().unwrap_or_default();
                if hash_find(&db.a_module, &z_mod).is_none() {
                    continue;
                }
            }
            let mut p_tab: Rc<Table> = Rc::clone(p_tab);
            view_get_column_names(db, parse, &mut p_tab);
            let Some(vid) = get_vtable(db, &p_tab) else {
                continue;
            };
            let has_integrity = db.vtabs.get(vid.slot()).map_or(false, |vt| {
                vt.p_mod.p_module.i_version() >= 4 && vt.p_mod.p_module.caps().integrity
            });
            if !has_integrity {
                continue;
            }
            let v = vd!();
            add_op3(v, OP_VCHECK, i as i32, 3, is_quick as i32);
            append_p4(v, P4::TableRef(Rc::clone(&p_tab)));
            let a1 = add_op1(v, OP_ISNULL, 3);
            integrity_check_result_row(v);
            jump_here(v, a1);
        }
    }
    let end_code: [VdbeOpList; 7] = [
        ol(OP_ADDIMM, 1, 0, 0),    /* 0 */
        ol(OP_IFNOTZERO, 1, 4, 0), /* 1 */
        ol(OP_STRING8, 0, 3, 0),   /* 2 */
        ol(OP_RESULTROW, 3, 1, 0), /* 3 */
        ol(OP_HALT, 0, 0, 0),      /* 4 */
        ol(OP_STRING8, 0, 3, 0),   /* 5 */
        ol(OP_GOTO, 0, 3, 0),      /* 6 */
    ];
    let v = vd!();
    if let Some(a) = add_op_list(v, &end_code, 2) {
        v.a_op[a].p2 = 1 - mx_err;
        v.a_op[a + 2].p4 = P4::Text(b"ok".to_vec());
        v.a_op[a + 5].p4 = P4::Text(err_str(SQLITE_CORRUPT).as_bytes().to_vec());
    }
    let cur = current_addr(parse);
    change_p3(vd!(), 0, cur - 2);
}

// ---------------------------------------------------------------------------------------------
// optimize
// ---------------------------------------------------------------------------------------------

/// `PRAGMA optimize`, `optimize(MASK)`, `schema.optimize` e `schema.optimize(MASK)`: tenta
/// otimizar o banco. Nas duas primeiras formas todos os esquemas são otimizados; nas outras duas,
/// só o nomeado.
///
/// Os detalhes das otimizações devem mudar e melhorar com o tempo: as aplicações devem esperar
/// que o pragma faça otimizações novas em versões futuras.
///
/// O argumento opcional é uma máscara de bits:
///
/// - 0x00001: modo de depuração. Não faz otimização alguma, mas devolve uma linha de texto para
///   cada uma que seria feita. Desligado por padrão.
/// - 0x00002: roda ANALYZE nas tabelas que podem se beneficiar. Ligado por padrão.
/// - 0x00010: roda todo ANALYZE com um `analysis_limit` que é o menor entre o corrente e
///   `SQLITE_DEFAULT_OPTIMIZE_LIMIT` (2000). Ligado por padrão.
/// - 0x10000: olha as tabelas para ver se precisam de nova análise por crescimento ou encolhimento
///   mesmo que não tenham sido consultadas na conexão corrente. Desligado por padrão.
///
/// A máscara padrão é, e sempre será, 0x0fffe. Otimizações desligadas por padrão e que precisam
/// ser pedidas têm máscaras de 0x10000 ou mais.
///
/// QUANDO RODAR ANALYZE: uma tabela é analisada se, e só se, (1) o bit 0x00002 está ligado, (2) a
/// tabela é comum, (3) o nome não começa com "sqlite_", (4) o bit 0x10000 está ligado, ou algum
/// índice da tabela não tem entrada no sqlite_stat1, ou o planejador usou estatísticas no estilo
/// sqlite_stat1 em algum índice da tabela durante a conexão, e (5) algum índice não tem entrada no
/// sqlite_stat1, ou o número de linhas da tabela cresceu ou encolheu dez vezes em relação ao do
/// sqlite_stat1.
pub(crate) fn pragma_optimize(db: &mut Connection, parse: &mut Parse, c: &PragmaCtx) {
    macro_rules! vd {
        () => {
            vdbe_of_parse(parse)
        };
    }
    let op_mask: u32; // Máscara das operações a fazer.
    if let Some(zr) = c.z_right {
        op_mask = atoi(zr) as u32;
        if (op_mask & 0x02) == 0 {
            return;
        }
    } else {
        op_mask = 0xfffe;
    }
    let mut n_limit: i32; // O limite de análise a usar.
    if (op_mask & 0x10) == 0 {
        n_limit = 0;
    } else if db.n_analysis_limit > 0 && db.n_analysis_limit < SQLITE_DEFAULT_OPTIMIZE_LIMIT {
        n_limit = 0;
    } else {
        n_limit = SQLITE_DEFAULT_OPTIMIZE_LIMIT;
    }
    let mut n_check = 0; // Número de tabelas a otimizar.
    let mut n_btree: i32 = 0; // Número de árvores-b a varrer.
    let i_tab_cur = parse.n_tab; // Cursor de uma tabela cujo tamanho precisa ser conferido.
    parse.n_tab += 1;
    let i_db_last = if c.z_db.is_some() { c.i_db } else { db.dbs.len() as i32 - 1 };
    let mut i_db = c.i_db;
    while i_db <= i_db_last {
        if i_db == 1 {
            i_db += 1;
            continue;
        }
        code_verify_schema(db, parse, i_db);
        for p_tab in schema_tables(db, i_db as usize) {
            // Só funciona com tabelas comuns.
            if !p_tab.is_ordinary_table() {
                continue;
            }

            // Não varre tabelas do sistema.
            if crate::util::strnicmp(Some(&p_tab.z_name), Some(b"sqlite_"), 7) == 0 {
                continue;
            }

            // Acha o tamanho da tabela como registrado por último no sqlite_stat1. Se algum
            // índice não foi analisado, o limiar é -1, que indica um índice novo não analisado.
            let mut sz_threshold = p_tab.n_row_log_est; // Limiar acima do qual reanalisa.
            let mut n_index = 0;
            for p_idx in p_tab.p_index.iter() {
                n_index += 1;
                if !p_idx.has_stat1 {
                    sz_threshold = -1; // Sempre analisa se algum índice não tem estatísticas
                }
            }

            // Se a tabela não foi usada de modo que se beneficie de estatísticas de análise
            // durante a sessão corrente, pula-a, a menos que o bit 0x10000 esteja ligado.
            if (p_tab.tab_flags & TF_MAYBE_REANALYZE) != 0 {
                // Confere a mudança de tamanho se o stat1 foi usado numa consulta
            } else if (op_mask & 0x10000) != 0 {
                // Confere a mudança de tamanho se o 0x10000 está ligado
            } else if !p_tab.p_index.is_empty() && sz_threshold < 0 {
                // Analisa se existem índices não analisados
            } else {
                // Senão a tabela pode ser pulada
                continue;
            }

            n_check += 1;
            if n_check == 2 {
                // Se o ANALYZE pode rodar duas ou mais vezes, mantém uma transação de escrita
                // aberta, por eficiência.
                begin_write_operation(db, parse, 0, i_db);
            }
            n_btree += n_index + 1;

            // Reanalisa se a tabela ficou dez vezes maior ou menor desde a última análise.
            // Reanálise incondicional se há índices não analisados.
            open_table(db, parse, i_tab_cur, i_db, &p_tab, OP_OPENREAD);
            let cur = current_addr(parse);
            if sz_threshold >= 0 {
                const I_RANGE: i32 = 33; // mudança de 10x no tamanho
                let sz = sz_threshold as i32;
                add_op4_int(
                    vd!(),
                    OP_IFSIZEBETWEEN,
                    i_tab_cur,
                    cur + 2 + (op_mask & 1) as i32,
                    if sz >= I_RANGE { sz - I_RANGE } else { -1 },
                    sz + I_RANGE,
                );
            } else {
                add_op2(vd!(), OP_REWIND, i_tab_cur, cur + 2 + (op_mask & 1) as i32);
            }
            let db_name = db.dbs[i_db as usize].z_db_s_name.clone();
            let z_sub_sql = db_printf(
                db,
                b"ANALYZE \"%w\".\"%w\"",
                &[text_arg(&db_name), text_arg(&p_tab.z_name)],
            )
            .unwrap_or_default();
            if (op_mask & 0x01) != 0 {
                let r1 = get_temp_reg(parse);
                let v = vd!();
                add_op4(v, OP_STRING8, 0, r1, 0, P4::Text(z_sub_sql));
                add_op2(v, OP_RESULTROW, r1, 1);
            } else {
                add_op4(
                    vd!(),
                    OP_SQLEXEC,
                    if n_limit != 0 { 0x02 } else { 0 },
                    n_limit,
                    0,
                    P4::Text(z_sub_sql),
                );
            }
        }
        i_db += 1;
    }
    add_op0(vd!(), OP_EXPIRE);

    // Num esquema com muitas tabelas e índices, diminui o `analysis_limit` para evitar tempo
    // excessivo no pior caso.
    if db.malloc_failed == 0 && n_limit > 0 && n_btree > 100 {
        n_limit = 100 * n_limit / n_btree;
        if n_limit < 100 {
            n_limit = 100;
        }
        for op in vd!().a_op.iter_mut() {
            if op.opcode == OP_SQLEXEC {
                op.p2 = n_limit;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// A tabela virtual epônima que roda um pragma
// ---------------------------------------------------------------------------------------------

/// `struct PragmaVtab`: a tabela virtual `pragma_xxx`.
struct PragmaVtab {
    /// `zErrMsg` da classe base.
    z_err_msg: Option<Vec<u8>>,
    /// O pragma (`pName`): o índice em `PRAGMA_NAMES`.
    p_name: usize,
    /// Número de colunas ocultas.
    n_hidden: u8,
    /// Índice da primeira coluna oculta.
    i_hidden: u8,
}

/// `struct PragmaVtabCursor`: o cursor da tabela virtual `pragma_xxx`.
#[derive(Default)]
struct PragmaVtabCursor {
    /// O comando do pragma a executar (`pPragma`).
    p_pragma: Option<crate::connection::StmtId>,
    /// O rowid corrente.
    i_rowid: i64,
    /// O valor do argumento e o do esquema (`azArg`).
    az_arg: [Option<Vec<u8>>; 2],
}

impl PragmaVtabCursor {
    /// `pragmaVtabCursorClear`: apaga todo o conteúdo do cursor.
    fn clear(&mut self, db: &mut Connection) {
        if let Some(id) = self.p_pragma.take() {
            finalize(db, id);
        }
        self.i_rowid = 0;
        self.az_arg = [None, None];
    }
}

impl Vtab for PragmaVtab {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn err_msg_mut(&mut self) -> &mut Option<Vec<u8>> {
        &mut self.z_err_msg
    }

    /// `pragmaVtabBestIndex`: acha o melhor índice para pesquisar uma tabela virtual de pragma.
    ///
    /// Na verdade não há escolhas de índice. Mas queremos animar o planejador a dar restrições
    /// `==` em quantos parâmetros ocultos for possível, e sobretudo no primeiro. Por isso o custo
    /// é alto se os parâmetros ocultos não têm restrição.
    fn best_index(&mut self, _db: &mut Connection, info: &mut IndexInfo) -> i32 {
        info.estimated_cost = 1.0;
        if self.n_hidden == 0 {
            return SQLITE_OK;
        }
        let mut seen = [0usize; 2];
        for (i, p_constraint) in info.a_constraint.iter().enumerate() {
            if p_constraint.i_column < self.i_hidden as i32 {
                continue;
            }
            if p_constraint.op as i32 != SQLITE_INDEX_CONSTRAINT_EQ {
                continue;
            }
            if !p_constraint.usable {
                return SQLITE_CONSTRAINT;
            }
            let j = (p_constraint.i_column - self.i_hidden as i32) as usize;
            debug_assert!(j < 2);
            if j < 2 {
                seen[j] = i + 1;
            }
        }
        if seen[0] == 0 {
            info.estimated_cost = 2147483647.0;
            info.estimated_rows = 2147483647;
            return SQLITE_OK;
        }
        let j = seen[0] - 1;
        info.a_constraint_usage[j].argv_index = 1;
        info.a_constraint_usage[j].omit = true;
        info.estimated_cost = 20.0;
        info.estimated_rows = 20;
        if seen[1] != 0 {
            let j = seen[1] - 1;
            info.a_constraint_usage[j].argv_index = 2;
            info.a_constraint_usage[j].omit = true;
        }
        SQLITE_OK
    }

    /// `pragmaVtabDisconnect`.
    fn disconnect(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `xDestroy` é nulo no módulo (a tabela é epônima e não se destrói).
    fn destroy(&mut self, _db: &mut Connection) -> i32 {
        SQLITE_OK
    }

    /// `pragmaVtabOpen`: cria um cursor novo da tabela virtual de pragma.
    fn open(&mut self, _db: &mut Connection) -> Result<Box<dyn VtabCursor>, i32> {
        Ok(Box::new(PragmaVtabCursor::default()))
    }
}

impl VtabCursor for PragmaVtabCursor {
    /// `pragmaVtabClose`.
    fn close(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        self.clear(db);
        SQLITE_OK
    }

    /// `pragmaVtabFilter`.
    fn filter(
        &mut self,
        db: &mut Connection,
        vtab: &mut dyn Vtab,
        _idx_num: i32,
        _idx_str: Option<&[u8]>,
        argv: &[Mem],
    ) -> i32 {
        let Some(p_tab) = vtab.as_any_mut().downcast_mut::<PragmaVtab>() else {
            return SQLITE_ERROR;
        };
        let p_name: &'static PragmaName = &PRAGMA_NAMES[p_tab.p_name];
        self.clear(db);
        let mut j = if (p_name.m_prag_flg & PRAG_FLG_RESULT1) != 0 { 0 } else { 1 };
        for a in argv {
            let mut m = a.clone();
            let z_text = value_text(&mut m).map(<[u8]>::to_vec);
            debug_assert!(j < self.az_arg.len());
            if let (Some(z), true) = (z_text, j < self.az_arg.len()) {
                self.az_arg[j] = Some(z);
            }
            j += 1;
        }
        let mut acc = StrAccum::new(db.a_limit[SQLITE_LIMIT_SQL_LENGTH as usize] as u32);
        acc.append_all(b"PRAGMA ");
        if let Some(z) = &self.az_arg[1] {
            acc.appendf(b"%Q.", &[text_arg(z)]);
        }
        acc.append_all(p_name.z_name.as_bytes());
        if let Some(z) = &self.az_arg[0] {
            acc.appendf(b"=%Q", &[text_arg(z)]);
        }
        let Some(z_sql) = acc.finish() else {
            return SQLITE_NOMEM;
        };
        let (rc, p_stmt, _) = prepare_v2(db, &z_sql, -1);
        self.p_pragma = p_stmt;
        if rc != SQLITE_OK {
            if let Some(p_tab) = vtab.as_any_mut().downcast_mut::<PragmaVtab>() {
                p_tab.z_err_msg = Some(errmsg(db));
            }
            return rc;
        }
        self.next(db, vtab)
    }

    /// `pragmaVtabNext`: avança o cursor para a próxima linha.
    fn next(&mut self, db: &mut Connection, _vtab: &mut dyn Vtab) -> i32 {
        let mut rc = SQLITE_OK;

        // Incrementa o valor do xRowid.
        self.i_rowid += 1;
        debug_assert!(self.p_pragma.is_some());
        if let Some(id) = self.p_pragma {
            if SQLITE_ROW != step(db, id) {
                rc = finalize(db, id);
                self.p_pragma = None;
                self.clear(db);
            }
        }
        rc
    }

    /// `pragmaVtabEof`.
    fn eof(&mut self, _vtab: &mut dyn Vtab) -> i32 {
        self.p_pragma.is_none() as i32
    }

    /// `pragmaVtabColumn`: devolve a coluna correspondente do PRAGMA.
    fn column(&mut self, vtab: &mut dyn Vtab, ctx: &mut Context<'_>, i: i32) -> i32 {
        let i_hidden = vtab.as_any_mut().downcast_mut::<PragmaVtab>().map_or(0, |t| t.i_hidden) as i32;
        if i < i_hidden {
            if let Some(id) = self.p_pragma {
                let m = column_value(ctx.db, id, i);
                result_value(ctx, &m);
            }
        } else {
            let z = self.az_arg.get((i - i_hidden) as usize).and_then(|a| a.as_deref());
            result_text(ctx, z, -1, StrDtor::Transient);
        }
        SQLITE_OK
    }

    /// `pragmaVtabRowid`.
    fn rowid(&mut self, _vtab: &mut dyn Vtab, rowid: &mut i64) -> i32 {
        *rowid = self.i_rowid;
        SQLITE_OK
    }
}

/// O módulo da tabela virtual de pragma (`pragmaVtabModule`): sem `xCreate`, portanto epônimo.
struct PragmaVtabModule;

impl VtabModule for PragmaVtabModule {
    fn caps(&self) -> ModuleCaps {
        ModuleCaps::default()
    }

    /// `pragmaVtabConnect`: o `aux` é o índice do pragma.
    fn x_connect(
        &self,
        db: &mut Connection,
        aux: &Option<Rc<dyn Any>>,
        _argv: &[Vec<u8>],
        err: &mut Option<Vec<u8>>,
    ) -> Result<Box<dyn Vtab>, i32> {
        let Some(idx) = aux.as_ref().and_then(|a| a.downcast_ref::<usize>()).copied() else {
            return Err(SQLITE_ERROR);
        };
        let p_pragma = &PRAGMA_NAMES[idx];
        let mut acc: Vec<u8> = b"CREATE TABLE x".to_vec();
        let mut c_sep = b'(';
        let mut i: u8 = 0;
        for j in 0..p_pragma.n_prag_c_name as usize {
            acc.push(c_sep);
            acc.push(b'"');
            acc.extend_from_slice(PRAG_C_NAME[p_pragma.i_prag_c_name as usize + j].as_bytes());
            acc.push(b'"');
            c_sep = b',';
            i += 1;
        }
        if i == 0 {
            acc.extend_from_slice(b"(\"");
            acc.extend_from_slice(p_pragma.z_name.as_bytes());
            acc.push(b'"');
            i += 1;
        }
        let mut j: u8 = 0;
        if (p_pragma.m_prag_flg & PRAG_FLG_RESULT1) != 0 {
            acc.extend_from_slice(b",arg HIDDEN");
            j += 1;
        }
        if (p_pragma.m_prag_flg & (PRAG_FLG_SCHEMA_OPT | PRAG_FLG_SCHEMA_REQ)) != 0 {
            acc.extend_from_slice(b",schema HIDDEN");
            j += 1;
        }
        acc.push(b')');
        let rc = declare_vtab(db, &acc);
        if rc == SQLITE_OK {
            Ok(Box::new(PragmaVtab { z_err_msg: None, p_name: idx, n_hidden: j, i_hidden: i }))
        } else {
            *err = Some(errmsg(db));
            Err(rc)
        }
    }
}

/// `sqlite3PragmaVtabRegister`: vê se `z_name` é mesmo o nome de um pragma. Se é, registra uma
/// tabela virtual epônima para o pragma e devolve o `Module` da tabela virtual nova.
pub fn pragma_vtab_register(db: &mut Connection, z_name: &[u8]) -> Option<Rc<Module>> {
    debug_assert!(crate::util::strnicmp(Some(z_name), Some(b"pragma_"), 7) == 0);
    let idx = pragma_locate(z_name.get(7..)?)?;
    if (PRAGMA_NAMES[idx].m_prag_flg & (PRAG_FLG_RESULT0 | PRAG_FLG_RESULT1)) == 0 {
        return None;
    }
    debug_assert!(hash_find(&db.a_module, z_name).is_none());
    vtab_create_module(db, z_name, Some(Rc::new(PragmaVtabModule)), Some(Rc::new(idx)), None)
}
