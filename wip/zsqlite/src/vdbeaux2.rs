//! `vdbeaux.c` (parte 2): trechos 005 a 009 do `vdbeaux.c` do SQLite 3.46.1. Cobre
//! `sqlite3VdbeDisplayP4`, `sqlite3VdbeUsesBtree`, a lista de opcodes do EXPLAIN
//! (`sqlite3VdbeNextOpcode`, `sqlite3VdbeList`), `sqlite3VdbeRewind`, `sqlite3VdbeMakeReady`,
//! fechamento de cursores e frames, nomes de colunas, commit (inclusive o de vários arquivos com
//! super-journal), `sqlite3VdbeHalt`, `sqlite3VdbeReset`, `sqlite3VdbeFinalize`,
//! `sqlite3VdbeDelete`, auxdata e a restauração de cursores.
//!
//! Desvios do C, todos decorrentes do modelo v2 (ver `vdbeaux.rs`, `vdbe_types.rs` e
//! `CONVENTIONS.md`):
//!
//! * O `Vdbe` não tem `db`: as funções que o C resolvia por `p->db` recebem `&mut Connection`.
//!   Quem chama já retirou o `Vdbe` do slab (`Connection.stmts.take`), por isso `(p, db)` não se
//!   sobrepõem.
//! * `sqlite3VdbeEnter`/`sqlite3VdbeLeave` e `sqlite3BtreeEnter`/`Leave` não existem: o cache
//!   compartilhado está desligado, `lockMask` nunca recebe bit e os mutexes somem. Por isso
//!   `uses_btree` tem só `(p, i)` (o teste `sqlite3BtreeSharable` seria sempre falso).
//! * `ReusableSpace`/`allocSpace` e o reaproveitamento do fim do vetor de opcodes somem: cada
//!   vetor do `Vdbe` é um `Vec` próprio (`apArg` também não existe).
//! * `sqlite3VdbeFreeCursor` (o `if (pCx)`) é repasse e não existe: o chamador testa o `Option`.
//!   `sqlite3VdbeClearObject` foi dobrado em `vdbe_delete` (um só chamador). `vdbeCloseStatement`
//!   e `sqlite3VdbeCloseStatement` são uma função só (`close_statement`).
//! * `sqlite3VdbeFrameMemDel` e `sqlite3VdbeFrameIsValid` não existem (o frame não mora num
//!   `Mem`, ver `vdbe_types.rs`). Frame restaurado fica com os registros e cursores do FILHO
//!   (troca por `mem::swap`), e `frame_delete` os libera.
//! * O vetor de subprogramas do EXPLAIN (o `aMem[9]` como blob de ponteiros) vive em
//!   `a_mem[9].agg` como `Vec<Rc<SubProgram>>`, com `MEM_AGG` ligado para que
//!   `release_mem_array` o solte.
//! * `sqlite3VdbeNextOpcode` não trata `eMode == 2` (`SQLITE_ENABLE_BYTECODE_VTAB` desligado).
//! * Ficam de fora os ramos `SQLITE_DEBUG`, `VDBE_PROFILE`, `SQLITE_ENABLE_SQLLOG`,
//!   `SQLITE_ENABLE_IOTRACE`, `SQLITE_ENABLE_NORMALIZE`, `sqlite3VdbePrintOp`,
//!   `sqlite3VdbePrintSql`, `checkActiveVdbeCnt`, o `pnBytesFreed` (estatística de memória), os
//!   `simulated_io_errors`/`BenignMalloc`, `sqlite3FileSuffix3` (sem `ENABLE_8_3_NAMES` é nulo) e
//!   o bloco `#if 0` de `sqlite3VdbeSerialType`.
//! * `sqlite3VdbeDisplayP4` com `P4_VTAB` imprime o `%p` do C com um endereço derivado do handle.

use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::btree::{btree_close, btree_cursor_has_moved, btree_cursor_restore};
use crate::btree_cursor::{
    btree_close_cursor, btree_commit_phase_one, btree_commit_phase_two, btree_savepoint,
    btree_table_moveto, BtDb,
};
use crate::btree_types::{BtCursor, BtShared, Btree};
use crate::btree_write::btree_txn_state;
use crate::connection::{Connection, Parse};
use crate::consts::*;
use crate::mem::{
    mem_release, mem_release_malloc, mem_set_int64, mem_set_str, mem_set_str_dynamic, Mem,
    StrDtor, ENC_UTF8,
};
use crate::opcodes::opcode_name;
use crate::pager::cstr;
use crate::printf::{snprintf, PrintfArg, StrAccum};
use crate::util::{err_str, strlen30};
use crate::vdbe_types::{
    db_mask_set, AuxData, Op, SubProgram, Vdbe, VdbeCursor, VdbeFrame, P4,
};
use crate::vdbeaux::{display_comment, resolve_p2_values, vdbe_error, vdbe_free_op_array};

/// Os bytes de `z` até o primeiro NUL.
fn until_nul(z: &[u8]) -> &[u8] {
    &z[..z.iter().position(|&c| c == 0).unwrap_or(z.len())]
}

