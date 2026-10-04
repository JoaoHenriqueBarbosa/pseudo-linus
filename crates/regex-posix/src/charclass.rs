//! Conjuntos de caracteres em C.UTF-8: as classes POSIX do glibc, a montagem de uma expressão de
//! colchetes num conjunto de faixas Unicode, e a semântica de caixa do glibc.
//!
//! As classes seguem o LC_CTYPE do C.UTF-8 do glibc 2.41 (gerado do Unicode pelo
//! `gen_unicode_ctype.py`): na parte ASCII são exatas; fora dela usam as propriedades Unicode
//! equivalentes (`alpha` inclui os dígitos não ASCII, `space` exclui os espaços sem quebra etc.).
//!
//! Caixa: com `RE_ICASE` o glibc passa padrão e texto pra maiúsculas (`towupper`) e compara. Um
//! caractere `c` do texto casa o conjunto `S` do padrão quando `towupper(c)` está em `S`; isso é
//! [`upper_preimage`]. O `dfa.c` (seleção de linhas do grep) dobra a caixa dos dois lados; isso é
//! [`fold`].

use std::sync::OnceLock;

use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, HirKind};

use crate::ast::{PosixClass, Set, SetItem, Unit};
use crate::parse::to_upper;

/// Conjunto já montado: caracteres Unicode e bytes inválidos (que só casam se aparecem literais
/// no padrão).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharSet {
    pub chars: ClassUnicode,
    /// Bytes de 0x80 a 0xff que casam como byte inválido de UTF-8.
    pub bytes: [bool; 256],
}

impl CharSet {
    pub fn empty() -> CharSet {
        CharSet { chars: ClassUnicode::empty(), bytes: [false; 256] }
    }

    pub fn has_bytes(&self) -> bool {
        self.bytes.iter().any(|&b| b)
    }

    pub fn contains_char(&self, c: char) -> bool {
        class_contains(&self.chars, c)
    }

    pub fn contains_unit(&self, u: Unit) -> bool {
        match u {
            Unit::Char(c) => self.contains_char(c),
            Unit::Byte(b) => self.bytes[b as usize],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chars.ranges().is_empty() && !self.has_bytes()
    }
}

pub fn class_contains(class: &ClassUnicode, c: char) -> bool {
    let r = class.ranges();
    let (mut lo, mut hi) = (0usize, r.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if r[mid].end() < c {
            lo = mid + 1;
        } else if r[mid].start() > c {
            hi = mid;
        } else {
            return true;
        }
    }
    false
}

fn single(c: char) -> ClassUnicode {
    ClassUnicode::new([ClassUnicodeRange::new(c, c)])
}

fn ranges(rs: &[(char, char)]) -> ClassUnicode {
    ClassUnicode::new(rs.iter().map(|&(a, b)| ClassUnicodeRange::new(a, b)))
}

/// Classe Unicode de uma expressão aceita pelo `regex-syntax` (ex.: `\p{Alphabetic}`).
fn property(expr: &str) -> ClassUnicode {
    let hir = regex_syntax::ParserBuilder::new()
        .unicode(true)
        .utf8(true)
        .build()
        .parse(expr)
        .unwrap_or_else(|e| panic!("propriedade Unicode {expr}: {e}"));
    match hir.into_kind() {
        HirKind::Class(Class::Unicode(c)) => c,
        HirKind::Literal(lit) => {
            let s = String::from_utf8_lossy(&lit.0).into_owned();
            let c = s.chars().next().unwrap_or('\0');
            single(c)
        }
        other => panic!("propriedade Unicode {expr} não é classe: {other:?}"),
    }
}

fn union(mut a: ClassUnicode, b: &ClassUnicode) -> ClassUnicode {
    a.union(b);
    a
}

fn minus(mut a: ClassUnicode, b: &ClassUnicode) -> ClassUnicode {
    a.difference(b);
    a
}

struct Tables {
    alpha: ClassUnicode,
    digit: ClassUnicode,
    alnum: ClassUnicode,
    upper: ClassUnicode,
    lower: ClassUnicode,
    space: ClassUnicode,
    blank: ClassUnicode,
    punct: ClassUnicode,
    print: ClassUnicode,
    graph: ClassUnicode,
    cntrl: ClassUnicode,
    xdigit: ClassUnicode,
    /// Caracteres cujo `towupper` é outro caractere, com o resultado.
    to_upper: Vec<(char, char)>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let ascii_digit = ranges(&[('0', '9')]);
        let nbsp = ranges(&[('\u{a0}', '\u{a0}'), ('\u{2007}', '\u{2007}'), ('\u{202f}', '\u{202f}')]);
        let alpha = union(property(r"\p{Alphabetic}"), &minus(property(r"\p{Nd}"), &ascii_digit));
        let alpha = minus(alpha, &ascii_digit);
        let alnum = union(alpha.clone(), &ascii_digit);
        let zs = property(r"\p{Zs}");
        let space = minus(
            union(union(ranges(&[('\t', '\r'), (' ', ' ')]), &zs), &property(r"[\p{Zl}\p{Zp}]")),
            &nbsp,
        );
        let blank = minus(union(ranges(&[('\t', '\t'), (' ', ' ')]), &zs), &nbsp);
        let cntrl = union(property(r"\p{Cc}"), &ranges(&[('\u{2028}', '\u{2029}')]));
        // Surrogates (Cs) não existem como `char`, então ficam de fora sozinhos.
        let print = minus(property(r"\P{Cn}"), &cntrl);
        let graph = minus(print.clone(), &space);
        let punct = minus(graph.clone(), &alnum);
        let mut to_upper_list = Vec::new();
        for r in property(r"\p{Changes_When_Uppercased}").ranges() {
            for c in r.start()..=r.end() {
                let u = to_upper(c);
                if u != c {
                    to_upper_list.push((c, u));
                }
            }
        }
        Tables {
            alpha,
            digit: ascii_digit,
            alnum,
            upper: property(r"\p{Uppercase}"),
            lower: property(r"\p{Lowercase}"),
            space,
            blank,
            punct,
            print,
            graph,
            cntrl,
            xdigit: ranges(&[('0', '9'), ('A', 'F'), ('a', 'f')]),
            to_upper: to_upper_list,
        }
    })
}

