// Mesclado das partes traduzidas de func_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Retorna a sequência de colação associada a uma função.
fn get_func_coll_seq(context: &sqlite3_context) -> CollSeqRef {
    debug_assert!(context.p_vdbe.upgrade().is_some());
    let p_vdbe = context.p_vdbe.upgrade().unwrap();
    let p_vdbe_borrow = p_vdbe.borrow();
    let p_op = &p_vdbe_borrow.a_op[(context.i_op - 1) as usize];
    debug_assert_eq!(p_op.opcode, OP_COLLSEQ);
    debug_assert_eq!(p_op.p4_type, P4_COLLSEQ);
    let coll_seq = if let P4::PCollSeq(ref coll) = p_op.p4 {
        coll.clone()
    } else {
        unreachable!()
    };
    coll_seq
}

/// Indica que o carregamento do acumulador deve ser pulado nesta iteração
/// do laço de agregação.
fn skip_accumulator_load(context: &mut sqlite3_context) {
    debug_assert!(context.is_error <= 0);
    context.is_error = -1;
    context.skip_flag = 1;
}

/// Byte de `z` na posição `i`, ou 0 além do fim (o terminador NUL do C).
#[inline]
fn byte_at_or_nul(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Equivalente a SQLITE_SKIP_UTF8: avança `i` sobre um caractere UTF-8.
#[inline]
fn skip_utf8(z: &[u8], i: &mut usize) {
    let c = byte_at_or_nul(z, *i);
    *i += 1;
    if c >= 0xc0 {
        while (byte_at_or_nul(z, *i) & 0xc0) == 0x80 {
            *i += 1;
        }
    }
}

/// Implementação das funções não agregadas min() e max().
fn minmax_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert!(argc > 1);
    // 0 para min(), -1 (0xffffffff) para max().
    let mask: i32 = if api::user_data_int(context) == 0 { 0 } else { -1 };
    let p_coll = get_func_coll_seq(context);
    debug_assert!(mask == -1 || mask == 0);
    let mut i_best = 0usize;
    if api::value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    for i in 1..argc as usize {
        if api::value_type(&argv[i]) == SQLITE_NULL {
            return;
        }
        if (mem_compare(&argv[i_best], &argv[i], &p_coll) ^ mask) >= 0 {
            i_best = i;
        }
    }
    api::result_value(context, &argv[i_best]);
}

/// Retorna o tipo do argumento.
fn typeof_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    const AZ_TYPE: [&[u8]; 5] = [b"integer", b"real", b"text", b"blob", b"null"];
    let i = api::value_type(&argv[0]) - 1;
    debug_assert!(i >= 0 && (i as usize) < AZ_TYPE.len());
    debug_assert_eq!(SQLITE_INTEGER, 1);
    debug_assert_eq!(SQLITE_FLOAT, 2);
    debug_assert_eq!(SQLITE_TEXT, 3);
    debug_assert_eq!(SQLITE_BLOB, 4);
    debug_assert_eq!(SQLITE_NULL, 5);
    api::result_text(context, Some(AZ_TYPE[i as usize]), -1, SQLITE_STATIC);
}

/// Retorna o subtipo de X.
fn subtype_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    api::result_int(context, api::value_subtype(&argv[0]) as i32);
}

/// Implementação da função length().
fn length_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    match api::value_type(&argv[0]) {
        SQLITE_BLOB | SQLITE_INTEGER | SQLITE_FLOAT => {
            api::result_int(context, api::value_bytes(&argv[0]));
        }
        SQLITE_TEXT => {
            let z = match api::value_text(&argv[0]) {
                Some(z) => z,
                None => return,
            };
            // Conta caracteres até o primeiro NUL: bytes de continuação não contam.
            let mut n: i32 = 0;
            let mut i = 0usize;
            while byte_at_or_nul(&z, i) != 0 {
                let c = z[i];
                i += 1;
                if c >= 0xc0 {
                    while (byte_at_or_nul(&z, i) & 0xc0) == 0x80 {
                        i += 1;
                    }
                }
                n += 1;
            }
            api::result_int(context, n);
        }
        _ => {
            api::result_null(context);
        }
    }
}

/// Implementação da função octet_length().
fn bytelength_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    match api::value_type(&argv[0]) {
        SQLITE_BLOB => {
            api::result_int(context, api::value_bytes(&argv[0]));
        }
        SQLITE_INTEGER | SQLITE_FLOAT => {
            let enc = api::context_db_handle(context).borrow().enc;
            let m: i64 = if enc <= SQLITE_UTF8 { 1 } else { 2 };
            api::result_int64(context, (api::value_bytes(&argv[0]) as i64) * m);
        }
        SQLITE_TEXT => {
            if api::value_encoding(&argv[0]) <= SQLITE_UTF8 {
                api::result_int(context, api::value_bytes(&argv[0]));
            } else {
                api::result_int(context, api::value_bytes16(&argv[0]));
            }
        }
        _ => {
            api::result_null(context);
        }
    }
}

/// Implementação da função abs().
///
/// IMP: R-23979-26855 A função abs(X) devolve o valor absoluto do argumento
/// numérico X.
fn abs_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    match api::value_type(&argv[0]) {
        SQLITE_INTEGER => {
            let mut i_val = api::value_int64(&argv[0]);
            if i_val < 0 {
                if i_val == SMALLEST_INT64 {
                    // IMP: R-31676-45509 Se X é o inteiro -9223372036854775808,
                    // abs(X) lança erro de estouro de inteiro.
                    api::result_error(context, b"integer overflow", -1);
                    return;
                }
                i_val = -i_val;
            }
            api::result_int64(context, i_val);
        }
        SQLITE_NULL => {
            // IMP: R-37434-19929 Abs(X) devolve NULL se X é NULL.
            api::result_null(context);
        }
        _ => {
            // IMP: R-01992-00519 Abs(X) devolve 0.0 se X é string ou blob que
            // não converte para número.
            let mut r_val = api::value_double(&argv[0]);
            if r_val < 0.0 {
                r_val = -r_val;
            }
            api::result_double(context, r_val);
        }
    }
}

/// Implementação da função instr(): posição (1-based, em caracteres para texto
/// e em bytes para blob) da primeira ocorrência de needle em haystack, ou 0.
fn instr_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    let type_haystack = api::value_type(&argv[0]);
    let type_needle = api::value_type(&argv[1]);
    if type_haystack == SQLITE_NULL || type_needle == SQLITE_NULL {
        return;
    }
    let mut n_haystack = api::value_bytes(&argv[0]) as i64;
    let mut n_needle = api::value_bytes(&argv[1]) as i64;
    let mut n: i32 = 1;
    if n_needle > 0 {
        let z_haystack: Vec<u8>;
        let z_needle: Vec<u8>;
        let is_text: bool;
        if type_haystack == SQLITE_BLOB && type_needle == SQLITE_BLOB {
            z_haystack = api::value_blob(&argv[0]).unwrap_or_default();
            z_needle = api::value_blob(&argv[1]).unwrap_or_default();
            is_text = false;
        } else if type_haystack != SQLITE_BLOB && type_needle != SQLITE_BLOB {
            z_haystack = api::value_text(&argv[0]).unwrap_or_default();
            z_needle = api::value_text(&argv[1]).unwrap_or_default();
            is_text = true;
        } else {
            // Um blob e um texto: ambos são duplicados e tratados como texto.
            let p_c1 = api::value_dup(&argv[0]);
            let z_h = match api::value_text(&p_c1) {
                Some(z) => z,
                None => {
                    api::value_free(p_c1);
                    api::result_error_nomem(context);
                    return;
                }
            };
            n_haystack = api::value_bytes(&p_c1) as i64;
            let p_c2 = api::value_dup(&argv[1]);
            let z_n = match api::value_text(&p_c2) {
                Some(z) => z,
                None => {
                    api::value_free(p_c1);
                    api::value_free(p_c2);
                    api::result_error_nomem(context);
                    return;
                }
            };
            n_needle = api::value_bytes(&p_c2) as i64;
            api::value_free(p_c1);
            api::value_free(p_c2);
            z_haystack = z_h;
            z_needle = z_n;
            is_text = true;
        }
        let first_char = byte_at_or_nul(&z_needle, 0);
        let nn = n_needle as usize;
        let mut h = 0usize;
        while n_needle <= n_haystack
            && (byte_at_or_nul(&z_haystack, h) != first_char
                || z_haystack.get(h..h + nn) != z_needle.get(..nn))
        {
            n += 1;
            loop {
                n_haystack -= 1;
                h += 1;
                if !(is_text && (byte_at_or_nul(&z_haystack, h) & 0xc0) == 0x80) {
                    break;
                }
            }
        }
        if n_needle > n_haystack {
            n = 0;
        }
    }
    api::result_int(context, n);
}

/// Implementação da função printf() (a.k.a. format()) do SQL.
fn printf_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    if argc < 1 {
        return;
    }
    if let Some(z_format) = api::value_text(&argv[0]) {
        // Os argumentos após o formato fazem o papel de PrintfArguments.
        let mut x = PrintfArguments {
            n_arg: argc - 1,
            n_used: 0,
            ap_arg: argv[1..argc as usize].to_vec(),
        };
        let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
        let mut str_ = StrAccum::default();
        str_accum_init(&mut str_, Some(db.clone()), None, 0, limit);
        str_.printf_flags = SQLITE_PRINTF_SQLFUNC;
        api::str_appendf(&mut str_, &z_format, &[PrintfArg::SqlFunc(&mut x)]);
        let n = str_.n_char as i32;
        let z_text = str_accum_finish(&mut str_);
        api::result_text(context, z_text.as_deref(), n, SQLITE_DYNAMIC);
    }
}

