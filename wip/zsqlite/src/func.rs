//! `func.c`: as funções SQL escalares e agregadas embutidas do SQLite 3.46.1 (`length`, `substr`,
//! `like`, `sum`, `group_concat`, as funções matemáticas, ...), a tabela `aBuiltinFunc` e o
//! registro de `like`/`glob`, no modelo v2 (ver `CONVENTIONS.md`).
//!
//! Desvios do C, todos decorrentes do modelo v2:
//!
//! * `argv` é `&[Mem]` imutável. O C converte o argumento no lugar (`sqlite3_value_text` troca a
//!   representação da célula); aqui os auxiliares [`text_of`], [`bytes_of`], [`blob_of`] e
//!   [`numeric_arg`] emprestam o conteúdo quando ele já está na forma pedida e convertem uma cópia
//!   quando não está. O resultado observável é o mesmo.
//! * Cadeia C (`const unsigned char*` terminada em NUL) vira fatia; o fim da fatia faz o papel do
//!   terminador (`at()` lê zero depois do fim) e os laços que no C param no NUL continuam parando
//!   no primeiro byte zero.
//! * O `pUserData` que carrega ponteiro de função (`ceil`, `exp`, ...) ou a `compareInfo` do
//!   `like` vira `UserData::Ptr` com [`Math1`], [`Math2`] ou [`CompareInfo`]; o que o C guarda com
//!   `SQLITE_INT_TO_PTR` vira `UserData::Int`.
//! * O acumulador de agregado é um `T: Default` em `Context.agg` (`aggregate_context`). O do
//!   `min`/`max` é uma `Mem` (flags zero é "ainda vazio", como o `pBest->flags` do C).
//! * `sqlite3GetFuncCollSeq` lê `Context.p_coll` (a colação do `OP_CollSeq` que precede a chamada),
//!   porque o `Context` não tem ponteiro para o `Vdbe`.
//! * `sqlite3_changes64`, `sqlite3_total_changes64` e `sqlite3_last_insert_rowid` são leituras de
//!   campo da conexão (`n_change`, `n_total_change`, `last_rowid`).
//! * Sem `SQLITE_DEBUG` (`fpdecode`), sem `SQLITE_ENABLE_UNKNOWN_SQL_FUNCTION`, sem
//!   `SQLITE_ENABLE_OFFSET_SQL_FUNC`, sem `SQLITE_USER_AUTHENTICATION` (não estão na lista do
//!   Debian 13). `SQLITE_SOUNDEX`, `SQLITE_ENABLE_MATH_FUNCTIONS` e a extensão carregável
//!   estão ligados.

use std::borrow::Cow;
use std::rc::Rc;

use crate::callback::{find_function, insert_builtin_funcs};
use crate::connection::{Connection, Context, FinalFn, FuncDef, ScalarFn, UserData};
use crate::consts::{
    INLINEFUNC_AFFINITY, INLINEFUNC_COALESCE, INLINEFUNC_EXPR_COMPARE,
    INLINEFUNC_EXPR_IMPLIES_EXPR, INLINEFUNC_IIF, INLINEFUNC_IMPLIES_NONNULL_ROW,
    INLINEFUNC_UNLIKELY, LARGEST_INT64, MEM_BLOB, MEM_NULL, MEM_STR, MEM_ZERO, SMALLEST_INT64,
    SQLITE_BLOB, SQLITE_FLOAT, SQLITE_FUNC_ANYORDER, SQLITE_FUNC_BUILTIN, SQLITE_FUNC_BYTELEN,
    SQLITE_FUNC_CASE, SQLITE_FUNC_CONSTANT, SQLITE_FUNC_COUNT, SQLITE_FUNC_DIRECT,
    SQLITE_FUNC_ENCMASK, SQLITE_FUNC_INLINE, SQLITE_FUNC_INTERNAL, SQLITE_FUNC_LENGTH,
    SQLITE_FUNC_LIKE, SQLITE_FUNC_MINMAX, SQLITE_FUNC_NEEDCOLL, SQLITE_FUNC_SLOCHNG,
    SQLITE_FUNC_TEST, SQLITE_FUNC_TYPEOF, SQLITE_FUNC_UNLIKELY, SQLITE_FUNC_UNSAFE,
    SQLITE_INTEGER, SQLITE_LIMIT_LENGTH, SQLITE_LIMIT_LIKE_PATTERN_LENGTH, SQLITE_LOAD_EXT_FUNC,
    SQLITE_NOMEM, SQLITE_NULL, SQLITE_TEXT, SQLITE_TOOBIG, SQLITE_UTF8, SQLITE_VERSION,
    TK_FUNCTION, TK_STRING,
};
use crate::ctype::{is_alpha, is_xdigit, to_lower, to_upper};
use crate::global::{log, randomness};
use crate::hash::hash_find_mut;
use crate::mem::{
    mem_compare, mem_copy, mem_release, value_numeric_type, value_type, CollSeq, Mem, StrDtor,
    ENC_UTF8, USE_LONG_DOUBLE,
};
use crate::printf::{mprintf, PrintfArg, PrintfArgs, PrintfArguments, PrintfValue, StrAccum};
use crate::sqlite_int::Expr;
use crate::utf::{skip_utf8, utf8_char_len, utf8_read, write_utf8};
use crate::util::{add_int64, at, atof, hex_to_int, is_overflow, oom_fault};
use crate::vdbeapi::text_of;
use crate::vdbeapi::{
    aggregate_context, result_blob, result_blob64, result_double, result_error, result_error_code,
    result_error_nomem, result_error_toobig, result_int, result_int64, result_null, result_text,
    result_text64, result_value, result_zeroblob64, value_blob, value_bytes, value_double,
    value_encoding, value_int, value_int64, value_subtype, value_text,
};

// ---------------------------------------------------------------------------------------------
// Auxiliares do modelo v2
// ---------------------------------------------------------------------------------------------


/// `sqlite3_value_bytes`: o tamanho do argumento em bytes UTF-8.
fn bytes_of(p: &Mem) -> i32 {
    if p.flags & MEM_STR != 0 && p.enc == ENC_UTF8 {
        return p.n;
    }
    if p.flags & MEM_BLOB != 0 {
        return if p.flags & MEM_ZERO != 0 { p.n.wrapping_add(p.n_zero) } else { p.n };
    }
    if p.flags & MEM_NULL != 0 {
        return 0;
    }
    let mut c = p.clone();
    value_bytes(&mut c)
}

/// `sqlite3_value_blob`: os bytes do argumento. `None` é o ponteiro nulo (blob vazio ou NULL).
fn blob_of(p: &Mem) -> Option<Cow<'_, [u8]>> {
    if p.flags & (MEM_BLOB | MEM_STR) != 0 {
        if p.flags & MEM_ZERO != 0 {
            let mut c = p.clone();
            return value_blob(&mut c).map(|s| Cow::Owned(s.to_vec()));
        }
        return if p.n != 0 { Some(Cow::Borrowed(p.bytes())) } else { None };
    }
    text_of(p)
}

/// `sqlite3_value_numeric_type` seguido das leituras numéricas: devolve o tipo e o valor (uma
/// cópia convertida quando o argumento é texto).
fn numeric_arg(p: &Mem) -> (i32, Cow<'_, Mem>) {
    if value_type(p) == SQLITE_TEXT {
        let mut c = p.clone();
        let t = value_numeric_type(&mut c);
        (t, Cow::Owned(c))
    } else {
        (value_type(p), Cow::Borrowed(p))
    }
}

/// O texto até o primeiro NUL (a cadeia C).
fn cstr(z: &[u8]) -> &[u8] {
    let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    &z[..n]
}

/// `sqlite3_user_data` quando o dado é um inteiro (`SQLITE_INT_TO_PTR`).
fn user_int(ctx: &Context<'_>) -> isize {
    match &ctx.arg_func.p_user_data {
        UserData::Int(i) => *i,
        _ => 0,
    }
}

/// `sqlite3GetFuncCollSeq`: a colação da chamada (do `OP_CollSeq` anterior).
fn get_func_coll_seq(ctx: &Context<'_>) -> Option<Rc<CollSeq>> {
    ctx.p_coll.clone()
}

/// `sqlite3SkipAccumulatorLoad`: a carga do acumulador pode ser pulada nesta volta do laço.
fn skip_accumulator_load(ctx: &mut Context<'_>) {
    debug_assert!(ctx.is_error <= 0);
    ctx.is_error = -1;
    ctx.skip_flag = 1;
}

/// `contextMalloc`: verdadeiro se `n_byte` cabe no limite de comprimento; senão grava
/// `SQLITE_TOOBIG` como resultado da função.
fn context_malloc_ok(ctx: &mut Context<'_>, n_byte: i64) -> bool {
    debug_assert!(n_byte > 0);
    if n_byte > ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64 {
        result_error_toobig(ctx);
        false
    } else {
        true
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 000
// ---------------------------------------------------------------------------------------------

/// `minmaxFunc`: os `min()` e `max()` escalares.
fn minmax_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() > 1);
    let mask: i32 = if user_int(ctx) == 0 { 0 } else { -1 };
    let coll = get_func_coll_seq(ctx);
    let mut i_best = 0usize;
    if value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    for i in 1..argv.len() {
        if value_type(&argv[i]) == SQLITE_NULL {
            return;
        }
        if (mem_compare(&argv[i_best], &argv[i], coll.as_deref()) ^ mask) >= 0 {
            i_best = i;
        }
    }
    result_value(ctx, &argv[i_best]);
}

/// `typeofFunc`.
fn typeof_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    const AZ_TYPE: [&[u8]; 5] = [b"integer", b"real", b"text", b"blob", b"null"];
    let i = (value_type(&argv[0]) - 1) as usize;
    debug_assert!(i < AZ_TYPE.len());
    result_text(ctx, Some(AZ_TYPE[i]), -1, StrDtor::Static);
}

/// `subtypeFunc`.
fn subtype_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    result_int(ctx, value_subtype(&argv[0]) as i32);
}

/// `lengthFunc`.
fn length_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    match value_type(&argv[0]) {
        SQLITE_BLOB | SQLITE_INTEGER | SQLITE_FLOAT => {
            let n = bytes_of(&argv[0]);
            result_int(ctx, n);
        }
        SQLITE_TEXT => {
            let Some(z) = text_of(&argv[0]) else { return };
            let mut p = 0usize;
            let mut n_skipped = 0i32;
            loop {
                let c = at(&z, p);
                if c == 0 {
                    break;
                }
                p += 1;
                if c >= 0xc0 {
                    while (at(&z, p) & 0xc0) == 0x80 {
                        p += 1;
                        n_skipped += 1;
                    }
                }
            }
            result_int(ctx, p as i32 - n_skipped);
        }
        _ => result_null(ctx),
    }
}

