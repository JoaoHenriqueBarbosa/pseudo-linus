// Mesclado das partes traduzidas de tokenize_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Classes de caracteres usadas na tokenização.
//
// Em `get_token()` o `match` sobre `AI_CLASS[c]` vira uma tabela de saltos,
// enquanto um `match` direto sobre `c` seria uma busca binária. A tabela é bem
// mais rápida; por isso todas as classes são inteiros pequenos e todas são
// usadas no `match`.
pub const CC_X: u8 = 0; // A letra 'x', ou o início de um literal BLOB
pub const CC_KYWD0: u8 = 1; // Primeira letra de uma palavra-chave
pub const CC_KYWD: u8 = 2; // Alfabéticos ou '_', usáveis numa palavra-chave
pub const CC_DIGIT: u8 = 3; // Dígitos
pub const CC_DOLLAR: u8 = 4; // '$'
pub const CC_VARALPHA: u8 = 5; // '@', '#', ':'. Variáveis SQL alfabéticas
pub const CC_VARNUM: u8 = 6; // '?'. Variáveis SQL numéricas
pub const CC_SPACE: u8 = 7; // Caracteres de espaço
pub const CC_QUOTE: u8 = 8; // '"', '\'' ou '`'. Literais de texto, ids entre aspas
pub const CC_QUOTE2: u8 = 9; // '['. Ids entre colchetes
pub const CC_PIPE: u8 = 10; // '|'. OR bit a bit ou concatenação
pub const CC_MINUS: u8 = 11; // '-'. Menos ou comentário no estilo SQL
pub const CC_LT: u8 = 12; // '<'. Parte de < ou <= ou <>
pub const CC_GT: u8 = 13; // '>'. Parte de > ou >=
pub const CC_EQ: u8 = 14; // '='. Parte de = ou ==
pub const CC_BANG: u8 = 15; // '!'. Parte de !=
pub const CC_SLASH: u8 = 16; // '/'. Divisão ou comentário no estilo C
pub const CC_LP: u8 = 17; // '('
pub const CC_RP: u8 = 18; // ')'
pub const CC_SEMI: u8 = 19; // ';'
pub const CC_PLUS: u8 = 20; // '+'
pub const CC_STAR: u8 = 21; // '*'
pub const CC_PERCENT: u8 = 22; // '%'
pub const CC_COMMA: u8 = 23; // ','
pub const CC_AND: u8 = 24; // '&'
pub const CC_TILDA: u8 = 25; // '~'
pub const CC_DOT: u8 = 26; // '.'
pub const CC_ID: u8 = 27; // Caracteres unicode usáveis em ids
pub const CC_ILLEGAL: u8 = 28; // Caractere ilegal
pub const CC_NUL: u8 = 29; // 0x00
pub const CC_BOM: u8 = 30; // Primeiro byte do BOM UTF-8: 0xEF 0xBB 0xBF

pub const AI_CLASS: [u8; 256] = [
    //   x0  x1  x2  x3  x4  x5  x6  x7  x8  x9  xa  xb  xc  xd  xe  xf
    /* 0x */ 29, 28, 28, 28, 28, 28, 28, 28, 28, 7, 7, 28, 7, 7, 28, 28,
    /* 1x */ 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28,
    /* 2x */ 7, 15, 8, 5, 4, 22, 24, 8, 17, 18, 21, 20, 23, 11, 26, 16,
    /* 3x */ 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 5, 19, 12, 14, 13, 6,
    /* 4x */ 5, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    /* 5x */ 1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 2, 9, 28, 28, 28, 2,
    /* 6x */ 8, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    /* 7x */ 1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 2, 28, 10, 28, 25, 28,
    /* 8x */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* 9x */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* Ax */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* Bx */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* Cx */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* Dx */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    /* Ex */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 30,
    /* Fx */ 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
];

/// O macro `charMap()` do C mapeia letras (somente) para o equivalente ASCII
/// minúsculo. Em máquinas ASCII é só o mapa de maiúscula para minúscula.
/// Usado por keywordhash.h.
#[inline]
pub fn char_map(x: u8) -> u8 {
    UPPER_TO_LOWER[x as usize]
}

