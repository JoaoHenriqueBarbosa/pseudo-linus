//! `vdbeapi.c`: a API do VDBE exposta ao programa (`sqlite3_step`, `sqlite3_reset`,
//! `sqlite3_finalize`, `sqlite3_column_*`, `sqlite3_value_*`, `sqlite3_result_*`,
//! `sqlite3_bind_*`, auxdata, informações do comando, pré-atualização e scanstatus) do SQLite
//! 3.46.1, no modelo v2 (ver `CONVENTIONS.md`).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * O `sqlite3_stmt*` é o handle [`StmtId`] e o `sqlite3*` que o `Vdbe` guardava (`p->db`) é o
//!   primeiro parâmetro `db: &mut Connection`. O ponteiro nulo do C não tem equivalente: quem
//!   chama a partir de uma fachada com `Option<StmtId>` trata o `None` antes (`finalize(NULL)`
//!   e `reset(NULL)` devolvem `SQLITE_OK`). Um handle que não está mais em `Connection.stmts`
//!   (já finalizado) faz o papel do `p->db==0` de `vdbeSafety`: registra o `SQLITE_MISUSE` no
//!   log e devolve `SQLITE_MISUSE`. Por isso `vdbeSafetyNotNull` não existe.
//! * Os mutexes (`sqlite3_mutex_enter`/`leave`) somem (uma conexão tem um dono só, ver
//!   `vdbeaux2.rs`), assim como todos os ramos `SQLITE_ENABLE_API_ARMOR`, `SQLITE_DEBUG` e
//!   `SQLITE_ENABLE_NORMALIZE`.
//! * As funções que executam o `Vdbe` (`step`, `reset`, `finalize`) retiram o comando do slab
//!   (`Connection.stmts.take`) durante a chamada, como exige `vdbe_exec(db, p)`. Fora disso o
//!   comando fica na vaga e as funções o alcançam por `db.stmt(id)`/`db.stmt_mut(id)`.
//! * Destrutores (`xDel`) viram [`StrDtor`] (o texto é sempre copiado para a célula, ver
//!   `mem.rs`); o `invokeValueDestructor` some porque o `Drop` faz o serviço. Onde o C chamava o
//!   destrutor para recusar um valor grande demais (`sqlite3_result_blob64` com `n > 0x7fffffff`)
//!   o resultado é só o `sqlite3_result_error_toobig`.
//! * `sqlite3_value_text` e companhia: `crate::mem::value_text`/`value_bytes` já implementam o
//!   `sqlite3ValueText`/`sqlite3ValueBytes` com o parâmetro de codificação; aqui `value_text`,
//!   `value_text16*`, `value_bytes` e `value_bytes16` são as formas da API (sem `enc`) e mudam só
//!   o nome do módulo. `sqlite3_value_type` é `crate::mem::value_type` (idêntica, não repetida).
//! * Texto e blob devolvidos pelas funções `column_*` e `value_*` são fatias `&[u8]` emprestadas
//!   (sem o terminador). Como o empréstimo impede chamar `sqlite3ApiExit` depois, o
//!   `columnMallocFailure` roda antes de a fatia ser tomada de volta da linha de resultado.
//! * Nomes de coluna, de parâmetro e SQL expandido saem como `Vec<u8>` (o C devolve ponteiros para
//!   memória do comando que a conversão de codificação pode trocar).
//! * `sqlite3_column_value`/preupdate devolvem uma CÓPIA (`Mem`) da célula, pois um ponteiro para
//!   dentro do `Vdbe` não atravessa o empréstimo da conexão.
//! * O `sqlite3_aggregate_context(p, nByte)` é o genérico [`aggregate_context`]: o acumulador é
//!   um `T: Default` guardado em `Context.agg` (o `MEM_Agg` do C).
//! * O `VList` do `Vdbe` é `Vec<VListEntry>` (`crate::vdbe_types`), por isso a busca por nome ou
//!   número é feita aqui direto sobre as entradas.
//! * Valores ponteiro (`sqlite3_value_pointer`, `_result_pointer`, `_bind_pointer`) e a iteração
//!   de `sqlite3_vtab_in` não estão aqui: dependem do `Mem` de ponteiro e do `ValueList`, ainda
//!   não definidos (ver o relatório da fatia).

use std::any::Any;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::btree_cursor::{btree_payload, btree_payload_size};
use crate::build::table_column_to_index;
use crate::connection::{Connection, Context, DestroyFn, StmtId, TraceEvent, UserData};
use crate::consts::opcodes::{OPCODE_PROPERTY, OPFLG_NCYCLE};
use crate::consts::{
    COLNAME_DATABASE, COLNAME_DECLTYPE, COLNAME_COLUMN, COLNAME_NAME, COLNAME_TABLE, MEM_BLOB,
    MEM_DYN, MEM_EPHEM, MEM_FROMBIND, MEM_INT, MEM_INTREAL, MEM_NULL, MEM_REAL, MEM_STATIC,
    MEM_STR, MEM_SUBTYPE, MEM_TERM, MEM_ZERO, SQLITE_AFF_REAL, SQLITE_BUSY, SQLITE_DELETE,
    SQLITE_DONE, SQLITE_ERROR, SQLITE_INSERT, SQLITE_INTEGER, SQLITE_FLOAT, SQLITE_BLOB,
    SQLITE_LIMIT_LENGTH, SQLITE_MAX_SCHEMA_RETRY, SQLITE_MISUSE, SQLITE_NOMEM, SQLITE_NOMEM_BKPT,
    SQLITE_OK, SQLITE_PREPARE_SAVESQL, SQLITE_RANGE,
    SQLITE_ROW, SQLITE_SCANSTAT_COMPLEX, SQLITE_SCANSTAT_EST, SQLITE_SCANSTAT_EXPLAIN,
    SQLITE_SCANSTAT_NAME, SQLITE_SCANSTAT_NCYCLE, SQLITE_SCANSTAT_NLOOP, SQLITE_SCANSTAT_NVISIT,
    SQLITE_SCANSTAT_PARENTID, SQLITE_SCANSTAT_SELECTID, SQLITE_SCHEMA, SQLITE_STMTSTATUS_MEMUSED,
    SQLITE_TEXT, SQLITE_TOOBIG, SQLITE_TRACE_PROFILE, SQLITE_TRACE_XPROFILE, SQLITE_UPDATE,
    SQLITE_UTF16, SQLITE_UTF16NATIVE, SQLITE_UTF8, VDBE_HALT_STATE, VDBE_READY_STATE,
    VDBE_RUN_STATE,
};
use crate::global::log;
use crate::mem::{
    mem_copy, mem_expand_blob, mem_make_writeable, mem_move, mem_realify, mem_release,
    mem_set_double, mem_set_int64, mem_set_null, mem_set_str, mem_set_zero_blob, mem_too_big,
    mem_zero_terminate_if_able, value_bytes as mem_value_bytes, value_text as mem_value_text,
    value_type, vdbe_change_encoding, vdbe_int_value, vdbe_real_value, KeyInfo, Mem, StrDtor,
    UnpackedRecord, ENC_UTF16BE, ENC_UTF16LE, ENC_UTF8,
};
use crate::os::os_current_time_int64;
use crate::printf::PrintfArg;
use crate::record::{alloc_unpacked_record, record_unpack};
use crate::util::{err_str, log_est_to_int};
use crate::vdbe::vdbe_exec;
use crate::vdbe_types::{AuxData, PreUpdate, Vdbe, P4};
use crate::vdbeaux2::{vdbe_finalize, vdbe_list, vdbe_reset, vdbe_rewind, vdbe_transfer_error};

/// `sqlite3VdbeSetChanges` mora em `vdbeaux3.rs` (o `vdbeaux2.rs` a chama por aqui).
pub use crate::vdbeaux3::vdbe_set_changes;

/// A codificação nativa como `u8` (`SQLITE_UTF16NATIVE`).
const ENC_NATIVE: u8 = SQLITE_UTF16NATIVE as u8;

/// Até 2^31 - 1 bytes de texto ou blob (`0x7fffffff`) nas variantes de 64 bits.
const MAX_INT_LEN: u64 = 0x7fff_ffff;

// ---------------------------------------------------------------------------------------------
// chunk 000: segurança, perfil, finalize, reset e clear_bindings
// ---------------------------------------------------------------------------------------------

/// `sqlite3_expired`: verdadeiro se o comando precisa ser recompilado. Um handle ausente conta
/// como expirado (`p==0`).
pub fn expired(db: &Connection, id: StmtId) -> bool {
    match db.stmt(id) {
        Some(p) => p.expired != 0,
        None => true,
    }
}

/// `vdbeSafety`: registra e devolve verdadeiro se o comando já foi finalizado (ou não é válido).
fn vdbe_safety(db: &Connection, id: StmtId) -> bool {
    if db.stmt(id).is_none() {
        log(SQLITE_MISUSE, b"API called with finalized prepared statement", &[]);
        true
    } else {
        false
    }
}

/// `invokeProfileCallback`: chama os ganchos de perfil. Só se chama quando `p.start_time > 0`.
fn invoke_profile_callback(db: &mut Connection, p: &mut Vdbe, id: StmtId) {
    debug_assert!(p.start_time > 0);
    debug_assert!((db.m_trace & (SQLITE_TRACE_PROFILE as u8 | SQLITE_TRACE_XPROFILE)) != 0);
    debug_assert!(db.init.busy == 0);
    debug_assert!(p.z_sql.is_some());
    let mut i_now = 0i64;
    if let Some(vfs) = db.p_vfs.as_ref() {
        os_current_time_int64(&**vfs, &mut i_now);
    }
    let i_elapse = i_now.wrapping_sub(p.start_time).wrapping_mul(1_000_000);
    if let Some(x_profile) = db.x_profile.as_mut() {
        x_profile(p.z_sql.as_deref().unwrap_or(&[]), i_elapse as u64);
    }
    if (db.m_trace as u32 & SQLITE_TRACE_PROFILE) != 0 {
        if let Some(mut f) = db.x_trace.take() {
            f(&TraceEvent::Profile { stmt: id, ns: i_elapse });
            if db.x_trace.is_none() {
                db.x_trace = Some(f);
            }
        }
    }
    p.start_time = 0;
}