/// `bytelengthFunc`: `octet_length()`.
fn bytelength_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    match value_type(&argv[0]) {
        SQLITE_BLOB => {
            let n = bytes_of(&argv[0]);
            result_int(ctx, n);
        }
        SQLITE_INTEGER | SQLITE_FLOAT => {
            let m: i64 = if ctx.db.enc <= SQLITE_UTF8 as u8 { 1 } else { 2 };
            let n = bytes_of(&argv[0]) as i64;
            result_int64(ctx, n * m);
        }
        SQLITE_TEXT => {
            if value_encoding(&argv[0]) <= SQLITE_UTF8 {
                let n = bytes_of(&argv[0]);
                result_int(ctx, n);
            } else {
                // `sqlite3_value_bytes16`: o texto já está em UTF-16, o tamanho é o próprio `n`.
                result_int(ctx, argv[0].n);
            }
        }
        _ => result_null(ctx),
    }
}

/// `absFunc`.
fn abs_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    match value_type(&argv[0]) {
        SQLITE_INTEGER => {
            let mut i_val = value_int64(&argv[0]);
            if i_val < 0 {
                if i_val == SMALLEST_INT64 {
                    result_error(ctx, b"integer overflow", -1);
                    return;
                }
                i_val = -i_val;
            }
            result_int64(ctx, i_val);
        }
        SQLITE_NULL => result_null(ctx),
        _ => {
            let mut r_val = value_double(&argv[0]);
            if r_val < 0.0 {
                r_val = -r_val;
            }
            result_double(ctx, r_val);
        }
    }
}

/// `instrFunc`.
fn instr_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let type_haystack = value_type(&argv[0]);
    let type_needle = value_type(&argv[1]);
    if type_haystack == SQLITE_NULL || type_needle == SQLITE_NULL {
        return;
    }
    let mut n_haystack = bytes_of(&argv[0]);
    let mut n_needle = bytes_of(&argv[1]);
    let mut n: i32 = 1;
    if n_needle > 0 {
        let hay: Option<Cow<'_, [u8]>>;
        let needle: Option<Cow<'_, [u8]>>;
        let is_text: bool;
        if type_haystack == SQLITE_BLOB && type_needle == SQLITE_BLOB {
            hay = blob_of(&argv[0]);
            needle = blob_of(&argv[1]);
            is_text = false;
        } else {
            hay = text_of(&argv[0]);
            needle = text_of(&argv[1]);
            if type_haystack == SQLITE_BLOB || type_needle == SQLITE_BLOB {
                // Um blob e um texto: os dois viram texto e os tamanhos são os do texto.
                if hay.is_none() || needle.is_none() {
                    result_error_nomem(ctx);
                    return;
                }
                n_haystack = hay.as_ref().map_or(0, |h| h.len() as i32);
                n_needle = needle.as_ref().map_or(0, |h| h.len() as i32);
            }
            is_text = true;
        }
        let Some(needle) = needle else {
            result_error_nomem(ctx);
            return;
        };
        if n_haystack != 0 && hay.is_none() {
            result_error_nomem(ctx);
            return;
        }
        let hay = hay.unwrap_or(Cow::Borrowed(&[]));
        let first_char = at(&needle, 0);
        let mut pos = 0usize;
        while n_needle <= n_haystack
            && (at(&hay, pos) != first_char
                || hay[pos..pos + n_needle as usize] != needle[..n_needle as usize])
        {
            n += 1;
            loop {
                n_haystack -= 1;
                pos += 1;
                if !(is_text && (at(&hay, pos) & 0xc0) == 0x80) {
                    break;
                }
            }
        }
        if n_needle > n_haystack {
            n = 0;
        }
    }
    result_int(ctx, n);
}

/// O argumento do `printf()` da linguagem SQL (`sqlite3_value_*` do `PrintfArguments`).
struct SqlArg(Mem);

impl PrintfValue for SqlArg {
    fn value_int64(&mut self) -> i64 {
        value_int64(&self.0)
    }
    fn value_double(&mut self) -> f64 {
        value_double(&self.0)
    }
    fn value_text(&mut self) -> Option<Vec<u8>> {
        value_text(&mut self.0).map(|s| s.to_vec())
    }
}

/// `printfFunc`: `printf()` e `format()`.
fn printf_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    if argv.is_empty() {
        return;
    }
    let Some(z_format) = text_of(&argv[0]) else { return };
    let mut args: Vec<SqlArg> = argv[1..].iter().map(|m| SqlArg(m.clone())).collect();
    let mut x = PrintfArguments {
        n_used: 0,
        ap_arg: args.iter_mut().map(|a| a as &mut dyn PrintfValue).collect(),
    };
    let mut str_ = StrAccum::new(ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
    // O `db` do acumulador só serve ao `sqlite3ErrorToParser`, que não faz nada fora de um parse.
    str_.has_db = true;
    str_.printf_flags = crate::consts::SQLITE_PRINTF_SQLFUNC;
    str_.str_vappendf(&z_format, PrintfArgs::SqlFunc(&mut x));
    let n = str_.n_char as i32;
    let out = str_.finish();
    result_text(ctx, out.as_deref(), n, StrDtor::Dynamic);
}

/// `substrFunc`: `substr()` e `substring()`.
fn substr_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 3 || argc == 2);
    if value_type(&argv[1]) == SQLITE_NULL || (argc == 3 && value_type(&argv[2]) == SQLITE_NULL) {
        return;
    }
    let p0type = value_type(&argv[0]);
    let mut p1: i64 = value_int(&argv[1]) as i64;
    let z: Cow<'_, [u8]>;
    let mut len: i32;
    if p0type == SQLITE_BLOB {
        len = bytes_of(&argv[0]);
        match blob_of(&argv[0]) {
            Some(b) => z = b,
            None => return,
        }
    } else {
        match text_of(&argv[0]) {
            Some(t) => z = t,
            None => return,
        }
        len = 0;
        if p1 < 0 {
            let mut z2 = 0usize;
            while at(&z, z2) != 0 {
                skip_utf8(&z, &mut z2);
                len += 1;
            }
        }
    }
    let mut p2: i64;
    let mut neg_p2 = false;
    if argc == 3 {
        p2 = value_int(&argv[2]) as i64;
        if p2 < 0 {
            p2 = -p2;
            neg_p2 = true;
        }
    } else {
        p2 = ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64;
    }
    if p1 < 0 {
        p1 += len as i64;
        if p1 < 0 {
            p2 += p1;
            if p2 < 0 {
                p2 = 0;
            }
            p1 = 0;
        }
    } else if p1 > 0 {
        p1 -= 1;
    } else if p2 > 0 {
        p2 -= 1;
    }
    if neg_p2 {
        p1 -= p2;
        if p1 < 0 {
            p2 += p1;
            p1 = 0;
        }
    }
    debug_assert!(p1 >= 0 && p2 >= 0);
    if p0type != SQLITE_BLOB {
        let mut zp = 0usize;
        while at(&z, zp) != 0 && p1 != 0 {
            skip_utf8(&z, &mut zp);
            p1 -= 1;
        }
        let mut z2 = zp;
        while at(&z, z2) != 0 && p2 != 0 {
            skip_utf8(&z, &mut z2);
            p2 -= 1;
        }
        let end = z2.min(z.len());
        let start = zp.min(end);
        result_text64(ctx, Some(&z[start..end]), (z2 - zp) as u64, StrDtor::Transient, SQLITE_UTF8 as u8);
    } else {
        if p1 + p2 > len as i64 {
            p2 = len as i64 - p1;
            if p2 < 0 {
                p2 = 0;
            }
        }
        let start = (p1 as usize).min(z.len());
        let end = (start + p2 as usize).min(z.len());
        result_blob64(ctx, Some(&z[start..end]), p2 as u64, StrDtor::Transient);
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 001
// ---------------------------------------------------------------------------------------------

/// `roundFunc`.
fn round_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    let mut n: i32 = 0;
    debug_assert!(argc == 1 || argc == 2);
    if argc == 2 {
        if value_type(&argv[1]) == SQLITE_NULL {
            return;
        }
        n = value_int(&argv[1]);
        if n > 30 {
            n = 30;
        }
        if n < 0 {
            n = 0;
        }
    }
    if value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    let mut r = value_double(&argv[0]);
    // Se Y==0 e X cabe num inteiro de 64 bits, o arredondamento é direto; senão passa pelo printf.
    if r < -4503599627370496.0 || r > 4503599627370496.0 {
        // O valor não tem parte fracionária: não há o que arredondar.
    } else if n == 0 {
        r = (r + if r < 0.0 { -0.5 } else { 0.5 }) as i64 as f64;
    } else {
        let Some(z_buf) = mprintf(b"%!.*f", &[PrintfArg::Int(n as i64), PrintfArg::Double(r)]) else {
            result_error_nomem(ctx);
            return;
        };
        r = atof(&z_buf, z_buf.len() as i32, ENC_UTF8, USE_LONG_DOUBLE).1;
    }
    result_double(ctx, r);
}

/// `upperFunc` e `lowerFunc`: `upper` é verdadeiro para `upper()`.
fn case_func(ctx: &mut Context<'_>, argv: &[Mem], upper: bool) {
    let Some(z2) = text_of(&argv[0]) else { return };
    let n = z2.len();
    if context_malloc_ok(ctx, n as i64 + 1) {
        let z1: Vec<u8> = z2
            .iter()
            .map(|&c| if upper { to_upper(c) } else { to_lower(c) })
            .collect();
        result_text(ctx, Some(&z1), n as i32, StrDtor::Dynamic);
    }
}

/// `upperFunc`.
fn upper_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    case_func(ctx, argv, true);
}

/// `lowerFunc`.
fn lower_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    case_func(ctx, argv, false);
}

/// `noopFunc` (`versionFunc`): o que as funções embutidas como código do VDBE usam; nunca chamada.
fn noop_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    version_func(ctx, argv);
}

/// `randomFunc`.
fn random_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    let mut buf = [0u8; 8];
    randomness(&mut buf);
    let mut r = i64::from_ne_bytes(buf);
    if r < 0 {
        // Evita 0x8000000000000000, que no `abs()` voltaria igual: tira o bit de sinal e nega.
        r = -(r & LARGEST_INT64);
    }
    result_int64(ctx, r);
}

/// `randomBlob`.
fn random_blob(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let mut n = value_int64(&argv[0]);
    if n < 1 {
        n = 1;
    }
    if context_malloc_ok(ctx, n) {
        let mut p = vec![0u8; n as usize];
        randomness(&mut p);
        result_blob(ctx, Some(&p), n as i32, StrDtor::Dynamic);
    }
}

/// `last_insert_rowid`: o `sqlite3_last_insert_rowid(db)`.
fn last_insert_rowid(ctx: &mut Context<'_>, _argv: &[Mem]) {
    let v = ctx.db.last_rowid;
    result_int64(ctx, v);
}

/// `changes`: o `sqlite3_changes64(db)`.
fn changes(ctx: &mut Context<'_>, _argv: &[Mem]) {
    let v = ctx.db.n_change;
    result_int64(ctx, v);
}

/// `total_changes`: o `sqlite3_total_changes64(db)`.
fn total_changes(ctx: &mut Context<'_>, _argv: &[Mem]) {
    let v = ctx.db.n_total_change;
    result_int64(ctx, v);
}

