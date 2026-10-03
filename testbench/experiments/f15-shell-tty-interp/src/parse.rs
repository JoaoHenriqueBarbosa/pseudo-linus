//! H37: o brush serve de base pro shell?
//!
//! Três medições:
//!
//! 1. **brush-parser contra `bash -n`** (aceita/rejeita igual) em quatro conjuntos: o corpus de shell
//!    (`corpus/cases/shell`), os scripts de todos os outros casos `script` da bancada, a suíte de
//!    testes do bash 5.2.37 (`tests/*.tests` e `*.sub`) e os comandos minerados dos transcripts
//!    (`corpus/agent/commands.jsonl`, só parse: o oráculo roda `bash -n`, que lê sem executar, e daqui
//!    só saem números agregados). Duas combinações de opção: bash padrão contra brush sem extglob, e
//!    `bash -O extglob` contra brush com extglob (o padrão do brush).
//! 2. **AST em dois níveis**: o brush guarda palavras e aritmética como texto cru; `$(...)` só aparece
//!    num segundo parse da palavra, e o conteúdo dele num terceiro. Medimos se esse re-parse recursivo
//!    cobre tudo (palavras, substituições aninhadas, aritmética, here-docs) e conferimos a profundidade
//!    de aninhamento em sondas construídas à mão.
//! 3. **brush-core e yash-env**: depscan do brush-core e pontos de host por arquivo (estimativa de
//!    fork); superfície de traits do yash-env como referência de desenho.

use std::collections::{BTreeMap, HashMap};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use brush_parser::word::{ParameterExpr, WordPiece, WordPieceWithSource};
use brush_parser::{ParserOptions, ast};
use harness::case::FileSpec;
use harness::{CandidateResult, Case, CaseFile, Fit, Verdict};
use serde::Serialize;
use serde_json::json;

use crate::common::{self, Section};
use crate::evidence;

/// Versão do bash cuja suíte de testes entra no corpus (a mesma do oráculo, 5.2.37-2 do Debian 13).
const BASH_VERSION: &str = "5.2.37";
const BASH_TARBALL_SHA256: &str = "9599b22ecd1d5787ad7d3b7bf0c59f312b3396d1e281175dd1f8a4014da621ff";

/// Um script a parsear. `weight` é quantas vezes ele foi chamado (comandos de agente) ou 1.
#[derive(Clone, Debug)]
pub struct Input {
    pub id: String,
    pub text: String,
    pub weight: u64,
}

/// Veredito do brush pra um script.
#[derive(Clone, Debug)]
enum Brush {
    Accepted,
    Rejected(String),
    Panicked(String),
}

impl Brush {
    fn accepted(&self) -> bool {
        matches!(self, Brush::Accepted)
    }
}

/// Concordância entre brush e bash num conjunto, numa combinação de opções.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Agreement {
    pub scripts: usize,
    pub calls: u64,
    pub both_accept: usize,
    pub both_reject: usize,
    /// bash aceita, brush rejeita (o pior caso: comando válido que não roda).
    pub brush_only_reject: usize,
    /// bash rejeita, brush aceita.
    pub brush_only_accept: usize,
    /// Dentre as divergências, quantas foram panic do brush.
    pub brush_panics: usize,
    pub agree_calls: u64,
    pub agreement: f64,
    pub agreement_calls: f64,
    /// Recursos presentes nos scripts divergentes (um script conta em vários).
    pub disagreement_features: BTreeMap<String, usize>,
    /// Classes de erro do brush nos scripts que o bash aceita.
    pub brush_error_classes: BTreeMap<String, usize>,
    /// Exemplos (só nos conjuntos que podem ir pro JSON: nunca no corpus de agente).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<Sample>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub id: String,
    pub bash_accepts: bool,
    pub brush: String,
    pub excerpt: String,
}

/// Estatística do parse em vários níveis (só scripts que o brush aceitou).
#[derive(Clone, Debug, Default, Serialize)]
pub struct DeepStats {
    pub programs: usize,
    /// Programas em que todos os níveis parsearam sem erro (inclusive aritmética).
    pub programs_fully_parsed: usize,
    /// Programas sem erro de sintaxe em nenhum nível (aritmética de fora, porque o bash só a avalia
    /// em tempo de execução).
    pub programs_syntax_clean: usize,
    pub words: usize,
    pub word_parse_errors: usize,
    pub command_substitutions: usize,
    pub backquoted_substitutions: usize,
    pub substitution_parse_errors: usize,
    pub process_substitutions: usize,
    /// Programas por profundidade máxima de substituição de comando (0 = nenhuma).
    pub max_depth_histogram: BTreeMap<usize, usize>,
    pub arithmetic: usize,
    /// Aritmética que contém `$` ou crase (o bash expande antes de avaliar).
    pub arithmetic_with_expansion: usize,
    pub arithmetic_parse_errors_plain: usize,
    pub arithmetic_parse_errors_with_expansion: usize,
    /// Palavras embutidas em `${x:-palavra}`, `${x/padrão/troca}` e afins, re-parseadas.
    pub parameter_inner_words: usize,
    pub parameter_inner_errors: usize,
    pub heredocs_expanding: usize,
    pub heredoc_parse_errors: usize,
    /// Exemplos de erro de segundo nível (só conjuntos commitáveis).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub error_samples: Vec<String>,
}

impl DeepStats {
    fn add(&mut self, o: &DeepStats, keep_samples: bool) {
        self.programs += o.programs;
        self.programs_fully_parsed += o.programs_fully_parsed;
        self.programs_syntax_clean += o.programs_syntax_clean;
        self.words += o.words;
        self.word_parse_errors += o.word_parse_errors;
        self.command_substitutions += o.command_substitutions;
        self.backquoted_substitutions += o.backquoted_substitutions;
        self.substitution_parse_errors += o.substitution_parse_errors;
        self.process_substitutions += o.process_substitutions;
        for (k, v) in &o.max_depth_histogram {
            *self.max_depth_histogram.entry(*k).or_default() += v;
        }
        self.arithmetic += o.arithmetic;
        self.arithmetic_with_expansion += o.arithmetic_with_expansion;
        self.arithmetic_parse_errors_plain += o.arithmetic_parse_errors_plain;
        self.arithmetic_parse_errors_with_expansion += o.arithmetic_parse_errors_with_expansion;
        self.parameter_inner_words += o.parameter_inner_words;
        self.parameter_inner_errors += o.parameter_inner_errors;
        self.heredocs_expanding += o.heredocs_expanding;
        self.heredoc_parse_errors += o.heredoc_parse_errors;
        if keep_samples {
            for s in &o.error_samples {
                if self.error_samples.len() < 12 {
                    self.error_samples.push(s.clone());
                }
            }
        }
    }
}

/// Percorre a AST do brush e re-parseia cada nível que ele deixou como texto cru.
struct Deep {
    opts: ParserOptions,
    stats: DeepStats,
    /// Erros de qualquer nível no programa corrente (inclui aritmética).
    errors_here: usize,
    /// Erros que o bash também daria no parse (palavra, substituição, here-doc, palavra em `${}`).
    /// Aritmética fica de fora: o bash só avalia (e reclama) em tempo de execução.
    syntax_errors_here: usize,
    max_depth: usize,
    keep_samples: bool,
}

impl Deep {
    fn new(opts: &ParserOptions, keep_samples: bool) -> Deep {
        Deep {
            opts: opts.clone(),
            stats: DeepStats::default(),
            errors_here: 0,
            syntax_errors_here: 0,
            max_depth: 0,
            keep_samples,
        }
    }

    fn sample(&mut self, what: &str, text: &str, err: &str) {
        self.errors_here += 1;
        if what != "aritmética" {
            self.syntax_errors_here += 1;
        }
        if self.keep_samples && self.stats.error_samples.len() < 12 {
            let t: String = text.chars().take(80).collect();
            let e: String = err.chars().take(120).collect();
            self.stats.error_samples.push(format!("{what}: {t:?}: {e}"));
        }
    }