/// Implementação da função substr(): p2 caracteres de x a partir de p1
/// (1-based; negativo conta do fim; p2 negativo devolve os que precedem p1).
/// Para texto conta caracteres UTF-8, para blob conta bytes.
fn substr_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert!(argc == 3 || argc == 2);
    if api::value_type(&argv[1]) == SQLITE_NULL
        || (argc == 3 && api::value_type(&argv[2]) == SQLITE_NULL)
    {
        return;
    }
    let p0_type = api::value_type(&argv[0]);
    let mut p1 = api::value_int(&argv[1]) as i64;
    let z: Vec<u8>;
    let mut len: i64 = 0;
    if p0_type == SQLITE_BLOB {
        len = api::value_bytes(&argv[0]) as i64;
        z = match api::value_blob(&argv[0]) {
            Some(z) => z,
            None => return,
        };
        debug_assert_eq!(len, api::value_bytes(&argv[0]) as i64);
    } else {
        z = match api::value_text(&argv[0]) {
            Some(z) => z,
            None => return,
        };
        if p1 < 0 {
            let mut i = 0usize;
            while byte_at_or_nul(&z, i) != 0 {
                skip_utf8(&z, &mut i);
                len += 1;
            }
        }
    }
    // SQLITE_SUBSTR_COMPATIBILITY não é definido no Debian: ramo removido.
    let mut neg_p2 = false;
    let mut p2: i64;
    if argc == 3 {
        p2 = api::value_int(&argv[2]) as i64;
        if p2 < 0 {
            p2 = -p2;
            neg_p2 = true;
        }
    } else {
        p2 = api::context_db_handle(context).borrow().a_limit[SQLITE_LIMIT_LENGTH as usize] as i64;
    }
    if p1 < 0 {
        p1 += len;
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
    if p0_type != SQLITE_BLOB {
        let mut i = 0usize;
        while byte_at_or_nul(&z, i) != 0 && p1 != 0 {
            skip_utf8(&z, &mut i);
            p1 -= 1;
        }
        let start = i.min(z.len());
        while byte_at_or_nul(&z, i) != 0 && p2 != 0 {
            skip_utf8(&z, &mut i);
            p2 -= 1;
        }
        let end = i.min(z.len());
        api::result_text64(
            context,
            Some(&z[start..end]),
            (end - start) as u64,
            SQLITE_TRANSIENT,
            SQLITE_UTF8,
        );
    } else {
        if p1 + p2 > len {
            p2 = len - p1;
            if p2 < 0 {
                p2 = 0;
            }
        }
        let start = (p1 as usize).min(z.len());
        let end = (start + p2 as usize).min(z.len());
        api::result_blob64(context, Some(&z[start..end]), (end - start) as u64, SQLITE_TRANSIENT);
    }
}


// ---- part_001.rs ----

/// Implementação da função round().
fn round_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    let mut n: i32 = 0;
    debug_assert!(argc == 1 || argc == 2);
    if argc == 2 {
        if api::value_type(&argv[1]) == SQLITE_NULL {
            return;
        }
        n = api::value_int(&argv[1]);
        if n > 30 {
            n = 30;
        }
        if n < 0 {
            n = 0;
        }
    }
    if api::value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    let mut r = api::value_double(&argv[0]);
    // Se Y==0 e X cabe em um inteiro de 64 bits, trata o arredondamento
    // diretamente, caso contrário usa printf.
    if r < -4503599627370496.0 || r > 4503599627370496.0 {
        // O valor não tem parte fracionária, então não há nada para arredondar.
    } else if n == 0 {
        r = ((r + (if r < 0.0 { -0.5 } else { 0.5 })) as i64) as f64;
    } else {
        // "%!.*f" é o formato do printf.c do SQLite (não o format! do Rust).
        let z_buf = match api::mprintf(b"%!.*f", &[PrintfArg::Int64(n as i64), PrintfArg::Double(r)]) {
            Some(z) => z,
            None => {
                api::result_error_nomem(context);
                return;
            }
        };
        ato_f(&z_buf, &mut r, z_buf.len() as i32, SQLITE_UTF8);
    }
    api::result_double(context, r);
}

/// Aloca `n_byte` bytes (zerados). Se `n_byte` passa do comprimento máximo de
/// string ou blob, levanta SQLITE_TOOBIG e devolve None. (A falta de memória do
/// C não existe aqui: o Vec aborta o processo.)
fn context_malloc(context: &mut sqlite3_context, n_byte: i64) -> Option<Vec<u8>> {
    let limit = api::context_db_handle(context).borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    debug_assert!(n_byte > 0);
    if n_byte > limit as i64 {
        api::result_error_toobig(context);
        None
    } else {
        Some(vec![0u8; n_byte as usize])
    }
}

/// Núcleo de upper() e lower(): aplica `map` byte a byte (ASCII).
fn convert_case(context: &mut sqlite3_context, argv: &[MemRef], map: fn(u8) -> u8) {
    let z2 = api::value_text(&argv[0]);
    let n = api::value_bytes(&argv[0]) as usize;
    if let Some(z2) = z2 {
        if let Some(mut z1) = context_malloc(context, (n as i64) + 1) {
            for i in 0..n {
                z1[i] = map(z2[i]);
            }
            api::result_text(context, Some(&z1[..n]), n as i32, SQLITE_DYNAMIC);
        }
    }
}

/// Implementação da função upper() do SQL.
fn upper_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    convert_case(context, argv, toupper);
}

/// Implementação da função lower() do SQL.
fn lower_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    convert_case(context, argv, tolower);
}

// Funções como COALESCE(), IFNULL() e UNLIKELY() são implementadas como código
// VDBE, mas a tabela de funções ainda precisa de uma implementação. noopFunc
// nunca é chamada, então usa-se version() como substituta.
pub use version_func as noop_func;

/// Implementação de random(): devolve um inteiro aleatório.
fn random_func(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    let mut buf = [0u8; 8];
    api::randomness(&mut buf);
    let mut r = i64::from_ne_bytes(buf);
    if r < 0 {
        // Evita 0x8000000000000000, cujo abs() devolve o mesmo valor: mascara o
        // bit de sinal e toma o complemento de 2. O mínimo resultante é
        // -9223372036854775807.
        r = -(r & LARGEST_INT64);
    }
    api::result_int64(context, r);
}

/// Implementação de randomblob(N): devolve um blob aleatório de N bytes.
fn random_blob(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    let mut n = api::value_int64(&argv[0]);
    if n < 1 {
        n = 1;
    }
    if let Some(mut p) = context_malloc(context, n) {
        api::randomness(&mut p);
        api::result_blob(context, Some(&p), n as i32, SQLITE_DYNAMIC);
    }
}

/// Implementação da função SQL last_insert_rowid(): mesmo valor da função de
/// API sqlite3_last_insert_rowid().
fn last_insert_rowid(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    let db = api::context_db_handle(context);
    // IMP: R-51513-12026 last_insert_rowid() é um invólucro de
    // sqlite3_last_insert_rowid().
    api::result_int64(context, api::last_insert_rowid(&db));
}

/// Implementação da função SQL changes().
///
/// IMP: R-32760-32347 changes() é um invólucro de sqlite3_changes64() e segue
/// as mesmas regras de contagem.
fn changes(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    let db = api::context_db_handle(context);
    api::result_int64(context, api::changes64(&db));
}

/// Implementação da função SQL total_changes(): mesmo valor de
/// sqlite3_total_changes64().
fn total_changes(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    let db = api::context_db_handle(context);
    // IMP: R-11217-42568 invólucro de sqlite3_total_changes64().
    api::result_int64(context, api::total_changes64(&db));
}

/// Estrutura que define como fazer comparações estilo GLOB.
#[derive(Clone, Copy)]
pub struct CompareInfo {
    /// "*" ou "%"
    pub match_all: u8,
    /// "?" ou "_"
    pub match_one: u8,
    /// "[" ou 0
    pub match_set: u8,
    /// verdadeiro para ignorar diferenças de maiúsculas e minúsculas
    pub no_case: u8,
}

pub const GLOB_INFO: CompareInfo = CompareInfo { match_all: b'*', match_one: b'?', match_set: b'[', no_case: 0 };
/// O comportamento correto do SQL-92 é o LIKE ignorar maiúsculas e minúsculas:
/// 'a' LIKE 'A' é verdadeiro.
pub const LIKE_INFO_NORM: CompareInfo = CompareInfo { match_all: b'%', match_one: b'_', match_set: 0, no_case: 1 };
/// Com SQLITE_CASE_SENSITIVE_LIKE o LIKE diferencia caixa.
pub const LIKE_INFO_ALT: CompareInfo = CompareInfo { match_all: b'%', match_one: b'_', match_set: 0, no_case: 0 };

/// Retornos possíveis de pattern_compare().
pub const SQLITE_MATCH: i32 = 0;
pub const SQLITE_NOMATCH: i32 = 1;
pub const SQLITE_NOWILDCARDMATCH: i32 = 2;

/// Utf8Read: lê um caractere de `z` em `*i` (0 no fim, como o NUL do C).
#[inline]
fn utf8_read_at(z: &[u8], i: &mut usize) -> u32 {
    if *i >= z.len() {
        return 0;
    }
    if z[*i] < 0x80 {
        let c = z[*i] as u32;
        *i += 1;
        c
    } else {
        utf8_read(z, i)
    }
}

