//! `vdbe.c` (parte 4): trechos 017 a 022 do `vdbe.c` do SQLite 3.46.1, os opcodes de `OP_Clear`
//! até o fim de `sqlite3VdbeExec`: `Clear`, `ResetSorter`, `CreateBtree`, `SqlExec`,
//! `ParseSchema`, `LoadAnalysis`, `DropTable`, `DropIndex`, `DropTrigger`, `IntegrityCk`,
//! `RowSetAdd`, `RowSetRead`, `RowSetTest`, `Program`, `Param`, `FkCounter`, `FkIfZero`,
//! `MemMax`, `IfPos`, `OffsetLimit`, `IfNotZero`, `DecrJumpZero`, `AggStep`, `AggInverse`,
//! `AggStep1`, `AggFinal`, `AggValue`, `Checkpoint`, `JournalMode`, `Vacuum`, `IncrVacuum`,
//! `Expire`, `CursorLock`, `CursorUnlock`, `TableLock`, `VBegin`, `VCreate`, `VDestroy`, `VOpen`,
//! `VCheck`, `VInitIn`, `VFilter`, `VColumn`, `VNext`, `VRename`, `VUpdate`, `Pagecount`,
//! `MaxPgcnt`, `Function`, `PureFunc`, `ClrSubtype`, `GetSubtype`, `SetSubtype`, `FilterAdd`,
//! `Filter`, `Trace`, `Init` e o `default` do C (`OP_Noop` e `OP_Explain`). A saída do laço
//! (`abort_due_to_error`, `vdbe_return`, ...) já está em [`crate::vdbe::vdbe_tail`] e não se
//! repete aqui.
//!
//! Convenções do laço: as mesmas de `vdbe_ops2.rs` (`pc` é o índice da instrução corrente no
//! programa corrente, `st.op` traz a cópia dos operandos escalares, `Flow::Continue(pc)` segue
//! para `pc + 1`, `Flow::Jump(dest)` é o `goto jump_to_p2` e `Flow::JumpCheck(dest)` o
//! `goto jump_to_p2_and_check_for_interrupt`). O `goto check_for_interrupt` (conferir a
//! interrupção e seguir para a instrução seguinte) vira `Flow::JumpCheck(pc + 1)`.
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * `OP_Program`: o frame é novo a cada chamada (ver `vdbe_types.rs`): guarda por valor os
//!   registros, cursores, programa, `auxdata` e contadores do CHAMADOR e o `Vdbe` passa a ter os
//!   do filho. O registro `P3` do pai, que no C guarda o frame para reaproveitá-lo, não é usado.
//! * `OP_AggStep`, `OP_AggInverse` e `OP_Function` montam o `Context` (o `sqlite3_context`) a
//!   cada chamada, a partir do `FuncCtx` imutável do P4. O acumulador do agregado viaja de
//!   `Mem.agg` para `Context.agg` e volta, e a célula de saída do `OP_Function` é movida para o
//!   contexto e devolvida ao registro (ver `connection.rs`). A reescrita do opcode para
//!   `OP_AggStep1` e do P4 para `P4::FuncCtx` só vale no programa principal; num subprograma
//!   (um `Rc<SubProgram>` imutável) a conversão é refeita a cada execução.
//! * `OP_Expire` com `P1 == 0` percorre `Connection.stmts`, de onde o `Vdbe` em execução está
//!   fora; por isso ele mesmo é marcado em seguida (o C o marca no laço).
//! * `OP_Init` e `OP_Trace`: o incremento de `P1` e o zeramento dos `OP_Once` só valem no
//!   programa principal (o `OP_Init` é sempre a instrução 0 dele).
//! * `OP_VOpen` e os demais opcodes de tabela virtual retiram o cursor, o `VTable` e a instância
//!   `Vtab` de seus donos durante a chamada ao módulo (como `OP_Rowid` em `vdbe_ops2.rs`).
//! * `OP_VInitIn`: o `ValueList` do C guarda o `BtCursor*` e o `sqlite3_value*` de saída; aqui
//!   guarda o número do cursor do VDBE e o índice do registro (ver [`ValueList`]).
//! * Somem os ramos `SQLITE_DEBUG`, `SQLITE_TEST`, `VdbeBranchTaken`, `REGISTER_TRACE`,
//!   `UPDATE_MAX_BLOBSIZE`, `memAboutToChange`, `sqlite3VdbeIncrWriteCounter`,
//!   `SQLITE_ENABLE_CURSOR_HINTS` (`OP_CursorHint`), `OP_Abortable` e `OP_ReleaseReg` (só existem
//!   em compilação de depuração) e `SQLITE_USE_FCNTL_TRACE`.

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::btree::{btree_last_page, btree_max_page_count};
use crate::btree_cursor::{btree_cursor_pin, btree_cursor_unpin, btree_incr_vacuum};
use crate::btree_write::{
    btree_clear_table, btree_create_table, btree_integrity_check, btree_lock_table,
    btree_set_version, btree_txn_state, IntegrityDb,
};
use crate::build::{
    reset_all_schemas_of_connection, unlink_and_delete_index, unlink_and_delete_table,
};
use crate::connection::{
    Connection, Context, FuncCtx, FuncDef, TraceEvent, VTableId, Vtab, VtabCursor,
};
use crate::consts::*;
use crate::legacy::{exec, exec_with_errmsg};
use crate::mem::{
    mem_copy, mem_integerify, mem_set_int64, mem_set_null, mem_set_row_set, mem_set_str_dynamic,
    mem_shallow_copy, value_text, vdbe_change_encoding, Mem, ENC_UTF8,
};
use crate::prepare::{init_callback, init_one, InitData};
use crate::printf::{mprintf, PrintfArg};
use crate::rowset::{row_set_insert, row_set_next, row_set_test};
use crate::util::{add_int64, strlen30};
use crate::vdbe::{
    allocate_cursor, cur_ops, filter_hash, is_sorter, out2_prerelease, p4_z, prog_ops, two_mut,
    ExecState, Flow,
};
use crate::vdbe_ops::p4_int32;
use crate::vdbe_ops2::csr;
use crate::vdbe_types::{Op, SubProgram, Vdbe, VdbeCursor, VdbeFrame, P4};
use crate::vdbeaux::vdbe_error;
use crate::vdbeaux2::{delete_aux_data, with_bt_db, with_btree_cursor};
use crate::vdbeaux3::{expire_prepared_statements, vtab_import_errmsg};
use crate::vdbesort::vdbe_sorter_reset;

/// `sqlite3GlobalConfig.iOnceResetThreshold`: o valor de `P1` do `OP_Init` a partir do qual os
/// `OP_Once` do programa são zerados (`SQLITE_TESTCTRL_ONCE_RESET_THRESHOLD` não é exposto).
const ONCE_RESET_THRESHOLD: i32 = 0x7fff_fff0;

/// O `ValueList` de `vdbeapi.c` (o que `OP_VInitIn` guarda no registro `P2` como valor ponteiro
/// do tipo `"ValueList"`): `pCsr` é o número do cursor do VDBE (o `BtCursor*` do C se alcança
/// por ele) e `pOut` o índice, em `Vdbe.a_mem`, do registro de saída (`P3`).
pub struct ValueList {
    /// Número do cursor do VDBE com os valores do lado direito do `IN`.
    pub p_csr: i32,
    /// Índice do registro onde `sqlite3_vtab_in_first/next` entregam o valor.
    pub p_out: usize,
}

// ---------------------------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------------------------

/// O texto do P4 (até o primeiro NUL) como cópia; vazio se o P4 não é texto.
fn p4_text(p: &Vdbe, pc: usize) -> Vec<u8> {
    p4_z(&cur_ops(p)[pc]).map(<[u8]>::to_vec).unwrap_or_default()
}

/// Grava em `out` uma string "estática" (`MEM_Str|MEM_Static|MEM_Term`) com os bytes de `z`,
/// em UTF-8 (o `pOut->z = "..."; pOut->n = ...; pOut->enc = SQLITE_UTF8` do C).
fn set_static_text(out: &mut Mem, z: &[u8]) {
    let mut v: Vec<u8> = Vec::with_capacity(z.len() + 1);
    v.extend_from_slice(z);
    v.push(0);
    out.flags = MEM_STR | MEM_STATIC | MEM_TERM;
    out.z = v;
    out.sz_malloc = 0;
    out.n = strlen30(z);
    out.enc = ENC_UTF8;
}

/// Retira a instância `Vtab` do `VTable` `vid` de seu dono, roda `f` com a conexão e a devolve.
/// `None` se o `VTable` ou a instância não existem (o `pVtab==0` do C).
fn with_vtab<R>(
    db: &mut Connection,
    vid: VTableId,
    f: impl FnOnce(&mut Connection, &mut dyn Vtab) -> R,
) -> Option<R> {
    let mut vt = db.vtabs.take(vid.slot())?;
    let r = match vt.p_vtab.take() {
        Some(mut vtab) => {
            let r = f(db, &mut *vtab);
            vt.p_vtab = Some(vtab);
            Some(r)
        }
        None => None,
    };
    db.vtabs.put(vid.slot(), vt);
    r
}

