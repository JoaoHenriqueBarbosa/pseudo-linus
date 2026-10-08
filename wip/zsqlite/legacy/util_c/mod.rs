// Mesclado das partes traduzidas de util_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Simula uma falha durante testes ou contorna a detecção normal de erros durante os testes
/// para permitir que a execução prossiga adiante.
///
/// Em implantação, `fault_sim()` sempre retorna `SQLITE_OK` (0). A função só retorna não zero
/// durante testes.
///
/// Durante os testes, se o arnês de testes instalou um callback de simulação de falha por meio
/// de `sqlite3_test_control(SQLITE_TESTCTRL_FAULT_INSTALL)`, cada chamada a `fault_sim()` é
/// passada para esse callback fornecido pelo aplicativo, e o valor inteiro retornado pelo
/// callback fornecido pelo aplicativo é retornado por `fault_sim()`.
///
/// O argumento inteiro para `fault_sim()` é um código que identifica qual instância de
/// `fault_sim()` está sendo invocada. Cada chamada a `fault_sim()` deve ter um código único.
/// Para evitar quebrar aplicações de testes legadas, os códigos não devem ser alterados ou reutilizados.
#[cfg(not(feature = "SQLITE_UNTESTABLE"))]
pub fn fault_sim(i_test: i32) -> i32 {
    if let Some(x_callback) = SQLITE_CONFIG.x_test_callback {
        x_callback(i_test)
    } else {
        SQLITE_OK
    }
}

/// Retorna verdadeiro se o valor de ponto flutuante é Não um Número (NaN).
///
/// Usa a função de biblioteca matemática `isnan()` se compilado com `SQLITE_HAVE_ISNAN`.
/// Caso contrário, temos nossa própria implementação que funciona na maioria dos sistemas.
#[cfg(not(feature = "SQLITE_OMIT_FLOATING_POINT"))]
pub fn is_nan(x: f64) -> bool {
    // Equivalente ao teste de bits do C (expoente todo 1 e mantissa não nula).
    x.is_nan()
}

/// Retorna verdadeiro se o valor de ponto flutuante é NaN ou +Inf ou -Inf.
#[cfg(not(feature = "SQLITE_OMIT_FLOATING_POINT"))]
pub fn is_overflow(x: f64) -> bool {
    // Equivalente a `IsOvfl(y)`: expoente todo 1.
    !x.is_finite()
}

/// Computa um comprimento de string limitado ao que pode ser armazenado nos 30 bits
/// inferiores de um inteiro assinado de 32 bits.
///
/// O valor retornado nunca será negativo. Tampouco será maior que o comprimento real
/// da string. Para strings muito longas (maiores que 1 GiB), o valor retornado pode ser
/// menor que o comprimento real da string.
pub fn strlen30(z: Option<&[u8]>) -> i32 {
    match z {
        None => 0,
        Some(z) => {
            let n = z.iter().position(|&b| b == 0).unwrap_or(z.len());
            (n & 0x3fff_ffff) as i32
        }
    }
}

