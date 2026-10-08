// Mesclado das partes traduzidas de vdbeapi_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Notas do porte de vdbeapi.c (opções do Debian 13 já resolvidas):
// - SQLITE_OMIT_DEPRECATED, SQLITE_OMIT_TRACE, SQLITE_OMIT_UTF16 e SQLITE_OMIT_INCRBLOB não estão
//   definidos, então os ramos correspondentes ficam sem condicional.
// - SQLITE_ENABLE_API_ARMOR, SQLITE_DEBUG, SQLITE_ENABLE_STAT4 e SQLITE_STRICT_SUBTYPE não estão
//   definidos: os ramos deles somem (os assert de mutex_held também, pois só existem em depuração).
// - Instrução preparada é `&VdbeRef` (ou `Option<&VdbeRef>` quando o C aceita NULL); valor é
//   `&Mem`/`&mut Mem`; contexto de função é `&mut Sqlite3Context`. Texto e blob devolvidos são
//   `Option<Vec<u8>>` (cópia dos `n` bytes, sem o terminador), onde `None` é o ponteiro NULL do C.

/// Retorna VERDADEIRO (não zero) se a instrução fornecida como argumento
/// precisa ser recompilada. Uma instrução precisa ser recompilada sempre que
/// o ambiente de execução muda de um modo que alteraria o programa que
/// `sqlite3_prepare()` gera. Por exemplo, se novas funções ou sequências de
/// ordenação são registradas ou se um autorizador é adicionado ou alterado.
pub fn expired(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        None => 1,
        Some(p) => (p.borrow().expired != 0) as i32,
    }
}

/// Registra um erro de mau uso da API no log (sqlite3_log com SQLITE_MISUSE).
fn log_misuse(msg: &[u8]) {
    let mut ap = VaList { args: std::collections::VecDeque::new(), arg_list: None };
    log(SQLITE_MISUSE, msg, &mut ap);
}

/// Verifica um Vdbe para garantir que não foi finalizado. Registra um erro
/// e retorna verdadeiro se foi finalizado (ou é inválido de outra forma).
/// Retorna falso se estiver ok.
fn vdbe_safety(p: &VdbeRef) -> i32 {
    if p.borrow().db.upgrade().is_none() {
        log_misuse(b"API called with finalized prepared statement");
        1
    } else {
        0
    }
}

fn vdbe_safety_not_null(p: Option<&VdbeRef>) -> i32 {
    match p {
        None => {
            log_misuse(b"API called with NULL prepared statement");
            1
        }
        Some(p) => vdbe_safety(p),
    }
}

/// Invoca o callback de perfil. Esta rotina só é chamada se já sabemos que o
/// callback de perfil está definido e precisa ser invocado.
#[inline(never)]
fn invoke_profile_callback(db: &Sqlite3Ref, p: &VdbeRef) {
    let mut i_now: i64 = 0;
    let p_vfs = db.borrow().p_vfs.clone();
    if let Some(vfs) = p_vfs {
        os_current_time_int64(&*vfs, &mut i_now);
    }
    let start_time = p.borrow().start_time;
    let i_elapse: i64 = (i_now - start_time).wrapping_mul(1000000);
    let (x_profile, p_profile_arg, m_trace, trace, p_trace_arg) = {
        let d = db.borrow();
        (
            d.x_profile.clone(),
            d.p_profile_arg.clone(),
            d.m_trace,
            d.trace.clone(),
            d.p_trace_arg.clone(),
        )
    };
    if let Some(x_profile) = x_profile {
        let z_sql = p.borrow().z_sql.clone().unwrap_or_default();
        x_profile(&p_profile_arg, &z_sql, i_elapse as u64);
    }
    if (m_trace & (SQLITE_TRACE_PROFILE as u8)) != 0 {
        if let Sqlite3Trace::V2(x_v2) = trace {
            x_v2(SQLITE_TRACE_PROFILE, &p_trace_arg, p, &i_elapse);
        }
    }
    p.borrow_mut().start_time = 0;
}

/// A macro checkProfileCallback(DB,P) verifica se um callback de perfil é
/// necessário e o invoca se for.
fn check_profile_callback(db: &Sqlite3Ref, p: &VdbeRef) {
    if p.borrow().start_time > 0 {
        invoke_profile_callback(db, p);
    }
}

/// Obtém o mutex da conexão (clone do `Rc`) para entrar e sair dele sem manter
/// o `RefCell` da conexão emprestado.
fn db_mutex(db: &Sqlite3Ref) -> Option<Rc<Sqlite3Mutex>> {
    db.borrow().mutex.clone()
}

/// A rotina a seguir destrói uma máquina virtual criada pela rotina
/// `sqlite3_compile()`. O inteiro retornado é um código de sucesso ou falha
/// SQLITE_ que descreve o resultado da execução da máquina virtual.
///
/// Esta rotina define o código e a string de erro retornados por
/// `sqlite3_errcode()`, `sqlite3_errmsg()` e `sqlite3_errmsg16()`.
pub fn finalize(p_stmt: Option<&VdbeRef>) -> i32 {
    let rc;
    match p_stmt {
        None => {
            // IMPLEMENTATION-OF: R-57228-12904 Invocar sqlite3_finalize() num
            // ponteiro NULL é uma operação inofensiva sem efeito.
            rc = SQLITE_OK;
        }
        Some(v) => {
            if vdbe_safety(v) != 0 {
                return SQLITE_MISUSE_BKPT;
            }
            let db = v.borrow().db.upgrade().expect("Vdbe.db vivo após vdbe_safety");
            mutex_enter(db_mutex(&db).as_deref());
            check_profile_callback(&db, v);
            debug_assert!(v.borrow().e_vdbe_state >= VDBE_READY_STATE);
            let rc0 = vdbe_reset(v);
            vdbe_delete(v);
            rc = api_exit(&mut db.borrow_mut(), rc0);
            leave_mutex_and_close_zombie(&db);
        }
    }
    rc
}

/// Termina a execução atual de uma instrução SQL e a devolve ao estado inicial
/// para que possa ser reutilizada. Um código de sucesso da execução anterior é
/// retornado.
///
/// Esta rotina define o código e a string de erro retornados por
/// `sqlite3_errcode()`, `sqlite3_errmsg()` e `sqlite3_errmsg16()`.
pub fn reset(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        None => SQLITE_OK,
        Some(v) => {
            let db = v.borrow().db.upgrade().expect("Vdbe.db deve apontar para uma conexão viva");
            let mutex = db_mutex(&db);
            mutex_enter(mutex.as_deref());
            check_profile_callback(&db, v);
            let rc0 = vdbe_reset(v);
            vdbe_rewind(v);
            debug_assert!((rc0 & db.borrow().err_mask) == rc0);
            let rc = api_exit(&mut db.borrow_mut(), rc0);
            mutex_leave(mutex.as_deref());
            rc
        }
    }
}

/// Define como NULL todos os parâmetros da instrução SQL compilada.
pub fn clear_bindings(p_stmt: &VdbeRef) -> i32 {
    let rc = SQLITE_OK;
    let db = p_stmt.borrow().db.upgrade().expect("Vdbe.db deve apontar para uma conexão viva");
    let mutex = db_mutex(&db);
    mutex_enter(mutex.as_deref());
    {
        let mut p = p_stmt.borrow_mut();
        for i in 0..(p.n_var as usize) {
            let mut v = p.a_var[i].borrow_mut();
            vdbe_mem_release(&mut v);
            v.flags = MEM_NULL;
        }
        debug_assert!((p.prep_flags & SQLITE_PREPARE_SAVESQL) != 0 || p.expmask == 0);
        if p.expmask != 0 {
            p.expired = 1;
        }
    }
    mutex_leave(mutex.as_deref());
    rc
}

/// As rotinas a seguir extraem informação de uma estrutura Mem ou
/// sqlite3_value. Devolve os `n` bytes de texto de `p_val` na codificação
/// `enc`, ou `None` quando o C devolveria NULL.
fn value_text_bytes(p_val: &mut Mem, enc: i32) -> Option<Vec<u8>> {
    let z = crate::vdbemem_c::value_text(Some(&mut *p_val), enc as u8).map(|s| s.to_vec());
    z.map(|mut v| {
        v.truncate(p_val.n.max(0) as usize);
        v
    })
}

/// Valor como blob.
pub fn value_blob(p_val: &mut Mem) -> Option<Vec<u8>> {
    if (p_val.flags & (MEM_BLOB | MEM_STR)) != 0 {
        if expand_blob(p_val) != SQLITE_OK {
            debug_assert!(p_val.flags == MEM_NULL && p_val.z.is_empty());
            return None;
        }
        p_val.flags |= MEM_BLOB;
        if p_val.n != 0 {
            let n = (p_val.n as usize).min(p_val.z.len());
            Some(p_val.z[..n].to_vec())
        } else {
            None
        }
    } else {
        value_text(p_val)
    }
}

pub fn value_bytes(p_val: &mut Mem) -> i32 {
    crate::vdbemem_c::value_bytes(p_val, SQLITE_UTF8 as u8)
}

pub fn value_bytes16(p_val: &mut Mem) -> i32 {
    crate::vdbemem_c::value_bytes(p_val, SQLITE_UTF16NATIVE as u8)
}

pub fn value_double(p_val: &Mem) -> f64 {
    vdbe_real_value(p_val)
}

pub fn value_int(p_val: &Mem) -> i32 {
    vdbe_int_value(p_val) as i32
}

pub fn value_int64(p_val: &Mem) -> i64 {
    vdbe_int_value(p_val)
}

pub fn value_subtype(p_val: &Mem) -> u32 {
    if (p_val.flags & MEM_SUBTYPE) != 0 {
        p_val.e_subtype as u32
    } else {
        0
    }
}

/// Devolve o conteúdo do valor "pointer" quando o tipo informado confere.
pub fn value_pointer(p_val: &Mem, z_p_type: Option<&[u8]>) -> Option<Vec<u8>> {
    let z_p_type = z_p_type?;
    if (p_val.flags & (MEM_TYPEMASK | MEM_TERM | MEM_SUBTYPE)) == (MEM_NULL | MEM_TERM | MEM_SUBTYPE)
        && p_val.e_subtype == b'p'
        && p_val.u.z_p_type.map_or(false, |t| t == z_p_type)
    {
        Some(p_val.z.clone())
    } else {
        None
    }
}

pub fn value_text(p_val: &mut Mem) -> Option<Vec<u8>> {
    value_text_bytes(p_val, SQLITE_UTF8)
}

pub fn value_text16(p_val: &mut Mem) -> Option<Vec<u8>> {
    value_text_bytes(p_val, SQLITE_UTF16NATIVE)
}

pub fn value_text16be(p_val: &mut Mem) -> Option<Vec<u8>> {
    value_text_bytes(p_val, SQLITE_UTF16BE)
}

pub fn value_text16le(p_val: &mut Mem) -> Option<Vec<u8>> {
    value_text_bytes(p_val, SQLITE_UTF16LE)
}

