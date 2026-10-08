//! `vdbe.c` (parte 3): trechos 011 a 016 do `vdbe.c` do SQLite 3.46.1, os opcodes de
//! `OP_SorterOpen` até `OP_Destroy`: `SorterOpen`, `SequenceTest`, `OpenPseudo`, `Close`, `SeekLT`,
//! `SeekLE`, `SeekGE`, `SeekGT`, `SeekScan`, `SeekHit`, `IfNotOpen`, `IfNoHope`, `NoConflict`,
//! `NotFound`, `Found`, `SeekRowid`, `NotExists`, `Sequence`, `NewRowid`, `Insert`, `RowCell`,
//! `Delete`, `ResetCount`, `SorterCompare`, `SorterData`, `RowData`, `Rowid`, `NullRow`, `SeekEnd`,
//! `Last`, `IfSizeBetween`, `SorterSort`, `Sort`, `Rewind`, `SorterNext`, `Prev`, `Next`,
//! `IdxInsert`, `SorterInsert`, `IdxDelete`, `DeferredSeek`, `IdxRowid`, `FinishSeek`, `IdxLE`,
//! `IdxGT`, `IdxLT`, `IdxGE` e `Destroy`. O resto cai em [`crate::vdbe_ops3::exec_op3`].
//!
//! Convenções do laço (ver `vdbe.rs` e `vdbe_ops.rs`): `pc` é o índice da instrução corrente no
//! programa corrente (`cur_ops`); `st.op` traz uma cópia dos operandos escalares;
//! `Flow::Continue(pc)` segue para `pc + 1`; `Flow::Jump(dest)` é o `goto jump_to_p2` do C e
//! `Flow::JumpCheck(dest)` o `goto jump_to_p2_and_check_for_interrupt`.
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * O `VdbeCursor` é emprestado direto de `Vdbe.ap_csr` (campo disjunto de `Vdbe.a_mem`, então os
//!   dois podem ser emprestados ao mesmo tempo); só os opcodes que precisam do `Vdbe` inteiro
//!   enquanto mexem no cursor (`DeferredSeek`/`IdxRowid`, `RowCell`) o retiram com `take` e o
//!   devolvem no fim.
//! * Os registros de uma chave desempacotada (`r.aMem = &aMem[P3]` do C) viram uma cópia das
//!   células em um `UnpackedRecord` ([`unpacked_from_regs`]): o `UnpackedRecord` do modelo v2 é
//!   dono dos seus valores.
//! * O cursor de pseudotabela e o cursor de `OP_NullRow` não têm `BtCursor` (`p_cursor` fica
//!   `None`): é o `sqlite3BtreeFakeValidCursor` do C, que nunca se move.
//! * `OP_Insert`, `OP_Delete`: o `zDb` do gancho de atualização é uma cópia do nome do banco.
//!   O gancho de pré-atualização recebe o `Vdbe` e o cursor por empréstimo compartilhado.
//! * `OP_NewRowid` com `P3` num subprograma: o registro do `AUTOINCREMENT` fica no primeiro frame
//!   da pilha `Vdbe.p_frame` (o que guarda os registros do programa raiz).
//! * `OP_Rowid` em tabela virtual move o cursor e o `Vtab` para fora de seus donos durante a
//!   chamada de `xRowid` (como em `free_cursor_nn`).
//! * `sqlite3ReportError(SQLITE_CORRUPT_INDEX, ...)` de `OP_IdxDelete` registra a mesma mensagem
//!   de `sqlite3_log` do C ("index corruption at line 99658 of [c9c2ab54ba]").
//! * Os ramos `SQLITE_DEBUG`, `SQLITE_TEST`, `VdbeBranchTaken`, `REGISTER_TRACE`,
//!   `UPDATE_MAX_BLOBSIZE`, `memAboutToChange`, `seekOp` e `sqlite3VdbeIncrWriteCounter` não
//!   existem.

use std::rc::Rc;

use crate::btree::btree_clear_cursor;
use crate::btree_cursor::{
    btree_cursor_is_valid_nn, btree_eof, btree_first, btree_index_moveto, btree_integer_key,
    btree_last, btree_next, btree_payload_size, btree_previous, btree_row_count_est,
    btree_table_moveto,
};
use crate::btree_types::{BtCursor, BtShared, Btree, BtreePayload};
use crate::btree_write::{btree_delete, btree_drop_table, btree_insert, btree_transfer_row};
use crate::build::writable_schema;
use crate::build2::root_page_moved;
use crate::connection::Connection;
use crate::consts::*;
use crate::global::{log, randomness};
use crate::mem::{
    apply_affinity, apply_numeric_affinity, int_float_compare, mem_expand_blob, mem_integerify,
    mem_set_null, vdbe_int_value, KeyInfo, Mem, UnpackedRecord,
};
use crate::printf::PrintfArg;
use crate::record::{alloc_unpacked_record, record_unpack};
use crate::util::log_est;
use crate::vdbe::{allocate_cursor, cur_ops, deephemeralize, is_sorter, out2_prerelease, ExecState, Flow};
use crate::vdbe_ops::p4_int32;
use crate::vdbe_types::{Vdbe, VdbeCursor, P4};
use crate::vdbeaux2::{
    cursor_btree, cursor_restore, finish_moveto, free_cursor_nn, with_btree_cursor,
};
use crate::vdbeaux3::{
    mem_from_cursor_zero_offset, vdbe_idx_key_compare, vdbe_idx_rowid, vdbe_pre_update_hook,
    vdbe_set_changes, vtab_import_errmsg,
};
use crate::vdbesort::{
    vdbe_sorter_compare, vdbe_sorter_init, vdbe_sorter_next, vdbe_sorter_rewind,
    vdbe_sorter_rowkey, vdbe_sorter_write,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------

/// O cursor `i` de `Vdbe.ap_csr` (`p->apCsr[i]`); `None` se não está aberto.
pub(crate) fn csr(ap_csr: &mut [Option<Box<VdbeCursor>>], i: i32) -> Option<&mut VdbeCursor> {
    ap_csr.get_mut(usize::try_from(i).ok()?).and_then(|s| s.as_deref_mut())
}

/// A chave desempacotada `r` do C: `r.aMem = &aMem[first]`, `r.nField = n_field`. As `n_field`
/// células de `a_mem` a partir de `first` são copiadas para o `UnpackedRecord`.
pub fn unpacked_from_regs(
    a_mem: &[Mem],
    first: i32,
    n_field: i32,
    key_info: &Rc<KeyInfo>,
    default_rc: i8,
) -> UnpackedRecord {
    let first = first.max(0) as usize;
    let n = n_field.max(0) as usize;
    UnpackedRecord {
        p_key_info: Rc::clone(key_info),
        a_mem: a_mem.get(first..first + n).unwrap_or(&[]).to_vec(),
        u_i: 0,
        n: 0,
        n_field: n as u16,
        default_rc,
        err_code: 0,
        r1: 0,
        r2: 0,
        eq_seen: 0,
    }
}

/// Como `with_btree_cursor`, mas entrega também `Btree.has_incrblob_cur` do `Btree` dono do
/// cursor (o `p->hasIncrblobCur` que `sqlite3BtreeInsert` e `sqlite3BtreeDelete` leem).
fn with_cursor_incr<R>(
    db: &mut Connection,
    c: &mut VdbeCursor,
    f: impl FnOnce(&mut BtCursor, &mut BtShared, bool) -> R,
) -> Option<R> {
    let incr = cursor_btree(db, c)?.has_incrblob_cur;
    with_btree_cursor(db, c, |cur, bt| f(cur, bt, incr))
}

/// `HAS_UPDATE_HOOK(db)`.
#[inline]
fn has_update_hook(db: &Connection) -> bool {
    db.x_pre_update_callback.is_some() || db.x_update_callback.is_some()
}

/// Executa um opcode da fatia `SorterOpen` até `Destroy`; devolve o destino do fluxo.
pub fn exec_op2(db: &mut Connection, p: &mut Vdbe, pc: usize, st: &mut ExecState) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    match o.opcode {
        // Opcode: SorterOpen P1 P2 P3 P4 *: como OP_OpenEphemeral, mas num ordenador externo.
        OP_SORTEROPEN => op_sorter_open(db, p, st, pc),

        // Opcode: SequenceTest P1 P2 * * *: if( cursor[P1].ctr++ ) pc = P2.
        OP_SEQUENCETEST => {
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(is_sorter(c));
            let first = c.seq_count == 0;
            c.seq_count = c.seq_count.wrapping_add(1);
            if first {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: OpenPseudo P1 P2 P3 * *: P3 colunas em r[P2].
        OP_OPENPSEUDO => {
            debug_assert!(o.p1 >= 0 && o.p3 >= 0);
            let Some(cx) = allocate_cursor(db, p, o.p1, o.p3, CURTYPE_PSEUDO) else {
                return Flow::NoMem;
            };
            cx.null_row = true;
            cx.seek_result = o.p2;
            cx.is_table = true;
            // O `uc.pCursor` do C é `sqlite3BtreeFakeValidCursor()`: aqui é `p_cursor == None`.
            debug_assert!(o.p5 == 0);
            Flow::Continue(pci)
        }

        // Opcode: Close P1 * * * *.
        OP_CLOSE => {
            let cx = p.ap_csr.get_mut(o.p1 as usize).and_then(|s| s.take());
            if let Some(cx) = cx {
                free_cursor_nn(db, cx);
            }
            Flow::Continue(pci)
        }

        // Opcode: SeekLT, SeekLE, SeekGE, SeekGT P1 P2 P3 P4 *.
        OP_SEEKLT | OP_SEEKLE | OP_SEEKGE | OP_SEEKGT => op_seek(db, p, st, pc),

        // Opcode: SeekScan P1 P2 * * P5: prefixo do OP_SeekGE seguinte.
        OP_SEEKSCAN => op_seek_scan(db, p, st, pc),

        // Opcode: SeekHit P1 P2 P3 * *: P2<=seekHit<=P3.
        OP_SEEKHIT => {
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(o.p3 >= o.p2);
            if (c.seek_hit as i32) < o.p2 {
                c.seek_hit = o.p2 as u16;
            } else if (c.seek_hit as i32) > o.p3 {
                c.seek_hit = o.p3 as u16;
            }
            Flow::Continue(pci)
        }

        // Opcode: IfNotOpen P1 P2 * * *: if( !csr[P1] ) goto P2.
        OP_IFNOTOPEN => {
            let none_or_null = match csr(&mut p.ap_csr, o.p1) {
                None => true,
                Some(c) => c.null_row,
            };
            if none_or_null {
                Flow::JumpCheck(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IfNoHope, NoConflict, NotFound, Found P1 P2 P3 P4 *: key=r[P3@P4].
        OP_IFNOHOPE | OP_NOCONFLICT | OP_NOTFOUND | OP_FOUND => op_found(db, p, st, pc),

        // Opcode: SeekRowid, NotExists P1 P2 P3 * *: intkey=r[P3].
        OP_SEEKROWID | OP_NOTEXISTS => op_seek_rowid(db, p, st, pc),

        // Opcode: Sequence P1 P2 * * *: r[P2]=cursor[P1].ctr++.
        OP_SEQUENCE => {
            out2_prerelease(&mut p.a_mem, o.p2);
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(c.e_cur_type != CURTYPE_VTAB);
            let v = c.seq_count;
            c.seq_count = v.wrapping_add(1);
            p.a_mem[o.p2 as usize].u_i = v;
            Flow::Continue(pci)
        }

        // Opcode: NewRowid P1 P2 P3 * *: r[P2]=rowid.
        OP_NEWROWID => op_new_rowid(db, p, st, pc),

        // Opcode: Insert P1 P2 P3 P4 P5: intkey=r[P3] data=r[P2].
        OP_INSERT => op_insert(db, p, st, pc),

        // Opcode: RowCell P1 P2 P3 * *: copia a linha corrente de P2 para P1.
        OP_ROWCELL => op_row_cell(db, p, st, pc),

        // Opcode: Delete P1 P2 P3 P4 P5.
        OP_DELETE => op_delete(db, p, st, pc),

        // Opcode: ResetCount * * * * *.
        OP_RESETCOUNT => {
            vdbe_set_changes(db, p.n_change);
            p.n_change = 0;
            Flow::Continue(pci)
        }

        // Opcode: SorterCompare P1 P2 P3 P4: if key(P1)!=trim(r[P3],P4) goto P2.
        OP_SORTERCOMPARE => {
            let n_key_col = p4_int32(&cur_ops(p)[pc]);
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(is_sorter(c));
            let mut res = 0;
            let rc = vdbe_sorter_compare(c, &p.a_mem[o.p3 as usize], n_key_col, &mut res);
            if rc != SQLITE_OK {
                st.abort(rc)
            } else if res != 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: SorterData P1 P2 P3 * *: r[P2]=data.
        OP_SORTERDATA => {
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(is_sorter(c));
            let rc = vdbe_sorter_rowkey(c, &mut p.a_mem[o.p2 as usize]);
            debug_assert!(rc != SQLITE_OK || (p.a_mem[o.p2 as usize].flags & MEM_BLOB) != 0);
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
            if let Some(pseudo) = csr(&mut p.ap_csr, o.p3) {
                pseudo.cache_status = CACHE_STALE;
            }
            Flow::Continue(pci)
        }

        // Opcode: RowData P1 P2 P3 * *: r[P2]=data.
        OP_ROWDATA => op_row_data(db, p, st, pci),

        // Opcode: Rowid P1 P2 * * *: r[P2]=PX rowid of P1.
        OP_ROWID => op_rowid(db, p, st, pci),

        // Opcode: NullRow P1 * * * *.
        OP_NULLROW => {
            if csr(&mut p.ap_csr, o.p1).is_none() {
                // Cursor ainda não aberto: um pseudocursor que devolve NULL em toda coluna.
                let Some(cx) = allocate_cursor(db, p, o.p1, 1, CURTYPE_PSEUDO) else {
                    return Flow::NoMem;
                };
                cx.seek_result = 0;
                cx.is_table = true;
                cx.no_reuse = true;
            }
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            c.null_row = true;
            c.cache_status = CACHE_STALE;
            if c.e_cur_type == CURTYPE_BTREE {
                with_btree_cursor(db, c, |cur, _bt| btree_clear_cursor(cur));
            }
            Flow::Continue(pci)
        }

        // Opcode: SeekEnd P1 * * * *  e  Opcode: Last P1 P2 * * *.
        OP_SEEKEND | OP_LAST => op_last(db, p, st, pci),

        // Opcode: IfSizeBetween P1 P2 P3 P4 *.
        OP_IFSIZEBETWEEN => {
            let p4 = p4_int32(&cur_ops(p)[pc]);
            debug_assert!(o.p3 >= -1 && o.p3 <= 640 * 2 && p4 >= -1 && p4 <= 640 * 2);
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            let r = with_btree_cursor(db, c, |cur, bt| {
                let mut res = 0;
                let rc = btree_first(cur, bt, &mut res);
                let sz: i64 = if rc != SQLITE_OK || res != 0 {
                    -1 // codificação de -infinito
                } else {
                    let n = btree_row_count_est(cur, bt);
                    debug_assert!(n > 0);
                    log_est(n as u64) as i64
                };
                (rc, sz)
            });
            let (rc, sz) = r.unwrap_or((SQLITE_ERROR, -1));
            if rc != SQLITE_OK {
                st.abort(rc)
            } else if sz >= o.p3 as i64 && sz <= p4 as i64 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: SorterSort, Sort, Rewind P1 P2 * * *.
        OP_SORTERSORT | OP_SORT | OP_REWIND => op_rewind(db, p, st, pci),

        // Opcode: SorterNext, Prev, Next P1 P2 P3 * P5.
        OP_SORTERNEXT | OP_PREV | OP_NEXT => op_next(db, p, st, pci),

        // Opcode: IdxInsert P1 P2 P3 P4 P5: key=r[P2].
        OP_IDXINSERT => op_idx_insert(db, p, st, pc),

        // Opcode: SorterInsert P1 P2 * * *: key=r[P2].
        OP_SORTERINSERT => {
            let key = &mut p.a_mem[o.p2 as usize];
            debug_assert!((key.flags & MEM_BLOB) != 0);
            if (key.flags & MEM_ZERO) != 0 {
                let rc = mem_expand_blob(key);
                if rc != SQLITE_OK {
                    return st.abort(rc);
                }
            }
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(is_sorter(c) && !c.is_table);
            let rc = vdbe_sorter_write(c, &p.a_mem[o.p2 as usize]);
            if rc != SQLITE_OK {
                st.abort(rc)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IdxDelete P1 P2 P3 * P5: key=r[P2@P3].
        OP_IDXDELETE => op_idx_delete(db, p, st, pci),

        // Opcode: DeferredSeek P1 * P3 P4 *  e  Opcode: IdxRowid P1 P2 * * *.
        OP_DEFERREDSEEK | OP_IDXROWID => op_idx_rowid(db, p, st, pc),

        // Opcode: FinishSeek P1 * * * *.
        OP_FINISHSEEK => {
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            if c.deferred_moveto {
                let rc = finish_moveto(c, db);
                if rc != SQLITE_OK {
                    return st.abort(rc);
                }
            }
            Flow::Continue(pci)
        }

        // Opcode: IdxLE, IdxGT, IdxLT, IdxGE P1 P2 P3 P4 *: key=r[P3@P4].
        OP_IDXLE | OP_IDXGT | OP_IDXLT | OP_IDXGE => op_idx_compare(db, p, st, pc),

        // Opcode: Destroy P1 P2 P3 * *.
        OP_DESTROY => op_destroy(db, p, st, pci),

        // Os demais opcodes.
        _ => crate::vdbe_ops3::exec_op3(db, p, pc, st),
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 011 e 012: SorterOpen, Seek*, SeekScan, Found e companhia
// ---------------------------------------------------------------------------------------------

/// `OP_SorterOpen`: abre o cursor P1 num ordenador externo (P2 campos; P3 campos da chave
/// estável; P4 a `KeyInfo`).
fn op_sorter_open(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    debug_assert!(o.p1 >= 0 && o.p2 >= 0);
    let key_info = match &cur_ops(p)[pc].p4 {
        P4::KeyInfo(ki) => Some(Rc::clone(ki)),
        _ => None,
    };
    let Some(cx) = allocate_cursor(db, p, o.p1, o.p2, CURTYPE_SORTER) else {
        return Flow::NoMem;
    };
    cx.p_key_info = key_info;
    let rc = vdbe_sorter_init(db, o.p3, cx);
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc as i32)
    }
}

/// `OP_SeekLT`, `OP_SeekLE`, `OP_SeekGE` e `OP_SeekGT`: reposiciona o cursor P1 na menor entrada
/// `>=` (ou `>`) ou na maior entrada `<=` (ou `<`) da chave, que está em r[P3] (tabela) ou em
/// r[P3@P4] (índice). Sem entrada, salta para P2.
fn op_seek(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let n_field = p4_int32(&cur_ops(p)[pc]);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && c.is_ordered);
    debug_assert!(OP_SEEKLE == OP_SEEKLT + 1 && OP_SEEKGE == OP_SEEKLT + 2 && OP_SEEKGT == OP_SEEKLT + 3);
    let mut oc = o.opcode;
    let mut eq_only = false;
    let mut res: i32 = 0;
    // `goto seek_not_found`: pula o ajuste de `res` pelo `Next`/`Previous`.
    let mut not_found = false;
    c.null_row = false;
    c.deferred_moveto = false;
    c.cache_status = CACHE_STALE;
    if c.is_table {
        // O valor de P3 pode ter qualquer tipo, mas precisa ser inteiro para o seek.
        let in3 = &mut p.a_mem[o.p3 as usize];
        let flags3 = in3.flags;
        if (flags3 & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_STR)) == MEM_STR {
            apply_numeric_affinity(in3, false);
        }
        let i_key = vdbe_int_value(in3);
        // O tipo depois da afinidade numérica; mas o registro volta ao tipo original.
        let new_type = in3.flags;
        in3.flags = flags3;
        let r_real = in3.u_r;

        // Se o valor não virou inteiro sem perda de informação, é preciso um tratamento especial.
        if (new_type & (MEM_INT | MEM_INTREAL)) == 0 {
            if (new_type & MEM_REAL) == 0 {
                if (new_type & MEM_NULL) != 0 || oc >= OP_SEEKGE {
                    return Flow::Jump(o.p2);
                }
                let rc = with_btree_cursor(db, c, |cur, bt| btree_last(cur, bt, &mut res))
                    .unwrap_or(SQLITE_ERROR);
                if rc != SQLITE_OK {
                    return st.abort(rc);
                }
                not_found = true;
            } else {
                let cmp = int_float_compare(i_key, r_real);
                if cmp > 0 {
                    // A aproximação inteira é maior que o real: troca > por >= e <= por <.
                    if (oc & 0x0001) == (OP_SEEKGT & 0x0001) {
                        oc -= 1;
                    }
                } else if cmp < 0 {
                    // A aproximação é menor que o real: troca < por <= e >= por >.
                    if (oc & 0x0001) == (OP_SEEKLT & 0x0001) {
                        oc += 1;
                    }
                }
            }
        }
        if !not_found {
            let rc = with_btree_cursor(db, c, |cur, bt| btree_table_moveto(cur, bt, i_key, 0, &mut res))
                .unwrap_or(SQLITE_ERROR);
            c.moveto_target = i_key; // usado pelo OP_Delete
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
        }
    } else {
        let Some(key_info) = c.p_key_info.clone() else {
            return st.abort(SQLITE_CORRUPT_BKPT);
        };
        debug_assert!(n_field > 0);
        // `(1 & (oc - OP_SeekLT)) ? -1 : +1`
        let default_rc: i8 = if (1 & (oc - OP_SEEKLT)) != 0 { -1 } else { 1 };
        let mut r = unpacked_from_regs(&p.a_mem, o.p3, n_field, &key_info, default_rc);
        r.eq_seen = 0;
        // Num cursor com a dica BTREE_SEEK_EQ só SeekGE e SeekLE são permitidos, seguidos de
        // IdxGT ou IdxLT com a mesma chave.
        let moved = with_btree_cursor(db, c, |cur, bt| {
            let eq = (cur.hints as u32 & BTREE_SEEK_EQ) != 0;
            let rc = btree_index_moveto(cur, bt, &mut r, &mut res);
            (eq, rc)
        });
        let Some((eq, rc)) = moved else {
            return st.abort(SQLITE_ERROR);
        };
        eq_only = eq;
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        if eq_only && r.eq_seen == 0 {
            debug_assert!(res != 0);
            not_found = true;
        }
    }
    if !not_found {
        let step = with_btree_cursor(db, c, |cur, bt| {
            let mut res2 = res;
            let mut rc = SQLITE_OK;
            if oc >= OP_SEEKGE {
                debug_assert!(oc == OP_SEEKGE || oc == OP_SEEKGT);
                if res2 < 0 || (res2 == 0 && oc == OP_SEEKGT) {
                    res2 = 0;
                    rc = btree_next(cur, bt, 0);
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                        res2 = 1;
                    }
                } else {
                    res2 = 0;
                }
            } else {
                debug_assert!(oc == OP_SEEKLT || oc == OP_SEEKLE);
                if res2 > 0 || (res2 == 0 && oc == OP_SEEKLT) {
                    res2 = 0;
                    rc = btree_previous(cur, bt, 0);
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                        res2 = 1;
                    }
                } else {
                    // `res` pode ser negativo porque a tabela está vazia: confere.
                    res2 = btree_eof(cur) as i32;
                }
            }
            (rc, res2)
        });
        let (rc, res2) = step.unwrap_or((SQLITE_ERROR, 0));
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        res = res2;
    }
    // seek_not_found:
    debug_assert!(o.p2 > 0);
    if res != 0 {
        Flow::Jump(o.p2)
    } else if eq_only {
        // Salta o OP_IdxLt ou OP_IdxGt que vem a seguir.
        Flow::Continue(pci + 1)
    } else {
        Flow::Continue(pci)
    }
}

/// `OP_SeekScan`: prefixo do `OP_SeekGE` seguinte. Tenta chegar à chave alvo andando até P1
/// passos com `sqlite3BtreeNext()`, em vez de refazer o seek.
fn op_seek_scan(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    // O `OP_SeekGE` que vem logo depois: seus P1 a P4 descrevem o alvo.
    let (seek_p1, seek_p2, seek_p3, seek_n_field) = {
        let ops = cur_ops(p);
        debug_assert!(ops[pc + 1].opcode == OP_SEEKGE);
        let next = &ops[pc + 1];
        (next.p1, next.p2, next.p3, p4_int32(next))
    };
    debug_assert!(o.p2 >= pci + 2);
    debug_assert!(o.p1 > 0);
    let Some(c) = csr(&mut p.ap_csr, seek_p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.is_table);
    let valid = with_btree_cursor(db, c, |cur, _bt| btree_cursor_is_valid_nn(cur)).unwrap_or(false);
    if !valid {
        return Flow::Continue(pci);
    }
    let mut n_step = o.p1;
    let Some(key_info) = c.p_key_info.clone() else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let mut r = unpacked_from_regs(&p.a_mem, seek_p3, seek_n_field, &key_info, 0);
    loop {
        let mut res = 0;
        let rc = with_btree_cursor(db, c, |cur, bt| vdbe_idx_key_compare(cur, bt, &mut r, &mut res))
            .unwrap_or(SQLITE_ERROR);
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        if res > 0 && o.p5 == 0 {
            // seekscan_search_fail: salta para o P2 do SeekGE, terminando o laço.
            return Flow::Jump(seek_p2);
        }
        if res >= 0 {
            // Salta para o P2 desta instrução, passando por cima do OP_SeekGE.
            return Flow::Jump(o.p2);
        }
        if n_step <= 0 {
            return Flow::Continue(pci);
        }
        n_step -= 1;
        c.cache_status = CACHE_STALE;
        let rc = with_btree_cursor(db, c, |cur, bt| btree_next(cur, bt, 0)).unwrap_or(SQLITE_ERROR);
        if rc != SQLITE_OK {
            if rc == SQLITE_DONE {
                return Flow::Jump(seek_p2);
            }
            return st.abort(rc);
        }
    }
}

/// `OP_IfNoHope`, `OP_NoConflict`, `OP_NotFound` e `OP_Found`: procura num índice (cursor P1) uma
/// entrada com a chave de P3 (um registro de `MakeRecord` se P4 é 0, senão P4 registros).
fn op_found(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let n_field = p4_int32(&cur_ops(p)[pc]);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    if o.opcode == OP_IFNOHOPE && (c.seek_hit as i32) >= n_field {
        return Flow::Continue(pci);
    }
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.is_table);
    let Some(key_info) = c.p_key_info.clone() else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let mut res: i32 = 0;
    let rc;
    if n_field > 0 {
        // Valores da chave em registros consecutivos.
        let mut r = unpacked_from_regs(&p.a_mem, o.p3, n_field, &key_info, 0);
        rc = with_btree_cursor(db, c, |cur, bt| btree_index_moveto(cur, bt, &mut r, &mut res))
            .unwrap_or(SQLITE_ERROR);
    } else {
        // Chave composta gerada por OP_MakeRecord.
        debug_assert!(o.opcode != OP_NOCONFLICT);
        let blob = &mut p.a_mem[o.p3 as usize];
        debug_assert!((blob.flags & MEM_BLOB) != 0);
        if (blob.flags & MEM_ZERO) != 0 && mem_expand_blob(blob) != SQLITE_OK {
            return Flow::NoMem;
        }
        let mut idx_key = alloc_unpacked_record(Rc::clone(&key_info));
        record_unpack(&key_info, blob.bytes(), &mut idx_key);
        idx_key.default_rc = 0;
        rc = with_btree_cursor(db, c, |cur, bt| btree_index_moveto(cur, bt, &mut idx_key, &mut res))
            .unwrap_or(SQLITE_ERROR);
    }
    // O C passa `&pC->seekResult` direto ao moveto.
    c.seek_result = res;
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    let already_exists = res == 0;
    c.null_row = !already_exists;
    c.deferred_moveto = false;
    c.cache_status = CACHE_STALE;
    if o.opcode == OP_FOUND {
        return if already_exists { Flow::Jump(o.p2) } else { Flow::Continue(pci) };
    }
    if !already_exists {
        return Flow::Jump(o.p2);
    }
    if o.opcode == OP_NOCONFLICT {
        // Se algum campo da chave é NULL, a chave não conflita com nenhuma outra.
        for ii in 0..n_field.max(0) {
            if (p.a_mem[(o.p3 + ii) as usize].flags & MEM_NULL) != 0 {
                return Flow::Jump(o.p2);
            }
        }
    }
    if o.opcode == OP_IFNOHOPE {
        c.seek_hit = n_field as u16;
    }
    Flow::Continue(pci)
}

// ---------------------------------------------------------------------------------------------
// chunk 013: SeekRowid, NotExists, NewRowid, Insert
// ---------------------------------------------------------------------------------------------

/// `OP_SeekRowid` e `OP_NotExists`: o cursor P1 (numa tabela) vai para a linha de rowid r[P3];
/// se não há, salta para P2 (ou, com P2 zero, é `SQLITE_CORRUPT`). Com `OP_SeekRowid` r[P3] pode
/// não ser inteiro e então o salto é sempre feito.
fn op_seek_rowid(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let in3 = &p.a_mem[o.p3 as usize];
    let i_key: i64;
    if o.opcode == OP_SEEKROWID && (in3.flags & (MEM_INT | MEM_INTREAL)) == 0 {
        // Se r[P3] não tem um inteiro, calcula a chave como o valor inteiro de uma cópia: não se
        // pode mudar o tipo do registro, que é usado por outras partes do comando.
        let mut x = in3.clone();
        apply_affinity(&mut x, SQLITE_AFF_NUMERIC, st.encoding);
        if (x.flags & MEM_INT) == 0 {
            return Flow::Jump(o.p2);
        }
        i_key = x.u_i;
    } else {
        debug_assert!((in3.flags & MEM_INT) != 0 || o.opcode == OP_SEEKROWID);
        i_key = in3.u_i;
    }
    // notExistsWithKey:
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.is_table && c.e_cur_type == CURTYPE_BTREE);
    let mut res: i32 = 0;
    let mut rc = with_btree_cursor(db, c, |cur, bt| btree_table_moveto(cur, bt, i_key, 0, &mut res))
        .unwrap_or(SQLITE_ERROR);
    debug_assert!(rc == SQLITE_OK || res == 0);
    c.moveto_target = i_key; // usado pelo OP_Delete
    c.null_row = false;
    c.cache_status = CACHE_STALE;
    c.deferred_moveto = false;
    c.seek_result = res;
    if res != 0 {
        debug_assert!(rc == SQLITE_OK);
        if o.p2 == 0 {
            rc = SQLITE_CORRUPT_BKPT;
        } else {
            return Flow::Jump(o.p2);
        }
    }
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pci)
    }
}

/// `OP_NewRowid`: r[P2] recebe um rowid novo, ainda não usado na tabela do cursor P1. Com P3 > 0,
/// r[P3] guarda o maior rowid já gerado (o `AUTOINCREMENT`) e é atualizado.
fn op_new_rowid(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let mut v: i64 = 0; // o rowid novo
    let mut res: i32 = 0;
    out2_prerelease(&mut p.a_mem, o.p2);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.is_table && c.e_cur_type == CURTYPE_BTREE);
    // O rowid vem de um algoritmo de dois passos: primeiro o maior rowid existente mais um; se
    // ele já é o maior inteiro positivo, o segundo escolhe rowids ao acaso (até 100 vezes).
    if !c.use_random_rowid {
        let r = with_btree_cursor(db, c, |cur, bt| {
            let rc = btree_last(cur, bt, &mut res);
            let key = if rc == SQLITE_OK && res == 0 { btree_integer_key(cur, bt) } else { 0 };
            (rc, key)
        });
        let (rc, key) = r.unwrap_or((SQLITE_ERROR, 0));
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        if res != 0 {
            v = 1; // IMP: R-61914-48074
        } else if key >= i64::MAX {
            c.use_random_rowid = true;
        } else {
            v = key + 1; // IMP: R-29538-34987
        }
    }

    if o.p3 != 0 {
        // O registro do AUTOINCREMENT mora no frame raiz, se estamos num subprograma.
        debug_assert!(o.p3 > 0);
        let mem = match p.p_frame.first_mut() {
            Some(root) => &mut root.a_mem[o.p3 as usize],
            None => &mut p.a_mem[o.p3 as usize],
        };
        mem_integerify(mem);
        debug_assert!((mem.flags & MEM_INT) != 0);
        if mem.u_i == i64::MAX || c.use_random_rowid {
            return st.abort(SQLITE_FULL); // IMP: R-17817-00630
        }
        if v < mem.u_i + 1 {
            v = mem.u_i + 1;
        }
        mem.u_i = v;
    }
    if c.use_random_rowid {
        // IMPLEMENTATION-OF: R-07677-41881 Se o maior ROWID é o maior inteiro possível, o motor
        // escolhe candidatos positivos ao acaso até achar um não usado.
        debug_assert!(o.p3 == 0);
        let mut cnt = 0;
        let mut rc;
        loop {
            let mut buf = [0u8; 8];
            randomness(&mut buf);
            v = i64::from_ne_bytes(buf);
            v &= i64::MAX >> 1;
            v += 1; // garante v > 0
            let key = v;
            rc = with_btree_cursor(db, c, |cur, bt| btree_table_moveto(cur, bt, key, 0, &mut res))
                .unwrap_or(SQLITE_ERROR);
            if rc != SQLITE_OK || res != 0 {
                break;
            }
            cnt += 1;
            if cnt >= 100 {
                break;
            }
        }
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        if res == 0 {
            return st.abort(SQLITE_FULL); // IMP: R-38219-53002
        }
        debug_assert!(v > 0); // EV: R-40812-03570
    }
    c.deferred_moveto = false;
    c.cache_status = CACHE_STALE;
    p.a_mem[o.p2 as usize].u_i = v;
    Flow::Continue(pci)
}

/// `OP_Insert`: grava no cursor P1 a linha de chave r[P3] (um inteiro) e dados r[P2] (um blob).
fn op_insert(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let key = p.a_mem[o.p3 as usize].u_i;
    debug_assert!((p.a_mem[o.p3 as usize].flags & MEM_INT) != 0);
    let Some(i_db) = csr(&mut p.ap_csr, o.p1).map(|c| c.i_db) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    // Se o gancho de atualização ou o de pré-atualização valem, `tab` e `z_db` os alimentam.
    let mut tab = match &cur_ops(p)[pc].p4 {
        P4::Table(t) if has_update_hook(db) => Some(Rc::clone(t)),
        _ => None,
    };
    let z_db: Vec<u8> = if tab.is_some() {
        db.dbs.get(i_db as usize).map(|s| s.z_db_s_name.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let is_update = (o.p5 & OPFLAG_ISUPDATE as u16) != 0;

    // Gancho de pré-atualização.
    if let Some(t) = tab.as_ref() {
        if db.x_pre_update_callback.is_some() && !is_update {
            if let Some(c) = p.ap_csr.get(o.p1 as usize).and_then(|s| s.as_deref()) {
                vdbe_pre_update_hook(db, p, p.stmt_id, c, SQLITE_INSERT, t, key, o.p2, -1);
            }
        }
    }
    // Evita que o gancho pós-atualização rode onde não deve.
    if tab.as_ref().is_some_and(|t| db.x_update_callback.is_none() || t.a_col.is_empty()) {
        tab = None;
    }
    if (o.p5 & OPFLAG_ISNOOP as u16) != 0 {
        return Flow::Continue(pci);
    }

    debug_assert!((o.p5 & OPFLAG_LASTROWID as u16) == 0 || (o.p5 & OPFLAG_NCHANGE as u16) != 0);
    if (o.p5 & OPFLAG_NCHANGE as u16) != 0 {
        p.n_change += 1;
        if (o.p5 & OPFLAG_LASTROWID as u16) != 0 {
            db.last_rowid = key;
        }
    }
    let data = &p.a_mem[o.p2 as usize];
    debug_assert!((data.flags & (MEM_BLOB | MEM_STR)) != 0 || data.n == 0);
    let x = BtreePayload {
        p_key: None,
        n_key: key,
        p_data: Some(data.bytes()),
        n_data: data.n,
        n_zero: if (data.flags & MEM_ZERO) != 0 { data.n_zero } else { 0 },
        ..BtreePayload::default()
    };
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.deferred_moveto);
    let seek_result = if (o.p5 & OPFLAG_USESEEKRESULT as u16) != 0 { c.seek_result } else { 0 };
    debug_assert!(BTREE_PREFORMAT == OPFLAG_PREFORMAT as u32);
    let bt_flags = (o.p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT) as u16) as i32;
    let rc = with_cursor_incr(db, c, |cur, bt, incr| {
        btree_insert(cur, bt, &x, bt_flags, seek_result, incr)
    })
    .unwrap_or(SQLITE_ERROR);
    c.deferred_moveto = false;
    c.cache_status = CACHE_STALE;
    st.col_cache_ctr = st.col_cache_ctr.wrapping_add(1);

    // Gancho de atualização.
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if let Some(t) = tab {
        debug_assert!(db.x_update_callback.is_some() && !t.a_col.is_empty());
        let op = if is_update { SQLITE_UPDATE } else { SQLITE_INSERT };
        if let Some(f) = db.x_update_callback.as_mut() {
            f(op, &z_db, &t.z_name, key);
        }
    }
    Flow::Continue(pci)
}

// ---------------------------------------------------------------------------------------------
// chunk 014: RowCell, Delete, RowData, Rowid
// ---------------------------------------------------------------------------------------------

/// `sqlite3BtreeTransferRow(dest, src, iKey)`: prepara em `dest` a célula que copia a linha
/// corrente de `src`. Os dois cursores podem estar no mesmo `BtShared` ou em `Btree`s diferentes
/// (bancos anexados ou arquivos temporários); no segundo caso o `Btree` de `src` é retirado do
/// dono durante a chamada.
fn transfer_row_between(
    db: &mut Connection,
    dest: &mut VdbeCursor,
    src: &mut VdbeCursor,
    i_key: i64,
) -> i32 {
    let (Some(_dest_id), Some(src_id)) = (dest.p_cursor, src.p_cursor) else {
        return SQLITE_ERROR;
    };
    let src_own = src.is_ephemeral && src.p_btx.is_some();
    let dest_own = dest.is_ephemeral && dest.p_btx.is_some();
    if !src_own && !dest_own && src.i_db == dest.i_db {
        // Mesmo `BtShared`: o cursor de origem sai do mesmo slab do de destino.
        return with_btree_cursor(db, dest, |dcur, bt| {
            let Some(mut scur) = bt.cursors.take(src_id) else {
                return SQLITE_ERROR;
            };
            let rc = btree_transfer_row(dcur, &mut scur, bt, None, i_key);
            bt.cursors.put(src_id, scur);
            rc
        })
        .unwrap_or(SQLITE_ERROR);
    }
    let src_slot = usize::try_from(src.i_db).ok();
    let owned: Option<Btree> = if src_own {
        src.p_btx.take().map(|b| *b)
    } else {
        src_slot.and_then(|i| db.dbs.get_mut(i)).and_then(|s| s.bt.take())
    };
    let Some(mut src_btree) = owned else {
        return SQLITE_ERROR;
    };
    let rc = with_btree_cursor(db, dest, |dcur, dbt| {
        let Some(mut scur) = src_btree.bt.cursors.take(src_id) else {
            return SQLITE_ERROR;
        };
        let rc = btree_transfer_row(dcur, &mut scur, dbt, Some(&mut src_btree.bt), i_key);
        src_btree.bt.cursors.put(src_id, scur);
        rc
    })
    .unwrap_or(SQLITE_ERROR);
    // Devolve o `Btree` ao dono.
    if src_own {
        src.p_btx = Some(Box::new(src_btree));
    } else if let Some(slot) = src_slot.and_then(|i| db.dbs.get_mut(i)) {
        slot.bt = Some(src_btree);
    }
    rc
}

/// `OP_RowCell`: o cursor P1 recebe uma cópia da linha corrente do cursor P2 (tabelas por rowid
/// r[P3]; índices sem P3). Vem sempre antes de um `OP_Insert` ou `OP_IdxInsert` com PREFORMAT.
fn op_row_cell(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let (di, si) = (o.p1 as usize, o.p2 as usize);
    debug_assert!(di != si);
    let i_key = if o.p3 != 0 { p.a_mem[o.p3 as usize].u_i } else { 0 };
    let mut dest = p.ap_csr.get_mut(di).and_then(|s| s.take());
    let mut src = p.ap_csr.get_mut(si).and_then(|s| s.take());
    let rc = match (dest.as_deref_mut(), src.as_deref_mut()) {
        (Some(d), Some(s)) => transfer_row_between(db, d, s, i_key),
        _ => SQLITE_CORRUPT_BKPT,
    };
    p.ap_csr[di] = dest;
    p.ap_csr[si] = src;
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc as i32)
    }
}