/// Retorna o tipo declarado de uma coluna. Ou retorna `z_dflt` se a coluna não tiver tipo
/// declarado.
///
/// O tipo da coluna é uma string adicional armazenada após o terminador nulo no nome da
/// coluna se, e somente se, a flag `COLFLAG_HASTYPE` estiver definida.
pub fn column_type<'a>(col: &'a Column, z_dflt: Option<&'a [u8]>) -> Option<&'a [u8]> {
    if col.col_flags & COLFLAG_HASTYPE != 0 {
        // O tipo vem logo após o terminador nulo do nome, até o próximo terminador.
        let name_len = col.z_cn_name.iter().position(|&b| b == 0).unwrap_or(col.z_cn_name.len());
        let rest = &col.z_cn_name[(name_len + 1).min(col.z_cn_name.len())..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Some(&rest[..end])
    } else if col.e_c_type != 0 {
        debug_assert!((col.e_c_type as usize) <= SQLITE_N_STDTYPE);
        Some(SQLITE_STD_TYPE[(col.e_c_type as usize) - 1].as_ref())
    } else {
        z_dflt
    }
}

/// Função auxiliar para `error()`, chamada raramente. Separada em uma rotina distinta
/// para evitar economias desnecessárias de registrador na entrada para `error()`.
fn error_finish(db: &mut Sqlite3, err_code: i32) {
    if let Some(p_err) = &db.p_err {
        let mut err = p_err.borrow_mut();
        value_set_null(&mut err);
    }
    system_error(db, err_code);
}

/// Define o código de erro atual para `err_code` e limpa qualquer mensagem de erro anterior.
/// Também define `i_sys_errno` (chamando `system_error()`) se o `err_code` indicar que seria
/// apropriado.
pub fn error(db: &mut Sqlite3, err_code: i32) {
    db.err_code = err_code;
    if err_code != SQLITE_OK || db.p_err.is_some() {
        error_finish(db, err_code);
    } else {
        db.err_byte_offset = -1;
    }
}

/// O equivalente de `error(db, SQLITE_OK)`. Limpa o estado de erro e mensagem de erro.
pub fn error_clear(db: &mut Sqlite3) {
    db.err_code = SQLITE_OK;
    db.err_byte_offset = -1;
    if let Some(p_err) = &db.p_err {
        let mut err = p_err.borrow_mut();
        value_set_null(&mut err);
    }
}

/// Carrega o campo `db.i_sys_errno` se for apropriado fazer isso com base no código de erro
/// do SQLite em `rc`.
pub fn system_error(db: &mut Sqlite3, rc: i32) {
    if rc == SQLITE_IOERR_NOMEM {
        return;
    }
    #[cfg(all(feature = "SQLITE_USE_SEH", not(feature = "SQLITE_OMIT_WAL")))]
    {
        if rc == SQLITE_IOERR_IN_PAGE {
            btree_enter_all(db);
            for ii in 0..db.n_db {
                if let Some(p_bt) = &db.a_db[ii as usize].p_bt {
                    let i_err = pager_wal_system_errno(btree_pager(p_bt));
                    if i_err != 0 {
                        db.i_sys_errno = i_err;
                    }
                }
            }
            btree_leave_all(db);
            return;
        }
    }
    let rc_base = (rc & 0xff) as i32;
    if rc_base == SQLITE_CANTOPEN || rc_base == SQLITE_IOERR {
        if let Some(p_vfs) = &db.p_vfs {
            db.i_sys_errno = p_vfs.get_last_error();
        }
    }
}

/// Define o código de erro mais recente e a string de erro para o identificador sqlite `db`.
/// O código de erro está definido como `err_code`.
///
/// Se não for nulo, a string `z_format` especifica o formato da string de erro. `z_format` e
/// qualquer string de token que o siga são assumidas como codificadas em UTF-8.
///
/// Para limpar o erro mais recente para o identificador sqlite `db`, `error` deve ser chamada
/// com `err_code` definido como `SQLITE_OK` e `z_format` definido como nulo.
pub fn error_with_msg(db: &mut Sqlite3, err_code: i32, z_format: Option<&[u8]>, args: &[PrintfArg]) {
    db.err_code = err_code;
    system_error(db, err_code);
    match z_format {
        None => error(db, err_code),
        Some(fmt) => {
            if db.p_err.is_none() {
                db.p_err = value_new(db);
            }
            if db.p_err.is_some() {
                let z = v_mprintf(db, fmt, args);
                if let Some(p_err) = &db.p_err {
                    let mut err = p_err.borrow_mut();
                    value_set_str(&mut err, -1, z, SQLITE_UTF8, SQLITE_DYNAMIC);
                }
            }
        }
    }
}

/// Verifica interrupções e invoca callback de progresso.
pub fn progress_check(p: &mut Parse) {
    let db = p.db;
    if db.is_interrupted != 0 {
        p.n_err += 1;
        p.rc = SQLITE_INTERRUPT;
    }
    #[cfg(not(feature = "SQLITE_OMIT_PROGRESS_CALLBACK"))]
    {
        if let Some(x_progress) = db.x_progress {
            if p.rc == SQLITE_INTERRUPT {
                p.n_progress_steps = 0;
            } else {
                p.n_progress_steps += 1;
                if p.n_progress_steps >= db.n_progress_ops {
                    if x_progress(db.p_progress_arg) != 0 {
                        p.n_err += 1;
                        p.rc = SQLITE_INTERRUPT;
                    }
                    p.n_progress_steps = 0;
                }
            }
        }
    }
}

/// Adiciona uma mensagem de erro a `pParse.z_err_msg` e incrementa `pParse.n_err`.
///
/// Esta função deve ser usada para relatar qualquer erro que ocorra durante a compilação de
/// uma instrução SQL (ou seja, dentro de `sqlite3_prepare()`). A última coisa que a função
/// `sqlite3_prepare()` faz é copiar o erro armazenado por esta função para o identificador
/// de banco de dados usando `error()`. As funções `error()` ou `error_with_msg()` devem ser
/// usadas durante a execução de instruções (`sqlite3_step()` etc.).
pub fn error_msg(pparse: &mut Parse, z_format: &[u8], args: &[PrintfArg]) {
    let db = pparse.db;
    db.err_byte_offset = -2;
    let z_msg = v_mprintf(db, z_format, args);
    if db.err_byte_offset < -1 {
        db.err_byte_offset = -1;
    }
    if db.suppress_err != 0 {
        db_free(db, z_msg);
        if db.malloc_failed != 0 {
            pparse.n_err += 1;
            pparse.rc = SQLITE_NOMEM;
        }
    } else {
        pparse.n_err += 1;
        db_free(db, pparse.z_err_msg.take());
        pparse.z_err_msg = z_msg;
        pparse.rc = SQLITE_ERROR;
        pparse.p_with = None;
    }
}

/// Se a conexão de banco de dados `db` estiver analisando SQL, transfira o código de erro
/// `err_code` para esse analisador se o analisador ainda não encontrou algum outro tipo de erro.
pub fn error_to_parser(db: Option<&mut Sqlite3>, err_code: i32) -> i32 {
    match db {
        None => err_code,
        Some(db) => {
            if let Some(pparse) = db.p_parse.as_mut() {
                pparse.rc = err_code;
                pparse.n_err += 1;
            }
            err_code
        }
    }
}

/// Converte uma string entre aspas no estilo SQL em uma string normal removendo os caracteres
/// de aspas. A conversão é feita no local. Se a entrada não começar com um caractere de aspas,
/// essa rotina não faz nada.
///
/// A string de entrada deve ser terminada em zero. Um novo terminador nulo é adicionado à
/// string sem aspas.
///
/// Não há valor de retorno: se a dequotação ocorrer, a string é modificada no local.
///
/// 2002-02-14: Esta rotina foi estendida para remover colchetes do estilo MS-Access ao redor
/// de identificadores. Por exemplo: "[a-b-c]" se torna "a-b-c".
pub fn dequote(z: &mut Vec<u8>) {
    if z.is_empty() {
        return;
    }
    let quote = z[0];
    if !isquote(quote) {
        return;
    }
    let quote_char = if quote == b'[' { b']' } else { quote };
    let mut j = 0;
    let mut i = 1;
    while i < z.len() {
        if z[i] == quote_char {
            if i + 1 < z.len() && z[i + 1] == quote_char {
                z[j] = quote_char;
                j += 1;
                i += 2;
            } else {
                break;
            }
        } else {
            z[j] = z[i];
            j += 1;
            i += 1;
        }
    }
    z.truncate(j);
}

/// Dequota o token da expressão `p` (que deve começar com aspas) e marca as flags
/// `EP_QUOTED` (e `EP_DBLQUOTED` para aspas duplas).
pub fn dequote_expr(p: &mut Expr) {
    debug_assert!(!expr_has_property(p, EP_INTVALUE));
    if p.u.z_token[0] == b'"' {
        p.flags |= EP_QUOTED | EP_DBLQUOTED;
    } else {
        p.flags |= EP_QUOTED;
    }
    dequote(&mut p.u.z_token);
}

/// A expressão `p` é um `QNUMBER` (número entre aspas). Dequota o valor em `p.u.z_token`
/// e define o tipo para `INTEGER` ou `FLOAT`. Números entre aspas (inteiros ou floats) são
/// aqueles que contêm caracteres `_` que devem ser removidos antes do processamento adicional.
pub fn dequote_number(pparse: &mut Parse, p: Option<&mut Expr>) {
    let p = match p {
        Some(p) => p,
        None => return,
    };

    let p_in = p.u.z_token.clone();
    let mut p_out = Vec::new();
    let b_hex = p_in.len() >= 2 && p_in[0] == b'0' && (p_in[1] == b'x' || p_in[1] == b'X');

    p.op = TK_INTEGER;

    for (idx, &byte) in p_in.iter().enumerate() {
        if byte != SQLITE_DIGIT_SEPARATOR {
            p_out.push(byte);
            if byte == b'e' || byte == b'E' || byte == b'.' {
                p.op = TK_FLOAT;
            }
        } else {
            let prev_ok = if idx > 0 {
                if b_hex {
                    isxdigit(p_in[idx - 1])
                } else {
                    isdigit(p_in[idx - 1])
                }
            } else {
                false
            };
            let next_ok = if idx + 1 < p_in.len() {
                if b_hex {
                    isxdigit(p_in[idx + 1])
                } else {
                    isdigit(p_in[idx + 1])
                }
            } else {
                false
            };

            if !prev_ok || !next_ok {
                error_msg(
                    pparse,
                    b"unrecognized token: \"%s\"",
                    &[PrintfArg::Str(p_in.clone())],
                );
            }
        }
    }

    p.u.z_token = p_out;

    if b_hex {
        p.op = TK_INTEGER;
    }

    // tag-20240227-a: Se após dequotação, o número é um inteiro que cabe em 32 bits,
    // ele deve ser convertido em `EP_INTVALUE`. Outras partes do código esperam isso.
    // Veja também tag-20240227-b.
    if p.op == TK_INTEGER {
        let mut i_value = 0i32;
        if get_int32(&p.u.z_token, &mut i_value) != 0 {
            p.u.i_value = i_value;
            p.flags |= EP_INTVALUE;
        }
    }
}


// ---- part_001.rs ----

/// Se o token de entrada `p` é entre aspas, tenta ajustar o token para remover as aspas.
/// Isso nem sempre é possível:
///
///     "abc"     ->   abc
///     "ab""cd"  ->   (impossível por causa do "" interno)
///
/// Remove as aspas se possível. Isto é uma otimização. O sistema como um todo deve retornar
/// a resposta correta mesmo que esta rotina seja sempre um não-op.
pub fn dequote_token(p: &mut Token) {
    if p.n < 2 {
        return;
    }
    if !isquote(p.z[0]) {
        return;
    }
    for i in 1..(p.n as usize - 1) {
        if isquote(p.z[i]) {
            return;
        }
    }
    p.n -= 2;
    p.z.remove(0);
}

/// Gera um objeto `Token` a partir de uma string.
pub fn token_init(p: &mut Token, z: &[u8]) {
    p.z = z.to_vec();
    p.n = strlen30(Some(z)) as u32;
}

/// Comparação sem diferenciar maiúsculas de minúsculas (`sqlite3_stricmp`), com tratamento
/// de ponteiro nulo. Usa a mesma definição de "independência de caixa" que o SQLite usa
/// internamente ao comparar identificadores.
pub fn stricmp(z_left: Option<&[u8]>, z_right: Option<&[u8]>) -> i32 {
    match (z_left, z_right) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(a), Some(b)) => str_i_cmp(a, b),
    }
}

/// Comparação sem diferenciar maiúsculas de minúsculas (`sqlite3StrICmp`). As strings terminam
/// no primeiro zero ou no fim do slice.
pub fn str_i_cmp(z_left: &[u8], z_right: &[u8]) -> i32 {
    let mut i = 0usize;
    loop {
        let mut c = z_left.get(i).copied().unwrap_or(0) as i32;
        let x = z_right.get(i).copied().unwrap_or(0) as i32;
        if c == x {
            if c == 0 {
                break;
            }
        } else {
            c = SQLITE_UPPER_TO_LOWER[c as usize] as i32 - SQLITE_UPPER_TO_LOWER[x as usize] as i32;
            if c != 0 {
                break;
            }
        }
        i += 1;
    }
    // `c` vale 0 quando as strings são iguais, ou a diferença não nula que interrompeu o laço.
    let c = z_left.get(i).copied().unwrap_or(0) as i32;
    let x = z_right.get(i).copied().unwrap_or(0) as i32;
    if c == x {
        0
    } else {
        SQLITE_UPPER_TO_LOWER[c as usize] as i32 - SQLITE_UPPER_TO_LOWER[x as usize] as i32
    }
}

