// Mesclado das partes traduzidas de vdbevtab_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tabelas virtuais bytecode() e tables_used(), que examinam o bytecode de uma
// declaração preparada. Todo o arquivo vdbevtab.c fica atrás de
// SQLITE_ENABLE_BYTECODE_VTAB; a tradução é completa para o integrador decidir
// se registra o módulo (a lista de opções do Debian em CONVENTIONS.md não
// cita essa opção, então `vdbe_bytecode_vtab_init` pode não ser chamada).
//
// Modelagem dos ponteiros do C:
// - `sqlite3_vtab*` base de `bytecodevtab` é o campo `base`; o `(bytecodevtab*)cur->pVtab`
//   do cursor vira o campo `p_tab` do cursor (a "classe base" não precisa de cast).
// - `Op *aOp` (um ponteiro para dentro do array de opcodes da declaração ou de
//   um subprograma) vira `Option<Rc<Vec<Op>>>`; o índice é `i_addr`.
// - `const char *zType/zSchema/zName` viram cópias donas (`Option<Vec<u8>>`).

/// Referência compartilhada para a tabela virtual.
pub type BytecodeVTabRef = Rc<RefCell<BytecodeVTab>>;
/// Referência compartilhada para o cursor da tabela virtual.
pub type BytecodeVTabCursorRef = Rc<RefCell<BytecodeVTabCursor>>;

/// Instância da função com valor de tabela bytecode().
pub struct BytecodeVTab {
    /// Classe base, deve ser o primeiro campo.
    pub base: Sqlite3Vtab,
    /// Conexão de banco de dados.
    pub db: Weak<RefCell<Sqlite3>>,
    /// 2 para tables_used(), 0 para bytecode().
    pub b_tables_used: i32,
}

/// Cursor para varrer o bytecode.
pub struct BytecodeVTabCursor {
    /// Classe base, deve ser o primeiro campo.
    pub base: Sqlite3VtabCursor,
    /// A tabela virtual dona do cursor (o `cur->pVtab` do C).
    pub p_tab: BytecodeVTabRef,
    /// A declaração cujo bytecode é exibido.
    pub p_stmt: Option<VdbeRef>,
    /// O rowid da tabela de saída.
    pub i_rowid: i32,
    /// Endereço.
    pub i_addr: i32,
    /// O cursor é dono de p_stmt e deve finalizá-lo.
    pub need_finalize: i32,
    /// Fornece a listagem de subprogramas.
    pub show_subprograms: i32,
    /// Array de operandos.
    pub a_op: Option<Rc<Vec<Op>>>,
    /// Valor P4 renderizado.
    pub z_p4: Option<Vec<u8>>,
    /// tables_used.type
    pub z_type: Option<Vec<u8>>,
    /// tables_used.schema
    pub z_schema: Option<Vec<u8>>,
    /// tables_used.name
    pub z_name: Option<Vec<u8>>,
    /// Subprogramas.
    pub sub: Mem,
}

/// Cria uma nova função com valor de tabela bytecode().
pub fn bytecodevtab_connect(
    db: &SqliteRef,
    p_aux: Option<&SqliteRef>,
    _argv: &[Vec<u8>],
    _pz_err: &mut Option<Vec<u8>>,
) -> (i32, Option<BytecodeVTabRef>) {
    let is_tab_used: usize = if p_aux.is_some() { 1 } else { 0 };
    let az_schema: [&[u8]; 2] = [
        // esquema de bytecode()
        b"CREATE TABLE x(addr INT,opcode TEXT,p1 INT,p2 INT,p3 INT,p4 TEXT,p5 INT,comment TEXT,subprog TEXT,nexec INT,ncycle INT,stmt HIDDEN);",
        // esquema de tables_used()
        b"CREATE TABLE x(type TEXT,schema TEXT,name TEXT,wr INT,subprog TEXT,stmt HIDDEN);",
    ];

    let rc = api::declare_vtab(db, az_schema[is_tab_used]);
    let mut pp_vtab = None;
    if rc == SQLITE_OK {
        pp_vtab = Some(Rc::new(RefCell::new(BytecodeVTab {
            base: Sqlite3Vtab::default(),
            db: Rc::downgrade(db),
            b_tables_used: (is_tab_used as i32) * 2,
        })));
    }
    (rc, pp_vtab)
}

/// Destruidor dos objetos bytecodevtab.
pub fn bytecodevtab_disconnect(_p_vtab: BytecodeVTabRef) -> i32 {
    // O Rc libera a memória ao sair de escopo (sqlite3_free).
    SQLITE_OK
}