/// `OP_Delete`: apaga a linha do cursor P1. P2 leva OPFLAG_NCHANGE, OPFLAG_ISNOOP e
/// OPFLAG_ISUPDATE; P5 leva OPFLAG_SAVEPOSITION e OPFLAG_AUXDELETE (os flags do btree).
fn op_delete(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let opflags = o.p2;
    let p4_tab = match &cur_ops(p)[pc].p4 {
        P4::Table(t) if has_update_hook(db) => Some(Rc::clone(t)),
        _ => None,
    };
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.deferred_moveto);
    // Se o gancho de atualização ou o de pré-atualização valem, `z_db` é o nome do banco e `tab`
    // a tabela; e se o cursor andou por Next ou Prev (P5) o `movetoTarget` é a linha corrente.
    let mut z_db: Vec<u8> = Vec::new();
    let mut tab = None;
    if let Some(t) = p4_tab {
        z_db = db.dbs.get(c.i_db as usize).map(|s| s.z_db_s_name.clone()).unwrap_or_default();
        if (o.p5 & OPFLAG_SAVEPOSITION as u16) != 0 && c.is_table {
            c.moveto_target =
                with_btree_cursor(db, c, |cur, bt| btree_integer_key(cur, bt)).unwrap_or(0);
        }
        tab = Some(t);
    }
    // Gancho de pré-atualização.
    if db.x_pre_update_callback.is_some() {
        if let Some(t) = tab.as_ref() {
            if let Some(c) = p.ap_csr.get(o.p1 as usize).and_then(|s| s.as_deref()) {
                let op = if (opflags & OPFLAG_ISUPDATE as i32) != 0 { SQLITE_UPDATE } else { SQLITE_DELETE };
                vdbe_pre_update_hook(db, p, p.stmt_id, c, op, t, c.moveto_target, o.p3, -1);
            }
        }
    }
    if (opflags & OPFLAG_ISNOOP as i32) != 0 {
        return Flow::Continue(pci);
    }

    // Só podem estar ligados SAVEPOSITION e AUXDELETE.
    debug_assert!((o.p5 & !(OPFLAG_SAVEPOSITION as u16 | OPFLAG_AUXDELETE as u16)) == 0);
    debug_assert!(OPFLAG_SAVEPOSITION as u32 == BTREE_SAVEPOSITION);
    debug_assert!(OPFLAG_AUXDELETE as u32 == BTREE_AUXDELETE);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let rc = with_cursor_incr(db, c, |cur, bt, incr| btree_delete(cur, bt, o.p5 as u8, incr))
        .unwrap_or(SQLITE_ERROR);
    c.cache_status = CACHE_STALE;
    st.col_cache_ctr = st.col_cache_ctr.wrapping_add(1);
    c.seek_result = 0;
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    let moveto_target = c.moveto_target;

    // Gancho de atualização.
    if (opflags & OPFLAG_NCHANGE as i32) != 0 {
        p.n_change += 1;
        if let Some(t) = tab.as_ref() {
            if t.has_rowid() {
                if let Some(f) = db.x_update_callback.as_mut() {
                    f(SQLITE_DELETE, &z_db, &t.z_name, moveto_target);
                }
            }
        }
    }
    Flow::Continue(pci)
}

