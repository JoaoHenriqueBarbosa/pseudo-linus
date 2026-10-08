//! Segunda parte de `build.c` (trechos 005 a 009, mais o fim de `sqlite3CreateIndex` que o
//! fatiamento deixou no trecho 010): `sqlite3AddGenerated`, `sqlite3ChangeCookie`,
//! `createTableStmt`, `convertToWithoutRowidTable`, as tabelas sombra, `sqlite3EndTable`,
//! `sqlite3CreateView`, `sqlite3ViewGetColumnNames`, o apagamento de tabelas (`sqlite3DropTable`,
//! `sqlite3CodeDropTable`), `sqlite3CreateForeignKey`, `sqlite3DeferForeignKey`,
//! `sqlite3RefillIndex`, `sqlite3HasExplicitNulls` e `sqlite3CreateIndex`.
//!
//! Segue as decisões de `build.rs` (ver o cabeçalho dele e CONVENTIONS.md):
//!
//! - toda função recebe `db: &mut Connection` e `parse: &mut Parse`, nessa ordem;
//! - a tabela em construção é `parse.p_new_table`. As funções que a alteram a tiram do `Parse`,
//!   trabalham e a devolvem (`with_new_table`, ou o `take` explícito onde o corpo precisa chamar
//!   de volta o analisador, como `sqlite3EndTable` e `sqlite3CreateIndex`);
//! - `Index` não tem `pTable`, `pNext` nem `pSchema`: as funções que no C liam a tabela dona a
//!   partir do índice recebem a tabela (ou só as colunas) por parâmetro. A lista `pTable->pIndex`
//!   é `Table.p_index` com a cabeça na posição 0 (o índice novo entra na posição 0);
//! - o texto do SQL em análise (o `zSql` de que os `Token.z` do C são ponteiros) é
//!   `Parse.z_sql`; os trechos `CREATE ... ` que o C monta por aritmética de ponteiros saem de
//!   `Token.i_ofst` e desse texto (`sql_span`);
//! - o `Rc<Table>` do esquema só se altera por `Rc::make_mut` sobre a entrada da tabela hash
//!   (`view_get_column_names`, `root_page_moved`, `sqlite_view_reset_all`).
//!
//! `OOM` não existe em Rust (`db.malloc_failed` é sempre zero), então os ramos `mallocFailed` e o
//! código de erro de `resizeIndexObject` somem.

use std::rc::Rc;

use crate::connection::{
    Connection, Parse, OPFLAG_BULKCSR, OPFLAG_P2ISREG, OPFLAG_USESEEKRESULT, PARSE_MODE_NORMAL,
};
use crate::consts::{
    Bitmask, BMS, BTREE_BLOBKEY, BTREE_SCHEMA_VERSION, COLFLAG_GENERATED, COLFLAG_HASTYPE,
    COLFLAG_NOINSERT, COLFLAG_PRIMKEY, COLFLAG_STORED, COLFLAG_UNIQUE, COLFLAG_VIRTUAL,
    COLTYPE_ANY, COLTYPE_CUSTOM, DBFLAG_SCHEMA_CHANGE, DB_UNRESETVIEWS, EP_INT_VALUE,
    EXPRDUP_REDUCE, LEGACY_SCHEMA_TABLE, LOCATE_VIEW, NC_GENCOL, NC_IDXEXPR, NC_ISCHECK,
    NC_PARTIDX, OE_ABORT, OE_DEFAULT, OE_NONE, OE_REPLACE, OMIT_TEMPDB, OP_CLEAR, OP_CLOSE,
    OP_CREATEBTREE, OP_DESTROY, OP_DROPTABLE, OP_EXPIRE, OP_GOTO, OP_IDXINSERT,
    OP_INITCOROUTINE, OP_INSERT, OP_MAKERECORD, OP_NEWROWID, OP_NEXT, OP_NOOP, OP_OPENREAD,
    OP_OPENWRITE, OP_REWIND, OP_SEEKEND, OP_SETCOOKIE, OP_SORTERCOMPARE, OP_SORTERDATA,
    OP_SORTERINSERT, OP_SORTERNEXT, OP_SORTEROPEN, OP_SORTERSORT, OP_SQLEXEC, OP_VBEGIN,
    OP_VDESTROY, OP_YIELD, SF_VIEW, SQLITE_AFF_BLOB, SQLITE_AFF_FLEXNUM, SQLITE_AFF_NONE,
    SQLITE_CORRUPT, SQLITE_CREATE_INDEX, SQLITE_CREATE_TEMP_INDEX, SQLITE_DEFENSIVE,
    SQLITE_DELETE, SQLITE_DROP_TABLE, SQLITE_DROP_TEMP_TABLE, SQLITE_DROP_TEMP_VIEW,
    SQLITE_DROP_VIEW, SQLITE_DROP_VTABLE, SQLITE_ERROR, SQLITE_IDXTYPE_APPDEF,
    SQLITE_IDXTYPE_PRIMARYKEY, SQLITE_INSERT, SQLITE_OK, SQLITE_REINDEX, SQLITE_SO_UNDEFINED,
    SRT_COROUTINE, TABTYP_VIEW, TF_AUTOINCREMENT, TF_EPONYMOUS, TF_HAS_GENERATED,
    TF_HAS_NOT_NULL, TF_HAS_PRIMARY_KEY, TF_HAS_STORED, TF_HAS_VIRTUAL, TF_NO_VISIBLE_ROWID,
    TF_READONLY, TF_SHADOW, TF_STRICT, TF_WITHOUT_ROWID, TK_COLLATE, TK_COLUMN, TK_ID, TK_NULL,
    TK_RAISE, TK_UPLUS, XN_EXPR, XN_ROWID,
};
use crate::ctype::{is_alnum, is_digit, is_space};
use crate::hash::{
    hash_data_mut, hash_find, hash_find_mut, hash_first, hash_insert, hash_iter, hash_next,
};
use crate::mem::KeyInfo;
use crate::printf::{mprintf, PrintfArg};
use crate::sqlite_int::{
    Column, Expr, ExprList, FKey, FKeyColMap, Index, SchemaId, Select, SelectDest, SrcList, Table,
    TableU, Token, ViewInfo,
};
use crate::util::{
    at, column_type, dequote, error_msg, log_est, str_icmp, strlen30, strnicmp, STR_BINARY,
};
use crate::vdbe_types::P4;
use crate::vdbeaux::{
    add_op0, add_op1, add_op2, add_op3, add_op4, add_op4_int, add_parse_schema_op, change_opcode,
    change_p3, change_p5, end_coroutine, jump_here, vdbe_goto,
};

// Funções de `build.rs` (a primeira parte de `build.c`) e dos trechos 011 em diante, que outra
// fatia traduz: nomes determinísticos, assinaturas supostas no relatório.
use crate::select::get_vdbe;
use crate::build::{nested_parse, 
    affinity_type, begin_write_operation, check_object_name, code_verify_named_schema,
    code_verify_schema, column_coll, column_set_expr, delete_column_names, find_index, find_table,
    force_not_read_only, key_info_of_index, locate_table_item, make_column_part_of_primary_key,
    may_abort, multi_write, name_from_token, primary_key_index, schema_table,
    src_list_assign_cursors, start_table, string_to_id, table_column_to_index, table_lock,
    text_arg, token_arg, two_part_name, with_new_table,
};

// Funções de outras fatias, chamadas pelo nome determinístico.
use crate::alter::{rename_exprlist_unmap, rename_token_map, rename_token_remap};
use crate::build3::default_row_est;
use crate::attach::{fix_init, fix_select, fix_src_list};
use crate::auth::auth_check;
use crate::callback::locate_coll_seq;
use crate::delete::{resolve_part_idx_label, src_list_lookup};
use crate::expr::{
    expr_alloc, expr_list_append, expr_list_check_length, expr_list_dup,
    expr_list_set_sort_order, expr_skip_collate, get_temp_reg, p_expr as new_p_expr,
    release_temp_reg, select_dup,
};
use crate::fkey::fk_drop_table;
use crate::insert::{open_table, table_affinity};
use crate::delete::generate_index_key;
use crate::build::unique_constraint;
use crate::prepare::{index_has_duplicate_root_page, read_schema, schema_to_index};
use crate::resolve::resolve_self_reference;
use crate::select::{
    columns_from_expr_list, result_set_of_select, select, select_dest_init,
    subquery_column_types,
};
use crate::tokenize::keyword_code;
use crate::trigger::{drop_trigger_ptr, trigger_list};
use crate::vtab::{get_vtable, vtab_call_connect, vtab_in_sync};

// ---------------------------------------------------------------------------------------------
// Pequenos auxiliares
// ---------------------------------------------------------------------------------------------

/// O texto de `Column.z_cn_name` até o primeiro NUL: o nome da coluna.
fn col_name(p_col: &Column) -> &[u8] {
    let z = &p_col.z_cn_name;
    &z[..strlen30(z) as usize]
}

/// `z` até o primeiro NUL (ou o fim), como um `char*` do C.
fn c_str(z: &[u8]) -> &[u8] {
    &z[..strlen30(z) as usize]
}

/// O trecho de `n` bytes do SQL em análise que começa em `i_ofst`. É o que o C obtém com
/// `"%.*s", n, pToken->z`, onde `pToken->z` aponta para dentro de `zSql`.
fn sql_span(parse: &Parse, i_ofst: i32, n: i32) -> Vec<u8> {
    let z = &parse.z_sql;
    let start = (i_ofst.max(0) as usize).min(z.len());
    let end = (start + n.max(0) as usize).min(z.len());
    z[start..end].to_vec()
}

// ---------------------------------------------------------------------------------------------
// Colunas geradas e cookie do esquema
// ---------------------------------------------------------------------------------------------

/// `sqlite3AddGenerated`: faz da coluna mais recente uma coluna GENERATED ALWAYS AS. O tipo,
/// quando existe, é `VIRTUAL` ou `STORED`.
pub fn add_generated(
    db: &mut Connection,
    parse: &mut Parse,
    p_expr: Option<Box<Expr>>,
    p_type: Option<&Token>,
) {
    let mut p_expr = p_expr;
    let Some(mut p_tab) = parse.p_new_table.take() else {
        // Coluna gerada num CREATE TABLE IF NOT EXISTS de tabela que já existe.
        return;
    };
    'generated_done: {
        if p_tab.n_col < 1 {
            break 'generated_done;
        }
        let i_col = (p_tab.n_col - 1) as usize;
        let mut e_type = COLFLAG_VIRTUAL;
        if parse.in_declare_vtab() {
            error_msg(db, parse, b"virtual tables cannot use computed columns", &[]);
            break 'generated_done;
        }
        let well_formed = 'check: {
            if p_tab.a_col[i_col].i_dflt > 0 {
                break 'check false;
            }
            if let Some(t) = p_type {
                if t.z.len() == 7 && strnicmp(Some(b"virtual"), Some(&t.z), 7) == 0 {
                    // Nada a fazer: VIRTUAL é o padrão.
                } else if t.z.len() == 6 && strnicmp(Some(b"stored"), Some(&t.z), 6) == 0 {
                    e_type = COLFLAG_STORED;
                } else {
                    break 'check false;
                }
            }
            true
        };
        if !well_formed {
            let z_name = p_tab.a_col[i_col].z_cn_name.clone();
            error_msg(
                db,
                parse,
                b"error in generated column \"%s\"",
                &[text_arg(c_str(&z_name))],
            );
            break 'generated_done;
        }
        if e_type == COLFLAG_VIRTUAL {
            p_tab.n_nv_col -= 1;
        }
        p_tab.a_col[i_col].col_flags |= e_type;
        debug_assert!(TF_HAS_VIRTUAL == COLFLAG_VIRTUAL as u32);
        debug_assert!(TF_HAS_STORED == COLFLAG_STORED as u32);
        p_tab.tab_flags |= e_type as u32;
        if (p_tab.a_col[i_col].col_flags & COLFLAG_PRIMKEY) != 0 {
            // Só para a mensagem de erro.
            make_column_part_of_primary_key(db, parse, &mut p_tab.a_col[i_col]);
        }
        if p_expr.as_deref().map_or(false, |e| e.op == TK_ID) {
            // O valor de uma coluna gerada precisa ser uma expressão de verdade, e não só uma
            // referência a outra coluna, para as otimizações de índice de cobertura funcionarem.
            // Se não for uma expressão, vira uma com um "+" unário.
            p_expr = new_p_expr(db, parse, TK_UPLUS as i32, p_expr.take(), None);
        }
        if let Some(e) = p_expr.as_deref_mut() {
            if e.op != TK_RAISE {
                e.aff_expr = p_tab.a_col[i_col].affinity;
            }
        }
        column_set_expr(db, parse, &mut p_tab, i_col, p_expr.take());
    }
    parse.p_new_table = Some(p_tab);
}