/// Compara duas strings UTF-8 por igualdade, sendo a primeira um padrão GLOB ou
/// LIKE. Retorna SQLITE_MATCH, SQLITE_NOMATCH ou SQLITE_NOWILDCARDMATCH (sem
/// correspondência apesar dos curingas * ou %).
///
/// GLOB: '*' qualquer sequência; '?' um caractere; [...] um caractere da lista;
/// [^...] um caractere fora da lista (']' primeiro na lista o inclui; intervalos
/// com '-'). LIKE: '%' qualquer sequência; '_' um caractere; Ec casa c literal.
/// Geralmente rápida, mas pode ser N**2 no pior caso.
fn pattern_compare(z_pattern: &[u8], z_string: &[u8], p_info: &CompareInfo, match_other: u32) -> i32 {
    let match_one = p_info.match_one as u32;
    let match_all = p_info.match_all as u32;
    let no_case = p_info.no_case != 0;
    let mut z_escaped: Option<usize> = None;
    let mut p = 0usize; // posição em z_pattern
    let mut s = 0usize; // posição em z_string

    loop {
        let mut c = utf8_read_at(z_pattern, &mut p);
        if c == 0 {
            break;
        }
        if c == match_all {
            // Pula "*" repetidos; cada "?" pulado consome um caractere da entrada.
            loop {
                c = utf8_read_at(z_pattern, &mut p);
                if !(c == match_all || (c == match_one && match_one != 0)) {
                    break;
                }
                if c == match_one && utf8_read_at(z_string, &mut s) == 0 {
                    return SQLITE_NOWILDCARDMATCH;
                }
            }
            if c == 0 {
                return SQLITE_MATCH; // "*" no fim do padrão casa
            } else if c == match_other {
                if p_info.match_set == 0 {
                    c = utf8_read_at(z_pattern, &mut p);
                    if c == 0 {
                        return SQLITE_NOWILDCARDMATCH;
                    }
                } else {
                    // "[...]" logo após o "*": busca recursiva lenta, caso incomum.
                    debug_assert!(match_other < 0x80);
                    while byte_at_or_nul(z_string, s) != 0 {
                        let b_match = pattern_compare(&z_pattern[p - 1..], &z_string[s..], p_info, match_other);
                        if b_match != SQLITE_NOMATCH {
                            return b_match;
                        }
                        skip_utf8(z_string, &mut s);
                    }
                    return SQLITE_NOWILDCARDMATCH;
                }
            }
            // Aqui c é o primeiro caractere do padrão após o "*". Procura na
            // entrada o primeiro caractere que casa e continua recursivamente.
            // Em busca sem diferenciar caixa procura c ou a sua outra caixa.
            if c < 0x80 {
                let (stop0, stop1) = if no_case {
                    (toupper(c as u8), tolower(c as u8))
                } else {
                    (c as u8, c as u8)
                };
                loop {
                    // strcspn(zString, zStop): para em stop0, stop1 ou NUL.
                    while s < z_string.len()
                        && z_string[s] != 0
                        && z_string[s] != stop0
                        && z_string[s] != stop1
                    {
                        s += 1;
                    }
                    if byte_at_or_nul(z_string, s) == 0 {
                        break;
                    }
                    s += 1;
                    let b_match = pattern_compare(&z_pattern[p..], &z_string[s..], p_info, match_other);
                    if b_match != SQLITE_NOMATCH {
                        return b_match;
                    }
                }
            } else {
                loop {
                    let c2 = utf8_read_at(z_string, &mut s);
                    if c2 == 0 {
                        break;
                    }
                    if c2 != c {
                        continue;
                    }
                    let b_match = pattern_compare(&z_pattern[p..], &z_string[s..], p_info, match_other);
                    if b_match != SQLITE_NOMATCH {
                        return b_match;
                    }
                }
            }
            return SQLITE_NOWILDCARDMATCH;
        }
        if c == match_other {
            if p_info.match_set == 0 {
                c = utf8_read_at(z_pattern, &mut p);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                z_escaped = Some(p);
            } else {
                let mut prior_c = 0u32;
                let mut seen = false;
                let mut invert = false;
                c = utf8_read_at(z_string, &mut s);
                if c == 0 {
                    return SQLITE_NOMATCH;
                }
                let mut c2 = utf8_read_at(z_pattern, &mut p);
                if c2 == b'^' as u32 {
                    invert = true;
                    c2 = utf8_read_at(z_pattern, &mut p);
                }
                if c2 == b']' as u32 {
                    if c == b']' as u32 {
                        seen = true;
                    }
                    c2 = utf8_read_at(z_pattern, &mut p);
                }
                while c2 != 0 && c2 != b']' as u32 {
                    if c2 == b'-' as u32
                        && byte_at_or_nul(z_pattern, p) != b']'
                        && byte_at_or_nul(z_pattern, p) != 0
                        && prior_c > 0
                    {
                        c2 = utf8_read_at(z_pattern, &mut p);
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
                    c2 = utf8_read_at(z_pattern, &mut p);
                }
                if c2 == 0 || seen == invert {
                    return SQLITE_NOMATCH;
                }
                continue;
            }
        }
        let c2 = utf8_read_at(z_string, &mut s);
        if c == c2 {
            continue;
        }
        if no_case && tolower(c as u8) == tolower(c2 as u8) && c < 0x80 && c2 < 0x80 {
            continue;
        }
        if c == match_one && Some(p) != z_escaped && c2 != 0 {
            continue;
        }
        return SQLITE_NOMATCH;
    }
    if byte_at_or_nul(z_string, s) == 0 {
        SQLITE_MATCH
    } else {
        SQLITE_NOMATCH
    }
}


// ---- part_002.rs ----

/// A interface `sqlite3_strglob()`. Devolve 0 em caso de coincidência (como
/// strcmp()) e valor diferente de zero se não houver coincidência. O ponteiro
/// nulo do C é `None`.
pub fn strglob(z_glob_pattern: Option<&[u8]>, z_string: Option<&[u8]>) -> i32 {
    match (z_glob_pattern, z_string) {
        (p, None) => p.is_some() as i32,
        (None, Some(_)) => 1,
        (Some(p), Some(s)) => pattern_compare(p, s, &GLOB_INFO, b'[' as u32),
    }
}

/// A interface `sqlite3_strlike()`. Devolve 0 em caso de coincidência e valor
/// diferente de zero para falha, como strcmp().
pub fn strlike(z_pattern: Option<&[u8]>, z_str: Option<&[u8]>, esc: u32) -> i32 {
    match (z_pattern, z_str) {
        (p, None) => p.is_some() as i32,
        (None, Some(_)) => 1,
        (Some(p), Some(s)) => pattern_compare(p, s, &LIKE_INFO_NORM, esc),
    }
}

// `sqlite3_like_count` só existe com SQLITE_TEST, que o build do Debian não
// define, então o contador some.

/// Implementação da função SQL like(). Implementa o operador LIKE embutido. O
/// primeiro argumento é o padrão e o segundo é a string. Assim, a instrução
/// `A LIKE B` é implementada como like(B,A).
///
/// A mesma função (com outra estrutura CompareInfo) calcula o operador GLOB.
fn like_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    let mut p_info: CompareInfo = api::user_data(context);
    let escape: u32;

    // SQLITE_LIKE_DOESNT_MATCH_BLOBS não é definido no Debian: ramo removido.

    // Limita o comprimento do padrão LIKE ou GLOB para evitar problemas de
    // recursão profunda e comportamento N*N em pattern_compare().
    let n_pat = api::value_bytes(&argv[0]);
    if n_pat > db.borrow().a_limit[SQLITE_LIMIT_LIKE_PATTERN_LENGTH as usize] {
        api::result_error(context, b"LIKE or GLOB pattern too complex", -1);
        return;
    }
    if argc == 3 {
        // A string de escape deve consistir em um único caractere UTF-8. Caso
        // contrário, devolve um erro.
        let z_esc = match api::value_text(&argv[2]) {
            Some(z) => z,
            None => return,
        };
        if utf8_char_len(&z_esc, -1) != 1 {
            api::result_error(context, b"ESCAPE expression must be a single character", -1);
            return;
        }
        let mut pos: usize = 0;
        escape = utf8_read(&z_esc, &mut pos);
        if escape == p_info.match_all as u32 || escape == p_info.match_one as u32 {
            // `p_info` já é uma cópia (o memcpy para backupInfo do C).
            if escape == p_info.match_all as u32 {
                p_info.match_all = 0;
            }
            if escape == p_info.match_one as u32 {
                p_info.match_one = 0;
            }
        }
    } else {
        escape = p_info.match_set as u32;
    }
    let z_b = api::value_text(&argv[0]);
    let z_a = api::value_text(&argv[1]);
    if let (Some(z_a), Some(z_b)) = (z_a, z_b) {
        api::result_int(
            context,
            (pattern_compare(&z_b, &z_a, &p_info, escape) == SQLITE_MATCH) as i32,
        );
    }
}

/// Implementação da função NULLIF(x,y). O resultado é o primeiro argumento se
/// os argumentos forem diferentes. O resultado é NULL se forem iguais.
fn nullif_func(context: &mut sqlite3_context, _not_used: i32, argv: &[MemRef]) {
    let p_coll = get_func_coll_seq(context);
    if mem_compare(&argv[0], &argv[1], &p_coll) != 0 {
        api::result_value(context, &argv[0]);
    }
}

/// Implementação da função sqlite_version(). O resultado é a versão da
/// biblioteca SQLite em execução.
fn version_func(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    // IMP: R-48699-48617 Esta função é um invólucro SQL da interface C
    // sqlite3_libversion().
    api::result_text(context, Some(api::libversion()), -1, SQLITE_STATIC);
}

/// Implementação da função sqlite_source_id(). O resultado é uma string que
/// identifica a versão particular do código-fonte usada para compilar o SQLite.
fn sourceid_func(context: &mut sqlite3_context, _not_used: i32, _not_used2: &[MemRef]) {
    // IMP: R-24470-31136 Esta função é um invólucro SQL da interface C
    // sqlite3_sourceid().
    api::result_text(context, Some(api::sourceid()), -1, SQLITE_STATIC);
}

/// Implementação da função sqlite_log(). É um invólucro de sqlite3_log(). O
/// valor devolvido é NULL. A função existe apenas pelos efeitos colaterais.
fn errlog_func(_context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    api::log(
        api::value_int(&argv[0]),
        b"%s",
        &[PrintfArg::Text(api::value_text(&argv[1]))],
    );
}

/// Implementação da função sqlite_compileoption_used(). O resultado é um
/// inteiro que indica se a opção de compilação foi usada para compilar o SQLite.
fn compileoption_used_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert!(argc == 1);
    // IMP: R-39564-36305 A função SQL sqlite_compileoption_used() é um invólucro
    // da função C/C++ sqlite3_compileoption_used().
    if let Some(z_opt_name) = api::value_text(&argv[0]) {
        api::result_int(context, api::compileoption_used(&z_opt_name));
    }
}

/// Implementação da função sqlite_compileoption_get(). O resultado é uma string
/// que identifica as opções de compilação usadas para compilar o SQLite.
fn compileoption_get_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert!(argc == 1);
    // IMP: R-04922-24076 A função SQL sqlite_compileoption_get() é um invólucro
    // da função C/C++ sqlite3_compileoption_get().
    let n = api::value_int(&argv[0]);
    api::result_text(context, api::compileoption_get(n), -1, SQLITE_STATIC);
}

/// Tabela para converter meios-bytes (nybbles) em dígitos hexadecimais ASCII.
static HEXDIGITS: [u8; 16] = [
    b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'A', b'B', b'C', b'D', b'E', b'F',
];

/// Acrescenta a `p_str` o texto que é a representação como literal SQL do valor
/// contido em `p_value`.
pub fn quote_value(p_str: &mut StrAccum, p_value: &MemRef) {
    // Como está implementado hoje, a string precisa estar inicialmente vazia.
    // Talvez esse requisito seja relaxado no futuro, mas isso exigirá melhorias
    // na implementação.
    debug_assert!(p_str.n_char == 0);

    match api::value_type(p_value) {
        SQLITE_FLOAT => {
            let r1 = api::value_double(p_value);
            api::str_appendf(p_str, b"%!0.15g", &[PrintfArg::Double(r1)]);
            if let Some(z_val) = api::str_value(p_str) {
                let mut r2: f64 = 0.0;
                ato_f(&z_val, &mut r2, p_str.n_char as i32, SQLITE_UTF8);
                if r1 != r2 {
                    api::str_reset(p_str);
                    api::str_appendf(p_str, b"%!0.20e", &[PrintfArg::Double(r1)]);
                }
            }
        }
        SQLITE_INTEGER => {
            api::str_appendf(p_str, b"%lld", &[PrintfArg::Int64(api::value_int64(p_value))]);
        }
        SQLITE_BLOB => {
            let z_blob = api::value_blob(p_value).unwrap_or_default();
            let n_blob = api::value_bytes(p_value) as i64;
            debug_assert!(n_blob as usize == z_blob.len()); // sem mudança de codificação
            str_accum_enlarge(p_str, n_blob * 2 + 4);
            if p_str.acc_error == 0 {
                let need = (n_blob * 2 + 4) as usize;
                if p_str.z_text.len() < need {
                    // O Vec do buffer precisa cobrir os índices escritos abaixo.
                    p_str.z_text.resize(need, 0);
                }
                let z_text = &mut p_str.z_text;
                for i in 0..n_blob as usize {
                    z_text[(i * 2) + 2] = HEXDIGITS[((z_blob[i] >> 4) & 0x0F) as usize];
                    z_text[(i * 2) + 3] = HEXDIGITS[(z_blob[i] & 0x0F) as usize];
                }
                z_text[(n_blob as usize * 2) + 2] = b'\'';
                z_text[(n_blob as usize * 2) + 3] = 0;
                z_text[0] = b'X';
                z_text[1] = b'\'';
                p_str.n_char = (n_blob * 2 + 3) as u32;
            }
        }
        SQLITE_TEXT => {
            let z_arg = api::value_text(p_value);
            api::str_appendf(p_str, b"%Q", &[PrintfArg::Text(z_arg)]);
        }
        _ => {
            debug_assert!(api::value_type(p_value) == SQLITE_NULL);
            api::str_append(p_str, b"NULL", 4);
        }
    }
}

