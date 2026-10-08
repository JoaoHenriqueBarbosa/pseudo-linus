//! `vdbe.c` (parte 1): trechos 000 a 004 do `vdbe.c` do SQLite 3.46.1. Cobre os auxiliares
//! estáticos (`allocateCursor`, `out2Prerelease`, `filterHash`, `vdbeColumnFromOverflow`,
//! `vdbeMemTypeName`), o início de `sqlite3VdbeExec` (estado local, preparação, laço de opcodes,
//! saída por `abort_due_to_error`/`vdbe_return`/`too_big`/`no_mem`/`abort_due_to_interrupt`) e os
//! opcodes que aparecem nesses trechos: `Goto`, `Gosub`, `Return`, `InitCoroutine`,
//! `EndCoroutine`, `Yield`, `HaltIfNull`, `Halt`, `Integer`, `Int64`, `Real`, `String8`,
//! `String`, `BeginSubrtn`, `Null`, `SoftNull`, `Blob`, `Variable`, `Move`, `Copy`, `SCopy`,
//! `IntCopy`, `FkCheck`, `ResultRow`, `Concat`, `Add`, `Subtract`, `Multiply`, `Divide` e
//! `Remainder`. Os demais opcodes ficam em `crate::vdbe_ops::exec_op`, que recebe o estado
//! definido aqui ([`ExecState`]) e devolve um [`Flow`].
//!
//! Já existem em `mem.rs` e NÃO foram repetidos: `alsoAnInt`, `applyNumericAffinity`,
//! `applyAffinity` (e `sqlite3ValueApplyAffinity`), `computeNumericType`, `numericType` e
//! `sqlite3_value_numeric_type`.
//!
//! Desvios do C, todos decorrentes do modelo v2 (ver `vdbe_types.rs`, `vdbeaux2.rs` e
//! `CONVENTIONS.md`):
//!
//! * `sqlite3VdbeExec(p)` recebe também a conexão: `vdbe_exec(db, p)`. Quem chama já retirou o
//!   `Vdbe` do slab (`Connection.stmts.take`), por isso `(db, p)` não se sobrepõem.
//! * `pOp`, `aOp` e `aMem` não são ponteiros locais: o laço guarda o contador de programa como
//!   `i32` (o C afirma `pOp >= &aOp[-1]`, então `-1` existe) e relê o opcode a cada passo. O
//!   programa corrente é `Vdbe.p_cur_prog` (`None` é `Vdbe.a_op`), que `OP_Program` e a volta do
//!   frame trocam; `aMem` é sempre `Vdbe.a_mem`. Os operandos escalares do opcode (`p1`, `p2`,
//!   `p3`, `p5`) são copiados para `ExecState.op`; o `p4` é lido sob demanda por [`cur_ops`].
//! * `pIn1`, `pIn2`, `pIn3` e `pOut` são índices em `a_mem` (o aliasing do C entre eles vira
//!   [`two_mut`] ou cópia prévia do que for lido).
//! * Os `goto` do `switch` viram o valor devolvido ([`Flow`]). A saída do laço
//!   (`abort_due_to_error`, `vdbe_return`, `too_big`, `no_mem`, `abort_due_to_interrupt`) fica em
//!   [`vdbe_tail`], aqui, porque é parte da estrutura do laço.
//! * `OP_String8` só reescreve a si mesmo (opcode `OP_String` e `p1`) no programa principal: um
//!   subprograma é um `Rc<SubProgram>` imutável, então ali a transformação é refeita a cada
//!   execução (mesmo resultado, só mais lenta).
//! * `OP_Halt` de subprograma libera o frame na hora (`frame_delete`): como o frame é novo a
//!   cada `OP_Program`, não há o que reaproveitar.
//! * Os ciclos do `SQLITE_ENABLE_STMT_SCANSTATUS` só são contados nos opcodes do programa
//!   principal (o `aScan` só se refere a ele). `sqlite3Hwtime` é um contador monotônico em
//!   nanossegundos.
//! * `sqlite3VdbeEnter`/`Leave` não existem (ver `vdbeaux2.rs`). Somem os ramos
//!   `SQLITE_DEBUG`, `SQLITE_TEST`, `VDBE_PROFILE`, `SQLITE_VDBE_COVERAGE` e `memAboutToChange`.
//! * O `RCStr` do cache de TEXT e BLOB grandes de `vdbeColumnFromOverflow` é o `Vec<u8>` do
//!   `VdbeTxtBlbCache`; a célula de destino recebe uma cópia.

use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::btree_cursor::{btree_max_record_size, btree_offset, btree_payload};
use crate::build::reset_one_schema;
use crate::connection::{Connection, TraceEvent};
use crate::consts::{
    CURTYPE_BTREE, LARGEST_UINT64, MEM_BLOB, MEM_CLEARED, MEM_DYN,
    MEM_EPHEM, MEM_FROMBIND, MEM_INT, MEM_INTREAL, MEM_NULL, MEM_REAL, MEM_STATIC, MEM_STR,
    MEM_SUBTYPE, MEM_TERM, MEM_ZERO, OE_IGNORE, OP_ADD, OP_BEGINSUBRTN, OP_BLOB, OP_CONCAT,
    OP_COPY, OP_DIVIDE, OP_ENDCOROUTINE, OP_FKCHECK, OP_GOSUB, OP_GOTO, OP_HALT, OP_HALTIFNULL,
    OP_INITCOROUTINE, OP_INT64, OP_INTCOPY, OP_INTEGER, OP_MOVE, OP_MULTIPLY, OP_NULL, OP_REAL,
    OP_REMAINDER, OP_RESULTROW, OP_RETURN, OP_SCOPY, OP_SOFTNULL, OP_STRING, OP_STRING8,
    OP_SUBTRACT, OP_VARIABLE, OP_YIELD, SMALLEST_INT64, SQLITE_BUSY, SQLITE_CORRUPT,
    SQLITE_CORRUPT_BKPT, SQLITE_CORRUPT_RD_ONLY, SQLITE_DONE, SQLITE_ERROR,
    SQLITE_IOERR_CORRUPTFS, SQLITE_IOERR_NOMEM, SQLITE_LIMIT_LENGTH, SQLITE_NOMEM,
    SQLITE_NOMEM_BKPT, SQLITE_OK, SQLITE_ROW, SQLITE_STMT_SCAN_STATUS,
    SQLITE_STMTSTATUS_VM_STEP, SQLITE_TOOBIG, SQLITE_TRACE_ROW, VDBE_RUN_STATE,
};
use crate::global::log;
use crate::mem::{
    mem_expand_blob, mem_grow, mem_make_writeable, mem_move, mem_set_int64, mem_set_null,
    mem_set_str, mem_set_zero_blob, mem_shallow_copy, mem_stringify, mem_too_big, numeric_type,
    value_type, vdbe_change_encoding, vdbe_int_value, vdbe_real_value, Mem, StrDtor, ENC_UTF8,
};
use crate::printf::{vm_printf, PrintfArg};
use crate::record::{serial_get, serial_type_len};
use crate::util::{add_int64, err_str, is_nan, mul_int64, strlen30, sub_int64};
use crate::vdbe_types::{Op, SubProgram, Vdbe, VdbeCursor, VdbeTxtBlbCache, P4};
use crate::vdbeaux::vdbe_error;
use crate::vdbeaux2::{
    check_fk, frame_delete, frame_restore, free_cursor_nn, vdbe_halt, with_btree_cursor,
};
use crate::vdbeaux3::vdbe_set_changes;