/// `sqlite3ChangeCookie`: gera o código que incrementa o cookie do esquema.
///
/// O cookie decide quando o esquema do banco mudou. Depois de cada mudança ele muda; quem lê o
/// esquema o registra, e a cada acesso confere se ele continua o mesmo.
pub fn change_cookie(db: &Connection, parse: &mut Parse, i_db: i32) {
    let cookie = db.dbs[i_db as usize].schema.schema_cookie;
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op3(
            v,
            OP_SETCOOKIE as i32,
            i_db,
            BTREE_SCHEMA_VERSION as i32,
            1u32.wrapping_add(cookie as u32) as i32,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// O texto de um CREATE TABLE gerado
// ---------------------------------------------------------------------------------------------

/// `identLength`: quantos caracteres são precisos para escrever o identificador, aspas
/// incluídas, sem o NUL final. A estimativa é conservadora.
fn ident_length(z: &[u8]) -> i32 {
    let mut n = 0;
    for &c in c_str(z) {
        if c == b'"' {
            n += 1;
        }
        n += 1;
    }
    n + 2
}

/// `identPut`: acrescenta a `out` o identificador `z_ident`. Se ele só tem caracteres
/// alfanuméricos, não começa por dígito e não é palavra-chave SQL, sai como está; senão sai entre
/// aspas duplas (com as aspas internas dobradas).
fn ident_put(out: &mut Vec<u8>, z_ident: &[u8]) {
    let z = c_str(z_ident);
    let mut j = 0usize;
    while j < z.len() {
        if !is_alnum(z[j]) && z[j] != b'_' {
            break;
        }
        j += 1;
    }
    let need_quote = is_digit(at(z, 0))
        || keyword_code(&z[..j]) != TK_ID as i32
        || at(z, j) != 0
        || j == 0;
    if need_quote {
        out.push(b'"');
    }
    for &c in z {
        out.push(c);
        if c == b'"' {
            out.push(b'"');
        }
    }
    if need_quote {
        out.push(b'"');
    }
}

/// `createTableStmt`: gera um comando CREATE TABLE apropriado para a tabela.
fn create_table_stmt(p: &Table) -> Vec<u8> {
    let mut n: i32 = 0;
    for p_col in p.a_col.iter() {
        n += ident_length(col_name(p_col)) + 5;
    }
    n += ident_length(&p.z_name);
    let (mut z_sep, z_sep2, z_end): (&[u8], &[u8], &[u8]) = if n < 50 {
        (&b""[..], &b","[..], &b")"[..])
    } else {
        (&b"\n  "[..], &b",\n  "[..], &b"\n)"[..])
    };
    let mut z_stmt: Vec<u8> = b"CREATE TABLE ".to_vec();
    ident_put(&mut z_stmt, &p.z_name);
    z_stmt.push(b'(');
    for p_col in p.a_col.iter() {
        const AZ_TYPE: [&[u8]; 6] = [
            b"",      // SQLITE_AFF_BLOB
            b" TEXT", // SQLITE_AFF_TEXT
            b" NUM",  // SQLITE_AFF_NUMERIC
            b" INT",  // SQLITE_AFF_INTEGER
            b" REAL", // SQLITE_AFF_REAL
            b" NUM",  // SQLITE_AFF_FLEXNUM
        ];
        z_stmt.extend_from_slice(z_sep);
        z_sep = z_sep2;
        ident_put(&mut z_stmt, col_name(p_col));
        debug_assert!(p_col.affinity >= SQLITE_AFF_BLOB);
        debug_assert!((p_col.affinity - SQLITE_AFF_BLOB) < 6);
        let z_type = AZ_TYPE[(p_col.affinity - SQLITE_AFF_BLOB) as usize];
        debug_assert!(
            p_col.affinity == SQLITE_AFF_BLOB
                || p_col.affinity == SQLITE_AFF_FLEXNUM
                || p_col.affinity == affinity_type(z_type, None)
        );
        z_stmt.extend_from_slice(z_type);
    }
    z_stmt.extend_from_slice(z_end);
    z_stmt
}

// ---------------------------------------------------------------------------------------------
// Índices: tamanhos, colunas repetidas, colunas não indexadas
// ---------------------------------------------------------------------------------------------

/// `resizeIndexObject`: redimensiona o índice para `n` colunas no total. Os vetores guardam
/// exatamente as `n_column` primeiras entradas e o resto vem zerado, como a alocação nova do C.
fn resize_index_object(p_idx: &mut Index, n: usize) {
    if p_idx.n_column as usize >= n {
        return;
    }
    debug_assert!(!p_idx.is_resized);
    let n_column = p_idx.n_column as usize;
    p_idx.az_coll.truncate(n_column);
    p_idx.az_coll.resize(n, Vec::new());
    p_idx.ai_column.truncate(n_column);
    p_idx.ai_column.resize(n, 0);
    p_idx.a_sort_order.truncate(n_column);
    p_idx.a_sort_order.resize(n, 0);
    let n_key_est = p_idx.n_key_col as usize + 1;
    p_idx.ai_row_log_est.truncate(n_key_est);
    p_idx.ai_row_log_est.resize(n.max(n_key_est), 0);
    p_idx.n_column = n as u16;
    p_idx.is_resized = true;
}

/// `estimateTableWidth`: estima a largura total de uma linha da tabela.
fn estimate_table_width(p_tab: &mut Table) {
    let mut w_table: u32 = 0;
    for p_tab_col in p_tab.a_col.iter() {
        w_table += p_tab_col.sz_est as u32;
    }
    if p_tab.i_p_key < 0 {
        w_table += 1;
    }
    p_tab.sz_tab_row = log_est(w_table.wrapping_mul(4) as u64);
}

/// `estimateIndexWidth`: estima o tamanho médio de uma linha do índice. `a_col` são as colunas
/// da tabela dona (o `pIdx->pTable->aCol` do C).
fn estimate_index_width(p_idx: &mut Index, a_col: &[Column]) {
    let mut w_index: u32 = 0;
    for i in 0..p_idx.n_column as usize {
        let x = p_idx.ai_column[i];
        debug_assert!((x as i32) < a_col.len() as i32);
        w_index += if x < 0 { 1 } else { a_col[x as usize].sz_est as u32 };
    }
    p_idx.sz_idx_row = log_est(w_index.wrapping_mul(4) as u64);
}

/// `hasColumn`: verdadeiro se o número de coluna `x` é uma das `n_col` primeiras entradas de
/// `ai_col`. Serve para saber se a coluna `x` aparece nas primeiras colunas de um índice.
fn has_column(ai_col: &[i16], n_col: usize, x: i16) -> bool {
    ai_col[..n_col].iter().any(|&c| c == x)
}

/// `isDupColumn`: verdadeiro se alguma das `n_key` primeiras entradas do índice `p_idx` casa
/// exatamente com a entrada `i_col` de `p_pk`. `p_pk` é sempre o índice da PRIMARY KEY de uma
/// tabela WITHOUT ROWID, e `p_idx` é outro índice da mesma tabela (ou o próprio `p_pk`).
///
/// Difere de `has_column` porque aqui a coluna e a colação precisam casar, e lá só a coluna.
/// As `n_key` primeiras entradas de `p_idx` são colunas comuns, nunca o rowid nem expressão.
fn is_dup_column(p_idx: &Index, n_key: usize, p_pk: &Index, i_col: usize) -> bool {
    debug_assert!(n_key <= p_idx.n_column as usize);
    debug_assert!(p_pk.idx_type == SQLITE_IDXTYPE_PRIMARYKEY);
    let j = p_pk.ai_column[i_col];
    debug_assert!(j != XN_ROWID && j != XN_EXPR);
    for i in 0..n_key {
        debug_assert!(p_idx.ai_column[i] >= 0 || j >= 0);
        if p_idx.ai_column[i] == j && str_icmp(&p_idx.az_coll[i], &p_pk.az_coll[i_col]) == 0 {
            return true;
        }
    }
    false
}

/// `recomputeColumnsNotIndexed`: recalcula `Index.col_not_idxed`, a máscara com 0 para cada
/// coluna indexada entre as 63 primeiras da tabela e 1 para todas as outras (o bit mais alto é
/// sempre 1). Colunas VIRTUAL não contam como cobertas, mesmo que estejam no índice: não se
/// confia na lógica de `whereIndexExprTrans()` para achar todas as referências a elas. A máscara
/// é combinada por AND com `SrcItem.col_used` para saber se o índice é de cobertura. `a_col` são
/// as colunas da tabela dona.
fn recompute_columns_not_indexed(p_idx: &mut Index, a_col: &[Column]) {
    let mut m: Bitmask = 0;
    for j in (0..p_idx.n_column as usize).rev() {
        let x = p_idx.ai_column[j] as i32;
        if x >= 0 && (a_col[x as usize].col_flags & COLFLAG_VIRTUAL) == 0 && x < BMS - 1 {
            m |= 1 << x;
        }
    }
    p_idx.col_not_idxed = !m;
    debug_assert!((p_idx.col_not_idxed >> 63) == 1);
}

// ---------------------------------------------------------------------------------------------
// WITHOUT ROWID
// ---------------------------------------------------------------------------------------------

/// `convertToWithoutRowidTable`: roda no fim de um CREATE TABLE com a cláusula WITHOUT ROWID e
/// converte o esquema em memória e o código do VDBE de tabela com rowid para tabela sem rowid.
///
/// 1. Marca todas as colunas da PRIMARY KEY como NOT NULL (menos nas tabelas impostoras).
/// 2. Troca o P3 do `OP_CreateBtree` de `BTREE_INTKEY` para `BTREE_BLOBKEY`.
/// 3. Pula a criação da entrada de sqlite_schema da PRIMARY KEY: o índice dela é identificado
///    pela entrada da própria tabela.
/// 4. Faz `Index.tnum` do índice da PRIMARY KEY ser a página raiz da tabela.
/// 5. Acrescenta todas as colunas da tabela ao índice da PRIMARY KEY, para ele ser de cobertura.
///    As colunas a mais fazem parte de `KeyInfo.nAllField` e não servem para ordenar, buscar nem
///    checar unicidade.
/// 6. Troca o rowid no fim de todos os índices UNIQUE automáticos pelas colunas da PRIMARY KEY.
///
/// Em tabelas virtuais só o passo 1 vale. A tabela em construção sai de `parse.p_new_table`
/// durante a chamada recursiva a `create_index` e volta para `p` antes de a função seguir.
fn convert_to_without_rowid_table(db: &mut Connection, parse: &mut Parse, p: &mut Box<Table>) {
    // Marca toda coluna da PRIMARY KEY como NOT NULL (menos nas tabelas impostoras).
    if !db.init.imposter_table {
        for p_col in p.a_col.iter_mut() {
            if (p_col.col_flags & COLFLAG_PRIMKEY) != 0 && p_col.not_null == OE_NONE {
                p_col.not_null = OE_ABORT;
            }
        }
        p.tab_flags |= TF_HAS_NOT_NULL;
    }

    // Troca o P3 do OP_CreateBtree de BTREE_INTKEY para BTREE_BLOBKEY.
    debug_assert!(parse.b_returning == 0);
    let addr_cr_tab = parse.addr_cr_tab;
    if addr_cr_tab != 0 {
        debug_assert!(parse.p_vdbe.is_some());
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            change_p3(v, addr_cr_tab, BTREE_BLOBKEY as i32);
        }
    }

    // Acha o índice da PRIMARY KEY. Se a tabela era uma INTEGER PRIMARY KEY, cria o índice.
    let pk_pos: usize;
    if p.i_p_key >= 0 {
        let tok = Token { z: col_name(&p.a_col[p.i_p_key as usize]).to_vec(), i_ofst: -1 };
        let mut p_list = expr_list_append(None, expr_alloc(TK_ID as i32, Some(&tok), 0));
        if parse.in_rename_object() {
            let to = p_list
                .as_deref()
                .and_then(|l| l.a[0].p_expr.as_deref())
                .map_or(0, |e| e as *const Expr as usize);
            rename_token_remap(parse, to, &p.i_p_key as *const i16 as usize);
        }
        if let Some(l) = p_list.as_deref_mut() {
            l.a[0].fg.sort_flags = parse.i_pk_sort_order;
        }
        debug_assert!(parse.p_new_table.is_none());
        p.i_p_key = -1;
        let key_conf = p.key_conf as i32;
        parse.p_new_table = Some(std::mem::take(p));
        create_index(
            db,
            parse,
            None,
            None,
            None,
            p_list,
            key_conf,
            None,
            None,
            0,
            0,
            SQLITE_IDXTYPE_PRIMARYKEY,
        );
        if let Some(t) = parse.p_new_table.take() {
            *p = t;
        }
        if parse.n_err != 0 {
            p.tab_flags &= !TF_WITHOUT_ROWID;
            return;
        }
        debug_assert!(db.malloc_failed == 0);
        let Some(pos) = p.p_index.iter().position(|x| x.is_primary_key_index()) else {
            return;
        };
        debug_assert!(p.p_index[pos].n_key_col == 1);
        pk_pos = pos;
    } else {
        let Some(pos) = p.p_index.iter().position(|x| x.is_primary_key_index()) else {
            return;
        };

        // Tira da PRIMARY KEY as colunas redundantes: "PRIMARY KEY(a,b,a,b,c,b,c,d)" vira
        // "PRIMARY KEY(a,b,c,d)". O código que vem depois supõe que a chave não repete colunas.
        let p_pk = Rc::make_mut(&mut p.p_index[pos]);
        let mut j = 1usize;
        for i in 1..p_pk.n_key_col as usize {
            if is_dup_column(p_pk, j, p_pk, i) {
                p_pk.n_column -= 1;
            } else {
                let z_coll = p_pk.az_coll[i].clone();
                p_pk.az_coll[j] = z_coll;
                p_pk.a_sort_order[j] = p_pk.a_sort_order[i];
                p_pk.ai_column[j] = p_pk.ai_column[i];
                j += 1;
            }
        }
        p_pk.n_key_col = j as u16;
        pk_pos = pos;
    }

    let imposter = db.init.imposter_table;
    let tab_tnum = p.tnum;
    let n_pk: usize;
    {
        let p_pk = Rc::make_mut(&mut p.p_index[pk_pos]);
        p_pk.is_covering = true;
        if !imposter {
            p_pk.uniq_not_null = true;
        }
        p_pk.n_column = p_pk.n_key_col;
        n_pk = p_pk.n_column as usize;

        // Pula a criação da árvore da PRIMARY KEY e da entrada dela em sqlite_schema. Só é
        // preciso quando se gera o código de um CREATE TABLE (e não ao ler o esquema).
        if p_pk.tnum > 0 {
            if let Some(v) = parse.p_vdbe.as_deref_mut() {
                debug_assert!(db.init.busy == 0);
                change_opcode(v, p_pk.tnum as i32, OP_GOTO);
            }
        }

        // A página raiz da PRIMARY KEY é a da tabela.
        p_pk.tnum = tab_tnum;
    }

    // Troca nos índices UNIQUE da memória o rowid final por uma ou mais colunas da PRIMARY KEY.
    let p_pk = p.p_index[pk_pos].clone();
    for k in 0..p.p_index.len() {
        if p.p_index[k].is_primary_key_index() {
            continue;
        }
        let n_key_col = p.p_index[k].n_key_col as usize;
        let mut n = 0usize;
        for i in 0..n_pk {
            if !is_dup_column(&p.p_index[k], n_key_col, &p_pk, i) {
                n += 1;
            }
        }
        let p_idx = Rc::make_mut(&mut p.p_index[k]);
        if n == 0 {
            // Este índice é um superconjunto da PRIMARY KEY.
            p_idx.n_column = p_idx.n_key_col;
            continue;
        }
        resize_index_object(p_idx, n_key_col + n);
        let mut j = n_key_col;
        for i in 0..n_pk {
            if !is_dup_column(p_idx, n_key_col, &p_pk, i) {
                p_idx.ai_column[j] = p_pk.ai_column[i];
                p_idx.az_coll[j] = p_pk.az_coll[i].clone();
                if p_pk.a_sort_order[i] != 0 {
                    // Ver https://www.sqlite.org/src/info/bba7b69f9849b5bf
                    p_idx.b_asc_key_bug = true;
                }
                j += 1;
            }
        }
        debug_assert!(p_idx.n_column as usize >= n_key_col + n);
        debug_assert!(p_idx.n_column as usize >= j);
    }
    drop(p_pk);

    // Acrescenta todas as colunas da tabela ao índice da PRIMARY KEY.
    let n_col = p.n_col as usize;
    let Table { a_col, p_index, n_nv_col, .. } = &mut **p;
    let p_pk = Rc::make_mut(&mut p_index[pk_pos]);
    let mut n_extra = 0usize;
    for i in 0..n_col {
        if !has_column(&p_pk.ai_column, n_pk, i as i16)
            && (a_col[i].col_flags & COLFLAG_VIRTUAL) == 0
        {
            n_extra += 1;
        }
    }
    resize_index_object(p_pk, n_pk + n_extra);
    let mut j = n_pk;
    for i in 0..n_col {
        if !has_column(&p_pk.ai_column, j, i as i16) && (a_col[i].col_flags & COLFLAG_VIRTUAL) == 0
        {
            debug_assert!(j < p_pk.n_column as usize);
            p_pk.ai_column[j] = i as i16;
            p_pk.az_coll[j] = STR_BINARY.as_bytes().to_vec();
            j += 1;
        }
    }
    debug_assert!(p_pk.n_column as usize == j);
    debug_assert!(*n_nv_col as usize <= j);
    recompute_columns_not_indexed(p_pk, a_col);
}

// ---------------------------------------------------------------------------------------------
// Tabelas sombra
// ---------------------------------------------------------------------------------------------

/// `sqlite3IsShadowTableOf`: verdadeiro se `p_tab` é uma tabela virtual e `z_name` é o nome de
/// uma tabela sombra dela.
pub fn is_shadow_table_of(db: &Connection, p_tab: &Table, z_name: &[u8]) -> bool {
    if !p_tab.is_virtual() {
        return false;
    }
    let n_name = strlen30(&p_tab.z_name) as usize;
    if strnicmp(Some(z_name), Some(&p_tab.z_name), n_name as i32) != 0 {
        return false;
    }
    if at(z_name, n_name) != b'_' {
        return false;
    }
    let Some(z_mod) = p_tab.u_vtab().and_then(|v| v.az_arg.first()) else {
        return false;
    };
    let Some(p_mod) = hash_find(&db.a_module, z_mod) else {
        return false;
    };
    if p_mod.p_module.i_version() < 3 {
        return false;
    }
    p_mod.p_module.x_shadow_name(z_name.get(n_name + 1..).unwrap_or(&[]))
}

/// `sqlite3MarkAllShadowTablesOf`: `p_tab` é uma tabela virtual. Se a implementação do módulo
/// existe e tem `xShadowName`, percorre as outras tabelas comuns do mesmo esquema atrás das
/// sombras de `p_tab` e liga `TF_Shadow` em cada uma que achar.
pub fn mark_all_shadow_tables_of(db: &mut Connection, p_tab: &Table) {
    debug_assert!(p_tab.is_virtual());
    let Some(z_mod) = p_tab.u_vtab().and_then(|v| v.az_arg.first()) else {
        return;
    };
    let Some(p_mod) = hash_find(&db.a_module, z_mod).cloned() else {
        return;
    };
    if p_mod.p_module.i_version() < 3 {
        return;
    }
    let n_name = strlen30(&p_tab.z_name) as usize;
    let i_db = schema_to_index(db, p_tab.p_schema);
    if i_db < 0 {
        return;
    }
    let tbl_hash = &mut db.dbs[i_db as usize].schema.tbl_hash;
    let mut k = hash_first(tbl_hash);
    while let Some(e) = k {
        k = hash_next(tbl_hash, e);
        let p_other = hash_data_mut(tbl_hash, e);
        if !p_other.is_ordinary_table() || (p_other.tab_flags & TF_SHADOW) != 0 {
            continue;
        }
        if strnicmp(Some(&p_other.z_name), Some(&p_tab.z_name), n_name as i32) == 0
            && at(&p_other.z_name, n_name) == b'_'
            && p_mod.p_module.x_shadow_name(p_other.z_name.get(n_name + 1..).unwrap_or(&[]))
        {
            Rc::make_mut(p_other).tab_flags |= TF_SHADOW;
        }
    }
}

/// `sqlite3ShadowTableName`: verdadeiro se `z_name` é o nome de uma tabela sombra na conexão
/// atual: o que vem antes do último "_" nomeia uma tabela virtual da qual `z_name` é sombra.
pub fn shadow_table_name(db: &Connection, z_name: &[u8]) -> bool {
    let z = c_str(z_name);
    let Some(pos) = z.iter().rposition(|&c| c == b'_') else {
        return false;
    };
    let Some(p_tab) = find_table(db, &z[..pos], None) else {
        return false;
    };
    if !p_tab.is_virtual() {
        return false;
    }
    is_shadow_table_of(db, &p_tab, z)
}

// ---------------------------------------------------------------------------------------------
// sqlite3EndTable
// ---------------------------------------------------------------------------------------------

/// Tira da lista de DEFAULT da tabela a expressão do slot `i_dflt` (1-based; 0 é "sem
/// expressão"), para a resolução de nomes poder emprestar a tabela inteira.
fn take_dflt_expr(p_tab: &mut Table, i_dflt: usize) -> Option<Box<Expr>> {
    if i_dflt == 0 {
        return None;
    }
    let TableU::Tab(info) = &mut p_tab.u else {
        return None;
    };
    info.p_dflt_list.as_deref_mut()?.a.get_mut(i_dflt - 1)?.p_expr.take()
}

/// Devolve ao slot `i_dflt` a expressão tirada por `take_dflt_expr`.
fn put_dflt_expr(p_tab: &mut Table, i_dflt: usize, p_expr: Option<Box<Expr>>) {
    if i_dflt == 0 {
        return;
    }
    if let TableU::Tab(info) = &mut p_tab.u {
        if let Some(item) = info.p_dflt_list.as_deref_mut().and_then(|l| l.a.get_mut(i_dflt - 1)) {
            item.p_expr = p_expr;
        }
    }
}

/// `sqlite3EndTable`: chamada para informar o ")" final que termina um CREATE TABLE.
///
/// A tabela que as outras ações vinham montando entra nas tabelas hash do esquema, se não houve
/// erro. Uma entrada da tabela vai para sqlite_schema no disco, a não ser que ela seja
/// temporária ou que `db.init.busy` seja 1. Com `db.init.busy==1` o SQL está sendo lido do
/// sqlite_schema (porque acabamos de conectar ou porque ele mudou) e a entrada já existe.
///
/// `p_select` não nulo quer dizer "CREATE TABLE ... AS SELECT ...": os nomes das colunas da
/// tabela nova são os do resultado do SELECT. `p_cons` é o "," depois da última coluna e `p_end`
/// o ")" antes das opções; `tab_opts` são as opções da tabela (`TF_Strict`, `TF_WithoutRowid`).
pub fn end_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_cons: Option<&Token>,
    p_end: Option<&Token>,
    tab_opts: u32,
    p_select: Option<&mut Select>,
) {
    if p_end.is_none() && p_select.is_none() {
        return;
    }
    let Some(p) = parse.p_new_table.take() else {
        return;
    };
    parse.p_new_table = end_table_body(db, parse, p, p_cons, p_end, tab_opts, p_select);
}

