//! `fts5_tokenize.c`: os tokenizadores embutidos do FTS5: `ascii`, `unicode61`, `porter` e
//! `trigram`, e o registro deles (`sqlite3Fts5TokenizerInit`).
//!
//! Modelo v2: cada tokenizador é um tipo que implementa [`Fts5Tokenizer`] (imutável: o buffer de
//! dobra de caixa `aFold`/`aBuf` do C, que era estado da instância, é uma variável local da
//! chamada, o que também deixa a tokenização reentrante) e uma fábrica que implementa
//! [`Fts5TokenizerFactory`] (o `xCreate`). O `xDelete` é o `Drop`. `sqlite3Fts5TokenizerPattern`
//! é o método [`Fts5Tokenizer::pattern`] (só o trigram o sobrescreve).
//!
//! As regras do Porter, que o C gera em cadeias de `switch`/`if`, são tabelas ([`PorterRule`])
//! percorridas na mesma ordem: dentro de um mesmo grupo do `switch` vale o primeiro sufixo que
//! casa (mesmo que a condição falhe), e grupos diferentes nunca casam juntos.

use std::rc::Rc;

use crate::consts::{SQLITE_DONE, SQLITE_ERROR, SQLITE_OK};
use crate::util::{at, str_icmp};
use crate::utf::{utf8_read, write_utf8};

use super::int::{
    Fts5Api, Fts5TokenFn, Fts5Tokenizer, Fts5TokenizerFactory, FTS5_PATTERN_GLOB,
    FTS5_PATTERN_LIKE, FTS5_PATTERN_NONE,
};
use super::unicode2::{
    fts5_unicode_ascii, fts5_unicode_cat_parse, fts5_unicode_category, fts5_unicode_fold,
    fts5_unicode_isdiacritic,
};

// ---------------------------------------------------------------------------------------------
// ascii
// ---------------------------------------------------------------------------------------------

/// Para tokenizadores sem o modificador "unicode", os caracteres de token são os alfanuméricos
/// da faixa ASCII.
#[rustfmt::skip]
static A_ASCII_TOKEN_CHAR: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0,   0, 0, 0, 0, 0, 0, 0, 0,   /* 0x00..0x0F */
    0, 0, 0, 0, 0, 0, 0, 0,   0, 0, 0, 0, 0, 0, 0, 0,   /* 0x10..0x1F */
    0, 0, 0, 0, 0, 0, 0, 0,   0, 0, 0, 0, 0, 0, 0, 0,   /* 0x20..0x2F */
    1, 1, 1, 1, 1, 1, 1, 1,   1, 1, 0, 0, 0, 0, 0, 0,   /* 0x30..0x3F */
    0, 1, 1, 1, 1, 1, 1, 1,   1, 1, 1, 1, 1, 1, 1, 1,   /* 0x40..0x4F */
    1, 1, 1, 1, 1, 1, 1, 1,   1, 1, 1, 0, 0, 0, 0, 0,   /* 0x50..0x5F */
    0, 1, 1, 1, 1, 1, 1, 1,   1, 1, 1, 1, 1, 1, 1, 1,   /* 0x60..0x6F */
    1, 1, 1, 1, 1, 1, 1, 1,   1, 1, 1, 0, 0, 0, 0, 0,   /* 0x70..0x7F */
];

/// `AsciiTokenizer`.
struct AsciiTokenizer {
    a_token_char: [u8; 128],
}

/// `fts5AsciiAddExceptions`: os caracteres ASCII de `z_arg` viram (ou deixam de ser) de token.
fn ascii_add_exceptions(p: &mut AsciiTokenizer, z_arg: &[u8], b_token_chars: u8) {
    for &c in z_arg.iter().take_while(|&&c| c != 0) {
        if (c & 0x80) == 0 {
            p.a_token_char[c as usize] = b_token_chars;
        }
    }
}

/// A fábrica do tokenizador `ascii` (`fts5AsciiCreate`).
struct AsciiFactory;

