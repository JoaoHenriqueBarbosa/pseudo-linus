//! Expressões regulares do gawk 5.2.1 (ERE do GNU no dialeto do awk), com casamento leftmost-longest.
//!
//! Esta é a fronteira entre o interpretador e o motor. O motor é o crate `regex-posix` (sintaxe do
//! `regcomp` do glibc, casada leftmost-longest, submatches com as escolhas do glibc); aqui fica só o
//! que o gawk faz por cima dele: o processamento de escapes do `make_regexp`, os avisos e o texto dos
//! erros.
//!
//! Contrato (não mude as assinaturas sem combinar com o integrador):
//!
//! - O texto de entrada é o corpo da regex como o awk o recebe: o miolo de `/.../` como está no fonte
//!   (com `\/` ainda escapado), ou o valor de uma string usada como regex dinâmica. O processamento das
//!   sequências de escape do awk (`\/`, `\n`, `\t`, `\"`, octal `\ddd`, `\y`, `\<`, `\>`, `\B`, `\w`,
//!   `\s`, `` \` ``, `\'`...) acontece aqui, como o `make_regexp` do gawk faz.
//! - Posições são em bytes no `hay`; o motor entende UTF-8 (o `.` casa um caractere).
//! - `start` é onde a busca começa, mas o contexto antes dele vale pra âncoras de palavra; `^` só casa na
//!   posição 0 e só se `not_bol` for falso; `$` só casa no fim do `hay` (o awk não é multilinha).
//! - Erros de compilação trazem o texto completo que o gawk escreve depois de `fatal: ` numa regex
//!   dinâmica: `invalid regexp: <mensagem do regerror>: /<regex>/`. Numa regex constante o gawk escreve
//!   `error: ` seguido do mesmo texto sem o `invalid regexp: ` ([`RegexError::static_message`]).
//! - Avisos de compilação (escape desconhecido etc.) voltam sem prefixo; o interpretador prefixa. O gawk
//!   avisa cada escape uma vez só por execução; aqui cada regex devolve os seus, sem repetição dentro da
//!   mesma regex, e quem deduplica entre regexes é o interpretador.
//!
//! # O que o gawk faz com os escapes (medido no gawk 5.2.1 em C.UTF-8)
//!
//! O gawk reescreve o texto da regex antes de entregá-lo ao `regcomp` e ao `dfa` dele, sem olhar se o
//! escape está dentro de colchetes:
//!
//! - `\a \b \f \n \r \t \v` viram o caractere de controle (`\b` é backspace, não fronteira de palavra);
//! - octal `\d`, `\dd`, `\ddd` (dígitos 0 a 7, no máximo três) vira o byte de valor módulo 256, e o
//!   byte entra cru: `\056` é o `.` operador, `\134` seguido de `<` vira o operador `\<`;
//! - `\x` com até dois dígitos hexadecimais vira o byte; sem dígito, aviso
//!   `` no hex digits in `\x' escape sequence `` e fica a letra `x`;
//! - `\8` e `\9` dão o aviso `` regexp escape sequence `\8' treated as plain `8' `` e ficam o dígito;
//! - `\y` vira o `\b` do GNU (fronteira de palavra), também dentro de colchetes, onde o `regcomp` o lê
//!   como a letra `b`;
//! - `\/`, `\"` e o escape de metacaractere ou de operador GNU (`$ ( ) * + - . < > ? B S W [ \ ] ^ `` `
//!   ' s w { | }`) passam intactos;
//! - qualquer outro `\c` passa intacto (o `regcomp` o lê como `c`) com o aviso
//!   `` regexp escape sequence `\c' is not a known regexp operator `` (o `\"` também avisa); com `c`
//!   fora do ASCII o aviso mostra só o primeiro byte;
//! - barra invertida no fim fica, e o `regcomp` dá `Trailing backslash`.
//!
//! Na mensagem de erro o gawk mostra o texto original da regex cortado no comprimento do texto já
//! reescrito (`/a\x5bb/` vira `a[b`, e o erro mostra `/a\x/`), e para no primeiro byte nulo.

use std::sync::Arc;

use regex_posix::{ExecFlags, RegexBuilder, Syntax};

/// Erro de compilação: o texto que o gawk escreve depois de `fatal: ` numa regex dinâmica,
/// `invalid regexp: <mensagem do regerror>: /<regex>/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexError {
    pub message: String,
}