/// `checkProfileCallback(DB, P)`: invoca o gancho de perfil se for preciso.
fn check_profile_callback(db: &mut Connection, p: &mut Vdbe, id: StmtId) {
    if p.start_time > 0 {
        invoke_profile_callback(db, p, id);
    }
}

/// `sqlite3_finalize`: destrói a VM e devolve o código de resultado da última execução. Também
/// grava o erro lido por `sqlite3_errcode()` e `sqlite3_errmsg()`.
pub fn finalize(db: &mut Connection, id: StmtId) -> i32 {
    if vdbe_safety(db, id) {
        return SQLITE_MISUSE;
    }
    let Some(mut v) = db.stmts.take(id.slot()) else {
        return SQLITE_MISUSE;
    };
    check_profile_callback(db, &mut v, id);
    debug_assert!(v.e_vdbe_state >= VDBE_READY_STATE);
    let rc = vdbe_finalize(v, db);
    // `sqlite3VdbeDelete` desencadeia a lista `pVdbe`: libera a vaga e tira o handle da ordem.
    db.stmts.remove(id.slot());
    db.stmt_list.retain(|s| *s != id);
    let rc = crate::main::api_exit(db, rc);
    crate::main::leave_mutex_and_close_zombie(db);
    rc
}

/// O núcleo de `sqlite3_reset` sobre o `Vdbe` já retirado do slab.
fn reset_vdbe(db: &mut Connection, v: &mut Vdbe, id: StmtId) -> i32 {
    check_profile_callback(db, v, id);
    let rc = vdbe_reset(v, db);
    vdbe_rewind(v);
    debug_assert!((rc & db.err_mask) == rc);
    crate::main::api_exit(db, rc)
}

/// `sqlite3_reset`: termina a execução corrente e devolve a VM ao estado inicial, para ser
/// reusada. Devolve o código de sucesso da execução anterior.
pub fn reset(db: &mut Connection, id: StmtId) -> i32 {
    let Some(mut v) = db.stmts.take(id.slot()) else {
        log(SQLITE_MISUSE, b"API called with finalized prepared statement", &[]);
        return SQLITE_MISUSE;
    };
    let rc = reset_vdbe(db, &mut v, id);
    db.stmts.put(id.slot(), v);
    rc
}

/// `sqlite3_clear_bindings`: põe todos os parâmetros do comando em NULL.
pub fn clear_bindings(db: &mut Connection, id: StmtId) -> i32 {
    let Some(p) = db.stmt_mut(id) else {
        return SQLITE_MISUSE;
    };
    let n = (p.n_var.max(0) as usize).min(p.a_var.len());
    for var in p.a_var[..n].iter_mut() {
        mem_release(var);
        var.flags = MEM_NULL;
    }
    debug_assert!((p.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) != 0 || p.exp_mask == 0);
    if p.exp_mask != 0 {
        p.expired = 1;
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// sqlite3_value_*
// ---------------------------------------------------------------------------------------------

/// `sqlite3_value_blob`: os bytes do valor como BLOB. `None` é o ponteiro nulo (blob vazio, NULL
/// ou falta de memória).
pub fn value_blob(p: &mut Mem) -> Option<&[u8]> {
    if p.flags & (MEM_BLOB | MEM_STR) != 0 {
        if p.flags & MEM_ZERO != 0 && mem_expand_blob(p) != SQLITE_OK {
            debug_assert!(p.flags == MEM_NULL);
            return None;
        }
        p.flags |= MEM_BLOB;
        if p.n != 0 {
            Some(p.bytes())
        } else {
            None
        }
    } else {
        mem_value_text(p, ENC_UTF8)
    }
}

/// `sqlite3_value_text` sobre um argumento que não pode ser alterado: o texto UTF-8, ou `None`
/// para NULL. Empresta quando o valor já é texto UTF-8; senão converte uma cópia.
pub fn text_of(p: &Mem) -> Option<std::borrow::Cow<'_, [u8]>> {
    if p.flags & MEM_STR != 0 && p.enc == ENC_UTF8 {
        return Some(std::borrow::Cow::Borrowed(p.bytes()));
    }
    if p.flags & MEM_NULL != 0 {
        return None;
    }
    let mut c = p.clone();
    value_text(&mut c).map(|s| std::borrow::Cow::Owned(s.to_vec()))
}

/// `sqlite3_value_bytes`: o tamanho do valor em bytes na codificação UTF-8.
pub fn value_bytes(p: &mut Mem) -> i32 {
    mem_value_bytes(p, ENC_UTF8)
}

/// `sqlite3_value_bytes16`: o tamanho do valor em bytes na codificação nativa UTF-16.
pub fn value_bytes16(p: &mut Mem) -> i32 {
    mem_value_bytes(p, ENC_NATIVE)
}

/// `sqlite3_value_double`.
pub fn value_double(p: &Mem) -> f64 {
    vdbe_real_value(p)
}

/// `sqlite3_value_int`: os 32 bits baixos do inteiro.
pub fn value_int(p: &Mem) -> i32 {
    vdbe_int_value(p) as i32
}

/// `sqlite3_value_int64`.
pub fn value_int64(p: &Mem) -> i64 {
    vdbe_int_value(p)
}

/// `sqlite3_value_subtype`.
pub fn value_subtype(p: &Mem) -> u32 {
    if p.flags & MEM_SUBTYPE != 0 {
        p.e_subtype as u32
    } else {
        0
    }
}

/// `sqlite3_value_text`: o texto UTF-8 do valor (sem o terminador).
pub fn value_text(p: &mut Mem) -> Option<&[u8]> {
    mem_value_text(p, ENC_UTF8)
}

/// `sqlite3_value_text16`: o texto na codificação nativa UTF-16.
pub fn value_text16(p: &mut Mem) -> Option<&[u8]> {
    mem_value_text(p, ENC_NATIVE)
}

/// `sqlite3_value_text16be`.
pub fn value_text16be(p: &mut Mem) -> Option<&[u8]> {
    mem_value_text(p, ENC_UTF16BE)
}

/// `sqlite3_value_text16le`.
pub fn value_text16le(p: &mut Mem) -> Option<&[u8]> {
    mem_value_text(p, ENC_UTF16LE)
}

/// `sqlite3_value_encoding`.
pub fn value_encoding(p: &Mem) -> i32 {
    p.enc as i32
}

/// `sqlite3_value_nochange`: verdadeiro se o parâmetro de `xUpdate` é uma coluna não alterada.
pub fn value_nochange(p: &Mem) -> bool {
    (p.flags & (MEM_NULL | MEM_ZERO)) == (MEM_NULL | MEM_ZERO)
}

/// `sqlite3_value_frombind`: verdadeiro se o valor veio de um `sqlite3_bind()`.
pub fn value_frombind(p: &Mem) -> bool {
    (p.flags & MEM_FROMBIND) != 0
}

/// `sqlite3_value_dup`: uma cópia independente do valor. `None` se faltou memória.
pub fn value_dup(orig: &Mem) -> Option<Mem> {
    let mut new = Mem {
        flags: orig.flags & !MEM_DYN,
        enc: orig.enc,
        e_subtype: orig.e_subtype,
        n: orig.n,
        u_i: orig.u_i,
        u_r: orig.u_r,
        n_zero: orig.n_zero,
        ..Mem::default()
    };
    if orig.flags & (MEM_STR | MEM_BLOB) != 0 {
        new.z = orig.z.clone();
        new.flags &= !(MEM_STATIC | MEM_DYN);
        new.flags |= MEM_EPHEM;
        if mem_make_writeable(&mut new) != SQLITE_OK {
            return None;
        }
    } else if new.flags & MEM_NULL != 0 {
        // Valores ponteiro não se duplicam.
        new.flags &= !(MEM_TERM | MEM_SUBTYPE);
    }
    Some(new)
}

/// `sqlite3_value_free`: destrói um valor obtido de [`value_dup`].
pub fn value_free(mut old: Mem) {
    mem_release(&mut old);
}

// ---------------------------------------------------------------------------------------------
// chunk 001: sqlite3_result_*
// ---------------------------------------------------------------------------------------------

/// O limite `SQLITE_LIMIT_LENGTH` da conexão do contexto.
#[inline]
fn length_limit(ctx: &Context<'_>) -> i32 {
    ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize]
}

/// `setResultStrOrError`: grava o texto ou blob como resultado, com os erros de `TOOBIG` e
/// `NOMEM` convertidos em erro da função.
fn set_result_str_or_error(
    ctx: &mut Context<'_>,
    z: Option<&[u8]>,
    n: i64,
    enc: u8,
    x_del: StrDtor,
) {
    let limit = length_limit(ctx);
    let rc = mem_set_str(&mut ctx.out, z, n, enc, x_del, limit);
    if rc != 0 {
        if rc == SQLITE_TOOBIG {
            result_error_toobig(ctx);
        } else {
            // Os únicos erros possíveis de `sqlite3VdbeMemSetStr` são TOOBIG e NOMEM.
            debug_assert!(rc == SQLITE_NOMEM);
            result_error_nomem(ctx);
        }
        return;
    }
    vdbe_change_encoding(&mut ctx.out, ctx.enc as i32);
    if mem_too_big(&ctx.out, limit) {
        result_error_toobig(ctx);
    }
}

/// `sqlite3_result_blob`.
pub fn result_blob(ctx: &mut Context<'_>, z: Option<&[u8]>, n: i32, x_del: StrDtor) {
    debug_assert!(n >= 0);
    set_result_str_or_error(ctx, z, n as i64, 0, x_del);
}

/// `sqlite3_result_blob64`.
pub fn result_blob64(ctx: &mut Context<'_>, z: Option<&[u8]>, n: u64, x_del: StrDtor) {
    if n > MAX_INT_LEN {
        result_error_toobig(ctx);
    } else {
        set_result_str_or_error(ctx, z, n as i64, 0, x_del);
    }
}

