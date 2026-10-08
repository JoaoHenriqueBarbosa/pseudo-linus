//! `insert.c` (primeira metade): chunks `insert_c.000` a `insert_c.003` do SQLite 3.46.1.
//! `sqlite3OpenTable`, afinidades de índice e tabela, `sqlite3ComputeGeneratedColumns`, o
//! contador AUTOINCREMENT, `sqlite3MultiValues` e `sqlite3Insert`.
//!
//! Convenções (as mesmas de `build.rs`, `expr.rs` e `expr_code.rs`, ver CONVENTIONS.md):
//!
//! - Funções de código recebem `(db: &mut Connection, parse: &mut Parse, ...)`; o `pParse->db` some.
//!   O `Vdbe` é `parse.p_vdbe`; as funções do `vdbeaux` recebem `&mut Vdbe`.
//! - `Table` e `Index` do esquema são `Rc` imutáveis. Por isso o cache `Table.zColAff` e
//!   `Index.zColAff` do C não é gravado: `table_affinity_str` e `index_affinity_str` recalculam
//!   a cada chamada (a saída é a mesma, só o cache some).
//! - `sqlite3ComputeGeneratedColumns` marca `COLFLAG_NOTAVAIL` e `COLFLAG_BUSY` nas colunas da
//!   tabela, que aqui é imutável: o trabalho é feito numa cópia local da tabela (`w_tab`) e cada
//!   chamada de `expr_code_generated_column` recebe um `Rc` novo dessa cópia como tabela dona da
//!   árvore (mesma técnica de `expr_code_get_column_of_table`).
//! - Ramos `SQLITE_DEBUG` (inclusive `sqlite3VdbeReleaseRegisters`, que no Debian é macro vazia),
//!   `SQLITE_OMIT_*` e `TREETRACE_ENABLED` não existem.
//! - `sqlite3VdbeAddOpList` recebe `VdbeOpList` com `p1`, `p2` e `p3` de 8 bits; os valores
//!   reais dos registradores são gravados depois, em `Vdbe.a_op`, como no C.

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::insert`.
pub use crate::insert2::*;

use std::rc::Rc;

// Funções de outras fatias, chamadas pelo nome determinístico (assinaturas supostas no relatório).
pub(crate) use crate::vdbeaux::{vdbe_of_parse, current_addr};
use crate::auth::auth_check;
use crate::build::{
    begin_write_operation, column_expr, has_explicit_nulls, may_abort, primary_key_index,
    table_column_to_storage, table_lock, text_arg,
};
use crate::connection::{AutoincInfo, Connection, Parse};
use crate::consts::{
    COLFLAG_GENERATED, COLFLAG_NOINSERT, COLFLAG_STORED, COLFLAG_VIRTUAL, COLFLAG_NOTAVAIL,
    COLFLAG_BUSY, DBFLAG_SCHEMA_KNOWN_OK, DBFLAG_VACUUM, EP_SUBQUERY, EU4_EXPR, EU4_IDX,
    OE_ABORT, OE_DEFAULT, OPFLAG_APPEND, OP_ADDIMM, OP_AFFINITY, OP_CLOSE, OP_COLUMN, OP_COPY,
    OP_GOTO, OP_INITCOROUTINE, OP_INTEGER, OP_LE, OP_MAKERECORD,
    OP_MEMMAX, OP_MUSTBEINT, OP_NE, OP_NEWROWID, OP_NEXT, OP_NOTNULL, OP_NULL, OP_OPENEPHEMERAL,
    OP_OPENREAD, OP_OPENWRITE, OP_REWIND, OP_ROWID, OP_SCOPY, OP_SOFTNULL, OP_TYPECHECK,
    OP_VOPEN, OP_VUPDATE, OP_YIELD, OP_INSERT, OP_ISNULL, SF_MULTIVALUE, SF_NESTEDFROM,
    SF_VALUES, SQLITE_AFF_BLOB, SQLITE_AFF_INTEGER, SQLITE_AFF_NONE, SQLITE_AFF_NUMERIC,
    SQLITE_CORRUPT_SEQUENCE, SQLITE_COUNT_ROWS, SQLITE_FOREIGN_KEYS, SQLITE_INSERT, SQLITE_JUMPIFNULL,
    SRT_COROUTINE, TF_AUTOINCREMENT, TF_HAS_GENERATED, TF_HAS_HIDDEN, TF_HAS_STORED,
    TF_OOO_HIDDEN, TF_STRICT, TK_ALL, TK_COLUMN, TK_INSERT, TK_NULL, TK_SELECT, TRIGGER_AFTER,
    TRIGGER_BEFORE, WRC_CONTINUE, XN_EXPR, XN_ROWID,
};
use crate::expr::{
    expr_affinity, expr_alloc, expr_code, expr_code_expr_list, expr_code_factorable, expr_dup,
    get_temp_range, get_temp_reg, release_temp_range, release_temp_reg, src_list_dup,
};
use crate::expr_code::{expr_code_generated_column, expr_code_target, expr_is_constant, is_rowid};
use crate::fkey::fk_check;
use crate::prepare::{read_schema, schema_to_index};
use crate::printf::{PrintfArg, PrintfSrcAnon, PrintfSrcItem};
use crate::resolve::{name_context_new, resolve_expr_list_names};
use crate::select::{
    get_vdbe, select, select_dest_init, select_new, select_wrong_num_terms_error,
};
use crate::sqlite_int::{
    Expr, ExprList, IdList, Index, Select, SelectDest, SrcItem, SrcList, SrcU1, Table, Trigger,
    Upsert, Walker,
};
use crate::trigger::{code_row_trigger, triggers_exist};
use crate::upsert::upsert_analyze_target;
use crate::util::{error_msg, str_icmp};
use crate::vdbe_types::{Vdbe, VdbeOpList, P4};
use crate::vdbeaux::{
    add_op1, add_op2, add_op3, add_op4, add_op4_int, add_op_list, append_p4, change_p4,
    change_p4_vtab, change_p5, end_coroutine, explain, get_last_op, get_op_ref, has_sub_program,
    jump_here, load_string, make_label, resolve_label, set_p4_key_info, vdbe_comment,
    vdbe_goto,
};
use crate::vdbeaux3::vdbe_count_changes;
use crate::vtab::{get_vtable, vtab_make_writable};
use crate::walker::walk_expr;
use crate::delete::{code_change_count, is_read_only, src_list_lookup};
use crate::build2::view_get_column_names;

// ---------------------------------------------------------------------------------------------
// Auxiliares privados (não existem no C)
// ---------------------------------------------------------------------------------------------


/// O argumento `%S` do `printf` interno a partir de um `SrcItem`. Quando o item não tem alias
/// nem nome, o C olha o `Select` do item (ver `PrintfSrcAnon`).
fn src_item_arg(item: &SrcItem) -> PrintfArg {
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

/// O `n`-ésimo elo (a partir de 0) da cadeia de `Upsert` (`pNextUpsert`). O chamador só passa
/// índices que existem na cadeia.
fn upsert_nth_mut(p: &mut Upsert, n: usize) -> &mut Upsert {
    let mut cur = p;
    for _ in 0..n {
        cur = cur.p_next_upsert.as_deref_mut().expect("cadeia de upsert");
    }
    cur
}

/// `sqlite3ExprCodeFactorable(pParse, sqlite3ColumnExpr(pTab, &pTab->aCol[i]), iReg)`. O valor
/// DEFAULT vive na tabela (imutável), então a expressão é duplicada antes de ser gerada. Coluna
/// sem DEFAULT é o ponteiro nulo do C; `sqlite3ExprCodeTarget(NULL)` trata como `TK_NULL`, então
/// um nó `TK_NULL` gera o mesmo bytecode.
fn expr_code_column_default(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab: &Rc<Table>,
    i_col: usize,
    i_reg: i32,
) {
    let mut p_dflt = match column_expr(p_tab, &p_tab.a_col[i_col]) {
        Some(e) => expr_dup(Some(e), 0),
        None => expr_alloc(TK_NULL as i32, None, 0),
    };
    if let Some(e) = p_dflt.as_deref_mut() {
        expr_code_factorable(db, parse, e, i_reg, Some(p_tab));
    }
}

// ---------------------------------------------------------------------------------------------
// sqlite3OpenTable e afinidades (chunk 000)
// ---------------------------------------------------------------------------------------------

/// `sqlite3OpenTable`: gera o código que (1) trava a tabela `p_tab` e (2) a abre como o cursor
/// `i_cur`. Numa tabela WITHOUT ROWID quem se abre é o índice da PRIMARY KEY.
pub fn open_table(
    db: &mut Connection,
    parse: &mut Parse,
    i_cur: i32,
    i_db: i32,
    p_tab: &Table,
    opcode: u8,
) {
    debug_assert!(!p_tab.is_virtual());
    debug_assert!(parse.p_vdbe.is_some());
    debug_assert!(opcode == OP_OPENWRITE || opcode == OP_OPENREAD);
    if db.no_shared_cache == 0 {
        table_lock(db, parse, i_db, p_tab.tnum, opcode == OP_OPENWRITE, &p_tab.z_name);
    }
    if p_tab.has_rowid() {
        let v = vdbe_of_parse(parse);
        add_op4_int(v, opcode as i32, i_cur, p_tab.tnum as i32, i_db, p_tab.n_nv_col as i32);
        vdbe_comment(v, b"%s", &[text_arg(&p_tab.z_name)]);
    } else {
        let Some(p_pk) = primary_key_index(p_tab) else {
            return;
        };
        debug_assert!(p_pk.tnum == p_tab.tnum);
        add_op3(vdbe_of_parse(parse), opcode as i32, i_cur, p_pk.tnum as i32, i_db);
        set_p4_key_info(parse, db, p_pk);
        vdbe_comment(vdbe_of_parse(parse), b"%s", &[text_arg(&p_tab.z_name)]);
    }
}

/// `sqlite3IndexAffinityStr` (com `computeIndexAffStr`): a string de afinidades de colunas do
/// índice, um caractere por coluna (`A` BLOB, `B` TEXT, `C` NUMERIC, `D` INTEGER, `E` REAL). Um
/// `D` extra no fim cobre o rowid que aparece como última coluna de todo índice (já está em
/// `ai_column`). `p_tab` é a tabela dona do índice (o `Index` não tem `pTable`) e é também a
/// dona das expressões do índice. O cache `Index.zColAff` do C não é gravado (o `Index` é
/// imutável): se a string já foi calculada pelo ANALYZE ou pela criação, ela é devolvida.
pub fn index_affinity_str(p_idx: &Index, p_tab: &Rc<Table>) -> Vec<u8> {
    if let Some(z) = &p_idx.z_col_aff {
        return z.clone();
    }
    let mut z: Vec<u8> = Vec::with_capacity(p_idx.n_column as usize);
    for n in 0..p_idx.n_column as usize {
        let x = p_idx.ai_column[n];
        let mut aff: u8 = if x >= 0 {
            p_tab.a_col[x as usize].affinity
        } else if x == XN_ROWID {
            SQLITE_AFF_INTEGER
        } else {
            debug_assert!(x == XN_EXPR);
            debug_assert!(p_idx.b_has_expr);
            let e = p_idx
                .a_col_expr
                .as_deref()
                .and_then(|l| l.a.get(n))
                .and_then(|it| it.p_expr.as_deref());
            debug_assert!(e.is_some());
            e.map_or(0, |e| expr_affinity(e, Some(p_tab)))
        };
        if aff < SQLITE_AFF_BLOB {
            aff = SQLITE_AFF_BLOB;
        }
        if aff > SQLITE_AFF_NUMERIC {
            aff = SQLITE_AFF_NUMERIC;
        }
        z.push(aff);
    }
    z
}

/// `sqlite3TableAffinityStr`: a string de afinidades da tabela (só as colunas que não são
/// VIRTUAL), sem as afinidades BLOB do fim. Pode sair vazia.
pub fn table_affinity_str(p_tab: &Table) -> Vec<u8> {
    let mut z: Vec<u8> = p_tab
        .a_col
        .iter()
        .filter(|c| (c.col_flags & COLFLAG_VIRTUAL) == 0)
        .map(|c| c.affinity)
        .collect();
    while let Some(&last) = z.last() {
        if last > SQLITE_AFF_BLOB {
            break;
        }
        z.pop();
    }
    z
}

/// `sqlite3TableAffinity`: altera o bytecode para aplicar as afinidades das colunas aos valores
/// que vão formar uma linha da tabela `p_tab`.
///
/// Tabela comum: se a string de afinidades é vazia não faz nada; com `i_reg>0` gera um
/// `OP_Affinity` para os registradores a partir de `i_reg`; com `i_reg==0` grava a string no P4
/// do opcode anterior (um `OP_MakeRecord`).
///
/// Tabela STRICT: gera um `OP_TypeCheck`. Com `i_reg==0` o `OP_MakeRecord` anterior vira o
/// `OP_TypeCheck` (mesmos operandos) e um novo `OP_MakeRecord` é acrescentado depois dele.
pub fn table_affinity(v: &mut Vdbe, p_tab: &Rc<Table>, i_reg: i32) {
    if (p_tab.tab_flags & TF_STRICT) != 0 {
        if i_reg == 0 {
            // Move o opcode anterior (que deve ser OP_MakeRecord) uma posição para a frente e
            // insere um OP_TypeCheck onde o OP_MakeRecord estava.
            append_p4(v, P4::Table(Rc::clone(p_tab)));
            let Some(p_prev) = get_last_op(v) else {
                return;
            };
            debug_assert!(p_prev.opcode == OP_MAKERECORD);
            p_prev.opcode = OP_TYPECHECK;
            let (p1, p2, p3) = (p_prev.p1, p_prev.p2, p_prev.p3);
            add_op3(v, OP_MAKERECORD as i32, p1, p2, p3);
        } else {
            // Um OP_TypeCheck isolado.
            add_op2(v, OP_TYPECHECK as i32, i_reg, p_tab.n_nv_col as i32);
            append_p4(v, P4::Table(Rc::clone(p_tab)));
        }
        return;
    }
    let z_col_aff = match &p_tab.z_col_aff {
        Some(z) => z.clone(),
        None => table_affinity_str(p_tab),
    };
    let i = z_col_aff.len() as i32;
    if i != 0 {
        if i_reg != 0 {
            add_op4(v, OP_AFFINITY as i32, i_reg, i, 0, P4::Text(z_col_aff));
        } else {
            debug_assert!(get_last_op(v).map_or(true, |o| o.opcode == OP_MAKERECORD));
            change_p4(v, -1, P4::Text(z_col_aff));
        }
    }
}

/// `readsTable`: verdadeiro se a tabela `p_tab` do banco `i_db`, ou um dos seus índices, foi
/// aberta em algum ponto do programa. Serve para saber se `INSERT INTO <iDb, pTab> SELECT ...`
/// pode rodar sem uma tabela temporária para o resultado do SELECT.
fn reads_table(db: &Connection, parse: &Parse, i_db: i32, p_tab: &Table) -> bool {
    let Some(v) = parse.p_vdbe.as_deref() else {
        return false;
    };
    let i_end = v.n_op();
    let p_vtab = if p_tab.is_virtual() { get_vtable(db, p_tab) } else { None };
    for i in 1..i_end {
        let Some(p_op) = get_op_ref(v, i) else {
            continue;
        };
        if p_op.opcode == OP_OPENREAD && p_op.p3 == i_db {
            let tnum = p_op.p2 as u32;
            if tnum == p_tab.tnum {
                return true;
            }
            for p_index in p_tab.p_index.iter() {
                if tnum == p_index.tnum {
                    return true;
                }
            }
        }
        if p_op.opcode == OP_VOPEN {
            if let (P4::Vtab(id), Some(vt)) = (&p_op.p4, p_vtab) {
                if *id == vt {
                    return true;
                }
            }
        }
    }
    false
}

/// `exprColumnFlagUnion`: callback do walker que junta (OU) os `colFlags` de todas as colunas
/// referenciadas numa expressão CHECK ou de coluna gerada. O contexto do walker é o vetor de
/// `colFlags` da tabela (o `pWalker->u.pTab` do C): a tabela real é imutável, então quem chama
/// entrega uma fotografia dos flags de trabalho.
fn expr_column_flag_union(w: &mut Walker<Vec<u16>>, p_expr: &mut Expr) -> i32 {
    if p_expr.op == TK_COLUMN && p_expr.i_column >= 0 {
        debug_assert!((p_expr.i_column as usize) < w.u.len());
        let f = w.u.get(p_expr.i_column as usize).copied().unwrap_or(0);
        w.e_code |= f;
    }
    WRC_CONTINUE
}

/// `sqlite3ComputeGeneratedColumns`: todas as colunas regulares de `p_tab` já foram postas em
/// registradores a partir de `i_reg_store`. Os registradores das colunas STORED e VIRTUAL ainda
/// não foram inicializados; esta rotina volta e calcula esses valores a partir das colunas já
/// calculadas.
pub fn compute_generated_columns(
    db: &mut Connection,
    parse: &mut Parse,
    i_reg_store: i32,
    p_tab: &Rc<Table>,
) {
    debug_assert!((p_tab.tab_flags & TF_HAS_GENERATED) != 0);

    // Antes de calcular as colunas geradas, garante que a afinidade apropriada foi aplicada às
    // colunas regulares.
    table_affinity(vdbe_of_parse(parse), p_tab, i_reg_store);
    if (p_tab.tab_flags & TF_HAS_STORED) != 0 {
        if let Some(p_op) = get_last_op(vdbe_of_parse(parse)) {
            if p_op.opcode == OP_AFFINITY {
                // Troca o argumento do OP_Affinity por '@' (NONE) para todas as colunas
                // STORED: '@' é a afinidade sem efeito e essas colunas ainda não foram
                // calculadas.
                if let P4::Text(z_p4) = &mut p_op.p4 {
                    let mut ii = 0usize;
                    let mut jj = 0usize;
                    while jj < z_p4.len() {
                        let fl = p_tab.a_col.get(ii).map_or(0, |c| c.col_flags);
                        ii += 1;
                        if (fl & COLFLAG_VIRTUAL) != 0 {
                            continue;
                        }
                        if (fl & COLFLAG_STORED) != 0 {
                            z_p4[jj] = SQLITE_AFF_NONE;
                        }
                        jj += 1;
                    }
                }
            } else if p_op.opcode == OP_TYPECHECK {
                // Se o OP_TypeCheck existe porque a tabela é STRICT, o P3 indica que as
                // colunas geradas não devem ser conferidas.
                p_op.p3 = 1;
            }
        }
    }

    // Como várias colunas geradas podem se referir umas às outras, o algoritmo tem duas
    // passadas. Na primeira, marca todas as colunas geradas como "não disponíveis" (na cópia de
    // trabalho da tabela, ver o cabeçalho do módulo).
    let mut w_tab: Table = (**p_tab).clone();
    for col in w_tab.a_col.iter_mut() {
        if (col.col_flags & COLFLAG_GENERATED) != 0 {
            col.col_flags |= COLFLAG_NOTAVAIL;
        }
    }

    let mut w: Walker<Vec<u16>> = Walker::default();
    w.x_expr_callback = Some(expr_column_flag_union);
    w.x_select_callback = None;
    w.x_select_callback2 = None;

    // Na segunda passada, calcula o valor de cada coluna NOTAVAIL. O código companheiro do caso
    // TK_COLUMN de `expr_code_target` calcula as dependências e remove a marca quando preciso.
    parse.i_self_tab = -i_reg_store;
    let mut p_redo: Option<usize>;
    loop {
        let mut e_progress = false;
        p_redo = None;
        for i in 0..w_tab.a_col.len() {
            if (w_tab.a_col[i].col_flags & COLFLAG_NOTAVAIL) != 0 {
                w_tab.a_col[i].col_flags |= COLFLAG_BUSY;
                w.u = w_tab.a_col.iter().map(|c| c.col_flags).collect();
                w.e_code = 0;
                let mut p_e = column_expr(&w_tab, &w_tab.a_col[i]).and_then(|e| expr_dup(Some(e), 0));
                walk_expr(&mut w, p_e.as_deref_mut());
                w_tab.a_col[i].col_flags &= !COLFLAG_BUSY;
                if (w.e_code & COLFLAG_NOTAVAIL) != 0 {
                    p_redo = Some(i);
                    continue;
                }
                e_progress = true;
                debug_assert!((w_tab.a_col[i].col_flags & COLFLAG_GENERATED) != 0);
                let x = table_column_to_storage(p_tab, i as i16) as i32 + i_reg_store;
                let p_work = Rc::new(w_tab.clone());
                expr_code_generated_column(db, parse, &p_work, i, x);
                w_tab.a_col[i].col_flags &= !COLFLAG_NOTAVAIL;
            }
        }
        if !(p_redo.is_some() && e_progress) {
            break;
        }
    }
    if let Some(i) = p_redo {
        error_msg(db, parse, b"generated column loop on \"%s\"", &[text_arg(&w_tab.a_col[i].z_cn_name)]);
    }
    parse.i_self_tab = 0;
}

// ---------------------------------------------------------------------------------------------
// AUTOINCREMENT (chunk 001)
// ---------------------------------------------------------------------------------------------

/// `autoIncBegin`: localiza ou cria o `AutoincInfo` da tabela `p_tab`, que está no banco `i_db`.
/// Devolve o registrador que guarda o maior rowid, ou zero se a tabela não é AUTOINCREMENT (ou
/// se é um VACUUM, que não deve atualizar os contadores).
///
/// Há no máximo um `AutoincInfo` por tabela, mesmo que ela seja incrementada várias vezes por
/// causa de INSERTs dentro de gatilhos. São reservados quatro registradores consecutivos: o nome
/// da tabela, o maior ROWID, o rowid em sqlite_sequence e o valor original do maior ROWID (ou
/// NULL). O segundo é o devolvido.
pub(crate) fn auto_inc_begin(db: &Connection, parse: &mut Parse, i_db: i32, p_tab: &Rc<Table>) -> i32 {
    let mut mem_id = 0;
    debug_assert!(db.dbs[i_db as usize].schema.id.0 != 0);
    if (p_tab.tab_flags & TF_AUTOINCREMENT) != 0 && (db.m_db_flags & DBFLAG_VACUUM) == 0 {
        // Confere que a tabela sqlite_sequence existe e é uma tabela comum com rowid e
        // exatamente duas colunas (ticket d8dc2b3a58cd5dc2918a1d4acb de 2018-05-23).
        let seq_ok = match db.dbs[i_db as usize].schema.p_seq_tab.as_ref() {
            Some(s) => s.has_rowid() && !s.is_virtual() && s.n_col == 2,
            None => false,
        };
        if !seq_ok {
            parse.n_err += 1;
            parse.rc = SQLITE_CORRUPT_SEQUENCE;
            return 0;
        }
        let p_toplevel = parse.toplevel_mut();
        let found = p_toplevel
            .p_ainc
            .iter()
            .position(|p| p.p_tab.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_tab)));
        match found {
            Some(k) => mem_id = p_toplevel.p_ainc[k].reg_ctr,
            None => {
                p_toplevel.n_mem += 1; // registrador do nome da tabela
                p_toplevel.n_mem += 1;
                let reg_ctr = p_toplevel.n_mem; // registrador do maior rowid
                p_toplevel.n_mem += 2; // rowid em sqlite_sequence e o valor original
                // A lista do C cresce pela cabeça: o mais novo vem primeiro.
                p_toplevel.p_ainc.insert(0, AutoincInfo { p_tab: Some(Rc::clone(p_tab)), i_db, reg_ctr });
                mem_id = reg_ctr;
            }
        }
    }
    mem_id
}

/// As instruções que inicializam os registradores do contador de uma tabela AUTOINCREMENT
/// (`autoInc` de `sqlite3AutoincrementBegin`). Os operandos reais são gravados depois.
const AUTO_INC: [VdbeOpList; 12] = [
    /* 0  */ VdbeOpList { opcode: OP_NULL, p1: 0, p2: 0, p3: 0 },
    /* 1  */ VdbeOpList { opcode: OP_REWIND, p1: 0, p2: 10, p3: 0 },
    /* 2  */ VdbeOpList { opcode: OP_COLUMN, p1: 0, p2: 0, p3: 0 },
    /* 3  */ VdbeOpList { opcode: OP_NE, p1: 0, p2: 9, p3: 0 },
    /* 4  */ VdbeOpList { opcode: OP_ROWID, p1: 0, p2: 0, p3: 0 },
    /* 5  */ VdbeOpList { opcode: OP_COLUMN, p1: 0, p2: 1, p3: 0 },
    /* 6  */ VdbeOpList { opcode: OP_ADDIMM, p1: 0, p2: 0, p3: 0 },
    /* 7  */ VdbeOpList { opcode: OP_COPY, p1: 0, p2: 0, p3: 0 },
    /* 8  */ VdbeOpList { opcode: OP_GOTO, p1: 0, p2: 11, p3: 0 },
    /* 9  */ VdbeOpList { opcode: OP_NEXT, p1: 0, p2: 2, p3: 0 },
    /* 10 */ VdbeOpList { opcode: OP_INTEGER, p1: 0, p2: 0, p3: 0 },
    /* 11 */ VdbeOpList { opcode: OP_CLOSE, p1: 0, p2: 0, p3: 0 },
];

/// `sqlite3AutoincrementBegin`: gera o código que inicializa todos os registradores usados pelo
/// rastreador de AUTOINCREMENT. Nunca é chamada durante a geração de gatilhos: só do nível mais
/// alto.
pub fn auto_increment_begin(db: &mut Connection, parse: &mut Parse) {
    // Esta rotina nunca é chamada durante a geração de gatilhos, só do nível mais alto.
    debug_assert!(parse.p_trigger_tab.is_none());
    debug_assert!(parse.p_toplevel.is_none());
    debug_assert!(parse.p_vdbe.is_some());
    for k in 0..parse.p_ainc.len() {
        let (i_db, mem_id, z_name) = {
            let p = &parse.p_ainc[k];
            (p.i_db, p.reg_ctr, p.p_tab.as_ref().map(|t| t.z_name.clone()).unwrap_or_default())
        };
        let Some(p_seq_tab) = db.dbs[i_db as usize].schema.p_seq_tab.clone() else {
            break;
        };
        open_table(db, parse, 0, i_db, &p_seq_tab, OP_OPENREAD);
        let v = vdbe_of_parse(parse);
        load_string(v, mem_id - 1, &z_name);
        let Some(a) = add_op_list(v, &AUTO_INC, 0) else {
            break;
        };
        v.a_op[a].p2 = mem_id;
        v.a_op[a].p3 = mem_id + 2;
        v.a_op[a + 2].p3 = mem_id;
        v.a_op[a + 3].p1 = mem_id - 1;
        v.a_op[a + 3].p3 = mem_id;
        v.a_op[a + 3].p5 = SQLITE_JUMPIFNULL as u16;
        v.a_op[a + 4].p2 = mem_id + 1;
        v.a_op[a + 5].p3 = mem_id;
        v.a_op[a + 6].p1 = mem_id;
        v.a_op[a + 7].p2 = mem_id + 2;
        v.a_op[a + 7].p1 = mem_id;
        v.a_op[a + 10].p2 = mem_id;
        if parse.n_tab == 0 {
            parse.n_tab = 1;
        }
    }
}

/// `autoIncStep`: atualiza o maior rowid de um cálculo de AUTOINCREMENT. Deve ser chamada
/// quando o registrador `reg_rowid` guarda um rowid novo que está para ser inserido; se ele é
/// maior que o do registrador `mem_id`, este é atualizado.
pub(crate) fn auto_inc_step(parse: &mut Parse, mem_id: i32, reg_rowid: i32) {
    if mem_id > 0 {
        add_op2(vdbe_of_parse(parse), OP_MEMMAX as i32, mem_id, reg_rowid);
    }
}

/// As instruções que gravam o maior rowid de volta em sqlite_sequence (`autoIncEnd`).
const AUTO_INC_END: [VdbeOpList; 5] = [
    /* 0 */ VdbeOpList { opcode: OP_NOTNULL, p1: 0, p2: 2, p3: 0 },
    /* 1 */ VdbeOpList { opcode: OP_NEWROWID, p1: 0, p2: 0, p3: 0 },
    /* 2 */ VdbeOpList { opcode: OP_MAKERECORD, p1: 0, p2: 2, p3: 0 },
    /* 3 */ VdbeOpList { opcode: OP_INSERT, p1: 0, p2: 0, p3: 0 },
    /* 4 */ VdbeOpList { opcode: OP_CLOSE, p1: 0, p2: 0, p3: 0 },
];

/// `sqlite3AutoincrementEnd` (com `autoIncrementEnd`): gera o código que escreve de volta, no
/// registrador de sqlite_sequence, os maiores rowids. Todo comando que pode fazer INSERT numa
/// tabela AUTOINCREMENT (direto ou por gatilhos) chama isto logo antes do código de saída.
pub fn auto_increment_end(db: &mut Connection, parse: &mut Parse) {
    if parse.p_ainc.is_empty() {
        return;
    }
    debug_assert!(parse.p_vdbe.is_some());
    for k in 0..parse.p_ainc.len() {
        let (i_db, mem_id) = (parse.p_ainc[k].i_db, parse.p_ainc[k].reg_ctr);
        let i_rec = get_temp_reg(parse);
        let Some(p_seq_tab) = db.dbs[i_db as usize].schema.p_seq_tab.clone() else {
            break;
        };
        let v = vdbe_of_parse(parse);
        let cur = v.n_op();
        add_op3(v, OP_LE as i32, mem_id + 2, cur + 7, mem_id);
        open_table(db, parse, 0, i_db, &p_seq_tab, OP_OPENWRITE);
        let v = vdbe_of_parse(parse);
        let Some(a) = add_op_list(v, &AUTO_INC_END, 0) else {
            break;
        };
        v.a_op[a].p1 = mem_id + 1;
        v.a_op[a + 1].p2 = mem_id + 1;
        v.a_op[a + 2].p1 = mem_id - 1;
        v.a_op[a + 2].p3 = i_rec;
        v.a_op[a + 3].p2 = i_rec;
        v.a_op[a + 3].p3 = mem_id + 1;
        v.a_op[a + 3].p5 = OPFLAG_APPEND as u16;
        release_temp_reg(parse, i_rec);
    }
}

// ---------------------------------------------------------------------------------------------
// VALUES de várias linhas (chunk 001)
// ---------------------------------------------------------------------------------------------

/// `sqlite3MultiValuesEnd`: se `p_val` é um `Select` devolvido por `multi_values` que pôde usar a
/// otimização de co-rotina, termina de gerar a co-rotina.
pub fn multi_values_end(parse: &mut Parse, p_val: &Select) {
    if let Some(p_item) = p_val.p_src.as_deref().and_then(|s| s.a.first()) {
        end_coroutine(parse, p_item.reg_return);
        jump_here(vdbe_of_parse(parse), p_item.addr_fill_sub - 1);
    }
}

/// `exprListIsConstant`: verdadeiro se todas as expressões da lista são constantes.
fn expr_list_is_constant(db: &mut Connection, parse: &mut Parse, p_row: &mut ExprList) -> bool {
    for it in p_row.a.iter_mut() {
        if 0 == expr_is_constant(Some((&mut *db, &mut *parse)), it.p_expr.as_deref_mut()) {
            return false;
        }
    }
    true
}

/// `exprListIsNoAffinity`: verdadeiro se todas as expressões da lista são constantes e sem
/// afinidade.
fn expr_list_is_no_affinity(db: &mut Connection, parse: &mut Parse, p_row: &mut ExprList) -> bool {
    if !expr_list_is_constant(db, parse, p_row) {
        return false;
    }
    for it in p_row.a.iter() {
        if let Some(p_expr) = it.p_expr.as_deref() {
            debug_assert!(p_expr.op != crate::consts::TK_RAISE);
            debug_assert!(p_expr.aff_expr == 0);
            if 0 != expr_affinity(p_expr, None) {
                return false;
            }
        }
    }
    true
}

/// `sqlite3MultiValues`: chamada pelo analisador para a segunda linha e as seguintes de uma
/// cláusula VALUES de várias linhas. `p_left` é a parte do VALUES já analisada e `p_row` o vetor
/// de valores da linha nova. O `Select` devolvido representa o VALUES completo, com a linha nova.
///
/// Há dois jeitos de conseguir isso: a codificação incremental de uma co-rotina (método da
/// "co-rotina") ou um `Select` equivalente a `pLeft UNION ALL SELECT pRow` (método do "UNION
/// ALL"). Com muitas linhas o composto consome muita memória. Na co-rotina, cada linha é gerada
/// dentro da co-rotina assim que passa por esta função, e o `Select` devolvido equivale a
/// `SELECT * FROM (Select que lê a co-rotina)`.
///
/// A co-rotina é usada na maioria dos casos. Exceções: (a) o comando tem WITH (a co-rotina usa
/// um registrador fixo nos `OP_Yield`, então dois cursores não poderiam percorrê-la ao mesmo
/// tempo); (b) o esquema está sendo lido (não há VM em geração); (c) há expressões não
/// constantes no VALUES; (d) algum valor da primeira linha tem afinidade (um CAST), porque as
/// regras de `sqlite3SubqueryColumnTypes()` precisam ver todos os valores da coluna juntos.
pub fn multi_values(
    db: &mut Connection,
    parse: &mut Parse,
    mut p_left: Box<Select>,
    mut p_row: Box<ExprList>,
) -> Box<Select> {
    let left_n_src = p_left.p_src.as_deref().map_or(0, |s| s.a.len());
    let left_no_affinity = |db: &mut Connection, parse: &mut Parse, l: &mut Select| -> bool {
        match l.p_e_list.as_deref_mut() {
            Some(e) => expr_list_is_no_affinity(db, parse, e),
            None => true,
        }
    };
    if parse.b_has_with != 0                                        // condição (a)
        || db.init.busy != 0                                        // condição (b)
        || !expr_list_is_constant(db, parse, &mut p_row)            // condição (c)
        || (left_n_src == 0 && !left_no_affinity(db, parse, &mut p_left)) // condição (d)
        || parse.in_special_parse()
    {
        // O método da co-rotina não serve. Recai no UNION ALL.
        let mut f = SF_VALUES | SF_MULTIVALUE;
        if left_n_src > 0 {
            multi_values_end(parse, &p_left);
            f = SF_VALUES;
        } else if p_left.p_prior.is_some() {
            // Aqui o SF_MultiValue só vale se já estava ligado em pLeft.
            f &= p_left.sel_flags;
        }
        let p_select = select_new(parse, Some(p_row), None, None, None, None, None, f, None);
        p_left.sel_flags &= !SF_MULTIVALUE;
        if let Some(mut p_select) = p_select {
            p_select.op = TK_ALL;
            p_select.p_prior = Some(p_left);
            p_left = p_select;
        }
    } else {
        if left_n_src == 0 {
            // A co-rotina ainda não começou e o `Select` especial que a acessa ainda não foi
            // criado: este bloco faz as duas coisas.
            get_vdbe(db, parse);
            let p_ret = select_new(parse, None, None, None, None, None, None, 0, None);

            // Garante que o esquema foi lido, para ter a codificação de texto correta.
            if (db.m_db_flags & DBFLAG_SCHEMA_KNOWN_OK) == 0 {
                read_schema(db, parse);
            }

            let Some(mut p_ret) = p_ret else {
                return p_left;
            };
            let mut dest = SelectDest::default();
            p_ret.p_src.get_or_insert_with(Default::default).a.push(SrcItem::default());
            p_ret.p_prior = p_left.p_prior.take();
            p_ret.op = p_left.op;
            if p_ret.p_prior.is_some() {
                p_ret.sel_flags |= SF_VALUES;
            }
            p_left.op = TK_SELECT;
            debug_assert!(!p_left.has_next);
            debug_assert!(!p_ret.has_next);
            let addr_fill_sub = vdbe_of_parse(parse).n_op() + 1;
            parse.n_mem += 1;
            let reg_return = parse.n_mem;
            add_op3(vdbe_of_parse(parse), OP_INITCOROUTINE as i32, reg_return, 0, addr_fill_sub);
            select_dest_init(&mut dest, SRT_COROUTINE as i32, reg_return);

            // Aloca os registradores da saída da co-rotina de modo que haja dois registradores
            // sem uso imediatamente antes dos usados por ela. Isso deixa o código de
            // `sqlite3Insert()` usar esses registradores direto, sem copiar a saída da
            // co-rotina para outro vetor.
            dest.i_sdst = parse.n_mem + 3;
            dest.n_sdst = p_left.p_e_list.as_deref().map_or(0, |l| l.a.len() as i32);
            parse.n_mem += 2 + dest.n_sdst;

            p_left.sel_flags |= SF_MULTIVALUE;
            select(db, parse, &mut p_left, &mut dest);
            {
                let p = &mut p_ret.p_src.as_deref_mut().expect("pRet->pSrc").a[0];
                p.fg.via_coroutine = true;
                p.addr_fill_sub = addr_fill_sub;
                p.reg_return = reg_return;
                p.i_cursor = -1;
                p.u1 = SrcU1::NRow(2);
                p.p_select = Some(p_left);
                p.reg_result = dest.i_sdst;
            }
            debug_assert!(parse.n_err != 0 || dest.i_sdst > 0);
            p_left = p_ret;
        } else {
            let p = &mut p_left.p_src.as_deref_mut().expect("pLeft->pSrc").a[0];
            debug_assert!(!p.fg.is_tab_func && !p.fg.is_indexed_by);
            if let SrcU1::NRow(n) = &mut p.u1 {
                *n += 1;
            }
        }

        if parse.n_err == 0 {
            let (n_sel_expr, reg_result, reg_return) = {
                let p = &p_left.p_src.as_deref().expect("pLeft->pSrc").a[0];
                (
                    p.p_select.as_deref().and_then(|s| s.p_e_list.as_deref()).map_or(0, |l| l.a.len()),
                    p.reg_result,
                    p.reg_return,
                )
            };
            if n_sel_expr != p_row.a.len() {
                if let Some(s) = p_left
                    .p_src
                    .as_deref()
                    .and_then(|s| s.a.first())
                    .and_then(|p| p.p_select.as_deref())
                {
                    select_wrong_num_terms_error(db, parse, s.op, s.sel_flags);
                }
            } else {
                expr_code_expr_list(db, parse, &mut p_row, reg_result, 0, 0, None);
                add_op1(vdbe_of_parse(parse), OP_YIELD as i32, reg_return);
            }
        }
        drop(p_row);
    }

    p_left
}

// ---------------------------------------------------------------------------------------------
// sqlite3Insert (chunks 002 e 003)
// ---------------------------------------------------------------------------------------------

/// `sqlite3Insert`: trata o SQL de uma das formas
///
/// ```text
/// insert into TABELA (IDLIST) values(EXPRLIST),(EXPRLIST),...
/// insert into TABELA (IDLIST) select
/// insert into TABELA (IDLIST) default values
/// ```
///
/// O IDLIST depois do nome da tabela é opcional; omitido, vale a lista de todas as colunas não
/// ocultas. Ele chega em `p_column` (`None` se omitido). `p_select` traz os valores das duas
/// primeiras formas (um VALUES é só abreviação de um SELECT sem FROM); `None` é a forma DEFAULT
/// VALUES.
///
/// O código gerado segue um de quatro moldes. O 1o é o INSERT simples de um VALUES de uma
/// linha: o código roda uma vez, de cima a baixo (abre a tabela e seus índices, põe as
/// expressões em registradores, grava o registro, limpa). Os três restantes supõem
/// `INSERT INTO <tabela> SELECT ...`. O 2o, quando o SELECT é `SELECT * FROM <tabela2>` sem
/// WHERE, LIMIT, GROUP BY nem ORDER BY, com tabelas distintas de esquemas idênticos (índices
/// inclusive), copia os registros crus (ver `xfer_optimization`). O 3o vale quando o 2o não vale
/// e o SELECT nunca lê a tabela: uma co-rotina X produz as linhas do SELECT, abre-se a tabela
/// para escrita e um laço `yield X` insere cada linha. O 4o vale quando o SELECT lê a própria
/// tabela: o resultado vai antes para uma tabela temporária, que depois é percorrida para
/// inserir.
///
/// Todos os argumentos de posse (`p_tab_list`, `p_select`, `p_column`, `p_upsert`) são
/// consumidos: o `Drop` faz o `insert_cleanup` do C.
pub fn insert(
    db: &mut Connection,
    parse: &mut Parse,
    p_tab_list: Option<Box<SrcList>>,
    p_select: Option<Box<Select>>,
    p_column: Option<Box<IdList>>,
    on_error: i32,
    p_upsert: Option<Box<Upsert>>,
) {
    let mut p_select = p_select;
    let mut p_column = p_column;
    let mut p_upsert = p_upsert;
    let mut p_list: Option<Box<ExprList>> = None; // lista de VALUES() a inserir
    let Some(mut p_tab_list) = p_tab_list else {
        return;
    };

    'insert_cleanup: {
        if parse.n_err != 0 {
            break 'insert_cleanup;
        }
        let mut dest = SelectDest::default(); // destino do SELECT do lado direito do INSERT
        dest.i_sd_parm = 0;

        // Se o `Select` é só uma lista VALUES() de uma linha (o caso comum), guarda a linha de
        // valores e descarta as demais partes (sem uso) do `Select`.
        if let Some(sel) = p_select.as_deref_mut() {
            if (sel.sel_flags & SF_VALUES) != 0 && sel.p_prior.is_none() {
                p_list = sel.p_e_list.take();
                p_select = None;
            }
        }

        // Localiza a tabela em que vamos inserir as informações novas.
        debug_assert!(p_tab_list.a.len() == 1);
        let Some(mut p_tab) = src_list_lookup(db, parse, &mut p_tab_list) else {
            break 'insert_cleanup;
        };
        let i_db = schema_to_index(db, p_tab.p_schema);
        debug_assert!((i_db as usize) < db.dbs.len());
        let z_db_s_name = db.dbs[i_db as usize].z_db_s_name.clone();
        if auth_check(db, parse, SQLITE_INSERT, Some(&p_tab.z_name), None, Some(&z_db_s_name)) != 0 {
            break 'insert_cleanup;
        }
        let without_rowid = !p_tab.has_rowid();

        // Descobre se há gatilhos e se a tabela é uma view.
        let mut tmask: i32 = 0; // máscara dos tempos dos gatilhos
        let p_trigger: Vec<Rc<Trigger>> =
            triggers_exist(db, parse, &p_tab, TK_INSERT as i32, None, &mut tmask);
        let has_trigger = !p_trigger.is_empty();
        let is_view = p_tab.is_view();
        debug_assert!((has_trigger && tmask != 0) || (!has_trigger && tmask == 0));

        // Se `p_tab` é uma view, garante que foi inicializada. `view_get_column_names` não faz
        // nada se não é view.
        if view_get_column_names(db, parse, &mut p_tab) != 0 {
            break 'insert_cleanup;
        }

        // Não se insere numa tabela só de leitura.
        if is_read_only(db, parse, &p_tab, &p_trigger) != 0 {
            break 'insert_cleanup;
        }

        // Aloca um VDBE.
        get_vdbe(db, parse);
        if parse.nested == 0 {
            vdbe_count_changes(vdbe_of_parse(parse));
        }
        begin_write_operation(db, parse, (p_select.is_some() || has_trigger) as i32, i_db);

        let mut reg_row_count: i32 = 0; // célula de memória do contador de linhas
        'insert_end: {
            // Se o comando é `INSERT INTO <tabela1> SELECT * FROM <tabela2>;`, otimizações
            // especiais tornam a transferência muito rápida e reduzem a fragmentação dos
            // índices. Este é o 2o molde.
            if p_column.is_none() && !has_trigger {
                if let Some(sel) = p_select.as_deref() {
                    if xfer_optimization(db, parse, &p_tab, sel, on_error, i_db) {
                        debug_assert!(p_list.is_none());
                        break 'insert_end;
                    }
                }
            }

            // Se a tabela é AUTOINCREMENT, procura o número de sequência em sqlite_sequence e o
            // guarda na célula de memória `reg_autoinc`.
            let reg_autoinc = auto_inc_begin(db, parse, i_db, &p_tab);

            // Aloca um bloco de registradores para o rowid e os valores de todas as colunas da
            // linha nova.
            let mut reg_ins = parse.n_mem + 1; // bloco de regs com rowid+dados inseridos
            let mut reg_rowid = reg_ins; // registradores do rowid de inserção
            parse.n_mem += p_tab.n_col as i32 + 1;
            if p_tab.is_virtual() {
                reg_rowid += 1;
                parse.n_mem += 1;
            }
            let mut reg_data = reg_rowid + 1; // registrador do primeiro dado a inserir

            // Se o INSERT trouxe um IDLIST, confere que todos os elementos são colunas da tabela
            // e lembra os índices das colunas.
            //
            // Se a tabela tem uma coluna INTEGER PRIMARY KEY e ela aparece no IDLIST, grava em
            // `ipk_column` o índice dela no IDLIST (e não na tabela original, cujo índice é
            // `pTab->iPKey`). Depois do laço, `ipk_column == -1` quer dizer que a chave inteira
            // não foi especificada: a tabela é WITHOUT ROWID ou gerará a chave sozinha.
            //
            // `b_id_list_in_order` é verdadeiro se as colunas do IDLIST estão na ordem de
            // armazenamento, o que dispensa embaralhá-las. Falso negativo é inofensivo; falso
            // positivo corromperia o banco.
            let mut ipk_column: i32 = -1; // coluna que é a INTEGER PRIMARY KEY
            let mut b_id_list_in_order = (p_tab.tab_flags & (TF_OOO_HIDDEN | TF_HAS_STORED)) == 0;
            let n_tab_col = p_tab.n_col as usize;
            if let Some(col) = p_column.as_deref_mut() {
                debug_assert!(col.e_u4 != EU4_EXPR);
                col.e_u4 = EU4_IDX;
                for it in col.a.iter_mut() {
                    it.idx = -1;
                }
                for i in 0..col.a.len() {
                    let mut found = false;
                    for j in 0..n_tab_col {
                        let z_name = col.a[i].z_name.as_deref().unwrap_or(&[]);
                        if str_icmp(z_name, &p_tab.a_col[j].z_cn_name) == 0 {
                            col.a[i].idx = j as i32;
                            if i != j {
                                b_id_list_in_order = false;
                            }
                            if j as i32 == p_tab.i_p_key as i32 {
                                ipk_column = i as i32;
                                debug_assert!(!without_rowid);
                            }
                            if (p_tab.a_col[j].col_flags & (COLFLAG_STORED | COLFLAG_VIRTUAL)) != 0 {
                                error_msg(
                                    db,
                                    parse,
                                    b"cannot INSERT into generated column \"%s\"",
                                    &[text_arg(&p_tab.a_col[j].z_cn_name)],
                                );
                                break 'insert_cleanup;
                            }
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        let z_name = col.a[i].z_name.as_deref().unwrap_or(&[]);
                        if is_rowid(z_name) && !without_rowid {
                            ipk_column = i as i32;
                            b_id_list_in_order = false;
                        } else {
                            error_msg(
                                db,
                                parse,
                                b"table %S has no column named %s",
                                &[src_item_arg(&p_tab_list.a[0]), text_arg(z_name)],
                            );
                            parse.check_schema = 1;
                            break 'insert_cleanup;
                        }
                    }
                }
            }

            // Descobre quantas colunas de dados vieram. Se vêm de um SELECT, gera uma co-rotina
            // que produz uma linha do SELECT a cada invocação; ela é o cabeçalho comum dos
            // moldes 3 e 4.
            let n_column: i32; // número de colunas dos dados
            let mut reg_from_select: i32 = 0; // registrador-base dos dados vindos do SELECT
            let mut src_tab: i32 = 0; // os dados vêm deste cursor temporário se >= 0
            let mut use_temp_table = false; // guarda o resultado do SELECT numa tabela temporária
            if let Some(sel) = p_select.as_deref_mut() {
                // Os dados vêm de um SELECT ou de um VALUES de várias linhas. Gera uma co-rotina
                // para rodar o SELECT.
                let is_coroutine = sel.p_src.as_deref().map_or(false, |s| {
                    s.a.len() == 1 && s.a[0].fg.via_coroutine
                }) && sel.p_prior.is_none();
                if is_coroutine {
                    let p_item = &sel.p_src.as_deref().expect("pSelect->pSrc").a[0];
                    dest.i_sd_parm = p_item.reg_return;
                    reg_from_select = p_item.reg_result;
                    n_column = p_item
                        .p_select
                        .as_deref()
                        .and_then(|s| s.p_e_list.as_deref())
                        .map_or(0, |l| l.a.len() as i32);
                    explain(parse, db, false, b"SCAN %S", &[src_item_arg(p_item)]);
                    if b_id_list_in_order && n_column == p_tab.n_col as i32 {
                        reg_data = reg_from_select;
                        reg_rowid = reg_data - 1;
                        reg_ins = reg_rowid - if p_tab.is_virtual() { 1 } else { 0 };
                    }
                } else {
                    parse.n_mem += 1;
                    let reg_yield = parse.n_mem;
                    let addr_top = vdbe_of_parse(parse).n_op() + 1; // topo da co-rotina
                    add_op3(vdbe_of_parse(parse), OP_INITCOROUTINE as i32, reg_yield, 0, addr_top);
                    select_dest_init(&mut dest, SRT_COROUTINE as i32, reg_yield);
                    dest.i_sdst = if b_id_list_in_order { reg_data } else { 0 };
                    dest.n_sdst = p_tab.n_col as i32;
                    let rc = select(db, parse, sel, &mut dest);
                    reg_from_select = dest.i_sdst;
                    if rc != 0 || parse.n_err != 0 {
                        break 'insert_cleanup;
                    }
                    end_coroutine(parse, reg_yield);
                    jump_here(vdbe_of_parse(parse), addr_top - 1); // rótulo B:
                    debug_assert!(sel.p_e_list.is_some());
                    n_column = sel.p_e_list.as_deref().map_or(0, |l| l.a.len() as i32);
                }

                // `use_temp_table` fica verdadeiro se o resultado do SELECT deve ir para uma
                // tabela temporária (molde 4), e falso se cada linha pode ser escrita direto na
                // tabela de destino (molde 3). Precisa de tabela temporária se a tabela sendo
                // alterada também é lida pelo SELECT, e no caso de gatilhos de linha.
                if has_trigger || reads_table(db, parse, i_db, &p_tab) {
                    use_temp_table = true;
                }

                if use_temp_table {
                    // Invoca a co-rotina para extrair a informação do SELECT e a acrescentar
                    // à tabela transitória `src_tab` (4o molde):
                    //
                    //      B: abre a tabela temporária
                    //      L: yield X, vai para M no EOF
                    //         insere a linha de R..R+n na tabela temporária
                    //         goto L
                    //      M: ...
                    src_tab = parse.n_tab;
                    parse.n_tab += 1;
                    let reg_rec = get_temp_reg(parse); // registrador do registro empacotado
                    let reg_temp_rowid = get_temp_reg(parse); // registrador do ROWID temporário
                    let v = vdbe_of_parse(parse);
                    add_op2(v, OP_OPENEPHEMERAL as i32, src_tab, n_column);
                    let addr_l = add_op1(v, OP_YIELD as i32, dest.i_sd_parm); // rótulo L
                    add_op3(v, OP_MAKERECORD as i32, reg_from_select, n_column, reg_rec);
                    add_op2(v, OP_NEWROWID as i32, src_tab, reg_temp_rowid);
                    add_op3(v, OP_INSERT as i32, src_tab, reg_rec, reg_temp_rowid);
                    vdbe_goto(v, addr_l);
                    jump_here(v, addr_l);
                    release_temp_reg(parse, reg_rec);
                    release_temp_reg(parse, reg_temp_rowid);
                }
            } else {
                // Este é o caso em que os dados do INSERT vêm de um VALUES de uma linha.
                let mut s_nc = name_context_new();
                src_tab = -1;
                debug_assert!(!use_temp_table);
                if let Some(l) = p_list.as_deref_mut() {
                    n_column = l.a.len() as i32;
                    if resolve_expr_list_names(db, parse, &mut s_nc, Some(l)) != 0 {
                        break 'insert_cleanup;
                    }
                } else {
                    n_column = 0;
                }
            }

            // Se não há IDLIST mas a tabela tem chave primária inteira, `ipk_column` recebe o
            // índice da coluna da chave primária inteira na definição original da tabela.
            if p_column.is_none() && n_column > 0 {
                ipk_column = p_tab.i_p_key as i32;
                if ipk_column >= 0 && (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
                    let mut i = ipk_column - 1;
                    while i >= 0 {
                        if (p_tab.a_col[i as usize].col_flags & COLFLAG_GENERATED) != 0 {
                            ipk_column -= 1;
                        }
                        i -= 1;
                    }
                }

                // Confere que o número de colunas dos dados de origem bate com o de colunas a
                // inserir na tabela.
                let mut n_hidden: i32 = 0; // colunas ocultas se a tabela é virtual
                if (p_tab.tab_flags & (TF_HAS_GENERATED | TF_HAS_HIDDEN)) != 0 {
                    for c in p_tab.a_col.iter() {
                        if (c.col_flags & COLFLAG_NOINSERT) != 0 {
                            n_hidden += 1;
                        }
                    }
                }
                if n_column != (p_tab.n_col as i32 - n_hidden) {
                    error_msg(
                        db,
                        parse,
                        b"table %S has %d columns but %d values were supplied",
                        &[
                            src_item_arg(&p_tab_list.a[0]),
                            PrintfArg::Int((p_tab.n_col as i32 - n_hidden) as i64),
                            PrintfArg::Int(n_column as i64),
                        ],
                    );
                    break 'insert_cleanup;
                }
            }
            if let Some(col) = p_column.as_deref() {
                if n_column != col.a.len() as i32 {
                    error_msg(
                        db,
                        parse,
                        b"%d values for %d columns",
                        &[PrintfArg::Int(n_column as i64), PrintfArg::Int(col.a.len() as i64)],
                    );
                    break 'insert_cleanup;
                }
            }

            // Inicializa a contagem das linhas a inserir.
            if (db.flags & SQLITE_COUNT_ROWS) != 0
                && parse.nested == 0
                && parse.p_trigger_tab.is_none()
                && parse.b_returning == 0
            {
                parse.n_mem += 1;
                reg_row_count = parse.n_mem;
                add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, 0, reg_row_count);
            }

            // Se não é uma view, abre a tabela e todos os índices.
            let mut i_data_cur: i32 = 0; // cursor do VDBE que é o repositório principal
            let mut i_idx_cur: i32 = 0; // primeiro cursor de índice
            let mut a_reg_idx: Vec<i32> = Vec::new(); // um registrador alocado para cada índice
            if !is_view {
                let n_idx = open_table_and_indices(
                    db,
                    parse,
                    &p_tab,
                    OP_OPENWRITE,
                    0,
                    -1,
                    None,
                    &mut i_data_cur,
                    &mut i_idx_cur,
                );
                a_reg_idx = vec![0; n_idx as usize + 2];
                for (i, p_idx) in p_tab.p_index.iter().enumerate().take(n_idx as usize) {
                    parse.n_mem += 1;
                    a_reg_idx[i] = parse.n_mem;
                    parse.n_mem += p_idx.n_column as i32;
                }
                parse.n_mem += 1;
                a_reg_idx[n_idx as usize] = parse.n_mem; // registrador do registro da tabela
            }
            if let Some(up) = p_upsert.as_deref_mut() {
                if p_tab.is_virtual() {
                    error_msg(
                        db,
                        parse,
                        b"UPSERT not implemented for virtual table \"%s\"",
                        &[text_arg(&p_tab.z_name)],
                    );
                    break 'insert_cleanup;
                }
                if p_tab.is_view() {
                    error_msg(db, parse, b"cannot UPSERT a view", &[]);
                    break 'insert_cleanup;
                }
                if has_explicit_nulls(db, parse, up.p_upsert_target.as_deref()) != 0 {
                    break 'insert_cleanup;
                }
                p_tab_list.a[0].i_cursor = i_data_cur;
                let mut k = 0usize;
                loop {
                    {
                        // `pNx->pUpsertSrc = pTabList`: cada cláusula leva uma cópia da lista.
                        let p_nx = upsert_nth_mut(up, k);
                        p_nx.p_upsert_src = src_list_dup(Some(&*p_tab_list), 0);
                        p_nx.reg_data = reg_data;
                        p_nx.i_data_cur = i_data_cur;
                        p_nx.i_idx_cur = i_idx_cur;
                    }
                    if upsert_nth_mut(up, k).p_upsert_target.is_some()
                        && upsert_analyze_target(db, parse, &p_tab_list, up, k) != 0
                    {
                        break 'insert_cleanup;
                    }
                    if upsert_nth_mut(up, k).p_next_upsert.is_none() {
                        break;
                    }
                    k += 1;
                }
            }

            // Este é o topo do laço principal de inserção.
            let mut addr_ins_top: i32 = 0; // pula para o rótulo "D"
            let mut addr_cont: i32 = 0; // topo do laço de inserção: rótulo "C" nos moldes 3 e 4
            if use_temp_table {
                // Este bloco gera só o topo do laço. O laço completo é (molde 4):
                //
                //         rewind da tabela temporária, se vazia vai para D
                //      C: laço sobre as linhas da tabela intermediária
                //           transfere os valores da intermediária para <tabela>
                //         fim do laço
                //      D: ...
                let v = vdbe_of_parse(parse);
                addr_ins_top = add_op1(v, OP_REWIND as i32, src_tab);
                addr_cont = v.n_op();
            } else if p_select.is_some() {
                // Este bloco gera só o topo do laço. O laço completo é (molde 3):
                //
                //      C: yield X, no EOF vai para D
                //         insere o resultado do select em <tabela> a partir de R..R+n
                //         goto C
                //      D: ...
                //
                // (O `sqlite3VdbeReleaseRegisters` do C só existe com SQLITE_DEBUG.)
                let v = vdbe_of_parse(parse);
                addr_ins_top = add_op1(v, OP_YIELD as i32, dest.i_sd_parm);
                addr_cont = addr_ins_top;
                if ipk_column >= 0 {
                    // tag-20191021-001: se a INTEGER PRIMARY KEY é gerada pelo SELECT, copia já o
                    // valor para o slot do rowid, para que não seja sobrescrito por um NULL na
                    // tag-20191021-002.
                    add_op2(v, OP_COPY as i32, reg_from_select + ipk_column, reg_rowid);
                }
            }

            // Calcula os dados das colunas comuns da entrada nova. Os valores são escritos em
            // ordem de armazenamento nos registradores a partir de `reg_data`. Só as colunas
            // comuns são calculadas aqui: o rowid (se houver) vem depois, e as colunas geradas
            // depois do rowid, porque podem depender dele.
            let mut n_hidden: i32 = 0;
            let mut i_reg_store = reg_data;
            debug_assert!(reg_data == reg_rowid + 1);
            for i in 0..n_tab_col {
                'body: {
                    debug_assert!(i as i32 >= n_hidden);
                    if i as i32 == p_tab.i_p_key as i32 {
                        // tag-20191021-002: as referências à INTEGER PRIMARY KEY são preenchidas
                        // com o rowid. Põe um NULL no slot da IPK do registro para não gastar
                        // espaço. A definição do formato exige esse NULL extra.
                        add_op1(vdbe_of_parse(parse), OP_SOFTNULL as i32, i_reg_store);
                        break 'body;
                    }
                    let col_flags = p_tab.a_col[i].col_flags;
                    if (col_flags & COLFLAG_NOINSERT) != 0 {
                        n_hidden += 1;
                        if (col_flags & COLFLAG_VIRTUAL) != 0 {
                            // Colunas virtuais não participam do OP_MakeRecord: volta
                            // `i_reg_store` uma posição para compensar o incremento do laço.
                            i_reg_store -= 1;
                            break 'body;
                        } else if (col_flags & COLFLAG_STORED) != 0 {
                            // As colunas STORED são calculadas depois. Mas com gatilhos BEFORE
                            // os slots delas são copiados (OP_Copy) para um segundo bloco de
                            // registradores, então o registrador precisa ser inicializado com
                            // NULL para não haver leitura de registrador não inicializado.
                            if (tmask & TRIGGER_BEFORE as i32) != 0 {
                                add_op1(vdbe_of_parse(parse), OP_SOFTNULL as i32, i_reg_store);
                            }
                            break 'body;
                        } else if p_column.is_none() {
                            // Colunas ocultas não citadas no INSERT recebem o valor padrão.
                            expr_code_column_default(db, parse, &p_tab, i, i_reg_store);
                            break 'body;
                        }
                    }
                    let k: i32;
                    if let Some(col) = p_column.as_deref() {
                        debug_assert!(col.e_u4 == EU4_IDX);
                        match col.a.iter().position(|it| it.idx == i as i32) {
                            None => {
                                // Coluna que não consta da lista do INSERT recebe o padrão.
                                expr_code_column_default(db, parse, &p_tab, i, i_reg_store);
                                break 'body;
                            }
                            Some(j) => k = j as i32,
                        }
                    } else if n_column == 0 {
                        // É INSERT INTO ... DEFAULT VALUES. Carrega o valor padrão.
                        expr_code_column_default(db, parse, &p_tab, i, i_reg_store);
                        break 'body;
                    } else {
                        k = i as i32 - n_hidden;
                    }

                    if use_temp_table {
                        add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, src_tab, k, i_reg_store);
                    } else if p_select.is_some() {
                        if reg_from_select != reg_data {
                            add_op2(vdbe_of_parse(parse), OP_SCOPY as i32, reg_from_select + k, i_reg_store);
                        }
                    } else if let Some(p_x) = p_list
                        .as_deref_mut()
                        .and_then(|l| l.a.get_mut(k as usize))
                        .and_then(|it| it.p_expr.as_deref_mut())
                    {
                        let y = expr_code_target(db, parse, p_x, i_reg_store, None);
                        if y != i_reg_store {
                            let op = if p_x.has_property(EP_SUBQUERY) { OP_COPY } else { OP_SCOPY };
                            add_op2(vdbe_of_parse(parse), op as i32, y, i_reg_store);
                        }
                    }
                }
                i_reg_store += 1;
            }

            // Roda os gatilhos BEFORE e INSTEAD OF, se houver.
            let end_of_loop = make_label(parse); // rótulo do fim do laço de inserção
            if (tmask & TRIGGER_BEFORE as i32) != 0 {
                let n_col1 = p_tab.n_col as i32 + 1;
                let reg_cols = get_temp_range(parse, n_col1);

                // Monta a linha de referência NEW.*. Se há uma INTEGER PRIMARY KEY em que se
                // insere um NULL, esse NULL vira um ID único da linha. Mas num gatilho BEFORE
                // não se sabe qual será o ID (o INSERT ainda não aconteceu), então usa-se o
                // rowid -1.
                if ipk_column < 0 {
                    add_op2(vdbe_of_parse(parse), OP_INTEGER as i32, -1, reg_cols);
                } else {
                    debug_assert!(!without_rowid);
                    if use_temp_table {
                        add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, src_tab, ipk_column, reg_cols);
                    } else {
                        debug_assert!(p_select.is_none()); // senão use_temp_table seria verdadeiro
                        if let Some(e) = p_list
                            .as_deref_mut()
                            .and_then(|l| l.a.get_mut(ipk_column as usize))
                            .and_then(|it| it.p_expr.as_deref_mut())
                        {
                            expr_code(db, parse, e, reg_cols, None);
                        }
                    }
                    let v = vdbe_of_parse(parse);
                    let addr1 = add_op1(v, OP_NOTNULL as i32, reg_cols);
                    add_op2(v, OP_INTEGER as i32, -1, reg_cols);
                    jump_here(v, addr1);
                    add_op1(v, OP_MUSTBEINT as i32, reg_cols);
                }

                // Copia os dados novos já gerados.
                debug_assert!(p_tab.n_nv_col > 0 || parse.n_err > 0);
                add_op3(
                    vdbe_of_parse(parse),
                    OP_COPY as i32,
                    reg_rowid + 1,
                    reg_cols + 1,
                    p_tab.n_nv_col as i32 - 1,
                );

                // Calcula o valor novo das colunas geradas depois de todas as outras. Precisa vir
                // depois do ROWID, caso alguma coluna gerada se refira a ele.
                if (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
                    compute_generated_columns(db, parse, reg_cols + 1, &p_tab);
                }

                // Num INSERT em view com gatilho INSTEAD OF INSERT, não faz conversão alguma
                // antes de montar o registro. Numa tabela real, faz as conversões que as
                // afinidades das colunas pedem.
                if !is_view {
                    table_affinity(vdbe_of_parse(parse), &p_tab, reg_cols + 1);
                }

                // Dispara os gatilhos BEFORE ou INSTEAD OF.
                code_row_trigger(
                    db,
                    parse,
                    &p_trigger,
                    TK_INSERT as i32,
                    None,
                    TRIGGER_BEFORE as i32,
                    &p_tab,
                    reg_cols - p_tab.n_col as i32 - 1,
                    on_error,
                    end_of_loop,
                );

                release_temp_range(parse, reg_cols, n_col1);
            }

            let mut append_flag = false; // o INSERT provavelmente é um append
            if !is_view {
                if p_tab.is_virtual() {
                    // A linha que o VUpdate apagará: nenhuma.
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_ins);
                }
                if ipk_column >= 0 {
                    // Calcula o rowid novo.
                    if use_temp_table {
                        add_op3(vdbe_of_parse(parse), OP_COLUMN as i32, src_tab, ipk_column, reg_rowid);
                    } else if p_select.is_some() {
                        // O rowid já foi inicializado na tag-20191021-001.
                    } else {
                        let ipk_is_null = p_list
                            .as_deref()
                            .and_then(|l| l.a.get(ipk_column as usize))
                            .and_then(|it| it.p_expr.as_deref())
                            .map_or(false, |e| e.op == TK_NULL);
                        if ipk_is_null && !p_tab.is_virtual() {
                            add_op3(
                                vdbe_of_parse(parse),
                                OP_NEWROWID as i32,
                                i_data_cur,
                                reg_rowid,
                                reg_autoinc,
                            );
                            append_flag = true;
                        } else if let Some(e) = p_list
                            .as_deref_mut()
                            .and_then(|l| l.a.get_mut(ipk_column as usize))
                            .and_then(|it| it.p_expr.as_deref_mut())
                        {
                            expr_code(db, parse, e, reg_rowid, None);
                        }
                    }
                    // Se a expressão da PRIMARY KEY é NULL, usa OP_NewRowid para gerar um valor
                    // único.
                    if !append_flag {
                        let v = vdbe_of_parse(parse);
                        if !p_tab.is_virtual() {
                            let addr1 = add_op1(v, OP_NOTNULL as i32, reg_rowid);
                            add_op3(v, OP_NEWROWID as i32, i_data_cur, reg_rowid, reg_autoinc);
                            jump_here(v, addr1);
                        } else {
                            let addr1 = v.n_op();
                            add_op2(v, OP_ISNULL as i32, reg_rowid, addr1 + 2);
                        }
                        add_op1(v, OP_MUSTBEINT as i32, reg_rowid);
                    }
                } else if p_tab.is_virtual() || without_rowid {
                    add_op2(vdbe_of_parse(parse), OP_NULL as i32, 0, reg_rowid);
                } else {
                    add_op3(
                        vdbe_of_parse(parse),
                        OP_NEWROWID as i32,
                        i_data_cur,
                        reg_rowid,
                        reg_autoinc,
                    );
                    append_flag = true;
                }
                auto_inc_step(parse, reg_autoinc, reg_rowid);

                // Calcula o valor novo das colunas geradas depois de todas as outras. Precisa vir
                // depois do ROWID, caso alguma coluna gerada derive da INTEGER PRIMARY KEY.
                if (p_tab.tab_flags & TF_HAS_GENERATED) != 0 {
                    compute_generated_columns(db, parse, reg_rowid + 1, &p_tab);
                }

                // Gera o código que confere as restrições, gera as chaves dos índices e faz a
                // inserção.
                if p_tab.is_virtual() {
                    let p_vtab = get_vtable(db, &p_tab);
                    vtab_make_writable(db, parse, &p_tab);
                    let a = add_op3(
                        vdbe_of_parse(parse),
                        OP_VUPDATE as i32,
                        1,
                        p_tab.n_col as i32 + 2,
                        reg_ins,
                    );
                    if let Some(id) = p_vtab {
                        change_p4_vtab(vdbe_of_parse(parse), db, a, id);
                    }
                    let p5 = if on_error == OE_DEFAULT as i32 { OE_ABORT as i32 } else { on_error };
                    change_p5(vdbe_of_parse(parse), p5 as u16);
                    may_abort(parse);
                } else {
                    let mut is_replace: i32 = 0; // verdadeiro se as restrições podem causar replace
                    generate_constraint_checks(
                        db,
                        parse,
                        &p_tab,
                        &mut a_reg_idx,
                        i_data_cur,
                        i_idx_cur,
                        reg_ins,
                        0,
                        (ipk_column >= 0) as u8,
                        on_error as u8,
                        end_of_loop,
                        &mut is_replace,
                        None,
                        p_upsert.as_deref_mut(),
                    );
                    if (db.flags & SQLITE_FOREIGN_KEYS) != 0 {
                        fk_check(db, parse, &p_tab, 0, reg_ins, None, 0);
                    }

                    // Liga OPFLAG_USESEEKRESULT se (a) não há restrições REPLACE ou (b) não há
                    // gatilhos e a tabela não é pai de chave estrangeira. No caso (b) é seguro
                    // porque, se alguma restrição REPLACE for atingida, um OP_Delete ou
                    // OP_IdxDelete rodará em cada cursor perturbado, e ambos limpam
                    // `VdbeCursor.seekResult`, desligando a função.
                    let b_use_seek =
                        is_replace == 0 || !has_sub_program(parse.p_vdbe.as_deref().expect("pParse->pVdbe"));
                    complete_insertion(
                        db,
                        parse,
                        &p_tab,
                        i_data_cur,
                        i_idx_cur,
                        reg_ins,
                        &a_reg_idx,
                        0,
                        append_flag as i32,
                        b_use_seek as i32,
                    );
                }
            }

            // Atualiza a contagem de linhas inseridas.
            if reg_row_count != 0 {
                add_op2(vdbe_of_parse(parse), OP_ADDIMM as i32, reg_row_count, 1);
            }

            if has_trigger {
                // Gera os gatilhos AFTER.
                code_row_trigger(
                    db,
                    parse,
                    &p_trigger,
                    TK_INSERT as i32,
                    None,
                    TRIGGER_AFTER as i32,
                    &p_tab,
                    reg_data - 2 - p_tab.n_col as i32,
                    on_error,
                    end_of_loop,
                );
            }

            // O fim do laço principal de inserção, se a fonte dos dados é um SELECT.
            resolve_label(parse, db, end_of_loop);
            if use_temp_table {
                let v = vdbe_of_parse(parse);
                add_op2(v, OP_NEXT as i32, src_tab, addr_cont);
                jump_here(v, addr_ins_top);
                add_op1(v, OP_CLOSE as i32, src_tab);
            } else if p_select.is_some() {
                let v = vdbe_of_parse(parse);
                vdbe_goto(v, addr_cont);
                jump_here(v, addr_ins_top);
            }
        }

        // insert_end: atualiza a tabela sqlite_sequence guardando o conteúdo dos contadores de
        // maior rowid registrados durante as inserções em tabelas AUTOINCREMENT.
        if parse.nested == 0 && parse.p_trigger_tab.is_none() {
            auto_increment_end(db, parse);
        }

        // Devolve o número de linhas inseridas. Se esta rotina está gerando código por causa de
        // um `sqlite3NestedParse()`, o callback não é invocado.
        if reg_row_count != 0 {
            code_change_count(vdbe_of_parse(parse), reg_row_count, b"rows inserted");
        }
    }

    // insert_cleanup: `p_tab_list`, `p_list`, `p_upsert`, `p_select`, `p_column` e `a_reg_idx`
    // são liberados pelo `Drop` ao sair.
}