/// Comparação sem diferenciar maiúsculas de minúsculas, limitada a `n` bytes
/// (`sqlite3_strnicmp`).
pub fn strnicmp(z_left: Option<&[u8]>, z_right: Option<&[u8]>, n: i32) -> i32 {
    let (a, b) = match (z_left, z_right) {
        (None, None) => return 0,
        (None, Some(_)) => return -1,
        (Some(_), None) => return 1,
        (Some(a), Some(b)) => (a, b),
    };
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0) as usize;
    let mut n = n;
    let mut i = 0usize;
    loop {
        // `N-- > 0` do C: o decremento acontece mesmo quando o teste falha.
        let keep = n > 0;
        n = n.wrapping_sub(1);
        if !(keep && at(a, i) != 0 && SQLITE_UPPER_TO_LOWER[at(a, i)] == SQLITE_UPPER_TO_LOWER[at(b, i)]) {
            break;
        }
        i += 1;
    }
    if n < 0 {
        0
    } else {
        SQLITE_UPPER_TO_LOWER[at(a, i)] as i32 - SQLITE_UPPER_TO_LOWER[at(b, i)] as i32
    }
}

/// Calcula um hash de 8 bits de uma string, insensível a diferenças de caixa.
pub fn str_i_hash(z: Option<&[u8]>) -> u8 {
    let mut h: u8 = 0;
    if let Some(z) = z {
        for &byte in z {
            if byte == 0 {
                break;
            }
            h = h.wrapping_add(SQLITE_UPPER_TO_LOWER[byte as usize]);
        }
    }
    h
}

/// Multiplicação Double-Double. `(x[0], x[1]) *= (y, yy)`.
///
/// Referência: T. J. Dekker, "A Floating-Point Technique for Extending the Available
/// Precision". 1971-07-26.
///
/// Em Rust (SSE2 em x86_64) os resultados intermediários já são binary64, sem precisão
/// estendida, que é o efeito dos `volatile` do C.
fn dekker_mul2(x: &mut [f64; 2], y: f64, yy: f64) {
    let hx = f64::from_bits(x[0].to_bits() & 0xffff_ffff_fc00_0000u64);
    let tx = x[0] - hx;
    let hy = f64::from_bits(y.to_bits() & 0xffff_ffff_fc00_0000u64);
    let ty = y - hy;
    let p = hx * hy;
    let q = hx * ty + tx * hy;
    let c = p + q;
    let mut cc = p - c + q + tx * ty;
    cc = x[0] * yy + x[1] * y + cc;
    x[0] = c + cc;
    x[1] = c - x[0];
    x[1] += cc;
}

/// A string `z` é a representação textual de um número real. Converte para `f64`.
///
/// A string tem `length` bytes (bytes, não caracteres) e usa a codificação `enc`. Não é
/// necessariamente terminada em zero. O valor vai para `p_result`, escrito mesmo quando só um
/// prefixo da entrada é um número válido, e o código de retorno é:
///
///      1          =>  A entrada é um inteiro puro
///      2 ou mais  =>  A entrada tem ponto decimal ou cláusula eNNN
///      0 ou menos =>  A entrada não é um número válido
///     -1          =>  Não é um número válido, mas tem um prefixo válido que inclui
///                     ponto decimal e/ou cláusula eNNN
///
/// Formatos válidos:
///
///     [+-]digits[E[+-]digits]
///     [+-]digits.[digits][E[+-]digits]
///     [+-].digits[E[+-]digits]
///
/// Espaços iniciais e finais são ignorados para determinar a validade.
pub fn ato_f(z: &[u8], p_result: &mut f64, length: i32, enc: u8) -> i32 {
    let mut length = length;
    let mut sign: i32 = 1; // sinal do significando
    let mut s: u64 = 0; // significando
    let mut d: i32 = 0; // ajuste do expoente pelo deslocamento do ponto decimal
    let mut esign: i32 = 1; // sinal do expoente
    let mut e: i32 = 0; // expoente
    let mut e_valid: i32 = 1; // o expoente não é usado ou está bem formado
    let mut n_digit: i32 = 0; // número de dígitos processados
    let mut e_type: i32 = 1; // 1: inteiro puro, 2+: fracionário, -1 ou menos: UTF16 ruim

    debug_assert!(enc == SQLITE_UTF8 || enc == SQLITE_UTF16LE || enc == SQLITE_UTF16BE);
    *p_result = 0.0; // valor de retorno padrão, em caso de erro
    if length == 0 {
        return 0;
    }

    // Os índices abaixo são relativos ao início de `z`, como o ponteiro do C.
    let at = |i: usize| z.get(i).copied().unwrap_or(0);
    let incr: usize;
    let z_end: usize;
    let mut zi: usize;
    if enc == SQLITE_UTF8 {
        incr = 1;
        z_end = length as usize;
        zi = 0;
    } else {
        incr = 2;
        length &= !1;
        let mut i = (3 - enc as i32) as usize;
        while (i as i32) < length && at(i) == 0 {
            i += 2;
        }
        if (i as i32) < length {
            e_type = -100;
        }
        z_end = i ^ 1;
        zi = (enc & 1) as usize;
    }

    // Etapas do C que usam `goto do_atof_calc` viram o bloco rotulado `'parse`.
    'parse: {
        // pula espaços iniciais
        while zi < z_end && isspace(at(zi)) {
            zi += incr;
        }
        if zi >= z_end {
            return 0;
        }

        // sinal do significando
        if at(zi) == b'-' {
            sign = -1;
            zi += incr;
        } else if at(zi) == b'+' {
            zi += incr;
        }

        // copia o máximo de dígitos significativos para o significando
        while zi < z_end && isdigit(at(zi)) {
            s = s.wrapping_mul(10).wrapping_add((at(zi) - b'0') as u64);
            zi += incr;
            n_digit += 1;
            if s >= ((LARGEST_UINT64 - 9) / 10) {
                // pula dígitos não significativos (aumenta o expoente em d)
                while zi < z_end && isdigit(at(zi)) {
                    zi += incr;
                    d += 1;
                }
            }
        }
        if zi >= z_end {
            break 'parse;
        }

        // ponto decimal
        if at(zi) == b'.' {
            zi += incr;
            e_type += 1;
            // copia os dígitos depois do ponto (diminui o expoente em d)
            while zi < z_end && isdigit(at(zi)) {
                if s < ((LARGEST_UINT64 - 9) / 10) {
                    s = s * 10 + (at(zi) - b'0') as u64;
                    d -= 1;
                    n_digit += 1;
                }
                zi += incr;
            }
        }
        if zi >= z_end {
            break 'parse;
        }

        // expoente
        if at(zi) == b'e' || at(zi) == b'E' {
            zi += incr;
            e_valid = 0;
            e_type += 1;

            // Este ramo evita uma leitura além do fim (inofensiva).
            if zi >= z_end {
                break 'parse;
            }

            // sinal do expoente
            if at(zi) == b'-' {
                esign = -1;
                zi += incr;
            } else if at(zi) == b'+' {
                zi += incr;
            }
            // copia os dígitos para o expoente
            while zi < z_end && isdigit(at(zi)) {
                e = if e < 10000 { e * 10 + (at(zi) - b'0') as i32 } else { 10000 };
                zi += incr;
                e_valid = 1;
            }
        }

        // pula espaços finais
        while zi < z_end && isspace(at(zi)) {
            zi += incr;
        }
    }

    // do_atof_calc: zero é um caso especial
    let result: f64;
    if s == 0 {
        result = if sign < 0 { -0.0 } else { 0.0 };
    } else {
        // ajusta o expoente por d e atualiza o sinal
        e = (e * esign) + d;

        // tenta ajustar o expoente para torná-lo menor
        while e > 0 && s < (LARGEST_UINT64 / 10) {
            s *= 10;
            e -= 1;
        }
        while e < 0 && (s % 10) == 0 {
            s /= 10;
            e += 1;
        }

        let mut r_out: f64;
        if e == 0 {
            r_out = s as f64;
        } else if SQLITE_CONFIG.b_use_long_double != 0 {
            // `long double` do x86_64 (80 bits), emulado em `crate::fp80::F80`.
            let mut r = F80::from_u64(s);
            if e > 0 {
                while e >= 100 {
                    e -= 100;
                    r = r.mul(&F80::pow10(100));
                }
                while e >= 10 {
                    e -= 10;
                    r = r.mul(&F80::pow10(10));
                }
                while e >= 1 {
                    e -= 1;
                    r = r.mul(&F80::pow10(1));
                }
            } else {
                while e <= -100 {
                    e += 100;
                    r = r.mul(&F80::pow10(-100));
                }
                while e <= -10 {
                    e += 10;
                    r = r.mul(&F80::pow10(-10));
                }
                while e <= -1 {
                    e += 1;
                    r = r.mul(&F80::pow10(-1));
                }
            }
            if r.exceeds_dbl_max_literal() {
                r_out = f64::INFINITY;
            } else {
                r_out = r.to_f64();
            }
        } else {
            let mut rr = [0.0f64; 2];
            rr[0] = s as f64;
            let s2 = rr[0] as u64;
            rr[1] = if s >= s2 { (s - s2) as f64 } else { -((s2 - s) as f64) };
            if e > 0 {
                while e >= 100 {
                    e -= 100;
                    dekker_mul2(&mut rr, 1.0e+100, -1.5902891109759918046e+83);
                }
                while e >= 10 {
                    e -= 10;
                    dekker_mul2(&mut rr, 1.0e+10, 0.0);
                }
                while e >= 1 {
                    e -= 1;
                    dekker_mul2(&mut rr, 1.0e+01, 0.0);
                }
            } else {
                while e <= -100 {
                    e += 100;
                    dekker_mul2(&mut rr, 1.0e-100, -1.99918998026028836196e-117);
                }
                while e <= -10 {
                    e += 10;
                    dekker_mul2(&mut rr, 1.0e-10, -3.6432197315497741579e-27);
                }
                while e <= -1 {
                    e += 1;
                    dekker_mul2(&mut rr, 1.0e-01, -5.5511151231257827021e-18);
                }
            }
            r_out = rr[0] + rr[1];
            if is_nan(r_out) {
                r_out = f64::INFINITY;
            }
        }
        if sign < 0 {
            r_out = -r_out;
        }
        debug_assert!(!is_nan(r_out));
        result = r_out;
    }

    // atof_return: verdadeiro se é número e não há texto extra além de espaços
    *p_result = result;
    if zi == z_end && n_digit > 0 && e_valid != 0 && e_type > 0 {
        e_type
    } else if e_type >= 2 && (e_type == 3 || e_valid != 0) && n_digit > 0 {
        -1
    } else {
        0
    }
}


