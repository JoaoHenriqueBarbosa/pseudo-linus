//! `vdbeaux.c` (parte 1): criação e preenchimento de um VDBE. Cobre os trechos 000 a 004 do
//! `vdbeaux.c` do SQLite 3.46.1: `sqlite3VdbeCreate`, a família `sqlite3VdbeAddOp*`, rótulos,
//! `sqlite3VdbeChange*`, listas de opcodes, `scanstatus`, `ChangeP4` e `DisplayComment`.
//!
//! Desvios do C, todos decorrentes do modelo v2 (ver `vdbe_types.rs` e `CONVENTIONS.md`):
//!
//! * O `Vdbe` não tem `db` nem `pParse`. Quando o código é gerado, o `Parse` possui o `Vdbe`
//!   (`Parse.p_vdbe`); as funções que o C resolvia por `v->pParse` recebem o `Parse`, e as que
//!   usavam `p->db` recebem a `Connection` por parâmetro (só as que de fato a consultam).
//! * `sqlite3VdbeCreate` não encadeia o `Vdbe` em `db->pVdbe`: quem move o `Vdbe` do `Parse` para
//!   `Connection.stmts` faz o encadeamento em `Connection.stmt_list`.
//! * `Vdbe.a_op.len()` é o `nOp`. `nOpAlloc` é `a_op.capacity()`, mantido por `reserve_exact`.
//! * O quarto operando é um `P4` possuído: `P4_STATIC`, `P4_TRANSIENT` e `P4_DYNAMIC` são todos
//!   `P4::Text`; a liberação do que o C chamava de `freeP4` é o `Drop`, exceto `P4::Vtab`, que
//!   precisa da conexão (`free_p4`).
//! * `sqlite3VdbeGetOp` devolve `Option`: `None` é o `dummy` do C (falha de alocação ou endereço
//!   inválido, caso em que o C escreveria num opcode descartável).
//! * Os ramos `SQLITE_DEBUG`, `SQLITE_VDBE_COVERAGE`, `SQLITE_ENABLE_NORMALIZE` e
//!   `SQLITE_ENABLE_CURSOR_HINTS` não existem na build do Debian e foram omitidos.
//! * `sqlite3VdbeCurrentAddr` é `Vdbe::n_op()` e `sqlite3VdbeParser` não existe (sem `pParse`).

// Fachada do mesmo arquivo C dividido em módulos: os chamadores importam de `crate::vdbeaux`.
pub use crate::vdbeaux2::*;
pub use crate::vdbeaux3::*;

use std::rc::Rc;

use crate::build::{key_info_of_index, may_abort, progress_check};
use crate::connection::{Connection, FuncCtx, FuncDef, Parse};
use crate::consts::opcodes::OPCODE_PROPERTY;
use crate::consts::{
    NC_SELFREF, OPFLAG_TYPEOFARG, OPFLG_JUMP, OP_AUTOCOMMIT, OP_CHECKPOINT, OP_COLUMN,
    OP_ENDCOROUTINE, OP_EXPIRE, OP_EXPLAIN, OP_FUNCTION, OP_GOTO, OP_INIT, OP_INTEGER,
    OP_JOURNALMODE, OP_NOOP, OP_NULL, OP_PARSESCHEMA, OP_PUREFUNC, OP_RESULTROW, OP_SAVEPOINT,
    OP_STRING8, OP_TRANSACTION, OP_VACUUM, OP_VFILTER, OP_VUPDATE, P4_INT64, P4_REAL,
    SQLITE_LIMIT_LENGTH, SQLITE_LIMIT_VDBE_OP, SQLITE_MAX_LENGTH, SQLITE_MX_JUMP_OPCODE,
    SQLITE_NOMEM, SQLITE_OK, SQLITE_PREPARE_SAVESQL, SQLITE_STMTSTATUS_REPREPARE,
    SQLITE_STMT_SCAN_STATUS, VDBE_INIT_STATE,
};
use crate::opcodes::opcode_name;
use crate::printf::{snprintf, vm_printf, PrintfArg};
use crate::sqlite_int::Index;
use crate::vdbe_types::{addr, Op, ScanStatus, SubProgram, Vdbe, VdbeOpList, P4};
use crate::vtab::{vtab_lock, vtab_unlock};

/// `1024/sizeof(Op)` do `growOpArray`: o `Op` do Debian (com `EXPLAIN_COMMENTS` e
/// `STMT_SCANSTATUS`) ocupa 48 bytes numa máquina de 64 bits.
const OP_ALLOC_UNIT: i64 = 1024 / 48;

/// `IS_STMT_SCANSTATUS(db)`.
#[inline]
pub(crate) fn is_stmt_scanstatus(db: &Connection) -> bool {
    (db.flags & SQLITE_STMT_SCAN_STATUS) != 0
}

