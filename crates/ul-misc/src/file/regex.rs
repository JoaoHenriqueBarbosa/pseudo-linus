//! As regex das regras de magic: ERE do POSIX com a semântica do `regcomp`/`regexec` da glibc
//! que o libmagic usa (`REG_EXTENDED`, `REG_NEWLINE` no casamento, `REG_ICASE` com `/c`), no
//! locale C (o `file_regcomp` troca pro "C" antes de compilar: cada byte é um caractere).
//!
//! O padrão é traduzido pra sintaxe do `regex-syntax` em modo de bytes e a casada é
//! leftmost-longest como no POSIX: um `meta::Regex` acha o começo mais à esquerda e um DFA
//! preguiçoso com `MatchKind::All`, ancorado nesse começo, acha o fim mais longo.

use std::fmt::Write as _;
use std::sync::Mutex;

use regex_automata::hybrid::dfa::{Cache, DFA};
use regex_automata::util::syntax;
use regex_automata::{Anchored, Input, MatchKind, meta};

/// Erro de compilação com o código e o texto do `regerror` da glibc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexError {
    pub code: i32,
    pub message: &'static str,
}

const REG_BADPAT: RegexError = RegexError {
    code: 2,
    message: "Invalid regular expression",
};
const REG_ECOLLATE: RegexError = RegexError {
    code: 3,
    message: "Invalid collation character",
};
const REG_ECTYPE: RegexError = RegexError {
    code: 4,
    message: "Invalid character class name",
};
const REG_ESUBREG: RegexError = RegexError {
    code: 6,
    message: "Invalid back reference",
};
const REG_EBRACK: RegexError = RegexError {
    code: 7,
    message: "Unmatched [, [^, [:, [., or [=",
};
const REG_EPAREN: RegexError = RegexError {
    code: 8,
    message: "Unmatched ( or \\(",
};
const REG_EBRACE: RegexError = RegexError {
    code: 9,
    message: "Unmatched \\{",
};
const REG_BADBR: RegexError = RegexError {
    code: 10,
    message: "Invalid content of \\{\\}",
};
const REG_ERANGE: RegexError = RegexError {
    code: 11,
    message: "Invalid range end",
};
const REG_ESPACE: RegexError = RegexError {
    code: 12,
    message: "Memory exhausted",
};
const REG_BADRPT: RegexError = RegexError {
    code: 13,
    message: "Invalid preceding regular expression",
};

const RE_DUP_MAX: u32 = 0x7fff;

/// Uma regex compilada.
pub struct Regex {
    start: meta::Regex,
    longest: DFA,
    caches: Mutex<Vec<Cache>>,
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Regex")
    }
}

impl Regex {
    /// `regcomp(pat, REG_EXTENDED | (newline ? REG_NEWLINE : 0) | (icase ? REG_ICASE : 0))`.
    pub fn compile(pat: &[u8], icase: bool, newline: bool) -> Result<Regex, RegexError> {
        let translated = translate(pat, newline)?;
        let syn = syntax::Config::new()
            .unicode(false)
            .utf8(false)
            .case_insensitive(icase)
            .multi_line(newline)
            .dot_matches_new_line(!newline);
        let start = meta::Regex::builder()
            .syntax(syn)
            .configure(
                meta::Config::new()
                    .utf8_empty(false)
                    .match_kind(MatchKind::LeftmostFirst),
            )
            .build(&translated)
            .map_err(|_| REG_ESPACE)?;
        let longest = DFA::builder()
            .syntax(syn)
            .thompson(regex_automata::nfa::thompson::Config::new().utf8(false))
            .configure(
                DFA::config()
                    .match_kind(MatchKind::All)
                    .cache_capacity(4 << 20)
                    .skip_cache_capacity_check(true),
            )
            .build(&translated)
            .map_err(|_| REG_ESPACE)?;
        Ok(Regex {
            start,
            longest,
            caches: Mutex::new(Vec::new()),
        })
    }

    /// `regexec` com um `regmatch_t`: a casada leftmost-longest em `hay` (sem o NUL: quem
    /// chama corta a cadeia no primeiro NUL, como o C).
    pub fn find(&self, hay: &[u8]) -> Option<(usize, usize)> {
        let m = self.start.find(Input::new(hay))?;
        let s = m.start();
        let mut cache = {
            let mut pool = self.caches.lock().unwrap_or_else(|e| e.into_inner());
            pool.pop().unwrap_or_else(|| self.longest.create_cache())
        };
        let r = self.longest.try_search_fwd(
            &mut cache,
            &Input::new(hay).range(s..).anchored(Anchored::Yes),
        );
        self.caches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(cache);
        let end = match r {
            Ok(Some(h)) => h.offset().max(m.end()),
            // O DFA preguiçoso desistiu: fica com o fim da casada leftmost-first.
            _ => m.end(),
        };
        Some((s, end))
    }
}