// ---- part_002.rs ----

/// Renderiza um inteiro de 64 bits com sinal como texto. Armazena o resultado em `z_out[]`
/// (seguido do terminador nulo) e retorna o comprimento do texto armazenado, em bytes. O valor
/// retornado não inclui o terminador nulo no final da saída.
///
/// O chamador deve garantir que `z_out[]` tenha pelo menos 21 bytes.
pub fn int64_to_text(v: i64, z_out: &mut [u8]) -> i32 {
    let mut z_temp = [0u8; 22];
    let mut x: u64 = if v < 0 {
        // Cobre também `SMALLEST_INT64`, cujo oposto é 1<<63 como inteiro sem sinal.
        v.wrapping_neg() as u64
    } else {
        v as u64
    };
    let mut i = z_temp.len() - 2;
    z_temp[z_temp.len() - 1] = 0;
    loop {
        z_temp[i] = (x % 10) as u8 + b'0';
        x /= 10;
        if x == 0 {
            break;
        }
        i -= 1;
    }
    if v < 0 {
        i -= 1;
        z_temp[i] = b'-';
    }
    let n = z_temp.len() - i;
    z_out[..n].copy_from_slice(&z_temp[i..]);
    (z_temp.len() - 1 - i) as i32
}

/// Compara a string de 19 caracteres `z_num` com a representação textual do valor 2^63:
/// 9223372036854775808. Retorna negativo, zero ou positivo se `z_num` for menor que, igual a
/// ou maior que a string. `z_num` deve conter exatamente 19 caracteres.
///
/// Diferente de `memcmp()`, esta rotina garante retornar a diferença dos valores do último
/// dígito se essa for a única diferença. Por exemplo,
///
///      compare2pow63("9223372036854775800", 1)
///
/// retorna -8.
fn compare2pow63(z_num: &[u8], incr: usize) -> i32 {
    let mut c: i32 = 0;
    //                       012345678901234567
    let pow63: &[u8; 18] = b"922337203685477580";
    let mut i = 0usize;
    while c == 0 && i < 18 {
        c = (z_num[i * incr] as i32 - pow63[i] as i32) * 10;
        i += 1;
    }
    if c == 0 {
        c = z_num[18 * incr] as i32 - b'8' as i32;
    }
    c
}

/// Converte `z_num` em um inteiro de 64 bits com sinal. `z_num` deve ser decimal. Esta rotina
/// *não* aceita notação hexadecimal.
///
/// Retorna:
///
///    -1    Nem um prefixo do texto de entrada parece um inteiro
///     0    Transformação bem-sucedida. Cabe em um inteiro de 64 bits com sinal.
///     1    Texto excedente que não é espaço depois do valor inteiro
///     2    Inteiro grande demais para 64 bits com sinal, ou malformado
///     3    Caso especial de 9223372036854775808
///
/// `length` é o número de bytes da string (bytes, não caracteres). A string não é
/// necessariamente terminada em zero. A codificação é dada por `enc`.
pub fn atoi64(z_num: &[u8], p_num: &mut i64, length: i32, enc: u8) -> i32 {
    let at = |i: usize| z_num.get(i).copied().unwrap_or(0);
    let incr: usize;
    let mut u: u64 = 0;
    let mut neg = false; // assume positivo
    let mut non_num = false; // entrada UTF16 com byte alto não nulo
    let z_end: usize;
    let mut zn: usize = 0; // posição corrente em `z_num`

    debug_assert!(enc == SQLITE_UTF8 || enc == SQLITE_UTF16LE || enc == SQLITE_UTF16BE);
    if enc == SQLITE_UTF8 {
        incr = 1;
        z_end = length as usize;
    } else {
        incr = 2;
        let length = length & !1;
        let mut i = (3 - enc as i32) as usize;
        while (i as i32) < length && at(i) == 0 {
            i += 2;
        }
        non_num = (i as i32) < length;
        z_end = i ^ 1;
        zn += (enc & 1) as usize;
    }
    while zn < z_end && isspace(at(zn)) {
        zn += incr;
    }
    if zn < z_end {
        if at(zn) == b'-' {
            neg = true;
            zn += incr;
        } else if at(zn) == b'+' {
            zn += incr;
        }
    }
    let z_start = zn;
    while zn < z_end && at(zn) == b'0' {
        // pula zeros à esquerda
        zn += incr;
    }
    let mut i: usize = 0;
    while zn + i < z_end {
        let c = at(zn + i);
        if !(b'0'..=b'9').contains(&c) {
            break;
        }
        u = u.wrapping_mul(10).wrapping_add((c - b'0') as u64);
        i += incr;
    }
    if u > LARGEST_INT64 as u64 {
        *p_num = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
    } else if neg {
        *p_num = (u as i64).wrapping_neg();
    } else {
        *p_num = u as i64;
    }
    let mut rc = 0;
    if i == 0 && z_start == zn {
        // sem dígitos
        rc = -1;
    } else if non_num {
        // UTF16 com bytes de ordem alta não nulos
        rc = 1;
    } else if zn + i < z_end {
        // bytes extras no final
        let mut jj = i;
        loop {
            if !isspace(at(zn + jj)) {
                rc = 1; // texto extra que não é espaço depois do inteiro
                break;
            }
            jj += incr;
            if zn + jj >= z_end {
                break;
            }
        }
    }
    if i < 19 * incr {
        // menos de 19 dígitos, logo cabe em 64 bits
        debug_assert!(u <= LARGEST_INT64 as u64);
        rc
    } else {
        // `z_num` tem 19 dígitos. Compara com 9223372036854775808.
        let c = if i > 19 * incr {
            1
        } else {
            compare2pow63(&z_num[zn..], incr)
        };
        if c < 0 {
            // menor que 9223372036854775808, então cabe
            debug_assert!(u <= LARGEST_INT64 as u64);
            rc
        } else {
            *p_num = if neg { SMALLEST_INT64 } else { LARGEST_INT64 };
            if c > 0 {
                // maior que 9223372036854775808, então estoura
                2
            } else {
                // exatamente 9223372036854775808. Cabe se negativo. O caso especial 2
                // estoura se positivo.
                debug_assert!(u.wrapping_sub(1) == LARGEST_INT64 as u64);
                if neg { rc } else { 3 }
            }
        }
    }
}