/// EVIDENCE-OF: R-12793-43283 Todo valor em SQLite tem um de cinco tipos de
/// dados fundamentais: inteiro de 64 bits, número de ponto flutuante IEEE de
/// 64 bits, string, BLOB, NULL.
pub fn value_type(p_val: &Mem) -> i32 {
    const A_TYPE: [u8; 64] = [
        SQLITE_BLOB as u8,    /* 0x00 (impossível) */
        SQLITE_NULL as u8,    /* 0x01 NULL */
        SQLITE_TEXT as u8,    /* 0x02 TEXT */
        SQLITE_NULL as u8,    /* 0x03 (impossível) */
        SQLITE_INTEGER as u8, /* 0x04 INTEGER */
        SQLITE_NULL as u8,    /* 0x05 (impossível) */
        SQLITE_INTEGER as u8, /* 0x06 INTEGER + TEXT */
        SQLITE_NULL as u8,    /* 0x07 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x08 FLOAT */
        SQLITE_NULL as u8,    /* 0x09 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x0a FLOAT + TEXT */
        SQLITE_NULL as u8,    /* 0x0b (impossível) */
        SQLITE_INTEGER as u8, /* 0x0c (impossível) */
        SQLITE_NULL as u8,    /* 0x0d (impossível) */
        SQLITE_INTEGER as u8, /* 0x0e (impossível) */
        SQLITE_NULL as u8,    /* 0x0f (impossível) */
        SQLITE_BLOB as u8,    /* 0x10 BLOB */
        SQLITE_NULL as u8,    /* 0x11 (impossível) */
        SQLITE_TEXT as u8,    /* 0x12 (impossível) */
        SQLITE_NULL as u8,    /* 0x13 (impossível) */
        SQLITE_INTEGER as u8, /* 0x14 INTEGER + BLOB */
        SQLITE_NULL as u8,    /* 0x15 (impossível) */
        SQLITE_INTEGER as u8, /* 0x16 (impossível) */
        SQLITE_NULL as u8,    /* 0x17 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x18 FLOAT + BLOB */
        SQLITE_NULL as u8,    /* 0x19 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x1a (impossível) */
        SQLITE_NULL as u8,    /* 0x1b (impossível) */
        SQLITE_INTEGER as u8, /* 0x1c (impossível) */
        SQLITE_NULL as u8,    /* 0x1d (impossível) */
        SQLITE_INTEGER as u8, /* 0x1e (impossível) */
        SQLITE_NULL as u8,    /* 0x1f (impossível) */
        SQLITE_FLOAT as u8,   /* 0x20 INTREAL */
        SQLITE_NULL as u8,    /* 0x21 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x22 INTREAL + TEXT */
        SQLITE_NULL as u8,    /* 0x23 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x24 (impossível) */
        SQLITE_NULL as u8,    /* 0x25 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x26 (impossível) */
        SQLITE_NULL as u8,    /* 0x27 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x28 (impossível) */
        SQLITE_NULL as u8,    /* 0x29 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x2a (impossível) */
        SQLITE_NULL as u8,    /* 0x2b (impossível) */
        SQLITE_FLOAT as u8,   /* 0x2c (impossível) */
        SQLITE_NULL as u8,    /* 0x2d (impossível) */
        SQLITE_FLOAT as u8,   /* 0x2e (impossível) */
        SQLITE_NULL as u8,    /* 0x2f (impossível) */
        SQLITE_BLOB as u8,    /* 0x30 (impossível) */
        SQLITE_NULL as u8,    /* 0x31 (impossível) */
        SQLITE_TEXT as u8,    /* 0x32 (impossível) */
        SQLITE_NULL as u8,    /* 0x33 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x34 (impossível) */
        SQLITE_NULL as u8,    /* 0x35 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x36 (impossível) */
        SQLITE_NULL as u8,    /* 0x37 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x38 (impossível) */
        SQLITE_NULL as u8,    /* 0x39 (impossível) */
        SQLITE_FLOAT as u8,   /* 0x3a (impossível) */
        SQLITE_NULL as u8,    /* 0x3b (impossível) */
        SQLITE_FLOAT as u8,   /* 0x3c (impossível) */
        SQLITE_NULL as u8,    /* 0x3d (impossível) */
        SQLITE_FLOAT as u8,   /* 0x3e (impossível) */
        SQLITE_NULL as u8,    /* 0x3f (impossível) */
    ];
    A_TYPE[(p_val.flags & MEM_AFFMASK) as usize] as i32
}

pub fn value_encoding(p_val: &Mem) -> i32 {
    p_val.enc as i32
}

/// Retorna verdadeiro se um parâmetro de xUpdate representa uma coluna inalterada.
pub fn value_nochange(p_val: &Mem) -> i32 {
    ((p_val.flags & (MEM_NULL | MEM_ZERO)) == (MEM_NULL | MEM_ZERO)) as i32
}

/// Retorna verdadeiro se um valor de parâmetro se originou de um `sqlite3_bind()`.
pub fn value_frombind(p_val: &Mem) -> i32 {
    ((p_val.flags & MEM_FROMBIND) != 0) as i32
}

/// Faz uma cópia de um objeto sqlite3_value.
pub fn value_dup(p_orig: Option<&Mem>) -> Option<Box<Mem>> {
    let p_orig = p_orig?;
    // memset(pNew, 0, sizeof) seguido de memcpy(pNew, pOrig, MEMCELLSIZE): copia só u, z, n,
    // flags, enc e e_subtype; o resto fica zerado.
    let mut p_new = Box::new(Mem {
        u: p_orig.u.clone(),
        z: p_orig.z.clone(),
        n: p_orig.n,
        flags: p_orig.flags,
        enc: p_orig.enc,
        e_subtype: p_orig.e_subtype,
        db: None,
        sz_malloc: 0,
        u_temp: 0,
        z_malloc: Vec::new(),
        x_del: None,
    });
    p_new.flags &= !MEM_DYN;
    p_new.db = None;
    if (p_new.flags & (MEM_STR | MEM_BLOB)) != 0 {
        p_new.flags &= !(MEM_STATIC | MEM_DYN);
        p_new.flags |= MEM_EPHEM;
        if vdbe_mem_make_writeable(&mut p_new) != SQLITE_OK {
            crate::vdbemem_c::value_free(Some(p_new));
            return None;
        }
    } else if (p_new.flags & MEM_NULL) != 0 {
        // Não duplica valores de ponteiro.
        p_new.flags &= !(MEM_TERM | MEM_SUBTYPE);
    }
    Some(p_new)
}


// ---- part_001.rs ----

/// Destrói um objeto sqlite3_value obtido anteriormente de `sqlite3_value_dup()`.
pub fn value_free(p_old: Option<Box<Mem>>) {
    crate::vdbemem_c::value_free(p_old);
}

/// Conexão dona do registrador de saída do contexto (`pCtx->pOut->db`).
fn ctx_db(p_ctx: &Sqlite3Context) -> Sqlite3Ref {
    p_ctx
        .p_out
        .borrow()
        .db
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("pOut.db deve apontar para uma conexão viva")
}

/// sqlite3_result_: as rotinas a seguir são usadas por funções definidas pelo
/// usuário para especificar o resultado da função.
///
/// `set_result_str_or_error()` chama `vdbe_mem_set_str()` para guardar o
/// resultado como string ou blob. Os erros apropriados são definidos se a
/// string ou blob for grande demais ou se faltar memória.
fn set_result_str_or_error(
    p_ctx: &mut Sqlite3Context,
    z: Option<&[u8]>,
    n: i32,
    enc: u8,
    x_del: Destructor,
) {
    let rc = vdbe_mem_set_str(&mut p_ctx.p_out.borrow_mut(), z, n as i64, enc, x_del);
    if rc != 0 {
        if rc == SQLITE_TOOBIG {
            result_error_toobig(p_ctx);
        } else {
            // Os únicos erros possíveis de vdbe_mem_set_str são SQLITE_TOOBIG e SQLITE_NOMEM.
            debug_assert!(rc == SQLITE_NOMEM);
            result_error_nomem(p_ctx);
        }
        return;
    }
    vdbe_change_encoding(&mut p_ctx.p_out.borrow_mut(), p_ctx.enc as i32);
    let too_big = vdbe_mem_too_big(&p_ctx.p_out.borrow());
    if too_big {
        result_error_toobig(p_ctx);
    }
}

/// `invokeValueDestructor(P,X)` invoca o destrutor X() sobre o valor P se P não
/// for usado e precisar ser destruído. Com `Destructor` (Static ou Transient) não
/// há nada a destruir: o conteúdo pertence ao chamador em Rust.
fn invoke_value_destructor(_x_del: Destructor, p_ctx: Option<&mut Sqlite3Context>) -> i32 {
    debug_assert!(p_ctx.is_some());
    if let Some(p_ctx) = p_ctx {
        result_error_toobig(p_ctx);
    }
    SQLITE_TOOBIG
}

pub fn result_blob(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: i32, x_del: Destructor) {
    debug_assert!(n >= 0);
    set_result_str_or_error(p_ctx, z, n, 0, x_del);
}

pub fn result_blob64(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: u64, x_del: Destructor) {
    if n > 0x7fffffff {
        let _ = invoke_value_destructor(x_del, Some(p_ctx));
    } else {
        set_result_str_or_error(p_ctx, z, n as i32, 0, x_del);
    }
}

pub fn result_double(p_ctx: &mut Sqlite3Context, r_val: f64) {
    vdbe_mem_set_double(&mut p_ctx.p_out.borrow_mut(), r_val);
}

pub fn result_error(p_ctx: &mut Sqlite3Context, z: &[u8], n: i32) {
    p_ctx.is_error = SQLITE_ERROR;
    vdbe_mem_set_str(&mut p_ctx.p_out.borrow_mut(), Some(z), n as i64, SQLITE_UTF8 as u8, SQLITE_TRANSIENT);
}

pub fn result_error16(p_ctx: &mut Sqlite3Context, z: &[u8], n: i32) {
    p_ctx.is_error = SQLITE_ERROR;
    vdbe_mem_set_str(
        &mut p_ctx.p_out.borrow_mut(),
        Some(z),
        n as i64,
        SQLITE_UTF16NATIVE as u8,
        SQLITE_TRANSIENT,
    );
}

pub fn result_int(p_ctx: &mut Sqlite3Context, i_val: i32) {
    vdbe_mem_set_int64(&mut p_ctx.p_out.borrow_mut(), i_val as i64);
}

pub fn result_int64(p_ctx: &mut Sqlite3Context, i_val: i64) {
    vdbe_mem_set_int64(&mut p_ctx.p_out.borrow_mut(), i_val);
}

pub fn result_null(p_ctx: &mut Sqlite3Context) {
    vdbe_mem_set_null(&mut p_ctx.p_out.borrow_mut());
}

pub fn result_pointer(
    p_ctx: &mut Sqlite3Context,
    p_ptr: Vec<u8>,
    z_p_type: Option<&'static [u8]>,
    x_destructor: Option<fn(Vec<u8>)>,
) {
    let mut p_out = p_ctx.p_out.borrow_mut();
    vdbe_mem_release(&mut p_out);
    p_out.flags = MEM_NULL;
    vdbe_mem_set_pointer(&mut p_out, p_ptr, z_p_type, x_destructor);
}

pub fn result_subtype(p_ctx: &mut Sqlite3Context, e_subtype: u32) {
    let mut p_out = p_ctx.p_out.borrow_mut();
    p_out.e_subtype = (e_subtype & 0xff) as u8;
    p_out.flags |= MEM_SUBTYPE;
}

pub fn result_text(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: i32, x_del: Destructor) {
    set_result_str_or_error(p_ctx, z, n, SQLITE_UTF8 as u8, x_del);
}

pub fn result_text64(
    p_ctx: &mut Sqlite3Context,
    z: Option<&[u8]>,
    n: u64,
    x_del: Destructor,
    enc: u8,
) {
    let mut enc = enc;
    let mut n = n;
    if enc != SQLITE_UTF8 as u8 {
        if enc == SQLITE_UTF16 as u8 {
            enc = SQLITE_UTF16NATIVE as u8;
        }
        n &= !1u64;
    }
    if n > 0x7fffffff {
        let _ = invoke_value_destructor(x_del, Some(p_ctx));
    } else {
        set_result_str_or_error(p_ctx, z, n as i32, enc, x_del);
        vdbe_mem_zero_terminate_if_able(&mut p_ctx.p_out.borrow_mut());
    }
}