/// Implementação da função QUOTE().
///
/// quote(X) devolve o texto de um literal SQL que é o valor do argumento, próprio
/// para inclusão numa instrução SQL. Strings saem entre aspas simples, com
/// escapes nas aspas internas quando preciso. BLOBs saem como literais
/// hexadecimais. Strings com NUL embutido não podem ser representadas como
/// literal SQL, então o literal devolvido é truncado antes do primeiro NUL.
fn quote_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    let db = api::context_db_handle(context);
    debug_assert!(argc == 1);
    let mut str_ = StrAccum::default();
    let limit = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize];
    str_accum_init(&mut str_, Some(db.clone()), None, 0, limit);
    quote_value(&mut str_, &argv[0]);
    let z_text = str_accum_finish(&mut str_);
    api::result_text(context, z_text.as_deref(), str_.n_char as i32, SQLITE_DYNAMIC);
    if str_.acc_error != SQLITE_OK {
        api::result_null(context);
        api::result_error_code(context, str_.acc_error);
    }
}

/// A função unicode(). Devolve o valor inteiro do ponto de código Unicode do
/// primeiro caractere da string de entrada.
fn unicode_func(context: &mut sqlite3_context, _argc: i32, argv: &[MemRef]) {
    if let Some(z) = api::value_text(&argv[0]) {
        if !z.is_empty() && z[0] != 0 {
            let mut pos: usize = 0;
            api::result_int(context, utf8_read(&z, &mut pos) as i32);
        }
    }
}

/// A função char(). Recebe zero ou mais argumentos inteiros e constrói uma string
/// em que cada caractere é o caractere Unicode do argumento correspondente.
fn char_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    // O sqlite3_malloc64 do C vira Vec: a falta de memória aborta o processo, então
    // o ramo sqlite3_result_error_nomem não tem como ocorrer aqui.
    let mut z: Vec<u8> = Vec::with_capacity(argc as usize * 4 + 1);
    for i in 0..argc as usize {
        let mut x: i64 = api::value_int64(&argv[i]);
        if x < 0 || x > 0x10ffff {
            x = 0xfffd;
        }
        let c: u32 = (x & 0x1fffff) as u32;
        if c < 0x00080 {
            z.push((c & 0xFF) as u8);
        } else if c < 0x00800 {
            z.push(0xC0 + ((c >> 6) & 0x1F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        } else if c < 0x10000 {
            z.push(0xE0 + ((c >> 12) & 0x0F) as u8);
            z.push(0x80 + ((c >> 6) & 0x3F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        } else {
            z.push(0xF0 + ((c >> 18) & 0x07) as u8);
            z.push(0x80 + ((c >> 12) & 0x3F) as u8);
            z.push(0x80 + ((c >> 6) & 0x3F) as u8);
            z.push(0x80 + (c & 0x3F) as u8);
        }
    }
    let n = z.len() as u64;
    api::result_text64(context, Some(&z), n, SQLITE_DYNAMIC, SQLITE_UTF8);
}


// ---- part_003.rs ----

/// A função hex(). Interpreta o argumento como blob e devolve a representação
/// hexadecimal como texto.
fn hex_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    let p_blob = api::value_blob(&argv[0]).unwrap_or_default();
    let n = api::value_bytes(&argv[0]) as usize;
    debug_assert!(p_blob.len() >= n || n == 0);
    if let Some(mut z_hex) = context_malloc(context, (n as i64) * 2 + 1) {
        let mut z = 0usize;
        for i in 0..n {
            let c = p_blob[i];
            z_hex[z] = HEXDIGITS[((c >> 4) & 0xf) as usize];
            z += 1;
            z_hex[z] = HEXDIGITS[(c & 0xf) as usize];
            z += 1;
        }
        api::result_text64(context, Some(&z_hex[..z]), z as u64, SQLITE_DYNAMIC, SQLITE_UTF8);
    }
}

/// O buffer `z_str` contém bytes de texto UTF-8. Retorna 1 se contém o
/// caractere `ch`, ou 0 se não contém.
fn str_contains_char(z_str: &[u8], ch: u32) -> i32 {
    let mut z = 0usize;
    while z < z_str.len() {
        let tst = utf8_read_at(z_str, &mut z);
        if tst == ch {
            return 1;
        }
    }
    0
}

/// A função unhex(), com um ou dois argumentos. O primeiro é lido como texto
/// com pares de dígitos hexadecimais, decodificados e devolvidos como blob.
///
/// Com um só argumento, ele deve ter apenas um número par de dígitos
/// hexadecimais; senão devolve NULL. Com o segundo argumento, qualquer caractere
/// dele também pode aparecer entre pares de dígitos do primeiro. Outro caractere,
/// ou um permitido entre os dois dígitos de um mesmo byte, devolve NULL.
///
///     unhex('ABCD')       IS x'ABCD'
///     unhex('AB CD')      IS NULL
///     unhex('AB CD', ' ') IS x'ABCD'
///     unhex('A BCD', ' ') IS NULL
fn unhex_func(p_ctx: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    let z_hex = api::value_text(&argv[0]);
    let n_hex = api::value_bytes(&argv[0]) as i64;
    let mut z_pass: Option<Vec<u8>> = Some(Vec::new());
    debug_assert!(argc == 1 || argc == 2);
    if argc == 2 {
        z_pass = api::value_text(&argv[1]);
        let _n_pass = api::value_bytes(&argv[1]);
    }
    let (z_hex, z_pass) = match (z_hex, z_pass) {
        (Some(h), Some(p)) => (h, p),
        _ => return,
    };

    let mut p_blob = match context_malloc(p_ctx, (n_hex / 2) + 1) {
        Some(b) => b,
        None => return,
    };
    let mut p = 0usize; // posição de escrita em p_blob
    let mut h = 0usize; // posição de leitura em z_hex

    while byte_at_or_nul(&z_hex, h) != 0x00 {
        let mut c = byte_at_or_nul(&z_hex, h); // dígito mais significativo
        while !isxdigit(c) {
            let ch = utf8_read_at(&z_hex, &mut h);
            if str_contains_char(&z_pass, ch) == 0 {
                return; // unhex_null: o resultado fica NULL
            }
            c = byte_at_or_nul(&z_hex, h);
            if c == 0x00 {
                // unhex_done
                api::result_blob(p_ctx, Some(&p_blob[..p]), p as i32, SQLITE_DYNAMIC);
                return;
            }
        }
        h += 1;
        let d = byte_at_or_nul(&z_hex, h); // dígito menos significativo
        h += 1;
        if !isxdigit(d) {
            return; // unhex_null
        }
        p_blob[p] = (hex_to_int(c) << 4) | hex_to_int(d);
        p += 1;
    }
    api::result_blob(p_ctx, Some(&p_blob[..p]), p as i32, SQLITE_DYNAMIC);
}

/// A função zeroblob(N) devolve um blob de N bytes preenchido com zeros.
fn zeroblob_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 1);
    let mut n = api::value_int64(&argv[0]);
    if n < 0 {
        n = 0;
    }
    let rc = api::result_zeroblob64(context, n); // IMP: R-00293-64994
    if rc != 0 {
        api::result_error_code(context, rc);
    }
}

/// A função replace(). Os três argumentos são strings: A, B e C. O resultado é
/// derivado de A trocando cada ocorrência de B por C. A correspondência é exata
/// e sequências de colação não são usadas.
fn replace_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    debug_assert_eq!(argc, 3);
    let z_str = match api::value_text(&argv[0]) {
        Some(z) => z,
        None => return,
    };
    let n_str = api::value_bytes(&argv[0]) as i64;
    let z_pattern = match api::value_text(&argv[1]) {
        Some(z) => z,
        None => return,
    };
    if byte_at_or_nul(&z_pattern, 0) == 0 {
        debug_assert!(api::value_type(&argv[1]) != SQLITE_NULL);
        api::result_text(context, Some(&z_str), n_str as i32, SQLITE_TRANSIENT);
        return;
    }
    let n_pattern = api::value_bytes(&argv[1]) as i64;
    let z_rep = match api::value_text(&argv[2]) {
        Some(z) => z,
        None => return,
    };
    let n_rep = api::value_bytes(&argv[2]) as i64;
    let mut n_out: i64 = n_str + 1;
    let limit = api::context_db_handle(context).borrow().a_limit[SQLITE_LIMIT_LENGTH as usize] as i64;
    let mut z_out = match context_malloc(context, n_out) {
        Some(b) => b,
        None => return,
    };
    z_out.clear();
    let loop_limit = n_str - n_pattern; // último z_str[] que pode casar
    let np = n_pattern as usize;
    let mut i: i64 = 0;
    while i <= loop_limit {
        let iu = i as usize;
        if z_str[iu] != z_pattern[0] || z_str[iu..iu + np] != z_pattern[..np] {
            z_out.push(z_str[iu]);
        } else {
            if n_rep > n_pattern {
                // Só a contabilidade de tamanho do C importa aqui: o Vec cresce
                // sozinho, mas o limite de comprimento precisa ser respeitado.
                n_out += n_rep - n_pattern;
                if n_out - 1 > limit {
                    api::result_error_toobig(context);
                    return;
                }
            }
            z_out.extend_from_slice(&z_rep[..n_rep as usize]);
            i += n_pattern - 1;
        }
        i += 1;
    }
    z_out.extend_from_slice(&z_str[i as usize..n_str as usize]);
    let j = z_out.len();
    api::result_text(context, Some(&z_out), j as i32, SQLITE_DYNAMIC);
}

