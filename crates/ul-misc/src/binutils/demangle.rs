//! Demangler do Itanium C++ ABI (`_Z...`), escrito a partir da especificação pública do ABI e do
//! formato de saída observável do `c++filt` (o código do libiberty é GPL e não foi consultado).
//!
//! Cobre: nomes aninhados, locais e não qualificados, `std::` e abreviações (`Sa`, `Sb`, `Ss`,
//! `Si`, `So`, `Sd`), substituições (`S_`, `S0_`...), parâmetros de template (`T_`), argumentos de
//! template (tipos, literais inteiros e `bool`, pacotes `J`), construtores e destrutores,
//! operadores, tipos builtin, ponteiros, referências, qualificadores, ponteiros para função,
//! arrays, ponteiros para membro, nomes especiais (`TV`, `TT`, `TI`, `TS`, `Th`, `Tv`, `GV`) e
//! sufixos de clone (`.constprop.0`).
//!
//! Fora do alcance (a entrada vira "não demanglável" e sai como veio): expressões (`X...E`,
//! `decltype`), `Dp` com expansão real de pacote, tipos vetoriais, lambdas com template, ABI tags
//! além de `B`, e os estilos java/gnat/dlang/rust.

#[derive(Clone, Debug)]
enum Node {
    Name(String),
    Ptr(Box<Node>),
    LRef(Box<Node>),
    RRef(Box<Node>),
    /// Tipo qualificado; o texto já traz o espaço inicial (` const`, ` volatile`).
    Qual(Box<Node>, String),
    Func {
        ret: Option<Box<Node>>,
        params: Vec<Node>,
    },
    Array(Box<Node>, String),
    /// Ponteiro para membro: classe e tipo do membro.
    Ptm(Box<Node>, Box<Node>),
}

fn params_text(params: &[Node]) -> String {
    if params.len() == 1 {
        if let Node::Name(n) = &params[0] {
            if n == "void" {
                return "()".to_string();
            }
        }
    }
    let v: Vec<String> = params.iter().map(to_string).collect();
    format!("({})", v.join(", "))
}

/// Parte esquerda e direita do tipo, como no algoritmo clássico de declaradores.
fn parts(n: &Node) -> (String, String) {
    match n {
        Node::Name(s) => (s.clone(), String::new()),
        Node::Qual(t, q) => {
            let (l, r) = parts(t);
            (format!("{l}{q}"), r)
        }
        Node::Ptr(t) => decl(t, "*"),
        Node::LRef(t) => decl(t, "&"),
        Node::RRef(t) => decl(t, "&&"),
        Node::Func { ret, params } => {
            let l = ret.as_ref().map(|r| to_string(r)).unwrap_or_default();
            (l, params_text(params))
        }
        Node::Array(t, dim) => {
            let (l, r) = parts(t);
            (l, format!(" [{dim}]{r}"))
        }
        Node::Ptm(class, mem) => {
            let (l, r) = parts(mem);
            let c = to_string(class);
            if matches!(**mem, Node::Func { .. }) {
                (format!("{l} ({c}::*"), format!("){r}"))
            } else {
                (format!("{l} {c}::*"), r)
            }
        }
    }
}

fn decl(t: &Node, sym: &str) -> (String, String) {
    let (l, r) = parts(t);
    match t {
        Node::Func { .. } | Node::Array(..) => (format!("{l} ({sym}"), format!("){r}")),
        _ => (format!("{l}{sym}"), r),
    }
}

fn to_string(n: &Node) -> String {
    let (l, r) = parts(n);
    format!("{l}{r}")
}

fn join_args(args: &[Node]) -> String {
    let v: Vec<String> = args.iter().map(to_string).collect();
    let mut s = String::from("<");
    s.push_str(&v.join(", "));
    if s.ends_with('>') {
        s.push(' ');
    }
    s.push('>');
    s
}

fn with_args(mut text: String, args: &[Node]) -> String {
    if text.ends_with('<') {
        text.push(' ');
    }
    text.push_str(&join_args(args));
    text
}