/// `sqlite3VdbeCreate`: cria o `Vdbe` do `Parse` (em `parse.p_vdbe`) com o `OP_Init` inicial.
/// O `Vdbe` em construção (`pParse->pVdbe`), onde o C o usa sem testar: lá o ponteiro é
/// garantido pelo `sqlite3GetVdbe` anterior.
pub(crate) fn vdbe_of_parse(parse: &mut Parse) -> &mut Vdbe {
    parse.p_vdbe.as_deref_mut().expect("pParse->pVdbe")
}

/// `sqlite3VdbeCurrentAddr(pParse->pVdbe)`: o endereço da próxima instrução.
pub(crate) fn current_addr(parse: &mut Parse) -> i32 {
    vdbe_of_parse(parse).n_op()
}

pub fn vdbe_create(parse: &mut Parse, db: &Connection) {
    let mut v = Box::new(Vdbe::default());
    v.limit_vdbe_op = db.a_limit[SQLITE_LIMIT_VDBE_OP as usize];
    debug_assert!(v.e_vdbe_state == VDBE_INIT_STATE);
    debug_assert!(parse.a_label.is_empty());
    debug_assert!(parse.n_label == 0);
    debug_assert!(parse.sz_op_alloc == 0);
    let v = parse.p_vdbe.insert(v);
    add_op2(v, OP_INIT as i32, 0, 1);
}

/// `sqlite3VdbeError`: troca a mensagem de erro de `Vdbe.z_err_msg`.
pub fn vdbe_error(p: &mut Vdbe, db: &Connection, fmt: &[u8], args: &[PrintfArg]) {
    let mx = db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32;
    p.z_err_msg = vm_printf(mx, fmt, args).0;
}

/// `sqlite3VdbeSetSql`: guarda o SQL do comando preparado (`n` bytes no máximo).
pub fn vdbe_set_sql(p: &mut Vdbe, z: &[u8], n: i32, prep_flags: u8) {
    p.prep_flags = prep_flags;
    if (prep_flags as u32 & SQLITE_PREPARE_SAVESQL) == 0 {
        p.exp_mask = 0;
    }
    debug_assert!(p.z_sql.is_none());
    let lim = (n.max(0) as usize).min(z.len());
    let end = z[..lim].iter().position(|&c| c == 0).unwrap_or(lim);
    p.z_sql = Some(z[..end].to_vec());
}

/// `sqlite3VdbeSwap`: troca o bytecode entre dois `Vdbe` depois de um `SQLITE_SCHEMA`. O
/// encadeamento (`pVNext`, `ppVPrev`) não existe no `Vdbe`; o SQL fica com cada um.
pub fn vdbe_swap(a: &mut Vdbe, b: &mut Vdbe) {
    std::mem::swap(a, b);
    std::mem::swap(&mut a.z_sql, &mut b.z_sql);
    b.exp_mask = a.exp_mask;
    b.prep_flags = a.prep_flags;
    b.a_counter = a.a_counter;
    b.a_counter[SQLITE_STMTSTATUS_REPREPARE as usize] += 1;
}

/// `growOpArray`: aumenta `a_op` (dobra, ou `1024/sizeof(Op)` na primeira vez). Falha se passar
/// de `SQLITE_LIMIT_VDBE_OP`, marcando `malloc_failed` (o chamador da preparação deve fazer o
/// `sqlite3OomFault`).
fn grow_op_array(v: &mut Vdbe) -> i32 {
    let cap = v.a_op.capacity() as i64;
    let n_new = if cap != 0 { 2 * cap } else { OP_ALLOC_UNIT };
    if n_new > v.limit_vdbe_op as i64 {
        v.malloc_failed = true;
        return SQLITE_NOMEM;
    }
    let extra = (n_new as usize).saturating_sub(v.a_op.len());
    v.a_op.reserve_exact(extra);
    SQLITE_OK
}

/// O corpo comum de `sqlite3VdbeAddOp3` e `sqlite3VdbeAddOp4Int`: devolve o endereço do novo
/// opcode, ou 1 se o vetor não pôde crescer (como o `growOp3` do C).
fn push_op(p: &mut Vdbe, op: i32, p1: i32, p2: i32, p3: i32, p4: P4) -> i32 {
    let i = p.a_op.len();
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    debug_assert!(op >= 0 && op < 0xff);
    if p.a_op.capacity() <= i && grow_op_array(p) != SQLITE_OK {
        return 1;
    }
    p.a_op.push(Op {
        opcode: op as u8,
        p5: 0,
        p1,
        p2,
        p3,
        p4,
        comment: None,
        n_exec: 0,
        n_cycle: 0,
    });
    i as i32
}

/// `sqlite3VdbeAddOp0`.
pub fn add_op0(p: &mut Vdbe, op: impl Into<i32>) -> i32 {
    add_op3(p, op.into(), 0, 0, 0)
}

/// `sqlite3VdbeAddOp1`.
pub fn add_op1(p: &mut Vdbe, op: impl Into<i32>, p1: i32) -> i32 {
    add_op3(p, op.into(), p1, 0, 0)
}

/// `sqlite3VdbeAddOp2`.
pub fn add_op2(p: &mut Vdbe, op: impl Into<i32>, p1: i32, p2: i32) -> i32 {
    add_op3(p, op.into(), p1, p2, 0)
}