/// `sqlite3_result_double`.
pub fn result_double(ctx: &mut Context<'_>, r_val: f64) {
    mem_set_double(&mut ctx.out, r_val);
}

/// `sqlite3_result_error`: a função falhou com a mensagem `z` (UTF-8, `n < 0` vai até o NUL).
pub fn result_error(ctx: &mut Context<'_>, z: &[u8], n: i32) {
    ctx.is_error = SQLITE_ERROR;
    let limit = length_limit(ctx);
    mem_set_str(&mut ctx.out, Some(z), n as i64, ENC_UTF8, StrDtor::Transient, limit);
}

/// `sqlite3_result_error16`.
pub fn result_error16(ctx: &mut Context<'_>, z: &[u8], n: i32) {
    ctx.is_error = SQLITE_ERROR;
    let limit = length_limit(ctx);
    mem_set_str(&mut ctx.out, Some(z), n as i64, ENC_NATIVE, StrDtor::Transient, limit);
}

/// `sqlite3_result_int`.
pub fn result_int(ctx: &mut Context<'_>, i_val: i32) {
    mem_set_int64(&mut ctx.out, i_val as i64);
}

/// `sqlite3_result_int64`.
pub fn result_int64(ctx: &mut Context<'_>, i_val: i64) {
    mem_set_int64(&mut ctx.out, i_val);
}

/// `sqlite3_result_null`.
pub fn result_null(ctx: &mut Context<'_>) {
    mem_set_null(&mut ctx.out);
}

/// `sqlite3_result_subtype`.
pub fn result_subtype(ctx: &mut Context<'_>, e_subtype: u32) {
    ctx.out.e_subtype = (e_subtype & 0xff) as u8;
    ctx.out.flags |= MEM_SUBTYPE;
}

/// `sqlite3_result_text`.
pub fn result_text(ctx: &mut Context<'_>, z: Option<&[u8]>, n: i32, x_del: StrDtor) {
    set_result_str_or_error(ctx, z, n as i64, ENC_UTF8, x_del);
}

/// `sqlite3_result_text64`: `enc` é `SQLITE_UTF8`, `SQLITE_UTF16`, `SQLITE_UTF16LE` ou
/// `SQLITE_UTF16BE`.
pub fn result_text64(ctx: &mut Context<'_>, z: Option<&[u8]>, n: u64, x_del: StrDtor, enc: u8) {
    let mut enc = enc;
    let mut n = n;
    if enc as i32 != SQLITE_UTF8 {
        if enc as i32 == SQLITE_UTF16 {
            enc = ENC_NATIVE;
        }
        n &= !1u64;
    }
    if n > MAX_INT_LEN {
        result_error_toobig(ctx);
    } else {
        set_result_str_or_error(ctx, z, n as i64, enc, x_del);
        mem_zero_terminate_if_able(&mut ctx.out);
    }
}

/// `sqlite3_result_text16`.
pub fn result_text16(ctx: &mut Context<'_>, z: Option<&[u8]>, n: i32, x_del: StrDtor) {
    set_result_str_or_error(ctx, z, (n & !1) as i64, ENC_NATIVE, x_del);
}

/// `sqlite3_result_text16be`.
pub fn result_text16be(ctx: &mut Context<'_>, z: Option<&[u8]>, n: i32, x_del: StrDtor) {
    set_result_str_or_error(ctx, z, (n & !1) as i64, ENC_UTF16BE, x_del);
}

/// `sqlite3_result_text16le`.
pub fn result_text16le(ctx: &mut Context<'_>, z: Option<&[u8]>, n: i32, x_del: StrDtor) {
    set_result_str_or_error(ctx, z, (n & !1) as i64, ENC_UTF16LE, x_del);
}

/// `sqlite3_result_value`: o resultado é uma cópia de `value`.
pub fn result_value(ctx: &mut Context<'_>, value: &Mem) {
    mem_copy(&mut ctx.out, value);
    vdbe_change_encoding(&mut ctx.out, ctx.enc as i32);
    if mem_too_big(&ctx.out, length_limit(ctx)) {
        result_error_toobig(ctx);
    }
}

/// `sqlite3_result_zeroblob`.
pub fn result_zeroblob(ctx: &mut Context<'_>, n: i32) {
    result_zeroblob64(ctx, if n > 0 { n as u64 } else { 0 });
}

/// `sqlite3_result_zeroblob64`: `SQLITE_TOOBIG` se passa do limite de comprimento.
pub fn result_zeroblob64(ctx: &mut Context<'_>, n: u64) -> i32 {
    if n > length_limit(ctx) as u64 {
        result_error_toobig(ctx);
        return SQLITE_TOOBIG;
    }
    mem_set_zero_blob(&mut ctx.out, n as i32);
    SQLITE_OK
}

/// `sqlite3_result_error_code`: a função falhou com o código `err_code`, e a mensagem padrão do
/// código vira o resultado se nenhum outro já foi gravado.
pub fn result_error_code(ctx: &mut Context<'_>, err_code: i32) {
    ctx.is_error = if err_code != 0 { err_code } else { -1 };
    if ctx.out.flags & MEM_NULL != 0 {
        set_result_str_or_error(
            ctx,
            Some(err_str(err_code).as_bytes()),
            -1,
            ENC_UTF8,
            StrDtor::Static,
        );
    }
}

/// `sqlite3_result_error_toobig`: força um erro `SQLITE_TOOBIG`.
pub fn result_error_toobig(ctx: &mut Context<'_>) {
    ctx.is_error = SQLITE_TOOBIG;
    let limit = length_limit(ctx);
    mem_set_str(
        &mut ctx.out,
        Some(&b"string or blob too big"[..]),
        -1,
        ENC_UTF8,
        StrDtor::Static,
        limit,
    );
}

/// `sqlite3_result_error_nomem`: um erro `SQLITE_NOMEM`.
pub fn result_error_nomem(ctx: &mut Context<'_>) {
    mem_set_null(&mut ctx.out);
    ctx.is_error = SQLITE_NOMEM_BKPT;
    crate::util::oom_fault(ctx.db);
}

/// `sqlite3ResultIntReal`: força o INT64 gravado como resultado a ser um `MEM_IntReal` (o
/// controle de teste `SQLITE_TESTCTRL_RESULT_INTREAL`).
pub fn result_int_real(ctx: &mut Context<'_>) {
    if ctx.out.flags & MEM_INT != 0 {
        ctx.out.flags &= !MEM_INT;
        ctx.out.flags |= MEM_INTREAL;
    }
}