/// Construtor de um novo objeto bytecodevtab_cursor.
pub fn bytecodevtab_open(p: &BytecodeVTabRef) -> (i32, Option<BytecodeVTabCursorRef>) {
    let db = p.borrow().db.upgrade();
    let mut sub = Mem::default();
    if let Some(db) = db.as_ref() {
        vdbe_mem_init(&mut sub, db, 1);
    }
    let p_cur = BytecodeVTabCursor {
        base: Sqlite3VtabCursor::default(),
        p_tab: p.clone(),
        p_stmt: None,
        i_rowid: 0,
        i_addr: 0,
        need_finalize: 0,
        show_subprograms: 0,
        a_op: None,
        z_p4: None,
        z_type: None,
        z_schema: None,
        z_name: None,
        sub,
    };
    (SQLITE_OK, Some(Rc::new(RefCell::new(p_cur))))
}

/// Limpa todo o conteúdo interno de um cursor bytecodevtab.
pub fn bytecodevtab_cursor_clear(p_cur: &mut BytecodeVTabCursor) {
    p_cur.z_p4 = None;
    vdbe_mem_release(&mut p_cur.sub);
    vdbe_mem_set_null(&mut p_cur.sub);
    if p_cur.need_finalize != 0 {
        if let Some(stmt) = p_cur.p_stmt.take() {
            api::finalize(stmt);
        }
    }
    p_cur.p_stmt = None;
    p_cur.need_finalize = 0;
    p_cur.z_type = None;
    p_cur.z_schema = None;
    p_cur.z_name = None;
}

/// Destruidor de um bytecodevtab_cursor.
pub fn bytecodevtab_close(cur: &BytecodeVTabCursorRef) -> i32 {
    bytecodevtab_cursor_clear(&mut cur.borrow_mut());
    SQLITE_OK
}

/// Avança um bytecodevtab_cursor para a próxima linha de saída.
pub fn bytecodevtab_next(cur: &BytecodeVTabCursorRef) -> i32 {
    let mut guard = cur.borrow_mut();
    let p_cur = &mut *guard;
    let b_tables_used = p_cur.p_tab.borrow().b_tables_used;
    p_cur.z_p4 = None;
    if p_cur.z_name.is_some() {
        p_cur.z_name = None;
        p_cur.z_type = None;
        p_cur.z_schema = None;
    }
    let stmt = p_cur.p_stmt.clone();
    let rc = vdbe_next_opcode(
        stmt.as_ref(),
        if p_cur.show_subprograms != 0 { Some(&mut p_cur.sub) } else { None },
        b_tables_used,
        &mut p_cur.i_rowid,
        &mut p_cur.i_addr,
        &mut p_cur.a_op,
    );
    if rc != SQLITE_OK {
        vdbe_mem_set_null(&mut p_cur.sub);
        p_cur.a_op = None;
    }
    SQLITE_OK
}

/// Retorna verdadeiro se o cursor passou da última linha de saída.
pub fn bytecodevtab_eof(cur: &BytecodeVTabCursorRef) -> i32 {
    if cur.borrow().a_op.is_none() { 1 } else { 0 }
}