impl Fts5TokenizerFactory for AsciiFactory {
    fn create(&self, _api: &dyn Fts5Api, args: &[Vec<u8>]) -> Result<Rc<dyn Fts5Tokenizer>, i32> {
        let n_arg = args.len();
        let mut rc = SQLITE_OK;
        if n_arg % 2 != 0 {
            return Err(SQLITE_ERROR);
        }
        let mut p = AsciiTokenizer { a_token_char: A_ASCII_TOKEN_CHAR };
        let mut i = 0;
        while rc == SQLITE_OK && i + 1 < n_arg {
            let z_arg = &args[i + 1];
            if 0 == str_icmp(&args[i], b"tokenchars") {
                ascii_add_exceptions(&mut p, z_arg, 1);
            } else if 0 == str_icmp(&args[i], b"separators") {
                ascii_add_exceptions(&mut p, z_arg, 0);
            } else {
                rc = SQLITE_ERROR;
            }
            i += 2;
        }
        if rc == SQLITE_OK && i < n_arg {
            rc = SQLITE_ERROR;
        }
        if rc != SQLITE_OK {
            return Err(rc);
        }
        Ok(Rc::new(p))
    }
}

impl Fts5Tokenizer for AsciiTokenizer {
    /// `fts5AsciiTokenize`.
    fn tokenize(&self, _flags: i32, text: &[u8], x_token: Fts5TokenFn<'_>) -> i32 {
        let a = &self.a_token_char;
        let n_text = text.len();
        let mut rc = SQLITE_OK;
        let mut is = 0usize;
        let mut fold: Vec<u8> = Vec::with_capacity(64);

        while is < n_text && rc == SQLITE_OK {
            /* Pula os caracteres divisores do começo. */
            while is < n_text && ((text[is] & 0x80) == 0 && a[text[is] as usize] == 0) {
                is += 1;
            }
            if is == n_text {
                break;
            }

            /* Conta os caracteres do token */
            let mut ie = is + 1;
            while ie < n_text && ((text[ie] & 0x80) != 0 || a[text[ie] as usize] != 0) {
                ie += 1;
            }

            /* Dobra para minúsculas */
            fold.clear();
            fold.extend(text[is..ie].iter().map(|c| c.to_ascii_lowercase()));

            /* Chama o callback do token */
            rc = x_token(0, &fold, is as i32, ie as i32);
            is = ie + 1;
        }

        if rc == SQLITE_DONE {
            rc = SQLITE_OK;
        }
        rc
    }
}

// ---------------------------------------------------------------------------------------------
// unicode61
// ---------------------------------------------------------------------------------------------

/// Valores de `eRemoveDiacritic` (precisam casar com os internos do `unicode2.rs`).
const FTS5_REMOVE_DIACRITICS_NONE: i32 = 0;
const FTS5_REMOVE_DIACRITICS_SIMPLE: i32 = 1;
const FTS5_REMOVE_DIACRITICS_COMPLEX: i32 = 2;

/// `Unicode61Tokenizer`.
struct Unicode61Tokenizer {
    /// Caracteres de token da faixa ASCII.
    a_token_char: [u8; 128],
    /// `eRemoveDiacritic`.
    e_remove_diacritic: i32,
    /// Exceções (`tokenchars`/`separators` fora do ASCII), em ordem crescente.
    ai_exception: Vec<i32>,
    /// Verdadeiro para as categorias que são de token.
    a_category: [u8; 32],
}

impl Unicode61Tokenizer {
    /// `fts5UnicodeAddExceptions`: os caracteres de `z` viram (`b_token_chars` = 1) ou deixam de
    /// ser (0) de token.
    fn add_exceptions(&mut self, z: &[u8], b_token_chars: u8) {
        let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
        if n > 0 {
            let z = &z[..n];
            let mut z_csr = 0usize;
            while z_csr < z.len() {
                let i_code = utf8_read(z, &mut z_csr);
                if i_code < 128 {
                    self.a_token_char[i_code as usize] = b_token_chars;
                } else {
                    let b_token = self.a_category[fts5_unicode_category(i_code) as usize];
                    debug_assert!(b_token == 0 || b_token == 1);
                    debug_assert!(b_token_chars == 0 || b_token_chars == 1);
                    if b_token != b_token_chars && !fts5_unicode_isdiacritic(i_code as i32) {
                        let i = self
                            .ai_exception
                            .iter()
                            .position(|&e| (e as u32) > i_code)
                            .unwrap_or(self.ai_exception.len());
                        self.ai_exception.insert(i, i_code as i32);
                    }
                }
            }
        }
    }