/// `struct compareInfo`: como fazer comparações no estilo GLOB ou LIKE. É o `pUserData` do `like`
/// e do `glob`; os três primeiros campos são os curingas que o planejador lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareInfo {
    /// `*` ou `%`.
    pub match_all: u8,
    /// `?` ou `_`.
    pub match_one: u8,
    /// `[` ou 0.
    pub match_set: u8,
    /// Verdadeiro para ignorar a diferença de maiúsculas.
    pub no_case: u8,
}

/// `globInfo`.
const GLOB_INFO: CompareInfo = CompareInfo { match_all: b'*', match_one: b'?', match_set: b'[', no_case: 0 };
/// `likeInfoNorm`: o comportamento correto do SQL-92 ignora maiúsculas.
const LIKE_INFO_NORM: CompareInfo = CompareInfo { match_all: b'%', match_one: b'_', match_set: 0, no_case: 1 };
/// `likeInfoAlt`: com `case_sensitive_like`.
const LIKE_INFO_ALT: CompareInfo = CompareInfo { match_all: b'%', match_one: b'_', match_set: 0, no_case: 0 };

/// Retornos de `patternCompare`.
const SQLITE_MATCH: i32 = 0;
const SQLITE_NOMATCH: i32 = 1;
const SQLITE_NOWILDCARDMATCH: i32 = 2;

/// `patternCompare`: compara a cadeia UTF-8 `z_string` (a partir de `si`) com o padrão GLOB ou
/// LIKE `pat` (a partir de `pi`). Devolve `SQLITE_MATCH`, `SQLITE_NOMATCH` ou
/// `SQLITE_NOWILDCARDMATCH` (não casa apesar de haver `*` ou `%`). `match_other` é o escape
/// (LIKE) ou `[` (GLOB). Costuma ser rápida, mas é N**2 no pior caso.
fn pattern_compare(
    pat: &[u8],
    mut pi: usize,
    z_string: &[u8],
    mut si: usize,
    info: &CompareInfo,
    match_other: u32,
) -> i32 {
    let match_one = info.match_one as u32;
    let match_all = info.match_all as u32;
    let no_case = info.no_case != 0;
    let mut z_escaped: Option<usize> = None;

    loop {
        let mut c = utf8_read(pat, &mut pi);
        if c == 0 {
            break;
        }
        if c == match_all {
            // Pula os `*` repetidos; cada `?` pulado consome um caractere da entrada.
            loop {
                c = utf8_read(pat, &mut pi);
                if c == match_all || (c == match_one && match_one != 0) {
                    if c == match_one && utf8_read(z_string, &mut si) == 0 {
                        return SQLITE_NOWILDCARDMATCH;
                    }
                } else {
                    break;
                }
            }
            if c == 0 {
                return SQLITE_MATCH;
            } else if c == match_other {
                if info.match_set == 0 {
                    c = utf8_read(pat, &mut pi);
                    if c == 0 {
                        return SQLITE_NOWILDCARDMATCH;
                    }
                } else {
                    // `[...]` logo depois do `*`: busca recursiva lenta, caso incomum.
                    debug_assert!(match_other < 0x80);
                    while at(z_string, si) != 0 {
                        let b = pattern_compare(pat, pi - 1, z_string, si, info, match_other);
                        if b != SQLITE_NOMATCH {
                            return b;
                        }
                        skip_utf8(z_string, &mut si);
                    }
                    return SQLITE_NOWILDCARDMATCH;
                }
            }
            // `c` é o primeiro caractere do padrão depois do `*`: procura-o na entrada e continua
            // recursivamente dali. Sem diferenciar maiúsculas, procura `c` ou o seu par.
            if c < 0x80 {
                let z_stop: Vec<u8> = if no_case {
                    vec![to_upper(c as u8), to_lower(c as u8)]
                } else {
                    vec![c as u8]
                };
                loop {
                    while at(z_string, si) != 0 && !z_stop.contains(&at(z_string, si)) {
                        si += 1;
                    }
                    if at(z_string, si) == 0 {
                        break;
                    }
                    si += 1;
                    let b = pattern_compare(pat, pi, z_string, si, info, match_other);
                    if b != SQLITE_NOMATCH {
                        return b;
                    }
                }
            } else {
                loop {
                    let c2 = utf8_read(z_string, &mut si);
                    if c2 == 0 {
                        break;
                    }
                    if c2 != c {
                        continue;
                    }
                    let b = pattern_compare(pat, pi, z_string, si, info, match_other);
                    if b != SQLITE_NOMATCH {
                        return b;
                    }
                }
            }
            return SQLITE_NOWILDCARDMATCH;
        }
        if c == match_other {
            if info.match_set == 0 {
                c = utf8_read(pat, &mut pi);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                z_escaped = Some(pi);
            } else {
                let mut prior_c: u32 = 0;
                let mut seen = false;
                let mut invert = false;
                c = utf8_read(z_string, &mut si);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                let mut c2 = utf8_read(pat, &mut pi);
                if c2 == b'^' as u32 {
                    invert = true;
                    c2 = utf8_read(pat, &mut pi);
                }
                if c2 == b']' as u32 {
                    if c == b']' as u32 {
                        seen = true;
                    }
                    c2 = utf8_read(pat, &mut pi);
                }
                while c2 != 0 && c2 != b']' as u32 {
                    if c2 == b'-' as u32
                        && at(pat, pi) != b']'
                        && at(pat, pi) != 0
                        && prior_c > 0
                    {
                        c2 = utf8_read(pat, &mut pi);
                        if c >= prior_c && c <= c2 {
                            seen = true;
                        }
                        prior_c = 0;
                    } else {
                        if c == c2 {
                            seen = true;
                        }
                        prior_c = c2;
                    }
                    c2 = utf8_read(pat, &mut pi);
                }
                if c2 == 0 || seen == invert {
                    return SQLITE_NOMATCH;
                }
                continue;
            }
        }
        let c2 = utf8_read(z_string, &mut si);
        if c == c2 {
            continue;
        }
        if no_case && to_lower(c as u8) == to_lower(c2 as u8) && c < 0x80 && c2 < 0x80 {
            continue;
        }
        if c == match_one && Some(pi) != z_escaped && c2 != 0 {
            continue;
        }
        return SQLITE_NOMATCH;
    }
    if at(z_string, si) == 0 {
        SQLITE_MATCH
    } else {
        SQLITE_NOMATCH
    }
}

/// `sqlite3_strglob`: 0 se casa (como `strcmp()`), diferente de zero se não casa.
pub fn strglob(z_glob_pattern: Option<&[u8]>, z_string: Option<&[u8]>) -> i32 {
    match (z_glob_pattern, z_string) {
        (p, None) => p.is_some() as i32,
        (None, _) => 1,
        (Some(p), Some(s)) => pattern_compare(p, 0, s, 0, &GLOB_INFO, b'[' as u32),
    }
}

/// `sqlite3_strlike`: 0 se casa, diferente de zero se não casa.
pub fn strlike(z_pattern: Option<&[u8]>, z_str: Option<&[u8]>, esc: u32) -> i32 {
    match (z_pattern, z_str) {
        (p, None) => p.is_some() as i32,
        (None, _) => 1,
        (Some(p), Some(s)) => pattern_compare(p, 0, s, 0, &LIKE_INFO_NORM, esc),
    }
}

/// A `compareInfo` do `pUserData` da função `like` ou `glob`.
fn compare_info_of(ctx: &Context<'_>) -> CompareInfo {
    match &ctx.arg_func.p_user_data {
        UserData::Ptr(p) => p.downcast_ref::<CompareInfo>().copied().unwrap_or(LIKE_INFO_NORM),
        _ => LIKE_INFO_NORM,
    }
}

/// `likeFunc`: o `like()` do SQL (`A LIKE B` é `like(B,A)`); com outra `compareInfo`, o `glob()`.
fn like_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    let mut info = compare_info_of(ctx);

    // Limita o padrão para evitar recursão profunda e N*N em `patternCompare`.
    let n_pat = bytes_of(&argv[0]);
    if n_pat > ctx.db.a_limit[SQLITE_LIMIT_LIKE_PATTERN_LENGTH as usize] {
        result_error(ctx, b"LIKE or GLOB pattern too complex", -1);
        return;
    }
    let escape: u32;
    if argc == 3 {
        // O escape tem de ser um único caractere UTF-8.
        let Some(z_esc) = text_of(&argv[2]) else { return };
        if utf8_char_len(&z_esc, -1) != 1 {
            result_error(ctx, b"ESCAPE expression must be a single character", -1);
            return;
        }
        let mut p = 0usize;
        escape = utf8_read(&z_esc, &mut p);
        if escape == info.match_all as u32 || escape == info.match_one as u32 {
            if escape == info.match_all as u32 {
                info.match_all = 0;
            }
            if escape == info.match_one as u32 {
                info.match_one = 0;
            }
        }
    } else {
        escape = info.match_set as u32;
    }
    let z_b = text_of(&argv[0]);
    let z_a = text_of(&argv[1]);
    if let (Some(z_a), Some(z_b)) = (z_a, z_b) {
        let r = (pattern_compare(&z_b, 0, &z_a, 0, &info, escape) == SQLITE_MATCH) as i32;
        result_int(ctx, r);
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 002
// ---------------------------------------------------------------------------------------------

/// `nullifFunc`: o resultado é o primeiro argumento se os dois diferem, senão NULL.
fn nullif_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let coll = get_func_coll_seq(ctx);
    if mem_compare(&argv[0], &argv[1], coll.as_deref()) != 0 {
        result_value(ctx, &argv[0]);
    }
}

/// `versionFunc`: `sqlite_version()`.
fn version_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    result_text(ctx, Some(SQLITE_VERSION.as_bytes()), -1, StrDtor::Static);
}

/// `sourceidFunc`: `sqlite_source_id()`.
fn sourceid_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    result_text(ctx, Some(crate::main::sourceid().as_bytes()), -1, StrDtor::Static);
}

/// `errlogFunc`: `sqlite_log()`, um invólucro de `sqlite3_log()`; só tem efeito colateral.
fn errlog_func(_ctx: &mut Context<'_>, argv: &[Mem]) {
    let code = value_int(&argv[0]);
    let msg = text_of(&argv[1]).map(|c| c.into_owned());
    log(code, b"%s", &[PrintfArg::Text(msg)]);
}

/// `compileoptionusedFunc`: `sqlite_compileoption_used()`.
fn compileoptionused_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    if let Some(z_opt_name) = text_of(&argv[0]) {
        let r = crate::ctime::compileoption_used(cstr(&z_opt_name));
        result_int(ctx, r);
    }
}

/// `compileoptiongetFunc`: `sqlite_compileoption_get()`.
fn compileoptionget_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let n = value_int(&argv[0]);
    let z = crate::ctime::compileoption_get(n);
    result_text(ctx, z.map(|s| s.as_bytes()), -1, StrDtor::Static);
}