pub fn result_text16(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: i32, x_del: Destructor) {
    set_result_str_or_error(p_ctx, z, ((n as u64) & !1u64) as i32, SQLITE_UTF16NATIVE as u8, x_del);
}

pub fn result_text16be(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: i32, x_del: Destructor) {
    set_result_str_or_error(p_ctx, z, ((n as u64) & !1u64) as i32, SQLITE_UTF16BE as u8, x_del);
}

pub fn result_text16le(p_ctx: &mut Sqlite3Context, z: Option<&[u8]>, n: i32, x_del: Destructor) {
    set_result_str_or_error(p_ctx, z, ((n as u64) & !1u64) as i32, SQLITE_UTF16LE as u8, x_del);
}

pub fn result_value(p_ctx: &mut Sqlite3Context, p_value: &Mem) {
    vdbe_mem_copy(&mut p_ctx.p_out.borrow_mut(), p_value);
    vdbe_change_encoding(&mut p_ctx.p_out.borrow_mut(), p_ctx.enc as i32);
    let too_big = vdbe_mem_too_big(&p_ctx.p_out.borrow());
    if too_big {
        result_error_toobig(p_ctx);
    }
}

pub fn result_zeroblob(p_ctx: &mut Sqlite3Context, n: i32) {
    let _ = result_zeroblob64(p_ctx, if n > 0 { n as u64 } else { 0 });
}

pub fn result_zeroblob64(p_ctx: &mut Sqlite3Context, n: u64) -> i32 {
    let db = ctx_db(p_ctx);
    let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    if n > (limit as i64 as u64) {
        result_error_toobig(p_ctx);
        return SQLITE_TOOBIG;
    }
    vdbe_mem_set_zero_blob(&mut p_ctx.p_out.borrow_mut(), n as i32);
    SQLITE_OK
}

pub fn result_error_code(p_ctx: &mut Sqlite3Context, err_code: i32) {
    p_ctx.is_error = if err_code != 0 { err_code } else { -1 };
    let is_null = (p_ctx.p_out.borrow().flags & MEM_NULL) != 0;
    if is_null {
        set_result_str_or_error(p_ctx, Some(err_str(err_code)), -1, SQLITE_UTF8 as u8, SQLITE_STATIC);
    }
}

/// Força um erro SQLITE_TOOBIG.
pub fn result_error_toobig(p_ctx: &mut Sqlite3Context) {
    p_ctx.is_error = SQLITE_TOOBIG;
    vdbe_mem_set_str(
        &mut p_ctx.p_out.borrow_mut(),
        Some(b"string or blob too big"),
        -1,
        SQLITE_UTF8 as u8,
        SQLITE_STATIC,
    );
}

/// Um erro SQLITE_NOMEM.
pub fn result_error_nomem(p_ctx: &mut Sqlite3Context) {
    vdbe_mem_set_null(&mut p_ctx.p_out.borrow_mut());
    p_ctx.is_error = SQLITE_NOMEM_BKPT;
    let db = ctx_db(p_ctx);
    oom_fault(&mut db.borrow_mut());
}

/// Força o valor INT64 guardado atualmente como resultado a ser um valor
/// MEM_IntReal. Ver o controle de teste SQLITE_TESTCTRL_RESULT_INTREAL.
pub fn result_int_real(p_ctx: &mut Sqlite3Context) {
    let mut p_out = p_ctx.p_out.borrow_mut();
    if (p_out.flags & MEM_INT) != 0 {
        p_out.flags &= !MEM_INT;
        p_out.flags |= MEM_INTREAL;
    }
}

/// Esta função é chamada depois que uma transação foi confirmada. Ela invoca os
/// callbacks registrados com `sqlite3_wal_hook()` conforme necessário.
fn do_wal_callbacks(db: &Sqlite3Ref) -> i32 {
    let mut rc = SQLITE_OK;
    let n_db = db.borrow().n_db;
    for i in 0..(n_db as usize) {
        let p_bt = db.borrow().a_db[i].p_bt.clone();
        if let Some(p_bt) = p_bt {
            btree_enter(&mut p_bt.borrow_mut());
            let p_pager = btree_pager(&p_bt.borrow());
            let n_entry = pager_wal_callback(&mut p_pager.borrow_mut());
            btree_leave(&mut p_bt.borrow_mut());
            let (x_wal, p_wal_arg, z_db_sname) = {
                let d = db.borrow();
                (d.x_wal_callback.clone(), d.p_wal_arg.clone(), d.a_db[i].z_db_sname.clone())
            };
            if n_entry > 0 && rc == SQLITE_OK {
                if let Some(x_wal) = x_wal {
                    rc = x_wal(&p_wal_arg, db, &z_db_sname.unwrap_or_default(), n_entry);
                }
            }
        }
    }
    rc
}


// ---- part_002.rs ----

/// Executa a instrução p até uma linha de dados ficar pronta, a instrução
/// terminar por completo ou ocorrer um erro.
///
/// Esta rotina implementa a maior parte da lógica por trás da API
/// `sqlite_step()`. A única coisa omitida é a recompilação automática se houve
/// mudança de esquema; esse detalhe é tratado pelo invólucro externo `step()`
/// (o `sqlite3_step()` público). No C esta função se chama `sqlite3Step`; o nome
/// `step` já é do invólucro público, então a interna leva o sufixo `_inner`.
fn step_inner(p: &VdbeRef) -> i32 {
    let db = p.borrow().db.upgrade().expect("Vdbe.db deve apontar para uma conexão viva");
    let mut rc: i32;

    if p.borrow().e_vdbe_state != VDBE_RUN_STATE {
        // restart_step:
        loop {
            let state = p.borrow().e_vdbe_state;
            if state == VDBE_READY_STATE {
                if p.borrow().expired != 0 {
                    p.borrow_mut().rc = SQLITE_SCHEMA;
                    rc = SQLITE_ERROR;
                    if (p.borrow().prep_flags & SQLITE_PREPARE_SAVESQL) != 0 {
                        // Se esta instrução foi preparada com SQL salvo e ocorreu um erro,
                        // devolve ao chamador o código de erro em p->rc. Define o código de
                        // erro do handle do banco com o mesmo valor.
                        rc = vdbe_transfer_error(&mut p.borrow_mut());
                    }
                    // end_of_step:
                    return rc & db.borrow().err_mask;
                }

                // Se não há outras instruções em execução, zera o flag de interrupção.
                // Isso impede que uma chamada a sqlite3_interrupt interrompa uma instrução
                // que ainda não começou.
                if db.borrow().n_vdbe_active == 0 {
                    db.borrow_mut().is_interrupted = 0;
                }

                debug_assert!(
                    db.borrow().n_vdbe_write > 0
                        || db.borrow().auto_commit == 0
                        || (db.borrow().n_deferred_cons == 0
                            && db.borrow().n_deferred_imm_cons == 0)
                );

                let trace_on = {
                    let d = db.borrow();
                    (d.m_trace & ((SQLITE_TRACE_PROFILE as u8) | SQLITE_TRACE_XPROFILE)) != 0
                        && d.init.busy == 0
                };
                if trace_on && p.borrow().z_sql.is_some() {
                    let p_vfs = db.borrow().p_vfs.clone();
                    if let Some(vfs) = p_vfs {
                        let mut t: i64 = 0;
                        os_current_time_int64(&*vfs, &mut t);
                        p.borrow_mut().start_time = t;
                    }
                } else {
                    debug_assert!(p.borrow().start_time == 0);
                }

                {
                    let mut d = db.borrow_mut();
                    d.n_vdbe_active += 1;
                    if p.borrow().read_only == 0 {
                        d.n_vdbe_write += 1;
                    }
                    if p.borrow().b_is_reader != 0 {
                        d.n_vdbe_read += 1;
                    }
                }
                let mut v = p.borrow_mut();
                v.pc = 0;
                v.e_vdbe_state = VDBE_RUN_STATE;
                break;
            } else if state == VDBE_HALT_STATE {
                // Antes da versão 3.7.0 era preciso chamar sqlite3_reset() antes de repetir
                // sqlite3_step() após qualquer erro ou após SQLITE_DONE. A partir da 3.7.0,
                // sqlite3_reset() é chamado automaticamente em vez de gerar SQLITE_MISUSE.
                // SQLITE_OMIT_AUTORESET não está definido no Debian.
                reset(Some(p));
                debug_assert!(p.borrow().e_vdbe_state == VDBE_READY_STATE);
                continue; // goto restart_step
            } else {
                break;
            }
        }
    }

    if p.borrow().explain != 0 {
        rc = vdbe_list(p);
    } else {
        db.borrow_mut().n_vdbe_exec += 1;
        rc = vdbe_exec(p);
        db.borrow_mut().n_vdbe_exec -= 1;
    }

    if rc == SQLITE_ROW {
        debug_assert!(p.borrow().rc == SQLITE_OK);
        debug_assert!(db.borrow().malloc_failed == 0);
        db.borrow_mut().err_code = SQLITE_ROW;
        return SQLITE_ROW;
    } else {
        // Se a instrução terminou com sucesso, invoca o callback de perfil.
        check_profile_callback(&db, p);
        p.borrow_mut().p_result_row = None;
        if rc == SQLITE_DONE && db.borrow().auto_commit != 0 {
            debug_assert!(p.borrow().rc == SQLITE_OK);
            let wal_rc = do_wal_callbacks(&db);
            p.borrow_mut().rc = wal_rc;
            if wal_rc != SQLITE_OK {
                rc = SQLITE_ERROR;
            }
        } else if rc != SQLITE_DONE && (p.borrow().prep_flags & SQLITE_PREPARE_SAVESQL) != 0 {
            // Se esta instrução foi preparada com SQL salvo e ocorreu um erro, devolve ao
            // chamador o código de erro em p->rc. Define o código de erro do handle do banco
            // com o mesmo valor.
            rc = vdbe_transfer_error(&mut p.borrow_mut());
        }
    }

    db.borrow_mut().err_code = rc;
    let p_rc = p.borrow().rc;
    if SQLITE_NOMEM == api_exit(&mut db.borrow_mut(), p_rc) {
        let mut v = p.borrow_mut();
        v.rc = SQLITE_NOMEM_BKPT;
        if (v.prep_flags & SQLITE_PREPARE_SAVESQL) != 0 {
            rc = v.rc;
        }
    }
    // end_of_step: só um número limitado de códigos de resultado é permitido para instruções
    // preparadas pela interface legada sqlite3_prepare().
    debug_assert!(
        (p.borrow().prep_flags & SQLITE_PREPARE_SAVESQL) != 0
            || rc == SQLITE_ROW
            || rc == SQLITE_DONE
            || rc == SQLITE_ERROR
            || (rc & 0xff) == SQLITE_BUSY
            || rc == SQLITE_MISUSE
    );
    rc & db.borrow().err_mask
}

