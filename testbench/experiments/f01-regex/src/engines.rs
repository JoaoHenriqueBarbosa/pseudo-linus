//! Adaptadores: cada motor candidato atrás da mesma interface.
//!
//! O contrato que os emuladores de grep/sed/gawk usam é o do GNU: primeira casada (com grupos) a
//! partir de uma posição da linha, enxergando o contexto à esquerda. Motores que não têm busca a
//! partir de posição recebem uma variante do padrão em que `^` nunca casa e buscam no sufixo, ou,
//! quando têm asserções de palavra, usam a iteração nativa deles.

use std::sync::Mutex;

use crate::ast::Regex;
use crate::emit::{EmitError, EmitOptions, Emitted, Flavor, emit};
use crate::parse::Dialect;

/// Grupo 0 é a casada inteira; os demais seguem a numeração original do padrão GNU.
pub type Caps = Vec<Option<(usize, usize)>>;

#[derive(Clone, Debug)]
pub enum CompileError {
    /// O motor não tem a construção (ex.: backref).
    Unsupported(String),
    /// O motor recusou o padrão traduzido.
    Engine(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Unsupported(s) => write!(f, "unsupported: {s}"),
            CompileError::Engine(s) => write!(f, "engine: {s}"),
        }
    }
}

impl From<EmitError> for CompileError {
    fn from(e: EmitError) -> Self {
        CompileError::Unsupported(e.to_string())
    }
}

/// Uma regex já compilada num motor.
pub trait Matcher: Send {
    /// Primeira casada que começa em `start` ou depois, com o contexto da linha inteira.
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String>;

    /// `false` quando `captures_at` só vale com `start == 0`; aí os emuladores usam [`Matcher::native_all`].
    fn supports_start(&self) -> bool {
        true
    }

    /// Todas as casadas sucessivas, pela iteração do próprio motor.
    fn native_all(&self, _hay: &[u8]) -> Result<Vec<(usize, usize)>, String> {
        Err("sem iteração nativa".into())
    }

    /// `false` quando os grupos que o motor devolve não têm significado (só o span vale).
    fn real_groups(&self) -> bool {
        true
    }
}

pub trait Engine: Send + Sync {
    /// Nome estável nos resultados.
    fn name(&self) -> &'static str;
    /// Crate e versão (pro depscan e pro JSON).
    fn crate_name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    /// Semântica declarada (pro relatório).
    fn semantics(&self) -> &'static str;
    fn compile(&self, re: &Regex, src: &str, dialect: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError>;

    /// Compilação quando só o span importa (grep, sed g); motores com grupos caros ou
    /// experimentais podem desligá-los aqui.
    fn compile_spans(&self, re: &Regex, src: &str, dialect: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        self.compile(re, src, dialect, icase)
    }
}

fn utf8(hay: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(hay).map_err(|_| "entrada não é UTF-8".to_string())
}

fn map_caps(raw: &[Option<(usize, usize)>], e: &Emitted) -> Caps {
    let mut out = Vec::with_capacity(e.group_map.len() + 1);
    out.push(raw.first().copied().flatten());
    for &g in &e.group_map {
        out.push(raw.get(g).copied().flatten());
    }
    out
}

/// Próxima fronteira de caractere UTF-8 depois de `i`.
pub fn next_boundary(hay: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    while j < hay.len() && (hay[j] & 0xC0) == 0x80 {
        j += 1;
    }
    j
}

// ---------------------------------------------------------------- regex

pub struct RustRegex;

struct RustRegexMatcher {
    re: regex::bytes::Regex,
    emitted: Emitted,
}

impl Engine for RustRegex {
    fn name(&self) -> &'static str {
        "regex"
    }
    fn crate_name(&self) -> &'static str {
        "regex"
    }
    fn version(&self) -> &'static str {
        "1.13.1"
    }
    fn semantics(&self) -> &'static str {
        "leftmost-first, sem backref"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        let emitted = emit(re, Flavor::Rust, EmitOptions { icase, notbol: false })?;
        let compiled = regex::bytes::RegexBuilder::new(&emitted.pattern)
            .size_limit(64 << 20)
            .build()
            .map_err(|e| CompileError::Engine(e.to_string()))?;
        Ok(Box::new(RustRegexMatcher { re: compiled, emitted }))
    }
}