/// `hexdigits`.
const HEXDIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// `sqlite3QuoteValue`: acrescenta a `p_str` (que tem de estar vazio) a representação do valor
/// como literal SQL.
pub fn quote_value(p_str: &mut StrAccum, p_value: &Mem) {
    debug_assert!(p_str.n_char == 0);
    match value_type(p_value) {
        SQLITE_FLOAT => {
            let r1 = value_double(p_value);
            p_str.appendf(b"%!0.15g", &[PrintfArg::Double(r1)]);
            let n_char = p_str.n_char as i32;
            let differs = match p_str.value() {
                Some(z_val) => atof(z_val, n_char, ENC_UTF8, USE_LONG_DOUBLE).1 != r1,
                None => false,
            };
            if differs {
                p_str.reset();
                p_str.appendf(b"%!0.20e", &[PrintfArg::Double(r1)]);
            }
        }
        SQLITE_INTEGER => {
            p_str.appendf(b"%lld", &[PrintfArg::Int(value_int64(p_value))]);
        }
        SQLITE_BLOB => {
            let z_blob = blob_of(p_value);
            let n_blob = bytes_of(p_value) as i64;
            p_str.str_accum_enlarge(n_blob * 2 + 4);
            if p_str.acc_error == 0 {
                let mut buf: Vec<u8> = Vec::with_capacity(n_blob as usize * 2 + 3);
                buf.extend_from_slice(b"X'");
                if let Some(b) = z_blob.as_deref() {
                    for &c in b.iter().take(n_blob as usize) {
                        buf.push(HEXDIGITS[((c >> 4) & 0x0f) as usize]);
                        buf.push(HEXDIGITS[(c & 0x0f) as usize]);
                    }
                }
                buf.push(b'\'');
                p_str.append(&buf);
            }
        }
        SQLITE_TEXT => {
            let z_arg = text_of(p_value).map(|c| c.into_owned());
            p_str.appendf(b"%Q", &[PrintfArg::Text(z_arg)]);
        }
        _ => {
            debug_assert!(value_type(p_value) == SQLITE_NULL);
            p_str.append(b"NULL");
        }
    }
}

/// `quoteFunc`: `quote(X)`, o literal SQL do valor.
fn quote_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let mut str_ = StrAccum::new(ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32);
    str_.has_db = true;
    quote_value(&mut str_, &argv[0]);
    let n = str_.n_char as i32;
    let err = str_.errcode();
    let out = str_.finish();
    result_text(ctx, out.as_deref(), n, StrDtor::Dynamic);
    if err != 0 {
        result_null(ctx);
        result_error_code(ctx, err);
    }
}

/// `unicodeFunc`: o ponto de código do primeiro caractere.
fn unicode_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(z) = text_of(&argv[0]) else { return };
    if at(&z, 0) != 0 {
        let mut p = 0usize;
        let c = utf8_read(&z, &mut p);
        result_int(ctx, c as i32);
    }
}

/// `charFunc`: monta um texto com o caractere Unicode de cada argumento inteiro.
fn char_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let mut z: Vec<u8> = Vec::with_capacity(argv.len() * 4 + 1);
    for a in argv {
        let mut x = value_int64(a);
        if !(0..=0x10ffff).contains(&x) {
            x = 0xfffd;
        }
        let c = (x & 0x1fffff) as u32;
        write_utf8(&mut z, c);
    }
    let n = z.len() as u64;
    result_text64(ctx, Some(&z), n, StrDtor::Dynamic, SQLITE_UTF8 as u8);
}

// ---------------------------------------------------------------------------------------------
// chunk 003
// ---------------------------------------------------------------------------------------------

/// `hexFunc`: o argumento como blob, em hexadecimal.
fn hex_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let blob = blob_of(&argv[0]);
    let n = bytes_of(&argv[0]) as i64;
    if context_malloc_ok(ctx, n * 2 + 1) {
        let mut z_hex: Vec<u8> = Vec::with_capacity(n as usize * 2);
        if let Some(b) = blob.as_deref() {
            for &c in b.iter().take(n as usize) {
                z_hex.push(HEXDIGITS[((c >> 4) & 0xf) as usize]);
                z_hex.push(HEXDIGITS[(c & 0xf) as usize]);
            }
        }
        let len = z_hex.len() as u64;
        result_text64(ctx, Some(&z_hex), len, StrDtor::Dynamic, SQLITE_UTF8 as u8);
    }
}

/// `strContainsChar`: verdadeiro se `z_str` (UTF-8) contém o caractere `ch`.
fn str_contains_char(z_str: &[u8], ch: u32) -> bool {
    let mut p = 0usize;
    while p < z_str.len() {
        let tst = utf8_read(z_str, &mut p);
        if tst == ch {
            return true;
        }
    }
    false
}

/// `unhexFunc`: `unhex(X)` e `unhex(X, Y)`, do texto hexadecimal para blob. Com `Y`, os
/// caracteres de `Y` podem aparecer entre pares de dígitos.
fn unhex_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    let z_hex = text_of(&argv[0]);
    let z_pass: Option<Cow<'_, [u8]>> =
        if argc == 2 { text_of(&argv[1]) } else { Some(Cow::Borrowed(&[][..])) };
    let (Some(z_hex), Some(z_pass)) = (z_hex, z_pass) else { return };
    let n_hex = z_hex.len();

    if !context_malloc_ok(ctx, (n_hex / 2 + 1) as i64) {
        // O C segue para `unhex_done` com o ponteiro nulo.
        result_blob(ctx, None, 0, StrDtor::Dynamic);
        return;
    }
    let mut out: Vec<u8> = Vec::with_capacity(n_hex / 2 + 1);
    let mut zh = 0usize;
    'outer: loop {
        let mut c = at(&z_hex, zh);
        if c == 0 {
            break;
        }
        while !is_xdigit(c) {
            let ch = utf8_read(&z_hex, &mut zh);
            if !str_contains_char(&z_pass, ch) {
                return; // unhex_null
            }
            c = at(&z_hex, zh);
            if c == 0 {
                break 'outer;
            }
        }
        zh += 1;
        let d = at(&z_hex, zh);
        zh += 1;
        if !is_xdigit(d) {
            return; // unhex_null
        }
        out.push((hex_to_int(c as i32) << 4) | hex_to_int(d as i32));
    }
    let n = out.len() as i32;
    result_blob(ctx, Some(&out), n, StrDtor::Dynamic);
}

/// `zeroblobFunc`: um blob de `N` zeros.
fn zeroblob_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let mut n = value_int64(&argv[0]);
    if n < 0 {
        n = 0;
    }
    let rc = result_zeroblob64(ctx, n as u64);
    if rc != 0 {
        result_error_code(ctx, rc);
    }
}

/// `replaceFunc`: troca toda ocorrência exata de B por C em A; sem colações.
fn replace_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 3);
    let Some(z_str) = text_of(&argv[0]) else { return };
    let n_str = z_str.len() as i32;
    let Some(z_pattern) = text_of(&argv[1]) else { return };
    if at(&z_pattern, 0) == 0 {
        result_text(ctx, Some(&z_str), n_str, StrDtor::Transient);
        return;
    }
    let n_pattern = z_pattern.len() as i32;
    let Some(z_rep) = text_of(&argv[2]) else { return };
    let n_rep = z_rep.len() as i32;
    let mut n_out: i64 = n_str as i64 + 1;
    if !context_malloc_ok(ctx, n_out) {
        return;
    }
    let limit = ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as i64;
    let loop_limit = n_str - n_pattern;
    let mut z_out: Vec<u8> = Vec::with_capacity(n_out as usize);
    let mut i: i32 = 0;
    while i <= loop_limit {
        let iu = i as usize;
        if z_str[iu] != z_pattern[0] || z_str[iu..iu + n_pattern as usize] != z_pattern[..] {
            z_out.push(z_str[iu]);
        } else {
            if n_rep > n_pattern {
                n_out += (n_rep - n_pattern) as i64;
                if n_out - 1 > limit {
                    result_error_toobig(ctx);
                    return;
                }
            }
            z_out.extend_from_slice(&z_rep);
            i += n_pattern - 1;
        }
        i += 1;
    }
    z_out.extend_from_slice(&z_str[i as usize..]);
    let j = z_out.len() as i32;
    result_text(ctx, Some(&z_out), j, StrDtor::Dynamic);
}

/// `trimFunc`: `trim()`, `ltrim()` e `rtrim()`. O dado do usuário é 1 (esquerda), 2 (direita) ou
/// 3 (os dois lados).
fn trim_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    if value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    let Some(z_in) = text_of(&argv[0]) else { return };
    let mut n_in = z_in.len() as u32;
    let mut start = 0usize;
    let z_char_set: Cow<'_, [u8]> = if argc == 1 {
        Cow::Borrowed(b" ")
    } else {
        match text_of(&argv[1]) {
            Some(c) => c,
            None => return,
        }
    };
    // Cada caractere do conjunto: (início, tamanho em bytes).
    let mut chars: Vec<(usize, usize)> = Vec::new();
    let mut z = 0usize;
    while at(&z_char_set, z) != 0 {
        let s = z;
        skip_utf8(&z_char_set, &mut z);
        chars.push((s, z - s));
    }
    let n_char = chars.len();
    if argc != 1 && n_char > 0 && !context_malloc_ok(ctx, n_char as i64 * 12) {
        return;
    }
    if n_char > 0 {
        let flags = user_int(ctx);
        if flags & 1 != 0 {
            while n_in > 0 {
                let mut len = 0u32;
                let mut found = false;
                for &(s, l) in &chars {
                    len = l as u32;
                    if len <= n_in && z_in[start..start + l] == z_char_set[s..s + l] {
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
                start += len as usize;
                n_in -= len;
            }
        }
        if flags & 2 != 0 {
            while n_in > 0 {
                let mut len = 0u32;
                let mut found = false;
                for &(s, l) in &chars {
                    len = l as u32;
                    let from = start + (n_in as usize).wrapping_sub(l);
                    if len <= n_in && z_in[from..from + l] == z_char_set[s..s + l] {
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
                n_in -= len;
            }
        }
    }
    let end = start + n_in as usize;
    result_text(ctx, Some(&z_in[start..end]), n_in as i32, StrDtor::Transient);
}

/// `concatFuncCore`: o texto concatenado de todos os argumentos não nulos, com `z_sep` entre eles.
fn concat_func_core(ctx: &mut Context<'_>, argv: &[Mem], z_sep: &[u8]) {
    let n_sep = z_sep.len();
    let mut z: Vec<u8> = Vec::new();
    for a in argv {
        let k = bytes_of(a);
        if k > 0 {
            if let Some(v) = text_of(a) {
                if !z.is_empty() && n_sep > 0 {
                    z.extend_from_slice(z_sep);
                }
                z.extend_from_slice(&v[..(k as usize).min(v.len())]);
            }
        }
    }
    let j = z.len() as u64;
    result_text64(ctx, Some(&z), j, StrDtor::Dynamic, SQLITE_UTF8 as u8);
}

// ---------------------------------------------------------------------------------------------
// chunk 004
// ---------------------------------------------------------------------------------------------

/// `concatFunc`: `concat(...)`.
fn concat_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    concat_func_core(ctx, argv, b"");
}

/// `concatwsFunc`: `concat_ws(SEP, ...)`.
fn concatws_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let Some(z_sep) = text_of(&argv[0]) else { return };
    concat_func_core(ctx, &argv[1..], &z_sep);
}

/// `soundexFunc`: a codificação soundex de uma palavra.
fn soundex_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    const I_CODE: [u8; 128] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 1, 2, 3, 0, 1, 2, 0, 0, 2, 2, 4, 5, 5, 0,
        1, 2, 6, 2, 3, 0, 1, 0, 2, 0, 2, 0, 0, 0, 0, 0,
        0, 0, 1, 2, 3, 0, 1, 2, 0, 0, 2, 2, 4, 5, 5, 0,
        1, 2, 6, 2, 3, 0, 1, 0, 2, 0, 2, 0, 0, 0, 0, 0,
    ];
    debug_assert!(argv.len() == 1);
    let z_in_c = text_of(&argv[0]);
    let z_in: &[u8] = z_in_c.as_deref().unwrap_or(&[]);
    let mut i = 0usize;
    while at(z_in, i) != 0 && !is_alpha(at(z_in, i)) {
        i += 1;
    }
    if at(z_in, i) != 0 {
        let mut prevcode = I_CODE[(at(z_in, i) & 0x7f) as usize];
        let mut z_result = [0u8; 4];
        z_result[0] = to_upper(at(z_in, i));
        let mut j = 1usize;
        while j < 4 && at(z_in, i) != 0 {
            let code = I_CODE[(at(z_in, i) & 0x7f) as usize];
            if code > 0 {
                if code != prevcode {
                    prevcode = code;
                    z_result[j] = code + b'0';
                    j += 1;
                }
            } else {
                prevcode = 0;
            }
            i += 1;
        }
        while j < 4 {
            z_result[j] = b'0';
            j += 1;
        }
        result_text(ctx, Some(&z_result), 4, StrDtor::Transient);
    } else {
        // A cadeia "?000" volta se o argumento é NULL ou não tem letra ASCII.
        result_text(ctx, Some(b"?000"), 4, StrDtor::Static);
    }
}