/// Lê o byte `i` do texto, que no C termina em NUL. Além do fim do slice o
/// resultado é 0, como o terminador do C.
#[inline]
fn byte_at(z: &[u8], i: usize) -> u8 {
    if i < z.len() {
        z[i]
    } else {
        0
    }
}

/// Se X é um caractere que pode ser usado num identificador, devolve `true`
/// (o macro `IdChar(X)` e a função `sqlite3IsIdChar` do C).
///
/// Para ASCII, qualquer caractere com o bit alto ligado é permitido num
/// identificador. Para caracteres de 7 bits, o mapa de tipos precisa marcar 1.
///
/// O padrão SQL não permite '$' no meio de identificadores, mas muitas
/// implementações permitem. O SQLite aceita '$' em identificadores por
/// compatibilidade, mas o recurso não é documentado.
#[inline]
pub fn is_id_char(c: u8) -> bool {
    (CTYPE_MAP[c as usize] & 0x46) != 0
}

/// Devolve o id do próximo token em `*z`. Antes de retornar, avança `*z` para
/// o byte seguinte ao token lido. (É o `getToken` estático do C.)
fn get_token_next(z: &mut &[u8]) -> i32 {
    let mut zz: &[u8] = *z;
    let mut t: i32 = 0; // Tipo de token a devolver
    loop {
        let n = get_token(zz, &mut t) as usize;
        zz = &zz[n.min(zz.len())..];
        if t != TK_SPACE as i32 {
            break;
        }
    }
    if t == TK_ID as i32
        || t == TK_STRING as i32
        || t == TK_JOIN_KW as i32
        || t == TK_WINDOW as i32
        || t == TK_OVER as i32
        || parser_fallback(t) == TK_ID as i32
    {
        t = TK_ID as i32;
    }
    *z = zz;
    t
}

/// As três funções seguintes são chamadas logo depois que o tokenizador lê as
/// palavras-chave WINDOW, OVER e FILTER, respectivamente, para decidir se o
/// token deve ser tratado como palavra-chave ou como identificador SQL. Isso
/// não pode ser resolvido pelo `%fallback` usual do lemon, por causa da
/// ambiguidade em algumas construções, por exemplo:
///
///   SELECT sum(x) OVER ...
///
/// Acima, "OVER" pode ser palavra-chave ou um alias da expressão sum(x). Se
/// uma diretiva "%fallback ID OVER" fosse acrescentada à gramática, o SQLite
/// trataria "OVER" sempre como alias, o que tornaria impossível chamar uma
/// função de janela sem cláusula FILTER.
///
/// WINDOW é palavra-chave se:
///
///   * o token seguinte é um identificador, ou uma palavra-chave que pode
///     recair em identificador, e
///   * o token depois desse é TK_AS.
///
/// OVER é palavra-chave se:
///
///   * o token anterior era TK_RP, e
///   * o próximo token é TK_LP ou um identificador.
///
/// FILTER é palavra-chave se:
///
///   * o token anterior era TK_RP, e
///   * o próximo token é TK_LP.
pub fn analyze_window_keyword(z: &[u8]) -> i32 {
    let mut z = z;
    let mut t = get_token_next(&mut z);
    if t != TK_ID as i32 {
        return TK_ID as i32;
    }
    t = get_token_next(&mut z);
    if t != TK_AS as i32 {
        return TK_ID as i32;
    }
    TK_WINDOW as i32
}

pub fn analyze_over_keyword(z: &[u8], last_token: i32) -> i32 {
    let mut z = z;
    if last_token == TK_RP as i32 {
        let t = get_token_next(&mut z);
        if t == TK_LP as i32 || t == TK_ID as i32 {
            return TK_OVER as i32;
        }
    }
    TK_ID as i32
}

pub fn analyze_filter_keyword(z: &[u8], last_token: i32) -> i32 {
    let mut z = z;
    if last_token == TK_RP as i32 && get_token_next(&mut z) == TK_LP as i32 {
        return TK_FILTER as i32;
    }
    TK_ID as i32
}

