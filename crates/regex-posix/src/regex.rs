//! A regex compilada: o caminho rápido do `regex-automata` (padrão sem referência) e o motor
//! próprio ([`crate::nfa`]) montados com a semântica do glibc.
//!
//! - Início da casada: o mais à esquerda (o `meta::Regex` leftmost-first acha o mesmo início que
//!   o POSIX).
//! - Fim: o mais longo a partir desse início, pelo DFA preguiçoso com `MatchKind::All` ancorado;
//!   se o DFA desiste (fronteira de palavra Unicode diante de texto não ASCII), pelo nosso NFA.
//! - Submatches: sempre pelo nosso NFA, com as regras do `set_regs` do glibc.
//! - Com referência: busca exaustiva no nosso NFA, com os candidatos a início filtrados por uma
//!   versão relaxada do padrão (referência trocada por "qualquer texto").

use std::sync::{Arc, Mutex};

use regex_automata::hybrid::dfa::{Cache as DfaCache, DFA};
use regex_automata::nfa::thompson::{self, WhichCaptures};
use regex_automata::util::look::LookMatcher;
use regex_automata::{Anchored, Input, MatchKind, meta};
use regex_syntax::hir::Hir;

use crate::ast::{self, Assertion, Node, Unit};
use crate::charclass::CaseMode;
use crate::error::{Error, ErrorCode, Warning};
use crate::hir::{HirOptions, to_hir};
use crate::nfa::{ExecFlags, Hook, Prog, ProgOptions, align};
use crate::parse::{View, parse, to_upper};
use crate::syntax::Syntax;

/// Limite de tamanho dos autômatos do `regex-automata`; acima disso fica só o nosso NFA.
const AUTOMATA_LIMIT: usize = 64 << 20;
const DFA_CACHE: usize = 8 << 20;

/// Como compilar uma regex. Os campos espelham o `re_pattern_buffer` do glibc.
#[derive(Clone)]
pub struct RegexBuilder {
    syntax: Syntax,
    newline_anchor: bool,
    separator: Option<u8>,
    no_sub: bool,
    dfa_view: bool,
    confusing_error: bool,
    whole_line: bool,
    hook: Option<Hook>,
}

impl std::fmt::Debug for RegexBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegexBuilder")
            .field("syntax", &self.syntax)
            .field("newline_anchor", &self.newline_anchor)
            .field("separator", &self.separator)
            .field("no_sub", &self.no_sub)
            .field("dfa_view", &self.dfa_view)
            .finish()
    }
}

impl RegexBuilder {
    pub fn new(syntax: Syntax) -> RegexBuilder {
        RegexBuilder {
            syntax,
            newline_anchor: false,
            separator: None,
            no_sub: syntax.contains(Syntax::NO_SUB),
            dfa_view: false,
            confusing_error: false,
            whole_line: false,
            hook: None,
        }
    }

    /// Sem distinção de caixa (o mesmo que ligar [`Syntax::ICASE`]).
    pub fn icase(mut self, yes: bool) -> RegexBuilder {
        self.syntax.set(Syntax::ICASE, yes);
        self
    }

    /// `newline_anchor` do glibc: `^` e `$` também casam depois e antes de `\n`. O
    /// `re_compile_pattern` liga isso por padrão; o sed liga com a flag `M`; o grep e o gawk desligam.
    pub fn newline_anchor(mut self, yes: bool) -> RegexBuilder {
        self.newline_anchor = yes;
        self
    }

    /// Modo linha (grep): o byte separador nunca casa (nem com `.`, `[^a]` ou `\s`) e `^`, `$`,
    /// `` \` `` e `\'` casam junto dele. Serve pra buscar num buffer com muitas linhas de uma vez.
    pub fn line_separator(mut self, sep: Option<u8>) -> RegexBuilder {
        self.separator = sep;
        self
    }

