// Mesclado das partes traduzidas de complete_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Tipos de token usados pela rotina complete(). Veja o comentário de cabeçalho dela.
pub const TK_SEMI: u8 = 0;
pub const TK_WS: u8 = 1;
pub const TK_OTHER: u8 = 2;
pub const TK_EXPLAIN: u8 = 3;
pub const TK_CREATE: u8 = 4;
pub const TK_TEMP: u8 = 5;
pub const TK_TRIGGER: u8 = 6;
pub const TK_END: u8 = 7;

/// Tabela de transição da máquina de estados do `complete`.
/// Linhas: estado (INVALID, START, NORMAL, EXPLAIN, CREATE, TRIGGER, SEMI, END).
/// Colunas: token (SEMI, WS, OTHER, EXPLAIN, CREATE, TEMP, TRIGGER, END).
static TRANS: [[u8; 8]; 8] = [
    /* 0 INVALID: */ [1, 0, 2, 3, 4, 2, 2, 2],
    /* 1   START: */ [1, 1, 2, 3, 4, 2, 2, 2],
    /* 2  NORMAL: */ [1, 2, 2, 2, 2, 2, 2, 2],
    /* 3 EXPLAIN: */ [1, 3, 3, 2, 4, 2, 2, 2],
    /* 4  CREATE: */ [1, 4, 2, 2, 2, 4, 5, 2],
    /* 5 TRIGGER: */ [6, 5, 5, 5, 5, 5, 5, 5],
    /* 6    SEMI: */ [6, 6, 5, 5, 5, 5, 5, 7],
    /* 7     END: */ [1, 7, 5, 5, 5, 5, 5, 5],
];

/// Devolve verdadeiro (1) se a string SQL termina em ponto e vírgula.
///
/// Tratamento especial para CREATE TRIGGER: o comando precisa terminar com ";END;".
/// A entrada é a string C sem o NUL final; o byte além do fim lê como 0, como o
/// terminador do C.
pub fn complete(z_sql: &[u8]) -> i32 {
    let mut state: u8 = 0; /* Estado atual, com os números do comentário de cabeçalho */
    let mut token: u8; /* Valor do próximo token */
    let mut i: usize = 0;

    // Leitura com NUL implícito no fim.
    let at = |idx: usize| -> u8 {
        if idx < z_sql.len() {
            z_sql[idx]
        } else {
            0
        }
    };

    while at(i) != 0 {
        match at(i) {
            b';' => {
                /* Um ponto e vírgula */
                token = TK_SEMI;
            }
            b' ' | b'\r' | b'\t' | b'\n' | 0x0c => {
                /* Espaço em branco é ignorado */
                token = TK_WS;
            }
            b'/' => {
                /* Comentários no estilo C */
                if at(i + 1) != b'*' {
                    token = TK_OTHER;
                } else {
                    i += 2;
                    while at(i) != 0 && (at(i) != b'*' || at(i + 1) != b'/') {
                        i += 1;
                    }
                    if at(i) == 0 {
                        return 0;
                    }
                    i += 1;
                    token = TK_WS;
                }
            }
            b'-' => {
                /* Comentários SQL de "--" até o fim da linha */
                if at(i + 1) != b'-' {
                    token = TK_OTHER;
                } else {
                    while at(i) != 0 && at(i) != b'\n' {
                        i += 1;
                    }
                    if at(i) == 0 {
                        return (state == 1) as i32;
                    }
                    token = TK_WS;
                }
            }
            b'[' => {
                /* Identificadores no estilo Microsoft, em [...] */
                i += 1;
                while at(i) != 0 && at(i) != b']' {
                    i += 1;
                }
                if at(i) == 0 {
                    return 0;
                }
                token = TK_OTHER;
            }
            b'`' | b'"' | b'\'' => {
                /* Símbolos entre crase (MySQL) e strings entre aspas simples ou duplas */
                let c = at(i);
                i += 1;
                while at(i) != 0 && at(i) != c {
                    i += 1;
                }
                if at(i) == 0 {
                    return 0;
                }
                token = TK_OTHER;
            }
            _ => {
                if id_char(at(i)) {
                    /* Palavras-chave e identificadores sem aspas */
                    let mut n_id: usize = 1;
                    while id_char(at(i + n_id)) {
                        n_id += 1;
                    }
                    match at(i) {
                        b'c' | b'C' => {
                            if n_id == 6 && str_n_i_cmp(&z_sql[i..], b"create", 6) == 0 {
                                token = TK_CREATE;
                            } else {
                                token = TK_OTHER;
                            }
                        }
                        b't' | b'T' => {
                            if n_id == 7 && str_n_i_cmp(&z_sql[i..], b"trigger", 7) == 0 {
                                token = TK_TRIGGER;
                            } else if n_id == 4 && str_n_i_cmp(&z_sql[i..], b"temp", 4) == 0 {
                                token = TK_TEMP;
                            } else if n_id == 9 && str_n_i_cmp(&z_sql[i..], b"temporary", 9) == 0 {
                                token = TK_TEMP;
                            } else {
                                token = TK_OTHER;
                            }
                        }
                        b'e' | b'E' => {
                            if n_id == 3 && str_n_i_cmp(&z_sql[i..], b"end", 3) == 0 {
                                token = TK_END;
                            } else if n_id == 7 && str_n_i_cmp(&z_sql[i..], b"explain", 7) == 0 {
                                token = TK_EXPLAIN;
                            } else {
                                token = TK_OTHER;
                            }
                        }
                        _ => {
                            token = TK_OTHER;
                        }
                    }
                    i += n_id - 1;
                } else {
                    /* Operadores e símbolos especiais */
                    token = TK_OTHER;
                }
            }
        }
        state = TRANS[state as usize][token as usize];
        i += 1;
    }
    (state == 1) as i32
}

/// Mesma rotina de `complete`, exceto que o parâmetro precisa estar em UTF-16 (ordem de
/// bytes nativa) e não em UTF-8. A entrada é a string sem o terminador de 16 bits.
pub fn complete16(z_sql: &[u8]) -> i32 {
    let mut rc: i32;

    rc = api::initialize();
    if rc != 0 {
        return rc;
    }
    let mut p_val = value_new(None);
    value_set_str(&mut p_val, -1, z_sql, SQLITE_UTF16NATIVE, SQLITE_STATIC);
    let z_sql8 = value_text(&mut p_val, SQLITE_UTF8);
    if let Some(z8) = z_sql8 {
        rc = complete(&z8);
    } else {
        rc = SQLITE_NOMEM;
    }
    value_free(p_val);
    rc & 0xff
}