/// `OP_RowData`: r[P2] recebe o conteúdo inteiro da linha do cursor P1 (a chave, se for índice).
fn op_row_data(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    let out = out2_prerelease(&mut p.a_mem, o.p2);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE && !is_sorter(c));
    debug_assert!(!c.null_row && !c.deferred_moveto);
    let r = with_btree_cursor(db, c, |cur, bt| {
        let n = btree_payload_size(cur, bt);
        if n > limit as u32 {
            return Err(());
        }
        Ok(mem_from_cursor_zero_offset(out, cur, bt, n))
    });
    let rc = match r {
        Some(Ok(rc)) => rc,
        Some(Err(())) => return Flow::TooBig,
        None => SQLITE_ERROR,
    };
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if o.p3 == 0 && deephemeralize(&mut p.a_mem[o.p2 as usize]) {
        return Flow::NoMem;
    }
    Flow::Continue(pc)
}

/// `xRowid` do cursor de tabela virtual P1 (com `sqlite3VtabImportErrmsg`).
fn vtab_rowid(db: &mut Connection, p: &mut Vdbe, i_cur: i32) -> Result<i64, i32> {
    let Some(c) = csr(&mut p.ap_csr, i_cur) else {
        return Err(SQLITE_CORRUPT_BKPT);
    };
    let (Some(mut vcur), Some(vid)) = (c.p_v_cur.take(), c.p_v_table) else {
        return Err(SQLITE_ERROR);
    };
    let mut v: i64 = 0;
    let mut rc = SQLITE_ERROR;
    if let Some(mut vt) = db.vtabs.take(vid.slot()) {
        if let Some(mut vtab) = vt.p_vtab.take() {
            rc = vcur.rowid(&mut *vtab, &mut v);
            vtab_import_errmsg(p, vtab.err_msg_mut());
            vt.p_vtab = Some(vtab);
        }
        db.vtabs.put(vid.slot(), vt);
    }
    if let Some(c) = csr(&mut p.ap_csr, i_cur) {
        c.p_v_cur = Some(vcur);
    }
    if rc != SQLITE_OK {
        Err(rc)
    } else {
        Ok(v)
    }
}

