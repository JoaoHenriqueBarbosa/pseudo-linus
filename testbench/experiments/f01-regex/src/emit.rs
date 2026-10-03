//! Tradutor do AST GNU pra sintaxe de cada motor candidato.
//!
//! Cada motor tem um "sabor" de sintaxe. O tradutor devolve o texto do padrão e o mapa de grupos
//! (motores sem grupo não capturante ganham grupos sintéticos, e o mapa diz onde ficou cada grupo
//! original). Construções que o motor não tem viram [`EmitError::Unsupported`], que é resultado
//! (o motor não serve praquilo), não falha da bancada.

use crate::ast::{Assertion, Node, PosixClass, Regex, Set, SetItem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// `regex`, `regex-automata`.
    Rust,
    /// `fancy-regex`: sintaxe do `regex` mais backrefs e lookaround.
    Fancy,
    /// Sintaxe Ruby do Oniguruma (`rusty_expressions`, `ferroni`).
    Onig,
    /// ERE POSIX pura (`revera`).
    PosixEre,
    /// BRE do `posix-regex` (relibc).
    PosixBre,
    /// `regast`: sintaxe Perl reduzida, sem classes POSIX nem flags.
    Regast,
    /// `resharp`: sintaxe do `regex` com `_`, `&` e `~` reservados.
    Resharp,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EmitOptions {
    pub icase: bool,
    /// Variante pra busca a partir de um sufixo da linha: `^` e `` \` `` nunca casam.
    pub notbol: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emitted {
    pub pattern: String,
    /// `group_map[i]` = índice, no motor, do grupo original `i + 1`.
    pub group_map: Vec<usize>,
    /// Total de grupos no padrão emitido (originais mais sintéticos).
    pub engine_groups: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmitError {
    Unsupported(&'static str),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmitError::Unsupported(what) => write!(f, "sem suporte a {what}"),
        }
    }
}

impl Flavor {
    fn noncapturing(self) -> bool {
        !matches!(self, Flavor::PosixEre | Flavor::PosixBre)
    }

    fn native_icase(self) -> bool {
        !matches!(self, Flavor::Regast)
    }

    fn backrefs(self) -> bool {
        matches!(self, Flavor::Fancy | Flavor::Onig | Flavor::PosixBre)
    }
}

pub fn emit(re: &Regex, flavor: Flavor, opts: EmitOptions) -> Result<Emitted, EmitError> {
    let root = if opts.icase && !flavor.native_icase() { fold_case(&re.root) } else { re.root.clone() };
    let mut e = Emitter { flavor, opts, out: String::new(), next_group: 1, group_map: vec![0; re.groups] };
    e.node(&root)?;
    let mut pattern = e.out;
    if opts.icase && matches!(flavor, Flavor::Rust | Flavor::Fancy) {
        pattern = format!("(?i){pattern}");
    }
    Ok(Emitted { pattern, group_map: e.group_map, engine_groups: e.next_group - 1 })
}

struct Emitter {
    flavor: Flavor,
    opts: EmitOptions,
    out: String,
    next_group: usize,
    group_map: Vec<usize>,
}

impl Emitter {
    fn open_group(&mut self, capture: bool) {
        match (self.flavor, capture) {
            (Flavor::PosixBre, _) => self.out.push_str("\\("),
            (_, true) => self.out.push('('),
            (f, false) if f.noncapturing() => self.out.push_str("(?:"),
            (_, false) => self.out.push('('),
        }
    }

    fn close_group(&mut self) {
        if self.flavor == Flavor::PosixBre {
            self.out.push_str("\\)");
        } else {
            self.out.push(')');
        }
    }

    /// Grupo sintético (agrupamento sem captura); em ERE/BRE POSIX vira captura extra.
    fn wrap(&mut self, n: &Node) -> Result<(), EmitError> {
        let capture = !self.flavor.noncapturing();
        if capture {
            self.next_group += 1;
        }
        self.open_group(false);
        self.node(n)?;
        self.close_group();
        Ok(())
    }

    fn alt_sep(&mut self) {
        if self.flavor == Flavor::PosixBre {
            self.out.push_str("\\|");
        } else {
            self.out.push('|');
        }
    }

    fn node(&mut self, n: &Node) -> Result<(), EmitError> {
        match n {
            Node::Empty => {
                if self.flavor == Flavor::PosixEre {
                    // ERE POSIX não aceita ramo nem grupo vazio; `a{0}` casa o vazio.
                    self.out.push_str("a{0}");
                }
            }
            Node::Char(c) => self.literal(*c),
            Node::Any => self.out.push('.'),
            Node::Set(s) => self.set(s)?,
            Node::Assert(a) => self.assertion(*a)?,
            Node::Group { index, inner } => {
                self.group_map[index - 1] = self.next_group;
                self.next_group += 1;
                self.open_group(true);
                self.node(inner)?;
                self.close_group();
            }
            Node::Concat(items) => {
                for item in items {
                    match item {
                        Node::Alt(_) => self.wrap(item)?,
                        _ => self.node(item)?,
                    }
                }
            }
            Node::Alt(branches) => {
                for (i, b) in branches.iter().enumerate() {
                    if i > 0 {
                        self.alt_sep();
                    }
                    self.node(b)?;
                }
            }
            Node::Repeat { .. } if repeated_assertion(n).is_some() => {
                // Repetir uma asserção (visão do dfa.c pra `^*`, `^+++`): zero vezes é vazio, uma ou
                // mais é ela mesma.
                if let Some((a, true)) = repeated_assertion(n) {
                    self.assertion(a)?;
                }
            }
            Node::Repeat { inner, min, max } => {
                // ERE POSIX limita contagem a RE_DUP_MAX = 255; o GNU aceita até 32767. Sem grupo
                // dentro, `x{0,300}` vira `x{0,255}x{0,45}`, que casa o mesmo.
                let limit = 255u32;
                let big = self.flavor == Flavor::PosixEre && (*min > limit || max.is_some_and(|m| m > limit));
                let mut chunks = vec![(*min, *max)];
                if big && !has_group(inner) {
                    chunks.clear();
                    let (mut lo, mut hi) = (*min, *max);
                    while lo > limit || hi.is_some_and(|h| h > limit) {
                        let c_lo = lo.min(limit);
                        let c_hi = match hi {
                            Some(h) => h.min(limit),
                            None => c_lo,
                        };
                        chunks.push((c_lo, Some(c_hi)));
                        lo -= c_lo;
                        hi = hi.map(|h| h - c_hi);
                        if hi.is_none() && lo <= limit {
                            break;
                        }
                    }
                    chunks.push((lo, hi));
                }
                let atomic = matches!(
                    **inner,
                    Node::Char(_) | Node::Any | Node::Set(_) | Node::Group { .. } | Node::Backref(_)
                );
                for (lo, hi) in chunks {
                    if atomic {
                        self.node(inner)?;
                    } else {
                        self.wrap(inner)?;
                    }
                    self.quantifier(lo, hi);
                }
            }
            Node::Backref(n) => {
                if !self.flavor.backrefs() {
                    return Err(EmitError::Unsupported("backref"));
                }
                let g = self.group_map[n - 1];
                match self.flavor {
                    Flavor::Fancy | Flavor::Onig if g > 9 => self.out.push_str(&format!("\\k<{g}>")),
                    _ => self.out.push_str(&format!("\\{g}")),
                }
            }
        }
        Ok(())
    }

    fn quantifier(&mut self, min: u32, max: Option<u32>) {
        self.quantifier_text(min, max);
    }

    fn quantifier_text(&mut self, min: u32, max: Option<u32>) {
        let bre = self.flavor == Flavor::PosixBre;
        let (open, close) = if bre { ("\\{", "\\}") } else { ("{", "}") };
        match (min, max) {
            (0, None) => self.out.push('*'),
            (1, None) if !bre => self.out.push('+'),
            (0, Some(1)) if !bre => self.out.push('?'),
            (n, None) => self.out.push_str(&format!("{open}{n},{close}")),
            (n, Some(m)) if n == m => self.out.push_str(&format!("{open}{n}{close}")),
            (n, Some(m)) => self.out.push_str(&format!("{open}{n},{m}{close}")),
        }
    }

    fn literal(&mut self, c: char) {
        let special = match self.flavor {
            Flavor::Rust | Flavor::Fancy => regex_syntax::is_meta_character(c),
            Flavor::Resharp => resharp_parser::is_meta_character(c),
            Flavor::Onig => "\\.^$|?*+()[]{}".contains(c),
            Flavor::PosixEre => "\\.^$|?*+()[]{}".contains(c),
            Flavor::PosixBre => "\\.[*^$".contains(c),
            Flavor::Regast => "\\.+*?()[]{}|^$-".contains(c),
        };
        if special {
            self.out.push('\\');
        }
        match c {
            '\n' if matches!(self.flavor, Flavor::Rust | Flavor::Fancy | Flavor::Resharp | Flavor::Onig) => {
                self.out.push_str("\\n")
            }
            _ => self.out.push(c),
        }
    }

    fn assertion(&mut self, a: Assertion) -> Result<(), EmitError> {
        let f = self.flavor;
        let start_never = self.opts.notbol;
        let text: &str = match a {
            Assertion::LineStart | Assertion::BufStart if start_never => ".^",
            Assertion::LineStart => match f {
                Flavor::Onig => "\\A",
                _ => "^",
            },
            Assertion::LineEnd => match f {
                Flavor::Onig => "\\z",
                _ => "$",
            },
            Assertion::BufStart => match f {
                Flavor::Rust | Flavor::Fancy | Flavor::Onig | Flavor::Resharp => "\\A",
                _ => "^",
            },
            Assertion::BufEnd => match f {
                Flavor::Rust | Flavor::Fancy | Flavor::Onig | Flavor::Resharp => "\\z",
                _ => "$",
            },
            Assertion::WordBoundary => match f {
                Flavor::Rust | Flavor::Fancy | Flavor::Onig | Flavor::Resharp => "\\b",
                _ => return Err(EmitError::Unsupported("word-boundary")),
            },
            Assertion::NotWordBoundary => match f {
                Flavor::Rust | Flavor::Fancy | Flavor::Onig | Flavor::Resharp => "\\B",
                _ => return Err(EmitError::Unsupported("word-boundary")),
            },
            Assertion::WordStart => match f {
                Flavor::Rust | Flavor::Resharp => "\\b{start}",
                Flavor::Fancy | Flavor::Onig => "\\b(?=\\w)",
                Flavor::PosixBre => "\\<",
                _ => return Err(EmitError::Unsupported("word-boundary")),
            },
            Assertion::WordEnd => match f {
                Flavor::Rust | Flavor::Resharp => "\\b{end}",
                Flavor::Fancy | Flavor::Onig => "\\b(?<=\\w)",
                Flavor::PosixBre => "\\>",
                _ => return Err(EmitError::Unsupported("word-boundary")),
            },
        };
        self.out.push_str(text);
        Ok(())
    }

    fn set(&mut self, s: &Set) -> Result<(), EmitError> {
        // `\w`, `\s` e afins fora de colchetes.
        if s.escape
            && s.items.len() == 1
            && let SetItem::Class(k) = s.items[0]
            && matches!(self.flavor, Flavor::Rust | Flavor::Fancy | Flavor::Onig | Flavor::Resharp)
        {
            let perl = match (k, s.negated) {
                (PosixClass::Word, false) => "\\w",
                (PosixClass::Word, true) => "\\W",
                (PosixClass::Space, false) => "\\s",
                (_, _) => "\\S",
            };
            self.out.push_str(perl);
            return Ok(());
        }
        match self.flavor {
            Flavor::PosixEre | Flavor::PosixBre => self.posix_bracket(s),
            Flavor::Regast => self.regast_class(s),
            _ => self.perl_class(s),
        }
        Ok(())
    }

    fn perl_class(&mut self, s: &Set) {
        let f = self.flavor;
        let mut body = String::new();
        let esc = |c: char, body: &mut String| {
            if "[]\\-^&~".contains(c) {
                body.push('\\');
            }
            match c {
                '\n' => body.push_str("\\n"),
                _ => body.push(c),
            }
        };
        for item in &s.items {
            match *item {
                SetItem::Char(c) => esc(c, &mut body),
                SetItem::Range(a, b) => {
                    esc(a, &mut body);
                    body.push('-');
                    esc(b, &mut body);
                }
                SetItem::Class(k) => {
                    let k = if self.opts.icase && matches!(k, PosixClass::Upper | PosixClass::Lower) {
                        PosixClass::Alpha
                    } else {
                        k
                    };
                    body.push_str(&perl_class_body(k, f));
                }
            }
        }
        self.out.push('[');
        if s.negated {
            self.out.push('^');
        }
        self.out.push_str(&body);
        self.out.push(']');
    }

    fn posix_bracket(&mut self, s: &Set) {
        let mut body = String::new();
        let mut has_rbracket = false;
        let point = |c: char| -> String {
            if "]-^[".contains(c) { format!("[.{c}.]") } else { c.to_string() }
        };
        for item in &s.items {
            match *item {
                SetItem::Char(']') => has_rbracket = true,
                SetItem::Char(c) => body.push_str(&point(c)),
                SetItem::Range(a, b) => {
                    body.push_str(&point(a));
                    body.push('-');
                    body.push_str(&point(b));
                }
                SetItem::Class(PosixClass::Word) => body.push_str("_[:alnum:]"),
                SetItem::Class(k) => {
                    let k = if self.opts.icase && matches!(k, PosixClass::Upper | PosixClass::Lower) {
                        PosixClass::Alpha
                    } else {
                        k
                    };
                    body.push_str(&format!("[:{}:]", k.name()));
                }
            }
        }
        self.out.push('[');
        if s.negated {
            self.out.push('^');
        }
        if has_rbracket {
            self.out.push(']');
        }
        self.out.push_str(&body);
        self.out.push(']');
    }

    fn regast_class(&mut self, s: &Set) {
        let mut body = String::new();
        let esc = |c: char, body: &mut String| {
            if "\\]-^[".contains(c) {
                body.push('\\');
            }
            body.push(c);
        };
        for item in &s.items {
            match *item {
                SetItem::Char(c) => esc(c, &mut body),
                SetItem::Range(a, b) => {
                    esc(a, &mut body);
                    body.push('-');
                    esc(b, &mut body);
                }
                SetItem::Class(k) => {
                    let k = if self.opts.icase && matches!(k, PosixClass::Upper | PosixClass::Lower) {
                        PosixClass::Alpha
                    } else {
                        k
                    };
                    for &(a, b) in k.ascii_ranges() {
                        esc(a, &mut body);
                        if a != b {
                            body.push('-');
                            esc(b, &mut body);
                        }
                    }
                }
            }
        }
        self.out.push('[');
        if s.negated {
            self.out.push('^');
        }
        self.out.push_str(&body);
        self.out.push(']');
    }
}

/// `Repeat(Repeat(...(Assert(a))))`: a asserção e se ela precisa valer (todos os mínimos > 0).
fn repeated_assertion(n: &Node) -> Option<(Assertion, bool)> {
    match n {
        Node::Assert(a) => Some((*a, true)),
        Node::Repeat { inner, min, .. } => repeated_assertion(inner).map(|(a, required)| (a, required && *min > 0)),
        _ => None,
    }
}

fn has_group(n: &Node) -> bool {
    let mut found = false;
    crate::ast::walk(n, &mut |x, _| found |= matches!(x, Node::Group { .. }));
    found
}

/// Corpo de classe (sem colchetes) pra sabores com `\p{..}` (Rust, fancy, resharp) e Oniguruma.
fn perl_class_body(k: PosixClass, f: Flavor) -> String {
    if f == Flavor::Onig {
        return match k {
            PosixClass::Word => "\\w".into(),
            other => format!("[:{}:]", other.name()),
        };
    }
    match k {
        PosixClass::Alpha => "\\p{Alphabetic}".into(),
        PosixClass::Digit => "0-9".into(),
        PosixClass::Alnum => "\\p{Alphabetic}0-9".into(),
        PosixClass::Upper => "\\p{Uppercase}".into(),
        PosixClass::Lower => "\\p{Lowercase}".into(),
        PosixClass::Space => "\\s".into(),
        PosixClass::Blank => "\\t\\p{Zs}".into(),
        PosixClass::Punct => "!-/:-@\\[-`{-~\\p{P}\\p{S}".into(),
        PosixClass::Print => "\\P{Cc}".into(),
        PosixClass::Graph => "\\p{L}\\p{M}\\p{N}\\p{P}\\p{S}".into(),
        PosixClass::Cntrl => "\\p{Cc}".into(),
        PosixClass::Xdigit => "0-9A-Fa-f".into(),
        PosixClass::Word => "\\w".into(),
    }
}

/// Reescreve a árvore pra casar sem distinção de caixa (pra motores sem a flag).
pub fn fold_case(n: &Node) -> Node {
    match n {
        Node::Char(c) => {
            let variants = case_variants(*c);
            if variants.len() == 1 {
                Node::Char(*c)
            } else {
                Node::Set(Set { negated: false, items: variants.into_iter().map(SetItem::Char).collect(), escape: false })
            }
        }
        Node::Set(s) => {
            let mut items = Vec::new();
            for item in &s.items {
                match *item {
                    SetItem::Char(c) => items.extend(case_variants(c).into_iter().map(SetItem::Char)),
                    SetItem::Range(a, b) => {
                        items.push(SetItem::Range(a, b));
                        for (lo, hi, delta) in [('a', 'z', -32i32), ('A', 'Z', 32)] {
                            let s0 = a.max(lo);
                            let e0 = b.min(hi);
                            if s0 <= e0 {
                                let m = |c: char| char::from_u32((c as i32 + delta) as u32).unwrap_or(c);
                                items.push(SetItem::Range(m(s0), m(e0)));
                            }
                        }
                    }
                    SetItem::Class(PosixClass::Upper | PosixClass::Lower) => items.push(SetItem::Class(PosixClass::Alpha)),
                    SetItem::Class(k) => items.push(SetItem::Class(k)),
                }
            }
            Node::Set(Set { negated: s.negated, items, escape: s.escape })
        }
        Node::Group { index, inner } => Node::Group { index: *index, inner: Box::new(fold_case(inner)) },
        Node::Concat(v) => Node::Concat(v.iter().map(fold_case).collect()),
        Node::Alt(v) => Node::Alt(v.iter().map(fold_case).collect()),
        Node::Repeat { inner, min, max } => Node::Repeat { inner: Box::new(fold_case(inner)), min: *min, max: *max },
        other => other.clone(),
    }
}

fn case_variants(c: char) -> Vec<char> {
    let mut v = vec![c];
    for x in c.to_lowercase().chain(c.to_uppercase()) {
        if !v.contains(&x) && x.to_string().chars().count() == 1 {
            v.push(x);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Dialect, parse};

    fn em(p: &str, d: Dialect, f: Flavor) -> String {
        emit(&parse(p, d).unwrap(), f, EmitOptions::default()).unwrap().pattern
    }

    #[test]
    fn translates_bre_to_each_flavor() {
        let d = Dialect::GrepBre;
        assert_eq!(em("\\(ab\\)*c\\{2,3\\}", d, Flavor::Rust), "(ab)*c{2,3}");
        assert_eq!(em("a.b+", d, Flavor::Rust), "a\\.b\\+".replace("\\.", "."));
        assert_eq!(em("a\\|b", d, Flavor::PosixBre), "a\\|b");
        assert_eq!(em("x**", d, Flavor::Rust), "(?:x*)*");
        assert_eq!(em("[]a-]", d, Flavor::PosixEre), "[]a[.-.]]");
        assert_eq!(em("\\<foo\\>", d, Flavor::Rust), "\\b{start}foo\\b{end}");
        assert_eq!(em("_&~", d, Flavor::Resharp), "\\_\\&\\~");
    }

    #[test]
    fn group_map_tracks_synthetic_groups() {
        let re = parse("(a)**(b)\\2", Dialect::GrepEre).unwrap();
        let e = emit(&re, Flavor::PosixBre, EmitOptions::default()).unwrap();
        assert_eq!(e.group_map, vec![2, 3]);
        assert!(e.pattern.ends_with("\\3"));
        assert!(matches!(emit(&re, Flavor::Rust, EmitOptions::default()), Err(EmitError::Unsupported("backref"))));
    }
}
