//! O resto do `vdbemem.c` que precisa de `Connection`, `Context`, `FuncDef` ou `Expr`
//! (`sqlite3VdbeMemFinalize`, `sqlite3VdbeMemAggValue`, `sqlite3VdbeMemSetPointer` e
//! `sqlite3ValueFromExpr`) mais o `sqlite3_value_pointer` do `vdbeapi.c`. O módulo é reexportado
//! por `crate::mem` (`pub use crate::mem2::*;`), então os chamadores continuam escrevendo
//! `crate::mem::value_from_expr` e afins.
//!
//! Convenções (ver o cabeçalho de `crate::mem`):
//!
//! - `SQLITE_ENABLE_STAT4` está desligado no Debian: `valueFromFunction` é a macro que vale
//!   `SQLITE_OK`, o ramo `TK_FUNCTION` de `valueFromExpr` não existe e o `ValueNewStat4Ctx` some;
//!   `valueNew` é só `sqlite3ValueNew`.
//! - O ponteiro de um valor ponteiro vive em `Mem.pointer` como `Rc<dyn Any>` (a cópia da célula
//!   compartilha o dado, como o C compartilha o ponteiro cru) e o `xDestructor` vira o `Drop`.

use std::any::Any;
use std::rc::Rc;

use crate::build::affinity_type;
use crate::connection::{Connection, Context, FuncDef};
use crate::consts::{
    EP_INT_VALUE, MEM_DYN, MEM_INT, MEM_INTREAL, MEM_NULL, MEM_REAL, MEM_STR, MEM_SUBTYPE,
    MEM_TERM, MEM_TYPEMASK, SMALLEST_INT64, SQLITE_LIMIT_LENGTH, SQLITE_NOMEM_BKPT, SQLITE_OK,
    TK_BLOB, TK_CAST, TK_FLOAT, TK_INTEGER, TK_NULL, TK_REGISTER, TK_SPAN, TK_STRING,
    TK_TRUEFALSE, TK_UMINUS, TK_UPLUS, SQLITE_AFF_BLOB, SQLITE_AFF_NUMERIC,
};
use crate::mem::{
    apply_affinity, mem_cast, mem_numerify, mem_release, mem_set_int64, mem_set_null,
    mem_set_str_dynamic, vdbe_change_encoding, Mem, ENC_UTF8, USE_LONG_DOUBLE,
};
use crate::printf::{mprintf, PrintfArg};
use crate::sqlite_int::Expr;
use crate::util::{at, atof, dec_or_hex_to_i64, hex_to_blob, oom_fault, strlen30};

// ---------------------------------------------------------------------------------------------
// Agregados (chunk 001 do vdbemem.c)
// ---------------------------------------------------------------------------------------------

/// Monta o `sqlite3_context` que `sqlite3VdbeMemFinalize` e `sqlite3VdbeMemAggValue` usam: sem
/// auxdata, sem hora corrente, sem colação, `argc` zero e `enc = ENC(db)`. O contexto do
/// agregado (`ctx.pMem`) entra em `agg` e `out` é a célula de saída (`ctx.pOut`).
fn agg_context<'a>(
    db: &'a mut Connection,
    p_func: &Rc<FuncDef>,
    aux: &'a mut Vec<crate::vdbe_types::AuxData>,
    clock: &'a mut i64,
    out: Mem,
    agg: Option<Box<dyn Any>>,
) -> Context<'a> {
    let enc = db.enc;
    Context {
        db,
        p_aux_data: aux,
        i_current_time: clock,
        out,
        arg_func: Rc::clone(p_func),
        agg,
        i_op: 0,
        is_pure_func: false,
        is_error: 0,
        enc,
        skip_flag: 0,
        argc: 0,
        p_coll: None,
    }
}