/// Transforma um literal inteiro UTF-8, decimal ou hexadecimal, em um inteiro de 64 bits com
/// sinal. Esta rotina aceita literais hexadecimais, ao contrário de `atoi64()`.
///
/// Retorna:
///
///     0    Transformação bem-sucedida. Cabe em um inteiro de 64 bits com sinal.
///     1    Texto excedente depois do valor inteiro
///     2    Inteiro grande demais para 64 bits com sinal, ou malformado
///     3    Caso especial de 9223372036854775808
pub fn dec_or_hex_to_i64(z: &[u8], p_out: &mut i64) -> i32 {
    let at = |i: usize| z.get(i).copied().unwrap_or(0);
    if at(0) == b'0' && (at(1) == b'x' || at(1) == b'X') {
        let mut u: u64 = 0;
        let mut i = 2usize;
        while at(i) == b'0' {
            i += 1;
        }
        let mut k = i;
        while isxdigit(at(k)) {
            u = u.wrapping_mul(16).wrapping_add(hex_to_int(at(k) as i32) as u64);
            k += 1;
        }
        *p_out = u as i64;
        if k - i > 16 {
            return 2;
        }
        if at(k) != 0 {
            return 1;
        }
        0
    } else {
        let mut n = 0usize;
        while matches!(at(n), b'+' | b'-' | b' ' | b'\n' | b'\t' | b'0'..=b'9') {
            n += 1;
        }
        let mut n = (0x3fff_ffff & n) as i32;
        if at(n as usize) != 0 {
            n += 1;
        }
        atoi64(z, p_out, n, SQLITE_UTF8)
    }
}

/// Se `z_num` representa um inteiro que cabe em 32 bits, grava-o em `*p_value` e retorna
/// verdadeiro (1). Caso contrário retorna 0.
///
/// Esta rotina aceita notação decimal e hexadecimal. Quaisquer caracteres não numéricos depois
/// de `z_num` são ignorados. Isso difere de `atoi64()`, que exige a entrada terminada em zero.
pub fn get_int32(z_num: &[u8], p_value: &mut i32) -> i32 {
    let at = |i: usize| z_num.get(i).copied().unwrap_or(0);
    let mut v: i64 = 0;
    let mut neg = 0i64;
    let mut zn = 0usize;
    if at(zn) == b'-' {
        neg = 1;
        zn += 1;
    } else if at(zn) == b'+' {
        zn += 1;
    } else if at(zn) == b'0' && (at(zn + 1) == b'x' || at(zn + 1) == b'X') && isxdigit(at(zn + 2)) {
        let mut u: u32 = 0;
        zn += 2;
        while at(zn) == b'0' {
            zn += 1;
        }
        let mut i = 0usize;
        while i < 8 && isxdigit(at(zn + i)) {
            u = u.wrapping_mul(16).wrapping_add(hex_to_int(at(zn + i) as i32) as u32);
            i += 1;
        }
        if (u & 0x8000_0000) == 0 && !isxdigit(at(zn + i)) {
            *p_value = u as i32;
            return 1;
        } else {
            return 0;
        }
    }
    if !isdigit(at(zn)) {
        return 0;
    }
    while at(zn) == b'0' {
        zn += 1;
    }
    let mut i = 0usize;
    while i < 11 {
        let c = at(zn + i) as i64 - b'0' as i64;
        if !(0..=9).contains(&c) {
            break;
        }
        v = v * 10 + c;
        i += 1;
    }

    // A representação decimal mais longa de um inteiro de 32 bits tem 10 dígitos:
    //
    //             1234567890
    //     2^31 -> 2147483648
    if i > 10 {
        return 0;
    }
    if v - neg > 2147483647 {
        return 0;
    }
    if neg != 0 {
        v = -v;
    }
    *p_value = v as i32;
    1
}

/// Retorna um inteiro de 32 bits extraído de uma string. Se a string não for um inteiro,
/// retorna 0.
pub fn atoi(z: &[u8]) -> i32 {
    let mut x = 0i32;
    get_int32(z, &mut x);
    x
}

/// Decodifica um valor de ponto flutuante em uma representação decimal aproximada.
///
/// Arredonda a representação decimal para `i_round` dígitos significativos se `i_round` for
/// positivo. Ou arredonda para `-i_round` dígitos significativos depois do ponto decimal se
/// for negativo. Nenhum arredondamento é feito se for zero.
///
/// Os dígitos significativos ficam em `p.z_buf[p.z..]` (índice em `z_buf`). Há `p.n` dígitos
/// significativos. A sequência *não* é terminada em zero.
pub fn fp_decode(p: &mut FpDecode, r: f64, i_round: i32, mx_round: i32) {
    let mut r = r;
    let mut i_round = i_round;
    let mut exp: i32 = 0;
    let mut v: u64;
    p.is_special = 0;
    p.z = 0;

    // Converte negativos em positivos. Trata Infinito, 0.0 e NaN.
    if r < 0.0 {
        p.sign = b'-';
        r = -r;
    } else if r == 0.0 {
        p.sign = b'+';
        p.n = 1;
        p.i_dp = 1;
        // O C aponta `p->z` para o literal "0"; aqui o dígito vai para o início de `z_buf`.
        p.z_buf[0] = b'0';
        p.z = 0;
        return;
    } else {
        p.sign = b'+';
    }
    v = r.to_bits();
    let e = (v >> 52) as i32;
    if (e & 0x7ff) == 0x7ff {
        p.is_special = 1 + (v != 0x7ff0_0000_0000_0000) as u8;
        p.n = 0;
        p.i_dp = 0;
        return;
    }

    // Multiplica `r` por potências de dez até cair entre 1.0e+19 e 1.0e+17.
    if SQLITE_CONFIG.b_use_long_double != 0 {
        // `long double` do x86_64 (80 bits), emulado em `crate::fp80::F80`.
        let mut rr = F80::from_f64(r);
        if rr.ge(&F80::pow10(19)) {
            while rr.ge(&F80::pow10(119)) {
                exp += 100;
                rr = rr.mul(&F80::pow10(-100));
            }
            while rr.ge(&F80::pow10(29)) {
                exp += 10;
                rr = rr.mul(&F80::pow10(-10));
            }
            while rr.ge(&F80::pow10(19)) {
                exp += 1;
                rr = rr.mul(&F80::pow10(-1));
            }
        } else {
            while rr.lt(&F80::pow10(-97)) {
                exp -= 100;
                rr = rr.mul(&F80::pow10(100));
            }
            while rr.lt(&F80::pow10(7)) {
                exp -= 10;
                rr = rr.mul(&F80::pow10(10));
            }
            while rr.lt(&F80::pow10(17)) {
                exp -= 1;
                rr = rr.mul(&F80::pow10(1));
            }
        }
        v = rr.to_u64();
    } else {
        // Sem `long double` de alta precisão, usa a computação double-double no estilo de
        // Dekker para aumentar a precisão.
        //
        // Os termos de erro de constantes como 1.0e+100 são calculados com a extensão decimal,
        // por exemplo: SELECT decimal_exp(decimal_sub('1.0e+100',decimal(1.0e+100)));
        let mut rr = [r, 0.0f64];
        if rr[0] > 9.223372036854774784e+18 {
            while rr[0] > 9.223372036854774784e+118 {
                exp += 100;
                dekker_mul2(&mut rr, 1.0e-100, -1.99918998026028836196e-117);
            }
            while rr[0] > 9.223372036854774784e+28 {
                exp += 10;
                dekker_mul2(&mut rr, 1.0e-10, -3.6432197315497741579e-27);
            }
            while rr[0] > 9.223372036854774784e+18 {
                exp += 1;
                dekker_mul2(&mut rr, 1.0e-01, -5.5511151231257827021e-18);
            }
        } else {
            while rr[0] < 9.223372036854774784e-83 {
                exp -= 100;
                dekker_mul2(&mut rr, 1.0e+100, -1.5902891109759918046e+83);
            }
            while rr[0] < 9.223372036854774784e+07 {
                exp -= 10;
                dekker_mul2(&mut rr, 1.0e+10, 0.0);
            }
            while rr[0] < 9.22337203685477478e+17 {
                exp -= 1;
                dekker_mul2(&mut rr, 1.0e+01, 0.0);
            }
        }
        v = if rr[1] < 0.0 {
            (rr[0] as u64).wrapping_sub((-rr[1]) as u64)
        } else {
            (rr[0] as u64).wrapping_add(rr[1] as u64)
        };
    }

    // Extrai os dígitos significativos. `i` é o índice livre mais alto (pode chegar a -1).
    let buf_len = p.z_buf.len() as i32;
    let mut i: i32 = buf_len - 1;
    debug_assert!(v > 0);
    while v != 0 {
        p.z_buf[i as usize] = (v % 10) as u8 + b'0';
        i -= 1;
        v /= 10;
    }
    debug_assert!(i >= 0 && i < buf_len - 1);
    p.n = buf_len - 1 - i;
    debug_assert!(p.n > 0);
    debug_assert!(p.n < buf_len);
    p.i_dp = p.n + exp;
    if i_round <= 0 {
        i_round = p.i_dp - i_round;
        if i_round == 0 && p.z_buf[(i + 1) as usize] >= b'5' {
            i_round = 1;
            p.z_buf[i as usize] = b'0';
            i -= 1;
            p.n += 1;
            p.i_dp += 1;
        }
    }
    if i_round > 0 && (i_round < p.n || p.n > mx_round) {
        let z = (i + 1) as usize; // `z` do C: &zBuf[i+1]
        if i_round > mx_round {
            i_round = mx_round;
        }
        p.n = i_round;
        if p.z_buf[z + i_round as usize] >= b'5' {
            let mut j = (i_round - 1) as usize;
            loop {
                p.z_buf[z + j] += 1;
                if p.z_buf[z + j] <= b'9' {
                    break;
                }
                p.z_buf[z + j] = b'0';
                if j == 0 {
                    p.z_buf[i as usize] = b'1';
                    i -= 1;
                    p.n += 1;
                    p.i_dp += 1;
                    break;
                } else {
                    j -= 1;
                }
            }
        }
    }
    p.z = (i + 1) as usize;
    debug_assert!(i + p.n < buf_len);
    while p.n > 0 && p.z_buf[p.z + p.n as usize - 1] == b'0' {
        p.n -= 1;
    }
}


