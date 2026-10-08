//! `fts3_unicode.c`: o tokenizador `unicode61` do FTS3. Um token é uma corrida de caracteres que o
//! `sqlite3FtsUnicodeIsalnum` classifica como letra ou número (com as exceções de `tokenchars=` e
//! `separators=`), dobrada para minúsculas e, conforme `remove_diacritics`, sem os diacríticos.

use std::rc::Rc;

use crate::consts::{SQLITE_DONE, SQLITE_ERROR};
use crate::utf::{utf8_read, write_utf8};

use super::int::{Fts3Token, Fts3Tokenizer, Fts3TokenizerCursor, Fts3TokenizerModule};
use super::unicode2::{fts_unicode_fold, fts_unicode_isalnum, fts_unicode_isdiacritic};

/// `unicode_tokenizer`.
struct UnicodeTokenizer {
    /// `eRemoveDiacritic`: 0 (mantém), 1 (remoção simples) ou 2 (remoção completa).
    e_remove_diacritic: i32,
    /// `aiException`/`nException`: os pontos de código cujo resultado de `IsAlnum` é invertido,
    /// em ordem crescente.
    ai_exception: Rc<[i32]>,
}

/// `unicode_cursor`.
struct UnicodeCursor {
    /// `aInput`/`nInput`.
    a_input: Vec<u8>,
    /// `iOff`: o deslocamento corrente em `a_input`.
    i_off: usize,
    /// `iToken`: o índice do próximo token.
    i_token: i32,
    /// `zToken`: o armazenamento do token corrente.
    z_token: Vec<u8>,
    /// `pCsr->base.pTokenizer->eRemoveDiacritic`.
    e_remove_diacritic: i32,
    /// `pCsr->base.pTokenizer->aiException`.
    ai_exception: Rc<[i32]>,
}

/// O módulo `unicode61` (o `module` estático de `sqlite3Fts3UnicodeTokenizer`).
struct UnicodeModule;

/// `sqlite3Fts3UnicodeTokenizer`: o módulo do tokenizador `unicode61`.
pub fn fts3_unicode_tokenizer_module() -> Rc<dyn Fts3TokenizerModule> {
    Rc::new(UnicodeModule)
}

/// `unicodeAddExceptions`: o `CREATE VIRTUAL TABLE` pediu (`tokenchars=`, `b_alnum` 1) ou negou
/// (`separators=`, `b_alnum` 0) que os caracteres de `z_in` sejam de token. Para cada ponto de
/// código em que `IsAlnum` ainda não dá o resultado pedido, ele entra em `ai_exception`, que é
/// mantido em ordem crescente. Um diacrítico solto em `z_in` é ignorado: o comportamento do
/// tokenizador para eles não pode ser mudado.
fn unicode_add_exceptions(ai_exception: &mut Vec<i32>, b_alnum: bool, z_in: &[u8]) {
    let mut z = 0usize;
    while z < z_in.len() {
        let i_code = utf8_read(z_in, &mut z) as i32;
        if fts_unicode_isalnum(i_code) != b_alnum && !fts_unicode_isdiacritic(i_code) {
            let i = ai_exception.iter().position(|&e| e >= i_code).unwrap_or(ai_exception.len());
            ai_exception.insert(i, i_code);
        }
    }
}

/// `unicodeIsException`: `ai_exception` contém `i_code`?
fn unicode_is_exception(ai_exception: &[i32], i_code: i32) -> bool {
    ai_exception.binary_search(&i_code).is_ok()
}

/// `unicodeIsAlnum`: `i_code` é, para a tokenização, um caractere de token (não um separador)?
fn unicode_is_alnum(ai_exception: &[i32], i_code: i32) -> bool {
    fts_unicode_isalnum(i_code) != unicode_is_exception(ai_exception, i_code)
}

