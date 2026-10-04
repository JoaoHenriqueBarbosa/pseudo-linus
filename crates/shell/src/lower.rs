//! Conversão do AST do brush-parser pro AST do interpretador ([`crate::ast`]). As palavras passam
//! pelo [`crate::word`] aqui, e o corpo de cada `$(...)` é parseado de novo: um erro de sintaxe em
//! qualquer nível aparece antes de executar o comando, como no bash.

use std::sync::Arc;

use brush_parser::ast as b;

use crate::ast::*;
use crate::parse::SyntaxError;
use crate::word::{self, Mode, WordOpts, make_word};

/// Estado da conversão: deslocamento de linha do trecho e linha "corrente" (pra quem não tem posição).
pub struct Lowerer {
    /// Somado às linhas do brush (que começam em 1 no trecho parseado).
    pub line_offset: Line,
    pub current_line: Line,
    /// Arquivo de origem (pro `BASH_SOURCE` das funções).
    pub source: Arc<str>,
    /// Avisos de here-doc terminado pelo fim do texto: (linha do `<<`, delimitador).
    pub heredoc_eof: Vec<(Line, String)>,
}

type R<T> = Result<T, SyntaxError>;

impl Lowerer {
    pub fn new(line_offset: Line, source: Arc<str>) -> Lowerer {
        Lowerer { line_offset, current_line: line_offset + 1, source, heredoc_eof: Vec::new() }
    }

    fn line_of(&mut self, loc: Option<&brush_parser::SourceSpan>) -> Line {
        if let Some(l) = loc {
            self.current_line = l.start.line as Line + self.line_offset;
        }
        self.current_line
    }

    pub fn program(&mut self, p: &b::Program) -> R<Program> {
        let mut commands = Vec::with_capacity(p.complete_commands.len());
        for c in &p.complete_commands {
            commands.push(self.list(c)?);
        }
        Ok(Program { commands })
    }

    pub fn list(&mut self, l: &b::CompoundList) -> R<List> {
        let mut items = Vec::with_capacity(l.0.len());
        for b::CompoundListItem(ao, sep) in &l.0 {
            items.push(ListItem { and_or: self.and_or(ao)?, background: matches!(sep, b::SeparatorOperator::Async) });
        }
        Ok(List { items })
    }

    fn and_or(&mut self, ao: &b::AndOrList) -> R<AndOr> {
        let first = self.pipeline(&ao.first)?;
        let mut rest = Vec::with_capacity(ao.additional.len());
        for a in &ao.additional {
            match a {
                b::AndOr::And(p) => rest.push((Connector::And, self.pipeline(p)?)),
                b::AndOr::Or(p) => rest.push((Connector::Or, self.pipeline(p)?)),
            }
        }
        Ok(AndOr { first, rest })
    }

    fn pipeline(&mut self, p: &b::Pipeline) -> R<Pipeline> {
        let time = p.timed.as_ref().map(|t| {
            let (posix, loc) = match t {
                b::PipelineTimed::Timed(l) => (false, l),
                b::PipelineTimed::TimedWithPosixOutput(l) => (true, l),
            };
            self.line_of(Some(loc));
            TimeSpec { posix }
        });
        let mut commands = Vec::with_capacity(p.seq.len());
        let mut line = None;
        for c in &p.seq {
            let cmd = self.command(c)?;
            if line.is_none() {
                line = Some(cmd.line());
            }
            commands.push(cmd);
        }
        Ok(Pipeline { negated: p.bang, time, commands, line: line.unwrap_or(self.current_line) })
    }

    fn command(&mut self, c: &b::Command) -> R<Command> {
        match c {
            b::Command::Simple(s) => Ok(Command::Simple(Arc::new(self.simple(s)?))),
            b::Command::Compound(cc, redirs) => {
                let comp = self.compound(cc)?;
                let r = self.redirect_list(redirs.as_ref())?;
                Ok(Command::Compound(Arc::new(comp), Arc::from(r)))
            }
            b::Command::Function(f) => {
                let line = self.line_of(f.fname.loc.as_ref());
                let body = self.compound(&f.body.0)?;
                let redirects = self.redirect_list(f.body.1.as_ref())?;
                Ok(Command::FunctionDef(Arc::new(FunctionDef {
                    name: f.fname.value.clone(),
                    body,
                    redirects,
                    line,
                    source: self.source.clone(),
                })))
            }
            b::Command::ExtendedTest(t, redirs) => {
                let line = self.line_of(Some(&t.loc));
                let expr = self.cond(&t.expr)?;
                let r = self.redirect_list(redirs.as_ref())?;
                Ok(Command::Compound(Arc::new(Compound { kind: CompoundKind::Cond(expr), line }), Arc::from(r)))
            }
        }
    }