    /// `fts5UnicodeIsException`: verdadeiro se `ai_exception` contém `i_code`.
    fn is_exception(&self, i_code: i32) -> bool {
        self.ai_exception.binary_search(&i_code).is_ok()
    }

    /// `fts5UnicodeIsAlnum`: verdadeiro se `i_code` é caractere de token para este tokenizador.
    fn is_alnum(&self, i_code: u32) -> bool {
        (self.a_category[fts5_unicode_category(i_code) as usize] != 0)
            ^ self.is_exception(i_code as i32)
    }

    /// Dobra `i_code` (caixa e diacríticos conforme `eRemoveDiacritic`) e o acrescenta a `fold`
    /// em UTF-8; um resultado zero (diacrítico removido) não acrescenta nada.
    fn fold_into(&self, fold: &mut Vec<u8>, i_code: u32) {
        let folded = fts5_unicode_fold(i_code as i32, self.e_remove_diacritic);
        if folded != 0 {
            write_utf8(fold, folded as u32);
        }
    }

    /// `unicodeSetCategories`.
    fn set_categories(&mut self, z_cat: &[u8]) -> i32 {
        let mut z = 0usize;
        while at(z_cat, z) != 0 {
            while at(z_cat, z) == b' ' || at(z_cat, z) == b'\t' {
                z += 1;
            }
            if at(z_cat, z) != 0
                && fts5_unicode_cat_parse(z_cat.get(z..).unwrap_or(&[]), &mut self.a_category) != 0
            {
                return SQLITE_ERROR;
            }
            while at(z_cat, z) != b' ' && at(z_cat, z) != b'\t' && at(z_cat, z) != 0 {
                z += 1;
            }
        }
        fts5_unicode_ascii(&self.a_category, &mut self.a_token_char);
        SQLITE_OK
    }
}

/// A fábrica do tokenizador `unicode61` (`fts5UnicodeCreate`).
struct Unicode61Factory;

impl Fts5TokenizerFactory for Unicode61Factory {
    fn create(&self, _api: &dyn Fts5Api, args: &[Vec<u8>]) -> Result<Rc<dyn Fts5Tokenizer>, i32> {
        let n_arg = args.len();
        if n_arg % 2 != 0 {
            return Err(SQLITE_ERROR);
        }
        let mut z_cat: &[u8] = b"L* N* Co";
        let mut p = Unicode61Tokenizer {
            a_token_char: [0; 128],
            e_remove_diacritic: FTS5_REMOVE_DIACRITICS_SIMPLE,
            ai_exception: Vec::new(),
            a_category: [0; 32],
        };

        /* Procura um argumento "categories" */
        let mut i = 0;
        while i + 1 < n_arg {
            if 0 == str_icmp(&args[i], b"categories") {
                z_cat = &args[i + 1];
            }
            i += 2;
        }

        let mut rc = p.set_categories(z_cat);

        i = 0;
        while rc == SQLITE_OK && i + 1 < n_arg {
            let z_arg: &[u8] = &args[i + 1];
            if 0 == str_icmp(&args[i], b"remove_diacritics") {
                if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1' && at(z_arg, 0) != b'2')
                    || at(z_arg, 1) != 0
                {
                    rc = SQLITE_ERROR;
                } else {
                    p.e_remove_diacritic = (at(z_arg, 0) - b'0') as i32;
                    debug_assert!(
                        p.e_remove_diacritic == FTS5_REMOVE_DIACRITICS_NONE
                            || p.e_remove_diacritic == FTS5_REMOVE_DIACRITICS_SIMPLE
                            || p.e_remove_diacritic == FTS5_REMOVE_DIACRITICS_COMPLEX
                    );
                }
            } else if 0 == str_icmp(&args[i], b"tokenchars") {
                p.add_exceptions(z_arg, 1);
            } else if 0 == str_icmp(&args[i], b"separators") {
                p.add_exceptions(z_arg, 0);
            } else if 0 == str_icmp(&args[i], b"categories") {
                /* no-op */
            } else {
                rc = SQLITE_ERROR;
            }
            i += 2;
        }

        if i < n_arg && rc == SQLITE_OK {
            rc = SQLITE_ERROR;
        }
        if rc != SQLITE_OK {
            return Err(rc);
        }
        Ok(Rc::new(p))
    }
}