impl Fts3TokenizerModule for UnicodeModule {
    /// `unicodeCreate`: argumentos `remove_diacritics=0|1|2`, `tokenchars=...` e `separators=...`;
    /// qualquer outro é erro.
    fn create(&self, args: &[Vec<u8>]) -> Result<Rc<dyn Fts3Tokenizer>, i32> {
        let mut e_remove_diacritic = 1;
        let mut ai_exception: Vec<i32> = Vec::new();

        for arg in args {
            let n = arg.iter().position(|&c| c == 0).unwrap_or(arg.len());
            let z = &arg[..n];
            if z == b"remove_diacritics=1" {
                e_remove_diacritic = 1;
            } else if z == b"remove_diacritics=0" {
                e_remove_diacritic = 0;
            } else if z == b"remove_diacritics=2" {
                e_remove_diacritic = 2;
            } else if n >= 11 && &z[..11] == b"tokenchars=" {
                unicode_add_exceptions(&mut ai_exception, true, &z[11..]);
            } else if n >= 11 && &z[..11] == b"separators=" {
                unicode_add_exceptions(&mut ai_exception, false, &z[11..]);
            } else {
                /* Argumento desconhecido. */
                return Err(SQLITE_ERROR);
            }
        }

        Ok(Rc::new(UnicodeTokenizer { e_remove_diacritic, ai_exception: Rc::from(ai_exception) }))
    }
}

impl Fts3Tokenizer for UnicodeTokenizer {
    /// `unicodeOpen`.
    fn open(&self, input: &[u8]) -> Result<Box<dyn Fts3TokenizerCursor>, i32> {
        Ok(Box::new(UnicodeCursor {
            a_input: input.to_vec(),
            i_off: 0,
            i_token: 0,
            z_token: Vec::new(),
            e_remove_diacritic: self.e_remove_diacritic,
            ai_exception: Rc::clone(&self.ai_exception),
        }))
    }
}

impl Fts3TokenizerCursor for UnicodeCursor {
    /// `unicodeNext`.
    fn next(&mut self) -> Result<Fts3Token<'_>, i32> {
        let a = &self.a_input;
        let z_term = a.len();
        let mut i_code: u32 = 0;
        let mut z = self.i_off;
        let mut z_start = z;

        /* Pula os separadores antes do começo do próximo token. Devolve SQLITE_DONE se isso leva ao
        ** fim da entrada. */
        while z < z_term {
            i_code = utf8_read(a, &mut z);
            if unicode_is_alnum(&self.ai_exception, i_code as i32) {
                break;
            }
            z_start = z;
        }
        if z_start >= z_term {
            return Err(SQLITE_DONE);
        }

        self.z_token.clear();
        let mut z_end;
        loop {
            /* Grava a forma dobrada do último caractere lido na saída. */
            z_end = z;
            let i_out = fts_unicode_fold(i_code as i32, self.e_remove_diacritic);
            if i_out != 0 {
                write_utf8(&mut self.z_token, i_out as u32);
            }

            /* Se o cursor não está no fim, lê o caractere seguinte. */
            if z >= z_term {
                break;
            }
            i_code = utf8_read(a, &mut z);
            if !(unicode_is_alnum(&self.ai_exception, i_code as i32) || fts_unicode_isdiacritic(i_code as i32)) {
                break;
            }
        }

        /* Grava as saídas e volta. */
        self.i_off = z;
        let i_position = self.i_token;
        self.i_token += 1;
        Ok(Fts3Token {
            z: &self.z_token,
            i_start_offset: z_start as i32,
            i_end_offset: z_end as i32,
            i_position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], text: &str) -> Vec<(String, i32, i32)> {
        let args: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
        let tok = UnicodeModule.create(&args).unwrap();
        let mut csr = tok.open(text.as_bytes()).unwrap();
        let mut out = Vec::new();
        while let Ok(t) = csr.next() {
            out.push((String::from_utf8(t.z.to_vec()).unwrap(), t.i_start_offset, t.i_end_offset));
        }
        out
    }

    #[test]
    fn folds_and_removes_diacritics() {
        let v = run(&[], "Café au LAIT, ÉTÉ");
        let words: Vec<&str> = v.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(words, vec!["cafe", "au", "lait", "ete"]);
        assert_eq!((v[0].1, v[0].2), (0, 5));
    }

    #[test]
    fn options() {
        let v = run(&["remove_diacritics=0"], "Café");
        assert_eq!(v[0].0, "café");
        let v = run(&["tokenchars=-"], "a-b c");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "a-b");
        let v = run(&["separators=b"], "abc");
        assert_eq!(v.len(), 2);
        assert!(UnicodeModule.create(&[b"foo=1".to_vec()]).is_err());
    }
}