/// `sqlite3VdbeAddOp3`: acrescenta uma instrução e devolve o endereço dela.
pub fn add_op3(p: &mut Vdbe, op: impl Into<i32>, p1: i32, p2: i32, p3: i32) -> i32 {
    push_op(p, op.into(), p1, p2, p3, P4::None)
}

/// `sqlite3VdbeAddOp4Int`: como `add_op3` com um P4 inteiro (`P4_INT32`).
pub fn add_op4_int(p: &mut Vdbe, op: impl Into<i32>, p1: i32, p2: i32, p3: i32, p4: i32) -> i32 {
    push_op(p, op.into(), p1, p2, p3, P4::Int32(p4))
}

/// `sqlite3VdbeGoto`: salto incondicional para `i_dest`.
pub fn vdbe_goto(p: &mut Vdbe, i_dest: i32) -> i32 {
    add_op3(p, OP_GOTO as i32, 0, i_dest, 0)
}

/// `sqlite3VdbeLoadString`: carrega a string `z_str` no registro `i_dest`.
pub fn load_string(p: &mut Vdbe, i_dest: i32, z_str: &[u8]) -> i32 {
    add_op4(p, OP_STRING8 as i32, 0, i_dest, 0, P4::Text(z_str.to_vec()))
}

/// `sqlite3VdbeMultiLoad`: inicializa registros consecutivos a partir de `i_dest`. Cada `s` de
/// `z_types` consome um `PrintfArg::Text` (nulo vira `OP_Null`) e cada `i` um `PrintfArg::Int`.
/// Se `z_types` não termina em `X` (qualquer outro caractere), gera o `OP_ResultRow`.
pub fn multi_load(p: &mut Vdbe, i_dest: i32, z_types: &[u8], args: &[PrintfArg]) {
    let mut next = args.iter();
    let mut i = 0usize;
    while i < z_types.len() && z_types[i] != 0 {
        let c = z_types[i];
        if c == b's' {
            let z = match next.next() {
                Some(PrintfArg::Text(z)) => z.clone(),
                _ => None,
            };
            match z {
                None => add_op4(p, OP_NULL as i32, 0, i_dest + i as i32, 0, P4::None),
                Some(z) => add_op4(p, OP_STRING8 as i32, 0, i_dest + i as i32, 0, P4::Text(z)),
            };
        } else if c == b'i' {
            let v = match next.next() {
                Some(PrintfArg::Int(v)) => *v as i32,
                _ => 0,
            };
            add_op2(p, OP_INTEGER as i32, v, i_dest + i as i32);
        } else {
            return;
        }
        i += 1;
    }
    add_op2(p, OP_RESULTROW as i32, i_dest, i as i32);
}

/// `sqlite3VdbeAddOp4`: acrescenta uma instrução com o P4 dado. Com `P4::Vtab` o chamador deve
/// ter feito o `vtab_lock` (ou usar `change_p4_vtab`).
pub fn add_op4(p: &mut Vdbe, op: impl Into<i32>, p1: i32, p2: i32, p3: i32, p4: P4) -> i32 {
    let a = add_op3(p, op.into(), p1, p2, p3);
    change_p4(p, a, p4);
    a
}

/// `sqlite3VdbeAddFunctionCall`: acrescenta um `OP_Function` ou `OP_PureFunc`. O `Context` é
/// montado a cada chamada pelo VDBE; o opcode guarda só a função e `argc` (`P4::FuncCtx`).
pub fn add_function_call(
    parse: &mut Parse,
    p1: i32,
    p2: i32,
    p3: i32,
    n_arg: i32,
    p_func: &Rc<FuncDef>,
    e_call_ctx: i32,
) -> i32 {
    let ctx = FuncCtx { p_func: Rc::clone(p_func), argc: n_arg as u8 };
    let op = if e_call_ctx != 0 { OP_PUREFUNC } else { OP_FUNCTION };
    let Some(v) = parse.p_vdbe.as_mut() else {
        return 0;
    };
    let a = add_op4(v, op as i32, p1, p2, p3, P4::FuncCtx(Rc::new(ctx)));
    change_p5(v, (e_call_ctx & NC_SELFREF) as u16);
    may_abort(parse);
    a
}

/// `sqlite3VdbeAddOp4Dup8`: P4 de 8 bytes, `P4_INT64` ou `P4_REAL`.
pub fn add_op4_dup8(
    p: &mut Vdbe,
    op: i32,
    p1: i32,
    p2: i32,
    p3: i32,
    z_p4: [u8; 8],
    p4type: i8,
) -> i32 {
    let p4 = if p4type == P4_INT64 {
        P4::Int64(i64::from_ne_bytes(z_p4))
    } else {
        debug_assert!(p4type == P4_REAL);
        P4::Real(f64::from_ne_bytes(z_p4))
    };
    add_op4(p, op, p1, p2, p3, p4)
}