/// Implementação de TRIM(), LTRIM() e RTRIM(). O userdata é 0x1 para cortar à
/// esquerda, 0x2 à direita e 0x3 dos dois lados.
fn trim_func(context: &mut sqlite3_context, argc: i32, argv: &[MemRef]) {
    if api::value_type(&argv[0]) == SQLITE_NULL {
        return;
    }
    let z_in = match api::value_text(&argv[0]) {
        Some(z) => z,
        None => return,
    };
    let mut n_in = api::value_bytes(&argv[0]) as usize;
    let mut start = 0usize;
    // Cada caractere do conjunto a cortar, como fatia de bytes.
    let mut az_char: Vec<Vec<u8>> = Vec::new();
    if argc == 1 {
        az_char.push(b" ".to_vec());
    } else {
        let z_char_set = match api::value_text(&argv[1]) {
            Some(z) => z,
            None => return,
        };
        let mut z = 0usize;
        while byte_at_or_nul(&z_char_set, z) != 0 {
            let from = z;
            skip_utf8(&z_char_set, &mut z);
            az_char.push(z_char_set[from..z.min(z_char_set.len())].to_vec());
        }
    }
    if !az_char.is_empty() {
        let flags = api::user_data_int(context);
        if flags & 1 != 0 {
            while n_in > 0 {
                let hit = az_char.iter().find(|ch| ch.len() <= n_in && z_in[start..start + ch.len()] == ch[..]);
                match hit {
                    Some(ch) => {
                        start += ch.len();
                        n_in -= ch.len();
                    }
                    None => break,
                }
            }
        }
        if flags & 2 != 0 {
            while n_in > 0 {
                let hit = az_char
                    .iter()
                    .find(|ch| ch.len() <= n_in && z_in[start + n_in - ch.len()..start + n_in] == ch[..]);
                match hit {
                    Some(ch) => n_in -= ch.len(),
                    None => break,
                }
            }
        }
    }
    api::result_text(context, Some(&z_in[start..start + n_in]), n_in as i32, SQLITE_TRANSIENT);
}

/// Núcleo de CONCAT(...) e CONCAT_WS(SEP,...): concatena todas as entradas não
/// nulas de argv[] usando z_sep como separador.
fn concat_func_core(context: &mut sqlite3_context, argc: i32, argv: &[MemRef], n_sep: i32, z_sep: &[u8]) {
    let mut n: i64 = 0;
    for i in 0..argc as usize {
        n += api::value_bytes(&argv[i]) as i64;
    }
    n += ((argc - 1) as i64) * (n_sep as i64);
    let mut z: Vec<u8> = Vec::with_capacity((n + 1).max(0) as usize);
    for i in 0..argc as usize {
        let k = api::value_bytes(&argv[i]) as usize;
        if k > 0 {
            if let Some(v) = api::value_text(&argv[i]) {
                if !z.is_empty() && n_sep > 0 {
                    z.extend_from_slice(&z_sep[..n_sep as usize]);
                }
                z.extend_from_slice(&v[..k]);
            }
        }
    }
    debug_assert!(z.len() as i64 <= n);
    api::result_text64(context, Some(&z), z.len() as u64, SQLITE_DYNAMIC, SQLITE_UTF8);
}


// ---- part_004.rs ----

/// A função CONCAT(...). Gera um texto que é a concatenação de todos os
/// argumentos não nulos.
fn concat_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    concat_func_core(context, argc, argv, 0, b"");
}

/// A função CONCAT_WS(separador, ...). Gera um texto que é a concatenação do
/// 2º ao N-ésimo argumento. O primeiro argumento (que não pode ser NULL) é o
/// separador.
fn concatws_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    let n_sep = api::value_bytes(&mut argv[0].borrow_mut());
    let z_sep = match api::value_text(&mut argv[0].borrow_mut()) {
        Some(z) => z,
        None => return,
    };
    concat_func_core(context, argc - 1, &argv[1..], n_sep, &z_sep);
}

/// Calcula a codificação soundex de uma palavra.
///
/// IMP: R-59782-00072 A função soundex(X) devolve o texto que é a codificação
/// soundex do texto X.
fn soundex_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
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
    debug_assert!(argc == 1);
    let mut z_in = api::value_text(&mut argv[0].borrow_mut()).unwrap_or_default();
    // O texto do C termina em zero: um NUL embutido encerra a leitura.
    z_in.push(0);
    let mut i = 0usize;
    while z_in[i] != 0 && !isalpha(z_in[i]) {
        i += 1;
    }
    if z_in[i] != 0 {
        let mut z_result = [0u8; 8];
        let mut prevcode = I_CODE[(z_in[i] & 0x7f) as usize];
        z_result[0] = toupper(z_in[i]);
        let mut j = 1usize;
        while j < 4 && z_in[i] != 0 {
            let code = I_CODE[(z_in[i] & 0x7f) as usize];
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
        z_result[j] = 0;
        api::result_text(context, Some(&z_result[..4]), 4, SQLITE_TRANSIENT);
    } else {
        // IMP: R-64894-50321 O texto "?000" é devolvido se o argumento é NULL
        // ou não contém caracteres alfabéticos ASCII.
        api::result_text(context, Some(b"?000"), 4, SQLITE_STATIC);
    }
}

/// Função que carrega uma extensão de biblioteca compartilhada e devolve NULL.
fn load_ext(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    let z_file = api::value_text(&mut argv[0].borrow_mut());
    let db = api::context_db_handle(context);
    let mut z_err_msg: Option<Vec<u8>> = None;

    // Proíbe a função SQL load_extension() a menos que a flag
    // SQLITE_LoadExtFunc esteja ligada. Veja sqlite3_enable_load_extension().
    if (db.borrow().flags & SQLITE_LOAD_EXT_FUNC) == 0 {
        api::result_error(context, b"not authorized", -1);
        return;
    }

    let z_proc = if argc == 2 {
        api::value_text(&mut argv[1].borrow_mut())
    } else {
        None
    };
    if let Some(z_file) = z_file {
        if api::load_extension(&db, &z_file, z_proc.as_deref(), &mut z_err_msg) != 0 {
            let msg = z_err_msg.take().unwrap_or_default();
            api::result_error(context, &msg, -1);
        }
    }
}

/// Guarda o contexto de uma agregação sum() ou avg().
#[derive(Default, Clone)]
pub struct SumCtx {
    /// Soma corrente como double.
    pub r_sum: f64,
    /// Termo de erro da soma de Kahan-Babushka-Neumaier.
    pub r_err: f64,
    /// Soma corrente como inteiro com sinal.
    pub i_sum: i64,
    /// Número de elementos somados.
    pub cnt: i64,
    /// Verdadeiro se algum valor não inteiro entrou na soma.
    pub approx: u8,
    /// Estouro de inteiro visto.
    pub ovrfl: u8,
}

/// Um passo da soma de Kahan-Babushka-Neumaier.
///
/// https://en.wikipedia.org/wiki/Kahan_summation_algorithm
///
/// No C as variáveis são "volatile" para impedir otimizações de ponto
/// flutuante que estragam o algoritmo; em Rust cada operação f64 é exata e
/// não é recombinada.
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

/// Soma um inteiro (possivelmente grande) à soma corrente.
fn kahan_babuska_neumaier_step_int64(p_sum: &mut SumCtx, i_val: i64) {
    if i_val <= -4503599627370496i64 || i_val >= 4503599627370496i64 {
        let i_sm = i_val % 16384;
        let i_big = i_val - i_sm;
        kahan_babuska_neumaier_step(p_sum, i_big as f64);
        kahan_babuska_neumaier_step(p_sum, i_sm as f64);
    } else {
        kahan_babuska_neumaier_step(p_sum, i_val as f64);
    }
}

/// Inicializa a soma de Kahan-Babaska-Neumaier a partir de um inteiro de 64 bits.
fn kahan_babuska_neumaier_init(p: &mut SumCtx, i_val: i64) {
    if i_val <= -4503599627370496i64 || i_val >= 4503599627370496i64 {
        let i_sm = i_val % 16384;
        p.r_sum = (i_val - i_sm) as f64;
        p.r_err = i_sm as f64;
    } else {
        p.r_sum = i_val as f64;
        p.r_err = 0.0;
    }
}

/// Rotinas usadas para calcular sum, avg e total.
///
/// SUM() segue o padrão SQL (quebrado): devolve NULL se não soma nenhuma
/// entrada. TOTAL devolve 0.0 nesse caso. TOTAL sempre devolve float, enquanto
/// SUM pode devolver inteiro se nunca vê um ponto flutuante. TOTAL nunca falha,
/// mas SUM pode lançar exceção se estourar um inteiro.
fn sum_step(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    debug_assert!(argc == 1);
    let p = api::aggregate_state::<SumCtx>(context, true);
    let type_ = api::value_numeric_type(&mut argv[0].borrow_mut());
    if let Some(p) = p {
        if type_ != SQLITE_NULL {
            let mut p = p.borrow_mut();
            p.cnt += 1;
            if p.approx == 0 {
                if type_ != SQLITE_INTEGER {
                    let i_sum = p.i_sum;
                    kahan_babuska_neumaier_init(&mut p, i_sum);
                    p.approx = 1;
                    kahan_babuska_neumaier_step(&mut p, api::value_double(&argv[0].borrow()));
                } else {
                    let mut x = p.i_sum;
                    if add_int64(&mut x, api::value_int64(&argv[0].borrow())) == 0 {
                        p.i_sum = x;
                    } else {
                        p.ovrfl = 1;
                        let i_sum = p.i_sum;
                        kahan_babuska_neumaier_init(&mut p, i_sum);
                        p.approx = 1;
                        kahan_babuska_neumaier_step_int64(&mut p, api::value_int64(&argv[0].borrow()));
                    }
                }
            } else if type_ == SQLITE_INTEGER {
                kahan_babuska_neumaier_step_int64(&mut p, api::value_int64(&argv[0].borrow()));
            } else {
                p.ovrfl = 0;
                kahan_babuska_neumaier_step(&mut p, api::value_double(&argv[0].borrow()));
            }
        }
    }
}

/// Passo inverso de sum()/avg()/total() para funções de janela.
fn sum_inverse(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    debug_assert!(argc == 1);
    let p = api::aggregate_state::<SumCtx>(context, true);
    let type_ = api::value_numeric_type(&mut argv[0].borrow_mut());
    // p nunca é NULL porque sum_step() terá rodado antes para inicializá-lo.
    if let Some(p) = p {
        if type_ != SQLITE_NULL {
            let mut p = p.borrow_mut();
            debug_assert!(p.cnt > 0);
            p.cnt -= 1;
            if p.approx == 0 {
                p.i_sum = p.i_sum.wrapping_sub(api::value_int64(&argv[0].borrow()));
            } else if type_ == SQLITE_INTEGER {
                let i_val = api::value_int64(&argv[0].borrow());
                if i_val != SMALLEST_INT64 {
                    kahan_babuska_neumaier_step_int64(&mut p, -i_val);
                } else {
                    kahan_babuska_neumaier_step_int64(&mut p, LARGEST_INT64);
                    kahan_babuska_neumaier_step_int64(&mut p, 1);
                }
            } else {
                kahan_babuska_neumaier_step(&mut p, -api::value_double(&argv[0].borrow()));
            }
        }
    }
}