impl Matcher for RustRegexMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        Ok(self.re.captures_at(hay, start).map(|c| {
            let raw: Vec<Option<(usize, usize)>> = c.iter().map(|m| m.map(|m| (m.start(), m.end()))).collect();
            map_caps(&raw, &self.emitted)
        }))
    }
}

// ---------------------------------------------------------------- regex-automata (montagem leftmost-longest)

/// Montagem nossa sobre o `regex-automata`: o início vem da busca leftmost-first (que acha o mesmo
/// início que o POSIX), e o fim mais longo vem de um DFA preguiçoso com `MatchKind::All` ancorado
/// nesse início. Grupos: leftmost-first dentro do span (não é a regra de subexpressão do POSIX).
pub struct AutomataLongest;

struct AutomataMatcher {
    meta: regex_automata::meta::Regex,
    dfa: regex_automata::hybrid::dfa::DFA,
    cache: Mutex<regex_automata::hybrid::dfa::Cache>,
    emitted: Emitted,
}

impl Engine for AutomataLongest {
    fn name(&self) -> &'static str {
        "regex-automata-longest"
    }
    fn crate_name(&self) -> &'static str {
        "regex-automata"
    }
    fn version(&self) -> &'static str {
        "0.4.18"
    }
    fn semantics(&self) -> &'static str {
        "montagem: início leftmost + fim mais longo por DFA MatchKind::All; sem backref"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        use regex_automata::{MatchKind, hybrid::dfa::DFA, meta, nfa::thompson, util::syntax};
        let emitted = emit(re, Flavor::Rust, EmitOptions { icase, notbol: false })?;
        let meta = meta::Regex::builder()
            .configure(meta::Config::new().nfa_size_limit(Some(64 << 20)))
            .build(&emitted.pattern)
            .map_err(|e| CompileError::Engine(e.to_string()))?;
        let dfa = DFA::builder()
            .configure(
                DFA::config()
                    .match_kind(MatchKind::All)
                    .unicode_word_boundary(true)
                    .cache_capacity(16 << 20),
            )
            .syntax(syntax::Config::new())
            .thompson(thompson::Config::new().nfa_size_limit(Some(64 << 20)))
            .build(&emitted.pattern)
            .map_err(|e| CompileError::Engine(e.to_string()))?;
        let cache = Mutex::new(dfa.create_cache());
        Ok(Box::new(AutomataMatcher { meta, dfa, cache, emitted }))
    }
}

impl Matcher for AutomataMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        use regex_automata::{Anchored, Input};
        let Some(m) = self.meta.search(&Input::new(hay).range(start..)) else {
            return Ok(None);
        };
        let s = m.start();
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let half = self
            .dfa
            .try_search_fwd(&mut cache, &Input::new(hay).range(s..).anchored(Anchored::Yes))
            .map_err(|e| e.to_string())?;
        let e = half.map(|h| h.offset()).unwrap_or(m.end()).max(m.end());
        drop(cache);
        let mut caps = self.meta.create_captures();
        self.meta.search_captures(&Input::new(hay).range(s..e).anchored(Anchored::Yes), &mut caps);
        let mut raw: Vec<Option<(usize, usize)>> =
            (0..caps.group_len()).map(|g| caps.get_group(g).map(|sp| (sp.start, sp.end))).collect();
        if raw.is_empty() {
            raw.push(None);
        }
        raw[0] = Some((s, e));
        Ok(Some(map_caps(&raw, &self.emitted)))
    }

    fn real_groups(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- fancy-regex

pub struct Fancy;

struct FancyMatcher {
    re: fancy_regex::Regex,
    emitted: Emitted,
}

impl Engine for Fancy {
    fn name(&self) -> &'static str {
        "fancy-regex"
    }
    fn crate_name(&self) -> &'static str {
        "fancy-regex"
    }
    fn version(&self) -> &'static str {
        "0.19.2"
    }
    fn semantics(&self) -> &'static str {
        "leftmost-first com backtracking, backref"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        let emitted = emit(re, Flavor::Fancy, EmitOptions { icase, notbol: false })?;
        let compiled = fancy_regex::RegexBuilder::new(&emitted.pattern)
            .backtrack_limit(1_000_000)
            .build()
            .map_err(|e| CompileError::Engine(e.to_string()))?;
        Ok(Box::new(FancyMatcher { re: compiled, emitted }))
    }
}