    /// Ponto de entrada: um programa de nível superior.
    fn top(&mut self, prog: &ast::Program) {
        self.errors_here = 0;
        self.syntax_errors_here = 0;
        self.max_depth = 0;
        self.program(prog, 0);
        self.stats.programs += 1;
        if self.errors_here == 0 {
            self.stats.programs_fully_parsed += 1;
        }
        if self.syntax_errors_here == 0 {
            self.stats.programs_syntax_clean += 1;
        }
        *self.stats.max_depth_histogram.entry(self.max_depth).or_default() += 1;
    }

    fn program(&mut self, prog: &ast::Program, depth: usize) {
        for cc in &prog.complete_commands {
            self.compound_list(cc, depth);
        }
    }

    fn compound_list(&mut self, list: &ast::CompoundList, depth: usize) {
        for item in &list.0 {
            self.and_or(&item.0, depth);
        }
    }

    fn and_or(&mut self, list: &ast::AndOrList, depth: usize) {
        self.pipeline(&list.first, depth);
        for next in &list.additional {
            match next {
                ast::AndOr::And(p) | ast::AndOr::Or(p) => self.pipeline(p, depth),
            }
        }
    }

    fn pipeline(&mut self, p: &ast::Pipeline, depth: usize) {
        for cmd in &p.seq {
            self.command(cmd, depth);
        }
    }

    fn command(&mut self, cmd: &ast::Command, depth: usize) {
        match cmd {
            ast::Command::Simple(s) => self.simple(s, depth),
            ast::Command::Compound(c, redirs) => {
                self.compound(c, depth);
                self.redirect_list(redirs.as_ref(), depth);
            }
            ast::Command::Function(f) => {
                self.compound(&f.body.0, depth);
                self.redirect_list(f.body.1.as_ref(), depth);
            }
            ast::Command::ExtendedTest(t, redirs) => {
                self.ext_test(&t.expr, depth);
                self.redirect_list(redirs.as_ref(), depth);
            }
        }
    }

    fn compound(&mut self, c: &ast::CompoundCommand, depth: usize) {
        match c {
            ast::CompoundCommand::Arithmetic(a) => self.arith(&a.expr.value, depth),
            ast::CompoundCommand::ArithmeticForClause(f) => {
                for e in [&f.initializer, &f.condition, &f.updater].into_iter().flatten() {
                    self.arith(&e.value, depth);
                }
                self.compound_list(&f.body.list, depth);
            }
            ast::CompoundCommand::BraceGroup(b) => self.compound_list(&b.list, depth),
            ast::CompoundCommand::Subshell(s) => self.compound_list(&s.list, depth),
            ast::CompoundCommand::ForClause(f) => {
                for w in f.values.iter().flatten() {
                    self.word(w, depth);
                }
                self.compound_list(&f.body.list, depth);
            }
            ast::CompoundCommand::CaseClause(c) => {
                self.word(&c.value, depth);
                for item in &c.cases {
                    for p in &item.patterns {
                        self.word(p, depth);
                    }
                    if let Some(cmd) = &item.cmd {
                        self.compound_list(cmd, depth);
                    }
                }
            }
            ast::CompoundCommand::IfClause(i) => {
                self.compound_list(&i.condition, depth);
                self.compound_list(&i.then, depth);
                for e in i.elses.iter().flatten() {
                    if let Some(c) = &e.condition {
                        self.compound_list(c, depth);
                    }
                    self.compound_list(&e.body, depth);
                }
            }
            ast::CompoundCommand::WhileClause(w) | ast::CompoundCommand::UntilClause(w) => {
                self.compound_list(&w.0, depth);
                self.compound_list(&w.1.list, depth);
            }
            ast::CompoundCommand::Coprocess(c) => self.command(&c.body, depth),
        }
    }

    fn simple(&mut self, s: &ast::SimpleCommand, depth: usize) {
        if let Some(prefix) = &s.prefix {
            for item in &prefix.0 {
                self.prefix_item(item, depth);
            }
        }
        if let Some(w) = &s.word_or_name {
            self.word(w, depth);
        }
        if let Some(suffix) = &s.suffix {
            for item in &suffix.0 {
                self.prefix_item(item, depth);
            }
        }
    }

    fn prefix_item(&mut self, item: &ast::CommandPrefixOrSuffixItem, depth: usize) {
        match item {
            ast::CommandPrefixOrSuffixItem::IoRedirect(r) => self.redirect(r, depth),
            ast::CommandPrefixOrSuffixItem::Word(w) => self.word(w, depth),
            ast::CommandPrefixOrSuffixItem::AssignmentWord(a, _) => match &a.value {
                ast::AssignmentValue::Scalar(w) => self.word(w, depth),
                ast::AssignmentValue::Array(items) => {
                    for (k, v) in items {
                        if let Some(k) = k {
                            self.word(k, depth);
                        }
                        self.word(v, depth);
                    }
                }
            },
            ast::CommandPrefixOrSuffixItem::ProcessSubstitution(_, sub) => {
                self.stats.process_substitutions += 1;
                self.compound_list(&sub.list, depth);
            }
        }
    }

    fn redirect_list(&mut self, list: Option<&ast::RedirectList>, depth: usize) {
        for r in list.map(|l| l.0.as_slice()).unwrap_or(&[]) {
            self.redirect(r, depth);
        }
    }

    fn redirect(&mut self, r: &ast::IoRedirect, depth: usize) {
        match r {
            ast::IoRedirect::File(_, _, target) => match target {
                ast::IoFileRedirectTarget::Filename(w) | ast::IoFileRedirectTarget::Duplicate(w) => {
                    self.word(w, depth)
                }
                ast::IoFileRedirectTarget::Fd(_) => {}
                ast::IoFileRedirectTarget::ProcessSubstitution(_, sub) => {
                    self.stats.process_substitutions += 1;
                    self.compound_list(&sub.list, depth);
                }
            },
            ast::IoRedirect::HereDocument(_, doc) => {
                if doc.requires_expansion {
                    self.stats.heredocs_expanding += 1;
                    match brush_parser::word::parse_heredoc(&doc.doc.value, &self.opts) {
                        Ok(pieces) => self.pieces(&pieces, depth),
                        Err(e) => {
                            self.stats.heredoc_parse_errors += 1;
                            self.sample("heredoc", &doc.doc.value, &e.to_string());
                        }
                    }
                }
            }
            ast::IoRedirect::HereString(_, w) => self.word(w, depth),
            ast::IoRedirect::OutputAndError(w, _) => self.word(w, depth),
        }
    }

    fn ext_test(&mut self, e: &ast::ExtendedTestExpr, depth: usize) {
        match e {
            ast::ExtendedTestExpr::And(a, b) | ast::ExtendedTestExpr::Or(a, b) => {
                self.ext_test(a, depth);
                self.ext_test(b, depth);
            }
            ast::ExtendedTestExpr::Not(a) | ast::ExtendedTestExpr::Parenthesized(a) => self.ext_test(a, depth),
            ast::ExtendedTestExpr::UnaryTest(_, w) => self.word(w, depth),
            ast::ExtendedTestExpr::BinaryTest(_, a, b) => {
                self.word(a, depth);
                self.word(b, depth);
            }
        }
    }

    /// Segundo nível: o texto cru de uma palavra vira peças.
    fn word(&mut self, w: &ast::Word, depth: usize) {
        self.stats.words += 1;
        match brush_parser::word::parse(&w.value, &self.opts) {
            Ok(pieces) => self.pieces(&pieces, depth),
            Err(e) => {
                self.stats.word_parse_errors += 1;
                self.sample("palavra", &w.value, &e.to_string());
            }
        }
    }