/// Implementação de nível superior de `sqlite3_step()`. Chama `step_inner()` para
/// fazer a maior parte do trabalho. Se ocorrer um erro de esquema, chama
/// `reprepare()` e tenta de novo.
pub fn step(p_stmt: Option<&VdbeRef>) -> i32 {
    let mut rc: i32;
    let mut cnt: i32 = 0; // Contador para evitar laço infinito de reprepares

    if vdbe_safety_not_null(p_stmt) != 0 {
        return SQLITE_MISUSE_BKPT;
    }
    let v = p_stmt.expect("vdbe_safety_not_null garante instrução não nula");
    let db = v.borrow().db.upgrade().expect("Vdbe.db vivo após vdbe_safety");
    let mutex = db_mutex(&db);
    mutex_enter(mutex.as_deref());
    loop {
        rc = step_inner(v);
        if rc != SQLITE_SCHEMA {
            break;
        }
        let old = cnt;
        cnt += 1;
        if old >= SQLITE_MAX_SCHEMA_RETRY {
            break;
        }
        let saved_pc = v.borrow().pc;
        rc = reprepare(v);
        if rc != SQLITE_OK {
            // Este caso ocorre após falhar ao recompilar uma instrução SQL. A mensagem de erro
            // do compilador SQL já foi carregada no handle do banco. Este bloco copia a
            // mensagem do handle para a instrução e zera o contador de programa para que, ao
            // finalizar ou reiniciar, a mensagem do parser fique disponível por
            // sqlite3_errmsg() e sqlite3_errcode().
            let p_err = db.borrow().p_err.clone();
            let z_err = p_err.and_then(|e| value_text(&mut e.borrow_mut()));
            let malloc_failed = db.borrow().malloc_failed;
            let mut vm = v.borrow_mut();
            vm.z_err_msg = None;
            if malloc_failed == 0 {
                vm.z_err_msg = z_err;
                rc = api_exit(&mut db.borrow_mut(), rc);
                vm.rc = rc;
            } else {
                vm.z_err_msg = None;
                rc = SQLITE_NOMEM_BKPT;
                vm.rc = rc;
            }
            break;
        }
        reset(Some(v));
        if saved_pc >= 0 {
            // Definir minWriteFileFormat como 254 sinaliza aos opcodes OP_Init e OP_Trace
            // para NÃO executar SQLITE_TRACE_STMT, porque já foi feito uma vez numa execução
            // anterior que falhou com SQLITE_SCHEMA. tag-20220401a
            v.borrow_mut().min_write_file_format = 254;
        }
        debug_assert!(v.borrow().expired == 0);
    }
    mutex_leave(mutex.as_deref());
    rc
}

/// Extrai os dados do usuário de uma estrutura sqlite3_context e os devolve.
pub fn user_data(p: &Sqlite3Context) -> CallbackArg {
    p.p_func.p_user_data.clone()
}

/// Devolve a conexão de banco de dados do contexto.
///
/// IMPLEMENTATION-OF: R-46798-50301 A interface sqlite3_context_db_handle()
/// retorna uma cópia do ponteiro para a conexão de banco de dados (o 1º
/// parâmetro) de `sqlite3_create_function()` e `sqlite3_create_function16()` que
/// registrou originalmente a função definida pelo aplicativo.
pub fn context_db_handle(p: &Sqlite3Context) -> Sqlite3Ref {
    ctx_db(p)
}

/// Se esta rotina for invocada dentro de um método xColumn de uma tabela
/// virtual, retorna verdadeiro se e somente se a chamada ocorre durante um UPDATE
/// e o valor da coluna não será modificado por ele.
///
/// Fora do xColumn de uma tabela virtual o valor retornado não tem significado.
pub fn vtab_nochange(p: &Sqlite3Context) -> i32 {
    value_nochange(&p.p_out.borrow())
}

/// A função destrutora de um objeto ValueList. Precisa ser uma função separada,
/// desconhecida do aplicativo, para garantir que chamadas a `vtab_in_first()` e
/// `vtab_in_next()` não precedidas da ativação do processamento IN via
/// `sqlite3_vtab_int()` não tentem acessar um ValueList falso inserido por uma
/// extensão hostil. O conteúdo é devolvido ao dono Rust (drop).
pub fn vdbe_value_list_free(_p_to_delete: Vec<u8>) {}

/// Implementação de `vtab_in_first()` (se b_next==0) e `vtab_in_next()` (se
/// b_next!=0). Como `Mem.z` é `Vec<u8>` e não guarda um `ValueList`, o ValueList
/// que o C recupera de `pVal->z` chega aqui no parâmetro `p_rhs`, entregue por
/// quem montou o valor "pointer" (o integrador liga os dois).
fn value_from_value_list(
    p_val: Option<&Mem>,
    p_rhs: Option<&mut ValueList>,
    pp_out: &mut Option<MemRef>,
    b_next: i32,
) -> i32 {
    *pp_out = None;
    let p_val = match p_val {
        None => return SQLITE_MISUSE_BKPT,
        Some(v) => v,
    };
    let x_free: fn(Vec<u8>) = vdbe_value_list_free;
    if (p_val.flags & MEM_DYN) == 0 || p_val.x_del != Some(x_free) {
        return SQLITE_ERROR;
    }
    debug_assert!(
        (p_val.flags & (MEM_TYPEMASK | MEM_TERM | MEM_SUBTYPE)) == (MEM_NULL | MEM_TERM | MEM_SUBTYPE)
    );
    debug_assert!(p_val.e_subtype == b'p');
    debug_assert!(p_val.u.z_p_type == Some(&b"ValueList"[..]));
    let p_rhs = match p_rhs {
        None => return SQLITE_MISUSE_BKPT,
        Some(r) => r,
    };
    let p_csr = p_rhs.p_csr.clone().expect("ValueList.p_csr");
    let mut rc: i32;
    if b_next != 0 {
        rc = btree_next(&mut p_csr.borrow_mut(), 0);
    } else {
        let mut dummy: i32 = 0;
        rc = btree_first(&mut p_csr.borrow_mut(), &mut dummy);
        debug_assert!(rc == SQLITE_OK || btree_eof(&p_csr.borrow()) != 0);
        if btree_eof(&p_csr.borrow()) != 0 {
            rc = SQLITE_DONE;
        }
    }
    if rc == SQLITE_OK {
        // Conteúdo bruto da linha atual e seu tamanho em bytes.
        let mut s_mem = Mem {
            u: MemValue::default(),
            z: Vec::new(),
            n: 0,
            flags: 0,
            enc: 0,
            e_subtype: 0,
            db: None,
            sz_malloc: 0,
            u_temp: 0,
            z_malloc: Vec::new(),
            x_del: None,
        };
        let sz: u32 = btree_payload_size(&mut p_csr.borrow_mut());
        rc = vdbe_mem_from_btree_zero_offset(&mut p_csr.borrow_mut(), sz, &mut s_mem);
        if rc == SQLITE_OK {
            let p_out_ref = p_rhs.p_out.clone().expect("ValueList.p_out");
            let mut i_serial: u32 = 0;
            let i_off: usize = 1 + get_varint32(&s_mem.z[1..], &mut i_serial) as usize;
            {
                let mut p_out = p_out_ref.borrow_mut();
                vdbe_serial_get(&s_mem.z[i_off..], i_serial, &mut p_out);
                let db = p_out.db.as_ref().and_then(|w| w.upgrade()).expect("pOut.db");
                p_out.enc = db.borrow().enc;
                if (p_out.flags & MEM_EPHEM) != 0 && vdbe_mem_make_writeable(&mut p_out) != 0 {
                    rc = SQLITE_NOMEM;
                }
            }
            if rc != SQLITE_NOMEM {
                *pp_out = Some(p_out_ref);
            }
        }
        vdbe_mem_release(&mut s_mem);
    }
    rc
}

/// Põe o iterador p_val apontando para o primeiro valor do conjunto. Define
/// `*pp_out` para apontar para esse valor antes de retornar.
pub fn vtab_in_first(
    p_val: Option<&Mem>,
    p_rhs: Option<&mut ValueList>,
    pp_out: &mut Option<MemRef>,
) -> i32 {
    value_from_value_list(p_val, p_rhs, pp_out, 0)
}

/// Põe o iterador p_val apontando para o próximo valor do conjunto. Define
/// `*pp_out` para apontar para esse valor antes de retornar.
pub fn vtab_in_next(
    p_val: Option<&Mem>,
    p_rhs: Option<&mut ValueList>,
    pp_out: &mut Option<MemRef>,
) -> i32 {
    value_from_value_list(p_val, p_rhs, pp_out, 1)
}

/// Retorna a hora atual de uma instrução. Se a hora atual for pedida mais de uma
/// vez numa mesma execução de uma instrução preparada, devolve exatamente a mesma
/// hora em cada chamada, independente do tempo decorrido entre elas: a hora
/// devolvida é sempre a da primeira chamada.
pub fn stmt_current_time(p: &Sqlite3Context) -> i64 {
    let p_vdbe = p.p_vdbe.upgrade().expect("pCtx.pVdbe não nulo sem STAT4");
    if p_vdbe.borrow().i_current_time == 0 {
        let db = ctx_db(p);
        let p_vfs = db.borrow().p_vfs.clone();
        let mut t: i64 = 0;
        let rc = match p_vfs {
            Some(vfs) => os_current_time_int64(&*vfs, &mut t),
            None => SQLITE_ERROR,
        };
        p_vdbe.borrow_mut().i_current_time = if rc != 0 { 0 } else { t };
    }
    let t = p_vdbe.borrow().i_current_time;
    t
}


// ---- part_003.rs ----

/// Cria um novo contexto de agregação para p e devolve o registrador cujo
/// `z` guarda o contexto (o `pMem->z` do C).
fn create_agg_context(p: &mut Sqlite3Context, n_byte: i32) -> Option<MemRef> {
    let p_mem = p.p_mem.clone().expect("pCtx.pMem");
    {
        let mut m = p_mem.borrow_mut();
        debug_assert!((m.flags & MEM_AGG) == 0);
        if n_byte <= 0 {
            vdbe_mem_set_null(&mut m);
            m.z = Vec::new();
        } else {
            vdbe_mem_clear_and_resize(&mut m, n_byte);
            m.flags = MEM_AGG;
            m.u.p_def = Some(p.p_func.clone());
            if !m.z.is_empty() {
                let n = (n_byte as usize).min(m.z.len());
                m.z[..n].fill(0);
            }
        }
    }
    let has_z = !p_mem.borrow().z.is_empty();
    if has_z {
        Some(p_mem)
    } else {
        None
    }
}

/// Aloca ou devolve o contexto de agregação de uma função de usuário. Um novo
/// contexto é alocado na primeira chamada. As chamadas seguintes devolvem o mesmo
/// contexto retornado nas anteriores. O contexto são os bytes de `z` do registrador
/// devolvido (`None` é o ponteiro NULL do C).
pub fn aggregate_context(p: &mut Sqlite3Context, n_byte: i32) -> Option<MemRef> {
    debug_assert!(p.p_func.x_finalize.is_some());
    let p_mem = p.p_mem.clone().expect("pCtx.pMem");
    let is_agg = (p_mem.borrow().flags & MEM_AGG) != 0;
    if !is_agg {
        create_agg_context(p, n_byte)
    } else {
        Some(p_mem)
    }
}

/// Devolve o ponteiro de dados auxiliares, se houver, do iArg-ésimo argumento da
/// função de usuário definida por p_ctx. O argumento mais à esquerda é 0.
///
/// Comportamento não documentado: se i_arg for negativo, acessa um cache de
/// ponteiros auxiliares disponível para todas as funções dentro de uma única
/// instrução preparada. Os valores de i_arg precisam casar.
pub fn get_auxdata(p_ctx: &Sqlite3Context, i_arg: i32) -> CallbackArg {
    let p_vdbe = p_ctx.p_vdbe.upgrade().expect("pCtx.pVdbe não nulo sem STAT4");
    let v = p_vdbe.borrow();
    let mut cur = v.p_aux_data.as_deref();
    while let Some(a) = cur {
        if a.i_aux_arg == i_arg && (a.i_aux_op == p_ctx.i_op || i_arg < 0) {
            return a.p_aux.clone();
        }
        cur = a.p_next_aux.as_deref();
    }
    None
}