// ---------------------------------------------------------------------------------------------
// Estado local do laço e fluxo de controle (usados também por `crate::vdbe_ops`)
// ---------------------------------------------------------------------------------------------

/// Os operandos escalares da instrução em execução (a cópia de `*pOp` que não carrega o `p4`).
#[derive(Debug, Clone, Copy, Default)]
pub struct OpRegs {
    /// `pOp->opcode`.
    pub opcode: u8,
    /// `pOp->p1`.
    pub p1: i32,
    /// `pOp->p2`.
    pub p2: i32,
    /// `pOp->p3`.
    pub p3: i32,
    /// `pOp->p5`.
    pub p5: u16,
}

/// O estado local de `sqlite3VdbeExec` que sobrevive entre instruções e que os opcodes de
/// `crate::vdbe_ops` precisam ler ou gravar. O contador de programa (`pOp - aOp`) é passado à
/// parte, como `pc`.
#[derive(Debug, Default)]
pub struct ExecState {
    /// A instrução corrente: copiada de `cur_ops(p)[pc]` antes de cada passo.
    pub op: OpRegs,
    /// O `rc` local: o código a devolver. Um opcode que faz `rc = X; goto abort_due_to_error` grava
    /// `X` aqui e devolve [`Flow::AbortDueToError`] (ver [`ExecState::abort`]).
    pub rc: i32,
    /// `resetSchemaOnFault`: depois de um erro, `sqlite3ResetOneSchema(db, valor - 1)` se positivo.
    pub reset_schema_on_fault: u8,
    /// `encoding`: a codificação do banco (`ENC(db)`).
    pub encoding: u8,
    /// `iCompare`: o resultado da última comparação (`OP_Compare`, lido por `OP_Jump`).
    pub i_compare: i32,
    /// `nVmStep`: instruções executadas nesta chamada.
    pub n_vm_step: u64,
    /// `nProgressLimit`: o `xProgress` roda quando `n_vm_step` chega aqui.
    pub n_progress_limit: u64,
    /// `colCacheCtr`: contador do cache de colunas grandes (`OP_Column`).
    pub col_cache_ctr: u32,
    /// `bStmtScanStatus`: `IS_STMT_SCANSTATUS(db)`.
    pub b_stmt_scan_status: bool,
    /// `pnCycle`: índice, em `Vdbe.a_op`, do opcode cujo `n_cycle` está sendo medido.
    pub pn_cycle: Option<usize>,
}

impl ExecState {
    /// `rc = valor; goto abort_due_to_error`.
    #[inline]
    pub fn abort(&mut self, rc: i32) -> Flow {
        self.rc = rc;
        Flow::AbortDueToError
    }
}

/// O destino de uma instrução: o que o `switch` do C faz com `pOp` e com os `goto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// O `break` do `switch` com `pOp == &aOp[pc]`: a próxima instrução executada é `pc + 1`.
    /// Para seguir adiante sem mudar nada, `Continue(pc)`. O C que faz `pOp = &aOp[X]; break;`
    /// vira `Continue(X)`; `pOp = &aOp[p2 - 1]; break;` vira `Continue(p2 - 1)` (ou `Jump(p2)`).
    Continue(i32),
    /// `goto jump_to_p2` com o destino já resolvido: a próxima instrução executada é `dest`,
    /// sem checar interrupção nem progresso.
    Jump(i32),
    /// `goto jump_to_p2_and_check_for_interrupt` (e `goto check_for_interrupt` depois de um
    /// `pOp = &aOp[dest - 1]`): confere `isInterrupted` e o `xProgress` e então vai para `dest`.
    JumpCheck(i32),
    /// `goto abort_due_to_error`, com o código em [`ExecState::rc`].
    AbortDueToError,
    /// `goto no_mem`.
    NoMem,
    /// `goto too_big`.
    TooBig,
    /// `goto abort_due_to_interrupt`.
    Interrupt,
    /// `goto vdbe_return` com o `rc` dado (`SQLITE_ROW`, `SQLITE_DONE`, `SQLITE_BUSY`...).
    Return(i32),
}

/// O programa corrente: `Vdbe.p_cur_prog` (um subprograma de `OP_Program`) ou `Vdbe.a_op`.
#[inline]
pub fn cur_ops(p: &Vdbe) -> &[Op] {
    prog_ops(&p.p_cur_prog, &p.a_op)
}

/// O `Op::p4.z` de um opcode de texto: o texto até o primeiro NUL; `None` é o ponteiro nulo.
pub fn p4_z(op: &Op) -> Option<&[u8]> {
    match &op.p4 {
        P4::Text(z) | P4::Blob(z) => Some(&z[..z.iter().position(|&c| c == 0).unwrap_or(z.len())]),
        _ => None,
    }
}

/// Duas células distintas do mesmo vetor, `(a[i], a[j])`, para os opcodes que leem uma e
/// gravam em outra (`pIn1` e `pOut`).
pub fn two_mut<T>(a: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    debug_assert!(i != j);
    if i < j {
        let (l, r) = a.split_at_mut(j);
        (&mut l[i], &mut r[0])
    } else {
        let (l, r) = a.split_at_mut(i);
        (&mut r[0], &mut l[j])
    }
}

/// `Deephemeralize(P)`: converte uma string efêmera em string própria da célula. Devolve `true`
/// se faltou memória (o `goto no_mem` do C).
#[inline]
pub fn deephemeralize(m: &mut Mem) -> bool {
    (m.flags & MEM_EPHEM) != 0 && mem_make_writeable(m) != SQLITE_OK
}

/// `sqlite3Hwtime`: um contador monotônico em nanossegundos.
fn hwtime() -> u64 {
    thread_local! {
        static T0: Instant = Instant::now();
    }
    T0.with(|t| t.elapsed().as_nanos() as u64)
}

// ---------------------------------------------------------------------------------------------
// chunk 000: allocateCursor
// ---------------------------------------------------------------------------------------------

/// `isSorter(x)`.
#[inline]
pub fn is_sorter(x: &VdbeCursor) -> bool {
    x.e_cur_type == crate::consts::CURTYPE_SORTER
}