/// `loadExt`: `load_extension()`, carrega uma biblioteca e devolve NULL.
fn load_ext(ctx: &mut Context<'_>, argv: &[Mem]) {
    let z_file = text_of(&argv[0]);
    // A função só vale se o `sqlite3_enable_load_extension()` ligou `SQLITE_LoadExtFunc`.
    if ctx.db.flags & SQLITE_LOAD_EXT_FUNC == 0 {
        result_error(ctx, b"not authorized", -1);
        return;
    }
    let z_proc = if argv.len() == 2 { text_of(&argv[1]) } else { None };
    if let Some(f) = z_file {
        let mut z_err_msg: Option<Vec<u8>> = None;
        let rc = crate::loadext::load_extension(
            &mut *ctx.db,
            cstr(&f),
            z_proc.as_deref().map(cstr),
            &mut z_err_msg,
        );
        if rc != 0 {
            let msg = z_err_msg.unwrap_or_default();
            result_error(ctx, &msg, -1);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// chunk 004 (cont.): sum, avg, total
// ---------------------------------------------------------------------------------------------

/// `SumCtx`: o contexto de `sum()`, `avg()` e `total()`.
#[derive(Default, Clone, Copy)]
struct SumCtx {
    /// Soma corrente como double.
    r_sum: f64,
    /// Termo de erro da soma de Kahan-Babuska-Neumaier.
    r_err: f64,
    /// Soma corrente como inteiro com sinal.
    i_sum: i64,
    /// Número de elementos somados.
    cnt: i64,
    /// Verdadeiro se algum valor não inteiro entrou na soma.
    approx: bool,
    /// Houve estouro de inteiro.
    ovrfl: bool,
}

/// `kahanBabuskaNeumaierStep`: um passo da soma de Kahan-Babuska-Neumaier.
fn kahan_babuska_neumaier_step(p_sum: &mut SumCtx, r: f64) {
    let s = p_sum.r_sum;
    let t = s + r;
    if s.abs() > r.abs() {
        p_sum.r_err += (s - t) + r;
    } else {
        p_sum.r_err += (r - t) + s;
    }
    p_sum.r_sum = t;
}

/// `kahanBabuskaNeumaierStepInt64`: soma um inteiro (possivelmente grande) à soma corrente.
fn kahan_babuska_neumaier_step_int64(p_sum: &mut SumCtx, i_val: i64) {
    if i_val <= -4503599627370496 || i_val >= 4503599627370496 {
        let i_sm = i_val % 16384;
        let i_big = i_val - i_sm;
        kahan_babuska_neumaier_step(p_sum, i_big as f64);
        kahan_babuska_neumaier_step(p_sum, i_sm as f64);
    } else {
        kahan_babuska_neumaier_step(p_sum, i_val as f64);
    }
}

/// `kahanBabuskaNeumaierInit`: inicia a soma a partir de um inteiro de 64 bits.
fn kahan_babuska_neumaier_init(p: &mut SumCtx, i_val: i64) {
    if i_val <= -4503599627370496 || i_val >= 4503599627370496 {
        let i_sm = i_val % 16384;
        p.r_sum = (i_val - i_sm) as f64;
        p.r_err = i_sm as f64;
    } else {
        p.r_sum = i_val as f64;
        p.r_err = 0.0;
    }
}

/// `sumStep`: passo de `sum()`, `avg()` e `total()`.
fn sum_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let (ty, arg) = numeric_arg(&argv[0]);
    let Some(p) = aggregate_context::<SumCtx>(ctx, true) else { return };
    if ty != SQLITE_NULL {
        p.cnt += 1;
        if !p.approx {
            if ty != SQLITE_INTEGER {
                let i_sum = p.i_sum;
                kahan_babuska_neumaier_init(p, i_sum);
                p.approx = true;
                kahan_babuska_neumaier_step(p, value_double(&arg));
            } else {
                let mut x = p.i_sum;
                if add_int64(&mut x, value_int64(&arg)) == 0 {
                    p.i_sum = x;
                } else {
                    p.ovrfl = true;
                    let i_sum = p.i_sum;
                    kahan_babuska_neumaier_init(p, i_sum);
                    p.approx = true;
                    kahan_babuska_neumaier_step_int64(p, value_int64(&arg));
                }
            }
        } else if ty == SQLITE_INTEGER {
            kahan_babuska_neumaier_step_int64(p, value_int64(&arg));
        } else {
            p.ovrfl = false;
            kahan_babuska_neumaier_step(p, value_double(&arg));
        }
    }
}

/// `sumInverse`: passo inverso (funções de janela).
fn sum_inverse(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let (ty, arg) = numeric_arg(&argv[0]);
    // `p` nunca é nulo: o `sumStep()` já rodou para inicializá-lo.
    let Some(p) = aggregate_context::<SumCtx>(ctx, true) else { return };
    if ty != SQLITE_NULL {
        debug_assert!(p.cnt > 0);
        p.cnt -= 1;
        if !p.approx {
            p.i_sum = p.i_sum.wrapping_sub(value_int64(&arg));
        } else if ty == SQLITE_INTEGER {
            let i_val = value_int64(&arg);
            if i_val != SMALLEST_INT64 {
                kahan_babuska_neumaier_step_int64(p, -i_val);
            } else {
                kahan_babuska_neumaier_step_int64(p, LARGEST_INT64);
                kahan_babuska_neumaier_step_int64(p, 1);
            }
        } else {
            kahan_babuska_neumaier_step(p, -value_double(&arg));
        }
    }
}

/// `sumFinalize`.
fn sum_finalize(ctx: &mut Context<'_>) {
    let p = aggregate_context::<SumCtx>(ctx, false).map(|p| *p);
    if let Some(p) = p {
        if p.cnt > 0 {
            if p.approx {
                if p.ovrfl {
                    result_error(ctx, b"integer overflow", -1);
                } else if !is_overflow(p.r_err) {
                    result_double(ctx, p.r_sum + p.r_err);
                } else {
                    result_double(ctx, p.r_sum);
                }
            } else {
                result_int64(ctx, p.i_sum);
            }
        }
    }
}

/// `avgFinalize`.
fn avg_finalize(ctx: &mut Context<'_>) {
    let p = aggregate_context::<SumCtx>(ctx, false).map(|p| *p);
    if let Some(p) = p {
        if p.cnt > 0 {
            let r = if p.approx {
                let mut r = p.r_sum;
                if !is_overflow(p.r_err) {
                    r += p.r_err;
                }
                r
            } else {
                p.i_sum as f64
            };
            result_double(ctx, r / p.cnt as f64);
        }
    }
}

/// `totalFinalize`.
fn total_finalize(ctx: &mut Context<'_>) {
    let mut r = 0.0f64;
    let p = aggregate_context::<SumCtx>(ctx, false).map(|p| *p);
    if let Some(p) = p {
        if p.approx {
            r = p.r_sum;
            if !is_overflow(p.r_err) {
                r += p.r_err;
            }
        } else {
            r = p.i_sum as f64;
        }
    }
    result_double(ctx, r);
}

/// `CountCtx`: o estado de `count()`.
#[derive(Default)]
struct CountCtx {
    n: i64,
}

// ---------------------------------------------------------------------------------------------
// chunk 005
// ---------------------------------------------------------------------------------------------

/// `countStep`.
fn count_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    let counts = argv.is_empty() || SQLITE_NULL != value_type(&argv[0]);
    if let Some(p) = aggregate_context::<CountCtx>(ctx, true) {
        if counts {
            p.n += 1;
        }
    }
}

/// `countFinalize`.
fn count_finalize(ctx: &mut Context<'_>) {
    let n = aggregate_context::<CountCtx>(ctx, false).map_or(0, |p| p.n);
    result_int64(ctx, n);
}

/// `countInverse`.
fn count_inverse(ctx: &mut Context<'_>, argv: &[Mem]) {
    let counts = argv.is_empty() || SQLITE_NULL != value_type(&argv[0]);
    // `p` nunca é nulo: o `countStep()` já rodou.
    if let Some(p) = aggregate_context::<CountCtx>(ctx, true) {
        if counts {
            p.n -= 1;
        }
    }
}

/// `minmaxStep`: passo de `min()` e `max()` agregados. O acumulador é uma `Mem`; sem `flags` é
/// "ainda vazio".
fn minmax_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    let arg = &argv[0];
    // `max()` tem `sqlite3_user_data()` diferente de zero; `min()` tem zero.
    let max = user_int(ctx) != 0;
    let have = match aggregate_context::<Mem>(ctx, true) {
        Some(p_best) => p_best.flags != 0,
        None => return,
    };
    if value_type(arg) == SQLITE_NULL {
        if have {
            skip_accumulator_load(ctx);
        }
    } else if have {
        let coll = get_func_coll_seq(ctx);
        let replaced = match aggregate_context::<Mem>(ctx, true) {
            Some(p_best) => {
                let cmp = mem_compare(p_best, arg, coll.as_deref());
                if (max && cmp < 0) || (!max && cmp > 0) {
                    mem_copy(p_best, arg);
                    true
                } else {
                    false
                }
            }
            None => return,
        };
        if !replaced {
            skip_accumulator_load(ctx);
        }
    } else if let Some(p_best) = aggregate_context::<Mem>(ctx, true) {
        mem_copy(p_best, arg);
    }
}