/// `sqlite3VdbeExplainParent`: o endereço da linha-base corrente do EXPLAIN QUERY PLAN (0 é
/// "nenhuma").
pub fn explain_parent(parse: &Parse) -> i32 {
    if parse.addr_explain == 0 {
        return 0;
    }
    parse
        .p_vdbe
        .as_deref()
        .and_then(|v| get_op_ref(v, parse.addr_explain))
        .map_or(0, |o| o.p2)
}

/// `sqlite3VdbeExplain`: acrescenta um `OP_Explain`. Com `b_push` o opcode vira o pai dos
/// próximos até `explain_pop`.
pub fn explain(
    parse: &mut Parse,
    db: &Connection,
    b_push: bool,
    fmt: &[u8],
    args: &[PrintfArg],
) -> i32 {
    let mut a = 0;
    if parse.explain == 2 || is_stmt_scanstatus(db) {
        let z_msg = vm_printf(db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32, fmt, args).0;
        let addr_explain = parse.addr_explain;
        let Some(v) = parse.p_vdbe.as_mut() else {
            return 0;
        };
        let i_this = v.n_op();
        a = add_op4(v, OP_EXPLAIN as i32, i_this, addr_explain, 0, z_msg.map_or(P4::None, P4::Text));
        if b_push {
            parse.addr_explain = i_this;
        }
        if let Some(v) = parse.p_vdbe.as_mut() {
            scan_status(v, db, i_this, -1, -1, 0, None);
        }
    }
    a
}

/// `sqlite3VdbeExplainPop`: desempilha um nível do EXPLAIN QUERY PLAN.
pub fn explain_pop(parse: &mut Parse) {
    parse.addr_explain = explain_parent(parse);
}

/// `sqlite3VdbeAddParseSchemaOp`: acrescenta um `OP_ParseSchema` e marca todos os btrees como
/// usados.
pub fn add_parse_schema_op(
    parse: &mut Parse,
    db: &Connection,
    i_db: i32,
    z_where: Option<Vec<u8>>,
    p5: u16,
) {
    let Some(v) = parse.p_vdbe.as_mut() else {
        return;
    };
    add_op4(v, OP_PARSESCHEMA as i32, i_db, 0, 0, z_where.map_or(P4::None, P4::Text));
    change_p5(v, p5);
    for j in 0..db.dbs.len() {
        uses_btree(v, j as i32);
    }
    may_abort(parse);
}

/// `sqlite3VdbeEndCoroutine`: o fim de uma co-rotina. Limpa o cache de registradores temporários
/// para que cada co-rotina tenha o seu conjunto independente.
pub fn end_coroutine(parse: &mut Parse, reg_yield: i32) {
    if let Some(v) = parse.p_vdbe.as_mut() {
        add_op1(v, OP_ENDCOROUTINE as i32, reg_yield);
    }
    parse.n_temp_reg = 0;
    parse.n_range_reg = 0;
}

/// `sqlite3VdbeMakeLabel`: um rótulo simbólico (um número negativo) para um endereço ainda não
/// gerado.
pub fn make_label(parse: &mut Parse) -> i32 {
    parse.n_label -= 1;
    parse.n_label
}

/// `resizeResolveLabel`: o caminho lento de `resolve_label` (`aLabel` precisa crescer).
fn resize_resolve_label(parse: &mut Parse, db: &mut Connection, j: usize, n_op: i32) {
    let n_new_size = (10 - parse.n_label) as usize;
    let n_label_alloc = parse.a_label.len();
    parse.a_label.resize(n_new_size, -1);
    if n_new_size >= 100 && (n_new_size / 100) > (n_label_alloc / 100) {
        progress_check(parse, db);
    }
    parse.a_label[j] = n_op;
}

/// `sqlite3VdbeResolveLabel`: resolve o rótulo `x` para o endereço da próxima instrução.
pub fn resolve_label(parse: &mut Parse, db: &mut Connection, x: i32) {
    let j = addr(x) as usize;
    let n_op = parse.p_vdbe.as_deref().map_or(0, |v| v.n_op());
    debug_assert!(parse.p_vdbe.as_deref().map_or(true, |v| v.e_vdbe_state == VDBE_INIT_STATE));
    debug_assert!((j as i32) < -parse.n_label);
    if (parse.a_label.len() as i64) + (parse.n_label as i64) < 0 {
        resize_resolve_label(parse, db, j, n_op);
    } else {
        debug_assert!(parse.a_label[j] == -1);
        parse.a_label[j] = n_op;
    }
}

/// `sqlite3VdbeRunOnlyOnce`: marca o VDBE como executável uma única vez.
pub fn run_only_once(p: &mut Vdbe) {
    add_op2(p, OP_EXPIRE as i32, 1, 1);
}

/// `sqlite3VdbeReusable`: marca o VDBE como executável várias vezes.
pub fn reusable(p: &mut Vdbe) {
    for i in 1..p.a_op.len() {
        if p.a_op[i].opcode == OP_EXPIRE {
            p.a_op[1].opcode = OP_NOOP;
            break;
        }
    }
}