/// Define o ponteiro de dados auxiliares e a função de remoção do iArg-ésimo
/// argumento da função de usuário definida por p_ctx. Qualquer valor anterior é
/// removido chamando a função de remoção dada quando ele foi definido. O argumento
/// mais à esquerda é 0.
///
/// Comportamento não documentado: se i_arg for negativo, torna os dados
/// disponíveis para todas as funções da instrução preparada atual, usando i_arg
/// como código de acesso.
pub fn set_auxdata(
    p_ctx: &mut Sqlite3Context,
    i_arg: i32,
    p_aux: CallbackArg,
    x_delete: Option<fn(Rc<dyn Any>)>,
) {
    let p_vdbe = p_ctx.p_vdbe.upgrade().expect("pCtx.pVdbe não nulo sem STAT4");
    // A alocação de AuxData (sqlite3DbMallocZero) não falha em Rust, então o rótulo `failed`
    // do C (que só chamava xDelete(pAux)) é inalcançável aqui.
    let mut velho: Option<(fn(Rc<dyn Any>), Rc<dyn Any>)> = None;
    {
        let mut v = p_vdbe.borrow_mut();
        // Procura o nó (posição na lista encadeada).
        let mut pos: Option<usize> = None;
        {
            let mut idx = 0usize;
            let mut cur = v.p_aux_data.as_deref();
            while let Some(a) = cur {
                if a.i_aux_arg == i_arg && (a.i_aux_op == p_ctx.i_op || i_arg < 0) {
                    pos = Some(idx);
                    break;
                }
                idx += 1;
                cur = a.p_next_aux.as_deref();
            }
        }
        let pos = match pos {
            Some(i) => i,
            None => {
                let novo = Box::new(AuxData {
                    i_aux_op: p_ctx.i_op,
                    i_aux_arg: i_arg,
                    p_aux: None,
                    x_delete_aux: None,
                    p_next_aux: v.p_aux_data.take(),
                });
                v.p_aux_data = Some(novo);
                if p_ctx.is_error == 0 {
                    p_ctx.is_error = -1;
                }
                0
            }
        };
        let mut cur = v.p_aux_data.as_deref_mut().expect("lista não vazia");
        for _ in 0..pos {
            cur = cur.p_next_aux.as_deref_mut().expect("nó existente");
        }
        if let Some(x_del_aux) = cur.x_delete_aux {
            if let Some(antigo) = cur.p_aux.take() {
                velho = Some((x_del_aux, antigo));
            }
        }
        cur.p_aux = p_aux;
        cur.x_delete_aux = x_delete;
    }
    if let Some((f, arg)) = velho {
        f(arg);
    }
}

/// Devolve o número de vezes que a função Step de uma agregação foi chamada.
///
/// Esta função é obsoleta. Não a use em código novo. Existe só para não quebrar
/// código legado. Novas implementações de agregação devem manter suas próprias
/// contagens dentro do contexto de agregação.
pub fn aggregate_count(p: &Sqlite3Context) -> i32 {
    debug_assert!(p.p_func.x_finalize.is_some());
    p.p_mem.as_ref().expect("pCtx.pMem").borrow().n
}

/// Devolve o número de colunas do conjunto de resultados da instrução p_stmt.
pub fn column_count(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        None => 0,
        Some(p) => p.borrow().n_res_column as i32,
    }
}

/// Devolve o número de valores disponíveis na linha atual da instrução em
/// execução p_stmt.
pub fn data_count(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        Some(p) if p.borrow().p_result_row.is_some() => p.borrow().n_res_column as i32,
        _ => 0,
    }
}

/// Devolve um registrador com um valor SQL NULL (a `nullMem` estática do C; aqui é
/// um registrador novo a cada chamada, com todos os campos zerados e MEM_NULL).
fn column_null_value() -> MemRef {
    Rc::new(RefCell::new(Mem {
        u: MemValue::default(),
        z: Vec::new(),
        n: 0,
        flags: MEM_NULL,
        enc: 0,
        e_subtype: 0,
        db: None,
        sz_malloc: 0,
        u_temp: 0,
        z_malloc: Vec::new(),
        x_del: None,
    }))
}

/// Verifica se a coluna i da instrução dada é válida. Se for, devolve o registrador
/// com o valor dessa coluna. Se i não for válido, devolve um registrador com valor
/// NULL. Entra no mutex da conexão (quem sai é `column_malloc_failure()`).
///
/// No C `pResultRow` aponta para um vetor de registradores; aqui `p_result_row` é o
/// primeiro registrador da linha dentro de `a_mem`, então a coluna i é `a_mem[base+i]`.
fn column_mem(p_stmt: Option<&VdbeRef>, i: i32) -> MemRef {
    let p_vm = match p_stmt {
        None => return column_null_value(),
        Some(p) => p,
    };
    let db = p_vm.borrow().db.upgrade().expect("Vdbe.db");
    mutex_enter(db_mutex(&db).as_deref());
    let p_out = {
        let vm = p_vm.borrow();
        match &vm.p_result_row {
            Some(first) if i < vm.n_res_column as i32 && i >= 0 => {
                let base = vm
                    .a_mem
                    .iter()
                    .position(|m| Rc::ptr_eq(m, first))
                    .expect("p_result_row deve ser um registrador de a_mem");
                Some(vm.a_mem[base + i as usize].clone())
            }
            _ => None,
        }
    };
    match p_out {
        Some(m) => m,
        None => {
            error(&mut db.borrow_mut(), SQLITE_RANGE);
            column_null_value()
        }
    }
}

/// Esta função é chamada depois de invocar uma função sqlite3_value_XXX num valor
/// de coluna (um valor devolvido pela avaliação de uma expressão na lista de seleção
/// de um SELECT) que pode causar falha de malloc(). Se o malloc() falhou, o flag
/// mallocFailed da thread é limpo e o código de resultado da instrução p_stmt vira
/// SQLITE_NOMEM. É chamada de dentro de column_int(), column_int64(), column_text(),
/// column_text16(), column_real(), column_bytes(), column_bytes16() e column_blob().
fn column_malloc_failure(p_stmt: Option<&VdbeRef>) {
    // Se o malloc() falhou durante uma conversão de codificação dentro de uma API
    // sqlite3_column_XXX, define o código de retorno da instrução como SQLITE_NOMEM. A
    // próxima chamada a _step() (se houver) devolve SQLITE_ERROR e _finalize() devolve NOMEM.
    if let Some(p) = p_stmt {
        let db = p.borrow().db.upgrade().expect("Vdbe.db");
        let rc = p.borrow().rc;
        let novo = api_exit(&mut db.borrow_mut(), rc);
        p.borrow_mut().rc = novo;
        mutex_leave(db_mutex(&db).as_deref());
    }
}

/// sqlite3_column_: as rotinas a seguir acessam elementos da linha atual do conjunto
/// de resultados.
pub fn column_blob(p_stmt: Option<&VdbeRef>, i: i32) -> Option<Vec<u8>> {
    let m = column_mem(p_stmt, i);
    let val = value_blob(&mut m.borrow_mut());
    // Embora não haja conversão de codificação, value_blob() pode precisar chamar malloc()
    // para expandir o resultado de uma expressão zeroblob().
    column_malloc_failure(p_stmt);
    val
}

pub fn column_bytes(p_stmt: Option<&VdbeRef>, i: i32) -> i32 {
    let m = column_mem(p_stmt, i);
    let val = value_bytes(&mut m.borrow_mut());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_bytes16(p_stmt: Option<&VdbeRef>, i: i32) -> i32 {
    let m = column_mem(p_stmt, i);
    let val = value_bytes16(&mut m.borrow_mut());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_double(p_stmt: Option<&VdbeRef>, i: i32) -> f64 {
    let m = column_mem(p_stmt, i);
    let val = value_double(&m.borrow());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_int(p_stmt: Option<&VdbeRef>, i: i32) -> i32 {
    let m = column_mem(p_stmt, i);
    let val = value_int(&m.borrow());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_int64(p_stmt: Option<&VdbeRef>, i: i32) -> i64 {
    let m = column_mem(p_stmt, i);
    let val = value_int64(&m.borrow());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_text(p_stmt: Option<&VdbeRef>, i: i32) -> Option<Vec<u8>> {
    let m = column_mem(p_stmt, i);
    let val = value_text(&mut m.borrow_mut());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_value(p_stmt: Option<&VdbeRef>, i: i32) -> MemRef {
    let p_out = column_mem(p_stmt, i);
    {
        let mut o = p_out.borrow_mut();
        if (o.flags & MEM_STATIC) != 0 {
            o.flags &= !MEM_STATIC;
            o.flags |= MEM_EPHEM;
        }
    }
    column_malloc_failure(p_stmt);
    p_out
}

pub fn column_text16(p_stmt: Option<&VdbeRef>, i: i32) -> Option<Vec<u8>> {
    let m = column_mem(p_stmt, i);
    let val = value_text16(&mut m.borrow_mut());
    column_malloc_failure(p_stmt);
    val
}

pub fn column_type(p_stmt: Option<&VdbeRef>, i: i32) -> i32 {
    let m = column_mem(p_stmt, i);
    let i_type = value_type(&m.borrow());
    column_malloc_failure(p_stmt);
    i_type
}

/// Nomes de coluna apropriados para EXPLAIN ou EXPLAIN QUERY PLAN.
const AZ_EXPLAIN_COL_NAMES8: [&[u8]; 12] = [
    b"addr", b"opcode", b"p1", b"p2", b"p3", b"p4", b"p5", b"comment", /* EXPLAIN */
    b"id", b"parent", b"notused", b"detail", /* EQP */
];

/// Os mesmos nomes em UTF-16 (unidades u16 terminadas em zero).
const AZ_EXPLAIN_COL_NAMES16_DATA: [u16; 60] = [
    /*   0 */ b'a' as u16, b'd' as u16, b'd' as u16, b'r' as u16, 0,
    /*   5 */ b'o' as u16, b'p' as u16, b'c' as u16, b'o' as u16, b'd' as u16, b'e' as u16, 0,
    /*  12 */ b'p' as u16, b'1' as u16, 0,
    /*  15 */ b'p' as u16, b'2' as u16, 0,
    /*  18 */ b'p' as u16, b'3' as u16, 0,
    /*  21 */ b'p' as u16, b'4' as u16, 0,
    /*  24 */ b'p' as u16, b'5' as u16, 0,
    /*  27 */ b'c' as u16, b'o' as u16, b'm' as u16, b'm' as u16, b'e' as u16, b'n' as u16,
    b't' as u16, 0,
    /*  35 */ b'i' as u16, b'd' as u16, 0,
    /*  38 */ b'p' as u16, b'a' as u16, b'r' as u16, b'e' as u16, b'n' as u16, b't' as u16, 0,
    /*  45 */ b'n' as u16, b'o' as u16, b't' as u16, b'u' as u16, b's' as u16, b'e' as u16,
    b'd' as u16, 0,
    /*  53 */ b'd' as u16, b'e' as u16, b't' as u16, b'a' as u16, b'i' as u16, b'l' as u16, 0,
];

const I_EXPLAIN_COL_NAMES16: [u8; 12] = [0, 5, 12, 15, 18, 21, 24, 27, 35, 38, 45, 53];

/// Converte o N-ésimo elemento de `pStmt->pColName[]` numa string usando
/// value_text() ou value_text16() e a devolve. Se N estiver fora do intervalo,
/// devolve `None`.
///
/// Há até 5 nomes para cada coluna. `use_type` determina qual nome é devolvido:
///
///    0      O nome da coluna como deve ser exibido na saída
///    1      O nome do tipo de dado da coluna
///    2      O nome do banco de dados de onde a coluna deriva
///    3      O nome da tabela de onde a coluna deriva
///    4      O nome da coluna da tabela de onde a coluna de resultado deriva
///
/// Se o resultado não é uma referência simples a coluna (é uma expressão ou
/// constante), os use_type 2, 3 e 4 devolvem NULL.
fn column_name_internal(
    p_stmt: Option<&VdbeRef>,
    n: i32,
    use_utf16: i32,
    use_type: i32,
) -> Option<Vec<u8>> {
    if n < 0 {
        return None;
    }
    let mut n = n;
    let mut ret: Option<Vec<u8>> = None;
    let p = p_stmt.expect("pStmt não nulo");
    let db = p.borrow().db.upgrade().expect("Vdbe.db");
    let mutex = db_mutex(&db);
    mutex_enter(mutex.as_deref());

    let explain = p.borrow().explain as i32;
    'column_name_end: {
        if explain != 0 {
            if use_type > 0 {
                break 'column_name_end;
            }
            let n_cols = if explain == 1 { 8 } else { 4 };
            if n >= n_cols {
                break 'column_name_end;
            }
            let idx = (n + 8 * explain - 8) as usize;
            if use_utf16 != 0 {
                let i = I_EXPLAIN_COL_NAMES16[idx] as usize;
                let mut bytes = Vec::new();
                for &u in AZ_EXPLAIN_COL_NAMES16_DATA[i..].iter().take_while(|&&u| u != 0) {
                    bytes.extend_from_slice(&u.to_le_bytes());
                }
                ret = Some(bytes);
            } else {
                ret = Some(AZ_EXPLAIN_COL_NAMES8[idx].to_vec());
            }
            break 'column_name_end;
        }
        let n_res = p.borrow().n_res_column as i32;
        if n < n_res {
            let prior_malloc_failed = db.borrow().malloc_failed;
            n += use_type * n_res;
            let p_col = p.borrow().a_col_name[n as usize].clone();
            if use_utf16 != 0 {
                ret = value_text16(&mut p_col.borrow_mut());
            } else {
                ret = value_text(&mut p_col.borrow_mut());
            }
            // Um malloc pode ter falhado dentro da chamada _text(). Se for o caso, limpa o flag
            // mallocFailed e devolve NULL.
            debug_assert!(db.borrow().malloc_failed == 0 || db.borrow().malloc_failed == 1);
            if db.borrow().malloc_failed > prior_malloc_failed {
                oom_clear(&mut db.borrow_mut());
                ret = None;
            }
        }
    }
    mutex_leave(mutex.as_deref());
    ret
}


// ---- part_004.rs ----

// Convenções deste trecho (o integrador precisa ligar estes nomes no mod.rs):
// - API pública `sqlite3_xxx` aparece aqui como `api_xxx` (o mod.rs reexporta em `api::xxx`),
//   porque a função estática de mesmo nome (`bindText` -> `bind_text`) vive no mesmo módulo.
// - Instrução preparada é `&VdbeRef` (`Rc<RefCell<Vdbe>>`); NULL do C é `Option<&VdbeRef>`.
// - `Vdbe.db` é `Weak<RefCell<Sqlite3>>`; `stmt_db` abre o ponteiro de volta.
// - `xDel` é o enum `Destructor { Static, Transient, Dynamic, Fn(Rc<dyn Fn(&[u8])>) }`.
// - `sqlite3_mutex_enter/leave` viram `mutex_enter/mutex_leave(&Option<MutexRef>)`.
// - Opções de compilação do Debian resolvidas: UTF16, DECLTYPE, COLUMN_METADATA ligados;
//   API_ARMOR desligado.

/// Abre o ponteiro de volta `p->db` de uma instrução preparada.
pub fn stmt_db(p: &VdbeRef) -> Sqlite3Ref {
    p.borrow().db.upgrade().expect("a conexão sobrevive às suas instruções")
}

/// Devolve o nome da coluna N do conjunto de resultados da instrução p_stmt.
pub fn api_column_name(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 0, COLNAME_NAME)
}

/// Variante UTF-16 de `api_column_name`.
pub fn api_column_name16(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 1, COLNAME_NAME)
}

/// Devolve o tipo declarado (se aplicável) da coluna N do conjunto de resultados.
pub fn api_column_decltype(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 0, COLNAME_DECLTYPE)
}

/// Variante UTF-16 de `api_column_decltype`.
pub fn api_column_decltype16(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 1, COLNAME_DECLTYPE)
}

/// Devolve o nome do banco de dados de onde uma coluna de resultado deriva.
/// Devolve None se a coluna é uma expressão, constante ou qualquer coisa que
/// não seja uma referência inequívoca a uma coluna de banco de dados.
pub fn api_column_database_name(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 0, COLNAME_DATABASE)
}