/// `minMaxValueFinalize`: com `b_value` falso é o finalizador (libera o acumulador); com `b_value`
/// verdadeiro é o `xValue` da janela (mantém).
fn min_max_value_finalize(ctx: &mut Context<'_>, b_value: bool) {
    let Some(boxed) = ctx.agg.take() else { return };
    match boxed.downcast::<Mem>() {
        Ok(mut p_res) => {
            if p_res.flags != 0 {
                result_value(ctx, &p_res);
            }
            if !b_value {
                mem_release(&mut p_res);
            } else {
                ctx.agg = Some(p_res);
            }
        }
        Err(other) => ctx.agg = Some(other),
    }
}

/// `minMaxValue`.
fn min_max_value(ctx: &mut Context<'_>) {
    min_max_value_finalize(ctx, true);
}

/// `minMaxFinalize`.
fn min_max_finalize(ctx: &mut Context<'_>) {
    min_max_value_finalize(ctx, false);
}

/// `GroupConcatCtx`: o estado de `group_concat()` e `string_agg()`. O separador vem ANTES do
/// texto (é trágico, mas o comportamento antigo não se muda).
struct GroupConcatCtx {
    /// A concatenação acumulada.
    str_: StrAccum,
    /// Número de textos concatenados no momento.
    n_accum: i32,
    /// Tamanho do primeiro separador, para detectar variação.
    n_first_sep_length: i32,
    /// Se existe, os tamanhos dos separadores entre os textos, como incorporados ao resultado
    /// (as vagas em uso são `n_accum - 1` entre chamadas); se não, `n_first_sep_length` vale
    /// para todos.
    pn_sep_lengths: Option<Vec<i32>>,
}

impl Default for GroupConcatCtx {
    /// O estado zerado do C: acumulador sem texto e com `mxAlloc` 0.
    fn default() -> Self {
        GroupConcatCtx {
            str_: StrAccum::new(0),
            n_accum: 0,
            n_first_sep_length: 0,
            pn_sep_lengths: None,
        }
    }
}

/// `groupConcatStep`.
fn group_concat_step(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    if value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    let limit = ctx.db.a_limit[SQLITE_LIMIT_LENGTH as usize] as u32;
    let Some(p_gcc) = aggregate_context::<GroupConcatCtx>(ctx, true) else { return };
    let first_term = p_gcc.str_.mx_alloc == 0;
    p_gcc.str_.mx_alloc = limit;
    if argc == 1 {
        if !first_term {
            p_gcc.str_.append_char(1, b',');
        } else {
            p_gcc.n_first_sep_length = 1;
        }
    } else if !first_term {
        let z_sep = text_of(&argv[1]);
        let mut n_sep = bytes_of(&argv[1]);
        if let Some(z) = &z_sep {
            p_gcc.str_.append(&z[..(n_sep.max(0) as usize).min(z.len())]);
        } else {
            n_sep = 0;
        }
        if n_sep != p_gcc.n_first_sep_length || p_gcc.pn_sep_lengths.is_some() {
            let n_accum = p_gcc.n_accum;
            let mut pnsl = match p_gcc.pn_sep_lengths.take() {
                None => {
                    // Primeira variação do tamanho do separador: começa a registrá-los.
                    let mut v = vec![0i32; (n_accum + 1).max(0) as usize];
                    let n_a = (n_accum - 1).max(0) as usize;
                    for slot in v.iter_mut().take(n_a) {
                        *slot = p_gcc.n_first_sep_length;
                    }
                    v
                }
                Some(mut v) => {
                    v.resize(n_accum.max(0) as usize, 0);
                    v
                }
            };
            if n_accum > 0 {
                pnsl[(n_accum - 1) as usize] = n_sep;
            }
            p_gcc.pn_sep_lengths = Some(pnsl);
        }
    } else {
        p_gcc.n_first_sep_length = bytes_of(&argv[1]);
    }
    p_gcc.n_accum += 1;
    let z_val = text_of(&argv[0]);
    let n_val = bytes_of(&argv[0]);
    if let Some(z) = z_val {
        p_gcc.str_.append(&z[..(n_val.max(0) as usize).min(z.len())]);
    }
}

/// `groupConcatInverse`: passo inverso (funções de janela).
fn group_concat_inverse(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1 || argv.len() == 2);
    if value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    // `p_gcc` nunca é nulo: o `groupConcatStep()` sempre rodou antes.
    let Some(p_gcc) = aggregate_context::<GroupConcatCtx>(ctx, true) else { return };
    let mut n_vs = bytes_of(&argv[0]);
    p_gcc.n_accum -= 1;
    if let Some(pnsl) = p_gcc.pn_sep_lengths.as_mut() {
        debug_assert!(p_gcc.n_accum >= 0);
        if p_gcc.n_accum > 0 {
            n_vs += pnsl[0];
            let n = p_gcc.n_accum as usize;
            pnsl.copy_within(1..n, 0);
        }
    } else {
        // Se remove o único texto acumulado, exagera sem dano.
        n_vs += p_gcc.n_first_sep_length;
    }
    let n_char = p_gcc.str_.n_char;
    p_gcc.str_.remove_prefix((n_vs.max(0) as u32).min(n_char));
    if p_gcc.str_.n_char == 0 {
        p_gcc.str_.mx_alloc = 0;
        p_gcc.pn_sep_lengths = None;
    }
}

/// `groupConcatFinalize`.
fn group_concat_finalize(ctx: &mut Context<'_>) {
    let Some(boxed) = ctx.agg.take() else { return };
    if let Ok(mut p_gcc) = boxed.downcast::<GroupConcatCtx>() {
        crate::printf::result_str_accum(ctx, &mut p_gcc.str_);
    }
}

/// `groupConcatValue`: o valor corrente da janela.
fn group_concat_value(ctx: &mut Context<'_>) {
    enum Value {
        TooBig,
        NoMem,
        Empty,
        Text(Option<Vec<u8>>, i32),
    }
    let v = match aggregate_context::<GroupConcatCtx>(ctx, false) {
        None => return,
        Some(g) => {
            if g.str_.acc_error as i32 == SQLITE_TOOBIG {
                Value::TooBig
            } else if g.str_.acc_error as i32 == SQLITE_NOMEM {
                Value::NoMem
            } else if g.n_accum > 0 && g.str_.n_char == 0 {
                Value::Empty
            } else {
                Value::Text(g.str_.value().map(|s| s.to_vec()), g.str_.n_char as i32)
            }
        }
    };
    match v {
        Value::TooBig => result_error_toobig(ctx),
        Value::NoMem => result_error_nomem(ctx),
        // O C passa `""` com `n = 1`: o texto tem um byte zero.
        Value::Empty => result_text(ctx, Some(b"\0"), 1, StrDtor::Static),
        Value::Text(z, n) => result_text(ctx, z.as_deref(), n, StrDtor::Transient),
    }
}

/// `sqlite3RegisterPerConnectionBuiltinFunctions`: as funções embutidas que não são globais; só
/// sobrecarrega `MATCH` com dois argumentos.
pub fn register_per_connection_builtin_functions(db: &mut Connection) {
    let rc = crate::main::overload_function(db, b"MATCH", 2);
    debug_assert!(rc == SQLITE_NOMEM || rc == crate::consts::SQLITE_OK);
    if rc == SQLITE_NOMEM {
        oom_fault(db);
    }
}

/// `sqlite3RegisterLikeFunctions`: registra de novo o `like()` embutido. `case_sensitive` diz se o
/// operador LIKE diferencia maiúsculas.
pub fn register_like_functions(db: &mut Connection, case_sensitive: i32) {
    let (info, flags) = if case_sensitive != 0 {
        (LIKE_INFO_ALT, SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE)
    } else {
        (LIKE_INFO_NORM, SQLITE_FUNC_LIKE)
    };
    for n_arg in 2..=3i32 {
        crate::main::create_func(
            db,
            b"like",
            n_arg,
            SQLITE_UTF8,
            UserData::Ptr(Rc::new(info)),
            Some(like_func),
            None,
            None,
            None,
            None,
            None,
        );
        // `sqlite3FindFunction` e a troca das flags: o `FuncDef` é imutável depois de
        // compartilhado, então a entrada da cadeia é substituída por uma cópia com as flags novas.
        if let Some(chain) = hash_find_mut(&mut db.a_func, b"like") {
            for slot in chain.iter_mut() {
                if slot.n_arg as i32 == n_arg
                    && (slot.func_flags & SQLITE_FUNC_ENCMASK) == SQLITE_UTF8 as u32
                {
                    let new = FuncDef {
                        n_arg: slot.n_arg,
                        func_flags: (slot.func_flags | flags) & !SQLITE_FUNC_UNSAFE,
                        p_user_data: slot.p_user_data.clone(),
                        x_s_func: slot.x_s_func,
                        x_finalize: slot.x_finalize,
                        x_value: slot.x_value,
                        x_inverse: slot.x_inverse,
                        z_name: slot.z_name.clone(),
                        p_destructor: slot.p_destructor.clone(),
                    };
                    *slot = Rc::new(new);
                    break;
                }
            }
        }
    }
}

/// `sqlite3IsLikeFunction`: `p_expr` é uma expressão de função. Se a otimização do LIKE se aplica,
/// grava os curingas e o escape em `a_wc[0..4]` (o escape é 0 sem ESCAPE) e devolve verdadeiro. O
/// ESCAPE só vale se é um literal de texto de um byte. `p_is_nocase` fica verdadeiro se maiúsculas
/// e minúsculas são equivalentes (o padrão do LIKE; falso no GLOB).
///
/// Recebe `&mut Connection` porque `find_function` o pede (sem `create_flag` não altera nada).
pub fn is_like_function(
    db: &mut Connection,
    p_expr: &Expr,
    p_is_nocase: &mut bool,
    a_wc: &mut [u8; 4],
) -> bool {
    debug_assert!(p_expr.op == TK_FUNCTION);
    let Some(list) = p_expr.x_list() else { return false };
    let n_expr = list.a.len();
    debug_assert!(!p_expr.has_property(crate::consts::EP_INT_VALUE));
    let z_token = p_expr.z_token().unwrap_or(&[]);
    let Some(p_def) = find_function(db, z_token, n_expr as i32, SQLITE_UTF8 as u8, 0) else {
        return false;
    };
    if (p_def.func_flags & SQLITE_FUNC_LIKE) == 0 {
        return false;
    }
    // Os três primeiros campos da `compareInfo` são os curingas.
    match &p_def.p_user_data {
        UserData::Ptr(p) => match p.downcast_ref::<CompareInfo>() {
            Some(ci) => {
                a_wc[0] = ci.match_all;
                a_wc[1] = ci.match_one;
                a_wc[2] = ci.match_set;
            }
            None => return false,
        },
        _ => return false,
    }
    if n_expr < 3 {
        a_wc[3] = 0;
    } else {
        let Some(p_escape) = list.a[2].p_expr.as_deref() else { return false };
        if p_escape.op != TK_STRING {
            return false;
        }
        debug_assert!(!p_escape.has_property(crate::consts::EP_INT_VALUE));
        let z_escape = p_escape.z_token().unwrap_or(&[]);
        if at(z_escape, 0) == 0 || at(z_escape, 1) != 0 {
            return false;
        }
        if z_escape[0] == a_wc[0] {
            return false;
        }
        if z_escape[0] == a_wc[1] {
            return false;
        }
        a_wc[3] = z_escape[0];
    }
    *p_is_nocase = (p_def.func_flags & SQLITE_FUNC_CASE) == 0;
    true
}