// ---------------------------------------------------------------------------------------------
// chunk 005
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeDisplayP4`: o texto que descreve o P4 do opcode. `None` é o ponteiro nulo.
pub fn display_p4(op: &Op) -> Option<Vec<u8>> {
    let mut x = StrAccum::new(SQLITE_MAX_LENGTH as u32);
    let mut z_p4: Option<Vec<u8>> = None;
    match &op.p4 {
        P4::KeyInfo(ki) => {
            x.appendf(b"k(%d", &[PrintfArg::Int(ki.n_key_field as i64)]);
            for j in 0..ki.n_key_field as usize {
                let coll = ki.a_coll.get(j).and_then(|c| c.as_ref());
                let mut z_coll: &[u8] = coll.map_or(&b""[..], |c| until_nul(&c.name));
                if z_coll == b"BINARY" {
                    z_coll = b"B";
                }
                let fl = ki.a_sort_flags.get(j).copied().unwrap_or(0);
                let desc: &[u8] = if (fl & KEYINFO_ORDER_DESC) != 0 { b"-" } else { b"" };
                let big: &[u8] = if (fl & KEYINFO_ORDER_BIGNULL) != 0 { b"N." } else { b"" };
                x.appendf(
                    b",%s%s%s",
                    &[
                        PrintfArg::Text(Some(desc.to_vec())),
                        PrintfArg::Text(Some(big.to_vec())),
                        PrintfArg::Text(Some(z_coll.to_vec())),
                    ],
                );
            }
            x.append(b")");
        }
        P4::Coll(Some(coll)) => {
            const ENCNAMES: [&[u8]; 4] = [b"?", b"8", b"16LE", b"16BE"];
            let enc = ENCNAMES.get(coll.enc as usize).copied().unwrap_or(b"?");
            x.appendf(
                b"%.18s-%s",
                &[
                    PrintfArg::Text(Some(until_nul(&coll.name).to_vec())),
                    PrintfArg::Text(Some(enc.to_vec())),
                ],
            );
        }
        P4::FuncDef(def) => {
            x.appendf(
                b"%s(%d)",
                &[
                    PrintfArg::Text(Some(until_nul(&def.z_name).to_vec())),
                    PrintfArg::Int(def.n_arg as i64),
                ],
            );
        }
        P4::FuncCtx(ctx) => {
            let def = &ctx.p_func;
            x.appendf(
                b"%s(%d)",
                &[
                    PrintfArg::Text(Some(until_nul(&def.z_name).to_vec())),
                    PrintfArg::Int(def.n_arg as i64),
                ],
            );
        }
        P4::Int64(v) => x.appendf(b"%lld", &[PrintfArg::Int(*v)]),
        P4::Int32(v) => x.appendf(b"%d", &[PrintfArg::Int(*v as i64)]),
        P4::Real(v) => x.appendf(b"%.16g", &[PrintfArg::Double(*v)]),
        P4::Mem(m) => {
            if (m.flags & MEM_STR) != 0 {
                z_p4 = Some(until_nul(m.bytes()).to_vec());
            } else if (m.flags & (MEM_INT | MEM_INTREAL)) != 0 {
                x.appendf(b"%lld", &[PrintfArg::Int(m.u_i)]);
            } else if (m.flags & MEM_REAL) != 0 {
                x.appendf(b"%.16g", &[PrintfArg::Double(m.u_r)]);
            } else if (m.flags & MEM_NULL) != 0 {
                z_p4 = Some(b"NULL".to_vec());
            } else {
                debug_assert!((m.flags & MEM_BLOB) != 0);
                z_p4 = Some(b"(blob)".to_vec());
            }
        }
        P4::Vtab(id) => {
            let addr = 0x55d4_c8e0_0000u64 + (id.0 as u64) * 0x190;
            x.append_all(format!("vtab:0x{:x}", addr).as_bytes());
        }
        P4::IntArray(ai) => {
            let n = ai.first().copied().unwrap_or(0);
            for i in 1..=n as usize {
                let c = if i == 1 { b'[' } else { b',' };
                x.appendf(
                    b"%c%u",
                    &[PrintfArg::Char(c as u32), PrintfArg::Int(ai.get(i).copied().unwrap_or(0) as i64)],
                );
            }
            x.append(b"]");
        }
        P4::Subprogram(_) => z_p4 = Some(b"program".to_vec()),
        P4::Table(t) | P4::TableRef(t) => z_p4 = Some(until_nul(&t.z_name).to_vec()),
        P4::Text(z) | P4::Blob(z) => z_p4 = Some(until_nul(z).to_vec()),
        P4::None | P4::Coll(None) | P4::Expr(_) => {}
    }
    if let Some(z) = z_p4 {
        x.append_all(&z);
    }
    x.finish()
}

/// `sqlite3VdbeUsesBtree`: declara que o `Btree` de `dbs[i]` é usado. Sem cache compartilhado
/// nenhum btree é "sharable", então `lock_mask` não muda.
pub fn uses_btree(p: &mut Vdbe, i: i32) {
    debug_assert!(i >= 0 && (i as usize) < 32);
    db_mask_set(&mut p.btree_mask, i as usize);
}

/// `initMemArray`: um vetor de `n` células com as flags dadas.
fn init_mem_array(n: usize, flags: u16) -> Vec<Mem> {
    (0..n).map(|_| Mem::init(flags)).collect()
}

/// `releaseMemArray`: libera o conteúdo auxiliar das células; as que tinham algo viram
/// `MEM_Undefined`.
fn release_mem_array(a: &mut [Mem]) {
    for p in a.iter_mut() {
        if (p.flags & (MEM_AGG | MEM_DYN)) != 0 {
            mem_release(p);
            p.flags = MEM_UNDEFINED;
        } else if p.sz_malloc != 0 {
            mem_release_malloc(p);
            p.flags = MEM_UNDEFINED;
        }
    }
}

/// De onde vem o vetor de opcodes devolvido por `next_opcode`.
#[derive(Clone)]
pub enum OpSrc {
    /// O programa principal (`Vdbe.a_op`).
    Main,
    /// Um subprograma de gatilho.
    Sub(Rc<SubProgram>),
}

impl OpSrc {
    /// O `aOp` correspondente.
    pub fn ops<'a>(&'a self, p: &'a Vdbe) -> &'a [Op] {
        match self {
            OpSrc::Main => &p.a_op,
            OpSrc::Sub(s) => &s.a_op,
        }
    }
}

/// `sqlite3VdbeNextOpcode`: localiza o próximo opcode a exibir no EXPLAIN. `list_subprogs`
/// equivale a `pSub != 0` (a lista vive em `a_mem[9]`). `e_mode`: 0 normal, 1 EQP. Devolve
/// `SQLITE_OK` quando achou um opcode e `SQLITE_DONE` no fim.
pub fn next_opcode(
    p: &mut Vdbe,
    list_subprogs: bool,
    e_mode: i32,
    pi_pc: &mut i32,
    pi_addr: &mut i32,
    pa_op: &mut OpSrc,
) -> i32 {
    let mut rc = SQLITE_OK;
    let mut n_row = p.n_op();
    let mut subs: Vec<Rc<SubProgram>> = Vec::new();
    if list_subprogs {
        if let Some(v) = p.a_mem.get(9).and_then(|m| m.agg.as_ref()) {
            if let Some(v) = v.downcast_ref::<Vec<Rc<SubProgram>>>() {
                subs = v.clone();
            }
        }
        for s in &subs {
            n_row += s.a_op.len() as i32;
        }
    }
    let mut i_pc = *pi_pc;
    let mut i;
    let mut src = OpSrc::Main;
    loop {
        i = i_pc;
        i_pc += 1;
        if i >= n_row {
            p.rc = SQLITE_OK;
            rc = SQLITE_DONE;
            break;
        }
        if i < p.n_op() {
            src = OpSrc::Main;
        } else {
            i -= p.n_op();
            let mut j = 0usize;
            while i >= subs[j].a_op.len() as i32 {
                i -= subs[j].a_op.len() as i32;
                j += 1;
            }
            src = OpSrc::Sub(Rc::clone(&subs[j]));
        }
        let (opcode, sub_prog) = {
            let op = &src.ops(p)[i as usize];
            (
                op.opcode,
                match &op.p4 {
                    P4::Subprogram(sp) => Some(Rc::clone(sp)),
                    _ => None,
                },
            )
        };
        if list_subprogs {
            if let Some(sp) = sub_prog {
                if !subs.iter().any(|s| Rc::ptr_eq(s, &sp)) {
                    n_row += sp.a_op.len() as i32;
                    subs.push(sp);
                    let m = &mut p.a_mem[9];
                    m.flags |= MEM_AGG;
                    m.set_type_flag(MEM_BLOB);
                    m.n = (subs.len() * std::mem::size_of::<usize>()) as i32;
                    m.agg = Some(Box::new(subs.clone()));
                    p.rc = SQLITE_OK;
                }
            }
        }
        if e_mode == 0 {
            break;
        }
        debug_assert!(e_mode == 1);
        if opcode == OP_EXPLAIN {
            break;
        }
        if opcode == OP_INIT && i_pc > 1 {
            break;
        }
    }
    *pi_pc = i_pc;
    *pi_addr = i;
    *pa_op = src;
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 006
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeFrameDelete`: libera o frame depois de `frame_restore` (neste ponto o frame guarda
/// os cursores e registros do filho).
pub fn frame_delete(db: &mut Connection, frame: VdbeFrame) {
    let VdbeFrame { mut ap_csr, mut a_mem, mut p_aux_data, n_child_csr, n_child_mem, .. } = frame;
    for c in ap_csr.iter_mut().take(n_child_csr.max(0) as usize) {
        if let Some(cx) = c.take() {
            free_cursor_nn(db, cx);
        }
    }
    let n = (n_child_mem.max(0) as usize).min(a_mem.len());
    release_mem_array(&mut a_mem[..n]);
    delete_aux_data(&mut p_aux_data, -1, 0);
}

/// `sqlite3VdbeList`: entrega uma linha do EXPLAIN. Devolve `SQLITE_ROW`, `SQLITE_DONE` ou
/// `SQLITE_ERROR`.
pub fn vdbe_list(p: &mut Vdbe, db: &mut Connection) -> i32 {
    let b_list_subprogs = p.explain == 1 || (db.flags & SQLITE_TRIGGER_EQP) != 0;
    debug_assert!(p.explain != 0);
    debug_assert!(p.e_vdbe_state == VDBE_RUN_STATE);
    debug_assert!(p.rc == SQLITE_OK || p.rc == SQLITE_BUSY || p.rc == SQLITE_NOMEM);
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];

    release_mem_array(&mut p.a_mem[1..9]);

    if p.rc == SQLITE_NOMEM {
        return SQLITE_ERROR;
    }
    debug_assert!(!b_list_subprogs || p.n_mem > 9);

    let mut i = 0;
    let mut src = OpSrc::Main;
    let mut pc = p.pc;
    let mut rc = next_opcode(p, b_list_subprogs, (p.explain == 2) as i32, &mut pc, &mut i, &mut src);
    p.pc = pc;

    if rc == SQLITE_OK {
        if db.interrupted.load(Ordering::Relaxed) {
            p.rc = SQLITE_INTERRUPT;
            rc = SQLITE_ERROR;
            let rc2 = p.rc;
            vdbe_error(p, db, err_str(rc2).as_bytes(), &[]);
        } else {
            let (opcode, p1, p2, p3, p5, z_p4, z_com) = {
                let op = &src.ops(p)[i as usize];
                let z_p4 = display_p4(op);
                let z_com = if p.explain != 2 {
                    display_comment(op, z_p4.as_deref().unwrap_or(&[]))
                } else {
                    None
                };
                (op.opcode, op.p1, op.p2, op.p3, op.p5, z_p4, z_com)
            };
            if p.explain == 2 {
                mem_set_int64(&mut p.a_mem[1], p1 as i64);
                mem_set_int64(&mut p.a_mem[2], p2 as i64);
                mem_set_int64(&mut p.a_mem[3], p3 as i64);
                mem_set_str_dynamic(&mut p.a_mem[4], z_p4, -1, ENC_UTF8, limit);
                debug_assert!(p.n_res_column == 4);
            } else {
                mem_set_int64(&mut p.a_mem[1], i as i64);
                mem_set_str(
                    &mut p.a_mem[2],
                    Some(until_nul(opcode_name(opcode))),
                    -1,
                    ENC_UTF8,
                    StrDtor::Static,
                    limit,
                );
                mem_set_int64(&mut p.a_mem[3], p1 as i64);
                mem_set_int64(&mut p.a_mem[4], p2 as i64);
                mem_set_int64(&mut p.a_mem[5], p3 as i64);
                mem_set_int64(&mut p.a_mem[7], p5 as i64);
                mem_set_str_dynamic(&mut p.a_mem[8], z_com, -1, ENC_UTF8, limit);
                mem_set_str_dynamic(&mut p.a_mem[6], z_p4, -1, ENC_UTF8, limit);
                debug_assert!(p.n_res_column == 8);
            }
            p.p_result_row = Some(1);
            if db.malloc_failed != 0 {
                p.rc = SQLITE_NOMEM;
                rc = SQLITE_ERROR;
            } else {
                p.rc = SQLITE_OK;
                rc = SQLITE_ROW;
            }
        }
    }
    rc
}