impl Matcher for FancyMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        let text = utf8(hay)?;
        let found = self.re.captures_from_pos(text, start).map_err(|e| e.to_string())?;
        Ok(found.map(|c| {
            let raw: Vec<Option<(usize, usize)>> =
                (0..c.len()).map(|i| c.get(i).map(|m| (m.start(), m.end()))).collect();
            map_caps(&raw, &self.emitted)
        }))
    }
}

// ---------------------------------------------------------------- revera

pub struct Revera;

struct ReveraMatcher {
    full: revera::Regex,
    notbol: revera::Regex,
    emitted: Emitted,
    emitted_notbol: Emitted,
}

impl Engine for Revera {
    fn name(&self) -> &'static str {
        "revera"
    }
    fn crate_name(&self) -> &'static str {
        "revera"
    }
    fn version(&self) -> &'static str {
        "0.2.1"
    }
    fn semantics(&self) -> &'static str {
        "ERE POSIX.1-2024, leftmost-longest, subexpressões POSIX; sem BRE, sem backref, sem extensões GNU"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        let emitted = emit(re, Flavor::PosixEre, EmitOptions { icase, notbol: false })?;
        let emitted_notbol = emit(re, Flavor::PosixEre, EmitOptions { icase, notbol: true })?;
        let build = |p: &str| {
            revera::RegexBuilder::new(p)
                .case_insensitive(icase)
                .build()
                .map_err(|e| CompileError::Engine(e.to_string()))
        };
        Ok(Box::new(ReveraMatcher {
            full: build(&emitted.pattern)?,
            notbol: build(&emitted_notbol.pattern)?,
            emitted,
            emitted_notbol,
        }))
    }
}

impl Matcher for ReveraMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        let text = utf8(hay)?;
        let (re, em, base) = if start == 0 {
            (&self.full, &self.emitted, 0)
        } else {
            (&self.notbol, &self.emitted_notbol, start)
        };
        let sub = text.get(base..).ok_or("início fora de fronteira UTF-8")?;
        let found = re.captures(sub).map_err(|e| e.to_string())?;
        Ok(found.map(|c| {
            let raw: Vec<Option<(usize, usize)>> =
                c.iter().map(|m| m.map(|m| (m.start() + base, m.end() + base))).collect();
            map_caps(&raw, em)
        }))
    }
}

// ---------------------------------------------------------------- posix-regex

pub struct PosixRegexCrate;

struct PosixRegexMatcher {
    re: posix_regex::PosixRegex<'static>,
    emitted: Emitted,
}

impl Engine for PosixRegexCrate {
    fn name(&self) -> &'static str {
        "posix-regex"
    }
    fn crate_name(&self) -> &'static str {
        "posix-regex"
    }
    fn version(&self) -> &'static str {
        "0.1.4"
    }
    fn semantics(&self) -> &'static str {
        "BRE/ERE do relibc, só ASCII, backref"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        let emitted = emit(re, Flavor::PosixBre, EmitOptions { icase, notbol: false })?;
        let compiled = posix_regex::PosixRegexBuilder::new(emitted.pattern.as_bytes())
            .with_default_classes()
            .compile()
            .map_err(|e| CompileError::Engine(format!("{e:?}")))?
            .case_insensitive(icase);
        Ok(Box::new(PosixRegexMatcher { re: compiled, emitted }))
    }
}