    fn redirect_list(&mut self, r: Option<&b::RedirectList>) -> R<Vec<Redirect>> {
        let mut out = Vec::new();
        if let Some(list) = r {
            for x in &list.0 {
                out.push(self.redirect(x)?);
            }
        }
        Ok(out)
    }

    fn word(&mut self, w: &b::Word, opts: WordOpts) -> R<Word> {
        let line = self.line_of(w.loc.as_ref());
        make_word(&w.value, WordOpts { line, ..opts })
    }

    fn simple(&mut self, s: &b::SimpleCommand) -> R<Simple> {
        let mut assigns = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        let mut line: Option<Line> = None;
        if let Some(prefix) = &s.prefix {
            for item in &prefix.0 {
                match item {
                    b::CommandPrefixOrSuffixItem::IoRedirect(r) => redirects.push(self.redirect(r)?),
                    b::CommandPrefixOrSuffixItem::AssignmentWord(a, w) => {
                        let l = self.line_of(w.loc.as_ref());
                        line.get_or_insert(l);
                        assigns.push(self.assignment(a, &w.value)?);
                    }
                    b::CommandPrefixOrSuffixItem::Word(w) => {
                        line.get_or_insert(self.line_of(w.loc.as_ref()));
                        words.push(self.word(w, WordOpts::normal(0))?);
                    }
                    b::CommandPrefixOrSuffixItem::ProcessSubstitution(kind, sub) => {
                        words.push(self.procsub_word(kind, sub)?);
                    }
                }
            }
        }
        if let Some(w) = &s.word_or_name {
            line.get_or_insert(self.line_of(w.loc.as_ref()));
            words.push(self.word(w, WordOpts::normal(0))?);
        }
        if let Some(suffix) = &s.suffix {
            for item in &suffix.0 {
                match item {
                    b::CommandPrefixOrSuffixItem::IoRedirect(r) => redirects.push(self.redirect(r)?),
                    b::CommandPrefixOrSuffixItem::AssignmentWord(a, w) => {
                        line.get_or_insert(self.line_of(w.loc.as_ref()));
                        let assign = self.assignment(a, &w.value)?;
                        let mut word = self.word(w, WordOpts::normal(0))?;
                        word.assign = Some(Box::new(assign));
                        words.push(word);
                    }
                    b::CommandPrefixOrSuffixItem::Word(w) => {
                        line.get_or_insert(self.line_of(w.loc.as_ref()));
                        words.push(self.word(w, WordOpts::normal(0))?);
                    }
                    b::CommandPrefixOrSuffixItem::ProcessSubstitution(kind, sub) => {
                        words.push(self.procsub_word(kind, sub)?);
                    }
                }
            }
        }
        let line = line.unwrap_or(self.current_line);
        Ok(Simple { assigns, words, redirects, line })
    }

    fn procsub(&mut self, kind: &b::ProcessSubstitutionKind, sub: &b::SubshellCommand) -> R<Arc<ProcSub>> {
        self.line_of(Some(&sub.loc));
        let body = self.list(&sub.list)?;
        let write = matches!(kind, b::ProcessSubstitutionKind::Write);
        Ok(Arc::new(ProcSub { write, body, src: Arc::from(sub.list.to_string().as_str()) }))
    }

    fn procsub_word(&mut self, kind: &b::ProcessSubstitutionKind, sub: &b::SubshellCommand) -> R<Word> {
        let ps = self.procsub(kind, sub)?;
        let raw = format!("{}({})", if ps.write { '>' } else { '<' }, ps.src);
        Ok(Word { raw: Arc::from(raw.as_str()), parts: Arc::from(vec![Part::ProcSub(ps)]), assign: None })
    }

