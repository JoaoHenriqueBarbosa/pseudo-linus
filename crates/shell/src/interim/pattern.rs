//! PROVISÓRIO: casamento de padrões mínimo pra desenvolvimento do núcleo, até chegar o módulo de
//! sala limpa (`src/pattern.rs`). Não é entregue.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchOpts {
    pub extglob: bool,
    pub nocase: bool,
    pub utf8: bool,
    pub period: bool,
    pub dotglob: bool,
}

#[derive(Clone, Debug)]
enum Tok {
    Lit(Vec<u8>),
    Any,
    Star,
    Class { neg: bool, items: Vec<ClassItem> },
    Ext { kind: u8, alts: Vec<Vec<Tok>> },
}

#[derive(Clone, Debug)]
enum ClassItem {
    Ch(u32),
    Range(u32, u32),
    Named(String),
}

pub struct Pattern {
    toks: Vec<Tok>,
    opts: MatchOpts,
}

fn units(s: &[u8], utf8: bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let n = if utf8 { crate::expand::utf8_char_len(&s[i..]) } else { 1 };
        out.push((i, i + n));
        i += n;
    }
    out
}

fn code(ch: &[u8]) -> u32 {
    match std::str::from_utf8(ch).ok().and_then(|s| s.chars().next()) {
        Some(c) if ch.len() > 1 => c as u32,
        _ => ch[0] as u32,
    }
}

fn parse(p: &[u8], i: &mut usize, opts: MatchOpts, stop_alt: bool) -> Vec<Tok> {
    let mut toks = Vec::new();
    while *i < p.len() {
        let c = p[*i];
        if stop_alt && (c == b'|' || c == b')') {
            break;
        }
        if opts.extglob && matches!(c, b'?' | b'*' | b'+' | b'@' | b'!') && p.get(*i + 1) == Some(&b'(') {
            let save = *i;
            *i += 2;
            let mut alts = Vec::new();
            loop {
                alts.push(parse(p, i, opts, true));
                if *i < p.len() && p[*i] == b'|' {
                    *i += 1;
                    continue;
                }
                break;
            }
            if *i < p.len() && p[*i] == b')' {
                *i += 1;
                toks.push(Tok::Ext { kind: c, alts });
                continue;
            }
            *i = save;
        }
        match c {
            b'\\' if *i + 1 < p.len() => {
                let n = if opts.utf8 { crate::expand::utf8_char_len(&p[*i + 1..]) } else { 1 };
                toks.push(Tok::Lit(p[*i + 1..*i + 1 + n].to_vec()));
                *i += 1 + n;
            }
            b'?' => {
                toks.push(Tok::Any);
                *i += 1;
            }
            b'*' => {
                toks.push(Tok::Star);
                *i += 1;
            }
            b'[' => match parse_class(p, *i, opts) {
                Some((t, end)) => {
                    toks.push(t);
                    *i = end;
                }
                None => {
                    toks.push(Tok::Lit(b"[".to_vec()));
                    *i += 1;
                }
            },
            _ => {
                let n = if opts.utf8 { crate::expand::utf8_char_len(&p[*i..]) } else { 1 };
                toks.push(Tok::Lit(p[*i..*i + n].to_vec()));
                *i += n;
            }
        }
    }
    toks
}