/// `doWalCallbacks`: chamada depois de um commit, invoca os ganchos registrados com
/// `sqlite3_wal_hook()`.
fn do_wal_callbacks(db: &mut Connection) -> i32 {
    let mut rc = SQLITE_OK;
    for i in 0..db.dbs.len() {
        let n_entry = match db.dbs[i].bt.as_mut() {
            Some(bt) => bt.bt.pager.wal_callback(),
            None => continue,
        };
        if n_entry > 0 && rc == SQLITE_OK {
            if let Some(hook) = db.x_wal_callback.as_mut() {
                rc = hook(&db.dbs[i].z_db_s_name, n_entry);
            }
        }
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// chunk 002: step, contexto de função, hora do comando
// ---------------------------------------------------------------------------------------------

/// `sqlite3Step`: a maior parte de `sqlite3_step()`; só falta a recompilação automática depois
/// de uma mudança de esquema, que `step` trata. Recebe o `Vdbe` já retirado do slab.
fn step_vdbe(db: &mut Connection, p: &mut Vdbe, id: StmtId) -> i32 {
    let save_sql = (p.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) != 0;
    let mut rc: i32;

    while p.e_vdbe_state != VDBE_RUN_STATE {
        if p.e_vdbe_state == VDBE_READY_STATE {
            if p.expired != 0 {
                p.rc = SQLITE_SCHEMA;
                rc = SQLITE_ERROR;
                if save_sql {
                    // Se o comando foi preparado com o SQL guardado e houve erro, devolve o
                    // código de `p.rc` e grava o mesmo código na conexão.
                    rc = vdbe_transfer_error(p, db);
                }
                return rc & db.err_mask;
            }

            // Se nenhum outro comando está rodando, limpa o sinal de interrupção: um
            // `sqlite3_interrupt()` não interrompe um comando que ainda não começou.
            if db.n_vdbe_active == 0 {
                db.interrupted.store(false, Ordering::Relaxed);
            }

            debug_assert!(
                db.n_vdbe_write > 0
                    || db.auto_commit == 0
                    || (db.n_deferred_cons == 0 && db.n_deferred_imm_cons == 0)
            );

            if (db.m_trace & (SQLITE_TRACE_PROFILE as u8 | SQLITE_TRACE_XPROFILE)) != 0
                && db.init.busy == 0
                && p.z_sql.is_some()
            {
                if let Some(vfs) = db.p_vfs.as_ref() {
                    os_current_time_int64(&**vfs, &mut p.start_time);
                }
            } else {
                debug_assert!(p.start_time == 0);
            }

            db.n_vdbe_active += 1;
            if !p.read_only {
                db.n_vdbe_write += 1;
            }
            if p.b_is_reader {
                db.n_vdbe_read += 1;
            }
            p.pc = 0;
            p.e_vdbe_state = VDBE_RUN_STATE;
        } else if p.e_vdbe_state == VDBE_HALT_STATE {
            // Antes da 3.7.0 era preciso chamar `sqlite3_reset()` antes de repetir o step depois
            // de um erro ou de `SQLITE_DONE`; hoje o reset é automático (`SQLITE_OMIT_AUTORESET`
            // desligado).
            reset_vdbe(db, p, id);
            debug_assert!(p.e_vdbe_state == VDBE_READY_STATE);
        } else {
            break;
        }
    }

    rc = if p.explain != 0 {
        vdbe_list(p, db)
    } else {
        db.n_vdbe_exec += 1;
        let r = vdbe_exec(db, p);
        db.n_vdbe_exec -= 1;
        r
    };

    if rc == SQLITE_ROW {
        debug_assert!(p.rc == SQLITE_OK);
        debug_assert!(db.malloc_failed == 0);
        db.err_code = SQLITE_ROW;
        return SQLITE_ROW;
    }
    // Se o comando terminou com sucesso, chama o gancho de perfil.
    check_profile_callback(db, p, id);
    p.p_result_row = None;
    if rc == SQLITE_DONE && db.auto_commit != 0 {
        debug_assert!(p.rc == SQLITE_OK);
        p.rc = do_wal_callbacks(db);
        if p.rc != SQLITE_OK {
            rc = SQLITE_ERROR;
        }
    } else if rc != SQLITE_DONE && save_sql {
        // Comando preparado com SQL guardado e erro: devolve o código de `p.rc`.
        rc = vdbe_transfer_error(p, db);
    }

    db.err_code = rc;
    if SQLITE_NOMEM == crate::main::api_exit(db, p.rc) {
        p.rc = SQLITE_NOMEM_BKPT;
        if save_sql {
            rc = p.rc;
        }
    }
    // Os comandos do `sqlite3_prepare()` antigo só devolvem um conjunto limitado de códigos.
    debug_assert!(
        save_sql
            || rc == SQLITE_ROW
            || rc == SQLITE_DONE
            || rc == SQLITE_ERROR
            || (rc & 0xff) == SQLITE_BUSY
            || rc == SQLITE_MISUSE
    );
    rc & db.err_mask
}

/// Retira o comando do slab, roda [`step_vdbe`] e o devolve à vaga.
fn step_once(db: &mut Connection, id: StmtId) -> i32 {
    let Some(mut v) = db.stmts.take(id.slot()) else {
        return SQLITE_MISUSE;
    };
    let rc = step_vdbe(db, &mut v, id);
    db.stmts.put(id.slot(), v);
    rc
}

/// `sqlite3_step`: executa o comando até produzir uma linha, terminar ou falhar. Se o esquema
/// mudou, recompila (`sqlite3Reprepare`) e tenta de novo.
pub fn step(db: &mut Connection, id: StmtId) -> i32 {
    if vdbe_safety(db, id) {
        return SQLITE_MISUSE;
    }
    let mut cnt = 0;
    let mut rc;
    loop {
        rc = step_once(db, id);
        if rc != SQLITE_SCHEMA {
            break;
        }
        let tries = cnt;
        cnt += 1;
        if tries >= SQLITE_MAX_SCHEMA_RETRY {
            break;
        }
        let saved_pc = db.stmt(id).map_or(-1, |v| v.pc);
        rc = crate::prepare::reprepare(db, id);
        if rc != SQLITE_OK {
            // Falhou ao recompilar o SQL. A mensagem do compilador já está na conexão: copia
            // para o comando e põe o contador de programa em 0, para que o `finalize` ou o
            // `reset` mostrem o erro do analisador em `sqlite3_errmsg()` e `sqlite3_errcode()`.
            let z_err = db.err_msg.clone();
            if db.malloc_failed == 0 {
                rc = crate::main::api_exit(db, rc);
                if let Some(v) = db.stmt_mut(id) {
                    v.z_err_msg = z_err;
                    v.rc = rc;
                }
            } else {
                rc = SQLITE_NOMEM_BKPT;
                if let Some(v) = db.stmt_mut(id) {
                    v.z_err_msg = None;
                    v.rc = rc;
                }
            }
            break;
        }
        reset(db, id);
        if saved_pc >= 0 {
            // `minWriteFileFormat` 254 sinaliza ao `OP_Init` e ao `OP_Trace` que NÃO devem
            // repetir o `SQLITE_TRACE_STMT`, já feito numa execução anterior que falhou por
            // `SQLITE_SCHEMA` (tag-20220401a).
            if let Some(v) = db.stmt_mut(id) {
                v.min_write_file_format = 254;
            }
        }
        debug_assert!(db.stmt(id).map_or(true, |v| v.expired == 0));
    }
    rc
}

/// `sqlite3_user_data`: o `pUserData` da função que está rodando.
pub fn user_data(ctx: &Context<'_>) -> UserData {
    ctx.arg_func.p_user_data.clone()
}

/// `sqlite3_context_db_handle`: a conexão que registrou a função.
pub fn context_db_handle<'a, 'b>(ctx: &'a mut Context<'b>) -> &'a mut Connection {
    ctx.db
}

/// `sqlite3_vtab_nochange`: dentro de `xColumn` de uma tabela virtual, verdadeiro se a chamada
/// é de um UPDATE que não vai alterar o valor da coluna.
pub fn vtab_nochange(ctx: &Context<'_>) -> bool {
    value_nochange(&ctx.out)
}

/// `sqlite3StmtCurrentTime`: o instante do comando. A primeira chamada de uma execução fixa o
/// valor, e as seguintes devolvem o mesmo.
pub fn stmt_current_time(ctx: &mut Context<'_>) -> i64 {
    if *ctx.i_current_time == 0 {
        let mut t = 0i64;
        let rc = match ctx.db.p_vfs.as_ref() {
            Some(vfs) => os_current_time_int64(&**vfs, &mut t),
            None => SQLITE_ERROR,
        };
        *ctx.i_current_time = if rc != 0 { 0 } else { t };
    }
    *ctx.i_current_time
}

// ---------------------------------------------------------------------------------------------
// chunk 003: agregados, auxdata, colunas
// ---------------------------------------------------------------------------------------------

/// `sqlite3_aggregate_context`: o acumulador do agregado, um `T` criado com `T::default()` na
/// primeira chamada com `create` (o `nByte > 0` do C) e devolvido igual nas seguintes. Sem
/// `create` (o `nByte <= 0`) devolve `None` se ainda não existe. `None` também se o acumulador
/// existente é de outro tipo.
pub fn aggregate_context<'c, T: Default + 'static>(
    ctx: &'c mut Context<'_>,
    create: bool,
) -> Option<&'c mut T> {
    if ctx.agg.is_none() {
        // `createAggContext`: com `nByte <= 0` a célula do acumulador só fica NULL.
        if !create {
            return None;
        }
        ctx.agg = Some(Box::new(T::default()));
    }
    ctx.agg.as_mut().and_then(|b| b.downcast_mut::<T>())
}

/// `sqlite3_get_auxdata`: o dado auxiliar do `i_arg`-ésimo argumento da função (o mais à
/// esquerda é 0). Com `i_arg` negativo, um cache comum a todas as funções do comando.
pub fn get_auxdata(ctx: &Context<'_>, i_arg: i32) -> Option<Rc<dyn Any>> {
    // O mais novo vem primeiro, como na lista encadeada do C.
    for a in ctx.p_aux_data.iter().rev() {
        if a.i_aux_arg == i_arg && (a.i_aux_op == ctx.i_op || i_arg < 0) {
            return a.p_aux.clone();
        }
    }
    None
}

/// `sqlite3_set_auxdata`: grava o dado auxiliar e o destrutor do `i_arg`-ésimo argumento. O dado
/// anterior é destruído pelo destrutor que veio com ele.
pub fn set_auxdata(
    ctx: &mut Context<'_>,
    i_arg: i32,
    p_aux: Option<Rc<dyn Any>>,
    x_delete: Option<DestroyFn>,
) {
    let i_op = ctx.i_op;
    let found = ctx
        .p_aux_data
        .iter()
        .rposition(|a| a.i_aux_arg == i_arg && (a.i_aux_op == i_op || i_arg < 0));
    let slot = match found {
        None => {
            ctx.p_aux_data.push(AuxData {
                i_aux_op: i_op,
                i_aux_arg: i_arg,
                p_aux: None,
                x_delete_aux: None,
            });
            if ctx.is_error == 0 {
                ctx.is_error = -1;
            }
            ctx.p_aux_data.len() - 1
        }
        Some(i) => {
            let old = &mut ctx.p_aux_data[i];
            if let Some(destroy) = old.x_delete_aux.take() {
                destroy(old.p_aux.take());
            }
            i
        }
    };
    let a = &mut ctx.p_aux_data[slot];
    a.p_aux = p_aux;
    a.x_delete_aux = x_delete;
}

/// `sqlite3_column_count`: o número de colunas do resultado.
pub fn column_count(db: &Connection, id: StmtId) -> i32 {
    db.stmt(id).map_or(0, |v| v.n_res_column as i32)
}

/// `sqlite3_data_count`: o número de valores da linha corrente.
pub fn data_count(db: &Connection, id: StmtId) -> i32 {
    match db.stmt(id) {
        Some(v) if v.p_result_row.is_some() => v.n_res_column as i32,
        _ => 0,
    }
}

/// `columnMem`: o índice em `a_mem` da célula da coluna `i` da linha corrente. `None` é a
/// célula NULL estática do C (`columnNullValue`); fora de faixa grava `SQLITE_RANGE`.
fn column_index(db: &mut Connection, id: StmtId, i: i32) -> Option<usize> {
    let (row, n) = {
        let v = db.stmt(id)?;
        (v.p_result_row, v.n_res_column as i32)
    };
    match row {
        Some(r) if i < n && i >= 0 => Some(r + i as usize),
        _ => {
            crate::main::error(db, SQLITE_RANGE);
            None
        }
    }
}

/// `columnMallocFailure`: chamada depois de um `sqlite3_value_*` numa coluna que pode ter falhado
/// por falta de memória: o código do comando passa por `sqlite3ApiExit`.
fn column_malloc_failure(db: &mut Connection, id: StmtId) {
    if let Some(rc) = db.stmt(id).map(|p| p.rc) {
        let rc = crate::main::api_exit(db, rc);
        if let Some(p) = db.stmt_mut(id) {
            p.rc = rc;
        }
    }
}

/// Aplica `f` à célula da coluna `i` (ou a um NULL, se a coluna não existe) e fecha com
/// `columnMallocFailure`.
fn with_column<R>(db: &mut Connection, id: StmtId, i: i32, f: impl FnOnce(&mut Mem) -> R) -> R {
    let r = match column_index(db, id, i) {
        Some(ix) => match db.stmt_mut(id) {
            Some(v) => f(&mut v.a_mem[ix]),
            None => f(&mut Mem::value_new()),
        },
        None => f(&mut Mem::value_new()),
    };
    column_malloc_failure(db, id);
    r
}