/// `sqlite3VdbeRewind`: volta o VDBE ao início, pronto para rodar.
pub fn vdbe_rewind(p: &mut Vdbe) {
    debug_assert!(
        p.e_vdbe_state == VDBE_INIT_STATE
            || p.e_vdbe_state == VDBE_READY_STATE
            || p.e_vdbe_state == VDBE_HALT_STATE
    );
    debug_assert!(!p.a_op.is_empty());
    p.e_vdbe_state = VDBE_READY_STATE;
    p.pc = -1;
    p.rc = SQLITE_OK;
    p.error_action = OE_ABORT;
    p.n_change = 0;
    p.cache_ctr = 1;
    p.min_write_file_format = 255;
    p.i_statement = 0;
    p.n_fk_constraint = 0;
}

/// `sqlite3VdbeMakeReady`: empacota o `Vdbe` de `parse.p_vdbe` (registros, variáveis, cursores)
/// e o põe em `VDBE_READY_STATE`. Chamada uma vez por VM.
pub fn make_ready(parse: &mut Parse) {
    let Parse {
        p_vdbe,
        p_v_list,
        n_var,
        n_mem,
        n_tab,
        n_max_arg,
        is_multi_write,
        may_abort,
        explain,
        a_label,
        n_label,
        ..
    } = parse;
    let Some(p) = p_vdbe.as_mut() else {
        return;
    };
    debug_assert!(!p.a_op.is_empty());
    debug_assert!(p.e_vdbe_state == VDBE_INIT_STATE);
    p.p_v_list = std::mem::take(p_v_list);
    let n_var = *n_var;
    let mut n_mem = *n_mem;
    let n_cursor = *n_tab;
    let mut n_arg = *n_max_arg;

    // Cada cursor usa uma célula; o cursor 0 usa aMem[0].
    n_mem += n_cursor;
    if n_cursor == 0 && n_mem > 0 {
        n_mem += 1;
    }

    resolve_p2_values(p, a_label, n_label, &mut n_arg);
    p.uses_stmt_journal = *is_multi_write != 0 && *may_abort != 0;
    if *explain != 0 {
        if n_mem < 10 {
            n_mem = 10;
        }
        p.explain = *explain;
        p.n_res_column = (12 - 4 * (*explain as i32)) as u16;
    }
    p.expired = 0;

    p.n_cursor = n_cursor;
    p.n_var = n_var;
    p.a_var = init_mem_array(n_var.max(0) as usize, MEM_NULL);
    p.n_mem = n_mem;
    p.a_mem = init_mem_array(n_mem.max(0) as usize, MEM_UNDEFINED);
    p.ap_csr = (0..n_cursor.max(0)).map(|_| None).collect();
    vdbe_rewind(p);
}