    fn pieces(&mut self, pieces: &[WordPieceWithSource], depth: usize) {
        for p in pieces {
            match &p.piece {
                WordPiece::Text(_)
                | WordPiece::SingleQuotedText(_)
                | WordPiece::AnsiCQuotedText(_)
                | WordPiece::TildeExpansion(_)
                | WordPiece::EscapeSequence(_) => {}
                WordPiece::DoubleQuotedSequence(inner) | WordPiece::GettextDoubleQuotedSequence(inner) => {
                    self.pieces(inner, depth)
                }
                WordPiece::ParameterExpansion(pe) => self.param(pe, depth),
                WordPiece::CommandSubstitution(text) => {
                    self.stats.command_substitutions += 1;
                    self.substitution(text, depth);
                }
                WordPiece::BackquotedCommandSubstitution(text) => {
                    self.stats.backquoted_substitutions += 1;
                    self.substitution(&unescape_backquoted(text), depth);
                }
                WordPiece::ArithmeticExpression(a) => self.arith(&a.value, depth),
            }
        }
    }

    /// Terceiro nível: o conteúdo de `$(...)` ou de crases vira um programa de novo.
    fn substitution(&mut self, text: &str, depth: usize) {
        let inner_depth = depth + 1;
        self.max_depth = self.max_depth.max(inner_depth);
        match parse_program(text, &self.opts) {
            Ok(prog) => self.program(&prog, inner_depth),
            Err(e) => {
                self.stats.substitution_parse_errors += 1;
                self.sample("substituição", text, &e);
            }
        }
    }

    fn arith(&mut self, text: &str, depth: usize) {
        self.stats.arithmetic += 1;
        let has_expansion = text.contains('$') || text.contains('`');
        if has_expansion {
            self.stats.arithmetic_with_expansion += 1;
            // O bash expande parâmetros e substituições do texto antes de avaliar: as substituições
            // de dentro são achadas tratando o texto como palavra.
            if let Ok(pieces) = brush_parser::word::parse(text, &self.opts) {
                self.pieces(&pieces, depth);
            }
        }
        if let Err(e) = brush_parser::arithmetic::parse(text) {
            if has_expansion {
                self.stats.arithmetic_parse_errors_with_expansion += 1;
            } else {
                self.stats.arithmetic_parse_errors_plain += 1;
                self.sample("aritmética", text, &e.to_string());
            }
        }
    }

    fn inner_word(&mut self, text: Option<&String>, depth: usize) {
        let Some(text) = text else { return };
        self.stats.parameter_inner_words += 1;
        match brush_parser::word::parse(text, &self.opts) {
            Ok(pieces) => self.pieces(&pieces, depth),
            Err(e) => {
                self.stats.parameter_inner_errors += 1;
                self.sample("palavra em ${...}", text, &e.to_string());
            }
        }
    }

    fn param(&mut self, pe: &ParameterExpr, depth: usize) {
        match pe {
            ParameterExpr::UseDefaultValues { default_value: v, .. }
            | ParameterExpr::AssignDefaultValues { default_value: v, .. }
            | ParameterExpr::IndicateErrorIfNullOrUnset { error_message: v, .. }
            | ParameterExpr::UseAlternativeValue { alternative_value: v, .. }
            | ParameterExpr::RemoveSmallestSuffixPattern { pattern: v, .. }
            | ParameterExpr::RemoveLargestSuffixPattern { pattern: v, .. }
            | ParameterExpr::RemoveSmallestPrefixPattern { pattern: v, .. }
            | ParameterExpr::RemoveLargestPrefixPattern { pattern: v, .. }
            | ParameterExpr::UppercaseFirstChar { pattern: v, .. }
            | ParameterExpr::UppercasePattern { pattern: v, .. }
            | ParameterExpr::LowercaseFirstChar { pattern: v, .. }
            | ParameterExpr::LowercasePattern { pattern: v, .. } => self.inner_word(v.as_ref(), depth),
            ParameterExpr::ReplaceSubstring { pattern, replacement, .. } => {
                self.inner_word(Some(pattern), depth);
                self.inner_word(replacement.as_ref(), depth);
            }
            ParameterExpr::Substring { offset, length, .. } => {
                self.arith(&offset.value, depth);
                if let Some(l) = length {
                    self.arith(&l.value, depth);
                }
            }
            ParameterExpr::Parameter { .. }
            | ParameterExpr::ParameterLength { .. }
            | ParameterExpr::Transform { .. }
            | ParameterExpr::VariableNames { .. }
            | ParameterExpr::MemberKeys { .. } => {}
        }
    }
}

/// Dentro de crases, `\\`, `` \` `` e `\$` perdem a barra antes do parse do conteúdo (bash, seção
/// "Command Substitution"). O brush devolve o conteúdo ainda escapado: quem consome tem que fazer isso.
fn unescape_backquoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&n) = chars.peek() {
                if n == '\\' || n == '`' || n == '$' {
                    out.push(n);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

fn parse_program(text: &str, opts: &ParserOptions) -> std::result::Result<ast::Program, String> {
    let mut parser = brush_parser::Parser::new(std::io::Cursor::new(text.as_bytes()), opts);
    parser.parse_program().map_err(|e| e.to_string())
}

fn options(extglob: bool) -> ParserOptions {
    ParserOptions { enable_extended_globbing: extglob, ..ParserOptions::default() }
}

/// Parse de nível superior protegido contra panic.
fn brush_verdict(text: &str, extglob: bool) -> (Brush, Option<ast::Program>) {
    let opts = options(extglob);
    match std::panic::catch_unwind(AssertUnwindSafe(|| parse_program(text, &opts))) {
        Ok(Ok(p)) => (Brush::Accepted, Some(p)),
        Ok(Err(e)) => (Brush::Rejected(e), None),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            (Brush::Panicked(msg), None)
        }
    }
}

/// Recursos de shell presentes no texto (heurística por substring), pra classificar divergências.
fn features(text: &str) -> Vec<&'static str> {
    let mut f = Vec::new();
    let has = |s: &str| text.contains(s);
    if has("<<") {
        f.push("heredoc");
    }
    if has("$(") && !has("$((") {
        f.push("command-substitution");
    } else if has("$(") {
        f.push("command-substitution-or-arith");
    }
    if has("`") {
        f.push("backquote");
    }
    if has("((") {
        f.push("arithmetic");
    }
    if has("[[") {
        f.push("dbracket");
    }
    if has("case ") && has(" in") {
        f.push("case");
    }
    if ["@(", "!(", "+(", "*(", "?("].iter().any(|p| text.contains(p)) {
        f.push("extglob-pattern");
    }
    if has("<(") || has(">(") {
        f.push("process-substitution");
    }
    if has("$'") {
        f.push("ansi-c-quote");
    }
    if has("coproc") {
        f.push("coproc");
    }
    if has("function ") || has("() {") || has("(){") {
        f.push("function");
    }
    if has("=(") {
        f.push("array-assignment");
    }
    if has("${") {
        f.push("param-expansion");
    }
    if has("#") {
        f.push("hash-char");
    }
    if !text.is_ascii() {
        f.push("non-ascii");
    }
    if f.is_empty() {
        f.push("plain");
    }
    f
}

/// Classe curta de um erro do brush, pra agregar. Corta antes de qualquer posição ou trecho citado
/// (tag de here-doc, texto perto do erro): no corpus de agente, nenhum fragmento de comando pode ir
/// pro JSON.
fn error_class(err: &str) -> String {
    let mut end = err.len();
    for cut in [" at ", " near ", " '", ";", " (", " [", ":"] {
        if let Some(i) = err.find(cut) {
            end = end.min(i);
        }
    }
    err[..end].chars().take(60).collect()
}

