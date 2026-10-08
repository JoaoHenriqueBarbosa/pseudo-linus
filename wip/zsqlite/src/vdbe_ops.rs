//! `vdbe.c` (parte 2): trechos 005 a 010 do `vdbe.c` do SQLite 3.46.1, os opcodes de
//! `OP_CollSeq` até `OP_OpenAutoindex` / `OP_OpenEphemeral`: `CollSeq`, `BitAnd`, `BitOr`,
//! `ShiftLeft`, `ShiftRight`, `AddImm`, `MustBeInt`, `RealAffinity`, `Cast`, `Eq`, `Ne`, `Lt`,
//! `Le`, `Gt`, `Ge`, `ElseEq`, `Permutation`, `Compare`, `Jump`, `And`, `Or`, `IsTrue`, `Not`,
//! `BitNot`, `Once`, `If`, `IfNot`, `IsNull`, `IsType`, `ZeroOrNull`, `NotNull`, `IfNullRow`,
//! `Column`, `TypeCheck`, `Affinity`, `MakeRecord`, `Count`, `Savepoint`, `AutoCommit`,
//! `Transaction`, `ReadCookie`, `SetCookie`, `ReopenIdx`, `OpenRead`, `OpenWrite`, `OpenDup`,
//! `OpenAutoindex` e `OpenEphemeral`. O resto cai em [`crate::vdbe_ops2::exec_op2`].
//!
//! Convenções do laço (ver `vdbe.rs`): `pc` é o índice da instrução corrente no programa corrente
//! (`cur_ops`); `st.op` traz uma cópia dos operandos escalares; `Flow::Continue(pc)` segue para
//! `pc + 1`; `Flow::Jump(dest)` é o `goto jump_to_p2` do C.
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * `OP_Column` trabalha com o `VdbeCursor` retirado do vetor `Vdbe.ap_csr` durante o passo (o
//!   `pC` do C é um ponteiro; aqui ele é um `Box` emprestado por `take` e devolvido ao fim), e o
//!   `goto op_column_restart` vira [`ColStep::Restart`]. `aRow` é uma CÓPIA dos bytes da página
//!   (ver `vdbe_types.rs`).
//! * O conteúdo "bogus" de `OP_Column` (o `sqlite3CtypeMap`) é uma célula sem bytes: só o tamanho
//!   `n` e o tipo importam a quem pediu `typeof()` ou o comprimento de um blob.
//! * `OP_OpenDup` abre o `BtCursor` novo no `Btree` temporário do cursor original. Como um
//!   `Option<Box<Btree>>` só pode ter um dono, o cursor duplicado guarda em `p_alt_cursor` o
//!   número do cursor que possui o `Btree` (em cursores efêmeros o `pAltCursor` do C nunca é
//!   usado, só o `OP_Column` de cursores com `deferredMoveto` o lê).
//! * Os ramos `SQLITE_DEBUG`, `SQLITE_TEST`, `VdbeBranchTaken`, `REGISTER_TRACE`,
//!   `UPDATE_MAX_BLOBSIZE`, `memAboutToChange`, `SQLITE_ENABLE_NULL_TRIM` e
//!   `SQLITE_ENABLE_OFFSET_SQL_FUNC` não existem. `sqlite3VdbeIncrWriteCounter` só existe sob
//!   `SQLITE_DEBUG` (some).

use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::btree::{
    btree_clear_cursor, btree_close, btree_cursor_has_moved, btree_cursor_hint_flags, btree_open,
};
use crate::btree_cursor::{
    btree_begin_stmt, btree_begin_trans, btree_cursor, btree_max_record_size, btree_payload,
    btree_payload_fetch, btree_payload_size, btree_row_count_est, btree_savepoint,
    btree_trip_all_cursors, BtDb,
};
use crate::btree_write::{
    btree_clear_table, btree_count, btree_create_table, btree_get_meta, btree_update_meta,
};
use crate::build::{reset_all_schemas_of_connection, reset_one_schema};
use crate::connection::{Connection, Savepoint};
use crate::consts::*;
use crate::ctype::{a_eq_b, a_gt_b, a_lt_b};
use crate::mem::{
    apply_affinity, apply_numeric_affinity, mem_cast, mem_clear_and_resize, mem_compare,
    mem_expand_blob, mem_from_btree, mem_grow, mem_integerify, mem_realify, mem_set_int64,
    mem_set_null, mem_shallow_copy, mem_stringify, value_type, vdbe_boolean_value,
    vdbe_int_value, CollSeq, KeyInfo, Mem,
};
use crate::pager::cstr;
use crate::printf::PrintfArg;
use crate::record::{
    one_byte_serial_type_len, serial_get, serial_type_len, SMALL_TYPE_SIZES,
};
use crate::util::{get_varint32, put_varint, str_icmp, varint_len, STD_TYPE};
use crate::vdbe::{
    allocate_cursor, cur_ops, out2_prerelease, p4_z, prog_ops, vdbe_mem_type_name, ExecState,
    Flow, OpRegs,
};
use crate::vdbe_types::{Op, Vdbe, VdbeCursor, P4};
use crate::vdbeaux::vdbe_error;
use crate::vdbeaux2::{
    check_fk, finish_moveto, handle_moved_cursor, vdbe_halt, with_bt_db, with_btree_cursor,
};
use crate::vdbeaux3::expire_prepared_statements;

/// `OP_OpenEphemeral`: `static const int vfsFlags`.
const EPHEMERAL_VFS_FLAGS: i32 = SQLITE_OPEN_READWRITE
    | SQLITE_OPEN_CREATE
    | SQLITE_OPEN_EXCLUSIVE
    | SQLITE_OPEN_DELETEONCLOSE
    | SQLITE_OPEN_TRANSIENT_DB;

/// O `pOp->p4.pColl` de um opcode de comparação (`None` é a colação BINARY implícita).
fn coll_of(op: &Op) -> Option<&CollSeq> {
    match &op.p4 {
        P4::Coll(Some(c)) => Some(&**c),
        _ => None,
    }
}

/// O `pOp->p4.i` de um opcode `P4_INT32`.
pub(crate) fn p4_int32(op: &Op) -> i32 {
    match &op.p4 {
        P4::Int32(v) => *v,
        _ => 0,
    }
}

/// Executa um opcode da fatia `CollSeq` até `OpenEphemeral`; devolve o destino do fluxo.
pub fn exec_op(db: &mut Connection, p: &mut Vdbe, pc: usize, st: &mut ExecState) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    match o.opcode {
        // Opcode: CollSeq P1 * * P4: o `pColl` é consultado por quem chama funções embutidas;
        // se P1 não é zero, o registro P1 é zerado.
        OP_COLLSEQ => {
            if o.p1 != 0 {
                mem_set_int64(&mut p.a_mem[o.p1 as usize], 0);
            }
            Flow::Continue(pci)
        }

        // Opcode: BitAnd, BitOr, ShiftLeft, ShiftRight P1 P2 P3 * *
        OP_BITAND | OP_BITOR | OP_SHIFTLEFT | OP_SHIFTRIGHT => {
            op_bitwise(&mut p.a_mem, o);
            Flow::Continue(pci)
        }

        // Opcode: AddImm P1 P2 * * *: r[P1]=r[P1]+P2.
        OP_ADDIMM => {
            let m = &mut p.a_mem[o.p1 as usize];
            mem_integerify(m);
            m.u_i = m.u_i.wrapping_add(o.p2 as i64);
            Flow::Continue(pci)
        }

        // Opcode: MustBeInt P1 P2 * * *
        OP_MUSTBEINT => {
            let m = &mut p.a_mem[o.p1 as usize];
            if (m.flags & MEM_INT) == 0 {
                apply_affinity(m, SQLITE_AFF_NUMERIC, st.encoding);
                if (m.flags & MEM_INT) == 0 {
                    return if o.p2 == 0 { st.abort(SQLITE_MISMATCH) } else { Flow::Jump(o.p2) };
                }
            }
            m.set_type_flag(MEM_INT);
            Flow::Continue(pci)
        }

        // Opcode: RealAffinity P1 * * * *
        OP_REALAFFINITY => {
            let m = &mut p.a_mem[o.p1 as usize];
            if (m.flags & (MEM_INT | MEM_INTREAL)) != 0 {
                mem_realify(m);
            }
            Flow::Continue(pci)
        }

        // Opcode: Cast P1 P2 * * *
        OP_CAST => {
            let m = &mut p.a_mem[o.p1 as usize];
            let mut rc = SQLITE_OK;
            if (m.flags & MEM_ZERO) != 0 {
                rc = mem_expand_blob(m);
            }
            if rc == SQLITE_OK {
                rc = mem_cast(m, o.p2 as u8, st.encoding);
            }
            if rc != SQLITE_OK {
                st.abort(rc)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: Eq, Ne, Lt, Le, Gt, Ge P1 P2 P3 P4 P5
        OP_EQ | OP_NE | OP_LT | OP_LE | OP_GT | OP_GE => op_compare_jump(p, st, pci),

        // Opcode: ElseEq * P2 * * *
        OP_ELSEEQ => {
            if st.i_compare == 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: Permutation * * * P4 *: só vale para o OP_Compare seguinte, que a lê.
        OP_PERMUTATION => Flow::Continue(pci),

        // Opcode: Compare P1 P2 P3 P4 P5
        OP_COMPARE => op_compare(p, st, pci),

        // Opcode: Jump P1 P2 P3 * *
        OP_JUMP => {
            if st.i_compare < 0 {
                Flow::Jump(o.p1)
            } else if st.i_compare == 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Jump(o.p3)
            }
        }

        // Opcode: And, Or P1 P2 P3 * *
        OP_AND | OP_OR => {
            const AND_LOGIC: [u8; 9] = [0, 0, 0, 0, 1, 2, 0, 2, 2];
            const OR_LOGIC: [u8; 9] = [0, 1, 2, 1, 1, 1, 2, 1, 2];
            let v1 = vdbe_boolean_value(&p.a_mem[o.p1 as usize], 2) as usize;
            let v2 = vdbe_boolean_value(&p.a_mem[o.p2 as usize], 2) as usize;
            let r = if o.opcode == OP_AND { AND_LOGIC[v1 * 3 + v2] } else { OR_LOGIC[v1 * 3 + v2] };
            let out = &mut p.a_mem[o.p3 as usize];
            if r == 2 {
                out.set_type_flag(MEM_NULL);
            } else {
                out.u_i = r as i64;
                out.set_type_flag(MEM_INT);
            }
            Flow::Continue(pci)
        }

        // Opcode: IsTrue P1 P2 P3 P4 *: r[P2] = coalesce(r[P1]==TRUE,P3) ^ P4.
        OP_ISTRUE => {
            let p4i = p4_int32(&cur_ops(p)[pc]);
            let v = vdbe_boolean_value(&p.a_mem[o.p1 as usize], o.p3) ^ p4i;
            mem_set_int64(&mut p.a_mem[o.p2 as usize], v as i64);
            Flow::Continue(pci)
        }

        // Opcode: Not P1 P2 * * *: r[P2]= !r[P1].
        OP_NOT => {
            let v = if (p.a_mem[o.p1 as usize].flags & MEM_NULL) == 0 {
                Some((vdbe_boolean_value(&p.a_mem[o.p1 as usize], 0) == 0) as i64)
            } else {
                None
            };
            match v {
                Some(x) => mem_set_int64(&mut p.a_mem[o.p2 as usize], x),
                None => mem_set_null(&mut p.a_mem[o.p2 as usize]),
            }
            Flow::Continue(pci)
        }

        // Opcode: BitNot P1 P2 * * *: r[P2]= ~r[P1]. A ordem é a do C (anula a saída antes de
        // ler a entrada), que importa se os dois registros forem o mesmo.
        OP_BITNOT => {
            let (i1, i2) = (o.p1 as usize, o.p2 as usize);
            mem_set_null(&mut p.a_mem[i2]);
            if (p.a_mem[i1].flags & MEM_NULL) == 0 {
                let v = !vdbe_int_value(&p.a_mem[i1]);
                let out = &mut p.a_mem[i2];
                out.flags = MEM_INT;
                out.u_i = v;
            }
            Flow::Continue(pci)
        }

        // Opcode: Once P1 P2 * * *
        OP_ONCE => op_once(p, st, pci),

        // Opcode: If P1 P2 P3 * *
        OP_IF => {
            if vdbe_boolean_value(&p.a_mem[o.p1 as usize], o.p3) != 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IfNot P1 P2 P3 * *
        OP_IFNOT => {
            if vdbe_boolean_value(&p.a_mem[o.p1 as usize], (o.p3 == 0) as i32) == 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IsNull P1 P2 * * *
        OP_ISNULL => {
            if (p.a_mem[o.p1 as usize].flags & MEM_NULL) != 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IsType P1 P2 P3 P4 P5
        OP_ISTYPE => op_is_type(p, st, pc),

        // Opcode: ZeroOrNull P1 P2 P3 * *
        OP_ZEROORNULL => {
            if (p.a_mem[o.p1 as usize].flags & MEM_NULL) != 0
                || (p.a_mem[o.p3 as usize].flags & MEM_NULL) != 0
            {
                mem_set_null(&mut p.a_mem[o.p2 as usize]);
            } else {
                mem_set_int64(&mut p.a_mem[o.p2 as usize], 0);
            }
            Flow::Continue(pci)
        }

        // Opcode: NotNull P1 P2 * * *
        OP_NOTNULL => {
            if (p.a_mem[o.p1 as usize].flags & MEM_NULL) == 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IfNullRow P1 P2 P3 * *
        OP_IFNULLROW => {
            let null_row = p
                .ap_csr
                .get(o.p1 as usize)
                .and_then(|s| s.as_deref())
                .is_some_and(|c| c.null_row);
            if null_row {
                mem_set_null(&mut p.a_mem[o.p3 as usize]);
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: Column P1 P2 P3 P4 P5
        OP_COLUMN => op_column(db, p, st, pci),

        // Opcode: TypeCheck P1 P2 P3 P4 *
        OP_TYPECHECK => op_type_check(db, p, st, pc),

        // Opcode: Affinity P1 P2 * P4 *
        OP_AFFINITY => op_affinity(p, st, pc),

        // Opcode: MakeRecord P1 P2 P3 P4 *
        OP_MAKERECORD => op_make_record(db, p, st, pc),

        // Opcode: Count P1 P2 P3 * *
        OP_COUNT => op_count(db, p, st, pci),

        // Opcode: Savepoint P1 * * P4 *
        OP_SAVEPOINT => op_savepoint(db, p, st, pci),

        // Opcode: AutoCommit P1 P2 * * *
        OP_AUTOCOMMIT => op_auto_commit(db, p, st, pci),

        // Opcode: Transaction P1 P2 P3 P4 P5
        OP_TRANSACTION => op_transaction(db, p, st, pci),

        // Opcode: ReadCookie P1 P2 P3 * *
        OP_READCOOKIE => {
            let i_db = o.p1 as usize;
            let meta = match db.dbs.get(i_db).and_then(|s| s.bt.as_ref()) {
                Some(bt) => btree_get_meta(bt, o.p3) as i32,
                None => 0,
            };
            out2_prerelease(&mut p.a_mem, o.p2).u_i = meta as i64;
            Flow::Continue(pci)
        }

        // Opcode: SetCookie P1 P2 P3 * P5
        OP_SETCOOKIE => op_set_cookie(db, p, st, pci),

        // Opcode: ReopenIdx, OpenRead, OpenWrite P1 P2 P3 P4 P5
        OP_REOPENIDX | OP_OPENREAD | OP_OPENWRITE => op_open_cursor(db, p, st, pc),

        // Opcode: OpenDup P1 P2 * * *
        OP_OPENDUP => op_open_dup(db, p, st, pci),

        // Opcode: OpenAutoindex, OpenEphemeral P1 P2 P3 P4 P5
        OP_OPENAUTOINDEX | OP_OPENEPHEMERAL => op_open_ephemeral(db, p, st, pc),

        // O resto da tabela de opcodes.
        _ => crate::vdbe_ops2::exec_op2(db, p, pc, st),
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 005: BitAnd e companhia, comparações
// ---------------------------------------------------------------------------------------------

/// `OP_BitAnd`, `OP_BitOr`, `OP_ShiftLeft` e `OP_ShiftRight`: `r[P3] = r[P2] op r[P1]`.
fn op_bitwise(a: &mut [Mem], o: OpRegs) {
    let (i1, i2, i3) = (o.p1 as usize, o.p2 as usize, o.p3 as usize);
    if ((a[i1].flags | a[i2].flags) & MEM_NULL) != 0 {
        mem_set_null(&mut a[i3]);
        return;
    }
    let mut i_a = vdbe_int_value(&a[i2]);
    let mut i_b = vdbe_int_value(&a[i1]);
    let mut op = o.opcode;
    if op == OP_BITAND {
        i_a &= i_b;
    } else if op == OP_BITOR {
        i_a |= i_b;
    } else if i_b != 0 {
        debug_assert!(op == OP_SHIFTRIGHT || op == OP_SHIFTLEFT);
        // Deslocar por um valor negativo desloca no outro sentido.
        if i_b < 0 {
            debug_assert!(OP_SHIFTRIGHT == OP_SHIFTLEFT + 1);
            op = (2 * OP_SHIFTLEFT as i32 + 1 - op as i32) as u8;
            i_b = if i_b > -64 { -i_b } else { 64 };
        }
        if i_b >= 64 {
            i_a = if i_a >= 0 || op == OP_SHIFTLEFT { 0 } else { -1 };
        } else {
            let mut u_a = i_a as u64;
            if op == OP_SHIFTLEFT {
                u_a <<= i_b as u32;
            } else {
                u_a >>= i_b as u32;
                // Extensão de sinal no deslocamento à direita de um número negativo.
                if i_a < 0 {
                    u_a |= u64::MAX << (64 - i_b as u32);
                }
            }
            i_a = u_a as i64;
        }
    }
    a[i3].u_i = i_a;
    a[i3].set_type_flag(MEM_INT);
}

/// `OP_Eq`, `OP_Ne`, `OP_Lt`, `OP_Le`, `OP_Gt` e `OP_Ge`: compara r[P3] com r[P1] e salta para P2
/// se o resultado é o do operador. Guarda a comparação em `iCompare`.
fn op_compare_jump(p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let enc = st.encoding;
    let (i1, i3) = (o.p1 as usize, o.p3 as usize);
    let null_eq = (o.p5 & SQLITE_NULLEQ as u16) != 0;
    let a = &mut p.a_mem;
    let mut flags1 = a[i1].flags;
    let mut flags3 = a[i3].flags;
    if (flags1 & flags3 & MEM_INT) != 0 {
        // Caso comum: comparação de dois inteiros.
        let (v3, v1) = (a[i3].u_i, a[i1].u_i);
        if v3 > v1 {
            if a_gt_b(o.opcode) != 0 {
                return Flow::Jump(o.p2);
            }
            st.i_compare = 1;
        } else if v3 < v1 {
            if a_lt_b(o.opcode) != 0 {
                return Flow::Jump(o.p2);
            }
            st.i_compare = -1;
        } else {
            if a_eq_b(o.opcode) != 0 {
                return Flow::Jump(o.p2);
            }
            st.i_compare = 0;
        }
        return Flow::Continue(pc);
    }
    let res: i32;
    if ((flags1 | flags3) & MEM_NULL) != 0 {
        // Um ou os dois operandos são NULL.
        if null_eq {
            // SQLITE_NULLEQ só aparece em OP_Eq e OP_Ne: o salto depende de os dois serem NULL.
            if (flags1 & flags3 & MEM_NULL) != 0 && (flags3 & MEM_CLEARED) == 0 {
                res = 0;
            } else {
                res = if (flags3 & MEM_NULL) != 0 { -1 } else { 1 };
            }
        } else {
            // Sem NULLEQ o resultado é NULL; salta só se SQLITE_JUMPIFNULL.
            if (o.p5 & SQLITE_JUMPIFNULL as u16) != 0 {
                return Flow::Jump(o.p2);
            }
            st.i_compare = 1;
            return Flow::Continue(pc);
        }
    } else {
        // Nenhum é NULL e o caso rápido de inteiros não valeu: comparação geral.
        let affinity = (o.p5 & SQLITE_AFF_MASK as u16) as u8;
        if affinity >= SQLITE_AFF_NUMERIC {
            if ((flags1 | flags3) & MEM_STR) != 0 {
                if (flags1 & (MEM_INT | MEM_INTREAL | MEM_REAL | MEM_STR)) == MEM_STR {
                    apply_numeric_affinity(&mut a[i1], false);
                    flags3 = a[i3].flags;
                }
                if (flags3 & (MEM_INT | MEM_INTREAL | MEM_REAL | MEM_STR)) == MEM_STR {
                    apply_numeric_affinity(&mut a[i3], false);
                }
            }
        } else if affinity == SQLITE_AFF_TEXT && ((flags1 | flags3) & MEM_STR) != 0 {
            if (flags1 & MEM_STR) != 0 {
                a[i1].flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
            } else if (flags1 & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0 {
                mem_stringify(&mut a[i1], enc, true);
                flags1 = (a[i1].flags & !MEM_TYPEMASK) | (flags1 & MEM_TYPEMASK);
                if i1 == i3 {
                    flags3 = flags1 | MEM_STR;
                }
            }
            if (flags3 & MEM_STR) != 0 {
                a[i3].flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
            } else if (flags3 & (MEM_INT | MEM_REAL | MEM_INTREAL)) != 0 {
                mem_stringify(&mut a[i3], enc, true);
                flags3 = (a[i3].flags & !MEM_TYPEMASK) | (flags3 & MEM_TYPEMASK);
            }
        }
        let ops = prog_ops(&p.p_cur_prog, &p.a_op);
        res = mem_compare(&a[i3], &a[i1], coll_of(&ops[pc as usize]));
    }

    // `res` é negativo, zero ou positivo se r[P3] é menor, igual ou maior que r[P1]. Os seis
    // operadores são consecutivos (NE, EQ, GT, LE, LT, GE).
    let res2 = if res < 0 {
        a_lt_b(o.opcode)
    } else if res == 0 {
        a_eq_b(o.opcode)
    } else {
        a_gt_b(o.opcode)
    };
    st.i_compare = res;

    // Desfaz as mudanças que a afinidade fez nos registros de entrada.
    a[i3].flags = flags3;
    a[i1].flags = flags1;

    if res2 != 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pc)
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 006: Compare, Once, IsType
// ---------------------------------------------------------------------------------------------

/// `OP_Compare`: compara os vetores r[P1@P3] e r[P2@P3] pela `KeyInfo` de P4 e guarda o resultado
/// em `iCompare` para o `OP_Jump` seguinte.
fn op_compare(p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let pcu = pc as usize;
    let ops = prog_ops(&p.p_cur_prog, &p.a_op);
    let permute: Option<&[u32]> = if (o.p5 & OPFLAG_PERMUTE as u16) == 0 {
        None
    } else {
        match pcu.checked_sub(1).and_then(|k| ops.get(k)).map(|prev| &prev.p4) {
            Some(P4::IntArray(v)) => Some(v.get(1..).unwrap_or(&[])),
            _ => None,
        }
    };
    let P4::KeyInfo(ki) = &ops[pcu].p4 else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let n = o.p3.max(0) as usize;
    for i in 0..n {
        let idx = match permute {
            Some(pm) => pm.get(i).copied().unwrap_or(0) as usize,
            None => i,
        };
        let m1 = &p.a_mem[o.p1 as usize + idx];
        let m2 = &p.a_mem[o.p2 as usize + idx];
        let coll = ki.a_coll.get(i).and_then(|c| c.as_deref());
        let sort = ki.a_sort_flags.get(i).copied().unwrap_or(0);
        st.i_compare = mem_compare(m1, m2, coll);
        if st.i_compare != 0 {
            if (sort & KEYINFO_ORDER_BIGNULL) != 0 && ((m1.flags | m2.flags) & MEM_NULL) != 0 {
                st.i_compare = -st.i_compare;
            }
            if (sort & KEYINFO_ORDER_DESC) != 0 {
                st.i_compare = -st.i_compare;
            }
            break;
        }
    }
    Flow::Continue(pc)
}

/// `OP_Once`: cai na instrução seguinte só na primeira vez por execução do programa.
fn op_once(p: &mut Vdbe, st: &ExecState, pc: i32) -> Flow {
    let o = st.op;
    let i_addr = pc as usize;
    if let Some(frame) = p.p_frame.last_mut() {
        // Subprograma: os bits do frame decidem (o auto-ajuste do P1 não vale em recursão).
        let (byte, bit) = (i_addr / 8, 1u8 << (i_addr & 7));
        match frame.a_once.get_mut(byte) {
            Some(b) => {
                if (*b & bit) != 0 {
                    return Flow::Jump(o.p2);
                }
                *b |= bit;
            }
            None => return Flow::Continue(pc),
        }
    } else {
        let init_p1 = p.a_op.first().map_or(0, |op| op.p1);
        if init_p1 == o.p1 {
            return Flow::Jump(o.p2);
        }
        // `pOp->p1 = p->aOp[0].p1`: o programa se reescreve.
        if let Some(op) = p.a_op.get_mut(i_addr) {
            op.p1 = init_p1;
        }
    }
    Flow::Continue(pc)
}

/// `OP_IsType`: salta se o tipo da coluna P3 do cursor P1 (ou do registro P3 se P1 é -1) está na
/// máscara P5.
fn op_is_type(p: &Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    const A_MASK: [u16; 12] = [0x10, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x2, 0x01, 0x01, 0x10, 0x10];
    let o = st.op;
    let pci = pc as i32;
    let type_mask: u16;
    if o.p1 >= 0 {
        let Some(c) = p.ap_csr.get(o.p1 as usize).and_then(|s| s.as_deref()) else {
            return st.abort(SQLITE_CORRUPT_BKPT);
        };
        let col = o.p3.max(0) as usize;
        if col < c.n_hdr_parsed as usize {
            let serial_type = c.a_type.get(col).copied().unwrap_or(0);
            if serial_type >= 12 {
                type_mask = if (serial_type & 1) != 0 { 0x04 } else { 0x08 };
            } else {
                type_mask = A_MASK[serial_type as usize];
            }
        } else {
            let t = p4_int32(&cur_ops(p)[pc]);
            type_mask = 1u16 << ((t - 1).clamp(0, 15) as u32);
        }
    } else {
        let t = value_type(&p.a_mem[o.p3 as usize]);
        type_mask = 1u16 << ((t - 1).clamp(0, 15) as u32);
    }
    if (type_mask & o.p5) != 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pci)
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 007: OP_Column
// ---------------------------------------------------------------------------------------------

/// O resultado de um passo de `OP_Column`: o `goto op_column_restart` (com o número do cursor a
/// reexecutar, que pode ser o `pAltCursor`) ou o fim com o fluxo a seguir.
enum ColStep {
    Restart(usize),
    Done(Flow),
}

/// `OP_Column`: extrai a coluna P2 do registro em que o cursor P1 está e a grava em r[P3].
fn op_column(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let mut ci = o.p1 as usize;
    let mut p2 = o.p2 as u32;
    loop {
        let Some(mut c) = p.ap_csr.get_mut(ci).and_then(|s| s.take()) else {
            return st.abort(SQLITE_CORRUPT_BKPT);
        };
        let step = column_step(db, p, st, pc, ci, &mut c, &mut p2);
        p.ap_csr[ci] = Some(c);
        match step {
            ColStep::Restart(next) => ci = next,
            ColStep::Done(flow) => return flow,
        }
    }
}

/// `op_column_corrupt`: se `aOp[0].p3 > 0` o programa trata a corrupção em `p3`; senão
/// `SQLITE_CORRUPT`.
fn column_corrupt(p: &Vdbe, st: &mut ExecState) -> ColStep {
    let p3 = cur_ops(p).first().map_or(0, |op| op.p3);
    if p3 > 0 {
        ColStep::Done(Flow::Continue(p3 - 1))
    } else {
        ColStep::Done(st.abort(SQLITE_CORRUPT_BKPT))
    }
}

/// `sqlite3BtreeCursorHasMoved(pC->uc.pCursor)`; um cursor sem `BtCursor` (a pseudotabela, cujo
/// cursor do C é o `sqlite3BtreeFakeValidCursor`) nunca se moveu.
fn cursor_has_moved(db: &mut Connection, c: &mut VdbeCursor) -> bool {
    if c.e_cur_type != CURTYPE_BTREE {
        return false;
    }
    with_btree_cursor(db, c, |cur, _bt| btree_cursor_has_moved(cur)).unwrap_or(false)
}

/// Um passo de `OP_Column` sobre o cursor `c` (de número `ci`). `p2` é o número da coluna, que o
/// `pAltCursor` pode trocar.
fn column_step(
    db: &mut Connection,
    p: &mut Vdbe,
    st: &mut ExecState,
    pc: i32,
    ci: usize,
    c: &mut VdbeCursor,
    p2: &mut u32,
) -> ColStep {
    let o = st.op;
    let enc = st.encoding;
    let pcu = pc as usize;
    let dest_i = o.p3 as usize;
    // `goto op_column_read_header` com `zData = aRow`.
    let mut skip_to_read = false;

    if c.cache_status != p.cache_ctr {
        if c.null_row {
            if c.e_cur_type == CURTYPE_PSEUDO && c.seek_result > 0 {
                // Na pseudotabela, `seekResult` é o registro que guarda o registro.
                let reg = &p.a_mem[c.seek_result as usize];
                debug_assert!((reg.flags & MEM_BLOB) != 0);
                c.payload_size = reg.n as u32;
                c.sz_row = reg.n as u32;
                c.a_row = reg.bytes().to_vec();
            } else {
                mem_set_null(&mut p.a_mem[dest_i]);
                return ColStep::Done(Flow::Continue(pc));
            }
        } else {
            if c.deferred_moveto {
                let i_map = if c.a_alt_map.is_empty() {
                    0
                } else {
                    c.a_alt_map.get(1 + *p2 as usize).copied().unwrap_or(0)
                };
                if i_map > 0 {
                    return match c.p_alt_cursor {
                        Some(alt) => {
                            *p2 = i_map - 1;
                            ColStep::Restart(alt as usize)
                        }
                        None => ColStep::Done(st.abort(SQLITE_CORRUPT_BKPT)),
                    };
                }
                let rc = finish_moveto(c, db);
                if rc != SQLITE_OK {
                    return ColStep::Done(st.abort(rc));
                }
            } else if cursor_has_moved(db, c) {
                let rc = handle_moved_cursor(c, db);
                if rc != SQLITE_OK {
                    return ColStep::Done(st.abort(rc));
                }
                return ColStep::Restart(ci);
            }
            let fetched = with_btree_cursor(db, c, |cur, bt| {
                let ps = btree_payload_size(cur, bt);
                let row = btree_payload_fetch(cur, bt).to_vec();
                (ps, row)
            });
            match fetched {
                Some((ps, row)) => {
                    c.payload_size = ps;
                    c.sz_row = row.len() as u32;
                    c.a_row = row;
                }
                None => return ColStep::Done(st.abort(SQLITE_ERROR)),
            }
        }
        c.cache_status = p.cache_ctr;
        let b0 = c.a_row.first().copied().unwrap_or(0);
        if b0 < 0x80 {
            c.a_offset[0] = b0 as u32;
            c.i_hdr_offset = 1;
        } else {
            let (n, v) = get_varint32(&c.a_row);
            c.a_offset[0] = v;
            c.i_hdr_offset = n as u32;
        }
        c.n_hdr_parsed = 0;

        if c.sz_row < c.a_offset[0] {
            // `aRow` não cobre o cabeçalho inteiro: ele será lido do btree. Antes, confere que um
            // banco corrompido não deu um cabeçalho gigante (3 bytes por tipo, no máximo 32768
            // colunas, mais 3 do próprio tamanho).
            c.a_row = Vec::new();
            c.sz_row = 0;
            if c.a_offset[0] > 98307 || c.a_offset[0] > c.payload_size {
                return column_corrupt(p, st);
            }
        } else {
            // Otimização do C: pula os primeiros testes da próxima seção.
            skip_to_read = true;
        }
    } else if cursor_has_moved(db, c) {
        let rc = handle_moved_cursor(c, db);
        if rc != SQLITE_OK {
            return ColStep::Done(st.abort(rc));
        }
        return ColStep::Restart(ci);
    }

    // Garante que ao menos as p2+1 primeiras entradas do cabeçalho foram lidas e que há
    // informação válida em `aOffset[]` e `aType[]`.
    let col = *p2 as usize;
    if col >= c.a_type.len() {
        return column_corrupt(p, st);
    }
    if skip_to_read || (c.n_hdr_parsed as u32) <= *p2 {
        // Se sobra cabeçalho no registro, tenta ler os campos até o p2+1-ésimo.
        if skip_to_read || c.i_hdr_offset < c.a_offset[0] {
            // `zData` precisa cobrir o cabeçalho.
            let mut hdr_buf: Vec<u8> = Vec::new();
            if c.a_row.is_empty() && c.a_offset[0] != 0 {
                let amt = c.a_offset[0];
                let r = with_btree_cursor(db, c, |cur, bt| {
                    let n_avail = btree_payload_fetch(cur, bt).len();
                    if amt as usize <= n_avail {
                        Ok(btree_payload_fetch(cur, bt)[..amt as usize].to_vec())
                    } else {
                        // `sqlite3VdbeMemFromBtreeZeroOffset`: o cabeçalho passa da parte local.
                        let mut sm = Mem::default();
                        let mrs = btree_max_record_size(bt).clamp(0, u32::MAX as i64) as u32;
                        let rc = mem_from_btree(&mut sm, mrs, 0, amt, |buf| {
                            btree_payload(cur, bt, 0, amt, buf)
                        });
                        if rc == SQLITE_OK {
                            Ok(sm.bytes().to_vec())
                        } else {
                            Err(rc)
                        }
                    }
                });
                match r {
                    Some(Ok(v)) => hdr_buf = v,
                    Some(Err(rc)) => return ColStep::Done(st.abort(rc)),
                    None => return ColStep::Done(st.abort(SQLITE_ERROR)),
                }
            }
            let z_data: &[u8] = if c.a_row.is_empty() { &hdr_buf } else { &c.a_row };

            // Preenche `aType[i]` e `aOffset[i]` até o p2-ésimo campo (op_column_read_header).
            let mut i = c.n_hdr_parsed as usize;
            let mut offset64: u64 = c.a_offset[i] as u64;
            let mut z_hdr = c.i_hdr_offset as usize;
            let z_end_hdr = c.a_offset[0] as usize;
            loop {
                let b = z_data.get(z_hdr).copied().unwrap_or(0);
                if b < 0x80 {
                    c.a_type[i] = b as u32;
                    z_hdr += 1;
                    offset64 += one_byte_serial_type_len(b) as u64;
                } else {
                    let (n, t) = get_varint32(z_data.get(z_hdr..).unwrap_or(&[]));
                    z_hdr += n as usize;
                    c.a_type[i] = t;
                    offset64 += serial_type_len(t) as u64;
                }
                i += 1;
                c.a_offset[i] = (offset64 & 0xffff_ffff) as u32;
                if !((i as u32) <= *p2 && z_hdr < z_end_hdr) {
                    break;
                }
            }

            // O registro é corrupto se: (1) os bytes do cabeçalho passam do tamanho declarado,
            // (2) o cabeçalho inteiro foi usado mas não todos os dados, ou (3) o fim dos dados
            // passa do fim do registro.
            let payload = c.payload_size as u64;
            if (z_hdr >= z_end_hdr && (z_hdr > z_end_hdr || offset64 != payload))
                || offset64 > payload
            {
                if c.a_offset[0] == 0 {
                    i = 0;
                    z_hdr = z_end_hdr;
                } else {
                    return column_corrupt(p, st);
                }
            }
            c.n_hdr_parsed = i as u16;
            c.i_hdr_offset = z_hdr as u32;
        }

        // Se depois de tentar ler mais do cabeçalho `nHdrParsed` ainda não chegou a p2, o
        // registro tem menos de p2 colunas: vale o DEFAULT (P4_MEM) ou NULL.
        if (c.n_hdr_parsed as u32) <= *p2 {
            let ops = prog_ops(&p.p_cur_prog, &p.a_op);
            let dest = &mut p.a_mem[dest_i];
            match &ops[pcu].p4 {
                P4::Mem(m) => mem_shallow_copy(dest, m, MEM_STATIC),
                _ => mem_set_null(dest),
            }
            return ColStep::Done(Flow::Continue(pc));
        }
    }

    // Extrai o conteúdo da coluna p2+1. `aOffset[p2]`, `aOffset[p2+1]` e `aType[p2]` são válidos.
    debug_assert!((col as u32) < c.n_hdr_parsed as u32);
    let t = c.a_type[col];
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    if p.a_mem[dest_i].is_dynamic() {
        mem_set_null(&mut p.a_mem[dest_i]);
    }
    if c.sz_row >= c.a_offset[col + 1] {
        // O caso comum: o conteúdo cabe na página original, fora das páginas de overflow.
        let off = c.a_offset[col] as usize;
        let z_data = c.a_row.get(off..).unwrap_or(&[]);
        let dest = &mut p.a_mem[dest_i];
        if t < 12 {
            serial_get(z_data, t, dest);
        } else {
            // Texto ou blob precisa de valor persistente, não de um MEM_Ephem: atalho equivalente
            // a `sqlite3VdbeSerialGet()` seguido de `sqlite3VdbeDeephemeralize()`.
            let len = ((t - 12) / 2) as usize;
            dest.n = len as i32;
            dest.enc = enc;
            if (dest.sz_malloc as i64) < len as i64 + 2 {
                if len as i64 > limit as i64 {
                    return ColStep::Done(Flow::TooBig);
                }
                dest.flags = MEM_NULL;
                if mem_grow(dest, len as i32 + 2, false) != SQLITE_OK {
                    return ColStep::Done(Flow::NoMem);
                }
            } else if dest.z.len() < len + 2 {
                dest.z.resize(len + 2, 0);
            }
            let take = len.min(z_data.len());
            dest.z[..take].copy_from_slice(&z_data[..take]);
            dest.z[take..len].fill(0);
            dest.z[len] = 0;
            dest.z[len + 1] = 0;
            dest.flags = if (t & 1) != 0 { MEM_STR | MEM_TERM } else { MEM_BLOB };
        }
    } else {
        // Só acontece com conteúdo em páginas de overflow.
        p.a_mem[dest_i].enc = enc;
        let p5 = o.p5 & OPFLAG_BYTELENARG as u16;
        if (p5 != 0
            && (p5 == OPFLAG_TYPEOFARG as u16
                || (t >= 12 && ((t & 1) == 0 || p5 == OPFLAG_BYTELENARG as u16))))
            || serial_type_len(t) == 0
        {
            // O conteúdo é irrelevante para typeof(), para length(X) de blob e para conteúdo de
            // tamanho zero: o C passa o `sqlite3CtypeMap` como bytes. Aqui a célula só leva o tipo
            // e o tamanho (os bytes nunca são lidos por quem pediu isto).
            let dest = &mut p.a_mem[dest_i];
            if t < 12 {
                serial_get(&[0u8; 8], t, dest);
            } else {
                dest.z = Vec::new();
                dest.sz_malloc = 0;
                dest.n = ((t - 12) / 2) as i32;
                dest.flags = if (t & 1) != 0 { MEM_STR | MEM_EPHEM } else { MEM_BLOB | MEM_EPHEM };
            }
        } else {
            let i_offset = c.a_offset[col] as i64;
            let rc = crate::vdbe::vdbe_column_from_overflow(
                db,
                c,
                col as i32,
                t as i32,
                i_offset,
                p.cache_ctr,
                st.col_cache_ctr,
                &mut p.a_mem[dest_i],
            );
            if rc != SQLITE_OK {
                if rc == SQLITE_NOMEM {
                    return ColStep::Done(Flow::NoMem);
                }
                if rc == SQLITE_TOOBIG {
                    return ColStep::Done(Flow::TooBig);
                }
                return ColStep::Done(st.abort(rc));
            }
        }
    }
    ColStep::Done(Flow::Continue(pc))
}

// ---------------------------------------------------------------------------------------------
// chunk 008: TypeCheck, Affinity, MakeRecord
// ---------------------------------------------------------------------------------------------

/// `OP_TypeCheck`: aplica as afinidades da tabela STRICT de P4 aos registros r[P1@P2] e falha se
/// algum valor não cabe no tipo da coluna.
fn op_type_check(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let enc = st.encoding;
    let pci = pc as i32;
    let tab = match &cur_ops(p)[pc].p4 {
        P4::Table(t) | P4::TableRef(t) => Rc::clone(t),
        _ => return Flow::Continue(pci),
    };
    debug_assert!((tab.tab_flags & TF_STRICT) != 0);
    debug_assert!(tab.n_nv_col as i32 == o.p2);
    let mut i1 = o.p1 as usize;
    for i in 0..tab.n_col.max(0) as usize {
        let col = &tab.a_col[i];
        if (col.col_flags & COLFLAG_GENERATED) != 0 {
            if (col.col_flags & COLFLAG_VIRTUAL) != 0 {
                continue;
            }
            if o.p3 != 0 {
                i1 += 1;
                continue;
            }
        }
        let m = &mut p.a_mem[i1];
        apply_affinity(m, col.affinity, enc);
        if (m.flags & MEM_NULL) == 0 {
            let bad = match col.e_c_type {
                COLTYPE_BLOB => (m.flags & MEM_BLOB) == 0,
                COLTYPE_INTEGER | COLTYPE_INT => (m.flags & MEM_INT) == 0,
                COLTYPE_TEXT => (m.flags & MEM_STR) == 0,
                COLTYPE_REAL => {
                    debug_assert!((m.flags & MEM_INTREAL) == 0);
                    if (m.flags & MEM_INT) != 0 {
                        // Com afinidade REAL, um MEM_Int que cabe em 6 bytes vira MEM_IntReal
                        // para guardar o inteiro de alta resolução sabendo que o tipo é REAL.
                        if m.u_i <= 140737488355327 && m.u_i >= -140737488355328 {
                            m.flags |= MEM_INTREAL;
                            m.flags &= !MEM_INT;
                        } else {
                            m.u_r = m.u_i as f64;
                            m.flags |= MEM_REAL;
                            m.flags &= !MEM_INT;
                        }
                        false
                    } else {
                        (m.flags & (MEM_REAL | MEM_INTREAL)) == 0
                    }
                }
                // COLTYPE_ANY aceita qualquer coisa.
                _ => false,
            };
            if bad {
                let type_name = vdbe_mem_type_name(&p.a_mem[i1]).to_vec();
                let std_type = STD_TYPE
                    .get((col.e_c_type as usize).wrapping_sub(1))
                    .copied()
                    .unwrap_or("");
                vdbe_error(
                    p,
                    db,
                    b"cannot store %s value in %s column %s.%s",
                    &[
                        PrintfArg::Text(Some(type_name)),
                        PrintfArg::Text(Some(std_type.as_bytes().to_vec())),
                        PrintfArg::Text(Some(cstr(&tab.z_name).to_vec())),
                        PrintfArg::Text(Some(cstr(&col.z_cn_name).to_vec())),
                    ],
                );
                return st.abort(SQLITE_CONSTRAINT_DATATYPE);
            }
        }
        i1 += 1;
    }
    Flow::Continue(pci)
}

/// `OP_Affinity`: aplica ao registro r[P1+k] a afinidade do k-ésimo caractere do texto de P4.
fn op_affinity(p: &mut Vdbe, st: &ExecState, pc: usize) -> Flow {
    let o = st.op;
    let enc = st.encoding;
    let ops = prog_ops(&p.p_cur_prog, &p.a_op);
    let z_affinity: &[u8] = match &ops[pc].p4 {
        P4::Text(z) | P4::Blob(z) => cstr(z),
        _ => &[],
    };
    debug_assert!(!z_affinity.is_empty() && o.p2 > 0);
    let mut i = o.p1 as usize;
    for &aff in z_affinity {
        let m = &mut p.a_mem[i];
        apply_affinity(m, aff, enc);
        if aff == SQLITE_AFF_REAL && (m.flags & MEM_INT) != 0 {
            // Com afinidade REAL, um MEM_Int que cabe em 6 bytes vira MEM_IntReal.
            if m.u_i <= 140737488355327 && m.u_i >= -140737488355328 {
                m.flags |= MEM_INTREAL;
                m.flags &= !MEM_INT;
            } else {
                m.u_r = m.u_i as f64;
                m.flags |= MEM_REAL;
                m.flags &= !(MEM_INT | MEM_STR);
            }
        }
        i += 1;
    }
    Flow::Continue(pc as i32)
}

/// `OP_MakeRecord`: r[P3] = registro com os campos r[P1@P2] (formato do cabeçalho, tipos seriais,
/// dados).
fn op_make_record(db: &mut Connection, p: &mut Vdbe, st: &ExecState, pc: usize) -> Flow {
    let o = st.op;
    let min_fmt = p.min_write_file_format;
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    let out_i = o.p3 as usize;
    let ops = prog_ops(&p.p_cur_prog, &p.a_op);
    let z_aff: Option<&[u8]> = match &ops[pc].p4 {
        P4::Text(z) | P4::Blob(z) => Some(cstr(z)),
        _ => None,
    };
    // O registro de saída não pode ser um dos de entrada (o C afirma isso): sai do vetor durante
    // a montagem para que as entradas e a saída possam ser emprestadas juntas.
    let mut out = std::mem::take(&mut p.a_mem[out_i]);
    let r = make_record_into(&mut p.a_mem, &mut out, o, st.encoding, min_fmt, limit, z_aff);
    p.a_mem[out_i] = out;
    match r {
        None => Flow::Continue(pc as i32),
        Some(flow) => flow,
    }
}

/// O miolo de `OP_MakeRecord`. `None` é sucesso; `Some(flow)` é `too_big` ou `no_mem`.
fn make_record_into(
    a: &mut [Mem],
    out: &mut Mem,
    o: OpRegs,
    enc: u8,
    min_fmt: u8,
    limit: i32,
    z_aff: Option<&[u8]>,
) -> Option<Flow> {
    let i0 = o.p1 as usize;
    let n_field = o.p2.max(1) as usize;
    let last = i0 + n_field - 1;
    debug_assert!(o.p3 < o.p1 || o.p3 >= o.p1 + o.p2);

    // Aplica a afinidade pedida a todas as entradas.
    if let Some(z) = z_aff {
        for (k, &aff) in z.iter().enumerate() {
            let ri = i0 + k;
            if ri > last {
                break;
            }
            let rec = &mut a[ri];
            apply_affinity(rec, aff, enc);
            if aff == SQLITE_AFF_REAL && (rec.flags & MEM_INT) != 0 {
                rec.flags |= MEM_INTREAL;
                rec.flags &= !MEM_INT;
            }
        }
    }

    // Calcula o espaço necessário. Ao fim, `u_temp` de cada termo guarda o tipo serial:
    //   0 NULL; 1 a 4 inteiro de 1, 2, 3, 4 bytes; 5 de 6 bytes; 6 de 8 bytes; 7 real IEEE;
    //   8 e 9 as constantes 0 e 1; 10 e 11 reservados; N>=12 par é BLOB; N>=13 ímpar é texto.
    let mut n_data: u64 = 0;
    let mut n_hdr: i32 = 0;
    let mut n_zero: i64 = 0;
    let mut ri = last;
    loop {
        let rec_flags = a[ri].flags;
        if (rec_flags & MEM_NULL) != 0 {
            // NULL com MEM_Zero vem de xColumn de tabela virtual (valor "sem mudança" do UPDATE):
            // recebe o tipo serial interno 10 para chegar ao xUpdate.
            a[ri].u_temp = if (rec_flags & MEM_ZERO) != 0 { 10 } else { 0 };
            n_hdr += 1;
        } else if (rec_flags & (MEM_INT | MEM_INTREAL)) != 0 {
            // Decide se usa 1, 2, 4, 6 ou 8 bytes.
            let i = a[ri].u_i;
            let uu: u64 = if i < 0 { (!i) as u64 } else { i as u64 };
            n_hdr += 1;
            if uu <= 127 {
                if (i & 1) == i && min_fmt >= 4 {
                    a[ri].u_temp = 8 + uu as u32;
                } else {
                    n_data += 1;
                    a[ri].u_temp = 1;
                }
            } else if uu <= 32767 {
                n_data += 2;
                a[ri].u_temp = 2;
            } else if uu <= 8388607 {
                n_data += 3;
                a[ri].u_temp = 3;
            } else if uu <= 2147483647 {
                n_data += 4;
                a[ri].u_temp = 4;
            } else if uu <= 140737488355327 {
                n_data += 6;
                a[ri].u_temp = 5;
            } else {
                n_data += 8;
                if (rec_flags & MEM_INTREAL) != 0 {
                    // Um IntReal que ocupa 8 bytes como inteiro vira real de 8 bytes.
                    a[ri].u_r = i as f64;
                    a[ri].flags &= !MEM_INTREAL;
                    a[ri].flags |= MEM_REAL;
                    a[ri].u_temp = 7;
                } else {
                    a[ri].u_temp = 6;
                }
            }
        } else if (rec_flags & MEM_REAL) != 0 {
            n_hdr += 1;
            n_data += 8;
            a[ri].u_temp = 7;
        } else {
            debug_assert!((rec_flags & (MEM_STR | MEM_BLOB)) != 0);
            let mut len = a[ri].n as u32;
            let mut serial_type = len
                .wrapping_mul(2)
                .wrapping_add(12)
                .wrapping_add(((rec_flags & MEM_STR) != 0) as u32);
            if (rec_flags & MEM_ZERO) != 0 {
                let nz = a[ri].n_zero;
                serial_type = serial_type.wrapping_add((nz as u32).wrapping_mul(2));
                if n_data != 0 {
                    if mem_expand_blob(&mut a[ri]) != SQLITE_OK {
                        return Some(Flow::NoMem);
                    }
                    len = len.wrapping_add(nz as u32);
                } else {
                    n_zero += nz as i64;
                }
            }
            n_data += len as u64;
            n_hdr += varint_len(serial_type as u64);
            a[ri].u_temp = serial_type;
        }
        if ri == i0 {
            break;
        }
        ri -= 1;
    }

    // O cabeçalho começa com um varint com o tamanho total dele, o próprio varint incluído.
    if n_hdr <= 126 {
        n_hdr += 1;
    } else {
        let n_varint = varint_len(n_hdr as u64);
        n_hdr += n_varint;
        if n_varint < varint_len(n_hdr as u64) {
            n_hdr += 1;
        }
    }
    let n_byte: i64 = n_hdr as i64 + n_data as i64;

    // O registro de saída precisa de um buffer grande o bastante.
    if n_byte + n_zero <= out.sz_malloc as i64 {
        if out.z.len() < n_byte as usize {
            out.z.resize(n_byte as usize, 0);
        }
    } else {
        if n_byte + n_zero > limit as i64 {
            return Some(Flow::TooBig);
        }
        if mem_clear_and_resize(out, n_byte as i32) != SQLITE_OK {
            return Some(Flow::NoMem);
        }
    }
    out.n = n_byte as i32;
    out.flags = MEM_BLOB;
    if n_zero != 0 {
        out.n_zero = n_zero as i32;
        out.flags |= MEM_ZERO;
    }

    // Escreve o registro.
    let mut zh: usize;
    let mut zp: usize = n_hdr as usize;
    if n_hdr < 0x80 {
        out.z[0] = n_hdr as u8;
        zh = 1;
    } else {
        zh = put_varint(&mut out.z[..], n_hdr as u64) as usize;
    }
    let mut ri = i0;
    loop {
        let rec = &a[ri];
        let serial_type = rec.u_temp;
        if serial_type <= 7 {
            out.z[zh] = serial_type as u8;
            zh += 1;
            if serial_type != 0 {
                let mut v: u64 = if serial_type == 7 { rec.u_r.to_bits() } else { rec.u_i as u64 };
                let len = SMALL_TYPE_SIZES[serial_type as usize] as usize;
                debug_assert!((1..=8).contains(&len) && len != 5 && len != 7);
                for k in (0..len).rev() {
                    out.z[zp + k] = (v & 0xff) as u8;
                    v >>= 8;
                }
                zp += len;
            }
        } else if serial_type < 0x80 {
            out.z[zh] = serial_type as u8;
            zh += 1;
            if serial_type >= 14 && rec.n > 0 {
                let n = rec.n as usize;
                let src = &rec.z[..n.min(rec.z.len())];
                out.z[zp..zp + src.len()].copy_from_slice(src);
                zp += n;
            }
        } else {
            zh += put_varint(&mut out.z[zh..], serial_type as u64) as usize;
            if rec.n != 0 {
                let n = rec.n as usize;
                let src = &rec.z[..n.min(rec.z.len())];
                out.z[zp..zp + src.len()].copy_from_slice(src);
                zp += n;
            }
        }
        if ri == last {
            break;
        }
        ri += 1;
    }
    debug_assert!(n_hdr as usize == zh);
    debug_assert!(n_byte as usize == zp);
    None
}

// ---------------------------------------------------------------------------------------------
// chunk 009: Count, Savepoint, AutoCommit, Transaction
// ---------------------------------------------------------------------------------------------

/// `OP_Count`: r[P2] = número de entradas da tabela ou do índice do cursor P1 (exato, ou
/// estimado se P3 não é zero).
fn op_count(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let idx = o.p1 as usize;
    let Some(mut c) = p.ap_csr.get_mut(idx).and_then(|s| s.take()) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_BTREE);
    let interrupted = db.interrupted.clone();
    let mut n_entry: i64 = 0;
    let mut rc = SQLITE_OK;
    if o.p3 != 0 {
        n_entry = with_btree_cursor(db, &mut c, |cur, bt| btree_row_count_est(cur, bt)).unwrap_or(0);
    } else {
        let r = with_btree_cursor(db, &mut c, |cur, bt| {
            let mut n: i64 = 0;
            let rc = btree_count(cur, bt, &mut || interrupted.load(Ordering::Relaxed), &mut n);
            (rc, n)
        });
        match r {
            Some((r_rc, n)) => {
                rc = r_rc;
                n_entry = n;
            }
            None => rc = SQLITE_ERROR,
        }
    }
    p.ap_csr[idx] = Some(c);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    out2_prerelease(&mut p.a_mem, o.p2).u_i = n_entry;
    // goto check_for_interrupt
    Flow::JumpCheck(pc + 1)
}

/// `OP_Savepoint`: abre (P1=0), libera (P1=1) ou desfaz (P1=2) o savepoint de nome P4.
fn op_savepoint(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let p1 = o.p1;
    let z_name: Vec<u8> = p4_z(&cur_ops(p)[pc as usize]).map(<[u8]>::to_vec).unwrap_or_default();
    let mut rc = SQLITE_OK;

    if p1 == SAVEPOINT_BEGIN {
        if db.n_vdbe_write > 0 {
            // Não se cria savepoint com comandos de escrita ativos (blobs incrementais abertos).
            vdbe_error(p, db, b"cannot open savepoint - SQL statements in progress", &[]);
            rc = SQLITE_BUSY;
        } else {
            // Vale mesmo que este savepoint seja o da transação (sem xSavepoint a disparar): se
            // ele abre a transação, `aVTrans` está vazio.
            debug_assert!(db.auto_commit == 0 || db.a_v_trans.is_empty());
            let n_stmt_total = db.n_statement + db.n_savepoint;
            rc = crate::vtab::vtab_savepoint(db, SAVEPOINT_BEGIN, n_stmt_total);
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
            // Sem transação aberta, este é o "savepoint de transação" especial.
            if db.auto_commit != 0 {
                db.auto_commit = 0;
                db.is_transaction_savepoint = 1;
            } else {
                db.n_savepoint += 1;
            }
            db.p_savepoint.push(Savepoint {
                z_name,
                n_deferred_cons: db.n_deferred_cons,
                n_deferred_imm_cons: db.n_deferred_imm_cons,
            });
        }
    } else {
        debug_assert!(p1 == SAVEPOINT_RELEASE || p1 == SAVEPOINT_ROLLBACK);
        // Acha o savepoint pelo nome, do mais novo (o topo da pilha) para o mais velho.
        let n_sp = db.p_savepoint.len();
        let found = (0..n_sp).rev().find(|&k| str_icmp(&db.p_savepoint[k].z_name, &z_name) == 0);
        match found {
            None => {
                vdbe_error(p, db, b"no such savepoint: %s", &[PrintfArg::Text(Some(z_name))]);
                rc = SQLITE_ERROR;
            }
            Some(k) => {
                let mut i_savepoint = (n_sp - 1 - k) as i32;
                if db.n_vdbe_write > 0 && p1 == SAVEPOINT_RELEASE {
                    // Não se solta (confirma) savepoint com comandos de escrita ativos.
                    vdbe_error(
                        p,
                        db,
                        b"cannot release savepoint - SQL statements in progress",
                        &[],
                    );
                    rc = SQLITE_BUSY;
                } else {
                    // Savepoint de transação: um RELEASE dele confirma a transação.
                    let is_transaction = k == 0 && db.is_transaction_savepoint != 0;
                    if is_transaction && p1 == SAVEPOINT_RELEASE {
                        rc = check_fk(p, db, true);
                        if rc != SQLITE_OK {
                            return Flow::Return(rc);
                        }
                        db.auto_commit = 1;
                        if vdbe_halt(p, db) == SQLITE_BUSY {
                            p.pc = pc;
                            db.auto_commit = 0;
                            p.rc = SQLITE_BUSY;
                            return Flow::Return(SQLITE_BUSY);
                        }
                        rc = p.rc;
                        if rc != SQLITE_OK {
                            db.auto_commit = 0;
                        } else {
                            db.is_transaction_savepoint = 0;
                        }
                    } else {
                        let is_schema_change: bool;
                        i_savepoint = db.n_savepoint - i_savepoint - 1;
                        if p1 == SAVEPOINT_ROLLBACK {
                            is_schema_change = (db.m_db_flags & DBFLAG_SCHEMA_CHANGE) != 0;
                            for ii in 0..db.dbs.len() {
                                if let Some(bt) = db.dbs[ii].bt.as_mut() {
                                    rc = btree_trip_all_cursors(
                                        bt,
                                        SQLITE_ABORT_ROLLBACK,
                                        !is_schema_change,
                                    );
                                    if rc != SQLITE_OK {
                                        return st.abort(rc);
                                    }
                                }
                            }
                        } else {
                            debug_assert!(p1 == SAVEPOINT_RELEASE);
                            is_schema_change = false;
                        }
                        for ii in 0..db.dbs.len() {
                            if let Some(bt) = db.dbs[ii].bt.as_mut() {
                                rc = btree_savepoint(bt, p1, i_savepoint);
                                if rc != SQLITE_OK {
                                    return st.abort(rc);
                                }
                            }
                        }
                        if is_schema_change {
                            expire_prepared_statements(db, 0);
                            reset_all_schemas_of_connection(db);
                            db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
                        }
                    }
                    if rc != SQLITE_OK {
                        return st.abort(rc);
                    }

                    // Em RELEASE ou ROLLBACK, destrói os savepoints aninhados neste.
                    while db.p_savepoint.len() > k + 1 {
                        db.p_savepoint.pop();
                        db.n_savepoint -= 1;
                    }

                    // RELEASE destrói também este; ROLLBACK TO restaura o número de violações
                    // adiadas que valia quando ele foi criado.
                    if p1 == SAVEPOINT_RELEASE {
                        db.p_savepoint.pop();
                        if !is_transaction {
                            db.n_savepoint -= 1;
                        }
                    } else {
                        debug_assert!(p1 == SAVEPOINT_ROLLBACK);
                        db.n_deferred_cons = db.p_savepoint[k].n_deferred_cons;
                        db.n_deferred_imm_cons = db.p_savepoint[k].n_deferred_imm_cons;
                    }

                    if !is_transaction || p1 == SAVEPOINT_ROLLBACK {
                        rc = crate::vtab::vtab_savepoint(db, p1, i_savepoint);
                        if rc != SQLITE_OK {
                            return st.abort(rc);
                        }
                    }
                }
            }
        }
    }
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if p.e_vdbe_state == VDBE_HALT_STATE {
        return Flow::Return(SQLITE_DONE);
    }
    Flow::Continue(pc)
}

/// `OP_AutoCommit`: liga ou desliga o auto-commit (P1) e, se P2, desfaz as transações ativas.
/// A instrução faz a VM parar.
fn op_auto_commit(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let desired_auto_commit = o.p1;
    let i_rollback = o.p2;
    debug_assert!(desired_auto_commit == 1 || desired_auto_commit == 0);
    debug_assert!(desired_auto_commit == 1 || i_rollback == 0);
    debug_assert!(db.n_vdbe_active > 0);

    if desired_auto_commit != db.auto_commit as i32 {
        if i_rollback != 0 {
            debug_assert!(desired_auto_commit == 1);
            crate::main::rollback_all(db, SQLITE_ABORT_ROLLBACK);
            db.auto_commit = 1;
        } else if desired_auto_commit != 0 && db.n_vdbe_write > 0 {
            // Um COMMIT com outras VMs escrevendo falha: elas precisam terminar antes.
            vdbe_error(
                p,
                db,
                b"cannot commit transaction - SQL statements in progress",
                &[],
            );
            return st.abort(SQLITE_BUSY);
        } else {
            let rc = check_fk(p, db, true);
            if rc != SQLITE_OK {
                return Flow::Return(rc);
            }
            db.auto_commit = desired_auto_commit as u8;
        }
        if vdbe_halt(p, db) == SQLITE_BUSY {
            p.pc = pc;
            db.auto_commit = (1 - desired_auto_commit) as u8;
            p.rc = SQLITE_BUSY;
            return Flow::Return(SQLITE_BUSY);
        }
        crate::main::close_savepoints(db);
        let rc = if p.rc == SQLITE_OK { SQLITE_DONE } else { SQLITE_ERROR };
        Flow::Return(rc)
    } else {
        let msg: &[u8] = if desired_auto_commit == 0 {
            b"cannot start a transaction within a transaction"
        } else if i_rollback != 0 {
            b"cannot rollback - no transaction is active"
        } else {
            b"cannot commit - no transaction is active"
        };
        vdbe_error(p, db, msg, &[]);
        st.abort(SQLITE_ERROR)
    }
}

/// `OP_Transaction`: abre uma transação no banco P1 (de escrita se P2) e, com P5, confere o
/// cookie do esquema (P3) e a geração (P4).
fn op_transaction(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    debug_assert!(p.read_only == false || o.p2 == 0);
    if o.p2 != 0 && (db.flags & (SQLITE_QUERY_ONLY | SQLITE_CORRUPT_RD_ONLY)) != 0 {
        // Escrita proibida por "PRAGMA query_only=TRUE" ou por um SQLITE_CORRUPT anterior na
        // transação corrente.
        let rc = if (db.flags & SQLITE_QUERY_ONLY) != 0 { SQLITE_READONLY } else { SQLITE_CORRUPT };
        return st.abort(rc);
    }
    let i_db = o.p1 as usize;
    let mut i_meta: i32 = 0;
    let mut rc = SQLITE_OK;

    if db.dbs.get(i_db).is_some_and(|s| s.bt.is_some()) {
        rc = with_bt_db(db, i_db, |bt, bdb| btree_begin_trans(bt, o.p2, Some(&mut i_meta), bdb))
            .unwrap_or(SQLITE_ERROR);
        if rc != SQLITE_OK {
            if (rc & 0xff) == SQLITE_BUSY {
                p.pc = pc;
                p.rc = rc;
                return Flow::Return(rc);
            }
            return st.abort(rc);
        }

        if p.uses_stmt_journal && o.p2 != 0 && (db.auto_commit == 0 || db.n_vdbe_read > 1) {
            if p.i_statement == 0 {
                debug_assert!(db.n_statement >= 0 && db.n_savepoint >= 0);
                db.n_statement += 1;
                p.i_statement = db.n_savepoint + db.n_statement;
            }
            rc = crate::vtab::vtab_savepoint(db, SAVEPOINT_BEGIN, p.i_statement - 1);
            if rc == SQLITE_OK {
                rc = match db.dbs[i_db].bt.as_mut() {
                    Some(bt) => btree_begin_stmt(bt, p.i_statement),
                    None => SQLITE_ERROR,
                };
            }
            // Guarda o contador de restrições adiadas da conexão: se o statement for desfeito,
            // o contador também volta.
            p.n_stmt_def_cons = db.n_deferred_cons;
            p.n_stmt_def_imm_cons = db.n_deferred_imm_cons;
        }
    }
    if rc == SQLITE_OK
        && o.p5 != 0
        && (i_meta != o.p3
            || db.dbs.get(i_db).map_or(0, |s| s.schema.i_generation) != p4_int32(&cur_ops(p)[pc as usize]))
    {
        // A versão do esquema é conferida a cada comando, para saber se mudou desde o prepare.
        p.z_err_msg = Some(b"database schema has changed".to_vec());
        // Se o cookie do arquivo é o da representação em memória, não relê o esquema (com tabelas
        // virtuais isso é necessário, não só otimização).
        if db.dbs.get(i_db).map_or(0, |s| s.schema.schema_cookie) != i_meta {
            reset_one_schema(db, o.p1);
        }
        p.expired = 1;
        rc = SQLITE_SCHEMA;
        // `sqlite3_changes()` não deve mudar em `vdbe_halt`; um novo prepare liga de novo.
        p.change_cnt_on = false;
    }
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    Flow::Continue(pc)
}

// ---------------------------------------------------------------------------------------------
// chunk 010: SetCookie, OpenRead e companhia
// ---------------------------------------------------------------------------------------------

/// `OP_SetCookie`: grava P3 no cookie P2 do banco P1.
fn op_set_cookie(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let i_db = o.p1 as usize;
    debug_assert!(!p.read_only);
    let rc = match db.dbs.get_mut(i_db).and_then(|s| s.bt.as_mut()) {
        Some(bt) => btree_update_meta(bt, o.p2, o.p3 as u32),
        None => SQLITE_ERROR,
    };
    if o.p2 as u32 == BTREE_SCHEMA_VERSION {
        // Quando o cookie muda, o novo valor é guardado na memória.
        db.dbs[i_db].schema.schema_cookie = (o.p3 as u32).wrapping_sub(o.p5 as u32) as i32;
        db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
        crate::fkey::fk_clear_trigger_cache(db, o.p1);
    } else if o.p2 as u32 == BTREE_FILE_FORMAT {
        // Registra a mudança do formato do arquivo.
        db.dbs[i_db].schema.file_format = o.p3 as u8;
    }
    if o.p1 == 1 {
        // Mudar o esquema do banco TEMP invalida todos os comandos preparados (ticket #1644).
        expire_prepared_statements(db, 0);
        p.expired = 0;
    }
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc)
    }
}

/// `OP_ReopenIdx`, `OP_OpenRead` e `OP_OpenWrite`: abre o cursor P1 na árvore de raiz P2 do banco
/// P3.
fn op_open_cursor(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let hint_flags = (o.p5 & (OPFLAG_BULKCSR as u16 | OPFLAG_SEEKEQ as u16)) as u32;

    if o.opcode == OP_REOPENIDX {
        if let Some(c) = p.ap_csr.get_mut(o.p1 as usize).and_then(|s| s.as_deref_mut()) {
            if c.pgno_root == o.p2 as u32 {
                // Já aberto na mesma árvore: só limpa o cursor e refaz as dicas.
                debug_assert!(c.e_cur_type == CURTYPE_BTREE);
                with_btree_cursor(db, c, |cur, _bt| {
                    btree_clear_cursor(cur);
                    btree_cursor_hint_flags(cur, hint_flags);
                });
                return Flow::Continue(pci);
            }
        }
        // Não aberto, ou aberto em outro índice: cai no OP_OpenRead para reabrir.
    }

    if p.expired == 1 {
        return st.abort(SQLITE_ABORT_ROLLBACK);
    }
    let i_db = o.p3 as usize;
    let (key_info, n_field): (Option<Rc<KeyInfo>>, i32) = match &cur_ops(p)[pc].p4 {
        P4::KeyInfo(ki) => (Some(Rc::clone(ki)), ki.n_all_field as i32),
        P4::Int32(n) => (None, *n),
        _ => (None, 0),
    };
    if db.dbs.get(i_db).and_then(|s| s.bt.as_ref()).is_none() {
        return st.abort(SQLITE_CORRUPT_BKPT);
    }
    let wr_flag: u32 = if o.opcode == OP_OPENWRITE {
        debug_assert!(OPFLAG_FORDELETE as u32 == BTREE_FORDELETE);
        let file_format = db.dbs[i_db].schema.file_format;
        if file_format < p.min_write_file_format {
            p.min_write_file_format = file_format;
        }
        BTREE_WRCSR | (o.p5 as u32 & OPFLAG_FORDELETE as u32)
    } else {
        0
    };
    let mut p2 = o.p2 as u32;
    if (o.p5 & OPFLAG_P2ISREG as u16) != 0 {
        // O P2 vem de um OP_CreateBtree anterior, que o deixa em 2 ou mais.
        let m = &mut p.a_mem[p2 as usize];
        mem_integerify(m);
        p2 = (m.u_i as i32) as u32;
        debug_assert!(p2 >= 2);
    }
    let is_table = key_info.is_none();
    let Some(cx) = allocate_cursor(db, p, o.p1, n_field, CURTYPE_BTREE) else {
        return Flow::NoMem;
    };
    cx.i_db = i_db as i8;
    cx.null_row = true;
    cx.is_ordered = true;
    cx.pgno_root = p2;
    let mut rc = SQLITE_OK;
    match db.dbs.get_mut(i_db).and_then(|s| s.bt.as_mut()) {
        Some(bt) => match btree_cursor(bt, p2, wr_flag, key_info.clone()) {
            Ok(id) => cx.p_cursor = Some(id),
            Err(e) => rc = e,
        },
        None => rc = SQLITE_ERROR,
    }
    cx.p_key_info = key_info;
    // O VdbeCursor.isTable: versões antigas conferiam as flags da página raiz aqui, mas a
    // conferência foi para a camada do btree.
    cx.is_table = is_table;
    if cx.p_cursor.is_some() {
        with_btree_cursor(db, cx, |cur, _bt| btree_cursor_hint_flags(cur, hint_flags));
    }
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pci)
    }
}

/// `OP_OpenDup`: abre o cursor P1 na mesma tabela efêmera do cursor P2.
fn op_open_dup(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let Some(orig) = p.ap_csr.get_mut(o.p2 as usize).and_then(|s| s.as_deref_mut()) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(orig.is_ephemeral);
    let n_field = orig.n_field as i32;
    let key_info = orig.p_key_info.clone();
    let is_table = orig.is_table;
    let pgno_root = orig.pgno_root;
    let is_ordered = orig.is_ordered;
    // O dono do `Btree` temporário: o próprio original, ou quem o original já duplica.
    let owner = orig.p_alt_cursor.unwrap_or(o.p2);
    orig.no_reuse = true;

    let Some(cx) = allocate_cursor(db, p, o.p1, n_field, CURTYPE_BTREE) else {
        return Flow::NoMem;
    };
    cx.null_row = true;
    cx.is_ephemeral = true;
    cx.p_key_info = key_info.clone();
    cx.is_table = is_table;
    cx.pgno_root = pgno_root;
    cx.is_ordered = is_ordered;
    cx.p_alt_cursor = Some(owner);
    cx.no_reuse = true;

    // `sqlite3BtreeCursor` só falha para o primeiro cursor aberto num banco; já existe um.
    let opened = match p
        .ap_csr
        .get_mut(owner as usize)
        .and_then(|s| s.as_deref_mut())
        .and_then(|c| c.p_btx.as_mut())
    {
        Some(btx) => btree_cursor(btx, pgno_root, BTREE_WRCSR, key_info),
        None => Err(SQLITE_ERROR),
    };
    match opened {
        Ok(id) => {
            if let Some(c) = p.ap_csr.get_mut(o.p1 as usize).and_then(|s| s.as_deref_mut()) {
                c.p_cursor = Some(id);
            }
            Flow::Continue(pc)
        }
        Err(rc) => st.abort(rc),
    }
}

/// `OP_OpenAutoindex` e `OP_OpenEphemeral`: abre o cursor P1 numa tabela transitória (índice se
/// P4 é uma `KeyInfo`) ou esvazia a que já está aberta.
fn op_open_ephemeral(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    let idx = o.p1 as usize;
    if o.p3 > 0 {
        // Faz de r[P3] um valor que serve de dado de tamanho zero para o OP_Insert.
        debug_assert!(o.p2 == 0);
        debug_assert!((p.a_mem[o.p3 as usize].flags & MEM_NULL) != 0);
        let m = &mut p.a_mem[o.p3 as usize];
        m.n = 0;
        m.z = Vec::new();
        m.sz_malloc = 0;
    }
    let key_info: Option<Rc<KeyInfo>> = match &cur_ops(p)[pc].p4 {
        P4::KeyInfo(ki) => Some(Rc::clone(ki)),
        _ => None,
    };
    let reusable = p
        .ap_csr
        .get(idx)
        .and_then(|s| s.as_deref())
        .is_some_and(|c| !c.no_reuse && o.p2 <= c.n_field as i32);
    let rc;
    if reusable {
        // Já aberto e sem duplicatas do OP_OpenDup: apaga o conteúdo em vez de criar outra.
        match p.ap_csr.get_mut(idx).and_then(|s| s.as_deref_mut()) {
            Some(c) => {
                debug_assert!(c.is_ephemeral);
                c.seq_count = 0;
                c.cache_status = CACHE_STALE;
                let root = c.pgno_root as i32;
                rc = match c.p_btx.as_mut() {
                    Some(btx) => btree_clear_table(btx, root, None),
                    None => SQLITE_ERROR,
                };
            }
            None => rc = SQLITE_ERROR,
        }
    } else {
        let Some(cx) = allocate_cursor(db, p, o.p1, o.p2, CURTYPE_BTREE) else {
            return Flow::NoMem;
        };
        cx.is_ephemeral = true;
        rc = open_ephemeral_btree(db, cx, o.p5, key_info);
    }
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if let Some(c) = p.ap_csr.get_mut(idx).and_then(|s| s.as_deref_mut()) {
        c.null_row = true;
    }
    Flow::Continue(pci)
}

/// Abre o `Btree` temporário de um cursor efêmero (`sqlite3BtreeOpen` mais a transação e a
/// árvore) e liga o `BtCursor`. Em erro fecha o `Btree`.
fn open_ephemeral_btree(
    db: &mut Connection,
    cx: &mut VdbeCursor,
    p5: u16,
    key_info: Option<Rc<KeyInfo>>,
) -> i32 {
    let Some(vfs) = db.p_vfs.clone() else {
        return SQLITE_ERROR;
    };
    // `sqlite3TempInMemory(db)`: o `Btree` nasce em memória.
    let mut flags = BTREE_OMIT_JOURNAL | BTREE_SINGLE | p5 as u32;
    if db.temp_store == 2 {
        flags |= BTREE_MEMORY;
    }
    match btree_open(vfs, None, flags as i32, EPHEMERAL_VFS_FLAGS) {
        Ok(b) => cx.p_btx = Some(Box::new(b)),
        Err(rc) => return rc,
    }
    let mut bdb = BtDb {
        n_savepoint: db.n_savepoint,
        n_vdbe_read: db.n_vdbe_read,
        temp_in_memory: db.temp_store == 2,
        busy: None,
        autovac_pages: None,
    };
    let mut rc = SQLITE_OK;
    if let Some(btx) = cx.p_btx.as_mut() {
        rc = btree_begin_trans(btx, 1, None, &mut bdb);
        if rc == SQLITE_OK {
            // Um índice transitório nasce de `sqlite3BtreeCreateTable()` com BTREE_BLOBKEY; uma
            // tabela transitória usa a tabela BLOB_INTKEY de raiz 1 que já existe.
            if let Some(ki) = key_info {
                cx.p_key_info = Some(Rc::clone(&ki));
                rc = btree_create_table(btx, (BTREE_BLOBKEY | p5 as u32) as i32, &mut cx.pgno_root);
                if rc == SQLITE_OK {
                    debug_assert!(cx.pgno_root == SCHEMA_ROOT + 1);
                    match btree_cursor(btx, cx.pgno_root, BTREE_WRCSR, Some(ki)) {
                        Ok(id) => cx.p_cursor = Some(id),
                        Err(e) => rc = e,
                    }
                }
                cx.is_table = false;
            } else {
                cx.pgno_root = SCHEMA_ROOT;
                match btree_cursor(btx, SCHEMA_ROOT, BTREE_WRCSR, None) {
                    Ok(id) => cx.p_cursor = Some(id),
                    Err(e) => rc = e,
                }
                cx.is_table = true;
            }
        }
        cx.is_ordered = p5 as u32 != BTREE_UNORDERED;
    }
    if rc != SQLITE_OK {
        if let Some(b) = cx.p_btx.take() {
            btree_close(*b, &BtDb::default(), None);
        }
    }
    rc
}