/// Como [`with_column`], mas devolve a fatia de bytes da célula quando `f` diz que há valor. A
/// fatia é tomada depois do `columnMallocFailure`, para poder ficar emprestada ao chamador.
fn column_slice(
    db: &mut Connection,
    id: StmtId,
    i: i32,
    f: impl FnOnce(&mut Mem) -> bool,
) -> Option<&[u8]> {
    if !with_column(db, id, i, f) {
        return None;
    }
    let v = db.stmt(id)?;
    let ix = v.p_result_row? + i as usize;
    Some(v.a_mem[ix].bytes())
}

/// `sqlite3_column_blob`.
pub fn column_blob(db: &mut Connection, id: StmtId, i: i32) -> Option<&[u8]> {
    column_slice(db, id, i, |m| value_blob(m).is_some())
}

/// `sqlite3_column_bytes`.
pub fn column_bytes(db: &mut Connection, id: StmtId, i: i32) -> i32 {
    with_column(db, id, i, value_bytes)
}

/// `sqlite3_column_bytes16`.
pub fn column_bytes16(db: &mut Connection, id: StmtId, i: i32) -> i32 {
    with_column(db, id, i, value_bytes16)
}

/// `sqlite3_column_double`.
pub fn column_double(db: &mut Connection, id: StmtId, i: i32) -> f64 {
    with_column(db, id, i, |m| value_double(m))
}

/// `sqlite3_column_int`.
pub fn column_int(db: &mut Connection, id: StmtId, i: i32) -> i32 {
    with_column(db, id, i, |m| value_int(m))
}

/// `sqlite3_column_int64`.
pub fn column_int64(db: &mut Connection, id: StmtId, i: i32) -> i64 {
    with_column(db, id, i, |m| value_int64(m))
}

/// `sqlite3_column_text`: o texto UTF-8 da coluna (sem o terminador). `None` para NULL.
pub fn column_text(db: &mut Connection, id: StmtId, i: i32) -> Option<&[u8]> {
    column_slice(db, id, i, |m| value_text(m).is_some())
}

/// `sqlite3_column_text16`: o texto da coluna em UTF-16 nativo.
pub fn column_text16(db: &mut Connection, id: StmtId, i: i32) -> Option<&[u8]> {
    column_slice(db, id, i, |m| value_text16(m).is_some())
}

/// `sqlite3_column_value`: uma cópia da célula da coluna. Texto `MEM_Static` passa a
/// `MEM_Ephem`, como no C, antes da cópia.
pub fn column_value(db: &mut Connection, id: StmtId, i: i32) -> Mem {
    with_column(db, id, i, |m| {
        if m.flags & MEM_STATIC != 0 {
            m.flags &= !MEM_STATIC;
            m.flags |= MEM_EPHEM;
        }
        m.clone()
    })
}

/// `sqlite3_column_type`: um de `SQLITE_INTEGER`, `FLOAT`, `TEXT`, `BLOB` ou `NULL`.
pub fn column_type(db: &mut Connection, id: StmtId, i: i32) -> i32 {
    with_column(db, id, i, |m| value_type(m))
}

/// `azExplainColNames8`: nomes de coluna do EXPLAIN (os 8 primeiros) e do EXPLAIN QUERY PLAN
/// (os 4 últimos). A versão UTF-16 (`azExplainColNames16data` com `iExplainColNames16`) é a
/// mesma tabela em UTF-16 nativo, gerada por alargamento (os nomes são ASCII).
const AZ_EXPLAIN_COL_NAMES8: [&str; 12] = [
    "addr", "opcode", "p1", "p2", "p3", "p4", "p5", "comment", // EXPLAIN
    "id", "parent", "notused", "detail", // EQP
];

/// `columnName`: o `n`-ésimo nome de `pColName`. `use_type` escolhe qual dos até 5 nomes:
/// 0 o nome para exibição, 1 o tipo declarado, 2 o banco, 3 a tabela, 4 a coluna de origem
/// (2 a 4 são NULL se o resultado não é uma referência simples a coluna). `None` fora de faixa.
fn column_name_of(
    db: &mut Connection,
    id: StmtId,
    n: i32,
    use_utf16: bool,
    use_type: i32,
) -> Option<Vec<u8>> {
    if n < 0 {
        return None;
    }
    let (explain, n_res) = {
        let p = db.stmt(id)?;
        (p.explain as i32, p.n_res_column as i32)
    };
    if explain != 0 {
        if use_type > 0 {
            return None;
        }
        let n_names = if explain == 1 { 8 } else { 4 };
        if n >= n_names {
            return None;
        }
        let name = AZ_EXPLAIN_COL_NAMES8[(n + 8 * explain - 8) as usize];
        return Some(if use_utf16 {
            name.bytes().flat_map(|b| [b, 0]).collect()
        } else {
            name.as_bytes().to_vec()
        });
    }
    if n >= n_res {
        return None;
    }
    let prior_malloc_failed = db.malloc_failed;
    let ix = (n + use_type * n_res) as usize;
    let enc = if use_utf16 { ENC_NATIVE } else { ENC_UTF8 };
    let ret = db
        .stmt_mut(id)
        .and_then(|p| p.a_col_name.get_mut(ix))
        .and_then(|m| mem_value_text(m, enc).map(|b| b.to_vec()));
    // Um malloc pode ter falhado dentro de `_text()`: limpa o sinal e devolve NULL.
    debug_assert!(db.malloc_failed == 0 || db.malloc_failed == 1);
    if db.malloc_failed > prior_malloc_failed {
        crate::util::oom_clear(db);
        return None;
    }
    ret
}

// ---------------------------------------------------------------------------------------------
// chunk 004: nomes de colunas e sqlite3_bind_*
// ---------------------------------------------------------------------------------------------

/// `sqlite3_column_name`.
pub fn column_name(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, false, COLNAME_NAME)
}

/// `sqlite3_column_name16`.
pub fn column_name16(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, true, COLNAME_NAME)
}

/// `sqlite3_column_decltype`: o tipo declarado da coluna de origem.
pub fn column_decltype(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, false, COLNAME_DECLTYPE)
}

/// `sqlite3_column_decltype16`.
pub fn column_decltype16(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, true, COLNAME_DECLTYPE)
}

/// `sqlite3_column_database_name`: o banco de onde a coluna do resultado vem.
pub fn column_database_name(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, false, COLNAME_DATABASE)
}

/// `sqlite3_column_database_name16`.
pub fn column_database_name16(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, true, COLNAME_DATABASE)
}

/// `sqlite3_column_table_name`: a tabela de onde a coluna do resultado vem.
pub fn column_table_name(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, false, COLNAME_TABLE)
}

/// `sqlite3_column_table_name16`.
pub fn column_table_name16(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, true, COLNAME_TABLE)
}

/// `sqlite3_column_origin_name`: a coluna da tabela de onde a coluna do resultado vem.
pub fn column_origin_name(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, false, COLNAME_COLUMN)
}

/// `sqlite3_column_origin_name16`.
pub fn column_origin_name16(db: &mut Connection, id: StmtId, n: i32) -> Option<Vec<u8>> {
    column_name_of(db, id, n, true, COLNAME_COLUMN)
}

/// `vdbeUnbind`: desliga o valor da variável `i` (a base é 0): o mesmo que ligar um NULL.
/// `SQLITE_RANGE` se `i` está fora de faixa. O código de erro da conexão é sobrescrito com o
/// resultado em qualquer caso.
fn vdbe_unbind(db: &mut Connection, id: StmtId, i: u32) -> i32 {
    if vdbe_safety(db, id) {
        return SQLITE_MISUSE;
    }
    let (state, n_var, z_sql) = match db.stmt(id) {
        Some(p) => (p.e_vdbe_state, p.n_var, p.z_sql.clone()),
        None => return SQLITE_MISUSE,
    };
    if state != VDBE_READY_STATE {
        crate::main::error(db, SQLITE_MISUSE);
        log(
            SQLITE_MISUSE,
            b"bind on a busy prepared statement: [%s]",
            &[PrintfArg::Text(z_sql)],
        );
        return SQLITE_MISUSE;
    }
    if i >= n_var.max(0) as u32 {
        crate::main::error(db, SQLITE_RANGE);
        return SQLITE_RANGE;
    }
    db.err_code = SQLITE_OK;
    let Some(p) = db.stmts.get_mut(id.slot()) else {
        return SQLITE_MISUSE;
    };
    let var = &mut p.a_var[i as usize];
    mem_release(var);
    var.flags = MEM_NULL;

    // Se o bit desta variável está em `exp_mask`, ligar um valor novo invalida o plano de
    // consulta (IMPLEMENTATION-OF R-57496-20354): o comando é recompilado como se o esquema
    // tivesse mudado, no primeiro `sqlite3_step()` depois da mudança.
    debug_assert!((p.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) != 0 || p.exp_mask == 0);
    if p.exp_mask != 0 && (p.exp_mask & (if i >= 31 { 0x8000_0000 } else { 1u32 << i })) != 0 {
        p.expired = 1;
    }
    SQLITE_OK
}

/// A célula da variável `i` (a base é 1) depois de um `vdbe_unbind` bem-sucedido.
fn bound_var(db: &mut Connection, id: StmtId, i: i32) -> Option<&mut Mem> {
    db.stmts.get_mut(id.slot()).and_then(|p| p.a_var.get_mut((i - 1) as usize))
}

/// `bindText`: liga um texto ou blob (`enc` 0).
fn bind_text_impl(
    db: &mut Connection,
    id: StmtId,
    i: i32,
    z: Option<&[u8]>,
    n: i64,
    x_del: StrDtor,
    encoding: u8,
) -> i32 {
    let mut rc = vdbe_unbind(db, id, i.wrapping_sub(1) as u32);
    if rc == SQLITE_OK && z.is_some() {
        let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
        let enc_db = db.enc as i32;
        if let Some(var) = bound_var(db, id, i) {
            rc = mem_set_str(var, z, n, encoding, x_del, limit);
            if rc == SQLITE_OK && encoding != 0 {
                rc = vdbe_change_encoding(var, enc_db);
            }
        }
        if rc != 0 {
            crate::main::error(db, rc);
            rc = crate::main::api_exit(db, rc);
        }
    }
    rc
}