/// Por onde o token começa depois de pular os separadores.
enum TokenStart {
    /// Caractere ASCII de token em `text[csr]` (ainda não consumido).
    Ascii,
    /// Caractere não ASCII de token, já lido.
    NonAscii(u32),
}

impl Fts5Tokenizer for Unicode61Tokenizer {
    /// `fts5UnicodeTokenize`.
    fn tokenize(&self, _flags: i32, text: &[u8], x_token: Fts5TokenFn<'_>) -> i32 {
        let mut rc = SQLITE_OK;
        let a = &self.a_token_char;
        let z_term = text.len();
        let mut z_csr = 0usize;
        let mut fold: Vec<u8> = Vec::with_capacity(64); /* Buffer de saída */

        /* Cada iteração engole uma corrida de separadores e depois o token seguinte. */
        'tokenize: while rc == SQLITE_OK {
            fold.clear();

            /* Pula os caracteres separadores. */
            let (is, start) = loop {
                if z_csr >= z_term {
                    break 'tokenize;
                }
                if text[z_csr] & 0x80 != 0 {
                    /* Um caractere fora da faixa ASCII. Pula se é separador; senão sai. */
                    let is = z_csr;
                    let i_code = utf8_read(text, &mut z_csr);
                    if self.is_alnum(i_code) {
                        break (is, TokenStart::NonAscii(i_code));
                    }
                } else {
                    if a[text[z_csr] as usize] != 0 {
                        break (z_csr, TokenStart::Ascii);
                    }
                    z_csr += 1;
                }
            };

            /* Percorre os caracteres do token, dobrando-os no buffer de saída. */
            /* O primeiro caractere já foi escolhido ao pular os separadores. */
            match start {
                TokenStart::Ascii => {
                    fold.push(text[z_csr].to_ascii_lowercase());
                    z_csr += 1;
                }
                TokenStart::NonAscii(i_code) => self.fold_into(&mut fold, i_code),
            }
            let mut ie = z_csr;
            while z_csr < z_term {
                if text[z_csr] & 0x80 != 0 {
                    /* Um caractere fora da faixa ASCII: dobra se é de token, ou sai do laço. */
                    let i_code = utf8_read(text, &mut z_csr);
                    if self.is_alnum(i_code) || fts5_unicode_isdiacritic(i_code as i32) {
                        self.fold_into(&mut fold, i_code);
                    } else {
                        break;
                    }
                } else if a[text[z_csr] as usize] == 0 {
                    /* Um separador ASCII: fim do token. */
                    break;
                } else {
                    fold.push(text[z_csr].to_ascii_lowercase());
                    z_csr += 1;
                }
                ie = z_csr;
            }

            /* Chama o callback do token */
            rc = x_token(0, &fold, is as i32, ie as i32);
        }

        if rc == SQLITE_DONE {
            rc = SQLITE_OK;
        }
        rc
    }
}

// ---------------------------------------------------------------------------------------------
// porter
// ---------------------------------------------------------------------------------------------

/// Tokens maiores que isto (em bytes) passam sem derivação.
const FTS5_PORTER_MAX_TOKEN: usize = 64;

/// `PorterTokenizer`: o tokenizador base (`pTokenizer`) é o `parent`.
struct PorterTokenizer {
    parent: Rc<dyn Fts5Tokenizer>,
}

/// A fábrica do tokenizador `porter` (`fts5PorterCreate`).
struct PorterFactory;

impl Fts5TokenizerFactory for PorterFactory {
    fn create(&self, api: &dyn Fts5Api, args: &[Vec<u8>]) -> Result<Rc<dyn Fts5Tokenizer>, i32> {
        let z_base: &[u8] = args.first().map(|v| v.as_slice()).unwrap_or(b"unicode61");
        let factory = api.find_tokenizer(z_base).ok_or(SQLITE_ERROR)?;
        let parent = factory.create(api, args.get(1..).unwrap_or(&[]))?;
        Ok(Rc::new(PorterTokenizer { parent }))
    }
}