/// `sqlite3VdbeMemFinalize`: a célula `p` guarda o contexto de um agregado; chama o `xFinalize`
/// e grava o resultado de volta em `p`. Devolve `ctx.isError` (`SQLITE_ERROR` se o finalizador
/// reportou erro, `SQLITE_OK` caso contrário).
pub fn mem_finalize(db: &mut Connection, p: &mut Mem, func: &Rc<FuncDef>) -> i32 {
    let x_finalize = match func.x_finalize {
        Some(f) => f,
        None => return SQLITE_OK,
    };
    debug_assert!(p.flags & MEM_NULL != 0 || p.flags & crate::consts::MEM_AGG != 0);
    let mut aux = Vec::new();
    let mut clock: i64 = 0;
    // `t.flags = MEM_Null`: a saída começa NULL e o contexto do agregado é o de `p`.
    let t = Mem { flags: MEM_NULL, ..Mem::default() };
    let mut ctx = agg_context(db, func, &mut aux, &mut clock, t, p.agg.take());
    x_finalize(&mut ctx); /* IMP: R-24505-23230 */
    let is_error = ctx.is_error;
    let t = std::mem::take(&mut ctx.out);
    drop(ctx);
    debug_assert!(p.flags & MEM_DYN == 0);
    // `memcpy(pMem, &t, sizeof(t))`: a célula inteira vira a saída; o buffer e o acumulador
    // antigos caem com o valor anterior.
    *p = t;
    is_error
}

/// `sqlite3VdbeMemAggValue`: a célula `accum` guarda o contexto de um agregado de janela; chama
/// o `xValue` e grava o resultado em `out`, sem consumir o acumulador. Devolve `ctx.isError`.
pub fn mem_agg_value(
    db: &mut Connection,
    accum: &mut Mem,
    out: &mut Mem,
    func: &Rc<FuncDef>,
) -> i32 {
    let x_value = match func.x_value {
        Some(f) => f,
        None => return SQLITE_OK,
    };
    debug_assert!(accum.flags & MEM_NULL != 0 || accum.flags & crate::consts::MEM_AGG != 0);
    mem_set_null(out);
    let mut aux = Vec::new();
    let mut clock: i64 = 0;
    let mut ctx =
        agg_context(db, func, &mut aux, &mut clock, std::mem::take(out), accum.agg.take());
    x_value(&mut ctx);
    let is_error = ctx.is_error;
    // O `xValue` só lê o acumulador: ele volta para a célula.
    accum.agg = ctx.agg.take();
    *out = std::mem::take(&mut ctx.out);
    is_error
}