    /// Não calcula submatches (`RE_NO_SUB`/`REG_NOSUB`).
    pub fn no_sub(mut self, yes: bool) -> RegexBuilder {
        self.no_sub = yes;
        self
    }

    /// Semântica do `dfa.c` (a que o grep usa pra escolher linhas): no ERE, repetição depois de
    /// âncora se aplica à âncora. Os spans do `grep -o` e os grupos usam a do glibc (padrão).
    pub fn dfa_view(mut self, yes: bool) -> RegexBuilder {
        self.dfa_view = yes;
        self
    }

    /// `[:space:]` fora de colchetes vira [`Error::ConfusingBrackets`] (grep e sed).
    pub fn confusing_brackets_error(mut self, yes: bool) -> RegexBuilder {
        self.confusing_error = yes;
        self
    }

    /// A casada tem que cobrir o texto inteiro, de `^` a `$` (o `grep -x`).
    pub fn whole_line(mut self, yes: bool) -> RegexBuilder {
        self.whole_line = yes;
        self
    }

    /// Gancho chamado periodicamente nas buscas longas do motor próprio (os programas ligam no
    /// `sysabi::sys::checkpoint`). A biblioteca não faz E/S nem conhece o kernel.
    pub fn checkpoint(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> RegexBuilder {
        self.hook = Some(hook);
        self
    }

    pub fn syntax(&self) -> Syntax {
        self.syntax
    }

    fn view(&self) -> View {
        if self.dfa_view { View::Dfa } else { View::Glibc }
    }

    /// Só analisa (erros de sintaxe e diagnósticos do `dfa.c`), sem compilar.
    pub fn check(&self, pattern: &[u8]) -> Result<Diagnostics, Error> {
        let units = ast::decode(pattern);
        let glibc = parse(&units, self.syntax, View::Glibc).map_err(Error::Syntax)?;
        let dfa = parse(&units, self.syntax, View::Dfa).map_err(Error::Syntax)?;
        if self.confusing_error && dfa.confusing_brackets {
            return Err(Error::ConfusingBrackets);
        }
        Ok(Diagnostics {
            warnings: dfa.warnings,
            confusing_brackets: dfa.confusing_brackets,
            groups: glibc.nsub,
            has_backrefs: ast::has_backref(&glibc.root),
        })
    }

    pub fn build(&self, pattern: &[u8]) -> Result<Regex, Error> {
        self.build_many(&[pattern]).map_err(|(_, e)| e)
    }

    /// Vários padrões numa alternação (como `grep -e p1 -e p2`): cada um é analisado sozinho (as
    /// referências são locais a cada padrão) e os grupos são renumerados em sequência. O erro traz
    /// o índice do padrão.
    pub fn build_many(&self, patterns: &[&[u8]]) -> Result<Regex, (usize, Error)> {
        let mut roots = Vec::with_capacity(patterns.len());
        let mut nsub = 0;
        let mut warnings = Vec::new();
        let mut confusing = false;
        for (i, p) in patterns.iter().enumerate() {
            let units = ast::decode(p);
            let parsed = parse(&units, self.syntax, self.view()).map_err(|e| (i, Error::Syntax(e)))?;
            let diag = if self.dfa_view {
                parsed.clone()
            } else {
                parse(&units, self.syntax, View::Dfa).map_err(|e| (i, Error::Syntax(e)))?
            };
            if self.confusing_error && diag.confusing_brackets {
                return Err((i, Error::ConfusingBrackets));
            }
            confusing |= diag.confusing_brackets;
            warnings.extend(diag.warnings);
            roots.push(renumber(&parsed.root, nsub));
            nsub += parsed.nsub;
        }
        self.assemble(roots, nsub, warnings, confusing).map_err(|e| (0, e))
    }

    /// Cadeias fixas (o `grep -F`), em alternação.
    pub fn build_literals(&self, literals: &[&[u8]]) -> Result<Regex, Error> {
        let icase = self.syntax.contains(Syntax::ICASE) && !self.dfa_view;
        let roots = literals
            .iter()
            .map(|l| {
                let units: Vec<Node> = ast::decode(l)
                    .into_iter()
                    .map(|u| match u {
                        Unit::Char(c) if icase => Node::Lit(Unit::Char(to_upper(c))),
                        other => Node::Lit(other),
                    })
                    .collect();
                match units.len() {
                    0 => Node::Empty,
                    1 => units.into_iter().next().unwrap_or(Node::Empty),
                    _ => Node::Concat(units),
                }
            })
            .collect();
        self.assemble(roots, 0, Vec::new(), false)
    }

    fn assemble(&self, mut roots: Vec<Node>, nsub: usize, warnings: Vec<Warning>, confusing: bool) -> Result<Regex, Error> {
        let mut root = match roots.len() {
            0 => Node::Alt(Vec::new()),
            1 => roots.pop().unwrap_or(Node::Empty),
            _ => Node::Alt(roots),
        };
        let never = matches!(&root, Node::Alt(v) if v.is_empty());
        if self.whole_line && !never {
            root = Node::Concat(vec![Node::Assert(Assertion::LineStart), root, Node::Assert(Assertion::LineEnd)]);
        }
        let icase = self.syntax.contains(Syntax::ICASE);
        let case = match (icase, self.dfa_view) {
            (false, _) => CaseMode::Sensitive,
            (true, false) => CaseMode::Upper,
            (true, true) => CaseMode::Fold,
        };
        let dot_newline = self.syntax.contains(Syntax::DOT_NEWLINE);
        let dot_not_null = self.syntax.contains(Syntax::DOT_NOT_NULL);
        let prog = Prog::compile(
            &root,
            nsub,
            ProgOptions { case, dot_newline, dot_not_null, newline_anchor: self.newline_anchor, separator: self.separator },
            self.hook.clone(),
        )
        .map_err(Error::Syntax)?;
        let hir_opts = HirOptions { case, dot_newline, dot_not_null, newline_anchor: self.newline_anchor, separator: self.separator };
        let has_backref = ast::has_backref(&root);
        let (fast, relaxed) = if never {
            (None, None)
        } else if has_backref {
            (None, to_hir(&relax(&root), &hir_opts).and_then(|h| build_meta(&h, self.line_terminator())))
        } else {
            (to_hir(&root, &hir_opts).and_then(|h| Fast::build(&h, self.line_terminator())), None)
        };
        Ok(Regex {
            inner: Arc::new(Inner {
                prog,
                fast,
                relaxed,
                nsub,
                no_sub: self.no_sub,
                has_backref,
                never,
                warnings,
                confusing,
            }),
        })
    }

    fn line_terminator(&self) -> u8 {
        self.separator.unwrap_or(b'\n')
    }
}

/// Resultado de [`RegexBuilder::check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostics {
    /// Avisos do `dfa.c` (o grep imprime `grep: warning: ...`).
    pub warnings: Vec<Warning>,
    /// `[:alpha:]` fora de colchetes.
    pub confusing_brackets: bool,
    /// `re_nsub`.
    pub groups: usize,
    pub has_backrefs: bool,
}