    fn assignment(&mut self, a: &b::Assignment, raw: &str) -> R<Assign> {
        let line = self.current_line;
        let (name, index) = match &a.name {
            b::AssignmentName::VariableName(n) => (n.clone(), None),
            b::AssignmentName::ArrayElementName(n, idx) => {
                let w = make_word(idx, WordOpts::mode(Mode::Subscript, line))?;
                (n.clone(), Some(w))
            }
        };
        let value = match &a.value {
            b::AssignmentValue::Scalar(w) => AssignValue::Scalar(make_word(&w.value, WordOpts::assignment(line))?),
            b::AssignmentValue::Array(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for (k, v) in elems {
                    out.push(self.array_elem(k.as_ref(), v, line)?);
                }
                AssignValue::Array(out)
            }
        };
        Ok(Assign { name, index, append: a.append, value, raw: Arc::from(raw) })
    }

    fn array_elem(&mut self, key: Option<&b::Word>, value: &b::Word, line: Line) -> R<ArrayElem> {
        if let Some(k) = key {
            let key = make_word(&k.value, WordOpts::mode(Mode::Subscript, line))?;
            let value = make_word(&value.value, WordOpts::assignment(line))?;
            return Ok(ArrayElem { key: Some(key), append: false, value });
        }
        // `[k]+=v` (o brush só reconhece `[k]=v`).
        let v = value.value.as_str();
        if v.starts_with('[') {
            if let Some(close) = v.find("]+=") {
                let key = make_word(&v[1..close], WordOpts::mode(Mode::Subscript, line))?;
                let val = make_word(&v[close + 3..], WordOpts::assignment(line))?;
                return Ok(ArrayElem { key: Some(key), append: true, value: val });
            }
        }
        Ok(ArrayElem { key: None, append: false, value: make_word(v, WordOpts::normal(line))? })
    }

    fn redirect(&mut self, r: &b::IoRedirect) -> R<Redirect> {
        match r {
            b::IoRedirect::NamedFd(name, inner) => {
                let mut red = self.redirect(inner)?;
                red.fd = RedirFd::Var(name.clone());
                Ok(red)
            }
            b::IoRedirect::File(fd, kind, target) => {
                let fd = fd.map_or(RedirFd::Default, RedirFd::Num);
                let op = match kind {
                    b::IoFileRedirectKind::Read => RedirOp::Read,
                    b::IoFileRedirectKind::Write => RedirOp::Write,
                    b::IoFileRedirectKind::Append => RedirOp::Append,
                    b::IoFileRedirectKind::ReadAndWrite => RedirOp::ReadWrite,
                    b::IoFileRedirectKind::Clobber => RedirOp::Clobber,
                    b::IoFileRedirectKind::DuplicateInput => RedirOp::DupIn,
                    b::IoFileRedirectKind::DuplicateOutput => RedirOp::DupOut,
                };
                let target = match target {
                    b::IoFileRedirectTarget::Filename(w) | b::IoFileRedirectTarget::Duplicate(w) => {
                        RedirTarget::Word(self.word(w, WordOpts::plain(0))?)
                    }
                    b::IoFileRedirectTarget::Fd(n) => RedirTarget::Word(word::literal_word(&n.to_string())),
                    b::IoFileRedirectTarget::ProcessSubstitution(kind, sub) => {
                        RedirTarget::ProcSub(self.procsub(kind, sub)?)
                    }
                };
                Ok(Redirect { fd, op, target })
            }
            b::IoRedirect::HereString(fd, w) => Ok(Redirect {
                fd: fd.map_or(RedirFd::Default, RedirFd::Num),
                op: RedirOp::HereString,
                target: RedirTarget::Word(self.word(w, WordOpts::plain(0))?),
            }),
            b::IoRedirect::OutputAndError(w, append) => Ok(Redirect {
                fd: RedirFd::Default,
                op: RedirOp::OutErr { append: *append },
                target: RedirTarget::Word(self.word(w, WordOpts::plain(0))?),
            }),
            b::IoRedirect::HereDocument(fd, h) => {
                let delimiter = brush_parser::unquote_str(&h.here_end.value);
                let start = h.start_line as Line + self.line_offset;
                if h.eof_terminated {
                    self.heredoc_eof.push((start, delimiter.clone()));
                }
                let body_line = h.doc.loc.as_ref().map_or(start + 1, |l| l.start.line as Line + self.line_offset);
                let body = h.doc.value.as_bytes().to_vec();
                let parts = if h.requires_expansion { word::parse_heredoc(&h.doc.value, body_line)? } else { Vec::new() };
                Ok(Redirect {
                    fd: fd.map_or(RedirFd::Default, RedirFd::Num),
                    op: RedirOp::HereDoc,
                    target: RedirTarget::HereDoc(Arc::new(HereDoc {
                        body,
                        expand: h.requires_expansion,
                        parts,
                        delimiter,
                        strip_tabs: h.remove_tabs,
                    })),
                })
            }
        }
    }