/// `sqlite3_bind_blob`.
pub fn bind_blob(db: &mut Connection, id: StmtId, i: i32, z: Option<&[u8]>, n: i32, x_del: StrDtor) -> i32 {
    bind_text_impl(db, id, i, z, n as i64, x_del, 0)
}

/// `sqlite3_bind_blob64`.
pub fn bind_blob64(db: &mut Connection, id: StmtId, i: i32, z: Option<&[u8]>, n: u64, x_del: StrDtor) -> i32 {
    bind_text_impl(db, id, i, z, n as i64, x_del, 0)
}

/// `sqlite3_bind_double`.
pub fn bind_double(db: &mut Connection, id: StmtId, i: i32, r_value: f64) -> i32 {
    let rc = vdbe_unbind(db, id, i.wrapping_sub(1) as u32);
    if rc == SQLITE_OK {
        if let Some(v) = bound_var(db, id, i) {
            mem_set_double(v, r_value);
        }
    }
    rc
}

/// `sqlite3_bind_int`.
pub fn bind_int(db: &mut Connection, id: StmtId, i: i32, i_value: i32) -> i32 {
    bind_int64(db, id, i, i_value as i64)
}

/// `sqlite3_bind_int64`.
pub fn bind_int64(db: &mut Connection, id: StmtId, i: i32, i_value: i64) -> i32 {
    let rc = vdbe_unbind(db, id, i.wrapping_sub(1) as u32);
    if rc == SQLITE_OK {
        if let Some(v) = bound_var(db, id, i) {
            mem_set_int64(v, i_value);
        }
    }
    rc
}

/// `sqlite3_bind_null`.
pub fn bind_null(db: &mut Connection, id: StmtId, i: i32) -> i32 {
    vdbe_unbind(db, id, i.wrapping_sub(1) as u32)
}

/// `sqlite3_bind_text`: `n < 0` mede o texto até o primeiro NUL.
pub fn bind_text(db: &mut Connection, id: StmtId, i: i32, z: Option<&[u8]>, n: i32, x_del: StrDtor) -> i32 {
    bind_text_impl(db, id, i, z, n as i64, x_del, ENC_UTF8)
}

/// `sqlite3_bind_text64`.
pub fn bind_text64(
    db: &mut Connection,
    id: StmtId,
    i: i32,
    z: Option<&[u8]>,
    n: u64,
    x_del: StrDtor,
    enc: u8,
) -> i32 {
    let mut enc = enc;
    let mut n = n;
    if enc as i32 != SQLITE_UTF8 {
        if enc as i32 == SQLITE_UTF16 {
            enc = ENC_NATIVE;
        }
        n &= !1u64;
    }
    bind_text_impl(db, id, i, z, n as i64, x_del, enc)
}

/// `sqlite3_bind_text16`.
pub fn bind_text16(db: &mut Connection, id: StmtId, i: i32, z: Option<&[u8]>, n: i32, x_del: StrDtor) -> i32 {
    bind_text_impl(db, id, i, z, (n as i64) & !1, x_del, ENC_NATIVE)
}

/// `sqlite3_bind_value`.
pub fn bind_value(db: &mut Connection, id: StmtId, i: i32, value: &Mem) -> i32 {
    match value_type(value) {
        t if t == SQLITE_INTEGER => bind_int64(db, id, i, value.u_i),
        t if t == SQLITE_FLOAT => {
            debug_assert!(value.flags & (MEM_REAL | MEM_INTREAL) != 0);
            let r = if value.flags & MEM_REAL != 0 { value.u_r } else { value.u_i as f64 };
            bind_double(db, id, i, r)
        }
        t if t == SQLITE_BLOB => {
            if value.flags & MEM_ZERO != 0 {
                bind_zeroblob(db, id, i, value.n_zero)
            } else {
                bind_blob(db, id, i, Some(value.bytes()), value.n, StrDtor::Transient)
            }
        }
        t if t == SQLITE_TEXT => {
            bind_text_impl(db, id, i, Some(value.bytes()), value.n as i64, StrDtor::Transient, value.enc)
        }
        _ => bind_null(db, id, i),
    }
}

/// `sqlite3_bind_zeroblob`.
pub fn bind_zeroblob(db: &mut Connection, id: StmtId, i: i32, n: i32) -> i32 {
    let rc = vdbe_unbind(db, id, i.wrapping_sub(1) as u32);
    if rc == SQLITE_OK {
        if let Some(v) = bound_var(db, id, i) {
            mem_set_zero_blob(v, n);
        }
    }
    rc
}

/// `sqlite3_bind_zeroblob64`.
pub fn bind_zeroblob64(db: &mut Connection, id: StmtId, i: i32, n: u64) -> i32 {
    let rc = if n > db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u64 {
        SQLITE_TOOBIG
    } else {
        debug_assert!((n & 0x7FFF_FFFF) == n);
        bind_zeroblob(db, id, i, n as i32)
    };
    crate::main::api_exit(db, rc)
}

/// `sqlite3_bind_parameter_count`: o número de parâmetros a que se pode ligar valor.
pub fn bind_parameter_count(db: &Connection, id: StmtId) -> i32 {
    db.stmt(id).map_or(0, |p| p.n_var)
}

// ---------------------------------------------------------------------------------------------
// chunk 005: parâmetros nomeados, transferência de bindings, informações do comando
// ---------------------------------------------------------------------------------------------

/// `sqlite3_bind_parameter_name`: o nome (com o prefixo) do parâmetro `i`, base 1. `None` se o
/// índice está fora de faixa ou o parâmetro não tem nome. O resultado é sempre UTF-8.
pub fn bind_parameter_name(db: &Connection, id: StmtId, i: i32) -> Option<Vec<u8>> {
    let p = db.stmt(id)?;
    p.p_v_list.iter().find(|e| e.i_var == i).map(|e| e.name.clone())
}

/// `sqlite3VdbeParameterIndex`: o índice da variável de nome `z_name` (os bytes até o primeiro
/// NUL), ou 0 se não existe.
pub fn vdbe_parameter_index(p: &Vdbe, z_name: &[u8]) -> i32 {
    let name = &z_name[..z_name.iter().position(|&c| c == 0).unwrap_or(z_name.len())];
    for e in p.p_v_list.iter() {
        let en = &e.name[..e.name.iter().position(|&c| c == 0).unwrap_or(e.name.len())];
        if en == name {
            return e.i_var;
        }
    }
    0
}

/// `sqlite3_bind_parameter_index`: o índice do parâmetro de nome `z_name`, ou 0.
pub fn bind_parameter_index(db: &Connection, id: StmtId, z_name: &[u8]) -> i32 {
    match db.stmt(id) {
        Some(p) => vdbe_parameter_index(p, z_name),
        None => 0,
    }
}

/// `sqlite3TransferBindings`: passa todos os bindings do primeiro comando para o segundo.
pub fn transfer_bindings(from: &mut Vdbe, to: &mut Vdbe) -> i32 {
    debug_assert!(to.n_var == from.n_var);
    let n = (from.n_var.max(0) as usize).min(from.a_var.len()).min(to.a_var.len());
    for i in 0..n {
        mem_move(&mut to.a_var[i], &mut from.a_var[i]);
    }
    SQLITE_OK
}

/// `sqlite3_transfer_bindings` (obsoleta): `SQLITE_ERROR` se os comandos têm número diferente
/// de bindings. Também marca como expirado o comando cuja estratégia depende de variáveis.
pub fn api_transfer_bindings(db: &mut Connection, from_id: StmtId, to_id: StmtId) -> i32 {
    if from_id == to_id {
        return SQLITE_OK;
    }
    let (Some(mut from), Some(mut to)) =
        (db.stmts.take(from_id.slot()), db.stmts.take(to_id.slot()))
    else {
        return SQLITE_MISUSE;
    };
    let rc = if from.n_var != to.n_var {
        SQLITE_ERROR
    } else {
        debug_assert!((to.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) != 0 || to.exp_mask == 0);
        if to.exp_mask != 0 {
            to.expired = 1;
        }
        debug_assert!((from.prep_flags as u32 & SQLITE_PREPARE_SAVESQL) != 0 || from.exp_mask == 0);
        if from.exp_mask != 0 {
            from.expired = 1;
        }
        transfer_bindings(&mut from, &mut to)
    };
    db.stmts.put(from_id.slot(), from);
    db.stmts.put(to_id.slot(), to);
    rc
}

/// `sqlite3_stmt_readonly`: verdadeiro se o comando garante não modificar o banco.
pub fn stmt_readonly(db: &Connection, id: StmtId) -> bool {
    db.stmt(id).map_or(true, |p| p.read_only)
}

/// `sqlite3_stmt_isexplain`: 1 se o comando é um EXPLAIN, 2 se é um EXPLAIN QUERY PLAN.
pub fn stmt_isexplain(db: &Connection, id: StmtId) -> i32 {
    db.stmt(id).map_or(0, |p| p.explain as i32)
}

/// `sqlite3_stmt_explain`: muda o modo EXPLAIN do comando (0 normal, 1 EXPLAIN, 2 EXPLAIN QUERY
/// PLAN). Pode recompilar o comando.
pub fn stmt_explain(db: &mut Connection, id: StmtId, e_mode: i32) -> i32 {
    let (explain, prep_flags, state, n_mem, have_eqp_ops) = match db.stmt(id) {
        Some(v) => (v.explain as i32, v.prep_flags as u32, v.e_vdbe_state, v.n_mem, v.have_eqp_ops),
        None => return SQLITE_MISUSE,
    };
    let rc;
    if explain == e_mode {
        rc = SQLITE_OK;
    } else if !(0..=2).contains(&e_mode) {
        rc = SQLITE_ERROR;
    } else if (prep_flags & SQLITE_PREPARE_SAVESQL) == 0 {
        rc = SQLITE_ERROR;
    } else if state != VDBE_READY_STATE {
        rc = SQLITE_BUSY;
    } else if n_mem >= 10 && (e_mode != 2 || have_eqp_ops) {
        // Não precisa recompilar.
        if let Some(v) = db.stmt_mut(id) {
            v.explain = e_mode as u8;
        }
        rc = SQLITE_OK;
    } else {
        if let Some(v) = db.stmt_mut(id) {
            v.explain = e_mode as u8;
        }
        rc = crate::prepare::reprepare(db, id);
        if let Some(v) = db.stmt_mut(id) {
            v.have_eqp_ops = e_mode == 2;
        }
    }
    if let Some(v) = db.stmt_mut(id) {
        if v.explain != 0 {
            v.n_res_column = (12 - 4 * v.explain as i32) as u16;
        } else {
            v.n_res_column = v.n_res_alloc;
        }
    }
    rc
}