/// `check_regex()` do funcs.c: o libmagic recusa algumas regex antes do `regcomp`.
pub fn check_regex(pat: &[u8]) -> Result<(), String> {
    let mut oc = 0u8;
    let shown = || super::apprentice::printable(pat);
    for (i, &c) in pat.iter().enumerate() {
        if c == 0 {
            break;
        }
        if c == oc && b"?*+{".contains(&c) {
            return Err(format!(
                "repetition-operator operand `{}' invalid in regex `{}'",
                c as char,
                shown()
            ));
        }
        if c == b'{' {
            let rest = &pat[i + 1..];
            let c1 = super::cutil::strtoull(rest, 10);
            if c1.used > 0 && c1.value > 1000 {
                return Err(format!(
                    "bounds too large {} in regex `{}'",
                    c1.value as i64,
                    shown()
                ));
            }
            if rest.get(c1.used) == Some(&b',') {
                let c2 = super::cutil::strtoull(&rest[c1.used + 1..], 10);
                if c2.used > 0 && c2.value > 1000 {
                    return Err(format!(
                        "bounds too large {} in regex `{}'",
                        c2.value as i64,
                        shown()
                    ));
                }
            }
        }
        oc = c;
        if super::cutil::is_print(c) || super::cutil::is_space(c) || c == 0x08 || c == 0x8a {
            continue;
        }
        return Err(format!(
            "non-ascii characters in regex \\{:#o} `{}'",
            c,
            shown()
        ));
    }
    Ok(())
}

/// Tradutor de ERE (glibc, `RE_SYNTAX_POSIX_EXTENDED`) pra sintaxe do `regex-syntax`.
struct Tr<'a> {
    p: &'a [u8],
    i: usize,
    out: String,
    newline: bool,
    open: usize,
}

fn lit(out: &mut String, b: u8) {
    if b.is_ascii_alphanumeric() || b == b' ' || b == b'_' {
        out.push(b as char);
    } else {
        let _ = write!(out, "\\x{b:02X}");
    }
}

fn translate(pat: &[u8], newline: bool) -> Result<String, RegexError> {
    let pat = super::cutil::cstr(pat);
    let mut t = Tr {
        p: pat,
        i: 0,
        out: String::new(),
        newline,
        open: 0,
    };
    t.parse_alt()?;
    if t.i < t.p.len() {
        // Sobrou um `)` sem par no nível de fora: o laço do parse_alt já o tratou como literal,
        // então aqui só chega lixo inesperado.
        return Err(REG_BADPAT);
    }
    Ok(t.out)
}