/// Devolve o comprimento (em bytes) do token que começa em `z[0]`. Guarda o
/// tipo do token em `*token_type` antes de retornar.
pub fn get_token(z: &[u8], token_type: &mut i32) -> i32 {
    let mut i: usize;
    let mut c: u8;
    // Seleciona pela classe do primeiro byte do token. Veja o comentário dos
    // defines CC_ acima.
    let class = AI_CLASS[byte_at(z, 0) as usize];
    match class {
        CC_SPACE => {
            i = 1;
            while isspace(byte_at(z, i)) {
                i += 1;
            }
            *token_type = TK_SPACE as i32;
            return i as i32;
        }
        CC_MINUS => {
            if byte_at(z, 1) == b'-' {
                i = 2;
                loop {
                    c = byte_at(z, i);
                    if c == 0 || c == b'\n' {
                        break;
                    }
                    i += 1;
                }
                *token_type = TK_SPACE as i32; // IMP: R-22934-25134
                return i as i32;
            } else if byte_at(z, 1) == b'>' {
                *token_type = TK_PTR as i32;
                return 2 + (byte_at(z, 2) == b'>') as i32;
            }
            *token_type = TK_MINUS as i32;
            return 1;
        }
        CC_LP => {
            *token_type = TK_LP as i32;
            return 1;
        }
        CC_RP => {
            *token_type = TK_RP as i32;
            return 1;
        }
        CC_SEMI => {
            *token_type = TK_SEMI as i32;
            return 1;
        }
        CC_PLUS => {
            *token_type = TK_PLUS as i32;
            return 1;
        }
        CC_STAR => {
            *token_type = TK_STAR as i32;
            return 1;
        }
        CC_SLASH => {
            if byte_at(z, 1) != b'*' || byte_at(z, 2) == 0 {
                *token_type = TK_SLASH as i32;
                return 1;
            }
            i = 3;
            c = byte_at(z, 2);
            while (c != b'*' || byte_at(z, i) != b'/') && {
                c = byte_at(z, i);
                c != 0
            } {
                i += 1;
            }
            if c != 0 {
                i += 1;
            }
            *token_type = TK_SPACE as i32; // IMP: R-22934-25134
            return i as i32;
        }
        CC_PERCENT => {
            *token_type = TK_REM as i32;
            return 1;
        }
        CC_EQ => {
            *token_type = TK_EQ as i32;
            return 1 + (byte_at(z, 1) == b'=') as i32;
        }
        CC_LT => {
            c = byte_at(z, 1);
            if c == b'=' {
                *token_type = TK_LE as i32;
                return 2;
            } else if c == b'>' {
                *token_type = TK_NE as i32;
                return 2;
            } else if c == b'<' {
                *token_type = TK_LSHIFT as i32;
                return 2;
            } else {
                *token_type = TK_LT as i32;
                return 1;
            }
        }
        CC_GT => {
            c = byte_at(z, 1);
            if c == b'=' {
                *token_type = TK_GE as i32;
                return 2;
            } else if c == b'>' {
                *token_type = TK_RSHIFT as i32;
                return 2;
            } else {
                *token_type = TK_GT as i32;
                return 1;
            }
        }
        CC_BANG => {
            if byte_at(z, 1) != b'=' {
                *token_type = TK_ILLEGAL as i32;
                return 1;
            } else {
                *token_type = TK_NE as i32;
                return 2;
            }
        }
        CC_PIPE => {
            if byte_at(z, 1) != b'|' {
                *token_type = TK_BITOR as i32;
                return 1;
            } else {
                *token_type = TK_CONCAT as i32;
                return 2;
            }
        }
        CC_COMMA => {
            *token_type = TK_COMMA as i32;
            return 1;
        }
        CC_AND => {
            *token_type = TK_BITAND as i32;
            return 1;
        }
        CC_TILDA => {
            *token_type = TK_BITNOT as i32;
            return 1;
        }
        CC_QUOTE => {
            let delim = byte_at(z, 0);
            i = 1;
            loop {
                c = byte_at(z, i);
                if c == 0 {
                    break;
                }
                if c == delim {
                    if byte_at(z, i + 1) == delim {
                        i += 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }
            if c == b'\'' {
                *token_type = TK_STRING as i32;
                return (i + 1) as i32;
            } else if c != 0 {
                *token_type = TK_ID as i32;
                return (i + 1) as i32;
            } else {
                *token_type = TK_ILLEGAL as i32;
                return i as i32;
            }
        }
        CC_DOT | CC_DIGIT => {
            // Em CC_DOT, se o próximo caractere é um dígito, é um número de
            // ponto flutuante que começa com "."; segue para o caso CC_DIGIT.
            if class == CC_DOT && !isdigit(byte_at(z, 1)) {
                *token_type = TK_DOT as i32;
                return 1;
            }
            *token_type = TK_INTEGER as i32;
            if byte_at(z, 0) == b'0'
                && (byte_at(z, 1) == b'x' || byte_at(z, 1) == b'X')
                && isxdigit(byte_at(z, 2))
            {
                i = 3;
                loop {
                    if !isxdigit(byte_at(z, i)) {
                        if byte_at(z, i) == SQLITE_DIGIT_SEPARATOR {
                            *token_type = TK_QNUMBER as i32;
                        } else {
                            break;
                        }
                    }
                    i += 1;
                }
            } else {
                i = 0;
                loop {
                    if !isdigit(byte_at(z, i)) {
                        if byte_at(z, i) == SQLITE_DIGIT_SEPARATOR {
                            *token_type = TK_QNUMBER as i32;
                        } else {
                            break;
                        }
                    }
                    i += 1;
                }
                if byte_at(z, i) == b'.' {
                    if *token_type == TK_INTEGER as i32 {
                        *token_type = TK_FLOAT as i32;
                    }
                    i += 1;
                    loop {
                        if !isdigit(byte_at(z, i)) {
                            if byte_at(z, i) == SQLITE_DIGIT_SEPARATOR {
                                *token_type = TK_QNUMBER as i32;
                            } else {
                                break;
                            }
                        }
                        i += 1;
                    }
                }
                if (byte_at(z, i) == b'e' || byte_at(z, i) == b'E')
                    && (isdigit(byte_at(z, i + 1))
                        || ((byte_at(z, i + 1) == b'+' || byte_at(z, i + 1) == b'-')
                            && isdigit(byte_at(z, i + 2))))
                {
                    if *token_type == TK_INTEGER as i32 {
                        *token_type = TK_FLOAT as i32;
                    }
                    i += 2;
                    loop {
                        if !isdigit(byte_at(z, i)) {
                            if byte_at(z, i) == SQLITE_DIGIT_SEPARATOR {
                                *token_type = TK_QNUMBER as i32;
                            } else {
                                break;
                            }
                        }
                        i += 1;
                    }
                }
            }
            while is_id_char(byte_at(z, i)) {
                *token_type = TK_ILLEGAL as i32;
                i += 1;
            }
            return i as i32;
        }
        CC_QUOTE2 => {
            i = 1;
            c = byte_at(z, 0);
            while c != b']' && {
                c = byte_at(z, i);
                c != 0
            } {
                i += 1;
            }
            *token_type = if c == b']' {
                TK_ID as i32
            } else {
                TK_ILLEGAL as i32
            };
            return i as i32;
        }
        CC_VARNUM => {
            *token_type = TK_VARIABLE as i32;
            i = 1;
            while isdigit(byte_at(z, i)) {
                i += 1;
            }
            return i as i32;
        }
        CC_DOLLAR | CC_VARALPHA => {
            let mut n: i32 = 0;
            *token_type = TK_VARIABLE as i32;
            i = 1;
            loop {
                c = byte_at(z, i);
                if c == 0 {
                    break;
                }
                if is_id_char(c) {
                    n += 1;
                } else if c == b'(' && n > 0 {
                    loop {
                        i += 1;
                        c = byte_at(z, i);
                        if !(c != 0 && !isspace(c) && c != b')') {
                            break;
                        }
                    }
                    if c == b')' {
                        i += 1;
                    } else {
                        *token_type = TK_ILLEGAL as i32;
                    }
                    break;
                } else if c == b':' && byte_at(z, i + 1) == b':' {
                    i += 1;
                } else {
                    break;
                }
                i += 1;
            }
            if n == 0 {
                *token_type = TK_ILLEGAL as i32;
            }
            return i as i32;
        }
        CC_KYWD0 => {
            if AI_CLASS[byte_at(z, 1) as usize] > CC_KYWD {
                i = 1;
            } else {
                i = 2;
                while AI_CLASS[byte_at(z, i) as usize] <= CC_KYWD {
                    i += 1;
                }
                if is_id_char(byte_at(z, i)) {
                    // Este token começou com caracteres que podem aparecer em
                    // palavras-chave, mas z[i] é um caractere não permitido
                    // dentro de palavras-chave, então deve ser um
                    // identificador.
                    i += 1;
                } else {
                    *token_type = TK_ID as i32;
                    // keywordCode(z, i, tokenType) grava o código da palavra-chave
                    // em *token_type e devolve o comprimento `i`.
                    return keyword_code(z, i as i32, token_type);
                }
            }
        }
        CC_X | CC_KYWD | CC_ID => {
            if class == CC_X && byte_at(z, 1) == b'\'' {
                *token_type = TK_BLOB as i32;
                i = 2;
                while isxdigit(byte_at(z, i)) {
                    i += 1;
                }
                if byte_at(z, i) != b'\'' || i % 2 != 0 {
                    *token_type = TK_ILLEGAL as i32;
                    while byte_at(z, i) != 0 && byte_at(z, i) != b'\'' {
                        i += 1;
                    }
                }
                if byte_at(z, i) != 0 {
                    i += 1;
                }
                return i as i32;
            }
            // Se não é um literal BLOB, então deve ser um ID, já que nenhuma
            // palavra-chave SQL começa com a letra 'x'.
            i = 1;
        }
        CC_BOM => {
            if byte_at(z, 1) == 0xbb && byte_at(z, 2) == 0xbf {
                *token_type = TK_SPACE as i32;
                return 3;
            }
            i = 1;
        }
        CC_NUL => {
            *token_type = TK_ILLEGAL as i32;
            return 0;
        }
        _ => {
            *token_type = TK_ILLEGAL as i32;
            return 1;
        }
    }
    while is_id_char(byte_at(z, i)) {
        i += 1;
    }
    *token_type = TK_ID as i32;
    i as i32
}


// ---- part_001.rs ----

// Trecho 1 de tokenize.c: `sqlite3RunParser()` e (fora do build) `sqlite3Normalize()`.
//
// Fora do porte, por não existirem no build do Debian:
//   - o bloco `SQLITE_DEBUG` (rastreio do parser com `ParserTrace`);
//   - `sqlite3ParserAlloc()`/`sqlite3ParserFree()`: o amalgamation define
//     `sqlite3Parser_ENGINEALWAYSONSTACK`, então só `sqlite3ParserInit()` e
//     `sqlite3ParserFinalize()` são usados (o `YyParser` vive na pilha desta função);
//   - o bloco `YYTRACKMAXSTACKDEPTH` (não é definido);
//   - `addSpaceSeparator()` e `sqlite3Normalize()`: dependem de `SQLITE_ENABLE_NORMALIZE`,
//     que não está nas opções de compilação do Debian 13.
//
// Nota para a integração: `analyze_window_keyword`, `analyze_over_keyword` e
// `analyze_filter_keyword` são `fn` privadas da parte 000 deste módulo (no C são `static`
// do mesmo arquivo); precisam estar visíveis para esta parte (por exemplo `pub(super)`).
//
// Convenção de `Token.z` adotada pelo resto do porte (ver `build_c`): o texto vai do início do
// token até o fim do SQL, e `n` é o comprimento do token em bytes. Assim a diferença de
// ponteiros do C vira diferença de comprimentos.

/// Devolve o byte de `z[i]`, ou 0 além do fim (o C lê o terminador nulo da string).
#[inline]
fn sql_byte_at(z: &[u8], i: usize) -> u8 {
    z.get(i).copied().unwrap_or(0)
}

/// Roda o analisador sobre a string SQL dada.
pub fn run_parser(p_parse: &ParseRef, z_sql: &[u8]) -> i32 {
    let mut n_err: i32 = 0; // Número de erros encontrados
    let mut n: i32; // Comprimento do próximo token
    let mut token_type: i32 = 0; // Tipo do próximo token
    let mut last_token_parsed: i32 = -1; // Tipo do token anterior
    let db: Sqlite3Ref = p_parse
        .borrow()
        .db
        .upgrade()
        .expect("Parse.db deve apontar para uma conexão viva");
    let mut mx_sql_len: i32; // Tamanho máximo de uma string SQL
    let p_parent_parse: Option<ParseRef>; // Contexto de análise externo, se houver
    // Espaço para o objeto Parser gerado pelo LEMON (sqlite3Parser_ENGINEALWAYSONSTACK)
    let mut s_engine = YyParser {
        yytos: 0,
        yyerrcnt: 0,
        p_parse: p_parse.clone(),
        yystack_end: 0,
        yystack: Vec::new(),
    };
    let started_with_oom: u8 = db.borrow().malloc_failed;
    // Posição em z_sql do próximo byte não lido (o `zSql` do C avança como ponteiro)
    let mut i: usize = 0;

    mx_sql_len = db.borrow().a_limit[SQLITE_LIMIT_SQL_LENGTH as usize];
    if db.borrow().n_vdbe_active == 0 {
        db.borrow_mut().is_interrupted = 0;
    }
    {
        let mut p = p_parse.borrow_mut();
        p.rc = SQLITE_OK;
        p.z_tail = i;
    }
    parser_init(&mut s_engine, p_parse.clone());
    debug_assert!(p_parse.borrow().p_new_table.is_none());
    debug_assert!(p_parse.borrow().p_new_trigger.is_none());
    debug_assert!(p_parse.borrow().n_var == 0);
    debug_assert!(p_parse.borrow().p_vlist.is_empty());
    p_parent_parse = db.borrow_mut().p_parse.take();
    db.borrow_mut().p_parse = Some(p_parse.clone());
    loop {
        n = get_token(z_sql.get(i..).unwrap_or(&[]), &mut token_type);
        mx_sql_len -= n;
        if mx_sql_len < 0 {
            let mut p = p_parse.borrow_mut();
            p.rc = SQLITE_TOOBIG;
            p.n_err += 1;
            break;
        }
        // SQLITE_OMIT_WINDOWFUNC não está definido
        if token_type >= TK_WINDOW as i32 {
            debug_assert!(
                token_type == TK_SPACE as i32
                    || token_type == TK_OVER as i32
                    || token_type == TK_FILTER as i32
                    || token_type == TK_ILLEGAL as i32
                    || token_type == TK_WINDOW as i32
                    || token_type == TK_QNUMBER as i32
            );
            if db.borrow().is_interrupted != 0 {
                let mut p = p_parse.borrow_mut();
                p.rc = SQLITE_INTERRUPT;
                p.n_err += 1;
                break;
            }
            if token_type == TK_SPACE as i32 {
                i += n as usize;
                continue;
            }
            if sql_byte_at(z_sql, i) == 0 {
                // Ao chegar ao fim da entrada, chama o analisador mais duas vezes
                // com os tokens TK_SEMI e 0, nessa ordem.
                if last_token_parsed == TK_SEMI as i32 {
                    token_type = 0;
                } else if last_token_parsed == 0 {
                    break;
                } else {
                    token_type = TK_SEMI as i32;
                }
                n = 0;
            } else if token_type == TK_WINDOW as i32 {
                debug_assert!(n == 6);
                token_type = analyze_window_keyword(z_sql.get(i + 6..).unwrap_or(&[]));
            } else if token_type == TK_OVER as i32 {
                debug_assert!(n == 4);
                token_type =
                    analyze_over_keyword(z_sql.get(i + 4..).unwrap_or(&[]), last_token_parsed);
            } else if token_type == TK_FILTER as i32 {
                debug_assert!(n == 6);
                token_type =
                    analyze_filter_keyword(z_sql.get(i + 6..).unwrap_or(&[]), last_token_parsed);
            } else if token_type != TK_QNUMBER as i32 {
                let x = Token {
                    z: z_sql.get(i..).unwrap_or(&[]).to_vec(),
                    n: n as u32,
                };
                // sqlite3ErrorMsg(pParse, "unrecognized token: \"%T\"", &x): o texto do token
                // entra no formato com cada '%' dobrado, e o %T imprime exatamente x.n bytes.
                let mut z_msg: Vec<u8> = b"unrecognized token: \"".to_vec();
                for &c in &x.z[..(x.n as usize).min(x.z.len())] {
                    if c == b'%' {
                        z_msg.push(b'%');
                    }
                    z_msg.push(c);
                }
                z_msg.push(b'"');
                error_msg(&mut p_parse.borrow_mut(), Some(&z_msg));
                break;
            }
        }
        let s_last_token = Token {
            z: z_sql.get(i..).unwrap_or(&[]).to_vec(),
            n: n as u32,
        };
        p_parse.borrow_mut().s_last_token = s_last_token.clone();
        parser(&mut s_engine, token_type, s_last_token);
        last_token_parsed = token_type;
        i += n as usize;
        debug_assert!(
            db.borrow().malloc_failed == 0
                || p_parse.borrow().rc != SQLITE_OK
                || started_with_oom != 0
        );
        if p_parse.borrow().rc != SQLITE_OK {
            break;
        }
    }
    debug_assert!(n_err == 0);
    parser_finalize(&mut s_engine);
    if db.borrow().malloc_failed != 0 {
        p_parse.borrow_mut().rc = SQLITE_NOMEM_BKPT;
    }
    {
        let mut p = p_parse.borrow_mut();
        if p.z_err_msg.is_some() || (p.rc != SQLITE_OK && p.rc != SQLITE_DONE) {
            if p.z_err_msg.is_none() {
                // sqlite3MPrintf(db, "%s", sqlite3ErrStr(pParse->rc)) é uma cópia do texto
                p.z_err_msg = Some(err_str(p.rc).to_vec());
            }
            // sqlite3_log(pParse->rc, "%s in \"%s\"", pParse->zErrMsg, pParse->zTail)
            let z_err_msg_copy: Vec<u8> = p.z_err_msg.clone().unwrap_or_default();
            let tail_start: usize = p.z_tail.min(z_sql.len());
            let mut z_tail_copy: Vec<u8> = z_sql[tail_start..].to_vec();
            if let Some(nul) = z_tail_copy.iter().position(|&c| c == 0) {
                z_tail_copy.truncate(nul);
            }
            api_log(
                p.rc,
                b"%s in \"%s\"",
                &[Value::Text(z_err_msg_copy), Value::Text(z_tail_copy)],
            );
            n_err += 1;
        }
        p.z_tail = i;
        // sqlite3_free(pParse->apVtabLock)
        p.ap_vtab_lock = Vec::new();
    }

    let has_new_table = p_parse.borrow().p_new_table.is_some();
    if has_new_table && !in_special_parse(&p_parse.borrow()) {
        // Se o flag pParse->declareVtab está definido, não apaga a estrutura de tabela
        // montada em pParse->pNewTable. O código chamador (veja vtab.c) assume a
        // responsabilidade de liberar a estrutura Table.
        let p_new_table = p_parse.borrow_mut().p_new_table.take();
        delete_table(&db, p_new_table);
    }
    let has_new_trigger = p_parse.borrow().p_new_trigger.is_some();
    if has_new_trigger && !in_rename_object(&p_parse.borrow()) {
        let p_new_trigger = p_parse.borrow_mut().p_new_trigger.take();
        delete_trigger(&db, p_new_trigger);
    }
    if !p_parse.borrow().p_vlist.is_empty() {
        // sqlite3DbNNFreeNN(db, pParse->pVList)
        p_parse.borrow_mut().p_vlist = Vec::new();
    }
    db.borrow_mut().p_parse = p_parent_parse;
    debug_assert!(n_err == 0 || p_parse.borrow().rc != SQLITE_OK);
    n_err
}

