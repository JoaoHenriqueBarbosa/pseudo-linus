//! `fts3_porter.c`: o tokenizador `porter` do FTS3, que reduz as palavras ao radical pelo algoritmo
//! de Porter. Palavras com caracteres fora de `[a-zA-Z]`, com menos de 3 ou com 21 ou mais bytes não
//! passam pelo algoritmo: vão pelo `copy_stemmer`.
//!
//! O C inverte a palavra num buffer de 28 bytes (`zReverse`) e trabalha com ponteiros nele; aqui
//! `z` é o deslocamento no mesmo buffer. Os 5 bytes finais valem zero e fazem o papel do
//! terminador, então a leitura além do fim (`z[1]` com `z[0]==0`) cai em zero, como no C.

use std::rc::Rc;

use crate::consts::SQLITE_DONE;
use crate::util::at;

use super::int::{
    scan_delimited_token, Fts3Token, Fts3Tokenizer, Fts3TokenizerCursor, Fts3TokenizerModule,
};

/// `porter_tokenizer`.
struct PorterTokenizer;

/// `porter_tokenizer_cursor`.
struct PorterTokenizerCursor {
    /// `zInput`/`nInput`.
    input: Vec<u8>,
    /// `iOffset`.
    i_offset: usize,
    /// `iToken`.
    i_token: i32,
    /// `zToken`: o armazenamento do token corrente.
    token: Vec<u8>,
}

/// O módulo `porter` (`porterTokenizerModule`).
struct PorterModule;

/// `sqlite3Fts3PorterTokenizerModule`: o módulo do tokenizador `porter`.
pub fn fts3_porter_tokenizer_module() -> Rc<dyn Fts3TokenizerModule> {
    Rc::new(PorterModule)
}

impl Fts3TokenizerModule for PorterModule {
    /// `porterCreate`: os argumentos são ignorados.
    fn create(&self, _args: &[Vec<u8>]) -> Result<Rc<dyn Fts3Tokenizer>, i32> {
        Ok(Rc::new(PorterTokenizer))
    }
}

impl Fts3Tokenizer for PorterTokenizer {
    /// `porterOpen`.
    fn open(&self, input: &[u8]) -> Result<Box<dyn Fts3TokenizerCursor>, i32> {
        Ok(Box::new(PorterTokenizerCursor {
            input: input.to_vec(),
            i_offset: 0,
            i_token: 0,
            token: Vec::new(),
        }))
    }
}

/// O buffer da palavra invertida: `char zReverse[28]`.
type Reverse = [u8; 28];

/// `cType`: vogal (0), consoante (1) ou o `y` (2), para as letras de `a` a `z`.
static C_TYPE: [u8; 26] = [0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 2, 1];

/// `isConsonant`: o primeiro caractere de `z[]` é uma consoante pelas regras de Porter? Consoante
/// é qualquer letra que não seja `a`, `e`, `i`, `o` ou `u`; o `y` é consoante a menos que siga
/// outra consoante, quando é vogal. Neste buffer a palavra está invertida, então a regra do `y` é
/// "consoante a menos que seja seguido por outra consoante".
fn is_consonant(b: &Reverse, z: usize) -> bool {
    let x = at(b, z);
    if x == 0 {
        return false;
    }
    debug_assert!(x.is_ascii_lowercase());
    let j = C_TYPE[(x - b'a') as usize];
    if j < 2 {
        return j != 0;
    }
    at(b, z + 1) == 0 || is_vowel(b, z + 1)
}

/// `isVowel`: o inverso de [`is_consonant`] (e falso no fim da palavra).
fn is_vowel(b: &Reverse, z: usize) -> bool {
    let x = at(b, z);
    if x == 0 {
        return false;
    }
    debug_assert!(x.is_ascii_lowercase());
    let j = C_TYPE[(x - b'a') as usize];
    if j < 2 {
        return j == 0;
    }
    is_consonant(b, z + 1)
}