// ---------------------------------------------------------------------------------------------
// chunk 006: funções matemáticas, sign e o registro
// ---------------------------------------------------------------------------------------------

/// O `pUserData` de uma função matemática de um argumento (`double (*)(double)`).
#[derive(Clone, Copy)]
struct Math1(fn(f64) -> f64);

/// O `pUserData` de uma função matemática de dois argumentos (`double (*)(double,double)`).
#[derive(Clone, Copy)]
struct Math2(fn(f64, f64) -> f64);

/// `M_PI`.
const M_PI: f64 = 3.141592653589793238462643383279502884;

/// O ponteiro de função de uma função de um argumento.
fn math1_of(ctx: &Context<'_>) -> Option<fn(f64) -> f64> {
    match &ctx.arg_func.p_user_data {
        UserData::Ptr(p) => p.downcast_ref::<Math1>().map(|m| m.0),
        _ => None,
    }
}

/// O ponteiro de função de uma função de dois argumentos.
fn math2_of(ctx: &Context<'_>) -> Option<fn(f64, f64) -> f64> {
    match &ctx.arg_func.p_user_data {
        UserData::Ptr(p) => p.downcast_ref::<Math2>().map(|m| m.0),
        _ => None,
    }
}

/// `ceilingFunc`: `ceil(X)`, `ceiling(X)`, `floor(X)` e `trunc(X)`.
fn ceiling_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let (ty, arg) = numeric_arg(&argv[0]);
    match ty {
        SQLITE_INTEGER => result_int64(ctx, value_int64(&arg)),
        SQLITE_FLOAT => {
            if let Some(x) = math1_of(ctx) {
                result_double(ctx, x(value_double(&arg)));
            }
        }
        _ => {}
    }
}

/// `xCeil`.
fn x_ceil(x: f64) -> f64 {
    x.ceil()
}

/// `xFloor`.
fn x_floor(x: f64) -> f64 {
    x.floor()
}

/// `logFunc`: `ln(X)`, `log(X)`, `log10(X)`, `log2(X)` e `log(B,X)`.
fn log_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    let (t0, a0) = numeric_arg(&argv[0]);
    let mut x: f64;
    match t0 {
        SQLITE_INTEGER | SQLITE_FLOAT => {
            x = value_double(&a0);
            if x <= 0.0 {
                return;
            }
        }
        _ => return,
    }
    let ans: f64;
    if argc == 2 {
        // O C testa de novo o tipo de `argv[0]`.
        match t0 {
            SQLITE_INTEGER | SQLITE_FLOAT => {
                let b = x.ln();
                if b <= 0.0 {
                    return;
                }
                x = value_double(&argv[1]);
                if x <= 0.0 {
                    return;
                }
                ans = x.ln() / b;
            }
            _ => return,
        }
    } else {
        ans = match user_int(ctx) {
            1 => x.log10(),
            2 => x.log2(),
            _ => x.ln(),
        };
    }
    result_double(ctx, ans);
}

/// `degToRad`.
fn deg_to_rad(x: f64) -> f64 {
    x * (M_PI / 180.0)
}

/// `radToDeg`.
fn rad_to_deg(x: f64) -> f64 {
    x * (180.0 / M_PI)
}

/// `math1Func`: as funções matemáticas de um argumento (`exp`, `sin`, ...).
fn math1_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let (t0, a0) = numeric_arg(&argv[0]);
    if t0 != SQLITE_INTEGER && t0 != SQLITE_FLOAT {
        return;
    }
    let v0 = value_double(&a0);
    if let Some(x) = math1_of(ctx) {
        result_double(ctx, x(v0));
    }
}

/// `math2Func`: as funções matemáticas de dois argumentos (`pow`, `atan2`, ...).
fn math2_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 2);
    let (t0, a0) = numeric_arg(&argv[0]);
    if t0 != SQLITE_INTEGER && t0 != SQLITE_FLOAT {
        return;
    }
    let (t1, a1) = numeric_arg(&argv[1]);
    if t1 != SQLITE_INTEGER && t1 != SQLITE_FLOAT {
        return;
    }
    let v0 = value_double(&a0);
    let v1 = value_double(&a1);
    if let Some(x) = math2_of(ctx) {
        result_double(ctx, x(v0, v1));
    }
}

/// `piFunc`.
fn pi_func(ctx: &mut Context<'_>, _argv: &[Mem]) {
    result_double(ctx, M_PI);
}

/// `signFunc`.
fn sign_func(ctx: &mut Context<'_>, argv: &[Mem]) {
    debug_assert!(argv.len() == 1);
    let (t0, a0) = numeric_arg(&argv[0]);
    if t0 != SQLITE_INTEGER && t0 != SQLITE_FLOAT {
        return;
    }
    let x = value_double(&a0);
    result_int(ctx, if x < 0.0 { -1 } else if x > 0.0 { 1 } else { 0 });
}

/// Monta um `FuncDef` embutido (`SQLITE_FUNC_BUILTIN` sempre ligado).
#[allow(clippy::too_many_arguments)]
fn make_def(
    name: &str,
    n_arg: i32,
    flags: u32,
    user: UserData,
    s: Option<ScalarFn>,
    f: Option<FinalFn>,
    v: Option<FinalFn>,
    i: Option<ScalarFn>,
) -> FuncDef {
    FuncDef {
        n_arg: n_arg as i8,
        func_flags: SQLITE_FUNC_BUILTIN | flags,
        p_user_data: user,
        x_s_func: s,
        x_finalize: f,
        x_value: v,
        x_inverse: i,
        z_name: name.as_bytes().to_vec(),
        p_destructor: None,
    }
}

/// A flag `SQLITE_UTF8` como `u32`.
const UTF8_FLAG: u32 = SQLITE_UTF8 as u32;

/// `bNC*SQLITE_FUNC_NEEDCOLL`.
#[inline]
fn need_coll(b_nc: bool) -> u32 {
    if b_nc {
        SQLITE_FUNC_NEEDCOLL
    } else {
        0
    }
}

/// Macro `FUNCTION`.
fn function(name: &str, n_arg: i32, i_arg: isize, b_nc: bool, x: Option<ScalarFn>) -> FuncDef {
    function2(name, n_arg, i_arg, b_nc, x, 0)
}

/// Macro `FUNCTION2`.
fn function2(
    name: &str,
    n_arg: i32,
    i_arg: isize,
    b_nc: bool,
    x: Option<ScalarFn>,
    extra_flags: u32,
) -> FuncDef {
    make_def(
        name,
        n_arg,
        SQLITE_FUNC_CONSTANT | UTF8_FLAG | need_coll(b_nc) | extra_flags,
        UserData::Int(i_arg),
        x,
        None,
        None,
        None,
    )
}

/// Macro `VFUNCTION`: função não constante (o resultado varia).
fn vfunction(name: &str, n_arg: i32, i_arg: isize, b_nc: bool, x: Option<ScalarFn>) -> FuncDef {
    make_def(name, n_arg, UTF8_FLAG | need_coll(b_nc), UserData::Int(i_arg), x, None, None, None)
}

/// Macro `SFUNCTION`: só em SQL de primeiro nível e não confiável.
fn sfunction(name: &str, n_arg: i32, i_arg: isize, x: Option<ScalarFn>) -> FuncDef {
    make_def(
        name,
        n_arg,
        UTF8_FLAG | SQLITE_FUNC_DIRECT | SQLITE_FUNC_UNSAFE,
        UserData::Int(i_arg),
        x,
        None,
        None,
        None,
    )
}

/// Macro `DFUNCTION`: muda devagar durante a execução.
fn dfunction(name: &str, n_arg: i32, x: Option<ScalarFn>) -> FuncDef {
    make_def(name, n_arg, SQLITE_FUNC_SLOCHNG | UTF8_FLAG, UserData::None, x, None, None, None)
}

/// Macro `MFUNCTION` com função de um argumento.
fn mfunction1(name: &str, n_arg: i32, p: fn(f64) -> f64, x: ScalarFn) -> FuncDef {
    make_def(
        name,
        n_arg,
        SQLITE_FUNC_CONSTANT | UTF8_FLAG,
        UserData::Ptr(Rc::new(Math1(p))),
        Some(x),
        None,
        None,
        None,
    )
}

/// Macro `MFUNCTION` com função de dois argumentos.
fn mfunction2(name: &str, n_arg: i32, p: fn(f64, f64) -> f64, x: ScalarFn) -> FuncDef {
    make_def(
        name,
        n_arg,
        SQLITE_FUNC_CONSTANT | UTF8_FLAG,
        UserData::Ptr(Rc::new(Math2(p))),
        Some(x),
        None,
        None,
        None,
    )
}

/// Macro `INLINE_FUNC`: implementada como código do VDBE; o `xSFunc` nunca é chamado.
fn inline_func(name: &str, n_arg: i32, i_arg: i32, m_flags: u32) -> FuncDef {
    make_def(
        name,
        n_arg,
        UTF8_FLAG | SQLITE_FUNC_INLINE | SQLITE_FUNC_CONSTANT | m_flags,
        UserData::Int(i_arg as isize),
        Some(noop_func),
        None,
        None,
        None,
    )
}

/// Macro `TEST_FUNC`: só com `SQLITE_TESTCTRL_INTERNAL_FUNCTIONS`.
fn test_func(name: &str, n_arg: i32, i_arg: i32, m_flags: u32) -> FuncDef {
    make_def(
        name,
        n_arg,
        UTF8_FLAG
            | SQLITE_FUNC_INTERNAL
            | SQLITE_FUNC_TEST
            | SQLITE_FUNC_INLINE
            | SQLITE_FUNC_CONSTANT
            | m_flags,
        UserData::Int(i_arg as isize),
        Some(noop_func),
        None,
        None,
        None,
    )
}

/// Macro `LIKEFUNC`.
fn likefunc(name: &str, n_arg: i32, info: CompareInfo, flags: u32) -> FuncDef {
    make_def(
        name,
        n_arg,
        SQLITE_FUNC_CONSTANT | UTF8_FLAG | flags,
        UserData::Ptr(Rc::new(info)),
        Some(like_func),
        None,
        None,
        None,
    )
}