// ---------------------------------------------------------------------------------------------
// chunk 007
// ---------------------------------------------------------------------------------------------

/// O `Btree` dono do cursor: o arquivo temporário (`ub.pBtx`) nos efêmeros, senão o de
/// `dbs[i_db]`.
pub(crate) fn cursor_btree<'a>(db: &'a mut Connection, cx: &'a mut VdbeCursor) -> Option<&'a mut Btree> {
    if cx.is_ephemeral && cx.p_btx.is_some() {
        return cx.p_btx.as_deref_mut();
    }
    if cx.i_db < 0 {
        return None;
    }
    db.dbs.get_mut(cx.i_db as usize).and_then(|s| s.bt.as_mut())
}

/// Empresta o `BtCursor` do VDBE (retirado do slab do `BtShared`) junto com o `BtShared`.
pub(crate) fn with_btree_cursor<R>(
    db: &mut Connection,
    cx: &mut VdbeCursor,
    f: impl FnOnce(&mut BtCursor, &mut BtShared) -> R,
) -> Option<R> {
    let id = cx.p_cursor?;
    let bt = cursor_btree(db, cx)?;
    let mut cur = bt.bt.cursors.take(id)?;
    let r = f(&mut cur, &mut bt.bt);
    bt.bt.cursors.put(id, cur);
    Some(r)
}

/// `sqlite3VdbeFreeCursorNN` (e `freeCursorWithCache`): fecha o cursor e libera o que ele guarda.
pub fn free_cursor_nn(db: &mut Connection, mut cx: Box<VdbeCursor>) {
    if cx.col_cache {
        cx.col_cache = false;
        cx.p_cache = None;
    }
    match cx.e_cur_type {
        CURTYPE_SORTER => {
            crate::vdbesort::vdbe_sorter_close(&mut cx);
        }
        CURTYPE_BTREE => {
            debug_assert!(cx.p_cursor.is_some());
            if let Some(id) = cx.p_cursor.take() {
                let last = match cursor_btree(db, &mut cx) {
                    Some(b) => btree_close_cursor(b, id),
                    None => false,
                };
                if last {
                    if let Some(b) = cx.p_btx.take() {
                        btree_close(*b, &BtDb::default(), None);
                    }
                }
            }
        }
        CURTYPE_VTAB => {
            if let (Some(mut vcur), Some(id)) = (cx.p_v_cur.take(), cx.p_v_table) {
                if let Some(mut vt) = db.vtabs.take(id.slot()) {
                    debug_assert!(vt.n_cursor > 0);
                    vt.n_cursor -= 1;
                    if let Some(mut vtab) = vt.p_vtab.take() {
                        vcur.close(db, &mut *vtab);
                        vt.p_vtab = Some(vtab);
                    }
                    db.vtabs.put(id.slot(), vt);
                }
            }
        }
        _ => {}
    }
}

/// `closeCursorsInFrame`: fecha todos os cursores do frame corrente.
fn close_cursors_in_frame(p: &mut Vdbe, db: &mut Connection) {
    let n = p.n_cursor.max(0) as usize;
    for i in 0..n.min(p.ap_csr.len()) {
        if let Some(cx) = p.ap_csr[i].take() {
            free_cursor_nn(db, cx);
        }
    }
}

/// `sqlite3VdbeFrameRestore`: devolve ao `Vdbe` o estado do chamador guardado no frame. O frame
/// passa a guardar os registros e cursores do filho (para `frame_delete`). Devolve o `pc`.
pub fn frame_restore(v: &mut Vdbe, db: &mut Connection, frame: &mut VdbeFrame) -> i32 {
    close_cursors_in_frame(v, db);
    std::mem::swap(&mut v.p_cur_prog, &mut frame.a_op);
    std::mem::swap(&mut v.a_mem, &mut frame.a_mem);
    std::mem::swap(&mut v.n_mem, &mut frame.n_mem);
    std::mem::swap(&mut v.ap_csr, &mut frame.ap_csr);
    std::mem::swap(&mut v.n_cursor, &mut frame.n_cursor);
    db.last_rowid = frame.last_rowid;
    v.n_change = frame.n_change;
    db.n_change = frame.n_db_change;
    delete_aux_data(&mut v.p_aux_data, -1, 0);
    v.p_aux_data = std::mem::take(&mut frame.p_aux_data);
    frame.pc
}