    fn opt_arith(&mut self, e: Option<&b::UnexpandedArithmeticExpr>) -> R<Option<Arc<ArithExp>>> {
        match e {
            Some(x) => Ok(Some(word::parse_arith(&x.value, self.current_line)?)),
            None => Ok(None),
        }
    }

    fn words(&mut self, ws: Option<&Vec<b::Word>>) -> R<Option<Vec<Word>>> {
        match ws {
            None => Ok(None),
            Some(v) => {
                let mut out = Vec::with_capacity(v.len());
                for w in v {
                    out.push(self.word(w, WordOpts::normal(0))?);
                }
                Ok(Some(out))
            }
        }
    }

    fn compound(&mut self, c: &b::CompoundCommand) -> R<Compound> {
        use brush_parser::ast::SourceLocation;
        let line = self.line_of(c.location().as_ref());
        let kind = match c {
            b::CompoundCommand::Arithmetic(a) => CompoundKind::Arith(word::parse_arith(&a.expr.value, line)?),
            b::CompoundCommand::ArithmeticForClause(f) => CompoundKind::ArithFor {
                init: self.opt_arith(f.initializer.as_ref())?,
                cond: self.opt_arith(f.condition.as_ref())?,
                step: self.opt_arith(f.updater.as_ref())?,
                body: self.list(&f.body.list)?,
            },
            b::CompoundCommand::BraceGroup(g) => CompoundKind::Brace(self.list(&g.list)?),
            b::CompoundCommand::Subshell(s) => CompoundKind::Subshell(self.list(&s.list)?),
            b::CompoundCommand::ForClause(f) => CompoundKind::For {
                var: f.variable_name.clone(),
                words: self.words(f.values.as_ref())?,
                body: self.list(&f.body.list)?,
            },
            b::CompoundCommand::SelectClause(f) => CompoundKind::Select {
                var: f.variable_name.clone(),
                words: self.words(f.values.as_ref())?,
                body: self.list(&f.body.list)?,
            },
            b::CompoundCommand::CaseClause(cc) => {
                let word = self.word(&cc.value, WordOpts::plain(0))?;
                let mut items = Vec::with_capacity(cc.cases.len());
                for it in &cc.cases {
                    let mut patterns = Vec::with_capacity(it.patterns.len());
                    for p in &it.patterns {
                        patterns.push(self.word(p, WordOpts::plain(0))?);
                    }
                    let body = match &it.cmd {
                        Some(l) => self.list(l)?,
                        None => List::default(),
                    };
                    let term = match it.post_action {
                        b::CaseItemPostAction::ExitCase => CaseTerm::Break,
                        b::CaseItemPostAction::UnconditionallyExecuteNextCaseItem => CaseTerm::FallThrough,
                        b::CaseItemPostAction::ContinueEvaluatingCases => CaseTerm::Continue,
                    };
                    items.push(CaseItem { patterns, body, term });
                }
                CompoundKind::Case { word, items }
            }
            b::CompoundCommand::IfClause(i) => {
                let mut branches = vec![(self.list(&i.condition)?, self.list(&i.then)?)];
                let mut else_body = None;
                if let Some(elses) = &i.elses {
                    for e in elses {
                        match &e.condition {
                            Some(c) => branches.push((self.list(c)?, self.list(&e.body)?)),
                            None => else_body = Some(self.list(&e.body)?),
                        }
                    }
                }
                CompoundKind::If { branches, else_body }
            }
            b::CompoundCommand::WhileClause(w) => CompoundKind::While { cond: self.list(&w.0)?, body: self.list(&w.1.list)? },
            b::CompoundCommand::UntilClause(w) => CompoundKind::Until { cond: self.list(&w.0)?, body: self.list(&w.1.list)? },
            b::CompoundCommand::Coprocess(cp) => {
                let name = match &cp.name {
                    Some(w) => w.value.clone(),
                    None => "COPROC".to_string(),
                };
                CompoundKind::Coproc { name, body: Box::new(self.command(&cp.body)?) }
            }
        };
        Ok(Compound { kind, line })
    }