// ---------------------------------------------------------------------------------------------
// Valores ponteiro
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeMemSetPointer`: a célula, que já deve ser `NULL`, passa a ser um valor ponteiro
/// de tipo `name` (o `zPType`). O destrutor do C (`xDestructor`) é o `Drop` do `Box`.
pub fn mem_set_pointer(p: &mut Mem, ptr: Box<dyn Any>, name: &'static [u8]) {
    debug_assert!(p.flags == MEM_NULL);
    mem_release(p);
    p.pointer = Some((Rc::from(ptr), name));
    p.flags = MEM_NULL | MEM_DYN | MEM_SUBTYPE | MEM_TERM;
    p.e_subtype = b'p';
}

/// `sqlite3_value_pointer`: o ponteiro da célula se ela é um valor ponteiro de tipo `name`;
/// `None` caso contrário.
pub fn value_pointer<'a>(p: &'a Mem, name: &[u8]) -> Option<&'a dyn Any> {
    if p.flags & (MEM_TYPEMASK | MEM_TERM | MEM_SUBTYPE) == (MEM_NULL | MEM_TERM | MEM_SUBTYPE)
        && p.e_subtype == b'p'
    {
        if let Some((ptr, z_ptype)) = p.pointer.as_ref() {
            if *z_ptype == name {
                return Some(&**ptr);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// sqlite3ValueFromExpr (chunk 004 do vdbemem.c)
// ---------------------------------------------------------------------------------------------

/// `valueFromExpr` (sem STAT4, então sem `pCtx`): extrai um valor da expressão `p_expr`, que só
/// funciona para expressões simples de um único token constante. Se a expressão não vira valor
/// `pp_val` fica `None`. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
fn value_from_expr_inner(
    db: &mut Connection,
    p_expr: &Expr,
    enc: u8,
    affinity: u8,
    pp_val: &mut Option<Mem>,
) -> i32 {
    let limit = db.a_limit[SQLITE_LIMIT_LENGTH as usize];
    let mut p_expr = p_expr;
    let mut op = p_expr.op;
    let mut neg_int: i64 = 1;
    let mut z_neg: &[u8] = b"";
    let mut rc = SQLITE_OK;

    while op == TK_UPLUS || op == TK_SPAN {
        match p_expr.p_left.as_deref() {
            Some(left) => {
                p_expr = left;
                op = left.op;
            }
            None => {
                *pp_val = None;
                return SQLITE_OK;
            }
        }
    }
    if op == TK_REGISTER {
        op = p_expr.op2;
    }

    if op == TK_CAST {
        debug_assert!(!p_expr.has_property(EP_INT_VALUE));
        let aff = affinity_type(p_expr.z_token().unwrap_or(b""), None);
        let left = match p_expr.p_left.as_deref() {
            Some(l) => l,
            None => {
                *pp_val = None;
                return SQLITE_OK;
            }
        };
        rc = value_from_expr_inner(db, left, enc, aff, pp_val);
        if let Some(v) = pp_val.as_mut() {
            // Blobs com zeros só vêm de funções, e funções só são processadas com STAT4.
            debug_assert!(v.flags & crate::consts::MEM_ZERO == 0);
            mem_cast(v, aff, enc);
            apply_affinity(v, affinity, enc);
        }
        return rc;
    }

    // Os inteiros negativos entram num passo só, necessário para -9223372036854775808. Exceto
    // os literais hexadecimais.
    if op == TK_UMINUS {
        if let Some(left) = p_expr.p_left.as_deref() {
            if left.op == TK_INTEGER || left.op == TK_FLOAT {
                let z = left.z_token().unwrap_or(b"");
                if left.has_property(EP_INT_VALUE) || at(z, 0) != b'0' || (at(z, 1) & !0x20) != b'X'
                {
                    p_expr = left;
                    op = p_expr.op;
                    neg_int = -1;
                    z_neg = b"-";
                }
            }
        }
    }

    let mut p_val: Option<Mem> = None;
    if op == TK_STRING || op == TK_FLOAT || op == TK_INTEGER {
        let mut v = Mem::value_new();
        if p_expr.has_property(EP_INT_VALUE) {
            mem_set_int64(&mut v, (p_expr.i_value() as i64).wrapping_mul(neg_int));
        } else {
            let z_token = p_expr.z_token().unwrap_or(b"");
            let mut i_val: i64 = 0;
            if op == TK_INTEGER && dec_or_hex_to_i64(z_token, &mut i_val) == 0 {
                mem_set_int64(&mut v, i_val.wrapping_mul(neg_int));
            } else {
                let z_val = mprintf(
                    b"%s%s",
                    &[
                        PrintfArg::Text(Some(z_neg.to_vec())),
                        PrintfArg::Text(Some(z_token.to_vec())),
                    ],
                );
                match z_val {
                    Some(z_val) => {
                        mem_set_str_dynamic(&mut v, Some(z_val), -1, ENC_UTF8, limit);
                    }
                    None => {
                        oom_fault(db);
                        *pp_val = None;
                        return SQLITE_NOMEM_BKPT;
                    }
                }
            }
        }
        if affinity == SQLITE_AFF_BLOB {
            if op == TK_FLOAT {
                debug_assert!(v.flags == (MEM_STR | MEM_TERM));
                v.u_r = atof(v.bytes(), v.n, ENC_UTF8, USE_LONG_DOUBLE).1;
                v.flags = MEM_REAL;
            } else if op == TK_INTEGER {
                // Este caso é necessário para -9223372036854775808 e outras strings que parecem
                // inteiros mas `sqlite3DecOrHexToI64()` não trata.
                apply_affinity(&mut v, SQLITE_AFF_NUMERIC, ENC_UTF8);
            }
        } else {
            apply_affinity(&mut v, affinity, ENC_UTF8);
        }
        debug_assert!(v.flags & MEM_INTREAL == 0);
        if v.flags & (MEM_INT | MEM_INTREAL | MEM_REAL) != 0 {
            v.flags &= !MEM_STR;
        }
        if enc != ENC_UTF8 {
            rc = vdbe_change_encoding(&mut v, enc as i32);
        }
        p_val = Some(v);
    } else if op == TK_UMINUS {
        // Este ramo acontece com vários sinais negativos, por exemplo -(-5).
        if let Some(left) = p_expr.p_left.as_deref() {
            if value_from_expr_inner(db, left, enc, affinity, &mut p_val) == SQLITE_OK {
                if let Some(v) = p_val.as_mut() {
                    mem_numerify(v);
                    if v.flags & MEM_REAL != 0 {
                        v.u_r = -v.u_r;
                    } else if v.u_i == SMALLEST_INT64 {
                        v.u_r = -(SMALLEST_INT64 as f64);
                        v.set_type_flag(MEM_REAL);
                    } else {
                        v.u_i = -v.u_i;
                    }
                    apply_affinity(v, affinity, enc);
                }
            }
        }
    } else if op == TK_NULL {
        let mut v = Mem::value_new();
        mem_set_null(&mut v);
        p_val = Some(v);
    } else if op == TK_BLOB {
        debug_assert!(!p_expr.has_property(EP_INT_VALUE));
        let z_token = p_expr.z_token().unwrap_or(b"");
        debug_assert!(at(z_token, 0) == b'x' || at(z_token, 0) == b'X');
        debug_assert!(at(z_token, 1) == b'\'');
        let mut v = Mem::value_new();
        let z_val = z_token.get(2..).unwrap_or(b"");
        let n_val = strlen30(z_val) - 1;
        debug_assert!(at(z_val, n_val.max(0) as usize) == b'\'');
        let blob = hex_to_blob(z_val, n_val);
        mem_set_str_dynamic(&mut v, Some(blob), (n_val / 2) as i64, 0, limit);
        p_val = Some(v);
    } else if op == TK_TRUEFALSE {
        debug_assert!(!p_expr.has_property(EP_INT_VALUE));
        let mut v = Mem::value_new();
        v.flags = MEM_INT;
        v.u_i = (at(p_expr.z_token().unwrap_or(b""), 4) == 0) as i64;
        apply_affinity(&mut v, affinity, enc);
        p_val = Some(v);
    }

    *pp_val = p_val;
    rc
}

/// `sqlite3ValueFromExpr`: cria um valor com o conteúdo de `p_expr`. Só funciona para expressões
/// muito simples, de um único token constante (`5`, `5.1`, `'texto'`, `x'ab'`, `NULL`, `-5`). Se
/// a expressão vira valor, ele é gravado em `pp_val` (o chamador o libera com `value_free`); se
/// não, `pp_val` fica `None`. Sem expressão (`p_expr` ausente) não toca em `pp_val` e devolve
/// `SQLITE_OK`. Se o resultado for texto, usa a codificação `enc`.
///
/// `p_expr` aceita `&Expr` ou `Option<&Expr>` e o destino `Option<Mem>` ou `Option<Box<Mem>>`
/// (os dois jeitos que os chamadores guardam o valor).
pub fn value_from_expr<'a, E, T>(
    db: &mut Connection,
    p_expr: E,
    enc: u8,
    affinity: u8,
    pp_val: &mut Option<T>,
) -> i32
where
    E: Into<Option<&'a Expr>>,
    T: From<Mem>,
{
    match p_expr.into() {
        Some(e) => {
            let mut p_val: Option<Mem> = None;
            let rc = value_from_expr_inner(db, e, enc, affinity, &mut p_val);
            *pp_val = p_val.map(T::from);
            rc
        }
        None => SQLITE_OK,
    }
}