/// `sqlite3_stmt_busy`: verdadeiro se o comando precisa de um `sqlite3_reset()`.
pub fn stmt_busy(db: &Connection, id: StmtId) -> bool {
    db.stmt(id).is_some_and(|v| v.e_vdbe_state == VDBE_RUN_STATE)
}

/// `sqlite3_next_stmt`: o comando preparado seguinte a `id` na conexão; sem `id`, o primeiro.
/// A lista do C tem o mais novo na frente, e `Connection.stmt_list` guarda do mais antigo ao
/// mais novo, então o "seguinte" é o elemento anterior do vetor.
pub fn next_stmt(db: &Connection, id: Option<StmtId>) -> Option<StmtId> {
    match id {
        None => db.stmt_list.last().copied(),
        Some(cur) => {
            let pos = db.stmt_list.iter().position(|s| *s == cur)?;
            if pos > 0 {
                Some(db.stmt_list[pos - 1])
            } else {
                None
            }
        }
    }
}

/// Bytes por estrutura do C (x86-64, Debian) usados para estimar a memória de um comando.
const SZ_VDBE: u64 = 360;
const SZ_OP: u64 = 40;
const SZ_MEM: u64 = 56;
const SZ_VDBE_CURSOR: u64 = 120;
const SZ_SCAN_STATUS: u64 = 56;

/// A estimativa de `SQLITE_STMTSTATUS_MEMUSED`. O C mede os bytes liberados por um
/// `sqlite3VdbeDelete` simulado (`db->pnBytesFreed`); como a memória aqui é do alocador do Rust,
/// a conta usa os tamanhos das estruturas do C para os mesmos objetos.
fn stmt_mem_used(v: &Vdbe) -> u64 {
    let mut n = SZ_VDBE;
    n += v.a_op.len() as u64 * SZ_OP;
    n += v.a_mem.len() as u64 * SZ_MEM;
    n += v.a_var.len() as u64 * SZ_MEM;
    n += v.a_col_name.len() as u64 * SZ_MEM;
    n += v.ap_csr.iter().flatten().count() as u64 * SZ_VDBE_CURSOR;
    n += v.a_scan.len() as u64 * SZ_SCAN_STATUS;
    n += v.z_sql.as_ref().map_or(0, |z| z.len() as u64 + 1);
    n += v.z_err_msg.as_ref().map_or(0, |z| z.len() as u64 + 1);
    for m in v.a_mem.iter().chain(v.a_var.iter()).chain(v.a_col_name.iter()) {
        n += m.sz_malloc.max(0) as u64;
    }
    n
}

/// `sqlite3_stmt_status`: o valor de um contador do comando; com `reset_flag` o contador volta
/// a zero (menos `SQLITE_STMTSTATUS_MEMUSED`, que não é contador).
pub fn stmt_status(db: &mut Connection, id: StmtId, op: i32, reset_flag: bool) -> i32 {
    let Some(v) = db.stmt_mut(id) else {
        return 0;
    };
    if op == SQLITE_STMTSTATUS_MEMUSED {
        return stmt_mem_used(v) as u32 as i32;
    }
    let Some(slot) = usize::try_from(op).ok().filter(|&o| o < v.a_counter.len()) else {
        return 0;
    };
    let r = v.a_counter[slot];
    if reset_flag {
        v.a_counter[slot] = 0;
    }
    r as i32
}

/// `sqlite3_sql`: o SQL que gerou o comando.
pub fn sql(db: &Connection, id: StmtId) -> Option<&[u8]> {
    db.stmt(id)?.z_sql.as_deref()
}

/// `sqlite3_expanded_sql`: o SQL do comando com os parâmetros ligados expandidos.
pub fn expanded_sql(db: &Connection, id: StmtId) -> Option<Vec<u8>> {
    let p = db.stmt(id)?;
    let z_sql = p.z_sql.as_deref()?;
    crate::vdbetrace::expand_sql(db, p, z_sql)
}

// ---------------------------------------------------------------------------------------------
// chunk 005 (final) e 006: pré-atualização (SQLITE_ENABLE_PREUPDATE_HOOK)
// ---------------------------------------------------------------------------------------------

/// `vdbeUnpackRecord`: o registro serializado `key` desempacotado segundo `key_info`.
fn vdbe_unpack_record(key_info: &Rc<KeyInfo>, key: &[u8]) -> Box<UnpackedRecord> {
    let mut ret = alloc_unpacked_record(Rc::clone(key_info));
    record_unpack(key_info, key, &mut ret);
    Box::new(ret)
}

/// O `pCsr->nField` do C: o número de campos do registro que o cursor lê (as colunas da tabela,
/// ou as do índice da chave primária numa tabela WITHOUT ROWID).
fn preupdate_n_field(p: &PreUpdate) -> i32 {
    match (&p.p_pk, &p.p_tab) {
        (Some(pk), _) => pk.n_column as i32,
        (None, Some(t)) => t.n_col as i32,
        _ => 0,
    }
}

/// `sqlite3TableColumnToIndex(p->pPk, iIdx)` com o `int` do C.
fn pk_column_to_index(p: &PreUpdate, i_idx: i32) -> i32 {
    match &p.p_pk {
        Some(pk) => i16::try_from(i_idx).map_or(-1, |c| table_column_to_index(pk, c) as i32),
        None => i_idx,
    }
}

/// `sqlite3_preupdate_old`: o valor da coluna `i_idx` da linha antiga, dentro de um gancho de
/// UPDATE ou DELETE. `p` é o `PreUpdate` que o gancho recebeu. Devolve o código de resultado e o
/// valor (uma cópia da célula, o C devolve o ponteiro dela).
pub fn preupdate_old(
    db: &mut Connection,
    p: &mut PreUpdate,
    i_idx: i32,
) -> (i32, Option<Mem>) {
    let mut i_idx = i_idx;
    let mut value: Option<Mem> = None;
    let rc = 'out: {
        // Só vale dentro de um gancho de DELETE ou UPDATE, e com `i_idx` em faixa.
        if p.op == SQLITE_INSERT {
            break 'out SQLITE_MISUSE;
        }
        if p.p_pk.is_some() {
            i_idx = pk_column_to_index(p, i_idx);
        }
        if i_idx >= preupdate_n_field(p) || i_idx < 0 {
            break 'out SQLITE_RANGE;
        }

        // Se o registro `old.*` ainda não está na memória, carrega agora.
        if p.p_unpacked.is_none() {
            let Some(key_info) = p.keyinfo.clone() else {
                break 'out SQLITE_MISUSE;
            };
            let Some(cid) = p.cursor else {
                break 'out SQLITE_MISUSE;
            };
            let Some(bt) = db.dbs.get_mut(p.i_db as usize).and_then(|s| s.bt.as_mut()) else {
                break 'out SQLITE_MISUSE;
            };
            let Some(mut cur) = bt.bt.cursors.take(cid) else {
                break 'out SQLITE_MISUSE;
            };
            let n_rec = btree_payload_size(&mut cur, &mut bt.bt);
            let mut a_rec = vec![0u8; n_rec as usize];
            let rc = btree_payload(&mut cur, &mut bt.bt, 0, n_rec, &mut a_rec);
            bt.bt.cursors.put(cid, cur);
            if rc != SQLITE_OK {
                break 'out rc;
            }
            p.p_unpacked = Some(vdbe_unpack_record(&key_info, &a_rec));
            p.a_record = a_rec;
        }

        let i_key1 = p.i_key1;
        let i_pkey = p.p_tab.as_ref().map_or(-1, |t| t.i_p_key as i32);
        let aff_real = p
            .p_tab
            .as_ref()
            .and_then(|t| t.a_col.get(i_idx as usize))
            .is_some_and(|c| c.affinity == SQLITE_AFF_REAL);
        let Some(unp) = p.p_unpacked.as_mut() else {
            break 'out SQLITE_MISUSE;
        };
        let n_field = unp.n_field as i32;
        match unp.a_mem.get_mut(i_idx as usize) {
            None => value = Some(Mem::value_new()),
            Some(m) => {
                if i_idx == i_pkey {
                    mem_set_int64(m, i_key1);
                    value = Some(m.clone());
                } else if i_idx >= n_field {
                    value = Some(Mem::value_new());
                } else {
                    if aff_real && m.flags & (MEM_INT | MEM_INTREAL) != 0 {
                        mem_realify(m);
                    }
                    value = Some(m.clone());
                }
            }
        }
        SQLITE_OK
    };
    crate::main::error(db, rc);
    (crate::main::api_exit(db, rc), value)
}

/// `sqlite3_preupdate_count`: o número de colunas da linha sendo atualizada, apagada ou inserida.
pub fn preupdate_count(p: &PreUpdate) -> i32 {
    p.keyinfo.as_ref().map_or(0, |k| k.n_key_field as i32)
}

/// `sqlite3_preupdate_depth`: 0 se a alteração é de um comando SQL do usuário; senão o número de
/// programas de gatilho empilhados (1 para um gatilho de nível superior, 2 para um disparado por
/// ele, etc.). Uma ação de chave estrangeira CASCADE, SET NULL ou SET DEFAULT conta como gatilho.
/// `v` é o comando em execução (o `p->v` do C).
pub fn preupdate_depth(v: &Vdbe) -> i32 {
    v.p_frame.len() as i32
}