/// Uma casada (offsets em bytes).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

impl Match {
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Submatches de uma casada: o índice 0 é a casada inteira; `None` é grupo que não participou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Captures {
    spans: Vec<Option<(usize, usize)>>,
}

impl Captures {
    pub fn get(&self, i: usize) -> Option<Match> {
        self.spans.get(i).copied().flatten().map(|(start, end)| Match { start, end })
    }

    /// Número de entradas (grupos + 1).
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn whole(&self) -> Match {
        self.get(0).unwrap_or(Match { start: 0, end: 0 })
    }

    pub fn iter(&self) -> impl Iterator<Item = Option<Match>> + '_ {
        (0..self.spans.len()).map(|i| self.get(i))
    }
}

/// Regex compilada. Barata de clonar; pode ser usada de várias threads.
#[derive(Clone)]
pub struct Regex {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Regex")
            .field("groups", &self.inner.nsub)
            .field("backrefs", &self.inner.has_backref)
            .field("fast", &self.inner.fast.is_some())
            .finish()
    }
}

struct Inner {
    prog: Prog,
    fast: Option<Fast>,
    relaxed: Option<meta::Regex>,
    nsub: usize,
    no_sub: bool,
    has_backref: bool,
    /// Nenhum padrão (o `grep -f /dev/null`): não casa nada.
    never: bool,
    warnings: Vec<Warning>,
    confusing: bool,
}