/// `m_gt_0`: o valor `m` da palavra (o número de pares vogal-consoante) é 1 ou mais? Neste buffer
/// procuramos uma consoante seguida de uma vogal.
fn m_gt_0(b: &Reverse, mut z: usize) -> bool {
    while is_vowel(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_consonant(b, z) {
        z += 1;
    }
    at(b, z) != 0
}

/// `m_eq_1`: `m` vale exatamente 1?
fn m_eq_1(b: &Reverse, mut z: usize) -> bool {
    while is_vowel(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_consonant(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_vowel(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return true;
    }
    while is_consonant(b, z) {
        z += 1;
    }
    at(b, z) == 0
}

/// `m_gt_1`: `m` é maior que 1?
fn m_gt_1(b: &Reverse, mut z: usize) -> bool {
    while is_vowel(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_consonant(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_vowel(b, z) {
        z += 1;
    }
    if at(b, z) == 0 {
        return false;
    }
    while is_consonant(b, z) {
        z += 1;
    }
    at(b, z) != 0
}

/// `hasVowel`: há uma vogal em algum lugar de `z[]`?
fn has_vowel(b: &Reverse, mut z: usize) -> bool {
    while is_consonant(b, z) {
        z += 1;
    }
    at(b, z) != 0
}

/// `doubleConsonant`: a palavra termina em consoante dobrada? (Invertida: os dois primeiros
/// caracteres de `z[]`.)
fn double_consonant(b: &Reverse, z: usize) -> bool {
    is_consonant(b, z) && at(b, z) == at(b, z + 1)
}

/// `star_oh`: a palavra termina em consoante-vogal-consoante e a última consoante não é `w`, `x`
/// nem `y`? (Invertida: as três primeiras letras, e a primeira não está em `[wxy]`.)
fn star_oh(b: &Reverse, z: usize) -> bool {
    is_consonant(b, z)
        && at(b, z) != b'w'
        && at(b, z) != b'x'
        && at(b, z) != b'y'
        && is_vowel(b, z + 1)
        && is_consonant(b, z + 2)
}

/// A condição de um passo de `stem`.
type Cond = fn(&Reverse, usize) -> bool;

/// `stem`: se a palavra termina em `z_from` (invertido) e `x_cond` vale para o radical que
/// precede esse final, troca o final por `z_to` (em ordem normal). Devolve verdadeiro se
/// `z_from` casou, mesmo que `x_cond` falhe e nada seja trocado.
fn stem(b: &mut Reverse, pz: &mut usize, z_from: &[u8], z_to: &[u8], x_cond: Option<Cond>) -> bool {
    let mut z = *pz;
    let mut i = 0usize;
    while i < z_from.len() && z_from[i] == at(b, z) {
        z += 1;
        i += 1;
    }
    if i != z_from.len() {
        return false;
    }
    if let Some(cond) = x_cond {
        if !cond(b, z) {
            return true;
        }
    }
    for &c in z_to {
        z -= 1;
        b[z] = c;
    }
    *pz = z;
    true
}

/// `copy_stemmer`: o radical de reserva. A palavra é copiada com a dobra de caixa ASCII. Se for
/// longa demais (mais de 20 bytes sem dígitos, ou de 6 com dígitos), fica só com 10 (ou 3) bytes
/// do começo e do fim.
fn copy_stemmer(z_in: &[u8]) -> Vec<u8> {
    let n_in = z_in.len();
    let mut has_digit = false;
    let mut z_out: Vec<u8> = Vec::with_capacity(n_in);
    for &c in z_in {
        if c.is_ascii_uppercase() {
            z_out.push(c - b'A' + b'a');
        } else {
            if c.is_ascii_digit() {
                has_digit = true;
            }
            z_out.push(c);
        }
    }
    let mx = if has_digit { 3 } else { 10 };
    if n_in > mx * 2 {
        let tail: Vec<u8> = z_out[n_in - mx..].to_vec();
        z_out.truncate(mx);
        z_out.extend_from_slice(&tail);
    }
    z_out
}

/// `porter_stemmer`: o radical da palavra `z_in`. As letras ASCII maiúsculas viram minúsculas e
/// as demais ficam como estão. O radical nunca é maior que a palavra.
fn porter_stemmer(z_in: &[u8]) -> Vec<u8> {
    let n_in = z_in.len();
    let mut z_reverse: Reverse = [0; 28];
    if n_in < 3 || n_in >= z_reverse.len() - 7 {
        /* A palavra é grande ou pequena demais para o Porter: cai no copy_stemmer. */
        return copy_stemmer(z_in);
    }
    let mut j = z_reverse.len() - 6;
    for &c in z_in {
        if c.is_ascii_uppercase() {
            z_reverse[j] = c + b'a' - b'A';
        } else if c.is_ascii_lowercase() {
            z_reverse[j] = c;
        } else {
            /* Um caractere fora de [a-zA-Z] manda para o copy_stemmer. */
            return copy_stemmer(z_in);
        }
        j -= 1;
    }
    let b = &mut z_reverse;
    let mut z = j + 1;

    /* Passo 1a */
    if b[z] == b's'
        && !stem(b, &mut z, b"sess", b"ss", None)
        && !stem(b, &mut z, b"sei", b"i", None)
        && !stem(b, &mut z, b"ss", b"ss", None)
    {
        z += 1;
    }

    /* Passo 1b */
    let z2 = z;
    if stem(b, &mut z, b"dee", b"ee", Some(m_gt_0)) {
        /* Nada a fazer: o trabalho todo foi o teste. */
    } else if (stem(b, &mut z, b"gni", b"", Some(has_vowel)) || stem(b, &mut z, b"de", b"", Some(has_vowel)))
        && z != z2
    {
        if stem(b, &mut z, b"ta", b"ate", None)
            || stem(b, &mut z, b"lb", b"ble", None)
            || stem(b, &mut z, b"zi", b"ize", None)
        {
            /* Nada a fazer: o trabalho todo foi o teste. */
        } else if double_consonant(b, z) && (b[z] != b'l' && b[z] != b's' && b[z] != b'z') {
            z += 1;
        } else if m_eq_1(b, z) && star_oh(b, z) {
            z -= 1;
            b[z] = b'e';
        }
    }

    /* Passo 1c */
    if b[z] == b'y' && has_vowel(b, z + 1) {
        b[z] = b'i';
    }

    /* Passo 2 */
    match at(b, z + 1) {
        b'a' => {
            if !stem(b, &mut z, b"lanoita", b"ate", Some(m_gt_0)) {
                stem(b, &mut z, b"lanoit", b"tion", Some(m_gt_0));
            }
        }
        b'c' => {
            if !stem(b, &mut z, b"icne", b"ence", Some(m_gt_0)) {
                stem(b, &mut z, b"icna", b"ance", Some(m_gt_0));
            }
        }
        b'e' => {
            stem(b, &mut z, b"rezi", b"ize", Some(m_gt_0));
        }
        b'g' => {
            stem(b, &mut z, b"igol", b"log", Some(m_gt_0));
        }
        b'l' => {
            if !stem(b, &mut z, b"ilb", b"ble", Some(m_gt_0))
                && !stem(b, &mut z, b"illa", b"al", Some(m_gt_0))
                && !stem(b, &mut z, b"iltne", b"ent", Some(m_gt_0))
                && !stem(b, &mut z, b"ile", b"e", Some(m_gt_0))
            {
                stem(b, &mut z, b"ilsuo", b"ous", Some(m_gt_0));
            }
        }
        b'o' => {
            if !stem(b, &mut z, b"noitazi", b"ize", Some(m_gt_0))
                && !stem(b, &mut z, b"noita", b"ate", Some(m_gt_0))
            {
                stem(b, &mut z, b"rota", b"ate", Some(m_gt_0));
            }
        }
        b's' => {
            if !stem(b, &mut z, b"msila", b"al", Some(m_gt_0))
                && !stem(b, &mut z, b"ssenevi", b"ive", Some(m_gt_0))
                && !stem(b, &mut z, b"ssenluf", b"ful", Some(m_gt_0))
            {
                stem(b, &mut z, b"ssensuo", b"ous", Some(m_gt_0));
            }
        }
        b't' => {
            if !stem(b, &mut z, b"itila", b"al", Some(m_gt_0))
                && !stem(b, &mut z, b"itivi", b"ive", Some(m_gt_0))
            {
                stem(b, &mut z, b"itilib", b"ble", Some(m_gt_0));
            }
        }
        _ => {}
    }

    /* Passo 3 */
    match at(b, z) {
        b'e' => {
            if !stem(b, &mut z, b"etaci", b"ic", Some(m_gt_0)) && !stem(b, &mut z, b"evita", b"", Some(m_gt_0)) {
                stem(b, &mut z, b"ezila", b"al", Some(m_gt_0));
            }
        }
        b'i' => {
            stem(b, &mut z, b"itici", b"ic", Some(m_gt_0));
        }
        b'l' => {
            if !stem(b, &mut z, b"laci", b"ic", Some(m_gt_0)) {
                stem(b, &mut z, b"luf", b"", Some(m_gt_0));
            }
        }
        b's' => {
            stem(b, &mut z, b"ssen", b"", Some(m_gt_0));
        }
        _ => {}
    }

    /* Passo 4 */
    match at(b, z + 1) {
        b'a' => {
            if at(b, z) == b'l' && m_gt_1(b, z + 2) {
                z += 2;
            }
        }
        b'c' => {
            if at(b, z) == b'e'
                && at(b, z + 2) == b'n'
                && (at(b, z + 3) == b'a' || at(b, z + 3) == b'e')
                && m_gt_1(b, z + 4)
            {
                z += 4;
            }
        }
        b'e' => {
            if at(b, z) == b'r' && m_gt_1(b, z + 2) {
                z += 2;
            }
        }
        b'i' => {
            if at(b, z) == b'c' && m_gt_1(b, z + 2) {
                z += 2;
            }
        }
        b'l' => {
            if at(b, z) == b'e'
                && at(b, z + 2) == b'b'
                && (at(b, z + 3) == b'a' || at(b, z + 3) == b'i')
                && m_gt_1(b, z + 4)
            {
                z += 4;
            }
        }
        b'n' => {
            if at(b, z) == b't' {
                if at(b, z + 2) == b'a' {
                    if m_gt_1(b, z + 3) {
                        z += 3;
                    }
                } else if at(b, z + 2) == b'e'
                    && !stem(b, &mut z, b"tneme", b"", Some(m_gt_1))
                    && !stem(b, &mut z, b"tnem", b"", Some(m_gt_1))
                {
                    stem(b, &mut z, b"tne", b"", Some(m_gt_1));
                }
            }
        }
        b'o' => {
            if at(b, z) == b'u' {
                if m_gt_1(b, z + 2) {
                    z += 2;
                }
            } else if at(b, z + 3) == b's' || at(b, z + 3) == b't' {
                stem(b, &mut z, b"noi", b"", Some(m_gt_1));
            }
        }
        b's' => {
            if at(b, z) == b'm' && at(b, z + 2) == b'i' && m_gt_1(b, z + 3) {
                z += 3;
            }
        }
        b't' => {
            if !stem(b, &mut z, b"eta", b"", Some(m_gt_1)) {
                stem(b, &mut z, b"iti", b"", Some(m_gt_1));
            }
        }
        b'u' => {
            if at(b, z) == b's' && at(b, z + 2) == b'o' && m_gt_1(b, z + 3) {
                z += 3;
            }
        }
        b'v' | b'z' => {
            if at(b, z) == b'e' && at(b, z + 2) == b'i' && m_gt_1(b, z + 3) {
                z += 3;
            }
        }
        _ => {}
    }

    /* Passo 5a */
    if at(b, z) == b'e' {
        if m_gt_1(b, z + 1) {
            z += 1;
        } else if m_eq_1(b, z + 1) && !star_oh(b, z + 1) {
            z += 1;
        }
    }

    /* Passo 5b */
    if m_gt_1(b, z) && at(b, z) == b'l' && at(b, z + 1) == b'l' {
        z += 1;
    }

    /* `z[]` agora é o radical em ordem inversa: vira a ordem normal e volta. */
    let mut z_out: Vec<u8> = b[z..].iter().copied().take_while(|&c| c != 0).collect();
    z_out.reverse();
    z_out
}

/// `porterIdChar`: os caracteres que podem fazer parte de um token (a partir de 0x30). Qualquer
/// caractere de valor 0x80 ou maior (UTF-8) também pode: os delimitadores valem 0x7f ou menos.
static PORTER_ID_CHAR: [u8; 80] = [
    /* x0 x1 x2 x3 x4 x5 x6 x7 x8 x9 xA xB xC xD xE xF */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, /* 3x */
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, /* 4x */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, /* 5x */
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, /* 6x */
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, /* 7x */
];

/// `isDelim`.
#[inline]
fn is_delim(ch: u8) -> bool {
    (ch & 0x80) == 0 && (ch < 0x30 || PORTER_ID_CHAR[(ch - 0x30) as usize] == 0)
}

impl Fts3TokenizerCursor for PorterTokenizerCursor {
    /// `porterNext`.
    fn next(&mut self) -> Result<Fts3Token<'_>, i32> {
        let Some((i_start_offset, i_end_offset)) =
            scan_delimited_token(&self.input, &mut self.i_offset, is_delim)
        else {
            return Err(SQLITE_DONE);
        };
        self.token = porter_stemmer(&self.input[i_start_offset..i_end_offset]);
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

    fn stem_str(s: &str) -> String {
        String::from_utf8(porter_stemmer(s.as_bytes())).unwrap()
    }

    #[test]
    fn porter_classic_words() {
        assert_eq!(stem_str("caresses"), "caress");
        assert_eq!(stem_str("ponies"), "poni");
        assert_eq!(stem_str("running"), "run");
        assert_eq!(stem_str("RUNNING"), "run");
    }

    #[test]
    fn copy_stemmer_fallbacks() {
        assert_eq!(stem_str("Ab"), "ab");
        assert_eq!(stem_str("a1b2c3d4"), "a1b3d4");
        assert_eq!(stem_str("abcdefghijklmnopqrstuvwxyz"), "abcdefghijqrstuvwxyz");
    }
}