impl Matcher for PosixRegexMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        if start != 0 {
            return Err("posix-regex não busca a partir de posição".into());
        }
        let found = self.re.matches(hay, Some(1));
        Ok(found.into_iter().next().map(|groups| map_caps(&groups, &self.emitted)))
    }

    fn supports_start(&self) -> bool {
        false
    }

    fn native_all(&self, hay: &[u8]) -> Result<Vec<(usize, usize)>, String> {
        Ok(self.re.matches(hay, None).into_iter().filter_map(|g| g.first().copied().flatten()).collect())
    }
}

// ---------------------------------------------------------------- regast

pub struct RegastCrate;

struct RegastMatcher {
    full: regast::Regast,
    notbol: regast::Regast,
    emitted: Emitted,
    emitted_notbol: Emitted,
}

impl Engine for RegastCrate {
    fn name(&self) -> &'static str {
        "regast"
    }
    fn crate_name(&self) -> &'static str {
        "regast"
    }
    fn version(&self) -> &'static str {
        "0.1.0"
    }
    fn semantics(&self) -> &'static str {
        "leftmost-longest com desambiguação POSIX (derivadas); sem backref, sem classes POSIX, sem flags"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        let emitted = emit(re, Flavor::Regast, EmitOptions { icase, notbol: false })?;
        let emitted_notbol = emit(re, Flavor::Regast, EmitOptions { icase, notbol: true })?;
        let build = |p: &str| {
            regast::Regast::builder(p).posix().build().map_err(|e| CompileError::Engine(e.to_string()))
        };
        Ok(Box::new(RegastMatcher {
            full: build(&emitted.pattern)?,
            notbol: build(&emitted_notbol.pattern)?,
            emitted,
            emitted_notbol,
        }))
    }
}

impl Matcher for RegastMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        let text = utf8(hay)?;
        let (re, em, base) = if start == 0 {
            (&self.full, &self.emitted, 0)
        } else {
            (&self.notbol, &self.emitted_notbol, start)
        };
        let sub = text.get(base..).ok_or("início fora de fronteira UTF-8")?;
        let found = re.find_parse(sub).map_err(|e| e.to_string())?;
        Ok(found.map(|tree| {
            let raw: Vec<Option<(usize, usize)>> =
                tree.captures_compat()
                    .into_iter()
                    .map(|s| s.map(|s| (s.start as usize + base, s.end as usize + base)))
                    .collect();
            map_caps(&raw, em)
        }))
    }
}

// ---------------------------------------------------------------- rusty_expressions

pub struct RustyExpressions;

struct RustyMatcher {
    first: rusty_expressions::Regex,
    longest: rusty_expressions::Regex,
    emitted: Emitted,
}

impl Engine for RustyExpressions {
    fn name(&self) -> &'static str {
        "rusty_expressions-longest"
    }
    fn crate_name(&self) -> &'static str {
        "rusty_expressions"
    }
    fn version(&self) -> &'static str {
        "0.2.2"
    }
    fn semantics(&self) -> &'static str {
        "Oniguruma em Rust: início pela busca normal, fim pelo FIND_LONGEST ancorado; backref"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        use rusty_expressions::{Options, Syntax};
        let emitted = emit(re, Flavor::Onig, EmitOptions { icase, notbol: false })?;
        let mut opts = Options::NONE;
        if icase {
            opts = opts.union(Options::IGNORECASE);
        }
        let build = |o: Options| {
            rusty_expressions::Regex::new_str(&emitted.pattern, o, Syntax::ONIGURUMA)
                .map_err(|e| CompileError::Engine(format!("{e:?}")))
        };
        let first = build(opts)?;
        let longest = build(opts.union(Options::FIND_LONGEST))?;
        Ok(Box::new(RustyMatcher { first, longest, emitted }))
    }
}