/// Variante UTF-16 de `api_column_database_name`.
pub fn api_column_database_name16(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 1, COLNAME_DATABASE)
}

/// Devolve o nome da tabela de onde uma coluna de resultado deriva.
/// Devolve None se a coluna é uma expressão, constante ou qualquer coisa que
/// não seja uma referência inequívoca a uma coluna de banco de dados.
pub fn api_column_table_name(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 0, COLNAME_TABLE)
}

/// Variante UTF-16 de `api_column_table_name`.
pub fn api_column_table_name16(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 1, COLNAME_TABLE)
}

/// Devolve o nome da coluna da tabela de onde uma coluna de resultado deriva.
/// Devolve None se a coluna é uma expressão, constante ou qualquer coisa que
/// não seja uma referência inequívoca a uma coluna de banco de dados.
pub fn api_column_origin_name(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 0, COLNAME_COLUMN)
}

/// Variante UTF-16 de `api_column_origin_name`.
pub fn api_column_origin_name16(p_stmt: &VdbeRef, n: i32) -> Option<Vec<u8>> {
    column_name(p_stmt, n, 1, COLNAME_COLUMN)
}

/******************************* sqlite3_bind_  ***************************
** Rotinas usadas para ligar valores aos curingas de uma instrução SQL compilada.
*/

/// Desliga o valor ligado à variável i na máquina virtual p. É o mesmo que ligar
/// NULL à coluna. Se "i" está fora do intervalo, devolve SQLITE_RANGE; senão SQLITE_OK.
///
/// Uma avaliação bem sucedida adquire o mutex de p. O mutex é liberado se qualquer
/// tipo de erro ocorrer. O código de erro guardado em p->db é sobrescrito com o
/// valor devolvido em qualquer caso.
fn vdbe_unbind(p: Option<&VdbeRef>, i: u32) -> i32 {
    if vdbe_safety_not_null(p) != 0 {
        return misuse_error(line!() as i32);
    }
    let p = p.unwrap();
    let db = stmt_db(p);
    mutex_enter(&db.borrow().mutex);
    if p.borrow().e_vdbe_state != VDBE_READY_STATE {
        let rc = misuse_error(line!() as i32);
        error(&mut db.borrow_mut(), rc);
        mutex_leave(&db.borrow().mutex);
        let mut msg = b"bind on a busy prepared statement: [".to_vec();
        match &p.borrow().z_sql {
            Some(z) => msg.extend_from_slice(z),
            None => msg.extend_from_slice(b"(null)"),
        }
        msg.push(b']');
        api_log(SQLITE_MISUSE, &msg);
        return misuse_error(line!() as i32);
    }
    if i >= p.borrow().n_var as u32 {
        error(&mut db.borrow_mut(), SQLITE_RANGE);
        mutex_leave(&db.borrow().mutex);
        return SQLITE_RANGE;
    }
    {
        let mut pb = p.borrow_mut();
        let p_var = &mut pb.a_var[i as usize];
        vdbe_mem_release(p_var);
        p_var.flags = MEM_NULL;
    }
    db.borrow_mut().err_code = SQLITE_OK;

    // Se o bit correspondente a esta variável em Vdbe.expmask está ligado, ligar um
    // novo valor a ela invalida o plano de consulta atual.
    //
    // IMPLEMENTATION-OF: R-57496-20354 Se o valor específico ligado a um parâmetro
    // hospedeiro na cláusula WHERE puder influenciar a escolha do plano de consulta,
    // a instrução é recompilada automaticamente, como se houvesse mudança de esquema,
    // na primeira chamada a sqlite3_step() após qualquer mudança nas ligações.
    let mut pb = p.borrow_mut();
    assert!((pb.prep_flags & SQLITE_PREPARE_SAVESQL) != 0 || pb.expmask == 0);
    if pb.expmask != 0 && (pb.expmask & (if i >= 31 { 0x80000000u32 } else { 1u32 << i })) != 0 {
        pb.expired = 1;
    }
    SQLITE_OK
}

/// Liga um valor de texto ou BLOB.
fn bind_text(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    n_data: i64,
    x_del: Destructor,
    encoding: u8,
) -> i32 {
    let mut rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        let p = p_stmt.unwrap();
        let db = stmt_db(p);
        if z_data.is_some() {
            {
                let mut pb = p.borrow_mut();
                let p_var = &mut pb.a_var[(i - 1) as usize];
                rc = vdbe_mem_set_str(p_var, z_data, n_data, encoding, x_del);
                if rc == SQLITE_OK && encoding != 0 {
                    rc = vdbe_change_encoding(p_var, enc(&db.borrow()) as i32);
                }
            }
            if rc != 0 {
                error(&mut db.borrow_mut(), rc);
                rc = api_exit(&mut db.borrow_mut(), rc);
            }
        }
        mutex_leave(&db.borrow().mutex);
    } else if let Destructor::Fn(x_fn) = &x_del {
        x_fn(z_data.unwrap_or(&[]));
    }
    rc
}

/// Liga um valor blob a uma variável de instrução SQL.
pub fn api_bind_blob(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    n_data: i32,
    x_del: Destructor,
) -> i32 {
    bind_text(p_stmt, i, z_data, n_data as i64, x_del, 0)
}

/// Liga um blob com tamanho de 64 bits.
pub fn api_bind_blob64(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    n_data: u64,
    x_del: Destructor,
) -> i32 {
    assert!(!matches!(x_del, Destructor::Dynamic));
    bind_text(p_stmt, i, z_data, n_data as i64, x_del, 0)
}

/// Liga um valor de ponto flutuante.
pub fn api_bind_double(p_stmt: Option<&VdbeRef>, i: i32, r_value: f64) -> i32 {
    let rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        let p = p_stmt.unwrap();
        vdbe_mem_set_double(&mut p.borrow_mut().a_var[(i - 1) as usize], r_value);
        mutex_leave(&stmt_db(p).borrow().mutex);
    }
    rc
}

/// Liga um inteiro de 32 bits.
pub fn api_bind_int(p: Option<&VdbeRef>, i: i32, i_value: i32) -> i32 {
    api_bind_int64(p, i, i_value as i64)
}

/// Liga um inteiro de 64 bits.
pub fn api_bind_int64(p_stmt: Option<&VdbeRef>, i: i32, i_value: i64) -> i32 {
    let rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        let p = p_stmt.unwrap();
        vdbe_mem_set_int64(&mut p.borrow_mut().a_var[(i - 1) as usize], i_value);
        mutex_leave(&stmt_db(p).borrow().mutex);
    }
    rc
}

/// Liga NULL.
pub fn api_bind_null(p_stmt: Option<&VdbeRef>, i: i32) -> i32 {
    let rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        mutex_leave(&stmt_db(p_stmt.unwrap()).borrow().mutex);
    }
    rc
}

/// Liga um valor de ponteiro (interface de passagem de ponteiros).
pub fn api_bind_pointer(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    p_ptr: Rc<dyn std::any::Any>,
    z_p_ttype: &[u8],
    x_destructor: Option<Rc<dyn Fn(Rc<dyn std::any::Any>)>>,
) -> i32 {
    let rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        let p = p_stmt.unwrap();
        vdbe_mem_set_pointer(
            &mut p.borrow_mut().a_var[(i - 1) as usize],
            p_ptr,
            z_p_ttype,
            x_destructor,
        );
        mutex_leave(&stmt_db(p).borrow().mutex);
    } else if let Some(x_destructor) = x_destructor {
        x_destructor(p_ptr);
    }
    rc
}