/// `allocateCursor`: cria o cursor de número `i_cur`, com `n_field` campos, e o põe em
/// `p.ap_csr[i_cur]` (fechando o que estivesse lá). `None` é a falta de memória (o `return 0` do
/// C). O C usava a célula `aMem[nMem-iCur]` como depósito do espaço do cursor; aqui o cursor é um
/// `Box` e a célula fica como está. Um cursor `CURTYPE_BTREE` nasce sem `p_cursor`: quem o abre
/// (`OP_OpenRead` e companhia) cria o `BtCursor` no slab do `BtShared` e grava o `CursorId`.
pub fn allocate_cursor<'a>(
    db: &mut Connection,
    p: &'a mut Vdbe,
    i_cur: i32,
    n_field: i32,
    e_cur_type: u8,
) -> Option<&'a mut VdbeCursor> {
    debug_assert!(i_cur >= 0 && i_cur < p.n_cursor);
    let idx = i_cur as usize;
    if let Some(old) = p.ap_csr[idx].take() {
        free_cursor_nn(db, old);
    }
    let nf = n_field.max(0) as usize;
    let mut a_type: Vec<u32> = Vec::new();
    let mut a_offset: Vec<u32> = Vec::new();
    if a_type.try_reserve_exact(nf).is_err() || a_offset.try_reserve_exact(nf + 1).is_err() {
        return None;
    }
    a_type.resize(nf, 0);
    a_offset.resize(nf + 1, 0);
    let cx = VdbeCursor {
        e_cur_type,
        n_field: n_field as i16,
        a_type,
        a_offset,
        ..VdbeCursor::default()
    };
    Some(&mut **p.ap_csr[idx].insert(Box::new(cx)))
}

// ---------------------------------------------------------------------------------------------
// chunk 002: vdbeMemTypeName; chunk 001 (final): out2Prerelease, filterHash
// ---------------------------------------------------------------------------------------------

/// `vdbeMemTypeName`: o nome simbólico do tipo de dados da célula.
pub fn vdbe_mem_type_name(m: &Mem) -> &'static [u8] {
    const AZ_TYPES: [&[u8]; 5] = [b"INT", b"REAL", b"TEXT", b"BLOB", b"NULL"];
    AZ_TYPES[(value_type(m) - 1).clamp(0, 4) as usize]
}

/// `out2Prerelease`: a célula `p2` (o registro de saída do opcode) já preparada para receber um
/// inteiro (`MEM_Int`). `a_mem` é `Vdbe.a_mem`.
pub fn out2_prerelease(a_mem: &mut [Mem], p2: i32) -> &mut Mem {
    debug_assert!(p2 > 0);
    let out = &mut a_mem[p2 as usize];
    if out.is_dynamic() {
        // out2PrereleaseWithClear
        mem_set_null(out);
    }
    out.flags = MEM_INT;
    out
}