/// Condição de uma regra do Porter: recebe o radical.
type PorterCond = fn(&[u8]) -> bool;

/// Uma regra do Porter: se o token termina em `suffix` (e é mais longo que ele) e `cond` vale
/// para o radical, troca o sufixo por `repl`. `ret` é o que a regra devolve quando se aplica.
struct PorterRule {
    suffix: &'static [u8],
    cond: Option<PorterCond>,
    repl: &'static [u8],
    ret: bool,
}

const fn rule(suffix: &'static [u8], cond: PorterCond, repl: &'static [u8]) -> PorterRule {
    PorterRule { suffix, cond: Some(cond), repl, ret: false }
}

fn porter_is_vowel(c: u8, b_y_is_vowel: bool) -> bool {
    c == b'a' || c == b'e' || c == b'i' || c == b'o' || c == b'u' || (b_y_is_vowel && c == b'y')
}

/// `fts5PorterGobbleVC`: engole uma sequência vogais-consoantes de `z_stem` e devolve o
/// deslocamento depois dela, ou 0 se não há.
fn porter_gobble_vc(z_stem: &[u8], b_prev_cons: bool) -> usize {
    let n_stem = z_stem.len();
    let mut b_cons = b_prev_cons;
    let mut i = 0;

    /* Procura uma vogal */
    while i < n_stem {
        b_cons = !porter_is_vowel(z_stem[i], b_cons);
        if !b_cons {
            break;
        }
        i += 1;
    }

    /* Procura uma consoante */
    i += 1;
    while i < n_stem {
        b_cons = !porter_is_vowel(z_stem[i], b_cons);
        if b_cons {
            return i + 1;
        }
        i += 1;
    }
    0
}

/// Condição do Porter: (m > 0)
fn porter_m_gt0(z_stem: &[u8]) -> bool {
    porter_gobble_vc(z_stem, false) != 0
}

/// Condição do Porter: (m > 1)
fn porter_m_gt1(z_stem: &[u8]) -> bool {
    let n = porter_gobble_vc(z_stem, false);
    n != 0 && porter_gobble_vc(&z_stem[n..], true) != 0
}

/// Condição do Porter: (m = 1)
fn porter_m_eq1(z_stem: &[u8]) -> bool {
    let n = porter_gobble_vc(z_stem, false);
    n != 0 && 0 == porter_gobble_vc(&z_stem[n..], true)
}

/// Condição do Porter: (*o)
fn porter_ostar(z_stem: &[u8]) -> bool {
    if matches!(z_stem.last(), Some(b'w') | Some(b'x') | Some(b'y')) {
        false
    } else {
        let mut mask = 0;
        let mut b_cons = false;
        for &c in z_stem {
            b_cons = !porter_is_vowel(c, b_cons);
            mask = (mask << 1) + b_cons as i32;
        }
        (mask & 0x0007) == 0x0005
    }
}

/// Condição do Porter: (m > 1 and (*S or *T))
fn porter_m_gt1_and_s_or_t(z_stem: &[u8]) -> bool {
    matches!(z_stem.last(), Some(b's') | Some(b't')) && porter_m_gt1(z_stem)
}

/// Condição do Porter: (*v*)
fn porter_vowel(z_stem: &[u8]) -> bool {
    z_stem.iter().enumerate().any(|(i, &c)| porter_is_vowel(c, i > 0))
}

/* Regras geradas pelo mkportersteps.tcl, na ordem do C. */

static STEP_1B: [PorterRule; 3] = [
    rule(b"eed", porter_m_gt0, b"ee"),
    PorterRule { suffix: b"ed", cond: Some(porter_vowel), repl: b"", ret: true },
    PorterRule { suffix: b"ing", cond: Some(porter_vowel), repl: b"", ret: true },
];

static STEP_1B2: [PorterRule; 3] = [
    PorterRule { suffix: b"at", cond: None, repl: b"ate", ret: true },
    PorterRule { suffix: b"bl", cond: None, repl: b"ble", ret: true },
    PorterRule { suffix: b"iz", cond: None, repl: b"ize", ret: true },
];