/// Retorna os valores das colunas da linha em que o cursor está.
pub fn bytecodevtab_column(
    cur: &BytecodeVTabCursorRef,
    ctx: &mut Sqlite3Context,
    i: i32,
) -> i32 {
    let mut guard = cur.borrow_mut();
    let p_cur = &mut *guard;
    let p_vtab = p_cur.p_tab.clone();
    let b_tables_used = p_vtab.borrow().b_tables_used;
    let db = p_vtab.borrow().db.upgrade();
    let a_op = p_cur.a_op.clone().unwrap();
    let p_op = &a_op[p_cur.i_addr as usize];
    let mut i = i;
    if b_tables_used != 0 {
        if i == 4 {
            i = 8;
        } else {
            if i <= 2 && p_cur.z_type.is_none() {
                let i_db = p_op.p3 as usize;
                let i_root: Pgno = p_op.p2 as Pgno;
                let db = db.as_ref().unwrap();
                let db_ref = db.borrow();
                let p_schema = db_ref.a_db[i_db].p_schema.clone();
                p_cur.z_schema = Some(db_ref.a_db[i_db].z_db_s_name.clone());
                let schema = p_schema.borrow();
                let mut k = sqlite_hash_first(&schema.tbl_hash);
                while let Some(e) = k {
                    let p_tab: TableRef = sqlite_hash_data(&e);
                    let t = p_tab.borrow();
                    if !is_virtual(&t) && t.tnum == i_root {
                        p_cur.z_name = Some(t.z_name.clone());
                        p_cur.z_type = Some(b"table".to_vec());
                        break;
                    }
                    k = sqlite_hash_next(&e);
                }
                if p_cur.z_name.is_none() {
                    let mut k = sqlite_hash_first(&schema.idx_hash);
                    while let Some(e) = k {
                        let p_idx: IndexRef = sqlite_hash_data(&e);
                        let x = p_idx.borrow();
                        if x.tnum == i_root {
                            p_cur.z_name = Some(x.z_name.clone());
                            p_cur.z_type = Some(b"index".to_vec());
                        }
                        k = sqlite_hash_next(&e);
                    }
                }
            }
            i += 20;
        }
    }
    match i {
        0 => {
            // addr
            api::result_int(ctx, p_cur.i_addr);
        }
        1 => {
            // opcode
            api::result_text(ctx, Some(opcode_name(p_op.opcode as i32)), -1, SQLITE_STATIC);
        }
        2 => {
            // p1
            api::result_int(ctx, p_op.p1);
        }
        3 => {
            // p2
            api::result_int(ctx, p_op.p2);
        }
        4 => {
            // p3
            api::result_int(ctx, p_op.p3);
        }
        5 | 7 => {
            // p4 e comment
            if p_cur.z_p4.is_none() {
                p_cur.z_p4 = vdbe_display_p4(db.as_ref().unwrap(), p_op);
            }
            if i == 5 {
                api::result_text(ctx, p_cur.z_p4.as_deref(), -1, SQLITE_STATIC);
            }
            // O ramo do comment depende de SQLITE_ENABLE_EXPLAIN_COMMENTS,
            // que não está nas opções do Debian: sem resultado (NULL).
        }
        6 => {
            // p5
            api::result_int(ctx, p_op.p5 as i32);
        }
        8 => {
            // subprog
            let p4_z = a_op[0].p4.z.as_deref();
            if p_cur.i_rowid == p_cur.i_addr + 1 {
                // O resultado é NULL para o programa principal.
            } else if let Some(z) = p4_z {
                api::result_text(ctx, Some(&z[3..]), -1, SQLITE_STATIC);
            } else {
                api::result_text(ctx, Some(b"(FK)"), 4, SQLITE_STATIC);
            }
        }
        // SQLITE_ENABLE_STMT_SCANSTATUS não está nas opções do Debian.
        9 | 10 => {
            // nexec e ncycle
            api::result_int(ctx, 0);
        }
        20 => {
            // tables_used.type
            api::result_text(ctx, p_cur.z_type.as_deref(), -1, SQLITE_STATIC);
        }
        21 => {
            // tables_used.schema
            api::result_text(ctx, p_cur.z_schema.as_deref(), -1, SQLITE_STATIC);
        }
        22 => {
            // tables_used.name
            api::result_text(ctx, p_cur.z_name.as_deref(), -1, SQLITE_STATIC);
        }
        23 => {
            // tables_used.wr
            api::result_int(ctx, if p_op.opcode == OP_OPENWRITE { 1 } else { 0 });
        }
        _ => {}
    }
    SQLITE_OK
}

/// Retorna o rowid da linha atual. Nesta implementação o rowid é o mesmo
/// valor de saída.
pub fn bytecodevtab_rowid(cur: &BytecodeVTabCursorRef, p_rowid: &mut i64) -> i32 {
    *p_rowid = cur.borrow().i_rowid as i64;
    SQLITE_OK
}