/// Liga um texto UTF-8.
pub fn api_bind_text(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    n_data: i32,
    x_del: Destructor,
) -> i32 {
    bind_text(p_stmt, i, z_data, n_data as i64, x_del, SQLITE_UTF8 as u8)
}

/// Liga um texto com tamanho de 64 bits e codificação explícita.
pub fn api_bind_text64(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    mut n_data: u64,
    x_del: Destructor,
    mut enc: u8,
) -> i32 {
    assert!(!matches!(x_del, Destructor::Dynamic));
    if enc != SQLITE_UTF8 as u8 {
        if enc == SQLITE_UTF16 as u8 {
            enc = SQLITE_UTF16NATIVE as u8;
        }
        // ~(u16)1 promove a int (-2) e estende o sinal para u64.
        n_data &= !1u64;
    }
    bind_text(p_stmt, i, z_data, n_data as i64, x_del, enc)
}

/// Liga um texto UTF-16 na ordem nativa.
pub fn api_bind_text16(
    p_stmt: Option<&VdbeRef>,
    i: i32,
    z_data: Option<&[u8]>,
    n: i32,
    x_del: Destructor,
) -> i32 {
    bind_text(
        p_stmt,
        i,
        z_data,
        ((n as i64 as u64) & !1u64) as i64,
        x_del,
        SQLITE_UTF16NATIVE as u8,
    )
}

/// Liga o valor de um `sqlite3_value`.
pub fn api_bind_value(p_stmt: Option<&VdbeRef>, i: i32, p_value: &Mem) -> i32 {
    match api_value_type(p_value) {
        SQLITE_INTEGER => api_bind_int64(p_stmt, i, p_value.u.i),
        SQLITE_FLOAT => {
            assert!((p_value.flags & (MEM_REAL | MEM_INTREAL)) != 0);
            api_bind_double(
                p_stmt,
                i,
                if (p_value.flags & MEM_REAL) != 0 { p_value.u.r } else { p_value.u.i as f64 },
            )
        }
        SQLITE_BLOB => {
            if (p_value.flags & MEM_ZERO) != 0 {
                api_bind_zeroblob(p_stmt, i, p_value.u.n_zero)
            } else {
                api_bind_blob(
                    p_stmt,
                    i,
                    Some(&p_value.z[..p_value.n as usize]),
                    p_value.n,
                    Destructor::Transient,
                )
            }
        }
        SQLITE_TEXT => bind_text(
            p_stmt,
            i,
            Some(&p_value.z[..p_value.n as usize]),
            p_value.n as i64,
            Destructor::Transient,
            p_value.enc,
        ),
        _ => api_bind_null(p_stmt, i),
    }
}

/// Liga um blob de zeros de n bytes.
pub fn api_bind_zeroblob(p_stmt: Option<&VdbeRef>, i: i32, n: i32) -> i32 {
    let rc = vdbe_unbind(p_stmt, (i - 1) as u32);
    if rc == SQLITE_OK {
        let p = p_stmt.unwrap();
        vdbe_mem_set_zero_blob(&mut p.borrow_mut().a_var[(i - 1) as usize], n);
        mutex_leave(&stmt_db(p).borrow().mutex);
    }
    rc
}

/// Liga um blob de zeros com tamanho de 64 bits.
pub fn api_bind_zeroblob64(p_stmt: Option<&VdbeRef>, i: i32, n: u64) -> i32 {
    let p = p_stmt.expect("sqlite3_bind_zeroblob64 chamada com instrução NULL");
    let db = stmt_db(p);
    mutex_enter(&db.borrow().mutex);
    let mut rc;
    if n > db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize] as u64 {
        rc = SQLITE_TOOBIG;
    } else {
        assert!((n & 0x7FFFFFFF) == n);
        rc = api_bind_zeroblob(p_stmt, i, n as i32);
    }
    rc = api_exit(&mut db.borrow_mut(), rc);
    mutex_leave(&db.borrow().mutex);
    rc
}

/// Devolve o número de curingas aos quais se pode ligar valores.
/// Rotina adicionada para suportar o DBD::SQLite.
pub fn api_bind_parameter_count(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        Some(p) => p.borrow().n_var as i32,
        None => 0,
    }
}


// ---- part_005.rs ----

// Mesmas convenções de part_004: `api_xxx` é `sqlite3_xxx`, `&VdbeRef` é `sqlite3_stmt*`,
// `&Sqlite3Ref` é `sqlite3*`. Em `preupdate_old`/`preupdate_new` o `sqlite3_value**` de saída
// vira `&mut Option<Mem>` e recebe uma cópia do valor (o C devolve ponteiro para a célula;
// as mutações que o C faz na célula antes de devolver são feitas aqui na célula, e a cópia sai depois).
// Opções do Debian: PREUPDATE_HOOK ligado; NORMALIZE e STMT_SCANSTATUS desligados.

/// Devolve o nome de um parâmetro curinga. Devolve None se o índice está fora do
/// intervalo ou se o curinga não tem nome. O resultado é sempre UTF-8.
pub fn api_bind_parameter_name(p_stmt: Option<&VdbeRef>, i: i32) -> Option<Vec<u8>> {
    let p = p_stmt?;
    vlist_num_to_name(p.borrow().p_v_list.as_deref(), i)
}

/// Dado o nome de um parâmetro curinga, devolve o índice da variável com esse nome.
/// Se não há variável com o nome dado, devolve 0.
pub fn vdbe_parameter_index(p: Option<&VdbeRef>, z_name: Option<&[u8]>, n_name: i32) -> i32 {
    match (p, z_name) {
        (Some(p), Some(z_name)) => vlist_name_to_num(p.borrow().p_v_list.as_deref(), z_name, n_name),
        _ => 0,
    }
}

/// Devolve o índice do parâmetro de nome z_name (terminado em NUL no C).
pub fn api_bind_parameter_index(p_stmt: Option<&VdbeRef>, z_name: &[u8]) -> i32 {
    vdbe_parameter_index(p_stmt, Some(z_name), strlen30(z_name))
}

/// Transfere todas as ligações da primeira instrução para a segunda.
pub fn transfer_bindings(p_from: &VdbeRef, p_to: &VdbeRef) -> i32 {
    let db = stmt_db(p_to);
    assert!(Rc::ptr_eq(&db, &stmt_db(p_from)));
    assert!(p_to.borrow().n_var == p_from.borrow().n_var);
    mutex_enter(&db.borrow().mutex);
    let n_var = p_from.borrow().n_var;
    {
        let mut from = p_from.borrow_mut();
        let mut to = p_to.borrow_mut();
        for i in 0..n_var as usize {
            vdbe_mem_move(&mut to.a_var[i], &mut from.a_var[i]);
        }
    }
    mutex_leave(&db.borrow().mutex);
    SQLITE_OK
}

/// Interface externa obsoleta. O código interno do SQLite deve chamar `transfer_bindings`.
///
/// Chamar com instruções de conexões diferentes é mau uso, mas como a interface
/// é obsoleta, essa condição não é verificada.
///
/// Se as duas instruções têm números diferentes de ligações, devolve SQLITE_ERROR.
/// Nada mais pode dar errado, então senão devolve SQLITE_OK.
pub fn api_transfer_bindings(p_from: &VdbeRef, p_to: &VdbeRef) -> i32 {
    if p_from.borrow().n_var != p_to.borrow().n_var {
        return SQLITE_ERROR;
    }
    {
        let mut to = p_to.borrow_mut();
        assert!((to.prep_flags & SQLITE_PREPARE_SAVESQL) != 0 || to.expmask == 0);
        if to.expmask != 0 {
            to.expired = 1;
        }
    }
    {
        let mut from = p_from.borrow_mut();
        assert!((from.prep_flags & SQLITE_PREPARE_SAVESQL) != 0 || from.expmask == 0);
        if from.expmask != 0 {
            from.expired = 1;
        }
    }
    transfer_bindings(p_from, p_to)
}

/// Devolve o handle `sqlite3*` ao qual pertence a instrução preparada. É o mesmo
/// handle que foi o primeiro argumento do sqlite3_prepare() que criou a instrução.
pub fn api_db_handle(p_stmt: Option<&VdbeRef>) -> Option<Sqlite3Ref> {
    p_stmt.map(stmt_db)
}

/// Devolve verdadeiro se a instrução preparada com certeza não modifica o banco.
pub fn api_stmt_readonly(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        Some(p) => p.borrow().read_only as i32,
        None => 1,
    }
}

/// Devolve 1 se a instrução é um EXPLAIN e 2 se é um EXPLAIN QUERY PLAN.
pub fn api_stmt_isexplain(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        Some(p) => p.borrow().explain as i32,
        None => 0,
    }
}

/// Define o modo explain de uma instrução.
pub fn api_stmt_explain(p_stmt: &VdbeRef, e_mode: i32) -> i32 {
    let db = stmt_db(p_stmt);
    let rc;
    mutex_enter(&db.borrow().mutex);
    let (explain, prep_flags, state, n_mem, have_eqp_ops) = {
        let v = p_stmt.borrow();
        (v.explain as i32, v.prep_flags, v.e_vdbe_state, v.n_mem, v.have_eqp_ops)
    };
    if explain == e_mode {
        rc = SQLITE_OK;
    } else if e_mode < 0 || e_mode > 2 {
        rc = SQLITE_ERROR;
    } else if (prep_flags & SQLITE_PREPARE_SAVESQL) == 0 {
        rc = SQLITE_ERROR;
    } else if state != VDBE_READY_STATE {
        rc = SQLITE_BUSY;
    } else if n_mem >= 10 && (e_mode != 2 || have_eqp_ops != 0) {
        // Nenhuma repreparação necessária
        p_stmt.borrow_mut().explain = e_mode as u8;
        rc = SQLITE_OK;
    } else {
        p_stmt.borrow_mut().explain = e_mode as u8;
        rc = reprepare(p_stmt);
        p_stmt.borrow_mut().have_eqp_ops = (e_mode == 2) as u8;
    }
    {
        let mut v = p_stmt.borrow_mut();
        if v.explain != 0 {
            v.n_res_column = (12 - 4 * v.explain as i32) as u16;
        } else {
            v.n_res_column = v.n_res_alloc;
        }
    }
    mutex_leave(&db.borrow().mutex);
    rc
}

/// Devolve verdadeiro se a instrução preparada precisa ser reiniciada.
pub fn api_stmt_busy(p_stmt: Option<&VdbeRef>) -> i32 {
    match p_stmt {
        Some(v) => (v.borrow().e_vdbe_state == VDBE_RUN_STATE) as i32,
        None => 0,
    }
}

/// Devolve a próxima instrução preparada depois de p_stmt associada à conexão p_db.
/// Se p_stmt é None, devolve a primeira instrução preparada da conexão.
/// Devolve None se não há mais.
pub fn api_next_stmt(p_db: &Sqlite3Ref, p_stmt: Option<&VdbeRef>) -> Option<VdbeRef> {
    mutex_enter(&p_db.borrow().mutex);
    let p_next = match p_stmt {
        None => p_db.borrow().p_vdbe.clone(),
        Some(p) => p.borrow().p_v_next.clone(),
    };
    mutex_leave(&p_db.borrow().mutex);
    p_next
}