    fn cond(&mut self, e: &b::ExtendedTestExpr) -> R<CondExpr> {
        Ok(match e {
            b::ExtendedTestExpr::And(l, r) => CondExpr::And(Box::new(self.cond(l)?), Box::new(self.cond(r)?)),
            b::ExtendedTestExpr::Or(l, r) => CondExpr::Or(Box::new(self.cond(l)?), Box::new(self.cond(r)?)),
            b::ExtendedTestExpr::Not(x) => CondExpr::Not(Box::new(self.cond(x)?)),
            b::ExtendedTestExpr::Parenthesized(x) => CondExpr::Group(Box::new(self.cond(x)?)),
            b::ExtendedTestExpr::UnaryTest(p, w) => {
                // O brush já transforma a palavra solta em `-n palavra` (mesma semântica).
                let op = unary_letter(p);
                let w = self.word(w, WordOpts::plain(0))?;
                CondExpr::Unary(op.to_string(), w)
            }
            b::ExtendedTestExpr::BinaryTest(p, l, r) => {
                let op = match p {
                    b::BinaryPredicate::FilesReferToSameDeviceAndInodeNumbers => CondBinOp::SameFile,
                    b::BinaryPredicate::LeftFileIsNewerOrExistsWhenRightDoesNot => CondBinOp::Newer,
                    b::BinaryPredicate::LeftFileIsOlderOrDoesNotExistWhenRightDoes => CondBinOp::Older,
                    b::BinaryPredicate::StringExactlyMatchesPattern => CondBinOp::Match,
                    b::BinaryPredicate::StringDoesNotExactlyMatchPattern => CondBinOp::NoMatch,
                    b::BinaryPredicate::StringMatchesRegex | b::BinaryPredicate::StringContainsSubstring => CondBinOp::Regex,
                    b::BinaryPredicate::StringExactlyMatchesString => CondBinOp::Match,
                    b::BinaryPredicate::StringDoesNotExactlyMatchString => CondBinOp::NoMatch,
                    b::BinaryPredicate::LeftSortsBeforeRight => CondBinOp::Less,
                    b::BinaryPredicate::LeftSortsAfterRight => CondBinOp::Greater,
                    b::BinaryPredicate::ArithmeticEqualTo => CondBinOp::Eq,
                    b::BinaryPredicate::ArithmeticNotEqualTo => CondBinOp::Ne,
                    b::BinaryPredicate::ArithmeticLessThan => CondBinOp::Lt,
                    b::BinaryPredicate::ArithmeticLessThanOrEqualTo => CondBinOp::Le,
                    b::BinaryPredicate::ArithmeticGreaterThan => CondBinOp::Gt,
                    b::BinaryPredicate::ArithmeticGreaterThanOrEqualTo => CondBinOp::Ge,
                };
                let lw = self.word(l, WordOpts::plain(0))?;
                let rw = self.word(r, WordOpts::plain(0))?;
                CondExpr::Binary(op, lw, rw)
            }
        })
    }
}

fn unary_letter(p: &b::UnaryPredicate) -> &'static str {
    use b::UnaryPredicate as U;
    match p {
        U::FileExists => "e",
        U::FileExistsAndIsBlockSpecialFile => "b",
        U::FileExistsAndIsCharSpecialFile => "c",
        U::FileExistsAndIsDir => "d",
        U::FileExistsAndIsRegularFile => "f",
        U::FileExistsAndIsSetgid => "g",
        U::FileExistsAndIsSymlink => "L",
        U::FileExistsAndHasStickyBit => "k",
        U::FileExistsAndIsFifo => "p",
        U::FileExistsAndIsReadable => "r",
        U::FileExistsAndIsNotZeroLength => "s",
        U::FdIsOpenTerminal => "t",
        U::FileExistsAndIsSetuid => "u",
        U::FileExistsAndIsWritable => "w",
        U::FileExistsAndIsExecutable => "x",
        U::FileExistsAndOwnedByEffectiveGroupId => "G",
        U::FileExistsAndModifiedSinceLastRead => "N",
        U::FileExistsAndOwnedByEffectiveUserId => "O",
        U::FileExistsAndIsSocket => "S",
        U::ShellOptionEnabled => "o",
        U::ShellVariableIsSetAndAssigned => "v",
        U::ShellVariableIsSetAndNameRef => "R",
        U::StringHasZeroLength => "z",
        U::StringHasNonZeroLength => "n",
    }
}