/// `OP_Rowid`: r[P2] recebe a chave da linha do cursor P1 (tabela comum ou virtual).
fn op_rowid(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    out2_prerelease(&mut p.a_mem, o.p2);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type != CURTYPE_PSEUDO || c.null_row);
    let v: i64;
    if c.null_row {
        p.a_mem[o.p2 as usize].flags = MEM_NULL;
        return Flow::Continue(pc);
    } else if c.deferred_moveto {
        v = c.moveto_target;
    } else if c.e_cur_type == CURTYPE_VTAB {
        match vtab_rowid(db, p, o.p1) {
            Ok(x) => v = x,
            Err(rc) => return st.abort(rc),
        }
    } else {
        debug_assert!(c.e_cur_type == CURTYPE_BTREE);
        let rc = cursor_restore(c, db);
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
        if c.null_row {
            p.a_mem[o.p2 as usize].flags = MEM_NULL;
            return Flow::Continue(pc);
        }
        v = with_btree_cursor(db, c, |cur, bt| btree_integer_key(cur, bt)).unwrap_or(0);
    }
    p.a_mem[o.p2 as usize].u_i = v;
    Flow::Continue(pc)
}

// ---------------------------------------------------------------------------------------------
// chunk 015: SeekEnd, Last, Rewind, Next, Prev, SorterNext, IdxInsert
// ---------------------------------------------------------------------------------------------