const BUILTINS: &[(u8, &str)] = &[
    (b'v', "void"),
    (b'w', "wchar_t"),
    (b'b', "bool"),
    (b'c', "char"),
    (b'a', "signed char"),
    (b'h', "unsigned char"),
    (b's', "short"),
    (b't', "unsigned short"),
    (b'i', "int"),
    (b'j', "unsigned int"),
    (b'l', "long"),
    (b'm', "unsigned long"),
    (b'x', "long long"),
    (b'y', "unsigned long long"),
    (b'n', "__int128"),
    (b'o', "unsigned __int128"),
    (b'f', "float"),
    (b'd', "double"),
    (b'e', "long double"),
    (b'g', "__float128"),
    (b'z', "..."),
];

const OPERATORS: &[(&str, &str)] = &[
    ("nw", "new"),
    ("na", "new[]"),
    ("dl", "delete"),
    ("da", "delete[]"),
    ("ps", "+"),
    ("ng", "-"),
    ("ad", "&"),
    ("de", "*"),
    ("co", "~"),
    ("pl", "+"),
    ("mi", "-"),
    ("ml", "*"),
    ("dv", "/"),
    ("rm", "%"),
    ("an", "&"),
    ("or", "|"),
    ("eo", "^"),
    ("aS", "="),
    ("pL", "+="),
    ("mI", "-="),
    ("mL", "*="),
    ("dV", "/="),
    ("rM", "%="),
    ("aN", "&="),
    ("oR", "|="),
    ("eO", "^="),
    ("ls", "<<"),
    ("rs", ">>"),
    ("lS", "<<="),
    ("rS", ">>="),
    ("eq", "=="),
    ("ne", "!="),
    ("lt", "<"),
    ("gt", ">"),
    ("le", "<="),
    ("ge", ">="),
    ("ss", "<=>"),
    ("nt", "!"),
    ("aa", "&&"),
    ("oo", "||"),
    ("pp", "++"),
    ("mm", "--"),
    ("cm", ","),
    ("pm", "->*"),
    ("pt", "->"),
    ("cl", "()"),
    ("ix", "[]"),
    ("qu", "?"),
];

struct NameRes {
    text: String,
    /// O nome termina em argumentos de template (função com tipo de retorno codificado).
    tpl: bool,
    /// Construtor, destrutor ou operador de conversão (sem tipo de retorno).
    special: bool,
    /// Qualificadores de função membro (` const`, ` &`...).
    cv: String,
}

struct Demangler<'a> {
    s: &'a [u8],
    p: usize,
    subs: Vec<Node>,
    targs: Vec<Node>,
    last_unq: String,
    no_params: bool,
    name_ctx: bool,
    targ_depth: u32,
    depth: u32,
}