/// Resultado do brush pra um script, nas duas combinações de opção, mais o parse profundo.
struct ScriptVerdict {
    noext: Brush,
    ext: Brush,
    /// `ext` com o parse profundo: aceito no primeiro nível mas com erro de sintaxe num nível de
    /// dentro (ex.: `$( if x; then )`) conta como rejeitado, que é o que o bash faz.
    ext_deep: Brush,
    deep: DeepStats,
    parse_us: f64,
}

fn brush_all(inputs: &[Input], keep_samples: bool) -> Vec<ScriptVerdict> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let chunk = inputs.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = inputs
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|inp| {
                            let start = Instant::now();
                            let (ext, prog) = brush_verdict(&inp.text, true);
                            let parse_us = start.elapsed().as_secs_f64() * 1e6;
                            let (noext, _) = brush_verdict(&inp.text, false);
                            let mut deep = Deep::new(&options(true), keep_samples);
                            let mut ext_deep = ext.clone();
                            if let Some(p) = &prog {
                                let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                                    deep.top(p);
                                    (deep.stats.clone(), deep.syntax_errors_here)
                                }));
                                let (stats, syntax_errors) = match r {
                                    Ok(x) => x,
                                    Err(_) => (DeepStats { programs: 1, ..DeepStats::default() }, 1),
                                };
                                deep.stats = stats;
                                if syntax_errors > 0 {
                                    ext_deep = Brush::Rejected("erro de sintaxe num nível de dentro".into());
                                }
                            }
                            ScriptVerdict { noext, ext, ext_deep, deep: deep.stats, parse_us }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("thread do brush")).collect()
    })
}

/// Resultado de um conjunto inteiro.
#[derive(Clone, Debug, Serialize)]
pub struct SetReport {
    pub name: String,
    pub scripts: usize,
    pub calls: u64,
    /// bash padrão x brush sem extglob.
    pub default_mode: Agreement,
    /// `bash -O extglob` x brush com extglob (o padrão do brush).
    pub extglob_mode: Agreement,
    /// Como `extglob_mode`, mas com o parse profundo decidindo (erro de sintaxe em `$(...)`,
    /// palavra ou here-doc conta como rejeição).
    pub extglob_deep_mode: Agreement,
    pub bash_accept_rate: f64,
    pub brush_accept_rate: f64,
    pub deep: DeepStats,
    pub brush_parse_us_median: f64,
    pub brush_parse_us_p99: f64,
}

fn agreement(inputs: &[Input], bash: &[bool], brush: &[&Brush], keep_samples: bool) -> Agreement {
    let mut a = Agreement::default();
    for ((inp, &b_ok), br) in inputs.iter().zip(bash).zip(brush) {
        a.scripts += 1;
        a.calls += inp.weight;
        let r_ok = br.accepted();
        match (b_ok, r_ok) {
            (true, true) => a.both_accept += 1,
            (false, false) => a.both_reject += 1,
            (true, false) => a.brush_only_reject += 1,
            (false, true) => a.brush_only_accept += 1,
        }
        if b_ok == r_ok {
            a.agree_calls += inp.weight;
            continue;
        }
        if matches!(br, Brush::Panicked(_)) {
            a.brush_panics += 1;
        }
        for f in features(&inp.text) {
            *a.disagreement_features.entry(f.to_string()).or_default() += 1;
        }
        let brush_text = match br {
            Brush::Accepted => "aceita".to_string(),
            Brush::Rejected(e) => {
                if b_ok {
                    *a.brush_error_classes.entry(error_class(e)).or_default() += 1;
                }
                format!("rejeita: {e}")
            }
            Brush::Panicked(m) => {
                if b_ok {
                    *a.brush_error_classes.entry("panic".into()).or_default() += 1;
                }
                format!("panic: {m}")
            }
        };
        if keep_samples && a.samples.len() < 25 {
            a.samples.push(Sample {
                id: inp.id.clone(),
                bash_accepts: b_ok,
                brush: brush_text.chars().take(160).collect(),
                excerpt: excerpt(&inp.text, &brush_text),
            });
        }
    }
    a.agreement = ratio(a.both_accept + a.both_reject, a.scripts);
    a.agreement_calls = if a.calls == 0 { 0.0 } else { a.agree_calls as f64 / a.calls as f64 };
    a
}

/// Trecho curto do script em volta da linha do erro (quando o erro tem linha), senão o começo.
fn excerpt(text: &str, err: &str) -> String {
    let line = err
        .split("line ")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse::<usize>().ok());
    let lines: Vec<&str> = text.lines().collect();
    let pick = match line {
        Some(n) if n >= 1 && n <= lines.len() => lines[n.saturating_sub(2)..(n + 1).min(lines.len())].join("\n"),
        _ => lines.iter().take(3).copied().collect::<Vec<_>>().join("\n"),
    };
    pick.chars().take(240).collect()
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { a as f64 / b as f64 }
}

fn percentile(xs: &[f64], q: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f64 - 1.0) * q).round() as usize]
}

fn evaluate_set(name: &str, inputs: &[Input], bash: &[(i32, i32)], keep_samples: bool) -> SetReport {
    let verdicts = brush_all(inputs, keep_samples);
    let bash_default: Vec<bool> = bash.iter().map(|(a, _)| *a == 0).collect();
    let bash_ext: Vec<bool> = bash.iter().map(|(_, b)| *b == 0).collect();
    let noext: Vec<&Brush> = verdicts.iter().map(|v| &v.noext).collect();
    let ext: Vec<&Brush> = verdicts.iter().map(|v| &v.ext).collect();
    let ext_deep: Vec<&Brush> = verdicts.iter().map(|v| &v.ext_deep).collect();
    let default_mode = agreement(inputs, &bash_default, &noext, keep_samples);
    let extglob_mode = agreement(inputs, &bash_ext, &ext, keep_samples);
    let extglob_deep_mode = agreement(inputs, &bash_ext, &ext_deep, keep_samples);
    let mut deep = DeepStats::default();
    for v in &verdicts {
        deep.add(&v.deep, keep_samples);
    }
    let times: Vec<f64> = verdicts.iter().map(|v| v.parse_us).collect();
    SetReport {
        name: name.to_string(),
        scripts: inputs.len(),
        calls: inputs.iter().map(|i| i.weight).sum(),
        bash_accept_rate: ratio(bash_default.iter().filter(|b| **b).count(), inputs.len()),
        brush_accept_rate: ratio(ext.iter().filter(|b| b.accepted()).count(), inputs.len()),
        default_mode,
        extglob_mode,
        extglob_deep_mode,
        deep,
        brush_parse_us_median: percentile(&times, 0.5),
        brush_parse_us_p99: percentile(&times, 0.99),
    }
}

// ---------------------------------------------------------------------------------------------
// bash -n no oráculo
// ---------------------------------------------------------------------------------------------