impl Matcher for RustyMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        let param = rusty_expressions::MatchParam::default();
        let Some(first) = self
            .first
            .search_range_param(hay, start, hay.len(), &param)
            .map_err(|e| format!("{e:?}"))?
        else {
            return Ok(None);
        };
        let s = first.range().start;
        let region = self.longest.find_at(hay, s).map_err(|e| format!("{e:?}"))?.unwrap_or(first);
        let n = self.emitted.engine_groups + 1;
        let raw: Vec<Option<(usize, usize)>> = (0..n).map(|i| region.get(i).map(|r| (r.start, r.end))).collect();
        Ok(Some(map_caps(&raw, &self.emitted)))
    }
}

// ---------------------------------------------------------------- ferroni

pub struct Ferroni {
    /// `true`: usa a sintaxe nativa do Oniguruma pra GNU (Grep/PosixExtended) sem o nosso tradutor.
    pub native_syntax: bool,
}

struct FerroniMatcher {
    first: ferroni::regint::RegexType,
    longest: ferroni::regint::RegexType,
    /// `None` na variante de sintaxe nativa (grupos na numeração do próprio padrão).
    emitted: Option<Emitted>,
    groups: usize,
}

impl Engine for Ferroni {
    fn name(&self) -> &'static str {
        if self.native_syntax { "ferroni-native-syntax" } else { "ferroni-longest" }
    }
    fn crate_name(&self) -> &'static str {
        "ferroni"
    }
    fn version(&self) -> &'static str {
        "1.8.1"
    }
    fn semantics(&self) -> &'static str {
        if self.native_syntax {
            "Oniguruma em Rust com ONIG_SYNTAX_GREP/POSIX_EXTENDED direto no padrão GNU, FIND_LONGEST ancorado"
        } else {
            "Oniguruma em Rust: início pela busca normal, fim pelo FIND_LONGEST ancorado; backref"
        }
    }
    fn compile(&self, re: &Regex, src: &str, dialect: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        use ferroni::encodings::utf8::ONIG_ENCODING_UTF8;
        use ferroni::oniguruma::{ONIG_OPTION_FIND_LONGEST, ONIG_OPTION_IGNORECASE, ONIG_OPTION_NONE};
        use ferroni::regsyntax::{OnigSyntaxGrep, OnigSyntaxOniguruma, OnigSyntaxPosixExtended};
        let mut opts = ONIG_OPTION_NONE;
        if icase {
            opts |= ONIG_OPTION_IGNORECASE;
        }
        let (pattern, syntax, emitted, groups) = if self.native_syntax {
            let syn = if dialect.ere() { &OnigSyntaxPosixExtended } else { &OnigSyntaxGrep };
            let text: String = if dialect.sed() { crate::parse::sed_normalize(src).into_iter().collect() } else { src.to_string() };
            (text, syn, None, re.groups)
        } else {
            let e = emit(re, Flavor::Onig, EmitOptions { icase, notbol: false })?;
            (e.pattern.clone(), &OnigSyntaxOniguruma, Some(e), re.groups)
        };
        let build = |o| {
            ferroni::regcomp::onig_new(pattern.as_bytes(), o, &ONIG_ENCODING_UTF8, syntax)
                .map_err(|e| CompileError::Engine(format!("{e:?}")))
        };
        let first = build(opts)?;
        let longest = build(opts | ONIG_OPTION_FIND_LONGEST)?;
        Ok(Box::new(FerroniMatcher { first, longest, emitted, groups }))
    }
}

impl Matcher for FerroniMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        use ferroni::oniguruma::{ONIG_MISMATCH, ONIG_OPTION_NONE, OnigRegion};
        use ferroni::regexec::{onig_match, onig_search};
        let (r, _) = onig_search(&self.first, hay, hay.len(), start, hay.len(), None, ONIG_OPTION_NONE);
        if r == ONIG_MISMATCH {
            return Ok(None);
        }
        if r < 0 {
            return Err(format!("onig_search: {r}"));
        }
        let s = r as usize;
        let (r2, region) = onig_match(&self.longest, hay, hay.len(), s, Some(OnigRegion::new()), ONIG_OPTION_NONE);
        if r2 < 0 {
            return Err(format!("onig_match: {r2}"));
        }
        let region = region.ok_or("sem região")?;
        let raw: Vec<Option<(usize, usize)>> = (0..region.num_regs.max(0) as usize)
            .map(|i| {
                let (b, e) = (region.beg[i], region.end[i]);
                (b >= 0 && e >= 0).then_some((b as usize, e as usize))
            })
            .collect();
        Ok(Some(match &self.emitted {
            Some(em) => map_caps(&raw, em),
            None => (0..=self.groups).map(|i| raw.get(i).copied().flatten()).collect(),
        }))
    }
}