/// Inicializa um cursor.
///
/// idxNum==0 mostra todos os subprogramas.
/// idxNum==1 mostra só o bytecode principal e omite os subprogramas.
pub fn bytecodevtab_filter(
    p_vtab_cursor: &BytecodeVTabCursorRef,
    idx_num: i32,
    _idx_str: Option<&[u8]>,
    argv: &[SqliteValueRef],
) -> i32 {
    let mut rc = SQLITE_OK;
    let p_vtab = p_vtab_cursor.borrow().p_tab.clone();
    let db = p_vtab.borrow().db.upgrade().unwrap();

    bytecodevtab_cursor_clear(&mut p_vtab_cursor.borrow_mut());
    {
        let mut p_cur = p_vtab_cursor.borrow_mut();
        p_cur.i_rowid = 0;
        p_cur.i_addr = 0;
        p_cur.show_subprograms = if idx_num == 0 { 1 } else { 0 };
    }
    if api::value_type(&argv[0]) == SQLITE_TEXT {
        match api::value_text(&argv[0]) {
            None => {
                rc = SQLITE_NOMEM;
            }
            Some(z_sql) => {
                let mut p_stmt: Option<VdbeRef> = None;
                rc = api::prepare_v2(&db, &z_sql, -1, &mut p_stmt, None);
                let mut p_cur = p_vtab_cursor.borrow_mut();
                p_cur.p_stmt = p_stmt;
                p_cur.need_finalize = 1;
            }
        }
    } else {
        p_vtab_cursor.borrow_mut().p_stmt = api::value_pointer(&argv[0], b"stmt-pointer");
    }
    if p_vtab_cursor.borrow().p_stmt.is_none() {
        let b_tables_used = p_vtab.borrow().b_tables_used;
        let mut msg = b"argument to ".to_vec();
        msg.extend_from_slice(if b_tables_used != 0 { b"tables_used" } else { b"bytecode" });
        msg.extend_from_slice(b"() is not a valid SQL statement");
        p_vtab.borrow_mut().base.z_err_msg = Some(msg);
        rc = SQLITE_ERROR;
    } else {
        bytecodevtab_next(p_vtab_cursor);
    }
    rc
}


// ---- part_001.rs ----

/// Precisamos de uma única restrição stmt=? que será repassada ao método
/// xFilter. Sem uma restrição stmt=? válida, devolve SQLITE_CONSTRAINT.
pub fn bytecodevtab_best_index(tab: &BytecodeVTabRef, p_idx_info: &mut Sqlite3IndexInfo) -> i32 {
    let mut rc = SQLITE_CONSTRAINT;
    let i_base_col: i32 = if tab.borrow().b_tables_used != 0 { 4 } else { 10 };
    p_idx_info.estimated_cost = 100.0;
    p_idx_info.estimated_rows = 100;
    p_idx_info.idx_num = 0;
    for i in 0..p_idx_info.a_constraint.len() {
        let p = &p_idx_info.a_constraint[i];
        if p.usable == 0 {
            continue;
        }
        if p.op == SQLITE_INDEX_CONSTRAINT_EQ && p.i_column == i_base_col + 1 {
            rc = SQLITE_OK;
            p_idx_info.a_constraint_usage[i].omit = 1;
            p_idx_info.a_constraint_usage[i].argv_index = 1;
        }
        if p.op == SQLITE_INDEX_CONSTRAINT_ISNULL && p.i_column == i_base_col {
            p_idx_info.a_constraint_usage[i].omit = 1;
            p_idx_info.idx_num = 1;
        }
    }
    rc
}

/// Define todos os métodos da tabela virtual (o `bytecodevtabModule` do C).
pub fn bytecodevtab_module() -> Sqlite3Module {
    Sqlite3Module {
        i_version: 0,
        x_create: None,
        x_connect: Some(bytecodevtab_connect),
        x_best_index: Some(bytecodevtab_best_index),
        x_disconnect: Some(bytecodevtab_disconnect),
        x_destroy: None,
        x_open: Some(bytecodevtab_open),
        x_close: Some(bytecodevtab_close),
        x_filter: Some(bytecodevtab_filter),
        x_next: Some(bytecodevtab_next),
        x_eof: Some(bytecodevtab_eof),
        x_column: Some(bytecodevtab_column),
        x_rowid: Some(bytecodevtab_rowid),
        x_update: None,
        x_begin: None,
        x_sync: None,
        x_commit: None,
        x_rollback: None,
        x_find_method: None,
        x_rename: None,
        x_savepoint: None,
        x_release: None,
        x_rollback_to: None,
        x_shadow_name: None,
        x_integrity: None,
    }
}

/// Registra os módulos bytecode e tables_used na conexão.
pub fn vdbe_bytecode_vtab_init(db: &SqliteRef) -> i32 {
    let mut rc = api::create_module(db, b"bytecode", Rc::new(bytecodevtab_module()), None);
    if rc == SQLITE_OK {
        // O `&db` do C só serve de marca (p_aux != 0) para tables_used.
        rc = api::create_module(db, b"tables_used", Rc::new(bytecodevtab_module()), Some(db.clone()));
    }
    rc
}

// O ramo `#elif defined(SQLITE_ENABLE_BYTECODE_VTAB)` do C (stub com
// SQLITE_OMIT_VIRTUALTABLE) não existe aqui: as tabelas virtuais estão ligadas.