impl Tr<'_> {
    fn peek(&self) -> Option<u8> {
        self.p.get(self.i).copied()
    }

    fn parse_alt(&mut self) -> Result<(), RegexError> {
        self.parse_branch()?;
        while self.peek() == Some(b'|') {
            self.i += 1;
            self.out.push('|');
            self.parse_branch()?;
        }
        Ok(())
    }

    fn parse_branch(&mut self) -> Result<(), RegexError> {
        loop {
            match self.peek() {
                None | Some(b'|') => return Ok(()),
                Some(b')') if self.open > 0 => return Ok(()),
                _ => self.parse_expression()?,
            }
        }
    }

    fn parse_expression(&mut self) -> Result<(), RegexError> {
        let c = self.peek().ok_or(REG_BADPAT)?;
        self.i += 1;
        let mut can_repeat = true;
        let atom_start = self.out.len();
        match c {
            b'*' | b'+' | b'?' => return Err(REG_BADRPT),
            b'{' => {
                // `{` no começo: operador de intervalo sem operando.
                return Err(REG_BADRPT);
            }
            b'(' => {
                self.out.push_str("(?:");
                self.open += 1;
                self.parse_alt()?;
                if self.peek() != Some(b')') {
                    return Err(REG_EPAREN);
                }
                self.i += 1;
                self.open -= 1;
                self.out.push(')');
            }
            b')' => {
                // RE_UNMATCHED_RIGHT_PAREN_ORD: `)` sem par é literal.
                lit(&mut self.out, b')');
            }
            b'.' => self.out.push('.'),
            b'^' => {
                self.out.push('^');
                can_repeat = false;
            }
            b'$' => {
                self.out.push('$');
                can_repeat = false;
            }
            b'[' => self.parse_bracket()?,
            b'\\' => {
                let Some(e) = self.peek() else {
                    // `\` no fim: REG_EESCAPE.
                    return Err(RegexError {
                        code: 5,
                        message: "Trailing backslash",
                    });
                };
                self.i += 1;
                match e {
                    b'1'..=b'9' => return Err(REG_ESUBREG),
                    b'<' => {
                        self.out.push_str("\\b{start}");
                        can_repeat = false;
                    }
                    b'>' => {
                        self.out.push_str("\\b{end}");
                        can_repeat = false;
                    }
                    b'b' => {
                        self.out.push_str("\\b");
                        can_repeat = false;
                    }
                    b'B' => {
                        self.out.push_str("\\B");
                        can_repeat = false;
                    }
                    b'`' => {
                        self.out.push_str("\\A");
                        can_repeat = false;
                    }
                    b'\'' => {
                        self.out.push_str("\\z");
                        can_repeat = false;
                    }
                    b'w' => self.out.push_str("[0-9A-Za-z_]"),
                    b'W' => self.out.push_str("[^0-9A-Za-z_]"),
                    b's' => self.out.push_str("[\\t\\n\\x0B\\x0C\\r ]"),
                    b'S' => self.out.push_str("[^\\t\\n\\x0B\\x0C\\r ]"),
                    other => lit(&mut self.out, other),
                }
            }
            other => lit(&mut self.out, other),
        }
        // Operadores de repetição. Repetição de repetição (`a**`, `a{2}{3}`), que a glibc aceita,
        // precisa de grupo na sintaxe do Rust.
        let mut repeated = false;
        loop {
            match self.peek() {
                Some(b'*') | Some(b'+') | Some(b'?') => {
                    if !can_repeat {
                        return Err(REG_BADRPT);
                    }
                    let q = self.p[self.i];
                    self.i += 1;
                    if repeated {
                        self.out.insert_str(atom_start, "(?:");
                        self.out.push(')');
                    }
                    repeated = true;
                    self.out.push(q as char);
                }
                Some(b'{') => {
                    if !can_repeat {
                        return Err(REG_BADRPT);
                    }
                    self.i += 1;
                    let (lo, hi) = self.parse_interval()?;
                    if repeated {
                        self.out.insert_str(atom_start, "(?:");
                        self.out.push(')');
                    }
                    repeated = true;
                    match hi {
                        Some(h) if h == lo => {
                            let _ = write!(self.out, "{{{lo}}}");
                        }
                        Some(h) => {
                            let _ = write!(self.out, "{{{lo},{h}}}");
                        }
                        None => {
                            let _ = write!(self.out, "{{{lo},}}");
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    /// `{m}`, `{m,}`, `{m,n}`, `{,n}` (depois do `{`).
    fn parse_interval(&mut self) -> Result<(u32, Option<u32>), RegexError> {
        let num = |t: &mut Tr<'_>| -> Option<u32> {
            let s = t.i;
            let mut v: u32 = 0;
            while let Some(c) = t.peek().filter(u8::is_ascii_digit) {
                v = v.saturating_mul(10).saturating_add(u32::from(c - b'0'));
                t.i += 1;
            }
            (t.i > s).then_some(v)
        };
        let lo = num(self);
        let (lo, hi) = if self.peek() == Some(b',') {
            self.i += 1;
            let hi = num(self);
            (lo.unwrap_or(0), hi)
        } else {
            match lo {
                Some(v) => (v, Some(v)),
                None => {
                    return Err(if self.peek().is_none() {
                        REG_EBRACE
                    } else {
                        REG_BADBR
                    });
                }
            }
        };
        if self.peek() != Some(b'}') {
            return Err(if self.peek().is_none() {
                REG_EBRACE
            } else {
                REG_BADBR
            });
        }
        self.i += 1;
        if lo > RE_DUP_MAX || hi.is_some_and(|h| h > RE_DUP_MAX || h < lo) {
            return Err(REG_BADBR);
        }
        Ok((lo, hi))
    }

    /// Expressão entre colchetes (depois do `[`).
    fn parse_bracket(&mut self) -> Result<(), RegexError> {
        let mut negated = false;
        if self.peek() == Some(b'^') {
            negated = true;
            self.i += 1;
        }
        let mut items = String::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(REG_EBRACK);
            };
            if c == b']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            let start = self.bracket_elem()?;
            match start {
                Elem::Class(name) => items.push_str(&format!("[:{name}:]")),
                Elem::Char(a) => {
                    // Faixa?
                    if self.peek() == Some(b'-')
                        && self.p.get(self.i + 1).is_some_and(|&n| n != b']')
                    {
                        self.i += 1;
                        match self.bracket_elem()? {
                            Elem::Char(b) => {
                                if b < a {
                                    return Err(REG_ERANGE);
                                }
                                push_class_byte(&mut items, a);
                                items.push('-');
                                push_class_byte(&mut items, b);
                            }
                            Elem::Class(_) => return Err(REG_ERANGE),
                        }
                    } else {
                        push_class_byte(&mut items, a);
                    }
                }
            }
        }
        let mut s = String::from("[");
        if negated {
            s.push('^');
            if self.newline {
                // RE_HAT_LISTS_NOT_NEWLINE.
                s.push_str("\\n");
            }
        }
        if items.is_empty() {
            // Não acontece (o primeiro `]` é literal), mas evita classe vazia.
            return Err(REG_EBRACK);
        }
        s.push_str(&items);
        s.push(']');
        self.out.push_str(&s);
        Ok(())
    }

    fn bracket_elem(&mut self) -> Result<Elem, RegexError> {
        let c = self.peek().ok_or(REG_EBRACK)?;
        self.i += 1;
        if c == b'[' {
            match self.peek() {
                Some(b':') => {
                    self.i += 1;
                    let s = self.i;
                    while self.i + 1 < self.p.len()
                        && !(self.p[self.i] == b':' && self.p[self.i + 1] == b']')
                    {
                        self.i += 1;
                    }
                    if self.i + 1 >= self.p.len() {
                        return Err(REG_EBRACK);
                    }
                    let name = String::from_utf8_lossy(&self.p[s..self.i]).into_owned();
                    self.i += 2;
                    const CLASSES: [&str; 12] = [
                        "alpha", "upper", "lower", "digit", "xdigit", "space", "print", "punct",
                        "graph", "cntrl", "blank", "alnum",
                    ];
                    if !CLASSES.contains(&name.as_str()) {
                        return Err(REG_ECTYPE);
                    }
                    return Ok(Elem::Class(name));
                }
                Some(d @ (b'=' | b'.')) => {
                    self.i += 1;
                    let s = self.i;
                    while self.i + 1 < self.p.len()
                        && !(self.p[self.i] == d && self.p[self.i + 1] == b']')
                    {
                        self.i += 1;
                    }
                    if self.i + 1 >= self.p.len() {
                        return Err(REG_EBRACK);
                    }
                    let sym = &self.p[s..self.i];
                    self.i += 2;
                    if sym.len() != 1 {
                        return Err(REG_ECOLLATE);
                    }
                    return Ok(Elem::Char(sym[0]));
                }
                _ => return Ok(Elem::Char(b'[')),
            }
        }
        Ok(Elem::Char(c))
    }
}

enum Elem {
    Char(u8),
    Class(String),
}

fn push_class_byte(out: &mut String, b: u8) {
    if b.is_ascii_alphanumeric() {
        out.push(b as char);
    } else {
        let _ = write!(out, "\\x{b:02X}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(p: &str, h: &str) -> Option<(usize, usize)> {
        Regex::compile(p.as_bytes(), false, true)
            .unwrap()
            .find(h.as_bytes())
    }

    #[test]
    fn leftmost_longest() {
        assert_eq!(find("a|ab", "xab"), Some((1, 3)));
        assert_eq!(find("(png|jpg|jpeg)", "x.jpeg"), Some((2, 6)));
        assert_eq!(find("[0-9]+", "ab123c"), Some((2, 5)));
    }

    #[test]
    fn newline_semantics() {
        assert_eq!(find("^b", "a\nb"), Some((2, 3)));
        assert_eq!(find("a.b", "a\nb"), None);
        assert_eq!(find("a[^x]b", "a\nb"), None);
        assert_eq!(find("a$", "a\nb"), Some((0, 1)));
    }

    #[test]
    fn brackets_and_classes() {
        assert_eq!(find("[]a]+", "x]a]"), Some((1, 4)));
        assert_eq!(find("[[:space:]]perl", "#! perl"), Some((2, 7)));
        assert_eq!(find("[a-]", "-"), Some((0, 1)));
        assert_eq!(find("x\\.y", "x.y"), Some((0, 3)));
        assert_eq!(find("\\<word\\>", "a word b"), Some((2, 6)));
        assert!(Regex::compile(b"[[:nope:]]", false, true).is_err());
        assert!(Regex::compile(b"*a", false, true).is_err());
        assert!(Regex::compile(b"(a", false, true).is_err());
    }

    #[test]
    fn intervals() {
        assert_eq!(find("a{2,3}", "aaaa"), Some((0, 3)));
        assert_eq!(find("a{,2}b", "aab"), Some((0, 3)));
        assert_eq!(find("\\{x\\}", "{x}"), Some((0, 3)));
    }

    #[test]
    fn icase_and_bytes() {
        let r = Regex::compile(b"hello", true, true).unwrap();
        assert_eq!(r.find(b"say HeLLo"), Some((4, 9)));
        let r = Regex::compile(b"\x8a{3}", false, true).unwrap();
        assert_eq!(r.find(b"x\x8a\x8a\x8a"), Some((1, 4)));
    }

    #[test]
    fn check_regex_rules() {
        assert!(check_regex(b"a**").is_err());
        assert!(check_regex(b"a{2000}").is_err());
        assert!(check_regex(b"\xff").is_err());
        assert!(check_regex(b"\x8a").is_ok());
        assert!(check_regex(b"^#!.*perl").is_ok());
    }
}