/// O corpo de `sqlite3EndTable`, com a tabela em construção fora do `Parse`. Devolve `Some` com
/// a tabela quando ela deve continuar em `parse.p_new_table` (todos os `return` do C, em que
/// `pNewTable` fica como estava) e `None` quando ela passou para o esquema.
fn end_table_body(
    db: &mut Connection,
    parse: &mut Parse,
    mut p: Box<Table>,
    p_cons: Option<&Token>,
    p_end: Option<&Token>,
    tab_opts: u32,
    p_select: Option<&mut Select>,
) -> Option<Box<Table>> {
    // O VDBE do `Parse` (existe depois de `get_vdbe`), ou volta com a tabela se sumiu.
    macro_rules! vdbe {
        () => {
            match parse.p_vdbe.as_deref_mut() {
                Some(v) => v,
                None => return Some(p),
            }
        };
    }

    let has_select = p_select.is_some();
    if !has_select && shadow_table_name(db, &p.z_name) {
        p.tab_flags |= TF_SHADOW;
    }

    // Se `db.init.busy` é 1, o SQL vem de sqlite_schema ou sqlite_temp_schema no disco: não se
    // escreve de novo. A página raiz da tabela vem de `db.init.new_tnum` (posta lá pelo
    // callback de abertura). Se a página raiz é 1, é a própria sqlite_schema: só leitura.
    if db.init.busy != 0 {
        if has_select || (!p.is_ordinary_table() && db.init.new_tnum != 0) {
            error_msg(db, parse, b"", &[]);
            return Some(p);
        }
        p.tnum = db.init.new_tnum;
        if p.tnum == 1 {
            p.tab_flags |= TF_READONLY;
        }
    }

    // Tratamento especial das tabelas com STRICT: não há tipos de coluna customizados (todo
    // tipo é INT, INTEGER, REAL, TEXT ou BLOB, ou ANY) e as colunas de uma PRIMARY KEY que não
    // seja a INTEGER PRIMARY KEY precisam de NOT NULL.
    if (tab_opts & TF_STRICT) != 0 {
        p.tab_flags |= TF_STRICT;
        for ii in 0..p.n_col as usize {
            let e_c_type = p.a_col[ii].e_c_type;
            if e_c_type == COLTYPE_CUSTOM {
                let z_tab = p.z_name.clone();
                let z_col = col_name(&p.a_col[ii]).to_vec();
                if (p.a_col[ii].col_flags & COLFLAG_HASTYPE) != 0 {
                    let z_type = column_type(&p.a_col[ii], b"").to_vec();
                    error_msg(
                        db,
                        parse,
                        b"unknown datatype for %s.%s: \"%s\"",
                        &[text_arg(&z_tab), text_arg(&z_col), text_arg(&z_type)],
                    );
                } else {
                    error_msg(
                        db,
                        parse,
                        b"missing datatype for %s.%s",
                        &[text_arg(&z_tab), text_arg(&z_col)],
                    );
                }
                return Some(p);
            } else if e_c_type == COLTYPE_ANY {
                p.a_col[ii].affinity = SQLITE_AFF_BLOB;
            }
            if (p.a_col[ii].col_flags & COLFLAG_PRIMKEY) != 0
                && p.i_p_key as i32 != ii as i32
                && p.a_col[ii].not_null == OE_NONE
            {
                p.a_col[ii].not_null = OE_ABORT;
                p.tab_flags |= TF_HAS_NOT_NULL;
            }
        }
    }

    debug_assert!(
        (p.tab_flags & TF_HAS_PRIMARY_KEY) == 0
            || p.i_p_key >= 0
            || primary_key_index(&p).is_some()
    );
    debug_assert!(
        (p.tab_flags & TF_HAS_PRIMARY_KEY) != 0
            || (p.i_p_key < 0 && primary_key_index(&p).is_none())
    );

    // Tratamento especial das tabelas WITHOUT ROWID.
    if (tab_opts & TF_WITHOUT_ROWID) != 0 {
        if (p.tab_flags & TF_AUTOINCREMENT) != 0 {
            error_msg(db, parse, b"AUTOINCREMENT not allowed on WITHOUT ROWID tables", &[]);
            return Some(p);
        }
        if (p.tab_flags & TF_HAS_PRIMARY_KEY) == 0 {
            error_msg(db, parse, b"PRIMARY KEY missing on table %s", &[text_arg(&p.z_name)]);
            return Some(p);
        }
        p.tab_flags |= TF_WITHOUT_ROWID | TF_NO_VISIBLE_ROWID;
        convert_to_without_rowid_table(db, parse, &mut p);
    }
    let i_db = schema_to_index(db, p.p_schema);

    // Resolve os nomes em todas as restrições CHECK.
    if p.p_check.is_some() {
        let mut p_check = p.p_check.take();
        resolve_self_reference(db, parse, Some(&p), NC_ISCHECK, None, p_check.as_deref_mut());
        if parse.n_err == 0 {
            p.p_check = p_check;
        }
        // Com erro as restrições CHECK são apagadas já: senão poderiam ser usadas de verdade se
        // PRAGMA writable_schema=ON.
    }

    if (p.tab_flags & TF_HAS_GENERATED) != 0 {
        let mut n_ng = 0;
        for ii in 0..p.n_col as usize {
            let col_flags = p.a_col[ii].col_flags;
            if (col_flags & COLFLAG_GENERATED) != 0 {
                let i_dflt = p.a_col[ii].i_dflt as usize;
                let mut p_x = take_dflt_expr(&mut p, i_dflt);
                if resolve_self_reference(db, parse, Some(&p), NC_GENCOL, p_x.as_deref_mut(), None) != 0 {
                    // Com erro na resolução a expressão vira NULL: assim os geradores de código
                    // não inserem partes extras numa árvore que mora no esquema.
                    column_set_expr(db, parse, &mut p, ii, expr_alloc(TK_NULL as i32, None, 0));
                } else {
                    put_dflt_expr(&mut p, i_dflt, p_x);
                }
            } else {
                n_ng += 1;
            }
        }
        if n_ng == 0 {
            error_msg(db, parse, b"must have at least one non-generated column", &[]);
            return Some(p);
        }
    }

    // Estima a largura média da linha da tabela e de todos os índices implícitos.
    estimate_table_width(&mut p);
    {
        let Table { a_col, p_index, .. } = &mut *p;
        for p_idx in p_index.iter_mut() {
            estimate_index_width(Rc::make_mut(p_idx), a_col);
        }
    }

    // Fora da leitura do esquema, cria o registro da tabela no sqlite_schema do banco (no
    // arquivo auxiliar, se TEMPORARY).
    if db.init.busy == 0 {
        get_vdbe(db, parse);
        add_op1(vdbe!(), OP_CLOSE as i32, 0);

        // `z_type` para a view ou tabela nova.
        let (z_type, z_type2): (&[u8], &[u8]) = if p.is_ordinary_table() {
            (&b"table"[..], &b"TABLE"[..])
        } else {
            (&b"view"[..], &b"VIEW"[..])
        };

        // Em CREATE TABLE xx AS SELECT ..., executa o SELECT para encher a tabela. A página
        // raiz da tabela nova está no registrador `parse.reg_root`. Depois que `select` gerou o
        // código, ele está em condição de dar os nomes e tipos das colunas.
        //
        // Não é preciso trava de escrita de cache compartilhado para escrever na tabela nova:
        // criá-la já exigiu uma trava de esquema, que exclui todos os outros usuários.
        if let Some(p_select) = p_select {
            if parse.in_special_parse() {
                parse.rc = SQLITE_ERROR;
                parse.n_err += 1;
                return Some(p);
            }
            let i_csr = parse.n_tab;
            parse.n_tab += 1;
            parse.n_mem += 1;
            let reg_yield = parse.n_mem;
            parse.n_mem += 1;
            let reg_rec = parse.n_mem;
            parse.n_mem += 1;
            let reg_rowid = parse.n_mem;
            may_abort(parse);
            let reg_root = parse.reg_root;
            add_op3(vdbe!(), OP_OPENWRITE as i32, i_csr, reg_root, i_db);
            change_p5(vdbe!(), OPFLAG_P2ISREG);
            let addr_top = vdbe!().n_op() + 1;
            add_op3(vdbe!(), OP_INITCOROUTINE as i32, reg_yield, 0, addr_top);
            if parse.n_err != 0 {
                return Some(p);
            }
            let Some(mut p_sel_tab) =
                result_set_of_select(db, parse, p_select, SQLITE_AFF_BLOB)
            else {
                return Some(p);
            };
            debug_assert!(p.a_col.is_empty());
            p.n_col = p_sel_tab.n_col;
            p.n_nv_col = p_sel_tab.n_col;
            p.a_col = std::mem::take(&mut p_sel_tab.a_col);
            drop(p_sel_tab);
            let mut dest = SelectDest::default();
            select_dest_init(&mut dest, SRT_COROUTINE as i32, reg_yield);
            select(db, parse, p_select, &mut dest);
            if parse.n_err != 0 {
                return Some(p);
            }
            end_coroutine(parse, reg_yield);
            jump_here(vdbe!(), addr_top - 1);
            let addr_ins_loop = add_op1(vdbe!(), OP_YIELD as i32, dest.i_sd_parm);
            add_op3(vdbe!(), OP_MAKERECORD as i32, dest.i_sdst, dest.n_sdst, reg_rec);
            table_affinity(vdbe!(), &Rc::new((*p).clone()), 0);
            add_op2(vdbe!(), OP_NEWROWID as i32, i_csr, reg_rowid);
            add_op3(vdbe!(), OP_INSERT as i32, i_csr, reg_rec, reg_rowid);
            vdbe_goto(vdbe!(), addr_ins_loop);
            jump_here(vdbe!(), addr_ins_loop);
            add_op1(vdbe!(), OP_CLOSE as i32, i_csr);
        }

        // O texto completo do comando CREATE.
        let z_stmt: Option<Vec<u8>> = if has_select {
            Some(create_table_stmt(&p))
        } else {
            let p_end2: Token = if tab_opts != 0 {
                parse.s_last_token.clone()
            } else {
                p_end.cloned().unwrap_or_default()
            };
            let mut n = p_end2.i_ofst - parse.s_name_token.i_ofst;
            if at(&p_end2.z, 0) != b';' {
                n += p_end2.z.len() as i32;
            }
            let mut z = b"CREATE ".to_vec();
            z.extend_from_slice(z_type2);
            z.push(b' ');
            z.extend_from_slice(&sql_span(parse, parse.s_name_token.i_ofst, n));
            Some(z)
        };

        // O espaço do registro já foi reservado em sqlite_schema. Falta preenchê-lo com tudo o
        // que se coletou.
        let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
        let reg_root = parse.reg_root;
        let reg_rowid = parse.reg_rowid;
        let fmt: Vec<u8> = [
            b"UPDATE %Q.".as_slice(),
            LEGACY_SCHEMA_TABLE,
            b" SET type='%s', name=%Q, tbl_name=%Q, rootpage=#%d, sql=%Q WHERE rowid=#%d"
                .as_slice(),
        ]
        .concat();
        nested_parse(
            db,
            parse,
            &fmt,
            &[
                text_arg(&z_db_name),
                text_arg(z_type),
                text_arg(&p.z_name),
                text_arg(&p.z_name),
                PrintfArg::Int(reg_root as i64),
                PrintfArg::Text(z_stmt),
                PrintfArg::Int(reg_rowid as i64),
            ],
        );
        change_cookie(db, parse, i_db);

        // Vê se é preciso criar a tabela sqlite_sequence, que guarda as chaves AUTOINCREMENT.
        if (p.tab_flags & TF_AUTOINCREMENT) != 0
            && !parse.in_special_parse()
            && db.dbs[i_db as usize].schema.p_seq_tab.is_none()
        {
            nested_parse(
                db,
                parse,
                b"CREATE TABLE %Q.sqlite_sequence(name,seq)",
                &[text_arg(&z_db_name)],
            );
        }

        // Relê tudo para atualizar as estruturas internas.
        let z_where = mprintf(b"tbl_name='%q' AND type!='trigger'", &[text_arg(&p.z_name)]);
        add_parse_schema_op(parse, db, i_db, z_where, 0);

        // Procura ciclos em colunas geradas e expressões ilegais em CHECK e DEFAULT.
        if (p.tab_flags & TF_HAS_GENERATED) != 0 {
            let z_sql = mprintf(
                b"SELECT*FROM\"%w\".\"%w\"",
                &[text_arg(&z_db_name), text_arg(&p.z_name)],
            );
            add_op4(
                vdbe!(),
                OP_SQLEXEC as i32,
                0x0001,
                0,
                0,
                z_sql.map_or(P4::None, P4::Text),
            );
        }
    }

    // Onde ALTER TABLE ADD COLUMN insere o texto de uma coluna nova.
    if !has_select && p.is_ordinary_table() {
        debug_assert!(p_cons.is_some() && p_end.is_some());
        let p_cons = match p_cons {
            Some(c) if !c.z.is_empty() => Some(c),
            _ => p_end,
        };
        if let (Some(c), TableU::Tab(info)) = (p_cons, &mut p.u) {
            info.add_col_offset = 13 + (c.i_ofst - parse.s_name_token.i_ofst);
        }
    }

    // Põe a tabela na representação em memória do banco.
    if db.init.busy != 0 {
        debug_assert!(p.has_rowid() || p.i_p_key < 0);
        let z_name = p.z_name.clone();
        let p_tab = Rc::new(*p);
        hash_insert(
            &mut db.dbs[i_db as usize].schema.tbl_hash,
            &z_name,
            Some(Rc::clone(&p_tab)),
        );
        db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;

        // A tabela mágica sqlite_sequence do AUTOINCREMENT fica registrada no esquema, para o
        // INSERT achá-la fácil.
        debug_assert!(parse.nested == 0);
        if z_name.as_slice() == b"sqlite_sequence" {
            db.dbs[i_db as usize].schema.p_seq_tab = Some(p_tab);
        }
        return None;
    }
    Some(p)
}