struct Fast {
    meta: meta::Regex,
    dfa: DFA,
    caches: Mutex<Vec<DfaCache>>,
}

fn build_meta(hir: &Hir, line_terminator: u8) -> Option<meta::Regex> {
    meta::Regex::builder()
        .configure(
            meta::Config::new()
                .nfa_size_limit(Some(AUTOMATA_LIMIT))
                .line_terminator(line_terminator)
                .utf8_empty(true)
                .which_captures(WhichCaptures::Implicit),
        )
        .build_from_hir(hir)
        .ok()
}

impl Fast {
    fn build(hir: &Hir, line_terminator: u8) -> Option<Fast> {
        let meta = build_meta(hir, line_terminator)?;
        let mut lm = LookMatcher::new();
        lm.set_line_terminator(line_terminator);
        let nfa = thompson::Compiler::new()
            .configure(
                thompson::Config::new()
                    .nfa_size_limit(Some(AUTOMATA_LIMIT))
                    .which_captures(WhichCaptures::None)
                    .look_matcher(lm),
            )
            .build_from_hir(hir)
            .ok()?;
        let dfa = DFA::builder()
            .configure(
                DFA::config()
                    .match_kind(MatchKind::All)
                    .unicode_word_boundary(true)
                    .cache_capacity(DFA_CACHE)
                    .skip_cache_capacity_check(true),
            )
            .build_from_nfa(nfa)
            .ok()?;
        Some(Fast { meta, dfa, caches: Mutex::new(Vec::new()) })
    }

    /// Fim mais longo de uma casada ancorada em `s`; `Err` se o DFA desistiu.
    fn longest(&self, hay: &[u8], s: usize) -> Result<Option<usize>, ()> {
        let mut cache = {
            let mut pool = self.caches.lock().unwrap_or_else(|e| e.into_inner());
            pool.pop().unwrap_or_else(|| self.dfa.create_cache())
        };
        let r = self.dfa.try_search_fwd(&mut cache, &Input::new(hay).range(s..).anchored(Anchored::Yes));
        self.caches.lock().unwrap_or_else(|e| e.into_inner()).push(cache);
        match r {
            Ok(h) => Ok(h.map(|h| h.offset())),
            Err(_) => Err(()),
        }
    }
}

impl Regex {
    /// Compila `pattern` com os bits `syntax` (como `re_compile_pattern` depois de
    /// `re_set_syntax`, mas com `newline_anchor` desligado; ver [`RegexBuilder::newline_anchor`]).
    pub fn new(pattern: &[u8], syntax: Syntax) -> Result<Regex, Error> {
        RegexBuilder::new(syntax).build(pattern)
    }

    pub fn builder(syntax: Syntax) -> RegexBuilder {
        RegexBuilder::new(syntax)
    }

    /// `re_nsub`: número de grupos.
    pub fn group_count(&self) -> usize {
        self.inner.nsub
    }

    pub fn has_backrefs(&self) -> bool {
        self.inner.has_backref
    }

    /// Avisos do `dfa.c` na ordem em que o grep os imprime.
    pub fn warnings(&self) -> &[Warning] {
        &self.inner.warnings
    }