/// Finaliza sum() e o valor corrente de sum() em janelas.
fn sum_finalize(context: &mut Sqlite3Context) {
    let p = api::aggregate_state::<SumCtx>(context, false);
    if let Some(p) = p {
        let p = p.borrow().clone();
        if p.cnt > 0 {
            if p.approx != 0 {
                if p.ovrfl != 0 {
                    api::result_error(context, b"integer overflow", -1);
                } else if !is_overflow(p.r_err) {
                    api::result_double(context, p.r_sum + p.r_err);
                } else {
                    api::result_double(context, p.r_sum);
                }
            } else {
                api::result_int64(context, p.i_sum);
            }
        }
    }
}

/// Finaliza avg().
fn avg_finalize(context: &mut Sqlite3Context) {
    let p = api::aggregate_state::<SumCtx>(context, false);
    if let Some(p) = p {
        let p = p.borrow().clone();
        if p.cnt > 0 {
            let r;
            if p.approx != 0 {
                let mut r0 = p.r_sum;
                if !is_overflow(p.r_err) {
                    r0 += p.r_err;
                }
                r = r0;
            } else {
                r = p.i_sum as f64;
            }
            api::result_double(context, r / p.cnt as f64);
        }
    }
}

/// Finaliza total().
fn total_finalize(context: &mut Sqlite3Context) {
    let mut r = 0.0f64;
    let p = api::aggregate_state::<SumCtx>(context, false);
    if let Some(p) = p {
        let p = p.borrow().clone();
        if p.approx != 0 {
            r = p.r_sum;
            if !is_overflow(p.r_err) {
                r += p.r_err;
            }
        } else {
            r = p.i_sum as f64;
        }
    }
    api::result_double(context, r);
}

/// Guarda o estado da agregação count().
#[derive(Default, Clone)]
pub struct CountCtx {
    pub n: i64,
}

/// Rotinas da agregação count().
fn count_step(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    let p = api::aggregate_state::<CountCtx>(context, true);
    if argc == 0 || SQLITE_NULL != api::value_type(&argv[0].borrow()) {
        if let Some(p) = p {
            p.borrow_mut().n += 1;
        }
    }
    // O assert sobre sqlite3_aggregate_count() é SQLITE_DEBUG: removido.
}


// ---- part_005.rs ----

/// Finaliza a agregação count() e o valor corrente de count() em janelas.
fn count_finalize(context: &mut Sqlite3Context) {
    let p = api::aggregate_state::<CountCtx>(context, false);
    let n = match p {
        Some(p) => p.borrow().n,
        None => 0,
    };
    api::result_int64(context, n);
}

/// Passo inverso de count() para funções de janela.
fn count_inverse(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    let p = api::aggregate_state::<CountCtx>(context, true);
    // p nunca é NULL, pois count_step() terá rodado antes.
    if argc == 0 || SQLITE_NULL != api::value_type(&argv[0].borrow()) {
        if let Some(p) = p {
            p.borrow_mut().n -= 1;
        }
    }
}

/// Rotinas das agregações min() e max().
fn minmax_step(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let p_arg = &argv[0];

    // pBest é o Mem do acumulador (no C, o próprio contexto de agregação).
    let p_best = match api::aggregate_state::<Mem>(context, true) {
        Some(p) => p,
        None => return,
    };

    if api::value_type(&p_arg.borrow()) == SQLITE_NULL {
        if p_best.borrow().flags != 0 {
            skip_accumulator_load(context);
        }
    } else if p_best.borrow().flags != 0 {
        // Este passo serve a min() e a max(); a única diferença é o sentido da
        // comparação. Para max(), sqlite3_user_data() devolve (void*)-1; para
        // min() devolve db. Logo 'max' vale 1 para max() e 0 para min().
        let p_coll = get_func_coll_seq(context);
        let max = api::user_data(context).is_some();
        let cmp = mem_compare(&p_best.borrow(), &p_arg.borrow(), Some(&p_coll.borrow()));
        if (max && cmp < 0) || (!max && cmp > 0) {
            vdbe_mem_copy(&mut p_best.borrow_mut(), &p_arg.borrow());
        } else {
            skip_accumulator_load(context);
        }
    } else {
        let db = api::context_db_handle(context);
        p_best.borrow_mut().db = Some(Rc::downgrade(&db));
        vdbe_mem_copy(&mut p_best.borrow_mut(), &p_arg.borrow());
    }
}

/// Núcleo de min_max_finalize() e min_max_value().
fn min_max_value_finalize(context: &mut Sqlite3Context, b_value: i32) {
    if let Some(p_res) = api::aggregate_state::<Mem>(context, false) {
        if p_res.borrow().flags != 0 {
            api::result_value(context, &p_res.borrow());
        }
        if b_value == 0 {
            vdbe_mem_release(&mut p_res.borrow_mut());
        }
    }
}

/// Valor corrente de min()/max() em função de janela.
fn min_max_value(context: &mut Sqlite3Context) {
    min_max_value_finalize(context, 1);
}

/// Finaliza min()/max(), liberando o acumulador.
fn min_max_finalize(context: &mut Sqlite3Context) {
    min_max_value_finalize(context, 0);
}

/// group_concat(EXPR, ?SEPARADOR?)
/// string_agg(EXPR, SEPARADOR)
///
/// O SEPARADOR vem antes do texto de EXPR. Isso é trágico, mas o comportamento
/// antigo existe há tanto tempo que não se ousa mudá-lo.
#[derive(Default)]
pub struct GroupConcatCtx {
    /// A concatenação acumulada.
    pub str: StrAccum,
    /// Número de textos concatenados no momento.
    pub n_accum: i32,
    /// Usado para detectar mudança no comprimento do separador.
    pub n_first_sep_length: i32,
    /// Se Some, guarda os comprimentos dos separadores entre textos, como
    /// efetivamente incorporados ao resultado acumulado (logo, n_accum-1
    /// posições em uso entre chamadas de método). Se None, n_first_sep_length
    /// é o comprimento usado em todo o resultado.
    pub pn_sep_lengths: Option<Vec<i32>>,
}

fn group_concat_step(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    debug_assert!(argc == 1 || argc == 2);
    if api::value_type(&argv[0].borrow()) == SQLITE_NULL {
        return;
    }
    let p_gcc = match api::aggregate_state::<GroupConcatCtx>(context, true) {
        Some(p) => p,
        None => return,
    };
    let mut p_gcc = p_gcc.borrow_mut();
    let db = api::context_db_handle(context);
    let first_term = p_gcc.str.mx_alloc == 0;
    p_gcc.str.mx_alloc = db.borrow().a_limit[SQLITE_LIMIT_LENGTH as usize] as _;
    if argc == 1 {
        if !first_term {
            str_appendchar(&mut p_gcc.str, 1, b',');
        } else {
            p_gcc.n_first_sep_length = 1;
        }
    } else if !first_term {
        let z_sep = api::value_text(&mut argv[1].borrow_mut());
        let mut n_sep = api::value_bytes(&mut argv[1].borrow_mut());
        if let Some(z_sep) = z_sep {
            str_append(&mut p_gcc.str, &z_sep, n_sep);
        } else {
            n_sep = 0;
        }
        if n_sep != p_gcc.n_first_sep_length || p_gcc.pn_sep_lengths.is_some() {
            let n_accum = p_gcc.n_accum;
            let pnsl = match p_gcc.pn_sep_lengths.take() {
                None => {
                    // Primeira variação de comprimento de separador: começa a rastreá-los.
                    let mut v = vec![0i32; (n_accum + 1) as usize];
                    let n_a = n_accum - 1;
                    let mut i = 0;
                    while i < n_a {
                        v[i as usize] = p_gcc.n_first_sep_length;
                        i += 1;
                    }
                    v
                }
                Some(mut v) => {
                    v.resize(n_accum as usize, 0);
                    v
                }
            };
            let mut pnsl = pnsl;
            if n_accum > 0 {
                pnsl[(n_accum - 1) as usize] = n_sep;
            }
            p_gcc.pn_sep_lengths = Some(pnsl);
            // Falha de alocação (SQLITE_NOMEM) não existe com Vec.
        }
    } else {
        p_gcc.n_first_sep_length = api::value_bytes(&mut argv[1].borrow_mut());
    }
    p_gcc.n_accum += 1;
    let z_val = api::value_text(&mut argv[0].borrow_mut());
    let n_val = api::value_bytes(&mut argv[0].borrow_mut());
    if let Some(z_val) = z_val {
        str_append(&mut p_gcc.str, &z_val, n_val);
    }
}

fn group_concat_inverse(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len() as i32;
    debug_assert!(argc == 1 || argc == 2);
    if api::value_type(&argv[0].borrow()) == SQLITE_NULL {
        return;
    }
    // p_gcc nunca é NULL pois group_concat_step() sempre roda primeiro.
    if let Some(p_gcc) = api::aggregate_state::<GroupConcatCtx>(context, true) {
        let mut p_gcc = p_gcc.borrow_mut();
        // É preciso chamar value_text() para converter o argumento em texto
        // antes de value_bytes(), caso a codificação seja UTF16.
        let _ = api::value_text(&mut argv[0].borrow_mut());
        let mut n_vs = api::value_bytes(&mut argv[0].borrow_mut());
        p_gcc.n_accum -= 1;
        if p_gcc.pn_sep_lengths.is_some() {
            debug_assert!(p_gcc.n_accum >= 0);
            if p_gcc.n_accum > 0 {
                let n_accum = p_gcc.n_accum as usize;
                let pnsl = p_gcc.pn_sep_lengths.as_mut().unwrap();
                n_vs += pnsl[0];
                pnsl.copy_within(1..n_accum, 0);
            }
        } else {
            // Ao remover o único texto acumulado, exagera sem dano.
            n_vs += p_gcc.n_first_sep_length;
        }
        if n_vs >= p_gcc.str.n_char as i32 {
            p_gcc.str.n_char = 0;
        } else {
            p_gcc.str.n_char -= n_vs as u32;
            let n_char = p_gcc.str.n_char as usize;
            let n_vs = n_vs as usize;
            p_gcc.str.z_text.copy_within(n_vs..n_vs + n_char, 0);
        }
        if p_gcc.str.n_char == 0 {
            p_gcc.str.mx_alloc = 0;
            p_gcc.pn_sep_lengths = None;
        }
    }
}

fn group_concat_finalize(context: &mut Sqlite3Context) {
    if let Some(p_gcc) = api::aggregate_state::<GroupConcatCtx>(context, false) {
        let mut p_gcc = p_gcc.borrow_mut();
        result_str_accum(context, &mut p_gcc.str);
        p_gcc.pn_sep_lengths = None;
    }
}

fn group_concat_value(context: &mut Sqlite3Context) {
    if let Some(p_gcc) = api::aggregate_state::<GroupConcatCtx>(context, false) {
        let p_gcc = p_gcc.borrow();
        let p_accum = &p_gcc.str;
        if p_accum.acc_error as i32 == SQLITE_TOOBIG {
            api::result_error_toobig(context);
        } else if p_accum.acc_error as i32 == SQLITE_NOMEM {
            api::result_error_nomem(context);
        } else if p_gcc.n_accum > 0 && p_accum.n_char == 0 {
            api::result_text(context, Some(b""), 1, SQLITE_STATIC);
        } else {
            let z_text = str_value(Some(p_accum));
            api::result_text(context, z_text.as_deref(), p_accum.n_char as i32, SQLITE_TRANSIENT);
        }
    }
}