/// Classe POSIX em C.UTF-8.
pub fn posix_class(k: PosixClass) -> &'static ClassUnicode {
    let t = tables();
    match k {
        PosixClass::Alpha => &t.alpha,
        PosixClass::Digit => &t.digit,
        PosixClass::Alnum => &t.alnum,
        PosixClass::Upper => &t.upper,
        PosixClass::Lower => &t.lower,
        PosixClass::Space => &t.space,
        PosixClass::Blank => &t.blank,
        PosixClass::Punct => &t.punct,
        PosixClass::Print => &t.print,
        PosixClass::Graph => &t.graph,
        PosixClass::Cntrl => &t.cntrl,
        PosixClass::Xdigit => &t.xdigit,
    }
}

/// Caractere de palavra para `\b`, `\<`, `\>` e `\B` (a mesma definição que o `regex-automata` usa
/// nas fronteiras Unicode, pra que os dois motores concordem).
pub fn is_word_char(c: char) -> bool {
    regex_syntax::try_is_word_character(c).unwrap_or(c == '_' || c.is_alphanumeric())
}

/// `{ c : towupper(c) ∈ S }`: o que um conjunto do padrão casa com `RE_ICASE` no glibc.
pub fn upper_preimage(s: &ClassUnicode) -> ClassUnicode {
    let mut add = ClassUnicode::empty();
    let mut del = ClassUnicode::empty();
    for &(c, u) in &tables().to_upper {
        if class_contains(s, u) {
            add.push(ClassUnicodeRange::new(c, c));
        } else {
            del.push(ClassUnicodeRange::new(c, c));
        }
    }
    let mut out = s.clone();
    out.union(&add);
    out.difference(&del);
    out
}

/// Dobra de caixa simétrica (a do `dfa.c`).
pub fn fold(s: &ClassUnicode) -> ClassUnicode {
    let mut out = s.clone();
    out.case_fold_simple();
    out
}

/// Como a caixa é tratada ao montar um conjunto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseMode {
    Sensitive,
    /// glibc: o texto vai pra maiúsculas antes do teste.
    Upper,
    /// dfa.c: dobra simétrica.
    Fold,
}

/// Monta o conjunto de um literal.
pub fn literal_set(u: Unit, case: CaseMode) -> CharSet {
    let mut cs = CharSet::empty();
    match u {
        Unit::Char(c) => {
            let base = single(c);
            cs.chars = match case {
                CaseMode::Sensitive => base,
                CaseMode::Upper => upper_preimage(&base),
                CaseMode::Fold => fold(&base),
            };
        }
        Unit::Byte(b) => cs.bytes[b as usize] = true,
    }
    cs
}