/// Macro `WAGGREGATE`.
#[allow(clippy::too_many_arguments)]
fn waggregate(
    name: &str,
    n_arg: i32,
    arg: isize,
    nc: bool,
    x_step: ScalarFn,
    x_final: FinalFn,
    x_value: FinalFn,
    x_inverse: Option<ScalarFn>,
    f: u32,
) -> FuncDef {
    make_def(
        name,
        n_arg,
        UTF8_FLAG | need_coll(nc) | f,
        UserData::Int(arg),
        Some(x_step),
        Some(x_final),
        Some(x_value),
        x_inverse,
    )
}

/// `sqlite3RegisterBuiltinFunctions`: acrescenta à tabela global de funções embutidas as funções
/// deste arquivo (na ordem de `aBuiltinFunc`), depois das de `alter.c`, `window.c`, `date.c` e
/// `json.c`, que se registram antes (a ordem decide a cadeia de cada balde, que o
/// `PRAGMA function_list` imprime).
pub fn register_builtin_functions() {
    // Para a máxima eficiência, a função mais usada vem por último.
    let mut v: Vec<FuncDef> = Vec::with_capacity(140);
    // Funções só disponíveis com SQLITE_TESTCTRL_INTERNAL_FUNCTIONS.
    v.push(test_func("implies_nonnull_row", 2, INLINEFUNC_IMPLIES_NONNULL_ROW, 0));
    v.push(test_func("expr_compare", 2, INLINEFUNC_EXPR_COMPARE, 0));
    v.push(test_func("expr_implies_expr", 2, INLINEFUNC_EXPR_IMPLIES_EXPR, 0));
    v.push(test_func("affinity", 1, INLINEFUNC_AFFINITY, 0));
    // Funções regulares.
    v.push(function("soundex", 1, 0, false, Some(soundex_func)));
    v.push(sfunction("load_extension", 1, 0, Some(load_ext)));
    v.push(sfunction("load_extension", 2, 0, Some(load_ext)));
    v.push(dfunction("sqlite_compileoption_used", 1, Some(compileoptionused_func)));
    v.push(dfunction("sqlite_compileoption_get", 1, Some(compileoptionget_func)));
    v.push(inline_func("unlikely", 1, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY));
    v.push(inline_func("likelihood", 2, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY));
    v.push(inline_func("likely", 1, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY));
    v.push(function("ltrim", 1, 1, false, Some(trim_func)));
    v.push(function("ltrim", 2, 1, false, Some(trim_func)));
    v.push(function("rtrim", 1, 2, false, Some(trim_func)));
    v.push(function("rtrim", 2, 2, false, Some(trim_func)));
    v.push(function("trim", 1, 3, false, Some(trim_func)));
    v.push(function("trim", 2, 3, false, Some(trim_func)));
    v.push(function("min", -1, 0, true, Some(minmax_func)));
    v.push(function("min", 0, 0, true, None));
    v.push(waggregate(
        "min", 1, 0, true, minmax_step, min_max_finalize, min_max_value, None,
        SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
    ));
    v.push(function("max", -1, 1, true, Some(minmax_func)));
    v.push(function("max", 0, 1, true, None));
    v.push(waggregate(
        "max", 1, 1, true, minmax_step, min_max_finalize, min_max_value, None,
        SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
    ));
    v.push(function2("typeof", 1, 0, false, Some(typeof_func), SQLITE_FUNC_TYPEOF));
    v.push(function2("subtype", 1, 0, false, Some(subtype_func), SQLITE_FUNC_TYPEOF));
    v.push(function2("length", 1, 0, false, Some(length_func), SQLITE_FUNC_LENGTH));
    v.push(function2("octet_length", 1, 0, false, Some(bytelength_func), SQLITE_FUNC_BYTELEN));
    v.push(function("instr", 2, 0, false, Some(instr_func)));
    v.push(function("printf", -1, 0, false, Some(printf_func)));
    v.push(function("format", -1, 0, false, Some(printf_func)));
    v.push(function("unicode", 1, 0, false, Some(unicode_func)));
    v.push(function("char", -1, 0, false, Some(char_func)));
    v.push(function("abs", 1, 0, false, Some(abs_func)));
    v.push(function("round", 1, 0, false, Some(round_func)));
    v.push(function("round", 2, 0, false, Some(round_func)));
    v.push(function("upper", 1, 0, false, Some(upper_func)));
    v.push(function("lower", 1, 0, false, Some(lower_func)));
    v.push(function("hex", 1, 0, false, Some(hex_func)));
    v.push(function("unhex", 1, 0, false, Some(unhex_func)));
    v.push(function("unhex", 2, 0, false, Some(unhex_func)));
    v.push(function("concat", -1, 0, false, Some(concat_func)));
    v.push(function("concat", 0, 0, false, None));
    v.push(function("concat_ws", -1, 0, false, Some(concatws_func)));
    v.push(function("concat_ws", 0, 0, false, None));
    v.push(function("concat_ws", 1, 0, false, None));
    v.push(inline_func("ifnull", 2, INLINEFUNC_COALESCE, 0));
    v.push(vfunction("random", 0, 0, false, Some(random_func)));
    v.push(vfunction("randomblob", 1, 0, false, Some(random_blob)));
    v.push(function("nullif", 2, 0, true, Some(nullif_func)));
    v.push(dfunction("sqlite_version", 0, Some(version_func)));
    v.push(dfunction("sqlite_source_id", 0, Some(sourceid_func)));
    v.push(function("sqlite_log", 2, 0, false, Some(errlog_func)));
    v.push(function("quote", 1, 0, false, Some(quote_func)));
    v.push(vfunction("last_insert_rowid", 0, 0, false, Some(last_insert_rowid)));
    v.push(vfunction("changes", 0, 0, false, Some(changes)));
    v.push(vfunction("total_changes", 0, 0, false, Some(total_changes)));
    v.push(function("replace", 3, 0, false, Some(replace_func)));
    v.push(function("zeroblob", 1, 0, false, Some(zeroblob_func)));
    v.push(function("substr", 2, 0, false, Some(substr_func)));
    v.push(function("substr", 3, 0, false, Some(substr_func)));
    v.push(function("substring", 2, 0, false, Some(substr_func)));
    v.push(function("substring", 3, 0, false, Some(substr_func)));
    v.push(waggregate("sum", 1, 0, false, sum_step, sum_finalize, sum_finalize, Some(sum_inverse), 0));
    v.push(waggregate(
        "total", 1, 0, false, sum_step, total_finalize, total_finalize, Some(sum_inverse), 0,
    ));
    v.push(waggregate("avg", 1, 0, false, sum_step, avg_finalize, avg_finalize, Some(sum_inverse), 0));
    v.push(waggregate(
        "count", 0, 0, false, count_step, count_finalize, count_finalize, Some(count_inverse),
        SQLITE_FUNC_COUNT | SQLITE_FUNC_ANYORDER,
    ));
    v.push(waggregate(
        "count", 1, 0, false, count_step, count_finalize, count_finalize, Some(count_inverse),
        SQLITE_FUNC_ANYORDER,
    ));
    v.push(waggregate(
        "group_concat", 1, 0, false, group_concat_step, group_concat_finalize,
        group_concat_value, Some(group_concat_inverse), 0,
    ));
    v.push(waggregate(
        "group_concat", 2, 0, false, group_concat_step, group_concat_finalize,
        group_concat_value, Some(group_concat_inverse), 0,
    ));
    v.push(waggregate(
        "string_agg", 2, 0, false, group_concat_step, group_concat_finalize,
        group_concat_value, Some(group_concat_inverse), 0,
    ));
    v.push(likefunc("glob", 2, GLOB_INFO, SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE));
    v.push(likefunc("like", 2, LIKE_INFO_NORM, SQLITE_FUNC_LIKE));
    v.push(likefunc("like", 3, LIKE_INFO_NORM, SQLITE_FUNC_LIKE));
    v.push(function("coalesce", 1, 0, false, None));
    v.push(function("coalesce", 0, 0, false, None));
    // Funções matemáticas (SQLITE_ENABLE_MATH_FUNCTIONS e SQLITE_HAVE_C99_MATH_FUNCS).
    v.push(mfunction1("ceil", 1, x_ceil, ceiling_func));
    v.push(mfunction1("ceiling", 1, x_ceil, ceiling_func));
    v.push(mfunction1("floor", 1, x_floor, ceiling_func));
    v.push(mfunction1("trunc", 1, f64::trunc, ceiling_func));
    v.push(function("ln", 1, 0, false, Some(log_func)));
    v.push(function("log", 1, 1, false, Some(log_func)));
    v.push(function("log10", 1, 1, false, Some(log_func)));
    v.push(function("log2", 1, 2, false, Some(log_func)));
    v.push(function("log", 2, 0, false, Some(log_func)));
    v.push(mfunction1("exp", 1, f64::exp, math1_func));
    v.push(mfunction2("pow", 2, f64::powf, math2_func));
    v.push(mfunction2("power", 2, f64::powf, math2_func));
    v.push(mfunction2("mod", 2, c_fmod, math2_func));
    v.push(mfunction1("acos", 1, f64::acos, math1_func));
    v.push(mfunction1("asin", 1, f64::asin, math1_func));
    v.push(mfunction1("atan", 1, f64::atan, math1_func));
    v.push(mfunction2("atan2", 2, f64::atan2, math2_func));
    v.push(mfunction1("cos", 1, f64::cos, math1_func));
    v.push(mfunction1("sin", 1, f64::sin, math1_func));
    v.push(mfunction1("tan", 1, f64::tan, math1_func));
    v.push(mfunction1("cosh", 1, f64::cosh, math1_func));
    v.push(mfunction1("sinh", 1, f64::sinh, math1_func));
    v.push(mfunction1("tanh", 1, f64::tanh, math1_func));
    v.push(mfunction1("acosh", 1, f64::acosh, math1_func));
    v.push(mfunction1("asinh", 1, f64::asinh, math1_func));
    v.push(mfunction1("atanh", 1, f64::atanh, math1_func));
    v.push(mfunction1("sqrt", 1, f64::sqrt, math1_func));
    v.push(mfunction1("radians", 1, deg_to_rad, math1_func));
    v.push(mfunction1("degrees", 1, rad_to_deg, math1_func));
    v.push(function("pi", 0, 0, false, Some(pi_func)));
    v.push(function("sign", 1, 0, false, Some(sign_func)));
    v.push(inline_func("coalesce", -1, INLINEFUNC_COALESCE, 0));
    v.push(inline_func("iif", 3, INLINEFUNC_IIF, 0));

    crate::alter::alter_functions();
    crate::window::window_functions();
    crate::date::register_date_time_functions();
    crate::json::register_json_functions();
    insert_builtin_funcs(v);
}

/// `fmod` do C: o resto com o sinal do dividendo, que é o `%` de `f64`.
fn c_fmod(x: f64, y: f64) -> f64 {
    x % y
}