// ---------------------------------------------------------------------------------------------
// CREATE VIEW
// ---------------------------------------------------------------------------------------------

/// `sqlite3CreateView`: o analisador a chama para criar uma VIEW nova. `p_begin` é o token
/// CREATE que começa o comando, `p_cnames` a lista opcional de nomes de coluna e `p_select` o
/// SELECT que vira a view.
#[allow(clippy::too_many_arguments)]
pub fn create_view(
    db: &mut Connection,
    parse: &mut Parse,
    p_begin: &Token,
    p_name1: &Token,
    p_name2: &Token,
    p_cnames: Option<Box<ExprList>>,
    p_select: Option<Box<Select>>,
    is_temp: i32,
    no_err: i32,
) {
    let mut p_select = p_select;
    'create_view_fail: {
        if parse.n_var > 0 {
            error_msg(db, parse, b"parameters are not allowed in views", &[]);
            break 'create_view_fail;
        }
        start_table(db, parse, p_name1, p_name2, is_temp, 1, 0, no_err);
        if parse.p_new_table.is_none() || parse.n_err != 0 {
            break 'create_view_fail;
        }

        // Versões antigas do SQLite aceitavam a coluna mágica "rowid" numa view, embora views
        // não tenham rowid. Esta flag corrige o problema (a opção `SQLITE_ALLOW_ROWID_IN_VIEW`
        // que o desligaria não existe no Debian).
        with_new_table(parse, |_, p| p.tab_flags |= TF_NO_VISIBLE_ROWID);

        let Some((_, p_name)) = two_part_name(db, parse, p_name1, p_name2) else {
            break 'create_view_fail;
        };
        let p_schema = parse.p_new_table.as_deref().map_or(SchemaId(0), |p| p.p_schema);
        let i_db = schema_to_index(db, p_schema);
        let mut s_fix = fix_init(db, i_db, "view", p_name);
        let Some(sel) = p_select.as_deref_mut() else {
            break 'create_view_fail;
        };
        if fix_select(db, parse, &mut s_fix, sel) != 0 {
            break 'create_view_fail;
        }

        // Faz uma cópia de todo o SELECT que define a view. Isso força todos os `Expr.u.z_token`
        // a serem alocados, e não apontarem para o texto de entrada: eles persistem depois que a
        // chamada atual de sqlite3_exec() volta.
        sel.sel_flags |= SF_VIEW;
        let p_view_select = if parse.in_rename_object() {
            p_select.take()
        } else {
            select_dup(p_select.as_deref(), EXPRDUP_REDUCE)
        };
        let p_check = expr_list_dup(p_cnames.as_deref(), EXPRDUP_REDUCE);
        with_new_table(parse, |_, p| {
            p.u = TableU::View(ViewInfo { p_select: p_view_select });
            p.p_check = p_check;
            p.e_tab_type = TABTYP_VIEW;
        });

        // Acha o fim do CREATE VIEW: `s_end` aponta para o último caractere que não é espaço.
        let last = parse.s_last_token.clone();
        let end = last.i_ofst + if at(&last.z, 0) != b';' { last.z.len() as i32 } else { 0 };
        let mut n = end - p_begin.i_ofst;
        debug_assert!(n > 0);
        let z = sql_span(parse, p_begin.i_ofst, n);
        while n > 0 && is_space(at(&z, (n - 1) as usize)) {
            n -= 1;
        }
        let s_end = Token {
            z: vec![at(&z, (n - 1).max(0) as usize)],
            i_ofst: p_begin.i_ofst + n - 1,
        };

        // Usa sqlite3EndTable() para pôr a view na tabela de esquema.
        end_table(db, parse, None, Some(&s_end), 0, None);
    }
    if parse.in_rename_object() {
        rename_exprlist_unmap(parse, p_cnames.as_deref());
    }
}

/// Grava no esquema (na tabela hash do banco `i_db`) o `Rc<Table>` atual da view. O C altera a
/// `Table` no lugar e todo mundo a vê; aqui a cópia que `Rc::make_mut` fez precisa substituir a
/// entrada da tabela hash para o resto da conexão enxergá-la.
pub(crate) fn publish_view(db: &mut Connection, i_db: i32, p_tab: &Rc<Table>) {
    if i_db < 0 {
        return;
    }
    if let Some(slot) = hash_find_mut(&mut db.dbs[i_db as usize].schema.tbl_hash, &p_tab.z_name) {
        *slot = Rc::clone(p_tab);
    }
}

/// `viewGetColumnNames`: a tabela é na verdade uma VIEW (ou uma tabela virtual). Preenche os
/// nomes das colunas da view em `p_table` e devolve diferente de zero se houve erros (a
/// mensagem fica em `parse`).
///
/// O C altera a `Table` do esquema no lugar. Aqui `p_table` é o `Rc` do chamador: a função o
/// altera com `Rc::make_mut` e grava o resultado de volta na tabela hash do esquema (também no
/// marcador `n_col == -1` que detecta views circulares), então depois da chamada `p_table` é a
/// tabela atualizada.
fn view_get_column_names_slow(
    db: &mut Connection,
    parse: &mut Parse,
    p_table: &mut Rc<Table>,
) -> i32 {
    let mut n_err = 0;

    if p_table.is_virtual() {
        db.n_schema_lock += 1;
        let rc = vtab_call_connect(db, parse, p_table);
        db.n_schema_lock -= 1;
        return rc;
    }

    // `n_col` positivo quer dizer que os nomes das colunas da view já são conhecidos; esta
    // rotina só é chamada se a tabela é virtual ou `n_col` é zero.
    debug_assert!(p_table.n_col <= 0);

    // `n_col` negativo é um marcador: estamos calculando os nomes das colunas. Se entramos aqui
    // com ele negativo, duas ou mais views formam um laço, como em:
    //
    //     CREATE TABLE main.ex1(a);
    //     CREATE TEMP VIEW ex1 AS SELECT a FROM ex1;
    //     SELECT * FROM temp.ex1;
    if p_table.n_col < 0 {
        error_msg(db, parse, b"view %s is circularly defined", &[text_arg(&p_table.z_name)]);
        return 1;
    }

    // `result_set_of_select` expande os "*" do resultado e dá cursores aos itens do FROM, mas
    // essas mudanças não podem ser permanentes: o cálculo roda numa cópia do SELECT da view.
    debug_assert!(p_table.is_view());
    let i_db = schema_to_index(db, p_table.p_schema);
    let mut p_sel = select_dup(p_table.u_view().and_then(|v| v.p_select.as_deref()), 0);
    if let Some(sel) = p_sel.as_deref_mut() {
        let e_parse_mode = parse.e_parse_mode;
        let n_tab = parse.n_tab;
        let n_select = parse.n_select;
        parse.e_parse_mode = PARSE_MODE_NORMAL;
        if let Some(src) = sel.p_src.as_deref_mut() {
            src_list_assign_cursors(parse, src);
        }
        Rc::make_mut(p_table).n_col = -1;
        publish_view(db, i_db, p_table);
        let x_auth = db.x_auth.take();
        let p_sel_tab = result_set_of_select(db, parse, sel, SQLITE_AFF_NONE);
        db.x_auth = x_auth;
        parse.n_tab = n_tab;
        parse.n_select = n_select;
        let t = Rc::make_mut(p_table);
        match p_sel_tab {
            None => {
                t.n_col = 0;
                n_err += 1;
            }
            Some(mut p_sel_tab) => {
                if t.p_check.is_some() {
                    // CREATE VIEW nome(lista) AS ...: os nomes das colunas vêm da lista, que
                    // fica em `p_check` (nas tabelas comuns guarda as restrições CHECK).
                    let p_check = t.p_check.take();
                    columns_from_expr_list(
                        db,
                        parse,
                        p_check.as_deref(),
                        &mut t.n_col,
                        &mut t.a_col,
                    );
                    t.p_check = p_check;
                    let n_expr = sel.p_e_list.as_deref().map_or(0, |l| l.a.len());
                    if parse.n_err == 0 && t.n_col as usize == n_expr {
                        debug_assert!(db.malloc_failed == 0);
                        subquery_column_types(db, parse, t, sel, SQLITE_AFF_NONE);
                    }
                } else {
                    // CREATE VIEW nome AS ... sem lista: os nomes vêm do SELECT da view.
                    debug_assert!(t.a_col.is_empty());
                    t.n_col = p_sel_tab.n_col;
                    t.a_col = std::mem::take(&mut p_sel_tab.a_col);
                    t.tab_flags |= p_sel_tab.tab_flags & COLFLAG_NOINSERT as u32;
                }
            }
        }
        t.n_nv_col = t.n_col;
        parse.e_parse_mode = e_parse_mode;
    } else {
        n_err += 1;
    }
    if i_db >= 0 {
        db.db_set_property(i_db as usize, DB_UNRESETVIEWS);
    }
    publish_view(db, i_db, p_table);
    n_err + parse.n_err
}

/// `sqlite3ViewGetColumnNames`: ver `view_get_column_names_slow`. Não faz nada se a view já tem
/// as colunas (`n_col > 0`).
pub fn view_get_column_names(
    db: &mut Connection,
    parse: &mut Parse,
    p_table: &mut Rc<Table>,
) -> i32 {
    if !p_table.is_virtual() && p_table.n_col > 0 {
        return 0;
    }
    view_get_column_names_slow(db, parse, p_table)
}

/// `sqliteViewResetAll`: apaga os nomes de coluna de toda VIEW do banco `idx`.
fn sqlite_view_reset_all(db: &mut Connection, idx: usize) {
    if !db.db_has_property(idx, DB_UNRESETVIEWS) {
        return;
    }
    let tbl_hash = &mut db.dbs[idx].schema.tbl_hash;
    let mut i = hash_first(tbl_hash);
    while let Some(e) = i {
        i = hash_next(tbl_hash, e);
        let p_tab = hash_data_mut(tbl_hash, e);
        if p_tab.is_view() {
            delete_column_names(Rc::make_mut(p_tab));
        }
    }
    db.db_clear_property(idx, DB_UNRESETVIEWS);
}