/// `sqlite3_preupdate_blobwrite`.
pub fn preupdate_blobwrite(p: &PreUpdate) -> i32 {
    p.i_blob_write
}

/// `sqlite3_preupdate_new`: o valor da coluna `i_idx` da linha nova, dentro de um gancho de
/// UPDATE ou INSERT. `v` é o comando em execução (o `p->v` do C), de onde vêm os registros
/// `new.*`. Devolve o código de resultado e uma cópia do valor.
pub fn preupdate_new(
    db: &mut Connection,
    p: &mut PreUpdate,
    v: &Vdbe,
    i_idx: i32,
) -> (i32, Option<Mem>) {
    let mut i_idx = i_idx;
    let mut value: Option<Mem> = None;
    let rc = 'out: {
        if p.op == SQLITE_DELETE {
            break 'out SQLITE_MISUSE;
        }
        if p.p_pk.is_some() && p.op != SQLITE_UPDATE {
            i_idx = pk_column_to_index(p, i_idx);
        }
        let n_field = preupdate_n_field(p);
        if i_idx >= n_field || i_idx < 0 {
            break 'out SQLITE_RANGE;
        }
        let i_pkey = p.p_tab.as_ref().map_or(-1, |t| t.i_p_key as i32);

        if p.op == SQLITE_INSERT {
            // Num INSERT, o registro `p.i_new_reg` guarda o registro serializado: desempacota.
            if p.p_new_unpacked.is_none() {
                let Some(src) = v.a_mem.get(p.i_new_reg as usize) else {
                    break 'out SQLITE_MISUSE;
                };
                let mut data = src.clone();
                if data.flags & MEM_ZERO != 0 {
                    let rc = mem_expand_blob(&mut data);
                    if rc != SQLITE_OK {
                        break 'out rc;
                    }
                }
                let Some(key_info) = p.keyinfo.clone() else {
                    break 'out SQLITE_MISUSE;
                };
                p.p_new_unpacked = Some(vdbe_unpack_record(&key_info, data.bytes()));
            }
            let i_key2 = p.i_key2;
            let Some(unpack) = p.p_new_unpacked.as_mut() else {
                break 'out SQLITE_MISUSE;
            };
            let un_field = unpack.n_field as i32;
            match unpack.a_mem.get_mut(i_idx as usize) {
                None => value = Some(Mem::value_new()),
                Some(m) => {
                    if i_idx == i_pkey {
                        mem_set_int64(m, i_key2);
                        value = Some(m.clone());
                    } else if i_idx >= un_field {
                        value = Some(Mem::value_new());
                    } else {
                        value = Some(m.clone());
                    }
                }
            }
        } else {
            // Num UPDATE, a célula `i_new_reg + 1 + i_idx` tem o valor. Copia-se a célula, pois
            // quem chama pode mudar a codificação do texto.
            debug_assert!(p.op == SQLITE_UPDATE);
            if p.a_new.is_empty() {
                p.a_new = (0..n_field).map(|_| Mem::default()).collect();
            }
            let i_key2 = p.i_key2;
            let src_ix = p.i_new_reg as usize + 1 + i_idx as usize;
            let Some(m) = p.a_new.get_mut(i_idx as usize) else {
                break 'out SQLITE_RANGE;
            };
            if m.flags == 0 {
                if i_idx == i_pkey {
                    mem_set_int64(m, i_key2);
                } else {
                    match v.a_mem.get(src_ix) {
                        Some(src) => {
                            let rc = mem_copy(m, src);
                            if rc != SQLITE_OK {
                                break 'out rc;
                            }
                        }
                        None => break 'out SQLITE_RANGE,
                    }
                }
            }
            value = Some(m.clone());
        }
        SQLITE_OK
    };
    crate::main::error(db, rc);
    (crate::main::api_exit(db, rc), value)
}

// ---------------------------------------------------------------------------------------------
// chunk 006: scanstatus (SQLITE_ENABLE_STMT_SCANSTATUS)
// ---------------------------------------------------------------------------------------------

/// O valor que `sqlite3_stmt_scanstatus_v2` grava em `*pOut`, conforme a métrica pedida.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanStatusValue {
    /// `SQLITE_SCANSTAT_NLOOP`, `NVISIT` e `NCYCLE`: um `sqlite3_int64`.
    Int64(i64),
    /// `SQLITE_SCANSTAT_EST`: um `double`.
    Double(f64),
    /// `SQLITE_SCANSTAT_NAME` e `EXPLAIN`: um `const char *` (`None` é o ponteiro nulo).
    Text(Option<Vec<u8>>),
    /// `SQLITE_SCANSTAT_SELECTID` e `PARENTID`: um `int`.
    Int(i32),
}

/// `sqlite3_stmt_scanstatus_v2`: dados de estado de um laço do comando. `None` é o código de
/// retorno 1 do C (laço ou métrica inexistente); `Some` é o código 0 com o valor gravado.
pub fn stmt_scanstatus_v2(
    v: &Vdbe,
    i_scan: i32,
    i_scan_status_op: i32,
    flags: i32,
) -> Option<ScanStatusValue> {
    // Dentro de um subprograma o `aOp` do C é o do programa mais externo, que é o principal.
    let a_op: &[crate::vdbe_types::Op] = match v.p_frame.first().and_then(|f| f.a_op.as_ref()) {
        Some(sp) => &sp.a_op,
        None => &v.a_op,
    };
    let n_op = a_op.len();

    if i_scan < 0 {
        if i_scan_status_op == SQLITE_SCANSTAT_NCYCLE {
            let res = a_op.iter().fold(0i64, |acc, o| acc.wrapping_add(o.n_cycle as i64));
            return Some(ScanStatusValue::Int64(res));
        }
        return None;
    }
    let idx: usize;
    if flags & SQLITE_SCANSTAT_COMPLEX != 0 {
        idx = i_scan as usize;
    } else {
        // Sem a flag COMPLEX, ignora os `ScanStatus` sem nome.
        let mut remaining = i_scan;
        let mut found = v.a_scan.len();
        for (k, s) in v.a_scan.iter().enumerate() {
            if s.z_name.is_some() {
                remaining -= 1;
                if remaining < 0 {
                    found = k;
                    break;
                }
            }
        }
        idx = found;
    }
    if idx >= v.a_scan.len() {
        return None;
    }
    let p_scan = &v.a_scan[idx];

    match i_scan_status_op {
        SQLITE_SCANSTAT_NLOOP => Some(ScanStatusValue::Int64(if p_scan.addr_loop > 0 {
            a_op.get(p_scan.addr_loop as usize).map_or(-1, |o| o.n_exec as i64)
        } else {
            -1
        })),
        SQLITE_SCANSTAT_NVISIT => Some(ScanStatusValue::Int64(if p_scan.addr_visit > 0 {
            a_op.get(p_scan.addr_visit as usize).map_or(-1, |o| o.n_exec as i64)
        } else {
            -1
        })),
        SQLITE_SCANSTAT_EST => {
            let mut r = 1.0f64;
            let mut x = p_scan.n_est;
            while x < 100 {
                x += 10;
                r *= 0.5;
            }
            Some(ScanStatusValue::Double(r * log_est_to_int(x) as f64))
        }
        SQLITE_SCANSTAT_NAME => Some(ScanStatusValue::Text(p_scan.z_name.clone())),
        SQLITE_SCANSTAT_EXPLAIN => {
            if p_scan.addr_explain != 0 {
                let z = match a_op.get(p_scan.addr_explain as usize).map(|o| &o.p4) {
                    Some(P4::Text(z)) => Some(z.clone()),
                    _ => None,
                };
                Some(ScanStatusValue::Text(z))
            } else {
                Some(ScanStatusValue::Text(None))
            }
        }
        SQLITE_SCANSTAT_SELECTID => Some(ScanStatusValue::Int(if p_scan.addr_explain != 0 {
            a_op.get(p_scan.addr_explain as usize).map_or(-1, |o| o.p1)
        } else {
            -1
        })),
        SQLITE_SCANSTAT_PARENTID => Some(ScanStatusValue::Int(if p_scan.addr_explain != 0 {
            a_op.get(p_scan.addr_explain as usize).map_or(-1, |o| o.p2)
        } else {
            -1
        })),
        SQLITE_SCANSTAT_NCYCLE => {
            let mut res = 0i64;
            if p_scan.a_addr_range[0] == 0 {
                res = -1;
            } else {
                for ii in (0..p_scan.a_addr_range.len()).step_by(2) {
                    let mut i_ins = p_scan.a_addr_range[ii];
                    let i_end = p_scan.a_addr_range[ii + 1];
                    if i_ins == 0 {
                        break;
                    }
                    if i_ins > 0 {
                        while i_ins <= i_end {
                            res = res.wrapping_add(
                                a_op.get(i_ins as usize).map_or(0, |o| o.n_cycle as i64),
                            );
                            i_ins += 1;
                        }
                    } else {
                        for o in a_op.iter().take(n_op) {
                            if o.p1 != i_end {
                                continue;
                            }
                            if (OPCODE_PROPERTY[o.opcode as usize] & OPFLG_NCYCLE) == 0 {
                                continue;
                            }
                            res = res.wrapping_add(o.n_cycle as i64);
                        }
                    }
                }
            }
            Some(ScanStatusValue::Int64(res))
        }
        _ => None,
    }
}

/// `sqlite3_stmt_scanstatus`: como [`stmt_scanstatus_v2`] sem flags.
pub fn stmt_scanstatus(v: &Vdbe, i_scan: i32, i_scan_status_op: i32) -> Option<ScanStatusValue> {
    stmt_scanstatus_v2(v, i_scan, i_scan_status_op, 0)
}

/// `sqlite3_stmt_scanstatus_reset`: zera os contadores do scanstatus.
pub fn stmt_scanstatus_reset(db: &mut Connection, id: StmtId) {
    if let Some(p) = db.stmt_mut(id) {
        for o in p.a_op.iter_mut() {
            o.n_exec = 0;
            o.n_cycle = 0;
        }
    }
}