/// `closeAllCursors`: fecha os cursores e libera os registros (e os frames pendentes).
fn close_all_cursors(p: &mut Vdbe, db: &mut Connection) {
    while let Some(mut f) = p.p_frame.pop() {
        frame_restore(p, db, &mut f);
        p.p_del_frame.push(f);
    }
    close_cursors_in_frame(p, db);
    release_mem_array(&mut p.a_mem);
    while let Some(f) = p.p_del_frame.pop() {
        frame_delete(db, f);
    }
    delete_aux_data(&mut p.p_aux_data, -1, 0);
}

/// `sqlite3VdbeSetNumCols`: número de colunas do resultado.
pub fn set_num_cols(p: &mut Vdbe, n_res_column: i32) {
    if p.n_res_alloc != 0 {
        release_mem_array(&mut p.a_col_name);
        p.a_col_name = Vec::new();
    }
    let n = n_res_column.max(0) * COLNAME_N;
    p.n_res_column = n_res_column as u16;
    p.n_res_alloc = n_res_column as u16;
    p.a_col_name = init_mem_array(n as usize, MEM_NULL);
}

/// `sqlite3VdbeSetColName`: o nome (ou outro `COLNAME_*`) da coluna `idx`.
pub fn set_col_name(
    p: &mut Vdbe,
    db: &Connection,
    idx: i32,
    var: i32,
    z_name: Option<&[u8]>,
    x_del: StrDtor,
) -> i32 {
    debug_assert!(idx < p.n_res_alloc as i32);
    debug_assert!(var < COLNAME_N);
    if db.malloc_failed != 0 {
        return SQLITE_NOMEM_BKPT;
    }
    let i = (idx + var * p.n_res_alloc as i32) as usize;
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    mem_set_str(&mut p.a_col_name[i], z_name, -1, ENC_UTF8, x_del, limit)
}