// ---------------------------------------------------------------------------------------------
// Páginas raiz, DROP TABLE
// ---------------------------------------------------------------------------------------------

/// `sqlite3RootPageMoved`: a VDBE a chama para ajustar o esquema interno quando a camada btree
/// move a página raiz de uma tabela. A raiz de uma tabela ou índice do banco `i_db` mudou de
/// `i_from` para `i_to`.
///
/// Chamado #1728: a tabela de símbolos ainda pode ter informação de tabelas ou índices em vias de
/// serem apagados, e um deles pode ter o mesmo número de página raiz do que está sendo movido.
/// Por isso a busca não para no primeiro casamento: continua até converter tudo o que tem
/// `i_from`.
pub fn root_page_moved(db: &mut Connection, i_db: usize, i_from: u32, i_to: u32) {
    let schema = &mut db.dbs[i_db].schema;
    let mut p_elem = hash_first(&schema.tbl_hash);
    while let Some(e) = p_elem {
        p_elem = hash_next(&schema.tbl_hash, e);
        let p_tab = hash_data_mut(&mut schema.tbl_hash, e);
        if p_tab.tnum == i_from {
            Rc::make_mut(p_tab).tnum = i_to;
        }
    }
    // `idx_hash` guarda o nome da tabela dona; o `Index` mora em `Table.p_index`.
    let owners: Vec<(Vec<u8>, Vec<u8>)> = hash_iter(&schema.idx_hash)
        .map(|(k, v)| (k.to_vec(), v.clone()))
        .collect();
    for (z_idx, z_tab) in owners {
        let Some(p_tab) = hash_find_mut(&mut schema.tbl_hash, &z_tab) else {
            continue;
        };
        if let Some(pos) = p_tab.p_index.iter().position(|x| str_icmp(&x.z_name, &z_idx) == 0) {
            if p_tab.p_index[pos].tnum == i_from {
                Rc::make_mut(&mut Rc::make_mut(p_tab).p_index[pos]).tnum = i_to;
            }
        }
    }
}

/// `destroyRootPage`: gera o código que apaga a tabela com página raiz `i_table` do banco
/// `i_db`. Gera também o que modifica sqlite_schema e o esquema interno se a camada btree move a
/// raiz de outra tabela enquanto apaga `i_table` (pode acontecer com auto-vacuum).
pub(crate) fn destroy_root_page(db: &mut Connection, parse: &mut Parse, i_table: u32, i_db: i32) {
    get_vdbe(db, parse);
    let r1 = get_temp_reg(parse);
    if i_table < 2 {
        error_msg(db, parse, b"corrupt schema", &[]);
    }
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op3(v, OP_DESTROY as i32, i_table as i32, r1, i_db);
    }
    may_abort(parse);

    // `OP_Destroy` grava um inteiro em r1. Se for diferente de zero, é o número da página raiz
    // de uma tabela movida para `i_table`; o código abaixo muda sqlite_schema para refletir
    // isso. O "#NNN" no SQL é uma constante especial que vale o que está no registrador NNN (ver
    // as regras da gramática do token TK_REGISTER).
    let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
    let fmt: Vec<u8> = [
        b"UPDATE %Q.".as_slice(),
        LEGACY_SCHEMA_TABLE,
        b" SET rootpage=%d WHERE #%d AND rootpage=#%d".as_slice(),
    ]
    .concat();
    nested_parse(
        db,
        parse,
        &fmt,
        &[
            text_arg(&z_db_name),
            PrintfArg::Int(i_table as i64),
            PrintfArg::Int(r1 as i64),
            PrintfArg::Int(r1 as i64),
        ],
    );
    release_temp_reg(parse, r1);
}

/// `destroyTable`: gera o código que apaga a tabela `p_tab` e todos os índices dela no disco.
///
/// Se o banco pode ser auto-vacuum, é preciso chamar `OP_Destroy` nas páginas raiz da tabela e
/// dos índices em ordem, começando pela de maior número: assim nenhuma raiz a destruir é
/// realocada por um `OP_Destroy` anterior. Se a raiz 5 é a maior do banco, "OP_Destroy 4" a move
/// para a página 4, e o "OP_Destroy 5" seguinte cairia numa página livre.
fn destroy_table(db: &mut Connection, parse: &mut Parse, p_tab: &Table) {
    let i_tab = p_tab.tnum;
    let mut i_destroyed: u32 = 0;
    loop {
        let mut i_largest: u32 = 0;
        if i_destroyed == 0 || i_tab < i_destroyed {
            i_largest = i_tab;
        }
        for p_idx in p_tab.p_index.iter() {
            let i_idx = p_idx.tnum;
            if (i_destroyed == 0 || i_idx < i_destroyed) && i_idx > i_largest {
                i_largest = i_idx;
            }
        }
        if i_largest == 0 {
            return;
        }
        let i_db = schema_to_index(db, p_tab.p_schema);
        debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
        destroy_root_page(db, parse, i_largest, i_db);
        i_destroyed = i_largest;
    }
}

/// `sqlite3ClearStatTables`: apaga as entradas das tabelas sqlite_statN (N de 1 a 4) depois de
/// um DROP INDEX ou DROP TABLE. `z_type` é "idx" ou "tbl".
pub(crate) fn clear_stat_tables(
    db: &mut Connection,
    parse: &mut Parse,
    i_db: i32,
    z_type: &[u8],
    z_name: &[u8],
) {
    let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
    for i in 1..=4 {
        let z_tab = format!("sqlite_stat{}", i).into_bytes();
        if find_table(db, &z_tab, Some(&z_db_name)).is_some() {
            nested_parse(
                db,
                parse,
                b"DELETE FROM %Q.%s WHERE %s=%Q",
                &[text_arg(&z_db_name), text_arg(&z_tab), text_arg(z_type), text_arg(z_name)],
            );
        }
    }
}

/// `sqlite3CodeDropTable`: gera o código que apaga uma tabela (ou view, se `is_view` não é 0).
pub fn code_drop_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_db: i32,
    is_view: u32,
) {
    get_vdbe(db, parse);
    let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
    begin_write_operation(db, parse, 1, i_db);

    if p_tab.is_virtual() {
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            add_op0(v, OP_VBEGIN as i32);
        }
    }

    // Apaga todos os gatilhos associados à tabela. O código gerado tira as entradas de
    // sqlite_schema e/ou sqlite_temp_schema se for preciso.
    for p_trigger in trigger_list(db, parse, p_tab) {
        debug_assert!(
            p_trigger.p_schema == p_tab.p_schema || p_trigger.p_schema == db.dbs[1].schema.id
        );
        drop_trigger_ptr(db, parse, &p_trigger);
    }

    // Tira as entradas de sqlite_sequence associadas à tabela. Isso se faz antes de apagar a
    // tabela no nível do btree, caso sqlite_sequence precise se mover por causa disso (pode
    // acontecer em auto-vacuum).
    if (p_tab.tab_flags & TF_AUTOINCREMENT) != 0 {
        nested_parse(
            db,
            parse,
            b"DELETE FROM %Q.sqlite_sequence WHERE name=%Q",
            &[text_arg(&z_db_name), text_arg(&p_tab.z_name)],
        );
    }

    // Apaga da tabela de esquema todas as entradas que se referem à tabela: o programa passa
    // por sqlite_schema e apaga cada linha que se refere a uma tabela de mesmo nome. Os
    // gatilhos ficam à parte porque um gatilho criado no banco temporário pode se referir a uma
    // tabela de outro banco.
    let fmt: Vec<u8> = [
        b"DELETE FROM %Q.".as_slice(),
        LEGACY_SCHEMA_TABLE,
        b" WHERE tbl_name=%Q and type!='trigger'".as_slice(),
    ]
    .concat();
    nested_parse(db, parse, &fmt, &[text_arg(&z_db_name), text_arg(&p_tab.z_name)]);
    if is_view == 0 && !p_tab.is_virtual() {
        destroy_table(db, parse, p_tab);
    }

    // Tira a tabela do esquema interno do SQLite e muda o cookie do esquema.
    if p_tab.is_virtual() {
        if let Some(v) = parse.p_vdbe.as_deref_mut() {
            add_op4(v, OP_VDESTROY as i32, i_db, 0, 0, P4::Text(p_tab.z_name.clone()));
        }
        may_abort(parse);
    }
    if let Some(v) = parse.p_vdbe.as_deref_mut() {
        add_op4(v, OP_DROPTABLE as i32, i_db, 0, 0, P4::Text(p_tab.z_name.clone()));
    }
    change_cookie(db, parse, i_db);
    sqlite_view_reset_all(db, i_db as usize);
}

/// `sqlite3ReadOnlyShadowTables`: verdadeiro se as tabelas sombra devem ser somente leitura no
/// contexto atual.
pub fn read_only_shadow_tables(db: &Connection) -> bool {
    (db.flags & SQLITE_DEFENSIVE) != 0
        && db.p_vtab_ctx.is_empty()
        && db.n_vdbe_exec == 0
        && !vtab_in_sync(db)
}

/// `tableMayNotBeDropped`: verdadeiro se não é permitido apagar a tabela.
fn table_may_not_be_dropped(db: &Connection, p_tab: &Table) -> bool {
    if strnicmp(Some(&p_tab.z_name), Some(b"sqlite_"), 7) == 0 {
        let z_tail = p_tab.z_name.get(7..).unwrap_or(&[]);
        if strnicmp(Some(z_tail), Some(b"stat"), 4) == 0 {
            return false;
        }
        if strnicmp(Some(z_tail), Some(b"parameters"), 10) == 0 {
            return false;
        }
        return true;
    }
    if (p_tab.tab_flags & TF_SHADOW) != 0 && read_only_shadow_tables(db) {
        return true;
    }
    if (p_tab.tab_flags & TF_EPONYMOUS) != 0 {
        return true;
    }
    false
}