// ---- part_003.rs ----

/// Tenta converter z em um inteiro sem sinal de 32 bits. Retorna 1 no
/// sucesso e 0 se houver um erro. Apenas notação decimal é aceita.
pub fn get_uint32(z: &[u8], p_i: &mut u32) -> i32 {
    let mut v: u64 = 0;
    let mut i = 0;

    // Processa cada dígito (o fim do slice equivale ao terminador nulo do C)
    while i < z.len() && isdigit(z[i]) {
        v = v * 10 + (z[i] - b'0') as u64;
        if v > 4294967296u64 {
            *p_i = 0;
            return 0;
        }
        i += 1;
    }

    // Exige ao menos um dígito e nada além deles
    if i == 0 || (i < z.len() && z[i] != 0) {
        *p_i = 0;
        return 0;
    }

    *p_i = v as u32;
    1
}

/// Escreve um inteiro de 64 bits codificado com comprimento variável
/// na memória começando em p[0]. O comprimento dos dados escrito
/// será entre 1 e 9 bytes. O número de bytes escritos é retornado.
fn put_varint64(p: &mut [u8], mut v: u64) -> usize {
    let mut buf: [u8; 10] = [0; 10];

    if v & (((0xffu64) << 32) << 24) != 0 {
        // Caso de 9 bytes: copiar todo o byte 8
        p[8] = v as u8;
        v >>= 8;
        for i in (0..8).rev() {
            p[i] = ((v & 0x7f) | 0x80) as u8;
            v >>= 7;
        }
        return 9;
    }

    let mut n = 0;
    loop {
        buf[n] = ((v & 0x7f) | 0x80) as u8;
        v >>= 7;
        n += 1;
        if v == 0 {
            break;
        }
    }

    buf[0] &= 0x7f;

    // Copia em ordem reversa
    for j in (0..n).rev() {
        p[n - 1 - j] = buf[j];
    }

    n
}

/// Escreve um inteiro de 64 bits codificado com comprimento variável
/// na memória começando em p[0].
pub fn put_varint(p: &mut [u8], v: u64) -> usize {
    if v <= 0x7f {
        p[0] = (v & 0x7f) as u8;
        return 1;
    }
    if v <= 0x3fff {
        p[0] = (((v >> 7) & 0x7f) | 0x80) as u8;
        p[1] = (v & 0x7f) as u8;
        return 2;
    }
    put_varint64(p, v)
}

/// Máscaras de bits usadas por get_varint(). Essas constantes pré-computadas
/// são definidas aqui em vez de apenas colocar as expressões de constante
/// inline para contornar erros do compilador RVT.
///
/// SLOT_2_0 é uma máscara para (0x7f<<14) | 0x7f
/// SLOT_4_2_0 é uma máscara para (0x7f<<28) | SLOT_2_0
const SLOT_2_0: u32 = 0x001fc07f;
const SLOT_4_2_0: u32 = 0xf01fc07f;