fn parse_class(p: &[u8], start: usize, opts: MatchOpts) -> Option<(Tok, usize)> {
    let mut i = start + 1;
    let mut neg = false;
    if i < p.len() && (p[i] == b'!' || p[i] == b'^') {
        neg = true;
        i += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    loop {
        if i >= p.len() {
            return None;
        }
        let c = p[i];
        if c == b']' && !first {
            return Some((Tok::Class { neg, items }, i + 1));
        }
        first = false;
        if c == b'[' && p.get(i + 1) == Some(&b':') {
            let rest = &p[i + 2..];
            if let Some(e) = rest.windows(2).position(|w| w == b":]") {
                items.push(ClassItem::Named(String::from_utf8_lossy(&rest[..e]).into_owned()));
                i += 2 + e + 2;
                continue;
            }
        }
        let (ch, n) = if c == b'\\' && i + 1 < p.len() {
            let n = if opts.utf8 { crate::expand::utf8_char_len(&p[i + 1..]) } else { 1 };
            (code(&p[i + 1..i + 1 + n]), 1 + n)
        } else {
            let n = if opts.utf8 { crate::expand::utf8_char_len(&p[i..]) } else { 1 };
            (code(&p[i..i + n]), n)
        };
        i += n;
        if i + 1 < p.len() && p[i] == b'-' && p[i + 1] != b']' {
            let n2 = if opts.utf8 { crate::expand::utf8_char_len(&p[i + 1..]) } else { 1 };
            let hi = code(&p[i + 1..i + 1 + n2]);
            items.push(ClassItem::Range(ch, hi));
            i += 1 + n2;
        } else {
            items.push(ClassItem::Ch(ch));
        }
    }
}

fn class_has(items: &[ClassItem], c: u32, nocase: bool) -> bool {
    let ch = char::from_u32(c);
    let test = |c: u32| {
        items.iter().any(|it| match it {
            ClassItem::Ch(x) => *x == c,
            ClassItem::Range(a, b) => *a <= c && c <= *b,
            ClassItem::Named(n) => {
                let Some(ch) = char::from_u32(c) else { return false };
                match n.as_str() {
                    "alpha" => ch.is_alphabetic(),
                    "digit" => ch.is_ascii_digit(),
                    "alnum" => ch.is_alphanumeric(),
                    "upper" => ch.is_uppercase(),
                    "lower" => ch.is_lowercase(),
                    "space" => ch.is_whitespace(),
                    "blank" => ch == ' ' || ch == '\t',
                    "punct" => ch.is_ascii_punctuation(),
                    "print" => !ch.is_control(),
                    "graph" => !ch.is_control() && ch != ' ',
                    "cntrl" => ch.is_control(),
                    "xdigit" => ch.is_ascii_hexdigit(),
                    "word" => ch.is_alphanumeric() || ch == '_',
                    _ => false,
                }
            }
        })
    };
    if test(c) {
        return true;
    }
    if nocase
        && let Some(ch) = ch {
            for alt in ch.to_lowercase().chain(ch.to_uppercase()) {
                if test(alt as u32) {
                    return true;
                }
            }
        }
    false
}

impl Pattern {
    pub fn new(pat: &[u8], opts: MatchOpts) -> Pattern {
        let mut i = 0;
        let toks = parse(pat, &mut i, opts, false);
        Pattern { toks, opts }
    }

    fn m(&self, toks: &[Tok], text: &[u8], u: &[(usize, usize)], pos: usize, depth: u32) -> bool {
        if depth > 2000 {
            return false;
        }
        let Some(t) = toks.first() else { return pos == u.len() };
        let rest = &toks[1..];
        let at_start = pos == 0;
        let leading_dot = at_start && self.opts.period && !self.opts.dotglob && u.first().is_some_and(|(a, b)| &text[*a..*b] == b".");
        match t {
            Tok::Lit(l) => {
                if pos >= u.len() {
                    return false;
                }
                let ch = &text[u[pos].0..u[pos].1];
                let eq = if self.opts.nocase {
                    crate::expand::case_all(ch, false, self.opts.utf8) == crate::expand::case_all(l, false, self.opts.utf8)
                } else {
                    ch == l.as_slice()
                };
                eq && self.m(rest, text, u, pos + 1, depth + 1)
            }
            Tok::Any => pos < u.len() && !leading_dot && self.m(rest, text, u, pos + 1, depth + 1),
            Tok::Star => {
                if leading_dot {
                    return false;
                }
                (pos..=u.len()).any(|k| self.m(rest, text, u, k, depth + 1))
            }
            Tok::Class { neg, items } => {
                if pos >= u.len() || leading_dot {
                    return false;
                }
                let c = code(&text[u[pos].0..u[pos].1]);
                class_has(items, c, self.opts.nocase) != *neg && self.m(rest, text, u, pos + 1, depth + 1)
            }
            Tok::Ext { kind, alts } => {
                let alt_matches = |a: usize, b: usize, s: &Self| -> bool {
                    let sub = &text[u.get(a).map_or(text.len(), |x| x.0)..u.get(b).map_or(text.len(), |x| x.0)];
                    let su = units(sub, s.opts.utf8);
                    alts.iter().any(|alt| s.m(alt, sub, &su, 0, depth + 1))
                };
                match kind {
                    b'@' => (pos..=u.len()).any(|k| alt_matches(pos, k, self) && self.m(rest, text, u, k, depth + 1)),
                    b'?' => self.m(rest, text, u, pos, depth + 1) || (pos..=u.len()).any(|k| alt_matches(pos, k, self) && self.m(rest, text, u, k, depth + 1)),
                    b'!' => {
                        if leading_dot {
                            return false;
                        }
                        (pos..=u.len()).any(|k| !alt_matches(pos, k, self) && self.m(rest, text, u, k, depth + 1))
                    }
                    _ => {
                        // `*(...)` e `+(...)`: repetição.
                        let min_one = *kind == b'+';
                        self.rep(alts, rest, text, u, pos, min_one, depth + 1)
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rep(&self, alts: &[Vec<Tok>], rest: &[Tok], text: &[u8], u: &[(usize, usize)], pos: usize, need: bool, depth: u32) -> bool {
        if depth > 2000 {
            return false;
        }
        if !need && self.m(rest, text, u, pos, depth + 1) {
            return true;
        }
        for k in pos + 1..=u.len() {
            let sub = &text[u[pos].0..u.get(k).map_or(text.len(), |x| x.0)];
            let su = units(sub, self.opts.utf8);
            if alts.iter().any(|alt| self.m(alt, sub, &su, 0, depth + 1)) && self.rep(alts, rest, text, u, k, false, depth + 1) {
                return true;
            }
        }
        false
    }

    pub fn matches(&self, text: &[u8]) -> bool {
        let u = units(text, self.opts.utf8);
        self.m(&self.toks, text, &u, 0, 0)
    }

    fn bounds(&self, text: &[u8]) -> Vec<usize> {
        let mut b: Vec<usize> = units(text, self.opts.utf8).into_iter().map(|x| x.0).collect();
        b.push(text.len());
        b
    }

    pub fn match_prefix(&self, text: &[u8], longest: bool) -> Option<usize> {
        let b = self.bounds(text);
        let it: Box<dyn Iterator<Item = &usize>> = if longest { Box::new(b.iter().rev()) } else { Box::new(b.iter()) };
        for &e in it {
            if self.matches(&text[..e]) {
                return Some(e);
            }
        }
        None
    }

    pub fn match_suffix(&self, text: &[u8], longest: bool) -> Option<usize> {
        let b = self.bounds(text);
        let it: Box<dyn Iterator<Item = &usize>> = if longest { Box::new(b.iter()) } else { Box::new(b.iter().rev()) };
        for &s in it {
            if self.matches(&text[s..]) {
                return Some(s);
            }
        }
        None
    }

    pub fn find(&self, text: &[u8], from: usize) -> Option<(usize, usize)> {
        let b = self.bounds(text);
        for &s in b.iter().filter(|x| **x >= from) {
            for &e in b.iter().rev().filter(|x| **x >= s) {
                if self.matches(&text[s..e]) {
                    return Some((s, e));
                }
            }
        }
        None
    }
}

pub fn has_glob_meta(pat: &[u8], extglob: bool) -> bool {
    let mut i = 0;
    while i < pat.len() {
        match pat[i] {
            b'\\' => i += 2,
            b'*' | b'?' => return true,
            b'[' => {
                if parse_class(pat, i, MatchOpts::default()).is_some() {
                    return true;
                }
                i += 1;
            }
            b'+' | b'@' | b'!' if extglob && pat.get(i + 1) == Some(&b'(') => return true,
            _ => i += 1,
        }
    }
    false
}

pub fn unescape(pat: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pat.len());
    let mut i = 0;
    while i < pat.len() {
        if pat[i] == b'\\' && i + 1 < pat.len() {
            out.push(pat[i + 1]);
            i += 2;
        } else {
            out.push(pat[i]);
            i += 1;
        }
    }
    out
}