/// Como [`with_vtab`], para o cursor de tabela virtual `i_cur` do VDBE: retira também o
/// `VtabCursor` de `Vdbe.ap_csr` e entrega `p` à função (`pCur->uc.pVCur` e `pVCur->pVtab`).
fn with_vcursor<R>(
    db: &mut Connection,
    p: &mut Vdbe,
    i_cur: i32,
    f: impl FnOnce(&mut Connection, &mut Vdbe, &mut dyn VtabCursor, &mut dyn Vtab) -> R,
) -> Option<R> {
    let c = csr(&mut p.ap_csr, i_cur)?;
    let vid = c.p_v_table?;
    let mut vcur = c.p_v_cur.take()?;
    let r = with_vtab(db, vid, |db, vtab| f(db, p, &mut *vcur, vtab));
    if let Some(c) = csr(&mut p.ap_csr, i_cur) {
        c.p_v_cur = Some(vcur);
    }
    r
}

/// A mensagem de erro de `sqlite3_value_text(m)` como valor de `%s`.
fn text_arg(m: &mut Mem) -> PrintfArg {
    PrintfArg::Text(value_text(m, ENC_UTF8).map(<[u8]>::to_vec))
}

// ---------------------------------------------------------------------------------------------
// O despacho
// ---------------------------------------------------------------------------------------------