/// Lê um inteiro de 64 bits codificado com comprimento variável
/// da memória começando em p[0]. Retorna o número de bytes lidos.
/// O valor é armazenado em *v.
pub fn get_varint(p: &[u8], v: &mut u64) -> u8 {
    let mut a: u32;
    let mut b: u32;
    let mut s: u32;
    let mut p_offset = 0usize;

    if (p[p_offset] as i8) >= 0 {
        *v = p[p_offset] as u64;
        return 1;
    }

    if (p[p_offset + 1] as i8) >= 0 {
        *v = (((p[p_offset] & 0x7f) as u32) << 7) as u64 | p[p_offset + 1] as u64;
        return 2;
    }

    // Verifica que as constantes foram pré-computadas corretamente
    debug_assert_eq!(SLOT_2_0, ((0x7f << 14) | 0x7f));
    debug_assert_eq!(SLOT_4_2_0, ((0xfU32 << 28) | (0x7f << 14) | 0x7f));

    a = (p[p_offset] as u32) << 14;
    b = p[p_offset + 1] as u32;
    p_offset += 2;
    a |= p[p_offset] as u32;
    // a: p0<<14 | p2 (não mascarado)

    if (a & 0x80) == 0 {
        a &= SLOT_2_0;
        b &= 0x7f;
        b <<= 7;
        a |= b;
        *v = a as u64;
        return 3;
    }

    // CSE1 abaixo
    a &= SLOT_2_0;
    p_offset += 1;
    b <<= 14;
    b |= p[p_offset] as u32;
    // b: p1<<14 | p3 (não mascarado)

    if (b & 0x80) == 0 {
        b &= SLOT_2_0;
        // CSE1 movido para cima
        // a &= (0x7f<<14)|(0x7f);
        a <<= 7;
        a |= b;
        *v = a as u64;
        return 4;
    }

    // a: p0<<14 | p2 (mascarado)
    // b: p1<<14 | p3 (não mascarado)
    // 1: salva p0<<21 | p1<<14 | p2<<7 | p3 (mascarado)
    // CSE1 movido para cima
    // a &= (0x7f<<14)|(0x7f);
    b &= SLOT_2_0;
    s = a;
    // s: p0<<14 | p2 (mascarado)

    p_offset += 1;
    a <<= 14;
    a |= p[p_offset] as u32;
    // a: p0<<28 | p2<<14 | p4 (não mascarado)

    if (a & 0x80) == 0 {
        // Podemos pular estes porque foram efetivamente feitos acima
        // enquanto calculávamos s
        // a &= (0x7f<<28)|(0x7f<<14)|(0x7f);
        // b &= (0x7f<<14)|(0x7f);
        b <<= 7;
        a |= b;
        s >>= 18;
        *v = ((s as u64) << 32) | (a as u64);
        return 5;
    }

    // 2: salva p0<<21 | p1<<14 | p2<<7 | p3 (mascarado)
    s <<= 7;
    s |= b;
    // s: p0<<21 | p1<<14 | p2<<7 | p3 (mascarado)

    p_offset += 1;
    b <<= 14;
    b |= p[p_offset] as u32;
    // b: p1<<28 | p3<<14 | p5 (não mascarado)

    if (b & 0x80) == 0 {
        // Podemos pular isto porque foi efetivamente feito acima
        // no cálculo de s
        // b &= (0x7f<<28)|(0x7f<<14)|(0x7f);
        a &= SLOT_2_0;
        a <<= 7;
        a |= b;
        s >>= 18;
        *v = ((s as u64) << 32) | (a as u64);
        return 6;
    }

    p_offset += 1;
    a <<= 14;
    a |= p[p_offset] as u32;
    // a: p2<<28 | p4<<14 | p6 (não mascarado)

    if (a & 0x80) == 0 {
        a &= SLOT_4_2_0;
        b &= SLOT_2_0;
        b <<= 7;
        a |= b;
        s >>= 11;
        *v = ((s as u64) << 32) | (a as u64);
        return 7;
    }

    // CSE2 abaixo
    a &= SLOT_2_0;
    p_offset += 1;
    b <<= 14;
    b |= p[p_offset] as u32;
    // b: p3<<28 | p5<<14 | p7 (não mascarado)

    if (b & 0x80) == 0 {
        b &= SLOT_4_2_0;
        // CSE2 movido para cima
        // a &= (0x7f<<14)|(0x7f);
        a <<= 7;
        a |= b;
        s >>= 4;
        *v = ((s as u64) << 32) | (a as u64);
        return 8;
    }

    p_offset += 1;
    a <<= 15;
    a |= p[p_offset] as u32;
    // a: p4<<29 | p6<<15 | p8 (não mascarado)

    // CSE2 movido para cima
    // a &= (0x7f<<29)|(0x7f<<15)|(0xff);
    b &= SLOT_2_0;
    b <<= 8;
    a |= b;

    s <<= 4;
    b = p[p_offset.saturating_sub(4)] as u32;
    b &= 0x7f;
    b >>= 3;
    s |= b;

    *v = ((s as u64) << 32) | (a as u64);

    9
}

/// Lê um inteiro de 32 bits codificado com comprimento variável
/// da memória começando em p[0]. Retorna o número de bytes lidos.
/// O valor é armazenado em *v.
///
/// Se o varint armazenado em p[0] é maior que o que cabe em um inteiro
/// de 32 bits sem sinal, então define *v como 0xffffffff.
///
/// Uma versão MACRO, getVarint32, é fornecida que embutida
/// o caso de um único byte. Todo código deve usar a versão MACRO
/// pois esta função assume que o caso de um único byte já foi tratado.
pub fn get_varint32(p: &[u8], v: &mut u32) -> u8 {
    let mut v_64: u64 = 0;
    let n: u8;

    // Assume que o caso de um único byte já foi tratado pela macro getVarint32()
    debug_assert!((p[0] & 0x80) != 0);

    if (p[1] & 0x80) == 0 {
        // Este é o caso de dois bytes
        *v = (((p[0] & 0x7f) as u32) << 7) | (p[1] as u32);
        return 2;
    }

    if (p[2] & 0x80) == 0 {
        // Este é o caso de três bytes
        *v = (((p[0] & 0x7f) as u32) << 14) | (((p[1] & 0x7f) as u32) << 7) | (p[2] as u32);
        return 3;
    }

    // Quatro ou mais bytes
    n = get_varint(p, &mut v_64);
    debug_assert!(n > 3 && n <= 9);

    if (v_64 & SQLITE_MAX_U32) != v_64 {
        *v = 0xffffffff;
    } else {
        *v = v_64 as u32;
    }

    n
}

/// Retorna o número de bytes que será necessário para armazenar
/// o inteiro de 64 bits fornecido.
pub fn varint_len(mut v: u64) -> i32 {
    let mut i = 1;

    loop {
        v >>= 7;
        if v == 0 {
            break;
        }
        debug_assert!(i < 10);
        i += 1;
    }

    i
}

/// Lê um valor inteiro big-endian de quatro bytes.
pub fn get4byte(p: &[u8]) -> u32 {
    // Little-endian (SQLITE_BYTEORDER==1234): o resultado é o mesmo do bswap32 do C.
    ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | (p[3] as u32)
}

/// Escreve um valor inteiro big-endian de quatro bytes.
pub fn put4byte(p: &mut [u8], v: u32) {
    p[0] = (v >> 24) as u8;
    p[1] = (v >> 16) as u8;
    p[2] = (v >> 8) as u8;
    p[3] = v as u8;
}

/// Traduz um único byte hexadecimal em um inteiro.
/// Esta rotina só funciona se `h` for de fato um caractere hexadecimal válido: 0..9a..fA..F
pub fn hex_to_int(h: i32) -> u8 {
    debug_assert!(
        (h >= b'0' as i32 && h <= b'9' as i32)
            || (h >= b'a' as i32 && h <= b'f' as i32)
            || (h >= b'A' as i32 && h <= b'F' as i32)
    );
    // ASCII: o bit 6 é 1 para letras, que precisam somar 9.
    let h = h + 9 * (1 & (h >> 6));
    (h & 0xf) as u8
}


// ---- part_004.rs ----

/// Converte um literal BLOB da forma "x'hhhhhh'" para o seu valor binário.
/// `n` é o mesmo `n` do C (a alocação tem `n/2 + 1` bytes e o laço vai até `n - 1`).
/// O vetor devolvido pertence ao chamador.
pub fn hex_to_blob(_db: &Sqlite3, z: &[u8], n: i32) -> Option<Vec<u8>> {
    let mut z_blob = vec![0u8; (n / 2 + 1).max(1) as usize];
    let n = n - 1;
    let mut i: i32 = 0;
    while i < n {
        let iu = i as usize;
        z_blob[iu / 2] = (hex_to_int(z[iu]) << 4) | hex_to_int(z[iu + 1]);
        i += 2;
    }
    z_blob[(i / 2) as usize] = 0;
    Some(z_blob)
}

/// Registra um erro de chamada de API sobre um ponteiro de conexão que não
/// deveria ter sido usado. `z_type` é uma palavra como "NULL", "closed" ou "invalid".
fn log_bad_connection(z_type: &str) {
    let mut msg: Vec<u8> = Vec::new();
    msg.extend_from_slice(b"API call with ");
    msg.extend_from_slice(z_type.as_bytes());
    msg.extend_from_slice(b" database connection pointer");
    api_log(SQLITE_MISUSE, &msg, &[]);
}

/// Confere que a conexão é válida. Devolve 1 se pode ser usada e 0 se não deve ser
/// desreferenciada por motivo nenhum. O chamador deve invocar SQLITE_MISUSE em seguida.
/// O caso `db==NULL` do C não existe aqui: a referência nunca é nula.
pub fn safety_check_ok(db: &Sqlite3) -> i32 {
    let e_open_state = db.e_open_state;
    if e_open_state != SQLITE_STATE_OPEN {
        if safety_check_sick_or_ok(db) != 0 {
            log_bad_connection("unopened");
        }
        0
    } else {
        1
    }
}

/// Como `safety_check_ok`, mas aceita uma conexão que falhou ao abrir e só serve
/// de argumento para `errmsg` ou `close`.
pub fn safety_check_sick_or_ok(db: &Sqlite3) -> i32 {
    let e_open_state = db.e_open_state;
    if e_open_state != SQLITE_STATE_SICK
        && e_open_state != SQLITE_STATE_OPEN
        && e_open_state != SQLITE_STATE_BUSY
    {
        log_bad_connection("invalid");
        0
    } else {
        1
    }
}