/// Devolve o valor de um contador de status de uma instrução preparada.
pub fn api_stmt_status(p_vdbe: &VdbeRef, op: i32, reset_flag: i32) -> i32 {
    let v: u32;
    if op == SQLITE_STMTSTATUS_MEMUSED {
        let db = stmt_db(p_vdbe);
        mutex_enter(&db.borrow().mutex);
        // Em vez do ponteiro db->pnBytesFreed para a variável local, o contador é
        // uma célula compartilhada que vdbe_delete incrementa ao liberar memória.
        let counter = Rc::new(std::cell::Cell::new(0u32));
        {
            let mut d = db.borrow_mut();
            d.pn_bytes_freed = Some(counter.clone());
            assert!(d.lookaside.p_end == d.lookaside.p_true_end);
            d.lookaside.p_end = d.lookaside.p_start;
        }
        vdbe_delete(p_vdbe);
        {
            let mut d = db.borrow_mut();
            d.pn_bytes_freed = None;
            d.lookaside.p_end = d.lookaside.p_true_end;
        }
        mutex_leave(&db.borrow().mutex);
        v = counter.get();
    } else {
        let mut pv = p_vdbe.borrow_mut();
        v = pv.a_counter[op as usize];
        if reset_flag != 0 {
            pv.a_counter[op as usize] = 0;
        }
    }
    v as i32
}

/// Devolve o SQL associado a uma instrução preparada.
pub fn api_sql(p_stmt: Option<&VdbeRef>) -> Option<Vec<u8>> {
    p_stmt.and_then(|p| p.borrow().z_sql.clone())
}

/// Devolve o SQL associado a uma instrução preparada com os parâmetros ligados
/// expandidos. O limite SQLITE_TRACE_SIZE_LIMIT impõe um teto ao tamanho dos
/// parâmetros ligados expandidos.
pub fn api_expanded_sql(p_stmt: Option<&VdbeRef>) -> Option<Vec<u8>> {
    let z_sql = api_sql(p_stmt)?;
    let p = p_stmt.unwrap();
    let db = stmt_db(p);
    mutex_enter(&db.borrow().mutex);
    let z = vdbe_expand_sql(p, &z_sql);
    mutex_leave(&db.borrow().mutex);
    z
}

/// Aloca e preenche uma estrutura UnpackedRecord a partir do registro serializado
/// em n_key/p_key. Devolve None se ocorrer erro de memória.
fn vdbe_unpack_record(p_key_info: &KeyInfo, n_key: i32, p_key: &[u8]) -> Option<Box<UnpackedRecord>> {
    let mut p_ret = vdbe_alloc_unpacked_record(p_key_info)?;
    for m in p_ret.a_mem.iter_mut().take(p_key_info.n_key_field as usize + 1) {
        *m = Mem::default();
    }
    vdbe_record_unpack(p_key_info, n_key, p_key, &mut p_ret);
    Some(p_ret)
}

/// Chamada de dentro de um callback de pré-atualização para obter um campo da
/// linha que está sendo atualizada ou apagada.
pub fn api_preupdate_old(db: &Sqlite3Ref, i_idx: i32, pp_value: &mut Option<Mem>) -> i32 {
    let mut rc = SQLITE_OK;
    let mut i_idx = i_idx;
    let p_pre = db.borrow().p_pre_update.clone();
    'preupdate_old_out: {
        // Testa se a chamada vem de dentro de um callback SQLITE_DELETE ou
        // SQLITE_UPDATE, e se i_idx está dentro do intervalo.
        let p_pre = match p_pre {
            Some(p) if p.borrow().op != SQLITE_INSERT => p,
            _ => {
                rc = misuse_error(line!() as i32);
                break 'preupdate_old_out;
            }
        };
        let mut p = p_pre.borrow_mut();
        if let Some(p_pk) = &p.p_pk {
            i_idx = table_column_to_index(p_pk, i_idx);
        }
        let n_field = p.p_csr.borrow().n_field;
        if i_idx >= n_field || i_idx < 0 {
            rc = SQLITE_RANGE;
            break 'preupdate_old_out;
        }

        // Se o registro old.* ainda não foi carregado em memória, carrega agora.
        if p.p_unpacked.is_none() {
            assert!(p.p_csr.borrow().e_cur_type == CURTYPE_BTREE);
            let n_rec;
            let mut a_rec;
            {
                let csr = p.p_csr.borrow();
                let p_cursor = csr.uc.p_cursor.clone();
                n_rec = btree_payload_size(&p_cursor.borrow());
                a_rec = vec![0u8; n_rec as usize];
                rc = btree_payload(&mut p_cursor.borrow_mut(), 0, n_rec, &mut a_rec);
            }
            if rc == SQLITE_OK {
                p.p_unpacked = vdbe_unpack_record(&p.keyinfo, n_rec as i32, &a_rec);
                if p.p_unpacked.is_none() {
                    rc = SQLITE_NOMEM;
                }
            }
            if rc != SQLITE_OK {
                break 'preupdate_old_out;
            }
            p.a_record = Some(a_rec);
        }

        let i_p_key = p.p_tab.borrow().i_p_key as i32;
        let i_key1 = p.i_key1;
        // A afinidade só é lida no terceiro ramo do C (i_idx não é a chave primária
        // nem passa de n_field), por isso a leitura com guarda de intervalo.
        let affinity_real = if i_idx >= 0 && i_idx != i_p_key {
            p.p_tab.borrow().a_col.get(i_idx as usize).map(|c| c.affinity == SQLITE_AFF_REAL)
                == Some(true)
        } else {
            false
        };
        let unpacked = p.p_unpacked.as_mut().unwrap();
        if i_idx == i_p_key {
            vdbe_mem_set_int64(&mut unpacked.a_mem[i_idx as usize], i_key1);
            *pp_value = Some(unpacked.a_mem[i_idx as usize].clone());
        } else if i_idx >= unpacked.n_field as i32 {
            *pp_value = Some(column_null_value());
        } else {
            let p_mem = &mut unpacked.a_mem[i_idx as usize];
            if affinity_real && (p_mem.flags & (MEM_INT | MEM_INTREAL)) != 0 {
                vdbe_mem_realify(p_mem);
            }
            *pp_value = Some(p_mem.clone());
        }
    }
    error(&mut db.borrow_mut(), rc);
    api_exit(&mut db.borrow_mut(), rc)
}

/// Chamada de dentro de um callback de pré-atualização para obter o número de
/// colunas da linha que está sendo atualizada, apagada ou inserida.
pub fn api_preupdate_count(db: &Sqlite3Ref) -> i32 {
    match &db.borrow().p_pre_update {
        Some(p) => p.borrow().keyinfo.n_key_field as i32,
        None => 0,
    }
}


// ---- part_006.rs ----

// Mesmas convenções de part_004/part_005. Opção do Debian: STMT_SCANSTATUS desligada,
// então `sqlite3_stmt_scanstatus*` (que no C vem logo depois destas) não existe aqui.

/// Projetada para ser chamada só de dentro de um callback de pré-atualização.
/// Devolve zero se a mudança que causou o callback foi feita imediatamente por uma
/// instrução SQL do usuário. Ou, se a mudança foi feita por um programa de gatilho,
/// devolve o número de programas de gatilho na pilha (1 para um gatilho de nível
/// superior, 2 para um gatilho disparado por um gatilho de nível superior, etc.).
///
/// Para os fins do parágrafo anterior, uma ação de chave estrangeira CASCADE,
/// SET NULL ou SET DEFAULT é considerada um gatilho.
pub fn api_preupdate_depth(db: &Sqlite3Ref) -> i32 {
    match &db.borrow().p_pre_update {
        Some(p) => p.borrow().v.borrow().n_frame as i32,
        None => 0,
    }
}

/// Projetada para ser chamada só de dentro de um callback de pré-atualização.
pub fn api_preupdate_blobwrite(db: &Sqlite3Ref) -> i32 {
    match &db.borrow().p_pre_update {
        Some(p) => p.borrow().i_blob_write,
        None => -1,
    }
}

/// Chamada de dentro de um callback de pré-atualização para obter um campo da
/// linha que está sendo atualizada ou inserida.
pub fn api_preupdate_new(db: &Sqlite3Ref, i_idx: i32, pp_value: &mut Option<Mem>) -> i32 {
    let mut rc = SQLITE_OK;
    let mut i_idx = i_idx;
    let p_pre = db.borrow().p_pre_update.clone();
    'preupdate_new_out: {
        let p_pre = match p_pre {
            Some(p) if p.borrow().op != SQLITE_DELETE => p,
            _ => {
                rc = misuse_error(line!() as i32);
                break 'preupdate_new_out;
            }
        };
        let mut p = p_pre.borrow_mut();
        if p.p_pk.is_some() && p.op != SQLITE_UPDATE {
            i_idx = table_column_to_index(p.p_pk.as_ref().unwrap(), i_idx);
        }
        let n_field = p.p_csr.borrow().n_field;
        if i_idx >= n_field || i_idx < 0 {
            rc = SQLITE_RANGE;
            break 'preupdate_new_out;
        }

        let i_p_key = p.p_tab.borrow().i_p_key as i32;
        let p_mem: Mem;
        if p.op == SQLITE_INSERT {
            // Num INSERT, a célula de memória p.i_new_reg contém o registro serializado
            // que está sendo inserido. Desserializa-o.
            if p.p_new_unpacked.is_none() {
                let v = p.v.clone();
                let mut v_ref = v.borrow_mut();
                let p_data = &mut v_ref.a_mem[p.i_new_reg as usize];
                rc = expand_blob(p_data);
                if rc != SQLITE_OK {
                    break 'preupdate_new_out;
                }
                let p_unpack = vdbe_unpack_record(&p.keyinfo, p_data.n, &p_data.z);
                if p_unpack.is_none() {
                    rc = SQLITE_NOMEM;
                    break 'preupdate_new_out;
                }
                p.p_new_unpacked = p_unpack;
            }
            let i_key2 = p.i_key2;
            let p_unpack = p.p_new_unpacked.as_mut().unwrap();
            if i_idx == i_p_key {
                vdbe_mem_set_int64(&mut p_unpack.a_mem[i_idx as usize], i_key2);
                p_mem = p_unpack.a_mem[i_idx as usize].clone();
            } else if i_idx >= p_unpack.n_field as i32 {
                p_mem = column_null_value();
            } else {
                p_mem = p_unpack.a_mem[i_idx as usize].clone();
            }
        } else {
            // Num UPDATE, a célula de memória (p.i_new_reg+1+i_idx) contém o valor
            // necessário. Faz uma cópia do conteúdo da célula e devolve uma referência
            // a ela. Não é seguro devolver a própria célula de memória, pois o
            // chamador pode modificar a codificação de texto do valor.
            assert!(p.op == SQLITE_UPDATE);
            if p.a_new.is_none() {
                p.a_new = Some(vec![Mem::default(); n_field as usize]);
            }
            assert!(i_idx >= 0 && i_idx < n_field);
            let i_key2 = p.i_key2;
            let i_src = (p.i_new_reg + 1 + i_idx) as usize;
            let v = p.v.clone();
            let p_mem_ref = &mut p.a_new.as_mut().unwrap()[i_idx as usize];
            if p_mem_ref.flags == 0 {
                if i_idx == i_p_key {
                    vdbe_mem_set_int64(p_mem_ref, i_key2);
                } else {
                    rc = vdbe_mem_copy(p_mem_ref, &v.borrow().a_mem[i_src]);
                    if rc != SQLITE_OK {
                        break 'preupdate_new_out;
                    }
                }
            }
            p_mem = p_mem_ref.clone();
        }
        *pp_value = Some(p_mem);
    }
    error(&mut db.borrow_mut(), rc);
    api_exit(&mut db.borrow_mut(), rc)
}