/// Roda `bash -n` e `bash -O extglob -n` no oráculo pra cada script, com cache por sha256 (o cache
/// guarda só hash e códigos de saída, nunca o texto). Nada é executado: `-n` só lê e parseia.
pub fn bash_n(texts: &[&str], fresh: bool) -> Result<Vec<(i32, i32)>> {
    let cache_path = common::cache_dir().join("bash-n.json");
    let mut cache: HashMap<String, (i32, i32)> = if fresh {
        HashMap::new()
    } else {
        std::fs::read_to_string(&cache_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    };
    let hashes: Vec<String> = texts.iter().map(|t| harness::memtree::sha256_hex(t.as_bytes())).collect();
    let mut missing: Vec<(String, &str)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (h, t) in hashes.iter().zip(texts) {
        if !cache.contains_key(h) && seen.insert(h.clone()) {
            missing.push((h.clone(), t));
        }
    }
    if !missing.is_empty() {
        let oracle = harness::Oracle::locate()?;
        let mut cases = Vec::new();
        let mut index: Vec<Vec<String>> = Vec::new();
        for (bi, batch) in missing.chunks(4000).enumerate() {
            let mut files = BTreeMap::new();
            let mut keys = Vec::new();
            for (i, (h, t)) in batch.iter().enumerate() {
                files.insert(format!("c/{i:05}"), FileSpec::Text(t.to_string()));
                keys.push(h.clone());
            }
            index.push(keys);
            cases.push(Case {
                id: format!("bash-n-batch-{bi}"),
                argv: Vec::new(),
                // Só `bash -n`: lê e parseia, não executa nada. Em paralelo dentro do container.
                script: Some(
                    "cd c || exit 1\n\
                     printf '%s\\0' * | xargs -0 -P 16 -n 64 bash -c 'for f; do bash -n \"$f\" 2>/dev/null; a=$?; bash -O extglob -n \"$f\" 2>/dev/null; echo \"$f $a $?\"; done' f15\n\
                     cd .. && rm -rf c\n"
                        .to_string(),
                ),
                stdin: None,
                stdin_b64: None,
                files,
                env: BTreeMap::new(),
                tags: Vec::new(),
                faketime: None,
                timeout_ms: Some(900_000),
            });
        }
        let outcomes = oracle.run(&cases).context("bash -n no oráculo")?;
        for (keys, out) in index.iter().zip(outcomes) {
            let text = String::from_utf8_lossy(out.stdout.as_slice()).into_owned();
            let mut got = 0;
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let [name, a, b] = parts.as_slice() {
                    let i: usize = name.parse().context("índice do bash -n")?;
                    cache.insert(keys[i].clone(), (a.parse()?, b.parse()?));
                    got += 1;
                }
            }
            anyhow::ensure!(got == keys.len(), "bash -n devolveu {got} de {} resultados", keys.len());
        }
        std::fs::write(&cache_path, serde_json::to_string(&cache)?)?;
    }
    Ok(hashes.iter().map(|h| cache[h]).collect())
}

// ---------------------------------------------------------------------------------------------
// Conjuntos de entrada
// ---------------------------------------------------------------------------------------------

/// Scripts de todos os casos `script` de uma ferramenta.
fn case_scripts(tool: &str) -> Result<Vec<Input>> {
    let mut out = Vec::new();
    for path in harness::paths::case_files(tool)? {
        let file = CaseFile::load(&path)?;
        for case in file.cases {
            if let Some(s) = case.script {
                out.push(Input { id: format!("{tool}/{}", case.id), text: s, weight: 1 });
            }
        }
    }
    Ok(out)
}

fn upstream_dir() -> PathBuf {
    harness::paths::corpus_dir().join("upstream").join("bash")
}

/// Garante `corpus/upstream/bash` com os `tests/*.tests` e `*.sub` do bash 5.2.37 (baixa se faltar).
pub fn ensure_upstream() -> Result<PathBuf> {
    let dir = upstream_dir();
    if dir.join("arith.tests").exists() {
        return Ok(dir);
    }
    let tarball = common::cache_dir().join(format!("bash-{BASH_VERSION}.tar.gz"));
    if !tarball.exists() {
        common::run_cmd(
            Command::new("curl")
                .args(["-sSfL", "-o"])
                .arg(&tarball)
                .arg(format!("https://ftp.gnu.org/gnu/bash/bash-{BASH_VERSION}.tar.gz")),
        )?;
    }
    let sum = common::run_cmd(Command::new("sha256sum").arg(&tarball))?;
    anyhow::ensure!(sum.starts_with(BASH_TARBALL_SHA256), "sha256 do tarball do bash não confere: {sum}");
    std::fs::create_dir_all(&dir)?;
    common::run_cmd(
        Command::new("tar")
            .arg("-xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(&dir)
            .args(["--strip-components=2", "--wildcards"])
            .arg(format!("bash-{BASH_VERSION}/tests/*.tests"))
            .arg(format!("bash-{BASH_VERSION}/tests/*.sub")),
    )?;
    Ok(dir)
}

fn upstream_scripts() -> Result<Vec<Input>> {
    let dir = ensure_upstream()?;
    let mut paths = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for item in std::fs::read_dir(&d)? {
            let p = item?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "tests" || x == "sub") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let bytes = std::fs::read(&p)?;
        // O brush trabalha sobre `char`: texto não UTF-8 nem chega nele. Conta à parte.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        out.push(Input { id: evidence::rel(&dir, &p), text, weight: 1 });
    }
    Ok(out)
}