impl RegexError {
    /// O texto que o gawk escreve depois de `error: ` quando a regex com erro é uma constante
    /// `/.../` do programa: o mesmo de [`RegexError::message`] sem o `invalid regexp: ` do começo.
    pub fn static_message(&self) -> &str {
        self.message.strip_prefix(INVALID_REGEXP).unwrap_or(&self.message)
    }
}

const INVALID_REGEXP: &str = "invalid regexp: ";

/// Uma regex compilada.
#[derive(Debug)]
pub struct Regex {
    pub(crate) re: regex_posix::Regex,
}

/// Grupos de uma casada: o índice 0 é a casada inteira; `None` é grupo que não participou.
pub type Captures = Vec<Option<(usize, usize)>>;

impl Regex {
    /// Compila no dialeto do gawk; `icase` é o `IGNORECASE`. Devolve a regex e os avisos.
    ///
    /// Avisos e mensagem de erro são texto; um byte fora do UTF-8 neles (escape seguido de caractere
    /// não ASCII, regex com byte inválido) vira U+FFFD. [`Regex::new_bytes`] devolve os bytes exatos.
    pub fn new(src: &[u8], icase: bool) -> Result<(Regex, Vec<String>), RegexError> {
        match Regex::new_bytes(src, icase) {
            Ok((re, warnings)) => Ok((re, warnings.iter().map(|w| String::from_utf8_lossy(w).into_owned()).collect())),
            Err(message) => Err(RegexError { message: String::from_utf8_lossy(&message).into_owned() }),
        }
    }

    /// Como [`Regex::new`], com avisos e mensagem de erro nos bytes exatos que o gawk escreve.
    pub fn new_bytes(src: &[u8], icase: bool) -> Result<(Regex, Vec<Vec<u8>>), Vec<u8>> {
        let (pattern, warnings) = preprocess(src);
        let built = RegexBuilder::new(Syntax::GNU_AWK)
            .icase(icase)
            .checkpoint(Arc::new(sysabi::sys::checkpoint))
            .build(&pattern);
        match built {
            Ok(re) => Ok((Regex { re }, warnings)),
            Err(e) => Err(error_message(e.message(), src, pattern.len())),
        }
    }

    /// Casada leftmost-longest que começa em `start` ou depois.
    pub fn find_at(&self, hay: &[u8], start: usize, not_bol: bool) -> Option<(usize, usize)> {
        self.re.find_at_with(hay, start, flags(not_bol)).map(|m| (m.start, m.end))
    }

    /// Como [`Regex::find_at`], com os grupos (semântica de subexpressão do glibc, que não é a do
    /// POSIX: ver o `regex-posix`).
    pub fn captures_at(&self, hay: &[u8], start: usize, not_bol: bool) -> Option<Captures> {
        let caps = self.re.captures_at_with(hay, start, flags(not_bol))?;
        Some(caps.iter().map(|m| m.map(|m| (m.start, m.end))).collect())
    }

}

fn flags(not_bol: bool) -> ExecFlags {
    ExecFlags { not_bol, not_eol: false }
}

/// `invalid regexp: <motivo>: /<regex>/`, com a regex original cortada no comprimento do texto
/// reescrito e no primeiro byte nulo, como o gawk imprime.
fn error_message(reason: &str, src: &[u8], processed_len: usize) -> Vec<u8> {
    let shown = &src[..processed_len.min(src.len())];
    let shown = match shown.iter().position(|&b| b == 0) {
        Some(nul) => &shown[..nul],
        None => shown,
    };
    let mut msg = Vec::with_capacity(INVALID_REGEXP.len() + reason.len() + shown.len() + 4);
    msg.extend_from_slice(INVALID_REGEXP.as_bytes());
    msg.extend_from_slice(reason.as_bytes());
    msg.extend_from_slice(b": /");
    msg.extend_from_slice(shown);
    msg.push(b'/');
    msg
}