/// `OP_SeekEnd` e `OP_Last`: posiciona o cursor P1 no fim da árvore. Com `OP_Last` e P2 > 0,
/// salta para P2 se a árvore está vazia.
fn op_last(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE);
    if o.opcode == OP_SEEKEND {
        debug_assert!(o.p2 == 0);
        c.seek_result = -1;
        let valid =
            with_btree_cursor(db, c, |cur, _bt| btree_cursor_is_valid_nn(cur)).unwrap_or(false);
        if valid {
            return Flow::Continue(pc);
        }
    }
    let mut res = 0;
    let rc = with_btree_cursor(db, c, |cur, bt| btree_last(cur, bt, &mut res)).unwrap_or(SQLITE_ERROR);
    c.null_row = res != 0;
    c.deferred_moveto = false;
    c.cache_status = CACHE_STALE;
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if o.p2 > 0 && res != 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pc)
    }
}

/// `OP_SorterSort`, `OP_Sort` e `OP_Rewind`: o cursor P1 vai para a primeira entrada; árvore
/// vazia salta para P2 (se P2 > 0).
fn op_rewind(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    if o.opcode != OP_REWIND {
        let k = SQLITE_STMTSTATUS_SORT as usize;
        p.a_counter[k] = p.a_counter[k].wrapping_add(1);
    }
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(is_sorter(c) == (o.opcode == OP_SORTERSORT));
    let mut res: i32 = 1;
    let rc;
    if is_sorter(c) {
        rc = vdbe_sorter_rewind(c, &mut res);
    } else {
        debug_assert!(c.e_cur_type == CURTYPE_BTREE);
        rc = with_btree_cursor(db, c, |cur, bt| btree_first(cur, bt, &mut res)).unwrap_or(SQLITE_ERROR);
        c.deferred_moveto = false;
        c.cache_status = CACHE_STALE;
    }
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    c.null_row = res != 0;
    if o.p2 > 0 && res != 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pc)
    }
}