/// `resolveP2Values`: depois de inserir todos os opcodes, resolve os rótulos (P2 negativo), calcula
/// o máximo de argumentos de função e acerta `read_only` e `b_is_reader`. Libera os rótulos do
/// `Parse` (`a_label` e `n_label`, passados à parte porque o `Parse` possui o `Vdbe`).
pub(crate) fn resolve_p2_values(
    p: &mut Vdbe,
    a_label: &mut Vec<i32>,
    n_label: &mut i32,
    p_max_func_args: &mut i32,
) {
    let mut n_max_args = *p_max_func_args;
    p.read_only = true;
    p.b_is_reader = false;
    debug_assert!(!p.a_op.is_empty() && p.a_op[0].opcode == OP_INIT);
    let mut i = p.a_op.len().saturating_sub(1);
    loop {
        let opcode = p.a_op[i].opcode;
        if opcode <= SQLITE_MX_JUMP_OPCODE {
            match opcode {
                OP_TRANSACTION => {
                    if p.a_op[i].p2 != 0 {
                        p.read_only = false;
                    }
                    p.b_is_reader = true;
                }
                OP_AUTOCOMMIT | OP_SAVEPOINT => {
                    p.b_is_reader = true;
                }
                OP_CHECKPOINT | OP_VACUUM | OP_JOURNALMODE => {
                    p.read_only = false;
                    p.b_is_reader = true;
                }
                OP_INIT => {
                    debug_assert!(p.a_op[i].p2 >= 0);
                    break;
                }
                OP_VUPDATE => {
                    if p.a_op[i].p2 > n_max_args {
                        n_max_args = p.a_op[i].p2;
                    }
                }
                _ => {
                    if opcode == OP_VFILTER {
                        debug_assert!(i >= 3);
                        debug_assert!(p.a_op[i - 1].opcode == OP_INTEGER);
                        let n = p.a_op[i - 1].p1;
                        if n > n_max_args {
                            n_max_args = n;
                        }
                    }
                    let p2 = p.a_op[i].p2;
                    if p2 < 0 {
                        debug_assert!((OPCODE_PROPERTY[opcode as usize] & OPFLG_JUMP) != 0);
                        debug_assert!(addr(p2) < -*n_label);
                        p.a_op[i].p2 = a_label[addr(p2) as usize];
                    }
                }
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    *a_label = Vec::new();
    *n_label = 0;
    *p_max_func_args = n_max_args;
    debug_assert!(p.b_is_reader || p.btree_mask == 0);
}

/// `sqlite3VdbeTakeOpArray`: entrega o vetor de opcodes do `Vdbe` do `parse` (com os rótulos
/// resolvidos). `p_max_arg` recebe o máximo entre o valor atual e os argumentos exigidos.
pub fn take_op_array(parse: &mut Parse, p_max_arg: &mut i32) -> Vec<Op> {
    let Parse { p_vdbe, a_label, n_label, .. } = parse;
    let Some(p) = p_vdbe.as_mut() else {
        return Vec::new();
    };
    debug_assert!(!p.a_op.is_empty());
    debug_assert!(p.btree_mask == 0);
    resolve_p2_values(p, a_label, n_label, p_max_arg);
    std::mem::take(&mut p.a_op)
}

/// `sqlite3VdbeAddOpList`: acrescenta uma lista de operações. Um P2 não nulo de um salto é
/// relativo à primeira operação inserida. Devolve o índice da primeira operação inserida, ou
/// `None` se o vetor não pôde crescer.
pub fn add_op_list(p: &mut Vdbe, a_op: &[VdbeOpList], _i_lineno: i32) -> Option<usize> {
    debug_assert!(!a_op.is_empty());
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    let n_op = a_op.len();
    if p.a_op.len() + n_op > p.a_op.capacity() && grow_op_array(p) != SQLITE_OK {
        return None;
    }
    let first = p.a_op.len();
    for o in a_op {
        let mut p2 = o.p2 as i32;
        debug_assert!(p2 >= 0);
        if (OPCODE_PROPERTY[o.opcode as usize] & OPFLG_JUMP) != 0 && p2 > 0 {
            p2 += first as i32;
        }
        p.a_op.push(Op {
            opcode: o.opcode,
            p1: o.p1 as i32,
            p2,
            p3: o.p3 as i32,
            p4: P4::None,
            p5: 0,
            comment: None,
            n_exec: 0,
            n_cycle: 0,
        });
    }
    Some(first)
}

/// `sqlite3VdbeScanStatus`: acrescenta uma entrada ao `sqlite3_stmt_scanstatus()`.
pub fn scan_status(
    p: &mut Vdbe,
    db: &Connection,
    addr_explain: i32,
    addr_loop: i32,
    addr_visit: i32,
    n_est: i16,
    z_name: Option<&[u8]>,
) {
    if is_stmt_scanstatus(db) {
        p.a_scan.push(ScanStatus {
            addr_explain,
            addr_loop,
            addr_visit,
            n_est,
            z_name: z_name.map(|z| z.to_vec()),
            ..ScanStatus::default()
        });
    }
}

/// `sqlite3VdbeScanStatusRange`: acrescenta a faixa de instruções `addr_start..=addr_end` às do
/// `OP_Explain` em `addr_explain`.
pub fn scan_status_range(
    p: &mut Vdbe,
    db: &Connection,
    addr_explain: i32,
    addr_start: i32,
    addr_end: i32,
) {
    if is_stmt_scanstatus(db) {
        let end = if addr_end < 0 { p.n_op() - 1 } else { addr_end };
        if let Some(scan) = p.a_scan.iter_mut().rev().find(|s| s.addr_explain == addr_explain) {
            for ii in (0..scan.a_addr_range.len()).step_by(2) {
                if scan.a_addr_range[ii] == 0 {
                    scan.a_addr_range[ii] = addr_start;
                    scan.a_addr_range[ii + 1] = end;
                    break;
                }
            }
        }
    }
}

/// `sqlite3VdbeScanStatusCounters`: endereços dos contadores `NLOOP` e `NROW` do `OP_Explain`.
pub fn scan_status_counters(
    p: &mut Vdbe,
    db: &Connection,
    addr_explain: i32,
    addr_loop: i32,
    addr_visit: i32,
) {
    if is_stmt_scanstatus(db) {
        if let Some(scan) = p.a_scan.iter_mut().rev().find(|s| s.addr_explain == addr_explain) {
            if addr_loop > 0 {
                scan.addr_loop = addr_loop;
            }
            if addr_visit > 0 {
                scan.addr_visit = addr_visit;
            }
        }
    }
}

/// `sqlite3VdbeChangeOpcode`.
pub fn change_opcode(p: &mut Vdbe, addr: i32, i_new_opcode: u8) {
    debug_assert!(addr >= 0);
    if let Some(op) = get_op(p, addr) {
        op.opcode = i_new_opcode;
    }
}

/// `sqlite3VdbeChangeP1`.
pub fn change_p1(p: &mut Vdbe, addr: i32, val: i32) {
    debug_assert!(addr >= 0);
    if let Some(op) = get_op(p, addr) {
        op.p1 = val;
    }
}

/// `sqlite3VdbeChangeP2`.
pub fn change_p2(p: &mut Vdbe, addr: i32, val: i32) {
    if let Some(op) = get_op(p, addr) {
        op.p2 = val;
    }
}

/// `sqlite3VdbeChangeP3`.
pub fn change_p3(p: &mut Vdbe, addr: i32, val: i32) {
    debug_assert!(addr >= 0);
    if let Some(op) = get_op(p, addr) {
        op.p3 = val;
    }
}

/// `sqlite3VdbeChangeP5`: altera o P5 do último opcode.
pub fn change_p5(p: &mut Vdbe, p5: u16) {
    debug_assert!(!p.a_op.is_empty() || p.malloc_failed);
    if let Some(op) = p.a_op.last_mut() {
        op.p5 = p5;
    }
}

/// `sqlite3VdbeTypeofColumn`: se o opcode anterior é um `OP_Column` que entrega em `i_dest`,
/// acrescenta `OPFLAG_TYPEOFARG`.
pub fn typeof_column(p: &mut Vdbe, i_dest: i32) {
    if let Some(op) = get_last_op(p) {
        if op.p3 == i_dest && op.opcode == OP_COLUMN {
            op.p5 |= OPFLAG_TYPEOFARG as u16;
        }
    }
}

/// `sqlite3VdbeJumpHere`: o P2 do opcode `addr` passa a apontar para a próxima instrução.
pub fn jump_here(p: &mut Vdbe, addr: i32) {
    let n_op = p.n_op();
    change_p2(p, addr, n_op);
}

/// `sqlite3VdbeJumpHereOrPopInst`: como `jump_here`, mas se o salto é o último opcode (e portanto
/// inútil), apenas o descarta.
pub fn jump_here_or_pop_inst(p: &mut Vdbe, addr: i32) {
    if addr == p.n_op() - 1 {
        debug_assert!(matches!(
            p.a_op[addr as usize].opcode,
            crate::consts::OP_ONCE | crate::consts::OP_IF | crate::consts::OP_FKIFZERO
        ));
        debug_assert!(matches!(p.a_op[addr as usize].p4, P4::None));
        p.a_op.pop();
    } else {
        let n_op = p.n_op();
        change_p2(p, addr, n_op);
    }
}

/// `freeP4`: o `Drop` libera tudo, menos o `VTable`, cuja referência precisa da conexão.
pub(crate) fn free_p4(db: &mut Connection, p4: P4) {
    if let P4::Vtab(id) = p4 {
        vtab_unlock(db, id);
    }
}

/// `vdbeFreeOpArray`: libera o vetor de opcodes e os P4 que ele possui.
pub(crate) fn vdbe_free_op_array(db: &mut Connection, mut a_op: Vec<Op>) {
    while let Some(op) = a_op.pop() {
        free_p4(db, op.p4);
    }
}

/// `sqlite3VdbeLinkSubProgram`: registra o subprograma para ser destruído com a VM.
pub fn link_sub_program(p_vdbe: &mut Vdbe, p: Rc<SubProgram>) {
    p_vdbe.p_program.push(p);
}

/// `sqlite3VdbeHasSubProgram`.
pub fn has_sub_program(p_vdbe: &Vdbe) -> bool {
    !p_vdbe.p_program.is_empty()
}

/// `sqlite3VdbeChangeToNoop`: transforma o opcode `addr` em `OP_Noop`. Devolve 1 se mudou.
pub fn change_to_noop(p: &mut Vdbe, db: &mut Connection, addr: i32) -> i32 {
    if p.malloc_failed {
        return 0;
    }
    debug_assert!(addr >= 0 && addr < p.n_op());
    let op = &mut p.a_op[addr as usize];
    let old = std::mem::take(&mut op.p4);
    op.opcode = OP_NOOP;
    free_p4(db, old);
    1
}

/// `sqlite3VdbeDeletePriorOpcode`: se o último opcode é `op`, vira `OP_Noop`. Devolve 1 se
/// removeu.
pub fn delete_prior_opcode(p: &mut Vdbe, db: &mut Connection, op: u8) -> i32 {
    if !p.a_op.is_empty() && p.a_op[p.a_op.len() - 1].opcode == op {
        let a = p.n_op() - 1;
        change_to_noop(p, db, a)
    } else {
        0
    }
}

/// `sqlite3VdbeChangeP4`: troca o P4 do opcode `addr` (negativo é o último). O C distinguia o
/// texto copiado (`n >= 0`) do apontado; aqui o `P4` já chega possuído, e `P4::None` é o
/// ponteiro nulo (nada é gravado). Com `P4::Vtab` o chamador já deve ter travado o `VTable`
/// (veja `change_p4_vtab`).
pub fn change_p4(p: &mut Vdbe, addr: i32, p4: P4) {
    if p.malloc_failed {
        return;
    }
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    debug_assert!(!p.a_op.is_empty());
    debug_assert!(addr < p.n_op());
    let a = if addr < 0 { p.a_op.len() - 1 } else { addr as usize };
    p.a_op[a].p4 = p4;
}

/// `sqlite3VdbeChangeP4(v, addr, pVTab, P4_VTAB)`: trava o `VTable` e o grava no P4.
pub fn change_p4_vtab(p: &mut Vdbe, db: &mut Connection, addr: i32, id: crate::connection::VTableId) {
    if p.malloc_failed {
        return;
    }
    vtab_lock(db, id);
    change_p4(p, addr, P4::Vtab(id));
}

/// `sqlite3VdbeAppendP4`: P4 do último opcode, que não podia ter P4 (nem `P4_INT32`/`P4_VTAB`).
pub fn append_p4(p: &mut Vdbe, p4: P4) {
    debug_assert!(!matches!(p4, P4::Int32(_) | P4::Vtab(_)));
    if p.malloc_failed {
        return;
    }
    if let Some(op) = p.a_op.last_mut() {
        debug_assert!(matches!(op.p4, P4::None));
        op.p4 = p4;
    }
}

/// `sqlite3VdbeSetP4KeyInfo`: o P4 do último opcode passa a ser o `KeyInfo` do índice.
pub fn set_p4_key_info(parse: &mut Parse, db: &mut Connection, p_idx: &Index) {
    debug_assert!(parse.p_vdbe.is_some());
    if let Some(ki) = key_info_of_index(parse, db, p_idx) {
        if let Some(v) = parse.p_vdbe.as_mut() {
            append_p4(v, P4::KeyInfo(ki));
        }
    }
}

/// `vdbeVComment`: grava o comentário já formatado no último opcode.
fn vdbe_v_comment(p: &mut Vdbe, fmt: &[u8], args: &[PrintfArg]) {
    if let Some(op) = p.a_op.last_mut() {
        op.comment = vm_printf(SQLITE_MAX_LENGTH as u32, fmt, args).0;
    }
}

/// `sqlite3VdbeComment`: troca o comentário do último opcode.
pub fn vdbe_comment(p: &mut Vdbe, fmt: &[u8], args: &[PrintfArg]) {
    vdbe_v_comment(p, fmt, args);
}

/// `sqlite3VdbeNoopComment`: acrescenta um `OP_Noop` com o comentário.
pub fn noop_comment(p: &mut Vdbe, fmt: &[u8], args: &[PrintfArg]) {
    add_op0(p, OP_NOOP as i32);
    vdbe_v_comment(p, fmt, args);
}

/// `sqlite3VdbeGetOp`: o opcode no endereço `addr`. `None` equivale ao `dummy` do C (falha de
/// alocação anterior).
pub fn get_op(p: &mut Vdbe, addr: i32) -> Option<&mut Op> {
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    if p.malloc_failed || addr < 0 {
        return None;
    }
    p.a_op.get_mut(addr as usize)
}

/// `sqlite3VdbeGetOp` para leitura.
pub fn get_op_ref(p: &Vdbe, addr: i32) -> Option<&Op> {
    if p.malloc_failed || addr < 0 {
        return None;
    }
    p.a_op.get(addr as usize)
}

/// `sqlite3VdbeGetLastOp`: o opcode mais recente.
pub fn get_last_op(p: &mut Vdbe) -> Option<&mut Op> {
    let a = p.n_op() - 1;
    get_op(p, a)
}

/// `translateP`: o valor de um parâmetro do opcode escolhido pelo caractere `c`.
fn translate_p(c: u8, op: &Op) -> i32 {
    match c {
        b'1' => op.p1,
        b'2' => op.p2,
        b'3' => op.p3,
        b'4' => match op.p4 {
            P4::Int32(i) => i,
            _ => 0,
        },
        _ => op.p5 as i32,
    }
}

/// Os bytes de `z` até o primeiro NUL.
fn until_nul(z: &[u8]) -> &[u8] {
    &z[..z.iter().position(|&c| c == 0).unwrap_or(z.len())]
}

/// `sqlite3VdbeDisplayComment`: o texto da coluna de comentário da listagem de um opcode (a
/// sinopse do `vdbe.c` traduzida mais o comentário). `z_p4` é o P4 já formatado. `None` é o
/// ponteiro nulo do C (nada acumulado). Não precisa da conexão: não há falha de alocação.
///
/// Traduções: `PX` vira `r[X]`; `PX@PY` vira `r[X..X+Y-1]` (ou `r[x]` se y é 0 ou 1);
/// `PX@PY+1` vira `r[X..X+Y]`; `PX..PY` vira `r[X..Y]` (ou `r[x]` se y é menor ou igual a x).
pub fn display_comment(p_op: &Op, z_p4: &[u8]) -> Option<Vec<u8>> {
    let mut x: Vec<u8> = Vec::new();
    let mut any = false;
    let mut put = |x: &mut Vec<u8>, z: &[u8]| {
        if !z.is_empty() {
            any = true;
        }
        x.extend_from_slice(z);
    };
    let z_op_name = opcode_name(p_op.opcode);
    let n_op_name = z_op_name.iter().position(|&c| c == 0).unwrap_or(z_op_name.len());
    let has_synopsis = z_op_name.get(n_op_name + 1).copied().unwrap_or(0) != 0;
    if has_synopsis {
        let mut seen_com = false;
        let mut syn: Vec<u8> = until_nul(&z_op_name[n_op_name + 1..]).to_vec();
        if syn.starts_with(b"IF ") {
            syn = snprintf(50, b"if %s goto P2", &[PrintfArg::Text(Some(syn[3..].to_vec()))]);
        }
        let at = |i: usize| syn.get(i).copied().unwrap_or(0);
        let rest = |i: usize| syn.get(i..).unwrap_or(&[]);
        let mut ii = 0usize;
        loop {
            let mut c = at(ii);
            if c == 0 {
                break;
            }
            if c == b'P' {
                ii += 1;
                c = at(ii);
                if c == b'4' {
                    put(&mut x, until_nul(z_p4));
                } else if c == b'X' {
                    if let Some(com) = &p_op.comment {
                        if !com.is_empty() && com[0] != 0 {
                            put(&mut x, until_nul(com));
                            seen_com = true;
                            break;
                        }
                    }
                } else {
                    let v1 = translate_p(c, p_op);
                    if rest(ii + 1).starts_with(b"@P") {
                        ii += 3;
                        let mut v2 = translate_p(at(ii), p_op);
                        if rest(ii + 1).starts_with(b"+1") {
                            ii += 2;
                            v2 += 1;
                        }
                        if v2 < 2 {
                            put(&mut x, v1.to_string().as_bytes());
                        } else {
                            let s = format!("{}..{}", v1, v1.wrapping_add(v2).wrapping_sub(1));
                            put(&mut x, s.as_bytes());
                        }
                    } else if rest(ii + 1).starts_with(b"@NP") {
                        let argc = match &p_op.p4 {
                            P4::FuncCtx(ctx) => Some(ctx.argc as i32),
                            _ => None,
                        };
                        match argc {
                            None | Some(1) => put(&mut x, v1.to_string().as_bytes()),
                            Some(n) if n > 1 => {
                                let s = format!("{}..{}", v1, v1.wrapping_add(n).wrapping_sub(1));
                                put(&mut x, s.as_bytes());
                            }
                            Some(_) => {
                                debug_assert!(x.len() > 2);
                                x.truncate(x.len().saturating_sub(2));
                                ii += 1;
                            }
                        }
                        ii += 3;
                    } else {
                        put(&mut x, v1.to_string().as_bytes());
                        if rest(ii + 1).starts_with(b"..P3") && p_op.p3 == 0 {
                            ii += 4;
                        }
                    }
                }
            } else {
                put(&mut x, &[c]);
            }
            ii += 1;
        }
        if !seen_com {
            if let Some(com) = &p_op.comment {
                put(&mut x, b"; ");
                put(&mut x, until_nul(com));
            }
        }
    } else if let Some(com) = &p_op.comment {
        put(&mut x, until_nul(com));
    }
    if any {
        Some(x)
    } else {
        None
    }
}