/// Comandos minerados (gitignored). Nunca executados: só brush-parser e `bash -n`.
fn agent_commands() -> Result<Option<Vec<Input>>> {
    let path = harness::paths::corpus_dir().join("agent").join("commands.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(None) };
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).with_context(|| format!("commands.jsonl linha {}", i + 1))?;
        let Some(cmd) = v.get("command").and_then(|c| c.as_str()) else { continue };
        let weight = v.get("count").and_then(|c| c.as_u64()).unwrap_or(1).max(1);
        out.push(Input { id: format!("agent#{i}"), text: cmd.to_string(), weight });
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------------------------------------
// Sondas de aninhamento
// ---------------------------------------------------------------------------------------------

/// Sondas construídas à mão: (id, script, profundidade máxima de substituição de comando esperada).
/// `None` = script inválido: o bash rejeita, e o parse em níveis tem que achar o erro (no primeiro
/// nível ou num de dentro).
pub const NESTING_PROBES: &[(&str, &str, Option<usize>)] = &[
    ("flat", "echo $(echo a)", Some(1)),
    ("dq-in-dq", r#"echo "$(echo "$(echo b)")""#, Some(2)),
    ("four-levels", "x=$(a $(b $(c $(d))))", Some(4)),
    ("printf-three", r#"x=$(printf '%s' "$(printf '%s' "$(printf '%s' 'deep')")")"#, Some(3)),
    ("backquote-nested", r"echo `echo \`echo c\``", Some(2)),
    ("backquote-in-dollar", r"echo $(echo `echo d`)", Some(2)),
    ("case-paren-inside", "echo $(case x in x) echo y;; esac)", Some(1)),
    ("case-paren-nested", "echo $(echo $(case x in (x) echo y;; esac))", Some(2)),
    ("heredoc-inside", "echo $(cat <<EOF\n$(echo in-heredoc)\nEOF\n)", Some(2)),
    ("param-default", "echo ${v:-$(echo d)}", Some(1)),
    ("param-default-nested", r#"echo "${v:-$(echo "${w:-$(echo e)}")}""#, Some(2)),
    ("arith-inside-cmdsub", "a=$(echo $((1+2)))", Some(1)),
    ("cmdsub-inside-arith", "echo $(( $(echo 3) + 1 ))", Some(1)),
    ("cmdsub-in-arith-in-cmdsub", "echo $(echo $(( $(echo 2) * 2 )))", Some(2)),
    ("dbracket", "[[ $(echo x) == x ]]", Some(1)),
    ("for-values", "for i in $(seq $(echo 3)); do :; done", Some(2)),
    ("procsub-then-cmdsub", "cat <(echo $(echo p))", Some(1)),
    ("quoted-paren", r#"echo "$(echo "a)b")""#, Some(1)),
    ("single-quoted-paren", "echo $(echo ')')", Some(1)),
    ("comment-with-paren", "echo $(# comentário com )\necho x)", Some(1)),
    ("subshell-inside", "echo $( (echo sub) )", Some(1)),
    ("escaped-dollar-inside", r#"echo $(echo "\$(nao)")"#, Some(1)),
    ("heredoc-param", "cat <<EOF\n${x:-$(echo h)}\nEOF", Some(1)),
    ("assignment-array", "arr=($(echo a) \"$(echo $(echo b))\")", Some(2)),
    ("redirect-target", "echo x > $(echo $(echo f)).txt", Some(2)),
    // `$(< arquivo)`: atalho do bash pra `$(cat arquivo)`, comum em script de agente.
    ("redirect-only-inside", "x=$(< file.txt)", Some(1)),
    ("redirect-only-inside-quoted", r#"x="$(< "$f")""#, Some(1)),
    // Inválidos: só o segundo nível enxerga o erro (o primeiro guarda o conteúdo de $(...) cru).
    ("invalid-escaped-paren-inside", r"echo $(echo \$(nao))", None),
    ("invalid-if-inside", "echo $( if x; then echo foo )", None),
    ("invalid-two-levels-down", "echo $(echo $(for in; do))", None),
];

#[derive(Clone, Debug, Serialize)]
pub struct ProbeResult {
    pub id: String,
    pub expected_depth: Option<usize>,
    pub found_depth: Option<usize>,
    pub bash_accepts: bool,
    /// Aceito no primeiro nível (o que um consumidor ingênuo do brush veria).
    pub brush_accepts_top_level: bool,
    pub syntax_errors_inside: usize,
    pub ok: bool,
}

fn run_probes(fresh: bool) -> Result<Vec<ProbeResult>> {
    let texts: Vec<&str> = NESTING_PROBES.iter().map(|(_, t, _)| *t).collect();
    let bash = bash_n(&texts, fresh)?;
    let mut out = Vec::new();
    for ((id, text, expected), (b, _)) in NESTING_PROBES.iter().zip(bash) {
        let (verdict, prog) = brush_verdict(text, true);
        let (found, errs) = match &prog {
            Some(p) => {
                let mut d = Deep::new(&options(true), true);
                d.top(p);
                (Some(d.max_depth), d.syntax_errors_here)
            }
            None => (None, 0),
        };
        let ok = match expected {
            Some(depth) => b == 0 && found == Some(*depth) && errs == 0,
            None => b != 0 && (!verdict.accepted() || errs > 0),
        };
        out.push(ProbeResult {
            id: id.to_string(),
            expected_depth: *expected,
            found_depth: found,
            bash_accepts: b == 0,
            brush_accepts_top_level: verdict.accepted(),
            syntax_errors_inside: errs,
            ok,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// brush-core (fork) e yash-env (referência)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct FileHostPoints {
    pub file: String,
    pub lines: usize,
    pub host_points: usize,
    pub tokio_refs: usize,
    pub async_fns: usize,
    pub unsafe_total: usize,
}

fn core_fork_estimate() -> Result<(serde_json::Value, CandidateResult)> {
    let scan = depscan::scan(&common::manifest(), "brush-core")?;
    let root = scan.root.manifest_dir.clone();
    let mut files = Vec::new();
    let mut stack = vec![root.join("src")];
    while let Some(d) = stack.pop() {
        for item in std::fs::read_dir(&d)? {
            let p = item?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p)?;
                let mut c = depscan::Counts::default();
                depscan::scan_source(&text, &mut c);
                files.push(FileHostPoints {
                    file: evidence::rel(&root, &p),
                    lines: text.lines().count(),
                    host_points: c.host_touch(),
                    tokio_refs: text.matches("tokio::").count(),
                    async_fns: text.matches("async fn").count(),
                    unsafe_total: c.unsafe_total(),
                });
            }
        }
    }
    files.sort_by(|a, b| b.host_points.cmp(&a.host_points).then(a.file.cmp(&b.file)));
    let total_lines: usize = files.iter().map(|f| f.lines).sum();
    let touching: Vec<&FileHostPoints> = files.iter().filter(|f| f.host_points > 0).collect();
    let touching_lines: usize = touching.iter().map(|f| f.lines).sum();
    let tokio_files = files.iter().filter(|f| f.tokio_refs > 0 || f.async_fns > 0).count();
    let async_fns: usize = files.iter().map(|f| f.async_fns).sum();
    let tokio_refs: usize = files.iter().map(|f| f.tokio_refs).sum();
    let host_points: usize = files.iter().map(|f| f.host_points).sum();
    let deps_touching: BTreeMap<String, usize> = scan.host_touching_deps.clone();
    let metrics = json!({
        "own_category": scan.root.category.letter(),
        "tree_category": scan.tree_category.letter(),
        "own_counts": scan.root.counts,
        "tree_deps": scan.deps.len(),
        "tree_host_points": scan.totals.host_touch(),
        "tree_unsafe": scan.totals.unsafe_total(),
        "c_deps": scan.c_deps,
        "host_touching_deps": deps_touching,
        "files": files.len(),
        "lines": total_lines,
        "files_touching_host": touching.len(),
        "lines_in_files_touching_host": touching_lines,
        "host_points": host_points,
        "files_with_async_or_tokio": tokio_files,
        "async_fns": async_fns,
        "tokio_refs": tokio_refs,
        "top_files": files.iter().take(15).collect::<Vec<_>>(),
    });
    let cand = CandidateResult {
        name: "brush-core".into(),
        version: scan.root.version.clone(),
        role: "shell-core".into(),
        category: Some(scan.root.category.letter().into()),
        conformance: None,
        fit: Fit::DoesNotFit,
        notes: format!(
            "Interpretador do brush: {host_points} pontos de host em {} de {} arquivos ({} de {} linhas), \
             {async_fns} async fn e {tokio_refs} referências a tokio em {tokio_files} arquivos; a árvore tem {} \
             dependências e {} pontos de host. Adaptar ao Ctx exige reescrever o executor inteiro (async sobre \
             tokio e processos do host) e não só trocar chamadas: o fork custaria mais que um interpretador \
             nosso sobre o brush-parser.",
            touching.len(),
            files.len(),
            touching_lines,
            total_lines,
            scan.deps.len(),
            scan.totals.host_touch(),
        ),
        metrics,
    };
    let summary = json!({
        "host_points": host_points,
        "files_touching_host": touching.len(),
        "files": files.len(),
        "async_fns": async_fns,
        "tokio_refs": tokio_refs,
        "lines_in_files_touching_host": touching_lines,
        "lines": total_lines,
    });
    Ok((summary, cand))
}

/// Traits públicas de um arquivo e quantos métodos cada uma tem.
fn traits_in(path: &Path) -> Result<Vec<(String, usize)>> {
    let text = std::fs::read_to_string(path)?;
    let file = syn::parse_file(&text).with_context(|| format!("syn {}", path.display()))?;
    let mut out = Vec::new();
    for item in file.items {
        if let syn::Item::Trait(t) = item
            && matches!(t.vis, syn::Visibility::Public(_))
        {
            let methods = t.items.iter().filter(|i| matches!(i, syn::TraitItem::Fn(_))).count();
            out.push((t.ident.to_string(), methods));
        }
    }
    Ok(out)
}

fn yash_reference() -> Result<CandidateResult> {
    let root = evidence::crate_root("yash-env")?;
    let manifest = std::fs::read_to_string(root.join("Cargo.toml"))?;
    let license = manifest
        .lines()
        .find(|l| l.starts_with("license"))
        .and_then(|l| l.split('"').nth(1))
        .unwrap_or("?")
        .to_string();
    let mut by_file: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    let mut stack = vec![root.join("src/system")];
    let mut system_files = vec![root.join("src/system.rs")];
    while let Some(d) = stack.pop() {
        for item in std::fs::read_dir(&d)? {
            let p = item?.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                system_files.push(p);
            }
        }
    }
    for p in &system_files {
        let traits = traits_in(p)?;
        if !traits.is_empty() {
            by_file.insert(evidence::rel(&root, p), traits);
        }
    }
    let trait_count: usize = by_file.values().map(|v| v.len()).sum();
    let method_count: usize = by_file.values().flat_map(|v| v.iter().map(|(_, m)| m)).sum();
    let virtual_lines = std::fs::read_to_string(root.join("src/system/virtual.rs")).map(|t| t.lines().count()).unwrap_or(0);
    let mut virtual_dir_lines = 0;
    if let Ok(rd) = std::fs::read_dir(root.join("src/system/virtual")) {
        for item in rd.flatten() {
            if let Ok(t) = std::fs::read_to_string(item.path()) {
                virtual_dir_lines += t.lines().count();
            }
        }
    }
    let ev = vec![
        evidence::find("yash-env", "Cargo.toml", "license", "licença GPL: não pode ser dependência nem fonte de código copiado")?,
        evidence::find("yash-env", "src/system/virtual.rs", "pub struct VirtualSystem", "implementação virtual completa do sistema (FS, processos, sinais) atrás das mesmas traits")?,
        evidence::find("yash-env", "src/system/file_system.rs", "pub trait Open", "operações de sistema separadas em traits pequenas (uma por syscall)")?,
        evidence::find("yash-env", "src/system/process.rs", "pub trait Fork", "fork é uma trait do sistema: no virtual vira clonar o estado")?,
    ];
    Ok(CandidateResult {
        name: "yash-env".into(),
        version: root.file_name().map(|s| s.to_string_lossy().trim_start_matches("yash-env-").to_string()).unwrap_or_default(),
        role: "shell-core".into(),
        category: None,
        conformance: None,
        fit: Fit::Reference,
        notes: format!(
            "Licença {license} e só POSIX sh (sem arrays, [[ ]], here-string, process substitution): referência \
             de desenho, nunca dependência. Vale copiar a ideia, não o código: o ambiente do shell genérico \
             sobre um sistema `S`, com {trait_count} traits públicas e {method_count} métodos em src/system \
             (uma trait por grupo de syscall: Open, Read, Write, Dup, Pipe, Fork, Wait, Exec, Sigaction...), e \
             uma VirtualSystem ({} linhas) que implementa tudo em memória pros testes. É o mesmo corte que o \
             nosso Ctx precisa: o interpretador nosso fala só com a trait, o kernel do sandbox implementa."
            ,
            virtual_lines + virtual_dir_lines
        ),
        metrics: json!({
            "license": license,
            "traits_by_file": by_file,
            "trait_count": trait_count,
            "method_count": method_count,
            "virtual_system_lines": virtual_lines + virtual_dir_lines,
            "evidence": ev,
        }),
    })
}

// ---------------------------------------------------------------------------------------------
// Orquestração do H37
// ---------------------------------------------------------------------------------------------

pub fn run(fresh: bool) -> Result<Section> {
    let started = Instant::now();
    let mut notes = Vec::new();

    let shell = case_scripts("shell")?;
    let mut others = Vec::new();
    for tool in harness::paths::tools()? {
        if tool != "shell" {
            others.extend(case_scripts(&tool)?);
        }
    }
    let upstream = upstream_scripts()?;
    let agent = agent_commands()?;

    // Um `bash -n` só pra todos os conjuntos (o cache deduplica por conteúdo).
    let mut all_texts: Vec<&str> = Vec::new();
    for set in [&shell, &others, &upstream] {
        all_texts.extend(set.iter().map(|i| i.text.as_str()));
    }
    if let Some(a) = &agent {
        all_texts.extend(a.iter().map(|i| i.text.as_str()));
    }
    let t_bash = Instant::now();
    let bash_all = bash_n(&all_texts, fresh)?;
    let bash_secs = t_bash.elapsed().as_secs_f64();
    let mut offset = 0;
    let mut take = |n: usize| {
        let s = bash_all[offset..offset + n].to_vec();
        offset += n;
        s
    };
    let bash_shell = take(shell.len());
    let bash_others = take(others.len());
    let bash_upstream = take(upstream.len());
    let bash_agent = agent.as_ref().map(|a| take(a.len()));

    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let t_brush = Instant::now();
    let r_shell = evaluate_set("shell-corpus", &shell, &bash_shell, true);
    let r_others = evaluate_set("other-tool-cases", &others, &bash_others, true);
    let r_upstream = evaluate_set("bash-5.2.37-tests", &upstream, &bash_upstream, true);
    let r_agent = match (&agent, &bash_agent) {
        (Some(a), Some(b)) => Some(evaluate_set("agent-commands", a, b, false)),
        _ => None,
    };
    let brush_secs = t_brush.elapsed().as_secs_f64();
    let probes = run_probes(fresh)?;
    std::panic::set_hook(quiet);

    if agent.is_none() {
        notes.push("corpus/agent/commands.jsonl ausente: conjunto de comandos de agente não medido".into());
    }

    // Evidência de código do brush-parser.
    let parser_evidence = vec![
        evidence::find("brush-parser", "src/ast.rs", "pub value: String,", "ast::Word guarda só o texto cru da palavra (primeiro nível)")?,
        evidence::find("brush-parser", "src/word.rs", "CommandSubstitution(String),", "word::parse devolve o conteúdo de $(...) como String: precisa de um terceiro parse")?,
        evidence::find("brush-parser", "src/ast.rs", "pub struct UnexpandedArithmeticExpr", "aritmética também fica crua até arithmetic::parse")?,
        evidence::find("brush-parser", "src/parser/winnow_str.rs", "unimplemented!", "a implementação winnow-parser da 0.4.0 é um stub: só a PEG funciona")?,
        evidence::find("brush-parser", "src/word.rs", "#[cfg(feature = \"debug-tracing\")]", "o println! que dá categoria b à crate está atrás da feature debug-tracing (desligada); o resto é o snapshot_tests.rs, só de teste")?,
        evidence::find("brush-parser", "src/tokenizer.rs", "unterminated here document sequence", "here-doc sem terminador antes do fim do texto é erro no brush; o bash aceita com aviso (todas as divergências do corpus de agente)")?,
    ];

    let parser_scan = depscan::scan(&common::manifest(), "brush-parser")?;
    let (core_summary, core_cand) = core_fork_estimate()?;
    let yash = yash_reference()?;

    let probes_ok = probes.iter().filter(|p| p.ok).count();
    let primary = r_agent.as_ref().unwrap_or(&r_shell);
    let agent_rate = r_agent.as_ref().map(|r| r.default_mode.agreement_calls);
    let shell_rate = r_shell.default_mode.agreement;
    let upstream_rate = r_upstream.default_mode.agreement;
    let deep_full = |r: &SetReport| ratio(r.deep.programs_fully_parsed, r.deep.programs);
    let deep_clean = |r: &SetReport| ratio(r.deep.programs_syntax_clean, r.deep.programs);

    let parser_ok = agent_rate.unwrap_or(shell_rate) >= 0.995 && shell_rate >= 0.98;
    let parser_fit = if parser_ok { Fit::FitsWithWork } else { Fit::DoesNotFit };
    let parser_cand = CandidateResult {
        name: "brush-parser".into(),
        version: parser_scan.root.version.clone(),
        role: "shell-parser".into(),
        category: Some(parser_scan.tree_category.letter().into()),
        conformance: None,
        fit: parser_fit,
        notes: format!(
            "Concordância com bash -n (bash padrão x brush sem extglob): corpus de shell {:.1}%, outros casos \
             {:.1}%, suíte do bash {:.1}%{}. Com extglob dos dois lados: shell {:.1}%, suíte {:.1}%{}; com o \
             parse profundo decidindo (erro dentro de $(...) conta), suíte {:.1}%. Divergências por causa: \
             here-doc sem terminador antes do fim do texto (o bash aceita com aviso, o brush rejeita: todas as \
             divergências do corpus de agente), `case` com padrão sem parêntese de abertura dentro de $(...), \
             `select`, `{{fd}}<arq` depois de `done`, `for ((;;))` com quebra antes do `do`, e extglob dentro \
             de [[ ]] com extglob desligado (o bash liga sozinho dentro de [[ ]]). AST em níveis: {:.1}% dos \
             programas do corpus de shell, {:.1}% da suíte e {} do agente sem erro de sintaxe em nenhum nível \
             (palavras, $(...) recursivo, here-docs, palavras em ${{...}}); sondas de aninhamento {probes_ok}/{}. \
             Categoria ({}) pelo depscan, {} pontos de host na árvore (getrandom, parking_lot e afins, nada no \
             caminho do parse). Serve de parser com camada nossa por cima (re-parse recursivo, desescape de \
             crases, aritmética depois da expansão, correção dos casos acima); a variante winnow da 0.4.0 é stub.",
            shell_rate * 100.0,
            r_others.default_mode.agreement * 100.0,
            upstream_rate * 100.0,
            agent_rate.map(|r| format!(", comandos de agente {:.3}% das chamadas", r * 100.0)).unwrap_or_default(),
            r_shell.extglob_mode.agreement * 100.0,
            r_upstream.extglob_mode.agreement * 100.0,
            r_agent
                .as_ref()
                .map(|r| format!(", agente {:.3}% das chamadas", r.extglob_mode.agreement_calls * 100.0))
                .unwrap_or_default(),
            r_upstream.extglob_deep_mode.agreement * 100.0,
            deep_clean(&r_shell) * 100.0,
            deep_clean(&r_upstream) * 100.0,
            r_agent.as_ref().map(|r| format!("{:.3}%", deep_clean(r) * 100.0)).unwrap_or_else(|| "n/d".into()),
            probes.len(),
            parser_scan.tree_category.letter(),
            parser_scan.totals.host_touch(),
        ),
        metrics: json!({
            "sets": [&r_shell, &r_others, &r_upstream],
            "agent": r_agent,
            "nesting_probes": probes,
            "depscan": {
                "own_category": parser_scan.root.category.letter(),
                "tree_category": parser_scan.tree_category.letter(),
                "tree_deps": parser_scan.deps.len(),
                "tree_host_points": parser_scan.totals.host_touch(),
                "host_touching_deps": parser_scan.host_touching_deps,
                "tree_unsafe": parser_scan.totals.unsafe_total(),
            },
            "evidence": parser_evidence,
        }),
    };

    let core_ok = false;
    let verdict = match (parser_ok, core_ok) {
        (true, true) => Verdict::Confirmed,
        (true, false) | (false, true) => Verdict::Partial,
        (false, false) => Verdict::Refuted,
    };
    let summary = format!(
        "Parcial: o brush-parser concorda com bash -n em {} e cobre o segundo nível com re-parse nosso \
         ({probes_ok}/{} sondas de $(...) aninhado certas), mas o brush-core tem {} pontos de host em {} \
         arquivos e {} async fn sobre tokio: parser sim (com camada nossa), interpretador nosso.",
        match (&r_agent, agent_rate) {
            (Some(r), Some(rate)) => format!(
                "{:.2}% das {} chamadas de agente ({} comandos únicos), {:.1}% do corpus de shell e {:.1}% da suíte do bash",
                rate * 100.0,
                r.calls,
                r.scripts,
                shell_rate * 100.0,
                upstream_rate * 100.0
            ),
            _ => format!("{:.1}% do corpus de shell e {:.1}% da suíte do bash", shell_rate * 100.0, upstream_rate * 100.0),
        },
        probes.len(),
        core_summary["host_points"],
        core_summary["files_touching_host"],
        core_summary["async_fns"],
    );
    let verdict = if parser_ok { verdict } else { Verdict::Refuted };
    let evidence = json!({
        "agreement_default_mode": {
            "shell_corpus": r_shell.default_mode.agreement,
            "other_tool_cases": r_others.default_mode.agreement,
            "bash_tests": r_upstream.default_mode.agreement,
            "agent_calls": agent_rate,
            "agent_scripts": r_agent.as_ref().map(|r| r.default_mode.agreement),
        },
        "agreement_extglob_mode": {
            "shell_corpus": r_shell.extglob_mode.agreement,
            "bash_tests": r_upstream.extglob_mode.agreement,
            "agent_calls": r_agent.as_ref().map(|r| r.extglob_mode.agreement_calls),
        },
        "agreement_extglob_deep_mode": {
            "shell_corpus": r_shell.extglob_deep_mode.agreement,
            "bash_tests": r_upstream.extglob_deep_mode.agreement,
            "agent_calls": r_agent.as_ref().map(|r| r.extglob_deep_mode.agreement_calls),
        },
        "agent_brush_only_reject": r_agent.as_ref().map(|r| r.default_mode.brush_only_reject),
        "agent_brush_only_accept": r_agent.as_ref().map(|r| r.default_mode.brush_only_accept),
        "deep_parse_syntax_clean": {
            "shell_corpus": deep_clean(&r_shell),
            "bash_tests": deep_clean(&r_upstream),
            "agent": r_agent.as_ref().map(deep_clean),
        },
        "deep_parse_fully_ok_including_arithmetic": {
            "shell_corpus": deep_full(&r_shell),
            "bash_tests": deep_full(&r_upstream),
            "agent": r_agent.as_ref().map(deep_full),
        },
        "agent_command_substitutions": r_agent.as_ref().map(|r| r.deep.command_substitutions),
        "agent_max_depth_histogram": r_agent.as_ref().map(|r| r.deep.max_depth_histogram.clone()),
        "nesting_probes_ok": format!("{probes_ok}/{}", probes.len()),
        "primary_set": primary.name,
        "brush_core": core_summary,
        "timing_s": { "bash_n": bash_secs, "brush": brush_secs, "total": started.elapsed().as_secs_f64() },
    });
    notes.push(format!(
        "H37: bash -n em {:.1}s (cache em target/f15-cache), brush em {:.1}s pra {} scripts",
        bash_secs,
        brush_secs,
        all_texts.len()
    ));
    Ok(Section {
        hypothesis: "H37",
        verdict,
        summary,
        evidence,
        candidates: vec![parser_cand, core_cand, yash],
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth_of(text: &str) -> (usize, usize) {
        let (_, prog) = brush_verdict(text, true);
        let prog = prog.expect("brush aceita a sonda");
        let mut d = Deep::new(&options(true), true);
        d.top(&prog);
        (d.max_depth, d.errors_here)
    }

    #[test]
    fn nested_substitutions_are_found_by_reparse() {
        assert_eq!(depth_of("echo $(echo a)"), (1, 0));
        assert_eq!(depth_of("x=$(a $(b $(c $(d))))"), (4, 0));
        assert_eq!(depth_of("echo hi").0, 0);
    }

    #[test]
    fn backquote_unescape() {
        assert_eq!(unescape_backquoted(r"echo \`echo c\`"), "echo `echo c`");
        assert_eq!(unescape_backquoted(r"a \\ b \$x \n"), r"a \ b $x \n");
    }

    #[test]
    fn agreement_counts_and_weights() {
        let inputs = vec![
            Input { id: "a".into(), text: "echo".into(), weight: 3 },
            Input { id: "b".into(), text: "if".into(), weight: 1 },
        ];
        let acc = Brush::Accepted;
        let rej = Brush::Rejected("x".into());
        let a = agreement(&inputs, &[true, true], &[&acc, &rej], true);
        assert_eq!((a.both_accept, a.brush_only_reject), (1, 1));
        assert_eq!(a.agree_calls, 3);
        assert!((a.agreement_calls - 0.75).abs() < 1e-9);
    }

    #[test]
    fn features_detect_heredoc_and_cmdsub() {
        let f = features("cat <<EOF\n$(x)\nEOF");
        assert!(f.contains(&"heredoc") && f.contains(&"command-substitution"));
    }
}