/// `filterHash`: o hash do filtro de Bloom sobre `p4.i` registros de `a_mem` a partir de `p3`.
pub fn filter_hash(a_mem: &[Mem], op: &Op) -> u64 {
    let n = match op.p4 {
        P4::Int32(n) => n,
        _ => 0,
    };
    let mut h: u64 = 0;
    let mut i = op.p3;
    let mx = i + n;
    while i < mx {
        let m = &a_mem[i as usize];
        if (m.flags & (MEM_INT | MEM_INTREAL)) != 0 {
            h = h.wrapping_add(m.u_i as u64);
        } else if (m.flags & MEM_REAL) != 0 {
            h = h.wrapping_add(vdbe_int_value(m) as u64);
        } else if (m.flags & (MEM_STR | MEM_BLOB)) != 0 {
            // Todas as strings têm o mesmo hash e todos os blobs têm o mesmo hash, mas os dois
            // diferem entre si e de NULL.
            h = h.wrapping_add(4093 + (m.flags & (MEM_STR | MEM_BLOB)) as u64);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------------------------
// chunk 001 (final): vdbeColumnFromOverflow
// ---------------------------------------------------------------------------------------------

/// `vdbeColumnFromOverflow`: o caminho de `OP_Column` em que o conteúdo está em páginas de
/// overflow. Lê a coluna `i_col` (tipo serial `t`, a partir de `i_offset` do payload) do cursor
/// `p_c` e a grava em `p_dest`. A codificação do texto é a de `p_dest.enc`.
#[allow(clippy::too_many_arguments)]
pub fn vdbe_column_from_overflow(
    db: &mut Connection,
    p_c: &mut VdbeCursor,
    i_col: i32,
    t: i32,
    i_offset: i64,
    cache_status: u32,
    col_cache_ctr: u32,
    p_dest: &mut Mem,
) -> i32 {
    let encoding = p_dest.enc;
    let len = serial_type_len(t as u32) as i32;
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    debug_assert!(p_c.e_cur_type == CURTYPE_BTREE);
    if len > limit {
        return SQLITE_TOOBIG;
    }
    if len > 4000 && p_c.p_key_info.is_none() {
        // Guarda em cache os valores grandes das páginas de overflow, para que uma releitura
        // não os copie de novo. Só em btrees de tabela, para que a escrita em btrees de índice
        // não precise limpar o cache.
        if !p_c.col_cache {
            p_c.p_cache = Some(Box::<VdbeTxtBlbCache>::default());
            p_c.col_cache = true;
        }
        let Some(cur_off) = with_btree_cursor(db, p_c, |cur, bt| btree_offset(cur, bt)) else {
            return SQLITE_ERROR;
        };
        let stale = match p_c.p_cache.as_deref() {
            Some(c) => {
                c.p_c_value.is_empty()
                    || c.i_col != i_col
                    || c.cache_status != cache_status
                    || c.col_cache_ctr != col_cache_ctr
                    || c.i_offset != cur_off
            }
            None => true,
        };
        if stale {
            let mut buf: Vec<u8> = Vec::new();
            if buf.try_reserve_exact(len as usize + 3).is_err() {
                return SQLITE_NOMEM;
            }
            buf.resize(len as usize + 3, 0);
            let rc = with_btree_cursor(db, p_c, |cur, bt| {
                btree_payload(cur, bt, i_offset as u32, len as u32, &mut buf[..len as usize])
            })
            .unwrap_or(SQLITE_ERROR);
            let cache = p_c.p_cache.get_or_insert_with(Default::default);
            cache.p_c_value = buf;
            if rc != SQLITE_OK {
                return rc;
            }
            cache.i_col = i_col;
            cache.cache_status = cache_status;
            cache.col_cache_ctr = col_cache_ctr;
            cache.i_offset = cur_off;
        }
        debug_assert!(t >= 12);
        let cache = p_c.p_cache.get_or_insert_with(Default::default);
        let rc;
        if (t & 1) != 0 {
            rc = mem_set_str(
                p_dest,
                Some(&cache.p_c_value[..]),
                len as i64,
                encoding,
                StrDtor::Func,
                limit,
            );
            p_dest.flags |= MEM_TERM;
        } else {
            rc = mem_set_str(
                p_dest,
                Some(&cache.p_c_value[..]),
                len as i64,
                0,
                StrDtor::Func,
                limit,
            );
        }
        p_dest.flags &= !MEM_EPHEM;
        return rc;
    }
    let rc = with_btree_cursor(db, p_c, |cur, bt| {
        let mrs = btree_max_record_size(bt).clamp(0, u32::MAX as i64) as u32;
        crate::mem::mem_from_btree(p_dest, mrs, i_offset as u32, len as u32, |buf| {
            btree_payload(cur, bt, i_offset as u32, len as u32, buf)
        })
    })
    .unwrap_or(SQLITE_ERROR);
    if rc != SQLITE_OK {
        return rc;
    }
    // O `sqlite3VdbeSerialGet` do C lê do próprio `pDest->z` e o aponta para dentro dele.
    let z = std::mem::take(&mut p_dest.z);
    serial_get(&z, t as u32, p_dest);
    if (t & 1) != 0 && encoding == ENC_UTF8 {
        // O terminador (o C o grava em `z[len]`, no byte a mais que a leitura reservou).
        p_dest.z.push(0);
        p_dest.flags |= MEM_TERM;
    }
    p_dest.flags &= !MEM_EPHEM;
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 002 a 004: sqlite3VdbeExec
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeExec`: executa o bytecode do comando até produzir uma linha (`SQLITE_ROW`),
/// terminar (`SQLITE_DONE`), esbarrar em `SQLITE_BUSY` ou falhar (`SQLITE_ERROR`; o código
/// específico fica em `Vdbe.rc`). É o miolo do `sqlite3_step()`.
pub fn vdbe_exec(db: &mut Connection, p: &mut Vdbe) -> i32 {
    let mut st = ExecState {
        encoding: db.enc,
        b_stmt_scan_status: (db.flags & SQLITE_STMT_SCAN_STATUS) != 0,
        ..ExecState::default()
    };
    // O `pOp` inicial do C é `aOp`.
    let mut pc: i32 = 0;
    debug_assert!(p.e_vdbe_state == VDBE_RUN_STATE);
    if db.x_progress.is_some() {
        let n_ops = db.n_progress_ops.max(1);
        let i_prior = p.a_counter[SQLITE_STMTSTATUS_VM_STEP as usize];
        st.n_progress_limit = (n_ops - (i_prior % n_ops)) as u64;
    } else {
        st.n_progress_limit = LARGEST_UINT64;
    }
    if p.rc == SQLITE_NOMEM {
        // Acontece se um malloc() dentro de sqlite3_column_text() ou _text16() falhou.
        return vdbe_tail(db, p, &mut st, pc, Flow::NoMem);
    }
    debug_assert!(p.rc == SQLITE_OK || (p.rc & 0xff) == SQLITE_BUSY);
    p.rc = SQLITE_OK;
    debug_assert!(p.b_is_reader || p.read_only);
    p.i_current_time = 0;
    debug_assert!(p.explain == 0);
    db.busy_handler.n_busy.set(0);
    if db.interrupted.load(Ordering::Relaxed) {
        return vdbe_tail(db, p, &mut st, pc, Flow::Interrupt);
    }
    pc = p.pc;
    loop {
        // Os erros são detectados pelos opcodes, com salto imediato para abort_due_to_error.
        debug_assert!(st.rc == SQLITE_OK);
        let Some(cur) = usize::try_from(pc).ok().and_then(|i| cur_ops(p).get(i)) else {
            // Endereço fora do programa: o C assume que o gerador de código nunca produz isso.
            st.rc = SQLITE_CORRUPT_BKPT;
            return vdbe_tail(db, p, &mut st, pc, Flow::AbortDueToError);
        };
        st.n_vm_step += 1;
        st.op = OpRegs { opcode: cur.opcode, p1: cur.p1, p2: cur.p2, p3: cur.p3, p5: cur.p5 };
        if st.b_stmt_scan_status && p.p_cur_prog.is_none() {
            let t = hwtime();
            let op = &mut p.a_op[pc as usize];
            op.n_exec += 1;
            op.n_cycle = op.n_cycle.wrapping_sub(t);
            st.pn_cycle = Some(pc as usize);
        }

        let o = st.op;
        let pcu = pc as usize;
        let flow = match o.opcode {
            // Opcode: Goto * P2 * * *: salto incondicional para P2.
            OP_GOTO => Flow::JumpCheck(o.p2),

            // Opcode: Gosub P1 P2 * * *: grava o endereço atual em P1 e salta para P2.
            OP_GOSUB => {
                let m = &mut p.a_mem[o.p1 as usize];
                debug_assert!(!m.is_dynamic());
                m.flags = MEM_INT;
                m.u_i = pc as i64;
                Flow::JumpCheck(o.p2)
            }

            // Opcode: Return P1 P2 P3 * *: salta para o endereço guardado no registro P1. Com P3
            // igual a 1 só salta se P1 guarda um inteiro (par do OP_BeginSubrtn).
            OP_RETURN => {
                let m = &p.a_mem[o.p1 as usize];
                if (m.flags & MEM_INT) != 0 {
                    Flow::Continue(m.u_i as i32)
                } else {
                    Flow::Continue(pc)
                }
            }

            // Opcode: InitCoroutine P1 P2 P3 * *
            OP_INITCOROUTINE => {
                debug_assert!(o.p2 >= 0 && (o.p2 as usize) < cur_ops(p).len());
                debug_assert!(o.p3 >= 0 && (o.p3 as usize) < cur_ops(p).len());
                let m = &mut p.a_mem[o.p1 as usize];
                debug_assert!(!m.is_dynamic());
                m.u_i = (o.p3 - 1) as i64;
                m.flags = MEM_INT;
                if o.p2 == 0 {
                    Flow::Continue(pc)
                } else {
                    // jump_to_p2
                    Flow::Jump(o.p2)
                }
            }

            // Opcode: EndCoroutine P1 * * * *
            OP_ENDCOROUTINE => {
                let caller = p.a_mem[o.p1 as usize].u_i;
                debug_assert!(p.a_mem[o.p1 as usize].flags == MEM_INT);
                let caller_p2 = cur_ops(p).get(caller as usize).map_or(0, |c| c.p2);
                p.a_mem[o.p1 as usize].u_i = (pc - 1) as i64;
                Flow::Continue(caller_p2 - 1)
            }

            // Opcode: Yield P1 P2 * * *: troca o contador de programa com o registro P1.
            OP_YIELD => {
                let m = &mut p.a_mem[o.p1 as usize];
                debug_assert!(!m.is_dynamic());
                m.flags = MEM_INT;
                let pc_dest = m.u_i as i32;
                m.u_i = pc as i64;
                Flow::Continue(pc_dest)
            }

            // Opcode: HaltIfNull P1 P2 P3 P4 P5: se r[P3] é NULL, cai no OP_Halt.
            OP_HALTIFNULL => {
                if (p.a_mem[o.p3 as usize].flags & MEM_NULL) == 0 {
                    Flow::Continue(pc)
                } else {
                    op_halt(db, p, &mut st, pc)
                }
            }
            OP_HALT => op_halt(db, p, &mut st, pc),

            // Opcode: Integer P1 P2 * * *: r[P2]=P1.
            OP_INTEGER => {
                out2_prerelease(&mut p.a_mem, o.p2).u_i = o.p1 as i64;
                Flow::Continue(pc)
            }

            // Opcode: Int64 * P2 * P4 *: r[P2]=P4.
            OP_INT64 => {
                let v = match &cur_ops(p)[pcu].p4 {
                    P4::Int64(v) => *v,
                    _ => 0,
                };
                out2_prerelease(&mut p.a_mem, o.p2).u_i = v;
                Flow::Continue(pc)
            }

            // Opcode: Real * P2 * P4 *: r[P2]=P4.
            OP_REAL => {
                let v = match &cur_ops(p)[pcu].p4 {
                    P4::Real(v) => *v,
                    _ => 0.0,
                };
                debug_assert!(!is_nan(v));
                let out = out2_prerelease(&mut p.a_mem, o.p2);
                out.flags = MEM_REAL;
                out.u_r = v;
                Flow::Continue(pc)
            }

            // Opcode: String8 * P2 * P4 *: vira OP_String na primeira execução.
            OP_STRING8 => op_string8(db, p, &mut st, pc),

            // Opcode: String P1 P2 P3 P4 P5: r[P2]='P4' (len=P1).
            OP_STRING => {
                let ops = prog_ops(&p.p_cur_prog, &p.a_op);
                let z: &[u8] = match &ops[pcu].p4 {
                    P4::Text(z) | P4::Blob(z) => z,
                    _ => &[],
                };
                exec_string(&mut p.a_mem, &st, o.p1, z);
                Flow::Continue(pc)
            }

            // Opcode: BeginSubrtn * P2 * * *  e  Opcode: Null P1 P2 P3 * *: r[P2..P3]=NULL.
            OP_BEGINSUBRTN | OP_NULL => {
                let cnt = o.p3 - o.p2;
                let null_flag = if o.p1 != 0 { MEM_NULL | MEM_CLEARED } else { MEM_NULL };
                let out = out2_prerelease(&mut p.a_mem, o.p2);
                out.flags = null_flag;
                out.n = 0;
                let mut i = o.p2 as usize;
                for _ in 0..cnt.max(0) {
                    i += 1;
                    let c = &mut p.a_mem[i];
                    mem_set_null(c);
                    c.flags = null_flag;
                    c.n = 0;
                }
                Flow::Continue(pc)
            }

            // Opcode: SoftNull P1 * * * *: r[P1]=NULL sem liberar o texto ou blob.
            OP_SOFTNULL => {
                let out = &mut p.a_mem[o.p1 as usize];
                out.flags = (out.flags & !(crate::consts::MEM_UNDEFINED | crate::consts::MEM_AFFMASK))
                    | MEM_NULL;
                Flow::Continue(pc)
            }

            // Opcode: Blob P1 P2 * P4 *: r[P2]=P4 (len=P1).
            OP_BLOB => {
                let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
                debug_assert!(o.p1 <= crate::consts::SQLITE_MAX_LENGTH);
                let ops = prog_ops(&p.p_cur_prog, &p.a_op);
                let out = out2_prerelease(&mut p.a_mem, o.p2);
                match &ops[pcu].p4 {
                    P4::Blob(z) | P4::Text(z) => {
                        let take = (o.p1.max(0) as usize).min(z.len());
                        // O C ignora o código (`SQLITE_TOOBIG` deixaria a célula NULL).
                        let _ = mem_set_str(out, Some(&z[..take]), o.p1 as i64, 0, StrDtor::Static, limit);
                        out.enc = st.encoding;
                        Flow::Continue(pc)
                    }
                    _ => {
                        mem_set_zero_blob(out, o.p1);
                        if mem_expand_blob(out) != SQLITE_OK {
                            Flow::NoMem
                        } else {
                            out.enc = st.encoding;
                            Flow::Continue(pc)
                        }
                    }
                }
            }

            // Opcode: Variable P1 P2 * * *: r[P2]=parameter(P1).
            OP_VARIABLE => {
                let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
                debug_assert!(o.p1 > 0 && o.p1 <= p.n_var);
                let var = &p.a_var[(o.p1 - 1) as usize];
                if mem_too_big(var, limit) {
                    Flow::TooBig
                } else {
                    let out = &mut p.a_mem[o.p2 as usize];
                    if out.is_dynamic() {
                        mem_set_null(out);
                    }
                    // memcpy(pOut, pVar, MEMCELLSIZE)
                    mem_shallow_copy(out, var, MEM_STATIC);
                    out.flags &= !(MEM_DYN | MEM_EPHEM);
                    out.flags |= MEM_STATIC | MEM_FROMBIND;
                    Flow::Continue(pc)
                }
            }

            // Opcode: Move P1 P2 P3 * *: r[P2@P3]=r[P1@P3], deixando NULL nos de origem.
            OP_MOVE => {
                let mut n = o.p3;
                let mut i1 = o.p1 as usize;
                let mut i2 = o.p2 as usize;
                debug_assert!(n > 0 && o.p1 > 0 && o.p2 > 0);
                let mut flow = Flow::Continue(pc);
                loop {
                    let (out, inn) = two_mut(&mut p.a_mem, i2, i1);
                    mem_move(out, inn);
                    if deephemeralize(out) {
                        flow = Flow::NoMem;
                        break;
                    }
                    i1 += 1;
                    i2 += 1;
                    n -= 1;
                    if n <= 0 {
                        break;
                    }
                }
                flow
            }

            // Opcode: Copy P1 P2 P3 * P5: r[P2@P3+1]=r[P1@P3+1] (cópia profunda).
            OP_COPY => {
                let mut n = o.p3;
                let mut i1 = o.p1 as usize;
                let mut i2 = o.p2 as usize;
                let mut flow = Flow::Continue(pc);
                loop {
                    let (out, inn) = two_mut(&mut p.a_mem, i2, i1);
                    mem_shallow_copy(out, inn, MEM_EPHEM);
                    if deephemeralize(out) {
                        flow = Flow::NoMem;
                        break;
                    }
                    if (out.flags & MEM_SUBTYPE) != 0 && (o.p5 & 0x0002) != 0 {
                        out.flags &= !MEM_SUBTYPE;
                    }
                    if n == 0 {
                        break;
                    }
                    n -= 1;
                    i1 += 1;
                    i2 += 1;
                }
                flow
            }

            // Opcode: SCopy P1 P2 * * *: cópia rasa de r[P1] em r[P2].
            OP_SCOPY => {
                let (out, inn) = two_mut(&mut p.a_mem, o.p2 as usize, o.p1 as usize);
                mem_shallow_copy(out, inn, MEM_EPHEM);
                Flow::Continue(pc)
            }

            // Opcode: IntCopy P1 P2 * * *: transfere o inteiro de r[P1] para r[P2].
            OP_INTCOPY => {
                debug_assert!((p.a_mem[o.p1 as usize].flags & MEM_INT) != 0);
                let v = p.a_mem[o.p1 as usize].u_i;
                mem_set_int64(&mut p.a_mem[o.p2 as usize], v);
                Flow::Continue(pc)
            }

            // Opcode: FkCheck * * * * *: falha se há violações de chave estrangeira.
            OP_FKCHECK => {
                let rc = check_fk(p, db, false);
                if rc != SQLITE_OK {
                    st.abort(rc)
                } else {
                    Flow::Continue(pc)
                }
            }

            // Opcode: ResultRow P1 P2 * * *: output=r[P1@P2].
            OP_RESULTROW => {
                debug_assert!(p.n_res_column as i32 == o.p2);
                debug_assert!(o.p1 > 0);
                p.cache_ctr = p.cache_ctr.wrapping_add(2) | 1;
                p.p_result_row = Some(o.p1 as usize);
                if db.malloc_failed != 0 {
                    Flow::NoMem
                } else {
                    if (db.m_trace as u32 & SQLITE_TRACE_ROW) != 0 {
                        if let Some(mut f) = db.x_trace.take() {
                            f(&TraceEvent::Row { stmt: p.stmt_id });
                            if db.x_trace.is_none() {
                                db.x_trace = Some(f);
                            }
                        }
                    }
                    p.pc = pc + 1;
                    Flow::Return(SQLITE_ROW)
                }
            }

            // Opcode: Concat P1 P2 P3 * *: r[P3]=r[P2]+r[P1].
            OP_CONCAT => op_concat(db, p, &st, pc),

            // Opcode: Add, Subtract, Multiply, Divide, Remainder P1 P2 P3 * *.
            OP_ADD | OP_SUBTRACT | OP_MULTIPLY | OP_DIVIDE | OP_REMAINDER => {
                op_arith(&mut p.a_mem, o);
                Flow::Continue(pc)
            }

            // Os demais opcodes (inclusive o `default` do C, OP_Noop e OP_Explain).
            _ => crate::vdbe_ops::exec_op(db, p, pcu, &mut st),
        };

        match flow {
            Flow::Continue(next) => {
                end_cycle(p, &mut st);
                pc = next + 1;
            }
            Flow::Jump(dest) => {
                end_cycle(p, &mut st);
                pc = dest;
            }
            Flow::JumpCheck(dest) => {
                // check_for_interrupt: o `pOp` do C já vale `dest - 1`.
                if db.interrupted.load(Ordering::Relaxed) {
                    return vdbe_tail(db, p, &mut st, dest - 1, Flow::Interrupt);
                }
                // Chama o callback de progresso, se configurado e se o número de opcodes
                // exigido já foi executado; se ele devolve diferente de zero, SQLITE_ABORT.
                while st.n_vm_step >= st.n_progress_limit && db.x_progress.is_some() {
                    debug_assert!(db.n_progress_ops != 0);
                    st.n_progress_limit += db.n_progress_ops as u64;
                    if call_progress(db) != 0 {
                        st.n_progress_limit = LARGEST_UINT64;
                        st.rc = crate::consts::SQLITE_INTERRUPT;
                        return vdbe_tail(db, p, &mut st, dest - 1, Flow::AbortDueToError);
                    }
                }
                end_cycle(p, &mut st);
                pc = dest;
            }
            other => return vdbe_tail(db, p, &mut st, pc, other),
        }
    }
}

/// O programa de um `Vdbe` sem tomar o `Vdbe` inteiro emprestado: `p_cur_prog` (um subprograma)
/// ou `a_op`. Serve ao código que, no mesmo passo, escreve em `Vdbe.a_mem`.
#[inline]
pub fn prog_ops<'a>(cur: &'a Option<Rc<SubProgram>>, main: &'a [Op]) -> &'a [Op] {
    match cur {
        Some(sp) => &sp.a_op,
        None => main,
    }
}

/// `*pnCycle += sqlite3Hwtime(); pnCycle = 0;`.
fn end_cycle(p: &mut Vdbe, st: &mut ExecState) {
    if let Some(i) = st.pn_cycle.take() {
        let t = hwtime();
        if let Some(op) = p.a_op.get_mut(i) {
            op.n_cycle = op.n_cycle.wrapping_add(t);
        }
    }
}

/// `db->xProgress(db->pProgressArg)`.
fn call_progress(db: &mut Connection) -> i32 {
    match db.x_progress.as_mut() {
        Some(f) => f(),
        None => 0,
    }
}

/// A saída do laço: `abort_due_to_error`, `too_big`, `no_mem`, `abort_due_to_interrupt` e
/// `vdbe_return` do C. `pc` é o `pOp - aOp` do momento (só serve à mensagem de log). Devolve o
/// valor de `sqlite3VdbeExec`.
pub fn vdbe_tail(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32, flow: Flow) -> i32 {
    let mut flow = flow;
    loop {
        match flow {
            // Uma string ou blob maior que SQLITE_MAX_LENGTH.
            Flow::TooBig => {
                vdbe_error(p, db, b"string or blob too big", &[]);
                st.rc = SQLITE_TOOBIG;
                flow = Flow::AbortDueToError;
            }
            // Um malloc() falhou.
            Flow::NoMem => {
                crate::util::oom_fault(db);
                vdbe_error(p, db, b"out of memory", &[]);
                st.rc = SQLITE_NOMEM_BKPT;
                flow = Flow::AbortDueToError;
            }
            // sqlite3_interrupt() ligou o flag.
            Flow::Interrupt => {
                debug_assert!(db.interrupted.load(Ordering::Relaxed));
                st.rc = crate::consts::SQLITE_INTERRUPT;
                flow = Flow::AbortDueToError;
            }
            Flow::AbortDueToError => {
                let mut rc = st.rc;
                if db.malloc_failed != 0 {
                    rc = SQLITE_NOMEM_BKPT;
                } else if rc == SQLITE_IOERR_CORRUPTFS {
                    rc = SQLITE_CORRUPT_BKPT;
                }
                debug_assert!(rc != 0);
                if p.z_err_msg.is_none() && rc != SQLITE_IOERR_NOMEM {
                    vdbe_error(
                        p,
                        db,
                        b"%s",
                        &[PrintfArg::Text(Some(err_str(rc).as_bytes().to_vec()))],
                    );
                }
                p.rc = rc;
                crate::util::system_error(db, rc);
                log(
                    rc,
                    b"statement aborts at %d: [%s] %s",
                    &[
                        PrintfArg::Int(pc as i64),
                        PrintfArg::Text(p.z_sql.clone()),
                        PrintfArg::Text(p.z_err_msg.clone()),
                    ],
                );
                if p.e_vdbe_state == VDBE_RUN_STATE {
                    vdbe_halt(p, db);
                }
                if rc == SQLITE_IOERR_NOMEM {
                    crate::util::oom_fault(db);
                }
                if rc == SQLITE_CORRUPT && db.auto_commit == 0 {
                    db.flags |= SQLITE_CORRUPT_RD_ONLY;
                }
                st.rc = SQLITE_ERROR;
                if st.reset_schema_on_fault > 0 {
                    reset_one_schema(db, st.reset_schema_on_fault as i32 - 1);
                }
                flow = Flow::Return(SQLITE_ERROR);
            }
            // vdbe_return: a única saída do procedimento.
            Flow::Return(rc) => {
                st.rc = rc;
                end_cycle(p, st);
                while st.n_vm_step >= st.n_progress_limit && db.x_progress.is_some() {
                    st.n_progress_limit += db.n_progress_ops as u64;
                    if call_progress(db) != 0 {
                        st.n_progress_limit = LARGEST_UINT64;
                        st.rc = crate::consts::SQLITE_INTERRUPT;
                        break;
                    }
                }
                if st.rc != rc {
                    // O callback pediu a interrupção: `rc = SQLITE_INTERRUPT; goto abort`.
                    flow = Flow::AbortDueToError;
                    continue;
                }
                let k = SQLITE_STMTSTATUS_VM_STEP as usize;
                p.a_counter[k] = p.a_counter[k].wrapping_add(st.n_vm_step as u32);
                return rc;
            }
            // Continue, Jump e JumpCheck nunca saem do laço.
            Flow::Continue(_) | Flow::Jump(_) | Flow::JumpCheck(_) => {
                debug_assert!(false, "fluxo sem saída do laço em vdbe_tail");
                return st.rc;
            }
        }
    }
}

/// `OP_Halt` (e o fim de `OP_HaltIfNull`): termina o programa ou, num subprograma, devolve o
/// controle ao chamador.
fn op_halt(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    debug_assert!(o.p1 != crate::consts::SQLITE_INTERNAL);
    if o.p1 == SQLITE_OK {
        if let Some(mut frame) = p.p_frame.pop() {
            // Termina o subprograma e devolve o controle ao frame pai.
            vdbe_set_changes(db, p.n_change);
            let mut pcx = frame_restore(p, db, &mut frame);
            frame_delete(db, frame);
            if o.p2 == OE_IGNORE as i32 {
                // A instrução pcx é o OP_Program que chamou o subprograma; com OE_Ignore o
                // subprograma lança uma exceção IGNORE e o salto vai ao P2 do OP_Program.
                pcx = cur_ops(p).get(pcx as usize).map_or(0, |op| op.p2) - 1;
            }
            return Flow::Continue(pcx);
        }
    }
    p.rc = o.p1;
    p.error_action = o.p2 as u8;
    debug_assert!(o.p5 <= 4);
    if p.rc != 0 {
        let z4: Option<Vec<u8>> = p4_z(&cur_ops(p)[pc as usize]).map(|z| z.to_vec());
        if o.p5 != 0 {
            const AZ_TYPE: [&[u8]; 4] = [b"NOT NULL", b"UNIQUE", b"CHECK", b"FOREIGN KEY"];
            let ty: &[u8] = AZ_TYPE.get(o.p5 as usize - 1).copied().unwrap_or(&b""[..]);
            vdbe_error(p, db, b"%s constraint failed", &[PrintfArg::Text(Some(ty.to_vec()))]);
            if let Some(z) = z4 {
                let mx = db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32;
                let old = p.z_err_msg.take();
                p.z_err_msg =
                    vm_printf(mx, b"%s: %s", &[PrintfArg::Text(old), PrintfArg::Text(Some(z))]).0;
            }
        } else {
            vdbe_error(p, db, b"%s", &[PrintfArg::Text(z4)]);
        }
        log(
            o.p1,
            b"abort at %d in [%s]: %s",
            &[
                PrintfArg::Int(pc as i64),
                PrintfArg::Text(p.z_sql.clone()),
                PrintfArg::Text(p.z_err_msg.clone()),
            ],
        );
    }
    let mut rc = vdbe_halt(p, db);
    debug_assert!(rc == SQLITE_BUSY || rc == SQLITE_OK || rc == SQLITE_ERROR);
    if rc == SQLITE_BUSY {
        p.rc = SQLITE_BUSY;
    } else {
        debug_assert!(rc == SQLITE_OK || (p.rc & 0xff) == crate::consts::SQLITE_CONSTRAINT);
        rc = if p.rc != 0 { SQLITE_ERROR } else { SQLITE_DONE };
    }
    Flow::Return(rc)
}

/// O corpo de `OP_String` (e o fim de `OP_String8`): `r[P2]` recebe os `p1` bytes de `z`
/// (a "string estática" do C) mais o terminador, e vira BLOB se `r[P3]` vale `P5`.
fn exec_string(a_mem: &mut [Mem], st: &ExecState, p1: i32, z: &[u8]) {
    let o = st.op;
    let in3 = if o.p3 > 0 { Some(a_mem[o.p3 as usize].u_i) } else { None };
    let out = out2_prerelease(a_mem, o.p2);
    let take = (p1.max(0) as usize).min(z.len());
    let mut v: Vec<u8> = Vec::with_capacity(take + 2);
    v.extend_from_slice(&z[..take]);
    v.push(0);
    if st.encoding != ENC_UTF8 {
        v.push(0);
    }
    out.flags = MEM_STR | MEM_STATIC | MEM_TERM;
    out.z = v;
    out.sz_malloc = 0;
    out.n = p1;
    out.enc = st.encoding;
    // SQLITE_LIKE_DOESNT_MATCH_BLOBS não está ligado no Debian.
    if let Some(i3) = in3 {
        if i3 == o.p5 as i64 {
            out.flags = MEM_BLOB | MEM_STATIC | MEM_TERM;
        }
    }
}

/// `OP_String8`: calcula o tamanho do P4, converte para a codificação do banco se preciso e se
/// transforma em `OP_String` (no programa principal), caindo no corpo dele.
fn op_string8(db: &mut Connection, p: &mut Vdbe, st: &mut ExecState, pc: i32) -> Flow {
    let o = st.op;
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    let mut z: Vec<u8> = p4_z(&cur_ops(p)[pc as usize]).map(<[u8]>::to_vec).unwrap_or_default();
    let mut len = strlen30(&z);
    if st.encoding != ENC_UTF8 {
        let out = out2_prerelease(&mut p.a_mem, o.p2);
        let rc = mem_set_str(out, Some(&z[..]), -1, ENC_UTF8, StrDtor::Static, limit);
        debug_assert!(rc == SQLITE_OK || rc == SQLITE_TOOBIG);
        if rc != SQLITE_OK {
            return Flow::TooBig;
        }
        if vdbe_change_encoding(out, st.encoding as i32) != SQLITE_OK {
            return Flow::NoMem;
        }
        // O texto convertido passa a ser o P4 do opcode (e a célula só o empresta).
        out.sz_malloc = 0;
        out.flags |= MEM_STATIC;
        len = out.n;
        z = out.z[..(len.max(0) as usize).min(out.z.len())].to_vec();
    }
    if len > limit {
        return Flow::TooBig;
    }
    if p.p_cur_prog.is_none() {
        let op = &mut p.a_op[pc as usize];
        op.p1 = len;
        if st.encoding != ENC_UTF8 {
            op.p4 = P4::Text(z.clone());
        }
        op.opcode = OP_STRING;
    }
    // Cai no OP_String.
    exec_string(&mut p.a_mem, st, len, &z);
    Flow::Continue(pc)
}

/// `OP_Concat`: `r[P3] = r[P2] || r[P1]`.
fn op_concat(db: &mut Connection, p: &mut Vdbe, st: &ExecState, pc: i32) -> Flow {
    let o = st.op;
    let enc = st.encoding;
    let (i1, i2, i3) = (o.p1 as usize, o.p2 as usize, o.p3 as usize);
    let a = &mut p.a_mem;
    debug_assert!(i1 != i3);
    let mut flags1 = a[i1].flags;
    if ((flags1 | a[i2].flags) & MEM_NULL) != 0 {
        mem_set_null(&mut a[i3]);
        return Flow::Continue(pc);
    }
    if (flags1 & (MEM_STR | MEM_BLOB)) == 0 {
        if mem_stringify(&mut a[i1], enc, false) != SQLITE_OK {
            return Flow::NoMem;
        }
        flags1 = a[i1].flags & !MEM_STR;
    } else if (flags1 & MEM_ZERO) != 0 {
        if mem_expand_blob(&mut a[i1]) != SQLITE_OK {
            return Flow::NoMem;
        }
        flags1 = a[i1].flags & !MEM_STR;
    }
    let mut flags2 = a[i2].flags;
    if (flags2 & (MEM_STR | MEM_BLOB)) == 0 {
        if mem_stringify(&mut a[i2], enc, false) != SQLITE_OK {
            return Flow::NoMem;
        }
        flags2 = a[i2].flags & !MEM_STR;
    } else if (flags2 & MEM_ZERO) != 0 {
        if mem_expand_blob(&mut a[i2]) != SQLITE_OK {
            return Flow::NoMem;
        }
        flags2 = a[i2].flags & !MEM_STR;
    }
    let n1 = a[i1].n;
    let n2 = a[i2].n;
    let mut n_byte: i64 = n1 as i64 + n2 as i64;
    if n_byte > db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        return Flow::TooBig;
    }
    if mem_grow(&mut a[i3], n_byte as i32 + 2, i3 == i2) != SQLITE_OK {
        return Flow::NoMem;
    }
    a[i3].set_type_flag(MEM_STR);
    if i3 != i2 {
        let (out, in2) = two_mut(a, i3, i2);
        let src = in2.bytes();
        out.z[..src.len()].copy_from_slice(src);
        in2.flags = flags2;
    }
    {
        let (out, in1) = two_mut(a, i3, i1);
        let src = in1.bytes();
        let at = n2.max(0) as usize;
        out.z[at..at + src.len()].copy_from_slice(src);
        in1.flags = flags1;
    }
    if enc > ENC_UTF8 {
        n_byte &= !1;
    }
    let out = &mut a[i3];
    out.z[n_byte as usize] = 0;
    out.z[n_byte as usize + 1] = 0;
    out.flags |= MEM_TERM;
    out.n = n_byte as i32;
    out.enc = enc;
    Flow::Continue(pc)
}

/// `OP_Add`, `OP_Subtract`, `OP_Multiply`, `OP_Divide` e `OP_Remainder`: `r[P3] = r[P2] op r[P1]`.
fn op_arith(a: &mut [Mem], o: OpRegs) {
    /// De onde continuar (os rótulos `int_math`, `fp_math` e `arithmetic_result_is_null`).
    enum Step {
        IntMath,
        FpMath,
        Null,
    }
    let (i1, i2, i3) = (o.p1 as usize, o.p2 as usize, o.p3 as usize);
    let mut type1 = a[i1].flags;
    let mut type2 = a[i2].flags;
    let mut step;
    if (type1 & type2 & MEM_INT) != 0 {
        step = Step::IntMath;
    } else if ((type1 | type2) & MEM_NULL) != 0 {
        step = Step::Null;
    } else {
        type1 = numeric_type(&mut a[i1]);
        type2 = numeric_type(&mut a[i2]);
        step = if (type1 & type2 & MEM_INT) != 0 { Step::IntMath } else { Step::FpMath };
    }
    loop {
        match step {
            Step::IntMath => {
                let mut i_a = a[i1].u_i;
                let mut i_b = a[i2].u_i;
                match o.opcode {
                    OP_ADD => {
                        if add_int64(&mut i_b, i_a) != 0 {
                            step = Step::FpMath;
                            continue;
                        }
                    }
                    OP_SUBTRACT => {
                        if sub_int64(&mut i_b, i_a) != 0 {
                            step = Step::FpMath;
                            continue;
                        }
                    }
                    OP_MULTIPLY => {
                        if mul_int64(&mut i_b, i_a) != 0 {
                            step = Step::FpMath;
                            continue;
                        }
                    }
                    OP_DIVIDE => {
                        if i_a == 0 {
                            step = Step::Null;
                            continue;
                        }
                        if i_a == -1 && i_b == SMALLEST_INT64 {
                            step = Step::FpMath;
                            continue;
                        }
                        i_b /= i_a;
                    }
                    _ => {
                        if i_a == 0 {
                            step = Step::Null;
                            continue;
                        }
                        if i_a == -1 {
                            i_a = 1;
                        }
                        i_b %= i_a;
                    }
                }
                a[i3].u_i = i_b;
                a[i3].set_type_flag(MEM_INT);
                return;
            }
            Step::FpMath => {
                let r_a = vdbe_real_value(&a[i1]);
                let mut r_b = vdbe_real_value(&a[i2]);
                match o.opcode {
                    OP_ADD => r_b += r_a,
                    OP_SUBTRACT => r_b -= r_a,
                    OP_MULTIPLY => r_b *= r_a,
                    OP_DIVIDE => {
                        if r_a == 0.0 {
                            step = Step::Null;
                            continue;
                        }
                        r_b /= r_a;
                    }
                    _ => {
                        let mut i_a = vdbe_int_value(&a[i1]);
                        let i_b = vdbe_int_value(&a[i2]);
                        if i_a == 0 {
                            step = Step::Null;
                            continue;
                        }
                        if i_a == -1 {
                            i_a = 1;
                        }
                        r_b = (i_b % i_a) as f64;
                    }
                }
                if is_nan(r_b) {
                    step = Step::Null;
                    continue;
                }
                a[i3].u_r = r_b;
                a[i3].set_type_flag(MEM_REAL);
                return;
            }
            Step::Null => {
                mem_set_null(&mut a[i3]);
                return;
            }
        }
    }
}