/// Faz o registro de funções por conexão. A maioria das funções embutidas
/// acima faz parte do conjunto global; esta rotina trata só as que não são.
pub fn register_per_connection_builtin_functions(db: &Sqlite3Ref) {
    let rc = api::overload_function(db, b"MATCH", 2);
    debug_assert!(rc == SQLITE_NOMEM || rc == SQLITE_OK);
    if rc == SQLITE_NOMEM {
        oom_fault(&mut db.borrow_mut());
    }
}

/// Registra de novo as funções LIKE embutidas. O parâmetro case_sensitive
/// determina se o operador LIKE diferencia maiúsculas de minúsculas.
pub fn register_like_functions(db: &Sqlite3Ref, case_sensitive: i32) {
    let (p_info, flags) = if case_sensitive != 0 {
        (&LIKE_INFO_ALT, SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE)
    } else {
        (&LIKE_INFO_NORM, SQLITE_FUNC_LIKE)
    };
    for n_arg in 2..=3 {
        let info: CallbackArg = Some(Rc::new(CompareInfo {
            match_all: p_info.match_all,
            match_one: p_info.match_one,
            match_set: p_info.match_set,
            no_case: p_info.no_case,
        }));
        create_func(
            db, Some(b"like"), n_arg, SQLITE_UTF8, info,
            Some(Rc::new(like_func)), None, None, None, None, None,
        );
        let p_def = find_function(db, b"like", n_arg, SQLITE_UTF8, false);
        if let Some(p_def) = p_def {
            let mut p_def = p_def.borrow_mut();
            p_def.func_flags |= flags;
            p_def.func_flags &= !SQLITE_FUNC_UNSAFE;
        }
    }
}

/// p_expr aponta para uma expressão que implementa uma função. Se for
/// apropriado aplicar a otimização de LIKE a essa função, grava em a_wc[0] a
/// a_wc[2] os curingas e o caractere de escape e devolve 1. Se a função não é
/// do estilo LIKE, devolve 0.
///
/// A expressão "a LIKE b ESCAPE c" só vale como operador LIKE se c é um literal
/// de texto de exatamente um byte. Esse byte vai em a_wc[3]; a_wc[3] é zero se
/// não há ESCAPE.
///
/// *p_is_nocase é verdadeiro se maiúsculas e minúsculas são equivalentes para a
/// função (padrão do LIKE). Se a função as distingue (como GLOB), é falso.
pub fn is_like_function(db: &Sqlite3Ref, p_expr: &Expr, p_is_nocase: &mut i32, a_wc: &mut [u8; 4]) -> i32 {
    debug_assert!(p_expr.op == TK_FUNCTION);
    debug_assert!(expr_use_x_list(p_expr));
    let p_list = match p_expr.x.p_list.as_ref() {
        Some(l) => l,
        None => return 0,
    };
    let n_expr = p_list.n_expr;
    debug_assert!(!expr_has_property(p_expr, EP_INT_VALUE));
    let z_token = p_expr.u.z_token.as_deref().unwrap_or(b"");
    let p_def = match find_function(db, z_token, n_expr, SQLITE_UTF8, false) {
        Some(d) => d,
        None => return 0,
    };
    let p_def = p_def.borrow();
    if (p_def.func_flags & SQLITE_FUNC_LIKE) == 0 {
        return 0;
    }

    // No C, o memcpy supõe que os curingas são os três primeiros campos de
    // compareInfo (matchAll, matchOne, matchSet).
    let info = match p_def.p_user_data.as_ref().and_then(|d| d.downcast_ref::<CompareInfo>()) {
        Some(i) => i,
        None => return 0,
    };
    a_wc[0] = info.match_all;
    a_wc[1] = info.match_one;
    a_wc[2] = info.match_set;

    if n_expr < 3 {
        a_wc[3] = 0;
    } else {
        let p_escape = match p_list.a[2].p_expr.as_ref() {
            Some(e) => e,
            None => return 0,
        };
        if p_escape.op != TK_STRING {
            return 0;
        }
        debug_assert!(!expr_has_property(p_escape, EP_INT_VALUE));
        let z_escape = p_escape.u.z_token.as_deref().unwrap_or(b"");
        if z_escape.is_empty() || z_escape[0] == 0 || (z_escape.len() > 1 && z_escape[1] != 0) {
            return 0;
        }
        if z_escape[0] == a_wc[0] {
            return 0;
        }
        if z_escape[0] == a_wc[1] {
            return 0;
        }
        a_wc[3] = z_escape[0];
    }

    *p_is_nocase = ((p_def.func_flags & SQLITE_FUNC_CASE) == 0) as i32;
    1
}


// ---- part_006.rs ----

/// Constantes matemáticas.
pub const M_PI: f64 = 3.141592653589793238462643383279502884;
pub const M_LN10: f64 = 2.302585092994045684017991454684364208;
pub const M_LN2: f64 = 0.693147180559945309417232121458176568;

/// Função de uma variável da libm guardada como dado do usuário.
type MathFn1 = fn(f64) -> f64;
/// Função de duas variáveis da libm guardada como dado do usuário.
type MathFn2 = fn(f64, f64) -> f64;

/// Implementação das funções SQL:
///
///   ceil(X)
///   ceiling(X)
///   floor(X)
///
/// O dado do usuário (sqlite3_user_data()) é a implementação da libm da
/// função C subjacente.
fn ceiling_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    debug_assert!(argv.len() == 1);
    match api::value_numeric_type(&mut argv[0].borrow_mut()) {
        SQLITE_INTEGER => {
            let v = api::value_int64(&argv[0].borrow());
            api::result_int64(context, v);
        }
        SQLITE_FLOAT => {
            let x = api::user_data(context)
                .and_then(|d| d.downcast_ref::<MathFn1>().copied())
                .expect("pUserData de ceiling");
            let v = api::value_double(&argv[0].borrow());
            api::result_double(context, x(v));
        }
        _ => {}
    }
}

/// Em alguns sistemas ceil() e floor() são intrínsecas e não se pode tomar um
/// ponteiro para elas. Por isso são embrulhadas em funções reais.
fn x_ceil(x: f64) -> f64 {
    x.ceil()
}
fn x_floor(x: f64) -> f64 {
    x.floor()
}

/// Implementação das funções SQL:
///
///   ln(X)       - logaritmo natural
///   log(X)      - log de X na base 10
///   log10(X)    - log de X na base 10
///   log(B,X)    - log de X na base B
fn log_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    let argc = argv.len();
    debug_assert!(argc == 1 || argc == 2);
    let mut x: f64;
    match api::value_numeric_type(&mut argv[0].borrow_mut()) {
        SQLITE_INTEGER | SQLITE_FLOAT => {
            x = api::value_double(&argv[0].borrow());
            if x <= 0.0 {
                return;
            }
        }
        _ => return,
    }
    let ans;
    if argc == 2 {
        // O C testa de novo o tipo de argv[0] (e não o de argv[1]); mantido.
        let b: f64;
        match api::value_numeric_type(&mut argv[0].borrow_mut()) {
            SQLITE_INTEGER | SQLITE_FLOAT => {
                b = x.ln();
                if b <= 0.0 {
                    return;
                }
                x = api::value_double(&argv[1].borrow());
                if x <= 0.0 {
                    return;
                }
            }
            _ => return,
        }
        ans = x.ln() / b;
    } else {
        let sel = api::user_data(context)
            .and_then(|d| d.downcast_ref::<isize>().copied())
            .unwrap_or(0);
        ans = match sel {
            1 => x.log10(),
            2 => x.log2(),
            _ => x.ln(),
        };
    }
    api::result_double(context, ans);
}

/// Converte graus em radianos e radianos em graus.
fn deg_to_rad(x: f64) -> f64 {
    x * (M_PI / 180.0)
}
fn rad_to_deg(x: f64) -> f64 {
    x * (180.0 / M_PI)
}

/// Implementação das funções SQL matemáticas de 1 argumento:
///
///   exp(X)  - e elevado à X-ésima potência
fn math1_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    debug_assert!(argv.len() == 1);
    let type0 = api::value_numeric_type(&mut argv[0].borrow_mut());
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let v0 = api::value_double(&argv[0].borrow());
    let x = api::user_data(context)
        .and_then(|d| d.downcast_ref::<MathFn1>().copied())
        .expect("pUserData de math1");
    let ans = x(v0);
    api::result_double(context, ans);
}

/// Implementação das funções SQL matemáticas de 2 argumentos:
///
///   power(X,Y)  - X elevado à Y-ésima potência
fn math2_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    debug_assert!(argv.len() == 2);
    let type0 = api::value_numeric_type(&mut argv[0].borrow_mut());
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let type1 = api::value_numeric_type(&mut argv[1].borrow_mut());
    if type1 != SQLITE_INTEGER && type1 != SQLITE_FLOAT {
        return;
    }
    let v0 = api::value_double(&argv[0].borrow());
    let v1 = api::value_double(&argv[1].borrow());
    let x = api::user_data(context)
        .and_then(|d| d.downcast_ref::<MathFn2>().copied())
        .expect("pUserData de math2");
    let ans = x(v0, v1);
    api::result_double(context, ans);
}

/// Implementação da função pi(), sem argumentos.
fn pi_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    debug_assert!(argv.is_empty());
    api::result_double(context, M_PI);
}

/// Implementação da função sign(X).
fn sign_func(context: &mut Sqlite3Context, argv: &[MemRef]) {
    debug_assert!(argv.len() == 1);
    let type0 = api::value_numeric_type(&mut argv[0].borrow_mut());
    if type0 != SQLITE_INTEGER && type0 != SQLITE_FLOAT {
        return;
    }
    let x = api::value_double(&argv[0].borrow());
    api::result_int(context, if x < 0.0 { -1 } else if x > 0.0 { 1 } else { 0 });
}