/// `OP_SorterNext`, `OP_Prev` e `OP_Next`: avança (ou recua) o cursor P1; se andou, salta para P2
/// e soma 1 no contador P5; no fim da árvore cai na instrução seguinte.
fn op_next(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let rc = match o.opcode {
        OP_SORTERNEXT => {
            debug_assert!(is_sorter(c));
            vdbe_sorter_next(c)
        }
        OP_PREV => {
            debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.deferred_moveto);
            with_btree_cursor(db, c, |cur, bt| btree_previous(cur, bt, o.p3)).unwrap_or(SQLITE_ERROR)
        }
        _ => {
            debug_assert!(c.e_cur_type == CURTYPE_BTREE && !c.deferred_moveto);
            with_btree_cursor(db, c, |cur, bt| btree_next(cur, bt, o.p3)).unwrap_or(SQLITE_ERROR)
        }
    };
    // next_tail:
    c.cache_status = CACHE_STALE;
    if rc == SQLITE_OK {
        c.null_row = false;
        let k = o.p5 as usize;
        p.a_counter[k] = p.a_counter[k].wrapping_add(1);
        return Flow::JumpCheck(o.p2);
    }
    if rc != SQLITE_DONE {
        return st.abort(rc);
    }
    c.null_row = true;
    // goto check_for_interrupt
    Flow::JumpCheck(pc + 1)
}