impl<'a> Demangler<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.p).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.s.get(self.p + n).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.p += 1;
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, t: &[u8]) -> bool {
        if self.s[self.p..].starts_with(t) {
            self.p += t.len();
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.p;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.p += 1;
        }
        if start == self.p {
            return None;
        }
        std::str::from_utf8(&self.s[start..self.p]).ok()?.parse().ok()
    }

    fn source_name(&mut self) -> Option<String> {
        let n = self.number()?;
        let end = self.p.checked_add(n)?;
        let raw = self.s.get(self.p..end)?;
        self.p = end;
        let name = String::from_utf8_lossy(raw).into_owned();
        if name.starts_with("_GLOBAL_") && name.len() > 9 && &name[9..10] == "N" {
            return Some("(anonymous namespace)".to_string());
        }
        Some(name)
    }

    fn parse_unqualified(&mut self) -> Option<(String, bool)> {
        let c = self.peek()?;
        if c.is_ascii_digit() {
            let mut name = self.source_name()?;
            self.last_unq = name.clone();
            while self.peek() == Some(b'B') {
                self.p += 1;
                let tag = self.source_name()?;
                name.push_str(&format!("[abi:{tag}]"));
            }
            return Some((name, false));
        }
        match c {
            b'L' => {
                self.p += 1;
                self.parse_unqualified()
            }
            b'C' => {
                self.p += 1;
                if self.eat(b'I') {
                    return None;
                }
                let d = self.peek()?;
                if !(b'1'..=b'5').contains(&d) {
                    return None;
                }
                self.p += 1;
                Some((self.last_unq.clone(), true))
            }
            b'D' if matches!(self.peek_at(1), Some(b'0'..=b'2' | b'4' | b'5')) => {
                self.p += 2;
                Some((format!("~{}", self.last_unq), true))
            }
            b'U' => {
                self.p += 1;
                if self.eat(b't') {
                    let idx = match self.number() {
                        Some(n) => n + 2,
                        None => 1,
                    };
                    if !self.eat(b'_') {
                        return None;
                    }
                    return Some((format!("{{unnamed type#{idx}}}"), false));
                }
                if self.eat(b'l') {
                    let mut params = Vec::new();
                    while self.peek()? != b'E' {
                        params.push(self.parse_type()?);
                    }
                    self.p += 1;
                    let idx = match self.number() {
                        Some(n) => n + 2,
                        None => 1,
                    };
                    if !self.eat(b'_') {
                        return None;
                    }
                    return Some((format!("{{lambda{}#{idx}}}", params_text(&params)), false));
                }
                None
            }
            b'a'..=b'z' => {
                let code = std::str::from_utf8(self.s.get(self.p..self.p + 2)?).ok()?;
                if code == "cv" {
                    self.p += 2;
                    let t = self.parse_type()?;
                    return Some((format!("operator {}", to_string(&t)), true));
                }
                if code == "li" {
                    self.p += 2;
                    let n = self.source_name()?;
                    return Some((format!("operator\"\" {n}"), false));
                }
                let (_, op) = OPERATORS.iter().find(|(k, _)| *k == code)?;
                self.p += 2;
                if op.as_bytes()[0].is_ascii_alphabetic() {
                    Some((format!("operator {op}"), false))
                } else {
                    Some((format!("operator{op}"), false))
                }
            }
            _ => None,
        }
    }

    fn parse_subst(&mut self) -> Option<Node> {
        if !self.eat(b'S') {
            return None;
        }
        let c = self.peek()?;
        let abbr = match c {
            b'a' => Some("std::allocator"),
            b'b' => Some("std::basic_string"),
            b's' => Some("std::string"),
            b'i' => Some("std::istream"),
            b'o' => Some("std::ostream"),
            b'd' => Some("std::iostream"),
            _ => None,
        };
        if let Some(a) = abbr {
            self.p += 1;
            return Some(Node::Name(a.to_string()));
        }
        let mut idx: usize = 0;
        if c == b'_' {
            self.p += 1;
            return self.subs.first().cloned();
        }
        loop {
            let d = self.peek()?;
            self.p += 1;
            let v = match d {
                b'0'..=b'9' => usize::from(d - b'0'),
                b'A'..=b'Z' => usize::from(d - b'A') + 10,
                b'_' => break,
                _ => return None,
            };
            idx = idx.checked_mul(36)?.checked_add(v)?;
        }
        self.subs.get(idx + 1).cloned()
    }

    fn parse_tparam(&mut self) -> Option<Node> {
        if !self.eat(b'T') {
            return None;
        }
        let idx = match self.number() {
            Some(n) => n + 1,
            None => 0,
        };
        if !self.eat(b'_') {
            return None;
        }
        self.targs.get(idx).cloned()
    }

    fn parse_template_args(&mut self) -> Option<Vec<Node>> {
        if !self.eat(b'I') {
            return None;
        }
        self.targ_depth += 1;
        let mut args = Vec::new();
        loop {
            match self.peek()? {
                b'E' => {
                    self.p += 1;
                    break;
                }
                b'X' => return None,
                b'L' => {
                    self.p += 1;
                    args.push(self.parse_literal()?);
                }
                b'J' => {
                    self.p += 1;
                    while self.peek()? != b'E' {
                        if self.peek()? == b'L' {
                            self.p += 1;
                            args.push(self.parse_literal()?);
                        } else {
                            args.push(self.parse_type()?);
                        }
                    }
                    self.p += 1;
                }
                _ => args.push(self.parse_type()?),
            }
        }
        self.targ_depth -= 1;
        if self.name_ctx && self.targ_depth == 0 {
            self.targs = args.clone();
        }
        Some(args)
    }

    fn parse_literal(&mut self) -> Option<Node> {
        if self.eat_str(b"_Z") {
            let e = self.parse_encoding(false)?;
            if !self.eat(b'E') {
                return None;
            }
            return Some(Node::Name(e));
        }
        let ty = self.parse_type()?;
        let start = self.p;
        while self.peek()? != b'E' {
            self.p += 1;
        }
        let raw = String::from_utf8_lossy(&self.s[start..self.p]).into_owned();
        self.p += 1;
        let val = match raw.strip_prefix('n') {
            Some(d) => format!("-{d}"),
            None => raw.clone(),
        };
        let t = to_string(&ty);
        let text = match t.as_str() {
            "bool" => (if raw == "0" { "false" } else { "true" }).to_string(),
            "int" => val,
            "unsigned int" => format!("{val}u"),
            "long" => format!("{val}l"),
            "unsigned long" => format!("{val}ul"),
            "long long" => format!("{val}ll"),
            "unsigned long long" => format!("{val}ull"),
            _ => format!("({t}){val}"),
        };
        Some(Node::Name(text))
    }

    fn parse_name(&mut self) -> Option<NameRes> {
        match self.peek()? {
            b'N' => self.parse_nested(),
            b'Z' => self.parse_local(),
            b'S' if self.peek_at(1) != Some(b't') => {
                let n = self.parse_subst()?;
                let mut text = to_string(&n);
                let mut tpl = false;
                if self.peek() == Some(b'I') {
                    let args = self.parse_template_args()?;
                    text = with_args(text, &args);
                    tpl = true;
                }
                Some(NameRes {
                    text,
                    tpl,
                    special: false,
                    cv: String::new(),
                })
            }
            _ => {
                let mut text = String::new();
                if self.eat_str(b"St") {
                    text.push_str("std::");
                }
                let (u, special) = self.parse_unqualified()?;
                text.push_str(&u);
                let mut tpl = false;
                if self.peek() == Some(b'I') {
                    self.subs.push(Node::Name(text.clone()));
                    let args = self.parse_template_args()?;
                    text = with_args(text, &args);
                    tpl = true;
                }
                Some(NameRes {
                    text,
                    tpl,
                    special,
                    cv: String::new(),
                })
            }
        }
    }

    fn parse_nested(&mut self) -> Option<NameRes> {
        self.eat(b'N');
        let mut quals = (false, false, false);
        loop {
            match self.peek()? {
                b'r' => quals.0 = true,
                b'V' => quals.1 = true,
                b'K' => quals.2 = true,
                _ => break,
            }
            self.p += 1;
        }
        let mut cv = String::new();
        if quals.2 {
            cv.push_str(" const");
        }
        if quals.1 {
            cv.push_str(" volatile");
        }
        if quals.0 {
            cv.push_str(" restrict");
        }
        if self.eat(b'R') {
            cv.push_str(" &");
        } else if self.eat(b'O') {
            cv.push_str(" &&");
        }
        let mut prefix = String::new();
        let mut tpl = false;
        let mut special = false;
        loop {
            let c = self.peek()?;
            if c == b'E' {
                self.p += 1;
                break;
            }
            tpl = false;
            special = false;
            match c {
                b'I' => {
                    let args = self.parse_template_args()?;
                    prefix = with_args(prefix, &args);
                    tpl = true;
                }
                b'S' if self.peek_at(1) == Some(b't') => {
                    self.p += 2;
                    prefix = "std".to_string();
                    continue;
                }
                b'S' => {
                    let n = self.parse_subst()?;
                    prefix = to_string(&n);
                }
                b'T' => {
                    let n = self.parse_tparam()?;
                    prefix = to_string(&n);
                }
                _ => {
                    let (u, sp) = self.parse_unqualified()?;
                    if !prefix.is_empty() {
                        prefix.push_str("::");
                    }
                    prefix.push_str(&u);
                    special = sp;
                }
            }
            if self.peek() != Some(b'E') {
                self.subs.push(Node::Name(prefix.clone()));
            }
        }
        Some(NameRes {
            text: prefix,
            tpl,
            special,
            cv,
        })
    }

    fn parse_local(&mut self) -> Option<NameRes> {
        self.eat(b'Z');
        let enc = self.parse_encoding(false)?;
        if !self.eat(b'E') {
            return None;
        }
        let mut res = if self.eat(b's') {
            NameRes {
                text: "string literal".to_string(),
                tpl: false,
                special: false,
                cv: String::new(),
            }
        } else {
            let saved = self.name_ctx;
            self.name_ctx = true;
            let r = self.parse_name();
            self.name_ctx = saved;
            r?
        };
        res.text = format!("{enc}::{}", res.text);
        // Discriminador: `_n` ou `__n_`.
        if self.peek() == Some(b'_') {
            if self.peek_at(1) == Some(b'_') {
                self.p += 2;
                self.number()?;
                if !self.eat(b'_') {
                    return None;
                }
            } else if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
                self.p += 2;
            }
        }
        Some(res)
    }

    fn push_sub(&mut self, n: &Node) {
        self.subs.push(n.clone());
    }

    fn parse_type(&mut self) -> Option<Node> {
        self.depth += 1;
        if self.depth > 256 {
            return None;
        }
        let r = self.parse_type_inner();
        self.depth -= 1;
        r
    }

    fn parse_type_inner(&mut self) -> Option<Node> {
        let c = self.peek()?;
        if let Some((_, n)) = BUILTINS.iter().find(|(k, _)| *k == c) {
            self.p += 1;
            return Some(Node::Name((*n).to_string()));
        }
        match c {
            b'r' | b'V' | b'K' => {
                let mut q = (false, false, false);
                loop {
                    match self.peek() {
                        Some(b'r') => q.0 = true,
                        Some(b'V') => q.1 = true,
                        Some(b'K') => q.2 = true,
                        _ => break,
                    }
                    self.p += 1;
                }
                let inner = self.parse_type()?;
                let mut s = String::new();
                if q.2 {
                    s.push_str(" const");
                }
                if q.1 {
                    s.push_str(" volatile");
                }
                if q.0 {
                    s.push_str(" restrict");
                }
                let n = Node::Qual(Box::new(inner), s);
                self.push_sub(&n);
                Some(n)
            }
            b'P' | b'R' | b'O' => {
                self.p += 1;
                let inner = Box::new(self.parse_type()?);
                let n = match c {
                    b'P' => Node::Ptr(inner),
                    b'R' => Node::LRef(inner),
                    _ => Node::RRef(inner),
                };
                self.push_sub(&n);
                Some(n)
            }
            b'F' => {
                self.p += 1;
                self.eat(b'Y');
                let ret = self.parse_type()?;
                let mut params = Vec::new();
                while self.peek()? != b'E' {
                    params.push(self.parse_type()?);
                }
                self.p += 1;
                let n = Node::Func {
                    ret: Some(Box::new(ret)),
                    params,
                };
                self.push_sub(&n);
                Some(n)
            }
            b'A' => {
                self.p += 1;
                let dim = match self.number() {
                    Some(d) => d.to_string(),
                    None => String::new(),
                };
                if !self.eat(b'_') {
                    return None;
                }
                let inner = self.parse_type()?;
                let n = Node::Array(Box::new(inner), dim);
                self.push_sub(&n);
                Some(n)
            }
            b'M' => {
                self.p += 1;
                let class = self.parse_type()?;
                let mem = self.parse_type()?;
                let n = Node::Ptm(Box::new(class), Box::new(mem));
                self.push_sub(&n);
                Some(n)
            }
            b'T' => {
                let mut n = self.parse_tparam()?;
                self.push_sub(&n);
                if self.peek() == Some(b'I') {
                    let args = self.parse_template_args()?;
                    n = Node::Name(with_args(to_string(&n), &args));
                    self.push_sub(&n);
                }
                Some(n)
            }
            b'S' if self.peek_at(1) != Some(b't') => {
                let mut n = self.parse_subst()?;
                if self.peek() == Some(b'I') {
                    let args = self.parse_template_args()?;
                    n = Node::Name(with_args(to_string(&n), &args));
                    self.push_sub(&n);
                }
                Some(n)
            }
            b'D' => {
                let t = self.peek_at(1)?;
                match t {
                    b'n' => {
                        self.p += 2;
                        Some(Node::Name("decltype(nullptr)".to_string()))
                    }
                    b'a' => {
                        self.p += 2;
                        Some(Node::Name("auto".to_string()))
                    }
                    b'c' => {
                        self.p += 2;
                        Some(Node::Name("decltype(auto)".to_string()))
                    }
                    b'i' => {
                        self.p += 2;
                        Some(Node::Name("char32_t".to_string()))
                    }
                    b's' => {
                        self.p += 2;
                        Some(Node::Name("char16_t".to_string()))
                    }
                    b'u' => {
                        self.p += 2;
                        Some(Node::Name("char8_t".to_string()))
                    }
                    b'p' => {
                        self.p += 2;
                        self.parse_type()
                    }
                    _ => None,
                }
            }
            b'u' => {
                self.p += 1;
                let n = Node::Name(self.source_name()?);
                self.push_sub(&n);
                Some(n)
            }
            b'N' | b'Z' | b'0'..=b'9' | b'S' => {
                let r = self.parse_name()?;
                let n = Node::Name(r.text);
                self.push_sub(&n);
                Some(n)
            }
            _ => None,
        }
    }

    fn parse_encoding(&mut self, top: bool) -> Option<String> {
        if self.peek() == Some(b'T') {
            let kind = self.peek_at(1)?;
            let label = match kind {
                b'V' => Some("vtable for "),
                b'T' => Some("VTT for "),
                b'I' => Some("typeinfo for "),
                b'S' => Some("typeinfo name for "),
                _ => None,
            };
            if let Some(l) = label {
                self.p += 2;
                let t = self.parse_type()?;
                return Some(format!("{l}{}", to_string(&t)));
            }
            match kind {
                b'h' => {
                    self.p += 2;
                    self.eat(b'n');
                    self.number()?;
                    if !self.eat(b'_') {
                        return None;
                    }
                    let e = self.parse_encoding(false)?;
                    return Some(format!("non-virtual thunk to {e}"));
                }
                b'v' => {
                    self.p += 2;
                    for _ in 0..2 {
                        self.eat(b'n');
                        self.number()?;
                        if !self.eat(b'_') {
                            return None;
                        }
                    }
                    let e = self.parse_encoding(false)?;
                    return Some(format!("virtual thunk to {e}"));
                }
                b'W' | b'H' => {
                    self.p += 2;
                    let r = self.parse_name()?;
                    let l = if kind == b'W' {
                        "TLS wrapper function for "
                    } else {
                        "TLS init function for "
                    };
                    return Some(format!("{l}{}", r.text));
                }
                _ => return None,
            }
        }
        if self.s[self.p..].starts_with(b"GV") {
            self.p += 2;
            let r = self.parse_name()?;
            return Some(format!("guard variable for {}", r.text));
        }
        let saved = self.name_ctx;
        self.name_ctx = true;
        let nm = self.parse_name();
        self.name_ctx = false;
        let nm = nm?;
        if matches!(self.peek(), None | Some(b'E') | Some(b'.')) {
            self.name_ctx = saved;
            return Some(nm.text);
        }
        let ret = if nm.tpl && !nm.special {
            Some(self.parse_type()?)
        } else {
            None
        };
        let mut params = Vec::new();
        while !matches!(self.peek(), None | Some(b'E') | Some(b'.')) {
            params.push(self.parse_type()?);
        }
        self.name_ctx = saved;
        if params.is_empty() {
            return None;
        }
        if top && self.no_params {
            return Some(nm.text);
        }
        let mut out = String::new();
        if let Some(r) = ret {
            out.push_str(&to_string(&r));
            out.push(' ');
        }
        out.push_str(&nm.text);
        out.push_str(&params_text(&params));
        out.push_str(&nm.cv);
        Some(out)
    }
}