/// Escapes que o gawk passa intactos ao `regcomp` sem aviso: metacaracteres e operadores GNU.
fn known_operator(c: u8) -> bool {
    matches!(
        c,
        b'$' | b'('
            | b')'
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'/'
            | b'<'
            | b'>'
            | b'?'
            | b'B'
            | b'S'
            | b'W'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'`'
            | b'\''
            | b's'
            | b'w'
            | b'{'
            | b'|'
            | b'}'
    )
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// O que o `make_regexp` do gawk faz antes do `regcomp`: devolve o texto reescrito e os avisos.
fn preprocess(src: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut out = Vec::with_capacity(src.len());
    let mut warnings: Vec<Vec<u8>> = Vec::new();
    let mut warn = |w: Vec<u8>| {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    };
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c != b'\\' || i + 1 == src.len() {
            // Barra no fim fica: o `regcomp` responde `Trailing backslash`.
            out.push(c);
            i += 1;
            continue;
        }
        let d = src[i + 1];
        i += 2;
        match d {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'0'..=b'7' => {
                let mut value = u32::from(d - b'0');
                let mut digits = 1;
                while digits < 3 && i < src.len() && (b'0'..=b'7').contains(&src[i]) {
                    value = value * 8 + u32::from(src[i] - b'0');
                    i += 1;
                    digits += 1;
                }
                out.push((value & 0xff) as u8);
            }
            b'8' | b'9' => {
                let mut w = b"regexp escape sequence `\\".to_vec();
                w.push(d);
                w.extend_from_slice(b"' treated as plain `");
                w.push(d);
                w.push(b'\'');
                warn(w);
                out.push(d);
            }
            b'x' => {
                let mut value = 0u8;
                let mut digits = 0;
                while digits < 2 && i < src.len() {
                    match hex_value(src[i]) {
                        Some(h) => {
                            value = value * 16 + h;
                            i += 1;
                            digits += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    warn(b"no hex digits in `\\x' escape sequence".to_vec());
                    out.push(b'x');
                } else {
                    out.push(value);
                }
            }
            // `\y` é a fronteira de palavra do gawk; o `regcomp` a chama de `\b`.
            b'y' => out.extend_from_slice(b"\\b"),
            _ => {
                if !known_operator(d) {
                    let mut w = b"regexp escape sequence `\\".to_vec();
                    w.push(d);
                    w.extend_from_slice(b"' is not a known regexp operator");
                    warn(w);
                }
                out.push(b'\\');
                out.push(d);
            }
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re(src: &str) -> Regex {
        Regex::new(src.as_bytes(), false).unwrap().0
    }

    fn re_ic(src: &str) -> Regex {
        Regex::new(src.as_bytes(), true).unwrap().0
    }

    fn find(src: &str, hay: &[u8]) -> Option<(usize, usize)> {
        re(src).find_at(hay, 0, false)
    }

    fn caps(src: &str, hay: &[u8]) -> Option<Captures> {
        re(src).captures_at(hay, 0, false)
    }

    fn err(src: &[u8]) -> String {
        Regex::new(src, false).unwrap_err().message
    }

    fn warns(src: &[u8]) -> Vec<String> {
        Regex::new(src, false).unwrap().1
    }

    #[test]
    fn escapes_of_control_characters() {
        assert_eq!(find(r"\b", b"a\x08b"), Some((1, 2)));
        assert_eq!(find(r"\a\f\v", b"x\x07\x0c\x0b"), Some((1, 4)));
        assert_eq!(find(r"a\nb", b"xa\nb"), Some((1, 4)));
        assert_eq!(find(r"\r\t", b"\r\t"), Some((0, 2)));
        assert_eq!(find(r"[\b]", b"ab\x08"), Some((2, 3)));
        assert_eq!(find(r"[\n]", b"ab\n"), Some((2, 3)));
    }

    #[test]
    fn octal_and_hex_escapes_enter_raw() {
        // `\056` é o `.` operador, não o ponto literal.
        assert_eq!(find(r"\056", b"abc"), Some((0, 1)));
        assert_eq!(find(r"a\052b", b"aab"), Some((0, 3)));
        assert_eq!(find(r"\1411", b"a1"), Some((0, 2)));
        assert_eq!(find(r"\777", b"x\xff"), Some((1, 2)));
        assert_eq!(find(r"\400", b"x\0"), Some((1, 2)));
        assert_eq!(find(r"\x414", b"A4"), Some((0, 2)));
        assert_eq!(find(r"a\x41", b"aA"), Some((0, 2)));
        assert_eq!(find(r"\x4", b"\x04"), Some((0, 1)));
        // `\134` é a barra invertida, que forma operador com o caractere seguinte.
        assert_eq!(find(r"\134<", b"x y"), Some((0, 0)));
        assert_eq!(find(r"a\134|b", b"xa|b"), Some((1, 4)));
        assert_eq!(find(r"\134w", b"!a"), Some((1, 2)));
        assert_eq!(find(r"\1", b"abc"), None);
    }

    #[test]
    fn hex_without_digits_and_plain_digits_warn() {
        assert_eq!(warns(br"\x"), vec!["no hex digits in `\\x' escape sequence"]);
        assert_eq!(find(r"\xg", b"axg"), Some((1, 3)));
        assert_eq!(warns(br"\8"), vec!["regexp escape sequence `\\8' treated as plain `8'"]);
        assert_eq!(find(r"\9", b"a9"), Some((1, 2)));
    }

    #[test]
    fn unknown_escape_warns_once_and_is_literal() {
        assert_eq!(warns(br"\q\q"), vec!["regexp escape sequence `\\q' is not a known regexp operator"]);
        assert_eq!(find(r"a\qb", b"aqb"), Some((0, 3)));
        assert_eq!(warns(br#"\""#), vec!["regexp escape sequence `\\\"' is not a known regexp operator"]);
        assert_eq!(warns(br"[\d]"), vec!["regexp escape sequence `\\d' is not a known regexp operator"]);
        assert_eq!(
            warns(br"\q\z"),
            vec![
                "regexp escape sequence `\\q' is not a known regexp operator",
                "regexp escape sequence `\\z' is not a known regexp operator"
            ]
        );
        // Escape seguido de caractere não ASCII: o aviso traz só o primeiro byte.
        let (_, w) = Regex::new_bytes("\\é".as_bytes(), false).unwrap();
        assert_eq!(w, vec![b"regexp escape sequence `\\\xc3' is not a known regexp operator".to_vec()]);
        for known in [r"\/", r"\.", r"\-", r"\}", r"\{", r"\|", r"\B", r"\<", r"\`", r"\'", r"\w", r"\S", r"\y"] {
            assert!(warns(known.as_bytes()).is_empty(), "{known}");
        }
    }

    #[test]
    fn gnu_operators() {
        assert_eq!(find(r"\y", b"ab"), Some((0, 0)));
        assert_eq!(find(r"a\yb", b"a b"), None);
        assert_eq!(find(r"\B", b"ab"), Some((1, 1)));
        assert_eq!(find(r"\>", b"ab c"), Some((2, 2)));
        assert_eq!(find(r"\<c", b"ab c"), Some((3, 4)));
        assert_eq!(find(r"\'", b"ab"), Some((2, 2)));
        assert_eq!(find(r"\`a", b"aa"), Some((0, 1)));
        assert_eq!(find(r"\w+", b"!ab_1 "), Some((1, 5)));
        assert_eq!(find(r"\W", b"ab\\"), Some((2, 3)));
        assert_eq!(find(r"\s", b"ab\tc"), Some((2, 3)));
        assert_eq!(find(r"\S", b" a"), Some((1, 2)));
        // Dentro de colchetes `\y` vira `\b` e o `regcomp` lê a letra `b`.
        assert_eq!(find(r"[\y]", b"ayb"), Some((2, 3)));
        assert_eq!(find(r"[\w]", b"aw"), Some((1, 2)));
        assert_eq!(find(r"a\/b", b"a/b"), Some((0, 3)));
    }

    #[test]
    fn brackets() {
        assert_eq!(find(r"[\]]", b"a]"), Some((1, 2)));
        assert_eq!(find(r"[\\]", b"a\\"), Some((1, 2)));
        assert_eq!(find(r"[\/]", b"a/"), Some((1, 2)));
        assert_eq!(find(r"[]a]", b"x]"), Some((1, 2)));
        assert_eq!(find(r"[^]a]", b"]ax"), Some((2, 3)));
        assert_eq!(find(r"[a-]", b"x-"), Some((1, 2)));
        assert_eq!(find(r"[--z]", b"!q"), Some((1, 2)));
        assert_eq!(find(r"[[:alpha:]]+", "1éa2".as_bytes()), Some((1, 4)));
        assert_eq!(find(r"[^a]", b"a\n"), Some((1, 2)));
        assert_eq!(find(r"[[.-.]]", b"a-"), Some((1, 2)));
        assert_eq!(find(r"[[=a=]]", b"ba"), Some((1, 2)));
    }

    #[test]
    fn intervals_and_context_dependent_operators() {
        assert_eq!(find(r"a{2}", b"aaa"), Some((0, 2)));
        assert_eq!(find(r"a{,2}", b"aaa"), Some((0, 2)));
        assert_eq!(find(r"a{,}", b"aaa"), Some((0, 3)));
        assert_eq!(find(r"a{", b"a{"), Some((0, 2)));
        assert_eq!(find(r"a{x}", b"a{x}"), Some((0, 4)));
        assert_eq!(find(r"a{1", b"a{1"), Some((0, 3)));
        assert_eq!(find(r"{1}", b"x{1}"), Some((1, 4)));
        assert_eq!(find(r"*a", b"x*a"), Some((1, 3)));
        assert_eq!(find(r"+a", b"x+a"), Some((1, 3)));
        assert_eq!(find(r"a|*b", b"x*b"), Some((1, 3)));
        assert_eq!(find(r"(+a)", b"+a"), Some((0, 2)));
        assert_eq!(find(r"^*", b"*a"), Some((0, 1)));
        assert_eq!(find(r"a**", b"aa"), Some((0, 2)));
        assert_eq!(find(r"a+?", b"xaa"), Some((0, 0)));
        assert_eq!(find(r")", b"a)"), Some((1, 2)));
        assert_eq!(find(r"a||b", b"xb"), Some((0, 0)));
        assert_eq!(find(r"()", b"x"), Some((0, 0)));
    }

    #[test]
    fn anchors_and_newlines() {
        assert_eq!(find(r"a^b", b"a^b"), None);
        assert_eq!(find(r"^b", b"a\nb"), None);
        assert_eq!(find(r"a$", b"a\nb"), None);
        assert_eq!(find(r"a.b", b"a\nb"), Some((0, 3)));
        assert_eq!(find(r"a.b", b"a\0b"), Some((0, 3)));
        assert_eq!(find(r"^\0$", b"\0"), Some((0, 1)));
        let r = re("^a");
        assert_eq!(r.find_at(b"aa", 0, true), None);
        assert_eq!(r.find_at(b"aa", 1, false), None);
        assert_eq!(re(r"\<a").find_at(b"aa a", 1, false), Some((3, 4)));
    }

    #[test]
    fn leftmost_longest() {
        assert_eq!(find(r"a|ab", b"xabc"), Some((1, 3)));
        assert_eq!(find(r"^#|^##|^###", b"### x"), Some((0, 3)));
        assert_eq!(find(r"(a|ab)(c|bcd)(d*)", b"abcd"), Some((0, 4)));
    }

    #[test]
    fn groups_like_glibc() {
        let s = |a: usize, b: usize| Some((a, b));
        assert_eq!(caps(r"(a|ab)(c|bcd)(d*)", b"abcd"), Some(vec![s(0, 4), s(0, 1), s(1, 4), s(4, 4)]));
        assert_eq!(caps(r"(a)(b)?(c)?", b"xaby"), Some(vec![s(1, 3), s(1, 2), s(2, 3), None]));
        assert_eq!(caps(r"(a*)*", b"aa"), Some(vec![s(0, 2), s(0, 2)]));
        assert_eq!(caps(r"(a*)*", b"b"), Some(vec![s(0, 0), s(0, 0)]));
        assert_eq!(caps(r"((a?))*", b"aa"), Some(vec![s(0, 2), s(1, 2), s(1, 2)]));
        assert_eq!(caps(r"(a)*a*", b"aa"), Some(vec![s(0, 2), s(1, 2)]));
        assert_eq!(caps(r"((a)|b)*", b"ab"), Some(vec![s(0, 2), s(1, 2), s(0, 1)]));
        assert_eq!(caps(r"(^)*", b"a"), Some(vec![s(0, 0), None]));
        assert_eq!(caps(r"(^)*a", b"a"), Some(vec![s(0, 1), s(0, 0)]));
        assert_eq!(caps(r"(()|a)*", b"aab"), Some(vec![s(0, 2), s(1, 2), s(0, 0)]));
        assert_eq!(re(r"(a)(b(c))").re.group_count(), 3);
    }

    #[test]
    fn ignorecase() {
        assert_eq!(re_ic("é").find_at("É".as_bytes(), 0, false), Some((0, 2)));
        assert_eq!(re_ic("s").find_at("ſ".as_bytes(), 0, false), Some((0, 2)));
        assert_eq!(re_ic("[a-z]+").find_at(b"xQz", 0, false), Some((0, 3)));
        assert_eq!(re_ic("[[:upper:]]").find_at("中".as_bytes(), 0, false), Some((0, 3)));
        assert_eq!(re_ic("[^a]").find_at(b"A", 0, false), None);
        assert_eq!(re_ic("ß").find_at("ẞ".as_bytes(), 0, false), None);
        // O glibc põe padrão e texto em maiúsculas: `[Z-a]` vira `[Z-A]`.
        assert_eq!(Regex::new(b"[Z-a]", true).unwrap_err().message, "invalid regexp: Invalid range end: /[Z-a]/");
        assert!(Regex::new(b"[a-Z]", true).is_ok());
    }

    #[test]
    fn invalid_utf8_in_text() {
        assert_eq!(find(r".", b"\xff"), None);
        assert_eq!(find(r"[^a]", b"\xff"), None);
        assert_eq!(find(r"\377", b"a\xffb"), Some((1, 2)));
        assert_eq!(find(r"a", b"\xffa"), Some((1, 2)));
        assert_eq!(find(r"\303", "é".as_bytes()), Some((0, 1)));
    }

    #[test]
    fn error_messages() {
        assert_eq!(err(b"["), "invalid regexp: Invalid regular expression: /[/");
        assert_eq!(err(b"[a"), "invalid regexp: Unmatched [, [^, [:, [., or [=: /[a/");
        assert_eq!(err(b"[^"), "invalid regexp: Invalid regular expression: /[^/");
        assert_eq!(err(b"[[:foo:]]"), "invalid regexp: Invalid character class name: /[[:foo:]]/");
        assert_eq!(err(b"a{2,1}"), "invalid regexp: Invalid content of \\{\\}: /a{2,1}/");
        assert_eq!(err(b"a{}"), "invalid regexp: Invalid content of \\{\\}: /a{}/");
        assert_eq!(err(b"a{1,2,3}"), "invalid regexp: Invalid content of \\{\\}: /a{1,2,3}/");
        assert_eq!(err(b"(a"), "invalid regexp: Unmatched ( or \\(: /(a/");
        assert_eq!(err(b"a\\"), "invalid regexp: Trailing backslash: /a\\/");
        assert_eq!(err(b"[b-a]"), "invalid regexp: Invalid range end: /[b-a]/");
        assert_eq!(err(b"[a-c-e]"), "invalid regexp: Invalid range end: /[a-c-e]/");
        assert_eq!(err(b"[[.ab.]]"), "invalid regexp: Invalid collation character: /[[.ab.]]/");
        assert_eq!(err("[[=á=]]".as_bytes()), "invalid regexp: Invalid collation character: /[[=á=]]/");
        assert_eq!(err(b"x{32768}"), "invalid regexp: Regular expression too big: /x{32768}/");
        assert_eq!(err(b"\\134\\061"), "invalid regexp: Invalid back reference: /\\1/");
        // A regex mostrada é a original cortada no comprimento da reescrita.
        assert_eq!(err(b"a\\x5bb"), "invalid regexp: Unmatched [, [^, [:, [., or [=: /a\\x/");
        assert_eq!(err(b"\\101("), "invalid regexp: Unmatched ( or \\(: /\\1/");
        assert_eq!(err(b"\\n("), "invalid regexp: Unmatched ( or \\(: /\\n/");
        assert_eq!(err(b"\\q("), "invalid regexp: Unmatched ( or \\(: /\\q(/");
        // ... e para no primeiro byte nulo.
        assert_eq!(err(b"a\0("), "invalid regexp: Unmatched ( or \\(: /a/");
        assert_eq!(err(b"a\\0("), "invalid regexp: Unmatched ( or \\(: /a\\0/");
        let e = Regex::new(b"(", false).unwrap_err();
        assert_eq!(e.static_message(), "Unmatched ( or \\(: /(/");
    }

    #[test]
    fn error_message_keeps_invalid_bytes() {
        assert_eq!(Regex::new_bytes(b"[\\351-z]", false).unwrap_err(), b"invalid regexp: Invalid collation character: /[\\351/".to_vec());
        assert_eq!(Regex::new_bytes(b"(\xff", false).unwrap_err(), b"invalid regexp: Unmatched ( or \\(: /(\xff/".to_vec());
    }
}