/// `OP_IdxInsert`: grava no índice P1 a chave r[P2] (um blob de `MakeRecord`); P4 > 0 diz que há
/// P4 registros desempacotados a partir de r[P3].
fn op_idx_insert(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let n_mem = p4_int32(&cur_ops(p)[pc]);
    if (o.p5 & OPFLAG_NCHANGE as u16) != 0 {
        p.n_change += 1;
    }
    let key = &mut p.a_mem[o.p2 as usize];
    debug_assert!((key.flags & MEM_BLOB) != 0 || (o.p5 & OPFLAG_PREFORMAT as u16) != 0);
    if (key.flags & MEM_ZERO) != 0 {
        let rc = mem_expand_blob(key);
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
    }
    let key = &p.a_mem[o.p2 as usize];
    let first = o.p3.max(0) as usize;
    let regs = p.a_mem.get(first..first + n_mem.max(0) as usize).unwrap_or(&[]);
    let x = BtreePayload {
        p_key: Some(key.bytes()),
        n_key: key.n as i64,
        a_mem: regs,
        n_mem: n_mem as u16,
        ..BtreePayload::default()
    };
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(!is_sorter(c) && c.e_cur_type == CURTYPE_BTREE && !c.is_table);
    let seek_result = if (o.p5 & OPFLAG_USESEEKRESULT as u16) != 0 { c.seek_result } else { 0 };
    let bt_flags = (o.p5 & (OPFLAG_APPEND | OPFLAG_SAVEPOSITION | OPFLAG_PREFORMAT) as u16) as i32;
    let rc = with_cursor_incr(db, c, |cur, bt, incr| {
        btree_insert(cur, bt, &x, bt_flags, seek_result, incr)
    })
    .unwrap_or(SQLITE_ERROR);
    debug_assert!(!c.deferred_moveto);
    c.cache_status = CACHE_STALE;
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc as i32)
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 016: IdxDelete, DeferredSeek, IdxRowid, IdxLE e companhia, Destroy
// ---------------------------------------------------------------------------------------------