static STEP_2: [PorterRule; 21] = [
    rule(b"ational", porter_m_gt0, b"ate"),
    rule(b"tional", porter_m_gt0, b"tion"),
    rule(b"enci", porter_m_gt0, b"ence"),
    rule(b"anci", porter_m_gt0, b"ance"),
    rule(b"izer", porter_m_gt0, b"ize"),
    rule(b"logi", porter_m_gt0, b"log"),
    rule(b"bli", porter_m_gt0, b"ble"),
    rule(b"alli", porter_m_gt0, b"al"),
    rule(b"entli", porter_m_gt0, b"ent"),
    rule(b"eli", porter_m_gt0, b"e"),
    rule(b"ousli", porter_m_gt0, b"ous"),
    rule(b"ization", porter_m_gt0, b"ize"),
    rule(b"ation", porter_m_gt0, b"ate"),
    rule(b"ator", porter_m_gt0, b"ate"),
    rule(b"alism", porter_m_gt0, b"al"),
    rule(b"iveness", porter_m_gt0, b"ive"),
    rule(b"fulness", porter_m_gt0, b"ful"),
    rule(b"ousness", porter_m_gt0, b"ous"),
    rule(b"aliti", porter_m_gt0, b"al"),
    rule(b"iviti", porter_m_gt0, b"ive"),
    rule(b"biliti", porter_m_gt0, b"ble"),
];

static STEP_3: [PorterRule; 7] = [
    rule(b"ical", porter_m_gt0, b"ic"),
    rule(b"ness", porter_m_gt0, b""),
    rule(b"icate", porter_m_gt0, b"ic"),
    rule(b"iciti", porter_m_gt0, b"ic"),
    rule(b"ful", porter_m_gt0, b""),
    rule(b"ative", porter_m_gt0, b""),
    rule(b"alize", porter_m_gt0, b"al"),
];

static STEP_4: [PorterRule; 19] = [
    rule(b"al", porter_m_gt1, b""),
    rule(b"ance", porter_m_gt1, b""),
    rule(b"ence", porter_m_gt1, b""),
    rule(b"er", porter_m_gt1, b""),
    rule(b"ic", porter_m_gt1, b""),
    rule(b"able", porter_m_gt1, b""),
    rule(b"ible", porter_m_gt1, b""),
    rule(b"ant", porter_m_gt1, b""),
    rule(b"ement", porter_m_gt1, b""),
    rule(b"ment", porter_m_gt1, b""),
    rule(b"ent", porter_m_gt1, b""),
    rule(b"ion", porter_m_gt1_and_s_or_t, b""),
    rule(b"ou", porter_m_gt1, b""),
    rule(b"ism", porter_m_gt1, b""),
    rule(b"ate", porter_m_gt1, b""),
    rule(b"iti", porter_m_gt1, b""),
    rule(b"ous", porter_m_gt1, b""),
    rule(b"ive", porter_m_gt1, b""),
    rule(b"ize", porter_m_gt1, b""),
];

/// Aplica o primeiro sufixo de `rules` que casa com o fim de `buf` (e deixa um radical não
/// vazio). Se a condição vale, troca o sufixo e devolve o `ret` da regra; senão não mexe em
/// nada (e não tenta as regras seguintes).
fn porter_apply(buf: &mut Vec<u8>, rules: &[PorterRule]) -> bool {
    let n_buf = buf.len();
    for r in rules {
        if n_buf > r.suffix.len() && buf.ends_with(r.suffix) {
            let n_stem = n_buf - r.suffix.len();
            if r.cond.map_or(true, |c| c(&buf[..n_stem])) {
                buf.truncate(n_stem);
                buf.extend_from_slice(r.repl);
                return r.ret;
            }
            return false;
        }
    }
    false
}

/// `fts5PorterStep1A`.
fn porter_step_1a(buf: &mut Vec<u8>) {
    let n_buf = buf.len();
    if buf[n_buf - 1] == b's' {
        if buf[n_buf - 2] == b'e' {
            if (n_buf > 4 && buf[n_buf - 4] == b's' && buf[n_buf - 3] == b's')
                || (n_buf > 3 && buf[n_buf - 3] == b'i')
            {
                buf.truncate(n_buf - 2);
            } else {
                buf.truncate(n_buf - 1);
            }
        } else if buf[n_buf - 2] != b's' {
            buf.truncate(n_buf - 1);
        }
    }
}