/// Monta o conjunto de uma expressão de colchetes.
pub fn bracket_set(set: &Set, case: CaseMode) -> CharSet {
    let mut chars = ClassUnicode::empty();
    let mut bytes = [false; 256];
    for item in &set.items {
        match *item {
            SetItem::Unit(Unit::Char(c)) => chars.push(ClassUnicodeRange::new(c, c)),
            SetItem::Unit(Unit::Byte(b)) => bytes[b as usize] = true,
            SetItem::Range(a, b) => chars.push(ClassUnicodeRange::new(a, b)),
            SetItem::ByteRange(a, b) => {
                if a <= b {
                    for x in a..=b {
                        if x < 0x80 {
                            chars.push(ClassUnicodeRange::new(x as char, x as char));
                        } else {
                            bytes[x as usize] = true;
                            // Sem colação, a faixa também cobre os pontos de código (como o glibc).
                            if let Some(c) = char::from_u32(x as u32) {
                                chars.push(ClassUnicodeRange::new(c, c));
                            }
                        }
                    }
                }
            }
            SetItem::Class(k) => chars.union(posix_class(k)),
        }
    }
    let chars = match (set.negated, case) {
        (false, CaseMode::Sensitive) => chars,
        (false, CaseMode::Upper) => upper_preimage(&chars),
        (false, CaseMode::Fold) => fold(&chars),
        (true, CaseMode::Sensitive) => {
            let mut c = chars;
            c.negate();
            c
        }
        (true, CaseMode::Upper) => {
            let mut c = chars;
            c.negate();
            upper_preimage(&c)
        }
        (true, CaseMode::Fold) => {
            let mut c = fold(&chars);
            c.negate();
            c
        }
    };
    // Colchete negado não casa byte inválido; o positivo casa os bytes listados.
    let bytes = if set.negated { [false; 256] } else { bytes };
    CharSet { chars, bytes }
}

/// Conjunto do `.`.
pub fn any_set(dot_newline: bool, dot_not_null: bool) -> CharSet {
    let mut chars = ClassUnicode::new([ClassUnicodeRange::new('\0', char::MAX)]);
    if !dot_newline {
        chars.difference(&single('\n'));
    }
    if dot_not_null {
        chars.difference(&single('\0'));
    }
    CharSet { chars, bytes: [false; 256] }
}

/// Tira o separador de linha (grep) de um conjunto.
pub fn without_separator(mut cs: CharSet, sep: u8) -> CharSet {
    if sep < 0x80 {
        cs.chars.difference(&single(sep as char));
    } else {
        cs.bytes[sep as usize] = false;
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_classes_are_exact() {
        let has = |k: PosixClass, c: char| class_contains(posix_class(k), c);
        assert!(has(PosixClass::Alpha, 'a') && !has(PosixClass::Alpha, '1'));
        assert!(has(PosixClass::Alpha, 'é') && has(PosixClass::Alpha, '٣'));
        assert!(has(PosixClass::Space, '\x0b') && !has(PosixClass::Space, '\u{a0}'));
        assert!(has(PosixClass::Punct, '_') && has(PosixClass::Punct, '~') && !has(PosixClass::Punct, 'a'));
        assert!(has(PosixClass::Print, ' ') && !has(PosixClass::Print, '\x7f'));
        assert!(!has(PosixClass::Graph, ' ') && has(PosixClass::Graph, '!'));
        assert!(has(PosixClass::Blank, '\t') && !has(PosixClass::Blank, '\n'));
        assert!(has(PosixClass::Cntrl, '\0') && has(PosixClass::Cntrl, '\x7f'));
    }

    #[test]
    fn icase_models() {
        // glibc: o literal 'A' (padrão já em maiúsculas) casa 'a' e 'A'; um 'a' minúsculo (escapado,
        // que o glibc não traduz) não casa nada alfabético.
        let up = literal_set(Unit::Char('A'), CaseMode::Upper);
        assert!(up.contains_char('a') && up.contains_char('A'));
        let low = literal_set(Unit::Char('a'), CaseMode::Upper);
        assert!(!low.contains_char('a') && !low.contains_char('A'));
        let f = literal_set(Unit::Char('a'), CaseMode::Fold);
        assert!(f.contains_char('a') && f.contains_char('A'));
        let neg = bracket_set(&Set { negated: true, items: vec![SetItem::Unit(Unit::Char('A'))] }, CaseMode::Upper);
        assert!(!neg.contains_char('a') && !neg.contains_char('A') && neg.contains_char('b'));
    }
}