/// Sufixos de clone do GCC (`.constprop.0`, `.isra.1`...) viram ` [clone .constprop.0]`.
fn clone_suffix(rest: &[u8]) -> Option<String> {
    let mut out = String::new();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] != b'.' {
            return None;
        }
        let start = i;
        i += 1;
        let name_start = i;
        while i < rest.len() && (rest[i].is_ascii_lowercase() || rest[i] == b'_') {
            i += 1;
        }
        if i == name_start {
            return None;
        }
        while i + 1 < rest.len() && rest[i] == b'.' && rest[i + 1].is_ascii_digit() {
            i += 1;
            while i < rest.len() && rest[i].is_ascii_digit() {
                i += 1;
            }
        }
        out.push_str(&format!(
            " [clone {}]",
            String::from_utf8_lossy(&rest[start..i])
        ));
    }
    Some(out)
}

/// Demangla um símbolo. `None` quando não é um nome Itanium inteiro e válido (o chamador imprime
/// o original). `types` liga a tentativa de demanglar também tipos soltos (`-t`).
pub fn demangle(input: &[u8], no_params: bool, types: bool) -> Option<String> {
    let mut d = Demangler {
        s: input,
        p: 0,
        subs: Vec::new(),
        targs: Vec::new(),
        last_unq: String::new(),
        no_params,
        name_ctx: false,
        targ_depth: 0,
        depth: 0,
    };
    if input.starts_with(b"_Z") {
        d.p = 2;
        let mut out = d.parse_encoding(true)?;
        if d.p < input.len() {
            out.push_str(&clone_suffix(&input[d.p..])?);
        }
        return Some(out);
    }
    if types {
        let t = d.parse_type()?;
        if d.p == input.len() {
            return Some(to_string(&t));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm(s: &str) -> Option<String> {
        demangle(s.as_bytes(), false, false)
    }

    #[test]
    fn simple() {
        assert_eq!(dm("_Z3foov").as_deref(), Some("foo()"));
        assert_eq!(dm("_Z3fooiPKc").as_deref(), Some("foo(int, char const*)"));
        assert_eq!(dm("_ZN3Foo3barEv").as_deref(), Some("Foo::bar()"));
        assert_eq!(dm("_ZNK3Foo3barEv").as_deref(), Some("Foo::bar() const"));
        assert_eq!(dm("_ZN3FooC1Ev").as_deref(), Some("Foo::Foo()"));
        assert_eq!(dm("_ZN3FooD0Ev").as_deref(), Some("Foo::~Foo()"));
    }

    #[test]
    fn templates_and_std() {
        assert_eq!(
            dm("_Z1fSt6vectorIiSaIiEE").as_deref(),
            Some("f(std::vector<int, std::allocator<int> >)")
        );
        assert_eq!(dm("_Z1fIiEvT_").as_deref(), Some("void f<int>(int)"));
        assert_eq!(dm("_ZNSt8ios_base4InitC1Ev").as_deref(), Some("std::ios_base::Init::Init()"));
        assert_eq!(dm("_Z1fPFviE").as_deref(), Some("f(void (*)(int))"));
        assert_eq!(dm("_Z1fS_").as_deref(), None);
    }

    #[test]
    fn specials() {
        assert_eq!(dm("_ZTV3Foo").as_deref(), Some("vtable for Foo"));
        assert_eq!(dm("_ZTI3Foo").as_deref(), Some("typeinfo for Foo"));
        assert_eq!(dm("_ZZ4mainE1x").as_deref(), Some("main::x"));
        assert_eq!(dm("_Z3foov.constprop.0").as_deref(), Some("foo() [clone .constprop.0]"));
        assert_eq!(dm("notmangled"), None);
    }
}