/// `OP_IdxDelete`: remove do índice P1 a entrada cuja chave são os P3 registros a partir de r[P2].
/// Com P5 e sem a entrada, é `SQLITE_CORRUPT_INDEX` (salvo com `writable_schema`).
fn op_idx_delete(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    debug_assert!(o.p3 > 0);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE);
    let Some(key_info) = c.p_key_info.clone() else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let mut r = unpacked_from_regs(&p.a_mem, o.p2, o.p3, &key_info, 0);
    let mut res: i32 = 0;
    let rc = with_cursor_incr(db, c, |cur, bt, incr| {
        let rc = btree_index_moveto(cur, bt, &mut r, &mut res);
        if rc != SQLITE_OK {
            return rc;
        }
        if res == 0 {
            btree_delete(cur, bt, BTREE_AUXDELETE as u8, incr)
        } else {
            SQLITE_OK
        }
    })
    .unwrap_or(SQLITE_ERROR);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if res != 0 && o.p5 != 0 && !writable_schema(db) {
        // sqlite3ReportError(SQLITE_CORRUPT_INDEX, __LINE__, "index corruption")
        log(
            SQLITE_CORRUPT_INDEX,
            b"%s at line %d of [%.10s]",
            &[
                PrintfArg::Text(Some(b"index corruption".to_vec())),
                PrintfArg::Int(99658),
                PrintfArg::Text(Some(b"c9c2ab54ba".to_vec())),
            ],
        );
        return st.abort(SQLITE_CORRUPT_INDEX);
    }
    debug_assert!(!c.deferred_moveto);
    c.cache_status = CACHE_STALE;
    c.seek_result = 0;
    Flow::Continue(pc)
}

/// `OP_DeferredSeek` e `OP_IdxRowid`: lê o rowid da entrada corrente do índice P1; o primeiro o
/// entrega como seek adiado ao cursor de tabela P3, o segundo o grava em r[P2].
fn op_idx_rowid(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let Some(mut c) = p.ap_csr.get_mut(o.p1 as usize).and_then(|s| s.take()) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let flow = idx_rowid_step(db, p, st, pc, &mut c);
    p.ap_csr[o.p1 as usize] = Some(c);
    match flow {
        Some(f) => f,
        None => Flow::Continue(pci),
    }
}

/// O miolo de [`op_idx_rowid`] sobre o cursor `c` já retirado de `ap_csr`. `None` é o `break`
/// do C.
fn idx_rowid_step(
    db: &mut Connection,
    p: &mut Vdbe,
    st: &mut ExecState,
    pc: usize,
    c: &mut VdbeCursor,
) -> Option<Flow> {
    let o = st.op;
    debug_assert!(c.e_cur_type == CURTYPE_BTREE || c.is_null_cursor());
    debug_assert!(!c.is_table || c.is_null_cursor());
    debug_assert!(!c.deferred_moveto);
    // O cursor pode ter sido perturbado desde o último posicionamento; restaurá-lo pode falhar
    // (falta de memória, erro de E/S).
    let rc = cursor_restore(c, db);
    if rc != SQLITE_OK {
        return Some(st.abort(rc));
    }
    if c.null_row {
        debug_assert!(o.opcode == OP_IDXROWID);
        mem_set_null(&mut p.a_mem[o.p2 as usize]);
        return None;
    }
    let mut rowid: i64 = 0;
    let rc = with_btree_cursor(db, c, |cur, bt| vdbe_idx_rowid(cur, bt, &mut rowid))
        .unwrap_or(SQLITE_ERROR);
    if rc != SQLITE_OK {
        return Some(st.abort(rc));
    }
    if o.opcode == OP_DEFERREDSEEK {
        // P4 pode ser um vetor de inteiros (P4_INTARRAY): o mapa de colunas da tabela para o
        // índice, que o OP_Column do cursor de tabela usa para ler direto do índice.
        let alt_map: Vec<u32> = match &cur_ops(p)[pc].p4 {
            P4::IntArray(ai) => ai.clone(),
            _ => Vec::new(),
        };
        let Some(tab_cur) = csr(&mut p.ap_csr, o.p3) else {
            return Some(st.abort(SQLITE_CORRUPT_BKPT));
        };
        debug_assert!(tab_cur.e_cur_type == CURTYPE_BTREE && tab_cur.is_table);
        debug_assert!(!tab_cur.is_ephemeral && !c.is_ephemeral);
        tab_cur.null_row = false;
        tab_cur.moveto_target = rowid;
        tab_cur.deferred_moveto = true;
        tab_cur.cache_status = CACHE_STALE;
        tab_cur.a_alt_map = alt_map;
        tab_cur.p_alt_cursor = Some(o.p1);
    } else {
        out2_prerelease(&mut p.a_mem, o.p2).u_i = rowid;
    }
    None
}

/// `OP_IdxLE`, `OP_IdxGT`, `OP_IdxLT` e `OP_IdxGE`: compara a chave de P3@P4 com a entrada
/// corrente do índice P1, ignorando o rowid, e salta para P2 conforme o opcode.
fn op_idx_compare(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let n_field = p4_int32(&cur_ops(p)[pc]);
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.is_ordered && c.e_cur_type == CURTYPE_BTREE && !c.deferred_moveto);
    let Some(key_info) = c.p_key_info.clone() else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let default_rc: i8 = if o.opcode < OP_IDXLT { -1 } else { 0 };
    let mut r = unpacked_from_regs(&p.a_mem, o.p3, n_field, &key_info, default_rc);
    // `sqlite3VdbeIdxKeyCompare` (o C o escreve inline neste opcode).
    let mut res: i32 = 0;
    let rc = with_btree_cursor(db, c, |cur, bt| vdbe_idx_key_compare(cur, bt, &mut r, &mut res))
        .unwrap_or(SQLITE_ERROR);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    debug_assert!((OP_IDXLE & 1) == (OP_IDXLT & 1) && (OP_IDXGE & 1) == (OP_IDXGT & 1));
    if (o.opcode & 1) == (OP_IDXLT & 1) {
        debug_assert!(o.opcode == OP_IDXLE || o.opcode == OP_IDXLT);
        res = res.wrapping_neg();
    } else {
        debug_assert!(o.opcode == OP_IDXGE || o.opcode == OP_IDXGT);
        res = res.wrapping_add(1);
    }
    if res > 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pc as i32)
    }
}

/// `OP_Destroy`: apaga a tabela ou índice de raiz P1 do banco P3; r[P2] recebe a raiz que o
/// auto-vacuum moveu (ou 0).
fn op_destroy(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    debug_assert!(!p.read_only && o.p1 > 1);
    out2_prerelease(&mut p.a_mem, o.p2).flags = MEM_NULL;
    if db.n_vdbe_read > db.n_v_destroy + 1 {
        p.error_action = OE_ABORT;
        return st.abort(SQLITE_LOCKED);
    }
    let i_db = o.p3 as usize;
    let mut i_moved: i32 = 0;
    let rc = match db.dbs.get_mut(i_db).and_then(|s| s.bt.as_mut()) {
        Some(bt) => btree_drop_table(bt, o.p1, &mut i_moved),
        None => SQLITE_ERROR,
    };
    let out = &mut p.a_mem[o.p2 as usize];
    out.flags = MEM_INT;
    out.u_i = i_moved as i64;
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if i_moved != 0 {
        root_page_moved(db, i_db, i_moved as u32, o.p1 as u32);
        // Todos os OP_Destroy ocorrem no mesmo btree.
        debug_assert!(st.reset_schema_on_fault == 0 || st.reset_schema_on_fault as usize == i_db + 1);
        st.reset_schema_on_fault = (i_db + 1) as u8;
    }
    Flow::Continue(pc)
}