// ---------------------------------------------------------------- resharp

pub struct Resharp;

struct ResharpMatcher {
    re: resharp::Regex,
    emitted: Emitted,
}

impl Engine for Resharp {
    fn name(&self) -> &'static str {
        "resharp"
    }
    fn crate_name(&self) -> &'static str {
        "resharp"
    }
    fn version(&self) -> &'static str {
        "0.7.5"
    }
    fn semantics(&self) -> &'static str {
        "derivadas simbólicas, leftmost-longest, lookaround; sem backref; grupos experimentais"
    }
    fn compile(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        resharp_compile(re, icase, true)
    }

    fn compile_spans(&self, re: &Regex, _src: &str, _d: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        resharp_compile(re, icase, false)
    }
}

/// Os grupos do resharp são experimentais (feature `experimental_capture_groups`) e recusam padrões
/// que o casamento sem grupos aceita; por isso só são ligados nas sondas de submatch.
fn resharp_compile(re: &Regex, icase: bool, groups: bool) -> Result<Box<dyn Matcher>, CompileError> {
    let mut emitted = emit(re, Flavor::Resharp, EmitOptions { icase, notbol: false })?;
    let opts = resharp::RegexOptions::default()
        .multiline(false)
        .case_insensitive(icase)
        .implicit_captures(groups && !emitted.group_map.is_empty());
    let compiled =
        resharp::Regex::with_options(&emitted.pattern, opts).map_err(|e| CompileError::Engine(format!("{e:?}")))?;
    if !groups {
        emitted.group_map.clear();
    }
    Ok(Box::new(ResharpMatcher { re: compiled, emitted }))
}

impl Matcher for ResharpMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        if start != 0 {
            return Err("resharp não busca a partir de posição".into());
        }
        if self.emitted.group_map.is_empty() {
            let all = self.re.find_all(hay).map_err(|e| format!("{e:?}"))?;
            return Ok(all.first().map(|m| vec![Some((m.start, m.end))]));
        }
        let caps = self.re.captures_all(hay).map_err(|e| format!("{e:?}"))?;
        Ok(caps.first().map(|c| map_caps(c.spans(), &self.emitted)))
    }

    fn supports_start(&self) -> bool {
        false
    }

    fn native_all(&self, hay: &[u8]) -> Result<Vec<(usize, usize)>, String> {
        Ok(self.re.find_all(hay).map_err(|e| format!("{e:?}"))?.into_iter().map(|m| (m.start, m.end)).collect())
    }
}

// ---------------------------------------------------------------- combinação (melhor do F01)

/// A combinação recomendada pelo F01: `regex-automata` (DFA, tempo linear, leftmost-longest por
/// montagem) quando o padrão não tem backref, `ferroni` (backtracking com FIND_LONGEST) quando tem.
/// Não entra em [`all_engines`]: o F01 mede a combinação por roteamento; o F02 usa esta como motor.
pub struct AutomataFerroni;

impl Engine for AutomataFerroni {
    fn name(&self) -> &'static str {
        "regex-automata+ferroni"
    }
    fn crate_name(&self) -> &'static str {
        "regex-automata"
    }
    fn version(&self) -> &'static str {
        "0.4.18+1.8.1"
    }
    fn semantics(&self) -> &'static str {
        "regex-automata (DFA) sem backref, ferroni com backref; leftmost-longest por montagem"
    }
    fn compile(&self, re: &Regex, src: &str, dialect: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        if crate::ast::uses_backref(re) {
            Ferroni { native_syntax: false }.compile(re, src, dialect, icase)
        } else {
            AutomataLongest.compile(re, src, dialect, icase)
        }
    }
}