/// Todas as estruturas FuncDef do array a_builtin_func abaixo vão para a
/// tabela hash global de funções. Isso ocorre no início (como consequência de
/// chamar sqlite3_initialize()).
pub fn register_builtin_functions() {
    fn m1(f: MathFn1) -> CallbackArg {
        Some(Rc::new(f))
    }
    fn m2(f: MathFn2) -> CallbackArg {
        Some(Rc::new(f))
    }
    let like_arg = |i: &CompareInfo| -> CallbackArg {
        Some(Rc::new(CompareInfo {
            match_all: i.match_all,
            match_one: i.match_one,
            match_set: i.match_set,
            no_case: i.no_case,
        }))
    };
    // Para eficiência máxima, a função mais usada vai por último.
    let a_builtin_func: Vec<FuncDef> = vec![
        // Funções disponíveis só com SQLITE_TESTCTRL_INTERNAL_FUNCTIONS.
        test_func("implies_nonnull_row", 2, INLINEFUNC_IMPLIES_NONNULL_ROW, 0),
        test_func("expr_compare", 2, INLINEFUNC_EXPR_COMPARE, 0),
        test_func("expr_implies_expr", 2, INLINEFUNC_EXPR_IMPLIES_EXPR, 0),
        test_func("affinity", 1, INLINEFUNC_AFFINITY, 0),
        // Funções regulares.
        function("soundex", 1, 0, 0, Rc::new(soundex_func)),
        sfunction("load_extension", 1, 0, 0, Rc::new(load_ext)),
        sfunction("load_extension", 2, 0, 0, Rc::new(load_ext)),
        dfunction("sqlite_compileoption_used", 1, 0, 0, Rc::new(compileoption_used_func)),
        dfunction("sqlite_compileoption_get", 1, 0, 0, Rc::new(compileoption_get_func)),
        inline_func("unlikely", 1, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY),
        inline_func("likelihood", 2, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY),
        inline_func("likely", 1, INLINEFUNC_UNLIKELY, SQLITE_FUNC_UNLIKELY),
        function("ltrim", 1, 1, 0, Rc::new(trim_func)),
        function("ltrim", 2, 1, 0, Rc::new(trim_func)),
        function("rtrim", 1, 2, 0, Rc::new(trim_func)),
        function("rtrim", 2, 2, 0, Rc::new(trim_func)),
        function("trim", 1, 3, 0, Rc::new(trim_func)),
        function("trim", 2, 3, 0, Rc::new(trim_func)),
        function("min", -1, 0, 1, Rc::new(minmax_func)),
        function0("min", 0, 0, 1),
        waggregate(
            "min", 1, 0, 1, Rc::new(minmax_step), Rc::new(min_max_finalize),
            Some(Rc::new(min_max_value)), None,
            SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
        ),
        function("max", -1, 1, 1, Rc::new(minmax_func)),
        function0("max", 0, 1, 1),
        waggregate(
            "max", 1, 1, 1, Rc::new(minmax_step), Rc::new(min_max_finalize),
            Some(Rc::new(min_max_value)), None,
            SQLITE_FUNC_MINMAX | SQLITE_FUNC_ANYORDER,
        ),
        function2("typeof", 1, 0, 0, Rc::new(typeof_func), SQLITE_FUNC_TYPEOF),
        function2("subtype", 1, 0, 0, Rc::new(subtype_func), SQLITE_FUNC_TYPEOF),
        function2("length", 1, 0, 0, Rc::new(length_func), SQLITE_FUNC_LENGTH),
        function2("octet_length", 1, 0, 0, Rc::new(bytelength_func), SQLITE_FUNC_BYTELEN),
        function("instr", 2, 0, 0, Rc::new(instr_func)),
        function("printf", -1, 0, 0, Rc::new(printf_func)),
        function("format", -1, 0, 0, Rc::new(printf_func)),
        function("unicode", 1, 0, 0, Rc::new(unicode_func)),
        function("char", -1, 0, 0, Rc::new(char_func)),
        function("abs", 1, 0, 0, Rc::new(abs_func)),
        function("round", 1, 0, 0, Rc::new(round_func)),
        function("round", 2, 0, 0, Rc::new(round_func)),
        function("upper", 1, 0, 0, Rc::new(upper_func)),
        function("lower", 1, 0, 0, Rc::new(lower_func)),
        function("hex", 1, 0, 0, Rc::new(hex_func)),
        function("unhex", 1, 0, 0, Rc::new(unhex_func)),
        function("unhex", 2, 0, 0, Rc::new(unhex_func)),
        function("concat", -1, 0, 0, Rc::new(concat_func)),
        function0("concat", 0, 0, 0),
        function("concat_ws", -1, 0, 0, Rc::new(concatws_func)),
        function0("concat_ws", 0, 0, 0),
        function0("concat_ws", 1, 0, 0),
        inline_func("ifnull", 2, INLINEFUNC_COALESCE, 0),
        vfunction("random", 0, 0, 0, Rc::new(random_func)),
        vfunction("randomblob", 1, 0, 0, Rc::new(random_blob)),
        function("nullif", 2, 0, 1, Rc::new(nullif_func)),
        dfunction("sqlite_version", 0, 0, 0, Rc::new(version_func)),
        dfunction("sqlite_source_id", 0, 0, 0, Rc::new(sourceid_func)),
        function("sqlite_log", 2, 0, 0, Rc::new(errlog_func)),
        function("quote", 1, 0, 0, Rc::new(quote_func)),
        vfunction("last_insert_rowid", 0, 0, 0, Rc::new(last_insert_rowid)),
        vfunction("changes", 0, 0, 0, Rc::new(changes)),
        vfunction("total_changes", 0, 0, 0, Rc::new(total_changes)),
        function("replace", 3, 0, 0, Rc::new(replace_func)),
        function("zeroblob", 1, 0, 0, Rc::new(zeroblob_func)),
        function("substr", 2, 0, 0, Rc::new(substr_func)),
        function("substr", 3, 0, 0, Rc::new(substr_func)),
        function("substring", 2, 0, 0, Rc::new(substr_func)),
        function("substring", 3, 0, 0, Rc::new(substr_func)),
        waggregate(
            "sum", 1, 0, 0, Rc::new(sum_step), Rc::new(sum_finalize),
            Some(Rc::new(sum_finalize)), Some(Rc::new(sum_inverse)), 0,
        ),
        waggregate(
            "total", 1, 0, 0, Rc::new(sum_step), Rc::new(total_finalize),
            Some(Rc::new(total_finalize)), Some(Rc::new(sum_inverse)), 0,
        ),
        waggregate(
            "avg", 1, 0, 0, Rc::new(sum_step), Rc::new(avg_finalize),
            Some(Rc::new(avg_finalize)), Some(Rc::new(sum_inverse)), 0,
        ),
        waggregate(
            "count", 0, 0, 0, Rc::new(count_step), Rc::new(count_finalize),
            Some(Rc::new(count_finalize)), Some(Rc::new(count_inverse)),
            SQLITE_FUNC_COUNT | SQLITE_FUNC_ANYORDER,
        ),
        waggregate(
            "count", 1, 0, 0, Rc::new(count_step), Rc::new(count_finalize),
            Some(Rc::new(count_finalize)), Some(Rc::new(count_inverse)),
            SQLITE_FUNC_ANYORDER,
        ),
        waggregate(
            "group_concat", 1, 0, 0, Rc::new(group_concat_step),
            Rc::new(group_concat_finalize), Some(Rc::new(group_concat_value)),
            Some(Rc::new(group_concat_inverse)), 0,
        ),
        waggregate(
            "group_concat", 2, 0, 0, Rc::new(group_concat_step),
            Rc::new(group_concat_finalize), Some(Rc::new(group_concat_value)),
            Some(Rc::new(group_concat_inverse)), 0,
        ),
        waggregate(
            "string_agg", 2, 0, 0, Rc::new(group_concat_step),
            Rc::new(group_concat_finalize), Some(Rc::new(group_concat_value)),
            Some(Rc::new(group_concat_inverse)), 0,
        ),
        likefunc("glob", 2, like_arg(&GLOB_INFO), SQLITE_FUNC_LIKE | SQLITE_FUNC_CASE),
        likefunc("like", 2, like_arg(&LIKE_INFO_NORM), SQLITE_FUNC_LIKE),
        likefunc("like", 3, like_arg(&LIKE_INFO_NORM), SQLITE_FUNC_LIKE),
        function0("coalesce", 1, 0, 0),
        function0("coalesce", 0, 0, 0),
        mfunction("ceil", 1, m1(x_ceil), Rc::new(ceiling_func)),
        mfunction("ceiling", 1, m1(x_ceil), Rc::new(ceiling_func)),
        mfunction("floor", 1, m1(x_floor), Rc::new(ceiling_func)),
        mfunction("trunc", 1, m1(f64::trunc), Rc::new(ceiling_func)),
        function("ln", 1, 0, 0, Rc::new(log_func)),
        function("log", 1, 1, 0, Rc::new(log_func)),
        function("log10", 1, 1, 0, Rc::new(log_func)),
        function("log2", 1, 2, 0, Rc::new(log_func)),
        function("log", 2, 0, 0, Rc::new(log_func)),
        mfunction("exp", 1, m1(f64::exp), Rc::new(math1_func)),
        mfunction("pow", 2, m2(f64::powf), Rc::new(math2_func)),
        mfunction("power", 2, m2(f64::powf), Rc::new(math2_func)),
        mfunction("mod", 2, m2(|a, b| a % b), Rc::new(math2_func)),
        mfunction("acos", 1, m1(f64::acos), Rc::new(math1_func)),
        mfunction("asin", 1, m1(f64::asin), Rc::new(math1_func)),
        mfunction("atan", 1, m1(f64::atan), Rc::new(math1_func)),
        mfunction("atan2", 2, m2(f64::atan2), Rc::new(math2_func)),
        mfunction("cos", 1, m1(f64::cos), Rc::new(math1_func)),
        mfunction("sin", 1, m1(f64::sin), Rc::new(math1_func)),
        mfunction("tan", 1, m1(f64::tan), Rc::new(math1_func)),
        mfunction("cosh", 1, m1(f64::cosh), Rc::new(math1_func)),
        mfunction("sinh", 1, m1(f64::sinh), Rc::new(math1_func)),
        mfunction("tanh", 1, m1(f64::tanh), Rc::new(math1_func)),
        mfunction("acosh", 1, m1(f64::acosh), Rc::new(math1_func)),
        mfunction("asinh", 1, m1(f64::asinh), Rc::new(math1_func)),
        mfunction("atanh", 1, m1(f64::atanh), Rc::new(math1_func)),
        mfunction("sqrt", 1, m1(f64::sqrt), Rc::new(math1_func)),
        mfunction("radians", 1, m1(deg_to_rad), Rc::new(math1_func)),
        mfunction("degrees", 1, m1(rad_to_deg), Rc::new(math1_func)),
        function("pi", 0, 0, 0, Rc::new(pi_func)),
        function("sign", 1, 0, 0, Rc::new(sign_func)),
        inline_func("coalesce", -1, INLINEFUNC_COALESCE, 0),
        inline_func("iif", 3, INLINEFUNC_IIF, 0),
    ];
    alter_functions();
    window_functions();
    register_date_time_functions();
    register_json_functions();
    let refs: Vec<FuncDefRef> = a_builtin_func
        .into_iter()
        .map(|d| Rc::new(RefCell::new(d)))
        .collect();
    insert_builtin_funcs(&refs, refs.len());
}


// ---- part_007.rs ----

// O trecho func.c.007 do C é vazio (só uma quebra de linha): nada a traduzir.

