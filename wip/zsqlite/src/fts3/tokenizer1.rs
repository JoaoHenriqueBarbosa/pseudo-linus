//! `fts3_tokenizer1.c`: o tokenizador `simple` do FTS3. Separa o texto nos delimitadores (por
//! padrão os caracteres ASCII que não são letra nem dígito) e dobra as letras ASCII para
//! minúsculas. Bytes não ASCII nunca são delimitadores.

use std::rc::Rc;

use crate::consts::{SQLITE_DONE, SQLITE_ERROR};

use super::int::{
    scan_delimited_token, Fts3Token, Fts3Tokenizer, Fts3TokenizerCursor, Fts3TokenizerModule,
};

/// `simple_tokenizer`.
struct SimpleTokenizer {
    /// `delim`: marca os delimitadores ASCII (diferente de zero é delimitador).
    delim: [u8; 128],
}

/// `simple_tokenizer_cursor`.
struct SimpleTokenizerCursor {
    /// `pInput`/`nBytes`: a entrada.
    input: Vec<u8>,
    /// `iOffset`: a posição corrente em `input`.
    i_offset: usize,
    /// `iToken`: o índice do próximo token.
    i_token: i32,
    /// `pToken`: o armazenamento do token corrente.
    token: Vec<u8>,
    /// Cópia dos delimitadores do tokenizador (`pCursor->pTokenizer`).
    delim: [u8; 128],
}

/// `simpleDelim`.
#[inline]
fn simple_delim(delim: &[u8; 128], c: u8) -> bool {
    c < 0x80 && delim[c as usize] != 0
}

/// `fts3_isalnum`.
#[inline]
fn fts3_isalnum(x: u8) -> bool {
    x.is_ascii_alphanumeric()
}

/// O módulo `simple` (`simpleTokenizerModule`).
struct SimpleModule;

/// `sqlite3Fts3SimpleTokenizerModule`: o módulo do tokenizador `simple`.
pub fn fts3_simple_tokenizer_module() -> Rc<dyn Fts3TokenizerModule> {
    Rc::new(SimpleModule)
}

impl Fts3TokenizerModule for SimpleModule {
    /// `simpleCreate`: com mais de um argumento o segundo lista os delimitadores (só ASCII); sem
    /// ele, todo ASCII que não é alfanumérico delimita.
    fn create(&self, args: &[Vec<u8>]) -> Result<Rc<dyn Fts3Tokenizer>, i32> {
        let mut t = SimpleTokenizer { delim: [0; 128] };

        /* Nota do autor do C: os delimitadores precisam ser os mesmos de uma execução para outra,
        ** senão é preciso reindexar. */
        if args.len() > 1 {
            let a = &args[1];
            let n = a.iter().position(|&c| c == 0).unwrap_or(a.len());
            for &ch in &a[..n] {
                /* UTF-8 não é aceito como delimitador por enquanto. */
                if ch >= 0x80 {
                    return Err(SQLITE_ERROR);
                }
                t.delim[ch as usize] = 1;
            }
        } else {
            /* Os ASCII não alfanuméricos são delimitadores (o NUL, índice 0, não é). */
            for i in 1..0x80usize {
                t.delim[i] = if !fts3_isalnum(i as u8) { 0xFF } else { 0 };
            }
        }
        Ok(Rc::new(t))
    }
}

impl Fts3Tokenizer for SimpleTokenizer {
    /// `simpleOpen`.
    fn open(&self, input: &[u8]) -> Result<Box<dyn Fts3TokenizerCursor>, i32> {
        Ok(Box::new(SimpleTokenizerCursor {
            input: input.to_vec(),
            i_offset: 0,
            i_token: 0,
            token: Vec::new(),
            delim: self.delim,
        }))
    }
}

impl Fts3TokenizerCursor for SimpleTokenizerCursor {
    /// `simpleNext`.
    fn next(&mut self) -> Result<Fts3Token<'_>, i32> {
        let delim = &self.delim;
        let Some((i_start_offset, i_end_offset)) =
            scan_delimited_token(&self.input, &mut self.i_offset, |c| simple_delim(delim, c))
        else {
            return Err(SQLITE_DONE);
        };

        /* Só as letras ASCII são dobradas; a caixa dos caracteres UTF-8 não é tratada. */
        self.token.clear();
        self.token.extend(self.input[i_start_offset..i_end_offset].iter().map(|ch| ch.to_ascii_lowercase()));
        let i_position = self.i_token;
        self.i_token += 1;
        Ok(Fts3Token {
            z: &self.token,
            i_start_offset: i_start_offset as i32,
            i_end_offset: i_end_offset as i32,
            i_position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(module: &dyn Fts3TokenizerModule, args: &[Vec<u8>], text: &[u8]) -> Vec<(Vec<u8>, i32, i32, i32)> {
        let tok = module.create(args).unwrap();
        let mut csr = tok.open(text).unwrap();
        let mut out = Vec::new();
        while let Ok(t) = csr.next() {
            out.push((t.z.to_vec(), t.i_start_offset, t.i_end_offset, t.i_position));
        }
        out
    }

    #[test]
    fn default_delimiters() {
        let m = SimpleModule;
        let v = tokens(&m, &[], b"I don't SEE how");
        let words: Vec<Vec<u8>> = v.iter().map(|t| t.0.clone()).collect();
        let expected: Vec<Vec<u8>> =
            ["i", "don", "t", "see", "how"].iter().map(|w| w.as_bytes().to_vec()).collect();
        assert_eq!(words, expected);
        assert_eq!(v[1].1, 2);
        assert_eq!(v[1].2, 5);
        assert_eq!(v[4].3, 4);
    }

    #[test]
    fn explicit_delimiters_and_utf8_rejected() {
        let m = SimpleModule;
        let v = tokens(&m, &[b"x".to_vec(), b",".to_vec()], b"a b,c");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, b"a b".to_vec());
        assert!(m.create(&[b"x".to_vec(), "é".as_bytes().to_vec()]).is_err());
    }
}
