//! Tradução de complete.c: `sqlite3_complete`, a detecção de fim de instrução SQL.
//!
//! `sqlite3_complete16` fica adiada: converte a entrada UTF-16 com `Mem` (`sqlite3ValueNew`,
//! `sqlite3ValueSetStr`, `sqlite3ValueText`), que ainda não existe.

use crate::tokenize::{at, is_id_char};
use crate::util::strnicmp as str_nicmp;

// Tipos de token usados por `complete`.
const TOK_SEMI: usize = 0; // tkSEMI
const TOK_WS: usize = 1; // tkWS
const TOK_OTHER: usize = 2; // tkOTHER
const TOK_EXPLAIN: usize = 3; // tkEXPLAIN
const TOK_CREATE: usize = 4; // tkCREATE
const TOK_TEMP: usize = 5; // tkTEMP
const TOK_TRIGGER: usize = 6; // tkTRIGGER
const TOK_END: usize = 7; // tkEND

/// `sqlite3_complete`: verdadeiro (1) se `z` termina num ponto e vírgula que não faz parte de
/// texto ou comentário. Em `CREATE TRIGGER` a instrução tem de terminar com ";END;".
///
/// Máquina de estados com 8 estados:
///
///   (0) INVALID  ainda não houve caractere diferente de espaço.
///   (1) START    início ou fim de instrução; só neste estado o resultado é 1.
///   (2) NORMAL   no meio de uma instrução que termina com um único ponto e vírgula.
///   (3) EXPLAIN  EXPLAIN visto no começo da instrução.
///   (4) CREATE   CREATE visto no começo (talvez após EXPLAIN, ou seguido de TEMP/TEMPORARY).
///   (5) TRIGGER  no meio de uma definição de trigger, que acaba em ";", END e ";".
///   (6) SEMI     primeiro ponto e vírgula do ";END;" final do trigger.
///   (7) END      o ";END" do ";END;" final do trigger.
pub fn sqlite3_complete(z: &[u8]) -> i32 {
    let mut state: usize = 0; // estado atual, numerado como no comentário acima
    // Linhas: estados. Colunas: SEMI WS OTHER EXPLAIN CREATE TEMP TRIGGER END.
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
    let mut p: usize = 0; // posição em z (o zSql do C)
    while at(z, p) != 0 {
        // Valor do próximo token
        let token = match at(z, p) {
            b';' => TOK_SEMI, // Um ponto e vírgula
            b' ' | b'\r' | b'\t' | b'\n' | 0x0c => TOK_WS, // Espaço em branco é ignorado
            b'/' => {
                // Comentários no estilo C
                if at(z, p + 1) != b'*' {
                    TOK_OTHER
                } else {
                    p += 2;
                    while at(z, p) != 0 && (at(z, p) != b'*' || at(z, p + 1) != b'/') {
                        p += 1;
                    }
                    if at(z, p) == 0 {
                        return 0;
                    }
                    p += 1;
                    TOK_WS
                }
            }
            b'-' => {
                // Comentários SQL de "--" até o fim da linha
                if at(z, p + 1) != b'-' {
                    TOK_OTHER
                } else {
                    while at(z, p) != 0 && at(z, p) != b'\n' {
                        p += 1;
                    }
                    if at(z, p) == 0 {
                        return (state == 1) as i32;
                    }
                    TOK_WS
                }
            }
            b'[' => {
                // Identificadores no estilo Microsoft, entre [...]
                p += 1;
                while at(z, p) != 0 && at(z, p) != b']' {
                    p += 1;
                }
                if at(z, p) == 0 {
                    return 0;
                }
                TOK_OTHER
            }
            b'`' | b'"' | b'\'' => {
                // Símbolos entre crase (MySQL) e textos entre aspas simples ou duplas
                let c = at(z, p);
                p += 1;
                while at(z, p) != 0 && at(z, p) != c {
                    p += 1;
                }
                if at(z, p) == 0 {
                    return 0;
                }
                TOK_OTHER
            }
            first => {
                if is_id_char(first) {
                    // Palavras-chave e identificadores sem aspas
                    let mut n_id = 1;
                    while is_id_char(at(z, p + n_id)) {
                        n_id += 1;
                    }
                    let word = &z[p..];
                    let token = match first {
                        b'c' | b'C' => {
                            if n_id == 6 && str_nicmp(Some(word), Some(b"create"), 6) == 0 {
                                TOK_CREATE
                            } else {
                                TOK_OTHER
                            }
                        }
                        b't' | b'T' => {
                            if n_id == 7 && str_nicmp(Some(word), Some(b"trigger"), 7) == 0 {
                                TOK_TRIGGER
                            } else if n_id == 4 && str_nicmp(Some(word), Some(b"temp"), 4) == 0 {
                                TOK_TEMP
                            } else if n_id == 9 && str_nicmp(Some(word), Some(b"temporary"), 9) == 0 {
                                TOK_TEMP
                            } else {
                                TOK_OTHER
                            }
                        }
                        b'e' | b'E' => {
                            if n_id == 3 && str_nicmp(Some(word), Some(b"end"), 3) == 0 {
                                TOK_END
                            } else if n_id == 7 && str_nicmp(Some(word), Some(b"explain"), 7) == 0 {
                                TOK_EXPLAIN
                            } else {
                                TOK_OTHER
                            }
                        }
                        _ => TOK_OTHER,
                    };
                    p += n_id - 1;
                    token
                } else {
                    // Operadores e símbolos especiais
                    TOK_OTHER
                }
            }
        };
        state = TRANS[state][token] as usize;
        p += 1;
    }
    (state == 1) as i32
}