// ---------------------------------------------------------------- red-sed (motor do red)

/// O motor de regex do `red-sed` recebe o padrão GNU direto (sem o nosso tradutor). Em locale
/// multibyte (C.UTF-8) ele sempre usa o NFA com backtracking, que devolve os grupos.
pub struct RedSed;

struct RedSedMatcher {
    m: red::regex::Matcher,
    groups: usize,
}

impl Engine for RedSed {
    fn name(&self) -> &'static str {
        "red-sed-regex"
    }
    fn crate_name(&self) -> &'static str {
        "red-sed"
    }
    fn version(&self) -> &'static str {
        "1.0.2"
    }
    fn semantics(&self) -> &'static str {
        "motor próprio do red (sed em Rust): parser BRE/ERE do GNU, DFA/NFA com backtracking, backref; sem tradutor"
    }
    fn compile(&self, re: &Regex, src: &str, dialect: Dialect, icase: bool) -> Result<Box<dyn Matcher>, CompileError> {
        // O red lê o locale do ambiente (setlocale) no início do main; sem isso usa "C" e cai no
        // DFA, que não devolve casada pela API com grupos.
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(red::mbcs::initialize);
        let m = red::regex::Matcher::compile_with_flags(src, dialect.ere(), icase, false, false)
            .map_err(|e| CompileError::Engine(e.to_string()))?;
        Ok(Box::new(RedSedMatcher { m, groups: re.groups }))
    }
}

impl Matcher for RedSedMatcher {
    fn captures_at(&self, hay: &[u8], start: usize) -> Result<Option<Caps>, String> {
        if start > hay.len() {
            return Ok(None);
        }
        let found = if start == hay.len() {
            // A API devolve None em início == fim; o vazio no fim é testado pelo texto inteiro.
            self.m
                .find_with_captures_bytes(hay)
                .filter(|(s, _, _)| *s == hay.len())
        } else {
            self.m.find_with_captures_bytes_from(hay, start)
        };
        Ok(found.map(|(s, e, caps)| {
            let mut out: Caps = vec![Some((s, e))];
            for g in 1..=self.groups {
                out.push(caps.get(&g).map(|c| (c.start, c.end)));
            }
            out
        }))
    }
}

/// Todos os candidatos do F01, na ordem do relatório.
pub fn all_engines() -> Vec<Box<dyn Engine>> {
    vec![
        Box::new(RustRegex),
        Box::new(AutomataLongest),
        Box::new(Fancy),
        Box::new(Revera),
        Box::new(PosixRegexCrate),
        Box::new(RegastCrate),
        Box::new(RustyExpressions),
        Box::new(Ferroni { native_syntax: false }),
        Box::new(Ferroni { native_syntax: true }),
        Box::new(Resharp),
        Box::new(RedSed),
    ]
}

pub fn engine_by_name(name: &str) -> Option<Box<dyn Engine>> {
    if name == AutomataFerroni.name() {
        return Some(Box::new(AutomataFerroni));
    }
    all_engines().into_iter().find(|e| e.name() == name)
}

#[cfg(test)]
mod tests {
    /// Defeito do `rusty_expressions` 0.2.2 achado pela bancada: `\[[a-z]+\]` não casa com `[db]`
    /// (o `ferroni`, outro porte do Oniguruma, casa; sem o `\[` inicial o `rusty_expressions` também).
    #[test]
    fn rusty_expressions_misses_escaped_bracket_before_class() {
        use rusty_expressions::{Options, Regex, Syntax};
        let buggy = Regex::new_str(r"\[[a-z]+\]", Options::NONE, Syntax::ONIGURUMA).unwrap();
        let found = buggy.search("[db]").unwrap();
        assert!(found.is_none(), "o defeito foi corrigido: {found:?}");
        let fine = Regex::new_str(r"[a-z]+\]", Options::NONE, Syntax::ONIGURUMA).unwrap();
        assert!(fine.search("[db]").unwrap().is_some());
    }
}