/// Executa um opcode da fatia `Clear` até o fim; devolve o destino do fluxo. O `default` do C
/// (`OP_Noop`, `OP_Explain`) não faz nada.
pub fn exec_op3(db: &mut Connection, p: &mut Vdbe, pc: usize, st: &mut ExecState) -> Flow {
    let o = st.op;
    let pci = pc as i32;
    match o.opcode {
        // Opcode: Clear P1 P2 P3: apaga o conteúdo da tabela ou índice de raiz P1 (banco P2).
        OP_CLEAR => op_clear(db, p, st, pci),

        // Opcode: ResetSorter P1 * * * *: esvazia o cursor efêmero ou ordenador P1.
        OP_RESETSORTER => op_reset_sorter(p, st, pci),

        // Opcode: CreateBtree P1 P2 P3 * *: r[P2]=root iDb=P1 flags=P3.
        OP_CREATEBTREE => {
            let mut pgno: u32 = 0;
            debug_assert!(o.p3 == BTREE_INTKEY as i32 || o.p3 == BTREE_BLOBKEY as i32);
            debug_assert!(!p.read_only);
            out2_prerelease(&mut p.a_mem, o.p2);
            let rc = match db.dbs.get_mut(o.p1 as usize).and_then(|s| s.bt.as_mut()) {
                Some(bt) => btree_create_table(bt, o.p3, &mut pgno),
                None => SQLITE_ERROR,
            };
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
            p.a_mem[o.p2 as usize].u_i = pgno as i64;
            Flow::Continue(pci)
        }

        // Opcode: SqlExec P1 P2 * P4 *: roda o SQL do P4.
        OP_SQLEXEC => op_sql_exec(db, p, st, pc),

        // Opcode: ParseSchema P1 * * P4 *: relê as entradas do esquema do banco P1.
        OP_PARSESCHEMA => op_parse_schema(db, p, st, pc),

        // Opcode: LoadAnalysis P1 * * * *: carrega o sqlite_stat1 do banco P1.
        OP_LOADANALYSIS => {
            debug_assert!(o.p1 >= 0 && (o.p1 as usize) < db.dbs.len());
            let rc = crate::analyze::analysis_load(db, o.p1);
            if rc != SQLITE_OK {
                st.abort(rc)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: DropTable P1 * * P4 *: esquece a tabela P4 do banco P1.
        OP_DROPTABLE => {
            let z = p4_text(p, pc);
            unlink_and_delete_table(db, o.p1 as usize, &z);
            Flow::Continue(pci)
        }

        // Opcode: DropIndex P1 * * P4 *: esquece o índice P4 do banco P1.
        OP_DROPINDEX => {
            let z = p4_text(p, pc);
            unlink_and_delete_index(db, o.p1 as usize, &z);
            Flow::Continue(pci)
        }

        // Opcode: DropTrigger P1 * * P4 *: esquece o gatilho P4 do banco P1.
        OP_DROPTRIGGER => {
            let z = p4_text(p, pc);
            crate::trigger::unlink_and_delete_trigger(db, o.p1 as usize, &z);
            Flow::Continue(pci)
        }

        // Opcode: IntegrityCk P1 P2 P3 P4 P5: confere o banco (PRAGMA integrity_check).
        OP_INTEGRITYCK => op_integrity_ck(db, p, st, pc),

        // Opcode: RowSetAdd P1 P2 * * *: rowset(P1)=r[P2].
        OP_ROWSETADD => {
            let v = p.a_mem[o.p2 as usize].u_i;
            debug_assert!((p.a_mem[o.p2 as usize].flags & MEM_INT) != 0);
            let in1 = &mut p.a_mem[o.p1 as usize];
            if (in1.flags & MEM_BLOB) == 0 && mem_set_row_set(in1) != SQLITE_OK {
                return Flow::NoMem;
            }
            if let Some(rs) = in1.row_set.as_deref_mut() {
                row_set_insert(rs, v);
            }
            Flow::Continue(pci)
        }

        // Opcode: RowSetRead P1 P2 P3 * *: r[P3]=rowset(P1), ou salta para P2 se vazio.
        OP_ROWSETREAD => {
            let in1 = &mut p.a_mem[o.p1 as usize];
            let val = if (in1.flags & MEM_BLOB) == 0 {
                None
            } else {
                in1.row_set.as_deref_mut().and_then(row_set_next)
            };
            match val {
                None => {
                    // O índice booleano está vazio.
                    mem_set_null(&mut p.a_mem[o.p1 as usize]);
                    Flow::JumpCheck(o.p2)
                }
                Some(v) => {
                    // Um valor saiu do índice.
                    mem_set_int64(&mut p.a_mem[o.p3 as usize], v);
                    Flow::JumpCheck(pci + 1)
                }
            }
        }

        // Opcode: RowSetTest P1 P2 P3 P4: se r[P3] está em rowset(P1) salta para P2.
        OP_ROWSETTEST => {
            let i_set = p4_int32(&cur_ops(p)[pc]);
            let v = p.a_mem[o.p3 as usize].u_i;
            debug_assert!((p.a_mem[o.p3 as usize].flags & MEM_INT) != 0);
            let in1 = &mut p.a_mem[o.p1 as usize];
            // Se há algo que não é um rowset na célula P1, apaga e a inicializa vazia.
            if (in1.flags & MEM_BLOB) == 0 && mem_set_row_set(in1) != SQLITE_OK {
                return Flow::NoMem;
            }
            debug_assert!(i_set == -1 || i_set >= 0);
            if let Some(rs) = in1.row_set.as_deref_mut() {
                if i_set != 0 && row_set_test(rs, i_set, v) {
                    return Flow::Jump(o.p2);
                }
                if i_set >= 0 {
                    row_set_insert(rs, v);
                }
            }
            Flow::Continue(pci)
        }

        // Opcode: Program P1 P2 P3 P4 P5: roda o subprograma (gatilho) do P4.
        OP_PROGRAM => op_program(db, p, st, pc),

        // Opcode: Param P1 P2 * * *: copia um registro do frame pai para r[P2].
        OP_PARAM => {
            out2_prerelease(&mut p.a_mem, o.p2);
            let Some(frame) = p.p_frame.last() else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            let Some(call) = prog_ops(&frame.a_op, &p.a_op).get(frame.pc as usize) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            let Some(src) = frame.a_mem.get((o.p1 + call.p1) as usize) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            mem_shallow_copy(&mut p.a_mem[o.p2 as usize], src, MEM_EPHEM);
            Flow::Continue(pci)
        }

        // Opcode: FkCounter P1 P2 * * *: fkctr[P1]+=P2.
        OP_FKCOUNTER => {
            if (db.flags & SQLITE_DEFER_FKS) != 0 {
                db.n_deferred_imm_cons += o.p2 as i64;
            } else if o.p1 != 0 {
                db.n_deferred_cons += o.p2 as i64;
            } else {
                p.n_fk_constraint += o.p2 as i64;
            }
            Flow::Continue(pci)
        }

        // Opcode: FkIfZero P1 P2 * * *: if fkctr[P1]==0 goto P2.
        OP_FKIFZERO => {
            let zero = if o.p1 != 0 {
                db.n_deferred_cons == 0 && db.n_deferred_imm_cons == 0
            } else {
                p.n_fk_constraint == 0 && db.n_deferred_imm_cons == 0
            };
            if zero {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: MemMax P1 P2 * * *: r[P1]=max(r[P1],r[P2]), com P1 no frame raiz.
        OP_MEMMAX => {
            let v2 = {
                let m = &mut p.a_mem[o.p2 as usize];
                mem_integerify(m);
                m.u_i
            };
            let idx1 = o.p1 as usize;
            // O frame raiz é o mais antigo da pilha (o que guarda os registros do programa raiz).
            let in1: &mut Mem = match p.p_frame.first_mut() {
                Some(f) => &mut f.a_mem[idx1],
                None => &mut p.a_mem[idx1],
            };
            mem_integerify(in1);
            if in1.u_i < v2 {
                in1.u_i = v2;
            }
            Flow::Continue(pci)
        }

        // Opcode: IfPos P1 P2 P3 * *: if r[P1]>0 then r[P1]-=P3, goto P2.
        OP_IFPOS => {
            let in1 = &mut p.a_mem[o.p1 as usize];
            debug_assert!((in1.flags & MEM_INT) != 0);
            if in1.u_i > 0 {
                in1.u_i = in1.u_i.wrapping_sub(o.p3 as i64);
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: OffsetLimit P1 P2 P3 * *: r[P2] = limite combinado com o deslocamento.
        OP_OFFSETLIMIT => {
            let lim = p.a_mem[o.p1 as usize].u_i;
            let off = p.a_mem[o.p3 as usize].u_i;
            debug_assert!((p.a_mem[o.p1 as usize].flags & MEM_INT) != 0);
            debug_assert!((p.a_mem[o.p3 as usize].flags & MEM_INT) != 0);
            let out = out2_prerelease(&mut p.a_mem, o.p2);
            let mut x = lim;
            if x <= 0 || add_int64(&mut x, if off > 0 { off } else { 0 }) != 0 {
                // LIMIT menor ou igual a zero: repete para sempre (documentado). LIMIT mais
                // OFFSET acima de 2^63 também (não documentado).
                out.u_i = -1;
            } else {
                out.u_i = x;
            }
            Flow::Continue(pci)
        }

        // Opcode: IfNotZero P1 P2 * * *: if r[P1]!=0 then r[P1]--, goto P2.
        OP_IFNOTZERO => {
            let in1 = &mut p.a_mem[o.p1 as usize];
            debug_assert!((in1.flags & MEM_INT) != 0);
            if in1.u_i != 0 {
                if in1.u_i > 0 {
                    in1.u_i -= 1;
                }
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: DecrJumpZero P1 P2 * * *: if (--r[P1])==0 goto P2.
        OP_DECRJUMPZERO => {
            let in1 = &mut p.a_mem[o.p1 as usize];
            debug_assert!((in1.flags & MEM_INT) != 0);
            if in1.u_i > SMALLEST_INT64 {
                in1.u_i -= 1;
            }
            if in1.u_i == 0 {
                Flow::Jump(o.p2)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: AggStep, AggInverse, AggStep1: passo (ou inverso) do agregado.
        OP_AGGINVERSE | OP_AGGSTEP | OP_AGGSTEP1 => op_agg_step(db, p, st, pc),

        // Opcode: AggFinal P1 P2 * P4 *  e  Opcode: AggValue * P2 P3 P4 *.
        OP_AGGVALUE | OP_AGGFINAL => op_agg_final(db, p, st, pc),

        // Opcode: Checkpoint P1 P2 P3 * *: faz o checkpoint do banco P1 no modo P2.
        OP_CHECKPOINT => {
            debug_assert!(!p.read_only);
            let mut a_res: [i32; 3] = [0, -1, -1];
            let (mut n_log, mut n_ckpt) = (-1i32, -1i32);
            let rc = crate::main::checkpoint(db, o.p1, o.p2, &mut n_log, &mut n_ckpt);
            a_res[1] = n_log;
            a_res[2] = n_ckpt;
            if rc != SQLITE_OK {
                if rc != SQLITE_BUSY {
                    return st.abort(rc);
                }
                // SQLITE_BUSY não é erro: r[P3] recebe 1.
                a_res[0] = 1;
            }
            for (i, v) in a_res.iter().enumerate() {
                mem_set_int64(&mut p.a_mem[o.p3 as usize + i], *v as i64);
            }
            Flow::Continue(pci)
        }

        // Opcode: JournalMode P1 P2 P3 * *: muda o modo de journal do banco P1 para P3.
        OP_JOURNALMODE => op_journal_mode(db, p, st, pc),

        // Opcode: Vacuum P1 P2 * * *: VACUUM do banco P1 (P2 é o registro do arquivo de saída).
        OP_VACUUM => {
            debug_assert!(!p.read_only);
            let out = if o.p2 != 0 { p.a_mem.get_mut(o.p2 as usize) } else { None };
            let rc = crate::vacuum::run_vacuum(&mut p.z_err_msg, db, o.p1, out);
            if rc != SQLITE_OK {
                st.abort(rc)
            } else {
                Flow::Continue(pci)
            }
        }

        // Opcode: IncrVacuum P1 P2 * * *: um passo do vacuum incremental; acabou salta para P2.
        OP_INCRVACUUM => {
            debug_assert!(o.p1 >= 0 && (o.p1 as usize) < db.dbs.len());
            debug_assert!(!p.read_only);
            let rc = match db.dbs.get_mut(o.p1 as usize).and_then(|s| s.bt.as_mut()) {
                Some(bt) => btree_incr_vacuum(bt),
                None => SQLITE_ERROR,
            };
            if rc != SQLITE_OK {
                if rc != SQLITE_DONE {
                    return st.abort(rc);
                }
                // O vacuum terminou: rc volta a ser SQLITE_OK e salta para P2.
                return Flow::Jump(o.p2);
            }
            Flow::Continue(pci)
        }

        // Opcode: Expire P1 P2 * * *: faz os comandos preparados expirarem.
        OP_EXPIRE => {
            debug_assert!(o.p2 == 0 || o.p2 == 1);
            if o.p1 == 0 {
                expire_prepared_statements(db, o.p2);
            }
            // Com P1 diferente de zero só este comando expira. Com P1 igual a zero o laço do C
            // também o alcança; aqui ele está fora de `Connection.stmts`, então é marcado à parte.
            p.expired = (o.p2 + 1) as u8;
            Flow::Continue(pci)
        }

        // Opcode: CursorLock P1 * * * *  e  Opcode: CursorUnlock P1 * * * *.
        OP_CURSORLOCK | OP_CURSORUNLOCK => {
            let Some(c) = csr(&mut p.ap_csr, o.p1) else {
                return st.abort(SQLITE_CORRUPT_BKPT);
            };
            debug_assert!(c.e_cur_type == CURTYPE_BTREE);
            let lock = o.opcode == OP_CURSORLOCK;
            let _ = with_btree_cursor(db, c, |cur, _bt| {
                if lock {
                    btree_cursor_pin(cur)
                } else {
                    btree_cursor_unpin(cur)
                }
            });
            Flow::Continue(pci)
        }

        // Opcode: TableLock P1 P2 P3 P4 *: iDb=P1 root=P2 write=P3.
        OP_TABLELOCK => {
            let is_write_lock = o.p3 as u8;
            if is_write_lock != 0 || (db.flags & SQLITE_READ_UNCOMMIT) == 0 {
                debug_assert!(o.p1 >= 0 && (o.p1 as usize) < db.dbs.len());
                debug_assert!(is_write_lock == 0 || is_write_lock == 1);
                let rc = match db.dbs.get_mut(o.p1 as usize).and_then(|s| s.bt.as_mut()) {
                    Some(bt) => btree_lock_table(bt, o.p2, is_write_lock),
                    None => SQLITE_OK,
                };
                if rc != SQLITE_OK {
                    if (rc & 0xff) == SQLITE_LOCKED {
                        let z = p4_z(&cur_ops(p)[pc]).map(<[u8]>::to_vec);
                        vdbe_error(p, db, b"database table is locked: %s", &[PrintfArg::Text(z)]);
                    }
                    return st.abort(rc);
                }
            }
            Flow::Continue(pci)
        }

        // Opcode: VBegin * * * P4 *: chama o xBegin da tabela virtual do P4.
        OP_VBEGIN => op_v_begin(db, p, st, pc),

        // Opcode: VCreate P1 P2 * * *: chama o xCreate da tabela virtual cujo nome está em r[P2].
        OP_VCREATE => {
            // O P2 é sempre uma string estática, então a cópia nunca falha.
            debug_assert!((p.a_mem[o.p2 as usize].flags & MEM_STR) != 0);
            let mut s = Mem::default();
            let rc0 = mem_copy(&mut s, &p.a_mem[o.p2 as usize]);
            debug_assert!(rc0 == SQLITE_OK);
            let z_tab = value_text(&mut s, ENC_UTF8).map(<[u8]>::to_vec);
            let mut rc = SQLITE_OK;
            if let Some(z) = z_tab {
                rc = crate::vtab::vtab_call_create(db, o.p1, &z, &mut p.z_err_msg);
            }
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
            Flow::Continue(pci)
        }

        // Opcode: VDestroy P1 * * P4 *: chama o xDestroy da tabela virtual P4 do banco P1.
        OP_VDESTROY => {
            let z = p4_text(p, pc);
            db.n_v_destroy += 1;
            let rc = crate::vtab::vtab_call_destroy(db, o.p1, &z);
            db.n_v_destroy -= 1;
            debug_assert!(p.error_action == OE_ABORT && p.uses_stmt_journal);
            if rc != SQLITE_OK {
                return st.abort(rc);
            }
            Flow::Continue(pci)
        }

        // Opcode: VOpen P1 * * P4 *: abre um cursor sobre a tabela virtual do P4.
        OP_VOPEN => op_v_open(db, p, st, pc),

        // Opcode: VCheck P1 P2 P3 P4 *: roda o xIntegrity da tabela virtual do P4.
        OP_VCHECK => op_v_check(db, p, st, pc),

        // Opcode: VInitIn P1 P2 P3 * *: r[P2]=ValueList(P1,P3).
        OP_VINITIN => {
            out2_prerelease(&mut p.a_mem, o.p2);
            let out = &mut p.a_mem[o.p2 as usize];
            out.flags = MEM_NULL;
            crate::mem::mem_set_pointer(
                out,
                Box::new(ValueList { p_csr: o.p1, p_out: o.p3 as usize }),
                b"ValueList",
            );
            Flow::Continue(pci)
        }

        // Opcode: VFilter P1 P2 P3 P4 *: iplan=r[P3] zplan='P4'.
        OP_VFILTER => op_v_filter(db, p, st, pc),

        // Opcode: VColumn P1 P2 P3 * P5: r[P3]=vcolumn(P2).
        OP_VCOLUMN => op_v_column(db, p, st, pc),

        // Opcode: VNext P1 P2 * * *: avança a tabela virtual P1 e salta para P2.
        OP_VNEXT => op_v_next(db, p, st, pc),

        // Opcode: VRename P1 * * P4 *: chama o xRename da tabela virtual P4 com o nome r[P1].
        OP_VRENAME => op_v_rename(db, p, st, pc),

        // Opcode: VUpdate P1 P2 P3 P4 P5: chama o xUpdate (data=r[P3@P2]).
        OP_VUPDATE => op_v_update(db, p, st, pc),

        // Opcode: Pagecount P1 P2 * * *: r[P2] = número de páginas do banco P1.
        OP_PAGECOUNT => {
            out2_prerelease(&mut p.a_mem, o.p2);
            let n = db
                .dbs
                .get(o.p1 as usize)
                .and_then(|s| s.bt.as_ref())
                .map_or(0, btree_last_page);
            p.a_mem[o.p2 as usize].u_i = n as i64;
            Flow::Continue(pci)
        }

        // Opcode: MaxPgcnt P1 P2 P3 * *: tenta fixar o máximo de páginas do banco P1 em P3.
        OP_MAXPGCNT => {
            out2_prerelease(&mut p.a_mem, o.p2);
            let mut v: u32 = 0;
            if let Some(bt) = db.dbs.get_mut(o.p1 as usize).and_then(|s| s.bt.as_mut()) {
                let mut new_max: u32 = 0;
                if o.p3 != 0 {
                    new_max = btree_last_page(bt);
                    if new_max < o.p3 as u32 {
                        new_max = o.p3 as u32;
                    }
                }
                v = btree_max_page_count(bt, new_max);
            }
            p.a_mem[o.p2 as usize].u_i = v as i64;
            Flow::Continue(pci)
        }

        // Opcode: Function P1 P2 P3 P4 *  e  Opcode: PureFunc P1 P2 P3 P4 *.
        OP_PUREFUNC | OP_FUNCTION => op_function(db, p, st, pc),

        // Opcode: ClrSubtype P1 * * * *: r[P1].subtype = 0.
        OP_CLRSUBTYPE => {
            p.a_mem[o.p1 as usize].flags &= !MEM_SUBTYPE;
            Flow::Continue(pci)
        }

        // Opcode: GetSubtype P1 P2 * * *: r[P2] = r[P1].subtype.
        OP_GETSUBTYPE => {
            let sub = {
                let in1 = &p.a_mem[o.p1 as usize];
                if (in1.flags & MEM_SUBTYPE) != 0 {
                    Some(in1.e_subtype as i64)
                } else {
                    None
                }
            };
            let out = &mut p.a_mem[o.p2 as usize];
            match sub {
                Some(v) => mem_set_int64(out, v),
                None => mem_set_null(out),
            }
            Flow::Continue(pci)
        }

        // Opcode: SetSubtype P1 P2 * * *: r[P2].subtype = r[P1].
        OP_SETSUBTYPE => {
            let (is_null, v) = {
                let in1 = &p.a_mem[o.p1 as usize];
                ((in1.flags & MEM_NULL) != 0, in1.u_i)
            };
            debug_assert!(is_null || (p.a_mem[o.p1 as usize].flags & MEM_INT) != 0);
            let out = &mut p.a_mem[o.p2 as usize];
            if is_null {
                out.flags &= !MEM_SUBTYPE;
            } else {
                out.flags |= MEM_SUBTYPE;
                out.e_subtype = (v & 0xff) as u8;
            }
            Flow::Continue(pci)
        }

        // Opcode: FilterAdd P1 * P3 P4 *: filter(P1) += key(P3@P4).
        OP_FILTERADD => {
            debug_assert!((p.a_mem[o.p1 as usize].flags & MEM_BLOB) != 0);
            debug_assert!(p.a_mem[o.p1 as usize].n > 0);
            let mut h = filter_hash(&p.a_mem, &cur_ops(p)[pc]);
            let in1 = &mut p.a_mem[o.p1 as usize];
            let nbits = (in1.n as i64).wrapping_mul(8) as u64;
            if nbits == 0 || in1.z.len() < in1.n.max(0) as usize {
                return st.abort(SQLITE_CORRUPT_BKPT);
            }
            h %= nbits;
            in1.z[(h / 8) as usize] |= 1u8 << (h & 7);
            Flow::Continue(pci)
        }

        // Opcode: Filter P1 P2 P3 P4 *: if key(P3@P4) not in filter(P1) goto P2.
        OP_FILTER => {
            debug_assert!((p.a_mem[o.p1 as usize].flags & MEM_BLOB) != 0);
            debug_assert!(p.a_mem[o.p1 as usize].n >= 1);
            let mut h = filter_hash(&p.a_mem, &cur_ops(p)[pc]);
            let in1 = &p.a_mem[o.p1 as usize];
            let nbits = (in1.n as i64).wrapping_mul(8) as u64;
            if nbits == 0 || in1.z.len() < in1.n.max(0) as usize {
                return st.abort(SQLITE_CORRUPT_BKPT);
            }
            h %= nbits;
            if (in1.z[(h / 8) as usize] & (1u8 << (h & 7))) == 0 {
                let k = SQLITE_STMTSTATUS_FILTER_HIT as usize;
                p.a_counter[k] = p.a_counter[k].wrapping_add(1);
                Flow::Jump(o.p2)
            } else {
                let k = SQLITE_STMTSTATUS_FILTER_MISS as usize;
                p.a_counter[k] = p.a_counter[k].wrapping_add(1);
                Flow::Continue(pci)
            }
        }

        // Opcode: Trace P1 P2 * P4 *  e  Opcode: Init P1 P2 P3 P4 *.
        OP_TRACE | OP_INIT => op_init(db, p, st, pc),

        // O `default` do C: OP_Noop e OP_Explain não fazem nada.
        _ => {
            debug_assert!(o.opcode == OP_NOOP || o.opcode == OP_EXPLAIN);
            Flow::Continue(pci)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 017: Clear, ResetSorter, SqlExec, ParseSchema, IntegrityCk
// ---------------------------------------------------------------------------------------------

/// `OP_Clear`: apaga o conteúdo da tabela ou índice de raiz P1 do banco P2. Com P3 diferente de
/// zero soma o número de linhas ao contador de mudanças (e, com P3 positivo, a r[P3]).
fn op_clear(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    debug_assert!(!p.read_only);
    let mut n_change: i64 = 0;
    let rc = match db.dbs.get_mut(o.p2 as usize).and_then(|s| s.bt.as_mut()) {
        Some(bt) => btree_clear_table(bt, o.p1, Some(&mut n_change)),
        None => SQLITE_ERROR,
    };
    if o.p3 != 0 {
        p.n_change = p.n_change.wrapping_add(n_change);
        if o.p3 > 0 {
            let m = &mut p.a_mem[o.p3 as usize];
            m.u_i = m.u_i.wrapping_add(n_change);
        }
    }
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc)
    }
}

/// `OP_ResetSorter`: esvazia o ordenador ou a tabela efêmera aberta no cursor P1.
fn op_reset_sorter(p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    if is_sorter(c) {
        if let Some(s) = c.p_sorter.as_deref_mut() {
            vdbe_sorter_reset(s);
        }
    } else {
        debug_assert!(c.e_cur_type == CURTYPE_BTREE && c.is_ephemeral);
        // `sqlite3BtreeClearTableOfCursor(pC->uc.pCursor)`: a raiz do cursor.
        let root = c.pgno_root as i32;
        let rc = match c.p_btx.as_mut() {
            Some(btx) => btree_clear_table(btx, root, None),
            None => SQLITE_ERROR,
        };
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
    }
    Flow::Continue(pc)
}

/// `OP_SqlExec`: roda o SQL do P4. Com P1 & 1 desliga a autorização e o rastreamento; com
/// P1 & 2 fixa `nAnalysisLimit` em P2 enquanto roda.
fn op_sql_exec(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let z_sql = p4_text(p, pc);
    db.n_sql_exec = db.n_sql_exec.wrapping_add(1);
    let mut z_err: Option<Vec<u8>> = None;
    let m_trace = db.m_trace;
    let saved_analysis_limit = db.n_analysis_limit;
    let disable = (o.p1 & 0x0001) != 0;
    let mut saved_auth = None;
    if disable {
        saved_auth = db.x_auth.take();
        db.m_trace = 0;
    }
    if (o.p1 & 0x0002) != 0 {
        db.n_analysis_limit = o.p2;
    }
    let rc = exec_with_errmsg(db, &z_sql, None, Some(&mut z_err));
    db.n_sql_exec = db.n_sql_exec.wrapping_sub(1);
    if disable {
        db.x_auth = saved_auth;
    }
    db.m_trace = m_trace;
    db.n_analysis_limit = saved_analysis_limit;
    if z_err.is_some() || rc != SQLITE_OK {
        vdbe_error(p, db, b"%s", &[PrintfArg::Text(z_err)]);
        if rc == SQLITE_NOMEM {
            return Flow::NoMem;
        }
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

/// `OP_ParseSchema`: lê de novo as entradas de `sqlite_schema` do banco P1 que casam com o WHERE
/// do P4 (todas, se o P4 é nulo). É reentrante: o analisador cria e roda outra VM.
fn op_parse_schema(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let i_db = o.p1;
    debug_assert!(i_db >= 0 && (i_db as usize) < db.dbs.len());
    let z_where: Option<Vec<u8>> = p4_z(&cur_ops(p)[pc]).map(<[u8]>::to_vec);
    let mut rc;
    match z_where {
        None => {
            crate::callback::schema_clear(db, i_db as usize);
            db.m_db_flags &= !DBFLAG_SCHEMA_KNOWN_OK;
            rc = init_one(db, i_db, &mut p.z_err_msg, o.p5 as u32);
            db.m_db_flags |= DBFLAG_SCHEMA_CHANGE;
            p.expired = 0;
        }
        Some(z_where) => {
            let z_schema = LEGACY_SCHEMA_TABLE;
            let mx_page = db.dbs[i_db as usize].bt.as_ref().map_or(0, btree_last_page);
            let z_db_name = db.dbs[i_db as usize].z_db_s_name.clone();
            let z_sql = mprintf(
                b"SELECT*FROM\"%w\".%s WHERE %s ORDER BY rowid",
                &[
                    PrintfArg::Text(Some(z_db_name)),
                    PrintfArg::Text(Some(z_schema.to_vec())),
                    PrintfArg::Text(Some(z_where)),
                ],
            );
            match z_sql {
                None => rc = SQLITE_NOMEM_BKPT,
                Some(z_sql) => {
                    debug_assert!(db.init.busy == 0);
                    db.init.busy = 1;
                    let mut init_data = InitData {
                        i_db,
                        rc: SQLITE_OK,
                        pz_err_msg: &mut p.z_err_msg,
                        m_init_flags: 0,
                        n_init_row: 0,
                        mx_page,
                    };
                    {
                        let mut cb = |db: &mut Connection,
                                      argv: &[Option<Vec<u8>>],
                                      _cols: &[Vec<u8>]| {
                            init_callback(db, &mut init_data, argv)
                        };
                        rc = exec(db, &z_sql, Some(&mut cb));
                    }
                    if rc == SQLITE_OK {
                        rc = init_data.rc;
                    }
                    if rc == SQLITE_OK && init_data.n_init_row == 0 {
                        // O OP_ParseSchema com P4 não nulo deve analisar ao menos um comando; menos
                        // que isso indica que `sqlite_schema` está corrompida.
                        rc = SQLITE_CORRUPT_BKPT;
                    }
                    db.init.busy = 0;
                }
            }
        }
    }
    if rc != SQLITE_OK {
        reset_all_schemas_of_connection(db);
        if rc == SQLITE_NOMEM {
            return Flow::NoMem;
        }
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

/// `OP_IntegrityCk`: confere o banco P5 (PRAGMA integrity_check). r[P1] guarda o número máximo
/// de erros menos um; r[P1+1] recebe o texto dos problemas (ou NULL); P4 é a lista de raízes e
/// as contagens de linha vão para r[P3...].
fn op_integrity_ck(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    debug_assert!(p.b_is_reader);
    let roots: Vec<u32> = match &cur_ops(p)[pc].p4 {
        P4::IntArray(ai) => ai.clone(),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    let n_root = o.p2.max(0) as usize;
    debug_assert!(n_root > 0 && roots.first().copied() == Some(n_root as u32));
    let Some(a_root) = roots.get(1..1 + n_root) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let p1 = o.p1 as usize;
    debug_assert!((p.a_mem[p1].flags & MEM_INT) != 0);
    debug_assert!((p.a_mem[p1].flags & (MEM_STR | MEM_BLOB)) == 0);
    let mx_err = (p.a_mem[p1].u_i as i32).wrapping_add(1);
    let first = o.p3 as usize;
    let Some(a_cnt) = p.a_mem.get_mut(first..first + n_root) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    let mut n_err: i32 = 0;
    let mut z: Option<Vec<u8>> = None;
    let rc = {
        let Connection { dbs, interrupted, x_progress, n_progress_ops, .. } = &mut *db;
        match dbs.get_mut(o.p5 as usize).and_then(|s| s.bt.as_mut()) {
            Some(bt) => {
                let mut is_interrupted = || interrupted.load(Ordering::Relaxed);
                let n_ops = if x_progress.is_some() { *n_progress_ops as u32 } else { 0 };
                let mut progress = || match x_progress.as_mut() {
                    Some(f) => f(),
                    None => 0,
                };
                btree_integrity_check(
                    IntegrityDb {
                        is_interrupted: &mut is_interrupted,
                        n_progress_ops: n_ops,
                        progress: &mut progress,
                    },
                    bt,
                    a_root,
                    a_cnt,
                    mx_err,
                    &mut n_err,
                    &mut z,
                )
            }
            None => SQLITE_ERROR,
        }
    };
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    mem_set_null(&mut p.a_mem[p1 + 1]);
    if n_err == 0 {
        debug_assert!(z.is_none());
    } else if rc != SQLITE_OK {
        return st.abort(rc);
    } else {
        let pn = &mut p.a_mem[p1];
        pn.u_i = pn.u_i.wrapping_sub(n_err as i64 - 1);
        mem_set_str_dynamic(&mut p.a_mem[p1 + 1], z, -1, ENC_UTF8, limit);
    }
    vdbe_change_encoding(&mut p.a_mem[p1 + 1], st.encoding as i32);
    // goto check_for_interrupt
    Flow::JumpCheck(pc as i32 + 1)
}

// ---------------------------------------------------------------------------------------------
// chunk 018: Program
// ---------------------------------------------------------------------------------------------

/// `OP_Program`: executa o subprograma (gatilho ou ação de chave estrangeira) do P4. P1 é o
/// primeiro registro dos argumentos; P2 é o destino do salto se o subprograma lança IGNORE; P5
/// diferente de zero liga a recursão entre gatilhos.
fn op_program(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let program: Rc<SubProgram> = match &cur_ops(p)[pc].p4 {
        P4::Subprogram(sp) => Rc::clone(sp),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    debug_assert!(!program.a_op.is_empty());

    // Com P5 limpo a invocação recursiva de gatilhos fica desligada, por compatibilidade (P5 vale
    // se o subprograma é de fato um gatilho, não uma ação de chave estrangeira, e a flag do
    // `PRAGMA recursive_triggers` está ligada). Os `SubProgram` de um mesmo gatilho (um por
    // algoritmo ON CONFLICT) têm o mesmo `token`.
    if o.p5 != 0 {
        let t = program.token;
        if p.p_frame.iter().any(|f| f.token == t) {
            return Flow::Continue(pc as i32);
        }
    }

    if p.p_frame.len() as i32 >= db.a_limit[SQLITE_LIMIT_TRIGGER_DEPTH as usize] {
        vdbe_error(p, db, b"too many levels of trigger recursion", &[]);
        return st.abort(SQLITE_ERROR);
    }

    // `SubProgram.nMem` é o número de células usadas pelo programa; além delas, uma por cursor.
    let mut n_mem = program.n_mem + program.n_csr;
    debug_assert!(n_mem > 0);
    if program.n_csr == 0 {
        n_mem += 1;
    }
    let n_csr = program.n_csr.max(0) as usize;
    let mut child_mem: Vec<Mem> = Vec::new();
    let mut child_csr: Vec<Option<Box<VdbeCursor>>> = Vec::new();
    if child_mem.try_reserve_exact(n_mem.max(0) as usize).is_err()
        || child_csr.try_reserve_exact(n_csr).is_err()
    {
        return Flow::NoMem;
    }
    // `MEM_Undefined` é a célula zerada.
    child_mem.resize_with(n_mem.max(0) as usize, Mem::default);
    child_csr.resize_with(n_csr, || None);

    let n_op_caller = cur_ops(p).len() as i32;
    let frame = VdbeFrame {
        a_op: p.p_cur_prog.take(),
        a_mem: std::mem::replace(&mut p.a_mem, child_mem),
        ap_csr: std::mem::replace(&mut p.ap_csr, child_csr),
        a_once: vec![0u8; program.a_op.len().div_ceil(8)],
        token: program.token,
        last_rowid: db.last_rowid,
        p_aux_data: std::mem::take(&mut p.p_aux_data),
        n_cursor: p.n_cursor,
        pc: pc as i32,
        n_op: n_op_caller,
        n_mem: p.n_mem,
        n_child_mem: n_mem,
        n_child_csr: program.n_csr,
        n_change: p.n_change,
        n_db_change: db.n_change,
    };
    p.p_frame.push(frame);
    p.n_change = 0;
    p.n_mem = n_mem;
    p.n_cursor = program.n_csr;
    p.p_cur_prog = Some(program);
    // pOp = &aOp[-1]; goto check_for_interrupt: a próxima instrução é a 0 do subprograma.
    Flow::JumpCheck(0)
}

// ---------------------------------------------------------------------------------------------
// chunk 019: AggStep, AggFinal, JournalMode
// ---------------------------------------------------------------------------------------------

/// `OP_AggStep`, `OP_AggInverse` e `OP_AggStep1`: roda o passo (P1 igual a 0) ou o inverso (P1
/// diferente de zero) do agregado do P4; P3 é o acumulador e r[P2@P5] os argumentos.
fn op_agg_step(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    // Na primeira avaliação o P4 é a `FuncDef`: vira um `FuncCtx` e o opcode vira `OP_AggStep1`.
    let (fctx, rewrite): (Rc<FuncCtx>, bool) = match &cur_ops(p)[pc].p4 {
        P4::FuncCtx(c) => (Rc::clone(c), false),
        P4::FuncDef(f) => (Rc::new(FuncCtx { p_func: Rc::clone(f), argc: o.p5 as u8 }), true),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    debug_assert!(o.p3 > 0);
    // OP_AggInverse tem P1 igual a 1 e OP_AggStep tem P1 igual a 0.
    if rewrite && p.p_cur_prog.is_none() {
        let op = &mut p.a_op[pc];
        op.p4 = P4::FuncCtx(Rc::clone(&fctx));
        op.opcode = OP_AGGSTEP1;
    }
    let m_idx = o.p3 as usize;
    let first = o.p2.max(0) as usize;
    let argc = fctx.argc as usize;
    p.a_mem[m_idx].n = p.a_mem[m_idx].n.wrapping_add(1);
    let agg = p.a_mem[m_idx].agg.take();
    let (is_error, skip_flag, out, agg) = {
        let p_coll = prev_coll(p, pc);
        let mut ctx = Context {
            db: &mut *db,
            p_coll,
            p_aux_data: &mut p.p_aux_data,
            i_current_time: &mut p.i_current_time,
            out: Mem::init(MEM_NULL),
            arg_func: Rc::clone(&fctx.p_func),
            agg,
            i_op: pc as i32,
            is_pure_func: false,
            is_error: 0,
            enc: st.encoding,
            skip_flag: 0,
            argc: fctx.argc,
        };
        let args = &p.a_mem[first..first + argc];
        if o.p1 != 0 {
            if let Some(f) = fctx.p_func.x_inverse {
                f(&mut ctx, args);
            }
        } else if let Some(f) = fctx.p_func.x_s_func {
            f(&mut ctx, args);
        }
        (ctx.is_error, ctx.skip_flag, ctx.out, ctx.agg)
    };
    if agg.is_some() {
        let m = &mut p.a_mem[m_idx];
        m.agg = agg;
        m.flags = MEM_AGG;
    }
    if is_error != 0 {
        let mut rc = SQLITE_OK;
        let mut out = out;
        if is_error > 0 {
            let arg = text_arg(&mut out);
            vdbe_error(p, db, b"%s", &[arg]);
            rc = is_error;
        }
        if skip_flag != 0 {
            debug_assert!(pc > 0 && cur_ops(p)[pc - 1].opcode == OP_COLLSEQ);
            let i = cur_ops(p).get(pc.wrapping_sub(1)).map_or(0, |op| op.p1);
            if i != 0 {
                mem_set_int64(&mut p.a_mem[i as usize], 1);
            }
        }
        drop(out);
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
    }
    Flow::Continue(pc as i32)
}

/// `OP_AggFinal` e `OP_AggValue`: roda o finalizador (ou `xValue`, com P3) do agregado do P4. P1
/// é o acumulador; com P3 o resultado vai para r[P3].
fn op_agg_final(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let func: Rc<FuncDef> = match &cur_ops(p)[pc].p4 {
        P4::FuncDef(f) => Rc::clone(f),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    debug_assert!(o.p1 > 0);
    debug_assert!(o.p3 == 0 || o.opcode == OP_AGGVALUE);
    let acc = o.p1 as usize;
    let (rc, idx) = if o.p3 != 0 {
        let out = o.p3 as usize;
        let (a, b) = two_mut(&mut p.a_mem, acc, out);
        (crate::mem::mem_agg_value(db, a, b, &func), out)
    } else {
        (crate::mem::mem_finalize(db, &mut p.a_mem[acc], &func), acc)
    };
    if rc != SQLITE_OK {
        let arg = text_arg(&mut p.a_mem[idx]);
        vdbe_error(p, db, b"%s", &[arg]);
        return st.abort(rc);
    }
    vdbe_change_encoding(&mut p.a_mem[idx], st.encoding as i32);
    Flow::Continue(pc as i32)
}

/// O resultado do trecho de `OP_JournalMode` que mexe no `Btree`.
enum JournalOutcome {
    /// A troca de/para WAL foi pedida dentro de uma transação (`true` se é para WAL).
    InTransaction(bool),
    /// `rc` e o modo de journal vigente depois de tudo.
    Done(i32, i32),
}

/// `OP_JournalMode`: muda o modo de journal do banco P1 para P3 (um `PAGER_JOURNALMODE_*`) e
/// grava em r[P2] o nome do modo final.
fn op_journal_mode(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    out2_prerelease(&mut p.a_mem, o.p2);
    debug_assert!(!p.read_only);
    let auto_commit = db.auto_commit;
    let n_vdbe_read = db.n_vdbe_read;
    let interrupted = Arc::clone(&db.interrupted);
    let e_new0 = o.p3;
    let outcome = with_bt_db(db, o.p1 as usize, |bt, bdb| {
        let wal = PAGER_JOURNALMODE_WAL as i32;
        let mut e_new = e_new0;
        let e_old = bt.bt.pager.journal_mode;
        if e_new == PAGER_JOURNALMODE_QUERY as i32 {
            e_new = e_old;
        }
        if !bt.bt.pager.ok_to_change_journal_mode() {
            e_new = e_old;
        }
        // Não permite a passagem para WAL num banco em armazenamento temporário nem se o VFS não
        // tem memória compartilhada.
        let is_temp_file = strlen30(bt.bt.pager.filename(true)) == 0;
        if e_new == wal && (is_temp_file || !bt.bt.pager.wal_supported()) {
            e_new = e_old;
        }
        let mut rc = SQLITE_OK;
        if e_new != e_old && (e_old == wal || e_new == wal) {
            if auto_commit == 0 || n_vdbe_read > 1 {
                return JournalOutcome::InTransaction(e_new == wal);
            }
            if e_old == wal {
                // Saindo do WAL: fecha o log. Se der certo, o `PagerCloseWal` faz o checkpoint e
                // apaga o arquivo; uma trava EXCLUSIVE pode continuar no banco.
                let mut intr = || interrupted.load(Ordering::Relaxed) as i32;
                rc = bt.bt.pager.close_wal(&mut intr);
                if rc == SQLITE_OK {
                    bt.bt.pager.set_journal_mode(e_new);
                }
            } else if e_old == PAGER_JOURNALMODE_MEMORY as i32 {
                // Não há passagem direta de MEMORY para WAL: usa OFF como etapa intermediária.
                bt.bt.pager.set_journal_mode(PAGER_JOURNALMODE_OFF as i32);
            }
            // Abre uma transação no arquivo; qualquer que seja o modo, ela usa um rollback journal.
            debug_assert!(btree_txn_state(Some(&*bt)) != SQLITE_TXN_WRITE);
            if rc == SQLITE_OK {
                rc = btree_set_version(bt, if e_new == wal { 2 } else { 1 }, bdb);
            }
        }
        if rc != SQLITE_OK {
            e_new = e_old;
        }
        e_new = bt.bt.pager.set_journal_mode(e_new);
        JournalOutcome::Done(rc, e_new)
    });
    let (rc, e_new) = match outcome {
        None => return st.abort(SQLITE_ERROR),
        Some(JournalOutcome::InTransaction(into_wal)) => {
            let which: &[u8] = if into_wal { b"into" } else { b"out of" };
            vdbe_error(
                p,
                db,
                b"cannot change %s wal mode from within a transaction",
                &[PrintfArg::Text(Some(which.to_vec()))],
            );
            return st.abort(SQLITE_ERROR);
        }
        Some(JournalOutcome::Done(rc, e_new)) => (rc, e_new),
    };
    let out = &mut p.a_mem[o.p2 as usize];
    set_static_text(out, crate::pragma::journal_modename(e_new));
    vdbe_change_encoding(out, st.encoding as i32);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

// ---------------------------------------------------------------------------------------------
// chunk 020 e 021: tabelas virtuais
// ---------------------------------------------------------------------------------------------

/// `OP_VBegin`: chama o `xBegin` da tabela virtual do P4 (e confere que não é um callback de
/// `xSync`: nesse caso o código é `SQLITE_LOCKED`).
fn op_v_begin(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let vid = match &cur_ops(p)[pc].p4 {
        P4::Vtab(id) => Some(*id),
        _ => None,
    };
    let rc = crate::vtab::vtab_begin(db, vid);
    if let Some(id) = vid {
        if let Some(vtab) = db.vtabs.get_mut(id.slot()).and_then(|vt| vt.p_vtab.as_mut()) {
            vtab_import_errmsg(p, vtab.err_msg_mut());
        }
    }
    if rc != SQLITE_OK {
        st.abort(rc)
    } else {
        Flow::Continue(pc as i32)
    }
}

/// `OP_VOpen`: abre o cursor P1 sobre a tabela virtual do P4.
fn op_v_open(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    debug_assert!(p.b_is_reader);
    let vid = match &cur_ops(p)[pc].p4 {
        P4::Vtab(id) => *id,
        _ => return st.abort(SQLITE_LOCKED),
    };
    let opened = with_vtab(db, vid, |db, vtab| {
        let r = vtab.open(db);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        r
    });
    let mut vcur = match opened {
        None => return st.abort(SQLITE_LOCKED),
        Some(Err(rc)) => return st.abort(rc),
        Some(Ok(c)) => c,
    };
    // Inicializa o objeto cursor do VDBE.
    match allocate_cursor(db, p, o.p1, 0, CURTYPE_VTAB) {
        Some(cx) => {
            cx.p_v_cur = Some(vcur);
            cx.p_v_table = Some(vid);
        }
        None => {
            with_vtab(db, vid, |db, vtab| vcur.close(db, vtab));
            return Flow::NoMem;
        }
    }
    if let Some(vt) = db.vtabs.get_mut(vid.slot()) {
        vt.n_cursor += 1;
    }
    Flow::Continue(pc as i32)
}

/// `OP_VCheck`: roda o `xIntegrity` da tabela virtual do P4 (banco P1, argumento P3); com erro,
/// r[P2] recebe o texto, senão NULL.
fn op_v_check(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    // Inocente até que se prove o contrário.
    mem_set_null(&mut p.a_mem[o.p2 as usize]);
    let tab = match &cur_ops(p)[pc].p4 {
        P4::TableRef(t) => Rc::clone(t),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    debug_assert!(tab.is_virtual());
    let Some(vid) = crate::vtab::get_vtable(db, &tab) else {
        return Flow::Continue(pc as i32);
    };
    let schema = db.dbs.get(o.p1 as usize).map(|s| s.z_db_s_name.clone()).unwrap_or_default();
    crate::vtab::vtab_lock(db, vid);
    let mut z_err: Option<Vec<u8>> = None;
    let rc = with_vtab(db, vid, |db, vtab| vtab.integrity(db, &schema, &tab.z_name, o.p3, &mut z_err))
        .unwrap_or(SQLITE_ERROR);
    crate::vtab::vtab_unlock(db, vid);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if z_err.is_some() {
        let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
        mem_set_str_dynamic(&mut p.a_mem[o.p2 as usize], z_err, -1, ENC_UTF8, limit);
    }
    Flow::Continue(pc as i32)
}

/// `OP_VFilter`: chama o `xFilter` do cursor virtual P1 com o plano r[P3] (o plano é um inteiro,
/// r[P3+1] é `argc` e r[P3+2...] os argumentos) e a cadeia do P4; salta para P2 se o resultado
/// filtrado é vazio.
fn op_v_filter(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    debug_assert!(o.p3 > 0);
    let q = o.p3 as usize;
    debug_assert!((p.a_mem[q].flags & MEM_INT) != 0 && p.a_mem[q + 1].flags == MEM_INT);
    let n_arg = p.a_mem[q + 1].u_i as i32;
    let i_query = p.a_mem[q].u_i as i32;
    let idx_str = p4_z(&cur_ops(p)[pc]).map(<[u8]>::to_vec);
    let first = q + 2;
    let n = n_arg.max(0) as usize;
    if first + n > p.a_mem.len() {
        return st.abort(SQLITE_CORRUPT_BKPT);
    }
    // Os argumentos saem dos registros durante a chamada e voltam depois (`apArg[i]`).
    let args: Vec<Mem> = (0..n).map(|i| std::mem::take(&mut p.a_mem[first + i])).collect();
    let res = with_vcursor(db, p, o.p1, |db, p, vcur, vtab| {
        let rc = vcur.filter(db, &mut *vtab, i_query, idx_str.as_deref(), &args);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        if rc != SQLITE_OK {
            return (rc, 0);
        }
        (rc, vcur.eof(&mut *vtab))
    });
    for (i, m) in args.into_iter().enumerate() {
        p.a_mem[first + i] = m;
    }
    let Some((rc, res)) = res else {
        return st.abort(SQLITE_ERROR);
    };
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if let Some(c) = csr(&mut p.ap_csr, o.p1) {
        c.null_row = false;
    }
    if res != 0 {
        Flow::Jump(o.p2)
    } else {
        Flow::Continue(pc as i32)
    }
}

/// `OP_VColumn`: r[P3] recebe a coluna P2 da linha corrente da tabela virtual do cursor P1. Com
/// `OPFLAG_NOCHNG` em P5 a célula começa como o NULL "sem mudança" de `xUpdate`.
fn op_v_column(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(o.p3 > 0);
    let dest = o.p3 as usize;
    if c.null_row {
        mem_set_null(&mut p.a_mem[dest]);
        return Flow::Continue(pc as i32);
    }
    debug_assert!(c.e_cur_type == CURTYPE_VTAB);
    let null_func = Rc::new(FuncDef { func_flags: SQLITE_RESULT_SUBTYPE as u32, ..FuncDef::default() });
    // A célula de saída viaja no contexto durante a chamada e volta ao registro depois.
    let mut out = std::mem::take(&mut p.a_mem[dest]);
    debug_assert!(o.p5 == OPFLAG_NOCHNG as u16 || o.p5 == 0);
    if (o.p5 & OPFLAG_NOCHNG as u16) != 0 {
        mem_set_null(&mut out);
        out.flags = MEM_NULL | MEM_ZERO;
        out.n_zero = 0;
    } else {
        out.set_type_flag(MEM_NULL);
    }
    let enc = st.encoding;
    let i_op = pc as i32;
    let i_col = o.p2;
    let res = with_vcursor(db, p, o.p1, move |db, p, vcur, vtab| {
        let p_coll = prev_coll(p, i_op as usize);
        let mut ctx = Context {
            db: &mut *db,
            p_coll,
            p_aux_data: &mut p.p_aux_data,
            i_current_time: &mut p.i_current_time,
            out,
            arg_func: null_func,
            agg: None,
            i_op,
            is_pure_func: false,
            is_error: 0,
            enc,
            skip_flag: 0,
            argc: 0,
        };
        let rc = vcur.column(&mut *vtab, &mut ctx, i_col);
        let (out, is_error) = (ctx.out, ctx.is_error);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        (rc, out, is_error)
    });
    let Some((mut rc, out, is_error)) = res else {
        mem_set_null(&mut p.a_mem[dest]);
        return st.abort(SQLITE_ERROR);
    };
    p.a_mem[dest] = out;
    if is_error > 0 {
        let arg = text_arg(&mut p.a_mem[dest]);
        vdbe_error(p, db, b"%s", &[arg]);
        rc = is_error;
    }
    vdbe_change_encoding(&mut p.a_mem[dest], st.encoding as i32);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

/// `OP_VNext`: avança o cursor virtual P1; havendo linha salta para P2, no fim cai adiante.
fn op_v_next(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let Some(c) = csr(&mut p.ap_csr, o.p1) else {
        return st.abort(SQLITE_CORRUPT_BKPT);
    };
    debug_assert!(c.e_cur_type == CURTYPE_VTAB);
    if c.null_row {
        return Flow::Continue(pc as i32);
    }
    // O módulo não tem como devolver erro do xNext: se acontece, devolve-se "há dados" e o código
    // sai na próxima chamada de xColumn ou de outro método.
    let res = with_vcursor(db, p, o.p1, |db, p, vcur, vtab| {
        let rc = vcur.next(db, &mut *vtab);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        if rc != SQLITE_OK {
            return (rc, 0);
        }
        (rc, vcur.eof(&mut *vtab))
    });
    let Some((rc, res)) = res else {
        return st.abort(SQLITE_ERROR);
    };
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    if res == 0 {
        // Há dados: salta para P2.
        Flow::JumpCheck(o.p2)
    } else {
        // goto check_for_interrupt
        Flow::JumpCheck(pc as i32 + 1)
    }
}

/// `OP_VRename`: chama o `xRename` da tabela virtual do P4 com o nome novo de r[P1].
fn op_v_rename(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let vid = match &cur_ops(p)[pc].p4 {
        P4::Vtab(id) => *id,
        _ => return st.abort(SQLITE_LOCKED),
    };
    let is_legacy = db.flags & SQLITE_LEGACY_ALTER;
    db.flags |= SQLITE_LEGACY_ALTER;
    debug_assert!(!p.read_only);
    let p1 = o.p1 as usize;
    debug_assert!((p.a_mem[p1].flags & MEM_STR) != 0);
    let rc = vdbe_change_encoding(&mut p.a_mem[p1], ENC_UTF8 as i32);
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    let name = p.a_mem[p1].bytes().to_vec();
    let rc = with_vtab(db, vid, |db, vtab| {
        let rc = vtab.rename(db, &name);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        rc
    })
    .unwrap_or(SQLITE_ERROR);
    if is_legacy == 0 {
        db.flags &= !SQLITE_LEGACY_ALTER;
    }
    p.expired = 0;
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

/// `OP_VUpdate`: chama o `xUpdate` da tabela virtual do P4 com os P2 registros a partir de r[P3]
/// (INSERT, UPDATE ou DELETE). Com P1 e sucesso, `last_insert_rowid` recebe o rowid devolvido.
/// P5 é a ação de conflito.
fn op_v_update(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    debug_assert!(!p.read_only);
    if db.malloc_failed != 0 {
        return Flow::NoMem;
    }
    let vid = match &cur_ops(p)[pc].p4 {
        P4::Vtab(id) => *id,
        _ => return st.abort(SQLITE_LOCKED),
    };
    let (has_update, b_constraint) = match db.vtabs.get(vid.slot()) {
        Some(vt) if vt.p_vtab.is_some() => (vt.p_mod.p_module.caps().update, vt.b_constraint),
        _ => return st.abort(SQLITE_LOCKED),
    };
    if !has_update {
        return Flow::Continue(pc as i32);
    }
    let n_arg = o.p2.max(0) as usize;
    let first = o.p3.max(0) as usize;
    if first + n_arg > p.a_mem.len() {
        return st.abort(SQLITE_CORRUPT_BKPT);
    }
    // Os argumentos saem dos registros durante a chamada e voltam depois (`apArg[i]`).
    let args: Vec<Mem> = (0..n_arg).map(|i| std::mem::take(&mut p.a_mem[first + i])).collect();
    let v_on_conflict = db.vtab_on_conflict;
    db.vtab_on_conflict = o.p5 as u8;
    let mut rowid: i64 = 0;
    let rc = with_vtab(db, vid, |db, vtab| {
        let rc = vtab.update(db, &args, &mut rowid);
        vtab_import_errmsg(p, vtab.err_msg_mut());
        rc
    })
    .unwrap_or(SQLITE_ERROR);
    db.vtab_on_conflict = v_on_conflict;
    for (i, m) in args.into_iter().enumerate() {
        p.a_mem[first + i] = m;
    }
    let mut rc = rc;
    if rc == SQLITE_OK && o.p1 != 0 {
        debug_assert!(n_arg > 1 && (p.a_mem[first].flags & MEM_NULL) != 0);
        db.last_rowid = rowid;
    }
    if (rc & 0xff) == SQLITE_CONSTRAINT && b_constraint != 0 {
        if o.p5 == OE_IGNORE as u16 {
            rc = SQLITE_OK;
        } else {
            p.error_action = if o.p5 == OE_REPLACE as u16 { OE_ABORT } else { o.p5 as u8 };
        }
    } else {
        p.n_change = p.n_change.wrapping_add(1);
    }
    if rc != SQLITE_OK {
        return st.abort(rc);
    }
    Flow::Continue(pc as i32)
}

// ---------------------------------------------------------------------------------------------
// chunk 021 e 022: Function, Trace, Init
// ---------------------------------------------------------------------------------------------

/// `OP_Function` e `OP_PureFunc`: chama a função SQL do P4 (um `FuncCtx`) com os argumentos a
/// partir de r[P2] e grava o resultado em r[P3]. P1 é a máscara de argumentos constantes.
fn op_function(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    let fctx: Rc<FuncCtx> = match &cur_ops(p)[pc].p4 {
        P4::FuncCtx(c) => Rc::clone(c),
        _ => return st.abort(SQLITE_CORRUPT_BKPT),
    };
    let out_idx = o.p3 as usize;
    let first = o.p2.max(0) as usize;
    let argc = fctx.argc as usize;
    // A célula de saída viaja no contexto durante a chamada e volta ao registro depois.
    let mut out = std::mem::take(&mut p.a_mem[out_idx]);
    out.set_type_flag(MEM_NULL);
    let (out, is_error) = {
        let p_coll = prev_coll(p, pc);
        let mut ctx = Context {
            db: &mut *db,
            p_coll,
            p_aux_data: &mut p.p_aux_data,
            i_current_time: &mut p.i_current_time,
            out,
            arg_func: Rc::clone(&fctx.p_func),
            agg: None,
            i_op: pc as i32,
            is_pure_func: o.opcode == OP_PUREFUNC,
            is_error: 0,
            enc: st.encoding,
            skip_flag: 0,
            argc: fctx.argc,
        };
        if let Some(f) = fctx.p_func.x_s_func {
            f(&mut ctx, &p.a_mem[first..first + argc]);
        }
        (ctx.out, ctx.is_error)
    };
    p.a_mem[out_idx] = out;
    // Se a função devolveu um erro, lança a exceção.
    if is_error != 0 {
        let mut rc = SQLITE_OK;
        if is_error > 0 {
            let arg = text_arg(&mut p.a_mem[out_idx]);
            vdbe_error(p, db, b"%s", &[arg]);
            rc = is_error;
        }
        delete_aux_data(&mut p.p_aux_data, pc as i32, o.p1);
        if rc != SQLITE_OK {
            return st.abort(rc);
        }
    }
    Flow::Continue(pc as i32)
}

/// `OP_Trace` e `OP_Init`: emite o texto do P4 (ou o SQL) no rastreamento do comando e, no
/// `OP_Init`, incrementa P1 (para o `OP_Once` saltar na primeira vez) e salta para P2.
fn op_init(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: usize) -> Flow {
    let o = st.op;
    if ((db.m_trace as u32) & (SQLITE_TRACE_STMT | SQLITE_TRACE_LEGACY as u32)) != 0
        && p.min_write_file_format != 254
    {
        let z_trace = p4_z(&cur_ops(p)[pc]).map(<[u8]>::to_vec).or_else(|| p.z_sql.clone());
        if let Some(z_trace) = z_trace {
            let sql = if (db.m_trace & SQLITE_TRACE_LEGACY) != 0 {
                crate::vdbetrace::expand_sql(db, p, &z_trace)
            } else if db.n_vdbe_exec > 1 {
                mprintf(b"-- %s", &[PrintfArg::Text(Some(z_trace))])
            } else {
                Some(z_trace)
            };
            if let Some(sql) = sql {
                if let Some(mut f) = db.x_trace.take() {
                    f(&TraceEvent::Stmt { stmt: p.stmt_id, sql });
                    if db.x_trace.is_none() {
                        db.x_trace = Some(f);
                    }
                }
            }
        }
    }
    debug_assert!(o.p2 > 0);
    if o.p1 >= ONCE_RESET_THRESHOLD {
        if o.opcode == OP_TRACE {
            return Flow::Continue(pc as i32);
        }
        if p.p_cur_prog.is_none() {
            for op in p.a_op.iter_mut().skip(1) {
                if op.opcode == OP_ONCE {
                    op.p1 = 0;
                }
            }
            p.a_op[pc].p1 = 0;
        }
    }
    if p.p_cur_prog.is_none() {
        let op: &mut Op = &mut p.a_op[pc];
        op.p1 = op.p1.wrapping_add(1);
    }
    let k = SQLITE_STMTSTATUS_RUN as usize;
    p.a_counter[k] = p.a_counter[k].wrapping_add(1);
    Flow::Jump(o.p2)
}

/// O `pOp[-1].p4.pColl` quando o opcode anterior é `OP_CollSeq`: a colação que `sqlite3_context`
/// guarda em `pColl` (`sqlite3_value_collation`/`sqlite3GetFuncCollSeq`).
fn prev_coll(p: &Vdbe, pc: usize) -> Option<Rc<crate::mem::CollSeq>> {
    let prev = cur_ops(p).get(pc.checked_sub(1)?)?;
    match &prev.p4 {
        P4::Coll(Some(c)) if prev.opcode == OP_COLLSEQ => Some(Rc::clone(c)),
        _ => None,
    }
}