/// Soma `i_b` ao inteiro de 64 bits em `p_a`. Devolve 0 em sucesso e 1 em overflow.
/// Como o `__builtin_add_overflow` do GCC (ramo escolhido no Debian), o resultado
/// com wraparound é sempre gravado em `*p_a`, mesmo no overflow.
pub fn add_int64(p_a: &mut i64, i_b: i64) -> i32 {
    let (r, o) = p_a.overflowing_add(i_b);
    *p_a = r;
    o as i32
}

/// Subtrai `i_b` de `*p_a`, com a semântica do `__builtin_sub_overflow`.
pub fn sub_int64(p_a: &mut i64, i_b: i64) -> i32 {
    let (r, o) = p_a.overflowing_sub(i_b);
    *p_a = r;
    o as i32
}

/// Multiplica `*p_a` por `i_b`, com a semântica do `__builtin_mul_overflow`.
pub fn mul_int64(p_a: &mut i64, i_b: i64) -> i32 {
    let (r, o) = p_a.overflowing_mul(i_b);
    *p_a = r;
    o as i32
}

/// Valor absoluto de um inteiro de 32 bits. Para -2147483648 devolve +2147483647.
pub fn abs_int32(x: i32) -> i32 {
    if x >= 0 {
        return x;
    }
    if x == i32::MIN {
        return 0x7fffffff;
    }
    -x
}

/// Soma aproximada de dois valores LogEst. Não é um "+" simples porque o LogEst
/// é armazenado de forma logarítmica.
pub fn log_est_add(a: LogEst, b: LogEst) -> LogEst {
    static X: [u8; 32] = [
        10, 10, // 0,1
        9, 9, // 2,3
        8, 8, // 4,5
        7, 7, 7, // 6,7,8
        6, 6, 6, // 9,10,11
        5, 5, 5, // 12-14
        4, 4, 4, 4, // 15-18
        3, 3, 3, 3, 3, 3, // 19-24
        2, 2, 2, 2, 2, 2, 2, // 25-31
    ];
    let (ai, bi) = (a as i32, b as i32);
    if ai >= bi {
        if ai > bi + 49 {
            return a;
        }
        if ai > bi + 31 {
            return (ai + 1) as LogEst;
        }
        (ai + X[(ai - bi) as usize] as i32) as LogEst
    } else {
        if bi > ai + 49 {
            return b;
        }
        if bi > ai + 31 {
            return (bi + 1) as LogEst;
        }
        (bi + X[(bi - ai) as usize] as i32) as LogEst
    }
}

/// Converte um inteiro em LogEst, ou seja, uma aproximação de 10*log2(x).
pub fn log_est(mut x: u64) -> LogEst {
    static A: [LogEst; 8] = [0, 2, 3, 5, 6, 7, 8, 9];
    let mut y: LogEst = 40;
    if x < 8 {
        if x < 2 {
            return 0;
        }
        while x < 8 {
            y -= 10;
            x <<= 1;
        }
    } else {
        let i = 60 - x.leading_zeros() as i32;
        y += (i * 10) as LogEst;
        x >>= i;
    }
    A[(x & 7) as usize] + y - 10
}

/// Converte um double em LogEst.
pub fn log_est_from_double(x: f64) -> LogEst {
    if x <= 1.0 {
        return 0;
    }
    if x <= 2000000000.0 {
        return log_est(x as u64);
    }
    let a: u64 = x.to_bits();
    let e: i32 = (a >> 52) as i32 - 1022;
    (e * 10) as LogEst
}

/// Converte um LogEst em inteiro.
pub fn log_est_to_int(x: LogEst) -> u64 {
    let mut x = x as i32;
    let mut n: u64 = (x % 10) as i64 as u64;
    x /= 10;
    if n >= 5 {
        n -= 2;
    } else if n >= 1 {
        n -= 1;
    }
    if x > 60 {
        return LARGEST_INT64 as u64;
    }
    if x >= 3 {
        n.wrapping_add(8).checked_shl((x - 3) as u32).unwrap_or(0)
    } else {
        n.wrapping_add(8).checked_shr((3 - x) as u32).unwrap_or(0)
    }
}

/// Grava o byte `k` do nome da VList, cujos bytes ficam sobrepostos aos inteiros a
/// partir da posição `base` (ordem nativa, little-endian no Debian amd64).
fn vlist_set_byte(list: &mut [i32], base: usize, k: usize, byte: u8) {
    let slot = base + k / 4;
    let mut b = list[slot].to_le_bytes();
    b[k % 4] = byte;
    list[slot] = i32::from_le_bytes(b);
}

/// Lê o byte `k` do nome da VList a partir da posição `base`.
fn vlist_get_byte(list: &[i32], base: usize, k: usize) -> u8 {
    list[base + k / 4].to_le_bytes()[k % 4]
}

/// Acrescenta um par nome/número a uma VList, realocando se preciso, e devolve a nova VList.
///
/// Uma VList é um vetor de inteiros: o primeiro é o total alocado, o segundo é o total
/// usado, e cada entrada seguinte é (valor, nº de slots, nome terminado em zero
/// sobreposto aos slots restantes). `n_name` é o número de bytes do nome.
pub fn v_list_add(
    _db: &Sqlite3,
    p_in: Option<Vec<i32>>,
    z_name: &[u8],
    n_name: i32,
    i_val: i32,
) -> Vec<i32> {
    let n_int: i32 = n_name / 4 + 3;
    let mut p_in = match p_in {
        Some(p) => {
            debug_assert!(p[0] >= 3);
            p
        }
        None => Vec::new(),
    };
    if p_in.is_empty() || p_in[1] + n_int > p_in[0] {
        // Amplia a alocação
        let n_alloc: i64 = (if p_in.is_empty() { 10 } else { 2 * p_in[0] as i64 }) + n_int as i64;
        let era_nula = p_in.is_empty();
        p_in.resize(n_alloc as usize, 0);
        if era_nula {
            p_in[1] = 2;
        }
        p_in[0] = n_alloc as i32;
    }
    let i = p_in[1] as usize;
    p_in[i] = i_val;
    p_in[i + 1] = n_int;
    p_in[1] = i as i32 + n_int;
    debug_assert!(p_in[1] <= p_in[0]);
    for k in 0..n_name as usize {
        vlist_set_byte(&mut p_in, i + 2, k, z_name[k]);
    }
    vlist_set_byte(&mut p_in, i + 2, n_name as usize, 0);
    p_in
}

/// Devolve o nome da variável com valor `i_val`, ou `None` se não existir.
pub fn v_list_num_to_name(p_in: &[i32], i_val: i32) -> Option<Vec<u8>> {
    if p_in.is_empty() {
        return None;
    }
    let mx = p_in[1];
    let mut i: i32 = 2;
    loop {
        if p_in[i as usize] == i_val {
            let base = i as usize + 2;
            let mut nome = Vec::new();
            let mut k = 0;
            loop {
                let c = vlist_get_byte(p_in, base, k);
                if c == 0 {
                    break;
                }
                nome.push(c);
                k += 1;
            }
            return Some(nome);
        }
        i += p_in[i as usize + 1];
        if i >= mx {
            break;
        }
    }
    None
}

/// Devolve o número da variável chamada `z_name` (`n_name` bytes), ou 0 se não existir.
pub fn v_list_name_to_num(p_in: &[i32], z_name: &[u8], n_name: i32) -> i32 {
    if p_in.is_empty() {
        return 0;
    }
    let mx = p_in[1];
    let mut i: i32 = 2;
    loop {
        let base = i as usize + 2;
        // strncmp(z, zName, nName)==0 && z[nName]==0
        let mut igual = true;
        for k in 0..n_name as usize {
            let c = vlist_get_byte(p_in, base, k);
            if c != z_name[k] {
                igual = false;
                break;
            }
            if c == 0 {
                break;
            }
        }
        if igual && vlist_get_byte(p_in, base, n_name as usize) == 0 {
            return p_in[i as usize];
        }
        i += p_in[i as usize + 1];
        if i >= mx {
            break;
        }
    }
    0
}


// ---- part_005.rs ----

// Parte 005: util.c linhas 36794-36803
// Código condicional para incluir hwtime.h sob VDBE_PROFILE, SQLITE_PERFORMANCE_TRACE ou
// SQLITE_ENABLE_STMT_SCANSTATUS. Essas flags não estão habilitadas no Debian 13, então essa
// seção não produz código.