    pub fn confusing_brackets(&self) -> bool {
        self.inner.confusing
    }

    pub fn is_match(&self, hay: &[u8]) -> bool {
        self.is_match_at(hay, 0)
    }

    /// Existe casada começando em `start` ou depois?
    pub fn is_match_at(&self, hay: &[u8], start: usize) -> bool {
        let i = &self.inner;
        if i.never || start > hay.len() {
            return false;
        }
        if let Some(f) = &i.fast {
            return f.meta.is_match(Input::new(hay).range(start..));
        }
        self.find_at_with(hay, start, ExecFlags::default()).is_some()
    }

    pub fn find(&self, hay: &[u8]) -> Option<Match> {
        self.find_at(hay, 0)
    }

    /// Casada leftmost-longest que começa em `start` ou depois. O texto antes de `start` conta
    /// como contexto (`^`, `\<`), como no `re_search`.
    pub fn find_at(&self, hay: &[u8], start: usize) -> Option<Match> {
        self.find_at_with(hay, start, ExecFlags::default())
    }

    pub fn find_at_with(&self, hay: &[u8], start: usize, flags: ExecFlags) -> Option<Match> {
        self.span(hay, start, flags).map(|(start, end)| Match { start, end })
    }

    fn span(&self, hay: &[u8], start: usize, flags: ExecFlags) -> Option<(usize, usize)> {
        let i = &self.inner;
        if i.never || start > hay.len() {
            return None;
        }
        // O glibc só começa casada no primeiro byte de um caractere.
        let start = align(hay, start);
        let plain = flags == ExecFlags::default();
        if plain && let Some(f) = &i.fast {
            let m = f.meta.search(&Input::new(hay).range(start..))?;
            let s = m.start();
            let e = match f.longest(hay, s) {
                Ok(Some(e)) => e.max(m.end()),
                Ok(None) => m.end(),
                Err(()) => i.prog.longest_end(hay, s, flags).unwrap_or(m.end()).max(m.end()),
            };
            return Some((s, e));
        }
        if i.has_backref {
            let candidates = |p: usize| -> Option<usize> {
                match &i.relaxed {
                    Some(r) if p <= hay.len() => r.search(&Input::new(hay).range(p..)).map(|m| m.start()),
                    Some(_) => None,
                    None => Some(p),
                }
            };
            return i.prog.find_backref(hay, start, flags, Some(&candidates));
        }
        i.prog.find(hay, start, flags)
    }

    /// `re_match`: fim da casada mais longa que começa exatamente em `pos`.
    pub fn longest_at(&self, hay: &[u8], pos: usize, flags: ExecFlags) -> Option<usize> {
        let i = &self.inner;
        if i.never || pos > hay.len() {
            return None;
        }
        if flags == ExecFlags::default()
            && let Some(f) = &i.fast
        {
            match f.longest(hay, pos) {
                Ok(e) => return e,
                Err(()) => return i.prog.longest_end(hay, pos, flags),
            }
        }
        i.prog.longest_end(hay, pos, flags)
    }

    pub fn captures(&self, hay: &[u8]) -> Option<Captures> {
        self.captures_at(hay, 0)
    }

    pub fn captures_at(&self, hay: &[u8], start: usize) -> Option<Captures> {
        self.captures_at_with(hay, start, ExecFlags::default())
    }

    /// Casada com submatches. Com `no_sub`, só o grupo 0.
    pub fn captures_at_with(&self, hay: &[u8], start: usize, flags: ExecFlags) -> Option<Captures> {
        let (s, e) = self.span(hay, start, flags)?;
        Some(self.groups_of(hay, s, e, flags))
    }