/// `fts5PorterCb`: o callback que deriva o token e o repassa ao `x_token` do chamador.
fn porter_cb(
    x_token: &mut dyn FnMut(i32, &[u8], i32, i32) -> i32,
    tflags: i32,
    token: &[u8],
    i_start: i32,
    i_end: i32,
) -> i32 {
    if token.len() > FTS5_PORTER_MAX_TOKEN || token.len() < 3 {
        return x_token(tflags, token, i_start, i_end);
    }
    let mut buf = token.to_vec();

    /* Passo 1. */
    porter_step_1a(&mut buf);
    if porter_apply(&mut buf, &STEP_1B) && !porter_apply(&mut buf, &STEP_1B2) {
        let n_buf = buf.len();
        let c = buf[n_buf - 1];
        if !porter_is_vowel(c, false)
            && c != b'l'
            && c != b's'
            && c != b'z'
            && n_buf >= 2
            && c == buf[n_buf - 2]
        {
            buf.pop();
        } else if porter_m_eq1(&buf) && porter_ostar(&buf) {
            buf.push(b'e');
        }
    }

    /* Passo 1C. */
    let n_buf = buf.len();
    if buf[n_buf - 1] == b'y' && porter_vowel(&buf[..n_buf - 1]) {
        buf[n_buf - 1] = b'i';
    }

    /* Passos 2 a 4. */
    porter_apply(&mut buf, &STEP_2);
    porter_apply(&mut buf, &STEP_3);
    porter_apply(&mut buf, &STEP_4);

    /* Passo 5a. */
    debug_assert!(!buf.is_empty());
    let n_buf = buf.len();
    if buf[n_buf - 1] == b'e' {
        let stem = &buf[..n_buf - 1];
        if porter_m_gt1(stem) || (porter_m_eq1(stem) && !porter_ostar(stem)) {
            buf.pop();
        }
    }

    /* Passo 5b. */
    let n_buf = buf.len();
    if n_buf > 1 && buf[n_buf - 1] == b'l' && buf[n_buf - 2] == b'l' && porter_m_gt1(&buf[..n_buf - 1]) {
        buf.pop();
    }

    x_token(tflags, &buf, i_start, i_end)
}

impl Fts5Tokenizer for PorterTokenizer {
    /// `fts5PorterTokenize`.
    fn tokenize(&self, flags: i32, text: &[u8], x_token: Fts5TokenFn<'_>) -> i32 {
        self.parent.tokenize(flags, text, &mut |tflags, tok, i_start, i_end| {
            porter_cb(&mut *x_token, tflags, tok, i_start, i_end)
        })
    }
}

// ---------------------------------------------------------------------------------------------
// trigram
// ---------------------------------------------------------------------------------------------

/// `TrigramTokenizer`.
struct TrigramTokenizer {
    /// Verdadeiro para dobrar para minúsculas.
    b_fold: bool,
    /// Parâmetro passado ao `fts5_unicode_fold`.
    i_fold_param: i32,
}

/// A fábrica do tokenizador `trigram` (`fts5TriCreate`).
struct TrigramFactory;

impl Fts5TokenizerFactory for TrigramFactory {
    fn create(&self, _api: &dyn Fts5Api, args: &[Vec<u8>]) -> Result<Rc<dyn Fts5Tokenizer>, i32> {
        let n_arg = args.len();
        let mut rc = SQLITE_OK;
        let mut p_new = TrigramTokenizer { b_fold: true, i_fold_param: 0 };
        let mut i = 0;
        while rc == SQLITE_OK && i + 1 < n_arg {
            let z_arg: &[u8] = &args[i + 1];
            if 0 == str_icmp(&args[i], b"case_sensitive") {
                if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1') || at(z_arg, 1) != 0 {
                    rc = SQLITE_ERROR;
                } else {
                    p_new.b_fold = at(z_arg, 0) == b'0';
                }
            } else if 0 == str_icmp(&args[i], b"remove_diacritics") {
                if (at(z_arg, 0) != b'0' && at(z_arg, 0) != b'1' && at(z_arg, 0) != b'2')
                    || at(z_arg, 1) != 0
                {
                    rc = SQLITE_ERROR;
                } else {
                    p_new.i_fold_param = if at(z_arg, 0) != b'0' { 2 } else { 0 };
                }
            } else {
                rc = SQLITE_ERROR;
            }
            i += 2;
        }
        if i < n_arg && rc == SQLITE_OK {
            rc = SQLITE_ERROR;
        }
        if p_new.i_fold_param != 0 && !p_new.b_fold {
            rc = SQLITE_ERROR;
        }
        if rc != SQLITE_OK {
            return Err(rc);
        }
        Ok(Rc::new(p_new))
    }
}