/// `sqlite3DropTable`: faz o trabalho de um DROP TABLE (ou DROP VIEW, com `is_view` igual a
/// `LOCATE_VIEW`). `p_name` é a lista de um só item com o nome da tabela a apagar.
pub fn drop_table(
    db: &mut Connection,
    parse: &mut Parse,
    p_name: Box<SrcList>,
    is_view: u32,
    no_err: i32,
) {
    'exit_drop_table: {
        if db.malloc_failed != 0 {
            break 'exit_drop_table;
        }
        debug_assert!(parse.n_err == 0);
        debug_assert!(p_name.a.len() == 1);
        if read_schema(db, parse) != 0 {
            break 'exit_drop_table;
        }
        if no_err != 0 {
            db.suppress_err += 1;
        }
        debug_assert!(is_view == 0 || is_view == LOCATE_VIEW);
        let p_tab = locate_table_item(db, parse, is_view, &p_name.a[0]);
        if no_err != 0 {
            db.suppress_err -= 1;
        }

        let Some(mut p_tab) = p_tab else {
            if no_err != 0 {
                code_verify_named_schema(db, parse, p_name.a[0].z_database.as_deref());
                force_not_read_only(db, parse);
            }
            break 'exit_drop_table;
        };
        let i_db = schema_to_index(db, p_tab.p_schema);
        debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());

        // Se `p_tab` é uma tabela virtual, `view_get_column_names` garante que ela foi iniciada.
        if p_tab.is_virtual() && view_get_column_names(db, parse, &mut p_tab) != 0 {
            break 'exit_drop_table;
        }
        {
            let z_tab = schema_table(i_db);
            let z_db = db.dbs[i_db as usize].z_db_s_name.clone();
            let mut z_arg2: Option<Vec<u8>> = None;
            if auth_check(db, parse, SQLITE_DELETE, Some(z_tab), None, Some(&z_db)) != 0 {
                break 'exit_drop_table;
            }
            let code = if is_view != 0 {
                if OMIT_TEMPDB == 0 && i_db == 1 {
                    SQLITE_DROP_TEMP_VIEW
                } else {
                    SQLITE_DROP_VIEW
                }
            } else if p_tab.is_virtual() {
                z_arg2 = get_vtable(db, &p_tab)
                    .and_then(|id| db.vtabs.get(id.slot()))
                    .map(|vt| vt.p_mod.z_name.clone());
                SQLITE_DROP_VTABLE
            } else if OMIT_TEMPDB == 0 && i_db == 1 {
                SQLITE_DROP_TEMP_TABLE
            } else {
                SQLITE_DROP_TABLE
            };
            if auth_check(db, parse, code, Some(&p_tab.z_name), z_arg2.as_deref(), Some(&z_db))
                != 0
            {
                break 'exit_drop_table;
            }
            if auth_check(db, parse, SQLITE_DELETE, Some(&p_tab.z_name), None, Some(&z_db)) != 0 {
                break 'exit_drop_table;
            }
        }
        if table_may_not_be_dropped(db, &p_tab) {
            error_msg(db, parse, b"table %s may not be dropped", &[text_arg(&p_tab.z_name)]);
            break 'exit_drop_table;
        }

        // Garante que DROP TABLE não é usado numa view e DROP VIEW não é usado numa tabela.
        if is_view != 0 && !p_tab.is_view() {
            error_msg(db, parse, b"use DROP TABLE to delete table %s", &[text_arg(&p_tab.z_name)]);
            break 'exit_drop_table;
        }
        if is_view == 0 && p_tab.is_view() {
            error_msg(db, parse, b"use DROP VIEW to delete view %s", &[text_arg(&p_tab.z_name)]);
            break 'exit_drop_table;
        }

        // Gera o código que tira a tabela da tabela de esquema no disco.
        {
            get_vdbe(db, parse);
            begin_write_operation(db, parse, 1, i_db);
            if is_view == 0 {
                clear_stat_tables(db, parse, i_db, b"tbl", &p_tab.z_name);
                fk_drop_table(db, parse, &p_name, &p_tab);
            }
            code_drop_table(db, parse, &p_tab, i_db, is_view);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// FOREIGN KEY
// ---------------------------------------------------------------------------------------------

/// Parte de `create_foreign_key` que trabalha com a tabela em construção já fora do `Parse`.
fn create_foreign_key_to(
    db: &mut Connection,
    parse: &mut Parse,
    p: &mut Table,
    p_from_col: Option<&ExprList>,
    p_to: &Token,
    p_to_col: Option<&ExprList>,
    flags: i32,
) {
    let n_col: usize;
    match p_from_col {
        None => {
            if p.n_col < 1 {
                return;
            }
            let i_col = (p.n_col - 1) as usize;
            if let Some(to_col) = p_to_col {
                if to_col.a.len() != 1 {
                    let z_col = col_name(&p.a_col[i_col]).to_vec();
                    let arg_to = token_arg(parse, p_to);
                    error_msg(
                        db,
                        parse,
                        b"foreign key on %s should reference only one column of table %T",
                        &[text_arg(&z_col), arg_to],
                    );
                    return;
                }
            }
            n_col = 1;
        }
        Some(from_col) => {
            if let Some(to_col) = p_to_col {
                if to_col.a.len() != from_col.a.len() {
                    error_msg(
                        db,
                        parse,
                        b"number of columns in foreign key does not match the number of columns in the referenced table",
                        &[],
                    );
                    return;
                }
            }
            n_col = from_col.a.len();
        }
    }

    debug_assert!(p.is_ordinary_table());
    let mut z_to = p_to.z.clone();
    z_to.push(0);
    dequote(&mut z_to);
    let n_to = strlen30(&z_to) as usize;
    z_to.truncate(n_to);
    if parse.in_rename_object() {
        rename_token_map(parse, z_to.as_ptr() as usize, p_to);
    }

    // O vetor `a_col` tem o tamanho final desde já: os endereços dos itens são a chave do mapa
    // de tokens do RENAME e não podem mudar.
    let mut a_col: Vec<FKeyColMap> = vec![FKeyColMap::default(); n_col];
    match p_from_col {
        None => {
            a_col[0].i_from = p.n_col as i32 - 1;
        }
        Some(from_col) => {
            for i in 0..n_col {
                let z_e_name: &[u8] = from_col.a[i].z_e_name.as_deref().unwrap_or(&[]);
                let mut j = 0usize;
                while j < p.n_col as usize {
                    if str_icmp(&p.a_col[j].z_cn_name, z_e_name) == 0 {
                        a_col[i].i_from = j as i32;
                        break;
                    }
                    j += 1;
                }
                if j >= p.n_col as usize {
                    error_msg(
                        db,
                        parse,
                        b"unknown column \"%s\" in foreign key definition",
                        &[text_arg(z_e_name)],
                    );
                    return;
                }
                if parse.in_rename_object() {
                    rename_token_remap(
                        parse,
                        &a_col[i] as *const FKeyColMap as usize,
                        z_e_name.as_ptr() as usize,
                    );
                }
            }
        }
    }
    if let Some(to_col) = p_to_col {
        for i in 0..n_col {
            let z_e_name: &[u8] = to_col.a[i].z_e_name.as_deref().unwrap_or(&[]);
            let n = strlen30(z_e_name) as usize;
            a_col[i].z_col = Some(z_e_name[..n].to_vec());
            if parse.in_rename_object() {
                let to = a_col[i].z_col.as_ref().map_or(0, |z| z.as_ptr() as usize);
                rename_token_remap(parse, to, z_e_name.as_ptr() as usize);
            }
        }
    }
    let p_fkey = Rc::new(FKey {
        z_from: p.z_name.clone(),
        z_to,
        n_col: n_col as i32,
        is_deferred: 0,
        a_action: [(flags & 0xff) as u8, ((flags >> 8) & 0xff) as u8],
        a_col,
    });

    // O FKey novo vira a cabeça da lista dos que referenciam a tabela pai.
    let i_db = schema_to_index(db, p.p_schema);
    if i_db >= 0 {
        let fkey_hash = &mut db.dbs[i_db as usize].schema.fkey_hash;
        if let Some(chain) = hash_find_mut(fkey_hash, &p_fkey.z_to) {
            chain.insert(0, Rc::clone(&p_fkey));
        } else {
            hash_insert(fkey_hash, &p_fkey.z_to, Some(vec![Rc::clone(&p_fkey)]));
        }
    }

    // Liga a chave estrangeira à tabela como último passo.
    if let TableU::Tab(info) = &mut p.u {
        info.p_f_key.insert(0, p_fkey);
    }
}

/// `sqlite3CreateForeignKey`: cria uma chave estrangeira nova na tabela em construção
/// (`parse.p_new_table`). `p_from_col` diz quais colunas desta tabela apontam para a outra; se é
/// `None`, a chave liga à última coluna inserida. `p_to` é o nome da tabela referenciada (a
/// "pai") e `p_to_col` a lista de colunas dela. `flags` tem os algoritmos de resolução de
/// conflito dos ON DELETE, ON UPDATE e ON INSERT.
///
/// A chave nasce para processamento IMMEDIATE; um `defer_foreign_key` posterior pode mudá-la
/// para DEFERRED.
pub fn create_foreign_key(
    db: &mut Connection,
    parse: &mut Parse,
    p_from_col: Option<Box<ExprList>>,
    p_to: &Token,
    p_to_col: Option<Box<ExprList>>,
    flags: i32,
) {
    if parse.p_new_table.is_none() || parse.in_declare_vtab() {
        return;
    }
    with_new_table(parse, |parse, p| {
        create_foreign_key_to(
            db,
            parse,
            p,
            p_from_col.as_deref(),
            p_to,
            p_to_col.as_deref(),
            flags,
        )
    });
}

/// `sqlite3DeferForeignKey`: chamada quando um INITIALLY IMMEDIATE ou INITIALLY DEFERRED aparece
/// na definição de uma chave estrangeira. `is_deferred` é 1 para DEFERRED e 0 para IMMEDIATE. O
/// comportamento da chave mais recente é ajustado.
///
/// O `FKey` é um `Rc` compartilhado entre a tabela e `fkey_hash`: a chave sai dos dois lugares,
/// é alterada (sem copiar os vetores, para o mapa de tokens do RENAME continuar valendo) e
/// volta para o mesmo lugar nos dois.
pub fn defer_foreign_key(db: &mut Connection, parse: &mut Parse, is_deferred: i32) {
    let Some(p_tab) = parse.p_new_table.as_deref_mut() else {
        return;
    };
    if !p_tab.is_ordinary_table() {
        return;
    }
    let i_db = schema_to_index(db, p_tab.p_schema);
    let TableU::Tab(info) = &mut p_tab.u else {
        return;
    };
    if info.p_f_key.is_empty() {
        return;
    }
    debug_assert!(is_deferred == 0 || is_deferred == 1);
    let old = info.p_f_key.remove(0);
    let mut chain_pos: Option<usize> = None;
    if i_db >= 0 {
        if let Some(chain) =
            hash_find_mut(&mut db.dbs[i_db as usize].schema.fkey_hash, &old.z_to)
        {
            chain_pos = chain.iter().position(|x| Rc::ptr_eq(x, &old));
            if let Some(pos) = chain_pos {
                chain.remove(pos);
            }
        }
    }
    let z_to = old.z_to.clone();
    let mut fkey = match Rc::try_unwrap(old) {
        Ok(f) => f,
        Err(shared) => (*shared).clone(),
    };
    fkey.is_deferred = is_deferred as u8;
    let fkey = Rc::new(fkey);
    if let Some(pos) = chain_pos {
        if let Some(chain) = hash_find_mut(&mut db.dbs[i_db as usize].schema.fkey_hash, &z_to) {
            chain.insert(pos, Rc::clone(&fkey));
        }
    }
    info.p_f_key.insert(0, fkey);
}

// ---------------------------------------------------------------------------------------------
// Índices
// ---------------------------------------------------------------------------------------------

/// `sqlite3RefillIndex`: gera o código que apaga e enche de novo o índice `p_index`. Serve para
/// inicializar um índice recém-criado ou recalcular o conteúdo dele num REINDEX. `p_tab` é a
/// tabela indexada e `i_db` o banco dela (o `pIndex->pTable` e o `pIndex->pSchema` do C).
///
/// Se `mem_root_page` não é negativo, o índice é novo e esse registrador tem o número da página
/// raiz dele. Se é negativo, o índice já existe, precisa ser esvaziado antes de ser enchido de
/// novo e a página raiz é `p_index.tnum`.
pub(crate) fn refill_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Table,
    p_index: &Index,
    i_db: i32,
    mem_root_page: i32,
) {
    // O C passa o ponteiro da tabela; aqui a geração de chave pede o `Rc` (para o `own` das
    // expressões de índice), e uma cópia de conteúdo igual basta.
    let p_tab_rc = Rc::new(p_tab.clone());
    // O VDBE do `Parse` (existe depois de `get_vdbe`).
    macro_rules! vdbe {
        () => {
            match parse.p_vdbe.as_deref_mut() {
                Some(v) => v,
                None => return,
            }
        };
    }

    let i_tab = parse.n_tab; // Cursor do btree usado para p_tab.
    parse.n_tab += 1;
    let i_idx = parse.n_tab; // Cursor do btree usado para p_index.
    parse.n_tab += 1;

    let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
    if auth_check(db, parse, SQLITE_REINDEX, Some(&p_index.z_name), None, Some(&z_db_name)) != 0 {
        return;
    }

    // Exige uma trava de escrita na tabela para fazer esta operação.
    table_lock(db, parse, i_db, p_tab.tnum, true, &p_tab.z_name);

    get_vdbe(db, parse);
    let tnum: u32 = if mem_root_page >= 0 { mem_root_page as u32 } else { p_index.tnum };
    let p_key = key_info_of_index(parse, db, p_index);
    debug_assert!(p_key.is_some() || parse.n_err != 0);
    let key_p4 = |k: &Option<Rc<KeyInfo>>| k.as_ref().map_or(P4::None, |k| P4::KeyInfo(Rc::clone(k)));

    // Abre o cursor do ordenador, se for usá-lo.
    let i_sorter = parse.n_tab;
    parse.n_tab += 1;
    add_op4(
        vdbe!(),
        OP_SORTEROPEN as i32,
        i_sorter,
        0,
        p_index.n_key_col as i32,
        key_p4(&p_key),
    );

    // Abre a tabela e percorre todas as linhas dela pondo os registros do índice no ordenador.
    open_table(db, parse, i_tab, i_db, p_tab, OP_OPENREAD);
    let mut addr1 = add_op2(vdbe!(), OP_REWIND as i32, i_tab, 0);
    let reg_record = get_temp_reg(parse);
    multi_write(parse);

    let mut i_part_idx_label: i32 = 0;
    generate_index_key(
        db,
        parse,
        &p_tab_rc,
        p_index,
        i_tab,
        reg_record,
        0,
        &mut i_part_idx_label,
        None,
        0,
    );
    add_op2(vdbe!(), OP_SORTERINSERT as i32, i_sorter, reg_record);
    resolve_part_idx_label(db, parse, i_part_idx_label);
    add_op2(vdbe!(), OP_NEXT as i32, i_tab, addr1 + 1);
    jump_here(vdbe!(), addr1);
    if mem_root_page < 0 {
        add_op2(vdbe!(), OP_CLEAR as i32, tnum as i32, i_db);
    }
    add_op4(vdbe!(), OP_OPENWRITE as i32, i_idx, tnum as i32, i_db, key_p4(&p_key));
    change_p5(
        vdbe!(),
        OPFLAG_BULKCSR | (if mem_root_page >= 0 { OPFLAG_P2ISREG } else { 0 }),
    );

    addr1 = add_op2(vdbe!(), OP_SORTERSORT as i32, i_sorter, 0);
    let addr2: i32;
    if p_index.is_unique_index() {
        let j2 = vdbe_goto(vdbe!(), 1);
        addr2 = vdbe!().n_op();
        add_op4_int(
            vdbe!(),
            OP_SORTERCOMPARE as i32,
            i_sorter,
            j2,
            reg_record,
            p_index.n_key_col as i32,
        );
        unique_constraint(db, parse, OE_ABORT as i32, p_tab, p_index);
        jump_here(vdbe!(), j2);
    } else {
        // A maioria dos CREATE INDEX e REINDEX que não são UNIQUE não pode abortar. A exceção
        // é se uma expressão indexada tem uma função de usuário que lança exceção ao ser
        // avaliada. Mas o custo de dar um diário de comando a um CREATE INDEX é muito pequeno
        // (a maioria das páginas escritas não tem conteúdo a restaurar se o comando abortar),
        // então `may_abort` vale para todo CREATE INDEX.
        may_abort(parse);
        addr2 = vdbe!().n_op();
    }
    add_op3(vdbe!(), OP_SORTERDATA as i32, i_sorter, reg_record, i_idx);
    if !p_index.b_asc_key_bug {
        // Este `OP_SeekEnd` acelera muito o INSERT no índice de um REINDEX porque evita seeks
        // desnecessários. Mas a otimização não vale para índices de restrição UNIQUE em tabelas
        // WITHOUT ROWID com PRIMARY KEY DESC, porque as chaves desses índices ficam numa ordem
        // diferente da tabela principal. Ver https://www.sqlite.org/src/info/bba7b69f9849b5bf
        add_op1(vdbe!(), OP_SEEKEND as i32, i_idx);
    }
    add_op2(vdbe!(), OP_IDXINSERT as i32, i_idx, reg_record);
    change_p5(vdbe!(), OPFLAG_USESEEKRESULT);
    release_temp_reg(parse, reg_record);
    add_op2(vdbe!(), OP_SORTERNEXT as i32, i_sorter, addr2);
    jump_here(vdbe!(), addr1);

    add_op1(vdbe!(), OP_CLOSE as i32, i_tab);
    add_op1(vdbe!(), OP_CLOSE as i32, i_idx);
    add_op1(vdbe!(), OP_CLOSE as i32, i_sorter);
}

/// `sqlite3AllocateIndexObject`: um `Index` para `n_col` colunas no total, com os vetores do
/// tamanho que o C aloca depois do objeto (`az_coll`, `ai_column` e `a_sort_order` com `n_col`
/// entradas, `ai_row_log_est` com `n_col + 1`), `n_column == n_col` e `n_key_col == n_col - 1`.
pub fn allocate_index_object(n_col: i16) -> Index {
    let n = n_col as usize;
    Index {
        az_coll: vec![Vec::new(); n],
        ai_row_log_est: vec![0; n + 1],
        ai_column: vec![0; n],
        a_sort_order: vec![0; n],
        n_column: n_col as u16,
        n_key_col: (n_col - 1) as u16,
        ..Index::default()
    }
}

/// `sqlite3HasExplicitNulls`: se a lista tem uma expressão lida com "NULLS FIRST" ou "NULLS LAST"
/// explícito, deixa um erro em `parse` e devolve diferente de zero; senão devolve zero.
pub fn has_explicit_nulls(db: &mut Connection, parse: &mut Parse, p_list: Option<&ExprList>) -> i32 {
    if let Some(list) = p_list {
        for item in list.a.iter() {
            if item.fg.b_nulls {
                let sf = item.fg.sort_flags;
                let z_which: &[u8] = if sf == 0 || sf == 3 { b"FIRST" } else { b"LAST" };
                error_msg(db, parse, b"unsupported use of NULLS %s", &[text_arg(z_which)]);
                return 1;
            }
        }
    }
    0
}

/// Onde `create_index` põe o índice: a tabela em construção (tirada de `parse.p_new_table`
/// durante a chamada) ou uma tabela do esquema (um CREATE INDEX ... ON tabela).
enum IndexTarget {
    New(Box<Table>),
    Existing(Rc<Table>),
}

impl IndexTarget {
    fn table(&self) -> &Table {
        match self {
            IndexTarget::New(b) => b,
            IndexTarget::Existing(r) => r,
        }
    }
}