    /// Submatches de uma casada já encontrada (de `s` a `e`).
    pub fn groups_of(&self, hay: &[u8], s: usize, e: usize, flags: ExecFlags) -> Captures {
        let i = &self.inner;
        let n = i.nsub + 1;
        let only_whole = || {
            let mut spans = vec![None; n];
            spans[0] = Some((s, e));
            Captures { spans }
        };
        if i.nsub == 0 || i.no_sub {
            return only_whole();
        }
        let regs = i.prog.groups(hay, s, e, flags).or_else(|| {
            // O caminho rápido e o nosso NFA discordaram do fim; refaz tudo no NFA.
            let (s2, e2) = i.prog.find(hay, s, flags)?;
            (s2 == s).then(|| i.prog.groups(hay, s2, e2, flags)).flatten()
        });
        match regs {
            Some(regs) => Captures {
                spans: regs
                    .into_iter()
                    .map(|(a, b)| (a >= 0 && b >= 0).then_some((a as usize, b as usize)))
                    .collect(),
            },
            None => only_whole(),
        }
    }

    /// Casadas não vazias na ordem do `grep -o`: depois de uma casada a busca recomeça no fim
    /// dela; casada vazia avança um byte (o glibc pula pro próximo caractere); nada começa no fim
    /// do texto.
    pub fn find_iter<'r, 'h>(&'r self, hay: &'h [u8]) -> Matches<'r, 'h> {
        Matches { re: self, hay, cur: 0 }
    }
}

/// Iterador de [`Regex::find_iter`].
pub struct Matches<'r, 'h> {
    re: &'r Regex,
    hay: &'h [u8],
    cur: usize,
}

impl Iterator for Matches<'_, '_> {
    type Item = Match;

    fn next(&mut self) -> Option<Match> {
        while self.cur < self.hay.len() {
            let m = self.re.find_at(self.hay, self.cur)?;
            if m.start >= self.hay.len() {
                self.cur = self.hay.len();
                return None;
            }
            if m.is_empty() {
                self.cur = align(self.hay, m.start + 1);
                continue;
            }
            self.cur = m.end;
            return Some(m);
        }
        None
    }
}

/// Soma `offset` aos índices de grupo e de referência.
fn renumber(n: &Node, offset: usize) -> Node {
    if offset == 0 {
        return n.clone();
    }
    match n {
        Node::Group { index, inner } => Node::Group { index: index + offset, inner: Box::new(renumber(inner, offset)) },
        Node::Backref(k) => Node::Backref(k + offset),
        Node::Concat(v) => Node::Concat(v.iter().map(|x| renumber(x, offset)).collect()),
        Node::Alt(v) => Node::Alt(v.iter().map(|x| renumber(x, offset)).collect()),
        Node::Repeat { inner, min, max } => Node::Repeat { inner: Box::new(renumber(inner, offset)), min: *min, max: *max },
        other => other.clone(),
    }
}

/// Troca cada referência por "qualquer texto": um superconjunto, pra filtrar inícios.
fn relax(n: &Node) -> Node {
    match n {
        Node::Backref(_) => {
            use crate::ast::{Set, SetItem};
            // Qualquer caractere, ou qualquer byte inválido (o grupo pode ter capturado um).
            let any_char = Node::Set(Set { negated: true, items: Vec::new() });
            let any_byte = Node::Set(Set { negated: false, items: vec![SetItem::ByteRange(0x80, 0xff)] });
            Node::Repeat { inner: Box::new(Node::Alt(vec![any_char, any_byte])), min: 0, max: None }
        }
        Node::Group { index, inner } => Node::Group { index: *index, inner: Box::new(relax(inner)) },
        Node::Concat(v) => Node::Concat(v.iter().map(relax).collect()),
        Node::Alt(v) => Node::Alt(v.iter().map(relax).collect()),
        Node::Repeat { inner, min, max } => Node::Repeat { inner: Box::new(relax(inner)), min: *min, max: *max },
        other => other.clone(),
    }
}

impl From<ErrorCode> for Error {
    fn from(e: ErrorCode) -> Error {
        Error::Syntax(e)
    }
}