impl Fts5Tokenizer for TrigramTokenizer {
    /// `fts5TriTokenize`.
    fn tokenize(&self, _flags: i32, text: &[u8], x_token: Fts5TokenFn<'_>) -> i32 {
        let mut z_in = 0usize;
        let mut buf: Vec<u8> = Vec::with_capacity(16);
        let mut a_start = [0i32; 3]; /* Deslocamento de cada caractere de buf na entrada */

        /* Preenche buf com os caracteres do primeiro trigrama. */
        for start in a_start.iter_mut() {
            loop {
                *start = z_in as i32;
                let mut i_code = utf8_read(text, &mut z_in);
                if i_code == 0 {
                    return SQLITE_OK;
                }
                if self.b_fold {
                    i_code = fts5_unicode_fold(i_code as i32, self.i_fold_param) as u32;
                }
                if i_code != 0 {
                    write_utf8(&mut buf, i_code);
                    break;
                }
            }
        }

        /* No começo de cada iteração: buf tem 3 caracteres (os do próximo trigrama) e a_start os
        ** deslocamentos deles na entrada. */
        debug_assert!(z_in <= text.len() + 1);
        let mut rc;
        loop {
            /* Lê caracteres da entrada até o primeiro que não é diacrítico */
            let mut i_next;
            let mut i_code;
            loop {
                i_next = z_in as i32; /* Início do caractere depois do trigrama corrente */
                i_code = utf8_read(text, &mut z_in);
                if i_code == 0 {
                    break;
                }
                if self.b_fold {
                    i_code = fts5_unicode_fold(i_code as i32, self.i_fold_param) as u32;
                }
                if i_code != 0 {
                    break;
                }
            }

            /* Devolve o trigrama corrente ao fts5 */
            rc = x_token(0, &buf, a_start[0], i_next);
            if i_code == 0 || rc != SQLITE_OK {
                break;
            }

            /* Tira o primeiro caractere de buf e acrescenta o de ponto de código i_code. */
            let mut z1 = 1usize;
            if buf[0] >= 0xc0 {
                while z1 < buf.len() && (buf[z1] & 0xc0) == 0x80 {
                    z1 += 1;
                }
            }
            buf.drain(..z1);
            write_utf8(&mut buf, i_code);

            /* Atualiza a_start */
            a_start[0] = a_start[1];
            a_start[1] = a_start[2];
            a_start[2] = i_next;
        }
        rc
    }

    /// `sqlite3Fts5TokenizerPattern` para o trigram: `LIKE` sem diferenciar caixa, `GLOB`
    /// diferenciando; com remoção de diacríticos, nenhum.
    fn pattern(&self) -> i32 {
        if self.i_fold_param == 0 {
            if self.b_fold {
                FTS5_PATTERN_LIKE
            } else {
                FTS5_PATTERN_GLOB
            }
        } else {
            FTS5_PATTERN_NONE
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Registro
// ---------------------------------------------------------------------------------------------

/// `sqlite3Fts5TokenizerInit`: registra os tokenizadores embutidos no FTS5.
pub fn fts5_tokenizer_init(api: &mut dyn Fts5Api) -> i32 {
    let a_builtin: [(&[u8], Rc<dyn Fts5TokenizerFactory>); 4] = [
        (b"unicode61", Rc::new(Unicode61Factory)),
        (b"ascii", Rc::new(AsciiFactory)),
        (b"porter", Rc::new(PorterFactory)),
        (b"trigram", Rc::new(TrigramFactory)),
    ];
    let mut rc = SQLITE_OK;
    for (z_name, factory) in a_builtin {
        if rc != SQLITE_OK {
            break;
        }
        rc = api.create_tokenizer(z_name, factory);
    }
    rc
}