/// O fim do bloco `exit_create_index` do C: garante que todos os índices REPLACE ficam no fim da
/// lista. A lista já estava ordenada quando a rotina começou, então no máximo o índice novo está
/// fora de ordem: o primeiro REPLACE passa por cima dos índices não REPLACE que vêm depois dele.
fn reorder_replace_indexes(p_index: &mut [Rc<Index>]) {
    let Some(k) = p_index.iter().position(|x| x.on_error == OE_REPLACE) else {
        return;
    };
    let mut m = k + 1;
    while m < p_index.len() && p_index[m].on_error != OE_REPLACE {
        m += 1;
    }
    p_index[k..m].rotate_left(1);
}

/// `sqlite3CreateIndex`: cria um índice novo para uma tabela SQL. `p_name1.p_name2` é o nome do
/// índice e `p_tbl_name` a tabela a indexar; os dois são `None` numa PRIMARY KEY ou num índice
/// criado para satisfazer uma restrição UNIQUE, caso em que a tabela é `parse.p_new_table`, a
/// que um CREATE TABLE está construindo.
///
/// `p_list` é a lista de colunas a indexar; é `None` quando se trata de uma chave primária ou
/// restrição UNIQUE sobre a última coluna acrescentada à tabela em construção. `on_error` é
/// OE_Abort, OE_Ignore, OE_Replace ou OE_None. `p_start` é o token CREATE que começa o comando,
/// `p_pi_where` a cláusula WHERE dos índices parciais, `sort_order` a ordem da chave primária
/// quando `p_list` é `None` e `idx_type` o tipo do índice.
///
/// Fora o que o C faz com os argumentos (liberar `p_list`, `p_pi_where` e `p_tbl_name` no fim),
/// o índice entra no INÍCIO de `Table.p_index` (a cabeça da lista do C). Com `IN_RENAME_OBJECT`
/// os índices redundantes (e o de um CREATE INDEX) ficam em `parse.p_new_index`, o mais novo na
/// posição 0, com a lista de colunas em `a_col_expr`.
#[allow(clippy::too_many_arguments)]
pub fn create_index(
    db: &mut Connection,
    parse: &mut Parse,
    p_name1: Option<&Token>,
    p_name2: Option<&Token>,
    p_tbl_name: Option<Box<SrcList>>,
    p_list: Option<Box<ExprList>>,
    on_error: i32,
    p_start: Option<&Token>,
    p_pi_where: Option<Box<Expr>>,
    sort_order: i32,
    if_not_exist: i32,
    idx_type: u8,
) {
    let mut p_list = p_list;
    let mut p_tbl_name = p_tbl_name;
    let mut p_pi_where = p_pi_where;
    let is_tbl = p_tbl_name.is_some();
    let mut target: Option<IndexTarget> = None;

    // A tabela a indexar (`pTab` do C); sai do bloco se não existe.
    macro_rules! tab {
        ($label:lifetime) => {
            match &target {
                Some(t) => t.table(),
                None => break $label,
            }
        };
    }

    'exit_create_index: {
        if parse.n_err != 0 {
            break 'exit_create_index;
        }
        debug_assert!(db.malloc_failed == 0);
        if parse.in_declare_vtab() && idx_type != SQLITE_IDXTYPE_PRIMARYKEY {
            break 'exit_create_index;
        }
        if SQLITE_OK != read_schema(db, parse) {
            break 'exit_create_index;
        }
        if has_explicit_nulls(db, parse, p_list.as_deref()) != 0 {
            break 'exit_create_index;
        }

        // Acha a tabela a indexar. Sai cedo se não achar.
        let i_db: i32;
        let mut p_name: Option<&Token> = None;
        let mut p_pk: Option<Rc<Index>> = None;
        if let Some(tbl) = p_tbl_name.as_deref_mut() {
            // Usa o nome do índice em duas partes para saber em que banco procurar a tabela, e
            // "fixa" o nome da tabela nesse banco antes de procurá-la.
            debug_assert!(p_name1.is_some() && p_name2.is_some());
            let (Some(n1), Some(n2)) = (p_name1, p_name2) else {
                break 'exit_create_index;
            };
            let Some((d, pn)) = two_part_name(db, parse, n1, n2) else {
                break 'exit_create_index;
            };
            let mut i_db_found = d;
            p_name = Some(pn);

            // Se o nome do índice não é qualificado, vê se a tabela é temporária: então o banco
            // é o 1. Não se faz isso ao inicializar o esquema de um banco.
            if OMIT_TEMPDB == 0 && db.init.busy == 0 {
                let p_looked_up = src_list_lookup(db, parse, tbl);
                if n2.z.is_empty() {
                    if let Some(t) = &p_looked_up {
                        if t.p_schema == db.dbs[1].schema.id {
                            i_db_found = 1;
                        }
                    }
                }
            }

            let mut s_fix = fix_init(db, i_db_found, "index", pn);
            if fix_src_list(db, parse, &mut s_fix, tbl) != 0 {
                // Como o analisador monta `p_tbl_name` de um identificador só,
                // `fix_src_list` nunca falha.
                debug_assert!(false);
            }
            let Some(t) = locate_table_item(db, parse, 0, &tbl.a[0]) else {
                break 'exit_create_index;
            };
            if i_db_found == 1 && db.dbs[i_db_found as usize].schema.id != t.p_schema {
                error_msg(
                    db,
                    parse,
                    b"cannot create a TEMP index on non-TEMP table \"%s\"",
                    &[text_arg(&t.z_name)],
                );
                break 'exit_create_index;
            }
            if !t.has_rowid() {
                p_pk = primary_key_index(&t).cloned();
            }
            i_db = i_db_found;
            target = Some(IndexTarget::Existing(t));
        } else {
            debug_assert!(p_start.is_none());
            let Some(t) = parse.p_new_table.take() else {
                break 'exit_create_index;
            };
            i_db = schema_to_index(db, t.p_schema);
            target = Some(IndexTarget::New(t));
        }
        let is_new = matches!(target, Some(IndexTarget::New(_)));
        let in_rename = parse.in_rename_object();
        let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();

        {
            let tab = tab!('exit_create_index);
            if strnicmp(Some(&tab.z_name), Some(b"sqlite_"), 7) == 0
                && db.init.busy == 0
                && is_tbl
            {
                error_msg(db, parse, b"table %s may not be indexed", &[text_arg(&tab.z_name)]);
                break 'exit_create_index;
            }
            if tab.is_view() {
                error_msg(db, parse, b"views may not be indexed", &[]);
                break 'exit_create_index;
            }
            if tab.is_virtual() {
                error_msg(db, parse, b"virtual tables may not be indexed", &[]);
                break 'exit_create_index;
            }
        }

        // Acha o nome do índice e confere que não há outro índice ou tabela com o mesmo nome.
        //
        // Exceção: ao ler os nomes de índices permanentes de sqlite_schema (porque outro
        // processo mudou o esquema), se um nome colide com o de uma tabela ou índice temporário,
        // o índice continua a ser processado.
        //
        // Se `p_name` é `None`, é uma PRIMARY KEY ou restrição UNIQUE e é preciso inventar um
        // nome.
        let z_name: Vec<u8>;
        {
            let tab = tab!('exit_create_index);
            if let Some(pn) = p_name {
                let Some(zn) = name_from_token(Some(pn)) else {
                    break 'exit_create_index;
                };
                if SQLITE_OK != check_object_name(db, parse, &zn, b"index", &tab.z_name) {
                    break 'exit_create_index;
                }
                if !in_rename {
                    if db.init.busy == 0 && find_table(db, &zn, Some(&z_db_name)).is_some() {
                        error_msg(
                            db,
                            parse,
                            b"there is already a table named %s",
                            &[text_arg(&zn)],
                        );
                        break 'exit_create_index;
                    }
                    if find_index(db, &zn, Some(&z_db_name)).is_some() {
                        if if_not_exist == 0 {
                            error_msg(db, parse, b"index %s already exists", &[text_arg(&zn)]);
                        } else {
                            debug_assert!(db.init.busy == 0);
                            code_verify_schema(db, parse, i_db);
                            force_not_read_only(db, parse);
                        }
                        break 'exit_create_index;
                    }
                }
                z_name = zn;
            } else {
                let n = tab.p_index.len() + 1;
                let mut zn = b"sqlite_autoindex_".to_vec();
                zn.extend_from_slice(&tab.z_name);
                zn.push(b'_');
                zn.extend_from_slice(n.to_string().as_bytes());

                // Os nomes de índice automáticos gerados dentro de sqlite3_declare_vtab() têm
                // de ser distintos dos normais: o comando abaixo converte "sqlite3_autoindex..."
                // em "sqlite3_butoindex...". O teste "vtab_err.test" mostra a necessidade.
                if parse.in_special_parse() {
                    zn[7] += 1;
                }
                z_name = zn;
            }

            // Confere a autorização para criar o índice.
            if !in_rename {
                if auth_check(
                    db,
                    parse,
                    SQLITE_INSERT,
                    Some(schema_table(i_db)),
                    None,
                    Some(&z_db_name),
                ) != 0
                {
                    break 'exit_create_index;
                }
                let code = if OMIT_TEMPDB == 0 && i_db == 1 {
                    SQLITE_CREATE_TEMP_INDEX
                } else {
                    SQLITE_CREATE_INDEX
                };
                if auth_check(
                    db,
                    parse,
                    code,
                    Some(&z_name),
                    Some(&tab.z_name),
                    Some(&z_db_name),
                ) != 0
                {
                    break 'exit_create_index;
                }
            }
        }

        // Se `p_list` é `None`, a rotina foi chamada para fazer uma chave primária da última
        // coluna acrescentada à tabela em construção: cria uma lista falsa para simular isso.
        let mut list: Box<ExprList>;
        match p_list.take() {
            None => {
                let z_prev: Vec<u8>;
                match target.as_mut() {
                    Some(IndexTarget::New(b)) => match b.a_col.last_mut() {
                        Some(p_col) => {
                            p_col.col_flags |= COLFLAG_UNIQUE;
                            z_prev = col_name(p_col).to_vec();
                        }
                        None => break 'exit_create_index,
                    },
                    _ => break 'exit_create_index,
                }
                let prev_col = Token { z: z_prev, i_ofst: -1 };
                let mut l = expr_list_append(None, expr_alloc(TK_ID as i32, Some(&prev_col), 0));
                debug_assert!(l.as_deref().map_or(0, |x| x.a.len()) == 1);
                expr_list_set_sort_order(l.as_deref_mut(), sort_order, SQLITE_SO_UNDEFINED);
                let Some(l) = l else {
                    break 'exit_create_index;
                };
                list = l;
            }
            Some(l) => {
                expr_list_check_length(db, parse, Some(&*l), b"index");
                if parse.n_err != 0 {
                    break 'exit_create_index;
                }
                list = l;
            }
        }

        // Aloca a estrutura do índice. Os nomes das colações vão em `az_coll` como `Vec`
        // próprios (o C os guarda no espaço extra depois do objeto).
        let tab = tab!('exit_create_index);
        let n_extra_col = p_pk.as_ref().map_or(1, |pk| pk.n_key_col as usize);
        debug_assert!(list.a.len() + n_extra_col <= 32767);
        let mut index = allocate_index_object((list.a.len() + n_extra_col) as i16);
        index.z_name = z_name;
        index.on_error = on_error as u8;
        index.uniq_not_null = on_error != OE_NONE as i32;
        index.idx_type = idx_type;
        index.n_key_col = list.a.len() as u16;
        if let Some(mut p_where) = p_pi_where.take() {
            resolve_self_reference(db, parse, Some(tab), NC_PARTIDX, Some(&mut *p_where), None);
            index.p_partial_idx_where = Some(p_where);
        }

        // Vê se se deve respeitar DESC nas colunas do índice.
        let sort_order_mask: u8 =
            if db.dbs[i_db as usize].schema.file_format >= 4 { 0xff } else { 0 };

        // Analisa a lista de expressões que formam os termos do índice e dá os erros. No caso
        // comum em que a expressão é exatamente uma coluna da tabela, guarda a coluna em
        // `ai_column`. Nas expressões gerais, guarda `XN_EXPR` (-2) em `ai_column` e a lista em
        // `a_col_expr`.
        //
        // TODO do C: avisar se duas ou mais colunas do índice são idênticas e se a chave
        // primária da tabela é usada como parte da chave do índice.
        //
        // Em RENAME ou ao achar a primeira expressão, a lista passa a ser do índice
        // (`pIndex->aColExpr = pList`): o laço continua lendo os mesmos itens, então aqui a lista
        // fica em `list` até o fim do laço e só então vai para `a_col_expr`.
        let mut list_goes_to_index = in_rename;
        for i in 0..index.n_key_col as usize {
            if let Some(e) = list.a[i].p_expr.as_deref_mut() {
                string_to_id(e);
                resolve_self_reference(db, parse, Some(tab), NC_IDXEXPR, Some(e), None);
            }
            if parse.n_err != 0 {
                break 'exit_create_index;
            }
            let Some(item_expr) = list.a[i].p_expr.as_deref() else {
                break 'exit_create_index;
            };
            let Some(p_c_expr) = expr_skip_collate(Some(item_expr)) else {
                break 'exit_create_index;
            };
            let mut j: i32;
            if p_c_expr.op != TK_COLUMN {
                if is_new {
                    error_msg(
                        db,
                        parse,
                        b"expressions prohibited in PRIMARY KEY and UNIQUE constraints",
                        &[],
                    );
                    break 'exit_create_index;
                }
                list_goes_to_index = true;
                j = XN_EXPR as i32;
                index.ai_column[i] = XN_EXPR;
                index.uniq_not_null = false;
                index.b_has_expr = true;
            } else {
                j = p_c_expr.i_column;
                debug_assert!(j <= 0x7fff);
                if j < 0 {
                    j = tab.i_p_key as i32;
                } else {
                    if tab.a_col[j as usize].not_null == 0 {
                        index.uniq_not_null = false;
                    }
                    if (tab.a_col[j as usize].col_flags & COLFLAG_VIRTUAL) != 0 {
                        index.b_has_v_col = true;
                        index.b_has_expr = true;
                    }
                }
                index.ai_column[i] = j as i16;
            }
            let mut z_coll: Option<Vec<u8>> = None;
            if item_expr.op == TK_COLLATE {
                debug_assert!(!item_expr.has_property(EP_INT_VALUE));
                z_coll = item_expr.z_token().map(|z| c_str(z).to_vec());
            } else if j >= 0 {
                z_coll = column_coll(&tab.a_col[j as usize]).map(|z| z.to_vec());
            }
            let z_coll = z_coll.unwrap_or_else(|| STR_BINARY.as_bytes().to_vec());
            if db.init.busy == 0 && locate_coll_seq(db, parse, &z_coll).is_none() {
                break 'exit_create_index;
            }
            index.az_coll[i] = z_coll;
            index.a_sort_order[i] = list.a[i].fg.sort_flags & sort_order_mask;
        }
        if list_goes_to_index {
            index.a_col_expr = Some(list);
        }

        // Acrescenta a chave da tabela ao fim do índice. Nas tabelas WITHOUT ROWID (com
        // `p_pk`) é a PRIMARY KEY declarada; nas tabelas normais é o rowid.
        let mut i = index.n_key_col as usize;
        if let Some(pk) = &p_pk {
            for j in 0..pk.n_key_col as usize {
                let x = pk.ai_column[j];
                debug_assert!(x >= 0);
                if is_dup_column(&index, index.n_key_col as usize, pk, j) {
                    index.n_column -= 1;
                } else {
                    index.ai_column[i] = x;
                    index.az_coll[i] = pk.az_coll[j].clone();
                    index.a_sort_order[i] = pk.a_sort_order[j];
                    i += 1;
                }
            }
            debug_assert!(i == index.n_column as usize);
        } else {
            index.ai_column[i] = XN_ROWID;
            index.az_coll[i] = STR_BINARY.as_bytes().to_vec();
        }
        let mut n_row_log_est = tab.n_row_log_est;
        default_row_est(&mut index, &mut n_row_log_est);
        let n_row_log_est_changed = n_row_log_est != tab.n_row_log_est;
        if !is_new {
            estimate_index_width(&mut index, &tab.a_col);
        }

        // Se o índice contém todas as colunas da tabela, marca-o como índice de cobertura.
        debug_assert!(
            tab.has_rowid() || tab.i_p_key < 0 || table_column_to_index(&index, tab.i_p_key) >= 0
        );
        recompute_columns_not_indexed(&mut index, &tab.a_col);
        if is_tbl && index.n_column as i32 >= tab.n_col as i32 {
            index.is_covering = true;
            for j in 0..tab.n_col as i32 {
                if j == tab.i_p_key as i32 {
                    continue;
                }
                if table_column_to_index(&index, j as i16) >= 0 {
                    continue;
                }
                index.is_covering = false;
                break;
            }
        }
        let z_tab_name = tab.z_name.clone();
        let tab_has_rowid = tab.has_rowid();

        if n_row_log_est_changed {
            match target.as_mut() {
                Some(IndexTarget::New(b)) => b.n_row_log_est = n_row_log_est,
                _ => {
                    if let Some(slot) =
                        hash_find_mut(&mut db.dbs[i_db as usize].schema.tbl_hash, &z_tab_name)
                    {
                        Rc::make_mut(slot).n_row_log_est = n_row_log_est;
                    }
                }
            }
        }

        if is_new {
            // Esta rotina foi chamada para criar um índice automático por causa de um PRIMARY
            // KEY ou UNIQUE na definição de uma coluna ou depois das colunas, isto é, um de:
            //
            //     CREATE TABLE t(x PRIMARY KEY, y);
            //     CREATE TABLE t(x, y, UNIQUE(x, y));
            //
            // Seja como for, vê se a tabela já tem um índice assim. Se tiver, não cria este. Isto
            // só vale para índices criados automaticamente: com índices explícitos, o usuário faz
            // o que quiser.
            //
            // Duas restrições UNIQUE ou PRIMARY KEY são equivalentes (e a segunda é suprimida)
            // mesmo que tenham ordens de classificação diferentes. Se as colações diferem, ou as
            // colunas aparecem em ordens diferentes, as restrições são distintas e cada uma gera
            // um índice.
            let mut dup: Option<(usize, u8)> = None;
            {
                let tab = tab!('exit_create_index);
                for (k, p_idx) in tab.p_index.iter().enumerate() {
                    debug_assert!(p_idx.is_unique_index());
                    debug_assert!(p_idx.idx_type != SQLITE_IDXTYPE_APPDEF);
                    debug_assert!(index.is_unique_index());
                    if p_idx.n_key_col != index.n_key_col {
                        continue;
                    }
                    let n_key = p_idx.n_key_col as usize;
                    let mut kk = 0usize;
                    while kk < n_key {
                        debug_assert!(p_idx.ai_column[kk] >= 0);
                        if p_idx.ai_column[kk] != index.ai_column[kk] {
                            break;
                        }
                        if str_icmp(&p_idx.az_coll[kk], &index.az_coll[kk]) != 0 {
                            break;
                        }
                        kk += 1;
                    }
                    if kk == n_key {
                        dup = Some((k, p_idx.on_error));
                        break;
                    }
                }
            }
            if let Some((k, idx_on_error)) = dup {
                let mut new_on_error: Option<u8> = None;
                if idx_on_error != index.on_error {
                    // Esta restrição cria o mesmo índice de uma anterior do CREATE TABLE, mas
                    // com ON CONFLICT diferente. Se as duas têm ON CONFLICT explícito, é um
                    // erro. Senão vale o comportamento explícito.
                    if !(idx_on_error == OE_DEFAULT || index.on_error == OE_DEFAULT) {
                        error_msg(db, parse, b"conflicting ON CONFLICT clauses specified", &[]);
                    }
                    if idx_on_error == OE_DEFAULT {
                        new_on_error = Some(index.on_error);
                    }
                }
                if let Some(IndexTarget::New(b)) = target.as_mut() {
                    let p_idx = Rc::make_mut(&mut b.p_index[k]);
                    if let Some(oe) = new_on_error {
                        p_idx.on_error = oe;
                    }
                    if idx_type == SQLITE_IDXTYPE_PRIMARYKEY {
                        p_idx.idx_type = idx_type;
                    }
                }
                if in_rename {
                    parse.p_new_index.insert(0, Box::new(index));
                }
                break 'exit_create_index;
            }
        }

        if !in_rename {
            // Liga o índice novo à tabela e às outras estruturas do banco em memória.
            debug_assert!(parse.n_err == 0);
            if db.init.busy != 0 {
                debug_assert!(!parse.in_special_parse());
                if is_tbl {
                    index.tnum = db.init.new_tnum;
                    if index_has_duplicate_root_page(tab!('exit_create_index), &index) {
                        error_msg(db, parse, b"invalid rootpage", &[]);
                        parse.rc = SQLITE_CORRUPT;
                        break 'exit_create_index;
                    }
                }
                // `idx_hash` guarda o nome da tabela dona do índice.
                hash_insert(
                    &mut db.dbs[i_db as usize].schema.idx_hash,
                    &index.z_name,
                    Some(z_tab_name.clone()),
                );
                db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
            } else if tab_has_rowid || is_tbl {
                // Se este é o CREATE INDEX inicial (ou o CREATE TABLE, se o índice é implícito
                // de um UNIQUE ou PRIMARY KEY), gera o código que aloca a página raiz do índice
                // no disco, faz a entrada dele em sqlite_schema e o enche de dados. Mas não se
                // está só lendo sqlite_schema para analisar o esquema, nem se o índice é a
                // PRIMARY KEY de uma tabela WITHOUT ROWID.
                //
                // Se `p_tbl_name` é `None`, o índice é uma PRIMARY KEY ou UNIQUE implícita de um
                // CREATE TABLE. Como a tabela acabou de nascer e não tem dados, não é preciso
                // inicializar o índice.
                parse.n_mem += 1;
                let i_mem = parse.n_mem;
                get_vdbe(db, parse);
                begin_write_operation(db, parse, 1, i_db);

                // Cria a página raiz do índice com `OP_CreateBtree`. Antes, gera um `OP_Noop` e
                // guarda o endereço dele em `Index.tnum`: se o índice for na verdade uma PRIMARY
                // KEY de tabela WITHOUT ROWID, `convert_to_without_rowid_table` troca o Noop por
                // um Goto que pula o código gerado abaixo.
                if let Some(v) = parse.p_vdbe.as_deref_mut() {
                    index.tnum = add_op0(v, OP_NOOP as i32) as u32;
                    add_op3(v, OP_CREATEBTREE as i32, i_db, i_mem, BTREE_BLOBKEY as i32);
                }

                // Junta em `z_stmt` o texto completo do CREATE INDEX.
                debug_assert!(p_name.is_some() || p_start.is_none());
                let z_stmt: Option<Vec<u8>> = match (p_start, p_name) {
                    (Some(_), Some(pn)) => {
                        // Um índice com nome e CREATE INDEX explícito.
                        let n_tok = parse.s_last_token.z.len() as i32;
                        let mut n = parse.s_last_token.i_ofst - pn.i_ofst + n_tok;
                        let z = sql_span(parse, pn.i_ofst, n);
                        if n > 0 && at(&z, (n - 1) as usize) == b';' {
                            n -= 1;
                        }
                        let mut s = b"CREATE".to_vec();
                        if on_error != OE_NONE as i32 {
                            s.extend_from_slice(b" UNIQUE");
                        }
                        s.extend_from_slice(b" INDEX ");
                        s.extend_from_slice(&z[..(n.max(0) as usize).min(z.len())]);
                        Some(s)
                    }
                    // Um índice automático criado por uma restrição PRIMARY KEY ou UNIQUE.
                    _ => None,
                };

                // Acrescenta a entrada do índice em sqlite_schema.
                let fmt: Vec<u8> = [
                    b"INSERT INTO %Q.".as_slice(),
                    LEGACY_SCHEMA_TABLE,
                    b" VALUES('index',%Q,%Q,#%d,%Q);".as_slice(),
                ]
                .concat();
                nested_parse(
                    db,
                    parse,
                    &fmt,
                    &[
                        text_arg(&z_db_name),
                        text_arg(&index.z_name),
                        text_arg(&z_tab_name),
                        PrintfArg::Int(i_mem as i64),
                        PrintfArg::Text(z_stmt),
                    ],
                );

                // Enche o índice de dados e relê o esquema. Gera um `OP_Expire` para invalidar
                // todos os comandos pré-compilados.
                if is_tbl {
                    refill_index(db, parse, tab!('exit_create_index), &index, i_db, i_mem);
                    change_cookie(db, parse, i_db);
                    let z_where =
                        mprintf(b"name='%q' AND type='index'", &[text_arg(&index.z_name)]);
                    add_parse_schema_op(parse, db, i_db, z_where, 0);
                    if let Some(v) = parse.p_vdbe.as_deref_mut() {
                        add_op2(v, OP_EXPIRE as i32, 0, 1);
                    }
                }
                if let Some(v) = parse.p_vdbe.as_deref_mut() {
                    jump_here(v, index.tnum as i32);
                }
            }
        }
        if db.init.busy != 0 || !is_tbl {
            match target.take() {
                Some(IndexTarget::New(mut b)) => {
                    b.p_index.insert(0, Rc::new(index));
                    target = Some(IndexTarget::New(b));
                }
                Some(IndexTarget::Existing(t)) => {
                    // Só chega aqui lendo o esquema: a tabela é a do esquema e o índice entra
                    // nela. O `Rc` local é largado antes para `make_mut` alterar no lugar.
                    let z_name_tab = t.z_name.clone();
                    drop(t);
                    if let Some(slot) =
                        hash_find_mut(&mut db.dbs[i_db as usize].schema.tbl_hash, &z_name_tab)
                    {
                        let t = Rc::make_mut(slot);
                        t.p_index.insert(0, Rc::new(index));
                        reorder_replace_indexes(&mut t.p_index);
                    }
                }
                None => {}
            }
        } else if in_rename {
            debug_assert!(parse.p_new_index.is_empty());
            parse.p_new_index.insert(0, Box::new(index));
        }
    }

    // exit_create_index: devolve a tabela em construção ao `Parse`, com os índices REPLACE no
    // fim da lista. Não há `sqlite3FreeIndex`: o índice não ligado cai aqui, como `p_list`,
    // `p_pi_where` e `p_tbl_name`.
    if let Some(IndexTarget::New(mut b)) = target {
        reorder_replace_indexes(&mut b.p_index);
        parse.p_new_table = Some(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{SQLITE_AFF_INTEGER, SQLITE_AFF_TEXT};

    fn idx(on_error: u8, name: &str) -> Rc<Index> {
        Rc::new(Index { z_name: name.as_bytes().to_vec(), on_error, ..Index::default() })
    }

    #[test]
    fn replace_indexes_go_to_the_end() {
        let mut v = vec![idx(OE_REPLACE, "a"), idx(OE_ABORT, "b"), idx(OE_ABORT, "c")];
        reorder_replace_indexes(&mut v);
        let names: Vec<&[u8]> = v.iter().map(|x| x.z_name.as_slice()).collect();
        assert_eq!(names, vec![&b"b"[..], &b"c"[..], &b"a"[..]]);

        let mut v = vec![idx(OE_ABORT, "a"), idx(OE_REPLACE, "b"), idx(OE_ABORT, "c"), idx(OE_REPLACE, "d")];
        reorder_replace_indexes(&mut v);
        let names: Vec<&[u8]> = v.iter().map(|x| x.z_name.as_slice()).collect();
        assert_eq!(names, vec![&b"a"[..], &b"c"[..], &b"b"[..], &b"d"[..]]);
    }

    #[test]
    fn ident_put_quotes_when_needed() {
        let mut out = Vec::new();
        ident_put(&mut out, b"abc_1");
        assert_eq!(out, b"abc_1");
        let mut out = Vec::new();
        ident_put(&mut out, b"1abc");
        assert_eq!(out, b"\"1abc\"");
        let mut out = Vec::new();
        ident_put(&mut out, b"select");
        assert_eq!(out, b"\"select\"");
        let mut out = Vec::new();
        ident_put(&mut out, b"a\"b");
        assert_eq!(out, b"\"a\"\"b\"");
        let mut out = Vec::new();
        ident_put(&mut out, b"");
        assert_eq!(out, b"\"\"");
        assert_eq!(ident_length(b"a\"b"), 6);
    }

    #[test]
    fn create_table_stmt_matches_sqlite() {
        let col = |name: &[u8], aff: u8| Column {
            z_cn_name: [name, b"\0"].concat(),
            affinity: aff,
            ..Column::default()
        };
        let t = Table {
            z_name: b"t".to_vec(),
            a_col: vec![col(b"a", SQLITE_AFF_INTEGER), col(b"b c", SQLITE_AFF_TEXT), col(b"d", SQLITE_AFF_BLOB)],
            n_col: 3,
            ..Table::default()
        };
        assert_eq!(create_table_stmt(&t), b"CREATE TABLE t(a INT,\"b c\" TEXT,d)".to_vec());
    }

    #[test]
    fn index_helpers() {
        let mut p = allocate_index_object(3);
        assert_eq!((p.n_column, p.n_key_col), (3, 2));
        p.ai_column = vec![0, 2, -1];
        p.az_coll = vec![b"BINARY".to_vec(), b"NOCASE".to_vec(), b"BINARY".to_vec()];
        assert!(has_column(&p.ai_column, 2, 2));
        assert!(!has_column(&p.ai_column, 1, 2));
        p.idx_type = SQLITE_IDXTYPE_PRIMARYKEY;
        assert!(is_dup_column(&p, 3, &p, 1));
        resize_index_object(&mut p, 5);
        assert_eq!(p.n_column, 5);
        assert_eq!(p.ai_column.len(), 5);
        assert!(p.is_resized);
    }
}