/// Cria o `BtDb` que as rotinas de transação do btree leem da conexão e roda `f` com o `Btree`
/// de `dbs[i]`. `None` se não há `Btree`. O busy-handler não é lido por commit e rollback.
pub(crate) fn with_bt_db<R>(
    db: &mut Connection,
    i: usize,
    f: impl FnOnce(&mut Btree, &mut BtDb<'_>) -> R,
) -> Option<R> {
    let Connection { dbs, x_autovac_pages, n_savepoint, n_vdbe_read, temp_store, .. } = db;
    let slot = dbs.get_mut(i)?;
    let name = slot.z_db_s_name.clone();
    let bt = slot.bt.as_mut()?;
    let has_autovac = x_autovac_pages.is_some();
    let mut cb = |n_orig: u32, n_free: u32, page_size: u32| -> u32 {
        match x_autovac_pages.as_mut() {
            Some(h) => h(&name, n_orig, n_free, page_size),
            None => 0,
        }
    };
    let mut bdb = BtDb {
        n_savepoint: *n_savepoint,
        n_vdbe_read: *n_vdbe_read,
        temp_in_memory: *temp_store == 2,
        busy: None,
        autovac_pages: if has_autovac { Some(&mut cb) } else { None },
    };
    Some(f(bt, &mut bdb))
}

/// `vdbeCommit`: confirma a transação ativa; com escrita em mais de um arquivo usa um
/// super-journal.
fn vdbe_commit(db: &mut Connection, p: &mut Vdbe) -> i32 {
    // Matriz de quais modos de journal usam super-journal: DELETE, PERSIST, OFF, TRUNCATE,
    // MEMORY, WAL.
    const MJ_NEEDED: [u8; 6] = [1, 1, 0, 1, 0, 0];
    let mut n_trans = 0;
    let mut need_xcommit = false;

    let mut rc = crate::vtab::vtab_sync(db, p);

    for i in 0..db.dbs.len() {
        if rc != SQLITE_OK {
            break;
        }
        if btree_txn_state(db.dbs[i].bt.as_ref()) == SQLITE_TXN_WRITE {
            need_xcommit = true;
            let safety = db.dbs[i].safety_level as u32;
            if let Some(bt) = db.dbs[i].bt.as_mut() {
                let pager = &mut bt.bt.pager;
                if safety != PAGER_SYNCHRONOUS_OFF
                    && MJ_NEEDED.get(pager.journal_mode as usize).copied().unwrap_or(0) != 0
                    && !pager.is_memdb()
                {
                    debug_assert!(i != 1);
                    n_trans += 1;
                }
                rc = pager.exclusive_lock();
            }
        }
    }
    if rc != SQLITE_OK {
        return rc;
    }

    if need_xcommit {
        if let Some(cb) = db.x_commit_callback.as_mut() {
            if cb() != 0 {
                return SQLITE_CONSTRAINT_COMMITHOOK;
            }
        }
    }

    let z_main_file: Vec<u8> = db
        .dbs
        .first()
        .and_then(|s| s.bt.as_ref())
        .map(|b| cstr(b.bt.pager.filename(true)).to_vec())
        .unwrap_or_default();

    if strlen30(&z_main_file) == 0 || n_trans <= 1 {
        for i in 0..db.dbs.len() {
            if rc != SQLITE_OK {
                break;
            }
            if let Some(r) = with_bt_db(db, i, |bt, bdb| btree_commit_phase_one(bt, None, bdb)) {
                rc = r;
            }
        }
        for i in 0..db.dbs.len() {
            if rc != SQLITE_OK {
                break;
            }
            if let Some(r) = with_bt_db(db, i, |bt, bdb| btree_commit_phase_two(bt, false, bdb)) {
                rc = r;
            }
        }
        if rc == SQLITE_OK {
            crate::vtab::vtab_commit(db);
        }
    } else {
        // Vários arquivos com transação de escrita: super-journal.
        let Some(vfs) = db.p_vfs.clone() else {
            return SQLITE_ERROR;
        };
        let n_main_file = z_main_file.len();
        let mut z_super: Vec<u8> = z_main_file.clone();
        let mut retry_count = 0;
        let mut res: i32 = 0;
        loop {
            if retry_count != 0 {
                if retry_count > 100 {
                    crate::global::log(
                        SQLITE_FULL,
                        b"MJ delete: %s",
                        &[PrintfArg::Text(Some(z_super.clone()))],
                    );
                    vfs.delete(&z_super, 0);
                    break;
                } else if retry_count == 1 {
                    crate::global::log(
                        SQLITE_FULL,
                        b"MJ collide: %s",
                        &[PrintfArg::Text(Some(z_super.clone()))],
                    );
                }
            }
            retry_count += 1;
            let mut rnd = [0u8; 4];
            crate::global::randomness(&mut rnd);
            let i_random = u32::from_ne_bytes(rnd);
            z_super.truncate(n_main_file);
            z_super.extend(snprintf(
                13,
                b"-mj%06X9%02X",
                &[
                    PrintfArg::Int(((i_random >> 8) & 0xffffff) as i64),
                    PrintfArg::Int((i_random & 0xff) as i64),
                ],
            ));
            debug_assert!(z_super[z_super.len() - 3] == b'9');
            rc = vfs.access(&z_super, SQLITE_ACCESS_EXISTS, &mut res);
            if !(rc == SQLITE_OK && res != 0) {
                break;
            }
        }
        let mut jrnl: Option<Box<dyn crate::os::VfsFile>> = None;
        if rc == SQLITE_OK {
            let mut out_flags = 0;
            match crate::os::os_open(
                &*vfs,
                Some(&z_super),
                SQLITE_OPEN_READWRITE
                    | SQLITE_OPEN_CREATE
                    | SQLITE_OPEN_EXCLUSIVE
                    | SQLITE_OPEN_SUPER_JOURNAL,
                &mut out_flags,
            ) {
                Ok(f) => jrnl = Some(f),
                Err(e) => rc = e,
            }
        }
        let Some(jf) = jrnl.as_mut() else {
            return rc;
        };
        if rc != SQLITE_OK {
            return rc;
        }

        // Grava o nome do journal de cada banco no super-journal.
        let mut offset: i64 = 0;
        for i in 0..db.dbs.len() {
            if btree_txn_state(db.dbs[i].bt.as_ref()) == SQLITE_TXN_WRITE {
                let z_file: Vec<u8> = match db.dbs[i].bt.as_ref() {
                    Some(b) => cstr(&b.bt.pager.z_journal).to_vec(),
                    None => Vec::new(),
                };
                if z_file.is_empty() {
                    continue;
                }
                let mut buf = z_file.clone();
                buf.push(0);
                rc = jf.write(&buf, offset);
                offset += buf.len() as i64;
                if rc != SQLITE_OK {
                    crate::os::os_close(&mut jrnl);
                    vfs.delete(&z_super, 0);
                    return rc;
                }
            }
        }

        // Sincroniza o super-journal, salvo com IOCAP_SEQUENTIAL.
        if (jf.device_characteristics() & SQLITE_IOCAP_SEQUENTIAL) == 0 {
            rc = crate::os::os_sync(&mut **jf, SQLITE_SYNC_NORMAL);
            if rc != SQLITE_OK {
                crate::os::os_close(&mut jrnl);
                vfs.delete(&z_super, 0);
                return rc;
            }
        }

        // Sincroniza os arquivos; a mesma chamada grava o ponteiro de super-journal em cada
        // journal.
        for i in 0..db.dbs.len() {
            if rc != SQLITE_OK {
                break;
            }
            let zs = z_super.clone();
            if let Some(r) =
                with_bt_db(db, i, |bt, bdb| btree_commit_phase_one(bt, Some(&zs), bdb))
            {
                rc = r;
            }
        }
        crate::os::os_close(&mut jrnl);
        debug_assert!(rc != SQLITE_BUSY);
        if rc != SQLITE_OK {
            return rc;
        }

        // Apagar o super-journal confirma a transação.
        rc = vfs.delete(&z_super, 1);
        if rc != 0 {
            return rc;
        }

        // Daqui em diante só se fecham arquivos e apagam journals; erro não importa.
        for i in 0..db.dbs.len() {
            with_bt_db(db, i, |bt, bdb| btree_commit_phase_two(bt, true, bdb));
        }
        crate::vtab::vtab_commit(db);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 008
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeCloseStatement` (com `vdbeCloseStatement`): fecha a transação de statement aberta
/// pela VM. `e_op` é `SAVEPOINT_ROLLBACK` ou `SAVEPOINT_RELEASE`.
pub fn close_statement(p: &mut Vdbe, db: &mut Connection, e_op: i32) -> i32 {
    if db.n_statement == 0 || p.i_statement == 0 {
        return SQLITE_OK;
    }
    let mut rc = SQLITE_OK;
    let i_savepoint = p.i_statement - 1;
    debug_assert!(e_op == SAVEPOINT_ROLLBACK || e_op == SAVEPOINT_RELEASE);
    debug_assert!(p.i_statement == db.n_statement + db.n_savepoint);

    for i in 0..db.dbs.len() {
        let mut rc2 = SQLITE_OK;
        if let Some(bt) = db.dbs[i].bt.as_mut() {
            if e_op == SAVEPOINT_ROLLBACK {
                rc2 = btree_savepoint(bt, SAVEPOINT_ROLLBACK, i_savepoint);
            }
            if rc2 == SQLITE_OK {
                rc2 = btree_savepoint(bt, SAVEPOINT_RELEASE, i_savepoint);
            }
            if rc == SQLITE_OK {
                rc = rc2;
            }
        }
    }
    db.n_statement -= 1;
    p.i_statement = 0;

    if rc == SQLITE_OK {
        if e_op == SAVEPOINT_ROLLBACK {
            rc = crate::vtab::vtab_savepoint(db, SAVEPOINT_ROLLBACK, i_savepoint);
        }
        if rc == SQLITE_OK {
            rc = crate::vtab::vtab_savepoint(db, SAVEPOINT_RELEASE, i_savepoint);
        }
    }

    // No rollback do statement, restaura os contadores de restrições adiadas.
    if e_op == SAVEPOINT_ROLLBACK {
        db.n_deferred_cons = p.n_stmt_def_cons;
        db.n_deferred_imm_cons = p.n_stmt_def_imm_cons;
    }
    rc
}

/// `sqlite3VdbeCheckFk`: há violações de chave estrangeira que impeçam o commit?
pub fn check_fk(p: &mut Vdbe, db: &Connection, deferred: bool) -> i32 {
    if (deferred && (db.n_deferred_cons + db.n_deferred_imm_cons) > 0)
        || (!deferred && p.n_fk_constraint > 0)
    {
        p.rc = SQLITE_CONSTRAINT_FOREIGNKEY;
        p.error_action = OE_ABORT;
        vdbe_error(p, db, b"FOREIGN KEY constraint failed", &[]);
        if (p.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) == 0 {
            return SQLITE_ERROR;
        }
        return SQLITE_CONSTRAINT_FOREIGNKEY;
    }
    SQLITE_OK
}

/// `sqlite3VdbeHalt`: chamada quando a VM tenta parar. Confirma ou desfaz o que for preciso e
/// move a VM de `VDBE_RUN_STATE` para `VDBE_HALT_STATE`. Devolve `SQLITE_BUSY` se o commit não
/// pôde terminar (a parada precisa ser repetida).
pub fn vdbe_halt(p: &mut Vdbe, db: &mut Connection) -> i32 {
    debug_assert!(p.e_vdbe_state == VDBE_RUN_STATE);
    if db.malloc_failed != 0 {
        p.rc = SQLITE_NOMEM_BKPT;
    }
    close_all_cursors(p, db);

    if p.b_is_reader {
        let mut e_statement_op = 0;
        let mrc;
        let is_special_error;
        if p.rc != 0 {
            mrc = p.rc & 0xff;
            is_special_error =
                mrc == SQLITE_NOMEM || mrc == SQLITE_IOERR || mrc == SQLITE_INTERRUPT || mrc == SQLITE_FULL;
        } else {
            mrc = 0;
            is_special_error = false;
        }
        if is_special_error && (!p.read_only || mrc != SQLITE_INTERRUPT) {
            if (mrc == SQLITE_NOMEM || mrc == SQLITE_FULL) && p.uses_stmt_journal {
                e_statement_op = SAVEPOINT_ROLLBACK;
            } else {
                crate::main::rollback_all(db, SQLITE_ABORT_ROLLBACK);
                crate::main::close_savepoints(db);
                db.auto_commit = 1;
                p.n_change = 0;
            }
        }

        // Violações imediatas de chave estrangeira.
        if p.rc == SQLITE_OK || (p.error_action == OE_FAIL && !is_special_error) {
            let _ = check_fk(p, db, false);
        }

        if !crate::vtab::vtab_in_sync(db)
            && db.auto_commit != 0
            && db.n_vdbe_write == (!p.read_only) as i32
        {
            if p.rc == SQLITE_OK || (p.error_action == OE_FAIL && !is_special_error) {
                let mut rc = check_fk(p, db, true);
                if rc != SQLITE_OK {
                    if p.read_only {
                        return SQLITE_ERROR;
                    }
                    rc = SQLITE_CONSTRAINT_FOREIGNKEY;
                } else if (db.flags & SQLITE_CORRUPT_RD_ONLY) != 0 {
                    rc = SQLITE_CORRUPT;
                    db.flags &= !SQLITE_CORRUPT_RD_ONLY;
                } else {
                    rc = vdbe_commit(db, p);
                }
                if rc == SQLITE_BUSY && p.read_only {
                    return SQLITE_BUSY;
                } else if rc != SQLITE_OK {
                    crate::util::system_error(db, rc);
                    p.rc = rc;
                    crate::main::rollback_all(db, SQLITE_OK);
                    p.n_change = 0;
                } else {
                    db.n_deferred_cons = 0;
                    db.n_deferred_imm_cons = 0;
                    db.flags &= !SQLITE_DEFER_FKS;
                    crate::build::commit_internal_changes(db);
                }
            } else if p.rc == SQLITE_SCHEMA && db.n_vdbe_active > 1 {
                p.n_change = 0;
            } else {
                crate::main::rollback_all(db, SQLITE_OK);
                p.n_change = 0;
            }
            db.n_statement = 0;
        } else if e_statement_op == 0 {
            if p.rc == SQLITE_OK || p.error_action == OE_FAIL {
                e_statement_op = SAVEPOINT_RELEASE;
            } else if p.error_action == OE_ABORT {
                e_statement_op = SAVEPOINT_ROLLBACK;
            } else {
                crate::main::rollback_all(db, SQLITE_ABORT_ROLLBACK);
                crate::main::close_savepoints(db);
                db.auto_commit = 1;
                p.n_change = 0;
            }
        }

        // Transação de statement a confirmar ou desfazer.
        if e_statement_op != 0 {
            let rc = close_statement(p, db, e_statement_op);
            if rc != 0 {
                if p.rc == SQLITE_OK || (p.rc & 0xff) == SQLITE_CONSTRAINT {
                    p.rc = rc;
                    p.z_err_msg = None;
                }
                crate::main::rollback_all(db, SQLITE_ABORT_ROLLBACK);
                crate::main::close_savepoints(db);
                db.auto_commit = 1;
                p.n_change = 0;
            }
        }

        // INSERT, UPDATE ou DELETE sem rollback do statement: atualiza o contador de mudanças.
        if p.change_cnt_on {
            if e_statement_op != SAVEPOINT_ROLLBACK {
                crate::vdbeapi::vdbe_set_changes(db, p.n_change);
            } else {
                crate::vdbeapi::vdbe_set_changes(db, 0);
            }
            p.n_change = 0;
        }
    }

    // A VM parou com sucesso.
    db.n_vdbe_active -= 1;
    if !p.read_only {
        db.n_vdbe_write -= 1;
    }
    if p.b_is_reader {
        db.n_vdbe_read -= 1;
    }
    debug_assert!(db.n_vdbe_active >= db.n_vdbe_read);
    debug_assert!(db.n_vdbe_read >= db.n_vdbe_write);
    debug_assert!(db.n_vdbe_write >= 0);
    p.e_vdbe_state = VDBE_HALT_STATE;
    if db.malloc_failed != 0 {
        p.rc = SQLITE_NOMEM_BKPT;
    }

    // Sem transação, as travas foram soltas: avisa quem espera por unlock-notify.
    if db.auto_commit != 0 {
        crate::notify::connection_unlocked(db);
    }

    debug_assert!(db.n_vdbe_active > 0 || db.auto_commit == 0 || db.n_statement == 0);
    if p.rc == SQLITE_BUSY {
        SQLITE_BUSY
    } else {
        SQLITE_OK
    }
}

/// `sqlite3VdbeResetStepResult`: o resultado do último `sqlite3_step()` volta a `SQLITE_OK`.
pub fn vdbe_reset_step_result(p: &mut Vdbe) {
    p.rc = SQLITE_OK;
}

/// `sqlite3VdbeTransferError`: copia código e mensagem de erro da VM para a conexão.
///
/// `Connection.err_msg` é `Option`: o `pErr` do C "alocado e nulo" e "inexistente" não se
/// distinguem aqui.
pub fn vdbe_transfer_error(p: &Vdbe, db: &mut Connection) -> i32 {
    let rc = p.rc;
    if let Some(msg) = &p.z_err_msg {
        db.err_msg = Some(msg.clone());
    } else if db.err_msg.is_some() {
        db.err_msg = None;
    }
    db.err_code = rc;
    db.err_byte_offset = -1;
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 009
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeReset`: limpa a VM depois da execução sem apagá-la; ela volta a poder rodar.
/// Devolve o código de resultado.
pub fn vdbe_reset(p: &mut Vdbe, db: &mut Connection) -> i32 {
    if p.e_vdbe_state == VDBE_RUN_STATE {
        vdbe_halt(p, db);
    }

    // Se a VM rodou, ainda que em parte, transfere o erro para a conexão.
    if p.pc >= 0 {
        if db.err_msg.is_some() || p.z_err_msg.is_some() {
            vdbe_transfer_error(p, db);
        } else {
            db.err_code = p.rc;
        }
    }

    p.z_err_msg = None;
    p.p_result_row = None;
    p.rc & db.err_mask
}

/// `sqlite3VdbeFinalize`: limpa e apaga a VM. Devolve o código de resultado.
pub fn vdbe_finalize(p: Vdbe, db: &mut Connection) -> i32 {
    let mut p = p;
    let mut rc = SQLITE_OK;
    if p.e_vdbe_state >= VDBE_READY_STATE {
        rc = vdbe_reset(&mut p, db);
        debug_assert!((rc & db.err_mask) == rc);
    }
    vdbe_delete(p, db);
    rc
}

/// `sqlite3VdbeDeleteAuxData`: com `i_op < 0` destrói todos os auxdata da VM; senão só os
/// criados pela função do `OP_Function` em `i_op` cujo argumento é o 32 ou posterior, ou cujo bit
/// em `mask` está limpo. O destrutor roda no `Drop` de `AuxData`.
pub fn delete_aux_data(pp: &mut Vec<AuxData>, i_op: i32, mask: i32) {
    pp.retain(|a| {
        let kill = i_op < 0
            || (a.i_aux_op == i_op
                && a.i_aux_arg >= 0
                && (a.i_aux_arg > 31 || (mask & (1i32 << a.i_aux_arg)) == 0));
        !kill
    });
}

/// `sqlite3VdbeClearObject` mais `sqlite3VdbeDelete`: libera tudo o que a VM possui. Quem chama
/// já tirou o `Vdbe` de `Connection.stmts` e de `Connection.stmt_list`.
pub fn vdbe_delete(p: Vdbe, db: &mut Connection) {
    let mut p = p;
    if !p.a_col_name.is_empty() {
        release_mem_array(&mut p.a_col_name);
    }
    // Primeiro o programa principal: os P4 dele seguram `Rc` dos subprogramas.
    vdbe_free_op_array(db, std::mem::take(&mut p.a_op));
    let subs = std::mem::take(&mut p.p_program);
    for sub in subs.into_iter().rev() {
        if let Ok(s) = Rc::try_unwrap(sub) {
            vdbe_free_op_array(db, s.a_op);
        }
    }
    if p.e_vdbe_state != VDBE_INIT_STATE {
        release_mem_array(&mut p.a_var);
    }
    // `z_sql`, `a_scan`, `p_v_list` e o resto caem com o `Vdbe`.
}

/// `sqlite3VdbeFinishMoveto`: faz o seek adiado do cursor.
pub fn finish_moveto(p: &mut VdbeCursor, db: &mut Connection) -> i32 {
    debug_assert!(p.deferred_moveto);
    debug_assert!(p.is_table);
    debug_assert!(p.e_cur_type == CURTYPE_BTREE);
    let target = p.moveto_target;
    let Some((rc, res)) = with_btree_cursor(db, p, |cur, bt| {
        let mut res = 0;
        let rc = btree_table_moveto(cur, bt, target, 0, &mut res);
        (rc, res)
    }) else {
        return SQLITE_ERROR;
    };
    if rc != 0 {
        return rc;
    }
    if res != 0 {
        return SQLITE_CORRUPT_BKPT;
    }
    p.deferred_moveto = false;
    p.cache_status = CACHE_STALE;
    SQLITE_OK
}

/// `sqlite3VdbeHandleMovedCursor`: algo tirou o cursor do lugar; tenta restaurá-lo e, se a linha
/// sumiu, aponta para uma linha nula.
pub fn handle_moved_cursor(p: &mut VdbeCursor, db: &mut Connection) -> i32 {
    debug_assert!(p.e_cur_type == CURTYPE_BTREE);
    let Some((rc, different)) = with_btree_cursor(db, p, |cur, bt| {
        let mut d = 0;
        let rc = btree_cursor_restore(cur, bt, &mut d);
        (rc, d)
    }) else {
        return SQLITE_OK;
    };
    p.cache_status = CACHE_STALE;
    if different != 0 {
        p.null_row = true;
    }
    rc
}

/// `sqlite3VdbeCursorRestore`: confere que o cursor é válido e o restaura se preciso.
pub fn cursor_restore(p: &mut VdbeCursor, db: &mut Connection) -> i32 {
    debug_assert!(p.e_cur_type == CURTYPE_BTREE || p.is_null_cursor());
    let Some(id) = p.p_cursor else {
        return SQLITE_OK;
    };
    let moved = match cursor_btree(db, p) {
        Some(b) => b.bt.cursors.get(id).is_some_and(btree_cursor_has_moved),
        None => false,
    };
    if moved {
        return handle_moved_cursor(p, db);
    }
    SQLITE_OK
}
